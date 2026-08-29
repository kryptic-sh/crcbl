//! The `Rgba16Float` scene target, read back and measured.
//!
//! Two claims a golden image cannot make. A shader that returned the vertex
//! colour unchanged would draw a perfectly good cube and pass a golden the day
//! it was blessed; what it could not do is make two faces of different colours
//! differ in brightness by the factor the Lambert term predicts. And an `Rgba8`
//! scene target would tonemap to the same picture while carrying nothing above
//! 1.0 — which is the whole of what P1's HDR correction asked for.
//!
//! Both need the linear values the shader wrote rather than the sRGB encoding of
//! them, so this is the only module in the suite that asks
//! [`render_mesh`](crate::mesh_scene::render_mesh) for the scene target beside
//! the swapchain image. The copy's row is `256 × 4 × 2` bytes wide, a multiple
//! of the 256-byte copy pitch wgpu and D3D12 enforce, so it needs no unpadding
//! of its own.

use crate::harness::Headless;
use crate::mesh_scene::{
    MESH_EXTENT, mesh_camera, place_cube, place_cube_at, render_mesh, render_mesh_lit,
};
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, Fog, ForwardRenderer, Light, PointLight,
    Projection, RenderEffects, Sky, TransientPool,
};
use crcbl_shaders::tonemap::TonemapCurve;

/// The frame's `Rgba16Float` scene target, as the bytes the copy produced.
pub(crate) struct HdrTarget(pub(crate) Vec<u8>);

impl HdrTarget {
    /// The linear HDR value at a texel, decoded from `Rgba16Float`.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let index = ((y * MESH_EXTENT.0 + x) * 4) as usize * 2;
        let mut out = [0.0f32; 4];
        for (channel, value) in out.iter_mut().enumerate() {
            let bits = u16::from_le_bytes(
                self.0[index + channel * 2..index + channel * 2 + 2]
                    .try_into()
                    .expect("two bytes"),
            );
            *value = half_to_f32(bits);
        }
        out
    }

    /// The brightest linear channel anywhere in the target, and where it is.
    fn peak(&self) -> (u32, u32, f32) {
        let mut hottest = (0u32, 0u32, 0.0f32);
        for y in 0..MESH_EXTENT.1 {
            for x in 0..MESH_EXTENT.0 {
                // Alpha is a constant 1.0 and would mask the interesting number.
                let value = self
                    .pixel(x, y)
                    .iter()
                    .take(3)
                    .fold(0.0f32, |peak, channel| peak.max(*channel));
                if value > hottest.2 {
                    hottest = (x, y, value);
                }
            }
        }
        hottest
    }
}

/// Decodes an IEEE binary16 into an `f32`.
///
/// Written out rather than pulled in: this is the only place in the engine that
/// reads an `Rgba16Float` on the CPU, and a dependency for twelve lines of
/// shifts would be a `cargo deny` conversation about a test helper.
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let value = match exponent {
        // Zero or subnormal.
        0 => {
            if mantissa == 0 {
                0
            } else {
                // Renormalise: shift the mantissa up until its leading bit
                // falls off, decrementing the exponent as it goes.
                let leading = mantissa.leading_zeros() - 21;
                let mantissa = (mantissa << (leading + 1)) & 0x3ff;
                ((127 - 15 - leading) << 23) | (mantissa << 13)
            }
        }
        // Infinity or NaN.
        31 => 0xff << 23 | (mantissa << 13),
        _ => ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(sign | value)
}

/// One frame of the cube scene, with the scene target read back beside the
/// tonemapped image, and the fixture torn down before either is measured.
///
/// **The antialiasing resolve is refused here**, and it is the one effect that
/// would change what these tests measure: they read a single swapchain texel —
/// the one under the HDR target's peak — and compare it against what the
/// tonemap should have written there. A resolve blends that texel with its
/// neighbours, so the value read back would be a filtered one and the
/// assertion would be about the filter rather than about the tonemap.
fn cube_frame() -> (crcbl_golden::Image, HdrTarget) {
    cube_frame_with(
        TonemapCurve::Clamp,
        crcbl_shaders::tonemap::DEFAULT_EXPOSURE,
    )
}

/// The same frame under a chosen tonemap operator and exposure.
fn cube_frame_with(curve: TonemapCurve, exposure: f32) -> (crcbl_golden::Image, HdrTarget) {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::ANTIALIASING, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_tonemap_curve(curve);
    renderer.set_exposure(exposure);
    place_cube(&mut renderer);
    let mut hdr = Vec::new();
    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    assert_eq!(
        hdr.len(),
        (MESH_EXTENT.0 * MESH_EXTENT.1 * 8) as usize,
        "the scene target came back the wrong size, so every value read out of it \
         is at the wrong offset"
    );
    (image, HdrTarget(hdr))
}

/// Milestone 4, measured rather than eyeballed: the directional light produces a
/// real gradient across the cube's faces.
///
/// A shader that returned the vertex colour unchanged would draw a perfectly
/// good cube and pass a golden image the day it was blessed. What it could not
/// do is make two faces of *different* colours differ in brightness by the same
/// factor the Lambert term predicts — which is what this checks, using the HDR
/// target so the sRGB transfer function is not in the way.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_directional_light_actually_shades_the_mesh() {
    let (_, hdr) = cube_frame();

    // Collect the linear luminance of every pixel the cube covers, in the HDR
    // target — where the values are the shader's own output rather than an sRGB
    // encoding of it.
    let mut lit: Vec<f32> = Vec::new();
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [r, g, b, _] = hdr.pixel(x, y);
            let luminance = 0.2126f32.mul_add(r, 0.7152f32.mul_add(g, 0.0722 * b));
            // Anything above the clear colour's luminance is geometry.
            if luminance > 0.05 {
                lit.push(luminance);
            }
        }
    }
    assert!(
        lit.len() > 1000,
        "the cube must cover a meaningful part of the frame; got {} pixels",
        lit.len()
    );
    lit.sort_by(f32::total_cmp);
    let dimmest = lit[lit.len() / 20];
    let brightest = lit[lit.len() - lit.len() / 20 - 1];
    eprintln!(
        "{}: linear luminance across the cube — 5th percentile {dimmest}, 95th {brightest}",
        crate::SUITE
    );
    assert!(
        brightest > dimmest * 1.5,
        "a directional light must produce a gradient across the faces: the 95th \
         percentile is {brightest} and the 5th is {dimmest}, a ratio of {}",
        brightest / dimmest
    );
    // And nothing is pure black: the ambient term exists so an unlit face is
    // dark rather than invisible.
    assert!(dimmest > 0.0, "the ambient term must lift the unlit faces");
}

/// **HDR from P1 is real**, not a format enum.
///
/// `docs/plan/ROADMAP.md`'s correction asks for an `Rgba16Float` scene target
/// and a trivial tonemap from the first lit mesh. This reads the scene target
/// back and asserts it carries a value above 1.0 — which an `Rgba8` attachment
/// could not have held — and that the tonemapped swapchain pixel underneath it
/// is at the top of its range, which is the tonemap doing the one thing it does.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_hdr_target_carries_values_an_eight_bit_target_could_not() {
    let (image, hdr) = cube_frame();

    let (x, y, peak) = hdr.peak();
    eprintln!(
        "{}: peak linear value in the HDR target — {peak} at ({x}, {y})",
        crate::SUITE
    );
    assert!(
        peak > 1.0,
        "the specular highlight must exceed 1.0 somewhere, or the RGBA16F target \
         is carrying nothing an Rgba8 one could not; peak was {peak}"
    );
    assert!(
        peak.is_finite() && peak < 100.0,
        "a peak of {peak} is a NaN or a runaway, not a specular highlight"
    );

    // And the tonemap clamped that texel rather than letting it wrap or go
    // black.
    let pixel = image.pixel(x, y).expect("inside the frame");
    let brightest_channel = pixel[..3].iter().copied().max().expect("three channels");
    assert_eq!(
        brightest_channel, 255,
        "the tonemap must clamp a linear {peak} to the top of the swapchain's range, \
         got {pixel:?} at ({x}, {y})"
    );
}

/// How many pixels of a frame are pinned at the top of every channel.
fn blown_out(image: &crcbl_golden::Image) -> usize {
    let mut count = 0;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let pixel = image.pixel(x, y).expect("inside the frame");
            if pixel[..3].iter().all(|channel| *channel == 255) {
                count += 1;
            }
        }
    }
    count
}

