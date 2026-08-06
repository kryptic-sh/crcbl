//! Breakout's art: the baked sheets, the layers they are drawn on, and the one
//! function that turns a frame of simulation into a list of sprites.
//!
//! ```text
//! assets/*.crpix ──build.rs──▶ PNG + sidecar ──include_bytes!──▶ Scene::new
//!                                                                    │
//!  RenderState ────────────────────────── Scene::build ──────────────┤
//!                                                                    ▼
//!                          LayerStack    court ▸ play        ──▶ &[Sprite]
//!                                                                    │
//!                                                     SpriteRenderer::begin_frame
//! ```
//!
//! # The sprite plane is world units, and the art scale lives on the source
//!
//! **Every rectangle in this module is in world units**, and the camera the
//! sprite pass is given matches. That is possible because
//! [`NineSliceSource`] carries its own texels-per-unit scale:
//! [`with_texels_per_unit`](NineSliceSource::with_texels_per_unit) divides the
//! fixed bands of an expansion by the scale, so `assets/field.crpix`'s 10-texel
//! wall comes out one world unit thick against a court 28 across instead of
//! ten. Flappy reached the same convention and `apps/flappy/src/art.rs`
//! explains it; the two scales are each sample's own, not a house style.
//!
//! **The number is breakout's own, and was not copied.** Flappy's 20 is derived
//! from a pipe 1.6 world units wide drawn from 32 texels of art. Here the
//! smallest thing on the field is the ball, `2 * BALL_RADIUS` = 0.6 units, and
//! six texels is the least that can be drawn round — which fixes the scale at
//! 10 and then lands every other collider on a whole texel: a brick 24 × 8, the
//! paddle 100 × 6, a wall 10 thick. See [`TEXELS_PER_UNIT`].
//!
//! # There is no parallax here, and that is not an omission
//!
//! Breakout's camera never moves: the field is fixed, the whole of it is on
//! screen, and [`crate::gpu`] centres the view on the origin every frame.
//! [`Parallax`] is `(1 − factor) × camera`, so with a camera at the origin
//! *every* factor produces the same offset of zero — a "distant" band and a
//! world-locked one would be the same picture. The [`LayerStack`] is here for
//! depth ordering, which is the half of it that still means something, and both
//! layers are [`Parallax::WORLD`]. A band that scrolled would be a band moving
//! for a reason the player cannot see.

use crcbl::hal::{Device, HalError};
use crcbl::math::DVec3;
use crcbl::render::{
    Layer, LayerStack, NineSliceSource, Parallax, SheetDesc, SheetId, Sprite, SpriteRenderer,
};
use crcbl::sprite::Sheet;
use crcbl::sprite::load::{Loaded, load};

use crate::game::{
    BALL_RADIUS, BRICK_GAP, BRICK_HEIGHT, BRICK_ROWS, BRICK_TOP, BRICK_WIDTH, PADDLE_HALF_HEIGHT,
    PADDLE_HALF_WIDTH, PADDLE_Y, WALL_THICKNESS, WORLD_LEFT, WORLD_RIGHT, WORLD_TOP,
};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`,
// with the sidecar `None` for art that needs no metadata.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

// ---------------------------------------------------------------------------
// The scale, the clock, and the colours
// ---------------------------------------------------------------------------

/// Texels of art per world unit — the art's authoring scale.
///
/// The court's nine-slice source is built with
/// [`with_texels_per_unit`](NineSliceSource::with_texels_per_unit) at this
/// scale, so its walls come out in world units, and the texel sizes of the
/// sheets below are converted to world units by it. It does **not** scale the
/// sprite plane or the camera — the plane is world units now.
///
/// **10 is chosen by the ball.** `2 * BALL_RADIUS` is 0.6 world units, the
/// smallest collider on the field, and a ball has to be at least six texels
/// across before the corners can be cut off a square and leave something round.
/// Six texels over 0.6 units is ten to the unit, and every other collider then
/// falls on a whole texel: a brick is 24 × 8, the paddle 100 × 6, and a wall —
/// [`WALL_THICKNESS`], the inset `assets/field.crpix` declares — is 10.
///
/// Half of this would put the ball at three texels, which cannot be drawn.
/// Double it and the paddle is two hundred texels of hand-written rows for a
/// shape that is a rectangle with rounded ends.
pub const TEXELS_PER_UNIT: f32 = 10.0;

/// What is outside the court, as the clear value for the swapchain.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; this is `#0a0a0f` put through the sRGB→linear transfer
/// function once, here, rather than looking washed out on screen.
///
/// Nearly black on purpose. It is the margin the camera keeps around the field
/// and the gap under the paddle that a lost ball falls through, and both want to
/// read as "not the game" rather than as scenery.
pub const SURROUND: [f32; 4] = [0.00304, 0.00304, 0.00478, 1.0];

