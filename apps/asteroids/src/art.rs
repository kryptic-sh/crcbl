//! Asteroids' art: the baked sheets, the layers they are drawn on, and the one
//! function that turns a frame of simulation into a list of sprites.
//!
//! ```text
//! assets/*.crpix ──build.rs──▶ PNG + sidecar ──include_bytes!──▶ Scene::new
//!                                                                    │
//!  RenderState + alpha ────────────────── Scene::build ──────────────┤
//!                                                                    ▼
//!                          LayerStack    field ▸ play        ──▶ &[Sprite]
//!                                                                    │
//!                                                     SpriteRenderer::begin_frame
//! ```
//!
//! # Sprite space is world space times [`TEXELS_PER_UNIT`]
//!
//! Every rectangle in this module is in **texels**, and [`crate::gpu`]'s camera
//! is scaled to match — the convention breakout and flappy both reached.
//!
//! **Here it is only a convention, and that is worth saying plainly.** In the
//! other two samples it is load-bearing: [`NineSliceSource::expand`](crcbl::render::NineSliceSource::expand) takes its
//! insets as target units, so breakout's 10-texel wall would be ten world units
//! thick and flappy's pipe cap would lose its shape, and both had to scale their
//! whole sprite plane to compensate. That is the known trap, and it is in
//! `docs/backlog.md`. **Asteroids has no nine-slice.** Nothing it draws is
//! stretched: a ship, a shot and three rocks are five fixed pictures at five
//! fixed sizes, so `expand` is never called and the trap does not apply. The
//! scale is kept anyway because it makes every rectangle in this file read in
//! texels — a rock is 34 across, not 3.4 — and because a sample that used a
//! different convention from the other two for no reason would be a third thing
//! to learn. If `expand` ever grows the scale the backlog asks for, this file
//! needs no change at all.
//!
//! # The number is asteroids' own
//!
//! [`TEXELS_PER_UNIT`] is 10, which is also breakout's, and it was **not**
//! copied: it comes out of the smallest thing here that has to read as a shape.
//! See the constant.
//!
//! # Every angle is interpolated, and no position is
//!
//! [`Scene::build`] takes an `alpha` and [`lerp_angle`]s each rotation from the
//! previous tick's value to this one. [`RenderState`]'s own docs carry why the
//! positions are not, and `game::lerp_angle`'s carry why the short way round is
//! the only correct answer for an angle.

use crcbl::hal::{Device, HalError};
use crcbl::render::{Layer, LayerStack, Parallax, SheetDesc, SheetId, Sprite, SpriteRenderer};
use crcbl_sprite::Sheet;
use crcbl_sprite::load::{Loaded, load};
use glam::DVec3;

use crate::game::{BulletView, RenderState, RockSize, RockView, lerp_angle};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`,
// with the sidecar `None` for art that needs no metadata.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

// ---------------------------------------------------------------------------
// The scale, the clock, and the colours
// ---------------------------------------------------------------------------

/// Texels of art per world unit.
///
/// **Ten is chosen by the small rock**, which is the smallest thing in this game
/// that has to read as a *shape* rather than as a mark. `2 *
/// RockSize::Small.radius()` is 1.1 world units, and a rock has to be lumpy or
/// the split does not read — three sizes of one circle is one rock at three
/// magnifications. A lump needs a texel to stick out and a texel to bite in,
/// either side of a rim that is already a texel of outline, which puts the floor
/// at about eleven texels across. Eleven over 1.1 units is ten to the unit.
///
/// It then lands the other two rocks on whole texels as well — the medium is 20
/// and the large 34 — because every rock radius in `game.rs` is a multiple of
/// 0.05. The ship and the bullet are deliberately **not** their colliders' size,
/// each for a reason its own `.crpix` gives.
///
/// Half of this would put the small rock at five texels, which cannot be lumpy.
/// Double it and the large rock is 68 rows of hand-written art for a shape whose
/// whole job is to be an irregular blob.
///
/// Breakout reached 10 as well, from its ball. That is a coincidence of two
/// games whose smallest object is about a tenth of their field; flappy's is 20.
pub const TEXELS_PER_UNIT: f32 = 10.0;