/// **The ACES curve runs on the device, and it buys back the highlights the
/// clamp threw away.**
///
/// The operator is a branch in `tonemap.slang` on a lane of a uniform block, so
/// every way it can be wrong ends with the frame looking plausible: a selector
/// that never reaches the block, a branch the compiler folded to the other arm,
/// a matrix that lowered to the identity. None of those change the picture in a
/// way a golden blessed alongside them would catch.
///
/// What the curve is *for* is what this measures instead. At an exposure that
/// pushes a good part of the cube past 1.0, exposure-and-clamp maps all of it to
/// the same white and the shading in it is gone; the fit keeps it ordered and
/// below one. So the two frames are rendered at the same exposure and compared
/// on how much of each is pinned at the top — which cannot come out equal unless
/// the curve did not run.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_aces_curve_keeps_the_shading_the_clamp_flattens() {
    // Bright enough that the clamp loses a real area of the cube, and well
    // inside `ForwardRenderer::set_exposure`'s range.
    const OVEREXPOSED: f32 = 4.0;

    let (clamped, hdr) = cube_frame_with(TonemapCurve::Clamp, OVEREXPOSED);
    let (curved, _) = cube_frame_with(TonemapCurve::Aces, OVEREXPOSED);

    let (x, y, peak) = hdr.peak();
    let clamped_blown = blown_out(&clamped);
    let curved_blown = blown_out(&curved);
    eprintln!(
        "{}: at exposure {OVEREXPOSED} the clamp pins {clamped_blown} pixels and the \
         ACES fit pins {curved_blown}; the HDR peak is {peak} at ({x}, {y})",
        crate::SUITE
    );

    assert!(
        clamped_blown > 100,
        "the fixture must actually overexpose under the clamp, or there is nothing \
         for the curve to buy back; only {clamped_blown} pixels were pinned"
    );
    assert!(
        curved_blown * 4 < clamped_blown,
        "the ACES fit must recover most of what the clamp pinned: {curved_blown} \
         pixels are still at the top against the clamp's {clamped_blown}"
    );

    // And the hottest texel specifically: bright, and no longer white.
    let peak_pixel = curved.pixel(x, y).expect("inside the frame");
    let brightest = peak_pixel[..3]
        .iter()
        .copied()
        .max()
        .expect("three channels");
    assert!(
        brightest < 255,
        "the fit must leave headroom above a linear {peak}, got {peak_pixel:?}"
    );
    assert!(
        brightest > 128,
        "and the frame's hottest texel must still read as a highlight, got {peak_pixel:?}"
    );
}

/// The cube scene's neutral material row, made to emit `emissive`, rendered
/// with only the effects that could disturb the comparison taken off.
///
/// **The reflections are off and the resolve is off**, and each for its own
/// reason. A resolve blends neighbouring texels, so a per-texel difference
/// would be a filtered one; and screen-space reflections sample the scene
/// colour, which is exactly the thing emission changes, so leaving them on
/// would let a difference reach a pixel the emitting surface does not cover.
/// Occlusion is left alone deliberately: it scales the environment term and not
/// this one, so it must cancel — and it is the only effect in the stack that
/// touches the same sum.
fn emitting_cube_hdr(emissive: [f32; 3]) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut scene = crcbl::render::scene::demo();
    scene.materials[crcbl::render::scene::DEMO_UNTINTED].emissive = emissive;
    let mut renderer = ForwardRenderer::with_scene(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        &scene,
    )
    .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    place_cube(&mut renderer);
    let mut hdr = Vec::new();
    let _ = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// **An emissive material adds its radiance to the surface and to nothing
/// else.**
///
/// `GpuMaterial::emissive` landed in three words the row already padded with,
/// so every way of getting it wrong leaves a frame that still looks right: a
/// field the CPU writes and the shader reads at another offset, a term folded
/// into the albedo instead of added after it, a term the occlusion scales. None
/// of those are visible in a picture, and the first is not visible in a
/// round-trip test either, because both sides would agree.
///
/// So the observable is a *difference between two frames of the same scene*.
/// Only the emissive triple changes, so on every texel the emitting surface
/// covers the linear red must rise by exactly what was asked for, on every
/// other texel it must not move at all, and green and blue must not move
/// anywhere — which is what says the value went into the channel it was written
/// to and was added rather than multiplied.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_emissive_material_adds_its_radiance_and_scales_nothing() {
    // Above one, so it is a value only the `Rgba16Float` target can hold and a
    // clamp anywhere in the path would truncate.
    const EMITTED: f32 = 2.0;
    // Half a per cent of the value, which is well inside `Rgba16Float`'s
    // precision at this magnitude and well outside any rounding the shader does.
    const TOLERANCE: f32 = 0.01;

    let dark = emitting_cube_hdr([0.0; 3]);
    let lit = emitting_cube_hdr([EMITTED, 0.0, 0.0]);

    let mut emitting = 0usize;
    let mut unchanged = 0usize;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [dr, dg, db, _] = dark.pixel(x, y);
            let [lr, lg, lb, _] = lit.pixel(x, y);
            assert!(
                (lg - dg).abs() < TOLERANCE && (lb - db).abs() < TOLERANCE,
                "emission in red moved green or blue at ({x}, {y}): {:?} against {:?}",
                [lr, lg, lb],
                [dr, dg, db],
            );
            let risen = lr - dr;
            if risen.abs() < TOLERANCE {
                unchanged += 1;
            } else {
                assert!(
                    (risen - EMITTED).abs() < TOLERANCE,
                    "a texel of the emitting surface at ({x}, {y}) rose by {risen}, not \
                     by {EMITTED} — so the term is scaled by something rather than added"
                );
                emitting += 1;
            }
        }
    }
    eprintln!(
        "{}: {emitting} texel(s) rose by {EMITTED} and {unchanged} did not move",
        crate::SUITE
    );
    assert!(
        emitting > 1000,
        "the cube must cover a meaningful part of the frame, or this asserts \
         nothing about a surface; only {emitting} texels changed"
    );
    assert!(
        unchanged > 1000,
        "and the background must not emit: only {unchanged} texels held still"
    );
}

/// The cube's frame under a given fog, as the linear values the shader wrote.
///
/// **The reflections are off and the resolve is off**, each for the reason the
/// emissive frame above turns them off. A resolve blends neighbouring texels,
/// so a per-texel identity would be measured through a filter; and
/// `ssr_blur.slang` composites the screen-space reflections **after** this pass
/// — so with them on, the value in the target is the fogged radiance plus an
/// unfogged reflection, and no exact relation between two densities survives
/// that. That ordering is a real gap rather than a test convenience, and
/// `docs/backlog.md` carries it.
fn fogged_cube_hdr(fog: Fog) -> HdrTarget {
    fogged_cube_hdr_via(fog, false)
}

/// [`fogged_cube_hdr`], with the medium integrated by whichever of the two paths
/// `froxels` names.
///
/// `false` is `mesh.slang`'s closed form, which every fog test above measures.
/// `true` is `docs/plan/51-volumetrics.md`'s froxel volume: three passes, and
/// the fragment stage's own density zeroed so the air is charged once. The two
/// are the same medium along the same rays, which is what makes one frame
/// evidence about the other.
fn fogged_cube_hdr_via(fog: Fog, froxels: bool) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::VOLUMETRIC_FOG, Some(froxels)),
        ..EffectRequest::default()
    });
    renderer.set_fog(fog);
    place_cube(&mut renderer);
    let mut hdr = Vec::new();
    let _ = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The fog colour these tests scatter, chosen to make the algebra below well
/// conditioned.
///
/// Red alone, and far above anything the lit cube reaches, so `A - F` in that
/// channel is large and the transmittance solved out of it is not a ratio of
/// two nearly equal numbers. Green and blue are exactly zero, which is what
/// lets the same transmittance be recovered a **second** way — as `B / A` —
/// with no knowledge of the fog colour at all.
const FOG_SCATTER: [f32; 3] = [8.0, 0.0, 0.0];

/// The medium the sun-scattering tests run in.
///
/// Thick enough that a background column is nearly opaque, so what a background
/// texel carries is the scattering source rather than a fraction of it — which
/// is what lets those tests compare two frames without knowing the column's
/// transmittance.
const SUN_DENSITY: f32 = 0.15;

/// What fraction of the sun's radiance that medium scatters per unit length.
///
/// Large, because these tests are about a ratio between two frames and not about
/// a magnitude: it keeps the green channel well clear of `Rgba16Float`'s floor
/// in the *backward* frame, which is the one the ratio divides by.
const SUN_SCATTERING: f32 = 4.0;

/// The medium's anisotropy where the lobe is meant to be visible. Mie scattering
/// in fog is about this.
const SUN_ANISOTROPY: f32 = 0.8;

/// The smallest forward-to-backward ratio
/// [`a_forward_lobe_brightens_the_frame_the_sun_is_ahead_of`] accepts at **any**
/// background texel.
///
/// **Measured, not chosen.** radv gives 56.9 at the frame's worst texel, and a
/// density sweep from 0.05 to 0.40 moved it not at all: a background column runs
/// to `CLUSTER_FAR` and is opaque at every density in that range, so what a
/// background texel carries is the scattering source itself and the ratio is the
/// phase function's alone. This sits under the measurement with room for another
/// driver's arithmetic, and far above the 1.0 a direction-blind source gives.
const SUN_LOBE_RATIO: f32 = 50.0;

/// How much of its unshadowed radiance a background texel has to keep to count
/// as being outside the shaft.
///
/// **Measured, not chosen.** radv puts 12804 of the 39110 background texels
/// under this and 31798 under `0.7`, so the threshold sits on the shoulder of
/// the distribution rather than in its tail — a shaft that lost a third of its
/// depth would still clear it, and one that vanished could not.
const SHAFT_DARKENING: f32 = 0.6;

/// How many background texels have to clear [`SHAFT_DARKENING`].
///
/// **Measured, not chosen**: 12804 do. Set well under that so the count is a
/// claim about a region of the frame rather than about a particular driver's
/// rounding, and well over zero so an occluder that shadowed a single tile
/// could not pass.
const SHAFT_TEXELS: usize = 8_000;

/// How far apart the deepest and the shallowest darkening over the background
/// have to be.
///
/// **This is the assertion that says "a shaft" and not "a dimmer".** A term
/// that scaled the whole medium — a visibility folded into the density, an
/// exposure that moved with the effect bit — darkens every texel by the same
/// factor and lands a spread of zero. **Measured**: radv's background runs from
/// `0.459` to `0.713`, a spread of `0.253`, because the slab covers the near
/// column of some rays and the far column of others.
const SHAFT_CONTRAST: f32 = 0.15;

