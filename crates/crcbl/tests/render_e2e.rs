//! The render layer, on whichever backend the registry opens — one frame of
//! every [`Scene`] through the engine's own renderers, read back and compared
//! against a checked-in golden.
//!
//! # Why this exists, and why here
//!
//! `docs/backlog.md`'s "The render layer has only ever run on Vulkan" is the
//! gap: the frame graph, the cull pass, draw generation, forward and tonemap
//! execute on `crcbl-vk` (`crates/crcbl-vk/tests/vk_e2e/mesh.rs`) and on nothing
//! else below the seam. `crcbl-mtl`'s own suite proves the *HAL* —
//! dispatch, encoders, bindings, copies — and has never constructed a
//! [`ForwardRenderer`]. A green `mtl e2e` is therefore not evidence about the
//! renderer.
//!
//! This test is in `crcbl` rather than in `crcbl-mtl` because `crcbl` is the
//! only crate that depends on the renderer *and* on the backends: `crcbl-render`
//! is above the seam and names no backend, `crcbl-mtl` is below it and names no
//! renderer. A dev-dependency from `crcbl-mtl` on `crcbl-render` would be
//! acyclic — `crcbl-render`'s manifest takes `crcbl-hal` and nothing below it —
//! but it would have to rebuild the offscreen surface, swapchain, readback and
//! row-unpadding that [`crate::screenshot`](crcbl::screenshot) already owns and
//! that `web/run-cross-backend-e2e.sh` already drives, and it could not assert
//! the thing a Metal run most needs asserted: that the *registry* picked Metal.
//!
//! # One test per scene, every backend
//!
//! [`OffscreenSetup::open`] opens whatever [`crcbl::backend::open`] selects, so
//! `CRCBL_GPU` decides which backend draws and this file is the same set of
//! tests on all of them. That is deliberate rather than incidental:
//!
//! * The golden is only trustworthy while something keeps re-deriving it, and
//!   the Metal arm cannot (see `run-render-e2e.sh` and the `mtl-e2e` job). The
//!   Vulkan arm is what stops it rotting.
//! * `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5 — a shader
//!   compiles to all four targets and *means something different on each* — has
//!   already cost this repo two real bugs (`SV_InstanceID`, `SV_VertexID`), and
//!   both were caught only by rendering one scene through two targets. MSL is a
//!   target nothing has ever crossed. Comparing Metal's frame against a
//!   Vulkan-blessed reference is the same detector pointed at the third target.
//!
//! # One reference set, shared by every backend
//!
//! **Do not split the references per backend.** The shared set _is_ the
//! cross-backend detector: a reference blessed on one backend and checked on
//! the others is the same comparison a dedicated compare script performs,
//! spread across CI jobs. A per-backend split deletes exactly the detection
//! this file exists for — every backend would then agree with itself and
//! nothing would compare them.
//!
//! `crcbl_golden::Tolerance::RASTERISER` was measured for this and not chosen:
//! radv against lavapipe differs over most of the frame on the HDR path at a
//! max channel delta of 1, which is why `max_channel_delta` is the load-bearing
//! number and the failing-pixel ratio is not. **A mean-error budget was tried and
//! rejected** — legitimate HDR drift exceeds a visible recolour regression, so
//! a budget loose enough to pass the first admits the second.
//!
//! # Why all three scenes, and why one test each
//!
//! `docs/backlog.md`'s "Decided: the four-backend compare is more scenes in
//! `render_e2e`, not a new job". This file used to draw [`Scene::Cube`] alone,
//! which exercises `mesh.slang` and `tonemap.slang`; `sprite.slang` and
//! `ui.slang` were compared across targets only by the vk-against-wgpu gate
//! that went with `crcbl-wgpu`. Those two
//! shaders are precisely the ones that have diverged per target here —
//! `SV_InstanceID` in the sprite pass, `SV_VertexID` in the mesh pool — so MSL
//! and DXIL had no comparison at all for the code with the actual history.
//!
//! One `#[test]` per scene rather than one that loops, because the name is the
//! failure report: a runner nobody can attach a debugger to prints the test
//! name and a diff, and `the_sprite_scene_…` has already said which shader
//! broke before anyone reads the numbers. It also means `-E 'test(sprite)'`
//! reruns the one scene under investigation instead of three device opens.

#![cfg(feature = "render-e2e")]

use crcbl::adapter::{ADAPTER_ENV_VAR, device_type_from_name};
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::hal::{Features, Format, GeometryPath};
use crcbl::screenshot::{OffscreenSetup, Scene};
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_render::RenderEffects;

/// What this binary calls itself in the lines [`Offscreen`] prints.
///
/// Read by `tests/offscreen/verdict.rs`, which is shared with `tiling_e2e.rs`
/// and `gltf_e2e.rs` and therefore cannot name any of them.
const SUITE: &str = "crcbl render e2e";

// The teardown, out of `tests/offscreen/` rather than in here, because the other
// two suites tear the same fixture down and a second copy is a second place a
// fix has to land. That directory holds no `main.rs`, so Cargo builds no target
// of its own from it.
#[path = "offscreen/verdict.rs"]
mod verdict;

use verdict::Offscreen;

/// The size the goldens were blessed at.
///
/// The same 256x192 the cross-backend harness and `crcbl-vk`'s mesh suite use,
/// and for the reason those state: the structural metric averages over 8x8
/// blocks, and a smaller frame gives it too few of them to mean anything.
const EXTENT: (u32, u32) = (256, 192);

/// A second size the odd-extent goldens were blessed at, chosen for being
/// awkward rather than round.
///
/// Neither dimension is a multiple of 64, and neither is a multiple of 4.
/// The vk-against-wgpu gate is where this size came from, and its
/// `CRCBL_CROSS_SIZES` said what it is for: a readback whose rows are padded to
/// a backend's own alignment — the 256-byte row pitch wgpu enforces and Vulkan
/// does not — hands back an image whose stride is wider than its width, and code
/// that assumes the two are equal produces a sheared frame at this size and a
/// correct one at every multiple of 64. [`EXTENT`] cannot catch that class,
/// because 256 already satisfies every alignment anything asks for.
const EXTENT_ODD: (u32, u32) = (97, 61);

/// The anti-vacuity floor for [`Scene::Cube`]: distinct RGBA colours the frame
/// must contain.
///
/// Two blank frames compare perfectly, so a tolerance alone cannot tell "the
/// same picture" from "no picture". Measured by that same gate on both ICDs at
/// both of its sizes: the cube scene has 44-49 distinct colours and
/// a cleared frame has one. This floor is that harness's own
/// `CRCBL_CROSS_MIN_COLORS_CUBE`, so losing the cube, the pyramid or the
/// tonemap trips it.
const MIN_COLORS_CUBE: usize = 16;

/// The same, for [`Scene::Dunes`]: distinct RGBA colours that frame must hold.
///
/// A lit height field against a flat clear, so the count is dominated by shading
/// rather than by how many objects drew, and it runs into the hundreds — the
/// narrower strip
/// [`the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone`] measures
/// prints its own figure on every run. The floor sits far below that because
/// what it has to separate is a lit surface from the clear alone and from one
/// flat quad, both of which are a handful of colours; the golden is what holds
/// the picture itself.
///
/// [`the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone`]: fn@the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone
const MIN_COLORS_DUNES: usize = 32;

/// The same, for [`Scene::Sprite`]: `CRCBL_CROSS_MIN_COLORS_SPRITE`.
///
/// That harness measured 17-24 distinct colours for this scene on both ICDs at
/// both sizes, and its floor sits just under the minimum — so losing one of the
/// three batches trips it.
const MIN_COLORS_SPRITE: usize = 16;

/// The same, for [`Scene::Lights`].
///
/// Higher than [`MIN_COLORS_CUBE`] and that is the scene's whole claim: three
/// coloured point lights falling off across the same geometry produce a
/// gradient where a single directional light produced flat faces, so a frame
/// that lost the light list would fail this floor before any golden was
/// consulted.
const MIN_COLORS_LIGHTS: usize = 64;

/// The same, for [`Scene::Spot`], and it is deliberately **not** set as high as
/// this frame will go.
///
/// The frame is one flat floor with one cone on it, so a cone contributing
/// nothing leaves a uniformly lit plane: measured at 37 distinct colours on
/// lavapipe, against a correct frame that runs past every floor here. This one
/// separates those two and stops there, because the *shape* of the cone is
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]'s claim and
/// it says far more about a wrong one than a colour count can. A cone stepped
/// rather than ramped measures 60 — over this floor, and refused a few lines
/// later with a message that names what is actually wrong with it.
///
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]: fn@the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor
const MIN_COLORS_SPOT: usize = 48;

/// The same, for [`Scene::Ui`]: `CRCBL_CROSS_MIN_COLORS_UI`.
///
/// The UI frame is the least colourful of the three — the clear, the panel, the
/// translucent bar over each of those, the outline and two text colours, which
/// that harness measured as 7 on both ICDs at both sizes. A floor of 16 would
/// fail a correct frame; this one fails a frame that lost an element.
const MIN_COLORS_UI: usize = 6;

/// How far apart a channel must be before two pixels count as different colours
/// rather than the same one off two rasterisers.
///
/// The comparison below runs at `crcbl_golden::Tolerance::RASTERISER`, whose
/// `max_channel_delta` is the largest per-channel difference two rasterisers are
/// allowed to have on the *same* picture. This threshold is several times that,
/// because the question it answers is "did anything draw here", not "is this the
/// right colour" — the golden is what answers the second one.
const PAINTED_DELTA: u8 = 8;

/// How far out from the frame's centre a [`Scene::Spot`] profile runs, in
/// pixels.
///
/// Half the frame's short axis, which is the furthest a straight line from the
/// centre stays inside it — and `crcbl::screenshot`'s `SPOT_CAMERA_UP` is chosen
/// so the cone has closed well before the end of it.
const SPOT_PROFILE_LENGTH: u32 = EXTENT.1 / 2;

/// How many samples of a [`Scene::Spot`] profile the floor outside the cone is
/// averaged over.
///
/// The last of them, which is the furthest from the cone. Averaged rather than
/// read off one pixel because it is the denominator of every claim below, and a
/// single pixel of driver dither in it moves all of them.
const SPOT_FLOOR_SAMPLES: usize = 16;

/// How far a [`Scene::Spot`] profile may rise before it counts as rising rather
/// than as two rasterisers disagreeing, in summed RGB.
///
/// Several times `crcbl_golden::Tolerance::RASTERISER`'s per-channel allowance,
/// summed over three channels — the question here is whether the pool is
/// brightest on its axis, not whether two drivers agree about a value.
const SPOT_PROFILE_NOISE: u32 = 24;

/// How many samples a [`Scene::Spot`] profile must have strictly inside the
/// penumbra band before the cone counts as a ramp rather than a step.
///
/// **This is the number the whole scene exists to produce**, so it is worth
/// saying what it is not: it is not "the pool has a gradient". A step-function
/// cone draws a pool with a gradient too, because distance falloff and Lambert
/// vary across it whatever the cone does. What a step cannot do is put pixels
/// *between* the lit floor and the dark floor, and `SPOT_HEIGHT` is chosen so
/// those other terms move the lit floor by only a few per cent — well inside the
/// band's own edges. Measured at 20 or more per direction on lavapipe and on
/// radv; this floor is under that and far above the nothing a step produces.
const SPOT_PENUMBRA_MIN: usize = 12;

/// How far the four directions' penumbra widths may differ, in samples.
///
/// The pool is a circle about the frame's centre — see `spot_camera` — so the
/// four are measuring one number four ways, and they differ only in where the
/// pixel grid falls across the ramp. A pool that is not round, or not centred,
/// fails this rather than any single direction's own claims.
const SPOT_ROUNDNESS: usize = 3;

/// The anti-vacuity floor for [`Scene::SpotShadow`].
///
/// A cone on a floor with a lit pyramid in it and a shadow behind it, so the
/// count runs well past [`MIN_COLORS_SPOT`]'s frame — this floor is the same
/// number for the same reason: it separates "the cone contributed nothing" from
/// a working frame and stops there, because *where* the dark region is is
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]'s claim.
///
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]: fn@the_caster_darkens_the_floor_behind_it_and_not_beside_it
const MIN_COLORS_SPOT_SHADOW: usize = 48;

/// The half-extent of each band [`Scene::SpotShadow`] is measured over, in
/// pixels.
///
/// Small enough that the band stays inside the region it names — the shadow is
/// about 19 pixels wide where it is sampled, and the lit floor beside it is
/// bounded by the pool's own edge — and large enough that PCF's several-texel
/// gradient and a driver's dither average out.
const SPOT_SHADOW_BAND: (u32, u32) = (6, 6);

/// Where [`Scene::SpotShadow`]'s shadow falls, in pixels.
///
/// `crcbl::screenshot`'s camera looks straight down from `SPOT_SHADOW_CAMERA_UP`
/// with `+Z` at the top of the frame, and the light is at
/// `SPOT_SHADOW_LIGHT_AT` — up and towards `+Z` — so the shadow falls towards
/// `-Z`, which is *down* the frame. The caster's own image ends about 26 pixels
/// below the centre and its shadow reaches about 92; this sits between the two,
/// on the frame's vertical axis where the shadow is widest.
const SPOT_SHADOW_DARK_AT: (u32, u32) = (EXTENT.0 / 2, EXTENT.1 / 2 + 42);

/// Its mirror image across the cone's axis: the same distance from the frame's
/// centre, on the side the light comes from.
///
/// **The same distance is the point.** Everything that varies over the pool —
/// the falloff, the cone's own ramp, Lambert — is a function of distance from
/// the cone's axis, so a band at the same distance has every one of those terms
/// at the same value and differs from the dark band in exactly one thing.
const SPOT_SHADOW_LIT_AT: (u32, u32) = (EXTENT.0 / 2, EXTENT.1 / 2 - 42);

/// And beside the caster: level with the shadow, across the frame, still inside
/// the pool.
///
/// What this refuses that the mirror does not is a frame that is simply darker
/// towards the bottom — a vignette, a wrong normal on the floor, a cone aimed a
/// little short. Each of those darkens a whole row and this band is in the same
/// row as the shadow.
const SPOT_SHADOW_SIDE_AT: (u32, u32) = (EXTENT.0 / 2 + 36, EXTENT.1 / 2 + 42);

/// How much brighter the lit bands must be than the shadowed one.
///
/// A ratio rather than a difference, for the reason `crcbl-vk`'s shadow suite
/// gives: what survives Lambert, the falloff, the cone and the tonemap is which
/// side leads, and by how much in proportion. Well above the few per cent the
/// other shading terms move across the pool and far below the several times a
/// real shadow produces.
const SPOT_SHADOW_RATIO: f32 = 1.5;

/// How far above the clear the lit half of the pool must measure.
///
/// The anti-vacuity half of the two ratios above: a frame that drew nothing has
/// every band equal to the clear, and two ratios between equal numbers say
/// nothing at all.
const SPOT_SHADOW_LIT_FLOOR: f32 = 10.0;

/// The anti-vacuity floor for [`Scene::PointShadow`].
///
/// [`MIN_COLORS_SPOT_SHADOW`]'s number for its reason: a floor lit by one
/// punctual light with two casters on it runs well past this, and what the floor
/// separates is "the light contributed nothing" from a working frame. *Where* the
/// two dark regions are is
/// [`each_caster_darkens_its_own_side_of_the_point_light`]'s claim.
///
/// [`each_caster_darkens_its_own_side_of_the_point_light`]: fn@each_caster_darkens_its_own_side_of_the_point_light
const MIN_COLORS_POINT_SHADOW: usize = 48;

/// How many pixels of the frame one world unit of [`Scene::PointShadow`]'s floor
/// is.
///
/// The camera looks straight down from `POINT_CAMERA_UP` with a 60° vertical
/// field of view, so the frame's short half-axis covers `up * tan(30°)` of floor
/// and a pixel is that over half the frame's height. Written as a constant
/// because every band below is placed in world units and converted here — a band
/// named in pixels alone is one nobody can check against the scene.
const POINT_PIXELS_PER_UNIT: f32 = (EXTENT.1 as f32 / 2.0) / (2.2 * 0.577_350_3);

/// Where a point on [`Scene::PointShadow`]'s floor lands in the frame.
///
/// **World `+X` is the frame's left and world `+Z` is its top.** The camera looks
/// down `-Y` with `+Z` up, and a right-handed basis built from those two puts
/// screen-right at `-X`: `cross((0,-1,0), (0,0,1))` is `(-1,0,0)`. Same flip, and
/// the same reason, as `crcbl-vk`'s spot suite records.
fn point_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * POINT_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * POINT_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every band below is inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// How far out along its own axis each of [`Scene::PointShadow`]'s shadows is
/// measured, in world units.
///
/// `screenshot`'s `POINT_CASTER_AT` is `0.6` and `POINT_LIGHT_UP` is `0.5`, so a
/// caster of height `0.27` throws its shadow from about `0.5` out to about `1.3`.
/// This sits in the middle of that and past the caster's own image, which the
/// camera magnifies out to about `0.7`.
///
/// **It is also past `POINT_LIGHT_UP`**, which is what puts it on a *side* face
/// of the light's map rather than on the `-Y` face under the light — the whole
/// reason this scene can tell a face selection apart from a fixed face.
const POINT_SHADOW_AT: f32 = 1.0;

/// The half-extent of each band [`Scene::PointShadow`] is measured over, in
/// pixels.
///
/// The shadow is about 30 pixels wide where it is sampled and its mirror is open
/// floor, so a band this size stays inside both while averaging over PCF's
/// several-texel gradient and a driver's dither.
const POINT_SHADOW_BAND: (u32, u32) = (6, 6);

/// How much brighter a lit band must be than the shadowed one it mirrors.
///
/// [`SPOT_SHADOW_RATIO`]'s number for its reason: what survives Lambert, the
/// falloff and the tonemap is which side leads and by how much in proportion. The
/// two bands are the same distance from the light, so every term but the shadow
/// has the same value in both.
const POINT_SHADOW_RATIO: f32 = 1.5;

/// The anti-vacuity floor for [`Scene::AreaLight`].
///
/// [`MIN_COLORS_AO`]'s number rather than the shadow scenes', and for its
/// reason: this frame is one flat floor under two smooth falloffs, so most of
/// its colours are the highlights' own ramps and a count high enough to be
/// interesting would be one that fails when a lobe gets broader. What it
/// separates is "the rectangles lit nothing" — a frame of clear colour, or one
/// flat floor under the ambient — from a working one, and the *shape* of the
/// two highlights is [`the_fill_strip_lights_the_floor_without_gleaming_on_it`]'s
/// claim.
///
/// [`the_fill_strip_lights_the_floor_without_gleaming_on_it`]: fn@the_fill_strip_lights_the_floor_without_gleaming_on_it
const MIN_COLORS_AREA_LIGHT: usize = 16;

/// How many pixels of the frame one world unit of [`Scene::AreaLight`]'s floor
/// is.
///
/// [`POINT_PIXELS_PER_UNIT`]'s arithmetic with that scene's camera height
/// swapped for `screenshot`'s `AREA_CAMERA_UP`.
const AREA_PIXELS_PER_UNIT: f32 = (EXTENT.1 as f32 / 2.0) / (2.0 * 0.577_350_3);

/// Where a point on [`Scene::AreaLight`]'s or [`Scene::FillLight`]'s floor lands
/// in the frame.
///
/// One mapping for the two scenes because they are drawn through one camera —
/// `screenshot`'s `area_camera`, which both name — so a second copy here would
/// be a second place a framing change has to land.
///
/// [`point_pixel`]'s flip for [`point_pixel`]'s reason: the camera looks down
/// `-Y` with `+Z` up, so world `+X` is the frame's left and world `+Z` is its
/// top.
fn area_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * AREA_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * AREA_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every band below is inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// How far from the frame's axis each of [`Scene::AreaLight`]'s highlights sits,
/// in world units.
///
/// **The mirror image of the strip in the floor, and it is arithmetic rather
/// than a measurement.** With the eye on the axis at `AREA_CAMERA_UP` and a
/// strip `AREA_STRIP_UP` above the floor at `AREA_STRIP_AT`, the mirror
/// direction at a floor point `p` leaves along `p` itself and reaches the
/// strip's height at `p * (1 + up / camera)` — so the point whose reflection
/// lands on the strip's centre is the strip's own `x` scaled by
/// `camera / (camera + up)`. `1.4 * 2.0 / 2.8`.
///
/// One number for both, which is what the whole comparison rests on: the two
/// bands it names are mirror images across the axis the camera stands on.
const AREA_HIGHLIGHT_AT: f32 = 1.0;

/// How far from each highlight's centre the second and third pairs of bands sit,
/// in world units.
///
/// **One distance, taken twice: once along the rectangle and once across it.**
/// That is what turns two ratios into a claim about a *shape*. The strip's
/// mirror image in the floor is `AREA_STRIP_LONG` scaled by the factor
/// [`AREA_HIGHLIGHT_AT`] uses — about `0.61` long — and a few hundredths wide,
/// and the lobe `crcbl_render::scene::PYRAMID_ROUGHNESS` gives spreads that
/// width to about a quarter of a unit. So this distance is inside the
/// reflection one way and outside it the other, and a highlight that reaches
/// equally far in both is not a rectangle's.
///
/// Swept rather than guessed. On radv the profile out from the highlight's
/// centre reads, in mean channel level, `194.6` at the centre, `192.2` at `0.40`
/// along and `188.8` at `0.48` along — the reflection's own end is at about
/// `0.60`, where it falls to `84.5` — against `98.7` at `0.40` across and `97.6`
/// here. Far enough out that the lobe's spread across the strip is spent, near
/// enough that the band is still well inside the reflection's length and inside
/// the frame: the outermost band's own edge lands five pixels off it.
const AREA_OFFSET: f32 = 0.45;

/// How far along the strips' axis the anti-vacuity band sits, in world units.
///
/// Past the end of the reflection — which runs to about `0.61`, so this band
/// carries no specular at all — and far enough out that the rectangle's
/// *diffuse* has fallen off with the distance to it. The sun's diffuse and the
/// ambient are flat over a flat floor, so neither differs between this band and
/// the one under the strip; the sun's own lobe does, being a function of the
/// view angle, and at `screenshot`'s `area_sun` key it is nowhere near the
/// `24.8` levels radv separates the two by.
const AREA_FAR: f32 = 1.0;

/// The half-extent of each band [`Scene::AreaLight`] is measured over, in
/// pixels.
///
/// **Narrow across the strips and long along them**, which is the shape of the
/// thing being measured: at [`AREA_PIXELS_PER_UNIT`] the highlight is about
/// twenty pixels across at half its height and about a hundred and ten along,
/// so a band as wide as it is tall would average most of its own area off the
/// highlight and report the floor beside it.
const AREA_BAND: (u32, u32) = (3, 6);

/// How much brighter a strip's highlight must be than the fill strip's mirror of
/// it.
///
/// [`SPOT_SHADOW_RATIO`]'s number for its reason — what survives the falloff,
/// Lambert and the tonemap is which side leads and by how much in proportion —
/// and it is a floor rather than a prediction. The two bands take identical sun,
/// ambient, occlusion and diffuse by construction (see [`Scene::AreaLight`]), so
/// everything between them is the specular lobe. radv reads `2.38` at the
/// highlight's centre and `2.48` [`AREA_OFFSET`] along it.
const AREA_HIGHLIGHT_RATIO: f32 = 1.5;

/// How far apart the two bands *across* the strips are allowed to be.
///
/// Just above one, because the claim there is that they are the **same**: at
/// [`AREA_OFFSET`] across the rectangle the reflection is spent, so the fill
/// flag has nothing left to remove and the two mirrored bands are the same
/// floor. radv reads `1.026`, so this is three times the excess actually
/// measured — and it is not a knob: a lobe that reached this far across would
/// have to be a round one, and a round one wide enough to do that fails
/// [`AREA_HIGHLIGHT_RATIO`] at the same distance along.
const AREA_ACROSS_TOLERANCE: f32 = 1.08;

/// How far above the far band the fill strip's own band must measure.
///
/// The anti-vacuity half of the ratios above, on [`SPOT_SHADOW_LIT_FLOOR`]'s
/// terms and with that constant's number: a fill light that lit nothing at all
/// would satisfy every ratio here while drawing half a frame of ambient, and
/// this is what says the light on that side is a light. radv separates the two
/// by `24.8` levels.
const AREA_FILL_LIT_FLOOR: f32 = 10.0;

/// What channel order [`OffscreenSetup::draw_and_readback`]'s bytes are in.
///
/// The same three lines as `crcbl-cli`'s `screenshot::channel_order` and
/// `vk_e2e/mesh.rs`'s, and not shared with either: `crcbl-golden` is a
/// **dev**-dependency here precisely so `png` reaches no shipped binary, so
/// `crcbl::screenshot` cannot name [`ChannelOrder`] to hand one out.
fn channel_order(format: Format) -> ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    }
}

/// Whether two pixels differ by more than driver noise, in RGB.
///
/// Alpha is ignored: the readback's alpha is the attachment's rather than
/// anything the scene chose, and both sprite and UI content is opaque by the
/// time it reaches the target.
fn differ(pixel: [u8; 4], other: [u8; 4]) -> bool {
    pixel[..3]
        .iter()
        .zip(&other[..3])
        .any(|(left, right)| left.abs_diff(*right) >= PAINTED_DELTA)
}

/// A lit cube and a pyramid, drawn by the engine's own renderer on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`draw_scene_and_match_its_golden`] for what each assertion group is for.
/// This scene's anti-vacuity claim is [`the_cube_is_lit_against_an_unpainted_corner`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_through_the_forward_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Cube,
        "cube",
        EXTENT,
        MIN_COLORS_CUBE,
        the_cube_scene_drew_its_geometry_and_every_material_column,
    );
}

/// [`Scene::Cube`]'s claims, in the order a failure would hit them: something
/// drew, each of the material row's columns did something visible, and the
/// clear behind the geometry did not leak into the occlusion along its edge.
fn the_cube_scene_drew_its_geometry_and_every_material_column(image: &Image) {
    the_cube_is_lit_against_an_unpainted_corner(image);
    the_textured_pyramid_is_quartered_and_the_plain_one_is_flat(image);
    the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one(image);
    the_clear_does_not_brighten_the_silhouette_in_front_of_it(image);
}

/// The dunes patch — `docs/plan/25-lod.md`'s cluster DAG — on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// **This is the scene that says a device with no amplification stage can draw
/// a DAG at all.** Every other resident is a flat mesh, so
/// [`Scene::Cube`]'s frame is identical whether level selection works, picks
/// nothing, or picks a level nothing then draws. Here every triangle arrives
/// through a level `draw_gen.slang` chose per instance and a bucket the CPU
/// recorded a draw for, so a selection that chose a bucket with no geometry in
/// it is a black frame rather than a passing one.
///
/// On a device that reports both a mesh stage and an amplification stage the
/// same frame comes out of the per-cluster descent instead, which is what makes
/// this a comparison between the two granularities rather than a test of one:
/// the levels differ across the surface and the silhouette does not, so the
/// golden holds for both. `crcbl-vk`'s
/// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is` is where
/// that claim is made in numbers instead of in pixels.
/// `docs/plan/18-render-features.md`'s light list, drawn.
///
/// The same geometry as the cube scene under three coloured point lights and a
/// sun turned right down, so the two goldens differ in the light list alone.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_lights_scene_draws_its_point_lights_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Lights,
        "lights",
        EXTENT,
        MIN_COLORS_LIGHTS,
        each_point_light_pools_where_it_was_put_and_nowhere_else,
    );
}

/// The three pools are where the three lights are, and the background is not
/// one of them.
///
/// **Where**, not merely *whether*: a fragment stage that ignored the froxel
/// grid and summed the whole list would light every pyramid in every colour,
/// which reads as a plausible picture and passes any "is it brighter" check.
/// Each quadrant is therefore asserted to lead in *its own* channel, which no
/// single dominant light can satisfy for all three at once.
///
/// The brightest pixel of each quadrant rather than a fixed coordinate: which
/// pixel a pyramid's lit face lands on is a rasteriser's business, and the
/// brightest one in a band that holds a pyramid and the clear behind it is on
/// the pyramid. The luminance floor is what says so — the clear is dark and
/// blue-dominant, so "leads blue" alone would be satisfiable by the background
/// and this is what refuses that.
fn each_point_light_pools_where_it_was_put_and_nowhere_else(image: &Image) {
    let clear = image.pixel(1, 1).expect("inside");
    let clear_peak = u32::from(*clear.iter().take(3).max().expect("three channels"));

    // `PYRAMID_BAND` is the left column; its mirror is the right one. The
    // pyramids sit above and below the cube's row, so the halves split them.
    let half = EXTENT.1 / 2;
    let right = (EXTENT.0 - PYRAMID_BAND.end)..EXTENT.0;
    // The order `scene_lights` places them: red over the plain pyramid, blue over
    // the blue-tinted one, green over the textured one. Which colour goes where
    // is the material's doing as much as the light's — see that function.
    let quadrants = [
        ("red", 0usize, PYRAMID_BAND, 0..half),
        ("blue", 2, right, 0..half),
        ("green", 1, PYRAMID_BAND, half..EXTENT.1),
    ];
    for (name, channel, columns, rows) in quadrants {
        let mut brightest = clear;
        let mut peak = 0u32;
        for row in rows {
            for column in columns.clone() {
                let Some(pixel) = image.pixel(column, row) else {
                    continue;
                };
                let luma: u32 = pixel.iter().take(3).map(|c| u32::from(*c)).sum();
                if luma > peak {
                    peak = luma;
                    brightest = pixel;
                }
            }
        }
        eprintln!("crcbl render e2e: lights — {name} quadrant peaks at {brightest:?}");
        assert!(
            u32::from(brightest[channel]) > clear_peak * 2,
            "the {name} quadrant's brightest pixel must be lit geometry rather than \
             the clear behind it: {brightest:?} against a clear of {clear:?}"
        );
        for other in 0..3usize {
            if other == channel {
                continue;
            }
            assert!(
                brightest[channel] > brightest[other],
                "the {name} light must lead its own channel where it was put, got {brightest:?}"
            );
        }
    }
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_lights_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Lights,
        "lights",
        MIN_COLORS_LIGHTS,
        each_point_light_pools_where_it_was_put_and_nowhere_else,
    );
}

/// `docs/plan/18-render-features.md`'s **spot** light, drawn — the one light
/// kind in the list that had no rendered pixel anywhere in the tree.
///
/// `crcbl_render::Light::row`'s unit tests already pin the conversion, including
/// the clamp that widens a caller's angles rather than inverting them, and
/// `mesh.slang`'s `spot_cone` already compiled to all four targets. None of that
/// is evidence about a cone: an inverted test, a swapped pair of angles and a
/// step where the ramp should be each compile, each read plausibly, and each
/// draws a picture. [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]
/// is what tells them apart.
///
/// [`the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor`]: fn@the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_scene_draws_its_cone_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Spot,
        "spot",
        EXTENT,
        MIN_COLORS_SPOT,
        the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Spot,
        "spot",
        MIN_COLORS_SPOT,
        the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor,
    );
}

