//! GPU→CPU readback, shaped so a browser can implement it without lying.
//!
//! # Why this is not `read_buffer(&self, …) -> Result<(), HalError>`
//!
//! A synchronous read is unimplementable on the one backend
//! `docs/plan/10-wasm-webgpu.md` makes a first-class target. WebGPU's readback
//! is `GPUBuffer.mapAsync`, which resolves on a **later turn of the event
//! loop**; on the browser main thread there is no way to block waiting for it,
//! and the main thread is where the rAF loop runs. Native `wgpu` can fake
//! synchrony with `device.poll(Wait)`, but the browser cannot, so a blocking
//! signature would be a method that exists in the trait and returns
//! [`Unsupported`](crate::HalError::Unsupported) on the target the engine most
//! wants to ship to. That is the same class of mistake as a Vulkan-shaped
//! `acquire_next_image(semaphore)` — see [`crate::swapchain`], which solves its
//! version of this problem the same way: make the *portable* shape the only
//! shape.
//!
//! # The shape
//!
//! Three calls, matching the engine's existing poll discipline
//! (`docs/plan/12-testing.md`: "poll for the condition with deadline, never
//! fixed sleeps"):
//!
//! ```text
//! request_readback(&ReadbackDesc)  ──▶ ReadbackHandle
//! poll_readback(handle, &mut out)  ──▶ Pending | Ready     (call once per frame)
//! destroy_readback(handle)
//! ```
//!
//! | Step | Vulkan | WebGPU |
//! | --- | --- | --- |
//! | request | record the completion point (timeline value) to watch | call `mapAsync` |
//! | poll | compare the timeline's current value | has the promise resolved? |
//! | ready | read the already-host-visible mapping | `getMappedRange` |
//! | destroy | drop the tracking entry | `unmap` |
//!
//! Neither backend has to invent anything, and neither has to block.
//!
//! # There is no blocking convenience wrapper
//!
//! Deliberately. A `read_buffer_blocking` documented as "native only" would
//! become what `crcbl screenshot` and the P1 golden-image path were built on —
//! they are written on Linux, against `crcbl-vk`, and would work — and the
//! breakage would surface at P5 when those exact paths are asked to run in a
//! browser. One API that both tiers implement is cheaper than two APIs and a
//! rule nobody remembers. Callers that genuinely have nothing else to do
//! ([`wait_idle`](crate::Device::wait_idle) at shutdown, a load-time asset
//! probe) poll in a loop with a deadline; that loop is three lines and it is
//! correct everywhere.
//!
//! # Ordering
//!
//! A readback observes work **already submitted** when
//! [`request_readback`](crate::Device::request_readback) is called. Requesting
//! before submitting the copy that fills the buffer is a caller bug that reads
//! stale bytes, not an error the backend can detect. Pass
//! [`ReadbackDesc::after`] to name an explicit completion point when the
//! caller has one — the render graph always does.
//!
//! # This is still the *one* permitted readback
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.5 allows exactly one readback in
//! the frame loop: culling stats, N frames latent, debug builds only. Making
//! readback poll-shaped is what makes "N frames latent" expressible at all — a
//! blocking read has no way to be latent.

use crcbl_core::Handle;

use crate::{BufferHandle, SemaphoreWait};

/// Marker type for readback handles. Uninhabited.
#[derive(Debug)]
pub enum Readback {}

/// An in-flight readback request.
pub type ReadbackHandle = Handle<Readback>;

/// What a readback request covers, and what it waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Buffer to read. Must be
    /// [`MemoryLocation::HostReadback`](crate::MemoryLocation::HostReadback) —
    /// the same requirement WebGPU imposes through `MAP_READ` usage.
    pub buffer: BufferHandle,
    /// Byte offset within the buffer.
    pub offset: u64,
    /// Bytes to read. [`poll_readback`](crate::Device::poll_readback)'s output
    /// slice must be exactly this long.
    pub size: u64,
    /// Completion point to wait for, if the caller has one.
    ///
    /// `None` means "everything submitted to this device before this call",
    /// which is exactly WebGPU's `mapAsync` semantics and what a Vulkan backend
    /// gets by snapshotting its own submission timeline.
    ///
    /// `Some(wait)` names a timeline value instead — the shape the render graph
    /// uses, since it already signals a value per frame and knows which one
    /// covers the copy.
    pub after: Option<SemaphoreWait>,
}

/// Where a readback request has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadbackState {
    /// Not ready. Poll again next frame; the output slice was not written.
    Pending,
    /// Ready. The output slice now holds the bytes.
    Ready,
}

impl ReadbackState {
    /// Whether the data is available.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_state_is_a_two_way_answer_not_three() {
        // Failure is reported as `Err(HalError)` from `poll_readback`, not as a
        // third state: a `Failed` variant would throw away *why*, which for a
        // device-lost or map-rejected readback is the only useful part.
        assert!(ReadbackState::Ready.is_ready());
        assert!(!ReadbackState::Pending.is_ready());
    }

    #[test]
    fn a_request_without_an_explicit_wait_means_everything_submitted_so_far() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let desc = ReadbackDesc {
            label: Some("culling stats"),
            buffer: pool.insert(0).cast(),
            offset: 0,
            size: 8,
            after: None,
        };
        assert!(desc.after.is_none());
        assert_eq!(desc.size, 8);
    }
}