/// How much brighter than its unshadowed self a texel may be before that counts
/// as brighter.
///
/// Not zero, because the two frames are two dispatches: an occluder that
/// shadows nothing at a texel still runs the whole PCF kernel there, and the
/// nine taps sum in an order the hardware picks. This is a rounding step at the
/// radiances these tests work in, well under the darkening they are looking
/// for.
const SHAFT_TOLERANCE: f32 = 1e-3;

/// The direction [`mesh_camera`] looks in, which is the axis the sun is put on
/// and against.
///
/// Derived from the camera rather than written down, so it cannot drift from the
/// fixture it has to agree with — a sun that was only nearly on the view axis
/// would weaken the lobe without failing anything.
fn camera_forward() -> crcbl::math::Vec3 {
    let camera = mesh_camera(Projection::default());
    (camera.target - camera.eye).normalize()
}

/// How far the froxel column's transmittance may sit from the closed form's at
/// this fixture's extent — see
/// [`the_froxel_volume_integrates_the_same_medium_the_closed_form_does`].
///
/// **Measured, not chosen.** The gap is the tile-centre quantisation, so it
/// grows with the medium and then falls again as the frame saturates: swept on
/// radv over densities from `0.02` to `0.6`, it rises from `0.0025` to a peak of
/// `0.0113` around `0.4` and falls back to `0.0099`. The test runs at `0.15`,
/// where it is `0.0082`. This is the peak of that whole sweep rounded up, so the
/// bound does not depend on which density the fixture happens to use — and it is
/// still an order of magnitude under the `0.13` a one-cell slice-boundary
/// disagreement between the two shaders produced, which is the mistake it caught.
const FROXEL_TRACKS: f32 = 0.012;

/// How far the froxel column's doubled transmittance may sit from the square of
/// its single one — see
/// [`doubling_the_density_squares_the_transmittance_through_the_froxel_volume`].
///
/// **Measured, not chosen.** This one moves the *other* way with the medium —
/// it is `Rgba16Float`'s precision on a transmittance near one rather than a
/// quantisation of the column — swept on radv from `0.0016` at a density of
/// `0.02` down to `0.0002` at `0.4`. The test runs at `0.15`, where it is
/// `0.0006`; this is the peak of the sweep rounded up.
const FROXEL_SQUARES: f32 = 0.002;

/// Uniform fog of a given density: a zero falloff is air that does not thin
/// with height, so the optical depth is `density * distance` exactly.
///
/// That is what makes the relation the test below asserts a *clean* one — at
/// twice the density every ray has exactly twice the optical depth, whatever
/// its length, and no geometry has to be known to say so.
fn uniform_fog(density: f32) -> Fog {
    Fog {
        density,
        falloff: 0.0,
        reference_height: 0.0,
        color: crcbl::math::Vec3::from_array(FOG_SCATTER),
        ..Fog::NONE
    }
}

/// **The fog colour is unobservable while the density is zero.**
///
/// Narrower than it first looks, and the narrowing is the point. The composite
/// runs unconditionally — there is no branch selecting fog — so this says the
/// colour reaches no pixel through a path that does not read the density, which
/// is how a fog term gets written that tints every frame in the tree.
///
/// **It is not, on its own, the additive-zero claim.** Both frames here run the
/// same shader, so anything that fogs them *equally* — a density floored to a
/// minimum, say — is invisible to a comparison between them. That was measured
/// rather than assumed: flooring the density at `0.01` leaves this test green,
/// and leaves this suite's `goldens` green too, because the darkening it
/// produces is inside [`crcbl_golden::Tolerance::RASTERISER`] on this scene.
/// See `docs/backlog.md`, which carries that as a coverage note.
///
/// What does carry the claim is the arithmetic, in three checked pieces:
/// `crcbl_shaders::fog`'s `the_exponential_is_exactly_one_at_zero` says the
/// transmittance at zero optical depth is exactly `1.0`; its
/// `the_shader_spells_the_same_constants` pins the composite as
/// `lit * t + fog * (1 - t)`, whose value at `t == 1` is `lit` bit for bit
/// where a `lerp` would be `fog + (lit - fog)`; and this test says the density
/// is what gates it. None of the three is the whole statement alone.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_fog_colour_is_unobservable_at_zero_density() {
    let untouched = fogged_cube_hdr(Fog::NONE);
    let configured = fogged_cube_hdr(uniform_fog(0.0));
    assert_eq!(
        untouched.0, configured.0,
        "a zero-density fog with a colour set moved the frame, so the colour is \
         reaching pixels through a path that does not read the density"
    );
}

/// **The fog thickens by the exponential law, per texel, with no geometry
/// known.**
///
/// The observable that separates a working height-fog term from every plausible
/// wrong one. Transmittance is `e^-tau` and uniform fog's `tau` is
/// `density * distance`, so **doubling the density squares the transmittance**
/// at every texel at once — whatever that texel's distance is. A linear falloff
/// passes none of this; a fog applied before the shading rather than after it
/// passes none of it; an `exp` that is subtly the wrong function fails it by
/// more than the tolerance below.
///
/// The transmittance is recovered two independent ways per texel and both are
/// checked against each other: from green, where the fog scatters nothing, as
/// `B / A`; and from red, where it scatters [`FOG_SCATTER`], as
/// `(B - F) / (A - F)`. Those agreeing is what says one transmittance drove
/// every channel rather than three unrelated numbers landing plausibly.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn doubling_the_fog_density_squares_the_transmittance() {
    /// Thick enough that the transmittance is well away from one at the cube,
    /// thin enough that squaring it does not put the doubled frame at the fog
    /// colour, where the algebra stops being able to see anything.
    const DENSITY: f32 = 0.15;
    /// Half a per cent of full transmittance. `Rgba16Float` carries about three
    /// decimal digits at these magnitudes and the relation squares one of them,
    /// so this is the precision of the target rather than of the arithmetic.
    const TOLERANCE: f32 = 0.005;
    /// A texel is the cube only if the fog moved its green measurably — the
    /// background is written by no fragment of `mesh.slang` and is unfogged.
    const LIT_FLOOR: f32 = 0.05;

    let clear = fogged_cube_hdr(uniform_fog(0.0));
    let once = fogged_cube_hdr(uniform_fog(DENSITY));
    let twice = fogged_cube_hdr(uniform_fog(DENSITY * 2.0));

    let mut checked = 0usize;
    let mut thickest = 1.0f32;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [ar, ag, _, _] = clear.pixel(x, y);
            let [br, bg, _, _] = once.pixel(x, y);
            let [cr, _, _, _] = twice.pixel(x, y);
            if ag <= LIT_FLOOR {
                // The background, which no fragment of this shader wrote.
                assert!(
                    (br - ar).abs() < TOLERANCE,
                    "fog moved a texel the mesh does not cover at ({x}, {y})"
                );
                continue;
            }

            let from_green = bg / ag;
            let from_red = (br - FOG_SCATTER[0]) / (ar - FOG_SCATTER[0]);
            assert!(
                (from_green - from_red).abs() < TOLERANCE,
                "the transmittance at ({x}, {y}) is {from_green} read from green \
                 and {from_red} read from red, so one number did not drive both"
            );

            let doubled = (cr - FOG_SCATTER[0]) / (ar - FOG_SCATTER[0]);
            assert!(
                (doubled - from_green * from_green).abs() < TOLERANCE,
                "doubling the density at ({x}, {y}) took the transmittance to \
                 {doubled}, where squaring {from_green} gives {}",
                from_green * from_green
            );
            thickest = thickest.min(from_green);
            checked += 1;
        }
    }

    assert!(
        checked > 1000,
        "only {checked} texels carried the mesh, so this measured almost nothing"
    );
    // And that the fog was thick enough to have been measuring something: a
    // density that changed no texel would satisfy every relation above with a
    // transmittance of one.
    assert!(
        thickest < 0.9,
        "the thickest fog left {thickest} of the radiance, so this test would \
         have passed on a shader that ignored the density entirely"
    );
}

/// **The froxel volume is exactly the identity at zero density.**
///
/// The off-switch this effect needs and cannot get from its own bit. Three
/// passes run on every frame the effect is on, whatever the medium is, so
/// "nobody set a fog" has to be a value rather than an absence: every froxel's
/// transmittance is exactly one and its radiance exactly zero, and
/// `volumetric_composite.slang` multiplies by that one and adds those zeroes.
///
/// Compared against the *analytic* path's own zero-density frame rather than
/// against itself, so it also says the two paths agree somewhere — a composite
/// that scaled or tinted whatever it read would fail here before any medium
/// existed to blame.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_froxel_volume_is_exactly_the_identity_at_zero_density() {
    let analytic = fogged_cube_hdr_via(uniform_fog(0.0), false);
    let froxels = fogged_cube_hdr_via(uniform_fog(0.0), true);
    assert_eq!(
        analytic.0, froxels.0,
        "the froxel volume moved a frame with no medium in it, so its composite is \
         reaching pixels through a path that does not read the density"
    );
}

