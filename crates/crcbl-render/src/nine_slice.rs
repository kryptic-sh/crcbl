//! Nine-slice: one frame drawn at any size with its corners left alone.
//!
//! ```text
//!   source frame (texels)              target (world units)
//!   ┌──┬────────┬──┐                   ┌──┬──────────────────┬──┐
//!   │TL│  top   │TR│   left/right      │TL│       top        │TR│
//!   ├──┼────────┼──┤   stretch on Y    ├──┼──────────────────┼──┤
//!   │L │ centre │R │   top/bottom      │L │                  │R │
//!   ├──┼────────┼──┤   stretch on X    │  │      centre      │  │
//!   │BL│ bottom │BR│   centre on both  ├──┼──────────────────┼──┤
//!   └──┴────────┴──┘                   │BL│      bottom      │BR│
//!                                      └──┴──────────────────┴──┘
//! ```
//!
//! # Why this is in `crcbl-render` and not in `crcbl-sprite`
//!
//! [`crcbl_sprite`] owns [`NineSlice`] and validates it, and the arithmetic here
//! is pure maths over rectangles with no device in it — so the crate that owns
//! the insets is the obvious first guess. It is the wrong one, for a reason that
//! crate states in its own first paragraph: *"Pixels are the unit … every
//! rectangle here is in texels of the sheet image, as unsigned integers"*, and
//! *"turning a description into pixels belongs to `crcbl-render`"*.
//!
//! A target rectangle is neither: it is **world units, as floats**, and it comes
//! from a game's simulation rather than from anything a sheet knows. Putting it
//! in `crcbl-sprite` would put a second unit system into a crate whose whole
//! discipline is having one, and would give a `build.rs` that only wants to
//! convert art a type it has no use for. `crcbl-render` already depends on
//! `crcbl-sprite`, so this side can read [`NineSlice`], [`Rect`] and the sheet's
//! size; the reverse dependency does not exist and must not.
//!
//! Nothing is lost by the move. This module has no device, no GPU type and no
//! I/O — every test below runs with no backend at all, which is the property
//! that made `crcbl-sprite` attractive in the first place.
//!
//! `docs/specs/crcbl/pix.md` §9 already draws the line the same way: *"how
//! nine-slice insets become geometry"* is listed under **Not specified here**,
//! alongside rendering.
//!
//! # Stretch, never tile
//!
//! The edges and the centre are **stretched**, and there is deliberately no
//! tiling mode. Two reasons, both concrete:
//!
//! * A tiled band is `ceil(extent / inset)` quads rather than one, so the
//!   instance count stops being bounded by nine and starts depending on how big
//!   the thing was drawn — a pipe stretched to a tall gap would quietly become
//!   hundreds of instances.
//! * Doing it with UVs instead, by letting `u1` run past 1, needs a repeating
//!   sampler; [`SpriteRenderer`](crate::SpriteRenderer) has exactly **one**
//!   sampler, `ClampToEdge`, shared by every sheet, and a second address mode
//!   would be a second bind group layout for a feature nothing has asked for.
//!
//! If tiling is ever wanted it is a new mode with its own quad emitter, not a
//! flag on this one.

use core::ops::Deref;

use crcbl_sprite::{NineSlice, Rect, Sheet};

use crate::sprite_pass::{SheetId, Sprite};

/// One quad of an expanded nine-slice: where to draw, and what to sample.
///
/// The same two rectangles a [`Sprite`] carries, and in the same layouts, so
/// [`NineQuads::sprites`] is a field copy rather than a conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliceQuad {
    /// World-space rectangle: `[x, y, w, h]`, minimum corner first, Y **up** —
    /// [`Sprite::rect`]'s layout.
    pub rect: [f32; 4],
    /// Normalised sheet UVs: `[u0, v0, u1, v1]`, top-left corner first, in image
    /// order — [`Sprite::uv`]'s layout.
    pub uv: [f32; 4],
}

/// The quads an expansion produced: at most nine, and never a heap allocation.
///
/// **Fewer than nine is the normal case, not an edge case.** A three-slice
/// (`top == bottom == 0`, the shape of a horizontal bar) emits three, and a
/// frame with no insets at all emits one. Emitting nine unconditionally would
/// cost a full instance for each empty band, and hand the shader a UV rectangle
/// of zero extent — see [`NineSliceSource::expand`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NineQuads {
    quads: [SliceQuad; 9],
    len: usize,
}

impl NineQuads {
    /// The quads, in image order: top row left to right, then middle, then
    /// bottom.
    #[must_use]
    pub fn as_slice(&self) -> &[SliceQuad] {
        &self.quads[..self.len]
    }

    /// One [`Sprite`] per quad, all naming `sheet` and carrying `tint`.
    ///
    /// An iterator rather than a `Vec`: the caller already has somewhere to put
    /// them — a frame's sprite list, or one layer of a
    /// [`LayerStack`](crate::layers::LayerStack) — and a nine-slice that
    /// allocated once per pipe per frame would be a strange thing to have built
    /// on top of a renderer that reuses its instance ring.
    pub fn sprites(
        &self,
        sheet: SheetId,
        tint: [f32; 4],
    ) -> impl Iterator<Item = Sprite> + Clone + '_ {
        self.as_slice().iter().map(move |quad| Sprite {
            sheet,
            rect: quad.rect,
            // **Unrotated, and there is no overload that is not.** The nine
            // quads of a slice are stretched against each other on two axes;
            // turning them individually about their own centres would open a
            // gap at every band boundary, and turning the frame as a whole
            // needs one pivot shared by all nine, which is a different feature
            // from `Sprite::rotation`. A rotated panel is not in this slice —
            // see `docs/backlog.md`.
            rotation: 0.0,
            uv: quad.uv,
            tint,
        })
    }

    /// Appends one quad, ignoring an empty one.
    fn push(&mut self, quad: SliceQuad) {
        // `len` cannot reach 9 before the loop that fills it ends, so this is a
        // guard against a future caller rather than a live branch.
        if self.len < self.quads.len() {
            self.quads[self.len] = quad;
            self.len += 1;
        }
    }
}

