//! The culling statistics, off the GPU and onto a ring of readback slots.
//!
//! ```text
//!  add_copy_pass ─▶ copy pass: cull stats ──▶ a free slot k        (frame f)
//!                                                  │
//!  begin_frame ─────── request_readback(slot k) ───┘               (frame f+1)
//!                                                  │
//!  begin_frame ─────── poll_readback(slot k) ──────┘               (frame f+2,
//!                              │                                    and every
//!                              │                                    frame until
//!                              │                                    it answers)
//!                              ▼  latest() ──▶ ForwardRenderer::counters()
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.6's culling-stats readback, and
//! `docs/plan/40-profiling.md`'s eighth missing piece: "the culling stats never
//! leave the GPU … there is no staging buffer, no copy inside the frame graph,
//! and no consumer". This is all three.
//!
//! # The answer *is* the synchronisation
//!
//! A slot is handed back for another copy **when its readback answers**, and not
//! before. That is the stronger of the two guarantees available: an answered
//! readback is proof that the copy which filled the slot completed, where a
//! count of frames is an assumption about how far ahead of the CPU the GPU is
//! allowed to get. There is no fence here, no `wait_idle` and no poll in a loop
//! — a slot that has not answered is left alone until the next frame, and a
//! frame that finds every slot still waiting simply records no copy.
//!
//! So the number is a few frames old — two at the very best, because the copy is
//! recorded on one frame, requested on the next and first polled on the one
//! after — and it says which frame it is about: [`CullStats::frame`] carries it,
//! and [`FrameCounters`](crate::counters::FrameCounters) puts it on the panel as
//! its own row. A latent counter beside live ones without saying so is the whole
//! reason that row exists.
//!
//! # A poll is a question, and on one backend it is asked across a wire
//!
//! Every outstanding slot is polled **once per frame**, which is exactly what
//! [`poll_readback`](Device::poll_readback) permits and what the browser needs.
//! `crcbl-webgpu` answers a poll by encoding the question onto the command
//! stream and returning [`ReadbackState::Pending`], because the answer cannot
//! arrive until that stream has been replayed and its replies drained: the
//! **first** poll on a handle there is never `Ready`, whatever the map has
//! already done. This module used to poll a slot once and release it on the next
//! line whatever it answered, which threw every browser answer away before it
//! could arrive — [`latest`](CullStatsRing::latest) stayed [`None`] for ever and
//! the panel showed no culling statistics at all. The native backends can answer
//! their first poll, which is why nothing else ever noticed.
//!
//! A deadline is the other half of that — `POLL_DEADLINE`, private to this
//! module. A device that has stopped answering must not end up holding every
//! slot in the ring mapped for the rest of the process, so a request that has
//! gone unanswered for that many frames is released and its slot reused. It is
//! not a wait: nothing blocks on it, and every other slot goes on being polled
//! meanwhile.
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

/// Frames a request is given to answer before its slot is taken back.
///
/// Long enough that no working device reaches it — a browser answers a poll
/// across a command stream, which costs a frame's round trip and not two orders
/// of magnitude of them — and short enough that a device which has *stopped*
/// answering releases what it holds. There is nothing else to release it: the
/// ring would otherwise run out of slots, stop recording copies, and hold every
/// buffer in it mapped until the renderer was destroyed.
///
/// The slot is safe to reuse when this expires for the reason the whole module
/// turns on: the copy that filled it was submitted this many frames ago, which
/// is far more than the frames the loop keeps in flight.
const POLL_DEADLINE: u64 = 120;

/// **What the per-cluster cull did, split by which test did it.**
///
/// Three numbers rather than one, because a survivor count alone cannot be read:
/// "27 of 338 survived" is equally consistent with the normal cone rejecting
/// every one of the other 311 and with it rejecting none of them, and
/// `docs/plan/sample/14-quarry.md` asks the panel to say which.
///
/// **Every cluster the amplification stage *tested* is in exactly one of the
/// three**, and a cluster it did not test is in none — see
/// [`tested`](Self::tested), which is the identity that makes them readable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterCull {
    /// Clusters both tests kept, which is what the mesh stage was dispatched
    /// for.
    pub survivors: u64,
    /// Clusters whose bounding sphere was entirely outside the frustum.
    pub frustum_rejects: u64,
    /// Clusters the frustum kept and the normal cone refused — every triangle
    /// facing away from the camera.
    ///
    /// **The two tests are ordered**, so this is "faced away *and* was on
    /// screen". A cluster that is both behind the camera and facing away is
    /// counted in [`frustum_rejects`](Self::frustum_rejects).
    pub cone_rejects: u64,
}

