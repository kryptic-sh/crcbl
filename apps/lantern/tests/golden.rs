//! The room off a real device, from the fixed camera, against a checked-in
//! golden — and the claims about the lighting in front of it.
//!
//! # A golden alone cannot make a claim about lighting
//!
//! A wrong picture is a plausible picture. A shadow map that is never written
//! leaves every surface lit; a window wall built as one quad leaves the floor
//! evenly bright; a material factor that never reached the device leaves a wall
//! grey instead of red; and every one of those produces a frame somebody would
//! bless. So the golden is the *last* of the assertions here, and the ones
//! before it are about **where** the frame is bright and dark, in the shape
//! `crates/crcbl/tests/render_e2e.rs` uses.
//!
//! Each of them is a ratio between two blocks of pixels rather than an
//! absolute value, because an absolute one is a second golden written in
//! numbers: it moves when the tonemap moves, and it says nothing a reviewer can
//! act on.
//!
//! # Two capability paths, one golden
//!
//! Rule 12 asks a sample's CI run for "the path its runner selects plus one
//! below it", and an adapter reports what it reports — so the lesser path is
//! reached by opening a device *without* the features that select the better
//! one. [`the_room_draws_the_same_on_a_path_below_the_devices_own`] is that
//! second arm, and it is held to the **same** reference: a lesser path is a
//! constraint on data layout rather than a separate renderer, so a difference
//! between the arms is a bug and a second golden would bless it.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-lantern-golden.sh` is the only thing that turns both off — and it
//! fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use crcbl::hal::{AdapterInfo, BindingModel, Features, Format, GeometryPath};
use crcbl::math::Vec3;
use crcbl::render::{Camera, EffectOverride, EffectRequest, Fog, ForwardRenderer, RenderEffects};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl::shaders::probe::GpuProbe;
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_lantern::{Forced, room};

/// The extent the checked-in golden is blessed at.
///
/// The same 4:3 the fixed camera frames for, and the same size every other
/// golden in the tree is: small enough to read in a diff, large enough that the
/// blocks below are tens of pixels rather than a handful.
const EXTENT: (u32, u32) = (256, 192);

/// The extent [`the_room_reads_the_same_at_presentation_size`] renders at.
///
/// Twenty-five times the pixels, and the same aspect, so it is the same framing
/// rather than a different picture. Nothing is blessed at this size — what it is
/// for is stated on that test.
const REVIEW_EXTENT: (u32, u32) = (1280, 960);

/// Where a review-size frame is written, relative to the workspace root.
///
/// Under `target/`, which is already ignored, so a reviewer has a path to open
/// and CI has a directory it can upload with no new rule.
const REVIEW_DIR: &str = "target/lantern";

/// How many distinct colours a frame of this room has to have.
///
/// A room lit by a sun through a window, an orange lamp, a cool downlight in the
/// far corner and a cool ambient, over every object and material row the room
/// declares: a frame with fewer than this drew the clear colour and very little
/// else. Counted rather than guessed at — see `Image::distinct_colors`, which
/// stops counting at the bound it is given.
const MIN_COLORS: usize = 64;

/// Half-extents, in pixels, of the block the downlight's two claims average
/// over, at [`EXTENT`].
///
/// Smaller than [`BLOCK`], and the corner is what makes it so: the downlight's
/// pool is the far end of the room seen from seven metres, its two read points
/// sit a third of a metre apart on a wall that is nearly edge-on to the camera,
/// and a block sized for the open floor takes the corner post's own silhouette
/// in on one side. Swept rather than guessed — the largest block that holds
/// both points inside their own region of the pool, with a couple of pixels
/// spare at [`EXTENT`], is this one.
const CORNER_BLOCK: (u32, u32) = (3, 3);

/// How much brighter the downlight's pool has to read than the band of it the
/// corner post shadows.
///
/// Both blocks are the same plaster on the same wall at the same normal, and
/// neither is in the sun or the lamp's reach at `t = 0` —
/// `crcbl_lantern::room::the_downlight_casts_a_shadow_of_the_corner_post` is
/// what proves that with no GPU. What separates them is the post, and what is
/// left under it is the ambient.
///
/// **Two rather than the ratio that was measured, and the room with no post in
/// it is why.** The shadowed block is further down the wall than the lit one,
/// so the falloff alone leaves it dimmer even with nothing standing in the
/// cone: with the corner post unplaced the pair reads 153.0 against 111.0 on
/// radv and 152.8 against 111.1 on lavapipe, a ratio of 1.38 either way. With
/// it placed the same pair reads 147.3 against 51.9 and 147.2 against 53.0,
/// which is 2.8. Two sits between the two states rather than under the measured
/// one, which is what makes this go red when the occluder goes away — and it is
/// the half [`SPOT_SHADOW_LIFT`] cannot cover, because it is the one a single
/// frame can carry, at both extents and on both capability arms.
const SPOT_SHADOW_RATIO: f32 = 2.0;

/// Half-extents, in pixels, of the block each other claim below averages over.
///
/// A block rather than a pixel, because a single pixel is a sample of the
/// rasteriser as much as of the lighting: one texel of the floor's check, one
/// step of the occlusion blur, or a triangle edge landing a pixel either way
/// all move it. Small enough at [`EXTENT`] that a block stays on the surface it
/// was aimed at.
const BLOCK: (u32, u32) = (4, 4);

// ---------------------------------------------------------------------------
// The claims, as world positions
// ---------------------------------------------------------------------------

/// A floor point **inside** the shaft of sunlight through the window.
///
/// [`room::SUNLIT_FLOOR`] — derived in that module from the window's opening and
/// the sun's direction, and checked there with no GPU.
const SUNLIT: Vec3 = room::SUNLIT_FLOOR;

/// A floor point on the same surface, **outside** the shaft and out of the
/// lamp's reach, on [`SUNLIT`]'s terms.
const SHADED: Vec3 = room::SHADED_FLOOR;

/// How much brighter the sunlit floor must be than the shaded floor beside it.
///
/// A ratio, not a difference. Well above one because these two points differ in
/// the whole of the sun's direct contribution and in nothing else — same
/// surface, same normal, same depth, same material, same (absent) lamp — so a
/// frame in which they are close is a frame in which the sun is not coming
/// through the window at all. Not higher still: the shaded floor is ambient
/// rather than black, deliberately, and the tonemap compresses the bright end.
const SHAFT_RATIO: f32 = 2.0;

/// How much of nearby plaster's brightness the mirror's SSR-miss control point
/// must retain.
///
/// This is a non-black fallback control, not a claim that the mirror is brighter
/// than plaster or that it rejects whole-frame brightening. [`MIRROR_AT`] has no
/// direct light on it at all — see `crcbl_lantern::room::MIRROR_MISSES` — so what
/// it reads is the probe environment the SSR march falls back to, while the
/// plaster it is measured against is directly lit.
///
/// **The probe updater is what moved it, not the mirror.** Lantern's rows were an
/// authored CPU bake until the reflective shadow map began filling them every
/// frame, and one sun-only bounce through the visibility gate is dimmer than that
/// bake was, which had neither a gate nor an occluder in it: the ratio went from a
/// measured 0.17 to 0.099 at [`EXTENT`] and 0.098 at [`REVIEW_EXTENT`] on radv,
/// and to 0.096 and 0.095 on lavapipe. Half of the smallest of those. The teeth
/// are unchanged: with the probe rows zeroed this point reads 0.0, which
/// [`zero_probes_only_remove_the_ssr_and_rough_fallbacks`] pins, so the failure
/// this guards lands the whole way to the floor rather than near the threshold.
///
/// [`zero_probes_only_remove_the_ssr_and_rough_fallbacks`]: fn@zero_probes_only_remove_the_ssr_and_rough_fallbacks
const MIRROR_FRACTION_OF_PLASTER: f32 = 0.05;

/// How much a fully screen-space hit may vary when the probe rows are zeroed.
///
/// **What the difference is** is the environment the march blends in behind its
/// own hit: `ssr.slang` weighs a crossing by a confidence and fills the rest
/// with the probe environment, so zeroing the probes removes exactly that
/// remainder and what is left is the screen hit alone.
///
/// **The march moved it once and the bent normals moved it again**, and neither
/// was the mirror. It was 6% against a measured 5.1% until `ssr.slang` was
/// rebuilt on the Hi-Z pyramid on 2026-08-27, which resolves a crossing to a
/// single texel instead of to a 1.5-pixel stride and so measures how far past
/// the surface the ray got on a finer basis — a lower confidence for the same
/// hit, and a larger remainder. That took it to 10% against a measured 7.3%.
///
/// `docs/plan/46-ambient-occlusion.md`'s bent-normal rung then widened the
/// remainder itself. What the surface behind this hit reflects is lit by the
/// probe grid, and that grid is now sampled along the occlusion channel's bent
/// direction rather than along the shading normal — so the reflected surface is
/// brighter, and zeroing the rows removes more of it. Measured 2026-09-02: the
/// point reads 53.7 -> 47.1 on the discrete adapter and 54.1 -> 47.7 on
/// llvmpipe, about 12% either way, against 49.7 -> 47.1 with
/// `crcbl_render::ssao::r_ssao_bent_normals` off — and the *zeroed* reading is
/// unmoved by the switch, which is what says the rung widened the remainder
/// rather than changing the hit. So 15%, on the old constant's terms: margin
/// over a measured difference the two adapters agree on.
///
/// The teeth are unchanged. A fallback substituted for the hit is not a few per
/// cent: with the probe rows zeroed the fallback reads at most 1.0, which the
/// miss point in the same test pins at 20.3 -> 0.0, so that failure lands an
/// order of magnitude below this threshold whichever of the two it is.
const SSR_HIT_TOLERANCE: f32 = 0.15;