impl Deref for NineQuads {
    type Target = [SliceQuad];

    fn deref(&self) -> &[SliceQuad] {
        self.as_slice()
    }
}

/// The art a nine-slice is expanded from: the insets, the frame they inset, and
/// the sheet the frame sits in.
///
/// The sheet's size is here because it is the UV divisor — the one number a
/// [`Rect`] on its own does not know, for exactly the reason
/// [`Sheet::uv`] lives on the sheet rather than on the rect.
///
/// No `Eq`: [`texels_per_unit`](Self::texels_per_unit) is an `f32`, and `f32`
/// is not `Eq`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NineSliceSource {
    /// The insets that stay fixed, in texels.
    pub nine: NineSlice,
    /// Where the frame sits in the sheet image, in texels.
    pub frame: Rect,
    /// The sheet image's width in texels.
    pub sheet_width: u32,
    /// The sheet image's height in texels.
    pub sheet_height: u32,
    /// How many texels of art one caller unit is.
    ///
    /// The fixed bands [`expand`](Self::expand) emits and
    /// [`minimum_size`](Self::minimum_size) come back divided by this, so a
    /// source whose caller's world is not one unit per texel does not have to
    /// scale its camera to compensate. [`from_sheet`](Self::from_sheet) defaults
    /// it to 1 — one caller unit per texel, which is the behaviour this type
    /// always had — and [`with_texels_per_unit`](Self::with_texels_per_unit) is
    /// the way a caller changes it.
    pub texels_per_unit: f32,
}

impl NineSliceSource {
    /// The source for frame `index` of `sheet`.
    ///
    /// `None` when the sheet declares no nine-slice, when it has no frame at
    /// that index, or when it has no size at all — the last because the UV
    /// divisor would be zero and every quad would carry a `NaN` into a vertex
    /// buffer, which draws nothing and gets blamed on the shader.
    /// [`Sheet::uv`] refuses the same case for the same reason.
    #[must_use]
    pub fn from_sheet(sheet: &Sheet, index: usize) -> Option<Self> {
        if sheet.width == 0 || sheet.height == 0 {
            return None;
        }
        Some(Self {
            nine: sheet.nine?,
            frame: sheet.frames.get(index)?.rect,
            sheet_width: sheet.width,
            sheet_height: sheet.height,
            // One caller unit per texel: the behaviour this type always had.
            // A caller whose world is not one unit per texel changes it with
            // [`with_texels_per_unit`](Self::with_texels_per_unit).
            texels_per_unit: 1.0,
        })
    }

    /// The source, with its texels-per-unit scale set.
    ///
    /// The fixed bands [`expand`](Self::expand) emits and
    /// [`minimum_size`](Self::minimum_size) come back divided by
    /// `texels_per_unit`. Any value is accepted: [`expand`](Self::expand) draws
    /// nothing when the scale is not a positive finite number, the same way it
    /// draws nothing for a nonsense target.
    #[must_use]
    pub fn with_texels_per_unit(mut self, texels_per_unit: f32) -> Self {
        self.texels_per_unit = texels_per_unit;
        self
    }

    /// The insets this source actually expands with, in texels.
    ///
    /// [`NineSliceSource::nine`] as given, trimmed so it cannot exceed the frame
    /// — the same trimming [`NineSliceSource::expand`] applies, exposed because a
    /// caller laying out *around* the fixed bands (a button sizing itself to its
    /// corners) has to agree with the geometry rather than with the request.
    #[must_use]
    pub fn insets(&self) -> NineSlice {
        self.clamped()
    }

    /// The smallest target this source draws at its natural corner size, in
    /// world units — [`NineSlice::minimum_size`] as floats, divided by
    /// [`texels_per_unit`](Self::texels_per_unit) so it is in the same caller
    /// units as the bands [`expand`](Self::expand) emits.
    #[must_use]
    pub fn minimum_size(&self) -> (f32, f32) {
        let (width, height) = self.clamped().minimum_size();
        (
            width as f32 / self.texels_per_unit,
            height as f32 / self.texels_per_unit,
        )
    }