impl ClusterCull {
    /// Clusters the amplification stage put to either test — the size of the
    /// cut it was handed, in a frame whose instance survived the camera's cull.
    ///
    /// The three counters partition that set, so this is their sum and there is
    /// no fourth bucket. A cluster the DAG descent did not select, or a group
    /// past the dispatch extents, was never tested and is in none of them: it
    /// was not rejected by a cull, it was never offered to one.
    #[must_use]
    pub const fn tested(&self) -> u64 {
        self.survivors + self.frustum_rejects + self.cone_rejects
    }
}

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
    /// What the per-cluster cull did — survivors and the two rejection counts —
    /// or [`None`] where nothing counted them.
    ///
    /// [`None`] on the two indirect geometry paths and on a device with
    /// `Features::MESH_SHADER` and no `Features::TASK_SHADER`: there is no
    /// amplification stage in those frames, so the words keep the zeroes the
    /// clearing pass wrote. Reporting those zeroes as counts would say "every
    /// cluster was rejected" about a frame that drew all of them.
    ///
    /// **One [`Option`] over the three and not three [`Option`]s**, because the
    /// condition is one condition: the same absent stage is what leaves all
    /// three words untouched, and three separately-unknown numbers would invite
    /// a caller to add two of them up.
    pub clusters: Option<ClusterCull>,
    /// Which frame these came from — a few frames behind the one being
    /// recorded, and how many is the device's to decide rather than this
    /// module's. See the module docs.
    pub frame: u64,
}

/// What one slot is doing, which is what decides whether a copy may take it.
///
/// A slot is [`Idle`](Self::Idle) only when nothing is outstanding on it at all,
/// so the state is what keeps this frame's copy off a buffer the last frame's
/// copy is still on its way into or a readback is still mapping.
#[derive(Debug)]
enum SlotState {
    /// Nothing outstanding; the next copy may take it.
    Idle,
    /// Handed to this frame's graph by [`CullStatsRing::add_copy_pass`].
    ///
    /// The request is made at the *next* [`CullStatsRing::begin_frame`], by
    /// which point the copy has been submitted — see below for why it cannot be
    /// made here — and only if the graph actually ran.
    Recording,
    /// A request is out for the copy `frame` recorded, made on frame `since`.
    Polling {
        /// The frame whose copy the body stamped into this slot.
        frame: u64,
        /// The frame [`request_readback`](Device::request_readback) was called
        /// on, which is what [`POLL_DEADLINE`] is measured from and what keeps
        /// the first poll off the frame that made the request.
        since: u64,
        readback: ReadbackHandle,
    },
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
    /// [`ResourceState::TransferDst`] thereafter. **This is the field the whole
    /// cross-frame hazard turns on**: the previous write to this buffer is
    /// another frame's `copy_buffer_to_buffer`, in another submission, and a
    /// barrier naming `Undefined` as its source carries no source scope at all —
    /// `srcStageMask = NONE` — so it would order the two writes against nothing.
    /// Naming `TransferDst` is what gives the graph's barrier a real prior
    /// access to depend on.
    imported: ResourceState,
    /// Whether this slot is free, recording, or waiting on an answer.
    state: SlotState,
}