/// The brightness of every pixel from the frame's centre outward along `step`,
/// summed over RGB.
///
/// One sample per pixel out to [`SPOT_PROFILE_LENGTH`], which is the furthest a
/// straight line from the centre of a `256x192` frame stays inside it — so the
/// four axis directions all fit and all sample the same radii.
fn spot_profile(image: &Image, step: (i32, i32)) -> Vec<u32> {
    let centre = (
        i32::try_from(EXTENT.0 / 2).expect("a frame edge fits in an i32"),
        i32::try_from(EXTENT.1 / 2).expect("a frame edge fits in an i32"),
    );
    (0..i32::try_from(SPOT_PROFILE_LENGTH).expect("a frame edge fits in an i32"))
        .map(|distance| {
            let x = centre.0 + step.0 * distance;
            let y = centre.1 + step.1 * distance;
            let pixel = image
                .pixel(
                    u32::try_from(x).expect("the profile stays inside the frame"),
                    u32::try_from(y).expect("the profile stays inside the frame"),
                )
                .expect("the profile stays inside the frame");
            pixel
                .iter()
                .take(3)
                .map(|channel| u32::from(*channel))
                .sum()
        })
        .collect()
}

/// [`Scene::Spot`]'s claim, and it is three claims: the cone has a lit core, the
/// floor outside it is dark, and **the band between them varies**.
///
/// The third is what the scene exists for, and the first two are what make it
/// mean something. Take them in the order a wrong cone hits them:
///
/// * A cone test with its sign inverted lights everything *except* the cone, so
///   the axis is dark and the far floor is bright: the core-against-floor ratio
///   goes the wrong way and fails first.
/// * A cone whose two angles arrive the wrong way round divides by a clamped
///   epsilon and comes out a step at the inner angle. That draws a lit disc
///   against a dark floor — the first two claims pass on it unchanged — and it
///   is the penumbra count that refuses it.
/// * So does a `spot_cone` written as a boolean in the first place, which is the
///   same picture by a different route.
///
/// **The penumbra count is not "the pool has a gradient".** A step-function cone
/// leaves a gradient across the pool too, because the distance falloff and
/// Lambert vary over it whatever the cone does. What a step cannot do is put
/// pixels *between* the lit floor and the dark floor, and `SPOT_HEIGHT` is
/// chosen so those other two terms move the lit floor by a few per cent — far
/// inside the band this counts, whose edges sit a fifth of the way in from each
/// end. See [`SPOT_PENUMBRA_MIN`].
///
/// Four directions rather than one, because the pool is a circle about the
/// frame's centre and four profiles of one circle are a claim about its
/// *shape*: [`SPOT_ROUNDNESS`] is what a pool that is off-centre, elongated or
/// aimed somewhere else fails, and no single profile can see any of those.
fn the_spot_cone_is_a_lit_core_a_varying_penumbra_and_dark_floor(image: &Image) {
    let mut penumbras = Vec::new();
    for (name, step) in [
        ("right", (1i32, 0i32)),
        ("left", (-1, 0)),
        ("down", (0, 1)),
        ("up", (0, -1)),
    ] {
        let profile = spot_profile(image, step);
        let core = profile[0];
        let tail = &profile[profile.len() - SPOT_FLOOR_SAMPLES..];
        let floor = tail.iter().sum::<u32>() / SPOT_FLOOR_SAMPLES as u32;

        // The floor outside the cone has to be *lit* floor rather than black, or
        // "dark outside the cone" is a claim about an unpainted frame. The sun
        // is turned right down and is what lights it — see `dim_sun`.
        assert!(
            floor > 0,
            "the floor outside the cone is black, so the sun contributed nothing and \
             the {name} profile is not a measurement of a cone against a lit surface"
        );
        assert!(
            core >= floor * 3,
            "the cone's core must be unmistakably brighter than the floor outside it: \
             the {name} profile runs {core} at the axis against a floor of {floor}"
        );
        let peak = *profile.iter().max().expect("the profile is not empty");
        assert!(
            core + SPOT_PROFILE_NOISE >= peak,
            "the pool must be brightest on the cone's axis: the {name} profile peaks at \
             {peak} against {core} at the axis"
        );

        // The band, a fifth of the way in from each end so neither edge of it can
        // be reached by the few per cent the other shading terms move across the
        // pool. Its own edges are derived from this frame's two ends rather than
        // written down, because the tonemap is what decides where a linear
        // radiance lands and it is not this test's business to model.
        let span = core - floor;
        let low = floor + span / 5;
        let high = floor + span * 4 / 5;
        let penumbra = profile
            .iter()
            .filter(|value| **value > low && **value < high)
            .count();
        eprintln!(
            "crcbl render e2e: spot — {name} runs {core} at the axis to {floor} at the edge, \
             with {penumbra} sample(s) between {low} and {high}"
        );
        assert!(
            penumbra >= SPOT_PENUMBRA_MIN,
            "the cone's penumbra holds {penumbra} sample(s) between {low} and {high} along \
             {name}, which is a step rather than a ramp — a boolean cone draws the same lit \
             disc on the same dark floor and differs from a working one only here"
        );
        penumbras.push((name, penumbra));
    }

    let widest = penumbras
        .iter()
        .map(|(_, count)| *count)
        .max()
        .expect("four");
    let narrowest = penumbras
        .iter()
        .map(|(_, count)| *count)
        .min()
        .expect("four");
    assert!(
        widest - narrowest <= SPOT_ROUNDNESS,
        "the four directions' penumbras are {penumbras:?}, which is not one circle about the \
         frame's centre — the cone is off-axis, elongated, or aimed somewhere this camera \
         does not see square on"
    );
}

/// `docs/plan/18-render-features.md`'s **shadowed spot**, drawn — the first
/// light in this engine other than the sun that occludes.
///
/// The golden is not the evidence and cannot be: a spot whose shadow lookup
/// always returned "lit" draws [`Scene::SpotShadow`] as an evenly lit pool with a
/// pyramid in it, which is a perfectly ordinary picture, and one that always
/// returned "shadowed" draws a pool that is uniformly dim. Both would be blessed
/// without comment. [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]
/// is what tells the three apart, and `crcbl-vk`'s
/// `a_spots_shadow_follows_its_caster` is what says the dark region is this
/// caster's rather than a fixed patch.
///
/// [`the_caster_darkens_the_floor_behind_it_and_not_beside_it`]: fn@the_caster_darkens_the_floor_behind_it_and_not_beside_it
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_shadow_scene_draws_its_shadow_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::SpotShadow,
        "spot_shadow",
        EXTENT,
        MIN_COLORS_SPOT_SHADOW,
        the_caster_darkens_the_floor_behind_it_and_not_beside_it,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_spot_shadow_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::SpotShadow,
        "spot_shadow",
        MIN_COLORS_SPOT_SHADOW,
        the_caster_darkens_the_floor_behind_it_and_not_beside_it,
    );
}

/// Every pixel of a block `half` wide and `half` tall about `centre`, clipped to
/// the frame.
///
/// One definition because three readers walk the same block and one of them —
/// [`predicted_block_channel`] — is a *model* of what the other two measure. Two
/// loops that have to agree pixel for pixel are two loops that will not.
fn block_pixels(centre: (u32, u32), half: (u32, u32)) -> impl Iterator<Item = (u32, u32)> {
    (centre.1.saturating_sub(half.1)..(centre.1 + half.1).min(EXTENT.1)).flat_map(move |y| {
        (centre.0.saturating_sub(half.0)..(centre.0 + half.0).min(EXTENT.0)).map(move |x| (x, y))
    })
}

/// The mean brightness of a `width` by `height` block of the frame centred on
/// `(x, y)`, summed over RGB.
///
/// A block rather than a pixel, for the reason `crcbl-vk`'s shadow suite gives:
/// PCF makes a shadow edge a gradient several texels wide, and a single sample
/// lands wherever that gradient happens to fall.
fn block_brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for (x, y) in block_pixels(centre, half) {
        let pixel = image.pixel(x, y).expect("inside the frame");
        total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
        count += 1;
    }
    assert!(count > 0, "an empty block measures nothing");
    total / (count as f32 * 3.0)
}

/// The mean of one channel over the same block [`block_brightness`] averages.
///
/// **For a claim about a hue rather than about a level.** A reflection adds the
/// reflected surface's colour to the reflector's own, so where the two are
/// different colours the change is much larger in the channel one of them has
/// least of than it is in the mean of three — see
/// [`the_floor_reflects_the_pyramid_and_only_under_it`], which is the one caller.
fn block_channel(image: &Image, centre: (u32, u32), half: (u32, u32), index: usize) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for (x, y) in block_pixels(centre, half) {
        let pixel = image.pixel(x, y).expect("inside the frame");
        total += f32::from(pixel[index]);
        count += 1;
    }
    assert!(count > 0, "an empty block measures nothing");
    total / count as f32
}

/// [`Scene::SpotShadow`]'s claim: **the floor is dark where the caster blocks
/// the light and lit where it does not**.
///
/// Three bands of the same frame, all inside the cone's pool and all on the
/// floor, and the claim is the relation between them rather than any absolute
/// colour — which is what survives Lambert, the falloff, the cone and the
/// tonemap:
///
/// * `SPOT_SHADOW_DARK_AT` is the floor just behind the caster, where the light
///   comes from `+Z` at 45° and the pyramid stands between. It must be the
///   darkest of the three.
/// * `SPOT_SHADOW_LIT_AT` is its mirror image on the far side, the same distance
///   from the frame's centre and the same distance from the cone's axis. It must
///   be unmistakably brighter.
/// * `SPOT_SHADOW_SIDE_AT` is beside the caster, level with the shadow but
///   across the frame, where nothing is between the light and the floor.
///
/// **The mirror is what makes it evidence.** A shadow lookup that always returned
/// "shadowed" darkens all three equally and fails the ratio; one that always
/// returned "lit" leaves all three equal and fails it the other way. A vignette,
/// a wrong normal or a cone aimed slightly off darkens one *side* of the frame,
/// which the third band refuses — it is level with the dark band and equally far
/// from the cone's axis, so nothing that varies with position alone can separate
/// the two.
fn the_caster_darkens_the_floor_behind_it_and_not_beside_it(image: &Image) {
    let dark = block_brightness(image, SPOT_SHADOW_DARK_AT, SPOT_SHADOW_BAND);
    let lit = block_brightness(image, SPOT_SHADOW_LIT_AT, SPOT_SHADOW_BAND);
    let side = block_brightness(image, SPOT_SHADOW_SIDE_AT, SPOT_SHADOW_BAND);
    eprintln!(
        "crcbl render e2e: spot shadow — behind the caster {dark:.1}, mirrored {lit:.1}, \
         beside it {side:.1}"
    );
    assert!(
        dark * SPOT_SHADOW_RATIO < lit,
        "the floor behind the caster must be unmistakably darker than its mirror across the \
         pool: {dark:.1} against {lit:.1}, which is not a shadow"
    );
    assert!(
        dark * SPOT_SHADOW_RATIO < side,
        "and darker than the floor beside it at the same distance from the cone's axis: \
         {dark:.1} against {side:.1} — a frame that is merely dim on one side satisfies the \
         first claim and not this one"
    );
    // The lit bands are lit *floor*, not a black frame the shadow is invisible
    // against. Without this the two ratios above are satisfiable by a scene that
    // drew nothing at all.
    let clear = block_brightness(image, (2, 2), (2, 2));
    assert!(
        lit > clear + SPOT_SHADOW_LIT_FLOOR,
        "the lit half of the pool measures {lit:.1} against a clear of {clear:.1}, so there \
         is no lit floor here for a shadow to be a shadow against"
    );
}

/// `docs/plan/18-render-features.md`'s **shadowed point light**, drawn — one
/// light occluding in two directions at once.
///
/// The golden is not the evidence and cannot be, on
/// [`the_spot_shadow_scene_draws_its_shadow_and_matches_its_golden`]'s terms and
/// one more: a point light's map is six tiles selected between per fragment, so a
/// frame in which five of the six selections are wrong still has a shadow in it —
/// the one under whichever face happens to be picked.
/// [`each_caster_darkens_its_own_side_of_the_point_light`] is what tells those
/// apart, because it asserts two shadows on two faces and the lit floor mirroring
/// each.
///
/// [`the_spot_shadow_scene_draws_its_shadow_and_matches_its_golden`]: fn@the_spot_shadow_scene_draws_its_shadow_and_matches_its_golden
/// [`each_caster_darkens_its_own_side_of_the_point_light`]: fn@each_caster_darkens_its_own_side_of_the_point_light
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_point_shadow_scene_draws_both_of_its_shadows_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::PointShadow,
        "point_shadow",
        EXTENT,
        MIN_COLORS_POINT_SHADOW,
        each_caster_darkens_its_own_side_of_the_point_light,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_point_shadow_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::PointShadow,
        "point_shadow",
        MIN_COLORS_POINT_SHADOW,
        each_caster_darkens_its_own_side_of_the_point_light,
    );
}

/// [`Scene::PointShadow`]'s claim: **each caster darkens the floor on its own
/// side of the light, and the opposite side stays lit** — which is one light
/// occluding through two different faces of its map.
///
/// Four bands of the same frame, all on the floor and all
/// [`POINT_SHADOW_AT`] from the light's axis, so the falloff and Lambert have the
/// same value in every one of them and the only thing that can separate them is
/// the shadow:
///
/// * `+X`, behind the caster at `POINT_CASTER_AT` — dark, and it is the `+X` face
///   of the light's map that says so.
/// * `-X`, its mirror — lit, because nothing stands on that side.
/// * `-Z`, behind the second caster — dark, through the `-Z` face.
/// * `+Z`, its mirror — lit.
///
/// **The pairing across faces is what makes it evidence about face selection.** A
/// lookup wired to one fixed face darkens one of the two shadow bands and lights
/// the other, which no single-shadow scene can see: the `-Z` receiver projected
/// through the `+X` face's matrix is behind that face's near plane, the shader
/// takes its "outside the map" path, and the floor there comes back fully lit. A
/// lookup that always returned "shadowed" darkens all four and fails both ratios;
/// one that always returned "lit" leaves all four equal and fails them the other
/// way.
fn each_caster_darkens_its_own_side_of_the_point_light(image: &Image) {
    let band = |x: f32, z: f32| block_brightness(image, point_pixel(x, z), POINT_SHADOW_BAND);
    let along_x = band(POINT_SHADOW_AT, 0.0);
    let mirrored_x = band(-POINT_SHADOW_AT, 0.0);
    let along_z = band(0.0, -POINT_SHADOW_AT);
    let mirrored_z = band(0.0, POINT_SHADOW_AT);
    eprintln!(
        "crcbl render e2e: point shadow — +X {along_x:.1} against -X {mirrored_x:.1}; \
         -Z {along_z:.1} against +Z {mirrored_z:.1}"
    );
    assert!(
        along_x * POINT_SHADOW_RATIO < mirrored_x,
        "the floor behind the +X caster must be unmistakably darker than the same distance out \
         on the other side: {along_x:.1} against {mirrored_x:.1}, which is not a shadow"
    );
    assert!(
        along_z * POINT_SHADOW_RATIO < mirrored_z,
        "and the floor behind the -Z caster must be darker than its own mirror: {along_z:.1} \
         against {mirrored_z:.1} — a frame that has the first shadow and not this one is a \
         frame whose face selection is stuck on one face"
    );
    // Both lit bands are lit *floor*, not a black frame the shadows are
    // invisible against. Without this the two ratios are satisfiable by a scene
    // that drew nothing at all.
    let clear = block_brightness(image, (2, 2), (2, 2));
    for (name, lit) in [("-X", mirrored_x), ("+Z", mirrored_z)] {
        assert!(
            lit > clear + SPOT_SHADOW_LIT_FLOOR,
            "the {name} band measures {lit:.1} against a clear of {clear:.1}, so there is no lit \
             floor here for a shadow to be a shadow against"
        );
    }
}

/// `docs/plan/44-lighting.md`'s **rectangular area light**, drawn — and the
/// first frame in the tree with a fill light in it;
/// [`the_fill_light_scene_draws_two_gleams_of_four_and_matches_its_golden`] is
/// the same claim on the two punctual kinds.
///
/// [`the_fill_light_scene_draws_two_gleams_of_four_and_matches_its_golden`]: fn@the_fill_light_scene_draws_two_gleams_of_four_and_matches_its_golden
///
/// The golden is half of the evidence and cannot be the other half:
/// `mesh.slang`'s linearly transformed cosine path draws *a* bright band under
/// a strip whether or not it is reading the rectangle's corners, and a fill flag
/// wired to nothing draws two identical bands. The picture is what says the
/// frame is the reviewed one;
/// [`the_fill_strip_lights_the_floor_without_gleaming_on_it`] is what says which
/// of those it is.
///
/// [`the_fill_strip_lights_the_floor_without_gleaming_on_it`]: fn@the_fill_strip_lights_the_floor_without_gleaming_on_it
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_area_light_scene_draws_its_strip_highlight_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::AreaLight,
        "area_light",
        EXTENT,
        MIN_COLORS_AREA_LIGHT,
        the_fill_strip_lights_the_floor_without_gleaming_on_it,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_area_light_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::AreaLight,
        "area_light",
        MIN_COLORS_AREA_LIGHT,
        the_fill_strip_lights_the_floor_without_gleaming_on_it,
    );
}

/// [`Scene::AreaLight`]'s claim, in three parts: **the rectangle gleams along
/// its own length and not across it, and the fill flag takes the gleam away and
/// leaves the light.**
///
/// Four mirrored pairs of bands, and the mirror is what makes each one evidence.
/// The camera stands on the axis the two strips are mirrored about, the floor is
/// one plane, and the sun points straight down — so the two bands of a pair are
/// the same distance from the eye, carry the same normal, and take the same
/// directional diffuse, ambient, occlusion, Lambert and falloff. The specular
/// lobe the fill flag removes is the only term that can separate them.
///
/// * **At each highlight's centre**, [`AREA_HIGHLIGHT_AT`] out on each side. The
///   ordinary strip's band must lead its fill mirror by
///   [`AREA_HIGHLIGHT_RATIO`]. A fill flag that never reached the specular term
///   leaves the two equal.
/// * **[`AREA_OFFSET`] out along the strips.** It must still lead by that ratio,
///   which is what says the highlight has the rectangle's *length*.
/// * **[`AREA_OFFSET`] out across them** — the same distance from the same
///   centre. Here the two must be equal to within
///   [`AREA_ACROSS_TOLERANCE`].
///
/// **The second and third are one claim and neither is it alone.** A point light
/// wearing a rectangle's row draws a round highlight, and a round highlight
/// reaches the same distance whichever way it is walked: wide enough to pass the
/// third's equality and it is too narrow to pass the second's ratio, wide enough
/// to pass the second and it fails the third. Taken at one distance in two
/// directions, the pair is the shape of the thing rather than its brightness —
/// and shape is the whole of what a polygon integral adds to a punctual light.
///
/// The fourth pair is the anti-vacuity floor, and it is on the fill side alone:
/// its band against one [`AREA_FAR`] out along the strip, which must lead by
/// [`AREA_FILL_LIT_FLOOR`]. Both take the same flat ambient and the same
/// straight-down sun, so what separates them is the fill rectangle's own diffuse
/// falling off with the distance to it — which is what says the three ratios
/// above are not a comparison against a half-frame the flag switched off.
fn the_fill_strip_lights_the_floor_without_gleaming_on_it(image: &Image) {
    let band = |x: f32, z: f32| block_brightness(image, area_pixel(x, z), AREA_BAND);
    let lit_centre = band(-AREA_HIGHLIGHT_AT, 0.0);
    let fill_centre = band(AREA_HIGHLIGHT_AT, 0.0);
    let lit_along = band(-AREA_HIGHLIGHT_AT, AREA_OFFSET);
    let fill_along = band(AREA_HIGHLIGHT_AT, AREA_OFFSET);
    let lit_across = band(-AREA_HIGHLIGHT_AT - AREA_OFFSET, 0.0);
    let fill_across = band(AREA_HIGHLIGHT_AT + AREA_OFFSET, 0.0);
    let fill_far = band(AREA_HIGHLIGHT_AT, AREA_FAR);
    eprintln!(
        "crcbl render e2e: area light — centre {lit_centre:.1} against {fill_centre:.1}; \
         {AREA_OFFSET} along {lit_along:.1} against {fill_along:.1}; {AREA_OFFSET} across \
         {lit_across:.1} against {fill_across:.1}; the fill strip {fill_centre:.1} against \
         {fill_far:.1} at its end"
    );
    assert!(
        fill_centre * AREA_HIGHLIGHT_RATIO < lit_centre,
        "the strip's own highlight must be unmistakably brighter than the fill strip's mirror \
         of it: {lit_centre:.1} against {fill_centre:.1}, which is not a lobe the flag removed"
    );
    assert!(
        fill_along * AREA_HIGHLIGHT_RATIO < lit_along,
        "and it must still lead {AREA_OFFSET} of a unit along the rectangle: {lit_along:.1} \
         against {fill_along:.1} — a highlight that dies this far along is one the \
         rectangle's length had no say in"
    );
    assert!(
        lit_across < fill_across * AREA_ACROSS_TOLERANCE,
        "but the same distance across the rectangle the two must be the same floor: \
         {lit_across:.1} against {fill_across:.1}, so this highlight reaches as far across the \
         strip as along it and it is a point light's lobe rather than a polygon's"
    );
    assert!(
        fill_centre > fill_far + AREA_FILL_LIT_FLOOR,
        "the fill strip must still light the floor under it: {fill_centre:.1} against \
         {fill_far:.1} at the strip's end, so the ratios above are a comparison against a \
         half-frame the fill flag switched off"
    );
}

/// The anti-vacuity floor for [`Scene::FillLight`].
///
/// [`MIN_COLORS_AREA_LIGHT`]'s number for its reason: this frame is one flat
/// floor under four smooth falloffs, so most of its colours are the highlights'
/// own ramps and a count high enough to be interesting would be one that fails
/// when a lobe gets broader. What it separates is "the four lights lit nothing"
/// — a frame of clear colour, or one flat floor under the ambient — from a
/// working one, and *which* of the four gleams is
/// [`the_fill_lights_light_the_floor_without_gleaming_on_it`]'s claim.
///
/// [`the_fill_lights_light_the_floor_without_gleaming_on_it`]: fn@the_fill_lights_light_the_floor_without_gleaming_on_it
const MIN_COLORS_FILL_LIGHT: usize = 16;

/// How far from the frame's axis each of [`Scene::FillLight`]'s four highlights
/// sits, in world units.
///
/// [`AREA_HIGHLIGHT_AT`]'s arithmetic, and it is arithmetic rather than a
/// measurement for that constant's reason: with the eye on the axis at
/// `screenshot`'s `AREA_CAMERA_UP` and a light `FILL_LIGHT_UP` above the floor,
/// the floor point whose mirror direction reaches the light is the light's own
/// horizontal offset scaled by `camera / (camera + up)`. `1.4 * 2.0 / 2.8`.
///
/// One number for all four, which is what the comparison rests on: each pair's
/// two bands are mirror images across the axis the camera stands on.
const FILL_HIGHLIGHT_AT: f32 = 1.0;

/// How far along `z` each pair's highlights sit from the frame's centre: the
/// point pair on `-z`, the spot pair on `+z`.
///
/// [`FILL_HIGHLIGHT_AT`]'s scaling applied to the other coordinate, because the
/// mirror direction carries the whole horizontal offset and not just its `x`.
/// `0.7 * 2.0 / 2.8`.
const FILL_HIGHLIGHT_Z: f32 = 0.5;

/// How far out along `x` each of [`Scene::FillLight`]'s lights hangs, in world
/// units — which is where the floor directly under it is.
///
/// `screenshot`'s `FILL_LIGHT_AT` unchanged. The *pool* is under the light and
/// the *gleam* is at [`FILL_HIGHLIGHT_AT`], and the two being different places
/// is what lets one band say the fill light lights while another says it does
/// not gleam.
const FILL_POOL_AT: f32 = 1.4;

/// How far along `z` each pool sits from the frame's centre, on the same terms.
///
/// `screenshot`'s `FILL_PAIR_Z` unchanged.
const FILL_POOL_Z: f32 = 0.7;

/// The half-extent of each band [`Scene::FillLight`] is measured over, in
/// pixels.
///
/// **Square, where [`AREA_BAND`] is long and narrow**, and that is the shape of
/// the thing being measured: a punctual light's highlight on a flat floor under
/// an overhead eye is round. Swept out from the highlight's centre, the profile
/// on radv reads `203.4` at the centre against `115.5` and `103.0` one tenth of
/// a unit either side, so at `screenshot`'s framing the gleam is about a dozen
/// pixels across and a band this size stays inside it.
const FILL_BAND: (u32, u32) = (3, 3);

/// How much brighter a lit light's highlight must be than its fill twin's
/// mirror of it.
///
/// A floor rather than a prediction, and a much higher one than
/// [`AREA_HIGHLIGHT_RATIO`] because a punctual light's lobe is tighter than a
/// strip's: the two bands take identical sun, ambient, occlusion and diffuse by
/// construction (see [`Scene::FillLight`]), so everything between them is the
/// specular lobe.
///
/// Swept rather than guessed. radv reads `5.42` at the point pair's highlight
/// and `4.84` at the spot pair's; lavapipe reads `5.42` and `4.83`, which is the
/// whole spread between a hardware driver and the software rasteriser CI
/// approximates. This is under half the smaller of them.
const FILL_HIGHLIGHT_RATIO: f32 = 2.5;

/// How far apart the mirrored pair of bands *between* the two highlights is
/// allowed to be, in either direction.
///
/// **Where the claim is that the two halves are the same floor.** The band sits
/// at [`FILL_HIGHLIGHT_AT`] from the axis — the same distance from the eye and
/// from the axis as every highlight — and midway along `z` between the point
/// pair's row and the spot pair's, where each lobe is spent. What is left there
/// is the diffuse, the ambient and the sun, and the mirror makes all three
/// equal.
///
/// Two-sided, and that is not decoration: the term the mirror does *not* carry
/// by construction is the shadow, since a fill light is refused a tile and its
/// twin takes one. Nothing in this frame casts, so the tile resolves to lit —
/// but a bias or a filter that darkened the lit half would show up here as a
/// ratio *under* one, which a one-sided bound would pass. Swept: radv reads
/// `1.0169` and lavapipe `1.0164`, and the widest excursion anywhere in the
/// frame's mirrored profile is `0.984` in the lit spot's penumbra. This is
/// several times either.
const FILL_MIRROR_TOLERANCE: f32 = 1.08;

/// How far above the frame's axis each fill light's own pool must measure.
///
/// The anti-vacuity half, on [`AREA_FILL_LIT_FLOOR`]'s terms: a fill light that
/// lit nothing at all would satisfy every ratio above while drawing half a frame
/// of ambient. The pool is the floor directly under the fill light; the band it
/// is compared against is on the mirror plane at the same `z`, which is
/// **further** from that light than the pool is and *nearer* to its lit twin —
/// so a fill light contributing nothing would leave the pool the darker of the
/// two and this would go red rather than merely narrow.
///
/// Swept: radv separates the point pair's by `10.3` levels and the spot pair's
/// by `20.6`, lavapipe by `10.6` and `20.7`. Half the smaller.
const FILL_LIT_FLOOR: f32 = 5.0;

/// `docs/plan/44-lighting.md`'s **fill flag on a point light and on a spot**,
/// drawn.
///
/// The golden is half of the evidence and cannot be the other half: four
/// punctual lights over a floor draw four pools whether or not any of them is a
/// fill light, and a `Light::row` that set
/// [`FLAG_FILL`](crcbl_shaders::light::FLAG_FILL) for a rectangle alone draws a
/// frame with four gleams in it that still looks like a lighting rig. The
/// picture is what says the frame is the reviewed one;
/// [`the_fill_lights_light_the_floor_without_gleaming_on_it`] is what says which
/// of those it is.
///
/// [`the_fill_lights_light_the_floor_without_gleaming_on_it`]: fn@the_fill_lights_light_the_floor_without_gleaming_on_it
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_fill_light_scene_draws_two_gleams_of_four_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::FillLight,
        "fill_light",
        EXTENT,
        MIN_COLORS_FILL_LIGHT,
        the_fill_lights_light_the_floor_without_gleaming_on_it,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_fill_light_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::FillLight,
        "fill_light",
        MIN_COLORS_FILL_LIGHT,
        the_fill_lights_light_the_floor_without_gleaming_on_it,
    );
}

/// [`Scene::FillLight`]'s claim, in three parts: **a point light and a spot each
/// gleam where they light, the fill flag takes the gleam from both kinds, and
/// what it takes is the gleam and not the light.**
///
/// Two mirrored pairs of bands, one per kind, and the mirror is what makes each
/// one evidence. The camera stands on the axis the four lights are mirrored
/// about, the floor is one plane, and the sun points straight down — so the two
/// bands of a pair are the same distance from the eye, carry the same normal,
/// and take the same directional diffuse, ambient, occlusion, Lambert and
/// falloff. The specular lobe the fill flag removes is the only term that can
/// separate them.
///
/// * **At the point pair's highlight**, [`FILL_HIGHLIGHT_AT`] out on each side
///   and [`FILL_HIGHLIGHT_Z`] along `-z`. The lit light's band must lead its
///   fill twin's by [`FILL_HIGHLIGHT_RATIO`].
/// * **At the spot pair's**, the same distance out and as far along `+z`. It
///   must lead by the same ratio, and that is the half of this the rectangle's
///   frame could never make: `mesh.slang` drops the lobe off one flag rather
///   than off the kind, so a row builder that set the bit for one kind and not
///   another passes one of these two and fails the other.
/// * **Midway between the two rows**, at the same distance from the axis. Here
///   the two must agree to within [`FILL_MIRROR_TOLERANCE`] in *either*
///   direction — which is what says the flag removed a lobe rather than dimming
///   a light, and what would catch a shadow term arriving on the lit half alone.
///
/// The fourth and fifth bands are the anti-vacuity floor and they are on the
/// fill side alone: each fill light's own pool against the frame's axis at the
/// same `z`, which must lead by [`FILL_LIT_FLOOR`]. Both take the same flat
/// ambient and the same straight-down sun, and the axis band is *nearer* to the
/// lit twin than the pool is — so what separates them is the fill light's own
/// diffuse, which is what says the three claims above are not a comparison
/// against a half-frame the flag switched off.
fn the_fill_lights_light_the_floor_without_gleaming_on_it(image: &Image) {
    let band = |x: f32, z: f32| block_brightness(image, area_pixel(x, z), FILL_BAND);
    let point_lit = band(-FILL_HIGHLIGHT_AT, -FILL_HIGHLIGHT_Z);
    let point_fill = band(FILL_HIGHLIGHT_AT, -FILL_HIGHLIGHT_Z);
    let spot_lit = band(-FILL_HIGHLIGHT_AT, FILL_HIGHLIGHT_Z);
    let spot_fill = band(FILL_HIGHLIGHT_AT, FILL_HIGHLIGHT_Z);
    let mirror_lit = band(-FILL_HIGHLIGHT_AT, 0.0);
    let mirror_fill = band(FILL_HIGHLIGHT_AT, 0.0);
    let point_pool = band(FILL_POOL_AT, -FILL_POOL_Z);
    let point_axis = band(0.0, -FILL_POOL_Z);
    let spot_pool = band(FILL_POOL_AT, FILL_POOL_Z);
    let spot_axis = band(0.0, FILL_POOL_Z);
    eprintln!(
        "crcbl render e2e: fill light — point {point_lit:.1} against {point_fill:.1}; spot \
         {spot_lit:.1} against {spot_fill:.1}; between them {mirror_lit:.1} against \
         {mirror_fill:.1}; the fill pools {point_pool:.1} and {spot_pool:.1} against \
         {point_axis:.1} and {spot_axis:.1} on the axis"
    );
    for (kind, lit, filled) in [
        ("point light", point_lit, point_fill),
        ("spot", spot_lit, spot_fill),
    ] {
        assert!(
            filled * FILL_HIGHLIGHT_RATIO < lit,
            "the lit {kind}'s highlight must be unmistakably brighter than its fill twin's \
             mirror of it: {lit:.1} against {filled:.1}, which is not a lobe the flag removed"
        );
    }
    assert!(
        mirror_lit < mirror_fill * FILL_MIRROR_TOLERANCE
            && mirror_fill < mirror_lit * FILL_MIRROR_TOLERANCE,
        "but between the two highlights, where neither lobe reaches, the two halves must be \
         the same floor: {mirror_lit:.1} against {mirror_fill:.1} — so the flag is dimming a \
         light rather than removing its lobe, or the lit half is carrying a shadow term its \
         twin was refused a tile for"
    );
    for (kind, pool, axis) in [
        ("point light", point_pool, point_axis),
        ("spot", spot_pool, spot_axis),
    ] {
        assert!(
            pool > axis + FILL_LIT_FLOOR,
            "the fill {kind} must still light the floor under it: {pool:.1} against {axis:.1} \
             on the axis, which is further from it and nearer its lit twin — so the ratios \
             above are a comparison against a half-frame the fill flag switched off"
        );
    }
}