/// The tick rate the sheets' frame holds were baked against.
///
/// **Must equal `build.rs`'s `ART_TICK_HZ`.** A `.crpix` counts holds in ticks
/// and an Aseprite sidecar counts milliseconds; bake converts one way and
/// [`load`] the other, so a mismatch scales every hold silently. Nothing here is
/// animated — the ship and the rocks *turn*, which is a rotation applied to a
/// still frame and not a clip — so this guard is as weak as breakout's, and for
/// the same reason. `docs/backlog.md` carries it.
const ART_TICK_HZ: u32 = 60;

/// What is outside the game, as the clear value for the swapchain.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; this is `#05050d` put through the sRGB→linear transfer
/// function once, here, rather than looking washed out on screen.
///
/// Near-black on purpose, and it is the whole background: this game has no
/// scenery, and space that read as a colour would compete with the rocks.
pub const SPACE: [f32; 4] = [0.00152, 0.00152, 0.00304, 1.0];

/// "The sheet as authored" — no tinting anywhere in this game.
const UNTINTED: [f32; 4] = [1.0; 4];

/// How much of a world unit the ship's sprite covers, as a half-extent.
///
/// **Larger than [`SHIP_RADIUS`](crate::game::SHIP_RADIUS), and that is the point.** `game.rs` says so in
/// as many words: the kill radius is deliberately smaller than the hull will
/// look, because a ship that dies to everything its longest spine touches dies
/// to near misses and this is a game of near misses. 0.8 against 0.55 is the
/// fins and the nozzle sticking out past the sphere.
pub const SHIP_HALF_EXTENT: f64 = 0.8;

/// The same for a shot.
///
/// Also larger than what it sweeps ([`BULLET_RADIUS`](crate::game::BULLET_RADIUS) = 0.12), because 0.24
/// units is 2.4 texels and a two-texel square is not a bullet, it is a fleck.
/// Nothing in the game can be hit *by* the drawn shot — the radius is the width
/// of a swept sphere, not a collider — so the sprite is free to be legible.
pub const BULLET_HALF_EXTENT: f64 = 0.2;

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

/// A sheet with exactly one frame in it, drawn at a rectangle the caller
/// computes. Every sheet in this game is one of these.
#[derive(Clone, Copy, Debug)]
struct StillArt {
    sheet: SheetId,
    uv: [f32; 4],
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// Everything asteroids draws, and the layers it draws them on.
///
/// Built once — registering a sheet is a blocking staging upload — and then
/// [`Scene::build`] per frame, which clears the stack and refills it without
/// allocating.
#[derive(Debug)]
pub struct Scene {
    stack: LayerStack,
    /// One per rock size, indexed by [`rock_index`]. Three sheets and not three
    /// frames of one: the sizes are 34, 20 and 11 texels square, and a `.crpix`
    /// has one frame size for the whole file.
    rocks: [StillArt; 3],
    ship: StillArt,
    bullet: StillArt,
    /// The rocks, behind the game.
    field: Layer,
    /// The ship and its shots, in front of them.
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
    /// If a baked sheet cannot be read back, or does not contain the frame this
    /// module names.
    pub fn new(device: &dyn Device, sprites: &mut SpriteRenderer) -> Result<Self, HalError> {
        let large = baked("rock_large", ROCK_LARGE_PNG, ROCK_LARGE_JSON);
        let medium = baked("rock_medium", ROCK_MEDIUM_PNG, ROCK_MEDIUM_JSON);
        let small = baked("rock_small", ROCK_SMALL_PNG, ROCK_SMALL_JSON);
        let ship = baked("ship", SHIP_PNG, SHIP_JSON);
        let bullet = baked("bullet", BULLET_PNG, BULLET_JSON);

        let large_sheet = register(device, sprites, "rock_large", &large)?;
        let medium_sheet = register(device, sprites, "rock_medium", &medium)?;
        let small_sheet = register(device, sprites, "rock_small", &small)?;
        let ship_sheet = register(device, sprites, "ship", &ship)?;
        let bullet_sheet = register(device, sprites, "bullet", &bullet)?;

        // Back to front, and this is the only place the depth order is written
        // down: `LayerStack` has no depth field to disagree with it. Both take
        // the world's rate — this camera never moves, so a parallax factor would
        // be an offset of zero however it was set.
        let mut stack = LayerStack::new();
        let field = stack.push_layer(Parallax::WORLD);
        let play = stack.push_layer(Parallax::WORLD);

        Ok(Self {
            stack,
            rocks: [
                still(large_sheet, &large.sheet),
                still(medium_sheet, &medium.sheet),
                still(small_sheet, &small.sheet),
            ],
            ship: still(ship_sheet, &ship.sheet),
            bullet: still(bullet_sheet, &bullet.sheet),
            field,
            play,
        })
    }

