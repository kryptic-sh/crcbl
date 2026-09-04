//! Where each shadow map looks, and how far: the sun's cascades, a spot light's
//! cone and a point light's six faces.
//!
//! `docs/plan/18-render-features.md`'s shadow section, arithmetic half. This
//! module owns the matrices and the tile budget; [`crate::forward`] owns the
//! atlas, the depth-only pipeline and the per-tile cull that fills them. The
//! split is where the testable part is: a cascade matrix is a pure function of a
//! camera and a light direction, a spot's is a pure function of the light, and
//! the tile budget is a pure function of the light list — so every claim below
//! is checked here without a device in the room.
//!
//! # One atlas, [`TILES`] slots, and an allocator behind them
//!
//! The maps live side by side in a single `D32Float` image rather than as layers
//! of an array. That is a consequence of the render graph, not a preference: a
//! [`crate::graph`] render pass attaches an *image*, and there is no way to
//! attach layer `i` of one. Tiles need nothing the graph does not already have —
//! the shadow pass sets a viewport over a tile and draws — so the tile count is
//! a constant either side of the seam and not a feature request.
//! `mesh.slang`'s `shadow_atlas` says the same from the sampling side.
//!
//! A **slot** is one map: an index into the frame block's matrices, and the
//! number a light's row carries. The split of them is topic 18's 2026-08-13
//! decision: **the sun's cascades take the first [`CASCADES`] slots and the rest
//! are handed out one per shadowed spot and [`POINT_FACES`] per shadowed
//! point**, with [`LIGHT_TILES`] of them. A light that gets no slots still lights
//! and simply does not occlude, which is what makes the budget a quality knob
//! rather than a correctness cliff — see [`Selection`].
//!
//! **Where a slot's map lives in the image is a separate question, and
//! [`AtlasAllocator`] answers it.** `docs/plan/45-shadows.md`'s atlas rung: the
//! image is a quadtree over [`ATLAS_COLUMNS`] by [`ATLAS_ROWS`] root cells of
//! [`TILE`] texels, a map takes a whole cell or a halving of one down to
//! [`MIN_TILE`], and the shader reads the rectangle it landed in — see
//! [`Selection::atlas_rect`] and `mesh.slang`'s `atlas_uv`.
//!
//! **What asks for a halving is the priority rung, and it is `coverage`**:
//! how much of the frame's height a light's map covers on screen decides both
//! which lights are worth a run of tiles and how large each of those tiles is.
//! A light covering less than `WHOLE_CELL_COVERAGE` takes a quarter of a cell,
//! and each halving of its coverage below that takes another —
//! `tile_level` is the ladder and `LEVEL_HOLD_RATIO` the band that stops a
//! light on a threshold halving and doubling its map every frame. A frame whose
//! lights all clear the top of the ladder lays out exactly as the fixed grid
//! did, cell for cell.
//!
//! # Two budgets, because they buy different things
//!
//! [`LIGHT_TILES`] is atlas space and [`LIGHT_SLOTS`] is *cull* space, and the
//! two stopped being one number the moment a light could own six tiles:
//!
//! * A tile is [`TILE`]² of `D32Float`, and the atlas is allocated once for the
//!   renderer's life. [`POINT_FACES`] of them is what a point light needs to be
//!   shadowable at all, since its faces have to fit in the region together, and
//!   [`LIGHT_TILES`] holds two such runs with two tiles over — so two cubes and
//!   two cones fit side by side.
//! * A slot is one shadowed *light*, and what it costs is a
//!   [`DrawGen`](crate::draw_gen::DrawGen) — roughly five megabytes, most of it
//!   per-instance LOD hysteresis state that is device-local and permanent. Topic
//!   18's fourth decision is that a point light gets **one** of these rather than
//!   one per face: the six faces' union is the light's sphere, which is what the
//!   cull tests against anyway, so one visible set feeds all six draws through
//!   six matrices into six tiles. A face draws what is behind it and the
//!   rasteriser discards it.
//!
//! So the light region holds two point lights *and* two spots, or [`LIGHT_SLOTS`]
//! spots, and a frame with more shadow-worthy lights than either budget covers
//! shadows the most influential ones it can fit and lights the rest without
//! occluding. A *third* point light is what no longer fits: three cubes are three
//! times [`POINT_FACES`] tiles and the region is not that long.
//!
//! # The 2026-08-26 re-tiling, which cost no memory
//!
//! The region held [`POINT_FACES`] tiles and one over until 2026-08-26, so
//! exactly one point light in any scene could occlude — and a rig with a light
//! either side of a walkway, which is an ordinary rig, had the torch that lost
//! the tie *re-lighting* the shadow its twin cast. Widening the grid by a column
//! and a row while shrinking [`TILE`] to match leaves [`atlas_extent`] where it
//! was to the texel, so the image, its `D32Float` format and its memory are
//! unchanged and the whole cost is per-tile resolution. What is *not* free is
//! [`LIGHT_SLOTS`]: a slot is a [`DrawGen`](crate::draw_gen::DrawGen), so the
//! culls this budget added are device-local memory the atlas did not ask for.
//!
//! # Which filter samples them, and the seam that shows two at once
//!
//! [`r_shadow_filter`] is the one knob here that reaches into `mesh.slang`'s
//! sampling rather than into this module's arithmetic: `docs/plan/45-shadows.md`
//! has three filter rungs and, until it existed, only the top one was in the
//! tree. [`Filter`] is the set, [`filter`] is the near side of the comparison
//! and [`shipped_filter`] is the far one.
//!
//! **The seam is per pixel and not per pass**, which is what [`r_shadow_split`]
//! buys and what makes it unlike `crate::ssao::r_ssao_split`: the occlusion
//! gather is a full-screen pass, so its two sides are two recordings under two
//! scissors and cost one target's worth of fragments between them. The filter
//! lives in a scene draw, where a second recording is every triangle again — so
//! the two modes and the seam's column ride in
//! [`crcbl_shaders::mesh::FrameUniforms::shadow_filter`] and the fragment picks
//! its own side. `crate::split`'s header carries both shapes.
//!
//! **`pcss` is the default and is what every golden was blessed at**, and a
//! frame nobody has touched either variable on writes a row of zeroes — one
//! mode in both lanes and no column — which is the picture this engine drew
//! before the row existed.
//!
//! # Stability: a sphere around the eye, snapped to texels
//!
//! The classic cascade shimmer is a shadow edge crawling along a static surface
//! while the camera moves. It has two causes and this module removes both:
//!
//! * **The cascade's extent changing with the camera's orientation.** A box
//!   fitted to the split's view frustum is tight but rotates with the camera, so
//!   every frame resamples the same geometry at a different scale. [`Cascades`]
//!   uses a *sphere* instead, and its radius is [`Cascades::far`] — a number that
//!   depends on the split alone. Rotating the camera cannot change it.
//! * **The cascade's origin moving by a fraction of a texel.** Fixed by
//!   quantising the light-space origin to whole texels, which is what
//!   `snap_to_texel` does and what the ortho box below is built around.
//!
//! The cost of the sphere is resolution: a sphere of radius `r` centred on the
//! eye covers everything within `r`, including the half of it behind the camera
//! that will never be shaded. A frustum-fitted box would be roughly twice as
//! dense for the same tile. That is the trade, taken deliberately: the tight fit
//! is the version that has to branch on [`Projection`](crate::Projection) — an
//! orthographic camera has no field of view to build corners from — and a shadow
//! pass that is correct on one projection and wrong on the other is worse than
//! one that is uniformly coarser.

mod atlas;
mod cadence;

use glam::{Mat4, Vec3, Vec4Swizzles};

pub use atlas::{AtlasAllocator, MIN_TILE, TILE_LEVELS, Tile, TileRect};
pub use cadence::{Cadence, GROUPS, Group, r_shadow_cadence, r_shadow_faces};

use crate::camera::Camera;
use crate::cull::Frustum;
use crate::light::{Light, PointLight, SpotLight};

crcbl_console::convar! {
    /// Which shadow filter runs: `pcss` ships, `disc` drops the blocker search,
    /// `box` is the 3×3.
    pub static r_shadow_filter: &'static str one_of ["pcss", "disc", "box"] = "pcss";
}

crcbl_console::convar! {
    /// Compare the console's shadow filter against the shipped one: seam at 0..1.
    pub static r_shadow_split: f32 in 0.0 ..= 1.0 = 0.0;
}

/// The shadow filter a fragment runs — `docs/plan/45-shadows.md`'s ladder, with
/// every rung of it still in the tree.
///
/// **A uniform lane and not a pipeline**, which is what separates this from
/// `crate::ssao`'s `Technique`: the occlusion gather is a full-screen pass and
/// two of them are two shaders, where this is a branch inside a *scene* draw.
/// `crate::split`'s header is where the consequence is argued — a scene pass
/// cannot record itself twice under a scissor without drawing every triangle
/// twice, so the seam rides in the frame block and `mesh.slang`'s
/// `shadow_filter_mode` picks a side per fragment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// The blocker search sizes the sun's disc per fragment and the punctual
    /// maps take it at its fixed reach: the ninth, tenth and eleventh decisions
    /// together. **What ships**, and what every golden was blessed under.
    Pcss,
    /// The same rotated disc at the fixed reach for the sun as well, so no
    /// blocker search runs and no penumbra widens with its caster's height: the
    /// ninth and eleventh decisions without the tenth.
    Disc,
    /// The 3×3 hardware-PCF box the ninth decision replaced.
    Box,
}

impl Filter {
    /// The name [`r_shadow_filter`] holds for this one.
    ///
    /// The `convar!` needs literals, so the names are written twice — three
    /// times counting `mesh.slang`'s constants — and this module's
    /// `the_variable_names_are_the_filters_labels` is what holds this copy to
    /// the other two.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pcss => "pcss",
            Self::Disc => "disc",
            Self::Box => "box",
        }
    }

    /// The value `mesh.slang` compares against, which is this filter's place in
    /// [`crcbl_shaders::mesh::SHADOW_FILTERS`].
    ///
    /// Derived from the list rather than written down, so a filter inserted in
    /// the middle of it renumbers here rather than silently selecting its
    /// neighbour.
    #[must_use]
    pub fn mode(self) -> u32 {
        let at = crcbl_shaders::mesh::SHADOW_FILTERS
            .iter()
            .position(|label| *label == self.label())
            // Unreachable while `the_variable_names_are_the_filters_labels`
            // passes: that test is what holds the two lists to one set of
            // names. Zero is what ships, which is the answer a caller would
            // rather have than a panic in a frame.
            .unwrap_or(0);
        u32::try_from(at).unwrap_or(0)
    }

    /// The filter `name` names.
    ///
    /// **[`Filter::Pcss`] for a name outside the set**, which the console cannot
    /// deliver — an enum variable refuses a value it does not declare — so this
    /// is what a caller reaching past the console would get, and it is what
    /// ships rather than an arbitrary arm. `crate::ssao`'s `Technique::of` is
    /// the same rule on the same shape.
    fn of(name: &str) -> Self {
        FILTERS
            .into_iter()
            .find(|filter| filter.label() == name)
            .unwrap_or(Self::Pcss)
    }
}

/// The order [`r_shadow_filter`] declares its names in.
///
/// `crate::ssao`'s `TECHNIQUES` has the same shape for the same reason: the
/// macro takes literals, so the names exist twice, and [`Filter::of`] searches
/// this copy.
const FILTERS: [Filter; 3] = [Filter::Pcss, Filter::Disc, Filter::Box];

/// [`r_shadow_filter`] as the near side of the seam wants it.
#[must_use]
pub fn filter() -> Filter {
    Filter::of(r_shadow_filter.get_enum())
}

/// The filter the engine ships, for the other side of the seam.
///
/// `crate::ssao`'s `shipped_technique` exactly: read off the variable's own
/// declared default rather than written down, so the far side follows a default
/// that moves and the comparison is never a configuration against itself.
#[must_use]
pub fn shipped_filter() -> Filter {
    match r_shadow_filter.default() {
        crcbl_console::Value::Enum(name) => Filter::of(name),
        // Unreachable by construction: `convar!` builds an enum cell and its
        // default together. This is what a `match` needs.
        _ => Filter::Pcss,
    }
}

/// [`r_shadow_split`] as `crate::split::halves` wants it, or `None` for a
/// frame comparing nothing.
///
/// `crate::ssao`'s `split_at` rule: **zero is off and is the default**, so a
/// frame nobody has touched the console on draws one filter everywhere — a
/// comparison is something a person asks for, and every golden in this
/// workspace was blessed without one. `halves` refuses a seam at either edge
/// anyway, so this is the readable spelling of a rule that holds whatever
/// reaches it.
#[must_use]
pub fn split_at() -> Option<f32> {
    let at = r_shadow_split.get_f32();
    (at > 0.0 && at < 1.0).then_some(at)
}

/// The lock [`r_shadow_filter`], [`r_shadow_split`], [`r_shadow_bias`] and
/// [`r_shadow_normal_offset`] are moved under.
///
/// **In this module rather than beside the test that takes it**, on
/// `crate::ssao::TECHNIQUE_SWITCH`'s terms exactly: two files move these
/// variables — this module's own tests and `crate::forward`'s
/// `shadow_filter_switch` — and a `cargo test` run shares one process between
/// them, so a variable one left moved is a frame the other did not ask for.
///
/// **One lock for the four variables**, unlike the occlusion chain's, because
/// they are read into two rows of one block: a test that moved the filter while
/// another moved the seam would be reading a lane neither of them wrote, and the
/// bias pair's row is rebuilt by [`Cascades::params`] beside it.
///
/// **Last in the lock order**, after `crate::ssao::TECHNIQUE_SWITCH`. A test
/// that takes it with the others takes it after all of them, so there is no
/// cycle to have.
#[cfg(test)]
pub(crate) static FILTER_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How many cascades the sun's shadow map is split into.
///
/// The shader's number, not a second one: `crcbl_shaders::mesh` is where it
/// lives and where it is checked against the `.slang` sources, because the
/// uniform block's layout depends on it.
pub const CASCADES: usize = crcbl_shaders::mesh::SHADOW_CASCADES;

/// How many **tiles** the atlas has for shadowed lights beside the cascades.
///
/// The shader's number for the same reason [`CASCADES`] is: the frame block
/// carries one matrix per light tile, so a block sized differently on the two
/// sides puts every member after it at the wrong offset.
pub const LIGHT_TILES: usize = crcbl_shaders::mesh::SHADOW_LIGHT_TILES;

/// Tiles one point light's shadow map is: the six faces of a cube.
///
/// The shader's number as well, because it is how far apart two of a light's
/// tiles are and `mesh.slang`'s `point_face` adds it to a row's base.
pub const POINT_FACES: usize = crcbl_shaders::mesh::SHADOW_POINT_FACES;

/// How many lights a frame can shadow at once.
///
/// **Not a shader number**, unlike [`LIGHT_TILES`], and the module docs say why:
/// nothing in a shader counts lights, and what this bounds is the number of
/// [`DrawGen`](crate::draw_gen::DrawGen)s the renderer holds — one cull per
/// shadowed light, whether that light is one tile or six.
///
/// Four, which is what the light region is sized for: two point lights' cubes
/// and two spots' maps beside them. A fifth shadowed light is refused here, and a
/// *third* point light is refused by [`Selection`] running out of tiles rather
/// than by a rule of its own — the two budgets bind in different places, which
/// is why they are two numbers.
pub const LIGHT_SLOTS: usize = 4;

/// The side of one **root cell** of the shadow atlas, in texels.
///
/// The largest map the allocator can hand out and the size of every cell of the
/// grid the quadtree's roots are laid out on — not the size of every map, which
/// is [`Assignment::level`]'s answer and varies with `coverage`.
///
/// **A host number, and no longer the shader's.** `mesh.slang` used to declare
/// it because every shadow bias there is denominated in the texels of the map
/// being sampled, and while every map was a whole cell the two were the same
/// number; since the priority rung they are not, and the shader reads each map's
/// own side out of its `shadow_atlas_rect` instead. What it sizes here is the
/// image — see [`atlas_extent`] — which is a fact about the resource the
/// renderer allocates and about nothing a shader can see.
pub const TILE: u32 = crcbl_shaders::mesh::SHADOW_TILE;

/// Tiles across the atlas.
///
/// At least [`CASCADES`], which is what keeps the cascades in the top row at the
/// origins they were blessed at — see [`tile_origin`].
pub const ATLAS_COLUMNS: u32 = crcbl_shaders::mesh::SHADOW_ATLAS_COLUMNS;