/// "The sheet as authored" — no tinting anywhere in this game.
///
/// The brick rows were tints once: one white rectangle and a four-entry colour
/// table in `app.rs`. They are four frames of one sheet now, which is what lets
/// them differ in their shading and not only in their hue.
const UNTINTED: [f32; 4] = [1.0; 4];

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

/// The brick sheet: one sheet, and the UV rectangle for each row of the grid.
///
/// The UVs are resolved once at start-up rather than asked of the [`Sheet`] per
/// brick — there are forty of them a frame and the answer never changes.
#[derive(Clone, Copy, Debug)]
struct BrickArt {
    sheet: SheetId,
    uv: [[f32; 4]; BRICK_ROWS],
}

/// A sheet with exactly one frame in it, drawn at a rectangle the caller
/// computes: the paddle and the ball.
#[derive(Clone, Copy, Debug)]
struct StillArt {
    sheet: SheetId,
    uv: [f32; 4],
}

/// The court: one sheet and the three-sided nine-slice cut out of it.
#[derive(Clone, Copy, Debug)]
struct FieldArt {
    sheet: SheetId,
    source: NineSliceSource,
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// Everything breakout draws, and the layers it draws them on.
///
/// Built once — registering a sheet is a blocking staging upload — and then
/// [`Scene::build`] per frame, which clears the stack and refills it without
/// allocating.
#[derive(Debug)]
pub struct Scene {
    stack: LayerStack,
    /// The court, behind everything.
    field: FieldArt,
    field_layer: Layer,
    bricks: BrickArt,
    paddle: StillArt,
    ball: StillArt,
    /// The layer the game is played on: bricks, then the paddle, then the ball
    /// in front of both.
    play: Layer,
}

impl Scene {
    /// Loads the baked sheets, uploads them, and builds the layer stack.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a sheet upload or a bind group was refused. Nothing here
    /// fails on the *art*: it was parsed, validated and baked by `build.rs`, so
    /// a sheet that will not load is a bug in this repository rather than a
    /// runtime condition, and it panics naming which one.
    ///
    /// # Panics
    ///
    /// If a baked sheet cannot be read back, or does not contain the frames this
    /// module names.
    pub fn new(device: &dyn Device, sprites: &mut SpriteRenderer) -> Result<Self, HalError> {
        let bricks = baked("bricks", BRICKS_PNG, BRICKS_JSON);
        let paddle = baked("paddle", PADDLE_PNG, PADDLE_JSON);
        let ball = baked("ball", BALL_PNG, BALL_JSON);
        let field = baked("field", FIELD_PNG, FIELD_JSON);

        let bricks_sheet = register(device, sprites, "bricks", &bricks)?;
        let paddle_sheet = register(device, sprites, "paddle", &paddle)?;
        let ball_sheet = register(device, sprites, "ball", &ball)?;
        let field_sheet = register(device, sprites, "field", &field)?;

        // Back to front, and this is the only place the depth order is written
        // down: `LayerStack` has no depth field to disagree with it. Both take
        // the world's rate — see the module docs on why a second rate would be
        // a lie in a game whose camera does not move.
        let mut stack = LayerStack::new();
        let field_layer = stack.push_layer(Parallax::WORLD);
        let play = stack.push_layer(Parallax::WORLD);

        Ok(Self {
            stack,
            field: FieldArt {
                sheet: field_sheet,
                source: NineSliceSource::from_sheet(&field.sheet, 0)
                    .expect("field.crpix declares a nine-slice over its one frame")
                    .with_texels_per_unit(TEXELS_PER_UNIT),
            },
            field_layer,
            bricks: BrickArt {
                sheet: bricks_sheet,
                uv: std::array::from_fn(|row| {
                    bricks
                        .sheet
                        .uv(row)
                        .expect("bricks.crpix declares one frame per brick row")
                }),
            },
            paddle: still(paddle_sheet, &paddle.sheet),
            ball: still(ball_sheet, &ball.sheet),
            play,
        })
    }