/// How much brighter the foot of the mirror panel is than its face further up.
///
/// The probe fallback fills both regions; the foot still has its screen-space
/// hit, so this is a modest relation rather than the old black-miss contrast.
const MIRROR_GRADIENT: f32 = 1.05;

/// The minimum authored-to-zeroed brightness ratio the brass probe fallback
/// control must retain.
///
/// **Re-measured 2026-09-03, when `ssr.slang`'s probe fallback gained the
/// Chebyshev visibility weight `mesh.slang` already had.** That weight takes
/// each of the eight corners' share down by what the room's own geometry hides
/// from this point, so the *correct* fallback here is smaller than the
/// unweighted one this constant was set against: the brass reads **98.5 against
/// 94.5 zeroed on both the discrete adapter and llvmpipe**, a ratio of 1.042,
/// where forcing that weight back to one reads 100.9 and 1.068. The two arms
/// agree to the printed digit on both adapters, so what this needs is margin
/// over a measurement rather than over a spread.
///
/// **The teeth are what they were.** A fallback removed outright — the failure
/// this control exists to catch — makes the two arms the same frame and the
/// ratio exactly 1.000, which is 2.8 levels below the threshold this value sets
/// at the measured pair. Raising it back to 1.05 would fail the correct frame.
const BRASS_PROBE_RATIO: f32 = 1.03;

/// The floor a lit block's mean brightness has to clear, out of 255.
///
/// What separates "correctly dark" from "nothing drew". Every ratio above is
/// satisfied by two blocks of zero, so each one is paired with this.
const LIT_FLOOR: f32 = 6.0;

/// How much of the coloured wall's brightness has to be red.
///
/// `base_color` is a factor the fragment stage multiplies in, so a material row
/// that never reached the device leaves this wall the same plaster as every
/// other — and the frame is a perfectly plausible picture of a white room. Well
/// above a third, which is what a neutral surface gives.
const BOUNCE_REDNESS: f32 = 0.55;

/// A point on the coloured wall, chosen where the lamp reaches it.
///
/// The lamp rather than the sun: the sun comes in low through a window in the
/// **opposite** wall, so what it reaches on this one is a band along its foot.
/// That is the room being a room, and it is also what makes this wall's absent
/// bounce legible — a wall in shadow beside a floor in full sun is exactly the
/// configuration global illumination would change.
const BOUNCE_AT: Vec3 = Vec3::new(room::HALF_WIDTH, 1.4, -2.0);

/// A point on the mirror-grade panel's `+Z` face, above everything of it that
/// reflects — see `crcbl_lantern::room::MIRROR_MISSES`, which is this point and
/// carries the proof.
const MIRROR_AT: Vec3 = room::MIRROR_MISSES;

/// A point on the plaster back wall, clear of the panel and of the plinth.
///
/// [`room::UNTINTED_PLASTER`] — the comparand the SSR-miss control above is
/// measured against, and directly lit, which is what makes it one.
const PLASTER_AT: Vec3 = room::UNTINTED_PLASTER;

/// Floor in the plinth's contact corner, where ambient occlusion is strongest.
///
/// The plinth's `+Z` face stands at `z = -2.4` and its run in `x` covers this
/// point, so this is floor a few centimetres out from a wall of it — inside the
/// occlusion radius on one whole side. Out of the sun (the shaft is at `+x`) and
/// out of the lamp's reach at `t = 0`, exactly as [`SHADED`] is, so the two are a
/// pair that differs in occlusion and in nothing else.
const AO_CORNER: Vec3 = Vec3::new(-1.2, 0.0, -2.32);

/// How much brighter the shadowed floor must get when the atlas is switched off.
///
/// A ratio rather than a difference, and against the *shadowed* reading, because
/// what that block gains is the sun's whole direct contribution — the same
/// quantity [`SHAFT_RATIO`] measures from the other side. Set below the measured
/// lift so a driver that resolves the shaft's edge differently does not flip it;
/// well above one, because a frame in which the two are close is a frame whose
/// shadow switch did nothing.
const SHADOW_LIFT: f32 = 1.6;

/// How much brighter the band the corner post shadows must get when the atlas
/// is switched off.
///
/// **The one block in this room whose whole direct term is the downlight's.**
/// The `-x` wall's inner face is back-facing to the sun, so no cascade can
/// change it whatever it resolves to, and the lamp at `t = 0` is past its own
/// reach, where `punctual_falloff` is exactly zero. So the only thing switching
/// the atlas off can add here is the light the corner post was blocking, which
/// is what makes this a claim about the spot's own tile rather than about the
/// frame — and its control, `room::SPOT_LIT`, is the same wall in the same pool
/// with nothing between it and the fitting.
///
/// Measured 50.9 → 105.2 on radv and 51.0 → 105.2 on lavapipe. With the corner
/// post unplaced the same block reads 112.5 → 112.5 — it does not move at all,
/// which is the state this whole claim exists to catch, and one and a half sits
/// between the two.
const SPOT_SHADOW_LIFT: f32 = 1.5;

/// How much brighter the contact corner must get when occlusion is switched off.
///
/// Smaller than [`SHADOW_LIFT`] and for a real reason: occlusion scales the
/// *ambient* term alone, so what this block can gain is bounded by the share of
/// it that is ambient — `docs/backlog.md` measures a quarter at a single wall.
/// This is a claim about that term, not about the pixel.
///
/// **Smaller again since the multi-bounce tint**, which lifts an occluded
/// fragment by the colour of what occludes it and so narrows the very gap this
/// measures. The corner reads `57.5` with occlusion against `59.7` without, a
/// ratio of `1.038` where the frame this was first set against left room for
/// `1.08`. The claim it exists to catch is unchanged and still lands at exactly
/// `1.00` — a corner that does not move when the pass is switched off, or a
/// placeholder that is not white — but it is caught with less room than before.
/// `docs/backlog.md` carries what that costs and how an AO intensity control
/// would return it.
const AO_LIFT: f32 = 1.02;

/// A point on the front wall, dead ahead of [`room::monitor_camera`].
///
/// The control half of the camera-stack claim: rough plaster in the room's
/// ambient with the lamp on it, a dielectric whose `F0` leaves the reflection
/// pass almost nothing to contribute. Measured on radv at
/// [`MONITOR_REVIEW`]: 69.64 with the reflections in the monitor's stack and
/// 69.55 without, which is a thousandth.
const FRONT_WALL_AT: Vec3 = Vec3::new(0.8, 1.55, room::HALF_DEPTH);

/// What a block reading "the reflection pass supplied all of this" is allowed to
/// keep once that pass is gone, out of 255.
///
/// `crcbl_render::ssr`'s composite is the whole of what lights a conductor with
/// no sun on it, so the honest answer is zero and the tolerance is for the
/// rasteriser's edge pixels. Measured on radv: exactly 0.0 at
/// [`room::BRASS_BACK`].
const MONITOR_UNLIT: f32 = 1.0;

/// How much brighter the part of the monitor's picture showing the ceiling has
/// to be than the part showing the block's unlit far face.
///
/// **The claim that the screen is showing the room**, and the two blocks are
/// placed by projecting two world points through the *monitor's* camera and
/// reading where they land on the screen quad — see [`room::screen_point`]. A
/// screen holding a flat fill fails it whatever the fill is, a screen nobody
/// wrote is black and fails the floor below, and a picture pasted on upside down
/// swaps the two and fails it the other way.
///
/// Measured on radv at [`LIVE_EXTENT`]: 58.8 against 4.0, which is fourteen
/// times. **And measured with the camera layer taken out of
/// `EffectRequest::resolve`**, where the monitor draws its reflections after all
/// and the same pair reads 58.8 against 17.3 — three and a half times. This
/// threshold sits between the two, so the composed frame carries the camera
/// layer as well rather than merely showing that something was copied onto the
/// screen.
const MONITOR_CONTRAST: f32 = 6.0;

/// A point on the ceiling the monitor's camera frames, and the bright half of
/// the claim above.
///
/// The lamp hangs below it, so it is the brightest thing in that view — 159.7 on
/// radv at [`MONITOR_REVIEW`], against the block's far face at 0.0.
const MONITOR_BRIGHT_AT: Vec3 = Vec3::new(0.8, room::HEIGHT, 1.0);

