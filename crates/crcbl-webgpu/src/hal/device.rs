//! `impl PendingDevice`/`impl Device` — the device request's poll, and the open
//! device.
//!
//! Every method here is one of three kinds, and the kind is the honest thing to
//! know about it:
//!
//! * **Wired** — a [`StreamWriter`](crate::StreamWriter) command exists, so the
//!   call encodes it and returns the caller-allocated handle. Every `create_*`
//!   and `destroy_*` the stream carries, `submit`, the swapchain calls,
//!   `present`, `acquire_next_frame` and readback are here.
//! * **Legitimately refused** — WebGPU cannot do it and never will, so refusing
//!   is correct rather than a gap. `create_mesh_pipeline` (no mesh stage),
//!   `update_bind_group` (a `GPUBindGroup` exposes a label and nothing else, so
//!   there is no mutation to encode), and the timeline half of the semaphore
//!   calls: WebGPU orders submissions implicitly and has no counter anything
//!   could observe.
//! * **Loudly unsupported** — the stream has no command for it *yet*, so a
//!   `Result`-returning method returns [`HalError::Unsupported`] naming the gap
//!   rather than a silent success a caller would mistake for a working device.
//!   The query methods are here; a later slice wires them.
//!
//! Which of the first two a refusal is, is `crcbl_hal::DivergenceKind` —
//! `ApiAbsence` against `Unwritten` — and the parity record carries the same
//! answer for every capability this backend declines.
//!
//! The first two kinds return the **same variant**, and deliberately: a caller
//! does the same thing with either, and the sentence in `what` is what says
//! which. What none of them is any longer is a silent `Ok` — `create_semaphore`,
//! `semaphore_value` and `wait_semaphores` used to succeed while doing nothing,
//! which is the one shape a caller cannot detect at all.
//!
//! A wired method may still refuse a descriptor the stream cannot carry — see
//! the `super::bounds` module for which fields are measured, and why the refusal
//! is a [`HalError`] rather than the writer's assert.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, Capability,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePipelineDesc,
    ComputePipelineHandle, Device, DeviceCaps, DeviceRequestState, DisplayTiming, Features,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageViewDesc,
    ImageViewHandle, MeshPipelineDesc, PendingDevice, PipelineLayoutDesc, PipelineLayoutHandle,
    PresentInfo, QueryKind, QuerySetDesc, QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc,
    ReadbackHandle, ReadbackState, SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle,
    SemaphoreKind, SemaphoreWait, ShaderModuleDesc, ShaderModuleHandle, SubmitInfo, Support,
    SurfaceError, SwapchainDesc, SwapchainHandle,
};

use crate::device::DeviceProbe;
use crate::reply::Reply;

use super::channel::{HandlePool, SharedChannel};
use super::encoder::{NO_COUNT_BUFFER_DRAW, NO_MESH_STAGE, WebGpuCommandEncoder};

// ── WebGpuPendingDevice ────────────────────────────────────────────────────

/// A device request in flight: the [`DeviceProbe`] plus what building the
/// [`WebGpuDevice`] needs once it settles.
///
/// The polled half the seam was shaped for — `requestDevice` resolves on a later
/// turn of the browser's event loop and the main thread cannot block on it, so
/// [`poll`](PendingDevice::poll) drains the channel, absorbs into the probe, and
/// hands over the device the frame the answer lands.
#[derive(Debug)]
pub struct WebGpuPendingDevice {
    channel: SharedChannel,
    pool: HandlePool,
    probe: DeviceProbe,
    /// `true` once the device has been handed over, so a second poll is the
    /// caller bug it is rather than a second device.
    done: bool,
}

impl WebGpuPendingDevice {
    pub(crate) fn new(channel: SharedChannel, probe: DeviceProbe, pool: HandlePool) -> Self {
        Self {
            channel,
            pool,
            probe,
            done: false,
        }
    }

    /// The sequence the request is waiting on, or `None` once it has settled —
    /// what a test feeds the [`Reply::Device`] against.
    #[must_use]
    pub fn sequence(&self) -> Option<u64> {
        self.probe.sequence()
    }
}

impl PendingDevice for WebGpuPendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::WebGpu
    }

    fn poll(&mut self) -> Result<DeviceRequestState, HalError> {
        if self.done {
            return Err(HalError::InvalidDescriptor(
                "this WebGPU device request already produced its device".to_string(),
            ));
        }
        if let Some(Ok(replies)) = self.channel.with(crate::web::StreamChannel::drain_replies) {
            self.probe.absorb(&replies);
        }
        match &self.probe {
            DeviceProbe::Opened { caps } => {
                let device = WebGpuDevice::new(self.channel.clone(), *caps, self.pool.clone());
                self.done = true;
                Ok(DeviceRequestState::Ready(Box::new(device)))
            }
            DeviceProbe::Failed {
                reason,
                unsupported,
            } => {
                if unsupported.is_empty() {
                    Err(HalError::DeviceLost(format!(
                        "WebGPU device request failed: {reason}"
                    )))
                } else {
                    Err(HalError::UnsupportedFeatures {
                        missing: *unsupported,
                    })
                }
            }
            DeviceProbe::Unasked => Err(HalError::DeviceLost(
                "the WebGPU device request was never encoded".to_string(),
            )),
            DeviceProbe::Waiting { .. } => Ok(DeviceRequestState::Pending),
        }
    }
}

// ── readback tracking ──────────────────────────────────────────────────────

