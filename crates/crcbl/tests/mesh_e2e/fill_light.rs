//! `docs/plan/44-lighting.md`'s **fill flag on the two punctual kinds**: a point
//! light and a spot over the slab, each rendered twice and differing in `fill`
//! alone.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh fill_light
//! ```
//!
//! # Why this exists beside `area_light.rs`
//!
//! `crcbl_shaders::light::FLAG_FILL` is **kind-agnostic**: `Light::row` sets it
//! from `Light::is_fill`, which is one question asked of three variants, and
//! `mesh.slang` zeroes the specular term off the flag rather than off the kind.
//! So the flag has exactly one failure mode that a rectangle's frame cannot
//! show — a bit set for one kind and not the others, or a lobe dropped on one
//! arm of the shading and not the rest — and until this file existed the flag's
//! only GPU evidence was `area_light.rs`'s rectangle and `crcbl::screenshot`'s
//! `Scene::AreaLight`. The host side already covers the row itself:
//! `crcbl_render::light`'s
//! `a_fill_light_of_any_kind_carries_the_flag_into_its_row` writes each kind
//! twice and reads the bit back, and `crcbl_render::shadow`'s
//! `a_fill_light_is_refused_a_tile_where_the_same_light_takes_one` covers the
//! other half of the flag. Neither draws anything.
//!
//! # Why the comparison is two frames rather than two halves of one
//!
//! `crcbl::screenshot`'s `Scene::FillLight` draws the mirrored version — a point
//! pair and a spot pair over one floor, each half differing in that one field —
//! and `tests/render_e2e.rs` measures it. That frame is the picture and it
//! reaches every backend the render suite runs on. What it cannot be is
//! *exact*: the two halves of a mirror are equal by construction in every term
//! but one, and the one is the shadow, since a fill light is refused a tile and
//! its twin is handed one.
//!
//! Here the two frames are the same scene under the same light in the same
//! place, with one boolean changed. Every term is the same arithmetic over the
//! same inputs except the ones the flag touches, so the assertions below can be
//! about *bits* rather than about a ratio with a tolerance under it.
//!
//! # The scene
//!
//! `area_light.rs`'s slab, through its [`slab_frame`], because the surface a
//! lobe is measured on should not be a per-kind choice — and because that slab
//! already carries the tighter lobe of
//! `crcbl_render::scene::PYRAMID_ROUGHNESS`, which is what gives a highlight an
//! edge to lose.

use crcbl::math::Vec3;
use crcbl::render::{Light, PointLight, Projection, SpotLight};

use crate::area_light::{radiance_lost, slab_frame};
use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera};

/// How far above the slab each light here hangs.
///
/// **Chosen so the highlight lands on the slab and not off its edge.**
/// `mesh_scene`'s camera is oblique — it stands off-axis on two of three axes —
/// so a light on the slab's own axis puts its mirror image out toward the eye
/// rather than under itself, and this height is what decides how far. Low enough
/// that the image lands inside `SLAB_SCALE`'s half-width, and high enough that
/// the lobe is a highlight with an edge rather than a wash under the light.
const LIGHT_HEIGHT: f32 = 1.1;

/// How far each light's influence reaches from its own position.
///
/// Comfortably past the slab's far corner, on `area_light.rs`'s `STRIP_REACH`'s
/// terms: the quartic window `crcbl_render::PointLight::radius` documents is
/// then nowhere near its zero anywhere on the slab, so what shapes the frame is
/// the inverse square and the lobe rather than the radius.
const LIGHT_REACH: f32 = 12.0;

/// The colour each light here carries, intensity included.
///
/// Well above one for `area_light.rs`'s `STRIP_COLOR`'s reason — the scene
/// target is `Rgba16Float` and the tonemap is what brings it back down — and
/// near-white so that what the assertions read is a level rather than a hue.
/// Lower than that constant because a rectangle spreads its radiance over a face
/// and a point light does not.
const LIGHT_COLOR: Vec3 = Vec3::new(6.0, 5.7, 5.1);

