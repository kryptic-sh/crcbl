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
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube, render_mesh, render_mesh_lit};
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, Fog, ForwardRenderer, Projection,
    RenderEffects, Sky, TransientPool,
};
use crcbl_shaders::tonemap::TonemapCurve;

/// The frame's `Rgba16Float` scene target, as the bytes the copy produced.
struct HdrTarget(Vec<u8>);

impl HdrTarget {
    /// The linear HDR value at a texel, decoded from `Rgba16Float`.
    fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
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
fn sky_cube_hdr(sky: Sky, light: &DirectionalLight) -> HdrTarget {
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
        &mesh_camera(Projection::default()),
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

/// Below this the texel is background, which no sky reaches: nothing draws the
/// gradient yet, so the clear colour is the same in every frame here and
/// including it would dilute every comparison with texels that cannot differ.
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
    let uniform = crcbl::math::Vec3::from_array(SKY_UNIFORM);
    let lit_by_sky = sky_cube_hdr(
        Sky {
            zenith: uniform,
            horizon: uniform,
            ground: uniform,
        },
        &DirectionalLight::default(),
    );
    let default_light = DirectionalLight::default();
    let lit_by_ambient = sky_cube_hdr(
        Sky::NONE,
        &DirectionalLight {
            ambient: default_light.ambient + uniform * std::f32::consts::PI,
            ..default_light
        },
    );

    let mut compared = 0usize;
    let mut worst = 0.0f32;
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
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
    let sky_above = sky_cube_hdr(
        Sky {
            zenith: bright,
            horizon,
            ground: dim,
        },
        &DirectionalLight::default(),
    );
    let sky_below = sky_cube_hdr(
        Sky {
            zenith: dim,
            horizon,
            ground: bright,
        },
        &DirectionalLight::default(),
    );
    let normals = unit_normals(&normals_view_hdr());

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
fn normals_view_hdr() -> HdrTarget {
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