/// Tiles down it.
///
/// A grid rather than one row: a point light is [`POINT_FACES`] tiles of exactly
/// this kind, and a single row holding [`TILES`] of them beside the cascades
/// would be an image several times wider than it is tall, for no gain. The
/// addressing below is written in terms of both extents, and so is
/// `mesh.slang`'s.
pub const ATLAS_ROWS: u32 = crcbl_shaders::mesh::SHADOW_ATLAS_ROWS;

/// Tiles in the whole atlas: [`CASCADES`] for the sun and [`LIGHT_TILES`] for
/// the lights that fit.
pub const TILES: usize = (ATLAS_COLUMNS * ATLAS_ROWS) as usize;

const _: () = assert!(
    TILES == CASCADES + LIGHT_TILES,
    "every tile of the grid is either a cascade's or a light's; a grid with a \
     tile nothing owns is atlas nothing writes and nothing samples"
);

const _: () = assert!(
    LIGHT_SLOTS <= LIGHT_TILES,
    "a slot with no tile to render into is a cull dispatch whose result nothing \
     can sample"
);

/// The widest half-angle a spot can have and still be given a tile, in radians.
///
/// A spot's map is one perspective projection whose field of view is twice the
/// cone's outer half-angle, and a projection's field of view cannot reach 180°:
/// the tangent runs to infinity and the matrix stops being one. So a cone wider
/// than this is **refused a tile** rather than given a map that covers part of
/// it — [`Selection`] is where that happens, and the light then lights without
/// occluding, which is the same honest degradation as running out of tiles.
///
/// 80° leaves `tan` at under six, so the near plane's footprint stays a sane
/// multiple of the tile it is rendered into.
pub const MAX_SPOT_HALF_ANGLE: f32 = 80.0 * std::f32::consts::PI / 180.0;

/// A punctual light's near plane, in world units.
///
/// Everything nearer than this to the light is inside it and casts nothing. It
/// is the only knob a perspective shadow map's depth distribution really has —
/// under reversed-Z the precision piles up at the near plane, so this being
/// small is what makes a caster far from the light cheap in depth resolution and
/// not the other way round.
///
/// One number for a spot's cone and a point light's faces, because it is one
/// piece of knowledge: how close to a light something has to be before it stops
/// being a caster and starts being the light's own housing.
const PUNCTUAL_NEAR: f32 = 0.05;

/// How far from the eye the sun's shadows reach, in world units.
///
/// Past this every surface is lit: the last cascade's box ends, the sampling
/// code takes its out-of-bounds path, and a distant hillside is simply not
/// shadowed. That is the honest failure for a shadow map, and the alternative —
/// extending the last cascade to the far plane — spends the whole atlas on
/// geometry too far away to see the result on.
///
/// Not a field on [`DirectionalLight`](crate::DirectionalLight): it is a
/// property of how much shadow detail a *view* is willing to pay for, which
/// topic 18 puts on the per-camera stack when there is one to put it on.
pub const DISTANCE: f32 = 24.0;

/// How much of the practical split scheme is logarithmic rather than uniform.
///
/// The two extremes are both wrong in the usual way: a uniform split gives the
/// nearest cascade — the one filling most of the screen — almost no resolution,
/// and a fully logarithmic one gives the far cascades so little that their
/// texels are visible as steps. `0.7` is the conventional compromise and is
/// where every reference implementation of this scheme sits.
const SPLIT_LAMBDA: f32 = 0.7;

/// Distance in front of the light's near plane that still writes depth, in world
/// units.
///
/// A caster between the sun and the cascade's sphere is outside the sphere and
/// must still darken what is inside it, so the light's box is pulled back this
/// far and its near plane put at zero. Too small and a tall object stops casting
/// as the camera approaches its base; too large and the depth range stretches
/// for nothing.
///
/// **It used to set the size of a light leak, and no longer does.** The sun's
/// bias was denominated in shadow-clip depth, so its world meaning was this
/// number plus `2 * radius` — 88 m on the outer cascade, of which 40 was this —
/// and a bias of 0.0094 in clip was 0.83 m of world slack against walls 0.15 m
/// thick. [`DEPTH_BIAS_TEXELS`] is what removed the coupling; this is a caster
/// budget again.
///
/// **Taken from the shader rather than declared here**, because the sampling
/// side inverts the box this builds: `mesh.slang`'s `sun_penumbra_texels` reads
/// `2 * radius + SHADOW_CASTER_REACH` back out of a shadow-clip depth to get the
/// metres a penumbra is sized from. Two declarations would let the matrix and
/// its inverse drift apart.
const CASTER_REACH: f32 = crcbl_shaders::mesh::SHADOW_CASTER_REACH;

/// The constant part of the sun's shadow comparison bias, in **texels of the
/// cascade the fragment landed in**.
///
/// # Texels, because a texel is what acne is made of
///
/// Acne is the shadow map quantising a receiver's own depth away across one
/// texel, so what has to be covered is a world distance: the footprint of one
/// texel, times how fast the surface climbs across it. `sun_visibility` in
/// `shaders/mesh.slang` converts through `2 * radius / TILE` — the footprint of
/// one texel of a cascade of that radius — and offsets the world position
/// towards the light before projecting, exactly as `punctual_visibility` does
/// for a cone or a cube face. One denomination for both light types.
///
/// The number this replaced was denominated in shadow-clip depth, which for an
/// orthographic projection is linear and therefore looks like a world distance
/// — the argument the shader used to carry. What it actually scaled with was the
/// cascade's whole depth range, [`CASTER_REACH`] included, so it grew when a
/// scene needed more caster reach and shrank when it needed less. The visible
/// consequence was `apps/lantern`'s room: a lit strip 0.60 m wide along the foot
/// of a wall, and a band down the left of the back wall three times too bright.
///
/// # This is the term the offset cannot predict, and one scene sets it
///
/// Depth quantisation is not what it covers: a cascade's range is tens of metres
/// over a `D32Float`, so that error is micrometres, and what a texel's footprint
/// explains on the facet the fragment is actually on is
/// [`NORMAL_OFFSET_TEXELS`]' job — it walks the lookup sideways until it reads a
/// texel the receiver owns. What is left over is the **seam between two
/// facets**: adjacent triangles of a tessellated surface climb at different
/// rates, and the texel their shared edge falls in stores the steeper one's
/// depth wherever the lookup lands. No offset read off either facet predicts the
/// other's, and this is what covers the difference.
///
/// `crcbl_render::scene::demo`'s dunes patch is the surface that sets it — an
/// analytic height field sampled onto one-metre quads, so a facet's neighbours
/// are as far from it in slope as anything in the tree. Counting the pixels in
/// its lit valley floor that sit more than ten luma below the median of their
/// own neighbourhood, at [`NORMAL_OFFSET_TEXELS`] held at two, on radv at
/// 1280×960:
///
/// | Constant, in texels | Dark pixels in the valley |
/// | ------------------- | ------------------------- |
/// | 0                   | 173                       |
/// | 0.5                 | 45                        |
/// | 1 (shipped)         | 24                        |
/// | 1.5                 | 24                        |
/// | 2                   | 24                        |
/// | 3                   | 24                        |
///
/// One is where the count stops falling, and there is **no margin above it**:
/// what it covers is a bounded, understood quantity rather than a cover for an
/// unknown, so buying margin in it is buying `apps/lantern`'s wall-foot strip
/// back for nothing.
///
/// # Three was the number while the second term was a depth bias
///
/// It fell to one when [`NORMAL_OFFSET_TEXELS`] replaced a slope-scaled *depth*
/// bias — the same count, but moving the receiver towards the light rather than
/// across its own map. That term is what the wall-foot strip was made of:
/// `apps/lantern`'s floor lies at a slope of 3.17, so two texels per unit of
/// `tan` was six and a half texels of depth slack wherever the sun grazes, on
/// top of this constant's three.
///
/// Measured through `apps/lantern`'s 1280×960 review frame on radv, walking the
/// floor out from the `-x` wall at `room::SHADED_FLOOR`'s own line:
///
/// | Artefact                                | Depth-biased slope | Normal offset |
/// | --------------------------------------- | ------------------ | ------------- |
/// | Peak luma in the strip at the wall's foot | 140.3             | 51.0          |
/// | Lit strip's half-fall width             | 0.391 m            | none          |
/// | Cornice lift over the shadowed back wall | 78.3 luma          | 11.7 luma     |
/// | Dark pixels in the dunes' valley floor  | 60                 | 24            |
///
/// **51.0 is the shadowed floor's own value**, which is what makes the second
/// column say the leak is gone rather than narrowed: the profile never rises
/// above the shadow it is walking through, so there is no half-fall to measure.
/// `crcbl_render::scene::demo`'s dunes patch reads 5 dark pixels before and 3
/// after at the golden's own 256×192.
///
/// What it cost is recorded rather than hidden: the brass block's foot picks up
/// a scalloped fringe a couple of pixels deep, on the period of the shadow
/// texel, where the offset walks a receiver near a silhouette across the edge of
/// its own caster. It is the standard cost of this direction, it is bounded by
/// the offset itself, and it is a tenth the size of the strip it replaced.
///
/// # And a half back, when the filter grew a radius
///
/// One held while the filter was a 3×3 box, whose furthest tap is one texel out.
/// `mesh.slang`'s `SHADOW_FILTER_TEXELS` reaches two, and a tap two texels out
/// reads a depth two texels' worth of the receiver's own slope away — which this
/// constant, and not [`NORMAL_OFFSET_TEXELS`], is what covers: an offset along
/// the normal moves every tap together and cannot close a gap between them. The
/// dunes patch, counted as above with the disc filter at two texels:
///
/// | This constant | Dark pixels in the valley | Strip at the wall's foot |
/// | ------------- | ------------------------- | ------------------------ |
/// | 1             | 47                        | none                     |
/// | 1.5 (shipped) | 25                        | none                     |
/// | 2             | 24                        | 0.015 m                  |
///
/// A half-texel is where the count returns to what the box filter had and the
/// strip has not yet come back. Two buys one more dark pixel and 15 mm of lit
/// floor at the wall — a fortieth of the 0.391 m the seventh decision removed,
/// but the wrong direction — so 1.5 ships.
///
/// # The console reads the cell, not this constant
///
/// [`r_shadow_bias`] declares this value as its own default and
/// [`Cascades::params`] reads that variable, so the count a frame is drawn with
/// is whatever the console holds. It is there for
/// `docs/plan/sample/18-sundial.md`'s milestone 2 — the comparison, not the
/// rung: acne and peter-panning are pulled in opposite directions by this number
/// and a fixture is the only place that trade can be looked at. The variable's
/// range starts at zero, which is where the acne it covers is read, and stops
/// comfortably past 112 texels, which is where `apps/sundial`'s plinth has lost
/// its shadow over the whole strip that fixture's camera can see — so nothing at
/// the top of it draws anything the walk has not already drawn.
const DEPTH_BIAS_TEXELS: f32 = 1.5;

/// How far along its own geometric normal a receiver is moved before the sun's
/// shadow lookup, per unit of `sin(acos(Ng·L))`, in the same texels.
///
/// `Ng` and not `N`: the offset is read off the rasterised facet, which is
/// `geometric_normal_of` in `shaders/mesh.slang` and its own reason. Why the
/// direction is the normal and not the light, and why a sine bounds it where the
/// tangent it replaced had to be clamped, is `shadow_normal_offset` in that file.
///
/// # What sets it, and what caps it
///
/// The dunes patch again, at [`DEPTH_BIAS_TEXELS`] held at one and the filter
/// still a 3×3 box, counted the same way, on radv at 1280×960:
///
/// | Offset, in texels | Dark pixels in the valley |
/// | ----------------- | ------------------------- |
/// | 0                 | 1159                      |
/// | 1                 | 74                        |
/// | 2 (shipped)       | 24                        |
/// | 3                 | 22                        |
/// | 4                 | 22                        |
///
/// Two is where the count reaches its floor — the last two rows are two pixels
/// apart, which is the noise in this measurement — and the ceiling above it is
/// **the thinnest wall in the tree**. An offset moves a receiver bodily, so one
/// larger than the geometry it stands against moves it through: at
/// [`DISTANCE`]'s outer cascade a texel is 62.5 mm of world, so two of them is
/// 125 mm and `apps/lantern`'s walls are a shell of `room::SHELL`, which is
/// 150 mm. Three would be 187.5 mm and past it. That bound is a property
/// of the scene rather than of this number, so it is the *reason* two is shipped
/// rather than three and not a claim that a leak was seen — none was.
///
/// # The console reads the cell here too
///
/// [`r_shadow_normal_offset`] declares this value as its default, on
/// [`DEPTH_BIAS_TEXELS`]' terms exactly and for its reason. **The ceiling is a
/// long way past the bound above**, and deliberately: two texels is what ships
/// because three would move a receiver through `apps/lantern`'s shell, and a
/// variable that stopped at the shipped count could not show what walking
/// through one looks like. It stops comfortably past 44 texels, which is where
/// `apps/sundial`'s plinth loses its whole shadow rather than only its contact —
/// that fixture's
/// `the_two_bias_counts_trade_acne_against_the_plinths_own_contact` is what
/// measured it.
const NORMAL_OFFSET_TEXELS: f32 = 2.0;

crcbl_console::convar! {
    /// The sun's constant shadow bias, in texels of the cascade a fragment
    /// landed in. Too little draws acne; too much lifts a shadow off its caster.
    pub static r_shadow_bias: f32 in 0.0 ..= 128.0 = DEPTH_BIAS_TEXELS;
}

crcbl_console::convar! {
    /// How far along its own normal a receiver moves before the sun's shadow
    /// lookup, in the same cascade texels.
    pub static r_shadow_normal_offset: f32 in 0.0 ..= 64.0 = NORMAL_OFFSET_TEXELS;
}

/// The cascade matrices a frame shades and culls with.
///
/// One [`Mat4`] per cascade plus the split distances the fragment stage selects
/// between them with. Everything here is what goes into
/// `crcbl_shaders::mesh::FrameUniforms`; nothing in it names a GPU resource.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascades {
    /// World → cascade `i`'s shadow clip. Orthographic and **reversed-Z**, like
    /// every other projection in the engine.
    pub view_proj: [Mat4; CASCADES],
    /// How far from the eye cascade `i` reaches, in world units, and therefore
    /// the radius of the sphere it is fitted to.
    ///
    /// A `[f32; 4]` rather than a `[f32; CASCADES]` because that is the shape
    /// the uniform block's `float4` takes; components past [`CASCADES`] repeat
    /// the last real split, so an out-of-range read — which cannot happen —
    /// would select the widest cascade rather than a zero that shadows
    /// everything.
    pub far: [f32; 4],
}