/// Where one readback has got to, polled across frames the way the browser
/// gate's readback probe is: the request is on the stream, then each poll asks
/// again until `mapAsync` resolves.
#[derive(Debug)]
enum ReadbackTracker {
    /// The request is on the stream; no poll is out.
    Requested,
    /// A poll is out and its answer has not arrived.
    Waiting(u64),
    /// The last poll answered pending; the next re-polls.
    Pending,
    /// The bytes are in.
    Ready(Vec<u8>),
    /// The readback failed and the bytes are never coming, with the reason the
    /// replayer gave.
    ///
    /// **Terminal, like [`Ready`](Self::Ready).** A failed readback is not
    /// re-polled: the map settled, and it settled the wrong way, so re-issuing
    /// would ask the replayer about a request it has already answered. Every
    /// poll from here on reports the same failure until
    /// [`destroy_readback`](Device::destroy_readback) drops it — which is what
    /// `poll_readback`'s "polling again after `Ready` yields the same bytes"
    /// looks like on this side of the answer.
    Failed(String),
}

impl ReadbackTracker {
    /// Take this readback's answer out of a drained frame's replies.
    fn absorb(&mut self, replies: &[(u64, Reply)]) {
        let Self::Waiting(sequence) = *self else {
            return;
        };
        let Some((_, reply)) = replies.iter().find(|(candidate, _)| *candidate == sequence) else {
            return;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready(data.clone()),
            Reply::ReadbackPending { .. } => Self::Pending,
            Reply::ReadbackFailed { reason, .. } => Self::Failed(reason.clone()),
            // A reply of another shape naming this poll is a replayer bug; drop
            // to `Pending` so the next poll re-issues rather than reporting a
            // readiness that never comes.
            _ => Self::Pending,
        };
    }
}

// ── error tracking ─────────────────────────────────────────────────────────

/// The device's out-of-band errors on this side of the seam: what has arrived
/// and not been handed out, and the ask that is in flight.
///
/// **One ask at a time**, which is what [`waiting`](Self::waiting) is for. A
/// [`Reply::DeviceErrors`] carries the replayer's whole queue, so a second
/// [`Command::TakeError`](crate::Command::TakeError) issued while the first is
/// unanswered would ask for errors already on their way and answer the caller
/// twice with the same message.
#[derive(Debug, Default)]
struct ErrorQueue {
    /// Arrived and not yet handed to a caller, oldest first — the order the
    /// browser reported them in, which is the order that says which failure
    /// caused the rest.
    ///
    /// Bounded by [`tag::MAX_DEVICE_ERRORS`](crate::tag::MAX_DEVICE_ERRORS)
    /// without a check of its own: a reply carries at most that many, and
    /// another is only asked for once this is empty.
    arrived: VecDeque<String>,
    /// Sequence of the ask whose answer has not arrived, or `None` when none is
    /// out.
    waiting: Option<u64>,
}

impl ErrorQueue {
    /// Take this device's errors out of a drained frame's replies.
    fn absorb(&mut self, replies: &[(u64, Reply)]) {
        let Some(sequence) = self.waiting else {
            return;
        };
        let Some((_, reply)) = replies.iter().find(|(candidate, _)| *candidate == sequence) else {
            return;
        };
        // Answered, whatever it says: one command is answered exactly once, so
        // leaving this set would wait for a second reply that the channel would
        // refuse. A reply of another shape naming this ask is a replayer bug and
        // carries no errors; the next `take_error` asks again.
        self.waiting = None;
        if let Reply::DeviceErrors { messages } = reply {
            self.arrived.extend(messages.iter().cloned());
        }
    }
}

/// Why a [`QueryKind::Timestamp`] set is refused on a device that has no
/// `'timestamp-query'`, shared by [`WebGpuDevice::create_query_set`]'s refusal
/// and the device's declaration so the two cannot drift.
///
/// **The refusal is load-bearing rather than descriptive.** `crcbl-render`'s
/// `PassTimers::new` gates on the device reporting
/// [`Features::TIMESTAMP_QUERY`] and then creates a set; a set handed out
/// without the feature would put `timestampWrites` into every frame of every
/// demo, and the browser would refuse each pass. Gating both on the same flag is
/// what makes "the timers are off" and "the set cannot be created" one answer.
pub(super) const NO_TIMESTAMP_FEATURE: &str = "a GPUQuerySet of type 'timestamp' needs the browser's 'timestamp-query' feature, and this \
     device opened without it";

/// WebGPU's answer for [`Capability::PipelineStatisticsQuery`], shared for
/// [`NO_TIMESTAMP_FEATURE`]'s reason.
pub(super) const NO_STATISTICS_SET: &str = "GPUQueryType is exactly 'occlusion' and 'timestamp', so there is no pipeline-statistics query \
     set for WebGPU to create";

/// One [`Device::query_results`] exchange for one query set.
///
/// [`ErrorQueue`]'s shape, and for its reason: nothing here can block on a
/// browser, so the call pumps, hands back whatever has arrived, and encodes an
/// awaited ask when none is outstanding.
#[derive(Debug, Default)]
struct QueryReadQueue {
    /// The answer that arrived and has not been handed out, as the range it
    /// covers and the values in it.
    arrived: Option<(u32, Vec<u64>)>,
    /// Sequence of the ask whose answer has not arrived, or `None` when none is
    /// out.
    waiting: Option<u64>,
}

impl QueryReadQueue {
    /// Take this set's answer out of a drained frame's replies.
    fn absorb(&mut self, set: QuerySetHandle, replies: &[(u64, Reply)]) {
        let Some(sequence) = self.waiting else {
            return;
        };
        let Some((_, reply)) = replies.iter().find(|(candidate, _)| *candidate == sequence) else {
            return;
        };
        // Answered, whatever it says — [`ErrorQueue::absorb`]'s rule: one command
        // is answered exactly once, so leaving this set would wait for a second
        // reply the channel would refuse.
        self.waiting = None;
        if let Reply::QueryResults {
            set: answered,
            first_query,
            values,
        } = reply
            && *answered == set
        {
            self.arrived = Some((*first_query, values.clone()));
        }
    }
}

// ── WebGpuDevice ───────────────────────────────────────────────────────────

