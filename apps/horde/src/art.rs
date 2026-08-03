//! Horde's art: two baked sheets, the layers they are drawn on, and the one
//! function that turns a frame of simulation into a list of sprites.
//!
//! ```text
//! assets/*.crpix ──build.rs──▶ PNG + sidecar ──include_bytes!──▶ Scene::new
//!                                                                    │
//!  RenderState + extent ───────────────── Scene::build ──────────────┤
//!                                                                    ▼
//!                LayerStack   gems ▸ crowd ▸ hero ▸ shots      ──▶ &[Sprite]
//!                                                                    │
//!                                                     SpriteRenderer::begin_frame
//! ```
//!
//! # Sprite space is world space times [`TEXELS_PER_UNIT`]
//!
//! Every rectangle in this module is in **texels**, and [`crate::gpu`]'s camera
//! is scaled to match — the convention breakout, flappy and asteroids all
//! reached. Nothing here is nine-sliced, so the trap
//! [`NineSliceSource::expand`](crcbl::render::NineSliceSource::expand) sets for
//! the other two does not apply; the scale is kept because it makes every
//! rectangle in this file read in texels and because a fourth sample using a
//! different convention for no reason would be a fourth thing to learn.
//!
//! # Two sheets, two batches, and neither number moves with the horde
//!
//! **This is the decision the scale sub-slice will measure.** [`SpriteRenderer`]
//! starts a new batch — a bind and a draw — whenever consecutive sprites name a
//! different sheet. Asteroids has three rock sheets and has to emit its field
//! largest-first to hold that at three; a field of ten thousand walked in the
//! order the game happens to hold it would be ten thousand batches.
//!
//! So the player, all three enemy kinds and the XP gems share **one** sheet, at
//! one frame size — [`ACTOR_HALF_EXTENT`], which is the largest collider in the
//! game. There is no order to get wrong and no grouping pass over the crowd:
//! whatever [`Scene::build`] pushes, the whole field is one run. The shot is the
//! only second sheet, because it is 8 texels against 34 and putting it in the
//! big frame would draw it in a quad twenty times its own area.
//!
//! What that costs is the transparent margin round the two small kinds. A runner
//! is 13 texels of art in a 34-texel quad, so about seven times the fill — and
//! it is bounded by the *screen* rather than by the horde, because a crowd
//! settles about 1.25 units apart and a view holds a few hundred of them
//! whatever the field size. `assets/actors.crpix` carries the arithmetic.
//!
//! # The camera moves, so this culls
//!
//! The arena is 96 × 72 units against a view of about 37 × 28, so most of a
//! large horde is off screen most of the time. An off-screen sprite is a 64-byte
//! instance uploaded and a quad clipped for nothing, and GPU culling is P7's, so
//! [`Scene::build`] rejects them on the CPU against the same
//! [`camera_centre`](crate::gpu::camera_centre) the projection uses. There is no
//! cap beyond that any more: the placeholder renderer had one because a
//! `DrawList` quad was six vertices uploaded per frame, and an instanced sprite
//! is not.

use crcbl::hal::{Device, HalError};
use crcbl::math::DVec3;
use crcbl::render::{Layer, LayerStack, Parallax, SheetDesc, SheetId, Sprite, SpriteRenderer};
use crcbl::sprite::Sheet;
use crcbl::sprite::load::{Loaded, load};

use crate::game::{EnemyKind, RenderState};
use crate::gpu::{camera_centre, view_half_width};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`,
// with the sidecar `None` for art that needs no metadata.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

// ---------------------------------------------------------------------------
// The scale, the clock, and the ground
// ---------------------------------------------------------------------------

/// Texels of art per world unit.
///
/// **Twenty is chosen by the runner**, which is the smallest thing in this game
/// that has to read as a *shape*. It is 0.64 world units across, and three enemy
/// kinds have to be told apart at a glance in a crowd — a harder ask than
/// asteroids' "not a circle", because it is a comparison rather than a
/// description. Each kind needs an identifying feature, and a feature needs a
/// texel to stand out, a texel to bite back in and a rim either side: about
/// thirteen texels across is the floor, and 13 / 0.64 is 20.3.
///
/// Half of it puts the runner at six texels, which is a dot. Double it and the
/// brute is 68 rows of hand-written art for one silhouette.
///
/// It is **not** copied from anywhere: breakout and asteroids reached 10 from
/// their smallest object and flappy 20 from its bird. That horde lands on
/// flappy's number is a coincidence of two games whose smallest actor is about a
/// twentieth of the view.
///
/// At 20 the brute's box — the frame size every actor shares — is exactly 34
/// texels, the player's exactly 20 and a gem's exactly 14. No scale makes all
/// three *enemy* boxes whole; `assets/actors.crpix` has the arithmetic.
pub const TEXELS_PER_UNIT: f32 = 20.0;

/// The tick rate the sheets' frame holds were baked against.
///
/// **Must equal `build.rs`'s `ART_TICK_HZ`.** A `.crpix` counts holds in ticks
/// and an Aseprite sidecar counts milliseconds; bake converts one way and
/// [`load`] the other, so a mismatch scales every hold silently. Nothing here is
/// animated, so this guard is as weak as breakout's and asteroids' and for the
/// same reason. `docs/backlog.md` carries it.
const ART_TICK_HZ: u32 = 60;

/// What the swapchain is cleared to — the arena's ground.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; this is the placeholder renderer's `#1a1a20` put through the
/// sRGB→linear transfer function once, here, rather than looking washed out on
/// screen. Dark and slightly cool, so the warm crowd and the green gems both
/// read against it.
pub const GROUND: [f32; 4] = [0.00972, 0.00972, 0.01444, 1.0];

