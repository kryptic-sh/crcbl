//! The culling statistics, off the GPU and onto a delayed ring.
//!
//! ```text
//!  add_copy_pass ─▶ copy pass: cull stats ──▶ slot k of the ring   (frame f)
//!                                                  │
//!  begin_frame ─────── request_readback(slot k) ───┘               (frame f+1)
//!                                                  │  … the ring turns …
//!  begin_frame ─────── poll_readback(slot k) ──────┘               (frame f+N)
//!                              │
//!                              ▼  latest() ──▶ ForwardRenderer::counters()
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.6's culling-stats readback, and
//! `docs/plan/40-profiling.md`'s eighth missing piece: "the culling stats never
//! leave the GPU … there is no staging buffer, no copy inside the frame graph,
//! and no consumer". This is all three.
//!
//! # The latency *is* the synchronisation
//!
//! [`crate::timing::PassTimers`] is the pattern, one resource down: a ring one
//! longer than the frames the loop keeps in flight, and a slot is only read once
//! it has come back round — by which point the submission that filled it has
//! certainly completed. There is no fence here, no `wait_idle`, and no poll in a
//! loop: [`poll_readback`](Device::poll_readback) is called **once** per slot per
//! turn of the ring, and a slot that is somehow still pending is dropped rather
//! than waited for.
//!
//! So the number is about a frame [`CullStatsRing::latency`] frames ago, and it
//! says which one — [`CullStats::frame`] carries it, and
//! [`FrameCounters`](crate::counters::FrameCounters) puts it on the panel as its
//! own row. A latent counter beside live ones without saying so is the whole
//! reason that row exists.
//!
//! # Three points in the frame, and why the request is not at the copy
//!
//! A readback "covers work **already submitted** when
//! [`request_readback`](Device::request_readback) is called". The copy is
//! recorded, not submitted, when [`CullStatsRing::add_copy_pass`] returns — the
//! encoder is still open — so requesting there would be the caller bug the seam
//! names: "requesting before submitting the copy that fills the buffer … reads
//! stale bytes". The request therefore happens at the *next*
//! [`CullStatsRing::begin_frame`], by which point the frame that recorded the
//! copy has been submitted, because a frame loop submits the graph it executed
//! before it starts another one.
//!
//! And it happens only if the copy pass's body actually ran. A graph that was
//! built and then dropped — a compile error, a swapchain that went out of date —
//! recorded nothing, and a request against that slot would return the bytes the
//! *previous* turn of the ring left in it and report them as this frame's. So
//! the body stamps the frame number it recorded — from inside
//! [`execute`](crate::graph::CompiledGraph::execute), through the one piece of
//! shared state this type has — and a slot nothing stamped is never read.
//!
//! # Degrading rather than breaking
//!
//! [`CullStatsRing::new`] answers [`None`] if the device will not give it the
//! buffers, and any seam error afterwards switches the ring off for good — one
//! log line, and [`latest`](CullStatsRing::latest) is [`None`] from then on. The
//! panel says `indirect` in that case, which is what it said before this ring
//! existed. **A zero would be worse than nothing**: it reads as "the cull kept
//! nothing", which is a scene that vanished rather than a counter that never
//! arrived.
//!
//! # One ring, for the camera's cull only
//!
//! A frame runs several culls — the camera's, and one per shadow cascade and
//! shadowed light — and each is its own [`DrawGen`](crate::draw_gen::DrawGen)
//! with its own statistics buffer. Only the camera's is read: it is the one
//! whose survivors are "the culling win", and summing the shadow culls into it
//! would produce a number larger than the instance count that means nothing at
//! all. A shadow cull's survivors are a different question about a different
//! frustum, and the day something asks it, it asks with a ring of its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crcbl_hal::{
    BufferCopy, BufferDesc, BufferHandle, BufferUsage, Device, MemoryLocation, ReadbackDesc,
    ReadbackHandle, ReadbackState, ResourceState,
};
use crcbl_shaders::cull;

use crate::graph::{BufferId, ImportedBuffer, RenderGraph};

