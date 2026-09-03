//! The per-probe **visibility map**: the octahedral depth and depth² image
//! `mesh.slang` and `ssr.slang` weigh each of a fragment's eight probes against,
//! in the layout those shaders read.
//!
//! ```text
//!   direction ──oct_encode──▶ uv ──▶ texel of layer `probe` ──▶ (mean, mean²)
//!                                                                    │
//!   distance from the probe to the shaded point ───────────────▶ chebyshev
//!                                                                    │
//!                                        the weight that corner keeps ▼
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s decision of 2026-08-30: the one thing
//! that makes a probe grid stop leaking is Majercik et al. 2019's per-probe
//! visibility test — *Dynamic Diffuse Global Illumination with Ray-Traced
//! Irradiance Fields*, §3, whose depth term is McGuire et al. 2017's light-field
//! probe. A probe on the far side of a wall from the surface being shaded is
//! further away than the wall the probe can see in that direction, so the
//! Chebyshev bound below gives it no weight, and the light it holds does not
//! reach through.
//!
//! # What the image is, and why it is two channels
//!
//! One layer of a `D2Array` per probe, [`EXTENT`](crate::probe_visibility::EXTENT) × [`EXTENT`](crate::probe_visibility::EXTENT) texels of
//! `Rg32Float`: `r` is the mean distance from the probe to the nearest surface
//! in that direction and `g` is the mean of its square. Those two are the first
//! two moments of the depth distribution over the texel's own solid angle, and
//! Chebyshev's inequality turns a pair of moments into a bound on the
//! probability that a surface is nearer than a given distance — which is what
//! [`chebyshev`](crate::probe_visibility::chebyshev) evaluates. One number could only answer "is the surface in
//! front of the wall", with the hard edge and the acne that a single depth
//! comparison always has; the second moment is what makes the answer a ramp.
//!
//! # Two channels of `f32`, not of `f16`
//!
//! A distance squared grows as the square of the scene, and half precision
//! stops at 65504 — a scene 256 units across already overflows the second
//! channel, and the overflow arrives as an infinity whose variance is a `NaN`.
//! The alternative is DDGI's, which divides every distance by a "maximum probe
//! distance" constant before storing it, and that constant is one more thing an
//! author has to get right for the lighting not to break. Full floats cost eight
//! bytes a texel — [`LAYER_BYTES`](crate::probe_visibility::LAYER_BYTES) for a whole probe — and need no such
//! constant.
//!
//! # It is read by four `Load`s and a blend written out, not by a sampler
//!
//! `crate::dfg`'s rule and `mesh.slang`'s, for their reason: a hardware filter
//! is fixed-function arithmetic whose weights four rasterisers compute
//! independently, and this engine's goldens are compared across all four. It
//! also means the format need not be filterable — `rg32float` is not, without
//! WebGPU's `float32-filterable` — and that no sampler is created, so no index
//! of Metal's sampler argument table moves.
//!
//! # The border is what makes the blend correct at the seam
//!
//! An octahedral map is a square that wraps: the direction just past the right
//! edge is a direction on the *left* half of the map, mirrored. A bilinear blend
//! near an edge therefore has to read texels from the other side of the wrap,
//! and a blend that instead clamped would smear the edge texel over the seam —
//! which in a visibility term reads as a ring of leaking around every probe.
//! [`BORDER`](crate::probe_visibility::BORDER) texels around the [`SIDE`](crate::probe_visibility::SIDE)-wide interior carry exactly those
//! wrapped texels, so the blend is four ordinary neighbours everywhere and the
//! shader needs no wrap arithmetic at all. [`interior_texel`](crate::probe_visibility::interior_texel) is the rule, and
//! [`texel_direction`](crate::probe_visibility::texel_direction) is what the capture evaluates so that a border texel and
//! the interior texel it stands for are filled with the same number by
//! construction rather than by a copy step that could be skipped.
//!
//! # A grid with no capture is a grid that occludes nothing
//!
//! [`ProbeVisibility::NONE`](crate::probe_visibility::ProbeVisibility::NONE) holds no layers and answers every query with
//! `1.0`, and the device's twin of it is the one-texel placeholder
//! `crcbl_render::forward` binds when nothing has been captured — a texel of
//! [`FAR`](crate::probe_visibility::FAR), which no surface is further away than. So "no visibility was
//! captured" is the value that occludes nothing rather than a branch, on the
//! terms `mesh.slang`'s white 1×1 occlusion image is bound: the frame a scene
//! with no probes draws is the frame it drew before this module existed.