/// **The froxel volume integrates the same medium the closed form does.**
///
/// Rung 1a of `docs/plan/51-volumetrics.md` has no light loop in it, so the
/// column it integrates is algebraically the exponential `mesh.slang`
/// composites: a single-scattering albedo of one against an isotropic
/// environment. The two frames therefore have to agree — and this is the only
/// thing that says the buffer, the two dispatches, the prefix scan and the
/// composite are wired to each other at all. Every one of them can be wrong in a
/// way that still produces a plausibly foggy picture.
///
/// **They agree closely rather than exactly**, and the gap is a design decision
/// rather than a defect: the column is built along each *tile's* centre ray, and
/// a pixel elsewhere in the tile looks along a slightly longer or shorter one.
/// At this fixture's extent the grid is four tiles by three, which is about as
/// coarse as it ever gets — [`FROXEL_TRACKS`] is what that measures out to, and
/// it is measured rather than chosen. Rung 3's reprojection is what shrinks it;
/// see topic 51.
///
/// The relation is checked on the *transmittance* rather than on the radiance,
/// for `doubling_the_fog_density_squares_the_transmittance`'s reason: green is
/// the channel [`FOG_SCATTER`] leaves alone, so `B / A` recovers what the medium
/// did with no knowledge of the medium's colour.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_froxel_volume_integrates_the_same_medium_the_closed_form_does() {
    const DENSITY: f32 = 0.15;
    const LIT_FLOOR: f32 = 0.05;

    let clear = fogged_cube_hdr_via(uniform_fog(0.0), false);
    let analytic = fogged_cube_hdr_via(uniform_fog(DENSITY), false);
    let froxels = fogged_cube_hdr_via(uniform_fog(DENSITY), true);

    let mut checked = 0usize;
    let mut worst = 0.0f32;
    let mut thickest = 1.0f32;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [_, ag, _, _] = clear.pixel(x, y);
            if ag <= LIT_FLOOR {
                continue;
            }
            let closed = analytic.pixel(x, y)[1] / ag;
            let marched = froxels.pixel(x, y)[1] / ag;
            worst = worst.max((closed - marched).abs());
            thickest = thickest.min(closed);
            checked += 1;
        }
    }

    eprintln!(
        "crcbl mesh e2e: volumetrics — {checked} texel(s), worst transmittance gap {worst:.4}, \
         thickest closed-form transmittance {thickest:.4}"
    );
    assert!(
        checked > 1000,
        "only {checked} texels carried the mesh, so this measured almost nothing"
    );
    assert!(
        thickest < 0.9,
        "the thickest fog left {thickest} of the radiance, so both paths would have \
         agreed on a shader that ignored the density entirely"
    );
    assert!(
        worst <= FROXEL_TRACKS,
        "the froxel volume and the closed form disagree by {worst} somewhere, past the \
         {FROXEL_TRACKS} this grid's tile-centre rays account for — the column is not \
         integrating the medium the fragment stage integrates"
    );
}

/// **The froxel path thickens by the same exponential law.**
///
/// [`doubling_the_fog_density_squares_the_transmittance`]'s claim, asked of the
/// other path. The test above says the two paths agree at one density, which a
/// composite that happened to land there — a fixed scale, say — would also
/// satisfy; this says the froxel column responds to the medium the way an
/// optical depth does, which nothing that is not integrating one will.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn doubling_the_density_squares_the_transmittance_through_the_froxel_volume() {
    const DENSITY: f32 = 0.15;
    const LIT_FLOOR: f32 = 0.05;

    let clear = fogged_cube_hdr_via(uniform_fog(0.0), true);
    let once = fogged_cube_hdr_via(uniform_fog(DENSITY), true);
    let twice = fogged_cube_hdr_via(uniform_fog(DENSITY * 2.0), true);

    let mut checked = 0usize;
    let mut worst = 0.0f32;
    let mut thickest = 1.0f32;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [_, ag, _, _] = clear.pixel(x, y);
            if ag <= LIT_FLOOR {
                continue;
            }
            let single = once.pixel(x, y)[1] / ag;
            let doubled = twice.pixel(x, y)[1] / ag;
            worst = worst.max((doubled - single * single).abs());
            thickest = thickest.min(single);
            checked += 1;
        }
    }

    eprintln!(
        "crcbl mesh e2e: volumetrics — {checked} texel(s), worst squaring gap {worst:.4}, \
         thickest transmittance {thickest:.4}"
    );
    assert!(
        checked > 1000,
        "only {checked} texels carried the mesh, so this measured almost nothing"
    );
    assert!(
        thickest < 0.9,
        "the thickest fog left {thickest} of the radiance, so this would have passed \
         on a column that ignored the density entirely"
    );
    assert!(
        worst <= FROXEL_SQUARES,
        "doubling the density moved the froxel column's transmittance off its square by \
         {worst}, past the {FROXEL_SQUARES} the slice split accounts for"
    );
}

/// A medium that scatters the sun as well as the environment.
///
/// `FOG_SCATTER` is the environment term and is red alone, so **green measures
/// the sun term and nothing else**: `DirectionalLight::default`'s colour has all
/// three channels, and the environment contributes exactly zero to this one.
/// That is the same trick `doubling_the_fog_density_squares_the_transmittance`
/// plays with the other channel, for the same reason — a quantity recovered with
/// no knowledge of what else is in the frame.
fn sun_fog(anisotropy: f32) -> Fog {
    Fog {
        anisotropy,
        sun_scattering: SUN_SCATTERING,
        ..uniform_fog(SUN_DENSITY)
    }
}

/// The fixture cube under a medium that scatters a sun pointing whichever way
/// the caller says, through the froxel path, with the cascades on or off.
///
/// **`shadows` is a parameter and not a constant** because the two things this
/// fixture is asked are opposites. The lobe tests want no occluder at all — a
/// shadow moves when the sun does, and would answer "the frame changed" for a
/// reason that is not the phase function. The shaft tests want the shadow and
/// nothing else to be the difference between two frames.
///
/// [`fogged_cube_hdr_via`]'s scene and its refusals; the light is the caller's
/// because the whole claim here is about the angle between it and the view ray.
fn sun_scattered_cube_hdr(
    fog: Fog,
    to_sun: crcbl::math::Vec3,
    shadows: bool,
    wall: bool,
) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::SHADOWS, Some(shadows))
            .force(RenderEffects::VOLUMETRIC_FOG, Some(true)),
        ..EffectRequest::default()
    });
    renderer.set_fog(fog);
    place_cube(&mut renderer);
    if wall {
        place_cube_at(&mut renderer, occluder_transform());
    }
    let light = DirectionalLight {
        direction: to_sun,
        ..DirectionalLight::default()
    };
    let mut hdr = Vec::new();
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        &light,
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The fixture cube in a medium with a point light hanging in it, through the
/// froxel path, at the coefficient the caller says.
///
/// The medium is black and sunless — `color` zero, `sun_scattering` zero — so
/// the only radiance a background column can carry is what the lamp puts into
/// it; the lamp is green so that channel is the lamp's alone. It hangs a little
/// ahead of the eye and to its right, with a radius short enough that the
/// columns on the frame's far side never come within it: those are the texels
/// the glow must leave byte for byte.
fn lamplit_cube_hdr(light_scattering: f32) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::VOLUMETRIC_FOG, Some(true)),
        ..EffectRequest::default()
    });
    renderer.set_fog(Fog {
        density: SUN_DENSITY,
        light_scattering,
        ..Fog::NONE
    });
    let camera = mesh_camera(Projection::default());
    let forward = (camera.target - camera.eye).normalize();
    let right = forward.cross(camera.up).normalize();
    renderer.set_lights(&[Light::Point(PointLight {
        position: camera.eye + forward * LAMP_AHEAD + right * LAMP_ASIDE,
        radius: LAMP_RADIUS,
        color: crcbl::math::Vec3::new(0.0, 4.0, 0.0),
    })]);
    place_cube(&mut renderer);
    let mut hdr = Vec::new();
    let _ = render_mesh(&headless, &mut renderer, &mut pool, &camera, Some(&mut hdr));
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// How far ahead of the eye [`lamplit_cube_hdr`]'s lamp hangs, along the view
/// axis.
///
/// Swept rather than chosen: at `1.4` the lamp's disc covers half the
/// background — 19207 of 39110 texels unmoved on radv — and at `2.0` it covers
/// a seventh, which is what makes the unmoved majority below a real claim
/// while the lamp still sits a unit clear of the cube.
const LAMP_AHEAD: f32 = 2.0;

/// And how far to the eye's right, so the glow is on one side of the frame and
/// the other side is its control.
const LAMP_ASIDE: f32 = 0.35;

/// The lamp's radius: the hard bound on where a column can glow.
const LAMP_RADIUS: f32 = 0.5;

/// The least a background texel must brighten, at the peak of the glow, in
/// linear green.
///
/// **Measured, not chosen.** The fixture's lamp lifts its brightest background
/// texel by 0.0450 on radv and on lavapipe alike — a background column is
/// opaque at [`SUN_DENSITY`], so the number is the scattering source and not a
/// fraction of it. This sits at under half of that and three orders above
/// `Rgba16Float`'s resolution at zero.
const LAMP_GLOW_FLOOR: f32 = 0.02;

