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
//! WebGPU serialises queue submission and inserts hazard barriers itself. A
//! Tier B backend therefore implements every type in this module as a no-op:
//! waits are satisfied trivially, signals are recorded, and
//! [`Device::wait_semaphores`](crate::Device::wait_semaphores) resolves against
//! `onSubmittedWorkDone`. The renderer's submit code is identical on both tiers
//! because the *shape* is the same; only the cost differs.

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

    #[test]
    fn timeline_semaphores_carry_an_initial_value() {
        let desc = SemaphoreDesc {
            label: Some("frame timeline"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        };
        assert_eq!(
            desc.kind,
            SemaphoreKind::Timeline { initial_value: 0 },
            "frames-in-flight pacing starts the timeline at zero and signals \
             frame_index + 1"
        );
        assert_ne!(desc.kind, SemaphoreKind::Binary);
    }
}