/// The anti-vacuity floor for [`Scene::Ao`].
///
/// Lower than the shadow scenes', and deliberately: this frame is one flat floor
/// under one ambient term, so most of its colours are the occlusion gradient
/// itself and a count high enough to be interesting would be a count that fails
/// when the gradient gets smoother. What it separates is "the box drew nothing"
/// — a frame of clear colour, or of one flat unlit floor — from a working one,
/// and the *shape* of the darkening is
/// [`the_corner_is_occluded_and_the_open_floor_is_not`]'s claim.
///
/// [`the_corner_is_occluded_and_the_open_floor_is_not`]: fn@the_corner_is_occluded_and_the_open_floor_is_not
const MIN_COLORS_AO: usize = 16;

/// How many pixels of the frame one world unit of [`Scene::Ao`]'s floor is.
///
/// [`POINT_PIXELS_PER_UNIT`]'s arithmetic with that scene's camera height
/// swapped for `screenshot`'s `AO_CAMERA_UP`: the frame's short half-axis covers
/// `up * tan(30°)` of floor and a pixel is that over half the frame's height.
const AO_PIXELS_PER_UNIT: f32 = (EXTENT.1 as f32 / 2.0) / (2.2 * 0.577_350_3);

/// Where a point on [`Scene::Ao`]'s floor lands in the frame.
///
/// [`point_pixel`]'s flip for [`point_pixel`]'s reason: the camera looks down
/// `-Y` with `+Z` up, so world `+X` is the frame's left and world `+Z` is its
/// top.
fn ao_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * AO_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * AO_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every band below is inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// The half-extent of each band [`Scene::Ao`] is measured over, in pixels.
///
/// `screenshot`'s `AO_TROUGH` puts each wall `0.8` from the frame's centre, so at
/// [`AO_PIXELS_PER_UNIT`] a wall's base is about 60 pixels out and the band below
/// sits about 11 pixels inside it. Five is what fits between the two with the
/// whole band still on floor, and it is wide enough that the 4×4 box blur
/// `ssao_blur.slang` applies has averaged over every pixel of it.
const AO_BAND: (u32, u32) = (5, 5);

/// How far from the frame's centre every band sits, in world units.
///
/// **One number for all four, and that is what the claim rests on.** The camera
/// looks straight down, so four points the same distance from the axis on the same
/// flat floor are the same distance from the eye, carry the same normal, and — the
/// sun being directional — receive exactly the same direct light. Two of the four
/// are against a wall and two are on open floor; occlusion is the only term left
/// that can separate them.
///
/// A tenth of a unit short of `screenshot`'s `AO_TROUGH` half-width, so the two
/// across-bands are pressed into the corner where floor meets wall and the two
/// along-bands are most of `AO_RUN` from either end.
const AO_BAND_AT: f32 = 0.7;

/// How much brighter the open floor must be than the floor against a wall.
///
/// A ratio rather than a difference, on [`SPOT_SHADOW_RATIO`]'s terms — and a much
/// smaller number than that one, for a reason worth writing down rather than
/// tuning around. **These bands are read after the sRGB encode**, which is very
/// nearly a `0.42` power: a term that halves in linear light moves by about a
/// quarter here. Occlusion also scales the ambient alone, so even a wall that
/// closes half the hemisphere over a pixel cannot halve what that pixel shows.
///
/// **Re-measured when the multi-bounce tint landed, and the separation it
/// guards is genuinely smaller now.** The frame this was first set against
/// measured the wall bands about a sixth below the open ones — a ratio of
/// `1.198`, which `1.10` sat under with room to spare. `mesh.slang`'s
/// `multi_bounce_occlusion` then began lifting the occluded bands, because
/// light bouncing off a bright floor is exactly what a scalar horizon integral
/// omits, and the same bands now read `70.6` against `74.7`: a ratio of
/// `1.058`. That is the published fit doing what it is for rather than the
/// occlusion weakening — the lift it applies at this scene's albedo accounts
/// for the whole of the difference.
///
/// So this is a floor set at roughly half the measured separation, still far
/// over the single-digit drift `crcbl_golden::Tolerance::RASTERISER` was
/// measured for, and it still separates what it was written to separate: a pass
/// that wrote a constant, or one whose result never reached the shading line,
/// leaves every band equal and lands at exactly `1.00`. What it has lost is
/// margin — `docs/backlog.md` carries that, and the AO intensity control that
/// would buy it back.
const AO_RATIO: f32 = 1.03;

/// How far above the clear the open bands must measure.
///
/// [`SPOT_SHADOW_LIT_FLOOR`]'s job: a frame that drew nothing has every band equal
/// to the clear, and a ratio between equal numbers says nothing.
const AO_LIT_FLOOR: f32 = 10.0;

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ao_scene_occludes_its_corner_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ao,
        "ao",
        EXTENT,
        MIN_COLORS_AO,
        the_corner_is_occluded_and_the_open_floor_is_not,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ao_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Ao,
        "ao",
        MIN_COLORS_AO,
        the_corner_is_occluded_and_the_open_floor_is_not,
    );
}

/// [`Scene::Ao`]'s claim: **the floor is darker where a wall closes over it and
/// not where the same floor is open.**
///
/// **The golden cannot make this claim and no golden ever could.** An occlusion
/// pass that wrote a constant `1.0` draws a perfectly plausible frame — a flat
/// floor under a flat ambient, which is what a trough lit this way looks like —
/// and it would be blessed without comment. So would one whose reconstructed
/// normal points into the surface, which occludes every pixel and produces the
/// same frame a step darker. Both of those are a *uniform* change, and every
/// assertion here is a ratio between bands of the same frame, which no uniform
/// change can move.
///
/// Four bands, all [`AO_BAND_AT`] from the frame's centre on the same flat floor:
///
/// * `+Z` and `-Z`, each a tenth of a unit off one of the trough's two walls.
/// * `+X` and `-X`, the same distance out along the trough's run, where the
///   nearest wall is most of `screenshot`'s `AO_RUN` away.
///
/// **The four share a distance from the eye, and that is what a vignette cannot
/// survive.** Anything that darkens with distance from the frame's centre — a
/// vignette, a falloff, an occlusion pass whose radius is being read in screen
/// space — moves all four together and satisfies none of the ratios. Anything
/// that darkens one side of the frame — a flipped `SV_Position.y` in
/// `ssao.slang`, a light pointing where it should not — moves one band of a pair
/// and is caught by the other. Only occlusion separates the axes.
fn the_corner_is_occluded_and_the_open_floor_is_not(image: &Image) {
    let band = |x: f32, z: f32| block_brightness(image, ao_pixel(x, z), AO_BAND);
    let near_wall = [
        ("+Z", band(0.0, AO_BAND_AT)),
        ("-Z", band(0.0, -AO_BAND_AT)),
    ];
    let open = [
        ("+X", band(AO_BAND_AT, 0.0)),
        ("-X", band(-AO_BAND_AT, 0.0)),
    ];
    eprintln!(
        "crcbl render e2e: ao — against the walls {:.1} and {:.1}, open floor {:.1} and {:.1}",
        near_wall[0].1, near_wall[1].1, open[0].1, open[1].1,
    );
    for (wall, dark) in near_wall {
        for (axis, lit) in open {
            assert!(
                dark * AO_RATIO < lit,
                "the floor against the {wall} wall must be unmistakably darker than the floor \
                 the same distance out along {axis}, which is open: {dark:.1} against {lit:.1} \
                 — that is not occlusion"
            );
        }
    }
    // The open bands are lit *floor*, not a black frame the corner is invisible
    // against. Without this the ratios above are satisfiable by a scene that drew
    // nothing at all.
    let clear = block_brightness(image, (2, 2), (2, 2));
    for (axis, lit) in open {
        assert!(
            lit > clear + AO_LIT_FLOOR,
            "the {axis} band measures {lit:.1} against a clear of {clear:.1}, so there is no lit \
             floor here for occlusion to be occlusion against"
        );
    }
}

/// The anti-vacuity floor for [`Scene::Ssr`].
///
/// The frame is one flat floor, one flat-faced pyramid and the clear behind
/// them, so most of its colours are the two shading gradients — comfortably past
/// this, and far enough past that a smoother gradient will not walk into it. What
/// it separates is a frame in which nothing drew.
const MIN_COLORS_SSR: usize = 32;

/// The half-extent of each band [`Scene::Ssr`] is measured over, in pixels.
///
/// [`AO_BAND`]'s size, and it has to fit inside the reflection: the reflected
/// pyramid is foreshortened to a wedge about forty-five pixels wide where
/// [`SSR_BAND_ROW`] cuts it, so a ten-pixel block centred on the frame's axis has
/// most of twenty pixels of margin on each side.
const SSR_BAND: (u32, u32) = (5, 5);

/// The row every band of [`Scene::Ssr`] sits on.
///
/// **One row for all three, and that is what the claim rests on.** The camera
/// looks along the plane `x = 0` at a floor whose normal is `+Y`, so three points
/// on one row of that floor are at the same view depth, carry the same normal,
/// and — `screenshot`'s `ssr_sun` having no X component — take exactly the same
/// direct light, the same ambient and the same specular sheen. The reflection is
/// the only term left that can separate the middle of the row from its ends.
///
/// A few rows below the pyramid's base, which lands near row 121: far enough that
/// the block is clear of the contact edge and of the occlusion pass's own
/// darkening, near enough that the reflection is still most of its own width.
const SSR_BAND_ROW: u32 = 128;

/// How far to each side of the frame's axis the two control bands sit, in pixels.
///
/// Past the reflection's own edge at [`SSR_BAND_ROW`] — which is about
/// twenty-three pixels out — by more than the band's own half-width, so no pixel
/// of a control band is a pixel of the reflection.
const SSR_ASIDE: u32 = 40;

/// Which channel [`Scene::Ssr`]'s bands are read on: **red**.
///
/// The floor is the cube's green `+Y` face through the tinted material row, whose
/// factor is `[0.15, 0.45, 1.0]` — so the floor has very little red — and the
/// pyramid it reflects is bright in every channel. Reading red is therefore
/// reading how much of the pyramid is in the floor, where the mean of three
/// channels reads that diluted by two the floor already owns.
///
/// It is the same claim either way and this is the sharper instrument: the frame
/// this was set against measures the mean a fourteenth up under the pyramid and
/// the red channel a third up.
const SSR_CHANNEL: usize = 0;

/// How much redder the floor under the pyramid must be than the floor beside it.
///
/// [`AO_RATIO`]'s shape, and a larger number because the effect is larger: a
/// reflection replaces a share of what a surface shows with the colour of
/// something else, where occlusion scales one term of it. The frame this was set
/// against measures about `1.34`, so this is a floor with margin over it and far
/// over the single-digit drift `crcbl_golden::Tolerance::RASTERISER` was measured
/// for.
///
/// What it separates is real, and it is the list the design asks a fixture to
/// fail: a pass that wrote nothing, one that wrote a constant, and one whose ray
/// direction is inverted all leave the three bands equal — the last because a ray
/// turned into the surface crosses it at the first tap and reflects every floor
/// pixel into itself, which brightens the whole floor and moves no ratio.
const SSR_RATIO: f32 = 1.15;

/// How far above the clear the control bands must measure, on the red channel.
///
/// [`AO_LIT_FLOOR`]'s job in [`SSR_CHANNEL`]'s units: a frame that drew nothing
/// has every band equal to the clear, and a ratio between equal numbers says
/// nothing. Smaller than that constant because the floor is deliberately poor in
/// red — it measures about `64` against a clear of `29`.
const SSR_LIT_FLOOR: f32 = 10.0;

/// The rows of [`Scene::Ssr`] the stepping is measured down, and the half-width
/// of the row averaged at each of them.
///
/// Inside the reflected pyramid at every one of them: it starts a few rows under
/// the base, where [`SSR_BAND_ROW`] already sits, and ends before the wedge
/// narrows past this width. Ten pixels either side of the axis rather than
/// [`SSR_BAND`]'s five, because what is measured here is a difference *between*
/// rows and a wider row is a quieter estimate of each.
const SSR_STEP_ROWS: std::ops::Range<u32> = 122..150;

/// The half-width of each of those rows — see [`SSR_STEP_ROWS`].
const SSR_STEP_HALF: u32 = 10;

/// How much the reflection may bend from one row of [`SSR_STEP_ROWS`] to the
/// next, in levels of [`SSR_CHANNEL`], averaged down the band.
///
/// **The march's stepping, in one number.** A ray's crossing lands on whichever
/// tap the walk happened to reach, so the reflected colour quantises and the
/// band alternates by several levels from one row to the next — visible in the
/// review frame, and the artefact `ssr_blur.slang` exists to remove. The
/// measurement is a *second* difference along the column, so a reflection that
/// merely fades down the band scores zero and only the alternation counts.
///
/// The frame this was set against measures 2.8 with the kernel as it stands and
/// 17.7 with the kernel cut down to its centre tap — which is the same composite
/// with no filter in it, and is the picture the slice before this one drew. At
/// the review extent the same pair reads 3.5 against 13.2. This sits between
/// them with room on both sides at either extent.
const SSR_STEP_LIMIT: f32 = 8.0;

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ssr_scene_reflects_its_pyramid_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ssr,
        "ssr",
        EXTENT,
        MIN_COLORS_SSR,
        the_floor_reflects_the_pyramid_and_only_under_it,
    );
}

/// How bright the pole this test lights its sky with is.
///
/// Large, and in red alone. The reflection reaches the floor through a Fresnel
/// term that is a fraction even at this scene's grazing angle, and the floor is
/// deliberately poor in red — [`SSR_CHANNEL`] says why — so a modest sky would
/// arrive as a level or two and be indistinguishable from the rasteriser's own
/// drift. Red alone for the same reason the fog tests scatter red: it puts the
/// whole of the effect in the channel every other constant here is measured on.
const SKY_POLE: f32 = 12.0;

/// How many levels of [`SSR_CHANNEL`] the sky must add to a missed ray before
/// this test believes the fallback reached it.
///
/// The run this was set against measures **12.0** with a sky bright above, and
/// **0.0** both with no sky and with the same sky turned upside down — so this
/// sits at half the effect with the two ways of being wrong pinned at zero. It
/// is also well above `crcbl_golden::Tolerance::RASTERISER`'s single-digit
/// drift, which is what a difference between two frames of one scene has to
/// clear to mean anything.
const SKY_FALLBACK_FLOOR: f32 = 6.0;

/// How much a **missed** ray may move when there is no environment for it to
/// fall back to.
///
/// The march finds nothing off the floor beside the pyramid, the probe volume
/// in this scene is empty, and a black sky adds nothing to an empty volume — so
/// switching the reflection pass on and off must leave those bands where they
/// are. `RASTERISER`'s drift is what this allows and nothing more; the run this
/// was set against measures exactly zero.
const SKY_UNTOUCHED: f32 = 2.0;

/// The least a sky lit only **below** the horizon may add to a **fully rough**
/// floor whose rays leave it upward.
///
/// The mirror floor's rays see next to none of that sky — its row of
/// `crcbl_shaders::sky_prefilter`'s table gives the opposite pole a share
/// under a hundredth at the band — and the roughest row gives it an eighth.
/// Half of the 17.1 on radv and 17.0 on lavapipe the sweep of 2026-08-29
/// measured, so a rasteriser's own drift cannot cross it, while every wrong
/// reading of the sky table — its mirror row, its axes swapped, the one
/// mirror direction — lands at the mirror floor's own single digits.
const ROUGH_SKY_BELOW_FLOOR: f32 = 8.0;

/// The most that same sky may add to the rough floor, which is the `DFG`
/// pair's claim: `f0 · scale + bias` at the band's grazing `N·V` on the
/// roughest row is under a third of Schlick along the one reflected
/// direction, and the environment scaled by Schlick measured 74.1 on radv and
/// 73.9 on lavapipe before the pair replaced it. Twice the pair's own
/// measurement and under half of Schlick's, so the split-sum's second half
/// being dropped for Schlick — or the pair read at roughness zero, whose row
/// is Schlick — fails here while the sky half stays untouched.
const ROUGH_SKY_BELOW_CEILING: f32 = 35.0;

/// **The sky is the environment a missed ray falls back to**, and it is read
/// along the ray rather than applied as a constant.
///
/// `docs/plan/43-render-standards.md` §8 ranks a sky above scenery for exactly
/// this: the environment SSR falls back to and the ambient a metal needs are
/// one term. The ambient half landed first; this is the reflection half.
///
/// **The comparison is between the pass being on and off, not between two
/// skies.** A sky lights the ambient term as well, and this floor's normal
/// points the same way its reflected rays go, so switching a sky on brightens
/// it twice over for two unrelated reasons. Inside one pair the ambient is
/// identical and cancels, and what is left is the reflection alone.
///
/// Three pairs, and each answers a different way of being wrong:
///
/// * **No sky.** A missed ray has nothing to fall back to here — the probe
///   volume is empty — so the pass must move these bands by nothing. A
///   fallback that returned a constant, or read uninitialised rows, fails here
///   and nowhere else.
/// * **Bright above.** The floor's rays leave it upward, so the fallback must
///   arrive. A block that declared the rows and never read them fails here.
/// * **Bright below.** The same radiance, pointed where these rays do not
///   look. A `sky_prefiltered` that collapsed the gradient to an average of
///   its bands, or took the two poles in the wrong order, brightens this pair
///   instead of leaving it alone; both were run against its predecessor
///   `sky_radiance` and both fail here.
///
/// **A fourth pair, on a fully rough floor, is what sees the lobe.** The
/// fallback reads the sky through `crcbl_shaders::sky_prefilter`'s table at the
/// surface's roughness, and a mirror reads the gradient itself — so the three
/// pairs above pass on a fallback that ignored roughness altogether. The rough
/// floor under the sky lit *below* is where the two differ in kind: the
/// mirror's upward rays see nothing of it, and the roughest lobe, which
/// reaches across the horizon, does. A table read at the wrong axis, at
/// roughness zero, or not at all fails here and nowhere else.
///
/// **The same pair has a ceiling, and that is the `DFG` half.** What the rough
/// floor takes of that sky is the prefiltered gradient scaled by
/// `f0 · scale + bias` at its grazing `N·V`, and on the roughest row that pair
/// is well under Schlick along one direction — [`ROUGH_SKY_BELOW_CEILING`]
/// says by how much. A pass that scaled by Schlick again passes the floor and
/// fails the ceiling.
///
/// **What this scene cannot see, stated rather than implied:** every reflected
/// ray in it leaves the floor *upward*, so the branch that picks the ground
/// below the horizon is never the right answer for any pixel here. A shader
/// that dropped that branch and always took the zenith renders this frame
/// correctly — it was run, and it passes. What covers that arm is
/// `crcbl_shaders::sky`'s own
/// `the_three_bands_are_returned_exactly_at_their_own_directions` on the host
/// side, and `docs/backlog.md` carries the gap: no fixture in this tree
/// reflects a ray downward.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_missed_reflection_falls_back_to_the_sky_along_its_own_ray() {
    crcbl_core::log::init_logging();

    type Build = fn(
        &dyn crcbl::hal::Device,
        crcbl::hal::QueueHandle,
        crcbl::hal::Format,
        crcbl::render::Sky,
        RenderEffects,
    )
        -> Result<crcbl::screenshot::ForwardScene, crcbl::screenshot::OffscreenError>;
    let frame = |build: Build, sky: crcbl::render::Sky, effects| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                build(device, queue, format, sky, effects)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the ssr scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    // The floor beside the pyramid, where `the_floor_reflects_the_pyramid_and_
    // only_under_it` establishes the march finds nothing. Both sides averaged:
    // the scene is symmetric about the axis and one number is quieter than two.
    let missed = |image: &Image| {
        let axis = EXTENT.0 / 2;
        let band =
            |column: u32| block_channel(image, (column, SSR_BAND_ROW), SSR_BAND, SSR_CHANNEL);
        (band(axis - SSR_ASIDE) + band(axis + SSR_ASIDE)) / 2.0
    };

    let pole = crcbl::math::Vec3::new(SKY_POLE, 0.0, 0.0);
    let dark = crcbl::math::Vec3::ZERO;
    // A horizon of zero in both, so the only thing separating the two skies is
    // which pole is lit — and so a ray that looks along the horizon gets
    // nothing from either, which is what makes the pair a clean contrast.
    let above = crcbl::render::Sky {
        zenith: pole,
        horizon: dark,
        ground: dark,
    };
    let below = crcbl::render::Sky {
        zenith: dark,
        horizon: dark,
        ground: pole,
    };

    let with_reflections = RenderEffects::DEFAULT_STACK;
    let without = RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS);
    let contribution = |build: Build, sky| {
        missed(&frame(build, sky, with_reflections)) - missed(&frame(build, sky, without))
    };

    let mirror: Build = crcbl::screenshot::ssr_forward;
    let rough: Build = crcbl::screenshot::ssr_rough_floor_forward;
    let unlit = contribution(mirror, crcbl::render::Sky::NONE);
    let from_above = contribution(mirror, above);
    let from_below = contribution(mirror, below);
    let rough_unlit = contribution(rough, crcbl::render::Sky::NONE);
    let rough_from_above = contribution(rough, above);
    let rough_from_below = contribution(rough, below);
    eprintln!(
        "crcbl render e2e: ssr sky fallback — no sky {unlit:.1}, bright above {from_above:.1}, \
         bright below {from_below:.1}; rough floor: no sky {rough_unlit:.1}, bright above \
         {rough_from_above:.1}, bright below {rough_from_below:.1}"
    );

    assert!(
        unlit.abs() <= SKY_UNTOUCHED,
        "with no sky and an empty probe volume the reflection pass moved a missed ray by \
         {unlit:.1}, so it is compositing something that is not an environment"
    );
    assert!(
        from_above >= SKY_FALLBACK_FLOOR,
        "a sky bright above added only {from_above:.1} to a floor whose rays leave it upward, so \
         the fallback is not reaching a missed ray"
    );
    assert!(
        from_above > from_below + SKY_FALLBACK_FLOOR,
        "a sky bright above added {from_above:.1} and the same sky turned upside down added \
         {from_below:.1}; these rays look up, so a fallback that read its direction would not \
         confuse the two"
    );

    // The rough floor: the same three skies, and the one that separates a lobe
    // from a mirror is the sky lit below.
    assert!(
        rough_unlit.abs() <= SKY_UNTOUCHED,
        "with no sky the reflection pass moved a missed ray off the rough floor by \
         {rough_unlit:.1}, so a fully rough surface is compositing something that is not an \
         environment"
    );
    assert!(
        rough_from_below >= ROUGH_SKY_BELOW_FLOOR,
        "a sky lit only below the horizon added {rough_from_below:.1} to a fully rough floor, \
         against {from_below:.1} on the mirror one. The roughest lobe reaches across the \
         horizon and the table's `W_opposite` says how far; a fallback that read the table at \
         roughness zero, along the mirror direction alone, or not at all leaves this at the \
         mirror's zero"
    );
    assert!(
        rough_from_below <= ROUGH_SKY_BELOW_CEILING,
        "a sky lit only below the horizon added {rough_from_below:.1} to a fully rough floor, \
         more than the `DFG` pair at a grazing N·V on the roughest row allows; the environment \
         is being scaled by Schlick along one direction, or by the pair's roughness-zero row, \
         which is the same number"
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ssr_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Ssr,
        "ssr",
        MIN_COLORS_SSR,
        the_floor_reflects_the_pyramid_and_only_under_it,
    );
}

/// [`Scene::Ssr`]'s claim: **the floor carries the pyramid's colour directly
/// below the pyramid, and not on either side of it.**
///
/// **The golden cannot make this claim and no golden ever could**, and the
/// design says so in as many words: a march has no denominator, so a golden here
/// is a review aid and the real check is a structural ratio between blocks of one
/// frame. A reflection pass that returned zero everywhere draws a perfectly
/// plausible frame — a polished floor is a plausible matt floor — and would be
/// blessed without comment. So would one that added a constant, which is a floor
/// a shade brighter.
///
/// Three bands on [`SSR_BAND_ROW`], all on the same flat floor:
///
/// * the frame's own axis, where the reflected pyramid is;
/// * [`SSR_ASIDE`] pixels to each side of it, where the same ray finds nothing
///   and the reflection is correctly zero.
///
/// **The two controls are on opposite sides, and that is not symmetry for its own
/// sake.** A ray inverted in the plane of the surface reflects what is on the far
/// side of the pixel from the real answer, so a one-sided claim could be
/// satisfied by a frame whose bright band is in the wrong place. Asserting the
/// dark side as well as the bright one leaves no arrangement that passes but the
/// intended one — and the two controls are exactly equal in the frame this was
/// set against, which is the symmetry `ssr_sun` was given no X component for.
fn the_floor_reflects_the_pyramid_and_only_under_it(image: &Image) {
    let band = |column: u32| block_channel(image, (column, SSR_BAND_ROW), SSR_BAND, SSR_CHANNEL);
    let axis = EXTENT.0 / 2;
    let under = band(axis);
    let aside = [
        ("-X", band(axis - SSR_ASIDE)),
        ("+X", band(axis + SSR_ASIDE)),
    ];
    eprintln!(
        "crcbl render e2e: ssr — under the pyramid {under:.1}, floor beside it {:.1} and {:.1}",
        aside[0].1, aside[1].1
    );
    for (side, plain) in aside {
        assert!(
            plain * SSR_RATIO < under,
            "the floor under the pyramid must carry unmistakably more of its colour than the \
             floor {side} of it at the same depth, same normal, same material and same light: \
             {under:.1} against {plain:.1} — that is not a reflection"
        );
    }
    // The controls are lit *floor*, not an unpainted frame the reflection is
    // bright against. Without this the ratios above are satisfiable by a scene
    // that drew nothing at all.
    let clear = block_channel(image, (2, 2), (2, 2), SSR_CHANNEL);
    for (side, plain) in aside {
        assert!(
            plain > clear + SSR_LIT_FLOOR,
            "the {side} band measures {plain:.1} against a clear of {clear:.1}, so there is no \
             lit floor here for a reflection to be a reflection on"
        );
    }
    the_reflection_does_not_step_down_the_band(image);
}

/// [`Scene::Ssr`]'s second claim: **the reflection does not step from one row of
/// the floor to the next.**
///
/// `ssr.slang` walks a fixed pixel stride and takes the first crossing it finds,
/// so the reflected colour quantises to whichever tap the walk reached — and
/// consecutive rows of the floor, whose rays differ only slightly, land on
/// different taps. The band alternates by several levels row to row as a result.
/// `ssr_blur.slang` is what removes it, and this is the number that says so.
///
/// **A second difference rather than a first**, because the reflection is
/// genuinely a gradient down the band: it fades as the reflected pyramid recedes,
/// and a first difference would measure that fade rather than the stepping. The
/// second difference of a straight ramp is zero, so what is left is the bend —
/// which is the alternation and nothing else.
///
/// **No golden could make this claim**, on the ratio above's terms and one more:
/// the alternation is a few levels on a floor that is already a gradient, and it
/// sat in the reference for as long as the unfiltered march did. It is also why
/// every other claim on this scene averages a block: block averaging is exactly
/// what hides this, so a measurement down single rows is the only one that can
/// see it.
fn the_reflection_does_not_step_down_the_band(image: &Image) {
    let axis = EXTENT.0 / 2;
    let rows: Vec<f32> = SSR_STEP_ROWS
        .map(|row| row_channel(image, (axis, row), SSR_STEP_HALF, SSR_CHANNEL))
        .collect();
    let bends: Vec<f32> = rows
        .windows(3)
        .map(|three| (three[0] - 2.0 * three[1] + three[2]).abs())
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let mean = bends.iter().sum::<f32>() / bends.len() as f32;
    let worst = bends.iter().fold(0.0f32, |worst, bend| worst.max(*bend));
    eprintln!(
        "crcbl render e2e: ssr — the reflection bends {mean:.2} level(s) per row down the band \
         and {worst:.1} at worst, over {} row(s)",
        rows.len()
    );
    assert!(
        mean < SSR_STEP_LIMIT,
        "the reflection alternates by {mean:.2} level(s) from one row to the next, against a \
         limit of {SSR_STEP_LIMIT} — the march's stepping is in the frame and nothing filtered \
         it out"
    );
}

/// The mean of one channel across a single row, `half` pixels either side of
/// `at`.
///
/// [`block_channel`] with the vertical extent taken away, and a function of its
/// own because that one averages at least two rows — which is the whole of what
/// [`the_reflection_does_not_step_down_the_band`] must not do.
fn row_channel(image: &Image, at: (u32, u32), half: u32, index: usize) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for x in at.0.saturating_sub(half)..(at.0 + half).min(EXTENT.0) {
        let pixel = image.pixel(x, at.1).expect("inside the frame");
        total += f32::from(pixel[index]);
        count += 1;
    }
    assert!(count > 0, "an empty row measures nothing");
    total / count as f32
}