/// **A lamp in the medium glows, and only where the lamp is.**
///
/// `docs/plan/51-volumetrics.md`'s rung 2 seen from the frame: the same scene
/// with the medium's punctual coefficient at zero and at a value, so the only
/// difference between the two frames is whether the froxel column reads the
/// froxel's light list. Three claims, and each rules out a different way of
/// being wrong:
///
/// * **No background texel is darker with the lamp scattering.** The glow is a
///   term added to the scattering source; one that multiplied, or reached the
///   transmittance, darkens somewhere.
/// * **The brightest background texel brightens meaningfully.** A list never
///   bound, a coefficient never carried, a kind test that skipped every row —
///   each draws the frame the fixture had and fails only this.
/// * **Most of the background does not move at all.** The lamp's radius is a
///   hard bound and the columns on the frame's far side never come inside it,
///   so their glow is exactly zero — byte for byte, not nearly. A falloff
///   without the window, or a light evaluated everywhere rather than through
///   the list, lights the whole frame a little and fails this. Three quarters
///   is the bar; the fixture measures 33713 of 39110 on both drivers.
///
/// The mesh's own texels are excluded because the lamp lights the cube through
/// `mesh.slang` in both frames, and the medium in front of a surface is the
/// composite's partial slice, which
/// `crates/crcbl/tests/mesh_e2e/froxels.rs` says why nothing here reaches.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_lamp_in_the_medium_glows_and_only_where_the_lamp_is() {
    let dark = lamplit_cube_hdr(0.0);
    let glowing = lamplit_cube_hdr(2.0);

    let background = background_texels();
    let mut still = 0usize;
    let mut peak = 0.0f32;
    let mut peak_at = (0u32, 0u32);
    for (x, y) in &background {
        let before = dark.pixel(*x, *y);
        let after = glowing.pixel(*x, *y);
        for channel in 0..3 {
            assert!(
                after[channel] >= before[channel],
                "background texel ({x}, {y}) channel {channel} fell from {} to {} with the lamp \
                 scattering, so the glow is not a term added to the source",
                before[channel],
                after[channel]
            );
        }
        let lift = after[1] - before[1];
        if before == after {
            still += 1;
        }
        if lift > peak {
            peak = lift;
            peak_at = (*x, *y);
        }
    }

    eprintln!(
        "{}: lamp glow — {} background texel(s), {still} unmoved, peak lift {peak:.4} at {:?}",
        crate::SUITE,
        background.len(),
        peak_at
    );

    assert!(
        peak >= LAMP_GLOW_FLOOR,
        "the lamp lifted its brightest background texel by only {peak}, under \
         {LAMP_GLOW_FLOOR}: the column is not reading the froxel's light list"
    );
    assert!(
        still * 4 >= background.len() * 3,
        "only {still} of {} background texels are byte for byte unmoved, so the lamp is \
         reaching columns its radius does not",
        background.len()
    );
}

/// Which texels the mesh does not cover, taken off a frame with no medium in it
/// at all.
///
/// The background is where the sun term is measurable on its own: no fragment of
/// `mesh.slang` wrote it, so what is there is the scene clear colour behind a
/// full column of air. The mask has to come from an **unfogged** frame, because
/// a fogged background is no longer the clear colour and the old `green <= floor`
/// test for "the mesh is not here" would call the whole frame covered.
fn background_texels() -> Vec<(u32, u32)> {
    background_of(&fogged_cube_hdr_via(uniform_fog(0.0), false))
}

/// The texels `clear` did not draw geometry into.
///
/// `clear` has to be a frame of the **same scene** with no medium in it: the
/// mask says "no fragment wrote here", and a frame with fog in it no longer
/// carries the clear colour to recognise that by.
fn background_of(clear: &HdrTarget) -> Vec<(u32, u32)> {
    const LIT_FLOOR: f32 = 0.05;
    (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0).map(move |x| (x, y)))
        .filter(|(x, y)| clear.pixel(*x, *y)[1] <= LIT_FLOOR)
        .collect()
}

/// **An isotropic medium does not care which way the sun points.**
///
/// The exact half of the phase function's claim, and the one that says the sun's
/// direction reaches the picture through `volumetric_phase` and through nothing
/// else. At `g = 0` the lobe is the constant `INV_FOUR_PI`, so reversing the sun
/// must leave every background texel byte for byte — a scatter that leaked the
/// direction into the source any other way (a Lambert term, a dot product folded
/// into the radiance) fails this while still drawing a picture that looks like
/// fog. The mesh's own texels are excluded because the same light shades the
/// cube, and that surface *should* change when the sun moves.
///
/// It is also what makes the test below evidence: that one shows a frame moving
/// when the sun turns, and this one shows the movement is the lobe's.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn an_isotropic_medium_scatters_the_sun_the_same_way_whichever_way_it_points() {
    let ahead = sun_scattered_cube_hdr(sun_fog(0.0), camera_forward(), false, false);
    let behind = sun_scattered_cube_hdr(sun_fog(0.0), -camera_forward(), false, false);

    let background = background_texels();
    for (x, y) in &background {
        assert_eq!(
            ahead.pixel(*x, *y),
            behind.pixel(*x, *y),
            "an isotropic medium scattered differently at ({x}, {y}) for two sun \
             directions, so the direction is reaching the scattering source outside \
             the phase function"
        );
    }
    assert!(
        background.len() > 1000,
        "only {} texels were background, so this compared almost nothing",
        background.len()
    );
}

/// **A forward lobe makes looking into the sun bright and looking away flat.**
///
/// What anyone means by a shaft, in the one form this rung can state: the sun is
/// put along the camera's own forward axis and then reversed, so every
/// background texel's scattering angle flips from near zero to near `pi`. A
/// forward-scattering medium has to brighten in the first and not the second, at
/// **every** background texel rather than on average — a term that brightened the
/// whole frame equally would pass a mean comparison and fail this.
///
/// The lobe is the Henyey-Greenstein one at [`SUN_ANISOTROPY`], where
/// `p(g, 1) / p(g, -1)` is in the hundreds, so the ratio asserted is far below
/// what the model gives and still far above anything a direction-blind term
/// could produce.
///
/// Green alone, per [`sun_fog`]: the environment term is red, so this channel is
/// the sun's contribution with nothing else folded into it.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_forward_lobe_brightens_the_frame_the_sun_is_ahead_of() {
    let ahead = sun_scattered_cube_hdr(sun_fog(SUN_ANISOTROPY), camera_forward(), false, false);
    let behind = sun_scattered_cube_hdr(sun_fog(SUN_ANISOTROPY), -camera_forward(), false, false);

    let background = background_texels();
    let mut weakest = f32::INFINITY;
    let mut dimmest_ahead = f32::INFINITY;
    for (x, y) in &background {
        let into = ahead.pixel(*x, *y)[1];
        let away = behind.pixel(*x, *y)[1];
        assert!(
            into > away,
            "the sun ahead of the camera scattered {into} at ({x}, {y}) and the same \
             sun behind it scattered {away} — a forward lobe cannot do that"
        );
        weakest = weakest.min(into / away.max(1e-9));
        dimmest_ahead = dimmest_ahead.min(into);
    }

    eprintln!(
        "crcbl mesh e2e: volumetrics — {} background texel(s), weakest forward ratio \
         {weakest:.1}, dimmest forward radiance {dimmest_ahead:.4}",
        background.len()
    );
    assert!(
        background.len() > 1000,
        "only {} texels were background, so this measured almost nothing",
        background.len()
    );
    assert!(
        weakest >= SUN_LOBE_RATIO,
        "the weakest forward-to-backward ratio over the background is {weakest}, under \
         the {SUN_LOBE_RATIO} a lobe this anisotropic gives even at the corners of the \
         frame — the direction is reaching the source too weakly to be the phase function"
    );
    assert!(
        dimmest_ahead > 0.0,
        "the sun scattered nothing anywhere, so this compared two black frames"
    );
}

/// Where the sun is put for the shaft tests: straight down the world `+x` axis,
/// which is the axis [`occluder_transform`]'s slab faces.
pub(crate) const SHAFT_TO_SUN: crcbl::math::Vec3 = crcbl::math::Vec3::X;

/// A slab standing between the sun and half the scene.
///
/// **Behind the camera and thin along the sun's axis**, which is what makes it
/// an occluder and not a second thing in the picture: it sits at `x = 2` facing
/// the sun's `+x`, while [`mesh_camera`] stands at `x = 1.6` looking the other
/// way, so no fragment of it is ever drawn and the *only* thing it does to the
/// frame is take the sun out of the air behind it.
///
/// **It covers `z > 0` and not `z < 0`**, and that asymmetry is the test. A slab
/// over the whole scene would darken every background texel by about the same
/// amount, which is a picture a global dimmer draws just as well; a half-covered
/// frustum has an edge in it, and the test asserts both sides of that edge.
pub(crate) fn occluder_transform() -> crcbl::math::Mat4 {
    crcbl::math::Mat4::from_scale_rotation_translation(
        crcbl::math::Vec3::new(0.4, 40.0, 40.0),
        crcbl::math::Quat::IDENTITY,
        crcbl::math::Vec3::new(2.0, 0.0, 20.0),
    )
}

/// The fixture cube and the slab above it, under a sun the slab faces.
fn shafted_hdr(fog: Fog, shadows: bool) -> HdrTarget {
    sun_scattered_cube_hdr(fog, SHAFT_TO_SUN, shadows, true)
}