/// "The sheet as authored" — no tinting anywhere in this game.
const UNTINTED: [f32; 4] = [1.0; 4];

/// Half the side of the one frame size every actor is drawn at, in world units.
///
/// `EnemyKind::Brute.radius()`, and therefore 34 texels — see this module's
/// header for why a grunt is drawn in a brute-sized quad. It is written here
/// rather than spelled out at the call sites because
/// `tests::every_sprite_covers_the_collider_it_stands_for` asserts the relation
/// against `game.rs`'s radii.
pub const ACTOR_HALF_EXTENT: f64 = 0.85;

/// The same for a shot.
///
/// Larger than [`BOLT_RADIUS`](crate::game::BOLT_RADIUS) = 0.15, deliberately:
/// 0.3 units is six texels, and a six-texel dot cannot be told from a speck on
/// the screen. Nothing in the game can be hit *by* the drawn shot — the radius
/// is the width of a swept sphere, not a collider — so the sprite is free to be
/// legible.
pub const BOLT_HALF_EXTENT: f64 = 0.2;

// ---------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------

/// One frame of a sheet, resolved to the UV rectangle it will ever use.
#[derive(Clone, Copy, Debug)]
struct FrameArt {
    sheet: SheetId,
    uv: [f32; 4],
}

/// Which frame of `assets/actors.crpix` an enemy kind draws.
const fn enemy_frame(kind: EnemyKind) -> &'static str {
    match kind {
        EnemyKind::Grunt => "grunt",
        EnemyKind::Runner => "runner",
        EnemyKind::Brute => "brute",
    }
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// Everything horde draws, and the layers it draws them on.
///
/// Built once — registering a sheet is a blocking staging upload — and then
/// [`Scene::build`] per frame, which clears the stack and refills it without
/// allocating.
#[derive(Debug)]
pub struct Scene {
    stack: LayerStack,
    /// One per [`EnemyKind`], indexed by [`kind_index`]. Three *frames* of one
    /// sheet, which is the whole batching argument in this module's header.
    enemies: [FrameArt; 3],
    player: FrameArt,
    gem: FrameArt,
    bolt: FrameArt,
    /// The gems, on the ground and under everything.
    gems: Layer,
    /// The horde.
    crowd: Layer,
    /// The player, over the crowd.
    hero: Layer,
    /// The shots, over the player.
    ///
    /// **Over, where asteroids draws its ship last and its bullets under it.**
    /// Two reasons, and the second is the load-bearing one: a bolt leaves the
    /// muzzle at `PLAYER_RADIUS + BOLT_RADIUS + 0.05`, so it is outside the hull
    /// before it is ever drawn and there is nothing to paste over; and the
    /// player shares the actors sheet, so putting the shots between the crowd
    /// and the player would split the field's one batch into three.
    shots: Layer,
    /// What the last [`Scene::build`] produced. See [`SceneStats`].
    stats: SceneStats,
}

/// What one [`Scene::build`] cost and produced.
///
/// **The numbers this sample exists to make visible**, which is why they are a
/// debug-panel module rather than a comment: the claim in this module's header
/// is that the batch count does not move with the horde, and the claim in
/// `docs/plan/sample/03-horde.md` is that the CPU cost of a frame is flat from
/// one thousand enemies to ten thousand. Neither can be read off a frame rate.
///
/// [`SceneStats::batches`] is
/// [`crcbl::render::sprite_pass::batch_count`], the pass's own answer, and it
/// used to be a mirror of the rule written out here — which would have left
/// this number right and the picture wrong the day the engine's batching
/// changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneStats {
    /// Enemies, gems and bolts the simulation handed over, before the cull.
    pub field: usize,
    /// How many of those the view box rejected. The work P7's GPU culling is
    /// there to delete, counted so it can be seen.
    pub culled: usize,
    /// Sprites that survived it, which is what the pass uploads and draws.
    /// Always at least one, because the player is never culled.
    pub drawn: usize,
    /// Draw calls the sprite pass will make for them.
    pub batches: usize,
}