/// Texels along one side of a map's **interior** — the part that holds
/// directions.
///
/// Sixteen, which is DDGI's own depth resolution (Majercik et al. 2019 §4 uses
/// 16×16 for the depth probe beside an 8×8 irradiance one). What it buys over a
/// coarser map is the corner between a wall and a floor: the texel's solid
/// angle is what the moments are averaged over, and a texel spanning both
/// surfaces carries a variance that lets a fragment on either of them through.
pub const SIDE: u32 = 16;

/// Texels of wrapped border on each side of the interior.
///
/// One, which is what a bilinear blend needs and no more: a blend reads the two
/// texels either side of its sample point, so a sample anywhere in the interior
/// reaches at most one texel past it. The [module docs](self) say why the border
/// exists at all.
pub const BORDER: u32 = 1;

/// Texels along one side of a whole layer, border included.
pub const EXTENT: u32 = SIDE + 2 * BORDER;

/// Bytes one texel occupies: two `f32` channels, `Rg32Float`.
pub const TEXEL_BYTES: usize = 2 * size_of::<f32>();

/// Bytes one probe's layer occupies.
pub const LAYER_BYTES: usize = (EXTENT * EXTENT) as usize * TEXEL_BYTES;

/// The distance a texel carries where the capture found no surface at all, and
/// the texel the one-texel placeholder holds.
///
/// Large enough that no scene reaches it and small enough that its square is an
/// ordinary `f32` — `1e16` squared is `1e32`, five orders under `f32::MAX`,
/// where a value near `f32::MAX` would square to an infinity and make the
/// variance a `NaN`. A probe whose map says the nearest surface in some
/// direction is this far away occludes nothing in that direction, which is
/// exactly what "the capture saw open space" means.
pub const FAR: f32 = 1.0e16;

/// How far along its own normal a shaded point is moved before its distance to
/// a probe is measured, in world units.
///
/// **Without it a surface sits on its own boundary.** A fragment is exactly as
/// far from a probe as the surface that probe recorded in that direction, so
/// whether it keeps the probe's light turns on which way the map's blend rounded
/// — and on a surface at a grazing angle, or one near the corner where a texel
/// straddles two planes, it rounds the wrong way. Moving the query point off the
/// surface along its own normal puts it unambiguously in front of what the probe
/// can see. Majercik et al. 2019 §3 call it the surface bias and give it the
/// same shape.
///
/// **World units, so this assumes a metre-scale world** — the scale
/// `crcbl_render::shadow`'s cascade distance and the ambient-occlusion radius
/// already assume. A scene authored a thousand times larger would want a
/// thousand times this, and would want the same of those.
pub const SURFACE_BIAS: f32 = 0.05;

/// The share of its trilinear weight a **fully occluded** corner keeps.
///
/// This is what defines the case where every one of a fragment's eight probes is
/// occluded — a fragment inside geometry, or one the capture disagrees with. The
/// weighted sum's divisor is the sum of the corner weights, so a floor of zero
/// would be a division by zero there; a floor of this instead means the sum can
/// never reach zero, and the value that comes back when *every* corner is at the
/// floor is the plain trilinear one, because the constant divides straight back
/// out. So a fragment nothing can see keeps the light it had before this module
/// existed rather than turning into a hole.
///
/// Small enough that an occluded corner beside a visible one contributes a ten
/// thousandth of what it would have — under one level of 255 for any radiance
/// this engine tonemaps — and large enough to divide by in `f32`.
pub const OCCLUDED_WEIGHT: f32 = 1.0e-4;

/// `+1` for a positive or zero component and `-1` for a negative one — the
/// octahedral mapping's `signNotZero`.
///
/// Zero maps to `+1` rather than to zero, and that is the whole of why it is not
/// `f32::signum`'s sibling: the fold below multiplies by this, and a zero factor
/// would collapse a direction on an axis plane onto the origin.
fn sign_not_zero(value: f32) -> f32 {
    if value >= 0.0 { 1.0 } else { -1.0 }
}