/// How far a control block is allowed to move, as a share of its own reading.
///
/// Every claim above is paired with a block the same switch must **not** move,
/// which is what separates "this effect stopped darkening its own corner" from
/// "the whole frame got brighter". Not zero: the two frames are separate runs of
/// a rasteriser, and a block on a lit floor carries the texture's check.
const UNCHANGED: f32 = 0.02;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The selectors [`the_room_draws_the_same_on_a_path_below_the_devices_own`]
/// holds its lesser arm at.
///
/// The floor of both axes — the browser's shape, which
/// `docs/plan/sample/13-lantern.md` already names as the combination this desktop
/// can be made to run. The floor rather than one named step down because an
/// adapter reports what it reports: from here, *any* adapter offering anything
/// above the floor gives the two arms a real difference to compare, and the one
/// that offers nothing above it says so through the assertion rather than
/// quietly.
const BELOW: Forced = Forced {
    geometry: Some(GeometryPath::IndirectPerBatch),
    binding: Some(BindingModel::ArrayPages),
};

/// What to ask the lesser arm's device for: [`draw`]'s own set, minus the flags
/// whose presence would select something above [`BELOW`].
///
/// **The subtraction is `crcbl_lantern`'s, not this file's.** `Forced` is what the
/// binary's `--force-geometry` and `--force-binding` go through, so taking the
/// difference there and applying it here means a selector that grows a flag
/// moves this arm too, instead of leaving a second table behind still naming the
/// old ones.
///
/// It has to be a difference rather than `BELOW.optional_features()` outright,
/// because the two sets do **not** share a base:
/// `Forced::optional_features` starts from `GpuContextDesc::default`'s optional
/// set plus `TASK_SHADER`, which also carries the timestamp, present-feedback
/// and present-timing flags [`OffscreenSetup::OPTIONAL_FEATURES`] never asks
/// for. An arm opened from a different base than the one it is compared against
/// is not a comparison.
fn below_features() -> Features {
    let selecting = Forced::default()
        .optional_features()
        .difference(BELOW.optional_features());
    OffscreenSetup::OPTIONAL_FEATURES.difference(selecting)
}

/// The air the room is drawn through by the one claim that is about a medium.
///
/// **The room ships in a vacuum, and this is not it.** Every blessed frame and
/// every other claim here is drawn with [`Fog::NONE`], because this suite's
/// controls are near-zero absolutes — "a conductor with no ambient and no
/// reflection is black", "the coloured wall's channels separate", "the screen's
/// unlit far face reads nothing" — and a participating medium adds a term to
/// every block in the frame, including those. Measured: at a density of 0.05
/// four of them fail, and they still fail at 0.008.
/// `docs/backlog.md` carries what it would take to give the room air it keeps.
///
/// So this is the medium `the_air_scatters_the_sun_where_the_cascades_let_it_through`
/// puts in front of the camera and nothing else does, and every number in it is
/// chosen against that claim's two blocks:
///
/// * [`falloff`](Fog::falloff) is [`room::HEIGHT`] over a
///   [`reference_height`](Fog::reference_height) of the floor, so the column
///   thins upward across the room it is measured in rather than standing as a
///   slab.
/// * [`anisotropy`](Fog::anisotropy) is **isotropic**, which real fog is not —
///   Mie scattering sits near `0.8` forward. The fixed camera looks *across* the
///   sun's path rather than into it, and a forward lobe aims the scattered
///   radiance away from the only eye this fixture has: measured at
///   [`room::SPOT_SHADOWED`], the shaft's lift falls from 9.4% at `0.0` to 3.1%
///   at `0.7` and 1.4% at `0.85`. Isotropic is what makes the claim below a
///   claim rather than a coin toss.
/// * [`sun_scattering`](Fog::sun_scattering) is one — the whole of the sun's
///   radiance per unit length, which is the conservative medium's ceiling. It is
///   the term the claim below switches off, and `Arm::without_shafts` is how.
const AIR: Fog = Fog {
    density: 0.05,
    falloff: room::HEIGHT,
    reference_height: 0.0,
    color: Vec3::new(0.10, 0.12, 0.16),
    sun_scattering: 1.0,
    light_scattering: 0.0,
    anisotropy: 0.0,
};

/// One arm of a comparison: which view of the room, at which camera stack, in
/// which air.
///
/// A struct rather than three arguments because the others are normally derived
/// from the view — `Arm::of` is what every arm but two uses — and each test that
/// overrides one is holding two arms apart at *exactly* that layer, which is
/// worth saying at the call site rather than passing as a bare flag set.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Arm {
    /// Which view: whose camera, and which objects are in it.
    view: room::View,
    /// The camera-stack layer of the resolution order this arm renders with.
    stack: RenderEffects,
    /// The medium the frame is drawn through.
    fog: Fog,
}

impl Arm {
    /// The view as the room declares it — [`room::View::stack`], its own camera,
    /// and the vacuum every blessed frame here is drawn in. See [`AIR`].
    const fn of(view: room::View) -> Self {
        Self {
            view,
            stack: view.stack(),
            fog: Fog::NONE,
        }
    }

    /// The same view with a different camera stack, which is what makes a pair
    /// of frames a claim about that layer.
    const fn with_stack(self, stack: RenderEffects) -> Self {
        Self { stack, ..self }
    }

    /// The same arm with [`AIR`] in front of the camera.
    const fn with_air(self) -> Self {
        Self { fog: AIR, ..self }
    }

    /// The same arm in [`AIR`] that the sun does not scatter into.
    ///
    /// The control half of
    /// `the_air_scatters_the_sun_where_the_cascades_let_it_through`:
    /// [`Fog::sun_scattering`] is the froxel path's whole reason for existing —
    /// zero it and the medium keeps its absorption and its environment term and
    /// loses the shaft, so two arms that differ in nothing else isolate what the
    /// scatter pass put there and where.
    const fn without_shafts(self) -> Self {
        Self {
            fog: Fog {
                sun_scattering: 0.0,
                ..AIR
            },
            ..self
        }
    }
}

/// Opens a device on the best path this adapter offers, builds the room on it,
/// and reads one frame back.
///
/// [`draw_with`] is this asking for something less, which is the whole of what
/// separates them: every claim below is about a frame, and a frame is the one
/// thing that does not say which tail drew it.
fn draw(extent: (u32, u32), effects: RenderEffects) -> (Image, String) {
    let (image, paths, _) = draw_with_probes(
        extent,
        effects,
        OffscreenSetup::OPTIONAL_FEATURES,
        false,
        Arm::of(room::View::Main),
    );
    (image, paths)
}

/// [`draw`] opening the device with `optional_features` instead of
/// [`OffscreenSetup::OPTIONAL_FEATURES`], and naming the adapter it opened.
///
/// Everything below the renderer — the offscreen surface, the adapter pin, the
/// ring, the barriers around the readback and the row unpadding — is
/// [`OffscreenSetup`]'s, reached through
/// [`OffscreenSetup::open_forward_with`](crcbl::screenshot::OffscreenSetup::open_forward_with).
/// A sample rebuilding that for itself is exactly what
/// `docs/plan/sample/00-samples-overview.md` rule 1 forbids.
///
/// The adapter comes back because two arms are only a comparison if they opened
/// the same one, and that is a claim the caller has to make.
fn draw_with(
    extent: (u32, u32),
    effects: RenderEffects,
    optional_features: Features,
) -> (Image, String, AdapterInfo) {
    draw_with_probes(
        extent,
        effects,
        optional_features,
        false,
        Arm::of(room::View::Main),
    )
}