/// The anti-vacuity floor for [`Scene::Bloom`].
///
/// The frame is one flat floor, one small bright patch and the halo between
/// them, and the halo is a smooth gradient — so most of this count is the
/// gradient itself. Set below what the frame this was blessed from measures,
/// which the harness prints, so a smoother chain does not walk into it. What it
/// separates is "the box drew nothing": a frame of clear colour, or one flat
/// floor with no patch on it.
const MIN_COLORS_BLOOM: usize = 128;

/// How many pixels of the frame one world unit of [`Scene::Bloom`]'s floor is.
///
/// [`AO_PIXELS_PER_UNIT`]'s arithmetic with that scene's camera height swapped
/// for `screenshot`'s `BLOOM_CAMERA_UP`: the frame's short half-axis covers
/// `up * tan(30°)` of floor and a pixel is that over half the frame's height.
const BLOOM_PIXELS_PER_UNIT: f32 = (EXTENT.1 as f32 / 2.0) / (2.6 * 0.577_350_3);

/// Where a point on [`Scene::Bloom`]'s floor lands in the frame.
///
/// [`ao_pixel`]'s flip for [`ao_pixel`]'s reason: the camera looks down `-Y` with
/// `+Z` up, so world `+X` is the frame's left and world `+Z` is its top.
fn bloom_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * BLOOM_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * BLOOM_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every band below is inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// Where [`Scene::Bloom`] puts its emitter along `+X`, in world units.
///
/// `screenshot`'s `BLOOM_EMITTER_AT`, restated because this suite reads that
/// module's fixtures through its public [`Scene`] and not through its private
/// constants — [`AO_PIXELS_PER_UNIT`] restates that scene's camera height for
/// the same reason.
const BLOOM_EMITTER_ON_X: f32 = 0.75;

/// The half-extent of each band [`Scene::Bloom`] is measured over, in pixels.
///
/// Narrow, and it has to be: the emitter's `+X` edge is at
/// `BLOOM_EMITTER_AT + BLOOM_EMITTER_SIZE / 2` — about thirteen pixels inside
/// [`BLOOM_BAND_AT`] at this scale — so a block wide enough to reach it would be
/// measuring the patch instead of the halo beside it.
const BLOOM_BAND: (u32, u32) = (4, 4);

/// How far from the frame's centre every band sits, in world units.
///
/// **One number for all four, and that is what the claim rests on**, on
/// [`AO_BAND_AT`]'s terms exactly: the camera looks straight down and the sun
/// points straight down, so four points the same distance from the axis on the
/// same flat floor are the same distance from the eye, carry the same normal and
/// receive exactly the same direct light and ambient. One of the four is a fifth
/// of a unit off the emitter's edge and the other three are more than a unit from
/// it; proximity to the emitter is the only term left that can separate them.
///
/// Far enough past `screenshot`'s `BLOOM_EMITTER_AT` plus half its
/// `BLOOM_EMITTER_SIZE` that no pixel of the band is a pixel of the patch, and
/// near enough that the band is inside the halo rather than past it.
const BLOOM_BAND_AT: f32 = 1.2;

/// How much brighter the floor beside the emitter must be than the same floor on
/// the other side of the frame.
///
/// A ratio rather than a difference, on [`AO_RATIO`]'s terms and with that
/// constant's sRGB caveat: these bands are read after the encode, which is very
/// nearly a `0.42` power, so a term that doubles in linear light moves by about a
/// third here.
///
/// **What it separates is the list this fixture exists for.** Every one of these
/// draws a plausible picture and would be blessed without comment:
///
/// * A chain that handed its input back unchanged, or a composite that added
///   nothing, leaves every band equal and lands at exactly `1.00`.
/// * A chain that wrote a constant — the classic "the pass ran and produced
///   something" failure — adds the same term everywhere and moves the ratio
///   *towards* `1.00`, never away from it.
/// * A global scale, an exposure change or a tonemap edit moves both bands
///   together and cannot move a ratio at all.
///
/// The number itself is a floor under what the frame this was blessed from
/// measures, with margin, and far over the single-digit drift
/// `crcbl_golden::Tolerance::RASTERISER` was measured for. The harness prints
/// both bands on every run.
const BLOOM_RATIO: f32 = 1.20;

/// How bright the control bands must be, on the frame's own 0–255 scale.
///
/// [`AO_LIT_FLOOR`]'s job — a ratio between two numbers near zero says nothing —
/// but an **absolute** level rather than a step above the clear, because this
/// frame has no clear in it: the floor fills it edge to edge, so the corner the
/// other fixtures read as "nothing drew here" is floor too. Well under what the
/// frame this was set against measures, which the harness prints on every run.
const BLOOM_LIT_FLOOR: f32 = 60.0;

/// How much brighter the emitter must be than the floor around it.
///
/// The other half of the anti-vacuity check, and the half that needs no absolute
/// number: a frame that drew nothing, and a frame of one flat colour, both put
/// the patch and the floor at the same level and fail this whatever that level
/// is. It is also what says the *source* of the halo is in the frame — a chain
/// haloing something that is not there would be a strange defect, and this is
/// what would name it.
///
/// The patch is clipped to white by the tonemap, so the measured ratio is the
/// floor's level against 255 and this has enormous margin. That is the point of
/// `screenshot`'s `BLOOM_EMITTER_GAIN`: a threshold-free chain blooms in
/// proportion to brightness, so the fixture's subject has to be unmistakably
/// above the display range.
const BLOOM_EMITTER_RATIO: f32 = 1.5;

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_bloom_scene_haloes_its_emitter_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Bloom,
        "bloom",
        EXTENT,
        MIN_COLORS_BLOOM,
        the_halo_is_beside_the_emitter_and_not_beside_its_mirror,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_bloom_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Bloom,
        "bloom",
        MIN_COLORS_BLOOM,
        the_halo_is_beside_the_emitter_and_not_beside_its_mirror,
    );
}

/// [`Scene::Bloom`]'s claim: **the floor beside the bright patch is lit by it,
/// and the same floor elsewhere in the frame is not.**
///
/// **The golden cannot make this claim and no golden ever could.** A chain that
/// returned its input untouched draws a perfectly plausible frame — a flat floor
/// with a white square on it, which is what this scene looks like — and it would
/// be blessed without comment. So would one whose composite added a constant, or
/// one that scaled the whole image. Every assertion here is a ratio between bands
/// of *the same frame*, which no uniform change can move.
///
/// Four bands, all [`BLOOM_BAND_AT`] from the frame's centre on the same flat
/// floor:
///
/// * `+X`, a fifth of a unit off the emitter's edge — inside the halo.
/// * `-X`, its mirror: the same distance from the eye, the same normal, the same
///   directional sun, and nearly two units of floor away from the patch.
/// * `+Z` and `-Z`, the same distance from the axis along the other one, and both
///   more than a unit from the patch.
///
/// **The four share a distance from the eye, and that is what a vignette cannot
/// survive.** Anything that brightens or darkens with distance from the frame's
/// centre moves all four together and satisfies none of the ratios. The `-X`
/// mirror is what a chain whose taps are flipped in one axis fails: such a chain
/// haloes the wrong side, and the assertion is then false in the direction that
/// reads as "the halo is on the left".
fn the_halo_is_beside_the_emitter_and_not_beside_its_mirror(image: &Image) {
    let band = |x: f32, z: f32| block_brightness(image, bloom_pixel(x, z), BLOOM_BAND);
    let halo = band(BLOOM_BAND_AT, 0.0);
    let away = [
        ("-X", band(-BLOOM_BAND_AT, 0.0)),
        ("+Z", band(0.0, BLOOM_BAND_AT)),
        ("-Z", band(0.0, -BLOOM_BAND_AT)),
    ];
    eprintln!(
        "crcbl render e2e: bloom — beside the emitter {halo:.1}, floor away from it {:.1}, \
         {:.1} and {:.1}",
        away[0].1, away[1].1, away[2].1,
    );
    for (axis, plain) in away {
        assert!(
            plain * BLOOM_RATIO < halo,
            "the floor beside the emitter must be unmistakably brighter than the floor the \
             same distance out along {axis}, which is the same floor under the same light: \
             {halo:.1} against {plain:.1} — that is not a halo"
        );
    }
    // The controls are lit *floor* and the emitter is unmistakably brighter than
    // it. Without both, the ratios above are satisfiable by a frame in which
    // nothing drew — see [`BLOOM_LIT_FLOOR`] and [`BLOOM_EMITTER_RATIO`].
    let patch = block_brightness(image, bloom_pixel(BLOOM_EMITTER_ON_X, 0.0), BLOOM_BAND);
    eprintln!("crcbl render e2e: bloom — the emitter itself {patch:.1}");
    for (axis, plain) in away {
        assert!(
            plain > BLOOM_LIT_FLOOR,
            "the {axis} band measures {plain:.1}, so there is no lit floor here for a halo \
             to be a halo on"
        );
        assert!(
            patch > plain * BLOOM_EMITTER_RATIO,
            "the emitter measures {patch:.1} against {plain:.1} of floor {axis} of it, so \
             there is nothing bright in this frame for the halo to have come from"
        );
    }
}

/// The anti-vacuity floor for [`Scene::Aa`].
///
/// The frame is two flat levels and the edge between them, so most of this count
/// is the resolve's own output — the blended pixels along the silhouette, of
/// which there are hundreds. Well below what the frame this was blessed from
/// measures, which the harness prints, so a differently-tuned filter does not
/// walk into it; well above the handful a frame of two flat levels and a hard
/// edge would show.
const MIN_COLORS_AA: usize = 48;

/// The level at or below which a pixel of [`Scene::Aa`] is background.
///
/// The frame's dark level is the cleared background lifted by [`aa_sun`]'s trace
/// of ambient, which measures a little over thirty-four of two hundred and
/// fifty-five on every adapter this was checked on. A few counts above that, so
/// a rasteriser that lands a count either side of it is still counted as
/// background.
const AA_DARK_CEILING: f32 = 42.0;

/// The level at or above which a pixel of [`Scene::Aa`] is the slab's face.
///
/// The face is one flat value under a light square on to it — see [`aa_sun`] —
/// and measures from about two hundred and five at its corners to two hundred and
/// twenty-eight at its middle, the spread being the tonemap's response to the
/// distance falloff across the face. A few counts below the lower end.
const AA_BRIGHT_FLOOR: f32 = 197.0;

/// How many pixels of [`Scene::Aa`] must lie strictly between its two levels.
///
/// **Measured, not chosen.** The fixture drawn at [`EXTENT`] puts 532 pixels
/// between [`AA_DARK_CEILING`] and [`AA_BRIGHT_FLOOR`] on three adapters —
/// discrete radv, integrated radv and llvmpipe — and puts **zero** there with
/// the resolve switched off, which is what
/// [`the_resolve_is_what_puts_the_soft_pixels_there`] asserts rather than
/// assumes. This is a little under half of the measured count, which leaves room
/// for a filter tuned differently without leaving room for one that does nothing.
///
/// It is a count and not a ratio because the silhouette's length in pixels is a
/// function of [`EXTENT`], and both golden tests below draw at that extent.
const AA_MIN_SOFT_PIXELS: usize = 256;

/// How much [`Scene::Aa`]'s mean level may move when the resolve is added, out of
/// 255.
///
/// **The other half of the claim, and the half that says the filter is a filter.**
/// A pass that lightened or darkened the whole frame would put plenty of pixels
/// between the two levels and pass the count above; so would one that blurred the
/// entire image rather than its edges. A redistribution along an edge moves the
/// mean by almost nothing — the measured pair differ by 0.24 — where either of
/// those moves it by a great deal.
const AA_MEAN_TOLERANCE: f32 = 2.0;

/// How many pixels of [`Scene::Aa`] lie strictly between its two levels.
///
/// Luma rather than a single channel because the frame is grey: the slab is
/// untinted and the sun is the demo scene's, so a channel taken alone measures
/// the same edge through a colour term that has nothing to do with it.
fn soft_pixels(image: &Image) -> usize {
    let mut soft = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y).expect("inside the frame");
            let luma = 0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2]);
            if luma > AA_DARK_CEILING && luma < AA_BRIGHT_FLOOR {
                soft += 1;
            }
        }
    }
    soft
}

/// The mean luma of a frame, out of 255.
fn mean_luma(image: &Image) -> f32 {
    let mut total = 0.0f32;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += 0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2]);
        }
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame of a few tens of thousands of pixels"
    )]
    let count = (image.width() * image.height()) as f32;
    total / count
}

/// [`Scene::Aa`]'s claim, as far as one frame can carry it: **the silhouette is
/// not a staircase.**
///
/// The frame is one slab, lit square on, against the cleared background — two
/// flat levels and one diagonal edge between them. A rasterised edge with no
/// resolve over it is a hard boundary: every pixel is one level or the other, and
/// this count is zero. Every pixel this finds is one the resolve wrote.
///
/// **The golden alone could not make even this much of the claim.** A resolve
/// that returned its input untouched draws a slab with a clean edge, which is
/// what a slab looks like, and it would be blessed without comment.
/// [`the_resolve_is_what_puts_the_soft_pixels_there`] is the other half: it draws
/// this same scene without the pass and shows the count going to nothing.
fn the_silhouette_is_not_a_staircase(image: &Image) {
    let soft = soft_pixels(image);
    let mean = mean_luma(image);
    eprintln!("crcbl render e2e: aa — {soft} soft pixel(s), mean luma {mean:.2}");
    assert!(
        soft >= AA_MIN_SOFT_PIXELS,
        "{soft} pixel(s) of this frame lie between {AA_DARK_CEILING} and \
         {AA_BRIGHT_FLOOR}, which is under the {AA_MIN_SOFT_PIXELS} an edge this long \
         is resolved into — the pass ran and changed nothing, or it did not run"
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_aa_scene_resolves_its_silhouette_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Aa,
        "aa",
        EXTENT,
        MIN_COLORS_AA,
        the_silhouette_is_not_a_staircase,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_aa_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Aa,
        "aa",
        MIN_COLORS_AA,
        the_silhouette_is_not_a_staircase,
    );
}

/// **The soft pixels along the silhouette are the resolve's, and nothing else in
/// the frame put them there.**
///
/// The claim no golden can make and no single frame can make either. Every other
/// fixture here is about a *value* somewhere in the picture, and a value is a
/// thing one frame can be asked about. An antialiased edge is not: a frame whose
/// filter did nothing has a clean hard silhouette in it, and a clean hard
/// silhouette is what a slab looks like. So this draws
/// [`Scene::Aa`]'s scene twice through
/// [`aa_forward`](crcbl::screenshot::aa_forward) — once with the effect and once
/// with the default stack, the same geometry, camera, sun and extent — and
/// compares the two.
///
/// Two assertions, and they fail on opposite mistakes:
///
/// * The count of pixels between the two levels must **rise**, and the control's
///   must be a small fraction of it. A pass that copied its input leaves the two
///   equal.
/// * The mean level must **not move** by more than [`AA_MEAN_TOLERANCE`]. A pass
///   that brightened, darkened or blurred the whole frame would satisfy the
///   first assertion handsomely and fail this one, which is the difference
///   between an edge filter and a filter.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_resolve_is_what_puts_the_soft_pixels_there() {
    crcbl_core::log::init_logging();

    let frame = |effects| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::aa_forward(device, queue, format, effects)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the aa scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    let resolved = frame(RenderEffects::DEFAULT_STACK.union(RenderEffects::ANTIALIASING));
    // **`difference` and not the bare default.** The resolve is in
    // `DEFAULT_STACK` now, so a control built on that alone is the resolved
    // frame again under another name — and this test would compare a frame with
    // itself and pass on any pair of numbers at all.
    let control = frame(RenderEffects::DEFAULT_STACK.difference(RenderEffects::ANTIALIASING));

    let (soft, plain) = (soft_pixels(&resolved), soft_pixels(&control));
    let (mean, plain_mean) = (mean_luma(&resolved), mean_luma(&control));
    eprintln!(
        "crcbl render e2e: aa — {soft} soft pixel(s) resolved against {plain} plain, \
         mean {mean:.2} against {plain_mean:.2}"
    );
    assert!(
        soft >= AA_MIN_SOFT_PIXELS,
        "the resolved frame has {soft} pixel(s) between the two levels, under the \
         {AA_MIN_SOFT_PIXELS} an edge this long is resolved into"
    );
    assert!(
        plain * 4 < soft,
        "the frame drawn without the resolve has {plain} pixel(s) between the two levels \
         against the resolved frame's {soft} — the effect's off-switch is not switching \
         anything off, so neither frame is evidence about the other"
    );
    assert!(
        (mean - plain_mean).abs() <= AA_MEAN_TOLERANCE,
        "the resolve moved the frame's mean level from {plain_mean:.2} to {mean:.2}, past \
         the {AA_MEAN_TOLERANCE} an edge filter may move it — this pass is doing something \
         to the whole image rather than to its edges"
    );
}

/// The anti-vacuity floor for [`Scene::Probes`].
///
/// The fixture deliberately has broad flat regions, so colour count only rejects
/// an unpainted frame. The probe-only colour ratios and absolute Rust-mirror
/// comparison below are the checks that prove the binding and evaluation.
const MIN_COLORS_PROBES: usize = 16;

/// How many pixels of the frame one world unit of [`Scene::Probes`]' floor is.
///
/// [`POINT_PIXELS_PER_UNIT`]'s arithmetic through `screenshot`'s own
/// [`PROBE_CAMERA_UP`](crcbl::screenshot::PROBE_CAMERA_UP): the frame's short
/// half-axis covers `up * tan(30°)` of floor and a pixel is that over half the
/// frame's height. Read from that constant rather than written out again,
/// because this scene inverts the mapping as well as using it and the two copies
/// would agree only until somebody moved the camera.
const PROBE_PIXELS_PER_UNIT: f32 =
    (EXTENT.1 as f32 / 2.0) / (crcbl::screenshot::PROBE_CAMERA_UP * 0.577_350_3);

/// Where a point on [`Scene::Probes`]' floor lands in the frame.
///
/// [`point_pixel`]'s flip for [`point_pixel`]'s reason: the camera looks down
/// `-Y` with `+Z` up, so world `+X` is the frame's left and world `+Z` is its
/// top.
fn probe_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * PROBE_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * PROBE_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both bands below are inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// The world position on the floor that a pixel's **centre** sees — the inverse
/// of [`probe_pixel`], and the thing the mirror comparison is evaluated at.
///
/// An inverse exists in closed form only because the camera looks straight down
/// at the plane `y = 0`: the projection of that plane is then linear in `x` and
/// `z`, so this is a subtraction and a division rather than an unprojection
/// through the frame's matrices. `+ 0.5` because a fragment is shaded at its
/// pixel's centre and the field this evaluates has a gradient across it: the
/// bright band moves about three quarters of a level per pixel, so getting the
/// convention wrong would spend a third of [`PROBE_MIRROR_LEVELS`] on nothing.
fn probe_world(column: u32, row: u32) -> [f32; 3] {
    let x = (EXTENT.0 as f32 / 2.0 - (column as f32 + 0.5)) / PROBE_PIXELS_PER_UNIT;
    let z = (EXTENT.1 as f32 / 2.0 - (row as f32 + 0.5)) / PROBE_PIXELS_PER_UNIT;
    [x, 0.0, z]
}

/// The half-extent of each band [`Scene::Probes`] is measured over, in pixels.
///
/// [`AO_BAND`]'s size, and it fits for a reason of its own: the probe field
/// varies across the block, so every pixel of it is predicted separately and the
/// block is an average of a gradient rather than of one value. Wide enough that
/// `ssao.slang`'s 4×4 rotation tile and the box blur over it have averaged out,
/// narrow enough to be a small fraction of [`PROBE_BAND_AT`]'s clearance from
/// the nearest wall.
const PROBE_BAND: (u32, u32) = (5, 5);

/// How far from the frame's centre the two bands sit, in world units.
///
/// **The same distance on both sides, and that is the whole claim.** The camera
/// looks straight down, the floor is one flat quad of one albedo, the sun's
/// contribution to it is exactly zero — see `screenshot`'s `probe_sun` — and the
/// occlusion at this distance from every wall is one. Two points at `±x` on that
/// floor therefore differ in exactly one thing: which way the probe grid says the
/// light at them arrives from.
///
/// A unit clear of the `±X` walls at `PROBE_ROOM_WIDTH / 2`, which is twice
/// `crcbl_render::ForwardRenderer`'s occlusion radius — so the bands are outside
/// the darkening, not merely near its edge.
const PROBE_BAND_AT: f32 = 0.6;

/// The second sample in each clamped region, nearer the central blend interval.
///
/// Its blocks remain clear of the interval and the SSAO reach. Comparing it to
/// [`PROBE_BAND_AT`] distinguishes broad endpoint regions from the reverted
/// full-floor gradient without relying on the golden.
const PROBE_FLAT_INNER_AT: f32 = 0.4;

/// How much more of its own colour each end of the floor must carry than the
/// other end does.
///
/// [`AO_RATIO`]'s shape and, like it, a number set under what the frame measures
/// rather than at it: the run that blessed the golden reads about `1.99` on red
/// and `1.96` on blue, both after the sRGB encode has compressed a linear ratio
/// of over four. This is a floor with margin under those and far over the
/// two-level drift `crcbl_golden::Tolerance::RASTERISER` allows.
///
/// **What it separates is the linear band of the spherical harmonic, alone.**
/// The two probes hold identical *constant* bands — see `screenshot`'s
/// `probe_grid`, which is built that way on purpose — so a shader that evaluated
/// `sh.w` and dropped the three dot products draws a uniform floor and lands at
/// exactly `1.00`, as does a zeroed linear band, as does a flat ambient in place
/// of the grid.
const PROBE_RATIO: f32 = 1.5;

/// Maximum per-channel difference between two blocks in either clamped region.
///
/// This pins the fixture's defining replacement for the reverted full-frame
/// gradient: points well outside the central blend interval must evaluate the
/// same endpoint probe. A regenerated golden cannot hide the interval expanding
/// back across the floor because these within-frame comparisons would diverge.
const PROBE_FLAT_DELTA: f32 = 0.5;

/// Minimum distance in levels from each clamped endpoint at the blend centre.
///
/// The centre must observe both probe rows rather than merely select an endpoint;
/// the Rust mirror below checks the interpolated value itself.
const PROBE_INTERPOLATION_DELTA: f32 = 5.0;

/// How far the frame may sit from what
/// [`crcbl_shaders::probe::irradiance_at`](crcbl::shaders::probe::irradiance_at)
/// predicts for it, in levels of 255.
///
/// **This is the number slice 1 owed.** The Rust mirror and `mesh.slang`'s
/// `probe_irradiance` are two implementations of one evaluation, and until this
/// scene existed nothing had ever compared them: the literature tests bind the
/// mirror, every golden in the tree was drawn with a volume of zeroes, and
/// `x + 0 == x` says nothing about the arithmetic on either side.
///
/// A rendered pixel has been through the whole shading chain, so the comparison
/// is only worth making because this scene takes that chain apart — every step
/// between the two is either exact or absent:
///
/// * `lit = diffuse_albedo * (irradiance * occluded + direct) + gloss`, and
///   `direct` and `gloss` are **exactly zero** on this floor because the sun is
///   horizontal and the floor's normal is `+Y`.
/// * `occluded` is **exactly one**: `ssao.slang` finds nothing within its radius
///   of a band this far from every wall, and `blocked == 0` gives `1.0` rather
///   than something near it.
/// * `diffuse_albedo` is the floor face's own colour out of
///   `crcbl_shaders::mesh::OPEN_BOX_FACES` — the material row is untinted, its
///   page layer is the white one and its metallic is zero.
/// * `tonemap.slang` is `saturate(color * 1.0)`, the identity below one, and the
///   scene's radiances are chosen so the brightest floor pixel stays under it.
/// * The swapchain is sRGB, so the only remaining step is the standard transfer
///   function, which [`srgb_encode`] is.
///
/// So what is left for this tolerance to cover is 8-bit rounding, the sRGB
/// encode's own precision, half a pixel of disagreement about where a fragment
/// centre is, and — since the visibility weighting arrived — the divide by a
/// summed weight the two sides each compute in their own float order. Measured
/// over six channel/band pairs at **0.20 levels at worst on radv** and **0.80 on
/// lavapipe**, which is the whole of the headroom the software rasteriser has
/// left: the weighting cost this comparison the margin it used to carry, and a
/// backend noisier than llvmpipe would need this budget raised rather than the
/// disagreement explained away.
///
/// **What it can and cannot catch was measured too**, by scaling the mirror's
/// result and watching this go red: a systematic error of 2% moves the worst
/// pair by 1.64 levels and fails, and one of 1% would move it by about 0.8 and
/// would not. So the honest statement is that this catches a disagreement of
/// roughly a per cent and a half or more — which is every shape of transcription
/// slip the two implementations can have, since a permuted lane, a dropped band
/// or a transfer coefficient off by a factor moves the bright band by tens of
/// levels.
///
/// **It is also the anti-vacuity claim**, and a stronger one than a floor: it
/// asserts a *value*, so an unpainted frame misses it by the whole of the value.
const PROBE_MIRROR_LEVELS: f32 = 1.0;

/// The sRGB transfer function, encoding linear light into the swapchain's
/// levels.
///
/// The standard piecewise curve — IEC 61966-2-1, and what Vulkan's
/// `*_SRGB` formats are defined to apply on a write. Written out rather than
/// reached for because nothing in this workspace has needed it before: the
/// engine hands linear colour to an sRGB attachment and the hardware encodes it,
/// so this is the only place that has ever had to say what the hardware did.
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The floor's albedo, read out of the mesh the room is made of.
///
/// Read rather than written down: it is the one number in the forward model
/// below that belongs to somebody else, and a copy of it here would be a second
/// place that has to change when the open box is recoloured.
fn probe_floor_albedo() -> [f32; 3] {
    crcbl::shaders::mesh::OPEN_BOX_FACES
        .iter()
        .find(|face| face.name == "floor")
        .expect("the open box has a floor")
        .color
}

/// What the Rust mirror says one channel of one block of a probe-lit floor
/// should measure, in the frame's own levels.
///
/// The forward model [`PROBE_MIRROR_LEVELS`] justifies: the irradiance the mirror
/// gives the floor's normal at each pixel's own world position, times the floor's
/// albedo, through the sRGB encode. Per pixel and then averaged, in the same
/// order and over the same pixels [`block_channel`] reads, because the field has
/// a gradient across the block and the encode is not linear — evaluating the
/// centre and encoding once is a different number.
///
/// **The mirror is evaluated against
/// `crcbl_shaders::probe_visibility::ProbeVisibility::NONE`, the map that
/// occludes nothing**, so what comes back is the *plain trilinear blend* of the
/// rows the grid holds. Each caller says why that is the right prediction for
/// its own fixture — for one of them because no corner is occluded, and for the
/// other because every corner is.
fn predicted_block_channel(
    grid: &crcbl::render::scene::ProbeGrid,
    centre: (u32, u32),
    half: (u32, u32),
    index: usize,
) -> f32 {
    let albedo = probe_floor_albedo();
    let mut total = 0.0f32;
    let mut count = 0u32;
    for (x, y) in block_pixels(centre, half) {
        let irradiance = crcbl::shaders::probe::irradiance_at(
            &grid.volume,
            &grid.probes,
            &crcbl::shaders::probe_visibility::ProbeVisibility::NONE,
            probe_world(x, y),
            [0.0, 1.0, 0.0],
        );
        let lit = (albedo[index] * irradiance[index]).min(1.0);
        total += srgb_encode(lit) * 255.0;
        count += 1;
    }
    assert!(count > 0, "an empty block predicts nothing");
    total / count as f32
}

/// How many times brighter the `-X` band must be with the wall taken away.
///
/// **A ratio, so no uniform change can satisfy it**, and a floor set under what
/// the pair measures rather than at it: the run that landed this reads `0.00`
/// levels with the wall and about `123` without, which is every level the probe
/// had. Four is a floor with room for every backend's own arithmetic and still
/// far above the two levels `crcbl_golden::Tolerance::RASTERISER` allows two
/// frames of one scene to differ by.
const LEAK_RATIO: f32 = 4.0;

/// The least the `+X` band must **gain** when the wall is added, in levels.
///
/// The other half of the opposite-directions claim, and a number rather than a
/// bare inequality because the band is bright and 8-bit rounding alone can move
/// it by a level: the run that landed this reads about 205 open and 233 walled,
/// which is a gain of nearly thirty. Ten is a floor well under that and well
/// over the two levels `crcbl_golden::Tolerance::RASTERISER` allows.
const LEAK_MIN_GAIN: f32 = 10.0;

/// The least the unobstructed `-X` band must measure, in levels of 255.
///
/// The anti-vacuity half: without it a fixture that drew a black frame both ways
/// would satisfy every ratio below by dividing nothing by nothing. Set well under
/// the level the fixture is built to reach — see
/// `crcbl::screenshot::probe_leak_grid`, whose radiance is chosen so the
/// brightest band stays inside the swapchain.
const LEAK_MIN_LEVELS: f32 = 40.0;

/// **The rung's claim, on the device: a probe on the far side of a wall lights
/// nothing through it.**
///
/// One room drawn twice, differing in a single instance — the divider on the
/// plane `x = 0`. `crcbl::screenshot::probe_leak_forward` is the fixture, and its
/// grid holds one black probe at `-X` and one lit probe at `+X`, so the only
/// light in the frame starts on the far side of that wall from the `-X` band.
///
/// **The two bands move in opposite directions, and that is what nothing else
/// can fake.** Anything that darkened the frame — a wall's own occlusion, a
/// shadow, an exposure change — moves both bands the same way.
///
/// * The `-X` band **loses** its light, because the wall now stands between it
///   and the only probe that has any.
/// * The `+X` band **gains**, because the same wall stands between it and the
///   *black* probe, whose quarter of the blend was dragging it down.
///
/// A frame in which the visibility test did nothing draws both bands identically
/// with the wall and without it, and fails both.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_probe_behind_a_wall_lights_nothing_through_it() {
    crcbl_core::log::init_logging();

    let frame = |wall: bool| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::probe_leak_forward(device, queue, format, wall)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the probe leak scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    let walled = frame(true);
    let open = frame(false);
    // The green channel: the lit probe holds a neutral environment, so all three
    // carry the measurement and one of them is enough. Green is the channel a
    // tint anywhere in the chain would be least likely to spare.
    let band = |image: &Image, x: f32| block_channel(image, probe_pixel(x, 0.0), PROBE_BAND, 1);
    let at = crcbl::screenshot::LEAK_BAND_AT;
    let (near_walled, near_open) = (band(&walled, -at), band(&open, -at));
    let (far_walled, far_open) = (band(&walled, at), band(&open, at));
    eprintln!(
        "crcbl render e2e: probe leak — -X band {near_walled:.2} walled against \
         {near_open:.2} open; +X band {far_walled:.2} against {far_open:.2}"
    );

    assert!(
        near_open >= LEAK_MIN_LEVELS,
        "with nothing in the way the -X band must carry the lit probe's quarter of the \
         blend, and it measures {near_open:.2} of the {LEAK_MIN_LEVELS} this fixture is \
         built to reach — the probe term is not in this frame at all"
    );
    assert!(
        near_walled * LEAK_RATIO < near_open,
        "the -X band must lose the probe the wall now hides: {near_walled:.2} walled \
         against {near_open:.2} open, which is not the {LEAK_RATIO}× drop a probe on the \
         far side of a wall costs — light is leaking through it"
    );
    assert!(
        far_open + LEAK_MIN_GAIN < far_walled,
        "and the +X band must *gain* by at least {LEAK_MIN_GAIN} level(s), because the same \
         wall hides the black probe that was a quarter of its blend: {far_walled:.2} walled \
         against {far_open:.2} open — two bands moving the same way is one thing dimming the \
         room, not a visibility test"
    );
}

