//! Errors crossing the seam.
//!
//! Two enums, split by who has to handle them:
//!
//! * [`HalError`] — something went wrong. Callers log it and usually give up;
//!   `docs/plan/02-vulkan-backend.md` is explicit that the MVP has no fallback
//!   paths ("error clearly and exit").
//! * [`SurfaceError`] — presentation-specific, and *routinely non-fatal*.
//!   [`SurfaceError::OutOfDate`] happens on every window resize and is part of
//!   the normal frame loop, not an exceptional condition.
//!
//! No variant carries a backend type. Where a backend has detail worth keeping
//! (a driver message, a shader compiler diagnostic) it goes into a `String`,
//! because the alternative is either losing it or leaking `ash::vk::Result`
//! into every caller.

use crate::Features;

/// A backend operation failed.
#[derive(Debug, thiserror::Error)]
pub enum HalError {
    /// No adapter with that index exists.
    #[error("no adapter with index {0}")]
    NoSuchAdapter(u32),

    /// The chosen adapter lacks features the caller required.
    ///
    /// `missing` is the exact gap, from
    /// [`DeviceCaps::missing`](crate::DeviceCaps::missing) — so the log line
    /// says which capability to go look up, not merely that something was
    /// absent.
    #[error("adapter lacks required features: {missing:?}")]
    UnsupportedFeatures {
        /// Features requested but not present.
        missing: Features,
    },

    /// A handle did not resolve: it was destroyed, never created, or belongs to
    /// a different device.
    ///
    /// The generational [`Handle`](crcbl_core::Handle) makes this a clean
    /// failure rather than an alias to whatever took the slot; see
    /// `crcbl-core`'s handle module.
    #[error("invalid or stale {kind} handle {bits:#x}")]
    InvalidHandle {
        /// Resource kind, e.g. `"buffer"`.
        kind: &'static str,
        /// The handle's packed bits, from
        /// [`Handle::to_bits`](crcbl_core::Handle::to_bits).
        bits: u64,
    },

    /// A handle resolved, but it belongs to a different [`Instance`] or
    /// [`Device`] than the one it was given to.
    ///
    /// Distinct from [`HalError::InvalidHandle`] because the causes and the
    /// fixes differ: a stale handle means "you kept this too long", a foreign
    /// one means "you crossed two objects that never met". Both are caller
    /// bugs; neither may be undefined behaviour.
    ///
    /// Handle bits are only unique within the backend that issued them, so two
    /// instances *will* hand out identical bits. Detecting this is therefore an
    /// implementation obligation on backends, not a property of the handle —
    /// see [`crate::device`] for the required mechanism.
    ///
    /// [`Instance`]: crate::Instance
    /// [`Device`]: crate::Device
    #[error("{kind} handle {bits:#x} belongs to a different instance or device")]
    ForeignObject {
        /// Resource kind, e.g. `"surface"`.
        kind: &'static str,
        /// The handle's packed bits.
        bits: u64,
    },

    /// Device memory exhausted.
    #[error("out of device memory")]
    OutOfDeviceMemory,

    /// Host memory exhausted.
    #[error("out of host memory")]
    OutOfHostMemory,

    /// The device was lost — driver reset, GPU hang, TDR, or the browser
    /// reclaiming the context. Everything created from it is dead.
    #[error("device lost: {0}")]
    DeviceLost(String),

    /// SPIR-V rejected, or cross-compilation to MSL/DXIL/WGSL failed.
    #[error("shader compilation failed: {0}")]
    ShaderCompilation(String),

    /// The pipeline state was rejected by the driver.
    #[error("pipeline creation failed: {0}")]
    PipelineCreation(String),

    /// A descriptor field was out of range or self-inconsistent — a caller bug,
    /// which is why the message is expected to name the field.
    #[error("invalid descriptor: {0}")]
    InvalidDescriptor(String),

    /// The operation is real but this backend cannot do it. Distinct from
    /// [`HalError::UnsupportedFeatures`], which is about a *device*.
    #[error("unsupported by the {backend} backend: {what}")]
    Unsupported {
        /// Backend that refused.
        backend: crate::BackendKind,
        /// What was asked for.
        what: &'static str,
    },

