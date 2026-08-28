//! The auto-exposure histogram, read out of the buffer the frame left it in and
//! checked bin by bin against `crcbl_shaders::exposure` on the host.
//!
//! `docs/plan/43-render-standards.md` §6's rung is two things — a histogram of
//! the finished frame and a reduce that turns it into one number — and a
//! picture cannot tell them apart. A histogram that binned the wrong texels and
//! a reduce that read the wrong window compose to *an* exposure, and the frame
//! it draws is a perfectly plausible one. So this module splits them: it copies
//! the bins back and rebuilds them from the same `Rgba16Float` target the
//! shader read, then copies the measured exposure back and compares it against
//! [`measure`] fed the bins the GPU actually produced.
//!
//! **And then it checks the number reached the picture**, which neither of
//! those can: the same frame is drawn again with the pass off and the manual
//! exposure set to what the reduce measured, and the two images have to be
//! identical. A measurement the tonemap ignored would pass both halves above
//! and change nothing on screen.

use crate::harness::{Headless, poisoned};
use crate::hdr::HdrTarget;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube, render_mesh_lit};
use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferUsage, CommandEncoderDesc,
    MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl::math::Vec3;
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, ExposureAdaptation, ExposureBuffers,
    ForwardRenderer, Projection, RenderEffects, TransientPool,
};
use crcbl::shaders::exposure::{
    BIN_COUNT, BIN_STRIDE, MEASURED_SIZE, adapt, bin_of, luma, measure,
};

/// How many texels the cube frame covers, which is what both histograms have to
/// total: the dispatch is one invocation per texel of the scene target, and the
/// host bins the same target.
const TEXELS: u32 = MESH_EXTENT.0 * MESH_EXTENT.1;

/// A light bright enough that the frame it draws wants *less* exposure than the
/// ring starts at, so the adaptation is travelling downward.
///
/// The multiplier is measured rather than picked: under the default light this
/// fixture's window averages a luminance below middle grey and asks for an
/// exposure above one, and the window is on the cube rather than on the
/// background — so scaling the light scales it. Four is far enough past the
/// crossing that the arm is not sitting on it.
const BRIGHT: DirectionalLight = DirectionalLight {
    direction: Vec3::new(0.4, 0.8, 0.45),
    color: Vec3::new(4.8, 4.4, 3.8),
    ambient: Vec3::new(0.4, 0.44, 0.56),
};

/// One frame, with whatever the auto-exposure pass left behind beside it.
struct Frame {
    /// The tonemapped swapchain image, which is the only thing a player sees.
    image: crcbl_golden::Image,
    /// The `Rgba16Float` scene target the histogram pass binned — the same
    /// image, read a second way, so the host bins exactly the texels the shader
    /// did rather than a re-render of them.
    hdr: HdrTarget,
    /// `[bin]`: the counts `histogramMain` accumulated, and the exposure
    /// `reduceMain` wrote. [`None`] when the frame ran without the pass, where
    /// the buffers hold whatever the ring last left in them.
    measured: Option<(Vec<u32>, f32)>,
}

/// Draws the cube scene once, either through the auto-exposure pass or at a
/// fixed exposure, and reads back everything the assertions below need.
///
/// **The antialiasing resolve is refused**, for [`hdr`](crate::hdr)'s reason
/// and one of this module's own: two of these frames are compared to each other
/// texel for texel, and a resolve is one more pass between the exposure and the
/// pixel that would have to be identical for the comparison to mean what it
/// says.
fn draw(auto: bool, exposure: f32, adaptation: Option<ExposureAdaptation>) -> Frame {
    draw_lit(auto, exposure, adaptation, &DirectionalLight::default())
}

/// The same frame under a light of the caller's choosing.
///
/// The only caller that reaches past [`draw`] is the one whose claim is about a
/// *darkening*: under the default light this fixture measures an exposure above
/// where the ring starts, so every arm that uses it is travelling one way.
fn draw_lit(
    auto: bool,
    exposure: f32,
    adaptation: Option<ExposureAdaptation>,
    light: &DirectionalLight,
) -> Frame {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::AUTO_EXPOSURE, Some(auto)),
        ..EffectRequest::default()
    });
    renderer.set_exposure(exposure);
    renderer.set_exposure_adaptation(adaptation);
    place_cube(&mut renderer);
    let mut hdr = Vec::new();
    let image = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        light,
        Some(&mut hdr),
    );

    let device = headless.device.as_ref();
    let measured = auto.then(|| read_back(&headless, renderer.exposure_buffers(renderer.frame())));

    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    assert_eq!(
        hdr.len(),
        (TEXELS * 8) as usize,
        "the scene target came back the wrong size, so every texel binned below \
         is at the wrong offset"
    );
    Frame {
        image,
        hdr: HdrTarget(hdr),
        measured,
    }
}