    /// This frame's sprites, back to front.
    ///
    /// Everything is in **world units**, and the same frame's view-projection
    /// must have been built at the same scale. [`crate::gpu`] builds it in
    /// world units too.
    pub fn build(&mut self, paddle_x: f64, ball: DVec3, bricks: &[DVec3]) -> &[Sprite] {
        self.stack.clear();

        let quads = self.field.source.expand(field_target());
        self.stack
            .extend(self.field_layer, quads.sprites(self.field.sheet, UNTINTED));

        let brick_art = self.bricks;
        self.stack.extend(
            self.play,
            bricks.iter().map(move |centre| Sprite {
                sheet: brick_art.sheet,
                rect: brick_rect(*centre),
                rotation: 0.0,
                uv: brick_art.uv[brick_frame(centre.y)],
                tint: UNTINTED,
            }),
        );

        self.stack.push(
            self.play,
            Sprite {
                sheet: self.paddle.sheet,
                rect: paddle_rect(paddle_x),
                rotation: 0.0,
                uv: self.paddle.uv,
                tint: UNTINTED,
            },
        );
        // Last, so a ball resting on the paddle's face is in front of it rather
        // than half swallowed by it.
        self.stack.push(
            self.play,
            Sprite {
                sheet: self.ball.sheet,
                rect: ball_rect(ball),
                rotation: 0.0,
                uv: self.ball.uv,
                tint: UNTINTED,
            },
        );

        // The camera is at the origin and both layers are `Parallax::WORLD`, so
        // this offset is zero twice over. It is passed rather than assumed
        // because `resolve` is what turns the stack into a frame, and a scene
        // that later grows a camera-locked layer must go through it.
        self.stack.resolve([0.0, 0.0])
    }
}

// ---------------------------------------------------------------------------
// Pure geometry — no device, no sheet ids
// ---------------------------------------------------------------------------

/// The rectangle the court is expanded into, in world units.
///
/// The **walls' outer faces**, derived from where `game.rs` puts them: each side
/// wall is [`WALL_THICKNESS`] thick and sits outside [`WORLD_LEFT`] /
/// [`WORLD_RIGHT`], the top wall the same above [`WORLD_TOP`], and the side
/// walls run down to `-WORLD_TOP`. The nine-slice's insets divided by its
/// texels-per-unit scale are that same thickness, so the *inner* faces of the
/// drawn walls land exactly on the lines the ball bounces off — which is what
/// [`tests::the_court_is_drawn_where_the_walls_actually_are`] measures.
///
/// There is no bottom wall and the art has no bottom inset, so the court is open
/// at `-WORLD_TOP` and the ball falls out of it into [`SURROUND`].
fn field_target() -> [f32; 4] {
    let left = WORLD_LEFT - WALL_THICKNESS;
    let bottom = -WORLD_TOP;
    let right = WORLD_RIGHT + WALL_THICKNESS;
    let top = WORLD_TOP + WALL_THICKNESS;
    [
        left as f32,
        bottom as f32,
        (right - left) as f32,
        (top - bottom) as f32,
    ]
}

/// One brick's rectangle, in world units: the box collider `spawn_brick_grid`
/// gives it, to the texel.
fn brick_rect(centre: DVec3) -> [f32; 4] {
    rect(centre.x, centre.y, BRICK_WIDTH / 2.0, BRICK_HEIGHT / 2.0)
}

/// The paddle's rectangle, in world units, at the x the simulation reports.
fn paddle_rect(x: f64) -> [f32; 4] {
    rect(x, PADDLE_Y, PADDLE_HALF_WIDTH, PADDLE_HALF_HEIGHT)
}

/// The ball's rectangle, in world units: its sphere collider's bounding square.
fn ball_rect(centre: DVec3) -> [f32; 4] {
    rect(centre.x, centre.y, BALL_RADIUS, BALL_RADIUS)
}

/// A world-space centre and half-extents as a sprite rectangle: `[x, y, w, h]`,
/// **minimum corner first**, which is what [`Sprite::rect`] takes.
fn rect(cx: f64, cy: f64, half_w: f64, half_h: f64) -> [f32; 4] {
    [
        (cx - half_w) as f32,
        (cy - half_h) as f32,
        (2.0 * half_w) as f32,
        (2.0 * half_h) as f32,
    ]
}

/// Which frame of `assets/bricks.crpix` a brick at world `y` is drawn with.
///
/// **The inverse of `game::brick_position`**, and the only place the two are
/// related. The simulation hands the renderer a bare list of live brick centres
/// — it has no row index to send, because a row is not a thing the physics
/// knows about — so the row is read back out of the position it put the brick
/// at. Rows are `BRICK_HEIGHT + BRICK_GAP` apart from [`BRICK_TOP`] down, which
/// is the arithmetic below, run backwards.
///
/// Clamped rather than trusted: a `y` from a brick this grid did not lay out
/// would otherwise index off the end of a four-entry array.
fn brick_frame(y: f64) -> usize {
    let pitch = BRICK_HEIGHT + BRICK_GAP;
    let row = ((BRICK_TOP - y) / pitch).round();
    row.clamp(0.0, (BRICK_ROWS - 1) as f64) as usize
}

// ---------------------------------------------------------------------------
// Start-up helpers
// ---------------------------------------------------------------------------

/// Decodes one baked sheet.
fn baked(name: &str, png: &[u8], json: Option<&str>) -> Loaded {
    load(png, json, ART_TICK_HZ)
        .unwrap_or_else(|error| panic!("the baked {name} sheet did not load: {error}"))
}

fn register(
    device: &dyn Device,
    sprites: &mut SpriteRenderer,
    label: &str,
    loaded: &Loaded,
) -> Result<SheetId, HalError> {
    sprites.register_sheet(
        device,
        &SheetDesc {
            label,
            width: loaded.image.width,
            height: loaded.image.height,
            sample: loaded.sheet.sample,
            pixels: &loaded.image.pixels,
        },
    )
}

/// A single-frame sheet, resolved to the one UV rectangle it will ever use.
fn still(sheet: SheetId, description: &Sheet) -> StillArt {
    StillArt {
        sheet,
        uv: description.uv(0).expect("a still sheet has one frame"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::hal::null::NullInstance;
    use crcbl::hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl::sprite::NineSlice;

    use crate::game::{BRICK_COLS, BRICK_COUNT, brick_position};

    /// Runs `body` against a scene built on the null backend.
    ///
    /// A real [`SpriteRenderer`] rather than a stub, because [`SheetId`] is
    /// opaque outside `crcbl-render` and there is no other way to have one —
    /// which is the point of it being opaque.
    fn with_scene(body: impl FnOnce(&mut Scene)) {
        let instance = NullInstance::tier_a();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut sprites = SpriteRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts the sprite pipeline");
        let mut scene = Scene::new(device.as_ref(), &mut sprites).expect("the sheets upload");
        body(&mut scene);
        sprites.destroy(device.as_ref());
    }

    /// Frame `index` of a sheet, as its own block of RGBA, cut out of the strip.
    fn frame_pixels(loaded: &Loaded, index: usize) -> Vec<u8> {
        let rect = loaded.sheet.frames[index].rect;
        let stride = loaded.image.width as usize * 4;
        (0..rect.h as usize)
            .flat_map(|row| {
                let start = (rect.y as usize + row) * stride + rect.x as usize * 4;
                loaded.image.pixels[start..start + rect.w as usize * 4].to_vec()
            })
            .collect()
    }

    /// A full grid of live bricks, as the simulation reports one on the first
    /// frame of a run.
    fn full_grid() -> Vec<DVec3> {
        (0..BRICK_COUNT).map(brick_position).collect()
    }

    // -----------------------------------------------------------------------
    // The art itself
    // -----------------------------------------------------------------------

    /// **The art is the art that was authored.** Sizes, frame names, holds and
    /// the nine-slice — every one of them a number written in a `.crpix` and
    /// carried through parse, bake and load.
    ///
    /// A test that only checked `bake` returned `Ok` would pass on a blank image
    /// with no frames in it, which is exactly the failure this is for; the alpha
    /// counts at the end are what rule that out.
    #[test]
    fn the_art_bakes_to_the_sheets_it_declares() {
        let bricks = baked("bricks", BRICKS_PNG, BRICKS_JSON);
        assert_eq!(
            (bricks.image.width, bricks.image.height),
            (24 * BRICK_ROWS as u32, 8),
            "four 24x8 frames in one horizontal strip"
        );
        let names: Vec<&str> = bricks
            .sheet
            .frames
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(names, ["red", "orange", "yellow", "green"]);
        for (index, frame) in bricks.sheet.frames.iter().enumerate() {
            assert_eq!(
                frame.rect,
                crcbl::sprite::Rect::new(index as u32 * 24, 0, 24, 8),
                "frame {index} is not a 24x8 cell of the strip"
            );
            assert_eq!(
                frame.hold, 1,
                "the default hold in ticks did not survive the millisecond \
                 round trip, so bake and load disagree about milliseconds"
            );
        }
        assert!(
            bricks.sheet.clips.is_empty(),
            "a brick does not animate; a clip here is art that was never asked for"
        );

        let paddle = baked("paddle", PADDLE_PNG, PADDLE_JSON);
        assert_eq!((paddle.image.width, paddle.image.height), (100, 6));
        assert_eq!(paddle.sheet.frames.len(), 1);
        assert_eq!(
            paddle.sheet.nine, None,
            "the paddle is one fixed width, so it is a picture and not a stretch"
        );

        let ball = baked("ball", BALL_PNG, BALL_JSON);
        assert_eq!((ball.image.width, ball.image.height), (6, 6));
        assert!(
            BALL_JSON.is_none(),
            "one still frame with no clip and no nine-slice needs no sidecar"
        );

        let field = baked("field", FIELD_PNG, FIELD_JSON);
        assert_eq!((field.image.width, field.image.height), (22, 12));
        assert_eq!(
            field.sheet.nine,
            Some(NineSlice::new(10, 10, 10, 0)),
            "a three-sided slice: breakout's bottom edge is the way out, not a wall"
        );

        // Anti-blank: the ball and the paddle have transparent corners *and*
        // opaque pixels, and the court is opaque everywhere. A sheet of zeroes
        // would satisfy every assertion above.
        let alpha = |loaded: &Loaded| -> (usize, usize) {
            let clear = loaded
                .image
                .pixels
                .chunks_exact(4)
                .filter(|p| p[3] == 0)
                .count();
            (clear, loaded.image.pixels.len() / 4 - clear)
        };
        for (name, loaded) in [("ball", &ball), ("paddle", &paddle)] {
            let (clear, opaque) = alpha(loaded);
            assert!(
                clear > 0 && opaque > 0,
                "{name}: {clear} clear, {opaque} opaque"
            );
        }
        assert_eq!(alpha(&field).0, 0, "the court has holes in it");
        assert_eq!(alpha(&bricks).0, 0, "a brick has holes in it");
    }

    /// **The four brick frames are four different pictures.**
    ///
    /// Four identical frames parse, bake, load and index exactly as four
    /// different ones do, and every other test in this file passes on them — so
    /// "the rows draw different frames" is worth nothing until the frames
    /// themselves are known to differ.
    #[test]
    fn the_brick_rows_are_actually_different_pictures() {
        let bricks = baked("bricks", BRICKS_PNG, BRICKS_JSON);
        let frames: Vec<Vec<u8>> = (0..BRICK_ROWS).map(|i| frame_pixels(&bricks, i)).collect();
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(frame.len(), 24 * 8 * 4);
            assert!(
                frame.chunks_exact(4).any(|p| p[3] != 0),
                "row {index} has nothing drawn in it"
            );
        }
        for a in 0..BRICK_ROWS {
            for b in (a + 1)..BRICK_ROWS {
                assert_ne!(
                    frames[a], frames[b],
                    "brick rows {a} and {b} are the same picture, so the grid \
                     reads as one wall recoloured"
                );
            }
        }

        // And each frame is shaded rather than flat: the lit row under the rim
        // and the shaded row above it differ, which is what a tint could not
        // have given.
        for (index, frame) in frames.iter().enumerate() {
            let row = |r: usize| frame[r * 24 * 4..(r + 1) * 24 * 4].to_vec();
            assert_ne!(row(1), row(6), "row {index} is a flat rectangle");
        }
    }

    /// **The court's walls keep their thickness however wide the court is.**
    ///
    /// The whole reason `field.crpix` is a nine-slice, and the assertion that
    /// catches the source's texels-per-unit scale being dropped: without
    /// `with_texels_per_unit(TEXELS_PER_UNIT)` the 10-texel inset would be ten
    /// *world* units of wall inside a court 28 across.
    #[test]
    fn the_courts_walls_keep_their_thickness_when_it_is_stretched() {
        let field = baked("field", FIELD_PNG, FIELD_JSON);
        // Built the way [`Scene::new`] builds it, so the walls are world units.
        let source = NineSliceSource::from_sheet(&field.sheet, 0)
            .expect("the sheet declares a nine-slice")
            .with_texels_per_unit(TEXELS_PER_UNIT);

        let target = field_target();
        let quads = source.expand(target);
        // Six, not nine: the bottom inset is zero, so the three quads of the
        // bottom band have no texels in them and are dropped.
        assert_eq!(quads.len(), 6, "{quads:?}");

        // The top-left corner is the fixed one, and it is a wall thick in both
        // directions — the 10-texel inset at `TEXELS_PER_UNIT` = 10 is one
        // world unit, [`WALL_THICKNESS`].
        let corner = quads[0].rect;
        let thickness = WALL_THICKNESS as f32;
        assert_eq!(corner[2], thickness, "the left wall is not one unit thick");
        assert_eq!(corner[3], thickness, "the top wall is not one unit thick");

        // And the quads tile the target exactly: no seam, nothing outside it.
        let area: f32 = quads.iter().map(|q| q.rect[2] * q.rect[3]).sum();
        assert!(
            (area - target[2] * target[3]).abs() < 1e-1,
            "the six quads cover {area} of a {} target",
            target[2] * target[3]
        );
    }

    // -----------------------------------------------------------------------
    // The scene
    // -----------------------------------------------------------------------

    /// Each sheet goes on the layer it belongs to, back to front, and the play
    /// layer carries the grid, the paddle and the ball in that order.
    #[test]
    fn every_sheet_is_submitted_on_the_layer_it_belongs_to() {
        with_scene(|scene| {
            assert_eq!(scene.stack.layer_count(), 2);
            let grid = full_grid();
            scene.build(0.0, DVec3::new(0.0, -5.0, 0.0), &grid);

            assert_eq!(scene.field_layer.depth(), 0, "the court is behind the game");
            assert_eq!(scene.play.depth(), 1);

            let court = scene.stack.sprites(scene.field_layer).to_vec();
            assert_eq!(court.len(), 6, "the court is six nine-slice quads");
            assert!(
                court.iter().all(|s| s.sheet == scene.field.sheet),
                "something that is not the court is on the court's layer"
            );

            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(
                play.len(),
                BRICK_COUNT + 2,
                "forty bricks, a paddle, a ball"
            );
            assert!(
                play[..BRICK_COUNT]
                    .iter()
                    .all(|s| s.sheet == scene.bricks.sheet)
            );
            assert_eq!(play[BRICK_COUNT].sheet, scene.paddle.sheet);
            assert_eq!(
                play[BRICK_COUNT + 1].sheet,
                scene.ball.sheet,
                "the ball must be pushed last, or the paddle covers it"
            );

            // And the resolved frame is the two layers concatenated, back first
            // — which is what the sprite pass draws in order.
            let flat = scene.stack.resolved();
            assert_eq!(flat.len(), court.len() + play.len());
            assert_eq!(flat[0].sheet, scene.field.sheet);
            assert_eq!(flat.last().expect("non-empty").sheet, scene.ball.sheet);
        });
    }

    /// **Each row of the grid draws its own frame of the sheet.**
    ///
    /// Asserted as UVs, not as "a sprite was submitted": four rows all drawing
    /// frame 0 submit exactly as many sprites at exactly the same rectangles,
    /// and the whole point of a four-frame sheet is that they do not.
    #[test]
    fn the_brick_rows_draw_the_frames_they_are_meant_to() {
        with_scene(|scene| {
            let grid = full_grid();
            scene.build(0.0, DVec3::ZERO, &grid);
            let play = scene.stack.sprites(scene.play).to_vec();

            // The four UVs are four different cells of the strip. Without this
            // the per-row assertion below would hold on a sheet whose frames all
            // covered the same texels.
            let uvs = scene.bricks.uv;
            for a in 0..BRICK_ROWS {
                for b in (a + 1)..BRICK_ROWS {
                    assert_ne!(uvs[a], uvs[b], "rows {a} and {b} cut the same cell");
                }
            }

            for (index, sprite) in play[..BRICK_COUNT].iter().enumerate() {
                let row = index / BRICK_COLS;
                assert_eq!(
                    sprite.uv,
                    uvs[row],
                    "brick {index} is in row {row} and drew frame {:?}",
                    uvs.iter().position(|uv| *uv == sprite.uv)
                );
            }

            // Each of the four is actually used — a grid that lost its bottom
            // row would still pass the loop above for the rows it had left.
            let drawn: Vec<[f32; 4]> = play[..BRICK_COUNT].iter().map(|s| s.uv).collect();
            for (row, uv) in uvs.iter().enumerate() {
                assert_eq!(
                    drawn.iter().filter(|d| *d == uv).count(),
                    BRICK_COLS,
                    "row {row} is not ten bricks wide on screen"
                );
            }
        });
    }

    /// **The rectangles drawn are the rectangles collided against.**
    ///
    /// Forty bricks, the paddle and the ball, each checked against the collider
    /// `game.rs` gives it rather than against a number repeated here. This is
    /// the assertion that catches art drifting from physics — a brick sprite an
    /// eighth of a unit wider than its box looks fine and makes the ball bounce
    /// off nothing.
    #[test]
    fn every_sprite_covers_the_collider_it_stands_for() {
        with_scene(|scene| {
            let grid = full_grid();
            let paddle_x = 3.5;
            let ball = DVec3::new(-2.0, -4.25, 0.0);
            scene.build(paddle_x, ball, &grid);
            let play = scene.stack.sprites(scene.play).to_vec();

            // `[x, y, w, h]` against a centre and half-extents, in world units.
            let covers = |rect: [f32; 4], cx: f64, cy: f64, hw: f64, hh: f64, what: &str| {
                let expected = [
                    (cx - hw) as f32,
                    (cy - hh) as f32,
                    (2.0 * hw) as f32,
                    (2.0 * hh) as f32,
                ];
                for (axis, (got, want)) in rect.iter().zip(expected.iter()).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-3,
                        "{what}: component {axis} is {got}, the collider says {want}"
                    );
                }
            };

            for (index, sprite) in play[..BRICK_COUNT].iter().enumerate() {
                let centre = brick_position(index);
                covers(
                    sprite.rect,
                    centre.x,
                    centre.y,
                    BRICK_WIDTH / 2.0,
                    BRICK_HEIGHT / 2.0,
                    &format!("brick {index}"),
                );
            }
            covers(
                play[BRICK_COUNT].rect,
                paddle_x,
                PADDLE_Y,
                PADDLE_HALF_WIDTH,
                PADDLE_HALF_HEIGHT,
                "the paddle",
            );
            covers(
                play[BRICK_COUNT + 1].rect,
                ball.x,
                ball.y,
                BALL_RADIUS,
                BALL_RADIUS,
                "the ball",
            );

            // Anti-vacuity: the helper above can fail. A brick drawn at its
            // neighbour's centre is a rectangle of the right size in the wrong
            // place, and `covers` says so.
            let neighbour = brick_position(1);
            let first = brick_position(0);
            assert_ne!(first.x, neighbour.x);
            assert!(
                (brick_rect(neighbour)[0] - brick_rect(first)[0]).abs() > 1.0,
                "two different bricks produced the same rectangle"
            );
        });
    }