impl crcbl::ui::DebugModule for SceneStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("scene");
        section.row("field", format_args!("{}", self.field));
        section.row("culled", format_args!("{}", self.culled));
        section.row("drawn", format_args!("{}", self.drawn));
        section.row("batches", format_args!("{}", self.batches));
    }
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
    /// If a baked sheet cannot be read back, or does not contain a frame this
    /// module names.
    pub fn new(device: &dyn Device, sprites: &mut SpriteRenderer) -> Result<Self, HalError> {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let bolt = baked("bolt", BOLT_PNG, BOLT_JSON);

        let actors_sheet = register(device, sprites, "actors", &actors)?;
        let bolt_sheet = register(device, sprites, "bolt", &bolt)?;

        // Back to front, and this is the only place the depth order is written
        // down: `LayerStack` has no depth field to disagree with it. All four
        // take the world's rate — the camera follows the player and there is
        // nothing behind the field to drift.
        let mut stack = LayerStack::new();
        let gems = stack.push_layer(Parallax::WORLD);
        let crowd = stack.push_layer(Parallax::WORLD);
        let hero = stack.push_layer(Parallax::WORLD);
        let shots = stack.push_layer(Parallax::WORLD);

        Ok(Self {
            stack,
            enemies: [
                frame(actors_sheet, &actors.sheet, enemy_frame(EnemyKind::Grunt)),
                frame(actors_sheet, &actors.sheet, enemy_frame(EnemyKind::Runner)),
                frame(actors_sheet, &actors.sheet, enemy_frame(EnemyKind::Brute)),
            ],
            player: frame(actors_sheet, &actors.sheet, "player"),
            gem: frame(actors_sheet, &actors.sheet, "gem"),
            // By index, not by name: a sheet with no sidecar has one frame
            // synthesised by `load` covering the whole image, and its name is
            // the loader's rather than the `.crpix`'s.
            bolt: still(bolt_sheet, &bolt.sheet),
            gems,
            crowd,
            hero,
            shots,
            stats: SceneStats::default(),
        })
    }

    /// What the last [`Scene::build`] produced.
    #[must_use]
    pub const fn stats(&self) -> SceneStats {
        self.stats
    }

    /// This frame's sprites, back to front, culled to the view.
    ///
    /// Everything is in **sprite units** — world units times
    /// [`TEXELS_PER_UNIT`] — and the same frame's view-projection must have been
    /// built at the same scale. `crate::gpu` applies it in one place.
    ///
    /// `extent` is the framebuffer's, and it decides both where the camera sits
    /// and how wide the cull box is; passing a different one from the
    /// projection's would cull against a view nobody is looking through.
    pub fn build(&mut self, render: &RenderState, extent: (u32, u32)) -> &[Sprite] {
        self.stack.clear();

        let camera = camera_centre(render.player, extent);
        // The view, grown by the drawn half-extent, so nothing pops in halfway
        // across the edge.
        let half_x = view_half_width(extent) + ACTOR_HALF_EXTENT;
        let half_y = crate::game::VIEW_HALF_HEIGHT + ACTOR_HALF_EXTENT;
        let visible =
            move |p: DVec3| (p.x - camera.x).abs() <= half_x && (p.y - camera.y).abs() <= half_y;

        let gem = self.gem;
        self.stack.extend(
            self.gems,
            render
                .pickups
                .iter()
                .filter(move |pickup| visible(pickup.position))
                .map(move |pickup| actor(gem, pickup.position)),
        );

        // **One pass, in the order the simulation holds them.** Nothing sorts
        // and nothing groups: every kind is a frame of the same sheet, so the
        // whole crowd is one batch however it comes out. See this module's
        // header.
        let enemies = self.enemies;
        self.stack.extend(
            self.crowd,
            render
                .enemies
                .iter()
                .filter(move |enemy| visible(enemy.position))
                .map(move |enemy| actor(enemies[kind_index(enemy.kind)], enemy.position)),
        );

        // The player is always inside the view — `gpu::camera_centre` is what
        // guarantees it — so there is nothing to cull here.
        self.stack
            .push(self.hero, actor(self.player, render.player));

        let bolt = self.bolt;
        self.stack.extend(
            self.shots,
            render
                .bolts
                .iter()
                .filter(move |shot| visible(shot.position))
                .map(move |shot| Sprite {
                    sheet: bolt.sheet,
                    rect: rect(shot.position, BOLT_HALF_EXTENT),
                    // Round, and with no attitude of its own; turning it would
                    // be a rotation nobody could see.
                    rotation: 0.0,
                    uv: bolt.uv,
                    tint: UNTINTED,
                }),
        );

        let frame = self.stack.resolve([
            (camera.x as f32) * TEXELS_PER_UNIT,
            (camera.y as f32) * TEXELS_PER_UNIT,
        ]);

        // Counted after `resolve`, on the list the pass will actually be handed:
        // the layer stack decides the emission order and the emission order is
        // what batching depends on, so counting the pieces as they were pushed
        // would be counting a different list.
        let field = render.enemies.len() + render.pickups.len() + render.bolts.len();
        // `- 1` for the player, which is in `frame` and not in `field`.
        let drawn = frame.len();
        self.stats = SceneStats {
            field,
            culled: field + 1 - drawn,
            drawn,
            batches: crcbl::render::sprite_pass::batch_count(frame),
        };
        frame
    }
}

// ---------------------------------------------------------------------------
// Pure geometry — no device, no sheet ids
// ---------------------------------------------------------------------------

/// Which of [`Scene::enemies`] a kind draws from.
const fn kind_index(kind: EnemyKind) -> usize {
    match kind {
        EnemyKind::Grunt => 0,
        EnemyKind::Runner => 1,
        EnemyKind::Brute => 2,
    }
}

/// One actor's sprite: the shared frame, at the shared size, centred on `at`.
fn actor(art: FrameArt, at: DVec3) -> Sprite {
    Sprite {
        sheet: art.sheet,
        rect: rect(at, ACTOR_HALF_EXTENT),
        rotation: 0.0,
        uv: art.uv,
        tint: UNTINTED,
    }
}

