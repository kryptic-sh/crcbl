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
//!   is correct rather than a gap. `create_mesh_pipeline` (no mesh stage), and
//!   the semaphore calls, which are no-ops because WebGPU auto-synchronises.
//! * **Loudly unsupported** — the stream has no command for it *yet*, so a
//!   `Result`-returning method returns [`HalError::Unsupported`] naming the gap
//!   rather than a silent success a caller would mistake for a working device.
//!   `update_bind_group` and the query methods are here; a later slice wires
//!   them.
//!
//! A wired method may still refuse a descriptor the stream cannot carry — see
//! the `super::bounds` module for which fields are measured, and why the refusal
//! is a [`HalError`] rather than the writer's assert.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crcbl_hal::{
    AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, Device,
    DeviceCaps, DeviceRequestState, DisplayTiming, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, ImageDesc, ImageHandle, ImageViewDesc, ImageViewHandle, MeshPipelineDesc,
    PendingDevice, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, QuerySetDesc,
    QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle, ReadbackState,
    SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, SemaphoreWait, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};

use crate::device::DeviceProbe;
use crate::reply::Reply;

use super::channel::{HandlePool, SharedChannel};
use super::encoder::WebGpuCommandEncoder;

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
        // Needed for the bindless streaming path, but the stream has no
        // `update_bind_group` command yet — a later slice adds one. Refuse
        // loudly rather than drop the update.
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "update_bind_group is not yet wired into the WebGPU stream",
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

    fn create_query_set(&self, _desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        // Needed, but the stream has no query commands yet — a later slice adds
        // them. Refuse loudly: a caller handed a query-set handle nothing backs
        // would write timestamps into a set that does not exist.
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "query sets are not yet wired into the WebGPU stream",
        })
    }

    fn destroy_query_set(&self, _set: QuerySetHandle) {
        // Nothing to encode: the stream has no query commands, so
        // `create_query_set` refuses and no live handle ever reaches this. A
        // documented no-op, not a silent drop of real work.
    }

    fn query_results(
        &self,
        _set: QuerySetHandle,
        _first_query: u32,
        _out: &mut [u64],
    ) -> Result<(), HalError> {
        // Needed, but unwired for `create_query_set`'s reason. Refuse loudly
        // rather than return zeros a profiler would read as real timings.
        Err(HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "query results are not yet wired into the WebGPU stream",
        })
    }

    // --- synchronisation ---

    fn create_semaphore(&self, _desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        // WebGPU auto-synchronises submissions, so it has no semaphores. A dummy
        // handle keeps a caller's book-keeping consistent — it is created,
        // waited on and destroyed like any other, and every one of those is a
        // no-op — rather than refusing a primitive the engine's own headless
        // descriptor asks for. Correct, and documented as such.
        Ok(self.pool.alloc())
    }

    fn destroy_semaphore(&self, _semaphore: SemaphoreHandle) {
        // No-op: WebGPU has no semaphore to destroy.
    }

    fn semaphore_value(&self, _semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        // No-op: there is no timeline behind a WebGPU semaphore, so nothing has
        // advanced. Zero is the value a freshly created timeline reports.
        Ok(0)
    }

    fn wait_semaphores(
        &self,
        _waits: &[SemaphoreWait],
        _timeout_ns: u64,
    ) -> Result<bool, HalError> {
        // No-op: WebGPU auto-synchronises, so every wait is already satisfied.
        Ok(true)
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
