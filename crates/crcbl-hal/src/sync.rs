//! Submission and cross-queue synchronisation.
//!
//! # The sync model, and where the line is
//!
//! `docs/plan/01-foundations.md` says "explicit sync **at the graph level**".
//! That splits into two halves, and this crate owns only the lower one:
//!
//! | Concern | Owner |
//! | --- | --- |
//! | *Where* a barrier goes, and between which states | `crcbl-render`'s graph |
//! | *How* a barrier is expressed to the driver | this seam, backends |
//! | Resource state tracking across passes | `crcbl-render`'s graph |
//! | Transient aliasing, frames-in-flight, deletion queues | `crcbl-render` |
//! | Semaphore objects and submit-level waits/signals | this seam |
//!
//! **The HAL is stateless with respect to resource states.** It never infers a
//! barrier, never tracks a layout, and never reorders a command. If the graph
//! forgets a transition, the backend's validation layer says so — that is the
//! intended failure mode, and it is why `docs/plan/02-vulkan-backend.md` demands
//! zero validation warnings as a P1 exit criterion.
//!
//! # Timeline semaphores are the primitive
//!
//! One synchronisation object, not the Vulkan trio of fence, binary semaphore
//! and event. A [`SemaphoreKind::Timeline`] semaphore is a monotonically
//! increasing `u64` that both the GPU and the CPU can wait on. It replaces
//! fences for frame pacing, replaces separate upload-tracking objects for the
//! staging ring (topic 03 §3.1 waits on a timeline value before consuming a
//! mesh), and maps directly onto DX12's `ID3D12Fence` and Metal's
//! `MTLSharedEvent`. Vulkan 1.3 with `timelineSemaphore` is a hard requirement
//! for `crcbl-vk`, so there is no fallback path to design around.
//!
//! [`SemaphoreKind::Binary`] exists only because WSI acquire/present still uses
//! binary semaphores on Vulkan. Nothing above the seam creates one: the
//! swapchain owns them and hands them out through
//! [`AcquiredFrame`](crate::AcquiredFrame).
//!
//! P0's seam review asked whether "semaphore" is Vulkan-only vocabulary, since
//! DX12 has fences and Metal has `MTLSharedEvent`. The answer recorded here so
//! it is not re-litigated: **the name describes the thing.** `Timeline` maps
//! one-for-one onto both of those, and `Binary` names a real object that WSI
//! acquire genuinely needs — a one-shot GPU-only signal with no value to read.
//! Renaming it "fence" would import DX12's noun for a concept DX12 does not
//! have in this shape, which is the same mistake in the other direction.
//!
//! # WebGPU has no semaphores at all
//!
//! WebGPU expresses cross-submit ordering **implicitly**: one queue, command
//! buffers executed in submission order, and hazards between them tracked by the
//! browser. There is no object to signal and nothing to wait on, and the only
//! completion signal it offers — `GPUQueue.onSubmittedWorkDone()` — resolves for
//! *everything* submitted so far and carries no value, so it cannot answer "has
//! the timeline reached N".
//!
//! A backend on it therefore **refuses** the timeline half rather than
//! implementing it as a no-op: [`Device::create_semaphore`](crate::Device::create_semaphore)
//! answers [`HalError::Unsupported`](crate::HalError::Unsupported) for
//! [`SemaphoreKind::Timeline`], and so do
//! [`Device::semaphore_value`](crate::Device::semaphore_value),
//! [`Device::signal_semaphore`](crate::Device::signal_semaphore) and
//! [`Device::wait_semaphores`](crate::Device::wait_semaphores). The
//! [`SemaphoreKind::Binary`] half is still handed out, because WSI acquire is
//! where binary semaphores come from and every device must have one to give.
//!
//! **A no-op that returns `Ok` is the wrong shape here**, and `crcbl-webgpu`
//! shipped it: a caller got a handle, polled its value, and read `0` for ever
//! with nothing in any return code to say why. The engine is unaffected either
//! way — `crcbl::engine`'s `GpuContext` and `crcbl_render::MeshPool` both check
//! [`Features::TIMELINE_SEMAPHORE`](crate::Features::TIMELINE_SEMAPHORE) before
//! creating one and take their documented `wait_idle` fallback when it is
//! absent, which it always is on a browser — so refusing costs nothing and buys
//! a caller that does *not* check the flag an immediate answer instead of a
//! silent one.

use crcbl_core::Handle;