impl Cascades {
    /// The cascade split distances, nearest first.
    ///
    /// The practical split scheme: a blend of a logarithmic and a uniform
    /// division of `near .. DISTANCE`, weighted by [`SPLIT_LAMBDA`].
    fn splits(near: f32) -> [f32; CASCADES] {
        let mut out = [DISTANCE; CASCADES];
        for (index, far) in out.iter_mut().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a cascade index is at most four"
            )]
            let ratio = (index + 1) as f32 / CASCADES as f32;
            let logarithmic = near * (DISTANCE / near).powf(ratio);
            let uniform = near.mul_add(1.0 - ratio, DISTANCE * ratio);
            *far = SPLIT_LAMBDA.mul_add(logarithmic, (1.0 - SPLIT_LAMBDA) * uniform);
        }
        out
    }

    /// Builds this frame's cascades for `camera` under a light shining *from*
    /// `to_light`.
    ///
    /// `to_light` is the same "towards the light" convention
    /// [`DirectionalLight::direction`](crate::DirectionalLight::direction) uses,
    /// and it does not have to be normalised.
    ///
    /// # Panics
    ///
    /// If `to_light` has no direction — a zero or non-finite vector — or if the
    /// camera is degenerate, on [`Camera::view`]'s terms. Both produce a matrix
    /// of `NaN`s that reaches the shader as an empty shadow map, which is the
    /// failure mode this whole slice can hide behind.
    #[must_use]
    pub fn new(camera: &Camera, to_light: Vec3) -> Self {
        let to_light = to_light.normalize_or_zero();
        assert!(
            to_light.length_squared() > 0.0,
            "the sun must point somewhere: {to_light:?} has no direction"
        );
        // Only the eye is read, so an orthographic camera needs no special case:
        // the sphere is centred on the eye whatever the projection is.
        let eye = camera.eye;
        assert!(eye.is_finite(), "the camera's eye is not finite: {eye:?}");

        let near = match camera.projection {
            crate::camera::Projection::Perspective { near, .. }
            | crate::camera::Projection::Orthographic { near, .. } => near.max(f32::MIN_POSITIVE),
        };
        let splits = Self::splits(near);

        let mut view_proj = [Mat4::IDENTITY; CASCADES];
        for (index, matrix) in view_proj.iter_mut().enumerate() {
            *matrix = cascade_matrix(eye, to_light, splits[index]);
        }
        // The last real split fills the unused components; see `far`.
        let mut far = [splits[CASCADES - 1]; 4];
        far[..CASCADES].copy_from_slice(&splits);
        Self { view_proj, far }
    }

    /// The `shadow_params` vector the fragment stage reads: the atlas's two
    /// texel sizes, then the constant and normal-offset biases — the biases in
    /// **cascade texels**, which is the unit this module's `DEPTH_BIAS_TEXELS`
    /// explains and `sun_visibility` in `shaders/mesh.slang` converts.
    ///
    /// **The two biases come from [`r_shadow_bias`] and
    /// [`r_shadow_normal_offset`] rather than from the constants**, whose values
    /// those variables declare as their defaults. This vector is rebuilt for
    /// every frame's block, so a cell moved between two frames reaches the second
    /// of them — which is what makes the pair a comparison a person can walk
    /// rather than a rebuild.
    ///
    /// **Both texel sizes, even where the grid is square and they are equal.**
    /// The shader's PCF kernel steps in *tile* space and scales by
    /// [`ATLAS_COLUMNS`] and [`ATLAS_ROWS`] to get back to one shadow-map texel,
    /// so a grid that is not square — which it stops being the moment a point
    /// light's six tiles arrive — needs the two apart. Deriving one from the
    /// other is how that kernel ends up sampling a rectangle.
    #[must_use]
    pub fn params() -> [f32; 4] {
        let (width, height) = atlas_extent();
        #[expect(
            clippy::cast_precision_loss,
            reason = "an atlas extent is a few thousand texels"
        )]
        let inverse = [1.0 / width as f32, 1.0 / height as f32];
        [
            inverse[0],
            inverse[1],
            r_shadow_bias.get_f32(),
            r_shadow_normal_offset.get_f32(),
        ]
    }
}

/// The shadow atlas's extent in texels: an [`ATLAS_COLUMNS`] by [`ATLAS_ROWS`]
/// grid of [`TILE`]-sided tiles.
#[must_use]
pub const fn atlas_extent() -> (u32, u32) {
    (TILE * ATLAS_COLUMNS, TILE * ATLAS_ROWS)
}

/// Where root cell `index` starts in the atlas, in texels.
///
/// Row-major, so the first cells are the top row while [`ATLAS_COLUMNS`] is at
/// least [`CASCADES`]. That is the arrangement the cascade goldens were blessed
/// under; the texels themselves move whenever [`TILE`] or [`ATLAS_COLUMNS`]
/// does, so a change to either is one those goldens have to be re-blessed
/// through.
///
/// **A cell, not a slot.** It is [`AtlasAllocator`]'s root order — where a
/// level-0 tile lands — and a slot's map is wherever the allocator put it, which
/// is [`Selection::atlas_rect`]. The two coincide while every map takes a whole
/// cell, which is what this rung shipped with.
#[must_use]
pub const fn tile_origin(index: usize) -> (u32, u32) {
    let index = index as u32;
    (
        TILE * (index % ATLAS_COLUMNS),
        TILE * (index / ATLAS_COLUMNS),
    )
}

/// Which atlas tile light tile `tile` of the light region is.
///
/// The one place the "cascades first, then the lights" split is written down.
/// Both the viewport the shadow pass sets and the tile `mesh.slang` samples come
/// through it — the shader's own `light_tile` is the same arithmetic, and
/// `crcbl_shaders::mesh`'s drift test is what holds the two together.
#[must_use]
pub const fn light_tile(tile: usize) -> usize {
    CASCADES + tile
}

/// How many light tiles `light` needs to be shadowed: [`POINT_FACES`] for a
/// point light, one for a spot.
///
/// The whole of what makes the light region a run allocator rather than an array
/// of slots, so it is one function both [`Selection`] and [`crate::forward`] ask
/// rather than a `match` each.
#[must_use]
pub const fn tile_span(light: &Light) -> usize {
    match light {
        Light::Point(_) => POINT_FACES,
        Light::Spot(_) => 1,
        // **No tiles, and never asked for any**: [`can_be_shadowed`] refuses a
        // rectangle before [`Selection`] reaches this, so the run allocator
        // never sees a zero-length run. The zero is what a rectangle's span
        // actually is rather than a placeholder — an area light has no single
        // view to render a map from, and giving it one is a rung of its own that
        // `docs/backlog.md` carries.
        Light::Rect(_) => 0,
    }
}

/// One cascade's world → shadow-clip matrix: an orthographic box around a
/// sphere of `radius` centred on `eye`, snapped to whole texels.
fn cascade_matrix(eye: Vec3, to_light: Vec3, radius: f32) -> Mat4 {
    // A basis that is not parallel to the light. `Vec3::Y` is the usual up and
    // is wrong for exactly one light — one straight overhead, which is the
    // commonest light there is — so the fallback is picked rather than asserted
    // against.
    let up = if to_light.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // **The light's view is anchored at the world origin, not at the camera.**
    // That is what makes the snap below mean anything: a view built from the
    // eye moves whenever the eye does, so quantising a position *inside* it
    // quantises a coordinate whose own frame is already sliding — the matrix
    // changes for a sub-texel step and the shimmer the snap exists to remove is
    // still there. Anchored, the frame is a pure function of the light, and
    // every cascade of every frame measures against the same one.
    let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, -to_light, up);

    // The snap. The sphere's centre is expressed in that fixed light frame and
    // quantised to whole texels, so a camera that moves by less than one
    // texel's worth of world produces a byte-identical matrix and the shadow
    // edges on a static surface do not move at all.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a tile is a few thousand texels"
    )]
    let texel = 2.0 * radius / TILE as f32;
    let centre = snap_to_texel((view * eye.extend(1.0)).xyz(), texel);

    // Right-handed view space looks down `-z`, so a light-space depth is `-z`.
    // The near end is pulled back past the sphere so a caster standing between
    // the sun and it still writes depth; the far end is the back of the sphere
    // and nothing beyond it can shadow anything inside.
    let farthest = radius - centre.z;
    let nearest = -centre.z - radius - CASTER_REACH;

    glam::camera::rh::proj::directx::orthographic(
        centre.x - radius,
        centre.x + radius,
        centre.y - radius,
        centre.y + radius,
        // **Swapped, and that is the whole reversal** — the same trick, for the
        // same reason, as `Projection::Orthographic`'s. The constructor maps its
        // fifth argument to depth 0 and its sixth to depth 1, so handing it the
        // far distance and then the near one puts 1.0 at the light. That is
        // reversed-Z, and it is what `CompareOp::Greater` everywhere else
        // expects.
        farthest,
        nearest,
    ) * view
}

/// A spot's world → shadow-clip matrix: a **perspective** projection down the
/// cone, reversed-Z, covering the cone exactly.
///
/// # Why there is no texel snap here
///
/// The cascades are snapped to whole texels because their box follows the
/// camera, so a sub-texel step of the eye would otherwise resample a static
/// surface at a new offset and crawl its shadow edges. Nothing about a spot's
/// map depends on the camera: it is a pure function of the light's own position,
/// axis and cone, so a still light produces a byte-identical matrix frame after
/// frame and there is nothing to quantise away. `a_still_spot_produces_the_same_matrix`
/// is the assertion, and it is the same guarantee the snap buys the sun rather
/// than a weaker one.
///
/// The snap could not transfer even if it were wanted: it works by quantising a
/// position in a *fixed light frame* whose texel size is constant across the
/// whole map, and a perspective map's texel covers a world footprint that grows
/// linearly with depth. There is no one step to round to.
///
/// # Panics
///
/// If the light's axis has no direction or its position is not finite. Both
/// produce a matrix of `NaN`s, which reaches the shader as a map that shadows
/// nothing — the failure this whole slice can hide behind.
#[must_use]
pub fn spot_matrix(spot: &SpotLight) -> Mat4 {
    let axis = spot.direction.normalize_or_zero();
    assert!(
        axis.length_squared() > 0.0,
        "a spot must point somewhere: {:?} has no direction",
        spot.direction
    );
    assert!(
        spot.position.is_finite(),
        "the spot's position is not finite: {:?}",
        spot.position
    );

    // A basis that is not parallel to the axis, on `cascade_matrix`'s terms and
    // for its reason: a spot pointing straight down is the commonest spot there
    // is, and it is exactly the one `Vec3::Y` cannot be the up vector of.
    let up = if axis.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = glam::camera::rh::view::look_at_mat4(spot.position, spot.position + axis, up);

    // The cone's outer half-angle is the projection's *half* field of view, so
    // the map covers the cone and no more: every texel of the tile is somewhere
    // the light can reach. `SpotLight::outer_angle` may be narrower than the
    // inner one on the way in — `Light::row` widens it rather than trusting it
    // — so the same widening happens here, or the map would be narrower than the
    // cone it is sampled for.
    let outer = spot.outer_angle.max(spot.inner_angle);
    let far = spot.radius.max(PUNCTUAL_NEAR * 2.0);

    glam::camera::rh::proj::directx::perspective(
        2.0 * outer,
        // Square: the tile is, and a cone has no other aspect ratio to have.
        1.0,
        // **Swapped, and that is the whole reversal** — the same trick, for the
        // same reason, as `cascade_matrix`'s orthographic pair and
        // `Projection::Perspective`'s. The constructor maps its third argument
        // to depth 0 and its fourth to depth 1, so handing it the far distance
        // and then the near one puts 1.0 at the light. That is reversed-Z, and
        // it is what `CompareOp::Greater` everywhere else expects.
        far,
        PUNCTUAL_NEAR,
    ) * view
}

/// The direction point-light face `face` looks along.
///
/// **The cube-map convention**: `+X, -X, +Y, -Y, +Z, -Z` as faces 0 to 5. It is a
/// convention rather than a derivation, and the only thing that has to agree with
/// it is `point_face` in `shaders/mesh.slang`, which picks a face out of the
/// largest component of the direction from the light. A shader selecting face 3
/// where the host built face 2's matrix samples a map of somewhere else, and
/// draws a frame that has shadows in it —
/// `every_face_of_a_point_light_covers_its_own_direction_and_no_other` is what
/// refuses that here, and `crcbl`'s `Scene::PointShadow` is what refuses it
/// through the shader.
///
/// # Panics
///
/// If `face` is not one of [`POINT_FACES`] faces.
#[must_use]
pub fn face_axis(face: usize) -> Vec3 {
    match face {
        0 => Vec3::X,
        1 => Vec3::NEG_X,
        2 => Vec3::Y,
        3 => Vec3::NEG_Y,
        4 => Vec3::Z,
        5 => Vec3::NEG_Z,
        _ => panic!("a point light has {POINT_FACES} faces, not a face {face}"),
    }
}

/// A point light's world → shadow-clip matrix for face `face`: a **90°
/// perspective** down that face's axis, reversed-Z.
///
/// The six of them tile the whole sphere exactly, which is what makes selecting
/// one by the major axis of the direction from the light correct rather than
/// approximate: at 90° with a square aspect the frustum's edge planes are the
/// diagonals `|x| = |z|` and `|y| = |z|`, and those are precisely where the major
/// axis changes.
///
/// # Why there is no texel snap here
///
/// [`spot_matrix`]'s reason exactly: nothing about the map depends on the camera,
/// so a still light produces a byte-identical matrix frame after frame and there
/// is nothing to quantise away.
///
/// # Panics
///
/// If the light's position is not finite, or if `face` is not one of
/// [`POINT_FACES`] faces. A non-finite position produces a matrix of `NaN`s,
/// which reaches the shader as a map that shadows nothing — the failure this
/// whole slice can hide behind.
#[must_use]
pub fn point_matrix(point: &PointLight, face: usize) -> Mat4 {
    assert!(
        point.position.is_finite(),
        "the point light's position is not finite: {:?}",
        point.position
    );
    let axis = face_axis(face);
    // A basis that is not parallel to the axis, on `cascade_matrix`'s terms. Two
    // of the six faces look along `Y`, so the fallback is picked rather than
    // asserted against — and *which* up vector a face takes only has to be
    // consistent, because both the matrix that renders the tile and the matrix
    // that samples it are this one. A cube map would have to match the API's own
    // per-face orientation; an atlas of six ordinary maps has no such obligation.
    let up = if axis.y == 0.0 { Vec3::Y } else { Vec3::Z };
    let view = glam::camera::rh::view::look_at_mat4(point.position, point.position + axis, up);

    let far = point.radius.max(PUNCTUAL_NEAR * 2.0);
    glam::camera::rh::proj::directx::perspective(
        // 90°, and it is the field of view that makes six faces a sphere: a
        // narrower one leaves a gap at every face's edge, a wider one overlaps
        // and wastes the tile.
        std::f32::consts::FRAC_PI_2,
        // Square: the tile is, and a cube face has no other aspect ratio.
        1.0,
        // **Swapped, and that is the whole reversal** — `spot_matrix`'s pair,
        // for its reason.
        far,
        PUNCTUAL_NEAR,
    ) * view
}

/// The frustum a point light's **one** cull runs against: the axis-aligned box
/// around its sphere of influence.
///
/// Topic 18's fourth decision, in the only place it has arithmetic. A cascade and
/// a spot cull against `Frustum::from_view_projection` of the matrix they render
/// with, and a point light has six of those — so it culls against what all six
/// share instead, which is the sphere the light reaches at all. Conservative
/// against every face and against the sphere itself (a box is larger), which is
/// the safe direction: a caster wrongly kept costs vertex work on one face, and
/// one wrongly rejected is a missing shadow.
#[must_use]
pub fn point_frustum(point: &PointLight) -> Frustum {
    let (centre, radius) = (point.position, point.radius);
    // Inward normals, on `Frustum`'s convention: a point is inside a plane when
    // `normal · p + w >= 0`.
    Frustum {
        planes: [
            glam::Vec4::new(1.0, 0.0, 0.0, radius - centre.x),
            glam::Vec4::new(-1.0, 0.0, 0.0, radius + centre.x),
            glam::Vec4::new(0.0, 1.0, 0.0, radius - centre.y),
            glam::Vec4::new(0.0, -1.0, 0.0, radius + centre.y),
            glam::Vec4::new(0.0, 0.0, 1.0, radius - centre.z),
            glam::Vec4::new(0.0, 0.0, -1.0, radius + centre.z),
        ],
    }
}

/// Whether a light can be given tiles at all.
///
/// Split out so the reason a light was refused is one predicate rather than a
/// condition buried in a fold: a zero radius has no frustum, a non-finite
/// position has no view, a cone at or past [`MAX_SPOT_HALF_ANGLE`] has no
/// projection, a **fill** light was asked not to occlude, and an area light has
/// no single view a map could be rendered from. All of them light without
/// occluding.
///
/// The fill test is the leading one and covers every kind, and it is the only
/// thing standing between a fill point light or spot and a tile: every variant
/// carries [`fill`](crate::PointLight::fill) now, and the arms below judge a
/// light by its shape alone. Delete the test and
/// `a_fill_light_is_refused_a_tile_where_the_same_light_takes_one` goes red,
/// which is what the flag's shadow half is asserted by. The `Rect` arm refuses
/// a rectangle regardless, so on that kind alone the test still decides
/// nothing.
///
/// What *is* enforced is that this function is the only thing that hands a
/// light a tile, so a fill light's row carries `NO_SHADOW_TILE` and
/// `mesh.slang` needs no test of its own for the shadow — only for the
/// specular, which is the flag's other half and is enforced there.
fn can_be_shadowed(light: &Light) -> bool {
    if light.is_fill() {
        return false;
    }
    match light {
        Light::Point(point) => point.radius > 0.0 && point.position.is_finite(),
        Light::Spot(spot) => {
            spot.radius > 0.0
                && spot.direction.normalize_or_zero().length_squared() > 0.0
                && spot.position.is_finite()
                && spot.outer_angle.max(spot.inner_angle) < MAX_SPOT_HALF_ANGLE
        }
        // A rectangle radiates from a surface rather than from a point, so
        // there is no single centre of projection a shadow map could be
        // rendered from. `docs/plan/45-shadows.md` is where an area light's
        // shadow belongs, and `docs/backlog.md` records that it is not built.
        Light::Rect(_) => false,
    }
}