    /// **The court is drawn where the walls actually are.**
    ///
    /// The inner face of each drawn wall lands on the line `game.rs` bounces the
    /// ball off — `WORLD_LEFT`, `WORLD_RIGHT`, `WORLD_TOP` — and the court is
    /// open at the bottom, where there is no wall to draw.
    #[test]
    fn the_court_is_drawn_where_the_walls_actually_are() {
        let target = field_target();
        assert_eq!(target[0], (WORLD_LEFT - WALL_THICKNESS) as f32);
        assert_eq!(target[1], -WORLD_TOP as f32);
        assert_eq!(target[0] + target[2], (WORLD_RIGHT + WALL_THICKNESS) as f32);
        assert_eq!(target[1] + target[3], (WORLD_TOP + WALL_THICKNESS) as f32);

        let field = baked("field", FIELD_PNG, FIELD_JSON);
        let source = NineSliceSource::from_sheet(&field.sheet, 0)
            .expect("a nine-slice")
            .with_texels_per_unit(TEXELS_PER_UNIT);
        let quads = source.expand(target);

        // In image order: top-left, top edge, top-right, then left edge, centre,
        // right edge. The three inner faces are the far side of the three fixed
        // bands.
        let left_face = quads[0].rect[0] + quads[0].rect[2];
        let right_face = quads[2].rect[0];
        let top_face = quads[0].rect[1];
        assert_eq!(left_face, WORLD_LEFT as f32, "the left wall's face");
        assert_eq!(right_face, WORLD_RIGHT as f32, "the right wall's face");
        assert_eq!(top_face, WORLD_TOP as f32, "the ceiling's face");

        // The centre band reaches the bottom of the target: nothing is drawn
        // below it, because nothing is there to bounce off.
        let centre = quads[4].rect;
        assert_eq!(centre[1], target[1], "the court has grown a floor");
    }