/// A unit direction as a point of the octahedral square, each component in
/// `[-1, 1]`.
///
/// Cigolle, Donow, Evangelakos, Mara, McGuire & Meyer 2014, *A Survey of
/// Efficient Representations for Independent Unit Vectors*, §3.2: project the
/// direction onto the octahedron `|x| + |y| + |z| = 1` by dividing by its
/// `L1` norm, then unfold the lower half outwards. It is the mapping whose
/// distortion is lowest of the square parameterisations, and — unlike a
/// paraboloid or a latitude/longitude pair — it has no pole and no
/// trigonometry.
///
/// The zero vector has no direction and no `L1` norm; it returns the square's
/// centre, which is `+Y`. Nothing calls it with one: `mesh.slang` asks about the
/// direction from a probe to a shaded point, and the surface bias keeps those
/// two apart.
#[must_use]
pub fn oct_encode(direction: [f32; 3]) -> [f32; 2] {
    let norm = direction[0].abs() + direction[1].abs() + direction[2].abs();
    if norm == 0.0 {
        return [0.0, 0.0];
    }
    let p = [direction[0] / norm, direction[2] / norm];
    if direction[1] >= 0.0 {
        p
    } else {
        [
            (1.0 - p[1].abs()) * sign_not_zero(p[0]),
            (1.0 - p[0].abs()) * sign_not_zero(p[1]),
        ]
    }
}

/// A point of the octahedral square as a unit direction — [`oct_encode`]'s
/// inverse, and the same paper's.
///
/// **The two halves of the square are `+Y` and `-Y`**, not `+Z` and `-Z`: this
/// engine's world is `+Y` up (`docs/plan/43-render-standards.md`'s axes), and
/// putting the fold on the vertical axis means the seam runs around the horizon
/// rather than through the ceiling and the floor — the two directions a probe in
/// a room has the most to say about.
///
/// Defined for points outside the square as well, which is what the border
/// wrap's arithmetic in [`interior_texel`] leans on being unnecessary: every
/// direction this returns is a unit vector, because the result is normalised.
#[must_use]
pub fn oct_decode(point: [f32; 2]) -> [f32; 3] {
    let mut x = point[0];
    let mut z = point[1];
    let y = 1.0 - x.abs() - z.abs();
    if y < 0.0 {
        let folded_x = (1.0 - z.abs()) * sign_not_zero(x);
        let folded_z = (1.0 - x.abs()) * sign_not_zero(z);
        x = folded_x;
        z = folded_z;
    }
    let length = (x * x + y * y + z * z).sqrt();
    [x / length, y / length, z / length]
}

/// Which **interior** texel a texel of a whole layer stands for.
///
/// An interior texel stands for itself. A border texel stands for the interior
/// texel the octahedral wrap puts across the seam from it: going out of the
/// square's right edge comes back in at the right edge going the other way
/// round, with the other axis reversed, because the two halves of the square
/// meet along their diagonals. The corners stand for the diagonally opposite
/// interior corner, which is the direction all four of them share.
///
/// This is Majercik et al. 2019's border-copy rule, written as a coordinate map
/// so the capture can evaluate a direction for every texel in one loop rather
/// than filling the interior and then copying — a copy step is a thing that can
/// be skipped, and a skipped one shows up only as a ring at the seam.
///
/// # Panics
///
/// If either coordinate is past [`EXTENT`].
#[must_use]
pub fn interior_texel(x: u32, y: u32) -> (u32, u32) {
    assert!(
        x < EXTENT && y < EXTENT,
        "texel ({x}, {y}) is outside a {EXTENT}×{EXTENT} visibility map"
    );
    let last = EXTENT - 1;
    let inside = |value: u32| value != 0 && value != last;
    // The interior texel that mirrors `value` along one axis of the border: the
    // interior runs `BORDER ..= EXTENT - 1 - BORDER`, and the wrap reverses it.
    let mirror = |value: u32| EXTENT - 1 - value;
    match (inside(x), inside(y)) {
        // Interior.
        (true, true) => (x, y),
        // Left or right border: the same edge column, the row reversed.
        (false, true) => (if x == 0 { BORDER } else { last - BORDER }, mirror(y)),
        // Top or bottom border: the same edge row, the column reversed.
        (true, false) => (mirror(x), if y == 0 { BORDER } else { last - BORDER }),
        // A corner, which is the direction the diagonally opposite interior
        // corner carries.
        (false, false) => (
            if x == 0 { last - BORDER } else { BORDER },
            if y == 0 { last - BORDER } else { BORDER },
        ),
    }
}

