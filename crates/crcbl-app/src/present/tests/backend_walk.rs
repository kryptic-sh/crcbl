//! An unpinned open moving to the next backend when one cannot present to the
//! window at all, and a pinned one stopping there.

use super::*;

use crate::backend::{Registration, ready, request_open_auto, request_open_named};
use crcbl_hal::null::NullInstance;

/// What [`RefusingSurface`]'s `create_surface` answers: Vulkan's refusal of a
/// transparent Win32 window, in kind.
fn unsupported() -> HalError {
    HalError::Unsupported {
        backend: crcbl_hal::BackendKind::Vulkan,
        what: "this backend cannot present to a transparent window",
    }
}

/// A refusal of another kind, which says something went wrong rather than
/// that the target is out of this backend's reach.
fn out_of_memory() -> HalError {
    HalError::OutOfHostMemory
}

/// A [`NullInstance`] that opens and enumerates as usual and refuses every
/// surface with `refusal`, standing in for a backend that cannot present to
/// the window it was given.
#[derive(Debug)]
struct RefusingSurface {
    inner: NullInstance,
    refusal: fn() -> HalError,
}

impl RefusingSurface {
    fn open(refusal: fn() -> HalError) -> crate::backend::InstanceFuture {
        ready(Ok(Box::new(Self {
            inner: NullInstance::gpu_driven(),
            refusal,
        })))
    }
}

impl Instance for RefusingSurface {
    fn backend(&self) -> crcbl_hal::BackendKind {
        crcbl_hal::BackendKind::Vulkan
    }

    fn adapters(&self) -> Vec<crcbl_hal::AdapterInfo> {
        self.inner.adapters()
    }

    unsafe fn create_surface(
        &self,
        _target: &crcbl_hal::SurfaceTarget,
    ) -> Result<SurfaceHandle, HalError> {
        Err((self.refusal)())
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.inner.destroy_surface(surface);
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: crcbl_hal::AdapterId,
    ) -> Result<crcbl_hal::SurfaceCaps, HalError> {
        self.inner.surface_caps(surface, adapter)
    }

    fn request_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<Box<dyn crcbl_hal::PendingDevice>, HalError> {
        self.inner.request_device(desc)
    }
}

fn null_open() -> crate::backend::InstanceFuture {
    ready(Ok(Box::new(NullInstance::gpu_driven())))
}

/// Windows' table in shape: a first backend that refuses the window as
/// unsupported, and an automatic one after it that presents to it.
static REFUSED_THEN_NULL: &[Registration] = &[
    Registration {
        backend: GpuBackend::Vulkan,
        auto: true,
        open: || RefusingSurface::open(unsupported),
    },
    Registration {
        backend: GpuBackend::Null,
        auto: true,
        open: null_open,
    },
];

/// The same, with the first backend failing for a reason that is not about
/// the target.
static FAILED_THEN_NULL: &[Registration] = &[
    Registration {
        backend: GpuBackend::Vulkan,
        auto: true,
        open: || RefusingSurface::open(out_of_memory),
    },
    Registration {
        backend: GpuBackend::Null,
        auto: true,
        open: null_open,
    },
];

/// A refusing backend with nothing automatic after it.
static REFUSED_ALONE: &[Registration] = &[
    Registration {
        backend: GpuBackend::Vulkan,
        auto: true,
        open: || RefusingSurface::open(unsupported),
    },
    Registration {
        backend: GpuBackend::Null,
        auto: false,
        open: null_open,
    },
];

/// Polls an open of the headless window whose instance comes from `instance`
/// until it yields a presenter or fails.
fn open_through(
    instance: crate::backend::PendingInstance,
) -> (HeadlessShell, Result<Presenter, PresentError>) {
    let (shell, mut pending) = adapter_walk_request(None);
    pending.stage = OpenStage::Instance(instance);
    let result = loop {
        match pending.poll() {
            Ok(Some(presenter)) => break Ok(presenter),
            Ok(None) => {}
            Err(error) => break Err(error),
        }
    };
    (shell, result)
}

/// **Unpinned, a backend that cannot present to the window is passed over
/// and the next one opens it.** Read from the finished presenter's own
/// instance, because which backend drew the window is the claim.
#[test]
fn an_unpinned_open_moves_past_a_backend_that_refuses_the_window() {
    let (_shell, opened) = open_through(request_open_auto(REFUSED_THEN_NULL));
    let mut gpu = opened.expect("the second backend presents to the window");
    assert_eq!(gpu.instance.backend(), crcbl_hal::BackendKind::Null);
    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// **The fall-through is said in one line**, naming the backend that refused
/// and why — the line a bug report about which backend drew a window starts
/// from.
#[test]
fn the_fall_through_says_which_backend_refused_and_why() {
    let logs = crcbl_core::log::capture();
    let (_shell, opened) = open_through(request_open_auto(REFUSED_THEN_NULL));
    let mut gpu = opened.expect("the second backend presents to the window");

    let records = logs.records();
    let said: Vec<_> = records
        .iter()
        .filter(|record| record.message.contains("trying the next backend"))
        .collect();
    assert_eq!(said.len(), 1, "once, for the one refusal: {records:?}");
    let line = &said[0].message;
    assert!(
        line.contains("this backend cannot present to a transparent window"),
        "the backend's own reason rides along: {line}"
    );
    assert!(
        !line.contains("  "),
        "one sentence, with no wrapped literal's indentation inside it: {line:?}"
    );

    gpu.drain().expect("nothing was submitted");
    gpu.destroy()
        .expect("teardown is in the seam's stated order");
}

/// **A backend named by `PresenterDesc::backend` or `CRCBL_GPU` that refuses
/// the window ends the open with its refusal**, rather than quietly drawing
/// on another one.
#[test]
fn a_named_backend_that_refuses_the_window_ends_the_open() {
    let named =
        request_open_named(REFUSED_THEN_NULL, GpuBackend::Vulkan).expect("the table has the entry");
    let (_shell, opened) = open_through(named);
    let refusal = opened.err();
    let Some(PresentError::Hal(HalError::Unsupported { what, .. })) = &refusal else {
        panic!("a named backend's refusal is final: {refusal:?}");
    };
    assert_eq!(*what, "this backend cannot present to a transparent window");
}

/// **Only an `Unsupported` refusal moves on.** Any other failure of
/// `create_surface` is a fault in the backend that opened, and walking past
/// it would hide that.
#[test]
fn a_surface_failure_that_is_not_unsupported_ends_the_open() {
    let (_shell, opened) = open_through(request_open_auto(FAILED_THEN_NULL));
    let refusal = opened.err();
    assert!(
        matches!(refusal, Some(PresentError::Hal(HalError::OutOfHostMemory))),
        "{refusal:?}"
    );
}

/// **With no automatic backend left, the refusal is what the open reports**,
/// not a bare "no backend".
#[test]
fn the_last_backend_refusing_the_window_reports_its_refusal() {
    let (_shell, opened) = open_through(request_open_auto(REFUSED_ALONE));
    let refusal = opened.err();
    assert!(
        matches!(
            refusal,
            Some(PresentError::Hal(HalError::Unsupported { .. }))
        ),
        "{refusal:?}"
    );
}