/// Half-angle of the spot's bright core, in degrees.
///
/// Degrees rather than a radius on the slab — the convention
/// `crcbl::screenshot`'s `SPOT_CORE_RADIUS` uses — because this cone is
/// **tilted**: see [`spot_aim`]. A radius on the surface is only a half-angle
/// when the axis is perpendicular to it, and here the footprint is an ellipse.
///
/// Wide enough that the highlight [`spot_aim`] puts on the axis is comfortably
/// inside the core, and narrow enough that the cone closes on the slab: a spot
/// whose penumbra never lands is a spot whose row nothing in the frame can tell
/// from a point light's.
const SPOT_CORE_DEGREES: f32 = 25.0;

/// Half-angle at which that cone has closed, on the same terms.
///
/// Outside [`SPOT_CORE_DEGREES`] by enough that the two cosines the row carries
/// are distinct and `spot_cone` divides by a real difference rather than by
/// something near zero.
const SPOT_EDGE_DEGREES: f32 = 35.0;

/// The direction the spot points **along**, which is at its own highlight.
///
/// **The oblique eye is why this is not straight down.** The mirror image of a
/// light in a flat surface — the point whose reflected view direction reaches
/// the light, and so where the specular lobe peaks — divides the ground segment
/// between the eye's track and the light's foot in the ratio of their two
/// heights. [`LIGHT_HEIGHT`] and `mesh_scene`'s eye are a tenth of a unit apart,
/// so that point is the segment's midpoint to within a tenth of a unit, and the
/// light's foot is the slab's own axis.
///
/// Aimed there rather than straight down so the highlight sits on the cone's
/// axis: a highlight in the penumbra has the cone's ramp multiplying it, and
/// then what the fill frame loses at its peak is the ramp as much as the lobe.
/// Reading the eye out of [`mesh_camera`] rather than writing it again here is
/// what keeps the aim pointed at the highlight if that camera ever moves.
fn spot_aim() -> Vec3 {
    let eye = mesh_camera(Projection::default()).eye;
    Vec3::new(eye.x, 0.0, eye.z) * 0.5 - Vec3::new(0.0, LIGHT_HEIGHT, 0.0)
}

/// The point light over the slab, a fill light or not.
fn point(fill: bool) -> Light {
    Light::Point(PointLight {
        position: Vec3::new(0.0, LIGHT_HEIGHT, 0.0),
        radius: LIGHT_REACH,
        color: LIGHT_COLOR,
        fill,
    })
}

/// The spot over the slab, aimed at its own highlight, a fill light or not.
///
/// In the same place as [`point`] and carrying the same colour and reach, so
/// what the two tests below differ in is the *kind* — the row builder and the
/// arm of `mesh.slang`'s light loop — and not the lighting.
fn spot(fill: bool) -> Light {
    Light::Spot(SpotLight {
        position: Vec3::new(0.0, LIGHT_HEIGHT, 0.0),
        radius: LIGHT_REACH,
        color: LIGHT_COLOR,
        // Along the cone, away from the light — the opposite convention from the
        // sun's, which `crcbl_render::SpotLight::direction` is where it is
        // spelled out.
        direction: spot_aim(),
        inner_angle: SPOT_CORE_DEGREES.to_radians(),
        outer_angle: SPOT_EDGE_DEGREES.to_radians(),
        fill,
    })
}

/// How small a share of the frame may lose radiance before the flag is judged
/// not to have reached the specular term.
///
/// The reciprocal of a fraction of the frame, and a floor rather than a
/// prediction: what dims is every texel the light reaches, since a GGX lobe is
/// non-zero wherever the light is, and neither the slab nor the spot's cone
/// fills the frame. Swept — the measured shares are in
/// [`the_flag_takes_the_lobe`]'s doc, and the smaller of them is more than twice
/// this.
const DIMMED_SHARE: u32 = 8;