/// How wide a light's shadow map is in the world, at the far end of the frustum
/// it is rendered with.
///
/// **The map's own footprint, not the light's sphere**, and the difference is
/// the whole reason this is a function rather than `2 * radius`: a spot's map
/// covers its cone and a point light's face covers a right angle, so two lights
/// of one radius spread their texels over world regions that differ by
/// `tan(outer)`. A narrow cone already has fine texels at the size it was given,
/// and judging it by its sphere would demote it for being narrow.
///
/// A rectangle has no map — [`can_be_shadowed`] refuses it one — so it has no
/// footprint either, and the zero it answers with is what
/// [`tile_span`] answers for the same light and for the same reason.
fn map_extent(light: &Light) -> f32 {
    match light {
        // A cube face is a 90° frustum, so its half-angle's tangent is one and
        // the far face is exactly the light's diameter across. `point_matrix`
        // is where that right angle is built.
        Light::Point(point) => 2.0 * point.radius,
        Light::Spot(spot) => {
            // The widened angle `spot_matrix` projects with, not the field as
            // it arrived: `Light::row` widens an outer angle narrower than the
            // inner one, and a footprint taken from the narrower of the two
            // would describe a map nothing renders.
            let outer = spot.outer_angle.max(spot.inner_angle);
            2.0 * spot.radius * outer.tan()
        }
        Light::Rect(_) => 0.0,
    }
}

/// How much of the frame's **height** a light's shadow map covers on screen.
///
/// Topic 18's rule — "projected screen influence", the metric family
/// `docs/plan/25-lod.md`'s level selection uses — with the two refinements this
/// module's priority rung needed to make one number answer two questions:
///
/// * **[`map_extent`] rather than the sphere's diameter**, so the number is
///   about the map rather than about the light. What decides both which lights
///   deserve a tile and how large that tile should be is how many texels a map
///   delivers per pixel of what it covers, and a spot's map covers its cone.
/// * **A fraction of the frame rather than a count of pixels**, which makes it
///   independent of the viewport: the projection's own scale over the distance
///   to the light, with no viewport height in it at all. A scene therefore
///   allocates the same tiles at 256×192 as at 1920×1080, which is what makes a
///   golden at the small extent evidence about the large one.
///
/// **An orthographic camera has no distance falloff**, so its coverage is the
/// footprint over the visible height and nothing else —
/// [`Projection::pixels_per_unit`](crate::camera::Projection::pixels_per_unit)
/// at a height of one is exactly that scale for both variants, and the divide
/// is what only a perspective camera takes. The distance is floored so a light
/// the eye is standing inside is the most important light there is rather than
/// an infinity.
fn coverage(light: &Light, camera: &Camera) -> f32 {
    let scale = camera.projection.pixels_per_unit(1.0);
    let extent = map_extent(light);
    if camera.projection.is_orthographic() {
        return extent * scale;
    }
    let (position, _) = light.sphere();
    extent * scale / camera.eye.distance(position).max(1e-4)
}

/// How much better a challenger must be before it takes a tile off the light
/// holding it.
///
/// The selection's hysteresis, and it is owed for the reason
/// `docs/plan/25-lod.md`'s was: two lights either side of the cutoff would
/// otherwise swap tiles whenever the camera drifts, and a shadow appearing and
/// disappearing frame to frame is far worse than the wrong one of two being
/// shadowed. Applied as a bonus on the incumbent's score, which is the same
/// shape as `ForwardRenderer::lod_hold_ratio` — a level is held until the error
/// clearly drops.
const HOLD_RATIO: f32 = 1.25;

/// The [`coverage`] at which a light's map is worth a whole root cell, and
/// therefore the top of the ladder [`tile_level`] walks.
///
/// # The ladder halves with the tile, which is why one number sets it
///
/// A level of the quadtree is a halving of the tile's side, so the coverage that
/// earns it halves too: a map covering half as much of the frame delivers the
/// same texels per pixel at half the side. Every threshold below the first is
/// therefore this over a power of two, and there is one number here rather than
/// [`TILE_LEVELS`] of them.
///
/// # What the number is, and what it is not
///
/// **It is the conservative end of a sweep, set by the fixtures that must not
/// move.** Read as texels per pixel it is generous: a quarter of a 1080-pixel
/// frame is 270 pixels of footprint against a whole cell's [`TILE`] texels, so
/// the shipped anchor hands a light nearly three shadow texels for every screen
/// pixel its map covers. The parameter-free rule is one texel per pixel, which
/// is where a map stops being oversampled — a light keeps a whole cell only
/// while its footprint spans more than half [`TILE`] pixels of the frame, so the
/// anchor would have to be that over the frame's height and would *rise* as the
/// frame shrinks. At the 256×192 every golden in this tree is blessed at, it
/// demotes every punctual light in the tree, `apps/lantern`'s pair included.
/// Shipping it is a re-bless of every punctual shadow golden, which is a
/// measurement rung rather than this one; `docs/backlog.md` carries it.
///
/// Measured against the tree's own fixtures, as this metric is independent of
/// the extent they are rendered at:
///
/// | Fixture                              | Coverage | Level here |
/// | ------------------------------------ | -------- | ---------- |
/// | `Scene::PointShadow`'s lamp          | 3.06     | 0          |
/// | `Scene::SpotShadow`'s cone           | 1.41     | 0          |
/// | `apps/lantern`'s lamp, worst of orbit | 1.06     | 0          |
/// | `apps/lantern`'s corner spot         | 0.37     | 0          |
///
/// The corner spot is the one that binds: at this anchor it clears a whole cell
/// by half as much again, and at an anchor of 0.375 it would not. Nothing in the
/// tree sits inside the band [`LEVEL_HOLD_RATIO`] opens under it.
const WHOLE_CELL_COVERAGE: f32 = 0.25;

/// How far a light's [`coverage`] must fall below the threshold that earned it
/// its tile before the tile halves.
///
/// `docs/plan/25-lod.md`'s "switch-up and switch-down differ", at this module's
/// other boundary: without a band a light drifting along a threshold halves and
/// doubles its map every frame, and a shadow whose resolution flickers is worse
/// than one that is a size too small. A ratio rather than an offset because the
/// thresholds are a geometric ladder — a fixed offset is most of the bottom rung
/// and none of the top.
///
/// The same fifth of the budget `ForwardRenderer::lod_hold_ratio` opens for the
/// same reason, and it is deliberately the same number: the two bands are one
/// rule about one kind of flicker, and a reader who has understood one has
/// understood the other.
const LEVEL_HOLD_RATIO: f32 = 0.8;

/// The [`coverage`] a map must reach to be worth level `level`.
///
/// [`WHOLE_CELL_COVERAGE`] halved once per level, so the ladder's steps are the
/// quadtree's own. The coarsest level has no threshold — every eligible light
/// gets at least [`MIN_TILE`] — and asking for one past the ladder answers with
/// a coverage nothing reaches rather than with a level nothing can allocate.
fn level_threshold(level: usize) -> f32 {
    if level + 1 >= TILE_LEVELS {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "the guard above bounds the shift by TILE_LEVELS"
    )]
    let halvings = (1u32 << level) as f32;
    WHOLE_CELL_COVERAGE / halvings
}

/// Which of [`TILE_LEVELS`] tile sizes a map of `coverage` earns, given the
/// level it held last frame.
///
/// `docs/plan/45-shadows.md`'s priority rung: a near, large light takes a whole
/// cell and a far or narrow one takes a halving of it. `previous` is the level
/// this light held last frame, or [`None`] for a light that held none — and a
/// light with no history starts at the coarsest level and climbs, which is
/// exactly the answer the ladder gives with no band at all. That is the property
/// `a_light_with_no_history_lands_where_the_sharp_ladder_puts_it` reads back.
///
/// The two walks are the band: a tile grows only once the coverage reaches the
/// finer level's threshold outright, and shrinks only once it falls
/// [`LEVEL_HOLD_RATIO`] below the one it holds. Between them the previous answer
/// stands, which is the whole of the hysteresis.
fn tile_level(coverage: f32, previous: Option<usize>) -> usize {
    let mut level = previous.unwrap_or(TILE_LEVELS - 1).min(TILE_LEVELS - 1);
    while level > 0 && coverage >= level_threshold(level - 1) {
        level -= 1;
    }
    while level + 1 < TILE_LEVELS && coverage < level_threshold(level) * LEVEL_HOLD_RATIO {
        level += 1;
    }
    level
}

/// Whether `span` tiles from `base` are inside the light region and all free.
///
/// A function rather than a closure inside [`Selection::update`]'s loop, because
/// the loop takes `used` mutably as soon as it has an answer.
fn run_is_free(used: &[bool; LIGHT_TILES], base: usize, span: usize) -> bool {
    base + span <= LIGHT_TILES && used[base..base + span].iter().all(|tile| !*tile)
}

/// One shadowed light: which light it is, where its run of tiles starts, and
/// how large each of them is.
///
/// The triple rather than a light index alone, because a slot no longer decides
/// a tile: a spot owns one tile and a point owns [`POINT_FACES`], so where a
/// light's map lives is an allocation and not an index — and since
/// `docs/plan/45-shadows.md`'s priority rung, how *large* it is is an allocation
/// too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assignment {
    /// Which light, as an index into the list [`Selection::update`] was given.
    pub light: usize,
    /// The first of the light's tiles, as an index into the atlas's light region
    /// — so [`light_tile`] of it is the atlas tile, and it is also the number
    /// `Light::row` puts in `GpuLight::shadow_tile`.
    pub base: usize,
    /// Which of [`TILE_LEVELS`] sizes each of those tiles is: 0 a whole root
    /// cell of [`TILE`] texels, each one below a halving of it.
    ///
    /// **One level for the whole run**, which is what a point light needs: its
    /// six faces are one map of one sphere seen six ways, and a cube with one
    /// face finer than its neighbour would change resolution across a seam.
    ///
    /// Carried on the assignment rather than recomputed because it is the
    /// hysteresis' memory — `tile_level` reads last frame's level out of
    /// [`Selection::slots`] to decide this frame's.
    pub level: usize,
}

/// Which lights hold the atlas's light tiles, and its memory of last frame.
///
/// Topic 18's 2026-08-13 rule, whole: eligible lights are ranked by projected
/// screen influence, ties break by index so a frame's answer does not depend on
/// the order a caller happened to build the list in, they take slots and runs of
/// tiles in that order until either budget runs out, and an incumbent keeps what
/// it holds until a challenger clearly beats it.
///
/// **A light that gets no tiles still lights.** Nothing here removes a light from
/// the frame; it decides which of them also occlude. A light can miss out two
/// ways — no free slot, or no free *run* long enough — and the second is what a
/// scene with three point lights hits: two cubes leave less than a cube behind
/// them, so the third point light lights without occluding while a spot ranked
/// below it still fits in the tiles left over.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// What holds slot `i`, or `None` if the slot is free.
    slots: [Option<Assignment>; LIGHT_SLOTS],
    /// The atlas's free space, spent and given back by [`Selection::lay_out`]
    /// once per [`Selection::update`].
    ///
    /// Kept here rather than rebuilt because it holds what every slot is
    /// holding: since `docs/plan/45-shadows.md`'s static-caching rung a tile
    /// outlives the frame it was allocated in, so an allocator built fresh each
    /// frame would be one that could not describe what the image contains.
    allocator: AtlasAllocator,
    /// The tile behind slot `i`'s rectangle, or [`None`] for a slot no map is
    /// rendered into.
    ///
    /// The allocator's own handle rather than a second record of it: `rects` is
    /// what everything else reads and this is what gives that rectangle back.
    /// Indexed as `rects` is.
    tiles: [Option<Tile>; TILES],
    /// Where atlas slot `i`'s map was rendered, or [`TileRect::EMPTY`] for a
    /// slot no map was rendered into.
    ///
    /// Indexed the way the frame block's matrices are — the cascades first,
    /// then [`light_tile`] of a light-region slot — so the row that names a
    /// matrix names this rectangle too.
    rects: [TileRect; TILES],
}