/// Bytes of one slot: the whole statistics buffer, in one copy.
///
/// Every counter the frame produces lives in that one buffer — see
/// [`cull::STATS_WORDS`] — so reading all of it is the same single copy as
/// reading any of it, and a slot that held a prefix would have to grow the day
/// something asked for a word it had left behind.
const SLOT_BYTES: u64 = cull::STATS_WORDS as u64 * 4;

/// What one frame's culling actually kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CullStats {
    /// Instances the camera's cull kept.
    ///
    /// The **true** survivor count, which can exceed
    /// [`DrawGen::visible_capacity`](crate::draw_gen::DrawGen::visible_capacity)
    /// — the cull counts every survivor and writes the prefix that fits, so a
    /// number larger than the list is a scene that outgrew it rather than an
    /// error here.
    pub instances: u64,
    /// Clusters the amplification stage kept, or [`None`] where nothing counted
    /// them.
    ///
    /// [`None`] on the two indirect geometry paths and on a device with
    /// `Features::MESH_SHADER` and no `Features::TASK_SHADER`: there is no
    /// amplification stage in those frames, so the word keeps the zero the
    /// clearing pass wrote. Reporting that zero as a count would say "every
    /// cluster was rejected" about a frame that drew all of them.
    pub clusters: Option<u64>,
    /// Which frame these came from — [`CullStatsRing::latency`] frames behind
    /// the one being recorded.
    pub frame: u64,
}

/// One slot: a host-readable buffer, and whatever is outstanding on it.
#[derive(Debug)]
struct Slot {
    /// The [`MemoryLocation::HostReadback`] buffer the copy pass writes into.
    ///
    /// One per slot rather than one buffer with per-slot offsets, because a
    /// readback is a mapping: WebGPU maps a buffer once, and `crcbl-wgpu`
    /// refuses a second request against a buffer that already has one in
    /// flight. A ring is exactly the shape that has several in flight.
    buffer: BufferHandle,
    /// What the **last frame that used this slot** left the buffer in, and what
    /// [`CullStatsRing::add_copy_pass`] imports it as.
    ///
    /// [`ResourceState::Undefined`] until a copy has been declared into it, and
    /// [`ResourceState::TransferDst`] thereafter — the shape
    /// `ForwardRenderer::shadow_imported` has, for the same reason. **This is
    /// the field the whole cross-frame hazard turns on**: the previous write to
    /// this buffer is another frame's `copy_buffer_to_buffer`, in another
    /// submission, and a barrier naming `Undefined` as its source carries no
    /// source scope at all — `srcStageMask = NONE` — so it would order the two
    /// writes against nothing. Naming `TransferDst` is what gives the graph's
    /// barrier a real prior access to depend on.
    imported: ResourceState,
    /// The frame whose copy the body stamped into this slot, once it ran.
    frame: Option<u64>,
    /// The request made for it, one frame after that copy was recorded.
    readback: Option<ReadbackHandle>,
}

/// A ring of host-readable buffers, one per frame in flight plus one.
#[derive(Debug)]
pub struct CullStatsRing {
    slots: Vec<Slot>,
    current: usize,
    /// The frame number the copy pass body stamped, or `0` for "nothing was
    /// recorded". Shared with the body, which runs inside
    /// [`execute`](crate::graph::CompiledGraph::execute) and therefore cannot
    /// hold `&mut` to this.
    recorded: Arc<AtomicU64>,
    /// Whether an amplification stage counts clusters in this frame at all —
    /// [`CullStats::clusters`]'s [`None`].
    counts_clusters: bool,
    frames: u64,
    latest: Option<CullStats>,
    /// Set by the first seam error. The ring reports nothing from then on rather
    /// than a number it cannot stand behind.
    off: bool,
}

