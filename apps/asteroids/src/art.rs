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
//! # Every angle and every position is interpolated — except across a teleport
//!
//! [`Scene::build`] takes an `alpha` and [`lerp_angle`]s each rotation from the
//! previous tick's value to this one, and lerps each position the same way.
//! A body the wrap moved that tick is drawn snapped to its current position
//! instead: interpolating would fly it back across the whole field.
//! [`RenderState`]'s own docs carry why the wrap needs a teleport flag rather
//! than a lerp, and `game::lerp_angle`'s carry why the short way round is the
//! only correct answer for an angle.
//!
//! # A body straddling a seam is drawn at both sides of it
//!
//! The field wraps, so while a rock crosses an edge half of it is past the
//! seam — and that half belongs at the opposite edge. [`wrapped_offsets`] is
//! the rule, applied to every rock: its own position plus a ghost per wrapped
//! offset, so a crossing reads as a crossing rather than as a rock losing a
//! chunk. The ship and the shots straddle the same seams (their crossings are
//! shorter, so the missing half is briefer and less conspicuous) and are left
//! single; the rule exists where the entry in `docs/backlog.md` put it.

use crcbl::hal::{Device, HalError};
use crcbl::math::DVec3;
use crcbl::render::{Layer, LayerStack, Parallax, SheetId, Sprite, SpriteRenderer};
use crcbl::sprite::Sheet;
use crcbl::sprite::load::{Loaded, load_baked};

use crate::game::{
    FLASH_LIFE, RenderState, RockSize, RockView, WORLD_HALF_HEIGHT, WORLD_HALF_WIDTH, lerp_angle,
};

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

/// What is outside the game, as the clear value for the swapchain.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; this is `#05050d` put through the sRGB→linear transfer
/// function once, here, rather than looking washed out on screen.
///
/// Near-black on purpose, and it is the whole background: this game has no
/// scenery, and space that read as a colour would compete with the rocks.
pub const SPACE: [f32; 4] = [0.00152, 0.00152, 0.00304, 1.0];

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

/// The ship's two frames, resolved to the UV rectangles they will ever use.
///
/// `hull` is the ship under no power; `flame` is the same hull with a plume
/// below the nozzle, drawn while the game reports thrust. [`Scene::build`]
/// picks between them, so the ship is not "one frame whatever it is doing".
#[derive(Clone, Copy, Debug)]
struct ShipArt {
    sheet: SheetId,
    hull: [f32; 4],
    flame: [f32; 4],
}

/// Resolve the ship's two frames by index, matching `assets/ship.crpix`'s
/// declared order — `hull` first, `flame` second.
fn ship_art(sheet: SheetId, description: &Sheet) -> ShipArt {
    ShipArt {
        sheet,
        hull: description.uv(0).expect("the ship sheet has a hull frame"),
        flame: description.uv(1).expect("the ship sheet has a flame frame"),
    }
}

/// The hit flash's two frames, resolved to the UV rectangles they will ever
/// use.
///
/// `first` is the white-hot burst, `second` the wider, dimmer fade.
/// [`Scene::build`] swaps between them halfway through the flash's life — the
/// same state-picked swap the ship's flame uses, driven by the game's age
/// counter rather than by a clip.
#[derive(Clone, Copy, Debug)]
struct FlashArt {
    sheet: SheetId,
    first: [f32; 4],
    second: [f32; 4],
}

