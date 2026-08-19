//! The one thing quarry measures that is not a count.
//!
//! Coverage, the per-cluster cut, the uniform cut's walk and the triangle counts
//! are all counts, and **a face lit from the wrong side leaves every one of them
//! unchanged**: it covers the same pixels, walks the same rungs and draws the
//! same triangles. That is the gap a golden frame closes and no counter can.
//!
//! A golden is not the only thing that closes it. Shading is a *mechanism*, so
//! it can be observed by changing its input and watching the output move — which
//! needs no committed image and no tolerance, and says something a golden cannot:
//! that the picture depends on the light rather than merely resembling one that
//! did.
//!
//! # `direction` points at the light, not along it
//!
//! [`DirectionalLight::default`] uses a **positive** Y, and a sun written with a
//! negative one is below the horizon rather than overhead. Worth stating here
//! because getting it backwards is exactly the defect this file exists to catch,
//! and the first draft of it had the sign wrong: both of its "side" lights were
//! underground, so moving the sun across changed 7.3% of the frame instead of
//! 58.3% and switching it off changed 1.1% instead of 37.2%.

use crcbl::math::Vec3;
use crcbl::render::DirectionalLight;

use crate::harness::{DEFAULT_BUDGET, DOLLY_START, Levels, Quarry, backend};

/// How much of the frame must change when the sun crosses to the other side.
///
/// A third. Measured, the change is 58.3% — essentially every pixel of the face
/// — but the sky behind it does not move, so a whole-frame bar would be
/// asserting the background is lit too.
const MUST_CHANGE: f32 = 0.33;

/// The sun over one shoulder or the other: the engine's own default with only
/// the side changed.
fn sun_from(x: f32) -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(x, 0.8, 0.6).normalize(),
        ..DirectionalLight::default()
    }
}

/// How many pixels of two frames differ.
fn moved(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len(), "two frames of one ring");
    assert!(!a.is_empty(), "a device that drew read back no pixels");
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

/// **The face is shaded by the light it is given, and an unlit face is unlit.**
///
/// Two claims, and the second is what stops the first from being satisfiable by
/// noise:
///
/// * Move the sun across and the picture changes. If it does not, the light is
///   not reaching the shader, or every normal is the same, or the surface is
///   drawn flat — three real defects every other assertion in this suite passes
///   through, because each counts something the bug does not touch.
/// * **A light of no colour has no direction**, byte for byte. Two frames whose
///   suns point opposite ways and carry `Vec3::ZERO` must be identical: the
///   directional term and everything gated on it, the shadow cascades included,
///   have to vanish together. A shadow that survived its own light would show up
///   here and nowhere else in this suite.
///
/// Neither is a claim about *which* pixels or by how much; that is a golden's
/// job, and quarry has none — see `docs/backlog.md`.
///
/// # What was tried and is not true: "a sun below the horizon is no sun"
///
/// It is an appealing clamp check and it is **wrong for this content**, which is
/// worth recording because the measurement looked like a bug. A sun 53° under
/// puts 12 pixels of the face somewhere no-sun does not; a sun 24° under moves
/// 12,227. The face carries 34 metres of relief over 120 of width, so it has
/// slopes steep enough to catch a low sun — and lighting them is correct. What
/// settled it was the pair above: with `color` at zero, turning the sun through
/// 180° changes nothing at all, so nothing is leaking past the clamp.
#[test]
fn the_face_is_shaded_by_the_light_it_is_given() {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry shading: the Null backend draws nothing, so no light can change a pixel of \
             it — run with CRCBL_GPU=vk"
        );
        return;
    }
    let mut quarry = Quarry::open(Levels::Flat, DEFAULT_BUDGET);
    let right = quarry.lit_frame(DOLLY_START, &sun_from(1.0)).pixels_rgba;
    let left = quarry.lit_frame(DOLLY_START, &sun_from(-1.0)).pixels_rgba;
    let unlit = quarry
        .lit_frame(
            DOLLY_START,
            &DirectionalLight {
                color: Vec3::ZERO,
                ..DirectionalLight::default()
            },
        )
        .pixels_rgba;
    let unlit_turned = quarry
        .lit_frame(
            DOLLY_START,
            &DirectionalLight {
                color: Vec3::ZERO,
                direction: -DirectionalLight::default().direction,
                ..DirectionalLight::default()
            },
        )
        .pixels_rgba;
    quarry.finish();

    let pixels = right.len() / 4;
    let across = moved(&right, &left);
    eprintln!(
        "quarry shading: the sun crossing changed {across} of {pixels} pixels ({:.1}%)",
        across as f32 / pixels as f32 * 100.0
    );
    assert!(
        across as f32 / pixels as f32 >= MUST_CHANGE,
        "moving the sun from one side to the other changed {across} of {pixels} pixels, under \
         {MUST_CHANGE} of the frame — so it is not being shaded by the light it is given, \
         whatever the counters say"
    );

    let leaked = moved(&unlit, &unlit_turned);
    assert_eq!(
        leaked, 0,
        "two suns of no colour pointing opposite ways differ in {leaked} pixel(s), so something \
         gated on the light survived the light — the shadow cascades are the candidate, since \
         they are built from its direction"
    );
    let off = moved(&right, &unlit);
    eprintln!(
        "quarry shading: switching the sun off changed {off} of {pixels} pixels ({:.1}%), and \
         turning an off sun around changed none",
        off as f32 / pixels as f32 * 100.0
    );
    assert!(
        off as f32 / pixels as f32 >= MUST_CHANGE,
        "switching the sun off changed {off} of {pixels} pixels, under {MUST_CHANGE} of the \
         frame — so the face was never lit and the comparison above means nothing"
    );
}