/// An open device: encodes resource, pass and submission commands onto the
/// shared stream, and reads the answers that come back.
#[derive(Debug)]
pub struct WebGpuDevice {
    channel: SharedChannel,
    caps: DeviceCaps,
    graphics_queue: QueueHandle,
    pool: HandlePool,
    /// Readbacks in flight, keyed by handle bits. Guarded so the device stays
    /// `Send + Sync` on native, where the seam demands it.
    readbacks: Mutex<HashMap<u64, ReadbackTracker>>,
    /// How many queries each live query set holds, keyed by handle bits.
    ///
    /// The seam documents [`HalError::InvalidDescriptor`] for a
    /// [`query_results`](Device::query_results) range that exceeds the set, and
    /// that is decidable here rather than a frame away: the browser is not
    /// needed to know how big a set this device asked for. The kind is not
    /// stored beside the count because nothing here asks it: both kinds this
    /// backend creates read back as one `u64` per query.
    query_sets: Mutex<HashMap<u64, u32>>,
    /// One [`QueryReadQueue`] per set a read has been asked of, keyed by handle
    /// bits.
    query_reads: Mutex<HashMap<u64, QueryReadQueue>>,
    /// The out-of-band errors the browser has reported, and the ask in flight
    /// for more. Guarded for [`readbacks`](Self::readbacks)'s reason.
    errors: Mutex<ErrorQueue>,
    /// The extent each live swapchain was configured at, keyed by handle bits.
    /// WebGPU's acquire is synchronous and answers no size, so the frame's
    /// extent is the one this device last configured.
    swapchains: Mutex<HashMap<u64, (u32, u32)>>,
}

impl WebGpuDevice {
    /// Assemble an open device. Called by [`WebGpuPendingDevice::poll`] with the
    /// caps the browser reported for *that device*, and by tests.
    pub(crate) fn new(channel: SharedChannel, caps: DeviceCaps, pool: HandlePool) -> Self {
        let graphics_queue: QueueHandle = pool.alloc();
        Self {
            channel,
            caps,
            graphics_queue,
            pool,
            readbacks: Mutex::new(HashMap::new()),
            query_sets: Mutex::new(HashMap::new()),
            query_reads: Mutex::new(HashMap::new()),
            errors: Mutex::new(ErrorQueue::default()),
            swapchains: Mutex::new(HashMap::new()),
        }
    }

    /// A clone of the channel this device encodes through, for inspecting the
    /// stream and feeding replies in a test.
    #[must_use]
    pub fn channel(&self) -> SharedChannel {
        self.channel.clone()
    }

    /// Drain the reply buffer once and dispatch it to every readback in flight.
    ///
    /// **Only one place may drain per frame**, or a reply one tracker needed is
    /// taken by another and its command waits for ever — so every poll-shaped
    /// method comes through here, and a second call in the same frame drains an
    /// empty buffer and changes nothing. A decode error or a borrowed inbox is
    /// left for the next frame; nothing here is the place to report one.
    ///
    /// **Every waiter is offered every reply**, for the same reason: the buffer
    /// is drained once, so a reply this frame carried for the error queue is
    /// gone by the time [`take_error`](Device::take_error) next runs unless it is
    /// dispatched here. Each waiter picks out the sequence it is waiting on and
    /// ignores the rest.
    fn pump(&self) {
        let Some(Ok(replies)) = self.channel.with(crate::web::StreamChannel::drain_replies) else {
            return;
        };
        if replies.is_empty() {
            return;
        }
        let mut readbacks = self
            .readbacks
            .lock()
            .expect("the readback map was poisoned");
        for tracker in readbacks.values_mut() {
            tracker.absorb(&replies);
        }
        drop(readbacks);
        let mut reads = self
            .query_reads
            .lock()
            .expect("the query-read map was poisoned");
        for (bits, queue) in reads.iter_mut() {
            let Some(set) = QuerySetHandle::from_bits(*bits) else {
                continue;
            };
            queue.absorb(set, &replies);
        }
        drop(reads);
        self.errors
            .lock()
            .expect("the device error queue was poisoned")
            .absorb(&replies);
    }
}