/// [`draw_with`] with every authored probe row replaced by [`GpuProbe::ZERO`]
/// while retaining the same probe-grid volume and table capacity.
fn draw_with_probes(
    extent: (u32, u32),
    effects: RenderEffects,
    optional_features: Features,
    zero_probes: bool,
    arm: Arm,
) -> (Image, String, AdapterInfo) {
    // A logger before anything opens: without one, every line a backend emits on
    // the way to a device goes nowhere, and a failure inside `open` names the
    // call that noticed rather than the one that caused it.
    crcbl::core::log::init_logging();

    let mut setup = OffscreenSetup::open_forward_with(
        extent.0,
        extent.1,
        optional_features,
        |device, queue, format| {
            Ok(ForwardScene {
                camera: arm.view.camera(),
                sun: room::sun(),
                renderer: Box::new(build(device, queue, format, effects, zero_probes, arm)?),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for lantern's room: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    // Printed unconditionally and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "lantern golden: device on adapter {id} {name:?} type={kind:?}",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
    );
    let paths = format!(
        "{backend} {:?} / {:?} / {:?}",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );
    eprintln!(
        "lantern golden: {paths} at {}x{}, asked for {optional_features:?}",
        extent.0, extent.1,
    );
    // **The device landed on exactly the path its request names.** The frame
    // alone cannot say — every path draws this room identically by construction
    // — so an arm on a tail other than the one it asked for would leave every
    // assertion below still passing. Met against the adapter, so on the default
    // request this is the claim it has always made, "the best path the adapter
    // offers"; on a forced one it is the lesser path, and an arm that got the
    // better tail anyway is a self-comparison wearing a cross-path label.
    let granted = optional_features.intersection(adapter.caps.features);
    assert_eq!(
        (caps.geometry_path(), caps.binding_model()),
        (
            GeometryPath::from_features(granted),
            BindingModel::from_features(granted),
        ),
        "adapter {} offers {:?}, this run asked for {optional_features:?}, and the device \
         opened on {:?} / {:?}",
        adapter.name,
        adapter.caps.features,
        caps.geometry_path(),
        caps.binding_model(),
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else.
    setup.finish().expect("the device reaches idle");

    assert_eq!(
        (width, height),
        extent,
        "the swapchain handed back an extent nothing was measured at"
    );
    let order = if format == Format::Bgra8UnormSrgb || format == Format::Bgra8Unorm {
        ChannelOrder::Bgra
    } else {
        ChannelOrder::Rgba
    };
    let image = Image::from_readback(width, height, &pixels, order)
        .expect("the readback is exactly one image");
    (image, paths, adapter)
}

/// The room, made resident and placed, on a device the caller opened, drawing
/// `effects`.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    effects: RenderEffects,
    zero_probes: bool,
    arm: Arm,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let mut scene = room::room();
    if zero_probes {
        // **The updater off and the rows zeroed**, which is what "zero probes"
        // has meant since `crcbl_lantern::bounce` stopped baking them: the rows
        // it ships are already zero, so filling them again is a control that
        // controls nothing — the gather would overwrite both arms with the same
        // light. `ProbeUpdate::Authored` is what leaves the table holding those
        // zeroes, which is the same binding and the same lookup shape with only
        // the radiance removed.
        scene.probes.probes.fill(GpuProbe::ZERO);
        scene.probes.update = crcbl::render::ProbeUpdate::Authored;
    }
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // The **programmatic** layer of topic 39's resolution order, which is the
    // one a test has any business driving — see `crcbl::render::effects`.
    renderer.set_effect_request(EffectRequest {
        // The **camera** layer is the view's, and the **programmatic** one is
        // this test's — see `crcbl::render::effects`. Two views resolving to two
        // effect sets is the whole of what `room::View::stack` is for, so a test
        // that wrote one layer for both would be exercising one of them.
        camera: arm.stack,
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(effects), Some(false)),
        ..EffectRequest::default()
    });
    // `crate::gpu` sets the same air on both of its renderers, and a golden
    // drawn without it would be a picture of a different room — see `Arm::fog`
    // for the one arm that takes it out on purpose.
    renderer.set_fog(arm.fog);
    if let Err(error) = room::place(device, queue, &mut renderer, arm.view) {
        renderer.destroy(device);
        return Err(crcbl::screenshot::OffscreenError::Hal(
            crcbl::hal::HalError::InvalidDescriptor(format!(
                "lantern's room does not fit its own instance pool: {error}"
            )),
        ));
    }
    // **`t = 0`, like every other screenshot in the tree.** The lamp's orbit is
    // a pure function of the time, so a golden is only worth comparing at a time
    // both runs agree on — and zero is the one the app's own first frame draws.
    // The corner downlight stands still and is in the list at every time.
    renderer.set_lights(&room::lights(0.0));
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in the frame, in pixels.
///
/// Through the very same [`Camera::view_projection`] the frame was drawn with,
/// so a claim about a surface is a claim about the pixels that surface actually
/// covers — rather than about a hand-derived mapping that a change of camera
/// would silently invalidate.
///
/// The projection produces **Y-up** normalised device coordinates with depth in
/// `0..1`; the framebuffer's rows run the other way, which is the flip below.
fn project(camera: &Camera, extent: (u32, u32), point: Vec3) -> (u32, u32) {
    #[allow(clippy::cast_precision_loss)]
    let aspect = extent.0 as f32 / extent.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    assert!(
        clip.w > 0.0,
        "{point:?} is behind the camera, so nothing in the frame is about it"
    );
    let ndc = clip.truncate() / clip.w;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let x = (ndc.x + 1.0) * 0.5 * width;
    let y = (1.0 - ndc.y) * 0.5 * height;
    assert!(
        x >= 0.0 && x < width && y >= 0.0 && y < height,
        "{point:?} projects to ({x:.1}, {y:.1}), outside a {width}x{height} frame — \
         the claim about it would be about a pixel that is not there"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x as u32, y as u32)
}

/// Mean luminance of a block of pixels around `centre`, out of 255.
///
/// Unweighted across the three channels: what every claim here is about is how
/// much light reached a surface, and a perceptual weighting would make the
/// coloured wall's red count for a third of what the plaster's white does.
fn brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image
                .pixel(x, y)
                .unwrap_or_else(|| panic!("({x}, {y}) is inside the frame"));
            total += (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// The mean of one channel over the same block.
fn channel(image: &Image, centre: (u32, u32), half: (u32, u32), index: usize) -> f32 {
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image
                .pixel(x, y)
                .unwrap_or_else(|| panic!("({x}, {y}) is inside the frame"));
            total += f32::from(pixel[index]);
            count += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// [`CORNER_BLOCK`] at the extent a caller's `block` was scaled to.
///
/// [`inspect`] is handed the block its caller measured at, so the ratio between
/// that and [`BLOCK`] is the extent's own scale — one at [`EXTENT`], five at
/// [`REVIEW_EXTENT`]. Deriving it rather than passing a second block is what
/// makes the corner's blocks the same patch of the room at both.
fn corner_block(block: (u32, u32)) -> (u32, u32) {
    let scale = (block.0 / BLOCK.0).max(1);
    (CORNER_BLOCK.0 * scale, CORNER_BLOCK.1 * scale)
}

/// Every claim this suite makes about the lighting, at whatever extent.
///
/// One function so the golden's extent and the review extent assert the *same*
/// things: a structural claim that only held at 256×192 would be a claim about
/// the rasteriser's sampling rather than about the room.
fn inspect(image: &Image, extent: (u32, u32), block: (u32, u32)) {
    let camera = room::fixed_camera();
    let colors = image.distinct_colors(MIN_COLORS);
    assert!(
        colors >= MIN_COLORS,
        "a room with {colors} distinct colour(s) (counted to {MIN_COLORS}) is not \
         evidence — nothing drew, or only the clear did"
    );

    // ---- 1. the sun comes through the window --------------------------------
    let sunlit = brightness(image, project(&camera, extent, SUNLIT), block);
    let shaded = brightness(image, project(&camera, extent, SHADED), block);
    assert!(
        sunlit > LIT_FLOOR,
        "the sunlit floor is at {sunlit:.1}/255, which is not a lit surface at all"
    );
    assert!(
        sunlit > shaded * SHAFT_RATIO,
        "the floor inside the window's shaft is {sunlit:.1} and the floor outside it is \
         {shaded:.1} — the sun is not coming through the opening, or the wall casts no \
         shadow at all"
    );

    // ---- 2. the shaded floor is ambient rather than black -------------------
    //
    // The other half of claim 1, and the one that stops it being satisfied by a
    // frame that is simply dark: `sunlit > shaded * SHAFT_RATIO` holds for
    // `shaded == 0`, which is what a lost ambient term looks like. Shown red by
    // zeroing `room::sun`'s ambient, which takes this block to exactly nothing.
    assert!(
        shaded > LIT_FLOOR,
        "the shaded floor is at {shaded:.1}/255, so the ambient term reached nothing"
    );

    // ---- 3. a conductor reflects the probe environment on an SSR miss --------
    let mirror = brightness(image, project(&camera, extent, MIRROR_AT), block);
    // Read again by claim 6 below, which is the far half of the bounce pair.
    let at_plaster = project(&camera, extent, PLASTER_AT);
    let plaster = brightness(image, at_plaster, block);
    assert!(
        plaster > LIT_FLOOR,
        "the plaster wall is at {plaster:.1}/255, so there is nothing to compare the \
         mirror against"
    );
    assert!(
        mirror > LIT_FLOOR && mirror > plaster * MIRROR_FRACTION_OF_PLASTER,
        "the SSR-miss point is {mirror:.1} and nearby plaster is {plaster:.1} — the probe \
         fallback is absent or the probe buffer was not bound"
    );

    // ---- 4. what a conductor owes the room instead is a reflection ----------
    //
    // The block hangs directly **above** the panel's bottom edge rather than
    // being centred on a point: the band that reflects is the lowest eighth of
    // the face, so a block centred in it would have to be a different size from
    // every other claim's, and one centred on the edge would take in the plinth
    // below. Sitting it on the edge keeps the caller's own block and puts every
    // reflecting row of the face inside it.
    let foot = project(&camera, extent, room::MIRROR_FOOT);
    let reflecting = brightness(image, (foot.0, foot.1 - block.1), block);
    assert!(
        reflecting > LIT_FLOOR,
        "the foot of the mirror panel is at {reflecting:.1}/255 — a conductor with no \
         ambient and no reflection is black, and this one still is"
    );
    assert!(
        reflecting > mirror * MIRROR_GRADIENT,
        "the mirror panel reads {reflecting:.1} at its foot and {mirror:.1} further up the \
         same face — same row, same normal, same F0, same roughness and no direct light on \
         either, so this says the reflection is not following the geometry"
    );

    // ---- 5. the material table's colour factor reached the device -----------
    let at = project(&camera, extent, BOUNCE_AT);
    let red = channel(image, at, block, 0);
    let green = channel(image, at, block, 1);
    let blue = channel(image, at, block, 2);
    let sum = red + green + blue;
    assert!(
        sum > 3.0 * LIT_FLOOR,
        "the coloured wall is at {:.1}/255, so nothing lit it",
        sum / 3.0
    );
    assert!(
        red / sum > BOUNCE_REDNESS,
        "the coloured wall reads {red:.1} / {green:.1} / {blue:.1} — its material row's \
         base-colour factor did not reach the fragment stage"
    );

    // ---- 6. the downlight lights a corner and the post stands in it ---------
    //
    // Two blocks of the `-x` wall's inner face, both inside the cone, one with
    // the corner post between it and the fitting. Neither is in the sun — that
    // face is back-facing to it — and neither is in the lamp's reach at `t = 0`,
    // so the whole of what separates them is the post. This is the half a
    // single frame can carry, which is why it is here rather than only in
    // `every_effect_toggles_and_the_frame_says_so`: it runs at both extents and
    // on both capability arms, and it goes red with the post unplaced — see
    // `SPOT_SHADOW_RATIO`, whose value was set from a run with it unplaced
    // rather than from the shipped one alone.
    let corner = corner_block(block);
    let pool = brightness(image, project(&camera, extent, room::SPOT_LIT), corner);
    let shadowed = brightness(image, project(&camera, extent, room::SPOT_SHADOWED), corner);
    eprintln!("lantern corner: downlight pool {pool:.1}, post's shadow {shadowed:.1}");
    assert!(
        pool > LIT_FLOOR,
        "the downlight's pool is at {pool:.1}/255 — the third light reached nothing, so \
         there is no cone in this frame to cast a shadow in"
    );
    assert!(
        pool > shadowed * SPOT_SHADOW_RATIO,
        "the downlight's pool reads {pool:.1} and the band the corner post shadows reads \
         {shadowed:.1} on the same wall — the post is not in the cone, or the cone is not \
         where it is aimed"
    );
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// **The room, from the fixed camera, against the checked-in golden.**
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_fixed_camera_draws_the_room_and_matches_its_golden() {
    let (image, paths) = draw(EXTENT, RenderEffects::all());
    inspect(&image, EXTENT, BLOCK);
    check_golden(&image, &paths);
}

/// **Zeroing authored probe rows removes fallbacks from SSR misses and rough
/// conductors, but preserves a screen-space hit.**
///
/// The zero control retains the authored volume and row count, so it exercises
/// the same binding and lookup shape while removing only the fallback radiance.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn zero_probes_only_remove_the_ssr_and_rough_fallbacks() {
    let effects = RenderEffects::all();
    let (authored, _, authored_adapter) =
        draw_with(EXTENT, effects, OffscreenSetup::OPTIONAL_FEATURES);
    let (zeroed, _, zeroed_adapter) = draw_with_probes(
        EXTENT,
        effects,
        OffscreenSetup::OPTIONAL_FEATURES,
        true,
        Arm::of(room::View::Main),
    );
    assert_eq!(
        authored_adapter, zeroed_adapter,
        "the authored and zero-probe frames opened different adapters, so they are not a control"
    );

    let camera = room::fixed_camera();
    let miss = project(&camera, EXTENT, MIRROR_AT);
    let foot = project(&camera, EXTENT, room::MIRROR_FOOT);
    let hit = (foot.0, foot.1.saturating_sub(1));
    let brass = project(&camera, EXTENT, room::BRASS_AT);
    let authored_miss = brightness(&authored, miss, BLOCK);
    let zeroed_miss = brightness(&zeroed, miss, BLOCK);
    let authored_hit = brightness(&authored, hit, (1, 1));
    let zeroed_hit = brightness(&zeroed, hit, (1, 1));
    let authored_brass = brightness(&authored, brass, BLOCK);
    let zeroed_brass = brightness(&zeroed, brass, BLOCK);
    eprintln!(
        "lantern probes: SSR miss {authored_miss:.1} -> {zeroed_miss:.1}, \
         SSR hit {authored_hit:.1} -> {zeroed_hit:.1}, \
         rough brass {authored_brass:.1} -> {zeroed_brass:.1}"
    );
    assert!(
        authored_miss > LIT_FLOOR && zeroed_miss <= 1.0,
        "the SSR miss reads {authored_miss:.1} with authored probes and {zeroed_miss:.1} with \
         zero rows — the fallback must be the only light removed"
    );
    assert!(
        authored_brass > LIT_FLOOR && authored_brass > zeroed_brass * BRASS_PROBE_RATIO,
        "the rough brass reads {authored_brass:.1} with authored probes and {zeroed_brass:.1} with \
         zero rows — it must be lit and retain at least a {BRASS_PROBE_RATIO:.2} probe ratio"
    );
    let hit_tolerance_percent = SSR_HIT_TOLERANCE * 100.0;
    assert!(
        (authored_hit - zeroed_hit).abs() < authored_hit * SSR_HIT_TOLERANCE,
        "the real SSR hit moved from {authored_hit:.1} to {zeroed_hit:.1}; the measured \
         {hit_tolerance_percent:.0}% tolerance covers the remaining probe blend but not \
         replacing a valid screen hit with the fallback"
    );
}

/// **The same room on the path below this device's own, against the same
/// golden.**
///
/// `docs/plan/sample/00-samples-overview.md` rule 12 asks each sample's CI run
/// to exercise "the path its runner selects plus one below it". Every other test
/// here opens through [`draw`], which asks for
/// [`OffscreenSetup::OPTIONAL_FEATURES`] — so without this one every frame the
/// suite draws comes off the best tail the adapter reports, and the lesser ones,
/// which is what browsers and Apple devices run, are code no run here executes.
/// The sample already *said* which path it took; this is what makes it take more
/// than one.
///
/// # One golden, both arms
///
/// Both frames are held to `tests/golden/room.png` rather than each to a
/// reference of its own. `docs/plan/03-gpu-driven-rendering.md` §3.5's design
/// rule is that a lesser path is a constraint on data layout and not a separate
/// renderer, so a difference between the arms is a **bug in the better path** —
/// and a second reference is exactly what would bless it.
///
/// # It cannot pass vacuously
///
/// Two frames drawn by the same code match perfectly, so the arms have to have
/// actually differed. Both are asserted to open the same adapter, and the
/// selectors they resolve to are asserted to differ **exactly when that adapter
/// offers one of the flags [`BELOW`] withholds**. A device already at the floor
/// of both axes — a software rasteriser, for instance — is a legitimate run of
/// this test, and the printed line and that assertion are what keep it from
/// being a silent one.
///
/// That pair is each other's alibi, though: a [`below_features`] withholding
/// *nothing* leaves both halves false on every machine, and the comparison then
/// holds trivially — which is what it did the first time it was broken on
/// purpose to watch it fail. So the withheld set is asserted non-empty first,
/// which is the claim that the second arm is a second arm at all.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_room_draws_the_same_on_a_path_below_the_devices_own() {
    let below = below_features();
    // Only the flags a *selector* reads decide whether the arms can differ:
    // `TASK_SHADER` comes out beside `MESH_SHADER` because it is an
    // amplification stage in front of one, and no selector reads it — see
    // `GeometryPath::INPUTS`, which is the table `downgrades` answers from.
    let withheld = OffscreenSetup::OPTIONAL_FEATURES
        .difference(below)
        .intersection(GeometryPath::INPUTS.union(BindingModel::INPUTS));
    // **Before either device opens**, and before the two claims below, which are
    // each other's alibi otherwise: an empty set here makes "the arms differ" and
    // "the adapter offers something better" both false on every machine, and the
    // test would then compare a frame against itself and report a lesser path
    // exercised.
    assert!(
        !withheld.is_empty(),
        "the lesser arm asks for {below:?}, which withholds no selector input at all — it is \
         the same request as the arm it is compared against"
    );

    let (best, best_paths, adapter) = draw_with(
        EXTENT,
        RenderEffects::all(),
        OffscreenSetup::OPTIONAL_FEATURES,
    );
    let (lesser, lesser_paths, lesser_adapter) = draw_with(EXTENT, RenderEffects::all(), below);
    assert_eq!(
        adapter, lesser_adapter,
        "the two arms opened different adapters, so they are not a comparison"
    );

    let offers_better = adapter.caps.features.intersects(withheld);
    eprintln!(
        "lantern golden: {best_paths} against {lesser_paths} — withheld {withheld:?}, \
         adapter {name} has {held:?}",
        name = adapter.name,
        held = adapter.caps.features.intersection(withheld),
    );
    assert_eq!(
        best_paths != lesser_paths,
        offers_better,
        "the adapter {} one of {withheld:?} and the two arms resolved to {best_paths} and \
         {lesser_paths} — one of those two facts is wrong, and a self-comparison that reads \
         as a cross-path one is worse than no test",
        if offers_better {
            "offers"
        } else {
            "offers none of"
        },
    );

    // Every claim, on every arm: the golden is a comparison of pixels and the
    // claims in front of it are what say the frame holds the room at all, so an
    // arm that lost a mesh or a material row on the way down its lesser tail
    // fails on the claim rather than on a diff nobody can read.
    for (image, paths) in [(&best, &best_paths), (&lesser, &lesser_paths)] {
        inspect(image, EXTENT, BLOCK);
        check_golden(image, paths);
    }
}

/// Holds one frame to the checked-in reference, and says what it found.
///
/// `label` is the arm's [`draw_with`] path row, so a failure names the tail the
/// frame came off rather than only the frame.
fn check_golden(image: &Image, label: &str) {
    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/room.png");
    let comparison = Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("on {label}: {message}"));
    eprintln!("lantern golden: room on {label} — {}", comparison.summary());
}

/// **The same claims at twenty-five times the pixels**, written where a human
/// can look at it.
///
/// Two jobs, and the first is the one that makes this a test. Every ratio in
/// [`inspect`] is measured over a block a few pixels across, and at 256×192 a
/// block is a large fraction of the surface it sits on — so a claim that
/// happened to hold because of where one triangle edge landed would hold at that
/// extent and nowhere else. Re-running them at [`REVIEW_EXTENT`], where the same
/// world positions cover twenty-five times the area, is what distinguishes a
/// measurement of the room from a measurement of the sampling.
///
/// The second is that the frame is saved: a lighting fixture is a thing people
/// look at, and 256×192 is not a size anybody can judge a shadow's edge at.
/// Nothing is blessed — there is no reference at this extent — so the picture is
/// an artefact of the run rather than a gate.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_room_reads_the_same_at_presentation_size() {
    let image = review(RenderEffects::all(), "fixed-camera");

    // The blocks grow with the frame, so each covers the same patch of the room
    // rather than a twenty-fifth of it.
    inspect(&image, REVIEW_EXTENT, review_block());
}

/// **Switching an effect off changes the frame where that effect darkens it, and
/// leaves the rest of the room alone.**
///
/// The charter's "every effect toggles independently", as the one thing a
/// picture can actually say about it. `crcbl-render`'s own tests already show the
/// toggle reaches the recorded pass list; what they cannot show is that the
/// passes it removed were the ones doing the darkening — a switch wired to the
/// wrong effect, or an occlusion placeholder that is not white, produces a frame
/// with the right passes in it and the wrong picture.
///
/// Each claim is therefore **a pair of blocks over a pair of frames**: one block
/// where the effect works and one where it does not, so a frame that simply got
/// brighter — a lost tonemap, a different exposure — fails the control half.
///
/// Nothing here is blessed. One frame per state is written for a reviewer, at
/// the size a shadow's edge can be judged at.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn every_effect_toggles_and_the_frame_says_so() {
    let block = review_block();
    let camera = room::fixed_camera();
    let at = |point: Vec3| project(&camera, REVIEW_EXTENT, point);

    let all_on = review(RenderEffects::all(), "all-effects");
    let no_shadows = review(
        RenderEffects::all().difference(RenderEffects::SHADOWS),
        "no-shadows",
    );
    let no_ao = review(
        RenderEffects::all().difference(RenderEffects::AMBIENT_OCCLUSION),
        "no-ao",
    );
    let no_reflections = review(
        RenderEffects::all().difference(RenderEffects::REFLECTIONS),
        "no-reflections",
    );

    // ---- shadows -----------------------------------------------------------
    //
    // `SHADED_FLOOR` is floor in the window wall's own shadow and `SUNLIT_FLOOR`
    // is floor inside the shaft: with no shadow atlas the first one takes the
    // sun it was being denied and the second one, already lit, does not move.
    let shaded_on = brightness(&all_on, at(SHADED), block);
    let shaded_off = brightness(&no_shadows, at(SHADED), block);
    let sunlit_on = brightness(&all_on, at(SUNLIT), block);
    let sunlit_off = brightness(&no_shadows, at(SUNLIT), block);
    eprintln!(
        "lantern toggles: shadowed floor {shaded_on:.1} -> {shaded_off:.1}, \
         sunlit floor {sunlit_on:.1} -> {sunlit_off:.1}"
    );
    assert!(
        shaded_off > shaded_on * SHADOW_LIFT,
        "the shadowed floor reads {shaded_on:.1} with shadows and {shaded_off:.1} without — \
         switching the atlas off did not stop the wall occluding the sun"
    );
    assert!(
        (sunlit_off - sunlit_on).abs() < sunlit_on * UNCHANGED,
        "the sunlit floor moved from {sunlit_on:.1} to {sunlit_off:.1} — a floor already in \
         full sun has no shadow to lose, so this is the whole frame changing rather than the \
         shadows"
    );

    // ---- the downlight's own shadow ----------------------------------------
    //
    // The `-x` wall's inner face is back-facing to the sun and out of the lamp's
    // reach at `t = 0` — `room::the_downlight_casts_a_shadow_of_the_corner_post`
    // is what proves both with no GPU — so the downlight is the whole of the
    // direct light on it and switching the atlas off can only add back what the
    // corner post was blocking. The control is the same wall in the same pool
    // with nothing between it and the fitting: it was never shadowed, so it
    // cannot move.
    let corner = corner_block(block);
    let post_on = brightness(&all_on, at(room::SPOT_SHADOWED), corner);
    let post_off = brightness(&no_shadows, at(room::SPOT_SHADOWED), corner);
    let pool_on = brightness(&all_on, at(room::SPOT_LIT), corner);
    let pool_off = brightness(&no_shadows, at(room::SPOT_LIT), corner);
    eprintln!(
        "lantern toggles: post's shadow {post_on:.1} -> {post_off:.1}, \
         downlight pool {pool_on:.1} -> {pool_off:.1}"
    );
    assert!(
        post_off > post_on * SPOT_SHADOW_LIFT,
        "the band the corner post shadows reads {post_on:.1} with shadows and \
         {post_off:.1} without — the downlight holds a tile in the atlas and nothing in \
         its cone is being occluded by it"
    );
    assert!(
        pool_on > LIT_FLOOR && (pool_off - pool_on).abs() < pool_on * UNCHANGED,
        "the downlight's pool moved from {pool_on:.1} to {pool_off:.1} — a patch of that \
         cone with nothing between it and the fitting has no shadow to lose, so this is \
         the whole frame changing rather than the spot's own map"
    );

    // ---- ambient occlusion -------------------------------------------------
    //
    // Two blocks of the same floor, same material, same normal, both out of the
    // sun and out of the lamp's reach: one in the plinth's contact corner and
    // one out in the open. Occlusion is the only term that separates them, so
    // switching it off has to lift the first and leave the second where it is.
    let corner_on = brightness(&all_on, at(AO_CORNER), block);
    let corner_off = brightness(&no_ao, at(AO_CORNER), block);
    let open_on = brightness(&all_on, at(SHADED), block);
    let open_off = brightness(&no_ao, at(SHADED), block);
    eprintln!(
        "lantern toggles: contact corner {corner_on:.1} -> {corner_off:.1}, \
         open floor {open_on:.1} -> {open_off:.1}"
    );
    assert!(
        corner_off > corner_on * AO_LIFT,
        "the plinth's contact corner reads {corner_on:.1} with occlusion and {corner_off:.1} \
         without — the occlusion pass is not darkening the corner, or the placeholder bound in \
         its place is not white"
    );
    assert!(
        (open_off - open_on).abs() < open_on * UNCHANGED,
        "open floor moved from {open_on:.1} to {open_off:.1} — a surface with nothing within \
         the occlusion radius has no occlusion to lose"
    );

    // ---- reflections -------------------------------------------------------
    //
    // The two blocks the [`MIRROR_GRADIENT`] claim already reads: the foot of the
    // panel, whose reflected ray finds the floor while still on screen, and a
    // point further up the same face whose ray finds nothing. With the march off
    // the first one has to lose what it was gaining and the second one, which was
    // gaining nothing, cannot move at all.
    //
    // The control is the strong half here. A conductor with no ambient is near
    // black, so "the foot got darker" is satisfied by a frame that went dark
    // everywhere; a face that reflected nothing either way and stayed exactly put
    // is what says the pass came out and nothing else did.
    let foot = at(room::MIRROR_FOOT);
    let foot_block = (foot.0, foot.1 - block.1);
    let foot_on = brightness(&all_on, foot_block, block);
    let foot_off = brightness(&no_reflections, foot_block, block);
    let missing_on = brightness(&all_on, at(MIRROR_AT), block);
    let missing_off = brightness(&no_reflections, at(MIRROR_AT), block);
    eprintln!(
        "lantern toggles: mirror foot {foot_on:.1} -> {foot_off:.1}, \
         mirror face {missing_on:.1} -> {missing_off:.1}"
    );
    assert!(
        foot_off * MIRROR_GRADIENT < foot_on,
        "the mirror panel's foot reads {foot_on:.1} with the march and {foot_off:.1} without — \
         a conductor with no ambient and no reflection is black, and this one is not"
    );
    assert!(
        missing_on > LIT_FLOOR && missing_off <= 1.0,
        "the SSR-miss point reads {missing_on:.1} with reflections and {missing_off:.1} without — \
         the probe fallback was not supplied by the reflection pass"
    );
}

/// How much brighter a block whose column is sunlit air has to get when the
/// medium is allowed to scatter the sun.
///
/// Measured at [`room::SPOT_SHADOWED`] on radv with [`AIR`]: 67.20 with
/// [`Fog::sun_scattering`] zeroed and 73.55 with it at one, which is 9.4%. Five
/// per cent is under half of that and still an order above the control below,
/// which reads 0.00% at the same two blocks.
const SHAFT_LIFT: f32 = 1.05;

/// **The froxel column puts the sun into the air the cascades let it reach, and
/// into no other air.**
///
/// The one rendered claim about `crcbl_render::volumetric` in this tree, and the
/// pair is what makes it one. Both arms draw the room in [`AIR`] through
/// [`room::View::Main`]'s stack, which asks for
/// [`RenderEffects::VOLUMETRIC_FOG`]; they differ in
/// [`Fog::sun_scattering`] and in nothing else, so absorption, the environment
/// term and every surface in the room are identical between them and the only
/// thing that can move a block is the scatter pass's own output.
///
/// * The **claim** block is [`room::SPOT_SHADOWED`], low on the window wall.
///   Its surface is in the corner post's shadow, so what the medium in front of
///   it gains cannot be a change in how that surface is lit.
/// * The **control** block is [`room::TINTED_PLASTER`], high on the back wall
///   beyond the window's throw. Its whole column is out of the sun, so a scatter
///   pass that ignored the visibility lane of `lighting[froxel]` — the
///   sabotage `docs/backlog.md` records as leaving all 28 `mesh_e2e` tests
///   green to the digit — lights it too. Measured here at exactly 0.00%.
///
/// Nothing is blessed: the room this draws is not the room this suite ships, and
/// a golden of it would be a second picture to keep in step for no claim the
/// pair does not already make. One frame per arm is written for a reviewer.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_air_scatters_the_sun_where_the_cascades_let_it_through() {
    let block = review_block();
    let camera = room::fixed_camera();
    let at = |point: Vec3| project(&camera, REVIEW_EXTENT, point);
    let arm = Arm::of(room::View::Main);
    let lit = review_in(RenderEffects::all(), arm.with_air(), "air-scattering");
    let flat = review_in(RenderEffects::all(), arm.without_shafts(), "air-absorbing");

    let shaft = at(room::SPOT_SHADOWED);
    let (shaft_off, shaft_on) = (
        brightness(&flat, shaft, block),
        brightness(&lit, shaft, block),
    );
    let shut = at(room::TINTED_PLASTER);
    let (shut_off, shut_on) = (
        brightness(&flat, shut, block),
        brightness(&lit, shut, block),
    );
    eprintln!(
        "lantern shaft: sunlit column {shaft_off:.2} -> {shaft_on:.2}, \
         shadowed column {shut_off:.2} -> {shut_on:.2}"
    );

    assert!(
        shaft_on > shaft_off * SHAFT_LIFT,
        "the air in front of the window wall reads {shaft_off:.2} with the sun's scattering off \
         and {shaft_on:.2} with it on — the scatter pass put nothing in the column the sun \
         reaches"
    );
    assert!(
        (shut_on - shut_off).abs() < shut_off * UNCHANGED,
        "the air beyond the window's throw reads {shut_off:.2} with the sun's scattering off and \
         {shut_on:.2} with it on — a froxel the cascades leave in shadow is being lit anyway, so \
         the scatter pass is not reading its own visibility"
    );
}

