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
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube, render_mesh};
use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferUsage, CommandEncoderDesc,
    MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl::render::{
    EffectOverride, EffectRequest, ExposureBuffers, ForwardRenderer, Projection, RenderEffects,
    TransientPool,
};
use crcbl::shaders::exposure::{BIN_COUNT, BIN_STRIDE, MEASURED_SIZE, bin_of, luma, measure};

/// How many texels the cube frame covers, which is what both histograms have to
/// total: the dispatch is one invocation per texel of the scene target, and the
/// host bins the same target.
const TEXELS: u32 = MESH_EXTENT.0 * MESH_EXTENT.1;

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
fn draw(auto: bool, exposure: f32) -> Frame {
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
    let frame = draw(true, crcbl_shaders::tonemap::DEFAULT_EXPOSURE);
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
    let auto = draw(true, crcbl_shaders::tonemap::DEFAULT_EXPOSURE);
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

    let manual = draw(false, exposure);
    assert!(
        manual.image == auto.image,
        "a frame drawn by hand at the measured exposure has to be the frame the \
         pass drew; the tonemap is reading something else"
    );

    let default = draw(false, crcbl_shaders::tonemap::DEFAULT_EXPOSURE);
    assert!(
        default.image != auto.image,
        "this fixture has to be one auto-exposure changes, or the equality above \
         holds for a pass that does nothing: it measured {exposure}"
    );
}