/// Copies the bins and the measured exposure out of the buffers the frame left
/// them in.
///
/// One encoder for both, and one barrier each way: [`crate::froxels`]'s shape,
/// and for the same reason — both buffers end a frame in
/// [`ResourceState::ShaderRead`], the histogram because the reduce read it and
/// the exposure because the tonemap did.
fn read_back(headless: &Headless, buffers: ExposureBuffers) -> (Vec<u32>, f32) {
    let device = headless.device.as_ref();
    let bin_bytes = (BIN_COUNT as usize * BIN_STRIDE) as u64;
    let staging = |label: &str, size: u64| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let copies = [
        (
            buffers.histogram,
            staging("exposure bins", bin_bytes),
            bin_bytes,
        ),
        (
            buffers.measured,
            staging("measured exposure", MEASURED_SIZE as u64),
            MEASURED_SIZE as u64,
        ),
    ];

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("exposure copy"),
        queue: headless.queue,
    });
    let barriers = |from: ResourceState, to: ResourceState| {
        copies.map(|(buffer, _, _)| BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        })
    };
    let out = barriers(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barriers(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    for (buffer, target, size) in copies {
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: buffer,
            src_offset: 0,
            dst: target,
            dst_offset: 0,
            size,
        });
    }
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let read = |index: usize| {
        let (_, target, size) = copies[index];
        let mut bytes = poisoned(size as usize);
        headless.readback(target, size, &mut bytes);
        device.destroy_buffer(target);
        bytes
    };
    let bins = read(0);
    let exposure = read(1);
    device.destroy_command_buffer(commands);

    (
        bins.chunks_exact(BIN_STRIDE)
            .map(|bin| u32::from_le_bytes(bin.try_into().expect("a bin is four bytes")))
            .collect(),
        f32::from_le_bytes(exposure.try_into().expect("the exposure is four bytes")),
    )
}

/// The histogram the host builds from the same target, bin for bin.
fn host_histogram(hdr: &HdrTarget) -> Vec<u32> {
    let mut bins = vec![0u32; BIN_COUNT as usize];
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [r, g, b, _] = hdr.pixel(x, y);
            bins[bin_of(luma([r, g, b])) as usize] += 1;
        }
    }
    bins
}

/// **The histogram is of this frame**, and of every texel of it.
///
/// The bins are rebuilt here from the scene target the shader read, through
/// [`bin_of`] — which is integer arithmetic on the exponent field, so the two
/// sides agree exactly wherever the luminance itself does. The one place they
/// can differ is a texel whose luminance sits within a unit in the last place
/// of a bin edge: the shader's `dot` is free to fuse its multiplies and the
/// host's is not, so such a texel can land either side. The totals cannot
/// differ at all, which is what catches a dispatch that covered the wrong
/// extent.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_histogram_bins_every_texel_of_the_frame_the_host_reads_back() {
    let frame = draw(true, crcbl_shaders::tonemap::DEFAULT_EXPOSURE, None);
    let (gpu, _) = frame
        .measured
        .expect("the pass ran, so it wrote its buffers");
    let host = host_histogram(&frame.hdr);

    assert_eq!(
        gpu.iter().sum::<u32>(),
        TEXELS,
        "every texel of the scene target has to land in exactly one bin"
    );
    assert_eq!(
        host.iter().sum::<u32>(),
        TEXELS,
        "the host binned a different number of texels than the frame holds"
    );

    let moved: u32 = gpu
        .iter()
        .zip(&host)
        .map(|(gpu, host)| gpu.abs_diff(*host))
        .sum();
    let occupied = gpu.iter().filter(|count| **count > 0).count();
    eprintln!(
        "{}: {occupied} bins occupied, {moved} texels of {TEXELS} on the other side of an edge",
        crate::SUITE
    );
    assert!(
        occupied > 1,
        "a frame that landed in one bin would make every comparison here vacuous"
    );
    // The bound is measured, not chosen: on radv this run moves nothing at all.
    // It is a *fraction* rather than a count so the claim survives a change of
    // extent, and small enough that a bin's worth of texels in the wrong place
    // fails it.
    assert!(
        moved * 1000 <= TEXELS,
        "at most a thousandth of the frame may sit on the other side of a bin \
         edge from the host's arithmetic; {moved} of {TEXELS} did"
    );
}

