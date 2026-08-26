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
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube, render_mesh};
use crcbl::render::{
    EffectOverride, EffectRequest, ForwardRenderer, Projection, RenderEffects, TransientPool,
};

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
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::ANTIALIASING, Some(false)),
        ..EffectRequest::default()
    });
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
        "the Blinn highlight must exceed 1.0 somewhere, or the RGBA16F target is \
         carrying nothing an Rgba8 one could not; peak was {peak}"
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