/// How many times more the unobstructed `-X` reflection must carry than the
/// walled one.
///
/// [`LEAK_RATIO`]'s shape and its reason — a ratio, so nothing uniform can
/// satisfy it. The run that landed this reads **0.00 levels walled against 71.10
/// open on radv and 0.00 against 71.20 on lavapipe**: the wall takes the whole
/// of the reflected probe, so the divisor is what it is set against rather than
/// what it measures. Four leaves every backend room and is still far above the
/// two levels `crcbl_golden::Tolerance::RASTERISER` allows.
const LEAK_MIRROR_RATIO: f32 = 4.0;

/// The least the `+X` reflection must **gain** when the wall is added, in levels.
///
/// The opposite-directions half. Measured at **17.5 levels on both radv and
/// lavapipe** — 138.00 walled against 120.50 open on each — and set at well
/// under a third of that, which is the same headroom [`LEAK_MIN_GAIN`] carries
/// over the band it guards.
const LEAK_MIRROR_MIN_GAIN: f32 = 5.0;

/// The least the unobstructed `-X` reflection must measure, in levels of 255.
///
/// [`LEAK_MIN_LEVELS`]'s anti-vacuity claim on the specular path: without it a
/// fixture whose reflection pass wrote nothing would satisfy the ratio by
/// dividing nothing by nothing. Measured at **71.10 levels on radv and 71.20 on
/// lavapipe**; set at a third of that.
const LEAK_MIRROR_MIN_LEVELS: f32 = 25.0;

/// **The rung's claim on the specular path: a probe on the far side of a wall
/// reflects nothing through it.**
///
/// [`a_probe_behind_a_wall_lights_nothing_through_it`]'s fixture through a
/// mirror — `crcbl::screenshot::probe_leak_reflection_forward` is the same room,
/// the same two probes and the same divider, with the room shaded as a metal at
/// roughness zero so `ssr.slang` marches it. Every ray the bands reflect leaves
/// the floor upward and outward, finds nothing in the depth buffer, and falls
/// back to the probe grid — which until this rung read those eight rows with no
/// visibility term at all, so the reflection leaked where the diffuse term had
/// stopped.
///
/// **What is measured is the reflection pass and nothing else**: each arm is
/// drawn twice, with the pair and without it, and the difference is what
/// `ssr_blur.slang` added. So the divider's own shadow, its occlusion and the
/// diffuse probe term all cancel before a single assertion runs.
///
/// **The two bands move in opposite directions**, which is what nothing uniform
/// can fake — [`a_probe_behind_a_wall_lights_nothing_through_it`] argues it in
/// full, and the argument is the same one with a mirror ray in place of a
/// hemisphere: the `-X` band loses the only lit probe to the wall, and the `+X`
/// band gains because the same wall hides the *black* probe that was dragging
/// its blend down.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_probe_behind_a_wall_reflects_nothing_through_it() {
    crcbl_core::log::init_logging();

    let frame = |wall: bool, effects| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::probe_leak_reflection_forward(
                    device, queue, format, wall, effects,
                )
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the probe leak mirror: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    let with_reflections = RenderEffects::DEFAULT_STACK;
    let without = RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS);
    let at = crcbl::screenshot::LEAK_BAND_AT;
    // The green channel, on the diffuse test's terms: the lit probe holds a
    // neutral environment, so one channel carries the whole measurement.
    let contribution = |wall: bool, x: f32| {
        let band =
            |effects| block_channel(&frame(wall, effects), probe_pixel(x, 0.0), PROBE_BAND, 1);
        band(with_reflections) - band(without)
    };
    let near_walled = contribution(true, -at);
    let near_open = contribution(false, -at);
    let far_walled = contribution(true, at);
    let far_open = contribution(false, at);
    eprintln!(
        "crcbl render e2e: probe leak mirror — -X band {near_walled:.2} walled against \
         {near_open:.2} open; +X band {far_walled:.2} against {far_open:.2}"
    );

    assert!(
        near_open >= LEAK_MIRROR_MIN_LEVELS,
        "with nothing in the way the -X band's reflection must carry the lit probe's quarter \
         of the blend, and it measures {near_open:.2} of the {LEAK_MIRROR_MIN_LEVELS} this \
         fixture is built to reach — the probe fallback is not in this reflection at all"
    );
    assert!(
        near_walled * LEAK_MIRROR_RATIO < near_open,
        "the -X band's reflection must lose the probe the wall now hides: {near_walled:.2} \
         walled against {near_open:.2} open, which is not the {LEAK_MIRROR_RATIO}× drop a \
         probe on the far side of a wall costs — light is leaking through it into the mirror"
    );
    assert!(
        far_open + LEAK_MIRROR_MIN_GAIN < far_walled,
        "and the +X band's reflection must *gain* by at least {LEAK_MIRROR_MIN_GAIN} level(s), \
         because the same wall hides the black probe that was a quarter of its blend: \
         {far_walled:.2} walled against {far_open:.2} open — two bands moving the same way is \
         one thing dimming the room, not a visibility test"
    );
}

/// The least the wall face must measure in red once the divider is gone, in
/// levels of 255.
///
/// **The anti-vacuity half, and here it is a strong one**: nothing but the
/// updater lights this face — the sun is turned away from it, the fixture's sun
/// carries no ambient and its sky is off — so a gather that never ran leaves it
/// black rather than merely dimmer. Measured at **101.59 levels on radv and
/// 101.00 on lavapipe**, and at **0.00 on both with the dispatch removed**, so
/// this is set at well under half of what the fixture reaches and an order of
/// magnitude above what the failure it guards leaves behind.
const BOUNCE_MIN_LEVELS: f32 = 40.0;

/// How much redder, in red-to-blue, the wall face must read with the divider
/// gone than with it in place.
///
/// **A ratio of ratios, so nothing that changes the room's brightness can
/// satisfy it** — the same shape [`LEAK_RATIO`] carries, and for the same
/// reason. The walled arm is not neutral and is not meant to be: the `-X` probe
/// still gathers its own half of a room lit by a warm sun, which is what the
/// 1.97 below is. What the divider takes from it is the red panel across the
/// room.
///
/// Measured at **3.078 open against 1.966 walled on radv** and **3.061 against
/// 1.966 on lavapipe**, which is a little over 1.56× either way. With
/// `probe_chebyshev` forced to one the walled arm reads **3.465** — it gathers
/// the panel through the divider — against 3.206 open, so the sabotage lands at
/// 0.93× and misses this by the whole of the distance rather than by a margin.
const BOUNCE_REDDER: f32 = 1.25;

/// **The updater's claim on the device: a probe gathers the sunlit surfaces it
/// can see, and nothing through a wall.**
///
/// `crcbl::screenshot::probe_bounce_forward` is the fixture — the divider room
/// with a red panel against its `+X` wall, a low white sun from `-X`, and two
/// probes whose rows the reflective shadow map fills every frame. Drawn twice,
/// with the divider and without it, exactly as
/// [`a_probe_behind_a_wall_lights_nothing_through_it`] draws its authored rows.
///
/// **What is measured is the probe term alone.** The camera looks straight at
/// the `-X` wall's inner face, which is turned away from the sun and reaches no
/// ambient — the fixture's sun carries none and its sky is off — so every level
/// in the block is irradiance the updater put in a row.
///
/// **The two channels move in opposite directions**, which is what nothing
/// uniform can fake. Taking the divider away hands the `-X` probe the red panel
/// across the room and takes away the divider's own sunlit `-X` face, which was
/// white: so red rises, blue falls, and the face's red-to-blue opens up. A
/// gather that ignored `probe_chebyshev` reads the red panel in **both** arms
/// and flattens that ratio; a dispatch that never ran leaves both arms' rows at
/// zero, and this face — which has no other light on it — goes black and misses
/// the floor below.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_updater_gathers_what_a_probe_can_see_and_nothing_through_a_wall() {
    crcbl_core::log::init_logging();

    let frame = |wall: bool| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::probe_bounce_forward(device, queue, format, wall)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the probe updater scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        // **The second frame, not the first.** The rows this reads are written
        // by the frame that reads them, but the visibility capture and the
        // instance uploads land on the first one, and a fixture that measured
        // the frame those share would be measuring the build as much as the
        // updater.
        let _build = setup.draw_and_readback().expect("the frame renders");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    let walled = frame(true);
    let open = frame(false);
    // The frame's middle, which is the `-X` wall's inner face and nothing else:
    // `crcbl::screenshot::probe_bounce_forward` aims the camera straight at it,
    // so this measurement inverts no projection.
    let middle = (EXTENT.0 / 2, EXTENT.1 / 2);
    let band = |image: &Image, channel| block_channel(image, middle, PROBE_BAND, channel);
    let (red_walled, red_open) = (band(&walled, 0), band(&open, 0));
    let (blue_walled, blue_open) = (band(&walled, 2), band(&open, 2));
    let walled_redness = red_walled / blue_walled;
    let open_redness = red_open / blue_open;
    eprintln!(
        "crcbl render e2e: probe updater — wall face red {red_walled:.2} walled against \
         {red_open:.2} open, blue {blue_walled:.2} against {blue_open:.2}, red-to-blue \
         {walled_redness:.3} against {open_redness:.3}"
    );

    assert!(
        red_open >= BOUNCE_MIN_LEVELS,
        "with the divider gone the wall face must carry the red panel's bounce, and it \
         measures {red_open:.2} of the {BOUNCE_MIN_LEVELS} this fixture is built to reach — \
         nothing but the updater lights this face, so a face this dark is a row the gather \
         never wrote"
    );
    assert!(
        open_redness > walled_redness * BOUNCE_REDDER,
        "and the face must read {BOUNCE_REDDER}× redder with the divider gone: a red-to-blue \
         of {open_redness:.3} against {walled_redness:.3} walled ({red_walled:.2}/\
         {blue_walled:.2} against {red_open:.2}/{blue_open:.2}) — the probe beside this face \
         is gathering the red panel through the wall"
    );
}

/// How far along `x` the clipmap profile is read, in world units.
///
/// Level 0's boundary is one [`crcbl::screenshot::probe_clipmap_grid`] spacing
/// from the centre and its band opens a quarter of that inside, so a sweep this
/// long carries the flat interior, the whole band and a stretch of pure level 1
/// past it. It stops well short of the `+X` wall: what is left between them is
/// more than `crcbl_render::ForwardRenderer`'s occlusion radius, so the
/// occlusion over every pixel read here is one rather than merely near it —
/// the same condition [`PROBE_BAND_AT`] is chosen under.
const CLIPMAP_SWEEP_TO: f32 = 1.1;

/// The half-height of the column each sample of the profile averages, in
/// pixels.
///
/// The field is a function of `x` alone, so averaging *down* the frame costs no
/// gradient at all and takes the 8-bit rounding out of a per-pixel difference —
/// which is the quantity this profile is measured in. **One column wide**, and
/// that is not an oversight: a sample two columns wide would share a column
/// with the sample beside it and halve the very step this measures. Narrow
/// enough on `z` to stay in the floor's flat middle, far from both `±Z` walls.
const CLIPMAP_COLUMN: u32 = 5;

/// How much steeper than an even ramp one step of the profile may be, in linear
/// light.
///
/// **The number the band blend is held to.** Level 0's band is
/// `crcbl_shaders::probe::LEVEL_BAND` of its half-extent, which this fixture
/// puts about fifteen pixels of the profile inside; the share therefore moves a
/// fifteenth per pixel and — *in linear light* — so does the value, while a
/// level **switch** moves all of it in one step. The measurement is taken in
/// linear light rather than in the frame's levels for exactly that reason: the
/// sRGB curve is three times steeper near black than a straight line is, so a
/// perfectly even blend still shows a threefold step in levels and the ceiling
/// would have to be loosened to a number that no longer separates the two
/// cases. The run that landed this measures a worst step of **1.07 times** the
/// even ramp on both radv and lavapipe, and a level switch — the whole 0.82 of
/// travel in one sample against a ramp of 0.054 — misses this ceiling by more
/// than a factor of seven.
const CLIPMAP_STEP_RAMPS: f32 = 2.0;

/// The least the profile must travel end to end, in linear light.
///
/// The anti-vacuity half, and it is what makes the step assertion above a check
/// rather than a formality: a frame that read one level the whole way is flat,
/// takes no step anywhere, and would satisfy any ceiling. The two levels are a
/// red and a blue constant environment of
/// `crcbl::screenshot::probe_clipmap_grid`'s radiance, so each channel travels
/// the whole way from that environment's reflected radiance to nothing — which
/// is `π · radiance · albedo`, and this is well under it.
const CLIPMAP_MIN_TRAVEL: f32 = 0.4;

/// How far the profile may sit from what
/// [`crcbl_shaders::probe::irradiance_at`](crcbl::shaders::probe::irradiance_at)
/// predicts for it, in levels of 255.
///
/// [`PROBE_MIRROR_LEVELS`]' forward model exactly — the same floor, the same
/// camera, the same sun with its direct and ambient terms at zero, the same
/// reflection refusal — so the same list of exactly-zero and exactly-one steps
/// applies and what is left to cover is 8-bit rounding, the sRGB encode's
/// precision and half a pixel of disagreement about where a fragment centre is.
///
/// **It is the check on the level pick itself.** The mirror decides which level
/// a point reads and in what share, so a shader that picked a different level,
/// or ramped over a different band, disagrees by tens of levels across the
/// blend rather than by a fraction of one. Measured over the whole profile at
/// **0.66 levels at worst on radv** and **0.67 on lavapipe**; the budget is a
/// little wider than `PROBE_MIRROR_LEVELS`' because this profile is read one
/// column at a time where the field has a steep gradient, so half a pixel of
/// disagreement about a fragment's centre is worth more of a level here and
/// there is no block average to take the 8-bit rounding out along `x`.
const CLIPMAP_MIRROR_LEVELS: f32 = 1.5;

/// The sRGB transfer function run backwards, from the frame's levels to the
/// linear light behind them.
///
/// [`srgb_encode`]'s inverse, and the profile above is differenced through it
/// because the step it measures is a *fraction of a blend*, which is a linear
/// quantity: the encode alone turns an even ramp into a curve three times
/// steeper at one end than at the other.
fn srgb_decode(encoded: f32) -> f32 {
    if encoded <= 0.040_449_935 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// **The clipmap's claim, on the device: a fragment crossing a level boundary
/// fades rather than steps, and it fades the way the host says it does.**
///
/// `docs/plan/50-irradiance-probes.md`'s layered density.
/// `crcbl::screenshot::probe_clipmap_forward` is the fixture: one room, one
/// flat floor, and a volume of two levels whose rows are a red constant
/// environment and a blue one. Every row of a level is identical, so the
/// trilinear gather inside a level is flat and the **only** thing that can move
/// a pixel along the floor is which level it read and in what share.
///
/// Two things are measured over one profile across the boundary:
///
/// * **It does not step.** The largest difference between neighbouring samples,
///   in linear light, is held under [`CLIPMAP_STEP_RAMPS`] times what an even
///   ramp across the band would take — a level *change* puts the whole travel
///   into one sample and misses it by a factor of five.
/// * **It is the profile the host predicts.** Every sample is compared against
///   `crcbl_shaders::probe::irradiance_at` over the rows the device was given,
///   which is what holds `mesh.slang`'s level pick and band to the mirror
///   rather than merely to smoothness — a shader that faded over the wrong
///   width, or into the wrong level, is smooth and wrong.
///
/// The two together fail in both directions: a flat frame misses
/// [`CLIPMAP_MIN_TRAVEL`], a switching one misses the step ceiling, and one
/// that fades over its own band misses the mirror.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_fragment_crossing_a_clipmap_level_fades_into_it() {
    crcbl_core::log::init_logging();

    let setup = OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, |device, queue, format| {
        crcbl::screenshot::probe_clipmap_forward(device, queue, format)
    })
    .unwrap_or_else(|why| panic!("a GPU backend opens for the probe clipmap scene: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);
    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    setup.finish();
    let image =
        Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image");

    // The profile: every column of the frame from the room's centre out to
    // `CLIPMAP_SWEEP_TO`, each averaged down a short strip of the floor.
    let grid = crcbl::screenshot::probe_clipmap_grid();
    let albedo = probe_floor_albedo();
    let centre = probe_pixel(0.0, 0.0);
    let outer = probe_pixel(CLIPMAP_SWEEP_TO, 0.0);
    assert!(
        outer.0 < centre.0,
        "world +X must be the frame's left, which is what `probe_pixel` says"
    );
    let rows =
        || centre.1.saturating_sub(CLIPMAP_COLUMN)..(centre.1 + CLIPMAP_COLUMN).min(EXTENT.1);

    let mut measured: Vec<[f32; 3]> = Vec::new();
    let mut predicted: Vec<[f32; 3]> = Vec::new();
    for column in outer.0..=centre.0 {
        let mut got = [0.0f32; 3];
        let mut want = [0.0f32; 3];
        let mut count = 0u32;
        for y in rows() {
            let pixel = image.pixel(column, y).expect("inside the frame");
            // The same pixels, through `predicted_block_channel`'s forward
            // model — evaluated per pixel because the field has a gradient
            // across the strip and the sRGB encode is not linear, so the mean
            // of the encodes is not the encode of the mean.
            let irradiance = crcbl::shaders::probe::irradiance_at(
                &grid.volume,
                &grid.probes,
                // Every probe of every level stands in open air inside the room
                // — which is what `probe_clipmap_grid`'s spacing is chosen for
                // — so the capture this fixture runs must leave every corner
                // its whole weight, and this comparison is what says so.
                &crcbl::shaders::probe_visibility::ProbeVisibility::NONE,
                probe_world(column, y),
                [0.0, 1.0, 0.0],
            );
            for channel in 0..3 {
                got[channel] += f32::from(pixel[channel]);
                want[channel] +=
                    srgb_encode((albedo[channel] * irradiance[channel]).min(1.0)) * 255.0;
            }
            count += 1;
        }
        assert!(count > 0, "an empty column measures nothing");
        measured.push(got.map(|sum| sum / count as f32));
        predicted.push(want.map(|sum| sum / count as f32));
    }
    assert!(
        measured.len() > 2,
        "a profile of {} sample(s) has no neighbouring pair to compare",
        measured.len()
    );

    let apart = |a: [f32; 3], b: [f32; 3]| {
        (0..3).fold(0.0f32, |worst, channel| {
            worst.max((a[channel] - b[channel]).abs())
        })
    };
    // The travel and the step in linear light, where a blend is even; the
    // mirror comparison stays in the frame's own levels, which is the unit
    // `PROBE_MIRROR_LEVELS` is written in.
    let linear: Vec<[f32; 3]> = measured
        .iter()
        .map(|sample| sample.map(|level| srgb_decode(level / 255.0)))
        .collect();
    let travel = apart(linear[0], linear[linear.len() - 1]);
    let step = linear
        .windows(2)
        .fold(0.0f32, |worst, pair| worst.max(apart(pair[0], pair[1])));
    let miss = measured
        .iter()
        .zip(&predicted)
        .fold(0.0f32, |worst, (got, want)| worst.max(apart(*got, *want)));

    // What an even ramp across the band would take per sample, which is the
    // unit the step is judged in. The band is `LEVEL_BAND` of level 0's
    // half-extent, read out of the volume rather than written down again.
    let half_extent = 0.5 * (grid.volume.counts[0] - 1) as f32 * grid.volume.level_spacing(0)[0];
    let band_samples = crcbl::shaders::probe::LEVEL_BAND * half_extent * PROBE_PIXELS_PER_UNIT;
    let ramp = travel / band_samples;
    eprintln!(
        "crcbl render e2e: probe clipmap — {} samples travel {travel:.4} in linear light, \
         the worst step is {step:.4} against a {ramp:.4} ramp, and the mirror misses by \
         {miss:.2} level(s)",
        measured.len()
    );

    assert!(
        travel >= CLIPMAP_MIN_TRAVEL,
        "the profile must cross from one level to the other: it travelled {travel:.4} of \
         the {CLIPMAP_MIN_TRAVEL} this fixture is built to, which is a frame that read one \
         level the whole way"
    );
    assert!(
        step <= CLIPMAP_STEP_RAMPS * ramp,
        "one sample of the profile moved {step:.4}, past the {:.4} that is \
         {CLIPMAP_STEP_RAMPS} times an even ramp across the band — the read changed level \
         rather than fading into it",
        CLIPMAP_STEP_RAMPS * ramp
    );
    assert!(
        miss <= CLIPMAP_MIRROR_LEVELS,
        "the frame and `crcbl_shaders::probe::irradiance_at` disagree by {miss:.2} \
         level(s), past the {CLIPMAP_MIRROR_LEVELS} this comparison allows — the shader \
         picks a level, or fades into it, differently from the host that decides both"
    );
}

/// The whole-probe steps `a_scrolled_volume_reads_the_rows_the_mirror_does`
/// sweeps the level through.
///
/// **Every residue of [`crcbl::screenshot::SCROLL_COUNT`] exactly once**, from
/// inputs that are negative, zero and past a whole level in turn — so what the
/// sweep covers is the wrap itself and not one convenient offset of it. A
/// shader that dropped the wrap draws the authored order at every one of them
/// except the offset of zero.
const SCROLL_STEPS: [i32; 4] = [-1, 0, 1, 2];

/// Where along the floor that sweep is measured, in world units on `x`.
///
/// Spread across the level rather than gathered at its centre, and each of them
/// clear of the room's `±X` walls by more than
/// `crcbl_render::ForwardRenderer`'s occlusion radius — the same clearance
/// [`PROBE_BAND_AT`] is chosen under, so the forward model
/// [`predicted_block_channel`] applies is the one this measures.
const SCROLL_BANDS: [f32; 5] = [-1.2, -0.6, 0.0, 0.6, 1.2];

/// How far a scrolled band may sit from the Rust mirror's prediction, in levels
/// of 255.
///
/// [`PROBE_MIRROR_LEVELS`]' forward model exactly — the same room, camera, sun
/// and reflection refusal — so what is left to cover is 8-bit rounding, the
/// sRGB encode's precision and half a pixel of disagreement about where a
/// fragment centre is. **Measured rather than chosen**: over the four offsets
/// and five bands of this sweep the run of 2026-09-05 reads a worst miss of
/// 0.30 levels on radv and 0.40 on lavapipe, and this is set with room over
/// both and far under the level at which a misread row would show — the rows
/// differ from one another by tens of levels, which is what makes a wrap that
/// is off by one a miss of that size rather than of this one.
const SCROLL_MIRROR_LEVELS: f32 = 2.0;

/// **A scrolled level reads the rows the host says it does, at every offset of
/// the wrap.**
///
/// `docs/plan/50-irradiance-probes.md`'s toroidal addressing on the device.
/// `crcbl_shaders::probe::ProbeVolume::row` and `mesh.slang`'s `probe_row` are
/// two spellings of one rule — add the level's scroll offset to the cell and
/// bring it back inside the counts — and nothing about a compile, a golden or
/// any other test in this suite says they agree: an unscrolled volume has an
/// offset of zero and the wrap is the identity, which is every scene this engine
/// ships.
///
/// So the fixture's four rows hold four *different* constant environments and
/// the level is swept through every offset it has. At each of them the floor is
/// measured at five places and compared against
/// `crcbl_shaders::probe::irradiance_at` over the same header — absolutely
/// rather than in proportion, on
/// [`the_shader_and_the_rust_mirror_agree_about_the_irradiance`]'s terms.
///
/// **The failure names the offset and the band**, because that is the thing a
/// wrap gets wrong: a shader that dropped the modulo agrees at the offset of
/// zero and disagrees at every other, and one that is off by one disagrees at
/// the band nearest the level's far face.
///
/// # How it was shown to fail
///
/// By deleting the compare from `mesh.slang`'s `probe_wrap` — `return at;` —
/// regenerating the artifacts and running this on radv, which reported
///
/// > at a step of -1 the band at x = -1.2 measures 230.00 on green and the Rust
/// > mirror of `probe_irradiance` predicts 166.08, a miss of 63.92 level(s)
/// > against a budget of 2 — the shader wraps a scrolled cell onto a different
/// > row than `ProbeVolume::row` does
///
/// and by weakening it to `at > count`, the off-by-one that leaves one cell of
/// every axis reading its neighbour's row, which reported the same band and the
/// same miss. Both were restored and the artifacts regenerated.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_scrolled_volume_reads_the_rows_the_mirror_does() {
    crcbl_core::log::init_logging();

    let mut worst = 0.0f32;
    let mut worst_at = (0, 0.0f32);
    for steps in SCROLL_STEPS {
        let setup = OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, |device, queue, format| {
            crcbl::screenshot::probe_scroll_forward(device, queue, format, steps)
        })
        .unwrap_or_else(|why| panic!("a GPU backend opens for the probe scroll scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        let image =
            Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image");

        let grid = crcbl::screenshot::probe_scroll_grid(steps);
        assert_eq!(
            grid.volume.level_offset(0)[0],
            steps.rem_euclid(crcbl::screenshot::SCROLL_COUNT as i32) as u32,
            "the fixture must carry the offset this sweep is about"
        );
        for x in SCROLL_BANDS {
            let at = probe_pixel(x, 0.0);
            for (name, channel) in [("red", 0), ("green", 1), ("blue", 2)] {
                let measured = block_channel(&image, at, PROBE_BAND, channel);
                let predicted = predicted_block_channel(&grid, at, PROBE_BAND, channel);
                let miss = (measured - predicted).abs();
                if miss > worst {
                    worst = miss;
                    worst_at = (steps, x);
                }
                assert!(
                    miss <= SCROLL_MIRROR_LEVELS,
                    "at a step of {steps} the band at x = {x} measures {measured:.2} on {name} \
                     and the Rust mirror of `probe_irradiance` predicts {predicted:.2}, a miss of \
                     {miss:.2} level(s) against a budget of {SCROLL_MIRROR_LEVELS} — the shader \
                     wraps a scrolled cell onto a different row than `ProbeVolume::row` does"
                );
            }
        }
    }
    // Anti-vacuity: the offsets really do draw different frames, so the
    // agreement above is not four readings of one picture. The rows differ from
    // one another by tens of levels and the mirror tracks the device through
    // every one of them.
    let spread = {
        let flat = crcbl::screenshot::probe_scroll_grid(0);
        let rolled = crcbl::screenshot::probe_scroll_grid(1);
        let mut apart = 0.0f32;
        for x in SCROLL_BANDS {
            let at = probe_pixel(x, 0.0);
            for channel in 0..3 {
                let a = predicted_block_channel(&flat, at, PROBE_BAND, channel);
                let b = predicted_block_channel(&rolled, at, PROBE_BAND, channel);
                apart = apart.max((a - b).abs());
            }
        }
        apart
    };
    assert!(
        spread > 10.0 * SCROLL_MIRROR_LEVELS,
        "one step of this fixture's level moves a band by {spread:.2} level(s), which is not \
         enough for the agreement above to be about the wrap at all"
    );
    eprintln!(
        "crcbl render e2e: probe scroll — the shader and the Rust mirror agree to {worst:.2} \
         level(s) at worst, at a step of {} and x = {}, over {} offsets × {} bands × 3 channels, \
         where one step moves a band by {spread:.2}",
        worst_at.0,
        worst_at.1,
        SCROLL_STEPS.len(),
        SCROLL_BANDS.len()
    );
}

/// How many pixels of the frame one world unit of
/// [`crcbl::screenshot::probe_slab_forward`]'s floor is.
///
/// [`PROBE_PIXELS_PER_UNIT`]'s arithmetic against that fixture's own higher
/// camera, read from the constant rather than written out again.
const SLAB_PIXELS_PER_UNIT: f32 =
    (EXTENT.1 as f32 / 2.0) / (crcbl::screenshot::SLAB_CAMERA_UP * 0.577_350_3);

/// Where a point on that fixture's floor lands in the frame — [`probe_pixel`]'s
/// mapping at [`SLAB_PIXELS_PER_UNIT`].
fn slab_pixel(x: f32, z: f32) -> (u32, u32) {
    let column = EXTENT.0 as f32 / 2.0 - x * SLAB_PIXELS_PER_UNIT;
    let row = EXTENT.1 as f32 / 2.0 - z * SLAB_PIXELS_PER_UNIT;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every band below is inside the frame, which the block reader asserts"
    )]
    (column as u32, row as u32)
}

/// Where the `+X` band of the slab fixture is measured, in world units.
///
/// Past the divider by more than `crcbl_render::ForwardRenderer`'s occlusion
/// radius, so the ambient-occlusion pass is not what darkens it — and inside the
/// cell the arrived probe is a corner of, so most of its blend is that probe's.
const SLAB_BAND_AT: f32 = 0.6;

/// The most red the `+X` band may carry once the arrived probe's map has been
/// re-captured, in levels of 255.
///
/// The arrived probe is the only red in the room and the divider stands between
/// it and this band, so the honest answer is zero and what this covers is 8-bit
/// rounding and the floor `crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT`
/// leaves. **Measured**: the run of 2026-09-05 reads 1.00 levels on radv and
/// 1.00 on lavapipe with the recapture, against 199.50 with the recapture made
/// a no-op — so this sits four levels over what the fixture measures and two
/// hundred under what a missing recapture reads.
const SLAB_LEAK_LEVELS: f32 = 8.0;

/// The least red the band under the arrived probe must carry, in levels of 255.
///
/// The anti-vacuity half: without it a frame with no red anywhere satisfies the
/// leak bound perfectly. **Measured**: the same run reads 234.00 on radv and
/// 234.00 on lavapipe, and this is a floor well under that and far over
/// [`SLAB_LEAK_LEVELS`].
const SLAB_MIN_LEVELS: f32 = 60.0;

/// Where the band over the probe that **stayed** is read, in world units on `x`.
///
/// Just inside that probe rather than over it, and the level's own edges are
/// what decide the difference: the retained probe is the level's *near* corner
/// before the step and its *far* corner after, so the floor beyond it reads the
/// other probe on one side of the step and clamps on the other. Inside it both
/// arms take their whole blend from the retained row — before the step by
/// clamping onto it, after the step because the only other corner is the
/// arrived probe with the divider in the way — which is the comparison this
/// band is for.
const SLAB_KEPT_AT: f32 = 2.4;