/// **The reduce is the host's, and the tonemap applied what it wrote.**
///
/// Three claims a single frame cannot make. The exposure in the buffer is
/// [`measure`] of the bins beside it, so a reduce that read the wrong window
/// fails here rather than composing with a wrong histogram into something
/// plausible. Drawn again at that exposure with the pass off, the picture is
/// the same one — so the number reached the fragment shader. And drawn at the
/// default it is a *different* picture, without which the first two would hold
/// just as well for a pass that measured 1.0 and changed nothing.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_measured_exposure_is_the_one_the_tonemap_applied() {
    let auto = draw(true, crcbl_shaders::tonemap::DEFAULT_EXPOSURE, None);
    let (bins, exposure) = auto
        .measured
        .as_ref()
        .map(|(bins, exposure)| (bins.clone(), *exposure))
        .expect("the pass ran, so it wrote its buffers");
    let wanted = measure(&bins);
    eprintln!(
        "{}: the reduce measured {exposure}, the host makes it {wanted}",
        crate::SUITE
    );
    assert!(
        (exposure - wanted).abs() <= wanted * 1e-5,
        "the reduce has to compute what `crcbl_shaders::exposure::measure` does \
         from the same bins: it wrote {exposure}, the host makes it {wanted}"
    );

    let manual = draw(false, exposure, None);
    assert!(
        manual.image == auto.image,
        "a frame drawn by hand at the measured exposure has to be the frame the \
         pass drew; the tonemap is reading something else"
    );

    let default = draw(false, crcbl_shaders::tonemap::DEFAULT_EXPOSURE, None);
    assert!(
        default.image != auto.image,
        "this fixture has to be one auto-exposure changes, or the equality above \
         holds for a pass that does nothing: it measured {exposure}"
    );
}

/// **The exposure steps toward its measurement instead of landing on it**, and
/// the step starts somewhere defined.
///
/// Every arm here is a *first* frame — each fixture opens its own device — so
/// what the reduce adapts away from is whatever `crcbl_render::exposure` put in
/// the ring before any frame existed. That makes this two claims at once: the
/// start-up fill landed, and the step is the one
/// [`adapt`](crcbl::shaders::exposure::adapt) predicts. A ring left
/// uninitialised would still produce *an* exposure, and it would differ between
/// runs and between drivers.
///
/// The frozen arm is the sharper of the two: a blend of zero has to leave the
/// exposure exactly where the fill put it, so the frame it draws is the frame
/// drawn at the default exposure with no auto-exposure in it at all — which is
/// checked as a picture and not just as a number.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_exposure_steps_toward_its_measurement_from_where_the_ring_started() {
    let default = crcbl_shaders::tonemap::DEFAULT_EXPOSURE;
    let snapped = draw(true, default, None);
    let (bins, target) = snapped
        .measured
        .as_ref()
        .map(|(bins, exposure)| (bins.clone(), *exposure))
        .expect("the pass ran, so it wrote its buffers");
    assert!(
        (target - default).abs() > 0.05,
        "this fixture has to measure an exposure away from the default, or a \
         step toward it is indistinguishable from no step: it measured {target}"
    );

    // Half the distance in this frame: a rate of one over a frame that lasted
    // half a second. The two are separate numbers on purpose — a blend read as
    // the rate alone, or as the delta alone, lands somewhere else.
    let half = ExposureAdaptation {
        brighten: 1.0,
        darken: 1.0,
        delta: 0.5,
    };
    let stepped = draw(true, default, Some(half))
        .measured
        .map(|(_, exposure)| exposure)
        .expect("the pass ran");
    let wanted = adapt(default, measure(&bins), 0.5, 0.5);
    eprintln!(
        "{}: snapping measures {target}, half a step measures {stepped}, the host \
         makes that {wanted}",
        crate::SUITE
    );
    assert!(
        (stepped - wanted).abs() <= wanted * 1e-5,
        "half a step from the ring's start toward {target} is {wanted}; the reduce \
         wrote {stepped}"
    );

    // A rate of zero: the exposure stays where the start-up fill put it, which
    // is the default, so this frame is the one drawn with no auto-exposure.
    let frozen = draw(
        true,
        default,
        Some(ExposureAdaptation {
            brighten: 0.0,
            darken: 0.0,
            delta: 0.5,
        }),
    );
    let held = frozen
        .measured
        .as_ref()
        .map(|(_, exposure)| *exposure)
        .expect("the pass ran");
    assert_eq!(
        held, default,
        "a blend of zero has to leave the exposure exactly where the ring was \
         filled, and the fill wrote the default"
    );
    assert!(
        frozen.image == draw(false, default, None).image,
        "a frozen adaptation at the ring's starting value has to draw the frame \
         this engine draws with no auto-exposure at all"
    );
}