/// **The medium behind an occluder goes dark, and only the sun goes with it.**
///
/// The observable that separates a real shaft from a uniform glow, and the one
/// `docs/plan/51-volumetrics.md` names as rung 1b-ii's: the same scene, the same
/// medium and the same sun, drawn once with the cascades on and once with them
/// off, so the **only** difference between the two frames is whether a froxel
/// is allowed to see what is standing in front of it.
///
/// Three claims, and each rules out a different way of being wrong:
///
/// * **No background texel is brighter with shadows on.** A visibility that
///   was added rather than multiplied, or read from the wrong froxel, brightens
///   somewhere.
/// * **A large patch is meaningfully darker.** A lookup that always returned
///   `1.0` — the wrong cascade, an atlas never bound, a projection off the tile —
///   draws a perfectly plausible frame and fails only this.
/// * **The darkening is not the same everywhere.** The slab covers the near
///   column of some rays and the far column of others, so the frame has an edge
///   in it; a factor applied to the whole medium has none. This is the
///   difference between a shaft and a dimmer, and nothing else here asks it.
/// * **Nothing goes to black.** The environment term is not occluded, so a
///   fully shadowed column still glows; a visibility multiplied into the whole
///   source instead of the sun term alone puts holes in the fog.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_medium_behind_an_occluder_loses_the_sun_and_keeps_the_sky() {
    let lit = shafted_hdr(sun_fog(SUN_ANISOTROPY), false);
    let shadowed = shafted_hdr(sun_fog(SUN_ANISOTROPY), true);

    let background = background_of(&shafted_hdr(Fog::NONE, false));
    let mut darkened = 0usize;
    let mut deepest = f32::INFINITY;
    let mut shallowest = 0.0f32;
    let mut floor = f32::INFINITY;
    for (x, y) in &background {
        let open = lit.pixel(*x, *y)[1];
        let behind = shadowed.pixel(*x, *y)[1];
        assert!(
            behind <= open + SHAFT_TOLERANCE,
            "shadowing the froxels made ({x}, {y}) brighter — {behind} against {open} — \
             so the visibility is not multiplying the sun term"
        );
        let ratio = behind / open.max(1e-9);
        if ratio < SHAFT_DARKENING {
            darkened += 1;
        }
        deepest = deepest.min(ratio);
        shallowest = shallowest.max(ratio);
        floor = floor.min(behind);
    }

    let contrast = shallowest - deepest;
    eprintln!(
        "crcbl mesh e2e: volumetrics — {darkened} of {} background texel(s) under \
         {SHAFT_DARKENING} of their unshadowed radiance, deepest {deepest:.4}, \
         shallowest {shallowest:.4}, dimmest shadowed radiance {floor:.4}",
        background.len()
    );
    assert!(
        darkened >= SHAFT_TEXELS,
        "only {darkened} background texels dropped below {SHAFT_DARKENING} of their \
         unshadowed radiance, so the slab is casting no shaft worth the name"
    );
    assert!(
        contrast >= SHAFT_CONTRAST,
        "the darkening runs from {deepest} to {shallowest}, a spread of {contrast} — \
         under the {SHAFT_CONTRAST} an occluder with an edge in it gives, which is what \
         a term that dimmed the whole medium equally would look like"
    );
    assert!(
        floor > 0.0,
        "a shadowed column went to exactly nothing, so the occlusion is reaching the \
         environment term and not only the sun"
    );
}

/// **A medium with no sun in it does not notice the cascades at all.**
///
/// The off-switch, and the exactness is the point: with `sun_scattering` at zero
/// the froxel volume is the environment term alone, which no occluder may touch,
/// so the two frames have to agree byte for byte. It is what says the visibility
/// multiplies the sun and only the sun — a factor that reached the environment
/// term, or the transmittance, moves a bit here while still drawing a shaft that
/// looks right in the test above.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn shadowing_the_froxels_leaves_a_sunless_medium_exactly_alone() {
    let sunless = Fog {
        sun_scattering: 0.0,
        ..sun_fog(SUN_ANISOTROPY)
    };
    let open = shafted_hdr(sunless, false);
    let occluded = shafted_hdr(sunless, true);

    let background = background_of(&shafted_hdr(Fog::NONE, false));
    for (x, y) in &background {
        assert_eq!(
            open.pixel(*x, *y),
            occluded.pixel(*x, *y),
            "the cascades moved ({x}, {y}) in a medium with no sun in it, so the \
             occlusion is reaching something other than the sun term"
        );
    }
    assert!(
        background.len() > 1000,
        "only {} texels were background, so this compared almost nothing",
        background.len()
    );
}

/// **The reference height is a height: raising the plane thickens the fog.**
///
/// Uniform fog above says nothing about the *height* half of exponential height
/// fog, because a zero falloff is exactly the branch that ignores it. This is
/// the cheapest statement that the other branch runs and runs the right way
/// round: density falls off **above** the reference plane, so moving the plane
/// up puts the scene deeper into the thick air and every texel must dim.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn raising_the_reference_plane_thickens_the_fog() {
    /// A falloff on the scale of the scene, so the height term is neither flat
    /// nor saturated across the cube.
    const FALLOFF: f32 = 2.0;
    const DENSITY: f32 = 0.15;
    const LIT_FLOOR: f32 = 0.05;

    let height = |reference_height: f32| Fog {
        density: DENSITY,
        falloff: FALLOFF,
        reference_height,
        color: crcbl::math::Vec3::from_array(FOG_SCATTER),
        ..Fog::NONE
    };
    let low = fogged_cube_hdr(height(-FALLOFF));
    let high = fogged_cube_hdr(height(FALLOFF));
    let clear = fogged_cube_hdr(uniform_fog(0.0));

    let mut dimmed = 0usize;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [_, ag, _, _] = clear.pixel(x, y);
            if ag <= LIT_FLOOR {
                continue;
            }
            let [_, lg, _, _] = low.pixel(x, y);
            let [_, hg, _, _] = high.pixel(x, y);
            assert!(
                hg <= lg,
                "raising the fog plane brightened ({x}, {y}): {hg} against {lg}, \
                 so the density is rising with height instead of falling"
            );
            if hg < lg {
                dimmed += 1;
            }
        }
    }
    assert!(
        dimmed > 1000,
        "only {dimmed} texels moved, so the reference height reached almost \
         nothing and this would pass on a shader that ignored it"
    );
}

/// One frame of the cube under a chosen sky and a chosen light, with the
/// screen-space passes forced off.
///
/// [`fogged_cube_hdr`]'s scene and its two refusals, for its reasons:
/// antialiasing would blend a texel with its neighbours and reflections would
/// add a term after the one under test. **And the cube is placed unspun**,
/// which fog did not need — the sky's whole claim is about a surface's normal,
/// so the three visible faces are the axis-aligned `+X`, `+Y` and `+Z` rather
/// than three arbitrary ones.
fn sky_cube_hdr(camera: &crcbl::render::Camera, sky: Sky, light: &DirectionalLight) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_sky(sky);
    crate::mesh_scene::place_cube_at(&mut renderer, crcbl::math::Mat4::IDENTITY);
    let mut hdr = Vec::new();
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        camera,
        light,
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The uniform sky these tests raise the ambient with, one value per channel.
///
/// Three different numbers, so a projection that wrote one channel's
/// coefficients into another row is a colour cast rather than a brightness that
/// happens to match.
const SKY_UNIFORM: [f32; 3] = [0.05, 0.11, 0.23];

/// How far apart two texels may be before this file calls them different, as a
/// fraction of the brighter one.
///
/// One step of the storage format, near enough: the scene target is
/// `Rgba16Float`, so a texel carries eleven bits of mantissa and a step is
/// about `1e-3` of the value. Two sums that are equal in exact arithmetic but
/// were added in a different order can land on either side of a rounding
/// boundary, and this is what absorbs that and nothing larger.
///
/// **Measured, and the measurement was stronger than this**: the two frames of
/// `a_uniform_sky_is_exactly_a_brighter_flat_ambient` came back bit-identical
/// on this workstation's Vulkan adapter, worst disagreement exactly zero over
/// 69876 texel channels. The tolerance stays because that is one adapter's f32
/// summation order and the goldens' standing rule is not to re-bless per
/// driver — not because anything here needed the slack.
const SKY_TOLERANCE: f32 = 1.0e-3;

/// The scale floor a relative comparison is taken against, so that two texels
/// which are both nearly black are not called different by a ratio of two very
/// small numbers.
///
/// **It is not a background mask and must not be used as one.** It was, before
/// `crcbl_render::sky_pass` existed and the background was a single dark clear
/// colour; the sky now paints that background with radiances well above this,
/// and what separates the cube from the sky is a normals-view frame — see
/// [`unit_normals`].
const SKY_LIT_FLOOR: f32 = 0.02;

/// **A uniform sky is exactly a brighter flat ambient**, and by `π` times its
/// radiance.
///
/// The law the projection exists to satisfy, checked where it lands rather than
/// where it is computed. A sky with all three bands equal has no linear band,
/// so what it adds is its constant band alone — `π · L`, the irradiance a
/// constant environment of radiance `L` delivers — and that is the same number
/// `DirectionalLight::ambient` carries. Raise the ambient by it and drop the
/// sky, and the frames must agree.
///
/// **This is what pins the `π`.** Nothing else in the GPU path does: the
/// layout test says which lane each coefficient lands in and the shader guard
/// says the fragment stage evaluates it, but both would pass a projection off
/// by any constant factor. A frame that has to match one built the other way
/// cannot be.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_uniform_sky_is_exactly_a_brighter_flat_ambient() {
    let camera = mesh_camera(Projection::default());
    let uniform = crcbl::math::Vec3::from_array(SKY_UNIFORM);
    let lit_by_sky = sky_cube_hdr(
        &camera,
        Sky {
            zenith: uniform,
            horizon: uniform,
            ground: uniform,
        },
        &DirectionalLight::default(),
    );
    let default_light = DirectionalLight::default();
    let lit_by_ambient = sky_cube_hdr(
        &camera,
        Sky::NONE,
        &DirectionalLight {
            ambient: default_light.ambient + uniform * std::f32::consts::PI,
            ..default_light
        },
    );

    // **Surfaces only.** The claim is about the ambient term a sky delivers to
    // geometry, and the two frames deliberately disagree everywhere else: one
    // has a uniform sky painted across its background and the other has the
    // clear colour, because `Sky::NONE` adds no background pass at all. The
    // mask is a normals-view frame of the same scene, so no part of this knows
    // where the cube is.
    let normals = unit_normals(&normals_view_hdr(&camera));
    let mut compared = 0usize;
    let mut worst = 0.0f32;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            if normals[(y * MESH_EXTENT.0 + x) as usize].is_none() {
                continue;
            }
            let through_sky = lit_by_sky.pixel(x, y);
            let through_ambient = lit_by_ambient.pixel(x, y);
            for channel in 0..3 {
                let a = through_sky[channel];
                let b = through_ambient[channel];
                let scale = a.max(b);
                if scale < SKY_LIT_FLOOR {
                    continue;
                }
                compared += 1;
                worst = worst.max((a - b).abs() / scale);
            }
        }
    }
    // The floor could match nothing at all — a frame that rendered black would
    // pass every comparison above by making none of them.
    assert!(
        compared > 1_000,
        "only {compared} texel channels were above the lit floor, so this compared almost nothing"
    );
    assert!(
        worst <= SKY_TOLERANCE,
        "a uniform sky and the ambient it should equal disagree by {worst} at worst, over \
         {compared} texel channels; the projection's constant band is not π times the radiance"
    );
}

