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
//! browser. One API that every backend implements is cheaper than two APIs and a
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

    /// [`ReadbackDesc::after`] is the field with two meanings, so both are read
    /// back through a backend rather than off the literal that was just
    /// written: `None` needs no synchronisation object to exist at all, and
    /// `Some` names one the device must resolve.
    ///
    /// That a `None` request observes *everything submitted so far* is not
    /// checked here and cannot be: [`crate::null`] records commands rather than
    /// executing them, so no submission it accepts changes a byte. The backends
    /// that do execute check it in their own e2e suites.
    #[test]
    fn a_readback_wait_is_optional_and_resolved_when_it_is_given() {
        use crate::null::NullInstance;
        use crate::{
            AdapterId, BufferDesc, BufferUsage, DeviceDesc, HalError, Instance, MemoryLocation,
            SemaphoreDesc, SemaphoreKind,
        };

        let instance = NullInstance::gpu_driven();
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .expect("the gpu_driven preset opens");
        let buffer = device
            .create_buffer(&BufferDesc {
                label: Some("culling stats"),
                size: 8,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        let desc = ReadbackDesc {
            label: Some("culling stats"),
            buffer,
            offset: 0,
            size: 8,
            after: None,
        };

        // No wait named, and no semaphore in existence anywhere: the request
        // still stands and completes.
        let readback = device
            .request_readback(&desc)
            .expect("an unqualified request needs no synchronisation object");
        let mut bytes = [0u8; 8];
        assert_eq!(
            device.poll_readback(readback, &mut bytes).expect("poll"),
            ReadbackState::Ready
        );
        // The size is the contract on the output slice, not a hint.
        let error = device
            .poll_readback(readback, &mut [0u8; 4])
            .expect_err("a slice of the wrong length is a caller bug");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        device.destroy_readback(readback);

        // A named wait is resolved, so a stale one is refused rather than
        // ignored — the failure mode that would make a latent readback return
        // whatever happened to be in the buffer.
        let semaphore = device
            .create_semaphore(&SemaphoreDesc {
                label: Some("frame timeline"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .expect("a timeline semaphore");
        let named = ReadbackDesc {
            after: Some(SemaphoreWait {
                semaphore,
                value: 1,
            }),
            ..desc
        };
        let waiting = device
            .request_readback(&named)
            .expect("a live semaphore is accepted");
        device.destroy_readback(waiting);
        device.destroy_semaphore(semaphore);
        let error = device
            .request_readback(&named)
            .expect_err("a destroyed semaphore must not be silently ignored");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    }
}