/// The extent the monitor's own view is inspected at.
///
/// Square, because [`room::MONITOR_EXTENT`] is — the page carries one extent for
/// every layer — and large enough that the block the claim below reads is a
/// patch of the room rather than a handful of texels. Nothing is blessed at this
/// size; the picture that *is* blessed is the room with the screen in it.
const MONITOR_REVIEW: (u32, u32) = (768, 768);

/// One frame of an arm, at `extent`, written where a reviewer can open it.
fn arm_frame(
    extent: (u32, u32),
    effects: RenderEffects,
    arm: Arm,
    name: &str,
) -> (Image, AdapterInfo) {
    let (image, paths, adapter) = draw_with_probes(
        extent,
        effects,
        OffscreenSetup::OPTIONAL_FEATURES,
        false,
        arm,
    );
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!("{name}-{}x{}.png", extent.0, extent.1));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("lantern golden: {paths} {name} frame at {}", path.display());
    (image, adapter)
}

/// **The camera stack is the only thing between the monitor's two frames**, and
/// it is legible in pixels.
///
/// `docs/plan/39-capabilities.md`'s first layer, rendered twice: the same view of
/// the same room on the same device at the same instant, with
/// [`EffectRequest::camera`](crcbl::render::EffectRequest::camera) the one field
/// that differs — [`room::MONITOR_STACK`] in the arm the sample ships, and every
/// effect in the arm beside it. Everything below that layer is identical, so a
/// difference between the two frames is the layer and can be nothing else.
///
/// **The read point is [`room::BRASS_BACK`]**, and its choice is the whole of
/// what makes this a claim rather than a brightness comparison: the block's far
/// face is fully metallic, so it has no diffuse albedo for the ambient to scale;
/// it faces away from the sun, so it has no direct sun on it either; and only the
/// monitor's camera can see it at all. What is left is the lamp's specular and
/// the environment the reflection pass supplies, so dropping
/// [`RenderEffects::REFLECTIONS`] from the stack takes most of it away.
///
/// The control is the sunlit floor in the same pair of frames, which the
/// reflection pass contributes almost nothing to: a frame that merely went dark
/// everywhere fails there.
///
/// **This test goes red if the camera layer is dropped from
/// `EffectRequest::resolve`.** With `self.camera` out of that expression the two
/// arms resolve to the same effect set, both frames are the same picture, and the
/// ratio below is one.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_camera_stack_is_the_only_thing_between_the_monitors_two_frames() {
    let effects = RenderEffects::all();
    let shipped = Arm::of(room::View::Monitor);
    // `DEFAULT_STACK` and not `all()`: the monitor's stack is that constant with
    // the reflections taken out — see `room::MONITOR_STACK` — so this is the one
    // arm that puts them back and changes nothing else. `all()` here would also
    // switch the bloom chain on, which no view in this sample asks for, and the
    // control block below would then be measuring the whole frame getting
    // brighter rather than the camera layer.
    let reflecting = shipped.with_stack(RenderEffects::DEFAULT_STACK);
    assert_ne!(
        shipped.stack, reflecting.stack,
        "two arms that ask for the same effects are not a comparison"
    );

    let (dropped, dropped_adapter) =
        arm_frame(MONITOR_REVIEW, effects, shipped, "monitor-camera-stack");
    let (kept, kept_adapter) = arm_frame(
        MONITOR_REVIEW,
        effects,
        reflecting,
        "monitor-camera-reflections",
    );
    assert_eq!(
        dropped_adapter, kept_adapter,
        "the two arms opened different adapters, so they are not a comparison"
    );

    let camera = room::monitor_camera();
    let block = (
        BLOCK.0 * MONITOR_REVIEW.0 / EXTENT.0,
        BLOCK.1 * MONITOR_REVIEW.1 / EXTENT.1,
    );
    let brass = project(&camera, MONITOR_REVIEW, room::BRASS_BACK);
    let control = project(&camera, MONITOR_REVIEW, FRONT_WALL_AT);
    let brass_dropped = brightness(&dropped, brass, block);
    let brass_kept = brightness(&kept, brass, block);
    let control_dropped = brightness(&dropped, control, block);
    let control_kept = brightness(&kept, control, block);
    eprintln!(
        "lantern monitor: brass far face {brass_kept:.1} -> {brass_dropped:.1}, \
         front wall {control_kept:.1} -> {control_dropped:.1}"
    );
    assert!(
        brass_kept > LIT_FLOOR,
        "the block's far face reads {brass_kept:.1} with the reflections in the monitor's \
         stack — nothing drew, or the face is not where this claim is aimed"
    );
    assert!(
        brass_dropped <= MONITOR_UNLIT,
        "the block's far face reads {brass_dropped:.1} with reflections out of the \
         monitor's camera stack — a conductor with no sun, no diffuse and no ambient has \
         nothing else to be lit by, so the camera layer did not reach the frame"
    );
    assert!(
        (control_dropped - control_kept).abs() < control_kept * UNCHANGED,
        "the front wall moved from {control_kept:.1} to {control_dropped:.1} — a rough \
         dielectric in full ambient has almost no reflection to lose, so this is the whole \
         frame changing rather than the camera stack"
    );
}