/// The scene's own normals, decoded from a normals-view frame.
///
/// `set_normals_view` writes `normal * 0.5 + 0.5` straight into the
/// `Rgba16Float` target, so this is that encoding undone. The length check is
/// what separates the cube from the background without knowing where either is:
/// a drawn texel carries a unit vector and the clear colour does not.
fn unit_normals(frame: &HdrTarget) -> Vec<Option<[f32; 3]>> {
    let mut out = Vec::with_capacity((MESH_EXTENT.0 * MESH_EXTENT.1) as usize);
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let encoded = frame.pixel(x, y);
            let normal = [
                encoded[0] * 2.0 - 1.0,
                encoded[1] * 2.0 - 1.0,
                encoded[2] * 2.0 - 1.0,
            ];
            let length = normal
                .iter()
                .fold(0.0f32, |sum, axis| sum + axis * axis)
                .sqrt();
            out.push(((length - 1.0).abs() <= 0.02).then_some(normal));
        }
    }
    out
}

/// **The sky's linear band scales with the surface's own `y`**: swap the zenith
/// and the ground, and an upward-facing surface changes while a sideways-facing
/// one does not move at all.
///
/// The constant band cannot see the swap — it is the two hemispheres' means
/// added, and addition does not care which order they arrive in — so the whole
/// difference between these two frames is the `y` coefficient evaluated against
/// each surface's normal. That makes the claim structural rather than a
/// magnitude: a fragment stage that read only `w`, or dropped the linear band
/// between the host and the dot product, moves *nothing*; one that took the
/// dot against the wrong lane, or against a constant, moves the sideways faces
/// too.
///
/// **Which texels are which comes out of a normals-view frame of the same
/// scene**, so no part of this test knows where the cube is or which way it
/// faces. The cube is placed unspun precisely so that both populations exist:
/// this camera sees its `+Y` face and its `+X` and `+Z` ones.
///
/// Ambient occlusion is forced off along with antialiasing and reflections,
/// which fog did not need: occlusion scales the ambient term and nothing else,
/// so leaving it on would put a per-texel factor on the very quantity under
/// test.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_skys_linear_band_moves_only_the_surfaces_facing_it() {
    // Bright above, dim below, and a horizon high enough that the constant band
    // stays above the linear one — otherwise the `max` in `sky_irradiance`
    // clamps a downward-facing surface and this stops being a clean swap.
    let bright = crcbl::math::Vec3::new(0.10, 0.22, 0.46);
    let dim = crcbl::math::Vec3::splat(0.02);
    let horizon = crcbl::math::Vec3::splat(0.12);
    let camera = mesh_camera(Projection::default());
    let sky_above = sky_cube_hdr(
        &camera,
        Sky {
            zenith: bright,
            horizon,
            ground: dim,
        },
        &DirectionalLight::default(),
    );
    let sky_below = sky_cube_hdr(
        &camera,
        Sky {
            zenith: dim,
            horizon,
            ground: bright,
        },
        &DirectionalLight::default(),
    );
    let normals = unit_normals(&normals_view_hdr(&camera));

    let mut facing_up = 0usize;
    let mut facing_up_still = 0usize;
    let mut sideways = 0usize;
    let mut sideways_moved = 0usize;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let Some(normal) = normals[(y * MESH_EXTENT.0 + x) as usize] else {
                continue;
            };
            let up = sky_above.pixel(x, y);
            let down = sky_below.pixel(x, y);
            let moved = (0..3).any(|channel| {
                let scale = up[channel].max(down[channel]).max(SKY_LIT_FLOOR);
                (up[channel] - down[channel]).abs() > scale * SKY_TOLERANCE
            });
            if normal[1] > 0.9 {
                facing_up += 1;
                if !moved {
                    facing_up_still += 1;
                }
            } else if normal[1].abs() < 0.05 {
                sideways += 1;
                if moved {
                    sideways_moved += 1;
                }
            }
        }
    }

    // Both populations have to exist, or each assertion below is a loop over
    // nothing dressed up as a check.
    assert!(
        facing_up > 500 && sideways > 500,
        "the normals frame found {facing_up} upward-facing and {sideways} sideways-facing texels; \
         this camera is supposed to see the cube's +Y face and its +X and +Z ones"
    );
    assert_eq!(
        facing_up_still, 0,
        "{facing_up_still} of {facing_up} upward-facing texels did not move when the sky was \
         turned upside down, so the linear band is not reaching them"
    );
    assert_eq!(
        sideways_moved, 0,
        "{sideways_moved} of {sideways} sideways-facing texels moved when the sky was turned \
         upside down, and a surface with no `y` in its normal has no linear band to receive"
    );
}

/// The same scene drawn as encoded normals, for [`unit_normals`] to read.
fn normals_view_hdr(camera: &crcbl::render::Camera) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_normals_view(true);
    crate::mesh_scene::place_cube_at(&mut renderer, crcbl::math::Mat4::IDENTITY);
    let mut hdr = Vec::new();
    let _ = render_mesh(&headless, &mut renderer, &mut pool, camera, Some(&mut hdr));
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// Where the slab under [`occlusion_probe_hdr`]'s cube sits.
///
/// **The cube alone is not a scene occlusion has anything to say about.** With
/// nothing under it the only occluded pixels are the rim its own silhouette
/// casts — four texels of this frame, measured — which is a population too thin
/// to tell a working pass from a rounding step. A floor gives the cube a contact
/// to close over, which is the same arrangement `crcbl::screenshot`'s
/// `Scene::Ao` uses and for the same reason.
///
/// Wide enough to fill the frame under the cube and thin enough that its own
/// sides are edge-on to this camera, and its top sits at `y = -0.5` — against
/// the unit cube's own underside, so the contact is a corner rather than a gap.
const OCCLUSION_FLOOR: (crcbl::math::Vec3, crcbl::math::Vec3) = (
    crcbl::math::Vec3::new(0.0, -0.55, 0.0),
    crcbl::math::Vec3::new(6.0, 0.1, 6.0),
);

/// One frame of a cube on a floor, drawn as a debug view rather than shaded.
///
/// [`normals_view_hdr`]'s two refusals, for its reasons, and one scene for all
/// three callers: the occlusion frames and the normals frame that says which of
/// their texels are geometry have to be the same arrangement, or the mask names
/// pixels the measurement never drew.
fn occlusion_probe_hdr(
    camera: &crcbl::render::Camera,
    normals: bool,
    occlusion: bool,
) -> HdrTarget {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(occlusion)),
        ..EffectRequest::default()
    });
    renderer.set_normals_view(normals);
    renderer.set_occlusion_view(!normals);
    crate::mesh_scene::place_cube_at(&mut renderer, crcbl::math::Mat4::IDENTITY);
    let (at, scale) = OCCLUSION_FLOOR;
    crate::mesh_scene::place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        crcbl::math::Mat4::from_scale(scale) * crcbl::math::Mat4::from_translation(at / scale),
    );
    let mut hdr = Vec::new();
    let _ = render_mesh(&headless, &mut renderer, &mut pool, camera, Some(&mut hdr));
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    HdrTarget(hdr)
}

/// The gradient sky the background reads as, chosen so no two bands and no two
/// channels are equal.
///
/// A frame drawn with this cannot pass by accident: a pass that took the wrong
/// band, swapped two of them, dropped the blend or wrote one channel's radiance
/// into another lands on a number that is nothing like the one predicted for
/// that pixel.
const SKY_ASYMMETRIC: Sky = Sky {
    zenith: crcbl::math::Vec3::new(0.18, 0.32, 0.75),
    horizon: crcbl::math::Vec3::new(0.62, 0.68, 0.80),
    ground: crcbl::math::Vec3::new(0.11, 0.09, 0.07),
};