/// A world-space centre and a half-extent as a sprite rectangle: `[x, y, w, h]`,
/// **minimum corner first**, which is what [`Sprite::rect`] takes.
fn rect(centre: DVec3, half: f64) -> [f32; 4] {
    let scale = f64::from(TEXELS_PER_UNIT);
    [
        ((centre.x - half) * scale) as f32,
        ((centre.y - half) * scale) as f32,
        (2.0 * half * scale) as f32,
        (2.0 * half * scale) as f32,
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

/// The single frame of a sheet that has exactly one.
fn still(sheet: SheetId, description: &Sheet) -> FrameArt {
    FrameArt {
        sheet,
        uv: description.uv(0).expect("a still sheet has one frame"),
    }
}

/// One named frame of a sheet, resolved to its UV rectangle.
fn frame(sheet: SheetId, description: &Sheet, name: &str) -> FrameArt {
    let index = description
        .frame_index(name)
        .unwrap_or_else(|| panic!("the baked sheet has no frame called {name}"));
    FrameArt {
        sheet,
        uv: description
            .uv(index)
            .expect("a frame that was found has a rectangle"),
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
    use crcbl::sprite::SampleMode;

    use crate::game::{
        BOLT_RADIUS, EnemyView, PLAYER_RADIUS, PickupView, VIEW_HALF_HEIGHT, XP_RADIUS,
    };

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

    /// The window every test measures against.
    const EXTENT: (u32, u32) = (960, 720);

    fn enemy(kind: EnemyKind, position: DVec3) -> EnemyView {
        EnemyView {
            position,
            kind,
            health: 1.0,
        }
    }

    /// The opaque bounding box of one frame of a loaded sheet, in texels.
    ///
    /// The measurement the "the art is the size of the collider" claim rests on.
    /// The frame's own rectangle says nothing about it — every frame in
    /// `actors.crpix` is 34 × 34 by construction, and a sheet of blank 34-texel
    /// squares would satisfy any assertion made about the frame rather than
    /// about what is drawn inside it.
    fn silhouette(loaded: &Loaded, name: &str) -> (u32, u32) {
        let index = loaded
            .sheet
            .frame_index(name)
            .unwrap_or_else(|| panic!("no frame {name}"));
        let rect = loaded.sheet.frames[index].rect;
        let width = loaded.image.width;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                let alpha = loaded.image.pixels[((y * width + x) * 4 + 3) as usize];
                if alpha == 0 {
                    continue;
                }
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        assert!(min_x <= max_x, "{name} is blank");
        (max_x - min_x + 1, max_y - min_y + 1)
    }

    /// How far the silhouette reaches from the frame's centre along each of
    /// eight compass rays, as a fraction of the frame's half-width.
    ///
    /// Scale free, so two frames that are one drawing at two magnifications
    /// produce the *same* eight numbers and can be told from two drawings.
    /// Marched along the ray and taking the **last** opaque texel, not the
    /// first gap: a leg with a gap beside it is still a leg.
    fn profile(loaded: &Loaded, name: &str) -> [f64; 8] {
        let index = loaded.sheet.frame_index(name).expect("a frame");
        let rect = loaded.sheet.frames[index].rect;
        let width = loaded.image.width;
        let half = f64::from(rect.w) / 2.0;
        let (cx, cy) = (
            f64::from(rect.x) + half,
            f64::from(rect.y) + f64::from(rect.h) / 2.0,
        );
        std::array::from_fn(|dir| {
            let theta = dir as f64 * std::f64::consts::TAU / 8.0;
            // Image rows run down and the world's +Y runs up, which does not
            // matter to the *set* of eight numbers but does decide which is
            // which.
            let (dx, dy) = (theta.cos(), -theta.sin());
            let mut reach = 0.0_f64;
            let mut t = 0.0_f64;
            while t <= half * 1.5 {
                let (x, y) = ((cx + t * dx) as u32, (cy + t * dy) as u32);
                if x < rect.x + rect.w
                    && x >= rect.x
                    && y >= rect.y
                    && y < rect.y + rect.h
                    && loaded.image.pixels[((y * width + x) * 4 + 3) as usize] != 0
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

    /// **The art bakes to the sheets it declares**, at the sizes and with the
    /// frames this module names.
    ///
    /// A test that only checked `load` returned `Ok` would pass on a blank image
    /// with no frames in it, which is exactly the failure this is for; the alpha
    /// counts at the end are what rule that out.
    #[test]
    fn the_art_bakes_to_the_sheets_it_declares() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let scale = f64::from(TEXELS_PER_UNIT);

        // One frame size for the whole file, and it is the largest collider's
        // bounding square — asserted against `game.rs`'s radius, not repeated.
        let side = (2.0 * ACTOR_HALF_EXTENT * scale).round() as u32;
        assert_eq!(side, 34);
        assert!(
            (f64::from(side) - 2.0 * ACTOR_HALF_EXTENT * scale).abs() < 1e-9,
            "the shared frame does not land on a whole texel",
        );
        assert_eq!(actors.sheet.frames.len(), 5, "five actors in one sheet");
        assert_eq!(
            (actors.image.width, actors.image.height),
            (5 * side, side),
            "the sheet is not five {side}-texel frames in a strip",
        );
        for frame in &actors.sheet.frames {
            assert_eq!(
                (frame.rect.w, frame.rect.h),
                (side, side),
                "{}: a .crpix has one frame size for the whole file",
                frame.name,
            );
            assert_eq!(
                frame.hold, 1,
                "{}: the default hold in ticks did not survive the millisecond \
                 round trip — build.rs and ART_TICK_HZ disagree",
                frame.name,
            );
        }
        assert!(
            actors.sheet.clips.is_empty(),
            "nothing in this game is animated",
        );
        assert_eq!(actors.sheet.nine, None, "nothing here is stretched");
        assert_eq!(
            actors.sheet.sample,
            SampleMode::Pixel,
            "sample rule 11 asks for SampleMode::Pixel",
        );

        let bolt = baked("bolt", BOLT_PNG, BOLT_JSON);
        assert_eq!((bolt.image.width, bolt.image.height), (8, 8));
        assert_eq!(
            f64::from(bolt.image.width) / scale,
            2.0 * BOLT_HALF_EXTENT,
            "the shot's frame and the rectangle it is drawn at disagree",
        );
        assert_eq!(bolt.sheet.sample, SampleMode::Pixel);
        const {
            assert!(
                BOLT_HALF_EXTENT > BOLT_RADIUS,
                "a shot must be drawn larger than the sphere it sweeps",
            );
        }
        assert!(
            BOLT_JSON.is_none(),
            "one still frame with no clip and no nine-slice needs no sidecar",
        );

        // Anti-blank: every frame has something drawn in it, and the sheet has
        // transparent texels too. A sheet of zeroes satisfies every assertion
        // above, and `silhouette` panics on a blank frame rather than returning
        // a size.
        for name in ["player", "grunt", "runner", "brute", "gem"] {
            let (w, h) = silhouette(&actors, name);
            assert!(w > 1 && h > 1, "{name} is a dot: {w} x {h}");
            assert!(w <= side && h <= side, "{name} overflows its frame");
        }
        let clear = actors
            .image
            .pixels
            .chunks_exact(4)
            .filter(|p| p[3] == 0)
            .count();
        assert!(
            clear > 0 && clear < (actors.image.pixels.len() / 4),
            "the actors sheet is {clear} clear of {} texels",
            actors.image.pixels.len() / 4,
        );
        let clear = bolt
            .image
            .pixels
            .chunks_exact(4)
            .filter(|p| p[3] == 0)
            .count();
        assert!(
            clear > 0 && clear < 64,
            "the shot is {clear}/64 transparent"
        );
    }

    /// **Every silhouette is the size of the collider it stands for.**
    ///
    /// The assertion that ties the art to the physics, and the one that has to
    /// carry the weight here: every frame is 34 × 34 by construction, so the
    /// *frame* size says nothing. What is drawn inside it is measured, in
    /// texels, against the radii `game.rs` declares — to the texel, because a
    /// crowd drawn wider than it collides is a crowd that looks jammed solid
    /// while the simulation says it is not.
    #[test]
    fn every_silhouette_is_the_size_of_the_collider_it_stands_for() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let scale = f64::from(TEXELS_PER_UNIT);

        let want = |radius: f64| (2.0 * radius * scale).ceil() as u32;
        for kind in EnemyKind::ALL {
            let name = enemy_frame(kind);
            let (w, h) = silhouette(&actors, name);
            assert_eq!(
                (w, h),
                (want(kind.radius()), want(kind.radius())),
                "{name} is {w} x {h} texels against a collider of {} units",
                2.0 * kind.radius(),
            );
        }
        assert_eq!(
            silhouette(&actors, "player"),
            (want(PLAYER_RADIUS), want(PLAYER_RADIUS)),
        );
        assert_eq!(
            silhouette(&actors, "gem"),
            (want(XP_RADIUS), want(XP_RADIUS))
        );
    }

    /// **The three enemy kinds are three different pictures.**
    ///
    /// Three frames of one drawing at three magnifications would parse, bake,
    /// load, index and draw exactly as three drawings do, and every other test
    /// in this file passes on them — so "three kinds" is worth nothing until the
    /// pictures are known to differ *in shape* and not only in pixel count.
    /// Hence [`profile`], which is scale free.
    ///
    /// It matters more here than it did for asteroids' rocks: those are three
    /// sizes of one thing and are *meant* to look related, while a player who
    /// cannot tell a runner from a grunt at a glance cannot play this game.
    #[test]
    fn the_three_enemy_kinds_are_three_different_pictures() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let profiles: [[f64; 8]; 3] =
            std::array::from_fn(|i| profile(&actors, enemy_frame(EnemyKind::ALL[i])));

        for (kind, profile) in EnemyKind::ALL.iter().zip(&profiles) {
            let min = profile.iter().copied().fold(f64::MAX, f64::min);
            assert!(min > 0.0, "{kind:?} has nothing drawn in one direction");
        }

        for a in 0..3 {
            for b in (a + 1)..3 {
                let apart: f64 = profiles[a]
                    .iter()
                    .zip(&profiles[b])
                    .map(|(x, y)| (x - y).abs())
                    .fold(f64::MIN, f64::max);
                assert!(
                    apart > 0.12,
                    "{:?} and {:?} are the same silhouette scaled: the outlines \
                     never differ by more than {apart:.3} of the half-width\n\
                     {:?}\n{:?}",
                    EnemyKind::ALL[a],
                    EnemyKind::ALL[b],
                    profiles[a],
                    profiles[b],
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The scene
    // -----------------------------------------------------------------------

    /// **The rectangle drawn covers the collider it stands for**, for every
    /// kind of thing on the field.
    ///
    /// Asserted against the radii `game.rs` declares rather than against numbers
    /// repeated here. Every actor is drawn at the largest collider's box, so for
    /// the two small enemies this is a cover with room to spare and the
    /// *silhouette* test above is what pins their apparent size; the brute is
    /// the one whose quad is its collider exactly, and it is checked as such.
    #[test]
    fn every_sprite_covers_the_collider_it_stands_for() {
        with_scene(|scene| {
            let render = RenderState {
                player: DVec3::new(-2.5, 1.25, 0.0),
                enemies: vec![
                    enemy(EnemyKind::Grunt, DVec3::new(4.0, -3.0, 0.0)),
                    enemy(EnemyKind::Runner, DVec3::new(-7.5, 2.0, 0.0)),
                    enemy(EnemyKind::Brute, DVec3::new(0.25, 9.0, 0.0)),
                ],
                bolts: vec![crate::game::BoltView {
                    position: DVec3::new(3.0, -1.5, 0.0),
                }],
                pickups: vec![PickupView {
                    position: DVec3::new(-1.0, -4.0, 0.0),
                }],
                ..RenderState::default()
            };
            scene.build(&render, EXTENT);

            let scale = f64::from(TEXELS_PER_UNIT);
            let covers = |rect: [f32; 4], centre: DVec3, radius: f64, what: &str| {
                let (min_x, min_y) = (f64::from(rect[0]), f64::from(rect[1]));
                let (max_x, max_y) = (min_x + f64::from(rect[2]), min_y + f64::from(rect[3]));
                for (axis, (lo, hi, c)) in [(min_x, max_x, centre.x), (min_y, max_y, centre.y)]
                    .into_iter()
                    .enumerate()
                {
                    assert!(
                        lo <= (c - radius) * scale + 1e-6 && hi >= (c + radius) * scale - 1e-6,
                        "{what}: axis {axis} is drawn {lo}..{hi}, the collider needs {}..{}",
                        (c - radius) * scale,
                        (c + radius) * scale,
                    );
                }
            };

            let crowd = scene.stack.sprites(scene.crowd).to_vec();
            assert_eq!(crowd.len(), 3, "one sprite per enemy");
            for (index, view) in render.enemies.iter().enumerate() {
                covers(
                    crowd[index].rect,
                    view.position,
                    view.kind.radius(),
                    "an enemy",
                );
            }
            // The brute's quad is its collider exactly — it is the frame size —
            // which is what stops the shared box drifting from the physics.
            assert!(
                (f64::from(crowd[2].rect[2]) - 2.0 * EnemyKind::Brute.radius() * scale).abs()
                    < 1e-3,
                "the shared frame is not the brute's bounding square",
            );

            covers(
                scene.stack.sprites(scene.hero)[0].rect,
                render.player,
                PLAYER_RADIUS,
                "the player",
            );
            covers(
                scene.stack.sprites(scene.gems)[0].rect,
                render.pickups[0].position,
                XP_RADIUS,
                "a gem",
            );
            covers(
                scene.stack.sprites(scene.shots)[0].rect,
                render.bolts[0].position,
                BOLT_RADIUS,
                "a shot",
            );

            // Anti-vacuity: `covers` can fail. A sprite at its neighbour's
            // position is the right size in the wrong place, and it says so.
            let moved = actor(scene.player, render.player + DVec3::X * 5.0);
            assert!(
                (f64::from(moved.rect[0]) - f64::from(scene.stack.sprites(scene.hero)[0].rect[0]))
                    .abs()
                    > 1.0,
                "two different positions produced the same rectangle",
            );
        });
    }

    /// **The whole field is two batches, whatever order it comes in.**
    ///
    /// The claim this module's sheet split exists for, and the one the scale
    /// sub-slice will measure: a `SpriteRenderer` batch is a run of consecutive
    /// sprites naming one sheet, so an interleaved field of every kind must
    /// still resolve to the actors sheet followed by the bolt sheet — two runs,
    /// not one per enemy.
    #[test]
    fn an_interleaved_field_of_every_kind_is_two_batches() {
        with_scene(|scene| {
            let kinds = [EnemyKind::Brute, EnemyKind::Grunt, EnemyKind::Runner];
            let render = RenderState {
                player: DVec3::ZERO,
                // Deliberately shuffled, which is what the simulation's
                // `swap_remove` actually leaves behind.
                enemies: (0..24)
                    .map(|i| {
                        enemy(
                            kinds[i % 3],
                            DVec3::new((i % 5) as f64 - 2.0, (i / 5) as f64 - 2.0, 0.0),
                        )
                    })
                    .collect(),
                pickups: (0..6)
                    .map(|i| PickupView {
                        position: DVec3::new(i as f64 * 0.5, 3.0, 0.0),
                    })
                    .collect(),
                bolts: (0..3)
                    .map(|i| crate::game::BoltView {
                        position: DVec3::new(i as f64, -3.0, 0.0),
                    })
                    .collect(),
                ..RenderState::default()
            };
            let frame = scene.build(&render, EXTENT).to_vec();
            assert_eq!(frame.len(), 6 + 24 + 1 + 3);

            let runs = 1 + frame
                .windows(2)
                .filter(|w| w[0].sheet != w[1].sheet)
                .count();
            assert_eq!(
                runs,
                2,
                "{} sprites in three kinds came out as {runs} batches",
                frame.len(),
            );
            // …and the two sheets really are two, or the count above is
            // satisfied by everything sharing one.
            let mut sheets: Vec<_> = frame.iter().map(|s| s.sheet).collect();
            sheets.dedup();
            assert_eq!(sheets.len(), 2);
            assert_eq!(sheets[1], scene.bolt.sheet, "the shots come last");

            // The three enemy kinds really are three different frames of that
            // one sheet, or "one batch" is one picture.
            let mut uvs: Vec<[u32; 4]> = scene
                .enemies
                .iter()
                .map(|art| art.uv.map(|v| v.to_bits()))
                .collect();
            uvs.sort_unstable();
            uvs.dedup();
            assert_eq!(uvs.len(), 3, "two enemy kinds share a frame");
        });
    }

    /// **A batch is a run, not a sheet**, asked of the engine that decides it.
    ///
    /// Written because the ten-thousand test below **cannot** tell the two
    /// apart: this game emits its two sheets in one order, so a run count and a
    /// distinct-sheet count agree on every frame it produces. `A A B A` is
    /// where they differ, and it is the shape a future layer order would
    /// produce — so the rule is pinned here rather than left to be discovered
    /// by a frame that draws in the wrong order and reports the wrong number.
    ///
    /// This used to call a copy of the rule kept in this module. It calls
    /// [`crcbl::render::sprite_pass::batch_count`] now, so the sample's central
    /// claim is checked against the pass rather than against horde's memory of
    /// what the pass does.
    #[test]
    fn a_batch_is_a_run_of_one_sheet_and_not_a_distinct_sheet_count() {
        with_scene(|scene| {
            let sprite = |art: FrameArt| Sprite {
                sheet: art.sheet,
                rect: [0.0; 4],
                rotation: 0.0,
                uv: art.uv,
                tint: UNTINTED,
            };
            let (a, b) = (sprite(scene.player), sprite(scene.bolt));
            assert_ne!(a.sheet, b.sheet, "the fixture needs two sheets");

            let batches = crcbl::render::sprite_pass::batch_count;
            assert_eq!(batches(&[]), 0);
            assert_eq!(batches(&[a]), 1);
            assert_eq!(batches(&[a, a, a]), 1);
            assert_eq!(batches(&[a, b]), 2);
            assert_eq!(
                batches(&[a, a, b, a]),
                3,
                "A A B A is three draws; a distinct-sheet count says two",
            );
            assert_eq!(batches(&[a, b, a, b]), 4);
        });
    }

    /// **Ten thousand enemies are still two batches**, which is the number the
    /// plan's exit criteria ask for and the one the 34-sprite test above cannot
    /// give.
    ///
    /// The whole field is packed *inside* the view so nothing is culled: a
    /// version that let the cull do its job would assert two batches over the
    /// fifteen hundred that survive, which is the same claim at a tenth of the
    /// pressure. The counting rule is [`batches`], this module's mirror of
    /// `sprite_pass`'s — see [`SceneStats`] and `docs/backlog.md` for why that
    /// is weaker than asking the pass.
    #[test]
    fn ten_thousand_visible_enemies_are_still_two_batches() {
        const COUNT: usize = 10_000;
        with_scene(|scene| {
            // A grid inside the view box, which is 2 * view_half_width by
            // 2 * VIEW_HALF_HEIGHT around a player at the origin.
            let half_x = crate::gpu::view_half_width(EXTENT) - 0.5;
            let half_y = VIEW_HALF_HEIGHT - 0.5;
            let cols = 125usize;
            let rows = COUNT / cols;
            let kinds = [EnemyKind::Brute, EnemyKind::Grunt, EnemyKind::Runner];
            let enemies: Vec<_> = (0..COUNT)
                .map(|i| {
                    let (col, row) = (i % cols, i / cols);
                    enemy(
                        // Interleaved, because that is what `swap_remove` leaves
                        // behind and the claim is that the order does not matter.
                        kinds[i % 3],
                        DVec3::new(
                            -half_x + 2.0 * half_x * (col as f64 / (cols - 1) as f64),
                            -half_y + 2.0 * half_y * (row as f64 / (rows - 1) as f64),
                            0.0,
                        ),
                    )
                })
                .collect();
            let render = RenderState {
                player: DVec3::ZERO,
                enemies,
                bolts: (0..8)
                    .map(|i| crate::game::BoltView {
                        position: DVec3::new(i as f64 * 0.5, 0.0, 0.0),
                    })
                    .collect(),
                ..RenderState::default()
            };

            let frame = scene.build(&render, EXTENT).to_vec();
            let stats = scene.stats();
            assert_eq!(
                stats.culled, 0,
                "the fixture put {} of the field outside the view",
                stats.culled,
            );
            assert_eq!(stats.field, COUNT + 8);
            assert_eq!(stats.drawn, COUNT + 8 + 1, "the player is drawn too");
            assert_eq!(frame.len(), stats.drawn);
            assert_eq!(
                stats.batches, 2,
                "{} sprites came out as {} batches",
                stats.drawn, stats.batches,
            );
            // …and the two really are the actors sheet then the bolt sheet, or
            // "two batches" is one sheet drawn twice.
            let mut sheets: Vec<_> = frame.iter().map(|s| s.sheet).collect();
            sheets.dedup();
            assert_eq!(sheets.len(), 2);
            assert_eq!(sheets[1], scene.bolt.sheet, "the shots come last");
        });
    }

    /// **How much of the shared frame is transparent margin**, weighted by the
    /// mix the spawner actually deals.
    ///
    /// The number `docs/plan/sample/03-horde.md` quotes when it says what the
    /// one-sheet decision costs, pinned here so it is checkable rather than
    /// recomputed by hand every time someone redraws a silhouette. Everything in
    /// it is derived — the silhouettes from the baked bytes, the weights from
    /// [`EnemyKind::from_roll`] — so a kind that changed size, or a spawn table
    /// that changed the mix, moves this number rather than leaving the doc
    /// quietly wrong.
    ///
    /// A transparent fragment is **not** free: `SpriteRenderer` has no alpha
    /// discard, so the margin is rasterised and blended exactly like the art.
    /// That is why this is a fill number and not a memory one.
    #[test]
    fn two_thirds_of_the_shared_frame_is_transparent_margin() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        // The quad every actor is drawn in, in texels — the same number
        // `the_art_bakes_to_the_sheets_it_declares` derives, off the constant
        // rather than off the sheet, so a sheet baked at the wrong size fails
        // there rather than making this pass against itself.
        let side = 2.0 * ACTOR_HALF_EXTENT * f64::from(TEXELS_PER_UNIT);
        let quad = side * side;

        // The spawner's own mix, sampled off `from_roll` rather than copied out
        // of it: the thresholds are that function's business.
        const ROLLS: usize = 100_000;
        let mut weight = [0.0f64; 3];
        for i in 0..ROLLS {
            let kind = EnemyKind::from_roll(i as f64 / ROLLS as f64);
            weight[kind_index(kind)] += 1.0 / ROLLS as f64;
        }
        assert!(
            (weight.iter().sum::<f64>() - 1.0).abs() < 1e-9,
            "the mix does not sum to one: {weight:?}",
        );

        let mut opaque = 0.0;
        for kind in EnemyKind::ALL {
            let (w, h) = silhouette(&actors, enemy_frame(kind));
            opaque += weight[kind_index(kind)] * f64::from(w) * f64::from(h);
        }
        let fraction = opaque / quad;
        assert!(
            (0.30..0.33).contains(&fraction),
            "the average enemy fills {:.1}% of its {side} x {side} quad; \
             docs/plan/sample/03-horde.md says 31.5%",
            fraction * 100.0,
        );
        // The brute is the one that fills its frame exactly — the frame size is
        // its collider — so the margin is entirely the two small kinds'.
        let (bw, bh) = silhouette(&actors, enemy_frame(EnemyKind::Brute));
        assert_eq!(
            (f64::from(bw), f64::from(bh)),
            (side, side),
            "the frame is no longer the brute's own size, so the margin moved",
        );
    }

    /// **The cull is what keeps the drawn count bounded by the screen**, which
    /// is the other half of the flat-cost claim: the field grows, the frame
    /// does not.
    ///
    /// A `Scene::build` that stopped culling would pass every batch assertion
    /// above and hand the pass ten thousand instances a frame.
    #[test]
    fn a_field_larger_than_the_view_is_culled_to_the_view() {
        with_scene(|scene| {
            // The arena, filled evenly — the same shape `Game::stage_field`
            // produces, which is the fixture every number in
            // `docs/plan/sample/03-horde.md` was taken through.
            let (half_x, half_y) = (
                crate::game::ARENA_HALF_WIDTH,
                crate::game::ARENA_HALF_HEIGHT,
            );
            let cols = 116usize;
            let count = 10_000usize;
            let rows = count.div_ceil(cols);
            let enemies: Vec<_> = (0..count)
                .map(|i| {
                    let (col, row) = (i % cols, i / cols);
                    enemy(
                        EnemyKind::Grunt,
                        DVec3::new(
                            -half_x + 2.0 * half_x * (col + 1) as f64 / (cols + 1) as f64,
                            -half_y + 2.0 * half_y * (row + 1) as f64 / (rows + 1) as f64,
                            0.0,
                        ),
                    )
                })
                .collect();
            let render = RenderState {
                player: DVec3::ZERO,
                enemies,
                ..RenderState::default()
            };
            scene.build(&render, EXTENT);
            let stats = scene.stats();
            assert_eq!(stats.field, count);
            assert_eq!(
                stats.field + 1,
                stats.culled + stats.drawn,
                "the cull's own arithmetic does not close",
            );

            // The view is about 37 x 28 units of a 96 x 72 arena, so a little
            // over an eighth of it. The bound is loose on purpose — the exact
            // number depends on the grid — but a cull that rejected nothing, or
            // everything, fails it.
            assert!(
                stats.drawn > 500 && stats.drawn < count / 4,
                "{} of {count} survived the cull",
                stats.drawn,
            );
            assert_eq!(stats.batches, 1, "one sheet, one batch");
        });
    }

    /// Each kind of thing goes on the layer it belongs to, back to front.
    #[test]
    fn the_gems_are_behind_the_crowd_and_the_shots_are_in_front() {
        with_scene(|scene| {
            assert_eq!(scene.stack.layer_count(), 4);
            assert_eq!(scene.gems.depth(), 0);
            assert_eq!(scene.crowd.depth(), 1);
            assert_eq!(scene.hero.depth(), 2);
            assert_eq!(scene.shots.depth(), 3);
        });
    }

    /// **An enemy outside the view is not a sprite**, and the near one is.
    ///
    /// The cull is what makes a 96 × 72 arena drawable at all, and a cull that
    /// silently matched nothing would look exactly like this test passing — so
    /// the near enemy is asserted *present* in the same run that asserts the far
    /// one absent.
    #[test]
    fn enemies_outside_the_view_are_culled_and_the_near_one_is_not() {
        with_scene(|scene| {
            let far = view_half_width(EXTENT) + ACTOR_HALF_EXTENT + 5.0;
            let render = |x: f64| RenderState {
                player: DVec3::ZERO,
                enemies: vec![enemy(EnemyKind::Grunt, DVec3::new(x, 0.0, 0.0))],
                ..RenderState::default()
            };
            assert!(
                far < crate::game::ARENA_HALF_WIDTH,
                "the far enemy has to be somewhere the arena can hold it",
            );

            scene.build(&render(2.0), EXTENT);
            assert_eq!(scene.stack.sprites(scene.crowd).len(), 1, "the near one");
            scene.build(&render(far), EXTENT);
            assert_eq!(scene.stack.sprites(scene.crowd).len(), 0, "the far one");

            // And vertically, which a cull written against one axis passes.
            let high = RenderState {
                player: DVec3::ZERO,
                enemies: vec![enemy(
                    EnemyKind::Grunt,
                    DVec3::new(0.0, VIEW_HALF_HEIGHT + ACTOR_HALF_EXTENT + 5.0, 0.0),
                )],
                ..RenderState::default()
            };
            scene.build(&high, EXTENT);
            assert_eq!(scene.stack.sprites(scene.crowd).len(), 0);
        });
    }

    /// **The cull follows the camera, not the origin.**
    ///
    /// A cull box fixed at the middle of the arena would pass every assertion
    /// above and would delete the crowd around a player standing in a corner.
    #[test]
    fn the_cull_box_travels_with_the_player() {
        with_scene(|scene| {
            let corner = DVec3::new(30.0, 20.0, 0.0);
            let render = RenderState {
                player: corner,
                enemies: vec![
                    enemy(EnemyKind::Grunt, corner + DVec3::new(2.0, 1.0, 0.0)),
                    enemy(EnemyKind::Runner, DVec3::ZERO),
                ],
                ..RenderState::default()
            };
            scene.build(&render, EXTENT);
            let crowd = scene.stack.sprites(scene.crowd).to_vec();
            assert_eq!(crowd.len(), 1, "the enemy beside the player was culled");
            assert_eq!(crowd[0].uv, scene.enemies[kind_index(EnemyKind::Grunt)].uv);
        });
    }
}