    /// This frame's sprites, back to front.
    ///
    /// Everything is in **sprite units** — world units times
    /// [`TEXELS_PER_UNIT`] — and the same frame's view-projection must have been
    /// built at the same scale. [`crate::gpu`] applies it in one place.
    ///
    /// `alpha` is how far this frame sits between the last tick and the next,
    /// from `FrameClock::alpha`. It moves the rotations and nothing else; see
    /// this module's header.
    ///
    /// **The rocks are emitted largest first**, which is a batching decision and
    /// not an artistic one: [`SpriteRenderer`] starts a new batch — a bind and a
    /// draw — whenever consecutive sprites name different sheets, and a field
    /// walked in spawn order alternates between three rock sheets at random. In
    /// size order it is at most five batches whatever is on the field. It also
    /// happens to put the big rocks behind the small ones, which is the right way
    /// round for a chip that just came off one.
    pub fn build(&mut self, render: &RenderState, alpha: f32) -> &[Sprite] {
        self.stack.clear();

        let alpha = f64::from(alpha);
        for size in [RockSize::Large, RockSize::Medium, RockSize::Small] {
            let art = self.rocks[rock_index(size)];
            self.stack.extend(
                self.field,
                render
                    .rocks
                    .iter()
                    .filter(move |rock| rock.size == size)
                    .map(move |rock| Sprite {
                        sheet: art.sheet,
                        rect: rock_rect(rock),
                        rotation: lerp_angle(rock.prev_angle, rock.angle, alpha) as f32,
                        uv: art.uv,
                        tint: UNTINTED,
                    }),
            );
        }

        let bullet = self.bullet;
        self.stack.extend(
            self.play,
            render.bullets.iter().map(move |shot| Sprite {
                sheet: bullet.sheet,
                rect: bullet_rect(shot),
                // A shot is round and has no attitude of its own; turning it
                // would be a rotation nobody could see.
                rotation: 0.0,
                uv: bullet.uv,
                tint: UNTINTED,
            }),
        );

        // Last, so a shot leaving the muzzle is behind the hull rather than
        // pasted over it — and not at all while the ship is waiting to respawn.
        if render.ship_alive {
            self.stack.push(
                self.play,
                Sprite {
                    sheet: self.ship.sheet,
                    rect: ship_rect(render.ship),
                    // Straight through, with no offset: `assets/ship.crpix` draws
                    // the nose up the frame and `game::heading_vector` puts a
                    // heading of zero along +Y, so the sprite's angle *is* the
                    // ship's heading.
                    rotation: lerp_angle(render.ship_heading_prev, render.ship_heading, alpha)
                        as f32,
                    uv: self.ship.uv,
                    tint: UNTINTED,
                },
            );
        }

        // The camera is at the origin and both layers are `Parallax::WORLD`, so
        // this offset is zero twice over. It is passed rather than assumed
        // because `resolve` is what turns the stack into a frame.
        self.stack.resolve([0.0, 0.0])
    }
}

// ---------------------------------------------------------------------------
// Pure geometry — no device, no sheet ids
// ---------------------------------------------------------------------------

/// Which of [`Scene::rocks`] a size draws from.
const fn rock_index(size: RockSize) -> usize {
    match size {
        RockSize::Large => 0,
        RockSize::Medium => 1,
        RockSize::Small => 2,
    }
}

/// One rock's rectangle, in sprite units: **its collider's bounding square, to
/// the texel**, which is what the frame sizes were chosen to be.
fn rock_rect(rock: &RockView) -> [f32; 4] {
    let radius = rock.size.radius();
    rect(rock.position, radius, radius)
}

/// A shot's rectangle. See [`BULLET_HALF_EXTENT`] for why it is not the sweep's.
fn bullet_rect(shot: &BulletView) -> [f32; 4] {
    rect(shot.position, BULLET_HALF_EXTENT, BULLET_HALF_EXTENT)
}

/// The ship's. See [`SHIP_HALF_EXTENT`].
fn ship_rect(centre: DVec3) -> [f32; 4] {
    rect(centre, SHIP_HALF_EXTENT, SHIP_HALF_EXTENT)
}

/// A world-space centre and half-extents as a sprite rectangle: `[x, y, w, h]`,
/// **minimum corner first**, which is what [`Sprite::rect`] takes.
///
/// The rectangle is the sprite **before** rotation — [`Sprite::rotation`] turns
/// it about its own centre — so a square rect is what makes a turned sprite stay
/// the size it was.
fn rect(centre: DVec3, half_w: f64, half_h: f64) -> [f32; 4] {
    let scale = f64::from(TEXELS_PER_UNIT);
    [
        ((centre.x - half_w) * scale) as f32,
        ((centre.y - half_h) * scale) as f32,
        (2.0 * half_w * scale) as f32,
        (2.0 * half_h * scale) as f32,
    ]
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

    use crate::game::{BULLET_RADIUS, SHIP_RADIUS};

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

    /// The three rock sheets, decoded, largest first.
    fn rock_sheets() -> [Loaded; 3] {
        [
            baked("rock_large", ROCK_LARGE_PNG, ROCK_LARGE_JSON),
            baked("rock_medium", ROCK_MEDIUM_PNG, ROCK_MEDIUM_JSON),
            baked("rock_small", ROCK_SMALL_PNG, ROCK_SMALL_JSON),
        ]
    }

    /// A rock, at rest, for a `Scene::build`.
    fn rock(size: RockSize, position: DVec3) -> RockView {
        RockView {
            position,
            size,
            angle: 0.0,
            prev_angle: 0.0,
        }
    }

    /// How far the silhouette reaches from the frame's centre along each of
    /// eight compass rays, as a fraction of the frame's half-width.
    ///
    /// The measurement the "three sizes" claim actually rests on: it is scale
    /// free, so three frames that were one picture at three magnifications
    /// produce the *same* eight numbers and can be told from three drawings.
    ///
    /// **Marched along the ray rather than taken as the widest texel in a
    /// 45° wedge**, which is the version this started as and which was too blunt
    /// to be a check: a wedge always contains a bump, so every rock came back
    /// within a few per cent of a circle and the "not a circle" assertion below
    /// could only have failed on art nobody would have committed. Walking the
    /// ray reads the outline where the ray crosses it, dents included.
    fn silhouette(loaded: &Loaded) -> [f64; 8] {
        let (w, h) = (loaded.image.width as i64, loaded.image.height as i64);
        let half = w as f64 / 2.0;
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        std::array::from_fn(|dir| {
            let theta = dir as f64 * std::f64::consts::TAU / 8.0;
            // Image rows run down and the world's +Y runs up, which does not
            // matter to the *set* of eight numbers but does decide which is
            // which — kept honest so a profile can be read against the art.
            let (dx, dy) = (theta.cos(), -theta.sin());
            let mut reach = 0.0_f64;
            let mut t = 0.0_f64;
            // The **last** opaque texel on the ray, not the first transparent
            // one: a crater is a hole in the middle of a rock, and stopping at
            // the first gap would measure the crater instead of the outline.
            while t <= half * 1.5 {
                let (x, y) = ((cx + t * dx) as i64, (cy + t * dy) as i64);
                if (0..w).contains(&x)
                    && (0..h).contains(&y)
                    && loaded.image.pixels[((y * w + x) * 4 + 3) as usize] != 0
                {
                    reach = t;
                }
                t += 0.25;
            }
            reach / half
        })
    }

    // -----------------------------------------------------------------------
    // The art itself
    // -----------------------------------------------------------------------

    /// **The art is the art that was authored**, at the sizes it was authored
    /// at — and those sizes are the colliders `game.rs` declares rather than
    /// numbers repeated here.
    ///
    /// A test that only checked `load` returned `Ok` would pass on a blank image
    /// with no frames in it, which is exactly the failure this is for; the alpha
    /// counts at the end are what rule that out.
    #[test]
    fn the_art_bakes_to_the_sheets_it_declares() {
        let scale = f64::from(TEXELS_PER_UNIT);

        for (name, loaded, size) in [
            ("rock_large", &rock_sheets()[0], RockSize::Large),
            ("rock_medium", &rock_sheets()[1], RockSize::Medium),
            ("rock_small", &rock_sheets()[2], RockSize::Small),
        ] {
            let expected = (2.0 * size.radius() * scale).round() as u32;
            assert_eq!(
                (loaded.image.width, loaded.image.height),
                (expected, expected),
                "{name} is not its collider's bounding square at {scale} texels a unit",
            );
            assert!(
                (f64::from(expected) - 2.0 * size.radius() * scale).abs() < 1e-9,
                "{name}: {} units does not land on a whole texel",
                2.0 * size.radius(),
            );
            assert_eq!(
                loaded.sheet.frames.len(),
                1,
                "{name} has more than one frame"
            );
            assert_eq!(
                loaded.sheet.frames[0].hold, 1,
                "{name}: the default hold in ticks did not survive the millisecond \
                 round trip — build.rs and ART_TICK_HZ disagree",
            );
            assert!(
                loaded.sheet.clips.is_empty(),
                "{name} declares a clip; a rock turns, it does not animate",
            );
            assert_eq!(
                loaded.sheet.nine, None,
                "{name} declares a nine-slice; nothing in this game is stretched",
            );
        }

        let ship = baked("ship", SHIP_PNG, SHIP_JSON);
        assert_eq!((ship.image.width, ship.image.height), (16, 16));
        assert_eq!(
            f64::from(ship.image.width) / scale,
            2.0 * SHIP_HALF_EXTENT,
            "the hull's frame and the rectangle it is drawn at disagree",
        );
        const {
            assert!(
                SHIP_HALF_EXTENT > SHIP_RADIUS,
                "the hull must be drawn larger than the sphere that kills it",
            );
        }

        let bullet = baked("bullet", BULLET_PNG, BULLET_JSON);
        assert_eq!((bullet.image.width, bullet.image.height), (4, 4));
        assert_eq!(
            f64::from(bullet.image.width) / scale,
            2.0 * BULLET_HALF_EXTENT,
        );
        const {
            assert!(
                BULLET_HALF_EXTENT > BULLET_RADIUS,
                "a shot must be drawn larger than the sphere it sweeps",
            );
        }
        assert!(
            BULLET_JSON.is_none(),
            "one still frame with no clip and no nine-slice needs no sidecar",
        );

        // Anti-blank: every sheet has transparent corners *and* opaque pixels. A
        // sheet of zeroes satisfies every assertion above.
        for (name, loaded) in [
            ("rock_large", &rock_sheets()[0]),
            ("rock_medium", &rock_sheets()[1]),
            ("rock_small", &rock_sheets()[2]),
            ("ship", &ship),
            ("bullet", &bullet),
        ] {
            let clear = loaded
                .image
                .pixels
                .chunks_exact(4)
                .filter(|p| p[3] == 0)
                .count();
            let opaque = loaded.image.pixels.len() / 4 - clear;
            assert!(
                clear > 0 && opaque > 0,
                "{name}: {clear} clear, {opaque} opaque",
            );
        }
    }

    /// **The three rock sizes are three different pictures.**
    ///
    /// Three frames of one drawing at three magnifications would parse, bake,
    /// load, index and draw exactly as three drawings do, and every other test
    /// in this file passes on them — so "three sizes" is worth nothing until the
    /// pictures themselves are known to differ *in shape* and not only in
    /// pixel count. Hence [`silhouette`], which is scale free.
    ///
    /// It also asserts each one is lumpy rather than round, because three
    /// different circles would pass a pairwise-difference test on their raw
    /// pixels and still be the thing this is guarding against.
    #[test]
    fn the_three_rock_sizes_are_three_different_pictures() {
        let sheets = rock_sheets();
        let profiles: [[f64; 8]; 3] = std::array::from_fn(|i| silhouette(&sheets[i]));

        for (name, profile) in ["large", "medium", "small"].iter().zip(&profiles) {
            let max = profile.iter().copied().fold(f64::MIN, f64::max);
            let min = profile.iter().copied().fold(f64::MAX, f64::min);
            assert!(min > 0.0, "{name} has nothing drawn in one direction");
            assert!(
                max / min > 1.15,
                "{name} is a circle, not a rock: the outline runs from {min:.3} \
                 to {max:.3} of the half-width",
            );
        }

        for a in 0..3 {
            for b in (a + 1)..3 {
                let apart: f64 = profiles[a]
                    .iter()
                    .zip(&profiles[b])
                    .map(|(x, y)| (x - y).abs())
                    .fold(f64::MIN, f64::max);
                assert!(
                    apart > 0.08,
                    "rock sizes {a} and {b} are the same silhouette scaled: the \
                     outlines never differ by more than {apart:.3} of the \
                     half-width\n{:?}\n{:?}",
                    profiles[a],
                    profiles[b],
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The scene
    // -----------------------------------------------------------------------

    /// **The rectangle drawn covers the collider it stands for**, for all four
    /// kinds of thing on the field.
    ///
    /// Asserted against the radii `game.rs` declares rather than against numbers
    /// repeated here, which is what makes it catch art drifting from physics. A
    /// rock is checked as an *equality* — its frame is its collider's bounding
    /// square by construction — and the ship and the shot as a strict cover,
    /// because both are deliberately drawn larger than what they collide with.
    #[test]
    fn every_sprite_covers_the_collider_it_stands_for() {
        with_scene(|scene| {
            let render = RenderState {
                ship: DVec3::new(-2.5, 1.25, 0.0),
                ship_alive: true,
                rocks: vec![
                    rock(RockSize::Large, DVec3::new(4.0, -3.0, 0.0)),
                    rock(RockSize::Medium, DVec3::new(-7.5, 2.0, 0.0)),
                    rock(RockSize::Small, DVec3::new(0.25, 9.0, 0.0)),
                ],
                bullets: vec![BulletView {
                    position: DVec3::new(11.0, -6.5, 0.0),
                }],
                ..RenderState::default()
            };
            scene.build(&render, 0.0);

            let scale = f64::from(TEXELS_PER_UNIT);
            // `[x, y, w, h]` in sprite units against a world centre and radius.
            let covers = |rect: [f32; 4], centre: DVec3, radius: f64, what: &str| {
                let (min_x, min_y) = (f64::from(rect[0]), f64::from(rect[1]));
                let (max_x, max_y) = (min_x + f64::from(rect[2]), min_y + f64::from(rect[3]));
                for (axis, (lo, hi, c)) in [(min_x, max_x, centre.x), (min_y, max_y, centre.y)]
                    .into_iter()
                    .enumerate()
                {
                    assert!(
                        lo <= (c - radius) * scale + 1e-6 && hi >= (c + radius) * scale - 1e-6,
                        "{what}: axis {axis} is drawn {lo}..{hi}, the collider needs \
                         {}..{}",
                        (c - radius) * scale,
                        (c + radius) * scale,
                    );
                }
            };

            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 3, "one sprite per rock");
            for (index, view) in render.rocks.iter().enumerate() {
                let radius = view.size.radius();
                covers(field[index].rect, view.position, radius, "a rock");
                // And exactly, not merely enough: a rock drawn wider than its
                // sphere is a rock the ship flies through the edge of.
                assert!(
                    (f64::from(field[index].rect[2]) - 2.0 * radius * scale).abs() < 1e-3,
                    "{:?} is not its collider's bounding square",
                    view.size,
                );
            }

            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(play.len(), 2, "one shot and the ship");
            covers(
                play[0].rect,
                render.bullets[0].position,
                BULLET_RADIUS,
                "a shot",
            );
            covers(play[1].rect, render.ship, SHIP_RADIUS, "the ship");

            // Anti-vacuity: `covers` can fail. A sprite at its neighbour's
            // position is the right size in the wrong place, and it says so.
            let elsewhere = rock(RockSize::Large, DVec3::new(9.0, -3.0, 0.0));
            let moved = rock_rect(&elsewhere);
            assert!(
                (f64::from(moved[0]) - f64::from(field[0].rect[0])).abs() > 1.0,
                "two different positions produced the same rectangle",
            );
        });
    }

    /// Each kind goes on the layer it belongs to, and the rocks come out grouped
    /// by sheet so the frame is a handful of batches rather than one per rock.
    #[test]
    fn the_rocks_are_batched_by_size_and_the_ship_is_drawn_last() {
        with_scene(|scene| {
            assert_eq!(scene.stack.layer_count(), 2);
            assert_eq!(scene.field.depth(), 0, "the rocks are behind the game");
            assert_eq!(scene.play.depth(), 1);

            // Interleaved on the way in, as a split leaves them.
            let render = RenderState {
                ship_alive: true,
                rocks: vec![
                    rock(RockSize::Small, DVec3::new(1.0, 0.0, 0.0)),
                    rock(RockSize::Large, DVec3::new(2.0, 0.0, 0.0)),
                    rock(RockSize::Small, DVec3::new(3.0, 0.0, 0.0)),
                    rock(RockSize::Medium, DVec3::new(4.0, 0.0, 0.0)),
                    rock(RockSize::Large, DVec3::new(5.0, 0.0, 0.0)),
                ],
                bullets: vec![BulletView {
                    position: DVec3::ZERO,
                }],
                ..RenderState::default()
            };
            scene.build(&render, 0.0);

            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 5);
            let runs = field
                .windows(2)
                .filter(|w| w[0].sheet != w[1].sheet)
                .count();
            assert_eq!(
                runs, 2,
                "five rocks in three sizes must be three runs, not five",
            );
            // And the three sheets really are three, or the count above is
            // satisfied by everything sharing one.
            let mut sheets: Vec<_> = field.iter().map(|s| s.sheet).collect();
            sheets.dedup();
            assert_eq!(sheets.len(), 3);

            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(play.len(), 2);
            assert_eq!(play[0].sheet, scene.bullet.sheet);
            assert_eq!(
                play[1].sheet, scene.ship.sheet,
                "the ship must be pushed last, or a shot leaving the muzzle is \
                 pasted over the hull",
            );

            // The resolved frame is the two layers concatenated, back first.
            let flat = scene.stack.resolved();
            assert_eq!(flat.len(), field.len() + play.len());
            assert_eq!(flat[0].sheet, scene.rocks[0].sheet);
            assert_eq!(flat.last().expect("non-empty").sheet, scene.ship.sheet);
        });
    }

    /// A destroyed ship is not drawn, and its shots still are.
    #[test]
    fn a_ship_waiting_to_respawn_is_not_on_screen() {
        with_scene(|scene| {
            let render = RenderState {
                ship_alive: false,
                bullets: vec![BulletView {
                    position: DVec3::ZERO,
                }],
                ..RenderState::default()
            };
            scene.build(&render, 0.0);
            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(play.len(), 1, "the wreck is still being drawn");
            assert_eq!(play[0].sheet, scene.bullet.sheet);
        });
    }

    /// **The ship's drawn rotation follows its heading**, with no offset and no
    /// sign flip, and it is the *interpolated* heading rather than the tick's.
    #[test]
    fn the_ship_is_drawn_turned_to_the_heading_it_is_flying() {
        with_scene(|scene| {
            let mut heading = |prev: f64, now: f64, alpha: f32| -> f64 {
                let render = RenderState {
                    ship_alive: true,
                    ship_heading: now,
                    ship_heading_prev: prev,
                    ..RenderState::default()
                };
                scene.build(&render, alpha);
                let play = scene.stack.sprites(scene.play).to_vec();
                f64::from(play[0].rotation)
            };

            // A ship that is not turning draws at its heading exactly, whatever
            // the alpha is — this is the assertion that catches a sign flip or a
            // quarter-turn offset between `heading_vector` and the sheet.
            for angle in [0.0, 0.5, 2.0, 4.0, 6.0] {
                let drawn = heading(angle, angle, 0.5);
                assert!(
                    (crate::game::wrap_to_pi(drawn - angle)).abs() < 1e-5,
                    "a ship holding {angle} rad was drawn at {drawn}",
                );
            }

            // And a turning ship is drawn between the two ticks, not at either.
            let mid = heading(1.0, 1.4, 0.5);
            assert!((mid - 1.2).abs() < 1e-5, "{mid}");
        });
    }

    /// **A rock's tumble is interpolated too**, and it is each rock's own.
    #[test]
    fn every_rock_is_drawn_at_its_own_interpolated_tumble() {
        with_scene(|scene| {
            let render = RenderState {
                rocks: vec![
                    RockView {
                        position: DVec3::ZERO,
                        size: RockSize::Large,
                        prev_angle: 0.2,
                        angle: 0.4,
                    },
                    RockView {
                        position: DVec3::new(5.0, 0.0, 0.0),
                        size: RockSize::Large,
                        prev_angle: 2.0,
                        angle: 1.6,
                    },
                ],
                ..RenderState::default()
            };
            scene.build(&render, 0.25);
            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 2);
            assert!((f64::from(field[0].rotation) - 0.25).abs() < 1e-5);
            assert!(
                (f64::from(field[1].rotation) - 1.9).abs() < 1e-5,
                "a rock turning the other way drew {}",
                field[1].rotation,
            );
        });
    }
}