/// How far the band over the probe that **stayed** may move across the step, in
/// levels of 255.
///
/// It should not move at all: that probe stands at the same world position
/// before and after, in the same row, with the same map, and the band over it
/// takes its whole blend from that one corner either way — which is the claim
/// that makes a scroll a slab rather than a level. **Measured**: the run of
/// 2026-09-05 reads a worst channel move of 0.00 levels on radv and 0.00 on
/// lavapipe, the recapture sabotage included. Two is `crcbl_golden::Tolerance::RASTERISER`'s allowance, which is
/// the least this may be without asserting bit-equality across a re-capture.
const SLAB_KEPT_LEVELS: f32 = 2.0;

/// **The slab a scroll exposes is re-captured, and the probes it did not expose
/// are left where they were.**
///
/// `docs/plan/50-irradiance-probes.md`'s recapture on the device. The fixture
/// captures two probes on the `+X` side of a divider and then takes one whole
/// probe step back, which stands the red probe a quarter unit from the divider's
/// far face and leaves the green one exactly where it was.
///
/// Three readings, and no two of them can be satisfied by the same mistake:
///
/// * The floor **under** the arrived probe carries its red, which says the step
///   happened and that row's map is one the fragment can be lit through.
/// * The band **past the divider** carries none of that red, which says the map
///   was taken again where the probe now stands. The map it held until then was
///   captured three units further out with nothing in the way, and reports open
///   space in exactly this direction — so a scroll that did not re-capture
///   lights this band through a wall.
/// * The band over the probe that **stayed** reads the same as it did before the
///   step, which says the step moved one slab rather than the level.
///
/// # How it was shown to fail
///
/// By making `crcbl_render::probe_capture::recapture` return `Ok(())` before it
/// records anything — the scroll without its slab — and running this on radv,
/// which reported
///
/// > the band past the divider carries 199.50 level(s) of the arrived probe's
/// > red, past the 8 this allows — the row the step exposed is holding the map
/// > it was captured with three units further out, which sees no wall in this
/// > direction
///
/// with the other two readings still green, since the arrived probe lights the
/// floor beneath it either way and the probe that stayed was never touched.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_scroll_recaptures_the_slab_it_exposed_and_nothing_else() {
    crcbl_core::log::init_logging();

    // What the step is about, asserted on the host first: one row exposed out of
    // two, and the other standing where it stood. A fixture whose follow moved
    // the level whole would satisfy every reading below for the wrong reason.
    let volume = crcbl::screenshot::probe_slab_grid().volume;
    let mut moved = volume;
    let exposed = moved.follow(crcbl::screenshot::slab_follow_point());
    assert_eq!(
        exposed,
        vec![1],
        "the fixture's step must expose row 1 alone, which is the red probe"
    );
    assert_eq!(
        moved.position(0, [1, 0, 0]),
        volume.position(0, [0, 0, 0]),
        "the probe that stayed must stand where it stood"
    );
    assert_eq!(moved.row(0, [1, 0, 0]), volume.row(0, [0, 0, 0]));

    let draw = |follow: bool| {
        let setup = OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, |device, queue, format| {
            crcbl::screenshot::probe_slab_forward(device, queue, format, follow)
        })
        .unwrap_or_else(|why| panic!("a GPU backend opens for the probe slab scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image")
    };
    let before = draw(false);
    let after = draw(true);

    let arrived = slab_pixel(crcbl::screenshot::SLAB_ARRIVED_AT, 0.0);
    let across = slab_pixel(SLAB_BAND_AT, 0.0);
    let stayed = slab_pixel(SLAB_KEPT_AT, 0.0);

    let lit = block_channel(&after, arrived, PROBE_BAND, 0);
    let leaked = block_channel(&after, across, PROBE_BAND, 0);
    let kept = (0..3).fold(0.0f32, |worst, channel| {
        worst.max(
            (block_channel(&after, stayed, PROBE_BAND, channel)
                - block_channel(&before, stayed, PROBE_BAND, channel))
            .abs(),
        )
    });
    eprintln!(
        "crcbl render e2e: probe slab — the floor under the arrived probe carries {lit:.2} \
         level(s) of its red, the band past the divider carries {leaked:.2}, and the band over \
         the probe that stayed moved {kept:.2}"
    );

    assert!(
        lit >= SLAB_MIN_LEVELS,
        "the floor under the arrived probe carries {lit:.2} level(s) of red, under the \
         {SLAB_MIN_LEVELS} this fixture is built to reach — the step did not happen, or the row \
         it exposed was captured somewhere it cannot light this floor from"
    );
    assert!(
        leaked <= SLAB_LEAK_LEVELS,
        "the band past the divider carries {leaked:.2} level(s) of the arrived probe's red, past \
         the {SLAB_LEAK_LEVELS} this allows — the row the step exposed is holding the map it was \
         captured with three units further out, which sees no wall in this direction"
    );
    assert!(
        kept <= SLAB_KEPT_LEVELS,
        "the band over the probe that stayed moved {kept:.2} level(s) across the step, past the \
         {SLAB_KEPT_LEVELS} this allows — a scroll re-captured or re-addressed a probe it was \
         supposed to leave alone"
    );
}

/// How far the sealed band may sit from the plain trilinear read, in levels of
/// 255.
///
/// [`PROBE_MIRROR_LEVELS`]' forward model exactly — the same floor, the same
/// camera, the same sun with its direct and ambient terms at zero, the same
/// reflection refusal, and a band chosen under the same clearance from every
/// surface — so the same list of exactly-zero and exactly-one steps applies and
/// what is left to cover is 8-bit rounding, the sRGB encode's precision and half
/// a pixel of disagreement about where a fragment centre is.
///
/// Measured over all three channels at **0.20 levels at worst on radv and 0.20
/// on lavapipe**, and set at [`PROBE_MIRROR_LEVELS`]' own budget — a band this
/// far from every surface, read as a block average, is the same measurement that
/// constant was set under.
const SEALED_MIRROR_LEVELS: f32 = 1.0;

/// The least the half-sealed band must carry over the plain trilinear read, in
/// levels of 255.
///
/// **The claim that the vaults occlude anything at all**, which is the one thing
/// the sealed arm cannot say on its own: a cell whose every corner is weighed out
/// draws the same picture as a cell whose every corner is whole, so without this
/// a fixture whose vaults leaked would satisfy the comparison below by agreeing
/// with the mirror for the wrong reason. With only the black `-X` probe sealed
/// its three quarters of the blend are weighed out and the band jumps to nearly
/// the whole of the lit probe.
///
/// The run that landed this reads **103.90 levels of gain at worst over the
/// three channels on radv and 103.90 on lavapipe** — 224.00 against a plain
/// blend of 120.10 on blue, the narrowest of them. Set at well under half of
/// that, and far over the two levels `crcbl_golden::Tolerance::RASTERISER`
/// allows two frames of one scene to differ by.
const SEALED_UNSEALED_GAIN: f32 = 40.0;

/// The least the sealed band must measure, in levels of 255.
///
/// The anti-vacuity half, and here it is also the finiteness claim: the case
/// this fixture puts on a device is the one where the gather's divisor would be
/// a sum of zeroes without `PROBE_OCCLUDED_WEIGHT`, and a fragment that divided
/// by nothing reaches the swapchain as a hole rather than as a floor.
///
/// Measured at **119.90 levels at worst over the three channels on radv and
/// 119.90 on lavapipe**; set at half of that.
const SEALED_MIN_LEVELS: f32 = 60.0;

/// **The floor under the gather's divisor, on the device: a cell hidden on every
/// side keeps the light it had rather than becoming a hole.**
///
/// `mesh.slang`'s `probe_irradiance` divides its weighted sum of eight corners
/// by the sum of their weights, and `crcbl_shaders::probe_visibility`'s
/// `OCCLUDED_WEIGHT` is the floor under each of those weights. No other probe
/// test here reaches the case it exists for — [`Scene::Probes`] hides none of a
/// band's corners and the divider fixtures hide some — and it is the only
/// arrangement in which that floor is what decides the pixel: until this test
/// existed nothing in the tree put a shaded point where every corner was
/// occluded, so the constant could be deleted with every golden and every
/// assertion still green.
///
/// `crcbl::screenshot::probe_sealed_forward` is the fixture: two probes, the
/// `-X` one black and the `+X` one a constant environment, with a closed vault
/// of its own around each. One arm seals both probes and one seals only the
/// black one, and the pair is what makes this a measurement:
///
/// * **Seal only the black probe** and its three quarters of the blend are
///   weighed out, so the band must carry nearly the whole of the lit probe —
///   [`SEALED_UNSEALED_GAIN`] over the quarter the plain blend would give it.
///   That is the proof the vaults occlude anything at all.
/// * **Seal both** and every corner is at the floor, which divides straight back
///   out: the band must be the plain trilinear read to within
///   [`SEALED_MIRROR_LEVELS`], evaluated by
///   `crcbl_shaders::probe::irradiance_at` over the rows the device was given.
///
/// **The mirror comparison is the one the floor holds up.** Delete the floor
/// from `probe_weight` in `mesh.slang` and the sealed arm's two corners keep raw
/// Chebyshev bounds instead — bounds that differ between a probe in a tight vault
/// and one in a roomy vault by orders of magnitude, which is what
/// `crcbl::screenshot`'s two vault half-widths are sized for — so the weighted
/// mean stops being the plain one and this band moves by tens of levels.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_probe_cell_sealed_on_every_side_keeps_the_plain_blend() {
    crcbl_core::log::init_logging();

    let frame = |both: bool| {
        let setup =
            OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, move |device, queue, format| {
                crcbl::screenshot::probe_sealed_forward(device, queue, format, both)
            })
            .unwrap_or_else(|why| panic!("a GPU backend opens for the sealed probe scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        Image::from_readback(width, height, &pixels, channel_order(format))
            .expect("the readback is exactly one image")
    };

    let sealed = frame(true);
    let half = frame(false);
    // **Every corner occluded, which is why the plain blend is the prediction
    // here.** `ProbeVisibility::NONE` gives every corner its whole weight and
    // `OCCLUDED_WEIGHT` gives every corner the same share of one; both are a
    // weighted mean whose weights are equal, so both are the unweighted
    // trilinear blend. That equality is the property this test is about, and it
    // is what makes a mirror that knows nothing about the vaults the right
    // thing to compare the sealed arm against.
    let grid = crcbl::screenshot::probe_sealed_grid();
    let at = probe_pixel(
        crcbl::screenshot::SEALED_BAND_X,
        crcbl::screenshot::SEALED_BAND_Z,
    );
    let mut worst = 0.0f32;
    for (name, channel) in [("red", 0), ("green", 1), ("blue", 2)] {
        let predicted = predicted_block_channel(&grid, at, PROBE_BAND, channel);
        let measured = block_channel(&sealed, at, PROBE_BAND, channel);
        let unsealed = block_channel(&half, at, PROBE_BAND, channel);
        let miss = (measured - predicted).abs();
        worst = worst.max(miss);
        eprintln!(
            "crcbl render e2e: sealed cell — {name} band {measured:.2} sealed against \
             {unsealed:.2} with only the black probe sealed, and a plain blend of \
             {predicted:.2}"
        );

        assert!(
            measured >= SEALED_MIN_LEVELS,
            "the sealed band's {name} channel measures {measured:.2} of the \
             {SEALED_MIN_LEVELS} this fixture is built to reach — a cell hidden on every \
             side has become a hole in the floor rather than keeping the light it had"
        );
        assert!(
            unsealed >= predicted + SEALED_UNSEALED_GAIN,
            "with only the black probe sealed the {name} band must lose that probe's three \
             quarters of the blend and carry nearly the whole of the lit one: it measures \
             {unsealed:.2} against a plain blend of {predicted:.2}, which is not the \
             {SEALED_UNSEALED_GAIN} level(s) of gain a weighed-out corner costs — these \
             vaults are not occluding anything, and the comparison below would then agree \
             with the mirror for the wrong reason"
        );
        assert!(
            miss <= SEALED_MIRROR_LEVELS,
            "the sealed band's {name} channel measures {measured:.2} and the plain trilinear \
             blend of the same rows is {predicted:.2}, a miss of {miss:.2} level(s) against a \
             budget of {SEALED_MIRROR_LEVELS} — a cell whose every corner is at \
             `crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT` must divide that constant \
             straight back out, so a band that moved is a gather weighing its corners by \
             something the floor no longer holds equal"
        );
    }
    eprintln!(
        "crcbl render e2e: sealed cell — the sealed band and the plain blend agree to \
         {worst:.2} level(s) at worst over all three channels"
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_probes_scene_lights_its_room_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Probes,
        "probes",
        EXTENT,
        MIN_COLORS_PROBES,
        the_probe_grid_lights_each_end_of_the_room_in_its_own_colour,
    );
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_probes_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Probes,
        "probes",
        MIN_COLORS_PROBES,
        the_probe_grid_lights_each_end_of_the_room_in_its_own_colour,
    );
}

/// [`Scene::Probes`]' claim: **the two ends of one flat floor carry opposite
/// colours, and the frame is the irradiance the Rust mirror computes.**
///
/// **The golden cannot make either claim.** A probe volume that evaluated to its
/// constant band alone draws a flat lit floor, which is a perfectly plausible
/// picture of a room under an even sky and would be blessed without comment; so
/// would a volume whose linear band was zero, and so would a flat ambient with no
/// probes in it at all. Every one of those is a *uniform* change, and the first
/// claim here is a ratio between two bands of one frame, which no uniform change
/// can move.
///
/// Two bands, both on the floor, both [`PROBE_BAND_AT`] from the frame's centre:
///
/// * `-X`, nearest the probe whose red source is overhead. It must be
///   unmistakably the redder of the two.
/// * `+X`, nearest the probe whose blue source is overhead. It must be
///   unmistakably the bluer.
///
/// **The two ratios run in opposite directions, and that is what nothing else can
/// fake.** Anything that brightens or darkens one side of the frame — a vignette,
/// a light nobody meant to leave on, an occlusion field that is not symmetric —
/// moves red and blue *together* and satisfies neither ratio. Only a field whose
/// linear band changes sign across the room moves them apart.
///
/// Then the second claim: [`the_shader_and_the_rust_mirror_agree_about_the_irradiance`],
/// which is what closes the gap the probe row's first slice left open.
///
/// [`the_shader_and_the_rust_mirror_agree_about_the_irradiance`]: fn@the_shader_and_the_rust_mirror_agree_about_the_irradiance
fn the_probe_grid_lights_each_end_of_the_room_in_its_own_colour(image: &Image) {
    let band =
        |x: f32, channel: usize| block_channel(image, probe_pixel(x, 0.0), PROBE_BAND, channel);
    let red = (band(-PROBE_BAND_AT, 0), band(PROBE_BAND_AT, 0));
    let blue = (band(-PROBE_BAND_AT, 2), band(PROBE_BAND_AT, 2));
    eprintln!(
        "crcbl render e2e: probes — red {:.1} at -X against {:.1} at +X; \
         blue {:.1} against {:.1}",
        red.0, red.1, blue.0, blue.1
    );
    assert!(
        red.1 * PROBE_RATIO < red.0,
        "the -X end of the floor must carry unmistakably more red than the +X end, at the same \
         distance from the eye, on the same surface, under the same albedo and with no direct \
         light on either: {:.1} against {:.1} — the probes' linear band is not in this frame",
        red.0,
        red.1
    );
    assert!(
        blue.0 * PROBE_RATIO < blue.1,
        "and the +X end must carry unmistakably more blue than the -X end: {:.1} against {:.1} \
         — a frame that leads in red *and* in blue at the same end is one thing brightening a \
         side of the room, not two probes disagreeing about where the light comes from",
        blue.1,
        blue.0
    );
    for (side, sign) in [("-X", -1.0), ("+X", 1.0)] {
        for (name, channel) in [("red", 0), ("green", 1), ("blue", 2)] {
            let outer = band(sign * PROBE_BAND_AT, channel);
            let inner = band(sign * PROBE_FLAT_INNER_AT, channel);
            let delta = (outer - inner).abs();
            assert!(
                delta <= PROBE_FLAT_DELTA,
                "the {side} endpoint region must stay flat in {name}: its outer block measures \
                 {outer:.2} and its inner block {inner:.2}, a {delta:.2}-level change against \
                 {PROBE_FLAT_DELTA}; the probe blend has spread back across the floor"
            );
        }
    }
    for (name, channel) in [("red", 0), ("blue", 2)] {
        let minus = band(-PROBE_BAND_AT, channel);
        let centre = band(0.0, channel);
        let plus = band(PROBE_BAND_AT, channel);
        let lower = minus.min(plus);
        let upper = minus.max(plus);
        assert!(
            lower + PROBE_INTERPOLATION_DELTA < centre
                && centre + PROBE_INTERPOLATION_DELTA < upper,
            "the centre must interpolate the probe rows in {name}: {centre:.2} must sit at least \
             {PROBE_INTERPOLATION_DELTA} level(s) inside its endpoints {minus:.2} and {plus:.2}"
        );
    }
    the_shader_and_the_rust_mirror_agree_about_the_irradiance(image);
}

/// [`Scene::Probes`]' second claim, and the one this scene exists for: **the
/// device's `probe_irradiance` computes what
/// [`crcbl_shaders::probe::irradiance_at`](crcbl::shaders::probe::irradiance_at)
/// computes.**
///
/// The probe row landed as two implementations of one evaluation — a Slang
/// function and a Rust mirror of it — checked against the literature on the host
/// side and against nothing at all on the device side, because no scene had a
/// probe in it and an additive term that is everywhere zero moves no golden. This
/// is the comparison that was owed.
///
/// Every channel of both endpoint regions plus the centre, absolutely rather
/// than in proportion, because this scene is built so the whole chain between
/// the two is exact — see [`PROBE_MIRROR_LEVELS`], which is where each step of it
/// is named. The red and blue channels exercise the constant and Y-linear bands
/// in opposite directions, the green channel exercises the constant band, and
/// the centre exercises interpolation between probe rows.
///
/// The floor normal cannot observe the X/Z bands; the host-side literature tests
/// cover those lanes. This fixture's narrower contract is the device path used by
/// this room, not every coefficient a probe can hold.
///
/// **A wrong tolerance here would be the worst kind of green light**, so the
/// number is measured rather than chosen; the printed line is what a later run
/// checks it against.
fn the_shader_and_the_rust_mirror_agree_about_the_irradiance(image: &Image) {
    // **Every probe fully visible, and that is a claim rather than a
    // convenience.** The fixture's two probes stand in open air over a flat
    // floor with nothing between them and it, so the visibility capture
    // `SceneState` runs for this scene must give every corner its whole weight —
    // and if it does not, the device's frame and the plain trilinear prediction
    // part company and this comparison is what says so.
    let grid = crcbl::screenshot::probe_grid();
    let mut worst = 0.0f32;
    for (side, x) in [
        ("-X", -PROBE_BAND_AT),
        ("centre", 0.0),
        ("+X", PROBE_BAND_AT),
    ] {
        let at = probe_pixel(x, 0.0);
        for (name, channel) in [("red", 0), ("green", 1), ("blue", 2)] {
            let measured = block_channel(image, at, PROBE_BAND, channel);
            let predicted = predicted_block_channel(&grid, at, PROBE_BAND, channel);
            let miss = (measured - predicted).abs();
            worst = worst.max(miss);
            assert!(
                miss <= PROBE_MIRROR_LEVELS,
                "the {side} band's {name} channel measures {measured:.2} and the Rust mirror of \
                 `probe_irradiance` predicts {predicted:.2} for it, a miss of {miss:.2} level(s) \
                 against a budget of {PROBE_MIRROR_LEVELS} — the shader and the host disagree \
                 about the same coefficients, or this frame did not reach the swapchain through \
                 the sRGB encode this model assumes"
            );
        }
    }
    eprintln!(
        "crcbl render e2e: probes — the shader and the Rust mirror agree to {worst:.2} level(s) \
         at worst over both endpoints, the centre and all three channels"
    );
}

/// **A scene that does not move draws the same frame every time.**
///
/// Every other test in this suite draws **one** frame per device and compares it
/// against a golden or against another path's single frame. A frame that is
/// correct on its own and different from the frame before it passes all of them,
/// which is how `docs/backlog.md`'s "lantern's window reveal flickers on the mesh
/// path" reached a shipped tree: one face of a room drops out for a frame and
/// comes back, with the scene frozen, and nothing in the suite looks at two
/// frames at once.
///
/// **What it asserts is that there is no second frame.** The pose is fixed —
/// `OffscreenSetup` draws at `t = 0` every time — the scene does not animate and
/// no input arrives, so every frame after the first is drawn from inputs
/// identical to its predecessor's. The comparison is on the readback bytes with
/// no tolerance, because there is no mechanism by which a still frame may differ
/// at all.
///
/// **Frame zero is excluded, and only frame zero.** The first frame of a run
/// fills caches the others inherit: the shadow atlas is drawn rather than reused,
/// and the culling-statistics ring has nothing in it yet. Everything from frame
/// one on is steady state.
///
/// **[`FRAMES`] passes of the ring, because the hazard is between frames rather
/// than inside one.** The renderer keeps `FRAMES_IN_FLIGHT` frames of state and
/// rotates through them, so a slot disagreeing with its neighbour only shows once
/// the ring has come round several times. At the time this was written the defect
/// reproduced on 16 of 63 frames, so this many is a wide margin and still well
/// under a second of GPU time.
///
/// **The two guards in front of the comparison are what stop it passing
/// vacuously.** This test asserts frames are *equal*, and a device that drew
/// nothing satisfies that perfectly — thirty-two identical blank frames are
/// thirty-two identical frames. So the frame is first shown to hold the scene,
/// with [`MIN_COLORS_CUBE`] and the inspector the golden test uses. And the run
/// is shown to have gone through the amplification stage, because that is where
/// the defect lives: on a device without [`Features::TASK_SHADER`] this scene
/// draws through a stage that selects and culls nothing, and a green run there
/// would be reporting on code the defect is not in. Forcing the same scene onto
/// [`GeometryPath::IndirectCount`] gave 0 differing frames against the mesh
/// path's 16, which is what makes that second guard the difference between a
/// test and a coin toss.
///
/// **Cannot-run and did-not-work are different answers, and this separates
/// them.** A device with no [`Features::TASK_SHADER`] declines above and says
/// so; failing it instead would red every backend whose hardware simply has no
/// amplification stage, which is what `dx12 e2e` on WARP and `mtl e2e` on the
/// macOS runner did for a day. What stays an assertion is the case that is
/// genuinely wrong: a device that *has* the stage and still drew another path.
/// So the skip is bounded by a capability rather than by a result, and it
/// cannot quietly widen to cover a real failure.
///
/// [`Scene::Cube`] rather than the DAG scene: it is the simplest scene that
/// reproduces, and on the mesh path its geometry goes through the same
/// per-cluster stage a larger one would.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_the_same_frame_on_every_pass_of_the_ring() {
    crcbl_core::log::init_logging();

    /// Frames to draw, several passes of the frame ring.
    const FRAMES: usize = 32;

    let mesh_stage = Features::MESH_SHADER.union(Features::TASK_SHADER);
    let setup = OffscreenSetup::open_with(
        EXTENT.0,
        EXTENT.1,
        Scene::Cube,
        OffscreenSetup::OPTIONAL_FEATURES.union(mesh_stage),
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for the cube scene: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);
    let path = setup.caps().geometry_path();
    let amplifies = setup
        .adapter()
        .caps
        .features
        .contains(Features::TASK_SHADER);
    eprintln!(
        "crcbl render e2e: cube on {backend} adapter {adapter:?} — drew through {path:?}, \
         amplification stage: {amplifies}",
        backend = setup.backend(),
        adapter = setup.adapter().name,
    );

    // **A device with no amplification stage cannot exercise this, and saying so
    // is not the same as passing.** The defect lives in the per-cluster task
    // stage; without [`Features::TASK_SHADER`] this scene draws through a path
    // that selects and culls nothing, and thirty-two identical frames there
    // would be a green light wired to code the hazard is not in. `crcbl-vk` on
    // lavapipe is where CI actually runs it — `apps/quarry`'s `read_the_cut`
    // declines on the same terms and in the same words.
    if !amplifies {
        eprintln!(
            "{SUITE}: no amplification stage on this device, so there is no per-cluster ring to \
             watch — that is every device without TASK_SHADER and every non-mesh path"
        );
        setup.finish();
        return;
    }

    let format = setup.format();
    let mut frames: Vec<Vec<u8>> = Vec::with_capacity(FRAMES);
    for index in 0..FRAMES {
        let ((width, height), pixels) = setup
            .draw_and_readback()
            .unwrap_or_else(|why| panic!("cube frame {index} renders on {path:?}: {why}"));
        assert_eq!(
            (width, height),
            EXTENT,
            "frame {index} drew a different extent"
        );
        frames.push(pixels);
    }
    // Before any assertion, on `draw_scene_on_every_geometry_path`'s terms: a
    // device lost mid-run, or a frame the validation layer refused, surfaces
    // here and nowhere else.
    setup.finish();

    // **And the run went through the stage the hazard is in.**
    //
    // A device *with* the stage that still drew another path is a fault worth
    // failing on — path selection is then disagreeing with the features it was
    // opened for. A device *without* it has already been skipped above, because
    // it could not have exercised this either way.
    assert!(
        path == GeometryPath::MeshShader,
        "this device has an amplification stage but drew through {path:?} — the per-cluster \
         stage this test is about was not the one that ran"
    );

    // **The order matters.** The equality is asserted first so that a run which
    // flickers reports the flicker; the two guards after it are what stop a
    // *passing* run being vacuous, and a run that reaches them has already shown
    // every frame identical, so whichever frame they inspect is every frame.
    let unstable: Vec<(usize, usize)> = frames
        .iter()
        .enumerate()
        .skip(2)
        .map(|(index, frame)| {
            (
                index,
                frame.iter().zip(&frames[1]).filter(|(a, b)| a != b).count(),
            )
        })
        .filter(|(_, differing)| *differing > 0)
        .collect();
    assert!(
        unstable.is_empty(),
        "{} of {} still frames differ from frame 1, as (frame, differing bytes of {}): \
         {unstable:?} — the pose, the scene and the effects are identical across all of \
         them, so geometry that comes and goes between passes of the ring is what this \
         reads as",
        unstable.len(),
        FRAMES - 2,
        frames[1].len(),
    );

    // **The frame holds the scene**, asserted on the very frame the equality
    // below rests on rather than on some other one.
    let baseline = Image::from_readback(EXTENT.0, EXTENT.1, &frames[1], channel_order(format))
        .expect("the readback is exactly one image");
    let colors = baseline.distinct_colors(MIN_COLORS_CUBE);
    assert!(
        colors >= MIN_COLORS_CUBE,
        "a cube frame with {colors} distinct colour(s) (counted to {MIN_COLORS_CUBE}) is not \
         evidence — blank frames are identical to one another, and this test would pass on \
         them"
    );
    the_cube_scene_drew_its_geometry_and_every_material_column(&baseline);
}

#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_dunes_scene_draws_its_cluster_dag_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Dunes,
        "dunes",
        EXTENT,
        MIN_COLORS_DUNES,
        the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone,
    );
}

/// [`Scene::Dunes`]'s anti-vacuity claim: lit ground below the horizon, clear
/// sky above it, and shading that varies down the patch.
///
/// The three together are what a frame drawn from an empty bucket fails. A
/// selection that resolved to a level with no geometry leaves the whole frame at
/// the clear, which the first two catch; one that drew a single flat quad — or
/// the patch collapsed to its coarsest level with the camera on top of it —
/// passes those and fails the third, because the dunes are a height field and a
/// lit height field is not one colour.
fn the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone(image: &Image) {
    // The camera looks along the patch from four units up, so the horizon sits
    // near the middle of the frame: a row a fifth of the way down is sky and one
    // four fifths down is ground, whichever level was selected.
    let sky = image.pixel(EXTENT.0 / 2, EXTENT.1 / 5).expect("inside");
    let ground = image.pixel(EXTENT.0 / 2, EXTENT.1 * 4 / 5).expect("inside");
    let clear = image.pixel(1, 1).expect("inside");
    assert!(
        !differ(sky, clear),
        "the sky at the top of the frame is {sky:?} against a clear of {clear:?} — the \
         patch is not where this camera puts it"
    );
    assert!(
        differ(ground, clear),
        "the ground at the bottom of the frame is still the clear {clear:?} (got \
         {ground:?}) — the selected level drew nothing"
    );
    // Down the middle of the patch, below the horizon. A height field lit by one
    // directional light shades every slope differently; a flat quad does not.
    let shades = distinct_colors_in(
        image,
        EXTENT.0 / 2 - 8..EXTENT.0 / 2 + 8,
        EXTENT.1 * 3 / 5..EXTENT.1,
    );
    eprintln!("crcbl render e2e: dunes — {shades} shade(s) down the patch's middle");
    assert!(
        shades >= 8,
        "the patch's middle holds {shades} distinct colour(s), which is a flat surface \
         rather than a lit height field"
    );
}

/// Four sprites over three batches and two sheets — `sprite.slang` — on the
/// backend `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// This is the scene `SV_InstanceID` broke: the third batch starts at instance
/// 3, and a backend that reads the wrong lowering draws the last rectangle in
/// the first's place. [`every_sprite_slot_is_painted_and_the_gaps_are_not`] is
/// what makes that a failure here rather than only in the golden's numbers.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_through_the_sprite_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Sprite,
        "sprite",
        EXTENT,
        MIN_COLORS_SPRITE,
        every_sprite_slot_is_painted_and_the_gaps_are_not,
    );
}

/// Rectangles, an outline and glyph-atlas text — `ui.slang` — on the backend
/// `CRCBL_GPU` names, against the reference in `tests/golden/`.
///
/// See [`the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds`] for
/// this scene's anti-vacuity claim, which is a blend rather than a lit centre:
/// the UI pass loads its target rather than clearing it, so "it composited"
/// is the thing worth asserting.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_through_the_ui_renderer_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ui,
        "ui",
        EXTENT,
        MIN_COLORS_UI,
        the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds,
    );
}

/// The inspector the [`EXTENT_ODD`] tests pass, and it checks nothing.
///
/// **This is where the odd-size tests are weaker than their [`EXTENT`]
/// counterparts, stated rather than papered over.** Every per-scene inspector in
/// this file computes its sample points from [`EXTENT`] — fractions of it, or
/// pixel constants read off a frame of that size — because each was measured
/// against that frame. At [`EXTENT_ODD`] those coordinates address different
/// parts of a differently-proportioned picture, so running one here would assert
/// something nobody measured: it would pass or fail on where the arithmetic
/// happened to land, which is not evidence either way.
///
/// What keeps these tests from being vacuous is the other half of the
/// anti-vacuity pair — the `min_colors` floor, which is a property of the frame
/// rather than of any coordinate in it and so carries across sizes unchanged —
/// together with the golden, which is the whole picture at this extent. The
/// claim they drop is *where* the scene drew; the claims they keep are that it
/// drew and that it drew what was reviewed.
fn nothing_measured_at_this_extent(_image: &Image) {}