/// A ring of host-readable buffers, one per frame in flight plus one.
#[derive(Debug)]
pub struct CullStatsRing {
    slots: Vec<Slot>,
    /// Where [`CullStatsRing::take_slot`] starts looking, so consecutive frames
    /// land in different slots while free ones remain.
    ///
    /// Round robin rather than first-free because the buffer a frame copies into
    /// is the one the *last* frame on that slot copied into, and spreading the
    /// copies across the ring is what leaves the readback of the slot before it
    /// a whole frame to answer in before its buffer is wanted again.
    next: usize,
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
        // One more than the frames in flight. The depth is how many copies may
        // be outstanding before a frame has to go without one, and a slot is
        // busy from the frame its copy is recorded to the frame that answer
        // lands: three is what keeps a device answering its first poll copying
        // every frame, with a slot's grace for one that answers a frame later.
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
                    state: SlotState::Idle,
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
            next: 0,
            recorded: Arc::new(AtomicU64::new(0)),
            counts_clusters,
            frames: 0,
            latest: None,
            off: false,
        })
    }

    /// The most recent frame whose statistics have actually landed, or [`None`]
    /// until the first readback has answered.
    #[must_use]
    pub const fn latest(&self) -> Option<CullStats> {
        self.latest
    }

    /// Starts a frame: requests the readback the last frame's copy earned, then
    /// polls everything outstanding.
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
        self.request_recorded(device);
        self.poll_outstanding(device);
    }

    /// Requests the readback for the copy the last frame recorded, or hands that
    /// slot back if the graph holding it never ran.
    ///
    /// The copy the recording slot was given last frame has been submitted by
    /// now, so a request against it covers work that is really on its way. A slot
    /// nothing stamped is released unread: the graph that would have filled it
    /// never ran, and what is in it belongs to an older frame.
    fn request_recorded(&mut self, device: &dyn Device) {
        let recorded = self.recorded.swap(0, Ordering::Relaxed);
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| matches!(slot.state, SlotState::Recording))
        else {
            return;
        };
        if recorded == 0 {
            self.slots[index].state = SlotState::Idle;
            return;
        }
        let since = self.frames;
        let request = device.request_readback(&ReadbackDesc {
            label: Some("cull stats"),
            buffer: self.slots[index].buffer,
            offset: 0,
            size: SLOT_BYTES,
            // Everything submitted so far, which is exactly the copy above and
            // nothing this ring has to name a timeline value for.
            after: None,
        });
        match request {
            Ok(readback) => {
                self.slots[index].state = SlotState::Polling {
                    frame: recorded,
                    since,
                    readback,
                };
            }
            Err(error) => {
                self.slots[index].state = SlotState::Idle;
                self.give_up(device, &format!("request failed ({error})"));
            }
        }
    }

    /// Polls every outstanding slot once, reports what has landed, and releases
    /// what has run out of time.
    ///
    /// **Once per frame per readback, and never on the frame the request was
    /// made.** Once per frame is what the seam permits and what a backend whose
    /// polls are themselves asynchronous needs — see the module docs. Not on the
    /// requesting frame because that poll can only ever say `Pending`: it asks
    /// about a submission the frame loop has not even reached the end of.
    ///
    /// No allocation: the bytes land in one stack buffer reused across the
    /// slots, which [`poll_readback`](Device::poll_readback) leaves untouched
    /// unless it answers `Ready`.
    fn poll_outstanding(&mut self, device: &dyn Device) {
        let mut bytes = [0u8; SLOT_BYTES as usize];
        for index in 0..self.slots.len() {
            let SlotState::Polling {
                frame,
                since,
                readback,
            } = self.slots[index].state
            else {
                continue;
            };
            if since >= self.frames {
                continue;
            }
            match device.poll_readback(readback, &mut bytes) {
                Ok(ReadbackState::Ready) => {
                    device.destroy_readback(readback);
                    self.slots[index].state = SlotState::Idle;
                    self.report(frame, &bytes);
                }
                Ok(ReadbackState::Pending) if self.frames - since >= POLL_DEADLINE => {
                    device.destroy_readback(readback);
                    self.slots[index].state = SlotState::Idle;
                    crcbl_core::log::debug!(
                        "cull stats: frame {frame} was never answered; its slot is back in the ring"
                    );
                }
                Ok(ReadbackState::Pending) => {}
                Err(error) => {
                    self.give_up(device, &format!("poll failed ({error})"));
                    return;
                }
            }
        }
    }

    /// Files a landed slot's bytes as the report, unless an even later frame has
    /// already landed.
    ///
    /// **The panel's number never goes backwards.** Slots answer in the order
    /// they were requested on every device seen so far, but nothing in the seam
    /// promises it: two can land on the same frame, and a device is free to
    /// answer the newer one first. Reporting whichever spoke last would then walk
    /// [`CullStats::frame`] back down, which reads as the frame loop having
    /// stalled.
    fn report(&mut self, frame: u64, bytes: &[u8; SLOT_BYTES as usize]) {
        if self.latest.is_some_and(|latest| latest.frame >= frame) {
            return;
        }
        let stats = CullStats {
            instances: u64::from(word(bytes, cull::INSTANCE_SURVIVOR_WORD)),
            clusters: self.counts_clusters.then(|| ClusterCull {
                survivors: u64::from(word(bytes, cull::CLUSTER_SURVIVOR_WORD)),
                frustum_rejects: u64::from(word(bytes, cull::CLUSTER_FRUSTUM_REJECT_WORD)),
                cone_rejects: u64::from(word(bytes, cull::CLUSTER_CONE_REJECT_WORD)),
            }),
            frame,
        };
        // At `trace!` and nowhere near `debug!`: this is a line per frame, and
        // the module's own rule about the failure path — one line, not one a
        // frame — cuts the same way here. It earns its place because the panel
        // is the only other place these numbers exist and the panel is *glyphs*:
        // a browser cannot read them back at all, and the bug this ring shipped
        // with was precisely that they silently stopped arriving there.
        crcbl_core::log::trace!(
            "cull stats: frame {frame} kept {} instances, and its cluster cull did {:?}",
            stats.instances,
            stats.clusters,
        );
        self.latest = Some(stats);
    }

    /// The slot this frame's copy goes into, marked as this frame's.
    ///
    /// [`None`] when every slot is still waiting on an answer, which is the
    /// frame that records no copy at all.
    ///
    /// A slot found still [`Recording`](SlotState::Recording) is that slot: it
    /// means two copies were recorded with no [`begin_frame`](Self::begin_frame)
    /// between them, so nothing ever requested a readback for what is in it and
    /// the newer copy may simply overwrite it. Taking a fresh slot instead would
    /// leave that one recording for the life of the ring.
    fn take_slot(&mut self) -> Option<usize> {
        let len = self.slots.len();
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| matches!(slot.state, SlotState::Recording))
        {
            return Some(index);
        }
        let index = (0..len)
            .map(|step| (self.next + step) % len)
            .find(|&index| matches!(self.slots[index].state, SlotState::Idle))?;
        self.slots[index].state = SlotState::Recording;
        self.next = (index + 1) % len;
        Some(index)
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
    /// Nothing is added on a frame that finds every slot still waiting on an
    /// answer. The alternative is a copy into a buffer a readback is still
    /// mapping, and a frame's statistics are worth far less than that: the panel
    /// keeps the number it has, still stamped with the frame it came from, and
    /// the next frame after an answer lands records a copy again.
    ///
    /// # The destination is in the graph too, and it has to be
    ///
    /// A slot is written by a copy this frame and was written by a copy some
    /// frames ago — **two writes, in two submissions, with nothing between
    /// them**. That is a write-after-write hazard exactly like the one the graph
    /// already computes for the depth transient across a frame boundary, and it
    /// is not made safe by the readback's completion point: a timeline gates the
    /// *host's read*, which is a different edge from the GPU's second write.
    ///
    /// So the slot is imported with what the last frame on it left it in —
    /// `Undefined` the first time and `TransferDst` thereafter — and the copy
    /// declares [`ResourceState::TransferDst`] on it, which makes the graph emit
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
        let Some(index) = self.take_slot() else {
            return;
        };
        let frame = self.frames;
        let recorded = Arc::clone(&self.recorded);
        let slot = &mut self.slots[index];
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
            if let SlotState::Polling { readback, .. } = slot.state {
                device.destroy_readback(readback);
            }
            slot.state = SlotState::Idle;
        }
    }

    /// Releases the ring. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for slot in &self.slots {
            if let SlotState::Polling { readback, .. } = slot.state {
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

    /// Frames a test runs before deciding the ring has stopped reporting.
    ///
    /// Generous rather than exact: which frame an answer lands on is the
    /// device's to decide — that is what polling until it answers means — so a
    /// test that pinned the number would be asserting the null backend's
    /// readback latency and calling it the ring's behaviour.
    const BOUND: u64 = 16;

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

    /// **Each counter is read from its own word.** Several counters in one buffer
    /// is exactly the arrangement where an off-by-one offset reports another
    /// one's total and looks entirely plausible, so each is given a different
    /// value and read back by name.
    #[test]
    fn a_counter_is_read_from_its_own_word() {
        let bytes = stats_bytes();
        assert_eq!(word(&bytes, cull::INSTANCE_SURVIVOR_WORD), 7);
        assert_eq!(word(&bytes, cull::CLUSTER_SURVIVOR_WORD), 9);
        assert_eq!(word(&bytes, cull::CLUSTER_FRUSTUM_REJECT_WORD), 13);
        assert_eq!(word(&bytes, cull::CLUSTER_CONE_REJECT_WORD), 17);
    }

    /// The statistics buffer with a different number in every word, so a field
    /// filled from the wrong one is a different number rather than a coincidence.
    ///
    /// Word 2 is the light grid's dropped-assignment count — a decoy on purpose:
    /// it sits *between* the survivor word and the two rejection words, so a
    /// mapping that walked the buffer in order rather than indexing it by name
    /// would report it as a cull statistic.
    fn stats_bytes() -> [u8; SLOT_BYTES as usize] {
        let mut bytes = [0u8; SLOT_BYTES as usize];
        for (index, value) in [
            (cull::INSTANCE_SURVIVOR_WORD, 7u32),
            (cull::CLUSTER_SURVIVOR_WORD, 9),
            (crcbl_shaders::light::CLUSTER_OVERFLOW_WORD, 11),
            (cull::CLUSTER_FRUSTUM_REJECT_WORD, 13),
            (cull::CLUSTER_CONE_REJECT_WORD, 17),
        ] {
            let at = index as usize * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// **The three cluster counters land in the three fields that name them.**
    ///
    /// The word-to-field mapping is the whole of what makes the split real, and
    /// nothing downstream can catch it being wrong: a frustum count reported as
    /// survivors is a plausible number in a plausible field, and the three would
    /// still sum to the size of the cut. So the report is driven directly, with
    /// a distinct value in every word.
    #[test]
    fn each_cluster_counter_reaches_the_field_that_names_it() {
        let harness = Harness::open();
        let mut ring = harness.ring(true);
        ring.report(3, &stats_bytes());
        assert_eq!(
            ring.latest(),
            Some(CullStats {
                instances: 7,
                clusters: Some(ClusterCull {
                    survivors: 9,
                    frustum_rejects: 13,
                    cone_rejects: 17,
                }),
                frame: 3,
            }),
        );
        harness.finish(ring);
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
        let slot = recording_slot(&ring);
        assert_eq!(
            barrier_before_the_copy_into(&harness.recorder, slot),
            Some((ResourceState::Undefined, ResourceState::TransferDst)),
            "a slot nothing has written orders against nothing, and says so",
        );

        // Round the ring until this same slot comes up again.
        for _ in 1..ring.slots.len() {
            harness.recorder.clear();
            harness.frame(&mut ring);
            assert_ne!(
                recording_slot(&ring),
                slot,
                "the ring must hand out a different slot until it has turned over",
            );
        }
        harness.recorder.clear();
        harness.frame(&mut ring);
        assert_eq!(
            recording_slot(&ring),
            slot,
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

    /// The buffer the frame just run handed to its graph.
    ///
    /// Exactly one slot is [`SlotState::Recording`] between a frame's
    /// [`CullStatsRing::add_copy_pass`] and the next
    /// [`CullStatsRing::begin_frame`], which is where every caller of this looks.
    fn recording_slot(ring: &CullStatsRing) -> BufferHandle {
        let index = ring
            .slots
            .iter()
            .position(|slot| matches!(slot.state, SlotState::Recording))
            .expect("a frame that recorded its copy left exactly one slot recording");
        ring.slots[index].buffer
    }

    /// How many of this ring's copies a recorded frame holds.
    ///
    /// By size rather than by destination, so it counts a copy into any slot:
    /// which slot a frame took is the ring's own business and the question here
    /// is only whether a copy was recorded at all.
    fn stats_copies(recorder: &Recorder) -> usize {
        recorder
            .commands()
            .iter()
            .filter(|command| {
                matches!(command, Command::CopyBufferToBuffer(copy) if copy.size == SLOT_BYTES)
            })
            .count()
    }

    /// Runs frames until the ring reports something, and fails if `bound` frames
    /// go by without one.
    ///
    /// The bound is what makes this a test rather than a hang: a ring that has
    /// stopped reporting has to go red here rather than spin, which is exactly
    /// the failure the browser saw.
    fn run_until_reported(
        harness: &mut Harness,
        ring: &mut CullStatsRing,
        bound: u64,
    ) -> CullStats {
        for _ in 0..bound {
            harness.frame(ring);
            if let Some(stats) = ring.latest() {
                return stats;
            }
        }
        panic!("nothing was reported in {bound} frames");
    }

    /// **Nothing is reported until a readback has actually answered, and the
    /// number says which frame it is from.**
    ///
    /// Three points, in the frames the module documents: the copy is recorded on
    /// frame 1, the request for it is made on frame 2 — a readback covers work
    /// already *submitted*, and frame 1's submission is what it covers — and the
    /// first poll is on frame 3. So a report on frame 1 or 2 would be one
    /// invented before any readback was even asked for, and the frame it names
    /// must be the frame whose copy it is rather than the frame it landed on.
    ///
    /// The frame numbers are what has teeth here: a ring that read the slot it
    /// had just requested would report frame 2 on frame 2, and one that reported
    /// the current frame would say 3.
    #[test]
    fn nothing_is_reported_until_a_readback_has_answered() {
        let mut harness = Harness::open();
        let mut ring = harness.ring(false);
        assert_eq!(ring.latest(), None, "nothing has been recorded yet");

        harness.frame(&mut ring);
        assert_eq!(ring.latest(), None, "frame 1 has only recorded its copy");
        harness.frame(&mut ring);
        assert_eq!(
            ring.latest(),
            None,
            "frame 2 made the request; polling it here would ask about a frame still being \
             recorded",
        );

        harness.frame(&mut ring);
        let stats = ring.latest().expect("the first request has answered");
        assert_eq!(
            stats.frame, 1,
            "the report names the frame whose copy it is, not the frame it landed on",
        );

        harness.frame(&mut ring);
        assert_eq!(
            ring.latest().expect("and the ring keeps turning").frame,
            2,
            "each further frame advances the report by one",
        );

        harness.finish(ring);
    }

    /// **A device whose first poll answers `Pending` still reports, within a
    /// bounded number of frames.**
    ///
    /// This is the browser, and it is the bug this test exists for: `crcbl-webgpu`
    /// answers a `poll_readback` by *encoding* the question onto the command
    /// stream, so the first poll on a handle is `Pending` however long the map
    /// has been ready, and the answer arrives a frame later. A ring that polled a
    /// slot once and released it on the next line — which this one did — threw
    /// every answer away unread and reported nothing at all in a browser, for
    /// ever, while every native backend went on passing because they can answer
    /// their first poll.
    ///
    /// Red against that ring: [`Recorder::set_readback_latency`] makes N polls
    /// answer `Pending` first, which is the one thing no hardware in CI will
    /// reproduce. A test that only ran the null backend's instant readback — or a
    /// real GPU — cannot fail here, which is why neither caught it.
    ///
    /// The counts run past the ring's own depth deliberately: three slots and a
    /// readback that needs four polls means a frame where every slot is still
    /// waiting, and the ring has to record no copy that frame and carry on rather
    /// than wedge.
    #[test]
    fn a_first_poll_that_answers_pending_still_reports() {
        for pending_polls in 1..=4u32 {
            let mut harness = Harness::open();
            harness.recorder.set_readback_latency(pending_polls);
            let mut ring = harness.ring(false);

            // Two frames to record and request, one poll per frame after that,
            // and room for the frames the ring spends with no free slot.
            let bound = 2 * (2 + u64::from(pending_polls) + ring.slots.len() as u64);
            let first = run_until_reported(&mut harness, &mut ring, bound);
            assert_eq!(
                first.frame, 1,
                "the first frame's copy is the first thing to answer, {pending_polls} pending \
                 polls or not",
            );

            // And it keeps moving: a ring that reported once and stopped —
            // because it never freed the slot it read — would pass everything
            // above.
            let mut later = first;
            for _ in 0..bound {
                harness.frame(&mut ring);
                later = ring.latest().expect("a report never goes back to nothing");
                if later.frame > first.frame {
                    break;
                }
            }
            assert!(
                later.frame > first.frame,
                "the ring stopped reporting after its first answer: {later:?}",
            );

            harness.finish(ring);
        }
    }

    /// **A slot that has not come back reports nothing rather than its bytes.**
    ///
    /// The null backend's readback latency is the only way to reach this. What is
    /// in the buffer belongs to an older frame — nothing has said otherwise —
    /// and reporting it would put a stale number on the panel under a fresh
    /// frame's label.
    #[test]
    fn a_readback_that_never_completes_reports_nothing() {
        let mut harness = Harness::open();
        // More polls than this test will ever make, so every slot stays pending.
        harness.recorder.set_readback_latency(1_000);
        let mut ring = harness.ring(false);

        for _ in 0..(ring.slots.len() * 3) {
            harness.frame(&mut ring);
            assert_eq!(ring.latest(), None, "a pending slot has nothing to report");
        }

        harness.finish(ring);
    }

    /// **A device that stops answering does not grow the ring, and does not wedge
    /// it either.**
    ///
    /// Every slot ends up waiting on an answer that never comes, and from there
    /// two things must hold: the ring holds no more than the buffers and requests
    /// it already had — one leaked readback per frame is what an unbounded
    /// version looks like — and [`POLL_DEADLINE`] eventually hands the slots back
    /// so the frame that follows records a copy again.
    ///
    /// The recorded stream is the observable for the second half: no copy while
    /// every slot is out, and a copy again once the deadline has passed.
    #[test]
    fn a_device_that_stops_answering_neither_leaks_slots_nor_wedges_the_ring() {
        let mut harness = Harness::open();
        // More polls than any device will ever be asked for, so nothing answers.
        harness.recorder.set_readback_latency(u32::MAX);
        let mut ring = harness.ring(false);

        // Enough frames to put every slot out on a request.
        for _ in 0..=ring.slots.len() {
            harness.frame(&mut ring);
        }
        let live = harness.recorder.total_live_objects();

        // Well inside the deadline: nothing has been handed back, so no frame
        // may copy into a buffer a readback still holds — and the ring must not
        // be growing while it waits.
        for _ in 0..(POLL_DEADLINE / 2) {
            harness.recorder.clear();
            harness.frame(&mut ring);
            assert_eq!(
                stats_copies(&harness.recorder),
                0,
                "a frame with no free slot must record no copy",
            );
            assert!(
                harness.recorder.total_live_objects() <= live,
                "the ring must not accumulate requests a device never answers",
            );
        }

        // And past it: the slots come back and the ring records copies again.
        let mut copies = 0;
        for _ in 0..(2 * POLL_DEADLINE) {
            harness.recorder.clear();
            harness.frame(&mut ring);
            copies += stats_copies(&harness.recorder);
            assert!(
                harness.recorder.total_live_objects() <= live,
                "the ring must not accumulate requests a device never answers",
            );
        }
        assert!(
            copies > 0,
            "the deadline must hand the slots back, so the ring records copies again",
        );
        assert_eq!(
            ring.latest(),
            None,
            "and still reports nothing it never read"
        );

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
        run_until_reported(&mut harness, &mut ring, BOUND);
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
        for (counts_clusters, expected) in [(false, None), (true, Some(ClusterCull::default()))] {
            let mut harness = Harness::open();
            let mut ring = harness.ring(counts_clusters);
            let stats = run_until_reported(&mut harness, &mut ring, BOUND);
            assert_eq!(stats.clusters, expected);
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

        for _ in 0..BOUND {
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
