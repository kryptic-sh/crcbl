//! The culling-statistics ring, copied back on the host — not a test module, and
//! not a test target: `tests/gpu_scene/` holds no `main.rs`, so Cargo compiles
//! nothing here on its own.
//!
//! **Two suites pull this in with `#[path]`** — `tests/draw_gen_e2e/` and
//! `tests/forward_e2e/` — because both read counters out of the one buffer topic
//! 03 §3.6 permits a readback from, and the barriers around that copy are the
//! part worth getting right once.
//!
//! Its own file rather than more of [`mesh_scene`](crate::mesh_scene) because
//! the third suite on that scene, `tests/mesh_e2e/`, reads no counter at all: a
//! scene and a statistics copy in one file left this symbol dead code in that
//! binary, which is the same seam that put the fixture in `harness.rs` and the
//! scene beside it.

use crate::harness::{Headless, POISON};
use crcbl::hal::{
    Barriers, BufferDesc, BufferUsage, CommandEncoderDesc, MemoryLocation, ResourceState,
    SubmitInfo,
};
use crcbl::render::ForwardRenderer;

/// One word of the frame's culling statistics, copied back after the frame that
/// wrote it.
///
/// Topic 03 §3.6's ring is one buffer with a counter per producer in it — the
/// cull pass's survivors, the amplification stage's, and `light_cluster.slang`'s
/// refused assignments — so reading any of them is the same copy with a
/// different offset. One helper rather than one per counter, because the
/// barriers around it are the part worth getting right once.
pub(crate) fn read_stats_word(
    headless: &Headless,
    renderer: &ForwardRenderer,
    word_index: u32,
) -> u32 {
    let device = headless.device.as_ref();
    let stats = renderer.draws().visible_count(renderer.frame());
    let word = u64::from(word_index) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("culling statistics readback"),
            size: 4,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("culling statistics copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl::hal::BufferBarrier {
            buffer: stats,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: stats,
        src_offset: word,
        dst: staging,
        dst_offset: 0,
        size: 4,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut bytes = [POISON; 4];
    headless.readback(staging, 4, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    u32::from_le_bytes(bytes)
}
