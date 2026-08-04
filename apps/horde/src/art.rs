//! Horde's art: four baked sheets, the layers they are drawn on, and the one
//! function that turns a frame of simulation into a list of sprites.
//!
//! ```text
//! assets/*.crpix ──build.rs──▶ PNG + sidecar ──include_bytes!──▶ Scene::new
//!                                                                    │
//!  RenderState + extent ───────────────── Scene::build ──────────────┤
//!                                                                    ▼
//!    LayerStack  ground ▸ props ▸ gems ▸ crowd ▸ hero ▸ shots  ──▶ &[Sprite]
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
//! # Four sheets, four batches, and the count is flat in the horde
//!
//! **This is the decision the scale sub-slice measured.** [`SpriteRenderer`]
//! starts a new batch — a bind and a draw — whenever consecutive sprites name a
//! different sheet. Asteroids has three rock sheets and has to emit its field
//! largest-first to hold that at three; a field of ten thousand walked in the
//! order the game happens to hold it would be ten thousand batches.
//!
//! So the player, all three enemy kinds and the XP gems share **one** sheet, at
//! one frame size — [`ACTOR_HALF_EXTENT`], which is the largest collider in the
//! game. There is no order to get wrong and no grouping pass over the crowd:
//! whatever [`Scene::build`] pushes, the whole field is one run. The shot is a
//! second sheet, because it is 8 texels against 34 and putting it in the big
//! frame would draw it in a quad twenty times its own area. The ground is a
//! third, because it is a 40-texel tile of a different subject and it is drawn
//! *under* everything, which is a layer of its own however it is packed. The
//! props are a fourth at 36 texels, and a `.crpix` declares one frame size for
//! the whole file, so no existing sheet could have held them.
//!
//! **The claim that matters is not the number four — it is that the number
//! does not move with the size of the horde**, which is what
//! [`SceneStats::batches`] is there to show. Every sheet is one uninterrupted
//! run, so ten enemies and ten thousand come out as the same four draws; the
//! terrain and props sheets each added a constant and constants are what this
//! claim tolerates. What would break it is emitting a sheet more than once —
//! putting the shots between the crowd and the player, say — and
//! `tests::an_interleaved_field_of_every_kind_is_four_batches` and
//! `tests::ten_thousand_visible_enemies_are_still_four_batches` are what say
//! it has not happened. The measured table in `docs/plan/sample/03-horde.md`
//! was taken before either sheet existed and reads two; it says so.
//!
//! What the shared actors frame costs is the transparent margin round the two
//! small kinds. A runner is 13 texels of art in a 34-texel quad, so about seven
//! times the fill — and it is bounded by the *screen* rather than by the horde,
//! because a crowd settles about 1.25 units apart and a view holds a few hundred
//! of them whatever the field size. `assets/actors.crpix` carries the
//! arithmetic.
//!
//! # One thing in this game is animated, and it is the player
//!
//! The wizard has a walk cycle — [`WALK_CLIP`] of `assets/actors.crpix` — and
//! everything else is a still frame at a moving position. Two consequences worth
//! stating, because both are the sort of thing a reader assumes the other way
//! round:
//!
//! * **The clock is [`RenderState::elapsed`]**, the run's simulated seconds,
//!   through [`walk_tick`]. Not a wall clock: `elapsed` is replicated, so a
//!   replay of a script animates frame for frame the way the run it replays did,
//!   and a level-up screen freezes the cycle with the rest of the field for free.
//! * **Facing is a reversed `u` range, not a second column of art.** The figure
//!   is drawn facing right and [`mirrored`] turns it round, so there is one
//!   column of art rather than two and the facings cannot drift apart. Which way
//!   it faces is [`RenderState::player_facing`], which the *input* decides — see
//!   [`game::Facing`](crate::game::Facing) for why it is not the aim.
//!
//! Neither costs a batch: every frame of the wizard is a frame of the one actors
//! sheet, and a flip is four floats on the instance.
//!
//! # The ground overruns the arena rather than stopping at it
//!
//! [`Scene::build`] lays [`GROUND_TILE`] tiles over the *view*, not over the
//! arena: the tile lattice is unbounded, the variant comes from the tile's own
//! coordinates, and there is no edge anywhere in it. So the clear colour is
//! never visible and there is no void strip to explain.
//!
//! The alternative — stopping the grass at
//! [`ARENA_HALF_WIDTH`](crate::game::ARENA_HALF_WIDTH) — would draw a boundary
//! that is invisible at every ordinary window shape anyway, because
//! [`camera_centre`] clamps the camera so the wall never comes into view. The
//! one shape where it *would* show is a window wider than the arena, which
//! `crate::gpu` handles by centring that axis and letting the margin be
//! symmetric; a void strip there would advertise the fallback rather than hide
//! it. And the wall the player actually collides with is
//! [`clamp_to_arena`](crate::game::clamp_to_arena), which nothing on screen
//! marks today — drawing an edge for it would be a gameplay change smuggled in
//! by a texture.
//!
//! # The props are above the ground and below everything that moves
//!
//! There is no other order available: over the crowd they would hide the horde,
//! and under the ground they would not be drawn. The cost of that order is the
//! one a tall prop pays — a canopy painted *under* a figure standing in front of
//! it — and `assets/props.crpix` answers it by not drawing a tall prop at all.
//! Both are footprints, seen from directly above like the lawn under them and
//! drawn to their own collider like every actor, so a wizard overlapping a
//! canopy edge is a wizard standing on that ground. Splitting the sheet so a
//! canopy could go over the actors would be a second run of one sheet, which is
//! another draw call in a number `docs/plan/sample/03-horde.md` and the
//! changelog both quote.
//!
//! They are above the ground rather than in it for a plainer reason: the ground
//! is generated from the view and the props are culled against it, so they are
//! two different kinds of list even before they are two sheets.
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
use crcbl::sprite::load::{Loaded, load};
use crcbl::sprite::{Clip, Playback, Sheet};

use crate::game::{EnemyKind, Facing, PropKind, RenderState};
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

/// What the swapchain is cleared to.
///
/// **Linear, not sRGB.** The target is an sRGB format, so the clear is encoded
/// on the way in; this is the placeholder renderer's `#1a1a20` put through the
/// sRGB→linear transfer function once, here, rather than looking washed out on
/// screen. Dark and slightly cool, so the warm crowd and the green gems both
/// read against it.
///
/// It used to *be* the ground. It is a backstop now — the grass tiles cover the
/// whole view, so nothing should ever see this colour, and
/// `tests::the_ground_covers_the_view_with_no_gap` is what says so. Keeping it
/// dark rather than making it something loud is deliberate: a magenta backstop
/// finds a hole faster and would be a worse thing to ship if one survived.
pub const GROUND: [f32; 4] = [0.00972, 0.00972, 0.01444, 1.0];

/// The side of one ground tile, in world units.
///
/// **The frame size of `assets/terrain.crpix`**, which at [`TEXELS_PER_UNIT`] is
/// 40 texels — that file carries the argument for the number, and this constant
/// is the one place the code says what it means. A tile is drawn at exactly this
/// size, so the art and the lattice cannot drift apart.
pub const GROUND_TILE: f64 = 2.0;

/// How many grass tiles `assets/terrain.crpix` holds.
///
/// A power of two so [`ground_variant`] can take the choice off the top of a
/// hash with a shift instead of a modulo, which is both exact and unbiased.
const GROUND_VARIANTS: usize = 4;

/// The frames of `assets/terrain.crpix`, indexed by what [`ground_variant`]
/// returns.
const GROUND_FRAMES: [&str; GROUND_VARIANTS] = ["grass-a", "grass-b", "grass-c", "grass-d"];

/// The seed the ground's variant hash is drawn from.
///
/// `"GRASS"` in ASCII, in the spirit of `game.rs`'s `COMPATIBILITY` — an
/// arbitrary constant that is at least readable when it turns up in a hex dump.
///
/// **Fixed, and not the run's seed.** The ground is not simulation: it does not
/// replicate, it does not affect play, and a field that redrew itself per run
/// would make every screenshot of this game a different picture for no reason.
const GROUND_SEED: u64 = 0x0000_0047_5241_5353;

/// "The sheet as authored" — no tinting anywhere in this game.
const UNTINTED: [f32; 4] = [1.0; 4];

/// The frame the wizard stands on.
const IDLE_FRAME: &str = "player";

/// The clip `assets/actors.crpix` names the wizard's walk cycle.
///
/// The frames it holds and how long each is held are the sheet's, not this
/// module's: [`Playback`] reads both off the baked sheet, so the `.crpix` is the
/// only place the cycle is written down. What this module pins is that the round
/// trip through the sidecar returned what that file authored —
/// `tests::the_walk_cycle_survives_the_round_trip_through_the_sidecar`.
const WALK_CLIP: &str = "walk";

/// Half the side of the one frame size every actor is drawn at, in world units.
///
/// `EnemyKind::Brute.radius()`, and therefore 34 texels — see this module's
/// header for why a grunt is drawn in a brute-sized quad. It is written here
/// rather than spelled out at the call sites because
/// `tests::every_sprite_covers_the_collider_it_stands_for` asserts the relation
/// against `game.rs`'s radii.
pub const ACTOR_HALF_EXTENT: f64 = 0.85;