// ---------------------------------------------------------------------------
// The live monitor: the frame the binary presented
// ---------------------------------------------------------------------------

/// The extent the live golden is blessed at.
///
/// `crcbl::engine::DEFAULT_WINDOW_SIZE` at scale 1, which is what a headless run
/// renders at when `--size` says nothing — so the picture is the frame the
/// default invocation produces rather than one a flag had to ask for. Larger
/// than [`EXTENT`] because the subject is a screen a metre and a half across at
/// the far end of an eight-metre room, and at 256 × 192 it is a smudge.
const LIVE_EXTENT: (u32, u32) = (960, 720);

/// How many frames the live run presents before the one that gets written.
///
/// **More than one, and that is load-bearing.** The monitor's view is drawn and
/// copied onto the screen at the *tail* of each frame — `crcbl_lantern::gpu`'s
/// `feed_monitor` says why — so the screen shows the previous frame's picture and
/// a run of one frame would present a screen nothing had written yet. Sixteen is
/// past start-up and several times round the offscreen ring, and the lamp's orbit
/// is a pure function of the fixed-step clock, so the frame written is the same
/// on every machine.
const LIVE_FRAMES: u32 = 16;

/// The backend the run must be pinned to, from the environment.
fn required_backend() -> String {
    std::env::var(crcbl::backend::BACKEND_ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{} is not set, so nothing would pin the backend and a fallback would pass. \
             Run tests/run-lantern-golden.sh, which names one.",
            crcbl::backend::BACKEND_ENV_VAR
        )
    })
}