    /// The quads that draw this frame stretched to `target`.
    ///
    /// `target` is `[x, y, w, h]` in world units, minimum corner first and Y
    /// **up** — [`Sprite::rect`]'s layout, because that is what the quads become.
    ///
    /// # What comes back
    ///
    /// Between zero and nine quads, in image order (top row left to right, then
    /// middle, then bottom). Corners are `left`/`right` wide and `top`/`bottom`
    /// tall in world units — `inset / texels_per_unit` each, so at the default
    /// of one caller unit per texel a target the size of the frame reproduces
    /// the single sprite exactly. Edges stretch on one axis and the centre on
    /// both.
    ///
    /// **Empty bands are not emitted.** A band whose extent is zero in texels or
    /// zero in world units is skipped, so a three-slice (`top == bottom == 0`)
    /// comes back as three quads and a frame with no insets as one. A zero-area
    /// quad is invisible and still costs a whole instance, and a UV rectangle of
    /// zero extent is a division waiting to happen in anything that later wants
    /// a texel-per-world-unit ratio.
    ///
    /// **The seams cannot bleed.** The four cut lines on each axis are computed
    /// once, and every quad indexes into them — so two adjacent quads share an
    /// edge as the *same* `f32`, in world space and in UV space, rather than as
    /// two numbers that happen to have been rounded the same way. A one-texel
    /// seam between bands is exactly what independent per-quad arithmetic
    /// produces.
    ///
    /// # A target smaller than the corners
    ///
    /// **The fixed bands shrink in proportion and the stretched band vanishes.**
    /// A target 6 units wide on a slice with `left = 4, right = 8` (at the
    /// default scale of one caller unit per texel) draws a 2-unit left cap and a
    /// 4-unit right cap and no centre at all.
    ///
    /// The alternatives were both worse. *Refusing* — no quads, or a clamp of
    /// the target up to the minimum — makes a pipe squeezed below its two caps
    /// either vanish or spill outside the rectangle it was given; a sprite that
    /// disappears at one size and not another is the failure
    /// [`NineSlice::fits_in`] already exists to prevent. *Letting the corners
    /// overlap* inverts the middle band, which with no backface culling
    /// rasterises a mirrored quad rather than nothing, and double-blends the
    /// overlap.
    ///
    /// Shrinking keeps three properties that matter more than the corners
    /// staying literally fixed at a size where they cannot: the quads still tile
    /// `target` exactly, nothing is drawn outside it, and the picture is
    /// continuous — at exactly the minimum size this and the ordinary path agree,
    /// so a pipe closing to nothing does not jump. The corners are squashed,
    /// which is visible and is the honest thing for a size where "corners at
    /// their natural size" is arithmetically impossible.
    ///
    /// Each axis decides independently: a target narrower than `left + right`
    /// but taller than `top + bottom` shrinks only horizontally.
    #[must_use]
    pub fn expand(&self, target: [f32; 4]) -> NineQuads {
        let mut out = NineQuads::default();
        if self.sheet_width == 0
            || self.sheet_height == 0
            || !target.iter().all(|v| v.is_finite())
            || !self.texels_per_unit.is_finite()
            || self.texels_per_unit <= 0.0
        {
            return out;
        }
        let nine = self.clamped();
        let (width, height) = (self.frame.w, self.frame.h);

        // --- the cut lines, computed once ---------------------------------
        //
        // Texel cuts run in image order on both axes, so `v` descends the image
        // and `top` is the first band. World cuts ascend on both axes, so on Y
        // the *last* band is the one `top` sampled — see the pairing below.
        let texel_x = [
            self.frame.x,
            self.frame.x + nine.left,
            self.frame.x + width - nine.right,
            self.frame.x + width,
        ];
        let texel_y = [
            self.frame.y,
            self.frame.y + nine.top,
            self.frame.y + height - nine.bottom,
            self.frame.y + height,
        ];
        let us = texel_x.map(|x| x as f32 / self.sheet_width as f32);
        let vs = texel_y.map(|y| y as f32 / self.sheet_height as f32);
        // The fixed bands come back in the caller's units: the texel insets
        // divided by the scale, so a six-texel cap at twenty texels per unit is
        // a 0.3-unit cap.
        let xs = cuts(
            target[0],
            bands(
                nine.left as f32 / self.texels_per_unit,
                nine.right as f32 / self.texels_per_unit,
                target[2],
            ),
            target[2],
        );
        // World Y is up and the frame's `top` inset is the top of the *image*,
        // so the low world band is `bottom` and the high one is `top`.
        let ys = cuts(
            target[1],
            bands(
                nine.bottom as f32 / self.texels_per_unit,
                nine.top as f32 / self.texels_per_unit,
                target[3],
            ),
            target[3],
        );

        // --- one quad per non-empty band pair -----------------------------
        for row in 0..3 {
            // Image row 0 is the top of the image, which is the *highest* world
            // band; row 2 is the bottom, which is the lowest.
            let world_row = 2 - row;
            let (y, h) = (ys[world_row], ys[world_row + 1] - ys[world_row]);
            if texel_y[row] == texel_y[row + 1] || h <= 0.0 {
                continue;
            }
            for column in 0..3 {
                let (x, w) = (xs[column], xs[column + 1] - xs[column]);
                if texel_x[column] == texel_x[column + 1] || w <= 0.0 {
                    continue;
                }
                out.push(SliceQuad {
                    rect: [x, y, w, h],
                    uv: [us[column], vs[row], us[column + 1], vs[row + 1]],
                });
            }
        }
        out
    }

    /// The insets, trimmed so they cannot exceed the frame.
    ///
    /// [`Sheet::validate`] refuses a sheet whose insets overlap, but this type
    /// can be built by hand from any [`NineSlice`], and insets past the frame's
    /// edge would make the cut lines run backwards — which is inside-out quads
    /// rather than a visible error.
    fn clamped(&self) -> NineSlice {
        let left = self.nine.left.min(self.frame.w);
        let top = self.nine.top.min(self.frame.h);
        NineSlice {
            left,
            right: self.nine.right.min(self.frame.w - left),
            top,
            bottom: self.nine.bottom.min(self.frame.h - top),
        }
    }
}

/// The three world-space band lengths along one axis: the low fixed band, the
/// stretched band, and the high fixed band.
///
/// `low` and `high` are the fixed bands in the caller's units — the texel
/// insets already divided by [`NineSliceSource::texels_per_unit`]. Below
/// `low + high` the two fixed bands shrink in proportion and the stretched band
/// is zero — see [`NineSliceSource::expand`]'s account of that choice.
fn bands(low: f32, high: f32, extent: f32) -> [f32; 3] {
    if extent.is_nan() || extent <= 0.0 {
        return [0.0; 3];
    }
    let fixed = low + high;
    if extent < fixed {
        // `fixed > extent > 0`, so the division is safe.
        let low = low * (extent / fixed);
        return [low, 0.0, extent - low];
    }
    [low, extent - fixed, high]
}