impl Device for WebGpuDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::WebGpu
    }

    fn caps(&self) -> DeviceCaps {
        self.caps
    }

    /// What this backend does with each seam behaviour.
    ///
    /// **The answer is about behaviour, not about return codes**, and on this
    /// backend the two come apart more than anywhere else: a command crosses the
    /// stream and the browser executes it a turn later, so a method can return
    /// `Ok` here and be refused there.
    /// [`MeshShading`](Capability::MeshShading) is the shape — the HAL call
    /// [`draw_mesh_tasks`](crcbl_hal::CommandEncoder::draw_mesh_tasks) returns
    /// `()`, and no command in the stream carries it, so what a caller sees is
    /// [`finish`](crcbl_hal::CommandEncoder::finish) refusing the whole command
    /// buffer. This method reports the behaviour.
    ///
    /// The semaphores used to be the clearest case and are no longer one:
    /// [`create_semaphore`](Self::create_semaphore) handed out a handle whose
    /// [`semaphore_value`](Self::semaphore_value) answered `0` for ever, so a
    /// caller watching a counter advance never saw one whatever the `Result`
    /// said. Those three now refuse, so the declarations below and the return
    /// codes agree — a declaration is what a caller *reads*, and it was never
    /// going to stop one that did not.
    ///
    /// A consequence worth stating: several refusals below are *declarations*
    /// this crate cannot demonstrate, because the seam suite is a native binary
    /// and this backend runs in a browser. The browser gate is what holds them
    /// to it.
    ///
    /// Exhaustive with no wildcard arm, and `deny`-ed as such.
    #[deny(clippy::wildcard_enum_match_arm)]
    fn supports(&self, capability: Capability) -> Support {
        let has = self.caps.features;
        let gated = |feature: Features, why: &'static str| -> Support {
            Support::granted(has, feature, why)
        };
        const NO_VALUE_FILL: &str = "WebGPU's only fill is GPUCommandEncoder.clearBuffer, which writes zero; the stream \
             carries the value so the replayer can refuse a non-zero one rather than write the \
             wrong bytes";
        const NO_MESH: &str = NO_MESH_STAGE;
        // One sentence for the three, because they are one obstacle: there is no
        // semaphore object, so there is nothing to signal, read or wait on.
        const NO_TIMELINE: &str = "WebGPU has no semaphores. It orders submissions implicitly and its only completion \
             signal, GPUQueue.onSubmittedWorkDone(), resolves for everything submitted so far and \
             carries no value — so nothing here could advance a counter, and create_semaphore, \
             semaphore_value and wait_semaphores all refuse rather than succeed while doing \
             nothing";

        match capability {
            Capability::BufferFillZero => Support::Yes,
            Capability::BufferFillRepeatedByte | Capability::BufferFillWord => {
                Support::No(NO_VALUE_FILL)
            }
            Capability::ImageToImageCopy => Support::Yes,
            // The replayer's `DEPTH_STENCIL_COPY` table carries the WebGPU
            // specification's depth-stencil rows, so a copy naming
            // `ImageAspect::DEPTH` reaches `copyTextureToBuffer` with the
            // plane's own bytes-per-texel and the `'depth-only'` aspect on the
            // texture side. What that table also carries is WebGPU's own
            // narrowing, which this `Yes` does not claim away: the depth plane
            // of `D32Float`, `D32FloatS8Uint` and `D16Unorm` copies OUT to a
            // buffer, only `D16Unorm`'s copies back IN, and
            // `D24UnormS8Uint`'s copies neither way because `depth24plus` has
            // no defined memory layout at all. Each combination WebGPU forbids
            // is refused by name on the device error queue rather than
            // recorded, and `Capability::DepthImageCopy` says which of them
            // this backend answers for.
            Capability::DepthImageCopy => Support::Yes,
            Capability::MsaaResolveAttachment => Support::Yes,
            // `GPURenderPassEncoder.setStencilReference(reference)` is core
            // WebGPU with an initial value of 0, and the stream carries a
            // `SetStencilReference` tag for it. The browser gate's stencil group
            // is what holds this `Yes` to a value rather than to a survived call:
            // it clears a stencil plane to a known value, compares `Equal`
            // against a pipeline reference that never matches, and reads back
            // which of two draws survived.
            Capability::StencilReference => Support::Yes,
            Capability::DrawIndirectCount => Support::No(NO_COUNT_BUFFER_DRAW),
            Capability::IndirectArgumentPaddedStride => Support::No(
                "WebGPU's drawIndirect reads one tightly packed argument structure and has no \
                 stride parameter to honour",
            ),
            Capability::MeshShading | Capability::TaskShaderStage => Support::No(NO_MESH),
            // The sentence `crcbl_hal::DIVERGENCES` carries for this pair, so
            // the declaration and the parity record cannot drift apart — they
            // had, and the constant is what settles it.
            Capability::UpdateBindGroup => Support::No(crcbl_hal::WEBGPU_BIND_GROUPS_ARE_IMMUTABLE),
            Capability::PushConstants => Support::No(
                "WebGPU has no push constants; the substitute is a dynamic-offset uniform buffer, \
                 which the seam already carries as bind-group dynamic offsets",
            ),
            Capability::BindlessDescriptorArray => {
                Support::No("WebGPU has no binding arrays at all")
            }
            // `GPUStorageTextureBindingLayout` requires a texel format and a
            // view dimension at layout creation, and this used to be a `No`
            // because `BindingKind::StorageImage` carried neither. The gap was
            // the seam's own descriptor rather than WebGPU's: the variant now
            // names both, `crate::StreamWriter` puts them on the wire after
            // `read_only`, and `web/engine/gpu-replay.js` builds the
            // `storageTexture` member out of them.
            //
            // **And this claims exactly what the capability defines**, which is
            // "a `BindingKind::StorageImage` entry in a bind group layout" — the
            // layout, not a shader that writes through it. The narrower claim is
            // the honest one on this backend for the same reason it is on the
            // others: nothing the seam records decides what a WGSL module
            // declares.
            //
            // A format WebGPU does not allow as a storage texture — every sRGB,
            // depth and block-compressed one — is refused by name at layout
            // creation on the far side rather than becoming a browser validation
            // error against a handle that already exists, which is
            // `webgpuTextureFormatFor`'s rule applied to a second member.
            Capability::StorageImageBinding => Support::Yes,
            Capability::PolygonModeLine => {
                Support::No("WebGPU has no core expression for a wireframe fill mode")
            }
            Capability::DepthClamp => gated(
                Features::DEPTH_CLAMP,
                "this device reports no DEPTH_CLAMP; WebGPU's depth-clip-control unclips rather \
                 than clamps",
            ),
            Capability::SamplerAnisotropy => gated(
                Features::SAMPLER_ANISOTROPY,
                "this device reports no SAMPLER_ANISOTROPY",
            ),
            // **A device question, not a constant.** `'timestamp-query'` is a
            // real `GPUFeatureName` and a browser may not have it, so this is
            // the flag the device actually opened with — the same gate
            // `create_query_set` applies, so a `Yes` here and a set that would
            // be refused cannot both happen. What the `Yes` claims is the whole
            // of the capability: a `GPUQuerySet` of type `'timestamp'`, the two
            // queries a pass names through
            // `GPURenderPassDescriptor.timestampWrites`, and `query_results`
            // reading them back. Probe group AF is what holds it to a value —
            // the native seam suite is a native binary and cannot open this
            // backend.
            Capability::TimestampQuery => gated(
                Features::TIMESTAMP_QUERY,
                "this device reports no TIMESTAMP_QUERY, so the browser has no 'timestamp-query'                  feature and no GPUQuerySet of that type could be created",
            ),
            // **And this claims exactly what the capability defines and no
            // more.** `Capability::OcclusionQuery` is "a QueryKind::Occlusion
            // query set" — `crcbl_hal::CommandEncoder` has no begin/end query
            // verb, so nothing a caller records through this seam can ever write
            // one, and the same is true of the Vulkan backend's `Yes`. What is
            // claimed here is that `create_query_set` builds a real
            // `GPUQuerySet` of the size asked for, that the seam's three query
            // verbs reach the browser against it, and that `query_results` reads
            // it back. `'occlusion'` needs no `GPUFeatureName`, so this is a
            // constant rather than a device question: every device this backend
            // opens serves it. Probe group AE is what holds the claim to a
            // value — the native seam suite is a native binary and cannot open
            // this backend.
            Capability::OcclusionQuery => Support::Yes,
            Capability::PipelineStatisticsQuery => Support::No(NO_STATISTICS_SET),
            // WebGPU has no semaphore of any kind. It orders submissions
            // implicitly — one queue, executed in order, hazards tracked by the
            // browser — and its only completion signal is
            // `GPUQueue.onSubmittedWorkDone()`, which resolves for everything
            // submitted so far and carries no value. So there is no counter to
            // advance, none to read, nothing for a CPU wait to block on and
            // nothing for a CPU signal to move; `create_semaphore` refuses the
            // timeline kind and the other three follow from that.
            Capability::TimelineSemaphore
            | Capability::CpuTimelineWait
            | Capability::CpuTimelineSignal
            | Capability::TimelineWaitBeforeSignal => Support::No(NO_TIMELINE),
            // The one that stays. `crcbl_hal::sync` requires every device to
            // hand out a binary semaphore because WSI acquire is where they come
            // from — and `acquire_next_frame` here answers `None` for both, so
            // nothing observes it. A handle is the honest answer: it is created
            // and destroyed, and no claim about a value is made about it.
            Capability::BinarySemaphore => Support::Yes,
        }
    }

    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        // WebGPU has one implicit queue, so only `Graphics` — which always
        // exists on the seam — is answered; async compute and transfer are
        // features this device does not report.
        match kind {
            QueueKind::Graphics => Some(self.graphics_queue),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    fn take_error(&self) -> Option<String> {
        // The browser's out-of-band errors, one per call, exactly as the seam
        // asks: `uncapturederror` fires on the JS side long after the command
        // that caused it returned, the replayer queues what it hears, and a
        // `TakeError` on the stream is what brings the queue across.
        //
        // **A frame late, and that is the seam's shape rather than a shortfall.**
        // Nothing here can block on a browser, so the answer to this frame's ask
        // lands in the next one — which is why `Gpu::acquire` calls this at the
        // top of a frame and refuses to record another when it answers `Some`.
        // The first frame after a failure therefore still records; the second
        // stops. That is what the stub could not do at all.
        self.pump();
        let mut errors = self
            .errors
            .lock()
            .expect("the device error queue was poisoned");
        if let Some(message) = errors.arrived.pop_front() {
            return Some(message);
        }
        // Nothing left, so ask for more — but only with nothing already out, and
        // only *after* the queue is empty: a reply carries the replayer's whole
        // queue, so asking again while holding messages would re-ask for what is
        // already here. A channel that cannot take the command (its buffers are
        // borrowed, or `MAX_WAITING_REPLIES` are already waiting) leaves
        // `waiting` as it was, so the next call tries again rather than waiting
        // for an answer to a command that was never encoded.
        if errors.waiting.is_none() {
            errors.waiting = self
                .channel
                .with(|channel| channel.encode_awaited(crate::StreamWriter::take_error));
        }
        None
    }

    // --- resources ---

    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        let handle: BufferHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_buffer(handle, desc)));
        Ok(handle)
    }

    fn destroy_buffer(&self, buffer: BufferHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_buffer(buffer)));
    }

    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        // Wired: the upload becomes `queue.writeBuffer(buffer, offset, data)` on
        // the replayer. The bytes cross as however many `WriteBuffer` commands
        // at increasing offsets the stream's field cap needs, so an upload of
        // any size goes over — see `StreamWriter::write_buffer`.
        //
        // The one thing refused here is an upload whose end address does not fit
        // a `u64`. That is the chunk arithmetic's precondition, and a caller bug
        // rather than a limit of the stream, so it comes back as an error the
        // caller can report instead of the writer's assert — which on wasm would
        // take the whole module down.
        if offset.checked_add(data.len() as u64).is_none() {
            return Err(HalError::InvalidDescriptor(format!(
                "a {} byte upload at offset {offset} runs past the end of the u64 address space",
                data.len()
            )));
        }
        self.channel
            .with(|channel| channel.encode(|stream| stream.write_buffer(buffer, offset, data)));
        Ok(())
    }

    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        let handle: ReadbackHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.request_readback(handle, desc)));
        self.readbacks
            .lock()
            .expect("the readback map was poisoned")
            .insert(handle.to_bits(), ReadbackTracker::Requested);
        Ok(handle)
    }

    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        // Drain first, so a reply committed since the last poll has updated this
        // tracker before we read it.
        self.pump();
        let mut readbacks = self
            .readbacks
            .lock()
            .expect("the readback map was poisoned");
        let tracker = readbacks
            .get_mut(&readback.to_bits())
            .ok_or_else(|| HalError::invalid_handle("readback", readback))?;
        match tracker {
            ReadbackTracker::Ready(bytes) => {
                if bytes.len() != out.len() {
                    return Err(HalError::InvalidDescriptor(format!(
                        "readback is {} bytes but `out` is {}",
                        bytes.len(),
                        out.len()
                    )));
                }
                out.copy_from_slice(bytes);
                Ok(ReadbackState::Ready)
            }
            // The map settled the wrong way. `ReadbackState` has no third
            // variant — `crcbl_hal::readback` says why: a `Failed` state would
            // drop the reason, which for a device lost or a map rejected is the
            // only useful part — so this is the `Err` arm `poll_readback`'s own
            // docs name, and it is what keeps a caller from polling for ever.
            ReadbackTracker::Failed(reason) => Err(HalError::DeviceLost(format!(
                "WebGPU readback failed: {reason}"
            ))),
            // A poll is already out; do not issue a second for the same sequence.
            ReadbackTracker::Waiting(_) => Ok(ReadbackState::Pending),
            // Nothing outstanding: ask again, and remember the sequence so the
            // next pump can settle it.
            ReadbackTracker::Requested | ReadbackTracker::Pending => {
                if let Some(sequence) = self
                    .channel
                    .with(|channel| channel.encode_awaited(|stream| stream.poll_readback(readback)))
                {
                    *tracker = ReadbackTracker::Waiting(sequence);
                }
                Ok(ReadbackState::Pending)
            }
        }
    }

    fn destroy_readback(&self, readback: ReadbackHandle) {
        // Quiet even when the map is still out, which is the case
        // `crcbl-render`'s culling-statistics ring is in every frame: the
        // replayer's `unmap` cancels a `mapAsync` in flight, WebGPU
        // acknowledges that by rejecting the promise with an `AbortError`, and
        // the replayer marks the request abandoned first so the rejection is
        // not filed as a device error. Before it did, releasing an outstanding
        // readback surfaced through `take_error` and stopped the frame loop.
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_readback(readback)));
        self.readbacks
            .lock()
            .expect("the readback map was poisoned")
            .remove(&readback.to_bits());
    }

    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let handle: ImageHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_image(handle, desc)));
        Ok(handle)
    }

    fn destroy_image(&self, image: ImageHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_image(image)));
    }

    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let handle: ImageViewHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_image_view(handle, desc)));
        Ok(handle)
    }

    fn destroy_image_view(&self, view: ImageViewHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_image_view(view)));
    }

    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        let handle: SamplerHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_sampler(handle, desc)));
        Ok(handle)
    }

    fn destroy_sampler(&self, sampler: SamplerHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_sampler(sampler)));
    }

    // --- shaders and pipelines ---

    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // The one descriptor here carrying compiled output rather than program
        // text, so its artifacts are measured against the stream's caps before
        // anything is encoded — see `super::bounds`.
        super::bounds::shader_module(desc)?;
        let handle: ShaderModuleHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_shader_module(handle, desc)));
        Ok(handle)
    }

    fn destroy_shader_module(&self, module: ShaderModuleHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_shader_module(module)));
    }

    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        let handle: BindGroupLayoutHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_bind_group_layout(handle, desc)));
        Ok(handle)
    }

    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_bind_group_layout(layout)));
    }

    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        let handle: BindGroupHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_bind_group(handle, desc)));
        Ok(handle)
    }

    fn update_bind_group(
        &self,
        _group: BindGroupHandle,
        _entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        // Needed for the bindless streaming path, and not coming: a
        // `GPUBindGroup` exposes a label and nothing else, so there is no
        // mutation for a stream command to carry. A caller rebuilds the group.
        // Refuse loudly rather than drop the update.
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "WebGPU bind groups are immutable once created, so update_bind_group has \
                   nothing to encode; rebuild the group instead",
        })
    }

    fn destroy_bind_group(&self, group: BindGroupHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_bind_group(group)));
    }

    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        let handle: PipelineLayoutHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_pipeline_layout(handle, desc)));
        Ok(handle)
    }

    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_pipeline_layout(layout)));
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let handle: GraphicsPipelineHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_graphics_pipeline(handle, desc)));
        Ok(handle)
    }

    fn create_mesh_pipeline(
        &self,
        _desc: &MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        // Legitimately refused, not a gap: WebGPU has no mesh or task shader
        // stage, so no device this backend opens can ever report
        // `Features::MESH_SHADER`. Refusing here is the seam's contract — an
        // absent capability is a named, loud failure at pipeline creation, not a
        // frame away at the draw.
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "WebGPU has no mesh-shader stage",
        })
    }

    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_graphics_pipeline(pipeline)));
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        let handle: ComputePipelineHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_compute_pipeline(handle, desc)));
        Ok(handle)
    }

    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_compute_pipeline(pipeline)));
    }

    // --- queries ---

    /// Creates an **occlusion** or **timestamp** query set, and refuses the
    /// statistics kind by name.
    ///
    /// `GPUQueryType` is exactly `'occlusion'` and `'timestamp'`. The first
    /// needs no `GPUFeatureName`, so every device this backend can open serves
    /// it; the second needs `'timestamp-query'`, which is why it is gated on the
    /// flag this device actually opened with. Both refusals carry the same
    /// sentences [`supports`](Device::supports) declares —
    /// `NO_TIMESTAMP_FEATURE` and `NO_STATISTICS_SET` — so a refusal and a
    /// declaration cannot drift.
    ///
    /// **The gate is what keeps a frame recordable.** `crcbl-render`'s
    /// `PassTimers` builds itself on [`Features::TIMESTAMP_QUERY`] alone, so a
    /// set handed out on a device without it would put `timestampWrites` into
    /// every frame that the browser would then refuse pass by pass.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] for [`QueryKind::PipelineStatistics`] and for
    /// [`QueryKind::Timestamp`] without the feature, and
    /// [`HalError::InvalidDescriptor`] for a set of no queries — WebGPU's
    /// `GPUQuerySetDescriptor.count` has a minimum of one, and a zero-length set
    /// accepted here would be a handle whose every read is out of range.
    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        match desc.kind {
            QueryKind::Timestamp => {
                if !self.caps.features.contains(Features::TIMESTAMP_QUERY) {
                    return Err(HalError::Unsupported {
                        backend: BackendKind::WebGpu,
                        what: NO_TIMESTAMP_FEATURE,
                    });
                }
            }
            QueryKind::PipelineStatistics => {
                return Err(HalError::Unsupported {
                    backend: BackendKind::WebGpu,
                    what: NO_STATISTICS_SET,
                });
            }
            QueryKind::Occlusion => {}
        }
        if desc.count == 0 {
            return Err(HalError::InvalidDescriptor(
                "a query set of 0 queries: GPUQuerySetDescriptor.count has a minimum of 1, and \
                 every read of such a set would be out of range"
                    .to_string(),
            ));
        }
        let handle: QuerySetHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_query_set(handle, desc)));
        self.query_sets
            .lock()
            .expect("the query-set map was poisoned")
            .insert(handle.to_bits(), desc.count);
        Ok(handle)
    }

    fn destroy_query_set(&self, set: QuerySetHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_query_set(set)));
        self.query_sets
            .lock()
            .expect("the query-set map was poisoned")
            .remove(&set.to_bits());
        // The read queue goes with it. A reply still in flight for this set then
        // finds no waiter, which is the same as any answer arriving for a
        // sequence nobody kept — `pump` offers it to every waiter and no waiter
        // takes it.
        self.query_reads
            .lock()
            .expect("the query-read map was poisoned")
            .remove(&set.to_bits());
    }

    /// Reads query values back, **a frame late**.
    ///
    /// [`take_error`](Device::take_error)'s shape, and for its reason: nothing
    /// here can block on a browser, so this pumps, hands back an answer that has
    /// arrived, and otherwise encodes an awaited ask and reports that the answer
    /// is not here yet. A caller that reads every frame — which is what a
    /// profiler does — gets values from the frame before, and one that reads
    /// once gets an `Err` and has to ask again.
    ///
    /// **The seam's own bounds check is answered here rather than on the wire.**
    /// How many queries this device asked for is known locally, so a read past
    /// the end is [`HalError::InvalidDescriptor`] at the call, which is what the
    /// seam documents and what tells a caller its range is wrong rather than its
    /// timing.
    ///
    /// **An empty `out` never reaches the browser.** Reading nothing is
    /// satisfied by writing nothing, and answering it over the wire would spend
    /// a round trip to learn that — and would make the empty `values` list a
    /// failed read is answered with ambiguous. See
    /// [`Command::QueryResults`](crate::Command::QueryResults).
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a set this device did not create or has
    /// destroyed, [`HalError::InvalidDescriptor`] for a range that exceeds the
    /// set, and [`HalError::Backend`] while the browser has not answered — which
    /// includes a read the replayer could not serve, whose reason reaches the
    /// caller through [`take_error`](Device::take_error).
    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError> {
        let count = *self
            .query_sets
            .lock()
            .expect("the query-set map was poisoned")
            .get(&set.to_bits())
            .ok_or_else(|| HalError::invalid_handle("query set", set))?;
        let Ok(wanted) = u32::try_from(out.len()) else {
            return Err(HalError::InvalidDescriptor(format!(
                "a read of {} queries, which is more than a u32 query index holds",
                out.len()
            )));
        };
        if u64::from(first_query) + u64::from(wanted) > u64::from(count) {
            return Err(HalError::InvalidDescriptor(format!(
                "queries {first_query}..{} of a {count}-query set",
                u64::from(first_query) + u64::from(wanted)
            )));
        }
        if out.is_empty() {
            return Ok(());
        }

        self.pump();
        let mut reads = self
            .query_reads
            .lock()
            .expect("the query-read map was poisoned");
        let queue = reads.entry(set.to_bits()).or_default();
        // An answer for a different range is one this caller no longer wants —
        // it asked for something else since. Dropped rather than kept, so the
        // ask below is issued instead of waiting behind a stale reply.
        if let Some((answered, values)) = queue.arrived.take()
            && answered == first_query
            && values.len() == out.len()
        {
            out.copy_from_slice(&values);
            return Ok(());
        }
        if queue.waiting.is_none() {
            queue.waiting = self.channel.with(|channel| {
                channel.encode_awaited(|stream| stream.query_results(set, first_query, wanted))
            });
        }
        Err(HalError::Backend(format!(
            "the browser has not answered queries {first_query}..{} of query set {:#x} yet; the \
             ask is on the stream and the values arrive in a later frame",
            u64::from(first_query) + u64::from(wanted),
            set.to_bits()
        )))
    }

    // --- synchronisation ---

    /// Hands out a **binary** semaphore and refuses a **timeline** one.
    ///
    /// The two kinds get different answers because the seam asks different
    /// things of them, and only one of the two is a question WebGPU can answer:
    ///
    /// * A [`SemaphoreKind::Timeline`] is a counter a caller *observes*. WebGPU
    ///   expresses cross-submit ordering implicitly — one queue, submissions
    ///   executed in order, hazards tracked by the browser — and its only
    ///   completion signal is `GPUQueue.onSubmittedWorkDone()`, which resolves
    ///   for everything submitted so far and carries no value to compare a wait
    ///   against. So there is nothing here to advance a counter, which is what
    ///   [`Capability::TimelineSemaphore`] names. Refusing is also what
    ///   [`Device::create_semaphore`] already documents for a device without
    ///   [`Features::TIMELINE_SEMAPHORE`], and this backend never reports that
    ///   flag: no `GPUFeatureName` satisfies it, so `requestDevice` refuses the
    ///   engine's default descriptor by name rather than granting it — see
    ///   [`crate::device`].
    /// * A [`SemaphoreKind::Binary`] is the *swapchain's*, not a caller's, and
    ///   [`crcbl_hal::sync`] requires every device to hand one out because WSI
    ///   acquire is where they come from. [`acquire_next_frame`] here answers
    ///   `None` for both of its semaphores, so nothing ever observes this one;
    ///   the handle costs a pool slot and keeps a caller's book-keeping uniform.
    ///
    /// **Why this is not `Ok` for both.** It was, and the handle it returned was
    /// a counter [`semaphore_value`] answered `0` for ever: a caller polling for
    /// progress saw success and no movement, with nothing in any return code to
    /// say why. The replayer already refuses a submit-level wait or signal by
    /// name rather than dropping it — see [`Command::Submit`] — and this is that
    /// same judgement one layer up, where a caller finds out before it submits
    /// instead of a frame later on the error queue.
    ///
    /// [`acquire_next_frame`]: Self::acquire_next_frame
    /// [`semaphore_value`]: Self::semaphore_value
    /// [`Command::Submit`]: crate::Command::Submit
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        match desc.kind {
            SemaphoreKind::Binary => Ok(self.pool.alloc()),
            SemaphoreKind::Timeline { .. } => Err(HalError::Unsupported {
                backend: BackendKind::WebGpu,
                what: "timeline semaphores: WebGPU orders submissions implicitly and its only \
                       completion signal, onSubmittedWorkDone, carries no value to observe",
            }),
        }
    }

    fn destroy_semaphore(&self, _semaphore: SemaphoreHandle) {
        // No-op: the binary semaphore `create_semaphore` hands out is a pool
        // slot and nothing else, and no timeline was ever created.
    }

    /// Refuses: every semaphore this backend hands out is binary.
    ///
    /// [`Device::semaphore_value`] documents [`HalError::Unsupported`] for a
    /// binary semaphore, which has no value to read — and `create_semaphore`
    /// here refuses the only kind that would have one.
    fn semaphore_value(&self, _semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "semaphore_value: every semaphore on this backend is binary, and WebGPU has no \
                   counter behind one",
        })
    }

    /// Refuses: there is no timeline on this backend to advance.
    ///
    /// A host signal needs a [`SemaphoreKind::Timeline`], `create_semaphore`
    /// hands out none, and the binary kind the seam does require is a pool slot
    /// with no counter behind it. `Ok(())` would be the shape this backend's
    /// semaphores were built in the first time — success, and nothing anywhere
    /// moved.
    fn signal_semaphore(&self, _semaphore: SemaphoreHandle, _value: u64) -> Result<(), HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "signal_semaphore: WebGPU has no timeline to advance; submissions are ordered \
                   and hazard-tracked by the browser",
        })
    }

    /// Refuses: there is no timeline on this backend to block on.
    ///
    /// A CPU wait needs a [`SemaphoreKind::Timeline`], `create_semaphore` hands
    /// out none, and the binary kind the seam does require is GPU-waitable only.
    /// `Ok(true)` stood here and meant "already satisfied", which was true of
    /// nothing — there was no counter to compare a value against.
    fn wait_semaphores(
        &self,
        _waits: &[SemaphoreWait],
        _timeout_ns: u64,
    ) -> Result<bool, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "wait_semaphores: WebGPU has no timeline to block on; submissions are ordered \
                   and hazard-tracked by the browser",
        })
    }

    fn wait_idle(&self) -> Result<(), HalError> {
        // No-op: the browser drives its own queue, and this seam has no fence to
        // block on. Shutdown does not need one.
        Ok(())
    }

    // --- commands ---

    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder> {
        Box::new(WebGpuCommandEncoder::new(
            self.channel.clone(),
            self.pool.clone(),
            desc,
        ))
    }

    fn destroy_command_buffer(&self, buffer: CommandBufferHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_command_buffer(buffer)));
    }

    fn submit(&self, _queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        self.channel
            .with(|channel| channel.encode(|stream| stream.submit(submit)));
        Ok(())
    }

    // --- presentation ---

    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        let handle: SwapchainHandle = self.pool.alloc();
        self.channel
            .with(|channel| channel.encode(|stream| stream.create_swapchain(handle, desc)));
        self.swapchains
            .lock()
            .expect("the swapchain map was poisoned")
            .insert(handle.to_bits(), desc.extent);
        Ok(handle)
    }

    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        self.channel
            .with(|channel| channel.encode(|stream| stream.reconfigure_swapchain(swapchain, desc)));
        self.swapchains
            .lock()
            .expect("the swapchain map was poisoned")
            .insert(swapchain.to_bits(), desc.extent);
        Ok(())
    }

    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_swapchain(swapchain)));
        self.swapchains
            .lock()
            .expect("the swapchain map was poisoned")
            .remove(&swapchain.to_bits());
    }

    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        // Synchronous and deterministic on WebGPU — `getCurrentTexture` answers
        // in the call — so wasm allocates the image and view ids and no reply is
        // waited on. Both semaphores are `None`: acquire and present are
        // implicit, so there is nothing for a caller to wait on or signal.
        let image: ImageHandle = self.pool.alloc();
        let view: ImageViewHandle = self.pool.alloc();
        self.channel.with(|channel| {
            channel.encode(|stream| stream.acquire_next_frame(swapchain, image, view))
        });
        let extent = self
            .swapchains
            .lock()
            .expect("the swapchain map was poisoned")
            .get(&swapchain.to_bits())
            .copied()
            .unwrap_or((0, 0));
        Ok(AcquiredFrame {
            image,
            view,
            extent,
            index: 0,
            acquire_semaphore: None,
            present_semaphore: None,
            suboptimal: false,
        })
    }

    fn present(&self, _queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        self.channel
            .with(|channel| channel.encode(|stream| stream.present(present)));
        Ok(())
    }

    fn wait_until_presented(
        &self,
        _swapchain: SwapchainHandle,
        _present_id: u64,
        _timeout: std::time::Duration,
    ) -> Result<(), SurfaceError> {
        // No-op: this device does not advertise `Features::PRESENT_FEEDBACK`, so
        // there is nothing here to wait for — "nothing to wait for", not "this
        // call was wrong", exactly as the seam prescribes.
        Ok(())
    }

    fn display_timing(&self, _swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError> {
        // This device does not advertise `Features::PRESENT_TIMING`, so it has
        // nothing to report about the panel. `Unknown` is an arm every caller
        // already handles.
        Ok(DisplayTiming::Unknown)
    }
}