impl CullStatsRing {
    /// Builds the ring, or `None` if the device will not give it its buffers.
    ///
    /// `counts_clusters` is whether the caller **built** an amplification stage
    /// — `ForwardRenderer::culls_clusters`, not a device capability — because
    /// that is what decides whether the cluster word is a count or the zero the
    /// clearing pass left.
    ///
    /// `None` is not an error: it is [`PassTimers::new`](crate::timing::PassTimers::new)'s
    /// answer to a device with no timestamps, for the same reason. The caller
    /// adds no copy pass and the panel says `indirect`.
    #[must_use]
    pub fn new(
        device: &dyn Device,
        frames_in_flight: usize,
        counts_clusters: bool,
    ) -> Option<Self> {
        // One more than the frames in flight, so the slot about to be reused is
        // always one whose submission has completed.
        let count = frames_in_flight.max(1) + 1;
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            match device.create_buffer(&BufferDesc {
                label: Some("cull stats readback"),
                size: SLOT_BYTES,
                // `TRANSFER_DST` and nothing else: this is the copy's
                // destination and it is never bound. The buffer a shader writes
                // stays device-local — see [`MemoryLocation`], which is where
                // that rule and the D3D12 device it cost are written down.
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            }) {
                Ok(buffer) => slots.push(Slot {
                    buffer,
                    // Nothing has written it, which is the one state a barrier
                    // may name as a source without ordering anything: there is
                    // nothing to order against yet.
                    imported: ResourceState::Undefined,
                    frame: None,
                    readback: None,
                }),
                Err(error) => {
                    crcbl_core::log::debug!(
                        "cull stats: readback buffer {index} refused ({error}); the culling \
                         counters stay on the GPU"
                    );
                    for slot in &slots {
                        device.destroy_buffer(slot.buffer);
                    }
                    return None;
                }
            }
        }
        Some(Self {
            slots,
            current: 0,
            recorded: Arc::new(AtomicU64::new(0)),
            counts_clusters,
            frames: 0,
            latest: None,
            off: false,
        })
    }

    /// The most recent frame whose statistics have actually landed, or [`None`]
    /// until the ring has come round once.
    #[must_use]
    pub const fn latest(&self) -> Option<CullStats> {
        self.latest
    }

    /// How many frames behind the recording frame a report is.
    ///
    /// The ring's length: a slot is copied into on one frame and read when the
    /// ring next reaches it. What [`CullStats::frame`] is measured against, and
    /// the number a reader needs to know how stale the panel's row is.
    #[must_use]
    pub const fn latency(&self) -> u64 {
        self.slots.len() as u64
    }

    /// Starts a frame: requests the readback the last frame's copy earned,
    /// rotates, and resolves the slot about to be reused.
    ///
    /// Called once per frame, before [`add_copy_pass`](Self::add_copy_pass), and
    /// **after the previous frame was submitted** — which is the frame loop's
    /// own order, since a frame is recorded, submitted and presented before the
    /// next one starts. See the module docs for why the request cannot happen at
    /// the copy.
    pub fn begin_frame(&mut self, device: &dyn Device) {
        if self.off {
            return;
        }
        self.frames += 1;

        // The copy this slot was given last frame has been submitted by now, so
        // a request against it covers work that is really on its way. A slot
        // nothing stamped is skipped: the graph that would have filled it never
        // ran, and what is in it belongs to an older turn of the ring.
        let recorded = self.recorded.swap(0, Ordering::Relaxed);
        if recorded != 0 {
            let slot = &mut self.slots[self.current];
            match device.request_readback(&ReadbackDesc {
                label: Some("cull stats"),
                buffer: slot.buffer,
                offset: 0,
                size: SLOT_BYTES,
                // Everything submitted so far, which is exactly the copy above
                // and nothing this ring has to name a timeline value for.
                after: None,
            }) {
                Ok(readback) => {
                    slot.frame = Some(recorded);
                    slot.readback = Some(readback);
                }
                Err(error) => {
                    self.give_up(device, &format!("request failed ({error})"));
                    return;
                }
            }
        }

        self.current = (self.current + 1) % self.slots.len();

        // The slot has come round: whatever was requested on it covers a frame
        // that has completed, so reading it neither stalls nor lies.
        self.resolve(device);
    }

    /// Adds the copy that takes this frame's statistics off the GPU.
    ///
    /// `stats` is the id
    /// [`GeneratedDraws::visible_count_id`](crate::draw_gen::GeneratedDraws::visible_count_id)
    /// handed back, so **the graph** moves the buffer into
    /// [`ResourceState::TransferSrc`] and back out again; there is not a
    /// hand-written barrier here, which is the whole difference between this and
    /// `crcbl::screenshot`'s copy.
    ///
    /// Declare it **last**, after every pass that writes that buffer: the cull
    /// dispatch, the amplification stage and the light grid all add to it, and a
    /// copy scheduled between them reads a total half the frame has not
    /// contributed to yet.
    ///
    /// # The destination is in the graph too, and it has to be
    ///
    /// A slot is written by a copy this frame and was written by a copy
    /// [`latency`](Self::latency) frames ago — **two writes, in two
    /// submissions, with nothing between them**. That is a write-after-write
    /// hazard exactly like the one the graph already computes for the depth
    /// transient across a frame boundary, and it is not made safe by the
    /// readback's completion point: a timeline gates the *host's read*, which is
    /// a different edge from the GPU's second write.
    ///
    /// So the slot is imported with what the previous turn of the ring left it
    /// in — `Undefined` the first time and `TransferDst` thereafter — and the
    /// copy declares
    /// [`ResourceState::TransferDst`] on it, which makes the graph emit
    /// `TransferDst → TransferDst` before the copy: a barrier whose source scope
    /// covers that earlier submission's transfer write. An earlier version of
    /// this function left the destination out on the grounds that "nothing else
    /// in the frame touches it", which is true and beside the point — the
    /// conflicting access is in *another* frame. CI's validation layer reported
    /// it as `SYNC-HAZARD-WRITE-AFTER-WRITE … write_barriers: 0`; ours cannot
    /// see across submissions at all.
    ///
    /// The state never changes after that, which is also what D3D12 requires: a
    /// resource on the `READBACK` heap is created in `COPY_DEST` and pinned
    /// there for its lifetime, so `TransferDst` is the only state this buffer
    /// may ever be declared in.
    pub fn add_copy_pass(&mut self, graph: &mut RenderGraph<'_>, stats: BufferId) {
        if self.off {
            return;
        }
        let frame = self.frames;
        let recorded = Arc::clone(&self.recorded);
        let slot = &mut self.slots[self.current];
        let handle = slot.buffer;
        let dst = graph.import_buffer(
            "cull-stats-slot",
            ImportedBuffer {
                buffer: handle,
                initial: slot.imported,
                final_state: ResourceState::TransferDst,
            },
        );
        // Set here rather than from inside the body, and deliberately: a graph
        // that is built and dropped leaves the buffer in whatever it was, and
        // claiming a write that never happened only makes the *next* barrier
        // wait for something already finished. Claiming `Undefined` after a
        // write that did happen would drop the source scope, which is the bug
        // this whole comment is about.
        slot.imported = ResourceState::TransferDst;
        graph
            .add_copy_pass("cull-stats-readback")
            .use_buffer(stats, ResourceState::TransferSrc)
            .use_buffer(dst, ResourceState::TransferDst)
            .execute(move |ctx| {
                let src = ctx.buffer(stats);
                let dst = ctx.buffer(dst);
                ctx.encoder().copy_buffer_to_buffer(&BufferCopy {
                    src,
                    src_offset: 0,
                    dst,
                    dst_offset: 0,
                    size: SLOT_BYTES,
                });
                // Recorded, so the next `begin_frame` may ask for it. Nothing
                // stamps this if the graph is dropped rather than executed.
                recorded.store(frame, Ordering::Relaxed);
            });
    }

    /// Reads the slot the ring has just reached, if anything is outstanding on
    /// it, and releases the request either way.
    ///
    /// One poll, never a loop: a slot that is still pending after a whole turn
    /// of the ring is a frame's statistics given up on, not a frame to wait for.
    fn resolve(&mut self, device: &dyn Device) {
        let slot = &self.slots[self.current];
        let (Some(readback), Some(frame)) = (slot.readback, slot.frame) else {
            return;
        };
        let mut bytes = [0u8; SLOT_BYTES as usize];
        let state = device.poll_readback(readback, &mut bytes);
        device.destroy_readback(readback);
        let slot = &mut self.slots[self.current];
        slot.readback = None;
        slot.frame = None;
        match state {
            Ok(ReadbackState::Ready) => {
                self.latest = Some(CullStats {
                    instances: u64::from(word(&bytes, cull::INSTANCE_SURVIVOR_WORD)),
                    clusters: self
                        .counts_clusters
                        .then(|| u64::from(word(&bytes, cull::CLUSTER_SURVIVOR_WORD))),
                    frame,
                });
            }
            // The buffer is about to be copied into again, so there is nothing
            // to come back to. Last frame's report stands and says which frame
            // it is from.
            Ok(ReadbackState::Pending) => {
                crcbl_core::log::debug!(
                    "cull stats: frame {frame} was still not back after a whole ring"
                );
            }
            Err(error) => self.give_up(device, &format!("poll failed ({error})")),
        }
    }

    /// Switches the ring off after a seam error, releasing what it can.
    ///
    /// Once rather than per frame: whatever refused a readback will refuse the
    /// next one, and a profiler that logs a line per frame forever is one nobody
    /// reads.
    fn give_up(&mut self, device: &dyn Device, why: &str) {
        self.off = true;
        self.latest = None;
        crcbl_core::log::warn!("cull stats: {why}; the culling counters are off (said once)");
        for slot in &mut self.slots {
            if let Some(readback) = slot.readback.take() {
                device.destroy_readback(readback);
            }
            slot.frame = None;
        }
    }

    /// Releases the ring. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for slot in &self.slots {
            if let Some(readback) = slot.readback {
                device.destroy_readback(readback);
            }
            device.destroy_buffer(slot.buffer);
        }
    }
}