use crate::command::CommandBufferHandle;

/// Marker type for semaphore handles. Uninhabited.
#[derive(Debug)]
pub enum Semaphore {}

/// A semaphore.
pub type SemaphoreHandle = Handle<Semaphore>;

/// What kind of semaphore to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemaphoreKind {
    /// A monotonic `u64` counter, waitable by both GPU and CPU. The engine's
    /// default and the only kind anything above the seam should create.
    Timeline {
        /// Value the counter starts at.
        initial_value: u64,
    },
    /// A one-shot signal, waitable only by the GPU. Exists for WSI
    /// acquire/present; the swapchain creates these, not the renderer.
    Binary,
}

/// Creation parameters for a semaphore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemaphoreDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Timeline or binary.
    pub kind: SemaphoreKind,
}

/// A wait a submission performs before executing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemaphoreWait {
    /// Semaphore to wait on.
    pub semaphore: SemaphoreHandle,
    /// Value to wait for. Ignored for [`SemaphoreKind::Binary`].
    pub value: u64,
}

/// A signal a submission performs when it completes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemaphoreSignal {
    /// Semaphore to signal.
    pub semaphore: SemaphoreHandle,
    /// Value to signal. Must exceed the semaphore's current value. Ignored for
    /// [`SemaphoreKind::Binary`].
    pub value: u64,
}

/// One queue submission.
///
/// Waits happen before any command buffer runs; signals happen after all of
/// them complete. Finer-grained per-stage wait masks (Vulkan's
/// `pWaitDstStageMask`) are deliberately **not** exposed: they buy overlap only
/// when a submission's first work is unrelated to what it waited on, which the
/// graph already arranges by splitting submissions, and Metal cannot express
/// them at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubmitInfo<'a> {
    /// Command buffers, executed in order.
    pub command_buffers: &'a [CommandBufferHandle],
    /// Waited on before execution.
    pub waits: &'a [SemaphoreWait],
    /// Signalled on completion.
    pub signals: &'a [SemaphoreSignal],
}

impl<'a> SubmitInfo<'a> {
    /// A submission with no waits or signals.
    ///
    /// Correct only when the work has no dependencies and nothing waits on it —
    /// one-shot uploads at load time, and tests.
    #[must_use]
    pub const fn new(command_buffers: &'a [CommandBufferHandle]) -> Self {
        Self {
            command_buffers,
            waits: &[],
            signals: &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_submit_has_no_waits_or_signals() {
        let submit = SubmitInfo::new(&[]);
        assert!(submit.waits.is_empty());
        assert!(submit.signals.is_empty());
        assert!(submit.command_buffers.is_empty());
    }

    /// The split the two kinds exist for. A backend on WebGPU has no timeline
    /// semaphores and must say so — but it must still hand out a **binary**
    /// one, because WSI acquire/present needs one and the swapchain is where
    /// they come from. A backend that refused both would satisfy every test
    /// that only checks the timeline half.
    ///
    /// `initial_value` is deliberately not asserted here: this test is about
    /// which *kinds* a device hands out. What [`crate::null`] does with the
    /// value is `a_host_signal_moves_a_timeline_forwards_and_only_forwards` for
    /// the host half and `a_submissions_signal_advances_the_timeline_it_names`
    /// for the counter a submission drives — which that backend advances too,
    /// since work it never executes has always already finished.
    #[test]
    fn only_the_timeline_kind_needs_the_timeline_feature() {
        use crate::null::NullInstance;
        use crate::{AdapterId, DeviceDesc, Features, HalError, Instance};

        let instance = NullInstance::portable();
        let device = instance
            .create_device(&DeviceDesc {
                required_features: Features::COMPUTE,
                ..DeviceDesc::for_adapter(AdapterId(0))
            })
            .expect("the portable preset has compute");
        assert!(!device.caps().supports(Features::TIMELINE_SEMAPHORE));

        let error = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("frame timeline"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect_err("a device without the feature cannot make a timeline");
        assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");

        let binary = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("acquire"),
                kind: SemaphoreKind::Binary,
            })
            .expect("WSI acquire needs a binary semaphore on every device");
        device.destroy_semaphore(binary);

        let instance = NullInstance::gpu_driven();
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .expect("the gpu_driven preset opens");
        let timeline = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("frame timeline"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("the gpu_driven preset has timeline semaphores");
        device.destroy_semaphore(timeline);
    }
}