/// The world-space direction the camera looks along through a pixel's centre.
///
/// The inverse of a projection applied to two depths and subtracted, which is
/// the one reconstruction that is right under a perspective *and* an
/// orthographic camera — see `sky.slang`, which makes the same argument for the
/// same reason. `Mat4::project_point3` is glam's own perspective divide rather
/// than a hand-written one, so the arithmetic under test and the arithmetic
/// checking it are not the same code written twice.
fn ray_through(camera: &crcbl::render::Camera, x: u32, y: u32) -> crcbl::math::Vec3 {
    let (width, height) = MESH_EXTENT;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a 256 by 192 frame is exact in f32"
    )]
    let aspect = width as f32 / height as f32;
    let inv_proj = camera.projection.matrix(aspect).inverse();
    let inv_view = camera.view().inverse();
    #[expect(
        clippy::cast_precision_loss,
        reason = "pixel coordinates inside a 256 by 192 frame are exact in f32"
    )]
    let ndc = crcbl::math::Vec2::new(
        (x as f32 + 0.5) / width as f32 * 2.0 - 1.0,
        1.0 - (y as f32 + 0.5) / height as f32 * 2.0,
    );
    // The reversed-Z near plane, and a depth between the planes. Both are
    // `sky.slang`'s `NDC_NEAR` and `NDC_MID`.
    let near = inv_proj.project_point3(ndc.extend(1.0));
    let beyond = inv_proj.project_point3(ndc.extend(0.5));
    inv_view.transform_vector3(beyond - near).normalize()
}

/// A camera pointed **up** at the sky rather than down at the scene.
///
/// [`mesh_camera`]'s own looks down at the origin from above, so every ray
/// through its frame leaves below the horizon — measured, not assumed: that
/// frame carries 38790 background texels and not one of them has a positive
/// `y`. One camera therefore cannot exercise both halves of the gradient, and
/// the zenith band is the half a scene camera never sees.
fn sky_camera_looking_up(projection: Projection) -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, 0.5, 3.0),
        target: crcbl::math::Vec3::new(0.0, 3.5, 0.0),
        up: crcbl::math::Vec3::Y,
        projection,
    }
}

/// **The background is the gradient the ray through it sees**, evaluated on the
/// host and compared texel by texel, from a camera aimed at each hemisphere.
///
/// `docs/plan/43-render-standards.md` §8's second half. The sky already reached
/// the ambient term and the reflection fallback; what this asks is whether the
/// pass that *draws* it puts the right radiance at the right pixel. Every part
/// of `sky.slang` is load-bearing for that: the unprojection, the view rotation,
/// the clamp, the cubic and which band sits at which end.
///
/// **Which texels are background comes out of a normals-view frame** of the same
/// scene through the same camera, so nothing here knows where the cube is — and
/// the cube's own texels are excluded rather than tested, since the sky pass is
/// depth-tested away from them.
///
/// **Two cameras, and each is required to deliver its own hemisphere.** The
/// scene camera looks down and sees only sky below the horizon; the second
/// looks up and sees only sky above it. Asserting the count per camera is what
/// stops this passing while one of the gradient's two arms is never entered —
/// which is the gap `docs/backlog.md` recorded when the march's fallback was the
/// only thing evaluating a sky on a GPU.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_background_is_the_gradient_the_ray_through_it_sees() {
    let gradient = crcbl_shaders::sky::SkyGradient {
        zenith: SKY_ASYMMETRIC.zenith.to_array(),
        horizon: SKY_ASYMMETRIC.horizon.to_array(),
        ground: SKY_ASYMMETRIC.ground.to_array(),
    };
    // `true` for the camera whose rays must leave above the horizon.
    for (aimed_up, camera) in [
        (false, mesh_camera(Projection::default())),
        (true, sky_camera_looking_up(Projection::default())),
    ] {
        let drawn = sky_cube_hdr(&camera, SKY_ASYMMETRIC, &DirectionalLight::default());
        let normals = unit_normals(&normals_view_hdr(&camera));

        let mut wanted = 0usize;
        let mut worst = 0.0f32;
        let mut worst_at = (0u32, 0u32);
        for y in 0..MESH_EXTENT.1 {
            for x in 0..MESH_EXTENT.0 {
                if normals[(y * MESH_EXTENT.0 + x) as usize].is_some() {
                    continue;
                }
                let direction = ray_through(&camera, x, y);
                if (direction.y >= 0.0) == aimed_up {
                    wanted += 1;
                }
                let expected = gradient.radiance(direction.to_array());
                let drawn = drawn.pixel(x, y);
                for channel in 0..3 {
                    let error = (drawn[channel] - expected[channel]).abs()
                        / expected[channel].max(SKY_LIT_FLOOR);
                    if error > worst {
                        worst = error;
                        worst_at = (x, y);
                    }
                }
            }
        }

        let half = if aimed_up { "above" } else { "below" };
        assert!(
            wanted > 1_000,
            "the camera aimed {half} the horizon found only {wanted} background texels on that \
             side, so that arm of the gradient is not being exercised"
        );
        assert!(
            worst <= SKY_TOLERANCE,
            "the drawn background and the gradient disagree by {worst} at worst, at texel \
             {worst_at:?}, on the camera aimed {half} the horizon"
        );
    }
}

/// **A renderer nobody gave a sky draws no background pass at all**, and the
/// frame behind its geometry is exactly [`crcbl::render::SCENE_CLEAR`].
///
/// The off switch, checked where it lands. `crcbl_render::sky_pass` argues that
/// a black gradient drawn over the clear colour would be a *change* to every
/// frame this workspace has ever blessed rather than an absence, so the pass is
/// conditional on the sky and not on a shader branch — and this is what says the
/// condition holds. It is also what would catch the pass drawing over geometry
/// in the other direction: the clear colour has to survive on exactly the texels
/// the gradient test found were background.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn no_sky_leaves_the_background_at_the_clear_colour() {
    let camera = mesh_camera(Projection::default());
    let drawn = sky_cube_hdr(&camera, Sky::NONE, &DirectionalLight::default());
    let normals = unit_normals(&normals_view_hdr(&camera));

    let mut background = 0usize;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            if normals[(y * MESH_EXTENT.0 + x) as usize].is_some() {
                continue;
            }
            background += 1;
            let texel = drawn.pixel(x, y);
            for channel in 0..3 {
                assert!(
                    (texel[channel] - crcbl::render::SCENE_CLEAR[channel]).abs() <= 1.0e-4,
                    "texel ({x}, {y}) is {texel:?} where no sky should have left it {:?}",
                    crcbl::render::SCENE_CLEAR
                );
            }
        }
    }
    assert!(
        background > 1_000,
        "only {background} texels were background, so this asserted almost nothing"
    );
}

/// **The occlusion view draws the channel the occlusion pass wrote**, and draws
/// the placeholder honestly when no pass wrote one.
///
/// Two frames of one scene, separated by `RenderEffects::AMBIENT_OCCLUSION`
/// alone, and the claim is a pair rather than either half:
///
/// * With the pass **off** every drawn texel is exactly one. That is the 1×1
///   white image the renderer binds in place of a computed channel, and it is
///   what says the branch reached the bound texture at all — a branch that
///   returned a literal white would pass this half and fail the other.
/// * With the pass **on** some drawn texels are below one. That is what says
///   the view reads the *computed* channel rather than the placeholder — a
///   `set_occlusion_view` wired to nothing leaves the two frames identical.
///
/// Which texels are drawn comes out of a normals-view frame of the same scene,
/// so no part of this test knows where the cube is: the background is the sky
/// pass's pixels and `mesh.slang`'s branch never runs on them.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_occlusion_view_draws_the_channel_and_not_a_constant() {
    let camera = mesh_camera(Projection::default());
    let normals = unit_normals(&occlusion_probe_hdr(&camera, true, false));
    let without = occlusion_probe_hdr(&camera, false, false);
    let with = occlusion_probe_hdr(&camera, false, true);

    let mut drawn = 0usize;
    let mut placeholder = 0usize;
    let mut occluded = 0usize;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            if normals[(y * MESH_EXTENT.0 + x) as usize].is_none() {
                continue;
            }
            drawn += 1;
            let off = without.pixel(x, y);
            // Grey, not a channel: the branch writes one value into all three,
            // so a frame that put the occlusion in red alone is not this frame.
            assert!(
                off[0] == off[1] && off[1] == off[2],
                "the occlusion view is grey by construction, and ({x}, {y}) reads {off:?}",
            );
            if (off[0] - 1.0).abs() <= OCCLUSION_VIEW_TOLERANCE {
                placeholder += 1;
            }
            if with.pixel(x, y)[0] < 1.0 - OCCLUSION_VIEW_STEP {
                occluded += 1;
            }
        }
    }
    eprintln!(
        "crcbl mesh e2e: occlusion view — {drawn} drawn texels, {placeholder} white without the \
         pass, {occluded} occluded with it"
    );
    assert!(
        drawn > 0,
        "the normals frame found no geometry, so neither half of this has a population"
    );
    assert_eq!(
        placeholder, drawn,
        "with the occlusion pass off every drawn texel must read the 1x1 white placeholder"
    );
    assert!(
        occluded > 0,
        "with the pass on the view is still uniformly white, so it is not reading the channel"
    );
}

/// How far a texel of the occlusion view may sit from one and still count as the
/// placeholder.
///
/// The value travels as an `R8Unorm` texel through an `Rgba16Float` target and a
/// tonemap that is the identity at the default exposure, so an exact one is what
/// this should measure; the tolerance is there for the float target's own
/// rounding and is far under [`OCCLUSION_VIEW_STEP`].
const OCCLUSION_VIEW_TOLERANCE: f32 = 1e-3;

/// How far below one a texel must fall to count as occluded.
///
/// An `R8Unorm` channel steps by 1/255, so this is several of its steps: what it
/// separates is a surface the pass darkened from one the pass rounded.
const OCCLUSION_VIEW_STEP: f32 = 0.02;