/// One `u32` of the statistics block, little-endian as every backend writes it.
///
/// Indexed by word rather than by byte offset so the two counters are named by
/// `crcbl_shaders`' constants and not by arithmetic repeated at each call site.
fn word(bytes: &[u8; SLOT_BYTES as usize], index: u32) -> u32 {
    let start = index as usize * 4;
    u32::from_le_bytes([
        bytes[start],
        bytes[start + 1],
        bytes[start + 2],
        bytes[start + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Command, NullInstance, Recorder};
    use crcbl_hal::{
        AdapterId, CommandEncoderDesc, DeviceDesc, Instance, QueueHandle, QueueKind, SubmitInfo,
    };

    use crate::graph::ImportedBuffer;
    use crate::transient::TransientPool;

    /// How many frames a caller keeps in flight, and therefore one less than the
    /// ring's length. Two, as every frame loop in this engine runs.
    const FRAMES_IN_FLIGHT: usize = 2;

    /// A device, a statistics buffer shaped like [`DrawGen`]'s, and a frame that
    /// runs the ring end to end without a driver.
    ///
    /// [`DrawGen`]: crate::draw_gen::DrawGen
    struct Harness {
        recorder: Recorder,
        device: Box<dyn Device>,
        queue: QueueHandle,
        stats: BufferHandle,
        pool: TransientPool,
    }

    impl Harness {
        fn open() -> Self {
            let recorder = Recorder::new();
            let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
            let device = instance
                .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
                .expect("the gpu_driven preset opens");
            let queue = device.queue(QueueKind::Graphics).expect("always present");
            let stats = device
                .create_buffer(&BufferDesc {
                    label: Some("cull stats"),
                    size: SLOT_BYTES,
                    usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a device-local counter buffer");
            Self {
                recorder,
                device,
                queue,
                stats,
                pool: TransientPool::new(),
            }
        }

        /// One frame: begin, declare the copy, compile, execute, submit — the
        /// order every caller records in, and the order the request depends on.
        fn frame(&mut self, ring: &mut CullStatsRing) {
            let device = self.device.as_ref();
            ring.begin_frame(device);
            let mut graph = RenderGraph::new(self.queue);
            let stats = graph.import_buffer(
                "cull-count",
                ImportedBuffer {
                    buffer: self.stats,
                    initial: ResourceState::ShaderRead,
                    final_state: ResourceState::ShaderRead,
                },
            );
            ring.add_copy_pass(&mut graph, stats);
            let compiled = graph.compile(&self.pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("cull stats frame"),
                queue: self.queue,
            });
            compiled
                .execute(device, &mut self.pool, encoder.as_mut(), None)
                .expect("the graph executed");
            let commands = encoder.finish().expect("recording succeeded");
            device
                .submit(self.queue, &SubmitInfo::new(&[commands]))
                .expect("submitted");
            device.destroy_command_buffer(commands);
        }

        fn ring(&self, counts_clusters: bool) -> CullStatsRing {
            CullStatsRing::new(self.device.as_ref(), FRAMES_IN_FLIGHT, counts_clusters)
                .expect("the null backend gives out readback buffers")
        }

        fn finish(self, ring: CullStatsRing) {
            ring.destroy(self.device.as_ref());
            self.device.destroy_buffer(self.stats);
        }
    }

    /// **Each counter is read from its own word.** Two counters in one buffer is
    /// exactly the arrangement where an off-by-one offset reports the other
    /// one's total and looks entirely plausible, so the two are given different
    /// values and read back by name.
    #[test]
    fn a_counter_is_read_from_its_own_word() {
        let mut bytes = [0u8; SLOT_BYTES as usize];
        bytes[0..4].copy_from_slice(&7u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&9u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&11u32.to_le_bytes());
        assert_eq!(word(&bytes, cull::INSTANCE_SURVIVOR_WORD), 7);
        assert_eq!(word(&bytes, cull::CLUSTER_SURVIVOR_WORD), 9);
    }

    /// **The copy really is in the frame, and it is outside every pass scope.**
    ///
    /// The recorded stream is the evidence for both halves: the command is
    /// there, it names this ring's own buffer as the destination, and the null
    /// backend — which checks the seam's scope rules — recorded no violation. A
    /// copy declared as a compute pass records the same command and fails the
    /// second half.
    #[test]
    fn the_copy_is_recorded_and_sits_outside_every_pass() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);
        harness.frame(&mut ring);

        let copies: Vec<BufferCopy> = harness
            .recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::CopyBufferToBuffer(copy) => Some(copy),
                _ => None,
            })
            .collect();
        assert_eq!(copies.len(), 1, "one copy per frame, no more and no less");
        assert_eq!(copies[0].src, harness.stats);
        assert_eq!(copies[0].size, SLOT_BYTES);
        assert!(
            ring.slots.iter().any(|slot| slot.buffer == copies[0].dst),
            "the copy must land in this ring's own slot",
        );
        harness.recorder.assert_valid();

        harness.finish(ring);
    }

    /// **The barriers around the copy are the graph's, and they put the counter
    /// buffer back.**
    ///
    /// `ShaderRead → TransferSrc` before it, and the graph's final barrier back
    /// to `ShaderRead` — which is the state the next frame on that slot imports
    /// it in. A copy pass that declared nothing would record the same copy with
    /// no transition at all, and the buffer would be read as a shader resource
    /// while the transfer was still in flight.
    #[test]
    fn the_graph_moves_the_counter_buffer_through_transfer_src_and_back() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);
        harness.frame(&mut ring);

        let transitions: Vec<(ResourceState, ResourceState)> = harness
            .recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::Barrier { buffers, .. } => Some(buffers),
                _ => None,
            })
            .flatten()
            .filter(|barrier| barrier.buffer == harness.stats)
            .map(|barrier| (barrier.from, barrier.to))
            .collect();
        assert_eq!(
            transitions,
            [
                (ResourceState::ShaderRead, ResourceState::TransferSrc),
                (ResourceState::TransferSrc, ResourceState::ShaderRead),
            ],
        );

        harness.finish(ring);
    }

    /// **A frame's write to a slot is ordered against the write the last frame
    /// on that slot made** — the cross-submission hazard, caught with no driver
    /// in the room.
    ///
    /// This is the bug this ring shipped with and CI's validation layer caught:
    /// `SYNC-HAZARD-WRITE-AFTER-WRITE … write_barriers: 0`, one frame's
    /// `vkCmdCopyBuffer` into a slot against an earlier frame's copy into the
    /// same slot, in an earlier submission, with nothing between them. The
    /// readback's completion point does not help: a timeline gates the *host's*
    /// read, which is a different edge from the GPU's second write.
    ///
    /// It is asserted here, against the null backend, because the layer that
    /// found it is one this repository cannot run everywhere: local runs report
    /// `cross-submission=no` and see nothing of this class. That makes a
    /// device-free assertion the durable half — the same argument
    /// `crcbl-render`'s `a_second_frame_barriers_against_what_the_first_one_left`
    /// makes for the depth transient.
    ///
    /// Both ends are checked, and the first is what stops the second being
    /// vacuous: the **first** use of a slot names `Undefined` (there is nothing
    /// to order against), and the **reusing** frame names `TransferDst` (there
    /// is). A version that imported `Undefined` every time would emit a barrier
    /// that passes a "was there a barrier" test and orders nothing at all,
    /// because `Undefined` as a source expands to `srcStageMask = NONE`.
    #[test]
    fn a_reused_slot_barriers_against_the_frame_that_wrote_it_last() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);

        // The first turn of the ring: this slot has never been written.
        harness.frame(&mut ring);
        let slot = ring.slots[ring.current].buffer;
        assert_eq!(
            barrier_before_the_copy_into(&harness.recorder, slot),
            Some((ResourceState::Undefined, ResourceState::TransferDst)),
            "a slot nothing has written orders against nothing, and says so",
        );

        // Round the ring until this same slot comes up again.
        for _ in 1..ring.latency() {
            harness.recorder.clear();
            harness.frame(&mut ring);
            assert_ne!(
                ring.slots[ring.current].buffer, slot,
                "the ring must hand out a different slot until it has turned over",
            );
        }
        harness.recorder.clear();
        harness.frame(&mut ring);
        assert_eq!(
            ring.slots[ring.current].buffer, slot,
            "the ring has come round and this frame reuses the first slot",
        );
        assert_eq!(
            barrier_before_the_copy_into(&harness.recorder, slot),
            Some((ResourceState::TransferDst, ResourceState::TransferDst)),
            "the copy into a reused slot must be ordered against the copy that wrote it last, \
             and a source of `Undefined` would carry no scope to order against",
        );

        harness.finish(ring);
    }

    /// The transition a recorded frame put on `buffer` **before** the copy that
    /// writes it, or `None` if there was no such barrier.
    ///
    /// The ordering is the assertion, not a detail: a barrier recorded *after*
    /// the copy orders the frame after next and leaves this one hazardous, and
    /// it would satisfy a test that only asked whether a barrier existed.
    fn barrier_before_the_copy_into(
        recorder: &Recorder,
        buffer: BufferHandle,
    ) -> Option<(ResourceState, ResourceState)> {
        let mut transition = None;
        for command in recorder.commands() {
            match command {
                Command::Barrier { buffers, .. } => {
                    if let Some(found) = buffers.iter().find(|barrier| barrier.buffer == buffer) {
                        transition = Some((found.from, found.to));
                    }
                }
                Command::CopyBufferToBuffer(copy) if copy.dst == buffer => return transition,
                _ => {}
            }
        }
        None
    }

    /// **A slot is read only once the ring has come back round to it**, and the
    /// number it reports says which frame it is from.
    ///
    /// The latency is the ring's length, so nothing is reported for the first
    /// [`CullStatsRing::latency`] frames — and the frame that finally lands is
    /// the *oldest* one, not the one just recorded. A resolve that read the slot
    /// it had only just requested would report frame 2 on frame 2, which is the
    /// stale-bytes bug this ordering exists to prevent.
    #[test]
    fn a_slot_is_read_only_after_the_ring_has_come_round() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);
        assert_eq!(ring.latency(), FRAMES_IN_FLIGHT as u64 + 1);
        assert_eq!(ring.latest(), None, "nothing has been recorded yet");

        for frame in 1..=ring.latency() {
            harness.frame(&mut ring);
            assert_eq!(
                ring.latest(),
                None,
                "frame {frame} is inside the ring's latency and has nothing to report yet",
            );
        }

        harness.frame(&mut ring);
        let stats = ring.latest().expect("the first slot has come round");
        assert_eq!(
            stats.frame, 1,
            "the report is the oldest frame in the ring, not the newest",
        );

        harness.frame(&mut ring);
        assert_eq!(
            ring.latest().expect("and the ring keeps turning").frame,
            2,
            "each further frame advances the report by one",
        );

        harness.finish(ring);
    }

    /// **A slot that has not come back reports nothing rather than its bytes.**
    ///
    /// The null backend's readback latency is the only way to reach this: the
    /// slot is about to be copied into again, so what it holds belongs to an
    /// older turn of the ring. Reporting it would put a several-frames-stale
    /// number on the panel under this frame's label.
    #[test]
    fn a_readback_that_never_completes_reports_nothing() {
        let mut harness = Harness::open();
        // More polls than this test will ever make, so every slot stays pending.
        harness.recorder.set_readback_latency(1_000);
        let mut ring = harness.ring(false);

        for _ in 0..(ring.latency() * 3) {
            harness.frame(&mut ring);
            assert_eq!(ring.latest(), None, "a pending slot has nothing to report");
        }

        harness.finish(ring);
    }

    /// **A device that refuses the readback reports nothing, not zero.**
    ///
    /// A zero in the culling row reads as "the cull kept nothing" — a scene that
    /// vanished — which is a far worse answer than an honest absence. So the
    /// ring gives up for good, and what it reported before the failure goes with
    /// it rather than sitting on the panel as a number that stopped moving.
    #[test]
    fn a_device_that_refuses_the_readback_reports_nothing_rather_than_zero() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);
        for _ in 0..=ring.latency() {
            harness.frame(&mut ring);
        }
        assert!(ring.latest().is_some(), "the ring was working");

        // The device goes down between frames, so the next request is refused.
        // Only `begin_frame` is run from here: a lost device refuses to finish a
        // command buffer too, and this test is about the readback rather than
        // about the caller's frame.
        harness.recorder.lose_device("the test lost the device");
        ring.begin_frame(harness.device.as_ref());
        assert_eq!(ring.latest(), None, "a refused readback is not a zero");
        // And it stays off rather than trying again every frame.
        ring.begin_frame(harness.device.as_ref());
        assert_eq!(ring.latest(), None);

        harness.finish(ring);
    }

    /// **The cluster word is unknown where nothing counts clusters, not zero.**
    ///
    /// Both rings read the same bytes out of the same shaped buffer; the one
    /// built for a path with no amplification stage reports [`None`] and the one
    /// built for a path with one reports the number it read. A ring that always
    /// reported the word would say "no clusters survived" about three of the
    /// four ways this engine draws.
    #[test]
    fn the_cluster_word_is_unknown_where_no_stage_counts_it() {
        for (counts_clusters, expected) in [(false, None), (true, Some(0))] {
            let mut harness = Harness::open();
            let mut ring = harness.ring(counts_clusters);
            for _ in 0..=ring.latency() {
                harness.frame(&mut ring);
            }
            assert_eq!(
                ring.latest().expect("the ring came round").clusters,
                expected,
            );
            harness.finish(ring);
        }
    }

    /// **A frame whose graph never ran leaves nothing to read.**
    ///
    /// The copy is declared, the graph is dropped rather than executed, and the
    /// slot must stay empty: requesting a readback against it would return
    /// whatever the previous turn of the ring left there and report it under
    /// this frame's number. The `recorded` stamp is what tells the two apart,
    /// and this is the test that fails without it.
    #[test]
    fn a_graph_that_was_dropped_rather_than_executed_stamps_nothing() {
        let harness = Harness::open();
        let mut ring = harness.ring(false);

        for _ in 0..(ring.latency() * 2) {
            ring.begin_frame(harness.device.as_ref());
            let mut graph = RenderGraph::new(harness.queue);
            let stats = graph.import_buffer(
                "cull-count",
                ImportedBuffer {
                    buffer: harness.stats,
                    initial: ResourceState::ShaderRead,
                    final_state: ResourceState::ShaderRead,
                },
            );
            ring.add_copy_pass(&mut graph, stats);
            drop(graph);
            assert_eq!(ring.latest(), None, "no copy ran, so there is nothing back");
        }

        harness.finish(ring);
    }

    /// The ring releases every buffer and every outstanding request.
    #[test]
    fn the_ring_leaks_nothing() {
        let mut harness = Harness::open();
        let before = harness.recorder.total_live_objects();
        let mut ring = harness.ring(false);
        assert!(harness.recorder.total_live_objects() > before);
        harness.frame(&mut ring);
        harness.frame(&mut ring);
        ring.destroy(harness.device.as_ref());
        assert_eq!(
            harness.recorder.total_live_objects(),
            before,
            "every slot and every live readback must be released",
        );
        harness.device.destroy_buffer(harness.stats);
    }
}