    /// Anything else the backend wants to report verbatim.
    #[error("{0}")]
    Backend(String),
}

/// A surface or swapchain operation failed.
///
/// Split out from [`HalError`] because the frame loop must handle
/// [`SurfaceError::OutOfDate`] as a normal event: every resize produces one, and
/// a caller that treats it as fatal has a bug that only appears when someone
/// drags a window edge.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    /// The swapchain no longer matches the surface — the window resized, the
    /// canvas changed size, or the display mode changed. Reconfigure with
    /// [`Device::reconfigure_swapchain`](crate::Device::reconfigure_swapchain)
    /// and try again next frame.
    ///
    /// This is expected traffic, not an error condition.
    #[error("swapchain out of date; reconfigure and retry")]
    OutOfDate,

    /// The surface is gone — the window was destroyed, or the compositor
    /// dropped the connection. Not recoverable by reconfiguring.
    #[error("surface lost")]
    Lost,

    /// Acquire timed out with no image available.
    ///
    /// Backends with an implicit acquire (WebGPU's `getCurrentTexture`) never
    /// produce this.
    #[error("timed out waiting for a swapchain image")]
    Timeout,

    /// A general failure underneath.
    #[error(transparent)]
    Hal(#[from] HalError),
}

impl HalError {
    /// Builds an [`HalError::InvalidHandle`] from a typed handle.
    ///
    /// Backends call this in their handle-lookup path so the error carries the
    /// same bits that appear in `Handle`'s `Debug` output.
    #[must_use]
    pub fn invalid_handle<T>(kind: &'static str, handle: crcbl_core::Handle<T>) -> Self {
        Self::InvalidHandle {
            kind,
            bits: handle.to_bits(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendKind, Buffer};
    use crcbl_core::{Handle, Pool};

    #[test]
    fn messages_name_the_specific_problem() {
        let missing = Features::DESCRIPTOR_INDEXING | Features::BUFFER_DEVICE_ADDRESS;
        let text = HalError::UnsupportedFeatures { missing }.to_string();
        assert!(text.contains("DESCRIPTOR_INDEXING"), "{text}");
        assert!(text.contains("BUFFER_DEVICE_ADDRESS"), "{text}");

        let text = HalError::Unsupported {
            backend: BackendKind::WebGpu,
            what: "push constants",
        }
        .to_string();
        assert_eq!(text, "unsupported by the webgpu backend: push constants");
    }

    #[test]
    fn invalid_handle_carries_the_handle_bits() {
        let mut pool: Pool<u8> = Pool::new();
        let handle: Handle<u8> = pool.insert(0);
        let typed: Handle<Buffer> = handle.cast();
        let error = HalError::invalid_handle("buffer", typed);
        let HalError::InvalidHandle { kind, bits } = error else {
            panic!("wrong variant");
        };
        assert_eq!(kind, "buffer");
        assert_eq!(bits, typed.to_bits());
        assert_ne!(bits, 0, "packed handle bits are never zero");
    }

    #[test]
    fn out_of_date_is_its_own_variant_and_survives_a_hal_error_wrap() {
        // The frame loop matches on `OutOfDate` directly; if a `HalError`
        // conversion ever swallowed it into `Hal(..)`, resize handling would
        // silently become fatal. (`matches!(SurfaceError::OutOfDate,
        // SurfaceError::OutOfDate)` used to stand here and could not fail.)
        let wrapped: SurfaceError = HalError::OutOfHostMemory.into();
        assert!(matches!(wrapped, SurfaceError::Hal(_)));
        assert!(
            !matches!(
                wrapped,
                SurfaceError::OutOfDate | SurfaceError::Lost | SurfaceError::Timeout
            ),
            "and the conversion must not manufacture a presentation variant \
             either — a resize is something a backend decides, never something \
             a generic error becomes"
        );
        // `transparent`, so the wrap keeps the inner message rather than
        // prefixing one a caller would have to strip.
        assert_eq!(wrapped.to_string(), HalError::OutOfHostMemory.to_string());
    }
}