/// The direction texel `(x, y)` of a layer holds the distance along.
///
/// The interior texel's **centre** in the square's own coordinates: texel `i` of
/// the interior covers `[i / SIDE, (i + 1) / SIDE]` of `[0, 1]`, so its centre is
/// `(i + 0.5) / SIDE`, and the square runs `-1 .. 1`. A border texel is handed
/// [`interior_texel`]'s answer first, so it holds the same direction as the
/// interior texel it stands for.
///
/// # Panics
///
/// On [`interior_texel`]'s terms.
#[must_use]
pub fn texel_direction(x: u32, y: u32) -> [f32; 3] {
    let (ix, iy) = interior_texel(x, y);
    let axis = |value: u32| (value - BORDER) as f32;
    let side = SIDE as f32;
    oct_decode([
        (axis(ix) + 0.5) / side * 2.0 - 1.0,
        (axis(iy) + 0.5) / side * 2.0 - 1.0,
    ])
}

/// The share of its weight a probe keeps for a point `distance` away from it,
/// given the two depth moments its map holds in that direction.
///
/// **The Chebyshev bound**, and it is a bound rather than a test: for a
/// distribution of mean `μ` and variance `σ²`, the probability of a sample at
/// least `d - μ` above the mean is at most `σ² / (σ² + (d - μ)²)`. Read as a
/// visibility that is one where the point is at or in front of the mean surface
/// and falls off as it goes behind it, with the fall-off's width set by how much
/// the depths inside the texel disagree — so a texel entirely on one flat wall
/// occludes sharply and a texel straddling a corner occludes softly. Donnelly &
/// Lauritzen 2006's variance shadow map is where the bound entered graphics;
/// Majercik et al. 2019 §3 is where it is applied to a probe.
///
/// **Cubed**, which is that paper's own sharpening: the raw bound is far looser
/// than the truth for a surface only a little behind the mean, and the cube
/// brings the ramp down to a width a room's worth of probes reads as a wall
/// rather than as a haze.
///
/// The denominator cannot be zero: the branch takes every `distance` at or
/// under `mean`, so the difference below it is strictly positive.
#[must_use]
pub fn chebyshev(mean: f32, mean_squared: f32, distance: f32) -> f32 {
    if distance <= mean {
        return 1.0;
    }
    // Clamped because the two moments are stored independently and a texel
    // whose neighbours were blended can land a hair below `mean²`, which is a
    // negative variance and a visibility above one.
    let variance = (mean_squared - mean * mean).max(0.0);
    let behind = distance - mean;
    let bound = variance / (variance + behind * behind);
    bound * bound * bound
}

/// One probe's map, the moments the capture wrote, layer by layer — and the
/// Rust mirror of the read `mesh.slang` makes of them.
///
/// The bytes are exactly what `crcbl_render::forward` uploads: `probes` layers
/// of [`LAYER_BYTES`], each [`EXTENT`] × [`EXTENT`] texels of two little-endian
/// `f32`s, rows top to bottom. That is a contract with the shader, so it lives
/// beside the shader's other layouts rather than beside the code that fills it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeVisibility {
    /// `probes × LAYER_BYTES` bytes, layer 0 first. Empty means no capture.
    layers: Vec<u8>,
}

impl ProbeVisibility {
    /// No capture: every probe visible from everywhere.
    ///
    /// The host twin of the one-texel placeholder the renderer binds when
    /// nothing has been captured, and what every caller that has not captured
    /// passes — see the [module docs](self) on why "not captured" is a value
    /// rather than a branch.
    pub const NONE: Self = Self { layers: Vec::new() };

    /// Takes ownership of `layers`, which must be a whole number of layers.
    ///
    /// # Panics
    ///
    /// If `layers` is not a multiple of [`LAYER_BYTES`] — the only shape the
    /// shader can read, and one a caller can only get wrong by building the
    /// bytes itself.
    #[must_use]
    pub fn new(layers: Vec<u8>) -> Self {
        assert!(
            layers.len().is_multiple_of(LAYER_BYTES),
            "a visibility image is {LAYER_BYTES} bytes a probe, and this is {}",
            layers.len()
        );
        Self { layers }
    }

    /// How many probes' maps this holds. Zero for [`ProbeVisibility::NONE`].
    #[must_use]
    pub fn probes(&self) -> u32 {
        u32::try_from(self.layers.len() / LAYER_BYTES).unwrap_or(u32::MAX)
    }