/// Runs the real binary with `--screenshot` and hands back the file it wrote.
///
/// **The one picture in this suite that comes off the binary rather than out of
/// this process**, and the monitor is why: `crcbl::screenshot::OffscreenSetup`
/// draws one view, and the second view — the copy that puts a picture on the
/// screen in the room — is recorded by `crcbl_lantern::gpu::Gpu::frame`. So the
/// composed frame, with a live screen in it, only exists in a run this binary
/// made. Every other sample's golden suite is already this shape;
/// `apps/hud/tests/golden.rs` is the one this follows.
///
/// **The stale file is removed first**, and its absence is what makes the
/// assertion below mean anything: a `--screenshot` that quietly did nothing would
/// otherwise pass on the previous run's picture forever.
fn live_frame(backend: &str) -> Image {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("room-live.png");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("could not clear {}: {error}", path.display()),
    }

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lantern"))
        .args([
            "--backend",
            backend,
            "--frames",
            &LIVE_FRAMES.to_string(),
            // Not because a headless run needs saying — `--screenshot` turns it
            // on — but because saying it is how this suite records that the
            // picture is of the offscreen ring and not of a window.
            "--headless",
            "--no-debug-overlay",
            "--screenshot",
        ])
        .arg(&path)
        .output()
        .expect("the lantern binary runs");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(
        output.status.code(),
        Some(0),
        "lantern exited {:?} on {backend}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    // The run has to have presented the whole budget: a screen fed at the tail
    // of a frame is a screen the *next* frame shows, so a run cut short is a run
    // whose monitor is still black.
    assert!(
        stdout.contains(&format!("{LIVE_FRAMES} frames")),
        "the summary does not say the run presented {LIVE_FRAMES} frames:\n{stdout}"
    );
    assert!(
        path.exists(),
        "lantern exited 0 and wrote no {} — `--screenshot` did nothing",
        path.display()
    );
    // Which adapter drew it, out of the binary's own log — the line
    // `tests/run-lantern-golden.sh` reads back.
    let adapter = stderr
        .lines()
        .find(|line| line.contains(" adapter \""))
        .unwrap_or_else(|| panic!("the run never said which adapter it opened:\n{stderr}"));
    eprintln!("lantern golden: device on {}", adapter.trim());

    let image = Image::load_png(&path).expect("the screenshot is a readable PNG");
    assert_eq!(
        (image.width(), image.height()),
        LIVE_EXTENT,
        "the binary wrote a {}x{} frame, which is not the extent the live golden was \
         blessed at",
        image.width(),
        image.height()
    );
    image
}