    /// The row a brick is drawn as is the row the grid put it in, for every
    /// brick in it — the inverse of `game::brick_position`, checked against the
    /// function itself rather than against a repeated constant.
    #[test]
    fn a_bricks_frame_is_the_row_the_grid_laid_it_out_in() {
        for index in 0..BRICK_COUNT {
            let centre = brick_position(index);
            assert_eq!(
                brick_frame(centre.y),
                index / BRICK_COLS,
                "brick {index} at y = {} drew the wrong row",
                centre.y
            );
        }
        // Every row is reached, so the loop above is not four passes over row 0.
        let rows: std::collections::BTreeSet<usize> = (0..BRICK_COUNT)
            .map(|i| brick_frame(brick_position(i).y))
            .collect();
        assert_eq!(rows.len(), BRICK_ROWS);

        // Half a row's pitch either side still lands on the row, and a y from
        // nowhere near the grid is clamped rather than indexing off the end.
        let pitch = BRICK_HEIGHT + BRICK_GAP;
        assert_eq!(brick_frame(BRICK_TOP - pitch * 0.4), 0);
        assert_eq!(brick_frame(BRICK_TOP - pitch * 0.6), 1);
        assert_eq!(brick_frame(1000.0), 0);
        assert_eq!(brick_frame(-1000.0), BRICK_ROWS - 1);
    }

    /// A grid that has lost bricks draws what is left, and nothing else.
    #[test]
    fn a_broken_grid_draws_only_the_bricks_that_remain() {
        with_scene(|scene| {
            let full = full_grid();
            scene.build(0.0, DVec3::ZERO, &full);
            let before = scene.stack.sprites(scene.play).len();

            let survivors: Vec<DVec3> = full.iter().copied().skip(7).collect();
            scene.build(0.0, DVec3::ZERO, &survivors);
            let after = scene.stack.sprites(scene.play).len();
            // Written this way round rather than `before - after`, which
            // underflows — and reports as an arithmetic panic rather than as the
            // count being wrong — the moment a frame stops clearing the stack.
            assert_eq!(before, after + 7, "seven broken bricks are still drawn");

            scene.build(0.0, DVec3::ZERO, &[]);
            assert_eq!(
                scene.stack.sprites(scene.play).len(),
                2,
                "a cleared grid leaves the paddle and the ball"
            );
            // And the court is still there — a won game is not a blank screen.
            assert_eq!(scene.stack.sprites(scene.field_layer).len(), 6);
        });
    }
}