/// The cube at [`EXTENT_ODD`] — the row-pitch case, on `mesh.slang`.
///
/// The three tests here are what the vk-against-wgpu gate covered by rendering
/// every scene at a second, deliberately awkward size. Same scenes,
/// same floors, against goldens blessed at [`EXTENT_ODD`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_at_an_odd_extent_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Cube,
        "cube_97x61",
        EXTENT_ODD,
        MIN_COLORS_CUBE,
        nothing_measured_at_this_extent,
    );
}

/// The sprite scene at [`EXTENT_ODD`] — the row-pitch case, on `sprite.slang`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_at_an_odd_extent_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Sprite,
        "sprite_97x61",
        EXTENT_ODD,
        MIN_COLORS_SPRITE,
        nothing_measured_at_this_extent,
    );
}

/// The UI scene at [`EXTENT_ODD`] — the row-pitch case, on `ui.slang`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_at_an_odd_extent_and_matches_its_golden() {
    draw_scene_and_match_its_golden(
        Scene::Ui,
        "ui_97x61",
        EXTENT_ODD,
        MIN_COLORS_UI,
        nothing_measured_at_this_extent,
    );
}

/// Draws one frame of `scene` at `extent` and compares it against
/// `tests/golden/{golden}.png`.
///
/// **What the assertions are for, in the order a failure would hit them:**
///
/// 1. The backend that opened is the one that was asked for. Every backend
///    draws these scenes identically by construction, so a Metal run that fell
///    back to wgpu would produce a passing frame and prove nothing about Metal.
/// 2. The adapter it opened is the class
///    [`ADAPTER_ENV_VAR`](crcbl::adapter::ADAPTER_ENV_VAR) named, and both the
///    adapter and the pin are printed. The same argument one layer down: this
///    frame died on `windows-latest` with `DXGI_ERROR_DEVICE_REMOVED` because
///    the first enumerated adapter is not a usable device there.
/// 3. The device's [`GeometryPath`](crcbl::hal::GeometryPath) is reported, and
///    the frame is drawn through whichever indirect tail it selects. Metal
///    reports no `DRAW_INDIRECT_COUNT` — the flag is absent from the API rather
///    than unimplemented — so it selects
///    [`IndirectPerBatch`](crcbl::hal::GeometryPath::IndirectPerBatch), the arm
///    that until now had only ever run on Vulkan behind a forced selector.
/// 4. Something drew: at least `min_colors` colours, plus `inspect`'s
///    scene-specific claim about *where* it drew. A full-screen quad and a blank
///    frame both fail these and neither is distinguishable by a tolerance.
/// 5. The picture is the one that was reviewed.
///
/// Both halves of 4 are per scene, because the scenes are not equally colourful
/// and they are not the same shape: the cube has a lit centre against a dark
/// clear, the sprite scene has four rectangles with gaps between them, and the
/// UI frame is a composite over a background it never clears.
fn draw_scene_and_match_its_golden(
    scene: Scene,
    golden: &str,
    extent: (u32, u32),
    min_colors: usize,
    inspect: fn(&Image),
) {
    // **Install a logger before opening anything.** Without one, every
    // `log::info!` a backend emits on the way to a device — the adapter it
    // chose, the surface it built, whether a validation layer loaded — goes
    // nowhere. Two sessions were spent diagnosing a D3D12 failure inside
    // `open` with no backend output at all in the run, for exactly this
    // reason: the panic message was the only evidence, and it named the call
    // that noticed rather than the one that caused it.
    crcbl_core::log::init_logging();

    // `unwrap_or_else` rather than `expect`, which would format the error with
    // `Debug` and escape the newlines out of the adapter listing a pin miss
    // carries — on a runner nobody can log into, that listing is the whole
    // diagnosis.
    let setup = OffscreenSetup::open(extent.0, extent.1, scene)
        .unwrap_or_else(|why| panic!("a GPU backend opens for the {golden} scene: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);

    let backend = setup.backend();
    let caps = setup.caps();
    // Printed unconditionally, and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "crcbl render e2e: {backend} selected {:?} / {:?} / {:?} for the {golden} scene",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );

    // The adapter line, and the raw pin beside it. `run-render-e2e.sh` matches
    // the pin back against what it exported, because the one failure this test
    // cannot see is the variable never reaching this process: the pin would be
    // `None` here, `select` would take the first adapter, and every assertion
    // below would pass on a device nobody asked for.
    let adapter = setup.adapter();
    let requested_adapter = crcbl::adapter::pin();
    eprintln!(
        "crcbl render e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = requested_adapter.as_deref().unwrap_or("<unset>"),
    );

    // **The device took the best path its adapter offers.** The frame alone
    // cannot say — every path draws this scene identically, which is what
    // `the_*_scene_draws_the_same_frame_on_every_geometry_path` checks — so a
    // request that omitted a selector's flag would leave the renderer on a
    // lesser tail and every assertion below would still pass. That is not
    // hypothetical: `Features::MESH_SHADER` is not part of `GPU_DRIVEN`, so
    // until `OffscreenSetup::OPTIONAL_FEATURES` named it, an adapter reporting
    // mesh shaders drew this frame through `IndirectCount` and nothing said so.
    assert_eq!(
        caps.geometry_path(),
        adapter.caps.geometry_path(),
        "adapter {} offers {:?} and the device opened on {:?} — a selector's flag \
         was not asked for, so the frame took a lesser tail than this machine can run",
        adapter.name,
        adapter.caps.geometry_path(),
        caps.geometry_path(),
    );

    // A pin the loader ignored is the failure this catches, and it is the same
    // class as a suite that runs no tests. Both names go through the mappings
    // that already exist rather than a third table.
    if let Ok(requested) = std::env::var(BACKEND_ENV_VAR) {
        let opened = GpuBackend::from_name(&backend.to_string())
            .expect("every backend the registry can open has a GpuBackend spelling");
        assert_eq!(
            Some(opened),
            GpuBackend::from_name(&requested),
            "{BACKEND_ENV_VAR}={requested} was asked for and {backend} drew the frame"
        );
    }
    // Same shape one layer down: `select` refuses a class it cannot find, so
    // this can only fire if it resolved to something else — but it is the
    // assertion that makes the pin's arrival observable rather than assumed.
    if let Some(requested) = requested_adapter.as_deref() {
        let want = device_type_from_name(requested)
            .unwrap_or_else(|| panic!("{ADAPTER_ENV_VAR}={requested} is not a device class"));
        assert_eq!(
            adapter.device_type, want,
            "{ADAPTER_ENV_VAR}={requested} was asked for and adapter {} ({:?}) drew the frame",
            adapter.name, adapter.device_type
        );
    }

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle and asks the device
    // what it saw, and a device lost during the frame — or a frame the
    // validation layer refused — surfaces there and nowhere else. A run that
    // panicked on the pixels first would report a wrong picture where the real
    // answer is that the GPU never legally drew it.
    setup.finish();

    assert_eq!(
        (width, height),
        extent,
        "the swapchain handed back {width}x{height} for a frame asked for at {}x{} — an extent \
         the {golden} golden was not blessed at",
        extent.0,
        extent.1
    );
    let image = Image::from_readback(width, height, &pixels, channel_order(format))
        .expect("the readback is exactly one image");

    let colors = image.distinct_colors(min_colors);
    assert!(
        colors >= min_colors,
        "a {golden} frame with {colors} distinct colour(s) (counted to {min_colors}) is not \
         evidence — nothing drew, or only the clear did"
    );
    inspect(&image);

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{golden}.png"));
    let golden_image = Golden::new(reference);
    let comparison = golden_image
        .check(&image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    eprintln!(
        "crcbl render e2e: golden {golden} on {backend} — {}",
        comparison.summary()
    );
}

/// The cube on both geometry paths this machine can reach — see
/// [`draw_scene_on_every_geometry_path`].
///
/// **This is the scene the comparison is about.** [`Scene::Cube`] is the only one
/// of the three drawn by `crcbl-render`'s `ForwardRenderer`, and that renderer is
/// the only thing above the seam that branches on
/// [`GeometryPath`](crcbl::hal::GeometryPath): a mesh device records
/// `draw_mesh_tasks` against a mesh pipeline with no vertex stage and no index
/// buffer, where the others record an indirect call reading the same pool through
/// a vertex stage. Three instances, a material table and a base-colour page, none
/// of which `crcbl-vk`'s own `mesh.png` scene has.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_cube_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Cube,
        "cube",
        MIN_COLORS_CUBE,
        the_cube_scene_drew_its_geometry_and_every_material_column,
    );
}

/// The dunes patch on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`].
///
/// **This is the two granularities of `docs/plan/25-lod.md` compared in
/// pixels.** The mesh arm descends the cluster DAG per cluster in its
/// amplification stage and the lesser arm takes a uniform cut per instance in
/// the cull pass; both are asserted here to produce the *same frame*, byte for
/// byte.
///
/// That is a real claim rather than a tautology, and it is one this scene's
/// camera and budget make available: at a one-pixel budget from two units off
/// the patch's near edge every group is expanded, so the per-cluster cut is the
/// whole of level 0 and the uniform cut's level is 0 — the same triangles by two
/// entirely different routes, one through an index pool and a vertex stage and
/// one through cluster records and a mesh stage.
///
/// **It is also why this is a pixel comparison and not the comparison.** Move
/// the camera back or raise the budget and the two paths diverge *by design*:
/// the per-cluster cut mixes levels across the surface where the uniform one
/// cannot, and the frames stop being equal. What stays true at every camera is
/// that the uniform cut's level is the finest level the per-cluster cut draws,
/// and that is asserted in numbers by `crcbl-shaders`'
/// `the_uniform_level_is_the_finest_level_the_per_cluster_cut_draws` and on real
/// devices by `crcbl-vk`'s
/// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_dunes_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Dunes,
        "dunes",
        MIN_COLORS_DUNES,
        the_dunes_patch_fills_the_lower_frame_and_leaves_the_sky_alone,
    );
}

/// The sprite scene on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`].
///
/// `crcbl-render`'s sprite pass reads no
/// [`GeometryPath`](crcbl::hal::GeometryPath), so the two arms here differ only in
/// whether the *device* was opened with mesh shading enabled. That is the claim
/// worth having and it is not the cube's: enabling an extension changes a Vulkan
/// device's pipeline cache, its enabled feature struct and, on some drivers, its
/// shader compiler — and nothing else in this tree would notice if that moved a
/// pass that never asked for it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_sprite_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Sprite,
        "sprite",
        MIN_COLORS_SPRITE,
        every_sprite_slot_is_painted_and_the_gaps_are_not,
    );
}

/// The UI scene on both geometry paths — see
/// [`draw_scene_on_every_geometry_path`], and
/// [`the_sprite_scene_draws_the_same_frame_on_every_geometry_path`] for what an
/// arm proves about a pass that reads no geometry path.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_ui_scene_draws_the_same_frame_on_every_geometry_path() {
    draw_scene_on_every_geometry_path(
        Scene::Ui,
        "ui",
        MIN_COLORS_UI,
        the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds,
    );
}

/// Draws `scene` twice — once on the best [`GeometryPath`] this adapter reports
/// and once on the path it selects without [`Features::MESH_SHADER`] — and
/// asserts the two frames are **byte for byte the same picture**.
///
/// # Why this has to exist before the mesh path is selected by default
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5 calls the mesh shader the primary
/// geometry path and its design rule is that "the lesser path is a constraint on
/// data layout, not a separate renderer". `crcbl-vk`'s
/// `every_geometry_path_draws_the_same_frame` already checks that on the vk
/// suite's own scene: one mesh, one pyramid, no material table. It says nothing
/// about the scene this crate's goldens are blessed on, which has three instances
/// across two buckets, a material table and a sampled base-colour page — every
/// piece of per-draw data the mesh stage has to fetch for itself because there is
/// no input assembler to hand it one.
///
/// So this is the check that lets `OffscreenSetup::OPTIONAL_FEATURES` ask for
/// mesh shading at all. A second, per-path golden would have hidden exactly what
/// it is for: a difference here is a **bug in the mesh path**, not two legitimate
/// pictures.
///
/// # The arms are asked for by subtraction
///
/// An adapter reports what it reports, so the only way one machine reaches more
/// than one path is to open a device *without* the flag that selects the better
/// one — `crcbl-vk`'s `Headless::open_for_mesh_with` and this crate's
/// [`OffscreenSetup::open_with`] exist for that and nothing else.
///
/// # On a device with no mesh shaders
///
/// Both arms land on the same path, and the test says so rather than passing
/// quietly: the arms are asserted to differ **exactly when the adapter reports
/// the flag**, so "this backend has no mesh shaders" is a checked claim instead
/// of a skip. `crcbl-webgpu`, `crcbl-dx12` and `crcbl-mtl` all report none — see
/// each backend's `caps` — so on those three this is a self-comparison and the
/// printed line is what says so.
fn draw_scene_on_every_geometry_path(
    scene: Scene,
    name: &str,
    min_colors: usize,
    inspect: fn(&Image),
) {
    crcbl_core::log::init_logging();

    let mut frames: Vec<(GeometryPath, Image)> = Vec::new();
    let mut adapter_offers_mesh = None;

    // **Both mesh-stage flags come out of the lesser arm, not just the one that
    // names the path.** `Features::TASK_SHADER` is an amplification stage in
    // front of a mesh stage, so a backend asked for it enables the mesh stage
    // too — and a device that ended up on `MeshShader` after `MESH_SHADER` was
    // subtracted is a self-comparison wearing a cross-path label, which is the
    // failure the assertion below reports.
    let mesh_stage = Features::MESH_SHADER.union(Features::TASK_SHADER);
    for optional in [
        OffscreenSetup::OPTIONAL_FEATURES.union(mesh_stage),
        OffscreenSetup::OPTIONAL_FEATURES.difference(mesh_stage),
    ] {
        let setup = OffscreenSetup::open_with(EXTENT.0, EXTENT.1, scene, optional)
            .unwrap_or_else(|why| panic!("a GPU backend opens for the {name} scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let path = setup.caps().geometry_path();
        let offers_mesh = setup
            .adapter()
            .caps
            .features
            .contains(Features::MESH_SHADER);
        eprintln!(
            "crcbl render e2e: {name} on {backend} adapter {adapter:?} — asked for \
             MESH_SHADER: {asked}, adapter has it: {offers_mesh}, drew through {path:?}",
            backend = setup.backend(),
            adapter = setup.adapter().name,
            asked = optional.contains(Features::MESH_SHADER),
        );
        assert_eq!(
            *adapter_offers_mesh.get_or_insert(offers_mesh),
            offers_mesh,
            "the two arms opened different adapters, so they are not a comparison"
        );

        let format = setup.format();
        let ((width, height), pixels) = setup
            .draw_and_readback()
            .unwrap_or_else(|why| panic!("the {name} frame renders on {path:?}: {why}"));
        setup.finish();

        assert_eq!((width, height), EXTENT, "{path:?} drew a different extent");
        frames.push((
            path,
            Image::from_readback(width, height, &pixels, channel_order(format))
                .expect("the readback is exactly one image"),
        ));
    }

    let adapter_offers_mesh = adapter_offers_mesh.expect("both arms opened a device");
    let (best_path, best) = &frames[0];
    let (lesser_path, lesser) = &frames[1];

    // **The anti-vacuity claim, and it is two claims.** Two blank frames match
    // perfectly, so the frame has to hold the scene before comparing it means
    // anything — that is `min_colors` and `inspect`, the same pair the golden
    // test uses. And two frames drawn by the same code match perfectly too, so
    // the paths have to actually differ: derived from the adapter rather than
    // written down, because a backend reporting no mesh shaders is a legitimate
    // run of this test and a silent one is not.
    let colors = best.distinct_colors(min_colors);
    assert!(
        colors >= min_colors,
        "a {name} frame with {colors} distinct colour(s) (counted to {min_colors}) is not \
         evidence — comparing it against another frame like it proves nothing"
    );
    inspect(best);
    assert_eq!(
        best_path != lesser_path,
        adapter_offers_mesh,
        "the adapter {} mesh shading and the two arms selected {best_path:?} and \
         {lesser_path:?} — one of those two facts is wrong, and a self-comparison \
         that reads as a cross-path one is worse than no test",
        if adapter_offers_mesh {
            "reports"
        } else {
            "reports no"
        },
    );

    // **Byte equality, except on the one scene that earns a budget.** This is one
    // adapter and one driver drawing one scene, so every differing pixel is a
    // difference the two submissions made, and the tolerance a *different*
    // rasteriser is allowed would hide exactly that. See [`path_lsb_channels`]
    // for why `Dunes` alone is not held to zero.
    let (budget, worst_allowed) = path_lsb_channels(scene);
    let (differing, worst, named) = channels_differing(best, lesser);
    let where_ = if named.is_empty() {
        "none".to_string()
    } else {
        named.join("; ")
    };
    eprintln!(
        "crcbl render e2e: {name} on {best_path:?} against {lesser_path:?} — {differing} \
         channel(s) differ, worst by {worst}, budget {budget} channel(s) at {worst_allowed} \
         level(s) ({colors} distinct colour(s)); the first are {where_}"
    );
    assert!(
        worst <= worst_allowed && differing <= budget,
        "{best_path:?} and {lesser_path:?} draw the {name} scene differently: {differing} \
         channel(s) differ, the worst by {worst} — this scene allows at most {budget} \
         channel(s) and only ever by {worst_allowed}, and this is a different picture. The \
         first are {where_}"
    );
}

/// How many channels the two geometry paths may disagree about on `scene`, and
/// even then only ever by one.
///
/// **Zero for every scene but the four that march, and that is the point.** The
/// two paths are meant to draw the same picture, so a budget handed to every
/// scene would be slack nobody measured — the exact comparison is what has
/// caught a path drawing something else, and it keeps its teeth everywhere it
/// still holds. Verified rather than assumed: radv and wgpu answer zero on every
/// scene, and llvmpipe answers zero on every scene but the ones named below.
///
/// `Dunes` is the exception because its two arms **deliberately draw different
/// geometry** — see `crcbl-vk`'s
/// `the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is`: the uniform
/// cut is the per-cluster cut's floor, so the mesh arm draws finer clusters. Its
/// byte equality was therefore always the luckier kind, and `mesh.slang`'s move
/// from a Blinn lobe to a GGX one is what ran the luck out: a sharper highlight
/// turns a last-bit difference in a world position into a last-bit difference in
/// a pixel, where a broad one absorbed it. Measured at **one** channel, off by
/// one, out of the frame's 196608, on llvmpipe alone.
///
/// `PointShadow` is the second exception, and it arrived with
/// `docs/plan/18-render-features.md`'s reflection march. That scene's caster
/// carries the tinted material row, whose roughness is the only one in the demo
/// scene under `ssr.slang`'s cutoff — so it is the one object in the suite whose
/// pixels are decided by a *march over the depth buffer* rather than by shading
/// the fragment the rasteriser handed over. **That makes the depth buffer's last
/// bits visible in the picture for the first time**: a ray whose origin differs
/// in the last place taps a neighbouring pixel at the crossing, which the design
/// says in as many words is the exposure a march has and a blurred term does not.
/// Measured at **one** channel, off by one, out of the frame's 196608, on
/// llvmpipe alone — radv and wgpu answer zero — and stable across repeated runs.
/// The blur that followed the march did not change it, which is the answer to
/// the obvious question: a sixteen-tap denominator makes a disagreement smaller
/// and does not make the tap the ray landed on the same one.
///
/// `Ssr` is the third, and it is the same exposure as `PointShadow` on the scene
/// built to have it: every pixel of the reflected band is decided by where a
/// march over the depth buffer crossed, so a world position differing in the
/// last place taps the neighbouring pixel at the crossing. It read zero until
/// 2026-08-27 and that was the luckier kind of equality, the way `Dunes`' was
/// before the GGX move — `mesh.slang` gaining the emissive add is what ran the
/// luck out, by moving nothing in the picture and enough in llvmpipe's codegen.
/// Measured at **two** channels, each off by one, out of the frame's 196608, and
/// on one llvmpipe alone: the Ubuntu runner's Mesa 25.2.8 / LLVM 20.1.2 build.
/// Arch's Mesa 26.2.1 / LLVM 22.1.8 answers zero at the same 256-bit vector
/// width, the Windows lavapipe job answers zero, and radv and wgpu answer zero.
///
/// `Probes` is the fourth, and it is the ambient occlusion pass that marches
/// rather than a reflection ray. `ssao.slang`'s horizon integral takes a **max**
/// over sixteen depth taps per pixel, so a depth differing in the last place can
/// flip which tap wins a horizon and move the angle by a step — the same
/// exposure the three above have, arriving through the term this scene is built
/// around: occlusion scales the ambient, and this scene's ambient is the probe
/// irradiance under test. It read zero until 2026-08-28 and the hemisphere it
/// replaced is what kept it there — a threshold count of eight samples rounds a
/// last-bit disagreement away where a max does not. Measured at **two**
/// channels, each off by one, out of the frame's 196608: the red channel of
/// `(137, 17)` and of `(138, 17)`, adjacent, `150` against `149`. On llvmpipe
/// alone — radv answers zero — and the same two channels on the Ubuntu runner's
/// Mesa 25.2.8 / LLVM 20.1.2 and on Arch's Mesa 26.2.1 / LLVM 22.1.8, which is
/// the first of these four to reproduce off the runner.
///
/// `Ao` is the fifth, and it is `Probes`' mechanism on the scene built around
/// that pass: the horizon integral's **max** over depth taps. It read zero
/// until 2026-08-30, when `ssao.slang` gained `STEP_OFFSETS` — a per-pixel
/// phase for the march, so that sixteen pixels of a tile tap sixteen distances
/// instead of one. Fifteen new tap distances per tile are fifteen more places
/// a last-bit depth disagreement can sit on the edge that decides which tap
/// wins a horizon. Measured at **one** channel, off by one, out of the frame's
/// 196608, on llvmpipe alone and reproducibly: radv answers zero, and forcing
/// every offset back to `1.0` — the old march — takes llvmpipe to zero too,
/// which is what says the offsets are the cause and not the two geometry paths.
///
/// All five budgets are two orders of magnitude under anything a level that
/// failed to draw would produce — the failure this exists for moves whole
/// clusters, not one channel.
///
/// # The magnitude, and the one scene that needed more than a level
///
/// The second half of the bound used to be a flat `worst <= 1` for every scene:
/// a reflection that landed on the wrong surface moves a channel by far more
/// than one, so it fails whatever the count. That half is **still one level
/// everywhere but `Probes`**, and is returned per scene rather than written into
/// the assertion so that widening it stays a scene's decision with a measurement
/// attached, the way the counts above already are.
///
/// `Probes` is two, since `docs/plan/46-ambient-occlusion.md`'s bent-normal rung
/// landed on 2026-09-02. Its entry above says a last-bit depth difference can
/// flip which tap wins a horizon; until that rung the flip could only move the
/// occlusion **scalar**, which scales the ambient and so moves a channel by a
/// fraction of it. The bent direction is downstream of the same max and the
/// ambient is now sampled *along* it, so a flipped tap turns the probe lookup by
/// a step instead of dimming it — a first-order change where the scalar's was
/// second-order. Measured at **9 channels, worst by 2**, out of the frame's
/// 196608: identical on the Ubuntu runner's Mesa 25.2.8 / LLVM 20.1.2 and on
/// Arch's Mesa 26.2.1 / LLVM 22.1.8, and radv answers zero as it does for all
/// five.
///
/// **The sensitive pixels moved rather than grew, which is worth stating
/// because the obvious guess is wrong.** The pre-rung pair above is at
/// `(137, 17)` and `(138, 17)`, and neither of those channels differs now. What
/// differs instead is a contiguous cluster along the top edge — `(131, 3)`
/// through `(139, 1)`, in blue and red — measured on Arch's Mesa 26.2.1; both
/// two-level channels are blue, `(132, 1)` reading 15 against 13 and `(132, 2)`
/// reading 55 against 53. Adjacent pixels in one neighbourhood is what a single
/// flipped tap looks like, and it is the evidence for the mechanism above; a
/// scatter across the frame would have been evidence against it.
///
/// **This is a guard that got weaker, and it is recorded as one rather than
/// absorbed.** Two levels out of 256 on nine channels is still three orders of
/// magnitude under a cluster that failed to draw, so the teeth this exists for
/// are intact; what is gone is the ability of *this* scene to catch a
/// one-to-two-level regression in the probe term.
const fn path_lsb_channels(scene: Scene) -> (usize, u8) {
    match scene {
        Scene::Dunes => (16, 1),
        Scene::PointShadow => (16, 1),
        Scene::Ssr => (16, 1),
        Scene::Probes => (16, 2),
        Scene::Ao => (16, 1),
        _ => (0, 1),
    }
}

/// How many channels of `left` and `right` differ, by how much at worst, and
/// where the first [`DIFFERING_CHANNELS_NAMED`] of them are.
///
/// **The coordinates exist because a count is not a diagnosis.**
/// [`path_lsb_channels`] argues each of its budgets from a mechanism, and the
/// evidence for that argument is *which* channels moved — adjacent pixels of one
/// silhouette read as one tap flipping, while a scatter across the frame reads
/// as something else entirely. Two of those entries name their pixels because
/// somebody went and found them by hand; this returns them, so the next one does
/// not have to.
fn channels_differing(left: &Image, right: &Image) -> (usize, u8, Vec<String>) {
    let width = left.width() as usize;
    let mut count = 0usize;
    let mut worst = 0u8;
    let mut named = Vec::new();
    for (index, (one, other)) in left.pixels().iter().zip(right.pixels()).enumerate() {
        if one == other {
            continue;
        }
        count += 1;
        worst = worst.max(one.abs_diff(*other));
        if named.len() < DIFFERING_CHANNELS_NAMED {
            let pixel = index / 4;
            let channel = ["r", "g", "b", "a"][index % 4];
            let (x, y) = (pixel % width, pixel / width);
            named.push(format!("{channel} of ({x}, {y}) {one} against {other}"));
        }
    }
    (count, worst, named)
}

/// How many differing channels [`channels_differing`] names before it stops.
///
/// A bound rather than the whole list: a genuine break moves thousands, and a
/// failure message that prints thousands is one nobody reads. Every budget in
/// [`path_lsb_channels`] is under this, so a run inside its budget names all of
/// them and the ones this is written for are exactly the runs that fit.
const DIFFERING_CHANNELS_NAMED: usize = 24;

/// [`Scene::Cube`]'s anti-vacuity claim: a corner still holding the clear, and a
/// centre that is not it.
///
/// The bounds are absolute rather than relative to the corner because this
/// scene's clear is near-black and its cube is lit — "dark here, bright there"
/// is the claim, and it is the one a full-screen quad and a blank frame both
/// fail.
fn the_cube_is_lit_against_an_unpainted_corner(image: &Image) {
    let corner = image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = image.pixel(EXTENT.0 / 2, EXTENT.1 / 2).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );
}

/// The left-hand column of [`Scene::Cube`], in pixels: wide enough for a
/// pyramid, narrow enough to exclude the cube.
///
/// The pyramids sit at world `x = -1.05` spanning `±0.4`, which at this frame's
/// 4:3 aspect is `-0.94 ..= -0.42` in NDC — pixels 8 to 74. The cube starts at
/// NDC `-0.325`, pixel 86. Eighty is between them, so this band is the pyramids
/// and the clear behind them and nothing else.
const PYRAMID_BAND: std::ops::Range<u32> = 0..80;

/// **The texture column of the material row, read off the frame.**
///
/// `Scene::Cube`'s left column holds two instances of one mesh at one
/// orientation whose materials differ in nothing but their base-colour page
/// layer: the upper names the page's white layer, the lower a layer of four
/// unequal texels. So the upper pyramid's faces are flat — one lit colour each,
/// because the mesh has flat normals — and the lower one's are quartered.
///
/// Counting distinct colours is what tells those apart without knowing which
/// texel landed where. It is also the assertion that fails if the *UV* is
/// missing rather than the index: a fragment stage handed a constant texture
/// coordinate samples one texel over a whole face and produces a flat pyramid
/// in a different shade, which passes "the two differ" and fails this.
fn the_textured_pyramid_is_quartered_and_the_plain_one_is_flat(image: &Image) {
    let half = EXTENT.1 / 2;
    let plain = distinct_colors_in(image, PYRAMID_BAND, 0..half);
    let textured = distinct_colors_in(image, PYRAMID_BAND, half..EXTENT.1);
    eprintln!(
        "crcbl render e2e: pyramid column — {plain} colour(s) above, {textured} below \
         (the lower one samples a four-texel layer)"
    );
    // Each visible face of the plain pyramid is one flat colour; each face of
    // the textured one is up to four. Three faces are visible from this camera,
    // so the gap is several colours wide and not one.
    assert!(
        textured >= plain + 4,
        "the lower pyramid samples a four-texel layer and the upper one a flat white layer, \
         so it must hold several more distinct colours: {textured} below vs {plain} above"
    );
}

/// The frame row [`Scene::Cube`]'s two top pyramids both show their `+Z` face
/// on, and how far the four measured blocks reach either way.
///
/// Both faces run unbroken from row 56 to row 79 at every column the blocks
/// touch — the base's silhouette starts at 80 — so a block thirteen rows tall
/// centred here sits inside the face with several rows of margin above and
/// below. Wide enough for the same reason: a dozen columns average out a
/// rasteriser's edge pixels without either block reaching the other.
const PYRAMID_FACE_AT: u32 = 70;

/// The half-extents of each of those blocks.
const PYRAMID_FACE_BAND: (u32, u32) = (6, 6);

/// The **inner** end of the smooth pyramid's `+Z` face: the column nearest the
/// frame's centre, where the sun's mirror direction lands.
///
/// `DirectionalLight::default`'s sun comes from `+X`, and this is the pyramid at
/// `+X` — so of everything in this frame it is that face's inner edge that sits
/// at the reflection angle, and it is the only surface here a specular lobe
/// peaks on at all. See `crcbl_render`'s `PYRAMID_ROUGHNESS`.
const SMOOTH_HIGHLIGHT_AT: (u32, u32) = (208, PYRAMID_FACE_AT);

/// The **outer** end of the same face, a fifth of the frame further out.
///
/// Same face, same flat normal, same albedo and the same sun, so the only thing
/// that differs between this block and [`SMOOTH_HIGHLIGHT_AT`] is how far the
/// half-vector has swung off the mirror direction — which is exactly the axis a
/// lobe's width is measured along.
const SMOOTH_TAIL_AT: (u32, u32) = (248, PYRAMID_FACE_AT);

/// [`SMOOTH_HIGHLIGHT_AT`] mirrored onto the rough pyramid, which stands the
/// same distance the other side of the frame's centre.
const ROUGH_HIGHLIGHT_AT: (u32, u32) = (EXTENT.0 - SMOOTH_HIGHLIGHT_AT.0, PYRAMID_FACE_AT);

/// And [`SMOOTH_TAIL_AT`] mirrored the same way.
const ROUGH_TAIL_AT: (u32, u32) = (EXTENT.0 - SMOOTH_TAIL_AT.0, PYRAMID_FACE_AT);

/// How much more the smooth material's face must fall off across its width than
/// the rough one's.
///
/// **This number is the control, not a threshold on a picture.** Set both rows
/// to one roughness and the two faces measure 1.06 and 1.00 — the smooth
/// pyramid's small lead there is geometry, because its face is the one at the
/// mirror angle, and 1.2 sits above it. With the rows as the renderer writes
/// them the same measurement is 1.36 against 1.00. So the gap this refuses is
/// the one where nothing read the column, and the margin is about an eighth on
/// either side of the line.
const HIGHLIGHT_FALLOFF_RATIO: f32 = 1.20;

/// How far above the clear each of the four blocks must measure.
///
/// A frame that drew no pyramid measures the clear in all four, and a ratio
/// between two equal numbers satisfies any relation asked of it. The clear is
/// about 37 on this scale and the dimmest of the four blocks about 187, so this
/// sits between them with room either side.
const PYRAMID_FACE_LIT_FLOOR: f32 = 100.0;

/// **The shading column of the material row, read off the frame: a smooth
/// material's highlight falls off across a face and a rough one's does not.**
///
/// `Scene::Cube`'s two top pyramids are the same mesh at the same orientation
/// under the same sun, one either side of the frame's centre, and their rows
/// differ in a base-colour factor and in `crcbl_render`'s `PYRAMID_ROUGHNESS`.
/// Each turns the same `+Z` face at the camera, and the sun's mirror direction
/// lands on the inner edge of the right-hand one — so that face crosses the
/// specular lobe from its peak to its flank while the left-hand face sits well
/// off it.
///
/// The measurement is therefore a **falloff across one face**, taken on both
/// pyramids: inner block over outer block, on the same surface, with the
/// diffuse term constant across it because the normal and the light direction
/// are. A tight lobe leaves the inner end far brighter than the outer; a broad
/// one covers the whole face and leaves it flat.
///
/// **The rough pyramid is the control and it is what makes this a claim about
/// the material.** Its face is off the mirror direction, so its falloff is
/// ~1.00 whatever roughness it carries — and the smooth face's falloff is
/// ~1.06 when it carries the *same* roughness, because that much of the lead is
/// the geometry. [`HIGHLIGHT_FALLOFF_RATIO`] is set above that and below the
/// 1.36 the two rows actually produce, so the assertion is red on a shader that
/// ignored `GpuMaterial::roughness` and green on one that read it.
///
/// **No golden could make this claim.** A lobe of the wrong width draws a
/// perfectly plausible picture — that is what the Blinn constant this replaced
/// was — and the reference would simply have been blessed from it.
fn the_smooth_pyramid_holds_a_tighter_highlight_than_the_rough_one(image: &Image) {
    let measure = |at| block_brightness(image, at, PYRAMID_FACE_BAND);
    let smooth = [measure(SMOOTH_HIGHLIGHT_AT), measure(SMOOTH_TAIL_AT)];
    let rough = [measure(ROUGH_HIGHLIGHT_AT), measure(ROUGH_TAIL_AT)];
    let clear = block_brightness(image, (2, 2), (2, 2));
    eprintln!(
        "crcbl render e2e: cube — the smooth pyramid's face runs {:.1} to {:.1} across its width \
         and the rough one's {:.1} to {:.1}, against a clear of {clear:.1}",
        smooth[0], smooth[1], rough[0], rough[1],
    );

    for (name, blocks) in [("smooth", smooth), ("rough", rough)] {
        for (end, value) in ["inner", "outer"].into_iter().zip(blocks) {
            assert!(
                value > PYRAMID_FACE_LIT_FLOOR,
                "the {end} end of the {name} pyramid's face measures {value:.1} against a clear \
                 of {clear:.1}, so there is no lit face here for a highlight to be on"
            );
        }
    }

    let smooth_falloff = smooth[0] / smooth[1];
    let rough_falloff = rough[0] / rough[1];
    assert!(
        smooth_falloff > rough_falloff * HIGHLIGHT_FALLOFF_RATIO,
        "the smoother material's highlight must be the narrower one: its face falls off by \
         {smooth_falloff:.3} across its width against the rough pyramid's {rough_falloff:.3}, \
         and one lobe shading both would leave those within {HIGHLIGHT_FALLOFF_RATIO} of each \
         other"
    );
}

/// The half-extents of each band read off the plain pyramid's underside, in
/// pixels.
///
/// **That underside is a surface whose pixels are the occlusion channel and
/// nothing else.** `crcbl_shaders::mesh::pyramid_vertices` gives the base one
/// flat normal pointing straight down and one flat albedo, and
/// `DirectionalLight::default`'s sun is above it — so no direct term reaches it,
/// every pixel of it is the ambient term times that albedo, and `mesh.slang`
/// scales exactly that term by the blurred occlusion. Anything that varies
/// across it is occlusion.
///
/// Two rows tall because the whole base is six rows of frame, being seen almost
/// edge-on, and twenty columns placed where those rows are the base alone: the
/// teal side face intrudes to the right of them and the base's own sloping
/// silhouette climbs a row to the left of them.
const PYRAMID_UNDERSIDE_BAND: (u32, u32) = (10, 1);

/// The centre of the band on the last rows of that underside before the clear
/// behind it.
///
/// `ssao_blur.slang`'s kernel reaches two pixels down, so these two rows are
/// exactly the ones whose taps fall on the far plane, and the rows above them
/// are exactly the ones whose taps do not.
const PYRAMID_UNDERSIDE_RIM_AT: (u32, u32) = (54, 85);

/// And the same band two rows further in: same face, same albedo, same ambient,
/// and no tap of the blur's kernel on anything but the surface itself.
const PYRAMID_UNDERSIDE_INSIDE_AT: (u32, u32) = (54, 83);

/// How much brighter the rim band may be than the band inside it.
///
/// **This is the halo, in one number.** A blur that averages the far plane's
/// "nothing occludes here" into the pixels along a silhouette lifts the rim and
/// leaves the rows behind it alone; one that weights its taps by view-space
/// depth cannot, because a tap on the far plane weighs nothing. The frame this
/// was set against measures the rim about a fortieth over the band inside it
/// with `ssao_blur.slang`'s depth-weighted kernel and about a thirteenth over it
/// with a box, on lavapipe and on radv alike — so this sits between the two with
/// room on both sides rather than on a boundary two rasterisers could straddle.
const PYRAMID_HALO_RATIO: f32 = 1.04;

/// How far above the clear the underside must measure.
///
/// [`AO_LIT_FLOOR`]'s job on this scene: a frame that lost the pyramid measures
/// the clear in both bands, and a ratio between two equal numbers is not
/// evidence of anything.
const PYRAMID_UNDERSIDE_LIT_FLOOR: f32 = 10.0;

/// [`Scene::Cube`]'s claim about `ssao_blur.slang`: **the clear does not
/// brighten the silhouette standing in front of it.**
///
/// The far plane has no surface, so `ssao.slang` writes "fully unoccluded" over
/// every pixel the geometry never covered. A box blur then averages that value
/// into the occlusion of the pixels along a silhouette, which draws a bright
/// fringe exactly one kernel deep around everything in the frame — the halo
/// `docs/plan/18-render-features.md` records. A kernel weighted on view-space
/// depth gives those taps no weight, so the rim keeps its own occlusion.
///
/// **No golden could make this claim.** A halo is a plausible picture: a smooth,
/// symmetric, faintly brighter edge, and it sat in the reference for as long as
/// the box blur did. Only a relation between two bands of one flat surface can
/// say the edge is wrong, and only a surface with no direct light on it can say
/// the difference between them is occlusion rather than shading.
///
/// It is this scene and not [`Scene::Ao`], whose name suggests it: that camera
/// looks into a closed trough, so every pixel of that frame is geometry and the
/// frame holds no far plane to bleed and no silhouette to bleed across. See
/// `docs/backlog.md`.
fn the_clear_does_not_brighten_the_silhouette_in_front_of_it(image: &Image) {
    let rim = block_brightness(image, PYRAMID_UNDERSIDE_RIM_AT, PYRAMID_UNDERSIDE_BAND);
    let inside = block_brightness(image, PYRAMID_UNDERSIDE_INSIDE_AT, PYRAMID_UNDERSIDE_BAND);
    let clear = block_brightness(image, (2, 2), (2, 2));
    eprintln!(
        "crcbl render e2e: cube — the pyramid's underside measures {rim:.1} along its silhouette \
         and {inside:.1} two rows in, against a clear of {clear:.1}"
    );
    assert!(
        inside > clear + PYRAMID_UNDERSIDE_LIT_FLOOR,
        "the band two rows inside the pyramid's silhouette measures {inside:.1} against a clear \
         of {clear:.1}, so there is no lit underside here for a halo to be a halo on"
    );
    assert!(
        rim < inside * PYRAMID_HALO_RATIO,
        "the pyramid's underside must not brighten along the edge the clear stands behind: \
         {rim:.1} on the last two rows against {inside:.1} two rows in — that is the far plane's \
         unoccluded 1.0 blurred into the rim"
    );
}

/// Distinct RGBA colours inside a rectangle of `image`.
///
/// [`Image::distinct_colors`] answers for the whole frame, which cannot separate
/// one object from another; this is the same question asked of a region.
fn distinct_colors_in(
    image: &Image,
    columns: std::ops::Range<u32>,
    rows: std::ops::Range<u32>,
) -> usize {
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        for column in columns.clone() {
            if let Some(pixel) = image.pixel(column, row) {
                seen.insert(pixel);
            }
        }
    }
    seen.len()
}