/// Resolve the flash's two frames by index, matching `assets/flash.crpix`'s
/// declared order — `burst` first, `fade` second.
fn flash_art(sheet: SheetId, description: &Sheet) -> FlashArt {
    FlashArt {
        sheet,
        first: description
            .uv(0)
            .expect("the flash sheet has a burst frame"),
        second: description.uv(1).expect("the flash sheet has a fade frame"),
    }
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
    ship: ShipArt,
    bullet: StillArt,
    flash: FlashArt,
    /// The rocks, behind the game.
    field: Layer,
    /// The ship, its shots and the hit flashes, in front of them.
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
        let flash = baked("flash", FLASH_PNG, FLASH_JSON);

        let large_sheet = sprites.register_baked(device, "rock_large", &large)?;
        let medium_sheet = sprites.register_baked(device, "rock_medium", &medium)?;
        let small_sheet = sprites.register_baked(device, "rock_small", &small)?;
        let ship_sheet = sprites.register_baked(device, "ship", &ship)?;
        let bullet_sheet = sprites.register_baked(device, "bullet", &bullet)?;
        let flash_sheet = sprites.register_baked(device, "flash", &flash)?;

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
            ship: ship_art(ship_sheet, &ship.sheet),
            bullet: still(bullet_sheet, &bullet.sheet),
            flash: flash_art(flash_sheet, &flash.sheet),
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
    /// from `FrameClock::alpha`. It moves the rotations and the positions,
    /// except on a tick a body teleported, when it snaps; see this module's
    /// header.
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
                    .flat_map(move |rock| {
                        let centre =
                            drawn_centre(rock.prev_position, rock.position, rock.teleported, alpha);
                        let sprite = move |at: DVec3| {
                            Sprite::new(art.sheet, rock_rect(at, rock), art.uv).with_rotation(
                                lerp_angle(rock.prev_angle, rock.angle, alpha) as f32,
                            )
                        };
                        // The rock, plus a ghost at every wrapped offset for a
                        // seam it straddles — see [`wrapped_offsets`]. Without
                        // the ghost, the half past the edge is missing for the
                        // whole of a crossing.
                        wrapped_offsets(centre, rock.size.radius()).map(sprite)
                    }),
            );
        }

        let bullet = self.bullet;
        self.stack.extend(
            self.play,
            // Unrotated: a shot is round and has no attitude of its own, so
            // turning it would be a rotation nobody could see.
            render.bullets.iter().map(move |shot| {
                Sprite::new(
                    bullet.sheet,
                    bullet_rect(drawn_centre(
                        shot.prev_position,
                        shot.position,
                        shot.teleported,
                        alpha,
                    )),
                    bullet.uv,
                )
            }),
        );

        let flash = self.flash;
        self.stack.extend(
            self.play,
            // Unrotated: a burst has no attitude, so turning it would be a
            // rotation nobody sees.
            render.flashes.iter().map(move |flash_at| {
                Sprite::new(
                    flash.sheet,
                    rect(
                        flash_at.position,
                        flash_at.size.radius(),
                        flash_at.size.radius(),
                    ),
                    // Frame 1 for the first half of the flash's life, frame 2
                    // for the second — the same state-picked frame swap the
                    // ship's flame uses, driven by the game's age rather than
                    // by a clip.
                    if flash_at.life > FLASH_LIFE * 0.5 {
                        flash.first
                    } else {
                        flash.second
                    },
                )
            }),
        );

        // Last, so a shot leaving the muzzle is behind the hull rather than
        // pasted over it — and not at all while the ship is waiting to respawn.
        if render.ship_alive {
            self.stack.push(
                self.play,
                Sprite::new(
                    self.ship.sheet,
                    ship_rect(drawn_centre(
                        render.ship_prev_pos,
                        render.ship,
                        render.ship_teleported,
                        alpha,
                    )),
                    if render.thrusting {
                        self.ship.flame
                    } else {
                        self.ship.hull
                    },
                )
                // Straight through, with no offset: `assets/ship.crpix` draws
                // the nose up the frame and `game::heading_vector` puts a
                // heading of zero along +Y, so the sprite's angle *is* the
                // ship's heading.
                .with_rotation(lerp_angle(
                    render.ship_heading_prev,
                    render.ship_heading,
                    alpha,
                ) as f32),
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
fn rock_rect(centre: DVec3, rock: &RockView) -> [f32; 4] {
    let radius = rock.size.radius();
    rect(centre, radius, radius)
}

/// Every position a body straddling a seam must be drawn at: its own, plus a
/// ghost per wrapped offset.
///
/// The field wraps, so a body past one edge has its far half on the other
/// side — one ghost. A body crossing a *corner* has three: the axis copies and
/// the diagonal one, which is where the corner piece lands. A body past no
/// edge has just its own position. The offsets are `±2 × half`, exactly what
/// [`wrap_axis`](crate::game::wrap_axis) does to a coordinate that left the
/// field; the ghost is the same sprite at the wrapped centre, and the part of
/// it that lies beyond the opposite edge is clipped by the viewport like any
/// other off-view sprite.
///
/// `pub(crate)` for the same reason as [`drawn_centre`]: the app loop's tests
/// count copies through the rule rather than beside it.
pub(crate) fn wrapped_offsets(centre: DVec3, radius: f64) -> impl Iterator<Item = DVec3> {
    let x_wrap = if centre.x + radius > WORLD_HALF_WIDTH {
        Some(-2.0 * WORLD_HALF_WIDTH)
    } else if centre.x - radius < -WORLD_HALF_WIDTH {
        Some(2.0 * WORLD_HALF_WIDTH)
    } else {
        None
    };
    let y_wrap = if centre.y + radius > WORLD_HALF_HEIGHT {
        Some(-2.0 * WORLD_HALF_HEIGHT)
    } else if centre.y - radius < -WORLD_HALF_HEIGHT {
        Some(2.0 * WORLD_HALF_HEIGHT)
    } else {
        None
    };
    // Main first, then the axis ghosts, then the diagonal.
    std::iter::once(0.0).chain(x_wrap).flat_map(move |dx| {
        std::iter::once(0.0)
            .chain(y_wrap)
            .map(move |dy| centre + DVec3::new(dx, dy, 0.0))
    })
}

/// A shot's rectangle. See [`BULLET_HALF_EXTENT`] for why it is not the sweep's.
fn bullet_rect(centre: DVec3) -> [f32; 4] {
    rect(centre, BULLET_HALF_EXTENT, BULLET_HALF_EXTENT)
}

/// The ship's. See [`SHIP_HALF_EXTENT`].
fn ship_rect(centre: DVec3) -> [f32; 4] {
    rect(centre, SHIP_HALF_EXTENT, SHIP_HALF_EXTENT)
}

/// Where a body is drawn on this frame: lerped between the previous and the
/// current tick's positions, or snapped to the current one on a tick it
/// teleported — the previous position is a whole field away then.
///
/// `pub(crate)` because the app loop's tests count the copies the scene will
/// draw, and the count has to come from the same rule the drawing uses.
pub(crate) fn drawn_centre(prev: DVec3, cur: DVec3, teleported: bool, alpha: f64) -> DVec3 {
    if teleported {
        cur
    } else {
        prev.lerp(cur, alpha)
    }
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

/// Decodes one baked sheet at *this crate's* bake rate.
///
/// [`ART_TICK_HZ`] is generated into each crate that bakes art, so the rate is
/// per-crate configuration; the failure policy is the shared half and lives in
/// [`load_baked`].
fn baked(name: &str, png: &[u8], json: Option<&str>) -> Loaded {
    load_baked(name, png, json, ART_TICK_HZ)
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

    use crate::game::{BULLET_RADIUS, BulletView, Flash, SHIP_RADIUS};

    /// Runs `body` against a scene built on the null backend.
    ///
    /// A real [`SpriteRenderer`] rather than a stub, because [`SheetId`] is
    /// opaque outside `crcbl-render` and there is no other way to have one —
    /// which is the point of it being opaque.
    fn with_scene(body: impl FnOnce(&mut Scene)) {
        let instance = NullInstance::gpu_driven();
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
            prev_position: position,
            teleported: false,
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
                 round trip, so bake and load disagree about milliseconds",
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
        // Two frames laid out as a horizontal strip: 32 x 16, both 16 x 16.
        assert_eq!((ship.image.width, ship.image.height), (32, 16));
        assert_eq!(ship.sheet.frames.len(), 2, "the ship sheet has two frames");
        let hull = ship.sheet.uv(0).expect("the ship sheet has a hull frame");
        let hull_texels = f64::from(ship.image.width) * f64::from(hull[2] - hull[0]);
        assert_eq!(
            hull_texels / scale,
            2.0 * SHIP_HALF_EXTENT,
            "the hull's frame and the rectangle it is drawn at disagree",
        );
        const {
            assert!(
                SHIP_HALF_EXTENT > SHIP_RADIUS,
                "the hull must be drawn larger than the sphere that kills it",
            );
        }
        // The flame frame is not a copy of the hull: the baked strip's two
        // halves differ. A sheet where they matched would pass every structural
        // check and defeat the field's whole point.
        let frame_bytes = 16 * 16 * 4;
        let (hull_pixels, flame_pixels) = ship.image.pixels.split_at(frame_bytes);
        assert_ne!(
            hull_pixels, flame_pixels,
            "the flame frame is a copy of the hull"
        );

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

        let flash = baked("flash", FLASH_PNG, FLASH_JSON);
        // Two frames laid out as a horizontal strip: 26 x 13, both 13 x 13.
        assert_eq!((flash.image.width, flash.image.height), (26, 13));
        assert_eq!(
            flash.sheet.frames.len(),
            2,
            "the flash sheet has two frames"
        );
        assert!(
            flash.sheet.clips.is_empty(),
            "the flash declares a clip; its frames are swapped by the game's \
             age counter, not by one",
        );
        // The fade frame is not a copy of the burst: the baked strip's two
        // halves differ. A sheet where they matched would pass every structural
        // check and defeat the field's whole point.
        let frame_bytes = 13 * 13 * 4;
        let (burst_pixels, fade_pixels) = flash.image.pixels.split_at(frame_bytes);
        assert_ne!(
            burst_pixels, fade_pixels,
            "the fade frame is a copy of the burst"
        );

        // Anti-blank: every sheet has transparent corners *and* opaque pixels. A
        // sheet of zeroes satisfies every assertion above.
        for (name, loaded) in [
            ("rock_large", &rock_sheets()[0]),
            ("rock_medium", &rock_sheets()[1]),
            ("rock_small", &rock_sheets()[2]),
            ("ship", &ship),
            ("bullet", &bullet),
            ("flash", &flash),
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
                ship_prev_pos: DVec3::new(-2.5, 1.25, 0.0),
                ship_alive: true,
                rocks: vec![
                    rock(RockSize::Large, DVec3::new(4.0, -3.0, 0.0)),
                    rock(RockSize::Medium, DVec3::new(-7.5, 2.0, 0.0)),
                    rock(RockSize::Small, DVec3::new(0.25, 9.0, 0.0)),
                ],
                bullets: vec![BulletView {
                    position: DVec3::new(11.0, -6.5, 0.0),
                    prev_position: DVec3::new(11.0, -6.5, 0.0),
                    teleported: false,
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
            let moved = rock_rect(elsewhere.position, &elsewhere);
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
                    prev_position: DVec3::ZERO,
                    teleported: false,
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
                    prev_position: DVec3::ZERO,
                    teleported: false,
                }],
                ..RenderState::default()
            };
            scene.build(&render, 0.0);
            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(play.len(), 1, "the wreck is still being drawn");
            assert_eq!(play[0].sheet, scene.bullet.sheet);
        });
    }

    /// **The ship draws its flame frame while thrusting and the plain hull
    /// otherwise**, and the two frames are different pictures — a sheet where
    /// the flame frame was a copy of the hull would pass every structure check
    /// and defeat the whole point of the field.
    #[test]
    fn the_ship_draws_its_flame_frame_only_while_thrusting() {
        with_scene(|scene| {
            let render = |thrusting| RenderState {
                ship_alive: true,
                thrusting,
                ..RenderState::default()
            };

            let hull = scene.build(&render(false), 0.0).to_vec();
            assert_eq!(
                hull.last().expect("the ship is on the play layer").uv,
                scene.ship.hull,
                "a coasting ship drew the flame frame",
            );

            let flame = scene.build(&render(true), 0.0).to_vec();
            assert_eq!(
                flame.last().expect("the ship is on the play layer").uv,
                scene.ship.flame,
                "a thrusting ship drew the hull frame",
            );
            assert_ne!(scene.ship.hull, scene.ship.flame);
        });
    }

    /// **A hit flash is drawn on the play layer, over the rocks, covering the
    /// dead rock** — and its frame follows its age: the burst for the first
    /// half of [`FLASH_LIFE`], the fade for the second.
    #[test]
    fn a_hit_flash_is_drawn_above_the_rocks_with_the_frame_its_age_picks() {
        with_scene(|scene| {
            let render = |life| RenderState {
                ship_alive: false,
                rocks: vec![rock(RockSize::Large, DVec3::new(1.0, 1.0, 0.0))],
                flashes: vec![Flash {
                    position: DVec3::new(1.0, 1.0, 0.0),
                    size: RockSize::Large,
                    life,
                }],
                ..RenderState::default()
            };

            // A fresh flash draws the burst frame, at the rock's position and
            // covering its collider's bounding square.
            scene.build(&render(FLASH_LIFE), 0.0);
            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(play.len(), 1, "only the flash is on the play layer");
            assert_eq!(play[0].sheet, scene.flash.sheet);
            assert_eq!(
                play[0].uv, scene.flash.first,
                "a fresh flash drew the fade frame",
            );
            let radius = RockSize::Large.radius();
            assert!(
                (f64::from(play[0].rect[2]) - 2.0 * radius * f64::from(TEXELS_PER_UNIT)).abs()
                    < 1e-3,
                "the flash does not cover the rock that died",
            );
            assert!(
                (rect_centre(play[0].rect) - DVec3::new(1.0, 1.0, 0.0)).length() < 1e-6,
                "the flash is not where the rock died",
            );

            // Halfway through its life it has swapped to the fade frame.
            scene.build(&render(FLASH_LIFE * 0.5), 0.0);
            let play = scene.stack.sprites(scene.play).to_vec();
            assert_eq!(
                play[0].uv, scene.flash.second,
                "an old flash drew the burst frame",
            );
            assert_ne!(scene.flash.first, scene.flash.second);

            // And it sits on the play layer: the rock is still on the field
            // layer beneath it, so the flash reads as the hit, not as a
            // sprite pasted on top of the rock it replaced.
            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 1, "the field layer lost the rock");
            assert_eq!(
                field[0].sheet, scene.rocks[0].sheet,
                "the field layer no longer holds the rock",
            );
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
                        prev_position: DVec3::ZERO,
                        teleported: false,
                        size: RockSize::Large,
                        prev_angle: 0.2,
                        angle: 0.4,
                    },
                    RockView {
                        position: DVec3::new(5.0, 0.0, 0.0),
                        prev_position: DVec3::new(5.0, 0.0, 0.0),
                        teleported: false,
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

    /// A rock past no seam is drawn exactly once — the ghost rule must not
    /// cost a sprite for a rock that has nothing across the edge.
    #[test]
    fn a_rock_away_from_the_edges_is_drawn_once() {
        with_scene(|scene| {
            let render = RenderState {
                rocks: vec![rock(RockSize::Large, DVec3::new(10.0, 5.0, 0.0))],
                ..RenderState::default()
            };
            scene.build(&render, 1.0);
            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 1, "a rock past no seam needs no ghost");
            assert_eq!(rect_centre(field[0].rect), DVec3::new(10.0, 5.0, 0.0));
        });
    }

    /// A rock straddling an edge is drawn at both sides of the seam: its own
    /// position, and the same sprite a full field to the wrapped side. That is
    /// the half that would otherwise be missing for the whole of a crossing.
    #[test]
    fn a_rock_straddling_an_edge_is_drawn_at_both_sides_of_the_seam() {
        with_scene(|scene| {
            // 15.5 + 1.7 > 16: the large rock's right half is past the seam.
            let render = RenderState {
                rocks: vec![rock(RockSize::Large, DVec3::new(15.5, 0.0, 0.0))],
                ..RenderState::default()
            };
            scene.build(&render, 1.0);
            let field = scene.stack.sprites(scene.field).to_vec();
            assert_eq!(field.len(), 2, "a straddling rock needs its ghost");
            let centres: Vec<DVec3> = field.iter().map(|s| rect_centre(s.rect)).collect();
            assert!(
                centres.contains(&DVec3::new(15.5, 0.0, 0.0)),
                "the rock itself is missing from {centres:?}",
            );
            assert!(
                centres.contains(&DVec3::new(-16.5, 0.0, 0.0)),
                "the wrapped copy is missing from {centres:?}",
            );
        });
    }

    /// A rock crossing a corner straddles both seams, and its corner piece
    /// lands at the *diagonal* wrapped offset — which neither axis ghost alone
    /// covers. All four copies have to be on the field.
    #[test]
    fn a_rock_crossing_a_corner_is_drawn_at_the_diagonal_too() {
        with_scene(|scene| {
            // 15.9 + 1.7 > 16 and 11.9 + 1.7 > 12: past both the right and the
            // top seams at once.
            let render = RenderState {
                rocks: vec![rock(RockSize::Large, DVec3::new(15.9, 11.9, 0.0))],
                ..RenderState::default()
            };
            scene.build(&render, 1.0);
            let field = scene.stack.sprites(scene.field).to_vec();
            let centres: Vec<DVec3> = field.iter().map(|s| rect_centre(s.rect)).collect();
            for expected in [
                DVec3::new(15.9, 11.9, 0.0),
                DVec3::new(-16.1, 11.9, 0.0),
                DVec3::new(15.9, -12.1, 0.0),
                DVec3::new(-16.1, -12.1, 0.0),
            ] {
                assert!(
                    centres.contains(&expected),
                    "missing the {expected:?} copy in {centres:?}",
                );
            }
        });
    }

    /// The centre of a sprite rect, back in world units. A rect is
    /// `[x, y, w, h]`, minimum corner first, in sprite units.
    fn rect_centre(rect: [f32; 4]) -> DVec3 {
        let scale = f64::from(TEXELS_PER_UNIT);
        DVec3::new(
            (f64::from(rect[0]) + f64::from(rect[2]) / 2.0) / scale,
            (f64::from(rect[1]) + f64::from(rect[3]) / 2.0) / scale,
            0.0,
        )
    }

    /// **A body is drawn between its previous tick's position and this one**,
    /// at exactly the frame clock's alpha — the position half of the rotation
    /// interpolation the tests above pin.
    #[test]
    fn a_body_is_drawn_between_its_previous_and_current_position() {
        with_scene(|scene| {
            let rock = RockView {
                position: DVec3::new(10.0, 0.0, 0.0),
                prev_position: DVec3::ZERO,
                teleported: false,
                size: RockSize::Large,
                angle: 0.0,
                prev_angle: 0.0,
            };
            for (alpha, expected) in [
                (0.0, DVec3::ZERO),
                (0.5, DVec3::new(5.0, 0.0, 0.0)),
                (1.0, DVec3::new(10.0, 0.0, 0.0)),
            ] {
                let render = RenderState {
                    ship: DVec3::new(10.0, 0.0, 0.0),
                    ship_prev_pos: DVec3::ZERO,
                    ship_alive: true,
                    rocks: vec![rock],
                    ..RenderState::default()
                };
                scene.build(&render, alpha);

                let field = scene.stack.sprites(scene.field).to_vec();
                let rock_centre = rect_centre(field[0].rect);
                assert!(
                    (rock_centre - expected).length() < 1e-6,
                    "a rock at alpha {alpha} was drawn at {rock_centre:?}, not {expected:?}",
                );

                let play = scene.stack.sprites(scene.play).to_vec();
                let ship_centre = rect_centre(play[0].rect);
                assert!(
                    (ship_centre - expected).length() < 1e-6,
                    "the ship at alpha {alpha} was drawn at {ship_centre:?}, not {expected:?}",
                );
            }
        });
    }

    /// **A teleported body is drawn at its current position whatever the
    /// alpha.** The whole point of the flag: lerping through a wrap would fly
    /// the body back across the field, so a flagged tick has to snap.
    #[test]
    fn a_teleported_body_is_drawn_at_its_current_position_whatever_the_alpha() {
        with_scene(|scene| {
            let rock = RockView {
                position: DVec3::new(10.0, 0.0, 0.0),
                prev_position: DVec3::new(-10.0, 0.0, 0.0),
                teleported: true,
                size: RockSize::Large,
                angle: 0.0,
                prev_angle: 0.0,
            };
            for alpha in [0.0, 0.5, 1.0] {
                let render = RenderState {
                    ship: DVec3::new(10.0, 0.0, 0.0),
                    ship_prev_pos: DVec3::new(-10.0, 0.0, 0.0),
                    ship_teleported: true,
                    ship_alive: true,
                    rocks: vec![rock],
                    ..RenderState::default()
                };
                scene.build(&render, alpha);

                let here = DVec3::new(10.0, 0.0, 0.0);
                let field = scene.stack.sprites(scene.field).to_vec();
                let rock_centre = rect_centre(field[0].rect);
                assert!(
                    (rock_centre - here).length() < 1e-6,
                    "a teleported rock at alpha {alpha} was drawn at {rock_centre:?}, \
                     not its current position",
                );

                let play = scene.stack.sprites(scene.play).to_vec();
                let ship_centre = rect_centre(play[0].rect);
                assert!(
                    (ship_centre - here).length() < 1e-6,
                    "a teleported ship at alpha {alpha} was drawn at {ship_centre:?}, \
                     not its current position",
                );
            }
        });
    }
}