/// **The screen in the room is showing the room**, in a frame the binary
/// presented, against a golden of its own.
///
/// The charter's "a second render-to-texture camera driving an in-scene
/// monitor", as the one thing a picture can say about it. The two blocks are
/// **not** placed by hand in the middle of the screen: each is a world point
/// projected through [`room::monitor_camera`] onto the screen quad by
/// [`room::screen_point`], and then projected again through the fixed camera —
/// so what is asserted is that *this* patch of the screen is showing *that* part
/// of the room. A screen holding any flat fill fails the ratio, a screen nothing
/// wrote is [`crcbl_lantern::room`]'s opaque black and fails the floor, and a
/// frame pasted on upside down swaps the two and fails the ratio the other way.
///
/// The dark half is the block's far face, which is black in the monitor's picture
/// for the reason
/// [`the_camera_stack_is_the_only_thing_between_the_monitors_two_frames`]
/// measures: the monitor's camera stack asks for no reflections, and a conductor
/// out of the sun has nothing else. So this claim carries the camera layer into
/// the composed frame as well — see [`MONITOR_CONTRAST`], whose threshold was
/// set against a run with that layer removed rather than against the shipped one
/// alone.
///
/// [`the_camera_stack_is_the_only_thing_between_the_monitors_two_frames`]: fn@the_camera_stack_is_the_only_thing_between_the_monitors_two_frames
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-lantern-golden.sh"]
fn the_screen_in_the_room_shows_the_room_and_matches_its_golden() {
    let backend = required_backend();
    let image = live_frame(&backend);

    let camera = room::fixed_camera();
    // Smaller than [`BLOCK`]: the screen is a fraction of the frame and the two
    // blocks sit inside it, so a block sized for the floor would take the bezel
    // in with it.
    let block = (BLOCK.0 * LIVE_EXTENT.0 / EXTENT.0 / 4).max(1);
    let block = (block, block);
    let at = |point: Vec3| {
        let on_screen = room::screen_point(point)
            .unwrap_or_else(|| panic!("{point:?} is not in the monitor's own view"));
        project(&camera, LIVE_EXTENT, on_screen)
    };

    let bright = brightness(&image, at(MONITOR_BRIGHT_AT), block);
    let dark = brightness(&image, at(room::BRASS_BACK), block);
    eprintln!("lantern monitor: screen ceiling {bright:.1}, screen block face {dark:.1}");
    assert!(
        bright > LIT_FLOOR,
        "the part of the screen showing the ceiling reads {bright:.1} — the monitor is \
         black, so the render-to-texture view never reached the page"
    );
    assert!(
        bright > dark * MONITOR_CONTRAST,
        "the screen reads {bright:.1} where the monitor's camera puts the ceiling and \
         {dark:.1} where it puts the block's unlit far face — a screen showing a flat fill \
         reads the same in both, and one showing the picture upside down reads them the \
         other way round"
    );

    check_live_golden(&image, &backend);
}

/// The live frame against its own checked-in golden.
fn check_live_golden(image: &Image, backend: &str) {
    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/live.png");
    Golden::new(reference)
        .check(image)
        .expect("the live golden is readable")
        .into_result()
        .unwrap_or_else(|mismatch| panic!("on {backend}: {mismatch}"));
}

/// One frame at [`REVIEW_EXTENT`], written where a reviewer can open it.
///
/// **Written before any claim is checked**, on `Golden::check`'s terms: a run
/// that is about to fail is exactly the run somebody wants the picture from, and
/// a save after the assertions is a save a failure skips.
fn review(effects: RenderEffects, name: &str) -> Image {
    review_in(effects, Arm::of(room::View::Main), name)
}

/// [`review`] of an arm that is not the room's own — the air-free control.
fn review_in(effects: RenderEffects, arm: Arm, name: &str) -> Image {
    let (image, paths, _) = draw_with_probes(
        REVIEW_EXTENT,
        effects,
        OffscreenSetup::OPTIONAL_FEATURES,
        false,
        arm,
    );
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!(
        "{name}-{}x{}.png",
        REVIEW_EXTENT.0, REVIEW_EXTENT.1
    ));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("lantern golden: {paths} review frame at {}", path.display());
    image
}

/// The block every claim at [`REVIEW_EXTENT`] averages over.
///
/// [`BLOCK`] scaled by the extent ratio, so a block covers the same patch of the
/// room rather than a twenty-fifth of it.
fn review_block() -> (u32, u32) {
    let scale = REVIEW_EXTENT.0 / EXTENT.0;
    (BLOCK.0 * scale, BLOCK.1 * scale)
}