    /// The whole image, layer 0 first — what the renderer uploads.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.layers
    }

    /// The two moments texel `(x, y)` of layer `probe` holds.
    ///
    /// Out of range in any of the three reads as [`FAR`] and its square, which
    /// is the placeholder's texel — so a caller that asks about a probe with no
    /// map gets the answer "nothing is in the way" rather than a panic.
    #[must_use]
    pub fn texel(&self, probe: u32, x: u32, y: u32) -> [f32; 2] {
        let far = [FAR, FAR * FAR];
        if x >= EXTENT || y >= EXTENT || probe >= self.probes() {
            return far;
        }
        let at = probe as usize * LAYER_BYTES + (y * EXTENT + x) as usize * TEXEL_BYTES;
        let word = |offset: usize| {
            let bytes: [u8; 4] = self.layers[at + offset..at + offset + 4]
                .try_into()
                .expect("a texel is two whole words");
            f32::from_le_bytes(bytes)
        };
        [word(0), word(4)]
    }

    /// The moments layer `probe` holds along `direction`, bilinearly blended —
    /// **the mirror of `probe_moments` in `shaders/mesh.slang`**.
    ///
    /// Four texels and a blend written out, for the reason the [module
    /// docs](self) give. The sample point is the direction's place in the
    /// interior, offset by [`BORDER`] and by the half texel that turns a
    /// coordinate into a texel index — so the four neighbours of a sample
    /// anywhere in the interior are inside the bordered layer, and no wrap
    /// arithmetic is needed here or in the shader.
    #[must_use]
    pub fn moments(&self, probe: u32, direction: [f32; 3]) -> [f32; 2] {
        let point = oct_encode(direction);
        let coord = |value: f32| (value * 0.5 + 0.5) * SIDE as f32 + BORDER as f32 - 0.5;
        let (u, v) = (coord(point[0]), coord(point[1]));
        // `floor` then `+ 1`: the sample point is never below `BORDER - 0.5`
        // nor above `EXTENT - BORDER - 0.5`, so both land inside the layer.
        let (fx, fy) = (u - u.floor(), v - v.floor());
        // The clamp is what makes the fetch a fact about the image rather than
        // a promise about the mapping, on `occlusion_at`'s terms in
        // `mesh.slang` — and it is what lets the one-texel placeholder be read
        // by this same code.
        let index = |value: f32| value.max(0.0).min((EXTENT - 1) as f32) as u32;
        let (x0, y0) = (index(u.floor()), index(v.floor()));
        let (x1, y1) = (index(u.floor() + 1.0), index(v.floor() + 1.0));
        let mut blended = [0.0f32; 2];
        for (channel, slot) in blended.iter_mut().enumerate() {
            let at = |x: u32, y: u32| self.texel(probe, x, y)[channel];
            let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
            let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
            *slot = top + (bottom - top) * fy;
        }
        blended
    }

    /// The share of its trilinear weight the probe at `probe_position` keeps for
    /// a surface at `world_position` facing `normal` — **the mirror of
    /// `probe_visibility` in `shaders/mesh.slang`**.
    ///
    /// Never below [`OCCLUDED_WEIGHT`], which is what defines the fully occluded
    /// case; see that constant. [`ProbeVisibility::NONE`] answers `1.0`, because
    /// its every texel reads as [`FAR`].
    #[must_use]
    pub fn weight(
        &self,
        probe: u32,
        probe_position: [f32; 3],
        world_position: [f32; 3],
        normal: [f32; 3],
    ) -> f32 {
        let mut to_probe = [0.0f32; 3];
        for axis in 0..3 {
            to_probe[axis] =
                probe_position[axis] - (world_position[axis] + normal[axis] * SURFACE_BIAS);
        }
        let distance =
            (to_probe[0] * to_probe[0] + to_probe[1] * to_probe[1] + to_probe[2] * to_probe[2])
                .sqrt();
        // From the probe outwards, which is the direction its map is indexed by.
        let away = [-to_probe[0], -to_probe[1], -to_probe[2]];
        let moments = self.moments(probe, away);
        chebyshev(moments[0], moments[1], distance).max(OCCLUDED_WEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spread of directions that reaches every octant and both poles.
    fn directions() -> Vec<[f32; 3]> {
        let mut all = Vec::new();
        for i in 0..17 {
            for j in 0..17 {
                let theta = std::f32::consts::PI * i as f32 / 16.0;
                let phi = 2.0 * std::f32::consts::PI * j as f32 / 16.0;
                all.push([
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                ]);
            }
        }
        all
    }

    /// **The mapping is a bijection on the sphere**, which is the property every
    /// other function here rests on: a direction encodes to a point of the
    /// square and that point decodes back to the direction.
    ///
    /// A permuted lane or a fold on the wrong axis passes a round trip in one
    /// octant and fails in the next, which is why this sweeps all of them.
    #[test]
    fn every_direction_survives_the_octahedral_round_trip() {
        let mut worst = 0.0f32;
        for direction in directions() {
            let back = oct_decode(oct_encode(direction));
            for axis in 0..3 {
                worst = worst.max((back[axis] - direction[axis]).abs());
            }
        }
        assert!(
            worst <= 1.0e-6,
            "the octahedral round trip moved a direction by {worst}, which is not rounding"
        );
    }

    /// The two poles are the square's centre and its four corners, and the
    /// horizon is the diamond between them — the mapping's defining landmarks,
    /// and what says the fold is on `+Y`/`-Y` rather than on `+Z`/`-Z`.
    #[test]
    fn the_square_folds_about_the_vertical_axis() {
        assert_eq!(oct_encode([0.0, 1.0, 0.0]), [0.0, 0.0], "+Y is the centre");
        for corner in [[1.0, 1.0], [1.0, -1.0], [-1.0, 1.0], [-1.0, -1.0]] {
            let decoded = oct_decode(corner);
            assert!(
                decoded[1] <= -0.999,
                "the corner {corner:?} must be -Y, and it decoded to {decoded:?}"
            );
        }
        // The horizon is `|x| + |z| == 1`, so `+X` is the right vertex.
        assert_eq!(oct_encode([1.0, 0.0, 0.0]), [1.0, 0.0]);
        assert_eq!(oct_encode([0.0, 0.0, 1.0]), [0.0, 1.0]);
    }

    /// **Every texel of the interior stands for itself, and every border texel
    /// stands for an interior one** — so the wrap never sends a read outside the
    /// interior, and never sends two different border texels to the same
    /// direction unless they genuinely share one.
    #[test]
    fn the_border_maps_onto_the_interior() {
        for y in 0..EXTENT {
            for x in 0..EXTENT {
                let (ix, iy) = interior_texel(x, y);
                assert!(
                    (BORDER..EXTENT - BORDER).contains(&ix)
                        && (BORDER..EXTENT - BORDER).contains(&iy),
                    "texel ({x}, {y}) maps to ({ix}, {iy}), which is not interior"
                );
                let interior = (BORDER..EXTENT - BORDER).contains(&x)
                    && (BORDER..EXTENT - BORDER).contains(&y);
                assert_eq!(
                    interior,
                    (ix, iy) == (x, y),
                    "texel ({x}, {y}) is {}interior and maps to ({ix}, {iy})",
                    if interior { "" } else { "not " }
                );
            }
        }
    }

    /// A layer filled with `field`, its border taken from `wrap`.
    fn filled(
        field: impl Fn([f32; 3]) -> f32,
        wrap: impl Fn(u32, u32) -> (u32, u32),
    ) -> ProbeVisibility {
        let mut layer = vec![0u8; LAYER_BYTES];
        for y in 0..EXTENT {
            for x in 0..EXTENT {
                let (ix, iy) = wrap(x, y);
                let axis = |value: u32| (value - BORDER) as f32;
                let direction = oct_decode([
                    (axis(ix) + 0.5) / SIDE as f32 * 2.0 - 1.0,
                    (axis(iy) + 0.5) / SIDE as f32 * 2.0 - 1.0,
                ]);
                let value = field(direction);
                let at = (y * EXTENT + x) as usize * TEXEL_BYTES;
                layer[at..at + 4].copy_from_slice(&value.to_le_bytes());
                layer[at + 4..at + 8].copy_from_slice(&(value * value).to_le_bytes());
            }
        }
        ProbeVisibility::new(layer)
    }

    /// The largest step between two neighbouring samples of `map` along the
    /// small circle at `y = -0.6`, which crosses all four of the square's edges.
    fn worst_step(map: &ProbeVisibility) -> f32 {
        const STEPS: usize = 2048;
        let at = |step: usize| {
            let angle = std::f32::consts::TAU * step as f32 / STEPS as f32;
            map.moments(0, [0.8 * angle.cos(), -0.6, 0.8 * angle.sin()])[0]
        };
        let mut worst = 0.0f32;
        for step in 0..STEPS {
            worst = worst.max((at(step + 1) - at(step)).abs());
        }
        worst
    }

    /// **The map a direction is read out of is continuous across the seam**,
    /// which is the whole reason the border exists — and a border filled by
    /// clamping is not, which is what says this measures the seam rather than
    /// the map's resolution.
    ///
    /// The field is linear in the direction, so along a circle of 2048 steps two
    /// neighbouring samples of a *continuous* reconstruction differ by about a
    /// thousandth; a seam the blend smears across shows up as one step tens of
    /// times that. The circle sits at `y = -0.6`, below the equator fold and
    /// crossing each of the square's four edges once, so neither the fold nor
    /// the map's own resolution is in the measurement.
    ///
    /// This is the interior-versus-border comparison written into the test, so
    /// it cannot pass with the wrap removed: `interior_texel` returning
    /// `(x, y)` clamped into the interior is exactly `clamped` below.
    #[test]
    fn the_border_carries_the_blend_across_the_seam() {
        let field = |direction: [f32; 3]| 2.0 + 1.5 * direction[0];
        let wrapped = worst_step(&filled(field, interior_texel));
        let clamped = worst_step(&filled(field, |x, y| {
            let clamp = |value: u32| value.clamp(BORDER, EXTENT - 1 - BORDER);
            (clamp(x), clamp(y))
        }));
        eprintln!("crcbl probes: seam step {wrapped:.5} wrapped against {clamped:.5} clamped");
        assert!(
            wrapped * 8.0 < clamped,
            "the wrapped border must make the read across the seam unmistakably smoother \
             than a clamped one: {wrapped:.5} against {clamped:.5}"
        );
        assert!(
            wrapped <= 0.01,
            "two neighbouring samples of a field this smooth differ by {wrapped:.5}, which is \
             a discontinuity rather than the sweep's own step"
        );
    }

    /// The two shaders that weigh a probe against this map, and the file each
    /// one is.
    ///
    /// `mesh.slang` is the diffuse gather and `ssr.slang` the reflection's probe
    /// fallback; both read the same eight rows of the same table, so both have
    /// to weigh them by the same bound or the specular term leaks through a wall
    /// the diffuse term does not.
    const SOURCES: [(&str, &str); 2] = [
        ("mesh.slang", include_str!("../shaders/mesh.slang")),
        ("ssr.slang", include_str!("../shaders/ssr.slang")),
    ];

    /// The body of the function named `signature` in `source`, brace to brace.
    ///
    /// [`None`] when that file has no such function, which is how a missing copy
    /// is reported as an absence rather than as a difference.
    fn body_of(source: &str, signature: &str) -> Option<String> {
        let at = source.find(signature)?;
        let open = source[at..].find('{')? + at;
        let mut depth = 0usize;
        for (offset, byte) in source[open..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(source[open..open + offset + 1].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// **Both shaders must name the same four numbers this module does.**
    ///
    /// Nothing else can catch a drift: each file compiles with any of them, and
    /// a mismatch shows up only as a frame lit a little wrong on whatever
    /// machine happens to look — a border offset out by one reads the wrong
    /// texels, a surface bias out by a factor lets a floor shadow itself, and an
    /// occluded weight out by orders of magnitude turns the fully occluded case
    /// from the trilinear blend into black. Reading the source is the check, and
    /// the source is hash-pinned by the manifest, so it is the same file the
    /// committed artifact was built from.
    #[test]
    fn the_visibility_map_constants_match_the_ones_the_shaders_declare() {
        for declaration in [
            format!(
                "static const float PROBE_VISIBILITY_SIDE = {:.1};",
                f64::from(SIDE)
            ),
            format!(
                "static const float PROBE_VISIBILITY_BORDER = {:.1};",
                f64::from(BORDER)
            ),
            format!("static const float PROBE_SURFACE_BIAS = {SURFACE_BIAS};"),
            format!("static const float PROBE_OCCLUDED_WEIGHT = {OCCLUDED_WEIGHT};"),
        ] {
            for (name, source) in SOURCES {
                assert!(
                    source.contains(&declaration),
                    "{name} does not declare `{declaration}`; the visibility map's constants \
                     have drifted from the shader that reads it"
                );
            }
        }
    }

    /// **The two shaders weigh a probe with the same arithmetic, character for
    /// character** — and it is the arithmetic [`ProbeVisibility::moments`] and
    /// [`ProbeVisibility::weight`] mirror.
    ///
    /// `crate::probe`'s `the_shaders_pick_a_level_the_way_this_module_does`
    /// applied to the read within a level: the manifest hashes one source per
    /// artifact, so an `#include` would be a file whose edits nothing downstream
    /// notices, and the two copies are written out instead. Nothing else in the
    /// tree would notice one being fixed and the other left — both files compile
    /// either way, and the failure is a reflection that takes light from a probe
    /// behind the wall the surface stands against while the diffuse term at the
    /// same point does not.
    ///
    /// **`oct_encode` and `sign_not_zero` are held too**, not only the two the
    /// bound is named after: `probe_moments` reads a texel through them, so a
    /// drift in either moves the answer while the pinned bodies stay identical —
    /// which is the silent half of the same failure.
    ///
    /// The bodies compare rather than the whole declarations, so each file's doc
    /// comment can say what that file uses them for; they do.
    #[test]
    fn the_shaders_weigh_a_probe_the_way_this_module_does() {
        for signature in [
            "float sign_not_zero(float value)",
            "float2 oct_encode(float3 direction)",
            "float2 probe_moments(uint index, float3 direction)",
            "float probe_weight(uint index, float3 probe_position, float3 world_position, \
             float3 normal)",
        ] {
            let copies: Vec<(&str, String)> = SOURCES
                .into_iter()
                .map(|(name, source)| {
                    let body = body_of(source, signature)
                        .unwrap_or_else(|| panic!("{name} does not declare `{signature}`"));
                    (name, body)
                })
                .collect();
            assert_eq!(
                copies[0].1, copies[1].1,
                "`{signature}` differs between {} and {}; the visibility weight is copied \
                 verbatim and one copy has drifted",
                copies[0].0, copies[1].0
            );
        }
    }

    /// The bound's two ends and its shape between them, against the arithmetic
    /// written out by hand — a transcription slip in it would pass every other
    /// test here, because nothing else knows what the right answer was.
    #[test]
    fn the_chebyshev_bound_matches_the_inequality() {
        // In front of the mean is fully visible, whatever the variance.
        assert_eq!(chebyshev(2.0, 4.5, 1.0), 1.0);
        assert_eq!(chebyshev(2.0, 4.5, 2.0), 1.0);
        // Behind it: `σ² / (σ² + (d - μ)²)`, cubed.
        let mean = 2.0f32;
        let mean_squared = 4.25f32;
        let variance = mean_squared - mean * mean;
        for distance in [2.5f32, 3.0, 4.0, 10.0] {
            let behind = distance - mean;
            let want = (variance / (variance + behind * behind)).powi(3);
            let got = chebyshev(mean, mean_squared, distance);
            assert!(
                (got - want).abs() <= 1.0e-6,
                "at {distance} the bound is {want} and this returned {got}"
            );
        }
        // A wall with no variance at all cuts off exactly.
        assert_eq!(chebyshev(2.0, 4.0, 2.5), 0.0);
    }

    /// **No capture occludes nothing**, which is what keeps a scene that never
    /// captured drawing the frame it always did.
    #[test]
    fn an_empty_map_lets_every_probe_through() {
        let none = ProbeVisibility::NONE;
        assert_eq!(none.probes(), 0);
        assert!(none.bytes().is_empty());
        for direction in directions() {
            assert_eq!(none.moments(0, direction), [FAR, FAR * FAR]);
        }
        assert_eq!(
            none.weight(0, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            1.0
        );
    }

    /// A wall between a probe and a surface takes the probe's weight away, and
    /// the same probe with the wall behind it keeps all of it.
    ///
    /// The map is authored rather than captured, so this is a statement about
    /// [`ProbeVisibility::weight`] alone — `crcbl_render::probe_visibility` is
    /// where the capture that fills such a map is checked.
    #[test]
    fn a_probe_behind_a_wall_keeps_no_weight() {
        // Every direction sees a wall one unit away.
        let mut layer = vec![0u8; LAYER_BYTES];
        for texel in 0..(EXTENT * EXTENT) as usize {
            let at = texel * TEXEL_BYTES;
            layer[at..at + 4].copy_from_slice(&1.0f32.to_le_bytes());
            layer[at + 4..at + 8].copy_from_slice(&1.02f32.to_le_bytes());
        }
        let map = ProbeVisibility::new(layer);
        let up = [0.0, 1.0, 0.0];
        let near = map.weight(0, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], up);
        let far = map.weight(0, [0.0, 4.0, 0.0], [0.0, 0.0, 0.0], up);
        assert_eq!(
            near, 1.0,
            "a probe in front of the wall it sees keeps its weight"
        );
        assert_eq!(
            far, OCCLUDED_WEIGHT,
            "a probe four units away behind a wall one unit away must keep only the floor"
        );
    }
}