/// How much of the frame's brightest linear texel the flag must take away.
///
/// **This is the half that is about a *lobe* and not about a level.** The
/// brightest texel of the lit frame is the highlight's own; on the fill frame
/// there is no highlight left, so the brightest texel is whatever the frame's
/// brightest surface happens to be. A flag that merely dimmed a light would move
/// both together and leave this ratio near one.
///
/// Swept rather than guessed — the measured ratios are in
/// [`the_flag_takes_the_lobe`]'s doc — and under half the smaller of them.
const PEAK_RATIO: f32 = 3.0;

/// Both tests below, which are one procedure over two light kinds: render
/// `light` twice — once ordinary, once as a fill light — and hold the three
/// claims the flag makes.
///
/// One helper because the flag is one flag; the whole reason this file draws two
/// kinds is that `mesh.slang` reads the bit and not the kind, so a difference
/// between the two tests would have to be a difference in the *lights* and never
/// in how they were compared.
///
/// * **A real share of the frame lost radiance** — [`DIMMED_SHARE`].
/// * **The frame's brightest texel lost most of itself** — [`PEAK_RATIO`], which
///   is what says a highlight went rather than a light.
/// * **Nothing anywhere gained any.** Exactly zero rather than a tolerance: the
///   diffuse term is the same arithmetic over the same inputs on both frames, so
///   it is the same bits. A flag that reached the wrong term — or a shading path
///   that renormalised what was left — shows up here and nowhere else.
///
/// # What the runs read
///
/// On radv the point light dims 28969 of the frame's 49152 texels and the spot
/// 16071, and their peaks fall from 7.277 to 1.010 and from 7.277 to 0.811 —
/// factors of 7.2 and 9.0. lavapipe reads one texel more for the spot and is
/// otherwise identical to the digit, which is the whole spread between a
/// hardware driver and the software rasteriser CI approximates. The largest gain
/// anywhere is exactly zero on every run.
fn the_flag_takes_the_lobe(kind: &str, light: fn(bool) -> Light) {
    let headless = Headless::open_for_mesh();
    let (_, lit) = slab_frame(&headless, &[light(false)]);
    let (_, fill) = slab_frame(&headless, &[light(true)]);
    headless.finish();

    let (dimmed, brightest_gain) = radiance_lost(&lit, &fill);
    let (_, _, lit_peak) = lit.peak();
    let (_, _, fill_peak) = fill.peak();
    let (width, height) = MESH_EXTENT;
    eprintln!(
        "{}: {dimmed} of {} texels lost radiance when the {kind} became a fill light, its peak fell from {lit_peak} to {fill_peak}, and the largest gain anywhere is {brightest_gain}",
        crate::SUITE,
        width * height
    );
    assert!(
        dimmed * DIMMED_SHARE > width * height,
        "only {dimmed} of {} texels lost radiance when the {kind} became a fill light, so the flag is not reaching the specular term on this kind",
        width * height
    );
    assert!(
        fill_peak * PEAK_RATIO < lit_peak,
        "the {kind}'s brightest texel fell only from {lit_peak} to {fill_peak}, so what the flag took away is a share of the light rather than the highlight"
    );
    assert_eq!(
        brightest_gain, 0.0,
        "a texel gained {brightest_gain} of radiance when the {kind} lost a term, which is not something removing a lobe can do"
    );
}

/// **The fill flag removes a point light's highlight and leaves its light.**
///
/// The claim `Scene::AreaLight` could never make: a rectangle's frame passes
/// whether the flag is read off the row or off the rectangle's own arm of the
/// shading.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_fill_point_light_keeps_its_diffuse_and_loses_its_highlight() {
    the_flag_takes_the_lobe("point light", point);
}

/// **And it removes a spot's**, which is a third arm of the same shading and a
/// third row builder.
///
/// Not the point light's test with a wider cone: a spot carries two more row
/// fields than a point light does and reaches `mesh.slang` through its own
/// `spot_cone` factor, so a flag that survived the cone's arithmetic on one and
/// not the other is a frame apart from this one.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_fill_spot_keeps_its_diffuse_and_loses_its_highlight() {
    the_flag_takes_the_lobe("spot", spot);
}