/// The four cut lines an axis's bands imply, starting at `origin`.
///
/// The far cut is `origin + extent` rather than the sum of the bands: a float
/// sum that lands a half-ulp short would leave the last quad a sliver narrower
/// than the target it was asked to fill.
fn cuts(origin: f32, bands: [f32; 3], extent: f32) -> [f32; 4] {
    [
        origin,
        origin + bands[0],
        origin + bands[0] + bands[1],
        origin + extent,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_sprite::{Frame, SampleMode};

    /// A 32×16 sheet with the frame **not** at the origin and **not** filling
    /// it: an expansion that ignored `frame.x`/`frame.y`, or that divided by the
    /// frame's size instead of the sheet's, lands on plausible-looking UVs on a
    /// sheet where the frame is the whole image.
    const FRAME: Rect = Rect::new(8, 4, 16, 8);

    /// Insets that are **all different**, so a transposed or rotated pair cannot
    /// compare equal by accident. Centre is 8 × 5 texels.
    const NINE: NineSlice = NineSlice::new(3, 5, 2, 1);

    fn source() -> NineSliceSource {
        NineSliceSource {
            nine: NINE,
            frame: FRAME,
            sheet_width: 32,
            sheet_height: 16,
            texels_per_unit: 1.0,
        }
    }

    /// The `us`/`vs` the tests are written against, spelled out rather than
    /// recomputed: `8/32, 11/32, 19/32, 24/32` and `4/16, 6/16, 11/16, 12/16`.
    const US: [f32; 4] = [0.25, 0.343_75, 0.593_75, 0.75];
    const VS: [f32; 4] = [0.25, 0.375, 0.687_5, 0.75];

    fn quad(rect: [f32; 4], uv: [f32; 4]) -> SliceQuad {
        SliceQuad { rect, uv }
    }

    // -----------------------------------------------------------------------
    // Exact geometry
    // -----------------------------------------------------------------------

    /// **The identity case.** A target the size of the source must reproduce the
    /// original single sprite exactly: nine quads that tile the frame's own
    /// rectangle at one world unit per texel, with the frame's own UVs.
    ///
    /// Written out in full rather than checked as a property, because a property
    /// ("the union is the target") is satisfied by nine quads with the *wrong*
    /// UVs in them.
    #[test]
    fn a_target_the_size_of_the_source_reproduces_the_original_sprite() {
        // World X cuts 100, 103, 111, 116; world Y cuts (ascending, so bottom
        // first) 200, 201, 206, 208.
        let quads = source().expand([100.0, 200.0, 16.0, 8.0]);
        assert_eq!(
            quads.as_slice(),
            [
                // Top row of the image: the highest world band, `top` = 2 tall.
                quad([100.0, 206.0, 3.0, 2.0], [US[0], VS[0], US[1], VS[1]]),
                quad([103.0, 206.0, 8.0, 2.0], [US[1], VS[0], US[2], VS[1]]),
                quad([111.0, 206.0, 5.0, 2.0], [US[2], VS[0], US[3], VS[1]]),
                // Middle row: the centre band, 5 tall.
                quad([100.0, 201.0, 3.0, 5.0], [US[0], VS[1], US[1], VS[2]]),
                quad([103.0, 201.0, 8.0, 5.0], [US[1], VS[1], US[2], VS[2]]),
                quad([111.0, 201.0, 5.0, 5.0], [US[2], VS[1], US[3], VS[2]]),
                // Bottom row of the image: the lowest world band, `bottom` = 1.
                quad([100.0, 200.0, 3.0, 1.0], [US[0], VS[2], US[1], VS[3]]),
                quad([103.0, 200.0, 8.0, 1.0], [US[1], VS[2], US[2], VS[3]]),
                quad([111.0, 200.0, 5.0, 1.0], [US[2], VS[2], US[3], VS[3]]),
            ]
        );

        // And the claim those numbers encode, said directly: every quad is at
        // exactly one world unit per texel, which is what "reproduces the
        // original" means and what a stretched identity case would break.
        let source = source();
        for quad in quads.as_slice() {
            let texels_wide = (quad.uv[2] - quad.uv[0]) * source.sheet_width as f32;
            let texels_tall = (quad.uv[3] - quad.uv[1]) * source.sheet_height as f32;
            assert!(
                (quad.rect[2] - texels_wide).abs() < 1e-4
                    && (quad.rect[3] - texels_tall).abs() < 1e-4,
                "{quad:?} covers {texels_wide}x{texels_tall} texels in \
                 {}x{} world units",
                quad.rect[2],
                quad.rect[3]
            );
        }
    }

    /// Stretched on **one** axis only: the corners keep their width *and* their
    /// height, the left and right edges take all of the extra height, and the
    /// top and bottom edges are untouched.
    #[test]
    fn a_vertical_stretch_grows_only_the_bands_that_stretch_vertically() {
        // Height 40 rather than 8: the centre band goes 5 → 37 and nothing else
        // changes. World Y cuts 200, 201, 238, 240.
        let quads = source().expand([100.0, 200.0, 16.0, 40.0]);
        assert_eq!(
            quads.as_slice(),
            [
                quad([100.0, 238.0, 3.0, 2.0], [US[0], VS[0], US[1], VS[1]]),
                quad([103.0, 238.0, 8.0, 2.0], [US[1], VS[0], US[2], VS[1]]),
                quad([111.0, 238.0, 5.0, 2.0], [US[2], VS[0], US[3], VS[1]]),
                quad([100.0, 201.0, 3.0, 37.0], [US[0], VS[1], US[1], VS[2]]),
                quad([103.0, 201.0, 8.0, 37.0], [US[1], VS[1], US[2], VS[2]]),
                quad([111.0, 201.0, 5.0, 37.0], [US[2], VS[1], US[3], VS[2]]),
                quad([100.0, 200.0, 3.0, 1.0], [US[0], VS[2], US[1], VS[3]]),
                quad([103.0, 200.0, 8.0, 1.0], [US[1], VS[2], US[2], VS[3]]),
                quad([111.0, 200.0, 5.0, 1.0], [US[2], VS[2], US[3], VS[3]]),
            ],
            "only the three quads of the middle image row may have changed height"
        );
    }

    /// Stretched both ways, and by different factors on each axis — a square
    /// target on a non-square frame is the one that cannot tell an axis swap.
    #[test]
    fn a_stretch_on_both_axes_grows_the_centre_on_both() {
        // 48 × 40: world X cuts 100, 103, 143, 148; Y cuts 200, 201, 238, 240.
        let quads = source().expand([100.0, 200.0, 48.0, 40.0]);
        assert_eq!(
            quads.as_slice(),
            [
                quad([100.0, 238.0, 3.0, 2.0], [US[0], VS[0], US[1], VS[1]]),
                quad([103.0, 238.0, 40.0, 2.0], [US[1], VS[0], US[2], VS[1]]),
                quad([143.0, 238.0, 5.0, 2.0], [US[2], VS[0], US[3], VS[1]]),
                quad([100.0, 201.0, 3.0, 37.0], [US[0], VS[1], US[1], VS[2]]),
                quad([103.0, 201.0, 40.0, 37.0], [US[1], VS[1], US[2], VS[2]]),
                quad([143.0, 201.0, 5.0, 37.0], [US[2], VS[1], US[3], VS[2]]),
                quad([100.0, 200.0, 3.0, 1.0], [US[0], VS[2], US[1], VS[3]]),
                quad([103.0, 200.0, 40.0, 1.0], [US[1], VS[2], US[2], VS[3]]),
                quad([143.0, 200.0, 5.0, 1.0], [US[2], VS[2], US[3], VS[3]]),
            ]
        );

        // The four corners kept their natural size, which is the whole point.
        for corner in [0usize, 2, 6, 8] {
            let rect = quads.as_slice()[corner].rect;
            assert!(
                (rect[2] == 3.0 || rect[2] == 5.0) && (rect[3] == 2.0 || rect[3] == 1.0),
                "corner {corner} is {}x{}, not an inset-sized block",
                rect[2],
                rect[3]
            );
        }
    }

    // -----------------------------------------------------------------------
    // Seams
    // -----------------------------------------------------------------------

    /// **The seam property, stated as one.** Adjacent quads must share their
    /// edge as the *same float*, in world space and in UV space. Anything less
    /// is a one-texel seam that shows up between bands at some sizes and not
    /// others.
    ///
    /// `assert_eq!` on `f32` and not a tolerance, deliberately: a tolerance is
    /// exactly what "rounded slightly differently" passes.
    #[test]
    fn adjacent_quads_share_their_edges_exactly_in_world_and_uv_space() {
        let source = source();
        for target in [
            [100.0f32, 200.0, 16.0, 8.0],
            [100.0, 200.0, 16.0, 40.0],
            [100.0, 200.0, 48.0, 40.0],
            // Awkward on purpose: fractional origin and fractional extents, so
            // the cuts are not exactly representable sums.
            [-13.7, 4.3, 91.1, 250.9],
            [0.1, 0.2, 8.3, 3.7],
        ] {
            let quads = source.expand(target);
            assert_eq!(quads.len(), 9, "{target:?} should be a full nine");
            // Image order is row-major over three rows of three.
            for row in 0..3 {
                for column in 0..2 {
                    let left = quads[row * 3 + column];
                    let right = quads[row * 3 + column + 1];
                    assert_eq!(
                        left.rect[0] + left.rect[2],
                        right.rect[0],
                        "world gap between columns {column} and {} of row {row} at {target:?}",
                        column + 1
                    );
                    assert_eq!(
                        left.uv[2],
                        right.uv[0],
                        "UV gap between columns {column} and {} of row {row} at {target:?}",
                        column + 1
                    );
                }
            }
            for column in 0..3 {
                for row in 0..2 {
                    let upper = quads[row * 3 + column];
                    let lower = quads[(row + 1) * 3 + column];
                    // Image row `row + 1` is *below* row `row`, so in world
                    // space it is the lower band: its top edge is the upper
                    // band's bottom.
                    assert_eq!(
                        lower.rect[1] + lower.rect[3],
                        upper.rect[1],
                        "world gap between rows {row} and {} of column {column} at {target:?}",
                        row + 1
                    );
                    assert_eq!(
                        upper.uv[3],
                        lower.uv[1],
                        "UV gap between rows {row} and {} of column {column} at {target:?}",
                        row + 1
                    );
                }
            }

            // And the outside edges are the target's own, not a sum that drifted.
            assert_eq!(quads[6].rect[0], target[0]);
            assert_eq!(quads[6].rect[1], target[1]);
            assert_eq!(quads[2].rect[0] + quads[2].rect[2], target[0] + target[2]);
            assert_eq!(quads[2].rect[1] + quads[2].rect[3], target[1] + target[3]);
            // The UVs span the whole frame, top-left first.
            assert_eq!([quads[0].uv[0], quads[0].uv[1]], [US[0], VS[0]]);
            assert_eq!([quads[8].uv[2], quads[8].uv[3]], [US[3], VS[3]]);
        }
    }

    // -----------------------------------------------------------------------
    // Degenerate bands
    // -----------------------------------------------------------------------

    /// **A three-slice emits three quads, not nine.** `top == bottom == 0` is
    /// the ordinary shape of a horizontal bar, and the six zero-height quads an
    /// unconditional nine would emit are invisible, cost a full instance each,
    /// and carry a UV rectangle of zero extent.
    #[test]
    fn a_three_slice_emits_three_quads_and_a_plain_frame_emits_one() {
        let mut source = source();
        source.nine = NineSlice::new(3, 5, 0, 0);
        let quads = source.expand([100.0, 200.0, 48.0, 40.0]);
        assert_eq!(quads.len(), 3, "left cap, stretch, right cap");
        assert_eq!(
            quads.as_slice(),
            [
                quad([100.0, 200.0, 3.0, 40.0], [US[0], VS[0], US[1], VS[3]]),
                quad([103.0, 200.0, 40.0, 40.0], [US[1], VS[0], US[2], VS[3]]),
                quad([143.0, 200.0, 5.0, 40.0], [US[2], VS[0], US[3], VS[3]]),
            ],
            "each spans the frame's full height, and none is zero-height"
        );

        // The other three-slice, vertical: `left == right == 0`.
        source.nine = NineSlice::new(0, 0, 2, 1);
        let quads = source.expand([100.0, 200.0, 48.0, 40.0]);
        assert_eq!(quads.len(), 3, "top cap, stretch, bottom cap");
        assert_eq!(
            quads.as_slice(),
            [
                quad([100.0, 238.0, 48.0, 2.0], [US[0], VS[0], US[3], VS[1]]),
                quad([100.0, 201.0, 48.0, 37.0], [US[0], VS[1], US[3], VS[2]]),
                quad([100.0, 200.0, 48.0, 1.0], [US[0], VS[2], US[3], VS[3]]),
            ]
        );

        // And no insets at all is one quad that *is* the plain sprite: the
        // target rect, and the frame's own UVs.
        source.nine = NineSlice::new(0, 0, 0, 0);
        let quads = source.expand([100.0, 200.0, 48.0, 40.0]);
        assert_eq!(quads.len(), 1);
        assert_eq!(
            quads[0],
            quad([100.0, 200.0, 48.0, 40.0], [US[0], VS[0], US[3], VS[3]])
        );
        let sheet_uv = Sheet {
            width: 32,
            height: 16,
            frames: vec![Frame {
                name: "only".into(),
                rect: FRAME,
                hold: 1,
            }],
            ..Sheet::default()
        }
        .uv(0)
        .expect("the frame exists");
        assert_eq!(
            quads[0].uv, sheet_uv,
            "an un-inset expansion must agree with Sheet::uv exactly, or the two \
             paths sample different pixels for the same frame"
        );
    }

    /// A band that is non-empty in texels but zero in **world** units is dropped
    /// too: a target exactly as tall as `top + bottom` has no centre to draw.
    #[test]
    fn a_band_with_no_world_extent_is_dropped_even_when_it_has_texels() {
        // Height exactly 3 = top + bottom, width 48.
        let quads = source().expand([100.0, 200.0, 48.0, 3.0]);
        assert_eq!(quads.len(), 6, "the middle image row has nowhere to go");
        for quad in quads.as_slice() {
            assert!(quad.rect[3] > 0.0, "{quad:?} has no height");
            assert!(quad.rect[2] > 0.0, "{quad:?} has no width");
        }
        // Still exactly the two rows, touching: the bottom band is 200..201 and
        // the top band 201..203, with no gap where the centre would have been.
        assert_eq!((quads[0].rect[1], quads[0].rect[3]), (201.0, 2.0));
        assert_eq!((quads[3].rect[1], quads[3].rect[3]), (200.0, 1.0));
        assert_eq!(quads[3].rect[1] + quads[3].rect[3], quads[0].rect[1]);
    }

    /// A target with no area, and a nonsense one, draw nothing rather than
    /// something inside out.
    #[test]
    fn a_target_with_no_area_draws_nothing() {
        let source = source();
        assert!(source.expand([0.0, 0.0, 0.0, 40.0]).is_empty());
        assert!(source.expand([0.0, 0.0, 40.0, 0.0]).is_empty());
        assert!(source.expand([0.0, 0.0, -10.0, -10.0]).is_empty());
        assert!(source.expand([0.0, 0.0, f32::NAN, 40.0]).is_empty());

        // A sheet with no size would divide by zero and hand a NaN to a vertex
        // buffer — the case `Sheet::uv` refuses for the same reason.
        let sizeless = NineSliceSource {
            sheet_width: 0,
            ..source
        };
        assert!(sizeless.expand([0.0, 0.0, 40.0, 40.0]).is_empty());
    }

    // -----------------------------------------------------------------------
    // Below the minimum
    // -----------------------------------------------------------------------

    /// **Below `minimum_size` the fixed bands shrink in proportion, the
    /// stretched band vanishes, and the quads still tile the target exactly.**
    ///
    /// The decision is written up on [`NineSliceSource::expand`]. What is
    /// asserted here is the part a reader would otherwise have to take on trust:
    /// nothing inverts, nothing leaves the target, and the four corners are
    /// still all that is drawn.
    #[test]
    fn a_target_below_the_minimum_shrinks_the_corners_rather_than_overlapping() {
        let source = source();
        assert_eq!(source.minimum_size(), (8.0, 3.0), "3 + 5 wide, 2 + 1 tall");

        // Half the minimum width, two thirds the minimum height.
        let quads = source.expand([0.0, 0.0, 4.0, 2.0]);
        assert_eq!(quads.len(), 4, "the four corners and nothing else");
        // `left = 3, right = 5` at half scale is 1.5 and 2.5; `bottom = 1,
        // top = 2` at two thirds is 0.666… and 1.333….
        let third = 2.0f32 / 3.0;
        assert_eq!(
            quads.as_slice(),
            [
                quad([0.0, third, 1.5, 2.0 - third], [US[0], VS[0], US[1], VS[1]]),
                quad([1.5, third, 2.5, 2.0 - third], [US[2], VS[0], US[3], VS[1]]),
                quad([0.0, 0.0, 1.5, third], [US[0], VS[2], US[1], VS[3]]),
                quad([1.5, 0.0, 2.5, third], [US[2], VS[2], US[3], VS[3]]),
            ]
        );

        // The properties, said directly rather than left implicit in the
        // numbers: inside the target, tiling it, no overlap.
        for quad in quads.as_slice() {
            assert!(
                quad.rect[2] > 0.0 && quad.rect[3] > 0.0,
                "{quad:?} inverted"
            );
            assert!(quad.rect[0] >= 0.0 && quad.rect[0] + quad.rect[2] <= 4.0);
            assert!(quad.rect[1] >= 0.0 && quad.rect[1] + quad.rect[3] <= 2.0);
        }
        let area: f32 = quads.iter().map(|quad| quad.rect[2] * quad.rect[3]).sum();
        assert!(
            (area - 8.0).abs() < 1e-5,
            "the four corners must tile the whole 4x2 target with no overlap; \
             they cover {area}"
        );

        // **Continuity at the boundary.** Approaching the minimum from below has
        // to arrive where the ordinary path starts, or a shrinking pipe jumps.
        let just_under = source.expand([0.0, 0.0, 8.0 - 1e-4, 3.0]);
        let at_minimum = source.expand([0.0, 0.0, 8.0, 3.0]);
        assert_eq!(at_minimum.len(), 4, "no centre and no edges at the minimum");
        for (under, at) in just_under.iter().zip(at_minimum.iter()) {
            for lane in 0..4 {
                assert!(
                    (under.rect[lane] - at.rect[lane]).abs() < 1e-3,
                    "a jump at the minimum: {under:?} against {at:?}"
                );
            }
        }

        // Each axis decides on its own.
        let narrow = source.expand([0.0, 0.0, 4.0, 40.0]);
        assert_eq!(narrow.len(), 6, "squashed across, still stretched down");
        assert_eq!(narrow[0].rect[2], 1.5, "the left cap shrank");
        assert_eq!(narrow[2].rect[3], 37.0, "and the left edge still stretched");
    }

    /// Insets that overrun the frame are trimmed rather than producing cut lines
    /// that run backwards. [`Sheet::validate`] refuses such a sheet, but this
    /// type can be built from any [`NineSlice`].
    #[test]
    fn insets_wider_than_the_frame_are_trimmed_rather_than_inverted() {
        let mut source = source();
        source.nine = NineSlice::new(20, 20, 0, 0);
        let quads = source.expand([0.0, 0.0, 100.0, 10.0]);
        assert!(!quads.is_empty(), "a bad slice must still draw something");
        for quad in quads.as_slice() {
            assert!(
                quad.uv[0] <= quad.uv[2] && quad.uv[1] <= quad.uv[3],
                "{quad:?} has an inside-out UV rect"
            );
            assert!(quad.rect[2] > 0.0 && quad.rect[3] > 0.0);
        }
        // The frame is 16 wide, so `left` takes all of it and `right` gets none.
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].uv, [US[0], VS[0], US[3], VS[3]]);
    }

    // -----------------------------------------------------------------------
    // The texels-per-unit scale
    // -----------------------------------------------------------------------

    /// **A scale turns the fixed bands into caller units.** A pipe-style
    /// three-slice (`top == bottom == 6` texels) at twenty texels per unit draws
    /// its caps 6 / 20 = 0.3 units tall, and `minimum_size` reports the same
    /// units.
    #[test]
    fn a_scale_makes_the_fixed_bands_come_back_in_caller_units() {
        let mut source = source();
        source.nine = NineSlice::new(0, 0, 6, 6);
        // Taller than the frame the helper ships with: the clamp trims insets
        // to the frame, and the pipe both caps fit in needs 12 texels of height.
        source.frame = Rect::new(8, 4, 16, 24);
        let scaled = source.with_texels_per_unit(20.0);

        assert_eq!(
            scaled.minimum_size(),
            (0.0, 12.0 / 20.0),
            "0 + 0 wide, 6 + 6 tall, in caller units"
        );
        assert_eq!(
            source.minimum_size(),
            (0.0, 12.0),
            "the un-scaled source keeps its texel-sized minimum"
        );

        let quads = scaled.expand([0.0, 0.0, 2.0, 20.0]);
        assert_eq!(quads.len(), 3, "a three-slice: cap, shaft, cap");
        for (cap, which) in [(0usize, "top"), (2, "bottom")] {
            let h = quads[cap].rect[3];
            assert!(
                (h - 6.0 / 20.0).abs() < 1e-4,
                "the {which} cap is {h} tall, not 0.3"
            );
        }
        // The shaft takes the rest of the 20-unit target.
        assert!((quads[1].rect[3] - 19.4).abs() < 1e-4);

        // Without the scale the same target draws 6-unit caps — the behaviour
        // this feature exists to replace.
        let plain = source.expand([0.0, 0.0, 2.0, 20.0]);
        assert_eq!(plain[0].rect[3], 6.0);
        assert_eq!(plain[2].rect[3], 6.0);
    }

    /// **A scaled expand still tiles the target exactly and samples the same
    /// texels.** Only the units change: the quads cover the target edge-to-edge
    /// with the corners at their natural size, and their UVs are the unscaled
    /// ones — the texel cuts never see the scale.
    #[test]
    fn a_scaled_expand_tiles_the_target_with_the_same_uvs() {
        let plain = source();
        let scaled = plain.with_texels_per_unit(20.0);
        let target = [100.0f32, 200.0, 16.0, 8.0];
        let quads = scaled.expand(target);
        assert_eq!(quads.len(), 9, "the target is well above the minimum");

        // Same nine quads, in the same order, sampling the same texels.
        for (scaled_quad, plain_quad) in quads.iter().zip(plain.expand(target).iter()) {
            assert_eq!(scaled_quad.uv, plain_quad.uv, "the scale touched the UVs");
        }

        // The corners are at their natural size: `inset / texels_per_unit`.
        for corner in [0usize, 2, 6, 8] {
            let (w, h) = (quads[corner].rect[2], quads[corner].rect[3]);
            assert!(
                (w - 3.0 / 20.0).abs() < 1e-4 || (w - 5.0 / 20.0).abs() < 1e-4,
                "corner {corner} is {w} wide, not an inset-sized block"
            );
            assert!(
                (h - 2.0 / 20.0).abs() < 1e-4 || (h - 1.0 / 20.0).abs() < 1e-4,
                "corner {corner} is {h} tall, not an inset-sized block"
            );
        }

        // And the quads tile `target` exactly: shared edges are the *same*
        // float and the outside edges are the target's own.
        for row in 0..3 {
            for column in 0..2 {
                let left = quads[row * 3 + column];
                let right = quads[row * 3 + column + 1];
                assert_eq!(
                    left.rect[0] + left.rect[2],
                    right.rect[0],
                    "world gap between columns {column} and {} of row {row}",
                    column + 1
                );
            }
        }
        for column in 0..3 {
            for row in 0..2 {
                let upper = quads[row * 3 + column];
                let lower = quads[(row + 1) * 3 + column];
                assert_eq!(
                    lower.rect[1] + lower.rect[3],
                    upper.rect[1],
                    "world gap between rows {row} and {} of column {column}",
                    row + 1
                );
            }
        }
        assert_eq!(quads[6].rect[0], target[0]);
        assert_eq!(quads[6].rect[1], target[1]);
        assert_eq!(quads[2].rect[0] + quads[2].rect[2], target[0] + target[2]);
        assert_eq!(quads[2].rect[1] + quads[2].rect[3], target[1] + target[3]);
    }

    /// **A scale of one is exactly the old behaviour.** `from_sheet` defaults
    /// the field to 1 and this pins the two together: the identity, the
    /// stretch, the below-minimum and the awkward-fractional targets all come
    /// back identical to an un-scaled source, bit for bit.
    #[test]
    fn a_scale_of_one_is_exactly_the_default() {
        let source = source();
        let one = source.with_texels_per_unit(1.0);
        for target in [
            [100.0f32, 200.0, 16.0, 8.0],
            [100.0, 200.0, 16.0, 40.0],
            [0.0, 0.0, 4.0, 2.0],
            [-13.7, 4.3, 91.1, 250.9],
        ] {
            assert_eq!(one.expand(target), source.expand(target), "{target:?}");
        }
        assert_eq!(one.minimum_size(), source.minimum_size());
    }

    // -----------------------------------------------------------------------
    // The bridge to the renderer
    // -----------------------------------------------------------------------

    /// `from_sheet` reads the insets, the frame and the divisor from one place,
    /// and refuses the three cases that would produce a `NaN` or index nothing.
    #[test]
    fn a_source_can_be_taken_straight_off_a_sheet() {
        let mut sheet = Sheet {
            width: 32,
            height: 16,
            frames: vec![Frame {
                name: "pipe".into(),
                rect: FRAME,
                hold: 1,
            }],
            clips: Vec::new(),
            nine: Some(NINE),
            sample: SampleMode::Pixel,
        };
        sheet.validate().expect("well formed");
        let source = NineSliceSource::from_sheet(&sheet, 0).expect("it has a nine and a frame");
        assert_eq!(source, self::source());
        assert_eq!(
            NineSliceSource::from_sheet(&sheet, 1),
            None,
            "no such frame"
        );

        sheet.nine = None;
        assert_eq!(
            NineSliceSource::from_sheet(&sheet, 0),
            None,
            "no nine-slice"
        );

        sheet.nine = Some(NINE);
        sheet.height = 0;
        assert_eq!(
            NineSliceSource::from_sheet(&sheet, 0),
            None,
            "a sheet with no size is the divide-by-zero Sheet::uv also refuses"
        );
    }

    /// The quads become sprites without a conversion: same rect, same uv, the
    /// sheet and tint the caller named, and **in the order they were emitted**.
    #[test]
    fn quads_become_sprites_in_order_with_the_sheet_and_tint_they_were_given() {
        let quads = source().expand([100.0, 200.0, 48.0, 40.0]);
        let sheet = SheetId(3);
        let tint = [0.1, 0.2, 0.3, 0.4];
        let sprites: Vec<Sprite> = quads.sprites(sheet, tint).collect();
        assert_eq!(sprites.len(), quads.len());
        for (sprite, quad) in sprites.iter().zip(quads.iter()) {
            assert_eq!(sprite.sheet, sheet);
            assert_eq!(sprite.rect, quad.rect);
            assert_eq!(sprite.uv, quad.uv);
            assert_eq!(sprite.tint, tint);
        }
    }
}