/// Where a point in [`Scene::Sprite`]'s world lands in this frame.
///
/// That scene is drawn with an orthographic camera down −Z with Y up and a fixed
/// half height in world units, so the half width is that times the frame's
/// aspect — `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT` and `sprite_camera`,
/// both private to that module. This is therefore the one place the projection
/// is written down twice; a change to it moves the picture, and re-blessing the
/// golden is the step that will notice.
fn sprite_pixel(world_x: f32, world_y: f32) -> (u32, u32) {
    /// `crcbl::screenshot`'s `SPRITE_HALF_HEIGHT`.
    const HALF_HEIGHT: f32 = 100.0;

    let half_width = HALF_HEIGHT * EXTENT.0 as f32 / EXTENT.1 as f32;
    let x = (world_x / half_width + 1.0) / 2.0 * EXTENT.0 as f32;
    let y = (1.0 - world_y / HALF_HEIGHT) / 2.0 * EXTENT.1 as f32;
    (x as u32, y as u32)
}

/// [`Scene::Sprite`]'s anti-vacuity claim: all four rectangles drew, and the
/// background between them did not.
///
/// The clear is read off the frame's corner rather than written down here — the
/// sprite pass loads its target instead of clearing it, so the corner is the
/// scene's clear by construction and comparing against it needs no assumption
/// about sRGB encoding or channel order.
///
/// The four samples are a quarter of a rectangle in from each centre rather than
/// on it: every sprite is a 2x2 or 4x2 texel sheet, so the exact centre lands on
/// a texel boundary and which of the four colours it resolves to is the
/// sampler's business. A quarter in is squarely inside one texel whichever way
/// V runs.
///
/// **This is the `SV_InstanceID` detector.** Three batches over two sheets, the
/// third starting at instance 3: a backend that reads the wrong lowering draws
/// the last rectangle over the first and leaves its own slot at the clear, which
/// is a slot that fails here by name rather than a number in a summary line.
fn every_sprite_slot_is_painted_and_the_gaps_are_not(image: &Image) {
    /// The world-space centres of `crcbl::screenshot`'s `SPRITE_RECTS`: four
    /// 40x40 rectangles on `y = 0`, ten units apart so no two of them touch.
    const SLOT_CENTRES: [f32; 4] = [-75.0, -25.0, 25.0, 75.0];
    /// Two of the three gaps between those rectangles, in world x. A quarter of
    /// the frame apart, and neither is the centre of a rectangle under any
    /// rotation of the batch order.
    const GAPS: [f32; 2] = [-50.0, 0.0];
    /// A quarter of a rectangle, in world units — see the sampling note above.
    const QUARTER: f32 = 10.0;

    let clear = image.pixel(1, 1).expect("inside");

    for (slot, centre) in SLOT_CENTRES.iter().enumerate() {
        let (x, y) = sprite_pixel(centre - QUARTER, QUARTER);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            differ(pixel, clear),
            "sprite slot {slot} at ({x}, {y}) is still the clear {clear:?} (got {pixel:?}) — \
             that batch did not draw, or it drew somewhere else"
        );
    }

    for gap in GAPS {
        let (x, y) = sprite_pixel(gap, 0.0);
        let pixel = image.pixel(x, y).expect("inside");
        assert!(
            !differ(pixel, clear),
            "the gap between two sprites at ({x}, {y}) is painted {pixel:?} against a clear of \
             {clear:?} — the sprites are not rectangles where the scene puts them"
        );
    }
}

/// Where a fraction of the frame lands in pixels.
///
/// `crcbl::screenshot`'s `ui_draw_list` lays [`Scene::Ui`] out as fractions of
/// the extent, in the pass's Y-down screen pixels, so that the same picture
/// arrives at every `--size`. The samples below are written in the same
/// fractions its rectangles are.
fn ui_pixel(fraction_x: f32, fraction_y: f32) -> (u32, u32) {
    (
        (fraction_x * EXTENT.0 as f32) as u32,
        (fraction_y * EXTENT.1 as f32) as u32,
    )
}

/// [`Scene::Ui`]'s anti-vacuity claim: the panel drew, and the translucent bar
/// blended rather than replaced.
///
/// There is no lit centre to assert here and no useful "the middle is not the
/// clear" — the UI pass **loads** its target rather than clearing it, so what is
/// worth proving is that it composited onto what was already there. The bar
/// straddles the panel's bottom edge, so it covers two different backgrounds:
/// blending gives two different results and replacing gives one. That is a claim
/// a blank frame fails, a frame that lost the panel fails, and a frame whose
/// blend state came up as `Replace` also fails.
///
/// The three samples avoid the two lines of text by construction: both start at
/// 12% of the width, and the panel sample is far to the right of where the
/// longest of them can reach while the bar samples sit below the second line.
fn the_ui_panel_is_painted_and_the_bar_blends_over_two_backgrounds(image: &Image) {
    let clear = image.pixel(1, 1).expect("inside");

    // Inside the panel (8%-92% across, 10%-62% down), outside the bar and
    // clear of the outline's band at the frame's edge.
    let (x, y) = ui_pixel(0.88, 0.25);
    let panel = image.pixel(x, y).expect("inside");
    assert!(
        differ(panel, clear),
        "the UI panel at ({x}, {y}) is still the clear {clear:?} (got {panel:?}) — nothing \
         filled it"
    );

    // The bar (30%-70% across, 45%-85% down) over the panel, then over the
    // clear below the panel's bottom edge.
    let (x, y) = ui_pixel(0.50, 0.50);
    let over_panel = image.pixel(x, y).expect("inside");
    let (x, y) = ui_pixel(0.62, 0.80);
    let over_clear = image.pixel(x, y).expect("inside");
    assert!(
        differ(over_panel, clear) && differ(over_clear, clear),
        "the translucent bar drew nothing: over the panel {over_panel:?}, over the clear \
         {over_clear:?}, against a clear of {clear:?}"
    );
    assert!(
        differ(over_panel, over_clear),
        "the bar is {over_panel:?} over the panel and {over_clear:?} over the clear — the same \
         colour on two backgrounds means the pass replaced rather than blended"
    );
    assert!(
        differ(over_panel, panel),
        "the bar over the panel is {over_panel:?} and the bare panel is {panel:?} — the bar did \
         not reach the panel at all"
    );
}

/// **The culling counters reach the CPU on whichever backend drew the frame**,
/// several frames after the frame they are about.
///
/// This is `docs/plan/40-profiling.md`'s item 8 checked on the backend
/// `CRCBL_GPU` names — and the point of running it here rather than only in
/// `crcbl-vk`'s suite is **wgpu**, whose readback is asynchronous by nature:
/// `map_async` resolves on a later turn of the event loop and only inside a
/// poll, so a ring that works against Vulkan's always-mapped allocation says
/// nothing about a browser's. `crcbl_render::cull_stats`' poll shape is what has
/// to hold on both, and the frame loop below never waits for it.
///
/// # What this asserts, and what it deliberately leaves to `crcbl-vk`
///
/// [`Scene::Cube`] has nothing outside the camera's frustum, so the survivor
/// count here **equals** the submitted count and this cannot tell a cull from a
/// counter wired to the pool's size. That claim needs a scene with something
/// parked off screen, and it is made against a real driver by `crcbl-vk`'s
/// `the_culling_counters_come_back_off_the_gpu_and_are_the_culls_own_answer`.
///
/// What it does assert is everything that is backend-shaped:
///
/// * nothing is reported before a readback has been polled at all, so a number
///   here is not one the CPU made up before any readback landed — and one does
///   arrive, within a bounded number of frames rather than never;
/// * the number that arrives is the instance survivor **word** — reading the
///   cluster word or the light-overflow word beside it, or a buffer no copy ever
///   reached, would all give zero, and zero is not this scene;
/// * it is stamped with the frame it came from, and that frame is older than the
///   frame just drawn.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_culling_counters_come_back_off_the_gpu_on_this_backend() {
    crcbl_core::log::init_logging();

    let setup = OffscreenSetup::open(EXTENT.0, EXTENT.1, Scene::Cube)
        .unwrap_or_else(|why| panic!("a GPU backend opens for the cube scene: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);
    eprintln!(
        "crcbl render e2e: culling counters on {backend}, {path:?}",
        backend = setup.backend(),
        path = setup.caps().geometry_path(),
    );

    // A frame records the copy and the next frame requests the readback for it,
    // so the third frame is the first that can poll anything: a report before
    // that is one nothing asked the GPU for.
    let floor = 2;

    for frame in 1..=floor {
        setup.draw_and_readback().expect("the frame renders");
        let counters = setup.counters();
        assert_eq!(
            counters.drawn, None,
            "frame {frame} is before the first poll, so the row must still say `indirect`",
        );
        assert_eq!(counters.cull_frame, None);
    }

    // And then it has to arrive. **Bounded, and that is the assertion**: how
    // many polls a readback needs is the backend's business — wgpu's `map_async`
    // resolves on a later poll and a browser's answers a frame after the
    // question — so a ring that throws an unanswered poll away reports nothing
    // for ever, and a test that waited without a bound would hang rather than
    // say so.
    let bound = 32;
    let mut rendered = floor;
    while setup.counters().cull_frame.is_none() {
        assert!(
            rendered < floor + bound,
            "no culling report arrived in {bound} frames; the readback is being polled and \
             released rather than answered",
        );
        setup.draw_and_readback().expect("the frame renders");
        rendered += 1;
    }
    let counters = setup.counters();
    eprintln!("crcbl render e2e: counters after {rendered} frames — {counters:?}");
    let drawn = counters
        .drawn
        .expect("the ring has come round, so there is a survivor count");
    assert_eq!(
        drawn, counters.instances,
        "every instance in this scene is in front of the camera, so the cull kept them all \
         — and a zero here is a copy that never landed: {counters:?}",
    );
    assert!(
        drawn > 1,
        "one would be the tonemap's own triangle and nothing else: {counters:?}",
    );
    let cull_frame = counters.cull_frame.expect("a readback answered");
    assert!(
        cull_frame >= 1 && cull_frame < rendered,
        "the report names frame {cull_frame} on a run that has drawn {rendered}: {counters:?}",
    );

    setup.finish();
}

/// Half-extents of the block each atmosphere band is read over, in pixels.
///
/// Small, where the probe bands are five square: the sky is a gradient with a
/// real slope across it, so a wide block averages a curve and the prediction
/// has to average the same curve back. Three square is nine pixels, enough to
/// take the readback's own noise out and narrow enough that the curve inside it
/// is nearly a plane.
const ATMOSPHERE_BAND: (u32, u32) = (3, 3);

/// Where in the frame the atmosphere bands are read, in fractions of the
/// extent.
///
/// Spread across both axes on purpose. The rows walk from just under the
/// horizon at the bottom to the deep sky at the top, which is the LUT's `v`
/// axis; the columns walk left to right, which is its `u` axis whenever the sun
/// is not straight overhead. A band at the exact centre of a frame would
/// exercise one texel of each.
const ATMOSPHERE_BANDS: [(f32, f32); 9] = [
    (0.15, 0.12),
    (0.50, 0.12),
    (0.85, 0.12),
    (0.15, 0.45),
    (0.50, 0.45),
    (0.85, 0.45),
    (0.15, 0.85),
    (0.50, 0.85),
    (0.85, 0.85),
];

/// Levels of 255 the device's atmosphere may sit from the host's.
///
/// **Swept on both local adapters before it was fixed**, the way
/// [`PROBE_MIRROR_LEVELS`] was. Over three suns × nine bands × three channels
/// the worst miss measured 0.29 levels on the discrete adapter (radv, an
/// RX 7900 XTX) and the same 0.29 on the software one (lavapipe) — the same
/// number to four figures, which is itself the finding: what is left between
/// the two sides is the frame's own eight-bit quantisation against a
/// prediction that has none, and not anything either rasteriser did. This is
/// three times it.
///
/// The rest of the gap could only be arithmetic. The host and the shader read
/// the *same* LUT rows — the buffer holds the `f32`s the march produced, so
/// there is no texel format between them — through the same clamped bilinear,
/// and the only floats the two do not agree about bit for bit are the ray,
/// which comes out of two matrix products the GPU is free to contract into
/// fused multiply-adds, and the blend weights derived from it.
///
/// **What this budget is worth in physical terms** is what
/// [`ATMOSPHERE_SENSITIVITY_SCALE`] says, and it is not flattering to sRGB: an
/// encoded level is a compressed view of a radiance, so a band this dark moves
/// by well under a level for a per-cent change in the sky. The test prints
/// both figures side by side rather than letting the budget read as tighter
/// than it is.
const ATMOSPHERE_MIRROR_LEVELS: f32 = 0.9;

/// How much brighter the sky is made in
/// [`an_atmosphere_frame_is_the_host_lut`]'s anti-vacuity check.
///
/// Ten per cent, which is the smallest round figure whose level shift clears
/// [`ATMOSPHERE_MIRROR_LEVELS`] by the margin that check asserts. It is a
/// statement about the sRGB encode rather than about this test: at the
/// radiances an atmosphere puts on a frame, one per cent of sky is a third of
/// a level.
const ATMOSPHERE_SENSITIVITY_SCALE: f32 = 1.10;

/// The world direction `sky.slang` shades pixel `(column, row)` along.
///
/// The shader's own unprojection, restated through `glam`: unproject two points
/// at different depths, take their difference, and rotate it into world space.
/// The **difference** rather than the near point normalised, for the reason
/// `sky.slang` gives — it is the direction under an orthographic projection
/// too — and `NDC_NEAR`/`NDC_MID` are that file's two depths.
fn atmosphere_ray(camera: &crcbl::render::Camera, column: u32, row: u32) -> [f32; 3] {
    let aspect = EXTENT.0 as f32 / EXTENT.1 as f32;
    let inv_proj = camera.projection.matrix(aspect).inverse();
    let inv_view = camera.view().inverse();
    // The centre of the pixel, then `sky.slang`'s own `uv` → NDC line.
    let u = (column as f32 + 0.5) / EXTENT.0 as f32;
    let v = (row as f32 + 0.5) / EXTENT.1 as f32;
    let ndc = glam::Vec2::new(u * 2.0 - 1.0, 1.0 - v * 2.0);
    let unproject = |depth: f32| {
        let point = inv_proj * glam::Vec4::new(ndc.x, ndc.y, depth, 1.0);
        point.truncate() / point.w
    };
    let along = unproject(0.5) - unproject(1.0);
    (inv_view * along.extend(0.0))
        .truncate()
        .normalize()
        .to_array()
}

/// What the host says the block at `centre` should read on `channel`.
///
/// Per pixel and then averaged, in that order, for
/// [`predicted_block_channel`]'s reason: the encode below is not linear and the
/// sky has a slope across the block, so averaging the radiances first and
/// encoding once would predict a different number than the frame contains.
///
/// The chain is short because the fixture made it short. Nothing is lit, so
/// there is no shading; the tonemap is
/// `crcbl_shaders::tonemap::DEFAULT_EXPOSURE` and a clamp, which is the
/// identity below one; the target is sRGB, so the encode is the last step.
fn predicted_atmosphere_channel(
    sky: &crcbl::shaders::atmosphere::SkyView,
    camera: &crcbl::render::Camera,
    centre: (u32, u32),
    channel: usize,
) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for (x, y) in block_pixels(centre, ATMOSPHERE_BAND) {
        let radiance = sky.radiance(atmosphere_ray(camera, x, y));
        total += srgb_encode(radiance[channel].min(1.0)) * 255.0;
        count += 1;
    }
    assert!(count > 0, "an empty block predicts nothing");
    total / count as f32
}

/// The frame drawn under an atmosphere is the sky-view LUT the host marched.
///
/// `docs/plan/43-render-standards.md` §8's device half. The fixture puts no
/// geometry in the frame, so every pixel is `sky.slang`'s background arm; this
/// unprojects each band's pixels into world rays, reads the same LUT through
/// `crcbl_shaders::atmosphere::SkyView::radiance`, and holds the two together.
///
/// **What makes it a test of the shader rather than of the host**: the host
/// spelling and the shader spelling of the LUT read are separate source — Slang
/// has no `#include` — and the sun's frame, the azimuth cosine, both
/// coordinate maps and the clamped bilinear are each written twice. A
/// disagreement in any of them lands here as a band that is the wrong colour.
///
/// **Shown red by sabotage** (2026-09-05, radv): `sky.slang`'s
/// `atmosphere_radiance` was changed to read its row from `-direction.y`,
/// which turns the sky upside down, the artifacts were recompiled, and this
/// reported `sun 0, band (38, 23), red measures 76.47 and the host LUT
/// predicts 26.65, a miss of 49.82 level(s) against a budget of 0.9`. Restored,
/// recompiled, and the re-run agreed to 0.29 levels.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn an_atmosphere_frame_is_the_host_lut() {
    crcbl_core::log::init_logging();

    let camera = crcbl::screenshot::atmosphere_camera();
    let mut worst = 0.0f32;
    let mut worst_at = (0usize, (0u32, 0u32), 0usize);
    for sun in 0..crcbl::screenshot::ATMOSPHERE_SUNS.len() {
        let setup = OffscreenSetup::open_forward(EXTENT.0, EXTENT.1, |device, queue, format| {
            crcbl::screenshot::atmosphere_forward(device, queue, format, sun)
        })
        .unwrap_or_else(|why| panic!("a GPU backend opens for the atmosphere scene: {why}"));
        let mut setup = Offscreen::guard(SUITE, setup);
        let format = setup.format();
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
        setup.finish();
        let image =
            Image::from_readback(width, height, &pixels, channel_order(format)).expect("one image");

        let sky = crcbl::screenshot::atmosphere_view(sun);
        for (across, down) in ATMOSPHERE_BANDS {
            let at = (
                (across * EXTENT.0 as f32) as u32,
                (down * EXTENT.1 as f32) as u32,
            );
            for (name, channel) in [("red", 0), ("green", 1), ("blue", 2)] {
                let measured = block_channel(&image, at, ATMOSPHERE_BAND, channel);
                let predicted = predicted_atmosphere_channel(&sky, &camera, at, channel);
                let miss = (measured - predicted).abs();
                if miss > worst {
                    worst = miss;
                    worst_at = (sun, at, channel);
                }
                assert!(
                    miss <= ATMOSPHERE_MIRROR_LEVELS,
                    "sun {sun}, band {at:?}, {name} measures {measured:.2} and the host LUT \
                     predicts {predicted:.2}, a miss of {miss:.2} level(s) against a budget of \
                     {ATMOSPHERE_MIRROR_LEVELS} — the shader and the host are reading the LUT \
                     differently, or this frame did not reach the swapchain through the sRGB \
                     encode this model assumes"
                );
            }
        }
    }

    // Anti-vacuity in two parts. The first is that this fixture draws a sky
    // with structure in it rather than a flat field, which is what makes nine
    // bands nine measurements; the second is that a brighter sky is a frame
    // this comparison rejects, so the agreement is about the sky rather than
    // about both sides being dark.
    let spread = {
        let sky = crcbl::screenshot::atmosphere_view(1);
        let mut lowest = f32::INFINITY;
        let mut highest = f32::NEG_INFINITY;
        for (across, down) in ATMOSPHERE_BANDS {
            let at = (
                (across * EXTENT.0 as f32) as u32,
                (down * EXTENT.1 as f32) as u32,
            );
            for channel in 0..3 {
                let level = predicted_atmosphere_channel(&sky, &camera, at, channel);
                lowest = lowest.min(level);
                highest = highest.max(level);
            }
        }
        highest - lowest
    };
    assert!(
        spread > 20.0 * ATMOSPHERE_MIRROR_LEVELS,
        "the brightest and dimmest of this fixture's bands are {spread:.2} level(s) apart, which \
         is not a sky with anything in it to agree about"
    );
    let sensitivity = {
        let sky = crcbl::screenshot::atmosphere_view(1);
        let mut apart = 0.0f32;
        for (across, down) in ATMOSPHERE_BANDS {
            let at = (
                (across * EXTENT.0 as f32) as u32,
                (down * EXTENT.1 as f32) as u32,
            );
            for channel in 0..3 {
                let level = predicted_atmosphere_channel(&sky, &camera, at, channel);
                let scaled = srgb_encode(
                    (srgb_decode(level / 255.0) * ATMOSPHERE_SENSITIVITY_SCALE).min(1.0),
                ) * 255.0;
                apart = apart.max((scaled - level).abs());
            }
        }
        apart
    };
    assert!(
        sensitivity > 3.0 * ATMOSPHERE_MIRROR_LEVELS,
        "a sky {ATMOSPHERE_SENSITIVITY_SCALE}× as bright moves a band by {sensitivity:.2} \
         level(s), which is not enough for the agreement above to be a claim about the sky at all"
    );
    eprintln!(
        "crcbl render e2e: atmosphere — the shader and the host LUT agree to {worst:.2} level(s) \
         at worst, at sun {} band {:?} channel {}, over {} suns × {} bands × 3 channels; the \
         bands span {spread:.2} level(s) and a {ATMOSPHERE_SENSITIVITY_SCALE}× sky would move \
         one by {sensitivity:.2}",
        worst_at.0,
        worst_at.1,
        worst_at.2,
        crcbl::screenshot::ATMOSPHERE_SUNS.len(),
        ATMOSPHERE_BANDS.len(),
    );
}