/// Half the side of the one frame size every prop is drawn at, in world units.
///
/// `PropKind::Tree.radius()`, and therefore 36 texels — the same relation
/// [`ACTOR_HALF_EXTENT`] has to the brute, for the same reason, and asserted
/// against `game.rs`'s radii by
/// `tests::every_prop_silhouette_is_the_size_of_the_collider_it_stands_for`.
/// A bush is 20 texels of art in that frame and the rest is transparent.
pub const PROP_HALF_EXTENT: f64 = 0.9;

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
///
/// **The one place a role becomes a picture.** [`EnemyKind`]'s variants name
/// what the simulation reasons about — the numerous one, the fast one, the one
/// that will not die — and `assets/actors.crpix`'s frames name what is drawn.
/// Keeping the two vocabularies apart is what let the art become Diablo II's
/// monsters without touching `game.rs`'s spawn table, its weights or the tests
/// that read them; that file argues which monster each role got and why the
/// choice is a consequence of the collider it has to be drawn inside.
const fn enemy_frame(kind: EnemyKind) -> &'static str {
    match kind {
        EnemyKind::Grunt => "fallen",
        EnemyKind::Runner => "quill-rat",
        EnemyKind::Brute => "overlord",
    }
}

/// Which frame of `assets/props.crpix` a prop kind draws.
const fn prop_frame(kind: PropKind) -> &'static str {
    match kind {
        PropKind::Tree => "tree",
        PropKind::Bush => "bush",
    }
}