impl Selection {
    /// Re-runs the selection over `lights` for `camera`.
    ///
    /// `lights` is the caller's own list and the indices below are into it; a
    /// caller whose shader rows are offset from it — the sun is row 0 — maps
    /// them itself.
    ///
    /// **The whole camera rather than its eye**, since the priority rung: what
    /// a light's map covers on screen depends on the projection as well as on
    /// the distance, and `coverage` is the one number both the ranking and the
    /// tile size are decided by.
    pub fn update(&mut self, lights: &[Light], camera: &Camera) {
        // Last frame's answer, kept whole: an incumbent's score is boosted and
        // its run is preferred, and both need to be read after `self.slots` has
        // been rebuilt.
        let previous = self.slots;
        let held_by = |index: usize| {
            previous
                .iter()
                .flatten()
                .find(|assignment| assignment.light == index)
        };

        // Score every eligible light, with the incumbents' scores boosted. That
        // ordering *is* the hysteresis: a challenger takes a held tile only by
        // beating the incumbent by `HOLD_RATIO`, which is the statement this
        // constant is written as. An incumbent that has dropped off the end of
        // the list, or stopped being eligible, is simply not in the ranking — the
        // list is rebuilt every frame and an index is only meaningful against the
        // list it was taken from.
        //
        // The coverage rides beside the score rather than being recomputed for
        // the light that wins a run: the score *is* the coverage with the
        // incumbent's bonus on it, and the tile size below is decided by the
        // coverage without it — one measurement, read twice.
        let mut ranked: Vec<(usize, f32, f32)> = lights
            .iter()
            .enumerate()
            .filter(|(_, light)| can_be_shadowed(light))
            .map(|(index, light)| {
                let coverage = coverage(light, camera);
                let score = coverage
                    * if held_by(index).is_some() {
                        HOLD_RATIO
                    } else {
                        1.0
                    };
                (index, score, coverage)
            })
            .collect();
        // Descending by score, ties by index. `total_cmp` rather than
        // `partial_cmp`: a `NaN` score would otherwise make the comparator
        // inconsistent and the sort's output arbitrary, and this is a frame's
        // *stable* answer or it is nothing.
        ranked.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));

        // Hand out runs in rank order. **Not truncated to the slot count first**:
        // a light that cannot fit — a point light with a spot already holding a
        // tile in the middle of the region — must not take the budget down with
        // it, so the walk continues and a smaller light behind it can still be
        // shadowed.
        let mut used = [false; LIGHT_TILES];
        let mut chosen: Vec<Assignment> = Vec::with_capacity(LIGHT_SLOTS);
        for (index, _, coverage) in ranked {
            if chosen.len() == LIGHT_SLOTS {
                break;
            }
            let span = tile_span(&lights[index]);
            // The run it already had, if that run is still free — so a light's
            // map does not move across the atlas while it goes on being
            // selected. Otherwise the first run long enough.
            let Some(base) = held_by(index)
                .map(|assignment| assignment.base)
                .filter(|base| run_is_free(&used, *base, span))
                .or_else(|| (0..LIGHT_TILES).find(|base| run_is_free(&used, *base, span)))
            else {
                continue;
            };
            for tile in &mut used[base..base + span] {
                *tile = true;
            }
            // The priority rung's answer, and the band around it: the level this
            // light held last frame is what [`tile_level`] holds against, and a
            // light that held none climbs the ladder from its bottom — which is
            // the sharp answer.
            let level = tile_level(coverage, held_by(index).map(|held| held.level));
            chosen.push(Assignment {
                light: index,
                base,
                level,
            });
        }

        // Incumbents keep the slot they had, so a light's cull — and with it the
        // per-instance LOD state that `DrawGen` holds for it — does not jump
        // between slots while the light is still selected. Whatever is left fills
        // the free slots in rank order.
        let mut slots = [None; LIGHT_SLOTS];
        for assignment in &chosen {
            if let Some(slot) = previous
                .iter()
                .position(|held| held.is_some_and(|held| held.light == assignment.light))
            {
                slots[slot] = Some(*assignment);
            }
        }
        for assignment in &chosen {
            if slots.contains(&Some(*assignment)) {
                continue;
            }
            if let Some(free) = slots.iter_mut().find(|slot| slot.is_none()) {
                *free = Some(*assignment);
            }
        }
        self.slots = slots;
        self.lay_out(lights);
    }

    /// Hands each occupied atlas slot a rectangle of the image, **keeping the
    /// one it already holds wherever it can**.
    ///
    /// **The cascades first and always**, whatever the light budget did: the
    /// shadow pass draws them every frame, and a fragment samples them on a
    /// frame with [`RenderEffects::SHADOWS`](crate::RenderEffects::SHADOWS) off
    /// too — it finds the pass's clear and comes back lit, which it can only do
    /// through a rectangle that names real texels. Then the light region's
    /// occupied slots, in slot order.
    ///
    /// **A cascade always takes a whole cell**, and a light's tiles take the
    /// level [`tile_level`] gave its assignment. The sun's map is the one this
    /// module never demotes: it covers the whole view rather than a light's
    /// reach, so there is no coverage to measure it by and every fragment the
    /// camera draws is a fragment it may shadow.
    ///
    /// # Retention, which is what makes the image outlive the frame
    ///
    /// `docs/plan/45-shadows.md`'s static-caching rung: a slot that wants the
    /// size it already holds keeps the *same texels*, and only a slot whose map
    /// is gone or whose size changed hands its tile back to
    /// [`AtlasAllocator::release`]. A frame that changed nothing therefore
    /// re-derives exactly the rectangles the image was drawn with, which is the
    /// precondition for not drawing it again — see
    /// `ForwardRenderer::shadow_atlas_inputs`, which decides that.
    ///
    /// It also means the rectangles are a function of the frames before this
    /// one and not of this one alone. What is *not* history-dependent is the
    /// case every golden in the tree is blessed under: an empty allocator and a
    /// frame whose maps all take whole cells lays out cell for cell in slot
    /// order, and every frame after it that wants the same sizes keeps it.
    ///
    /// # Coarsest first, and that is what makes the fit a fact
    ///
    /// The requests that still need a tile are sorted by level before they are
    /// spent, so every whole cell is handed out before anything subdivides one.
    /// Spending a finer request first is what would break it — a level-1 request
    /// splits a root and leaves three quarters that no whole cell can use — and
    /// it is the only ordering constraint the allocator has.
    ///
    /// The allocation cannot run out, which is why the failure below is
    /// `unreachable!` rather than a fallback: a fallback here would be a map
    /// rendered into a rectangle another map is also using. There are at most
    /// [`TILES`] slots — a cascade or a light tile each, which is what the
    /// assertion beside [`LIGHT_TILES`] counts — and each wants at most a whole
    /// root. A retained tile occupies at most one root, so `k` of them leave at
    /// least `TILES - k` whole roots free against at most `TILES - k` remaining
    /// requests, none of which needs more than one.
    fn lay_out(&mut self, lights: &[Light]) {
        // What each atlas slot wants this frame, or `None` where no map is
        // rendered into it. Per slot rather than per light because a point
        // light's run is `POINT_FACES` tiles of one level and the atlas
        // addresses tiles.
        let mut wanted: [Option<usize>; TILES] = [None; TILES];
        for want in &mut wanted[..CASCADES] {
            *want = Some(0);
        }
        for assignment in self.slots.iter().flatten() {
            let Some(light) = lights.get(assignment.light) else {
                continue;
            };
            let span = tile_span(light);
            for tile in assignment.base..assignment.base + span {
                wanted[light_tile(tile)] = Some(assignment.level);
            }
        }

        // Give back every tile whose slot no longer wants exactly what it
        // holds, **before** anything is allocated: the space a demoted light
        // frees is space the frame's remaining requests may need, and a merge
        // that ran after them would come too late to answer a whole cell.
        for (slot, want) in wanted.iter().enumerate() {
            let Some(held) = self.tiles[slot] else {
                continue;
            };
            if *want == Some(held.level()) {
                continue;
            }
            self.allocator.release(held);
            self.tiles[slot] = None;
            self.rects[slot] = TileRect::EMPTY;
        }

        // The slots still owed a tile and the level each wants, in slot order —
        // an array rather than a `Vec` because the count is bounded by the
        // atlas's own slots and this runs every frame.
        let mut requests = [(0usize, 0usize); TILES];
        let mut count = 0;
        for (slot, want) in wanted.iter().enumerate() {
            let Some(level) = *want else {
                continue;
            };
            if self.tiles[slot].is_some() {
                continue;
            }
            requests[count] = (slot, level);
            count += 1;
        }
        // Stable, so the order within one level is still the slot order every
        // shadow golden was blessed under: a frame whose lights all take whole
        // cells lays out exactly as the fixed grid did.
        requests[..count].sort_by_key(|(_, level)| *level);

        for &(slot, level) in &requests[..count] {
            let tile = self.allocator.allocate(level).unwrap_or_else(|| {
                unreachable!("a whole root cell per atlas slot, coarsest first")
            });
            self.tiles[slot] = Some(tile);
            self.rects[slot] = tile.rect();
        }
    }

    /// What holds each slot, in slot order.
    #[must_use]
    pub const fn slots(&self) -> &[Option<Assignment>; LIGHT_SLOTS] {
        &self.slots
    }

    /// Where atlas slot `slot`'s map was rendered, in texels — the viewport the
    /// shadow pass sets over it.
    ///
    /// [`TileRect::EMPTY`] for a slot no map was rendered into, and for a `slot`
    /// past [`TILES`], which is the same answer for the same reason: nothing
    /// sampled a map there.
    #[must_use]
    pub fn atlas_rect(&self, slot: usize) -> TileRect {
        self.rects.get(slot).copied().unwrap_or(TileRect::EMPTY)
    }

    /// The same rectangles as the shader reads them, ready for
    /// [`FrameUniforms::shadow_atlas_rect`](crcbl_shaders::mesh::FrameUniforms::shadow_atlas_rect):
    /// a scale into the atlas in `xy` and an offset in `zw`, one row per slot.
    #[must_use]
    pub fn atlas_rects(&self) -> [[f32; 4]; TILES] {
        self.rects.map(TileRect::to_uv)
    }

    /// Where light `index`'s run of tiles starts, if it was given one.
    ///
    /// The number a row carries: `Light::row` puts it in `GpuLight::shadow_tile`
    /// and the shader reads `light_view_proj` at it — plus the face, for a point
    /// light.
    #[must_use]
    pub fn base_of(&self, index: usize) -> Option<usize> {
        self.slots
            .iter()
            .flatten()
            .find(|assignment| assignment.light == index)
            .map(|assignment| assignment.base)
    }
}