/// **Which of the two rates a frame reads is decided by the direction it is
/// travelling**, and the shader decides it, not the host.
///
/// Every other arm here hands both rates the same number, so a reduce that read
/// the wrong one would agree with all of them. This fixture measures an
/// exposure *above* where the ring starts, so the travel is a brightening: the
/// arm that gives brightening a rate and darkening none has to arrive, and the
/// arm that gives it the other way round has to not move at all.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_direction_of_travel_picks_the_rate() {
    let default = crcbl_shaders::tonemap::DEFAULT_EXPOSURE;
    let measured_at = |brighten: f32, darken: f32| {
        draw(
            true,
            default,
            Some(ExposureAdaptation {
                brighten,
                darken,
                delta: 1.0,
            }),
        )
        .measured
        .map(|(_, exposure)| exposure)
        .expect("the pass ran")
    };
    let target = measured_at(1.0, 1.0);
    assert!(
        target > default,
        "this fixture has to measure an exposure above the ring's start, or \
         neither arm below says which rate was read: it measured {target}"
    );
    assert_eq!(
        measured_at(1.0, 0.0),
        target,
        "a brightening frame reads the brightening rate, and a whole step of it \
         arrives at the target"
    );
    assert_eq!(
        measured_at(0.0, 1.0),
        default,
        "a brightening frame must not read the darkening rate, and with no \
         brightening rate it stays where the ring started"
    );

    // And the other way round, under a light bright enough that the frame wants
    // *less* exposure than the ring starts with. Without this arm a shader that
    // read `brighten_blend` whatever the direction would pass every assertion
    // above — measured, by making that mistake and running this suite.
    let dark_at = |brighten: f32, darken: f32| {
        draw_lit(
            true,
            default,
            Some(ExposureAdaptation {
                brighten,
                darken,
                delta: 1.0,
            }),
            &BRIGHT,
        )
        .measured
        .map(|(_, exposure)| exposure)
        .expect("the pass ran")
    };
    let below = dark_at(1.0, 1.0);
    assert!(
        below < default,
        "the bright fixture has to measure an exposure below the ring's start, or \
         the two arms below say nothing: it measured {below}"
    );
    assert_eq!(
        dark_at(0.0, 1.0),
        below,
        "a darkening frame reads the darkening rate, and a whole step of it \
         arrives at the target"
    );
    assert_eq!(
        dark_at(1.0, 0.0),
        default,
        "a darkening frame must not read the brightening rate, and with no \
         darkening rate it stays where the ring started"
    );
}

/// **The frame before this one is the frame it adapts away from**, which is a
/// claim no single frame can make.
///
/// Every arm above draws one frame, and on a first frame every slot of the ring
/// still holds the value the start-up fill wrote — so a reduce reading its
/// *own* slot instead of the one behind it would agree with all of them. This
/// draws two frames through one renderer and predicts the second from the first
/// rather than from the fill: two steps of the same size, compounding.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn each_frame_adapts_away_from_the_frame_before_it() {
    let half = ExposureAdaptation {
        brighten: 1.0,
        darken: 1.0,
        delta: 0.5,
    };
    let (exposures, bins) = draw_frames(3, half);
    let target = measure(&bins);

    // Each step is half of what is left, from the fill's default: the sequence
    // is the fill's value walked toward the target and never reaching it.
    let mut wanted = crcbl_shaders::tonemap::DEFAULT_EXPOSURE;
    for (index, measured) in exposures.iter().enumerate() {
        wanted = adapt(wanted, target, 0.5, 0.5);
        eprintln!(
            "{}: frame {index} measured {measured}, the host makes it {wanted}",
            crate::SUITE
        );
        assert!(
            (measured - wanted).abs() <= wanted * 1e-5,
            "frame {index} has to be half a step on from frame {}: the host makes \
             that {wanted} and the reduce wrote {measured}",
            index.wrapping_sub(1)
        );
    }
    assert!(
        exposures
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).abs() > f32::EPSILON),
        "the exposures have to be moving, or a reduce that ignored the step would \
         match every one of them: {exposures:?}"
    );
}

/// Draws `count` frames through one renderer and reads the measured exposure
/// back after each, with the last frame's bins beside them.
///
/// One fixture for all of them, unlike [`draw`]: the whole point is that the
/// ring carries a value from one frame into the next, and a second `Headless`
/// would be a second ring starting again from the fill.
fn draw_frames(count: usize, adaptation: ExposureAdaptation) -> (Vec<f32>, Vec<u32>) {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::AUTO_EXPOSURE, Some(true)),
        ..EffectRequest::default()
    });
    renderer.set_exposure_adaptation(Some(adaptation));
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());

    let mut exposures = Vec::with_capacity(count);
    let mut bins = Vec::new();
    for _ in 0..count {
        let _ = render_mesh_lit(
            &headless,
            &mut renderer,
            &mut pool,
            &camera,
            &DirectionalLight::default(),
            None,
        );
        let (frame_bins, exposure) =
            read_back(&headless, renderer.exposure_buffers(renderer.frame()));
        exposures.push(exposure);
        bins = frame_bins;
    }

    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    (exposures, bins)
}