/// Which of [`Scene::props`] a kind draws from.
const fn prop_index(kind: PropKind) -> usize {
    match kind {
        PropKind::Tree => 0,
        PropKind::Bush => 1,
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
    /// The wizard standing still.
    player: FrameArt,
    /// The sheet the wizard's walk is played out of, and the clip that names it.
    ///
    /// Kept whole rather than flattened to a list of frames, because that is
    /// what [`Playback`] takes: the holds live on the sheet's frames, so the
    /// timing comes from `assets/actors.crpix` through the sidecar and there is
    /// no second copy of it here to disagree.
    actors_desc: Sheet,
    walk: Clip,
    gem: FrameArt,
    bolt: FrameArt,
    /// The grass, indexed by [`ground_variant`]. One sheet, so the whole ground
    /// is a run however the variants fall out.
    grass: [FrameArt; GROUND_VARIANTS],
    /// The trees and bushes, indexed by [`prop_index`].
    props: [FrameArt; 2],
    /// The grass, under everything including the gems.
    ground: Layer,
    /// The scenery, on the grass and under everything that moves.
    props_layer: Layer,
    /// The gems, on the ground and under everything that moves.
    gems: Layer,
    /// The horde.
    crowd: Layer,
    /// The player, over the crowd.
    hero: Layer,
    /// The shots, over the player.
    ///
    /// **Over, where asteroids draws its ship last and its bullets under it.**
    /// Two reasons, and both are load-bearing now: a bolt leaves the head of the
    /// wizard's staff — [`game::STAFF_MUZZLE`](crate::game::STAFF_MUZZLE), which
    /// is *inside* the figure's own box, because the whole figure is drawn to
    /// the collider — so a shot on any layer below this one would spend its
    /// first frames behind the robe; and the player shares the actors sheet, so
    /// putting the shots between the crowd and the player would split the
    /// field's one batch into three.
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
    /// Ground tiles the view asked for.
    ///
    /// Not part of [`SceneStats::field`] and not subject to [`SceneStats::culled`]
    /// — the ground is generated from the view rather than filtered against it,
    /// so it has nothing to reject. It is reported separately because it is in
    /// [`SceneStats::drawn`] and would otherwise look like a horde that stopped
    /// being culled.
    pub ground: usize,
    /// Props the view kept, of the whole scatter.
    ///
    /// Out of [`SceneStats::field`] and [`SceneStats::culled`] for the same
    /// reason [`SceneStats::ground`] is, arrived at from the other side: props
    /// *are* culled, but their total is fixed by the seed and does not move with
    /// the horde, and those two numbers exist to show what does. Counting a
    /// rejected tree beside a rejected enemy would put a constant into the one
    /// figure that has to be read against the field.
    pub props: usize,
    /// Sprites the pass uploads and draws: the ground, plus whatever survived
    /// the cull, plus the player — who is never culled.
    pub drawn: usize,
    /// Draw calls the sprite pass will make for them.
    pub batches: usize,
}

impl crcbl::ui::DebugModule for SceneStats {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("scene");
        section.row("field", format_args!("{}", self.field));
        section.row("culled", format_args!("{}", self.culled));
        section.row("ground", format_args!("{}", self.ground));
        section.row("props", format_args!("{}", self.props));
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
        let props = baked("props", PROPS_PNG, PROPS_JSON);
        let terrain = baked("terrain", TERRAIN_PNG, TERRAIN_JSON);

        let actors_sheet = register(device, sprites, "actors", &actors)?;
        let bolt_sheet = register(device, sprites, "bolt", &bolt)?;
        let props_sheet = register(device, sprites, "props", &props)?;
        let terrain_sheet = register(device, sprites, "terrain", &terrain)?;

        // Back to front, and this is the only place the depth order is written
        // down: `LayerStack` has no depth field to disagree with it. All six
        // take the world's rate — the camera follows the player and there is
        // nothing behind the field to drift. The ground least of all: a parallax
        // factor on it would slide the grass under the player's feet, and one on
        // the props would slide a tree off the ground the player collides with.
        let mut stack = LayerStack::new();
        let ground = stack.push_layer(Parallax::WORLD);
        let props_layer = stack.push_layer(Parallax::WORLD);
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
            player: frame(actors_sheet, &actors.sheet, IDLE_FRAME),
            walk: actors
                .sheet
                .clip(WALK_CLIP)
                .unwrap_or_else(|| panic!("the baked sheet has no clip called {WALK_CLIP}"))
                .clone(),
            actors_desc: actors.sheet.clone(),
            gem: frame(actors_sheet, &actors.sheet, "gem"),
            // By index, not by name: a sheet with no sidecar has one frame
            // synthesised by `load` covering the whole image, and its name is
            // the loader's rather than the `.crpix`'s.
            bolt: still(bolt_sheet, &bolt.sheet),
            grass: GROUND_FRAMES.map(|name| frame(terrain_sheet, &terrain.sheet, name)),
            props: [
                frame(props_sheet, &props.sheet, prop_frame(PropKind::Tree)),
                frame(props_sheet, &props.sheet, prop_frame(PropKind::Bush)),
            ],
            ground,
            props_layer,
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

        // The ground, first and under everything. **Generated, not culled**: the
        // tile lattice is infinite, so the visible tiles are the answer to a
        // range question rather than what is left after rejecting the rest. The
        // range is taken against the *unpadded* view — a tile already covers up
        // to a whole tile past each edge, so there is nothing to grow it by.
        let grass = self.grass;
        let (xs, ys) = ground_tiles(
            camera,
            view_half_width(extent),
            crate::game::VIEW_HALF_HEIGHT,
        );
        let ground_count = xs.clone().count() * ys.clone().count();
        self.stack.extend(
            self.ground,
            ys.flat_map(move |ty| {
                xs.clone().map(move |tx| {
                    let art = grass[ground_variant(tx, ty)];
                    Sprite {
                        sheet: art.sheet,
                        rect: rect(tile_centre(tx, ty), GROUND_TILE / 2.0),
                        // Square, and the same way up in every tile: rotating
                        // the quad would be a fifth variant for free and a
                        // rotated *blade*, which grows up.
                        rotation: 0.0,
                        uv: art.uv,
                        tint: UNTINTED,
                    }
                })
            }),
        );

        // The scenery, culled like the field and not generated like the ground:
        // the scatter is a finite list the simulation owns. The cull box is the
        // actors' one grown to the prop frame, so a tree does not pop in halfway
        // across the edge either.
        let prop_art = self.props;
        let prop_half_x = view_half_width(extent) + PROP_HALF_EXTENT;
        let prop_half_y = crate::game::VIEW_HALF_HEIGHT + PROP_HALF_EXTENT;
        let near = move |p: DVec3| {
            (p.x - camera.x).abs() <= prop_half_x && (p.y - camera.y).abs() <= prop_half_y
        };
        self.stack.extend(
            self.props_layer,
            render
                .props
                .iter()
                .filter(move |prop| near(prop.position))
                .map(move |prop| Sprite {
                    sheet: prop_art[prop_index(prop.kind)].sheet,
                    rect: rect(prop.position, PROP_HALF_EXTENT),
                    // Square quad, round shape: a rotation would be an instance
                    // field spent on a picture nobody could tell had turned.
                    rotation: 0.0,
                    uv: prop_art[prop_index(prop.kind)].uv,
                    tint: UNTINTED,
                }),
        );
        let prop_count = self.stack.sprites(self.props_layer).len();

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
        let mut wizard = self.wizard(render);
        if render.player_facing == Facing::Left {
            wizard.uv = mirrored(wizard.uv);
        }
        self.stack.push(self.hero, actor(wizard, render.player));

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
        let drawn = frame.len();
        self.stats = SceneStats {
            field,
            // What is left of `frame` once the ground, the props and the
            // never-culled player are taken out of it is the part of `field`
            // that survived.
            culled: field + 1 + ground_count + prop_count - drawn,
            ground: ground_count,
            props: prop_count,
            drawn,
            batches: crcbl::render::sprite_pass::batch_count(frame),
        };
        frame
    }

    /// Which frame of the wizard this render state draws.
    ///
    /// The standing frame while the player is not being driven, and the walk
    /// cycle while they are — the whole of "the animation plays when the wizard
    /// walks". Both come out of the same sheet, so neither costs a batch.
    fn wizard(&self, render: &RenderState) -> FrameArt {
        if !render.player_walking {
            return self.player;
        }
        let mut play = Playback::new();
        play.seek(walk_tick(render.elapsed));
        // Both `expect`s are `Sheet::validate`'s rules, which `load` ran on this
        // sheet at start-up: a clip with no frames, or one naming a frame the
        // sheet does not have, never gets this far.
        let index = play
            .frame_index(&self.actors_desc, &self.walk)
            .expect("a validated clip always has a frame to show");
        FrameArt {
            sheet: self.player.sheet,
            uv: self
                .actors_desc
                .uv(index)
                .expect("a clip's frames are the sheet's own"),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure geometry — no device, no sheet ids
// ---------------------------------------------------------------------------

/// The animation clock, in the ticks a `.crpix` hold is counted in.
///
/// **[`RenderState::elapsed`] and not a wall clock.** It is the run's *simulated*
/// seconds, and it is replicated — so a replay of a script animates frame for
/// frame the way the run it replays did, which a clock read off the host would
/// not. It is also what freezes the cycle with the rest of the field on the
/// level-up screen, for free: `run_tick` stops advancing it.
///
/// [`ART_TICK_HZ`] is the rate `build.rs` baked the holds against, so it is the
/// rate that turns those seconds back into the units the holds are in. It is
/// **not** the simulation's tick rate: `--tick-hz 20` changes how often the game
/// thinks, and a walk cycle that slowed down to match would be a different
/// animation on a slower machine.
fn walk_tick(elapsed: f64) -> u64 {
    // `elapsed` never goes backwards within a run and never reaches the range
    // where this saturates; the clamp is what makes the cast total rather than
    // an assumption about the caller.
    (elapsed * f64::from(ART_TICK_HZ)).max(0.0) as u64
}

/// The same frame, mirrored left to right.
///
/// [`Sprite::uv`] is `[u0, v0, u1, v1]` with the **top-left** corner first, and
/// `crates/crcbl-shaders`' `sprite.slang` builds each vertex's `u` as
/// `lerp(uv.x, uv.z, corner.x)` — an unconditional linear interpolation with no
/// clamp and no `saturate` — so swapping the two ends runs the frame backwards
/// across the quad rather than degenerating. The reversed range is a *subset* of
/// the same interval, which is what stops a mirrored actor sampling the frame
/// next to it in the strip. `tests::facing_left_reverses_the_frames_u_range`
/// asserts both halves.
///
/// `v` is untouched: this is a horizontal flip, and swapping it too would stand
/// the wizard on its hat.
const fn mirrored(uv: [f32; 4]) -> [f32; 4] {
    [uv[2], uv[1], uv[0], uv[3]]
}

/// The tiles whose squares meet the view box, as a pair of inclusive ranges
/// over the tile lattice.
///
/// Tile `(tx, ty)` owns `[tx, tx + 1) × [ty, ty + 1)` scaled by [`GROUND_TILE`],
/// so `floor` of each edge is the first and last tile the box touches and the
/// range covers the box with **no gap and at most one tile of overhang** on each
/// side. That is the whole coverage argument: it is a property of `floor` over a
/// uniform lattice rather than of a margin somebody tuned, which is why there is
/// no `+ 1` here to be off by.
fn ground_tiles(
    camera: DVec3,
    half_x: f64,
    half_y: f64,
) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
    let first = |lo: f64| (lo / GROUND_TILE).floor() as i32;
    (
        first(camera.x - half_x)..=first(camera.x + half_x),
        first(camera.y - half_y)..=first(camera.y + half_y),
    )
}

/// The centre of tile `(tx, ty)`, in world units.
fn tile_centre(tx: i32, ty: i32) -> DVec3 {
    DVec3::new(
        (f64::from(tx) + 0.5) * GROUND_TILE,
        (f64::from(ty) + 0.5) * GROUND_TILE,
        0.0,
    )
}

/// Which grass variant tile `(tx, ty)` draws.
///
/// A pure function of the tile's own coordinates, through the workspace's index
/// hash — the one in [`crcbl::core::rand`], which every sample that needed
/// random numbers now shares and which exists precisely so nobody writes a fifth
/// one. Being a function of the coordinates rather than of an emission counter
/// is what makes a tile draw the same grass whichever direction the player walks
/// into it from.
///
/// The two coordinates are packed into one index as a pair of `u32` halves.
/// That is a bijection over every `i32` tile, so two distinct tiles never
/// collide on one index — which `crcbl::core::rand`'s header says is the
/// caller's job to arrange, because it cannot be arranged for callers in
/// general.
///
/// The choice comes off the **top** bits: [`GROUND_VARIANTS`] is a power of two,
/// so a shift is exact where a modulo would need the count to divide 2^64 to
/// avoid bias, and the top of a splitmix64 word is its best-mixed end.
fn ground_variant(tx: i32, ty: i32) -> usize {
    const {
        assert!(
            GROUND_VARIANTS.is_power_of_two(),
            "the variant is a shift off the top of the hash, which needs a power of two",
        );
    }
    let index = (u64::from(tx as u32) << 32) | u64::from(ty as u32);
    let bits = GROUND_VARIANTS.ilog2();
    (crcbl::core::rand::hash_u64(GROUND_SEED, index) >> (u64::BITS - bits)) as usize
}

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
    use crcbl::sprite::{Direction, SampleMode};

    use crate::game::{
        ARENA_HALF_HEIGHT, ARENA_HALF_WIDTH, BOLT_RADIUS, EnemyView, PLAYER_RADIUS, PickupView,
        PropView, STAFF_MUZZLE, VIEW_HALF_HEIGHT, XP_RADIUS,
    };

    /// The wizard's frames: the standing frame, then the walk cycle in the order
    /// [`WALK_CLIP`] plays them.
    const WIZARD_FRAMES: [&str; 5] = [IDLE_FRAME, "walk-a", "walk-b", "walk-c", "walk-d"];

    /// The hold `assets/actors.crpix` authors for each frame of the walk, in
    /// simulation ticks.
    ///
    /// **A checksum, not the source.** Nothing in this module reads it to *play*
    /// the clip — [`Playback`] takes each frame's hold off the baked sheet — so
    /// this is here to be compared against what came back, and
    /// `the_walk_cycle_survives_the_round_trip_through_the_sidecar` is what
    /// compares them. The round trip is `hold → ceil(ms) → floor(ticks)` through
    /// the Aseprite sidecar, and a hold of one survives that at almost any pair
    /// of rates; a hold of several does not, which is the whole reason this
    /// number is worth asserting now that horde has a clip at all.
    const WALK_HOLD_TICKS: u32 = 4;

    /// The colour of the head of the wizard's staff in `assets/actors.crpix`, as
    /// the baked PNG carries it.
    ///
    /// That file gives the orb a palette entry of its own and uses it nowhere
    /// else, so "the texels of this colour" *is* the staff head — which is how
    /// `the_staff_head_is_where_the_muzzle_says_it_is` locates it without
    /// hard-coding where it expects to find it. The second half of that test
    /// asserts the colour really is unique to the wizard, so a repalette that
    /// spent it somewhere else fails loudly rather than dragging the measured
    /// centroid off.
    const STAFF_HEAD_RGBA: [u8; 4] = [0xff, 0xf3, 0xb0, 0xff];

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

    /// One texel's Rec. 601 luma.
    ///
    /// What "how bright does this look" means for eight-bit sRGB without a
    /// colour-management pass, which nothing in this game has.
    fn luma(texel: &[u8]) -> f64 {
        0.299 * f64::from(texel[0]) + 0.587 * f64::from(texel[1]) + 0.114 * f64::from(texel[2])
    }

    /// The luma of every opaque texel of one frame, or of a whole sheet when
    /// `frame` is `None`.
    ///
    /// Transparent texels are dropped rather than counted as black: they are
    /// the margin round a silhouette, and averaging them in would make a small
    /// kind measure darker than a large one drawn in the same colours.
    fn lumas(loaded: &Loaded, frame: Option<&str>) -> Vec<f64> {
        let rect = frame.map(|name| {
            let index = loaded.sheet.frame_index(name).expect("a frame");
            loaded.sheet.frames[index].rect
        });
        let width = loaded.image.width;
        let mut out = Vec::new();
        for y in 0..loaded.image.height {
            for x in 0..width {
                if let Some(r) = rect
                    && !(x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
                {
                    continue;
                }
                let at = ((y * width + x) * 4) as usize;
                if loaded.image.pixels[at + 3] == 0 {
                    continue;
                }
                out.push(luma(&loaded.image.pixels[at..at + 4]));
            }
        }
        out
    }

    /// The mean luma of a frame's rim, and of everything that rim encloses.
    ///
    /// **The rim is found, not named.** It is the boundary of the opaque
    /// region — an opaque texel with a transparent or off-frame
    /// four-neighbour — so this measures whatever outline the art actually has,
    /// rather than trusting a palette entry to still be the one the outline is
    /// drawn in. For a frame drawn out to its own edges, which the brute's is,
    /// the frame border counts as boundary: what is past it is the next frame
    /// in the strip, not more of this shape.
    fn rim_and_body(loaded: &Loaded, name: &str) -> (f64, f64) {
        let index = loaded.sheet.frame_index(name).expect("a frame");
        let rect = loaded.sheet.frames[index].rect;
        let width = loaded.image.width;
        let opaque = |x: u32, y: u32| {
            x >= rect.x
                && x < rect.x + rect.w
                && y >= rect.y
                && y < rect.y + rect.h
                && loaded.image.pixels[((y * width + x) * 4 + 3) as usize] != 0
        };
        let (mut rim, mut body) = (Vec::new(), Vec::new());
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                if !opaque(x, y) {
                    continue;
                }
                // Wrapping is the point at x = 0: the neighbour off the left of
                // the sheet is not opaque, which is what `opaque`'s range check
                // then says.
                let edge = !opaque(x.wrapping_sub(1), y)
                    || !opaque(x + 1, y)
                    || !opaque(x, y.wrapping_sub(1))
                    || !opaque(x, y + 1);
                let at = ((y * width + x) * 4) as usize;
                let value = luma(&loaded.image.pixels[at..at + 4]);
                if edge { &mut rim } else { &mut body }.push(value);
            }
        }
        assert!(!rim.is_empty(), "{name} has no boundary, so it is blank");
        assert!(
            !body.is_empty(),
            "{name} is all boundary, so it has no interior to compare against",
        );
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        (mean(&rim), mean(&body))
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
        // The wizard's five frames, the three enemy kinds and the gem.
        let frames = WIZARD_FRAMES.len() + EnemyKind::ALL.len() + 1;
        assert_eq!(actors.sheet.frames.len(), frames);
        assert_eq!(
            (actors.image.width, actors.image.height),
            (frames as u32 * side, side),
            "the sheet is not {frames} {side}-texel frames in a strip",
        );
        for frame in &actors.sheet.frames {
            assert_eq!(
                (frame.rect.w, frame.rect.h),
                (side, side),
                "{}: a .crpix has one frame size for the whole file",
                frame.name,
            );
        }
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
        for frame in &actors.sheet.frames {
            let (w, h) = silhouette(&actors, &frame.name);
            let name = &frame.name;
            assert!(w > 1 && h > 1, "{name} is a dot: {w} x {h}");
            assert!(w <= side && h <= side, "{name} overflows its frame");
        }

        // …and the wizard's five are five different pictures. Five copies of one
        // pose would bake, load, index and play exactly as a cycle does, and
        // every assertion in this file passes on them — which would make the
        // walk a slideshow of one drawing.
        let mut poses: Vec<Vec<u8>> = WIZARD_FRAMES
            .iter()
            .map(|name| {
                let index = actors.sheet.frame_index(name).expect("a wizard frame");
                let rect = actors.sheet.frames[index].rect;
                (rect.y..rect.y + rect.h)
                    .flat_map(|y| {
                        let row = (y * actors.image.width + rect.x) as usize * 4;
                        actors.image.pixels[row..row + rect.w as usize * 4].to_vec()
                    })
                    .collect()
            })
            .collect();
        poses.sort_unstable();
        poses.dedup();
        assert_eq!(
            poses.len(),
            WIZARD_FRAMES.len(),
            "two of the wizard's frames are the same picture",
        );
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

    /// **The ground bakes to four opaque tiles of the size the code lays.**
    ///
    /// The frame size is the tile size — a `.crpix` has one for the whole file —
    /// so this is what stops `assets/terrain.crpix` and [`GROUND_TILE`] drifting
    /// apart into a lattice of stretched or squeezed grass. The opacity half is
    /// the ground's own contract: it is the bottom layer, so a transparent texel
    /// is [`GROUND`] showing through a lawn.
    #[test]
    fn the_ground_bakes_to_four_opaque_tiles() {
        let terrain = baked("terrain", TERRAIN_PNG, TERRAIN_JSON);
        let side = GROUND_TILE * f64::from(TEXELS_PER_UNIT);
        assert_eq!(
            side,
            side.round(),
            "the tile is not a whole number of texels at {TEXELS_PER_UNIT} a unit",
        );
        let side = side as u32;

        assert_eq!(terrain.sheet.frames.len(), GROUND_VARIANTS);
        assert_eq!(
            (terrain.image.width, terrain.image.height),
            (GROUND_VARIANTS as u32 * side, side),
            "the sheet is not {GROUND_VARIANTS} {side}-texel frames in a strip",
        );
        for name in GROUND_FRAMES {
            let index = terrain
                .sheet
                .frame_index(name)
                .unwrap_or_else(|| panic!("the terrain sheet has no frame {name}"));
            let rect = terrain.sheet.frames[index].rect;
            assert_eq!((rect.w, rect.h), (side, side), "{name}");
        }
        assert!(
            terrain.sheet.clips.is_empty(),
            "the ground does not animate"
        );
        assert_eq!(terrain.sheet.nine, None);
        assert_eq!(terrain.sheet.sample, SampleMode::Pixel);

        let clear = terrain
            .image
            .pixels
            .chunks_exact(4)
            .filter(|p| p[3] != u8::MAX)
            .count();
        assert_eq!(
            clear,
            0,
            "{clear} of {} ground texels are not fully opaque, so the clear \
             colour shows through the grass",
            terrain.image.pixels.len() / 4,
        );

        // Four *different* tiles. Four copies of one would tile, bake, load and
        // draw exactly as four variants do, and every assertion above passes on
        // them — which would make `ground_variant` a shuffle of one stamp.
        let mut frames: Vec<Vec<u8>> = (0..GROUND_VARIANTS)
            .map(|i| {
                let rect = terrain.sheet.frames[i].rect;
                (rect.y..rect.y + rect.h)
                    .flat_map(|y| {
                        let row = (y * terrain.image.width + rect.x) as usize * 4;
                        terrain.image.pixels[row..row + rect.w as usize * 4].to_vec()
                    })
                    .collect()
            })
            .collect();
        frames.sort_unstable();
        frames.dedup();
        assert_eq!(
            frames.len(),
            GROUND_VARIANTS,
            "two tiles are the same picture"
        );
    }

    /// **The props bake to two frames of the size the code draws them at**, and
    /// each silhouette is the collider the player will actually be stopped by.
    ///
    /// Two claims in one test because neither is worth anything alone. The frame
    /// size is 36 × 36 by construction, so a sheet of blank squares satisfies
    /// every assertion about the *frame*; and a silhouette measured without
    /// pinning the frame it sits in says nothing about the quad the scene emits.
    /// The sizes are taken off `game.rs`'s radii rather than repeated here, so a
    /// prop that changed size fails this instead of quietly colliding at a size
    /// it is not drawn at.
    #[test]
    fn every_prop_silhouette_is_the_size_of_the_collider_it_stands_for() {
        let props = baked("props", PROPS_PNG, PROPS_JSON);
        let scale = f64::from(TEXELS_PER_UNIT);

        let side = (2.0 * PROP_HALF_EXTENT * scale).round() as u32;
        assert_eq!(side, 36);
        assert!(
            (f64::from(side) - 2.0 * PROP_HALF_EXTENT * scale).abs() < 1e-9,
            "the shared prop frame does not land on a whole texel",
        );
        assert_eq!(props.sheet.frames.len(), PropKind::ALL.len());
        assert_eq!(
            (props.image.width, props.image.height),
            (PropKind::ALL.len() as u32 * side, side),
            "the sheet is not {} {side}-texel frames in a strip",
            PropKind::ALL.len(),
        );
        assert!(props.sheet.clips.is_empty(), "nothing here animates");
        assert_eq!(props.sheet.nine, None, "nothing here is stretched");
        assert_eq!(props.sheet.sample, SampleMode::Pixel);

        for kind in PropKind::ALL {
            let name = prop_frame(kind);
            let want = (2.0 * kind.radius() * scale).ceil() as u32;
            assert_eq!(
                silhouette(&props, name),
                (want, want),
                "{name} is not drawn to the collider the player is stopped by",
            );
        }
        // The tree is the one that fills its frame exactly — the frame size is
        // its collider — so the transparent margin is entirely the bush's.
        assert_eq!(
            silhouette(&props, prop_frame(PropKind::Tree)),
            (side, side),
            "the frame is no longer the tree's own size",
        );
        let clear = props
            .image
            .pixels
            .chunks_exact(4)
            .filter(|p| p[3] == 0)
            .count();
        assert!(
            clear > 0 && clear < (props.image.pixels.len() / 4),
            "the props sheet is {clear} clear of {} texels",
            props.image.pixels.len() / 4,
        );
    }

    /// **A prop reads against the grass and never against the crowd.**
    ///
    /// The same pair of bounds `assets/actors.crpix` puts the monsters between,
    /// one shelf lower. Below is the ground: scenery inside the grass's own band
    /// is scenery the player walks into. Above is the horde: there are tens of
    /// props against hundreds of monsters, and a static obstacle that out-shines
    /// a moving one pulls the eye to the thing that cannot kill you.
    ///
    /// Both are relations between baked sheets rather than numbers written here,
    /// so a repalette of either side moves this instead of leaving
    /// `assets/props.crpix`'s paragraph quietly wrong — and stating both is what
    /// stops either being vacuous, since the lower bound alone is met by white
    /// trees and the upper alone by black ones.
    #[test]
    fn the_props_sit_between_the_grass_and_the_crowd_in_luma() {
        let props = baked("props", PROPS_PNG, PROPS_JSON);
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let terrain = baked("terrain", TERRAIN_PNG, TERRAIN_JSON);
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;

        // The *brightest* grass texel, not its average: a prop has to clear the
        // lit tip of a blade, which is what it stands on.
        let grass_hi = lumas(&terrain, None)
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        // The dimmest monster's *average*, because the claim is about which of
        // two masses on screen is brighter, not about either one's highlights.
        let crowd_lo = EnemyKind::ALL
            .iter()
            .map(|kind| mean(&lumas(&actors, Some(enemy_frame(*kind)))))
            .fold(f64::MAX, f64::min);
        assert!(grass_hi < crowd_lo, "there is no band to sit in");

        for kind in PropKind::ALL {
            let name = prop_frame(kind);
            let average = mean(&lumas(&props, Some(name)));
            assert!(
                average > grass_hi,
                "{name} averages {average:.1} and the brightest grass texel is \
                 {grass_hi:.1}, so the ground swallows it",
            );
            assert!(
                average < crowd_lo,
                "{name} averages {average:.1} against the dimmest monster's \
                 {crowd_lo:.1}, so the scenery is brighter than the horde",
            );
            // And it is rimmed like a monster rather than like the player: the
            // wizard's bright edge is only findable while nothing else has one.
            let (rim, body) = rim_and_body(&props, name);
            assert!(
                rim < body,
                "{name} is outlined at {rim:.1} against {body:.1} inside it, so \
                 a tree has the edge the player is supposed to have",
            );
        }
    }

    /// **The grass is darker and flatter than what stands on it.**
    ///
    /// `assets/actors.crpix` gives the player a bright rim so it can be found in
    /// a crowd of hundreds at the same size, and losing the player is the one
    /// unplayable bug this genre has. A ground with brightness or contrast of
    /// its own is what takes that away — so both are measured against the player
    /// frame rather than against numbers written down here, and a repalette that
    /// brightened the grass moves this rather than leaving the doc wrong.
    #[test]
    fn the_grass_is_dimmer_and_flatter_than_the_player_it_carries() {
        let terrain = baked("terrain", TERRAIN_PNG, TERRAIN_JSON);
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);

        let span = |v: &[f64]| {
            let lo = v.iter().copied().fold(f64::MAX, f64::min);
            let hi = v.iter().copied().fold(f64::MIN, f64::max);
            (lo, hi, v.iter().sum::<f64>() / v.len() as f64)
        };

        let (grass_lo, grass_hi, grass_mean) = span(&lumas(&terrain, None));
        let (player_lo, player_hi, player_mean) = span(&lumas(&actors, Some(IDLE_FRAME)));

        assert!(
            grass_hi < player_hi / 3.0,
            "the brightest grass texel is {grass_hi:.1} against the player's \
             {player_hi:.1}: a ground that bright competes with the rim",
        );
        assert!(
            grass_hi - grass_lo < (player_hi - player_lo) / 3.0,
            "the grass spans {:.1} of luma and the player {:.1}: a ground with \
             that much contrast is busy under a crowd",
            grass_hi - grass_lo,
            player_hi - player_lo,
        );
        assert!(
            grass_mean * 3.0 < player_mean,
            "the average grass texel is {grass_mean:.1} against the player's \
             {player_mean:.1}",
        );
        // …and it is not simply black, which would satisfy all three and would
        // be a ground nobody can see is there.
        assert!(grass_lo > 8.0, "the darkest grass texel is {grass_lo:.1}");
    }

    /// **A monster reads against the grass and never against the wizard.**
    ///
    /// Two bounds pulling opposite ways, and the crowd has to sit between them.
    /// Below is the ground: `assets/terrain.crpix` is deliberately dark and
    /// flat, and a monster inside that band is one the player walks into rather
    /// than one they saw coming. Above is the player, whose bright rim —
    /// `the_monsters_have_a_dark_rim_and_the_player_a_bright_one` — is only
    /// findable while nothing else on screen is as bright, and there are
    /// hundreds of monsters against the one wizard.
    ///
    /// Both are stated as relations between two baked sheets rather than as
    /// numbers written down here, so a repalette of either side moves this
    /// instead of leaving `assets/actors.crpix`'s paragraph quietly wrong. The
    /// pair is also what stops either half being vacuous: the lower bound alone
    /// is satisfied by painting the horde white, and the upper alone by
    /// painting it black.
    #[test]
    fn the_monsters_sit_between_the_grass_and_the_player_in_luma() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let terrain = baked("terrain", TERRAIN_PNG, TERRAIN_JSON);
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;

        // The *brightest* grass texel, not its average: a monster has to clear
        // the lit tip of a blade, which is what it will be standing on.
        let grass_hi = lumas(&terrain, None)
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        // The player's *average*, not its brightest: the rim is a thin edge, so
        // comparing against it would let a monster be brighter than the whole
        // of the figure it must not compete with.
        let player = mean(&lumas(&actors, Some(IDLE_FRAME)));

        for kind in EnemyKind::ALL {
            let name = enemy_frame(kind);
            let texels = lumas(&actors, Some(name));
            let (average, brightest) = (
                mean(&texels),
                texels.iter().copied().fold(f64::MIN, f64::max),
            );
            assert!(
                average > grass_hi,
                "{name} averages {average:.1} and the brightest grass texel is \
                 {grass_hi:.1}, so the ground swallows it",
            );
            assert!(
                brightest < player,
                "{name}'s brightest texel is {brightest:.1} against the \
                 player's average {player:.1}, so a crowd of these competes \
                 with the one figure that has to be found",
            );
        }
    }

    /// **The monsters have a dark rim and the player a bright one**, which is
    /// what lets a player find themselves among hundreds of bodies drawn at the
    /// same size.
    ///
    /// `assets/actors.crpix` has stated this as a rule since the wizard was
    /// drawn, and a rule in a comment is worth what one is worth: the outline is
    /// a palette choice repeated in every row of every frame, and nothing else
    /// notices when one of them changes. So each silhouette's boundary is found
    /// and its brightness compared with the interior it encloses.
    ///
    /// **The direction is the assertion.** A test that only checked the two
    /// differ passes on a monster outlined in white, which is the exact failure
    /// this exists for.
    #[test]
    fn the_monsters_have_a_dark_rim_and_the_player_a_bright_one() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);

        for kind in EnemyKind::ALL {
            let name = enemy_frame(kind);
            let (rim, body) = rim_and_body(&actors, name);
            assert!(
                rim < body,
                "{name} is outlined at {rim:.1} against {body:.1} inside it, so \
                 it is rimmed lighter than it is filled",
            );
        }
        // Every frame of the wizard, not only the standing one: each pose is
        // drawn by hand, so the walk is where an outline gets lost.
        for name in WIZARD_FRAMES {
            let (rim, body) = rim_and_body(&actors, name);
            assert!(
                rim > body,
                "{name} is outlined at {rim:.1} against {body:.1} inside it, so \
                 the player has stopped being the one bright edge on screen",
            );
        }
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
        // Every frame of the wizard, not only the standing one: the walk moves
        // the figure inside its box — the body rises, the feet step, the hem
        // swings — and a frame that grew past the collider while doing it would
        // be a player who is bigger than they collide on one tick in four.
        for name in WIZARD_FRAMES {
            assert_eq!(
                silhouette(&actors, name),
                (want(PLAYER_RADIUS), want(PLAYER_RADIUS)),
                "{name} is not drawn to the player's collider",
            );
        }
        assert_eq!(
            silhouette(&actors, "gem"),
            (want(XP_RADIUS), want(XP_RADIUS))
        );
    }

    /// **The walk's holds came back the length `assets/actors.crpix` authored.**
    ///
    /// A `.crpix` counts a hold in simulation ticks and an Aseprite sidecar
    /// counts milliseconds, so every hold makes a `ceil` out to milliseconds and
    /// a `floor` back — and the two conversions have to use the same rate or
    /// every animation in the game plays at the wrong speed. This is the guard
    /// on that, and until horde had a clip it could not exist: the default hold
    /// of one tick survives a fairly wide range of wrong arithmetic, which is
    /// what `apps/horde/build.rs` used to say and no longer does.
    ///
    /// [`WALK_HOLD_TICKS`] is the only place this module writes the number down,
    /// and it is not what plays the clip — see that constant.
    #[test]
    fn the_walk_cycle_survives_the_round_trip_through_the_sidecar() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        assert_eq!(
            actors.sheet.clips.len(),
            1,
            "the wizard's walk is the only clip in this game",
        );
        let clip = actors
            .sheet
            .clip(WALK_CLIP)
            .expect("the sheet declares the walk");
        assert_eq!(clip.direction, Direction::Forward);
        assert!(clip.looping, "a walk that stopped would be a stumble");

        let walk = &WIZARD_FRAMES[1..];
        let named: Vec<&str> = clip
            .frames
            .iter()
            .map(|index| actors.sheet.frames[*index].name.as_str())
            .collect();
        assert_eq!(named, walk, "the clip does not play the frames it should");

        for frame in &actors.sheet.frames {
            let want = if walk.contains(&frame.name.as_str()) {
                WALK_HOLD_TICKS
            } else {
                // A frame no clip names keeps the parser's default, and a still
                // sprite held for anything else would be a hold that survived
                // the round trip in one direction only.
                1
            };
            assert_eq!(
                frame.hold, want,
                "{}: authored at {want} ticks and came back at {}, so bake and \
                 load disagree about milliseconds",
                frame.name, frame.hold,
            );
        }

        assert_eq!(
            actors.sheet.clip_duration(clip),
            Some(u64::from(WALK_HOLD_TICKS) * walk.len() as u64),
            "the cycle is not the sum of its holds",
        );
    }

    /// **A bolt leaves the staff the art actually draws.**
    ///
    /// [`STAFF_MUZZLE`] is where `game.rs` starts a shot, and it is a point on a
    /// picture — so it is measured off the baked bytes rather than trusted. The
    /// orb has a palette entry of its own in `assets/actors.crpix`, so the check
    /// is exact: find every texel of that colour, take the centroid, and it must
    /// land on the muzzle to the texel, in **every** frame of the wizard.
    ///
    /// That last part is the animation's constraint. The staff is the one thing
    /// the walk does not move, precisely so the muzzle can be a constant; a walk
    /// frame that nudged the orb would put a shot somewhere else one tick in
    /// four, and this is what says it has not.
    #[test]
    fn the_staff_head_is_where_the_muzzle_says_it_is() {
        let actors = baked("actors", ACTORS_PNG, ACTORS_JSON);
        let width = actors.image.width;

        let mut measured = Vec::new();
        let mut elsewhere = 0usize;
        for frame in &actors.sheet.frames {
            let rect = frame.rect;
            let found: Vec<(u32, u32)> = (rect.y..rect.y + rect.h)
                .flat_map(|y| (rect.x..rect.x + rect.w).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let at = ((y * width + x) * 4) as usize;
                    actors.image.pixels[at..at + 4] == STAFF_HEAD_RGBA
                })
                .collect();
            if !WIZARD_FRAMES.contains(&frame.name.as_str()) {
                elsewhere += found.len();
                continue;
            }
            assert!(
                !found.is_empty(),
                "{}: no staff head, so the wizard has nothing to fire from",
                frame.name,
            );
            let count = found.len() as f64;
            // Texel centres, so an even-sized orb lands on a texel boundary and
            // the arithmetic below is exact rather than nearly.
            let cx = found.iter().map(|(x, _)| f64::from(*x) + 0.5).sum::<f64>() / count;
            let cy = found.iter().map(|(_, y)| f64::from(*y) + 0.5).sum::<f64>() / count;
            // Relative to the frame's own centre. Image rows run down and the
            // world's +Y runs up, so the vertical one is subtracted the other
            // way round.
            measured.push((
                frame.name.clone(),
                cx - (f64::from(rect.x) + f64::from(rect.w) / 2.0),
                (f64::from(rect.y) + f64::from(rect.h) / 2.0) - cy,
            ));
        }

        assert_eq!(
            elsewhere, 0,
            "the staff head's colour is used outside the wizard, so it no \
             longer identifies the orb",
        );
        assert_eq!(measured.len(), WIZARD_FRAMES.len());

        let scale = f64::from(TEXELS_PER_UNIT);
        let (want_x, want_y) = (STAFF_MUZZLE.x * scale, STAFF_MUZZLE.y * scale);
        for (name, dx, dy) in &measured {
            assert!(
                (dx - want_x).abs() < 1e-9 && (dy - want_y).abs() < 1e-9,
                "{name}: the staff head is {dx}, {dy} texels from the centre of \
                 its frame and game::STAFF_MUZZLE says {want_x}, {want_y}",
            );
        }
    }

    /// **Facing left is the same frame with its `u` range reversed**, and the
    /// reversal is asserted rather than the mere fact that something changed.
    ///
    /// A test that only checked the two `u`s differ cannot tell a flip from a
    /// crop or from the frame next door. So: the rectangle is the *same* four
    /// numbers with the ends swapped, the reversed range really does run
    /// backwards, and — the half that matters on a sheet laid out as a strip —
    /// every point the shader will sample stays inside the frame's own interval,
    /// so a mirrored wizard cannot sample the frame beside it.
    ///
    /// The sampling half reproduces `sprite.slang`'s vertex rule, which is a
    /// copy of it and is worth what a copy is worth: what it catches is this
    /// module handing the shader something the rule would degenerate on. See
    /// [`mirrored`] for the reading of the shader itself.
    #[test]
    fn facing_left_reverses_the_frames_u_range() {
        with_scene(|scene| {
            let hero = |scene: &mut Scene, facing| {
                scene.build(
                    &RenderState {
                        player: DVec3::ZERO,
                        player_facing: facing,
                        ..RenderState::default()
                    },
                    EXTENT,
                );
                scene.stack.sprites(scene.hero)[0]
            };
            let right = hero(scene, Facing::Right);
            let left = hero(scene, Facing::Left);

            assert_eq!(right.uv, scene.player.uv, "the frame as authored");
            assert_eq!(
                left.uv,
                [right.uv[2], right.uv[1], right.uv[0], right.uv[3]],
                "a horizontal flip swaps u and leaves v alone",
            );
            assert!(
                left.uv[0] > left.uv[2],
                "the mirrored range does not run backwards: {:?}",
                left.uv,
            );
            assert_eq!(left.rect, right.rect, "a flip is not a move");

            // `lerp(uv.x, uv.z, corner.x)`, over the two corners the quad has.
            let sample = |uv: [f32; 4], corner: f32| uv[0] + (uv[2] - uv[0]) * corner;
            assert_eq!(
                sample(left.uv, 0.0),
                sample(right.uv, 1.0),
                "the left edge of a mirrored quad must show the frame's right edge",
            );
            assert_eq!(sample(left.uv, 1.0), sample(right.uv, 0.0));
            for corner in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let u = sample(left.uv, corner);
                assert!(
                    u >= right.uv[0] && u <= right.uv[2],
                    "a mirrored quad samples u = {u}, outside the frame's \
                     {}..{}",
                    right.uv[0],
                    right.uv[2],
                );
            }
            // …and there really is something next door to sample by mistake.
            assert!(
                right.uv[2] < 1.0,
                "the wizard is the whole sheet, so staying inside its frame is \
                 not a claim about anything",
            );
        });
    }

    /// **The walk plays while the player is walking and not while they are
    /// standing**, and it is the frame that is asserted to move, not the clock.
    ///
    /// The trap this is written against: a test that reads the frame at
    /// `elapsed = 0.0` passes identically whether the clip advances or is frozen
    /// solid. So every frame of the cycle is asked for by name, they are checked
    /// to be four *different* frames, the cycle is checked to come back to its
    /// first frame after exactly one period, and the boundary between two holds
    /// is checked from both sides.
    #[test]
    fn the_walk_cycle_plays_while_walking_and_holds_still_while_stopped() {
        with_scene(|scene| {
            let uv_at = |scene: &mut Scene, walking: bool, elapsed: f64| {
                scene.build(
                    &RenderState {
                        player: DVec3::ZERO,
                        player_walking: walking,
                        elapsed,
                        ..RenderState::default()
                    },
                    EXTENT,
                );
                scene.stack.sprites(scene.hero)[0].uv.map(f32::to_bits)
            };
            let steps = WIZARD_FRAMES.len() - 1;
            let hold = f64::from(WALK_HOLD_TICKS) / f64::from(ART_TICK_HZ);
            let cycle = hold * steps as f64;

            // Standing still is one frame, whatever the clock says.
            let idle = uv_at(scene, false, 0.0);
            assert_eq!(idle, scene.player.uv.map(f32::to_bits));
            assert_eq!(
                idle,
                uv_at(scene, false, cycle * 3.7),
                "the standing wizard animated",
            );

            // Walking is a different frame each hold, all of them, none of them
            // the standing frame.
            let seen: Vec<[u32; 4]> = (0..steps)
                .map(|step| uv_at(scene, true, (step as f64 + 0.5) * hold))
                .collect();
            assert_ne!(seen[0], seen[1], "the cycle never left its first frame");
            let mut distinct = seen.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(
                distinct.len(),
                steps,
                "the cycle showed {} of its {steps} frames",
                distinct.len(),
            );
            assert!(
                !distinct.contains(&idle),
                "a walk frame is the standing frame, so stopping would not show",
            );

            // …and it comes back, which is what makes it a cycle rather than a
            // sequence that ran off the end and parked.
            assert_eq!(
                uv_at(scene, true, cycle + 0.5 * hold),
                seen[0],
                "a full cycle did not return to the first frame",
            );

            // The boundary between two holds, from both sides: half a tick short
            // of it is still the first frame and half a tick past it is not.
            let half_tick = 0.5 / f64::from(ART_TICK_HZ);
            assert_eq!(uv_at(scene, true, hold - half_tick), seen[0]);
            assert_eq!(uv_at(scene, true, hold + half_tick), seen[1]);
        });
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

    /// **The ground covers the whole view, with no gap and no hole**, at every
    /// window shape and wherever the player is standing.
    ///
    /// The failure this exists for is a strip of [`GROUND`] along an edge, which
    /// is one `floor` away and looks like a rendering bug rather than an
    /// off-by-one. Coverage is asserted two ways, because either alone is weak:
    /// the tiles must reach past all four edges of the view box, *and* they must
    /// form a complete rectangular lattice at exactly [`GROUND_TILE`] pitch —
    /// a bounding box says nothing about a missing tile in the middle, and a
    /// lattice says nothing about where it sits.
    #[test]
    fn the_ground_covers_the_view_with_no_gap() {
        with_scene(|scene| {
            let scale = f64::from(TEXELS_PER_UNIT);
            let pitch = GROUND_TILE * scale;
            for extent in [(960, 720), (1920, 1080), (600, 900), (4000, 400)] {
                for player in [
                    DVec3::ZERO,
                    DVec3::new(0.37, -0.11, 0.0),
                    DVec3::new(ARENA_HALF_WIDTH, ARENA_HALF_HEIGHT, 0.0),
                    DVec3::new(-ARENA_HALF_WIDTH, -ARENA_HALF_HEIGHT, 0.0),
                    DVec3::new(-13.5, 21.75, 0.0),
                ] {
                    let render = RenderState {
                        player,
                        ..RenderState::default()
                    };
                    scene.build(&render, extent);
                    let tiles = scene.stack.sprites(scene.ground).to_vec();
                    assert!(
                        !tiles.is_empty(),
                        "{extent:?} at {player:?}: the ground emitted nothing, so \
                         everything below is vacuous",
                    );

                    // Every tile is a tile, or "the lattice covers the view" is
                    // a claim about rectangles of some other size.
                    for tile in &tiles {
                        assert_eq!(
                            (f64::from(tile.rect[2]), f64::from(tile.rect[3])),
                            (pitch, pitch),
                            "{extent:?} at {player:?}: a ground quad is not one tile",
                        );
                    }

                    // A complete lattice: as many tiles as distinct columns
                    // times distinct rows, and each axis a contiguous run.
                    let axis = |i: usize| {
                        let mut v: Vec<i64> = tiles
                            .iter()
                            .map(|t| (f64::from(t.rect[i]) / pitch).round() as i64)
                            .collect();
                        v.sort_unstable();
                        v.dedup();
                        v
                    };
                    let (cols, rows) = (axis(0), axis(1));
                    assert_eq!(
                        tiles.len(),
                        cols.len() * rows.len(),
                        "{extent:?} at {player:?}: {} tiles over {} x {} lattice \
                         positions — one is missing or doubled",
                        tiles.len(),
                        cols.len(),
                        rows.len(),
                    );
                    for run in [&cols, &rows] {
                        assert!(
                            run.windows(2).all(|w| w[1] - w[0] == 1),
                            "{extent:?} at {player:?}: the lattice has a gap: {run:?}",
                        );
                    }

                    // …and it is in the right place: the view box, in sprite
                    // units, through the same two functions the projection uses.
                    let camera = camera_centre(player, extent);
                    let (half_x, half_y) = (view_half_width(extent), VIEW_HALF_HEIGHT);
                    let covers = |lo: f64, hi: f64, want_lo: f64, want_hi: f64, what: &str| {
                        assert!(
                            lo <= want_lo && hi >= want_hi,
                            "{extent:?} at {player:?}: the ground reaches \
                             {lo}..{hi} on {what} and the view needs \
                             {want_lo}..{want_hi}",
                        );
                    };
                    let bound = |i: usize| {
                        let lo = tiles
                            .iter()
                            .map(|t| f64::from(t.rect[i]))
                            .fold(f64::MAX, f64::min);
                        (
                            lo,
                            lo + (if i == 0 { cols.len() } else { rows.len() }) as f64 * pitch,
                        )
                    };
                    let (lo, hi) = bound(0);
                    covers(
                        lo,
                        hi,
                        (camera.x - half_x) * scale,
                        (camera.x + half_x) * scale,
                        "x",
                    );
                    let (lo, hi) = bound(1);
                    covers(
                        lo,
                        hi,
                        (camera.y - half_y) * scale,
                        (camera.y + half_y) * scale,
                        "y",
                    );

                    assert_eq!(scene.stats().ground, tiles.len(), "the stat disagrees");
                }
            }
        });
    }

    /// **A tile's grass is a function of the tile, not of the camera or the
    /// frame count.**
    ///
    /// The property that makes the ground a *place*: walk away and back and the
    /// same patch is there. A variant drawn from an emission counter — the
    /// obvious way to write this — passes every coverage assertion above and
    /// makes the whole field shimmer the moment the player moves.
    ///
    /// The last assertion is the anti-vacuous half: a constant function is
    /// perfectly deterministic, so the tiles in one view have to actually differ.
    #[test]
    fn a_tiles_grass_is_a_function_of_the_tile_and_not_of_the_camera() {
        with_scene(|scene| {
            let pitch = GROUND_TILE * f64::from(TEXELS_PER_UNIT);
            let key = |scene: &Scene| {
                scene
                    .stack
                    .sprites(scene.ground)
                    .iter()
                    .map(|t| {
                        (
                            (f64::from(t.rect[0]) / pitch).round() as i64,
                            (f64::from(t.rect[1]) / pitch).round() as i64,
                            t.uv.map(f32::to_bits),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let at = |scene: &mut Scene, player: DVec3| {
                scene.build(
                    &RenderState {
                        player,
                        ..RenderState::default()
                    },
                    EXTENT,
                );
                key(scene)
            };

            let home = at(scene, DVec3::ZERO);
            // Deliberately not a whole number of tiles, and back past the start,
            // so the two views overlap in a phase a lattice-relative variant
            // would get wrong.
            let away = at(scene, DVec3::new(7.3, -5.9, 0.0));
            let back = at(scene, DVec3::ZERO);
            assert_eq!(home, back, "the same camera gave two different grounds");

            let shared: Vec<_> = home
                .iter()
                .filter(|(x, y, _)| away.iter().any(|(ax, ay, _)| ax == x && ay == y))
                .collect();
            assert!(
                shared.len() > 50,
                "the two views only share {} tiles, which is not a test",
                shared.len(),
            );
            for (x, y, uv) in shared {
                let other = away
                    .iter()
                    .find(|(ax, ay, _)| ax == x && ay == y)
                    .expect("just filtered on it");
                assert_eq!(
                    &other.2, uv,
                    "tile ({x}, {y}) changed its grass when the camera moved",
                );
            }

            // …and the ground is not one stamp: every variant the sheet holds
            // turns up in a single view.
            let mut seen: Vec<[u32; 4]> = home.iter().map(|(_, _, uv)| *uv).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(
                seen.len(),
                GROUND_VARIANTS,
                "one view of the ground uses {} of {GROUND_VARIANTS} variants",
                seen.len(),
            );
        });
    }

    /// **The ground is bounded by the view, not by the arena** — the same claim
    /// the cull makes for the horde, arrived at differently: the tiles are
    /// *generated* from the view box rather than filtered against it.
    ///
    /// A ground laid over the whole arena would look identical on screen and
    /// hand the pass the arena's worth of quads every frame, forever.
    #[test]
    fn the_visible_ground_is_bounded_by_the_view_and_not_the_arena() {
        with_scene(|scene| {
            let arena_tiles = ((2.0 * ARENA_HALF_WIDTH / GROUND_TILE).ceil()
                * (2.0 * ARENA_HALF_HEIGHT / GROUND_TILE).ceil())
                as usize;

            let count = |scene: &mut Scene, player: DVec3, extent: (u32, u32)| {
                scene.build(
                    &RenderState {
                        player,
                        ..RenderState::default()
                    },
                    extent,
                );
                scene.stats().ground
            };

            let middle = count(scene, DVec3::ZERO, EXTENT);
            let corner = count(
                scene,
                DVec3::new(ARENA_HALF_WIDTH, ARENA_HALF_HEIGHT, 0.0),
                EXTENT,
            );
            assert!(middle > 0, "the ground emitted nothing");
            assert!(
                middle < arena_tiles / 4,
                "{middle} tiles for a view of a {arena_tiles}-tile arena",
            );
            // The two numbers `assets/terrain.crpix` argues 2.0 units from,
            // pinned rather than described — a change to the view, the arena or
            // the tile takes that file's reasoning down with it here.
            assert_eq!(middle, 300, "the tiles a {EXTENT:?} window asks for moved");
            assert_eq!(arena_tiles, 1_728, "the arena is no longer 48 x 36 tiles");
            // The lattice phase can add a row and a column, and no more: a
            // ground that grew towards the wall is one laid over the arena.
            assert!(
                corner <= middle + (middle / 10),
                "standing in a corner asked for {corner} tiles against {middle} \
                 in the middle",
            );

            // …and it really does follow the *window*, or "bounded by the view"
            // is satisfied by a constant. Wider, not taller: `gpu.rs` fixes the
            // vertical extent and lets the horizontal one grow, so a taller
            // window is a *narrower* view and would move this the wrong way.
            let wide = count(scene, DVec3::ZERO, (EXTENT.0 * 2, EXTENT.1));
            assert!(
                wide > middle,
                "a window twice as wide asked for {wide} tiles against {middle}",
            );
        });
    }

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

    /// **The whole field is four batches, whatever order it comes in.**
    ///
    /// The claim this module's sheet split exists for, and the one the scale
    /// sub-slice measured: a `SpriteRenderer` batch is a run of consecutive
    /// sprites naming one sheet, so an interleaved field of every kind must
    /// still resolve to the terrain sheet, then the props sheet, then the actors
    /// sheet, then the bolt sheet — four runs, not one per enemy and not one per
    /// ground tile.
    #[test]
    fn an_interleaved_field_of_every_kind_is_four_batches() {
        with_scene(|scene| {
            let kinds = [EnemyKind::Brute, EnemyKind::Grunt, EnemyKind::Runner];
            let render = RenderState {
                player: DVec3::ZERO,
                // Interleaved kinds here too: the props are two frames of one
                // sheet, so the order they come in must not matter either.
                props: (0..8)
                    .map(|i| PropView {
                        position: DVec3::new(i as f64 * 1.5 - 6.0, 6.0, 0.0),
                        kind: PropKind::ALL[i % 2],
                    })
                    .collect(),
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
            let ground = scene.stats().ground;
            assert!(ground > 0, "the ground layer emitted nothing");
            assert_eq!(scene.stats().props, 8, "a prop was culled by accident");
            assert_eq!(frame.len(), ground + 8 + 6 + 24 + 1 + 3);

            let runs = 1 + frame
                .windows(2)
                .filter(|w| w[0].sheet != w[1].sheet)
                .count();
            assert_eq!(
                runs,
                4,
                "{} sprites in four kinds came out as {runs} batches",
                frame.len(),
            );
            // …and the four sheets really are four, or the count above is
            // satisfied by everything sharing one.
            let mut sheets: Vec<_> = frame.iter().map(|s| s.sheet).collect();
            sheets.dedup();
            assert_eq!(sheets.len(), 4);
            assert_eq!(sheets[0], scene.grass[0].sheet, "the ground comes first");
            assert_eq!(sheets[1], scene.props[0].sheet, "then the scenery");
            assert_eq!(sheets[3], scene.bolt.sheet, "the shots come last");
            // The two prop kinds really are two frames of that one sheet.
            assert_ne!(
                scene.props[0].uv.map(f32::to_bits),
                scene.props[1].uv.map(f32::to_bits),
                "a tree and a bush are the same frame",
            );

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

    /// **Ten thousand enemies are still four batches**, which is what the
    /// plan's exit criterion asks for — a count that does not move with the
    /// horde — and what the 42-sprite test above cannot give.
    ///
    /// The whole field is packed *inside* the view so nothing is culled: a
    /// version that let the cull do its job would assert the batch count over
    /// the fifteen hundred that survive, which is the same claim at a tenth of
    /// the pressure. The counting rule is
    /// [`crcbl::render::sprite_pass::batch_count`], the pass's own.
    #[test]
    fn ten_thousand_visible_enemies_are_still_four_batches() {
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
            // The real scatter, so the batch claim is made against the arena the
            // game actually deals rather than against a fixture.
            let props = crate::game::scatter_props(crate::game::DEFAULT_SEED);
            assert!(!props.is_empty(), "the scatter dealt nothing");
            let render = RenderState {
                player: DVec3::ZERO,
                enemies,
                bolts: (0..8)
                    .map(|i| crate::game::BoltView {
                        position: DVec3::new(i as f64 * 0.5, 0.0, 0.0),
                    })
                    .collect(),
                props,
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
            assert!(stats.ground > 0, "the ground layer emitted nothing");
            assert!(stats.props > 0, "the props layer emitted nothing");
            assert_eq!(
                stats.drawn,
                stats.ground + stats.props + COUNT + 8 + 1,
                "the ground, the scenery and the player are drawn too",
            );
            assert_eq!(frame.len(), stats.drawn);
            assert_eq!(
                stats.batches, 4,
                "{} sprites came out as {} batches",
                stats.drawn, stats.batches,
            );
            // …and the four really are terrain, then props, then actors, then
            // the bolt sheet, or "four batches" is one sheet drawn four times.
            let mut sheets: Vec<_> = frame.iter().map(|s| s.sheet).collect();
            sheets.dedup();
            assert_eq!(sheets.len(), 4);
            assert_eq!(sheets[0], scene.grass[0].sheet, "the ground comes first");
            assert_eq!(sheets[1], scene.props[0].sheet, "then the scenery");
            assert_eq!(sheets[3], scene.bolt.sheet, "the shots come last");
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
                props: crate::game::scatter_props(crate::game::DEFAULT_SEED),
                ..RenderState::default()
            };
            scene.build(&render, EXTENT);
            let stats = scene.stats();
            assert_eq!(stats.field, count);
            assert!(stats.props > 0, "the props layer emitted nothing");
            assert_eq!(
                stats.field + 1 + stats.ground + stats.props,
                stats.culled + stats.drawn,
                "the cull's own arithmetic does not close",
            );

            // The enemies alone: the ground is generated rather than culled and
            // the scenery is a constant of the seed, so both would otherwise pad
            // the number this test is about.
            let survivors = stats.drawn - stats.ground - stats.props - 1;
            // The view is about 37 x 28 units of a 96 x 72 arena, so a little
            // over an eighth of it. The bound is loose on purpose — the exact
            // number depends on the grid — but a cull that rejected nothing, or
            // everything, fails it.
            assert!(
                survivors > 500 && survivors < count / 4,
                "{survivors} of {count} survived the cull",
            );
            assert_eq!(
                stats.batches, 3,
                "the ground, the scenery, then one sheet of actors",
            );
        });
    }

    /// Each kind of thing goes on the layer it belongs to, back to front.
    ///
    /// The props sit between the grass and everything that moves, which is the
    /// order this module's header argues is the only one available — and the
    /// reason `assets/props.crpix` draws a footprint rather than a treetop.
    #[test]
    fn the_ground_is_behind_the_props_and_the_shots_are_in_front() {
        with_scene(|scene| {
            assert_eq!(scene.stack.layer_count(), 6);
            assert_eq!(scene.ground.depth(), 0);
            assert_eq!(scene.props_layer.depth(), 1);
            assert_eq!(scene.gems.depth(), 2);
            assert_eq!(scene.crowd.depth(), 3);
            assert_eq!(scene.hero.depth(), 4);
            assert_eq!(scene.shots.depth(), 5);
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

    /// **A prop outside the view is not a sprite**, and the near one is.
    ///
    /// The scatter is a whole arena's worth of scenery and a view is a fifteenth
    /// of the arena, so a prop layer that drew the lot would hand the pass the
    /// same constant every frame forever. Both halves are asserted in one run,
    /// because a cull that rejected everything looks exactly like a cull that
    /// works if only the far one is checked.
    #[test]
    fn props_outside_the_view_are_culled_and_the_near_one_is_not() {
        with_scene(|scene| {
            let far = view_half_width(EXTENT) + PROP_HALF_EXTENT + 5.0;
            assert!(
                far < ARENA_HALF_WIDTH,
                "the far prop has to be somewhere the arena can hold it",
            );
            let render = RenderState {
                player: DVec3::ZERO,
                props: vec![
                    PropView {
                        position: DVec3::new(2.0, 0.0, 0.0),
                        kind: PropKind::Tree,
                    },
                    PropView {
                        position: DVec3::new(far, 0.0, 0.0),
                        kind: PropKind::Bush,
                    },
                    // And vertically, which a cull written against one axis
                    // passes.
                    PropView {
                        position: DVec3::new(0.0, VIEW_HALF_HEIGHT + PROP_HALF_EXTENT + 5.0, 0.0),
                        kind: PropKind::Bush,
                    },
                ],
                ..RenderState::default()
            };
            scene.build(&render, EXTENT);
            let drawn = scene.stack.sprites(scene.props_layer).to_vec();
            assert_eq!(drawn.len(), 1, "the near prop, and only it");
            assert_eq!(drawn[0].uv, scene.props[prop_index(PropKind::Tree)].uv);
            assert_eq!(scene.stats().props, 1, "the stat disagrees");
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