/// Quantises a light-space position to whole `texel`-sized steps.
///
/// Separate from [`cascade_matrix`] because it is the half that is checkable on
/// its own: "the snapped value is a whole number of texels" is an assertion, and
/// "the shadow does not shimmer" is not.
///
/// All three components, not just the two across the map: the depth range is
/// derived from `z`, so leaving it unquantised would move the near and far
/// planes every frame and with them the depth a given caster stores — which is
/// the same shimmer one axis further in.
fn snap_to_texel(position: Vec3, texel: f32) -> Vec3 {
    if texel <= 0.0 || !texel.is_finite() {
        return position;
    }
    (position / texel).floor() * texel
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Projection;

    fn camera_at(eye: Vec3) -> Camera {
        Camera {
            eye,
            target: eye + Vec3::NEG_Z,
            ..Camera::default()
        }
    }

    const SUN: Vec3 = Vec3::new(0.4, 0.8, 0.6);

    /// The splits must be strictly increasing and must end exactly at the
    /// distance shadows are declared to reach.
    ///
    /// A scheme that overshot would leave the last cascade covering geometry the
    /// fragment stage has already decided is out of range; one that undershot
    /// would leave a lit gap before it.
    #[test]
    fn the_splits_increase_and_end_at_the_shadow_distance() {
        let splits = Cascades::splits(0.1);
        assert!(
            (splits[CASCADES - 1] - DISTANCE).abs() < 1e-4,
            "the last split is the shadow distance: {splits:?}"
        );
        for window in splits.windows(2) {
            assert!(
                window[1] > window[0],
                "the splits must increase: {splits:?}"
            );
        }
        assert!(splits[0] > 0.0, "a cascade with no reach: {splits:?}");
    }

    /// **The cascade must actually contain the split it names.**
    ///
    /// This is the assertion that fails if the sphere, the pull-back or the
    /// reversed-Z swap is wrong, and it is deliberately about points rather than
    /// about the matrix: a point at the edge of the cascade's reach, in every
    /// direction, must land inside the unit box with a depth in `0..1`. A
    /// projection that mapped everything outside the box would render an empty
    /// shadow map, which is the exact failure a golden image cannot see.
    #[test]
    fn every_point_within_a_cascades_reach_lands_inside_its_box() {
        let camera = camera_at(Vec3::new(3.0, 1.0, -2.0));
        let cascades = Cascades::new(&camera, SUN);
        for (index, matrix) in cascades.view_proj.iter().enumerate() {
            let radius = cascades.far[index];
            for direction in [
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::Z,
                Vec3::NEG_Z,
                Vec3::ONE.normalize(),
            ] {
                // Just inside the sphere, so the texel snap's half-texel of
                // slack cannot be what carries the point.
                let point = camera.eye + direction * (radius * 0.98);
                let clip = *matrix * point.extend(1.0);
                let ndc = clip.xyz() / clip.w;
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
                    "cascade {index} does not cover {direction:?}: ndc {ndc:?}"
                );
                assert!(
                    (0.0..=1.0).contains(&ndc.z),
                    "cascade {index} puts {direction:?} outside its depth range: ndc {ndc:?}"
                );
            }
        }
    }

    /// Reversed-Z, stated as the only thing that distinguishes it: a point
    /// nearer the light gets the **larger** depth.
    ///
    /// The whole shadow comparison is `Greater`, so a matrix that mapped depth
    /// the conventional way would invert every shadow in the scene — lighting
    /// exactly what it should darken, which reads as a bug in the light
    /// direction rather than in the projection.
    #[test]
    fn a_caster_nearer_the_light_gets_the_larger_depth() {
        let camera = camera_at(Vec3::ZERO);
        let cascades = Cascades::new(&camera, SUN);
        let to_light = SUN.normalize();
        let matrix = cascades.view_proj[0];
        let near_light = (matrix * (to_light * 2.0).extend(1.0)).z;
        let far_light = (matrix * (to_light * -2.0).extend(1.0)).z;
        assert!(
            near_light > far_light,
            "reversed-Z: {near_light} must exceed {far_light}"
        );
    }

    /// **Rotating the camera must not change a cascade at all**, which is the
    /// property the sphere buys and the reason it is a sphere.
    ///
    /// A box fitted to the split's view frustum would fail this outright, and
    /// the visible consequence is a shadow edge crawling across a wall while the
    /// player turns on the spot.
    #[test]
    fn turning_on_the_spot_leaves_every_cascade_alone() {
        let eye = Vec3::new(-2.0, 3.0, 5.0);
        let looking_north = Cascades::new(&camera_at(eye), SUN);
        let looking_east = Cascades::new(
            &Camera {
                eye,
                target: eye + Vec3::X,
                ..Camera::default()
            },
            SUN,
        );
        assert_eq!(
            looking_north, looking_east,
            "a cascade must not depend on where the camera looks"
        );
        // And the projection type is not a rotation either: an orthographic
        // camera at the same place gets the same cascades, because only the eye
        // and the near plane are read.
        let orthographic = Cascades::new(
            &camera_at(eye).with_projection(Projection::Orthographic {
                half_height: 4.0,
                near: 0.1,
                far: 50.0,
            }),
            SUN,
        );
        assert_eq!(looking_north, orthographic);
    }

    /// Sub-texel camera motion must produce a **byte-identical** matrix.
    ///
    /// This is the snap, and it is asserted as equality rather than as "close"
    /// on purpose: the shimmer it removes is caused by a difference far smaller
    /// than any tolerance a test would pick, so anything short of equality would
    /// pass with the snap deleted.
    #[test]
    fn moving_less_than_one_texel_changes_nothing() {
        let eye = Vec3::new(1.0, 2.0, 3.0);
        let splits = Cascades::splits(0.1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tile is a few thousand texels"
        )]
        let texel = 2.0 * splits[0] / TILE as f32;
        let still = Cascades::new(&camera_at(eye), SUN);
        // A tenth of a texel, along an axis the light basis is not parallel to.
        let nudged = Cascades::new(&camera_at(eye + Vec3::X * texel * 0.1), SUN);
        assert_eq!(
            still.view_proj[0], nudged.view_proj[0],
            "a sub-texel move must snap to the same origin"
        );
        // And a move of many texels must *not* — or the assertion above would
        // be passing because the matrix ignores the eye entirely.
        let moved = Cascades::new(&camera_at(eye + Vec3::X * texel * 40.0), SUN);
        assert_ne!(
            still.view_proj[0], moved.view_proj[0],
            "the cascade must follow the camera at all"
        );
    }

    /// A light straight overhead is the commonest light there is, and it is the
    /// one the obvious up vector is parallel to.
    #[test]
    fn a_light_straight_overhead_still_produces_a_basis() {
        let cascades = Cascades::new(&camera_at(Vec3::ZERO), Vec3::Y);
        for (index, matrix) in cascades.view_proj.iter().enumerate() {
            assert!(
                matrix.to_cols_array().iter().all(|value| value.is_finite()),
                "cascade {index} is not finite under an overhead sun: {matrix:?}"
            );
        }
        // And it is a real projection, not a degenerate one: the origin is
        // inside the box.
        let clip = cascades.view_proj[0] * Vec3::ZERO.extend(1.0);
        let ndc = clip.xyz() / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z));
    }

    /// The atlas is a grid of equal tiles, and every tile has one of its own.
    ///
    /// **Two tiles sharing an origin is the failure this is here for**: the
    /// shadow pass sets a viewport per tile out of `tile_origin` and the shader
    /// addresses one out of the same arithmetic, so a collision is two maps
    /// written over each other and read as one — a picture, and a plausible one.
    #[test]
    fn every_tile_of_the_grid_has_an_origin_of_its_own() {
        let (width, height) = atlas_extent();
        assert_eq!(width, TILE * ATLAS_COLUMNS);
        assert_eq!(height, TILE * ATLAS_ROWS);
        let params = Cascades::params();
        #[expect(clippy::cast_precision_loss, reason = "an extent is a few thousand")]
        let expected = [1.0 / width as f32, 1.0 / height as f32];
        assert!((params[0] - expected[0]).abs() < f32::EPSILON);
        assert!((params[1] - expected[1]).abs() < f32::EPSILON);
        assert!(params[2] > 0.0 && params[3] > 0.0, "the bias must bias");

        let origins: Vec<(u32, u32)> = (0..TILES).map(tile_origin).collect();
        for (index, origin) in origins.iter().enumerate() {
            assert!(
                origin.0 + TILE <= width && origin.1 + TILE <= height,
                "tile {index} at {origin:?} runs off a {width}x{height} atlas"
            );
            assert_eq!(
                origins.iter().filter(|other| *other == origin).count(),
                1,
                "tile {index}'s origin {origin:?} is shared with another tile"
            );
        }

        // **The cascades are the first tiles and they are in the top row**,
        // which is the arrangement the sun's goldens were blessed under: cascade
        // `i` rasterises into exactly the texels it did before the grid gained a
        // second row, so a change to the grid's shape leaves them alone.
        for cascade in 0..CASCADES {
            assert_eq!(tile_origin(cascade), (TILE * cascade as u32, 0));
        }
        // And every light tile is past every cascade's, which is the whole of the
        // split.
        for tile in 0..LIGHT_TILES {
            assert!(light_tile(tile) >= CASCADES && light_tile(tile) < TILES);
        }
    }

    fn spot_at(position: Vec3, direction: Vec3) -> SpotLight {
        SpotLight {
            position,
            radius: 4.0,
            color: Vec3::ONE,
            direction,
            inner_angle: 0.2,
            outer_angle: 0.5,
            fill: false,
        }
    }

    /// **Reversed-Z, stated as the only thing that distinguishes it**: a caster
    /// nearer the light gets the larger depth, the near plane is 1 and the
    /// radius is 0.
    ///
    /// The comparison sampler is `Greater` everywhere in this engine, so a
    /// matrix built the conventional way round would light exactly what it
    /// should darken — which reads as a bug in the cone rather than in the
    /// projection, and which no golden can tell from a scene with no occluder.
    #[test]
    fn a_caster_nearer_a_spot_gets_the_larger_depth() {
        let spot = spot_at(Vec3::new(0.0, 2.0, 0.0), Vec3::NEG_Y);
        let matrix = spot_matrix(&spot);
        let depth_at = |distance: f32| {
            let point = spot.position + Vec3::NEG_Y * distance;
            let clip = matrix * point.extend(1.0);
            (clip.xyz() / clip.w).z
        };
        let near = depth_at(0.5);
        let far = depth_at(3.0);
        assert!(
            near > far,
            "reversed-Z: a caster at 0.5 stores {near} and one at 3.0 stores {far}"
        );
        // And the two ends land where reversed-Z says: the near plane at 1 and
        // the radius at 0. An assertion about the *order* alone would pass on a
        // matrix whose range was 0.4..0.6, which is a map with almost no
        // precision in it.
        assert!(
            (depth_at(PUNCTUAL_NEAR) - 1.0).abs() < 1e-4,
            "the near plane must store 1.0, got {}",
            depth_at(PUNCTUAL_NEAR)
        );
        assert!(
            depth_at(spot.radius).abs() < 1e-4,
            "the radius must store 0.0, got {}",
            depth_at(spot.radius)
        );
        // Every depth in between is inside the range the comparison runs over.
        for step in 1..40u8 {
            let depth =
                depth_at(PUNCTUAL_NEAR + (spot.radius - PUNCTUAL_NEAR) * f32::from(step) / 40.0);
            assert!(
                (0.0..=1.0).contains(&depth),
                "a caster inside the cone stored {depth}, outside 0..1"
            );
        }
    }

    /// The map covers the cone and stops there: a point on the cone's outer edge
    /// is at the frustum's edge, and one well outside it is out of the box.
    ///
    /// A projection wider than the cone spends texels where the light has
    /// already fallen to zero; one narrower leaves a ring of the lit pool with
    /// no map behind it, which reads as a shadow that stops short of the pool's
    /// edge.
    #[test]
    fn a_spots_map_covers_its_cone_and_no_more() {
        let spot = spot_at(Vec3::new(1.0, 2.0, -1.0), Vec3::new(0.0, -1.0, 0.0));
        let matrix = spot_matrix(&spot);
        let ndc_of = |point: Vec3| {
            let clip = matrix * point.extend(1.0);
            clip.xyz() / clip.w
        };
        // A metre below the light, the cone's outer edge is `tan(outer)` out.
        let drop = 1.0;
        let edge = spot.outer_angle.tan() * drop;
        let on_edge = ndc_of(spot.position + Vec3::new(edge, -drop, 0.0));
        assert!(
            (on_edge.x.abs() - 1.0).abs() < 1e-3,
            "the cone's edge must land on the frustum's, got {on_edge:?}"
        );
        let inside = ndc_of(spot.position + Vec3::new(edge * 0.5, -drop, 0.0));
        assert!(inside.x.abs() < 1.0 && inside.y.abs() < 1.0, "{inside:?}");
        let outside = ndc_of(spot.position + Vec3::new(edge * 1.5, -drop, 0.0));
        assert!(outside.x.abs() > 1.0, "{outside:?}");
        // And behind the light is behind the light: the shader's `clip.w <= 0`
        // guard is what turns this into "lit" rather than a mirrored sample.
        let behind = matrix * (spot.position + Vec3::Y).extend(1.0);
        assert!(
            behind.w <= 0.0,
            "a point behind the spot has w {}",
            behind.w
        );
    }

    /// A spot pointing straight down is the commonest spot there is, and it is
    /// the one the obvious up vector is parallel to.
    #[test]
    fn a_spot_pointing_straight_down_still_produces_a_basis() {
        let matrix = spot_matrix(&spot_at(Vec3::new(0.0, 3.0, 0.0), Vec3::NEG_Y));
        assert!(
            matrix.to_cols_array().iter().all(|value| value.is_finite()),
            "a spot pointing down produced {matrix:?}"
        );
        let clip = matrix * Vec3::ZERO.extend(1.0);
        let ndc = clip.xyz() / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z));
    }

    /// **A still spot produces a byte-identical matrix**, which is the stability
    /// the cascades need a texel snap for and a spot gets for free.
    ///
    /// Asserted as equality rather than as "close", exactly as the cascades'
    /// snap is: shimmer is caused by a difference far below any tolerance a test
    /// would pick, so anything short of equality would pass on a matrix that
    /// moved every frame. The camera is not in the arithmetic at all, so this is
    /// a statement about `spot_matrix`'s inputs and nothing else.
    #[test]
    fn a_still_spot_produces_the_same_matrix() {
        let spot = spot_at(Vec3::new(0.5, 2.0, -1.5), Vec3::new(0.2, -1.0, 0.1));
        assert_eq!(spot_matrix(&spot), spot_matrix(&spot));
        // And a spot that *moved* does not, or the equality above would be
        // passing because the matrix ignores the light.
        let mut moved = spot;
        moved.position.x += 0.5;
        assert_ne!(spot_matrix(&spot), spot_matrix(&moved));
    }

    fn point_at(position: Vec3, radius: f32) -> PointLight {
        PointLight {
            position,
            radius,
            color: Vec3::ONE,
            fill: false,
        }
    }

    /// Whether `point` is inside the frustum `matrix` projects with, on the
    /// shader's terms exactly: `mesh.slang`'s `punctual_visibility` refuses a
    /// non-positive `w` first and then a point outside the unit box or outside
    /// the depth range.
    fn inside(matrix: Mat4, point: Vec3) -> bool {
        let clip = matrix * point.extend(1.0);
        if clip.w <= 0.0 {
            return false;
        }
        let ndc = clip.xyz() / clip.w;
        ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z)
    }

    /// **Each face covers one named direction and none of the other five**,
    /// which is the assertion a face indexing mistake cannot survive.
    ///
    /// The six directions are written out here rather than taken from
    /// [`face_axis`], and that is the whole point of the test: a table compared
    /// against itself passes on any *permutation* of the six, and a permutation
    /// is exactly what a wrong convention is. This list is `mesh.slang`'s
    /// `point_face` transcribed — it picks a face out of the largest component of
    /// the direction from the light, in this order — so a host that built its
    /// matrices in another order fails here rather than in a frame where five of
    /// the six faces sample somewhere else.
    ///
    /// It is also not "the six matrices differ" or "a caster projects somewhere
    /// sensible", both of which pass on any order at all.
    #[test]
    fn every_face_of_a_point_light_covers_its_own_direction_and_no_other() {
        let light = point_at(Vec3::new(0.5, -1.0, 2.0), 4.0);
        let convention = [
            (0, Vec3::X),
            (1, Vec3::NEG_X),
            (2, Vec3::Y),
            (3, Vec3::NEG_Y),
            (4, Vec3::Z),
            (5, Vec3::NEG_Z),
        ];
        assert_eq!(
            convention.len(),
            POINT_FACES,
            "the convention below has to name every face there is"
        );
        for (face, axis) in convention {
            let towards = light.position + axis * 2.0;
            for other in 0..POINT_FACES {
                let matrix = point_matrix(&light, other);
                assert_eq!(
                    inside(matrix, towards),
                    other == face,
                    "a caster at {towards:?} is {axis:?} of the light, which is face {face}, \
                     and face {other}'s matrix {} it",
                    if other == face {
                        "does not cover"
                    } else {
                        "covers"
                    }
                );
            }
        }
    }

    /// **The six faces leave no direction uncovered**, which is the other half:
    /// a set of matrices that each covered only their own axis and nothing
    /// between them would pass the test above and leave a shadow with holes in
    /// it wherever the light shines diagonally.
    ///
    /// A spiral of directions rather than the six axes, because the axes are the
    /// case that cannot fail — the corners between three faces are where a field
    /// of view narrower than 90° stops reaching.
    #[test]
    fn the_six_faces_between_them_cover_every_direction() {
        let light = point_at(Vec3::new(-2.0, 0.5, 1.0), 6.0);
        let matrices: Vec<Mat4> = (0..POINT_FACES)
            .map(|face| point_matrix(&light, face))
            .collect();
        for step in 0..512u32 {
            // A Fibonacci spiral: `z` walks the range and the angle turns by the
            // golden ratio, which spreads the samples over the sphere rather than
            // clustering them at its poles the way a latitude/longitude grid
            // does.
            let fraction = (f64::from(step) + 0.5) / 512.0;
            let z = 1.0 - 2.0 * fraction;
            let radius = (1.0 - z * z).max(0.0).sqrt();
            let angle = std::f64::consts::PI * (3.0 - 5.0f64.sqrt()) * f64::from(step);
            #[expect(clippy::cast_possible_truncation, reason = "a direction is an f32")]
            let direction = Vec3::new(
                (radius * angle.cos()) as f32,
                (radius * angle.sin()) as f32,
                z as f32,
            );
            let caster = light.position + direction * 3.0;
            let covered = matrices
                .iter()
                .filter(|matrix| inside(**matrix, caster))
                .count();
            assert!(
                covered >= 1,
                "no face covers a caster {direction:?} from the light, so a shadow cast that \
                 way would be missing"
            );
        }
    }

    /// **Reversed-Z on a face**, stated as the only thing that distinguishes it,
    /// and checked on every face rather than on one: a caster nearer the light
    /// gets the larger depth, the near plane stores 1 and the radius stores 0.
    ///
    /// The same claim `a_caster_nearer_a_spot_gets_the_larger_depth` makes, and
    /// it is made again rather than assumed to carry over: this is a different
    /// field of view through a different constructor call, and a matrix built the
    /// conventional way round lights exactly what it should darken.
    #[test]
    fn a_caster_nearer_a_point_lights_face_gets_the_larger_depth() {
        let light = point_at(Vec3::new(0.0, 2.0, 0.0), 4.0);
        for face in 0..POINT_FACES {
            let matrix = point_matrix(&light, face);
            let depth_at = |distance: f32| {
                let clip = matrix * (light.position + face_axis(face) * distance).extend(1.0);
                (clip.xyz() / clip.w).z
            };
            let near = depth_at(0.5);
            let far = depth_at(3.0);
            assert!(
                near > far,
                "reversed-Z on face {face}: a caster at 0.5 stores {near} and one at 3.0 \
                 stores {far}"
            );
            assert!(
                (depth_at(PUNCTUAL_NEAR) - 1.0).abs() < 1e-4,
                "face {face}'s near plane must store 1.0, got {}",
                depth_at(PUNCTUAL_NEAR)
            );
            assert!(
                depth_at(light.radius).abs() < 1e-4,
                "face {face}'s radius must store 0.0, got {}",
                depth_at(light.radius)
            );
        }
    }

    /// A point light straight over the origin is the commonest one there is, and
    /// two of its faces look along the axis the obvious up vector is.
    #[test]
    fn the_faces_that_look_along_y_still_produce_a_basis() {
        let light = point_at(Vec3::new(0.0, 3.0, 0.0), 5.0);
        for face in 0..POINT_FACES {
            let matrix = point_matrix(&light, face);
            assert!(
                matrix.to_cols_array().iter().all(|value| value.is_finite()),
                "face {face} produced {matrix:?}"
            );
        }
        // And the `-Y` face really does look at the floor under it, which is the
        // one a degenerate basis would leave empty.
        assert!(inside(point_matrix(&light, 3), Vec3::ZERO));
    }

    /// **A still light produces a byte-identical matrix**, on `spot_matrix`'s
    /// terms and for its reason: there is no texel snap here, so this is what
    /// says none is needed.
    #[test]
    fn a_still_point_light_produces_the_same_matrices() {
        let light = point_at(Vec3::new(0.5, 2.0, -1.5), 4.0);
        for face in 0..POINT_FACES {
            assert_eq!(point_matrix(&light, face), point_matrix(&light, face));
        }
        // And a light that *moved* does not, or the equality above would be
        // passing because the matrix ignores the light.
        let mut moved = light;
        moved.position.x += 0.5;
        assert_ne!(point_matrix(&light, 0), point_matrix(&moved, 0));
    }

    /// The one cull a point light gets must keep what is inside its reach and
    /// reject what is outside it.
    ///
    /// The union of the six faces is the light's sphere — topic 18's fourth
    /// decision — so this is the frustum all six draws share, and a box that
    /// rejected a caster inside the light's radius would be a missing shadow on
    /// whichever face it belonged to.
    #[test]
    fn a_point_lights_cull_keeps_what_is_inside_its_radius() {
        let light = point_at(Vec3::new(1.0, 2.0, -3.0), 4.0);
        let frustum = point_frustum(&light);
        for face in 0..POINT_FACES {
            let axis = face_axis(face);
            let near = crate::cull::Aabb {
                min: light.position + axis - Vec3::splat(0.1),
                max: light.position + axis + Vec3::splat(0.1),
            };
            assert!(
                frustum.intersects(&near),
                "a caster a unit along face {face} is inside the light's reach"
            );
            let beyond = crate::cull::Aabb {
                min: light.position + axis * 12.0 - Vec3::splat(0.1),
                max: light.position + axis * 12.0 + Vec3::splat(0.1),
            };
            assert!(
                !frustum.intersects(&beyond),
                "a caster three radii along face {face} cannot shadow anything this light lights"
            );
        }
    }

    fn spot_light_at(position: Vec3, radius: f32) -> Light {
        Light::Spot(SpotLight {
            position,
            radius,
            color: Vec3::ONE,
            direction: Vec3::NEG_Y,
            inner_angle: 0.2,
            outer_angle: 0.5,
            fill: false,
        })
    }

    /// A slot holding light `light` from tile `base` at a whole root cell, for
    /// the assertions below.
    ///
    /// Level 0 rather than a parameter because every light in the lists these
    /// assertions are built from covers more of the frame than
    /// [`WHOLE_CELL_COVERAGE`]: what they are about is which light gets a run,
    /// and the size of that run is
    /// [`the_tile_size_falls_with_the_coverage`]'s subject instead.
    const fn held(light: usize, base: usize) -> Option<Assignment> {
        Some(Assignment {
            light,
            base,
            level: 0,
        })
    }

    /// The budget goes to the lights with the largest projected influence, and a
    /// light that misses out gets no slot at all.
    ///
    /// **Radius over distance, not distance alone**: a small light close to the
    /// eye and a large one further off are the pair that tells the two rules
    /// apart, and the list below is built so that the nearest light is *not* the
    /// most influential one.
    #[test]
    fn the_tiles_go_to_the_largest_projected_influence() {
        let eye = Vec3::ZERO;
        // One light more than there are slots, so exactly one has to miss out.
        let lights = [
            // 0.5 / 1 = 0.5 — nearest, and the least influential.
            spot_light_at(Vec3::new(0.0, 0.0, -1.0), 0.5),
            // 8 / 4 = 2.0 — the most influential.
            spot_light_at(Vec3::new(0.0, 0.0, -4.0), 8.0),
            // 3 / 2 = 1.5.
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 3.0),
            // 5 / 5 = 1.0.
            spot_light_at(Vec3::new(0.0, 0.0, -5.0), 5.0),
            // 2.25 / 3 = 0.75.
            spot_light_at(Vec3::new(0.0, 0.0, -3.0), 2.25),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(eye));
        assert_eq!(
            selection.slots(),
            &[held(1, 0), held(2, 1), held(3, 2), held(4, 3)],
            "the largest influences take the slots and the first tiles, in rank order"
        );
        assert_eq!(selection.base_of(1), Some(0));
        assert_eq!(selection.base_of(2), Some(1));
        assert_eq!(selection.base_of(3), Some(2));
        assert_eq!(selection.base_of(4), Some(3));
        assert_eq!(
            selection.base_of(0),
            None,
            "the light past the budget gets no tile — it still lights, it just \
             does not occlude"
        );
    }

    /// Two lights of exactly equal influence break by index, so a frame's answer
    /// does not depend on the order a caller happened to build the list in.
    #[test]
    fn an_exact_tie_breaks_by_index() {
        // One more than there are slots, or the tie decides nothing: every light
        // would be shadowed whichever way it broke.
        let lights = [
            spot_light_at(Vec3::new(3.0, 0.0, 0.0), 1.0),
            spot_light_at(Vec3::new(-3.0, 0.0, 0.0), 1.0),
            spot_light_at(Vec3::new(0.0, 3.0, 0.0), 1.0),
            spot_light_at(Vec3::new(0.0, -3.0, 0.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, 3.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, 1), held(2, 2), held(3, 3)]
        );
        assert_eq!(
            selection.base_of(4),
            None,
            "the last light of an all-way tie is the one that loses it"
        );
    }

    /// **A light drifting across the cutoff must not flicker its shadow on and
    /// off**, which is the hysteresis and the reason it is owed.
    ///
    /// The incumbent is overtaken by a hair and keeps its tile; overtaken
    /// clearly, it loses it. Both halves, because a rule that never yields is
    /// not hysteresis — it is a first-come allocation, and the more important
    /// light would never get a map.
    #[test]
    fn an_incumbent_keeps_its_tile_until_a_challenger_clearly_beats_it() {
        // One slot's worth of contention: a list no longer than `LIGHT_SLOTS`
        // means nothing is contended, so it is one longer than the budget. The
        // light at the end is the outsider and the one before it the incumbent
        // it comes for.
        let contender = |influence: f32| spot_light_at(Vec3::new(0.0, 0.0, -1.0), influence);
        let mut lights = [
            contender(5.0),
            contender(4.0),
            contender(3.0),
            contender(2.0),
            contender(1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, 1), held(2, 2), held(3, 3)]
        );

        // The outsider edges past the incumbent by a few per cent. It does not
        // take the tile, and — this is the half that matters — the incumbent
        // stays in the *same* slot and on the *same* tile rather than being
        // reshuffled.
        lights[4] = contender(2.1);
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, 1), held(2, 2), held(3, 3)],
            "a challenger 5% ahead must not take a held tile"
        );

        // Clearly past it, and the tile changes hands.
        lights[4] = contender(2.0 * HOLD_RATIO + 0.1);
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, 1), held(2, 2), held(4, 3)],
            "a challenger past the hold ratio must take the tile"
        );
    }

    /// A light with no map at all is refused a tile rather than given one it
    /// cannot fill — and the tile goes to the next light instead.
    ///
    /// A cone at or past a right angle has no perspective projection, and a light
    /// with no radius has no frustum. Both still light.
    #[test]
    fn a_light_with_no_map_is_refused_a_tile_and_the_next_one_takes_it() {
        let mut wide = spot_light_at(Vec3::new(0.0, 0.0, -1.0), 10.0);
        if let Light::Spot(spot) = &mut wide {
            spot.outer_angle = MAX_SPOT_HALF_ANGLE + 0.01;
        }
        let lights = [
            // A point light with no reach: influence is infinite by the metric
            // and there is no frustum to render, which is why eligibility is a
            // predicate of its own rather than a consequence of the ranking.
            Light::Point(point_at(Vec3::new(0.0, 0.0, -1.0), 0.0)),
            wide,
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -3.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -4.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -5.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(2, 0), held(3, 1), held(4, 2), held(5, 3)],
            "the two brightest are ineligible, so the tiles go to the ones that are"
        );
    }

    /// **A fill light is refused a tile, where the same light not marked fill
    /// takes one** — the flag's shadow half, which is [`can_be_shadowed`]'s
    /// leading test and nothing else.
    ///
    /// The pair is the assertion. A fill light getting no tile is equally what
    /// a build that shadowed nothing at all would report, so the run before it
    /// is the same light with the flag cleared and it has to take tile 0. Only
    /// `fill` differs between the two: the position, the radius and the cone
    /// are copied from the one value.
    #[test]
    fn a_fill_light_is_refused_a_tile_where_the_same_light_takes_one() {
        // Built once and copied with the flag set, so the pair below differs in
        // `fill` and in nothing else — same position, same radius, same cone.
        let point = point_at(Vec3::new(0.0, 0.0, -2.0), 4.0);
        let spot = spot_at(Vec3::new(0.0, 0.0, -2.0), Vec3::NEG_Y);
        let pairs = [
            (
                "point",
                Light::Point(point),
                Light::Point(PointLight {
                    fill: true,
                    ..point
                }),
            ),
            (
                "spot",
                Light::Spot(spot),
                Light::Spot(SpotLight { fill: true, ..spot }),
            ),
        ];
        for (kind, casting, filling) in pairs {
            let mut selection = Selection::default();
            selection.update(&[casting], &camera_at(Vec3::ZERO));
            assert_eq!(
                selection.base_of(0),
                Some(0),
                "a {kind} light that is not fill must take the first tile, or \
                 the refusal below would pass against a build that shadows \
                 nothing"
            );
            let mut selection = Selection::default();
            selection.update(&[filling], &camera_at(Vec3::ZERO));
            assert_eq!(
                selection.base_of(0),
                None,
                "a fill {kind} light lights without occluding, so it is handed \
                 no tile at all"
            );
        }
    }

    /// **A point light and a spot are shadowed in the same frame**, which is what
    /// the light region having a tile over a whole cube buys.
    ///
    /// The budget's shape, stated as the frame a caller actually gets: the cube
    /// is [`POINT_FACES`] tiles and the region is longer than that, so the spot's
    /// map is a tile left over and both lights occlude. Until [`LIGHT_TILES`]
    /// grew past [`POINT_FACES`] this pair was the plan's documented degradation
    /// — the point took everything and the spot lit without occluding — and an
    /// ordinary lighting rig hit it.
    ///
    /// The spot's base is asserted, not just that it has one: it must land
    /// *after* the cube rather than inside it, and "it got a tile" is true
    /// either way.
    #[test]
    fn a_point_light_and_a_spot_behind_it_are_both_shadowed() {
        let lights = [
            Light::Point(point_at(Vec3::new(0.0, 0.0, -1.0), 4.0)),
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, POINT_FACES), None, None],
            "the point light's cube is the first {POINT_FACES} tiles and the spot's map is \
             the one after it"
        );
        assert_eq!(selection.base_of(0), Some(0));
        assert_eq!(selection.base_of(1), Some(POINT_FACES));
    }

    /// **Two point lights both occlude in one frame**, which is what the
    /// 2026-08-26 re-tiling of the atlas was for.
    ///
    /// Until then [`LIGHT_TILES`] was one tile past a single cube, so the second
    /// of a pair of torches was refused a run and *re-lit* the shadow its twin
    /// cast — a rig with a light either side of a walkway, which is an ordinary
    /// rig, had no working shadow at all.
    ///
    /// Both halves are asserted, because the host handing out a run is only half
    /// of a light that casts: `mesh.slang` admits a point light's cube only where
    /// the whole run is inside the region, written there as
    /// `shadow_tile <= SHADOW_LIGHT_TILES - SHADOW_POINT_FACES`, and a base of
    /// [`POINT_FACES`] failed that test on every backend while the region was
    /// seven tiles long.
    #[test]
    fn two_point_lights_are_both_shadowed() {
        let lights = [
            Light::Point(point_at(Vec3::new(-2.0, 0.0, -1.0), 4.0)),
            Light::Point(point_at(Vec3::new(2.0, 0.0, -1.0), 4.0)),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[held(0, 0), held(1, POINT_FACES), None, None],
            "the first cube takes the first {POINT_FACES} tiles and the second the \
             {POINT_FACES} after it"
        );
        for light in 0..lights.len() {
            let base = selection
                .base_of(light)
                .unwrap_or_else(|| panic!("point light {light} was given no run"));
            assert!(
                base + POINT_FACES <= LIGHT_TILES,
                "point light {light}'s run starts at {base}, which `mesh.slang` refuses: \
                 its faces run off the end of a {LIGHT_TILES}-tile region"
            );
        }
    }

    /// A point light that cannot fit does not take the budget down with it, and
    /// **the lights around it keep their tiles**.
    ///
    /// The fragmentation case, and the reason the allocation walks the whole
    /// ranking rather than stopping at the first light it cannot place. Two cubes
    /// leave [`LIGHT_TILES`] minus twice [`POINT_FACES`] tiles behind them, which
    /// is less than a cube, so a *third* point light is what no longer fits. A
    /// walk that stopped there would be a frame where a distant point light
    /// silently switched off a nearer spot's shadow.
    #[test]
    fn a_point_light_that_cannot_fit_leaves_the_lights_around_it_alone() {
        let lights = [
            Light::Point(point_at(Vec3::new(0.0, 0.0, -1.0), 4.0)),
            Light::Point(point_at(Vec3::new(0.0, 0.0, -2.0), 6.0)),
            Light::Point(point_at(Vec3::new(0.0, 0.0, -4.0), 2.0)),
            spot_light_at(Vec3::new(0.0, 0.0, -3.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(
            selection.slots(),
            &[
                held(0, 0),
                held(1, POINT_FACES),
                held(3, 2 * POINT_FACES),
                None
            ],
            "two cubes fill the region but for what is left of it, the third point light \
             does not fit in that, and the spot ranked behind it still takes a tile of it"
        );
        assert_eq!(
            selection.base_of(2),
            None,
            "the third cube has nowhere to go"
        );
    }

    /// Every run a selection hands out is inside the region and no two runs
    /// overlap, whatever the list is.
    ///
    /// **Two lights sharing a tile is the failure this is here for**: both would
    /// render into it, the second over the first, and both would sample it — a
    /// picture, and a plausible one. Asserted over a list that changes kind under
    /// a held index, which is the case the incumbents' preference for their own
    /// run has to survive.
    #[test]
    fn no_two_lights_are_ever_given_the_same_tile() {
        let spot = spot_light_at(Vec3::new(0.0, 0.0, -1.0), 4.0);
        let other = spot_light_at(Vec3::new(0.0, 0.0, -2.0), 2.0);
        let point = Light::Point(point_at(Vec3::new(0.0, 0.0, -1.0), 4.0));
        let mut selection = Selection::default();
        for lights in [
            vec![spot, other],
            // The light at index 0 is a point light now, and it wants a whole
            // cube's run where its incumbent run had one tile.
            vec![point, other],
            vec![other, point],
            vec![point],
            vec![spot, other, point],
            // More cubes than the region has runs for, which is where a light
            // that is refused one must not leave a partial run behind it.
            vec![point, point, point],
            vec![point, spot, point, other, point],
        ] {
            selection.update(&lights, &camera_at(Vec3::ZERO));
            let mut used = [false; LIGHT_TILES];
            for assignment in selection.slots().iter().flatten() {
                let span = tile_span(&lights[assignment.light]);
                assert!(
                    assignment.base + span <= LIGHT_TILES,
                    "light {} runs off the end of the region from {}",
                    assignment.light,
                    assignment.base
                );
                for tile in &mut used[assignment.base..assignment.base + span] {
                    assert!(!*tile, "two lights hold one tile: {:?}", selection.slots());
                    *tile = true;
                }
            }
        }
    }

    /// Where a spot of this list's shape has to stand for its map to cover
    /// `wanted` of the frame's height.
    ///
    /// [`coverage`] inverted, through the module's own [`map_extent`] and the
    /// camera's own scale rather than through a second copy of the arithmetic:
    /// what the assertions below are about is the *ladder* and its band, and a
    /// distance written as a literal would make them about the scorer's
    /// constants instead and go stale the moment one moved.
    fn distance_for(wanted: f32, light: &Light, camera: &Camera) -> f32 {
        map_extent(light) * camera.projection.pixels_per_unit(1.0) / wanted
    }

    /// A spot standing far enough down `-z` to cover `wanted` of the frame.
    fn spot_covering(wanted: f32, camera: &Camera) -> Light {
        let template = spot_light_at(Vec3::ZERO, 1.0);
        let distance = distance_for(wanted, &template, camera);
        spot_light_at(Vec3::new(0.0, 0.0, -distance), 1.0)
    }

    /// **A light with no history lands exactly where the ladder with no band at
    /// all would put it**, which is what makes a frame's first answer
    /// reproducible rather than a function of what ran before it.
    ///
    /// The two walks in [`tile_level`] start from the coarsest level, so the
    /// shrink walk cannot fire on a fresh light and the grow walk stops at the
    /// first threshold the coverage does not clear. Asserted against the ladder
    /// written out independently — the first level whose threshold the coverage
    /// reaches — rather than against [`tile_level`]'s own walk.
    #[test]
    fn a_light_with_no_history_lands_where_the_sharp_ladder_puts_it() {
        // A fine grid over the whole ladder and well past its top, so every rung
        // is sampled inside the band `LEVEL_HOLD_RATIO` opens under its
        // threshold as well as outside it. Coarser steps would land on the
        // thresholds themselves and miss the only coverages where a start from
        // the wrong end of the ladder gives a different answer.
        for numerator in 1..=512u32 {
            let coverage = f32::from(u16::try_from(numerator).expect("a small count")) / 512.0;
            let sharp = (0..TILE_LEVELS)
                .find(|level| coverage >= level_threshold(*level))
                .unwrap_or(TILE_LEVELS - 1);
            assert_eq!(
                tile_level(coverage, None),
                sharp,
                "a coverage of {coverage} with no history"
            );
        }
    }

    /// **A light sitting on a threshold does not flicker its tile size**, and
    /// the band is crossed in both directions.
    ///
    /// The half that a one-sided test would miss is the second pair: a rule that
    /// never shrinks is not hysteresis, it is a tile a light keeps for ever, and
    /// a scene that walked away from its lights would spend the atlas on maps
    /// nothing can see. Every coverage below is written as a fraction of
    /// [`WHOLE_CELL_COVERAGE`] so the assertion is about the band rather than
    /// about where the ladder happens to start.
    #[test]
    fn a_light_on_a_threshold_holds_its_tile_size_until_it_clearly_crosses() {
        let inside_band = WHOLE_CELL_COVERAGE * (1.0 + LEVEL_HOLD_RATIO) / 2.0;
        let past_band = WHOLE_CELL_COVERAGE * LEVEL_HOLD_RATIO * 0.95;
        assert!(
            past_band < WHOLE_CELL_COVERAGE * LEVEL_HOLD_RATIO && inside_band < WHOLE_CELL_COVERAGE,
            "both excursions must be below the threshold, or the test is about the grow walk"
        );

        // Holding a whole cell, drifting under the threshold but not out of the
        // band: the tile stays whole.
        assert_eq!(tile_level(inside_band, Some(0)), 0);
        // Clearly under it: the tile halves.
        assert_eq!(tile_level(past_band, Some(0)), 1);
        // And back the other way. Inside the band from below is *not* enough —
        // a challenger has to reach the threshold outright — and past it is.
        assert_eq!(tile_level(inside_band, Some(1)), 1);
        assert_eq!(tile_level(WHOLE_CELL_COVERAGE, Some(1)), 0);
    }

    /// **A far or narrow light takes a halving of a cell and a near, large one
    /// takes the whole of it**, which is the priority rung's whole claim and the
    /// first thing in this tree to ask [`AtlasAllocator`] for anything but level
    /// zero.
    ///
    /// Read off the rectangles rather than off the levels, because the rectangle
    /// is what the shadow pass sets its viewport to and what the shader samples
    /// through: a level that never reached the allocator would still pass an
    /// assertion about [`Assignment::level`].
    #[test]
    fn the_tile_size_falls_with_the_coverage() {
        let camera = camera_at(Vec3::ZERO);
        // One coverage comfortably inside each rung of the ladder: the top one
        // well clear of the anchor, then each threshold's midpoint below it, and
        // one under the last threshold entirely.
        let lights: Vec<Light> = (0..TILE_LEVELS)
            .map(|level| {
                let wanted = if level + 1 == TILE_LEVELS {
                    level_threshold(level - 1) * 0.5
                } else {
                    level_threshold(level) * 1.5
                };
                spot_covering(wanted, &camera)
            })
            .collect();
        let mut selection = Selection::default();
        selection.update(&lights, &camera);
        for (level, _) in lights.iter().enumerate() {
            let base = selection
                .base_of(level)
                .unwrap_or_else(|| panic!("light {level} was given no tile"));
            let rect = selection.atlas_rect(light_tile(base));
            assert_eq!(
                rect.side,
                TILE >> level,
                "light {level} was laid out at {rect:?}, not the level-{level} tile its \
                 coverage earned"
            );
        }
        // And the allocation is still a partition: every size at once is exactly
        // where a quadtree can hand out a rectangle inside another one.
        let (width, height) = atlas_extent();
        let mut covered = vec![false; (width * height) as usize];
        for slot in 0..TILES {
            let rect = selection.atlas_rect(slot);
            for row in rect.y..rect.y + rect.side {
                for column in rect.x..rect.x + rect.side {
                    let texel = (row * width + column) as usize;
                    assert!(
                        !covered[texel],
                        "texel ({column}, {row}) is in slot {slot}'s {rect:?} and in a \
                         slot laid out before it"
                    );
                    covered[texel] = true;
                }
            }
        }
    }

    /// The same claim through a whole [`Selection`], with the light *moving*:
    /// a small excursion across the threshold does not change the rectangle it
    /// is rendered into, and a large one does.
    ///
    /// The unit test above is about [`tile_level`]; this one is about the memory
    /// that feeds it. A selection that rebuilt its assignments without carrying
    /// last frame's level would pass that test and flicker here, because every
    /// frame would be a fresh light landing at the sharp answer.
    #[test]
    fn a_moving_light_keeps_its_tile_size_across_a_small_excursion() {
        let camera = camera_at(Vec3::ZERO);
        let inside_band = WHOLE_CELL_COVERAGE * (1.0 + LEVEL_HOLD_RATIO) / 2.0;
        let past_band = WHOLE_CELL_COVERAGE * LEVEL_HOLD_RATIO * 0.95;
        let mut selection = Selection::default();
        let side = |selection: &Selection| {
            selection
                .atlas_rect(light_tile(selection.base_of(0).expect("a tile")))
                .side
        };

        selection.update(
            &[spot_covering(WHOLE_CELL_COVERAGE * 2.0, &camera)],
            &camera,
        );
        assert_eq!(
            side(&selection),
            TILE,
            "a light well over the anchor is a whole cell"
        );

        selection.update(&[spot_covering(inside_band, &camera)], &camera);
        assert_eq!(
            side(&selection),
            TILE,
            "a light that drifted under the threshold but not out of the band must keep \
             the tile it had"
        );

        selection.update(&[spot_covering(past_band, &camera)], &camera);
        assert_eq!(
            side(&selection),
            TILE >> 1,
            "a light clearly under the threshold must take the halving"
        );

        selection.update(&[spot_covering(inside_band, &camera)], &camera);
        assert_eq!(
            side(&selection),
            TILE >> 1,
            "coming back inside the band is not reaching the threshold, so the tile stays \
             halved — the half of the band a one-sided test would miss"
        );

        selection.update(&[spot_covering(WHOLE_CELL_COVERAGE, &camera)], &camera);
        assert_eq!(
            side(&selection),
            TILE,
            "reaching the threshold outright takes the whole cell back"
        );
    }

    /// **A narrow cone is not demoted for being narrow.**
    ///
    /// [`coverage`] is about the map rather than about the light, which is the
    /// difference between this rung and "radius over distance": two lights of
    /// one radius at one distance spread their texels over world regions that
    /// differ by `tan(outer)`, and the narrow one already has the finer texels.
    /// Judging it by its sphere would hand the whole cell to the light that
    /// needs it least.
    #[test]
    fn a_narrow_cone_outscores_a_wide_one_of_the_same_reach() {
        let camera = camera_at(Vec3::ZERO);
        let at = Vec3::new(0.0, 0.0, -4.0);
        let mut narrow = spot_light_at(at, 1.0);
        if let Light::Spot(spot) = &mut narrow {
            spot.inner_angle = 0.05;
            spot.outer_angle = 0.1;
        }
        let wide = spot_light_at(at, 1.0);
        assert!(
            coverage(&narrow, &camera) < coverage(&wide, &camera),
            "a cone covering less of the frame must score below a wider one at the same reach"
        );
        // And the sphere metric this replaced cannot tell them apart at all,
        // which is the reason the footprint is read instead.
        assert_eq!(narrow.sphere().1, wide.sphere().1);
    }

    /// **A slot with no map hands the shader a rectangle of zeros, and that is
    /// the condition every lookup in `mesh.slang` guards on.**
    ///
    /// The host half of `crcbl_shaders::mesh`'s
    /// `no_shadow_lookup_divides_by_a_rectangle_before_it_checks_one`: that
    /// test says the shader answers "lit" before it divides by a rectangle
    /// whose `x` is not positive, and this says the rectangle it will be handed
    /// for an empty slot is exactly that. Neither half is worth anything on its
    /// own — a guard on a condition the host never produces is dead, and a host
    /// producing a condition nothing guards is
    /// `crates/crcbl/tests/forward_e2e/depth_probe.rs`'s black frame on
    /// llvmpipe 20.1.2.
    ///
    /// **And a real tile is never mistaken for an empty one**, which is the
    /// other half of a two-sided condition: the finest tile the allocator can
    /// hand out is [`MIN_TILE`] texels, so its scale into the atlas is a
    /// positive number and no map is ever read as absent.
    #[test]
    fn a_slot_with_no_map_hands_the_shader_a_rectangle_of_zeros() {
        assert_eq!(
            TileRect::EMPTY.to_uv(),
            [0.0; 4],
            "an empty slot's rectangle is what `mesh.slang`'s `atlas_rect_is_empty` tests, and \
             it tests `x` against zero"
        );

        // A frame with no shadowed light at all — `depth_probe.rs`'s frame, and
        // every light-region slot of it.
        let mut selection = Selection::default();
        selection.update(&[], &camera_at(Vec3::ZERO));
        for tile in 0..LIGHT_TILES {
            assert_eq!(
                selection.atlas_rect(light_tile(tile)).to_uv(),
                [0.0; 4],
                "light tile {tile} of a frame with no shadowed light must read as empty"
            );
        }

        // And every size a light can actually be given reads as present.
        for level in 0..TILE_LEVELS {
            let mut allocator = AtlasAllocator::new();
            let tile = allocator
                .allocate(level)
                .unwrap_or_else(|| panic!("an empty atlas has room for a level-{level} tile"));
            assert!(
                tile.rect().to_uv()[0] > 0.0,
                "a level-{level} tile reads as an empty slot, so `mesh.slang` would answer lit \
                 through a map that was rendered"
            );
        }
    }

    /// **A frame that changed nothing lays out on exactly the texels the last
    /// one did**, which is the precondition every cached shadow map rests on:
    /// the shader reads a rectangle out of `atlas_rects`, and a map not redrawn
    /// this frame is only readable through the rectangle it *was* drawn into.
    ///
    /// The rectangles alone would be a weak claim — they were equal before this
    /// rung too, because the allocator was replayed from empty in the same
    /// order every frame. What says the tiles were *kept* rather than
    /// re-derived is the second half: the allocation is compared against a
    /// selection that has only ever seen this one frame, so the layout a
    /// caching frame carries forward is still the layout a golden was blessed
    /// under.
    #[test]
    fn a_still_frame_keeps_the_texels_the_last_one_drew_into() {
        let camera = camera_at(Vec3::ZERO);
        let lights = [
            Light::Point(point_at(Vec3::new(0.0, 0.0, -3.0), 4.0)),
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 2.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera);
        let first: Vec<TileRect> = (0..TILES).map(|slot| selection.atlas_rect(slot)).collect();
        assert!(
            first[..CASCADES].iter().all(|rect| rect.side == TILE),
            "the cascades must hold whole cells for this to be about the lights: {first:?}"
        );
        for round in 0..4 {
            selection.update(&lights, &camera);
            let again: Vec<TileRect> = (0..TILES).map(|slot| selection.atlas_rect(slot)).collect();
            assert_eq!(
                again, first,
                "round {round} moved a map that nothing asked to move"
            );
        }

        let mut fresh = Selection::default();
        fresh.update(&lights, &camera);
        assert_eq!(
            selection, fresh,
            "a selection that has held its tiles for five frames must hold exactly what one \
             that has just laid them out holds — allocator included, or the atlas is drifting \
             towards a layout no golden was blessed under"
        );
    }

    /// **Every tile a light stopped needing comes back**, merged, so an atlas
    /// that has churned through lights is the atlas it started as.
    ///
    /// The leak assertion for `AtlasAllocator::release`'s only caller, and it
    /// is deliberately about the allocator rather than about the rectangles: a
    /// `lay_out` that released nothing would still hand out plausible
    /// rectangles for a long time, and would then run out of atlas on a frame
    /// nothing about the scene explains. Compared against a selection that has
    /// only ever seen the empty list, so a quarter cell left out on loan is a
    /// difference in the quadtree even while every rectangle still reads empty.
    #[test]
    fn a_light_that_leaves_hands_every_tile_it_held_back() {
        let camera = camera_at(Vec3::ZERO);
        let spot = spot_light_at(Vec3::new(0.0, 0.0, -1.0), 4.0);
        let other = spot_light_at(Vec3::new(0.0, 0.0, -2.0), 2.0);
        let point = Light::Point(point_at(Vec3::new(0.0, 0.0, -1.0), 4.0));
        let mut selection = Selection::default();
        for lights in [
            vec![spot, other],
            vec![point, other],
            vec![point],
            vec![],
            vec![spot, other, point],
            vec![point, point, point],
            vec![spot_covering(WHOLE_CELL_COVERAGE / 8.0, &camera)],
            vec![point, spot, point, other, point],
        ] {
            selection.update(&lights, &camera);
        }
        selection.update(&[], &camera);

        let mut fresh = Selection::default();
        fresh.update(&[], &camera);
        assert_eq!(
            selection, fresh,
            "the atlas is not back where it started after every light left it"
        );
    }

    /// **Demoting one light moves no other light\'s map**, which is the whole of
    /// what holding a tile across frames buys over laying the atlas out again.
    ///
    /// This is the assertion a `lay_out` that released everything and replayed
    /// the requests cannot pass, and it is the reason the retention is not
    /// merely an optimisation: the requests are spent coarsest first, so a
    /// light that demotes changes *where in that order* every request after it
    /// falls, and a replay hands the light behind it a different root. The map
    /// in that root was not redrawn — nothing about it changed — so a shader
    /// reading the new rectangle reads its neighbour\'s texels.
    ///
    /// Built so the demoting light is the one that sorts *first*: with the
    /// order the other way round a replay lands on the same answer by accident,
    /// and the test would pass without the retention being there at all.
    #[test]
    fn demoting_one_light_leaves_the_maps_around_it_where_they_were() {
        let camera = camera_at(Vec3::ZERO);
        // The first is the brighter of the two, so it takes the first run and
        // is the request the demotion reorders; the second is what must not
        // move under it.
        let near = spot_covering(WHOLE_CELL_COVERAGE * 4.0, &camera);
        let steady = spot_covering(WHOLE_CELL_COVERAGE * 2.0, &camera);
        let mut selection = Selection::default();
        selection.update(&[near, steady], &camera);
        assert_eq!(
            selection.base_of(0),
            Some(0),
            "the brighter light must hold the first run for this to be about the reordering"
        );
        let before = selection.atlas_rect(light_tile(1));
        assert_eq!(
            before.side, TILE,
            "the steady light must start on a whole cell"
        );

        selection.update(
            &[spot_covering(WHOLE_CELL_COVERAGE / 8.0, &camera), steady],
            &camera,
        );
        assert!(
            selection.atlas_rect(light_tile(0)).side < TILE,
            "the first light did not demote, so nothing reordered"
        );
        assert_eq!(
            selection.atlas_rect(light_tile(1)),
            before,
            "the steady light\'s map moved because its neighbour demoted"
        );
    }

    /// **A demotion gives the cell back whole**, so the space a light stopped
    /// needing is space the next whole-cell request can have.
    ///
    /// The merge in [`AtlasAllocator::release`] read from the renderer's side:
    /// a light demoted down the ladder splits a root, and the promotion back up
    /// can only be answered by that same root if the four quarters — and the
    /// sixteenths under them — merged on the way back.
    #[test]
    fn a_demoted_light_gives_its_cell_back_and_can_take_it_again() {
        let camera = camera_at(Vec3::ZERO);
        let near = [spot_covering(WHOLE_CELL_COVERAGE * 2.0, &camera)];
        let far = [spot_covering(WHOLE_CELL_COVERAGE / 8.0, &camera)];
        let tile = |selection: &Selection| selection.atlas_rect(light_tile(0)).side;

        let mut selection = Selection::default();
        selection.update(&near, &camera);
        assert_eq!(
            tile(&selection),
            TILE,
            "a near light must earn a whole cell"
        );

        selection.update(&far, &camera);
        let demoted = tile(&selection);
        assert!(
            demoted < TILE,
            "the far light kept a whole cell, so this test is not about a demotion"
        );

        selection.update(&near, &camera);
        assert_eq!(
            tile(&selection),
            TILE,
            "the cell the demotion released did not come back whole"
        );
        let mut fresh = Selection::default();
        fresh.update(&near, &camera);
        assert_eq!(
            selection, fresh,
            "a round trip down the ladder and back left the atlas fragmented"
        );
    }

    /// An incumbent that leaves the list — or stops being eligible — frees its
    /// tile rather than holding an index into a list that no longer has it.
    ///
    /// The list is rebuilt every frame, so an index is only meaningful against
    /// the list it came from. Carrying one across a shorter list is how a slot
    /// ends up naming a light that is not there.
    #[test]
    fn an_incumbent_that_leaves_the_list_frees_its_tile() {
        let lights = [
            spot_light_at(Vec3::new(0.0, 0.0, -1.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, &camera_at(Vec3::ZERO));
        assert_eq!(selection.slots(), &[held(0, 0), held(1, 1), None, None]);

        selection.update(&lights[..1], &camera_at(Vec3::ZERO));
        assert_eq!(selection.slots(), &[held(0, 0), None, None, None]);

        selection.update(&[], &camera_at(Vec3::ZERO));
        assert_eq!(selection.slots(), &[None; LIGHT_SLOTS]);
    }

    /// [`FILTERS`], [`r_shadow_filter`] and
    /// [`crcbl_shaders::mesh::SHADOW_FILTERS`] must name the same set, in the
    /// same order.
    ///
    /// `crate::ssao`'s `the_variable_names_are_the_techniques_labels` is this
    /// test on the other enum variable, with a third list to hold: the console's
    /// literals, this module's labels and the shader's constants are three
    /// copies of one set of names, and `mesh.slang` numbers its constants by
    /// position in the shader's list. A name that drifted would be a value a
    /// person can set and [`filter`] answers with [`Filter::Pcss`] — the
    /// selector silently doing nothing; an *order* that drifted would be a mode
    /// the shader reads as its neighbour, which is a filter nobody asked for.
    ///
    /// **It holds the names and not the enum's completeness**, on the occlusion
    /// chain's terms: a variant added to [`Filter`] and left out of [`FILTERS`]
    /// passes here, and costs nothing a frame can reach because the console
    /// cannot name it.
    #[test]
    fn the_variable_names_are_the_filters_labels() {
        let crcbl_console::Kind::Enum(names) = r_shadow_filter.kind() else {
            panic!("`r_shadow_filter` is an enum variable");
        };
        let labels: Vec<&str> = FILTERS.into_iter().map(Filter::label).collect();
        assert_eq!(names, labels.as_slice());
        assert_eq!(
            labels.as_slice(),
            crcbl_shaders::mesh::SHADOW_FILTERS.as_slice(),
            "the shader numbers its `SHADOW_FILTER_*` constants by position in its own list, \
             so a set that differs here is a mode the fragment reads as another filter"
        );
        // And the numbering itself, which is the half a set comparison cannot
        // see: `Filter::mode` searches by label, so a list in the right order
        // with the wrong lookup would still pass above.
        for (mode, filter) in FILTERS.into_iter().enumerate() {
            assert_eq!(
                filter.mode() as usize,
                mode,
                "`{}` is the shader's mode {}, not {mode}",
                filter.label(),
                filter.mode()
            );
        }
    }

    /// A frame nobody has touched the console on runs what ships, and so does
    /// the far side of the seam whatever the console holds.
    ///
    /// `crate::ssao`'s `the_seams_other_side_is_the_technique_that_ships`, and
    /// the same failure: a [`shipped_filter`] that followed the console would be
    /// a comparison of one filter against itself, which is a seam that draws one
    /// picture and says nothing.
    #[test]
    fn the_seams_other_side_is_the_filter_that_ships() {
        use crcbl_console::Value;

        let _guard = FILTER_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = r_shadow_filter.get_enum();
        assert_eq!(
            filter(),
            Filter::Pcss,
            "a frame nobody has touched the console on must sample through the filter every \
             golden in this workspace was blessed under"
        );
        let shipped_before = shipped_filter();

        r_shadow_filter
            .set(&Value::Enum(Filter::Box.label()))
            .expect("a writable enum in the set");
        // Anti-vacuity: the knob has to have actually moved, or the equality
        // below holds of a console nobody changed.
        assert_eq!(
            filter(),
            Filter::Box,
            "the getter did not follow the console"
        );
        assert_eq!(
            shipped_filter(),
            shipped_before,
            "the shipped filter moved when the console did, so the seam compares a filter \
             against itself"
        );
        assert_ne!(
            shipped_filter(),
            filter(),
            "the shipped filter is what the console holds, so both sides of the seam run one \
             kernel"
        );

        r_shadow_filter
            .set(&Value::Enum(before))
            .expect("putting the filter back");
    }

    /// A seam at either end of [`r_shadow_split`]'s range is no comparison, and
    /// the default is one of them.
    ///
    /// `crate::ssao::split_at`'s rule, asserted here because this is a second
    /// copy of it: a default that compared would put a seam through every
    /// golden in the workspace, and an end of the range that compared would ask
    /// `crate::split::halves` for a rectangle with no columns in it.
    #[test]
    fn a_seam_at_either_edge_is_no_comparison() {
        use crcbl_console::Value;

        let _guard = FILTER_SWITCH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = r_shadow_split.get_f32();
        assert_eq!(
            split_at(),
            None,
            "a frame nobody has touched the console on must draw one filter over the whole \
             target, which is what every golden in this workspace was blessed as"
        );
        for (at, wanted) in [
            (0.0, None),
            (1.0, None),
            (0.5, Some(0.5)),
            (0.25, Some(0.25)),
        ] {
            r_shadow_split
                .set(&Value::Float(at))
                .expect("a writable float in range");
            assert_eq!(split_at(), wanted, "a seam asked for at {at}");
        }
        r_shadow_split
            .set(&Value::Float(before))
            .expect("putting the seam back");
    }
}
