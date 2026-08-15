//! Surfaces, swapchains, and the acquire/present pair — the slice that makes
//! macOS able to put a pixel on a screen.
//!
//! `docs/backlog.md`'s 2026-08-05 decision makes Metal the *only* Apple GPU
//! path, so until this module existed macOS had no presentation at all. There
//! are two targets here and they are genuinely different mechanisms, not one
//! with a flag:
//!
//! * [`SurfaceTarget::AppKit`] — a `CAMetalLayer` the shell owns. Its "images"
//!   are `CAMetalDrawable`s borrowed from Core Animation one at a time and
//!   given back at present.
//! * [`SurfaceTarget::Offscreen`] — no window at all, and a ring of plain
//!   `MTLTexture`s this module allocates. Acquire rotates the ring, present
//!   advances it. That is what makes `crcbl screenshot` and the golden-image
//!   e2e run through the *same* acquire/present path as a window, and — because
//!   it needs neither a window server nor a shader — it is the half CI can
//!   actually execute end to end.
//!
//! # The extent rule, discharged
//!
//! `crcbl-hal`'s [`swapchain`](crcbl_hal::swapchain) module states four numbered
//! obligations, and Metal meets them more simply than Vulkan does because
//! **nothing pins a range**: a `CAMetalLayer`'s `drawableSize` is whatever it is
//! set to, so there is no `minImageExtent == maxImageExtent == currentExtent`
//! case like X11's, and obligation 1 holds exactly rather than approximately.
//!
//! 1. **The shell's size is the request.** [`resolve_extent`] starts from
//!    [`SwapchainDesc::extent`] and never reads the layer's own size.
//! 2. **A backend clamps into the platform's range and reports what it
//!    configured.** The only range here is the device's own
//!    [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d), which no window
//!    reaches; the clamp exists so a nonsense request produces a texture Metal
//!    will actually make rather than an Objective-C raise, and it logs when it
//!    fires because that means the request was wrong rather than merely stale.
//! 3. **A caller renders at [`AcquiredFrame::extent`].** On the layer path that
//!    number is read off the drawable's own texture rather than remembered,
//!    because Core Animation may resize the layer between the configure and the
//!    acquire — and when it differs from what was configured, the frame is
//!    reported [`suboptimal`](AcquiredFrame::suboptimal), which is exactly what
//!    that flag means.
//! 4. **A zero extent is the caller's problem.** Refused with the rule named,
//!    never guessed at from the layer.
//!
//! # sRGB: one table, no second mapping
//!
//! `CAMetalLayer` has its own `pixelFormat`, which is the second place in this
//! backend a texel format could be decided — and the first, `conv`'s table,
//! records the bug `crcbl-wgpu` shipped when an sRGB encode went missing and the
//! browser build came out too dark. So the layer's format is set from
//! [`conv::pixel_format`] and from nothing else, and the offered
//! [`SurfaceCaps::formats`] list is ordered so that
//! [`SurfaceCaps::preferred_format`] — documented as "the first sRGB format, or
//! the first format at all" — lands on [`Format::Bgra8UnormSrgb`].
//!
//! **BGRA and not RGBA**, and that is not a preference: Apple documents
//! `CAMetalLayer::pixelFormat` as accepting `BGRA8Unorm`, `BGRA8Unorm_sRGB`,
//! `RGBA16Float` and the extended-range formats, and *not* the RGBA8 pair. A
//! caps list naming [`Format::Rgba8UnormSrgb`] would hand a caller a format the
//! layer raises on. The offscreen ring has no such restriction and offers both,
//! which is the only reason the two lists differ.
//!
//! # WSI acquire needs no semaphore here, which is how MTL3's open problem ends
//!
//! MTL3 emulates a binary semaphore with an `MTLEvent` plus a counter, under the
//! rule that it must be signalled by an **earlier submission** than the one
//! waiting on it — and `docs/backlog.md` recorded that WSI acquire breaks the
//! rule, because in Vulkan the presentation engine signals it and no submission
//! does.
//!
//! **Metal has no such signal to reconcile.** `nextDrawable` blocks the *CPU*
//! until a drawable is free and hands back a texture that is ready to be
//! rendered into; ordering against the previous frame's present is the queue's,
//! established by committing the presenting command buffer onto it. There is
//! nothing for a semaphore to represent, so this backend creates none and
//! [`AcquiredFrame::acquire_semaphore`] and
//! [`AcquiredFrame::present_semaphore`] are both `None` — the implicit-acquire
//! shape the seam documents for `crcbl-wgpu`, and the reason it made them
//! `Option`s at all. MTL3's rule is therefore never engaged rather than worked
//! around, and no seam change is needed.
//!
//! A caller may still hand [`PresentInfo::waits`] a semaphore it signalled
//! itself; those are honoured, encoded onto the presenting command buffer with
//! `encodeWaitForEvent:value:` ahead of `presentDrawable:`.
//!
//! # A present is a command buffer, not `MTLDrawable::present`
//!
//! `MTLDrawable` has a bare `present`, and using it would be wrong: it hands the
//! drawable to the compositor *now*, from the CPU, with the GPU possibly still
//! writing it. `MTLCommandBuffer::presentDrawable:` schedules the hand-off for
//! when that command buffer completes, which — given a queue that runs its
//! command buffers in commit order, the premise MTL3's `wait_idle` and readback
//! already rest on — is after the caller's own submission has finished writing
//! the image.
//!
//! # Present feedback is a callback, so the number is kept on this side
//!
//! [`Features::PRESENT_FEEDBACK`](crcbl_hal::Features::PRESENT_FEEDBACK) is
//! advertised, and Metal answers it in the third of the three shapes the seam
//! names: `MTLDrawable::addPresentedHandler:` runs a block once the drawable
//! has been shown, with no id in it and nothing to block on. So `present`
//! attaches a handler carrying the caller's own
//! [`PresentInfo::present_id`](crcbl_hal::PresentInfo::present_id), and
//! `wait_until_presented` sleeps on a condition variable until that number has
//! come back. `crcbl_mtl::present` is where all of that lives — plain `u64`s
//! under a lock, no Objective-C — and it argues the ordering, the reset across
//! a reconfigure and why an `Arc` of it is sound to hand to a block that runs
//! on a thread Core Animation picks.
//!
//! **The offscreen ring answers immediately**, because a cursor bump has no
//! drawable and nothing will ever call back for it. That is not a check in the
//! wait: [`SwapchainTarget::Offscreen`] simply has no ledger to consult, so the
//! seam's documented answer for a swapchain with nothing to wait on is the
//! shape of the type.
//!
//! # A drawable's handles die at present; a ring image's do not
//!
//! [`AcquiredFrame::image`] is documented as "valid until the matching
//! present", and on the layer path this backend takes that literally: the image
//! and view rows are inserted into the device's pools at acquire and **removed
//! at present**, so using either afterwards is a clean
//! [`HalError::InvalidHandle`].
//!
//! That is not tidiness. A `CAMetalDrawable` is borrowed from a pool of
//! `maximumDrawableCount` drawables, and holding its texture past the present
//! keeps the drawable out of that pool: hold `image_count` of them and the next
//! `nextDrawable` has nothing to hand back and blocks until it times out. The
//! ring images are ours and have no such owner, so their handles are stable for
//! the whole life of the swapchain, exactly as `crcbl-vk`'s are.

use core::ptr::NonNull;
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    AcquiredFrame, AdapterId, CompositeAlpha, Device as _, DisplayTiming, Extent3d, Format,
    HalError, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc,
    ImageViewHandle, ImageViewType, PresentInfo, PresentMode, QueueHandle, SurfaceCaps,
    SurfaceError, SurfaceHandle, SwapchainDesc, SwapchainHandle,
};
use objc2::ClassType;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSObjectProtocol, NSString};
use objc2_metal::{MTLCommandBuffer, MTLDrawable, MTLTexture};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

use crate::conv;
use crate::device::{
    DeviceState, MetalDevice, Owned, local_handle, lookup, lookup_mut, owned, to_ns,
};
use crate::instance::{InstanceInner, MetalInstance};
use crate::present::{PresentLedger, PresentWait};

/// What a [`SurfaceHandle`] names.
pub(crate) enum SurfaceKind {
    /// A `CAMetalLayer` the shell created and still owns. Retained, so the
    /// obligation that a swapchain outlive its surface *handle* is discharged
    /// by reference counting — see [`SurfaceEntry`].
    Layer(Retained<CAMetalLayer>),
    /// No window system at all.
    Offscreen,
}

impl core::fmt::Debug for SurfaceKind {
    /// A `CAMetalLayer`'s own `description` is long and would put a pointer in
    /// every log line, so only which kind of surface this is gets printed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Layer(_) => "Layer",
            Self::Offscreen => "Offscreen",
        })
    }
}

/// A surface, and the instance that issued it.
///
/// # Obligation 2 costs nothing here
///
/// `crcbl-hal`'s [`device`](crcbl_hal::device) obligation 2 requires a backend
/// to invalidate a surface handle immediately on
/// [`destroy_surface`](crcbl_hal::Instance::destroy_surface) while **deferring**
/// the underlying driver object until the last swapchain on it is gone, because
/// `vkDestroySurfaceKHR` with a live swapchain is undefined behaviour.
/// `crcbl-vk` needs a reference count and a zombie list for that.
///
/// This backend needs neither. The object is a `CAMetalLayer`, every swapchain
/// holds its own `Retained` clone of it, and Objective-C reference counting *is*
/// the deferral: dropping this entry releases one reference and the layer stays
/// alive while any swapchain still names it. The handle dies when the caller
/// says so; the object dies when it is safe.
#[derive(Debug)]
pub(crate) struct SurfaceEntry {
    pub(crate) owner: u64,
    pub(crate) kind: SurfaceKind,
}

/// The images a swapchain hands out, which is a different mechanism per target.
pub(crate) enum SwapchainTarget {
    /// A `CAMetalLayer`. Nothing is allocated up front — `nextDrawable` is the
    /// allocation — so what is kept here is only what has to be given back.
    Layer {
        layer: Retained<CAMetalLayer>,
        /// The drawable the last acquire handed out, retained until present
        /// returns it to Core Animation.
        drawable: Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>,
        /// The pool rows naming that drawable's texture, removed at present.
        /// See the module docs for why they must not outlive it.
        rows: Option<(ImageHandle, ImageViewHandle)>,
        /// What [`MetalDevice::wait_until_presented_impl`] is answered from,
        /// and what each drawable's presented handler reports into. Shared with
        /// those handlers, which is why it is an `Arc` and why it holds nothing
        /// but numbers under a lock — [`PresentLedger`] argues both.
        ///
        /// **Only this variant has one.** An offscreen ring has no drawable to
        /// observe, so the seam's immediate answer for a ring is the absence of
        /// a field rather than a branch that could be deleted.
        presents: Arc<PresentLedger>,
    },
    /// A ring of plain textures this module allocated, with stable handles.
    Offscreen {
        images: Vec<ImageHandle>,
        views: Vec<ImageViewHandle>,
    },
}

impl core::fmt::Debug for SwapchainTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Layer { drawable, .. } => f
                .debug_struct("Layer")
                .field("acquired", &drawable.is_some())
                .finish_non_exhaustive(),
            Self::Offscreen { images, .. } => f
                .debug_struct("Offscreen")
                .field("images", &images.len())
                .finish_non_exhaustive(),
        }
    }
}

/// What a [`SwapchainHandle`] resolves to.
#[derive(Debug)]
pub(crate) struct SwapchainEntry {
    /// Device that created it, per obligation 3.
    pub(crate) owner: u64,
    /// The surface it was configured on. Kept so a reconfigure naming a
    /// different one is refused rather than silently retargeting.
    surface: SurfaceHandle,
    target: SwapchainTarget,
    format: Format,
    /// The extent this swapchain was **configured** at — what the layer was
    /// told, and what the ring textures were made at. On the layer path the
    /// number reported to a caller comes off the drawable instead; see
    /// obligation 3 in the module docs.
    extent: (u32, u32),
    /// Ring position of the outstanding acquire, if there is one.
    acquired: Option<u32>,
    /// Next ring position to hand out.
    next_index: u32,
    /// How many positions the ring has.
    image_count: u32,
}

owned!(SurfaceEntry, SwapchainEntry);

/// The most drawables a `CAMetalLayer` will vend.
///
/// Apple documents `maximumDrawableCount` as accepting 2 or 3 and raising
/// otherwise, so this is an API constant rather than a device one — the same
/// class of fact as
/// [`MAX_SAMPLER_ANISOTROPY`](crate::device::MAX_SAMPLER_ANISOTROPY).
const MAX_LAYER_DRAWABLES: u32 = 3;

/// The fewest drawables a `CAMetalLayer` will vend. See
/// [`MAX_LAYER_DRAWABLES`].
const MIN_LAYER_DRAWABLES: u32 = 2;

/// The most images the offscreen ring will allocate. The same ceiling
/// `crcbl-vk`'s ring uses, so a caller asking for one image count gets the same
/// ring on both backends.
const MAX_OFFSCREEN_IMAGES: u32 = 8;

/// The formats a `CAMetalLayer` is offered, best first.
///
/// See the module docs: sRGB leads so [`SurfaceCaps::preferred_format`] picks
/// it, and the RGBA8 pair is absent because Core Animation does not accept it.
const LAYER_FORMATS: &[Format] = &[
    Format::Bgra8UnormSrgb,
    Format::Bgra8Unorm,
    Format::Rgba16Float,
];

/// The formats the offscreen ring is offered, best first.
///
/// Deliberately the same list `crcbl-vk`'s `offscreen_surface_caps` offers, so a
/// golden-image test that picks a format on one backend picks the same one on
/// the other.
const OFFSCREEN_FORMATS: &[Format] = &[
    Format::Rgba8UnormSrgb,
    Format::Bgra8UnormSrgb,
    Format::Rgba8Unorm,
    Format::Bgra8Unorm,
    Format::Rgba16Float,
];

/// What a `CAMetalLayer` supports.
///
/// `present_modes` is `[Fifo, Immediate]` because those are the two
/// `displaySyncEnabled` expresses. [`PresentMode::Mailbox`] is **absent** rather
/// than aliased onto `Immediate`: mailbox promises no tearing and
/// `displaySyncEnabled = false` tears. [`PresentMode::FifoRelaxed`] has no Metal
/// spelling at all.
///
/// `composite_alpha` is `[Opaque]` alone. `CALayer::setOpaque:` is the only knob
/// Core Animation offers here, and what a non-opaque Metal layer does with an
/// alpha channel is not something this slice has verified end to end — so it is
/// not claimed.
#[must_use]
pub(crate) fn layer_surface_caps(layer: &CAMetalLayer) -> SurfaceCaps {
    SurfaceCaps {
        formats: LAYER_FORMATS.to_vec(),
        present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: MIN_LAYER_DRAWABLES,
        max_image_count: MAX_LAYER_DRAWABLES,
        current_extent: layer_extent(layer),
    }
}

/// The capabilities an offscreen "surface" reports.
///
/// There is no window system to ask, so this is a statement of what the ring
/// supports rather than a query. `current_extent` is `None` because — exactly
/// like Wayland — nothing here has an opinion about the size.
#[must_use]
pub(crate) fn offscreen_surface_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: OFFSCREEN_FORMATS.to_vec(),
        present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: 1,
        max_image_count: MAX_OFFSCREEN_IMAGES,
        current_extent: None,
    }
}

/// The layer's own idea of its size in pixels, or `None` when it has none yet.
///
/// **Obligation 4's cross-check, and never the source of truth.** A
/// `CAMetalLayer` reports `drawableSize` in device pixels — Core Animation
/// derives it from `bounds × contentsScale` until something sets it — so unlike
/// Wayland there genuinely is a number here. It is still only a cross-check: a
/// zero or non-finite size is the layer saying "not laid out yet", which is the
/// absence of a value rather than a size of zero, and is reported as `None`
/// rather than passed through the way Vulkan's `0xFFFFFFFF` sentinel must not
/// be.
fn layer_extent(layer: &CAMetalLayer) -> Option<(u32, u32)> {
    let size = layer.drawableSize();
    Some((to_pixels(size.width)?, to_pixels(size.height)?))
}

/// One axis of a `CGSize` as a pixel count, or `None` if it is not one.
fn to_pixels(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 1.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(value.min(f64::from(u32::MAX)) as u32)
}

/// What [`resolve_extent`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExtentDecision {
    /// The extent the swapchain is actually configured at.
    pub(crate) configured: (u32, u32),
    /// Whether the device's texture limit forced a change.
    pub(crate) clamped: bool,
}

/// Resolves the extent a swapchain is configured at.
///
/// **Obligations 1 and 4.** The requested extent is the shell's and is where
/// this starts; the layer's own `drawableSize` is never consulted. The clamp is
/// against `ceiling`, this device's `max_image_2d`, because that is the largest
/// texture Metal will make and asking for more raises rather than returning nil.
///
/// # Errors
///
/// A zero extent, which obligation 4 makes the caller's problem: an
/// unconfigured or minimized window has no size, and the answer is to not
/// create a swapchain yet.
pub(crate) fn resolve_extent(
    requested: (u32, u32),
    ceiling: u32,
) -> Result<ExtentDecision, SurfaceError> {
    if requested.0 == 0 || requested.1 == 0 {
        return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
            "SwapchainDesc::extent is {requested:?}; an unconfigured or minimized window means \
             'do not create one yet' rather than 'pick something'"
        ))));
    }
    let ceiling = ceiling.max(1);
    let configured = (requested.0.min(ceiling), requested.1.min(ceiling));
    Ok(ExtentDecision {
        configured,
        clamped: configured != requested,
    })
}

/// Picks the present mode a swapchain actually gets.
///
/// The seam documents a fallback rather than a failure — "the backend falls back
/// to [`PresentMode::Fifo`] if unavailable, which is always supported" — so an
/// unsupported request is downgraded and logged, never refused.
#[must_use]
pub(crate) fn resolve_present_mode(requested: PresentMode, caps: &SurfaceCaps) -> PresentMode {
    if caps.supports_present_mode(requested) {
        return requested;
    }
    crcbl_core::log::debug!(
        "crcbl-mtl: {requested:?} is not available on this surface; falling back to Fifo, which \
         always is"
    );
    PresentMode::Fifo
}

impl MetalInstance {
    /// Creates a surface from a shell's native handles.
    ///
    /// # Safety
    ///
    /// The caller's obligations are
    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface)'s, and
    /// the one that does work here is the first: for
    /// [`SurfaceTarget::AppKit`] the pointer must name a live `CAMetalLayer *`.
    /// This retains it, which is a message send, so a pointer that is not an
    /// Objective-C object at all is undefined behaviour no check inside this
    /// function could catch.
    ///
    /// What *is* checked, immediately afterwards, is that the object is a
    /// `CAMetalLayer` rather than some other layer or an `NSView` — the mistake
    /// a shell is actually likely to make — which produces an `Err` instead of
    /// an Objective-C raise later at `nextDrawable`.
    pub(crate) unsafe fn create_surface_impl(
        &self,
        target: &SurfaceTarget,
    ) -> Result<SurfaceHandle, HalError> {
        let kind = match *target {
            SurfaceTarget::AppKit { layer } => {
                // SAFETY: the trait's contract puts this on the caller — the
                // pointer names a live `CAMetalLayer *` that outlives both this
                // surface and every swapchain on it. Nothing in this crate can
                // check that; only the shell that made the layer knows.
                // `Retained::retain` takes the reference the surface then owns,
                // which is what makes "outlives the surface" satisfiable at all
                // rather than a rule someone has to follow.
                let retained = unsafe { Retained::retain(layer.as_ptr().cast::<CAMetalLayer>()) };
                let Some(layer) = retained else {
                    return Err(HalError::InvalidDescriptor(
                        "SurfaceTarget::AppKit named a layer that could not be retained"
                            .to_string(),
                    ));
                };
                if !layer.isKindOfClass(CAMetalLayer::class()) {
                    return Err(HalError::InvalidDescriptor(
                        "SurfaceTarget::AppKit must name a CAMetalLayer; this object is some \
                         other class, and Metal would raise on it at the first nextDrawable"
                            .to_string(),
                    ));
                }
                SurfaceKind::Layer(layer)
            }
            SurfaceTarget::Offscreen => SurfaceKind::Offscreen,
            SurfaceTarget::Wayland { .. } | SurfaceTarget::Xcb { .. } => {
                return Err(Self::not_yet(
                    "Wayland and X11 surfaces — Metal is macOS's window system, not Linux's",
                ));
            }
            SurfaceTarget::Win32 { .. } => {
                return Err(Self::not_yet("Win32 surfaces — that is the DX12 backend's"));
            }
            SurfaceTarget::Web { .. } => {
                return Err(Self::not_yet(
                    "canvas surfaces — a canvas is crcbl-wgpu's target, not Metal's",
                ));
            }
        };

        let row = self.inner.surfaces().insert(SurfaceEntry {
            owner: self.inner.id,
            kind,
        });
        let handle: SurfaceHandle = crate::device::stamp(&*self.inner, row);
        crcbl_core::log::debug!(
            "crcbl-mtl: created a {} surface {handle:?}",
            target.platform_name()
        );
        Ok(handle)
    }

    /// Drops this instance's reference to a surface.
    ///
    /// The handle dies here; the `CAMetalLayer` dies when the last swapchain
    /// holding a clone of it is destroyed. See [`SurfaceEntry`] for why that
    /// needs no deletion queue.
    pub(crate) fn destroy_surface_impl(&self, surface: SurfaceHandle) {
        let mut surfaces = self.inner.surfaces();
        crate::device::take_owned(&mut surfaces, surface, &*self.inner);
    }

    /// What `surface` supports on `adapter`.
    ///
    /// # Errors
    ///
    /// [`HalError::NoSuchAdapter`] for an unknown adapter, or
    /// [`HalError::InvalidHandle`] / [`HalError::ForeignObject`] for a surface
    /// this instance did not issue.
    pub(crate) fn surface_caps_impl(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        // The adapter first, because an unknown one is a distinct contract from
        // a stale surface and must not be reported as the other.
        if !self
            .inner
            .adapters
            .iter()
            .any(|record| record.info.id == adapter)
        {
            return Err(HalError::NoSuchAdapter(adapter.0));
        }
        let surfaces = self.inner.surfaces();
        let entry = lookup(&surfaces, "surface", surface, &*self.inner)?;
        // **Every Metal device on a Mac can drive a `CAMetalLayer`.** Presenting
        // is `setDevice:` plus `nextDrawable`; there is no per-device support
        // query in the API and no queue family to find one on. So the
        // [`HalError::Unsupported`] branch the trait requires for a
        // non-presentable adapter is unreachable on this backend — and neither
        // list below is ever empty, which is the failure the trait calls out by
        // name. (`preferredDevice` names the GPU driving the layer's display and
        // is a *performance* answer, not a capability one; reporting a
        // non-preferred adapter as unsupported would refuse a pairing that
        // works.)
        Ok(match &entry.kind {
            SurfaceKind::Layer(layer) => layer_surface_caps(layer),
            SurfaceKind::Offscreen => offscreen_surface_caps(),
        })
    }
}

impl InstanceInner {
    /// The surface a handle names, resolved against *this* instance and cloned
    /// out from under its lock.
    ///
    /// Cloned rather than borrowed for the reason
    /// [`buffer_raw`](crate::device::DeviceInner::buffer_raw) gives: the caller
    /// then takes the *device* lock, and holding two at once is a deadlock
    /// ordering to design before it is a problem to have.
    pub(crate) fn surface_kind(&self, surface: SurfaceHandle) -> Result<SurfaceKindRef, HalError> {
        let surfaces = self.surfaces();
        Ok(match &lookup(&surfaces, "surface", surface, self)?.kind {
            SurfaceKind::Layer(layer) => SurfaceKindRef::Layer(layer.clone()),
            SurfaceKind::Offscreen => SurfaceKindRef::Offscreen,
        })
    }
}

/// A surface resolved out from under the instance lock. See
/// [`InstanceInner::surface_kind`].
pub(crate) enum SurfaceKindRef {
    Layer(Retained<CAMetalLayer>),
    Offscreen,
}

impl MetalDevice {
    /// Configures a swapchain on a surface.
    pub(crate) fn create_swapchain_impl(
        &self,
        desc: &SwapchainDesc<'_>,
    ) -> Result<SwapchainHandle, SurfaceError> {
        let kind = self.inner.instance.surface_kind(desc.surface)?;
        let built = self.build_swapchain(desc, &kind)?;
        let handle = self.state().swapchains.insert(built);
        Ok(self.stamp(handle))
    }

    /// Rebuilds a swapchain in place — resize, present-mode change, format
    /// change — keeping the handle, which is what stops a resize storm churning
    /// everything above the seam.
    pub(crate) fn reconfigure_swapchain_impl(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        // Resolved before anything is built, so a stale handle costs no work
        // and leaves no half-configured layer.
        let surface = {
            let state = self.state();
            lookup(&state.swapchains, "swapchain", swapchain, &*self.inner)?.surface
        };
        if desc.surface != surface {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "reconfigure_swapchain must name the surface the swapchain was created on; \
                 retargeting one is destroy-and-create, not a reconfigure"
                    .to_string(),
            )));
        }
        let kind = self.inner.instance.surface_kind(desc.surface)?;
        let built = self.build_swapchain(desc, &kind)?;

        let mut state = self.state();
        let slot = local_handle::<SwapchainEntry, _>("swapchain", swapchain, &*self.inner)
            .ok()
            .and_then(|local| state.swapchains.get_mut(local))
            .filter(|entry| entry.owner == self.inner.id);
        let Some(slot) = slot else {
            // The lock was released while `built` was under construction, so a
            // concurrent `destroy_swapchain` can land here. Releasing it is not
            // optional: its ring rows are already in the pools, and dropping
            // the entry alone would leave them there with nothing left that
            // could ever name them.
            self.release_swapchain_rows(&mut state, built);
            return Err(SurfaceError::Hal(HalError::invalid_handle(
                "swapchain",
                swapchain,
            )));
        };
        let previous = core::mem::replace(slot, built);
        // Invalidating the old rows is the point: a caller holding an image or
        // a view across a resize gets `InvalidHandle`, not a stale texture at
        // the wrong size. The seam says so on `AcquiredFrame::view`.
        self.release_swapchain_rows(&mut state, previous);
        Ok(())
    }

    pub(crate) fn destroy_swapchain_impl(&self, swapchain: SwapchainHandle) {
        let mut state = self.state();
        let removed = crate::device::remove_owned(&mut state.swapchains, swapchain, &*self.inner);
        if let Some(entry) = removed {
            self.release_swapchain_rows(&mut state, entry);
        }
    }

    /// Acquires the next image to render into.
    pub(crate) fn acquire_next_frame_impl(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        // The layer path calls `nextDrawable`, which blocks until Core
        // Animation has one — so the layer is cloned out and the device lock
        // released before the call. Holding it across that would stall every
        // other thread's resource creation for as long as the compositor takes.
        let (layer, index, format, configured, stale) = {
            let mut state = self.state();
            let entry = lookup_mut(&mut state.swapchains, "swapchain", swapchain, &*self.inner)?;
            let index = entry.next_index;
            let format = entry.format;
            let configured = entry.extent;
            let ring = match &entry.target {
                SwapchainTarget::Offscreen { images, views } => {
                    Some((images[index as usize], views[index as usize]))
                }
                SwapchainTarget::Layer { .. } => None,
            };
            if let Some((image, view)) = ring {
                // The implicit-acquire shape, which is also `crcbl-wgpu`'s: no
                // semaphores, so the caller's `Option::as_slice()` splices
                // nothing. Nothing orders the reuse of a ring image either, and
                // on Metal nothing has to — every texture here is
                // driver-hazard-tracked, which is the premise
                // `pipeline_barrier` already rests on.
                entry.acquired = Some(index);
                return Ok(AcquiredFrame {
                    image,
                    view,
                    extent: configured,
                    index,
                    acquire_semaphore: None,
                    present_semaphore: None,
                    // A ring image is exactly what was configured, and nothing
                    // outside this process can resize it. See
                    // [`SwapchainEntry`] for why the layer path is the only one
                    // that can ever report this.
                    suboptimal: false,
                });
            }
            let SwapchainTarget::Layer {
                layer,
                drawable,
                rows,
                ..
            } = &mut entry.target
            else {
                unreachable!("the ring branch returned above")
            };
            // An acquire with one still outstanding is a caller bug, but the
            // recoverable kind: a frame that failed between acquire and present
            // leaves one behind, and refusing every subsequent frame would turn
            // one bad frame into a dead window. The unpresented drawable goes
            // back to Core Animation's pool when it drops.
            if drawable.take().is_some() {
                crcbl_core::log::warn!(
                    "crcbl-mtl: acquiring with a drawable already outstanding; the previous frame \
                     was never presented"
                );
            }
            (layer.clone(), index, format, configured, rows.take())
        };
        // Before `nextDrawable`, not after: releasing the previous texture is
        // what returns its drawable to the pool this call is about to draw from.
        if let Some(rows) = stale {
            let mut state = self.state();
            self.remove_swapchain_rows(&mut state, rows);
        }

        let Some(drawable) = layer.nextDrawable() else {
            // The one thing [`SurfaceError::Timeout`] is for. `nextDrawable`
            // returns nil when the layer has none to give: no device set, a
            // zero drawable size, or every drawable still in flight past Core
            // Animation's own timeout.
            return Err(SurfaceError::Timeout);
        };
        let texture = drawable.texture();
        // **Obligation 3, read rather than remembered.** Core Animation may have
        // resized the layer since the configure, and the drawable's own texture
        // is the only thing that knows what size actually came back.
        #[allow(clippy::cast_possible_truncation)]
        let extent = (texture.width() as u32, texture.height() as u32);

        let mut state = self.state();
        let rows = self.insert_drawable_rows(&mut state, &texture, format);
        let entry = match lookup_mut(&mut state.swapchains, "swapchain", swapchain, &*self.inner) {
            Ok(entry) => entry,
            Err(error) => {
                // Destroyed or resized while `nextDrawable` was blocking. The
                // rows are undone rather than left behind, and the drawable
                // returns to Core Animation when it drops at the end of this
                // function.
                self.remove_swapchain_rows(&mut state, rows);
                return Err(error.into());
            }
        };
        // **The only place this backend can report suboptimal**, and the reason
        // [`SwapchainEntry`] carries no remembered flag: `presentDrawable:`
        // returns nothing, so unlike `vkQueuePresentKHR` a Metal present cannot
        // discover the mismatch. Comparing the drawable against what was
        // configured is the whole of it.
        let suboptimal = extent != configured;
        if suboptimal {
            crcbl_core::log::debug!(
                "crcbl-mtl: the drawable came back {extent:?} for a swapchain configured at \
                 {configured:?}; reporting the frame suboptimal and the size that arrived"
            );
        }
        let count = entry.image_count;
        match &mut entry.target {
            SwapchainTarget::Layer {
                drawable: slot,
                rows: row_slot,
                ..
            } => {
                *slot = Some(drawable);
                *row_slot = Some(rows);
            }
            SwapchainTarget::Offscreen { .. } => {
                // A reconfigure between the two locks turned this into a ring,
                // so the drawable and its rows belong to nothing. Nothing on
                // the entry has been touched yet, which is why the assignments
                // below sit after this match rather than before it.
                self.remove_swapchain_rows(&mut state, rows);
                return Err(SurfaceError::OutOfDate);
            }
        }
        entry.acquired = Some(index);
        entry.next_index = (index + 1) % count;
        Ok(AcquiredFrame {
            image: rows.0,
            view: rows.1,
            extent,
            index,
            acquire_semaphore: None,
            present_semaphore: None,
            suboptimal,
        })
    }

    /// Blocks until the drawable numbered `present_id` has been shown.
    ///
    /// Two of the seam's three immediate answers are here and the third is in
    /// [`PresentLedger::wait_until_shown`]: an offscreen ring has no drawable
    /// and therefore no ledger, a swapchain whose ledger was never given this
    /// id names a frame that will never arrive, and — the case that does not
    /// apply to this backend — a device without the capability. Every Metal
    /// device has it; see `crcbl_mtl::adapter`.
    ///
    /// **The device lock is released before the wait.** Blocking under it would
    /// stall every other thread's resource creation for as long as the
    /// compositor takes, which is the same reason `acquire_next_frame_impl`
    /// drops it before `nextDrawable`. The ledger is an `Arc` precisely so it
    /// can outlive that guard, and it is also the only thing the presented
    /// handler touches — so a handler firing mid-wait needs no lock this
    /// function holds.
    ///
    /// The handle is still resolved first: a caller waiting on a swapchain it
    /// already destroyed has a bug whether or not anyone was going to block.
    pub(crate) fn wait_until_presented_impl(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        timeout: Duration,
    ) -> Result<(), SurfaceError> {
        let presents = {
            let state = self.state();
            let entry = lookup(&state.swapchains, "swapchain", swapchain, &*self.inner)?;
            match &entry.target {
                SwapchainTarget::Layer { presents, .. } => Arc::clone(presents),
                SwapchainTarget::Offscreen { .. } => return Ok(()),
            }
        };
        let outcome = presents.wait_until_shown(present_id, timeout)?;
        if outcome == PresentWait::Shown {
            // Said once, and it is the only thing that tells a closed loop from
            // a device that advertises the capability and then answers every
            // wait immediately: `displaySyncEnabled` already paces a Fifo
            // swapchain to the display, so the two are indistinguishable in a
            // frame time. `crcbl-vk` logs the same fact for the same reason.
            self.inner.first_present_wait.call_once(|| {
                crcbl_core::log::info!(
                    "crcbl-mtl: present {present_id} reached its presented handler; \
                     the loop is closed"
                );
            });
        }
        Ok(())
    }

    /// Resolves the handle and answers [`DisplayTiming::Unknown`].
    ///
    /// The lookup is the whole of the work, and it is not a formality: it is
    /// the seam's obligation 3, so a swapchain from another device is a
    /// `ForeignObject` here exactly as it is on a backend that has a real
    /// answer to give. See
    /// [`Device::display_timing`](crcbl_hal::Device::display_timing) on
    /// [`MetalDevice`](crate::MetalDevice) for why the answer is `Unknown`.
    pub(crate) fn display_timing_impl(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<DisplayTiming, SurfaceError> {
        let state = self.state();
        lookup(&state.swapchains, "swapchain", swapchain, &*self.inner)?;
        Ok(DisplayTiming::Unknown)
    }

    /// Presents the acquired image.
    pub(crate) fn present_impl(
        &self,
        queue: QueueHandle,
        present: &PresentInfo<'_>,
    ) -> Result<(), SurfaceError> {
        self.inner.check_queue(queue)?;
        let mut state = self.state();
        // Waits first: a handle that does not resolve must fail before the
        // drawable is taken, or the frame would be lost with nothing presented.
        let mut waits = Vec::with_capacity(present.waits.len());
        for handle in present.waits {
            waits.push(self.inner.semaphore_wait(&state, *handle)?);
        }

        let entry = lookup_mut(
            &mut state.swapchains,
            "swapchain",
            present.swapchain,
            &*self.inner,
        )?;
        let Some(index) = entry.acquired.take() else {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "present without a matching acquire_next_frame".to_string(),
            )));
        };
        let count = entry.image_count;
        let taken = match &mut entry.target {
            SwapchainTarget::Offscreen { .. } => None,
            SwapchainTarget::Layer {
                drawable,
                rows,
                presents,
                ..
            } => Some((drawable.take(), rows.take(), Arc::clone(presents))),
        };
        let Some((drawable, rows, presents)) = taken else {
            // "Presenting" a ring image is advancing the ring. The image stays
            // valid and is reused when the cursor comes back round, exactly as
            // a real swapchain image is.
            entry.next_index = (index + 1) % count;
            return Ok(());
        };
        if let Some(rows) = rows {
            // The seam says the image is valid until this call, and the module
            // docs say why holding the texture any longer starves Core
            // Animation's drawable pool. Before the check below, not after: a
            // swapchain that somehow holds rows without a drawable would
            // otherwise leak them on the way out.
            self.remove_swapchain_rows(&mut state, rows);
        }
        let Some(drawable) = drawable else {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                "present without a matching acquire_next_frame".to_string(),
            )));
        };
        drop(state);

        let command_buffer = crate::fault::command_buffer(&self.inner.queue, "crcbl present")
            .ok_or_else(|| {
                HalError::DeviceLost(
                    "MTLCommandQueue::commandBufferWithDescriptor: returned nil".to_string(),
                )
            })?;
        for (event, value) in &waits {
            command_buffer.encodeWaitForEvent_value(event, *value);
        }
        // The present's number, taken **before** the drawable can be shown, so
        // the ledger's `committed` can never trail a `shown` the handler has
        // already reported. An id the ledger refuses — zero, or one that does
        // not follow the last — presents unnumbered and is exactly the "no
        // record of this id" case the wait answers immediately.
        let numbered = present
            .present_id
            .filter(|present_id| presents.record_present(*present_id));
        if let (Some(present_id), None) = (present.present_id, numbered) {
            crcbl_core::log::warn!(
                "crcbl-mtl: present id {present_id} does not follow this swapchain's last; \
                 presenting unnumbered"
            );
        }
        if let Some(present_id) = numbered {
            attach_presented_handler(&drawable, &presents, present_id);
        }
        // Not `MTLDrawable::present`: see the module docs.
        command_buffer.presentDrawable(ProtocolObject::from_ref(&*drawable));
        command_buffer.commit();
        // Tracked, because a present that faults is still a submission that
        // failed and nothing else would ever look at this one's `status` —
        // `DeviceState::in_flight` is the whole of that path. The lock is
        // reacquired rather than held across the Metal calls above, which is why
        // it was dropped in the first place.
        self.inner.state().track(command_buffer.clone());
        // Deliberately **not** recorded as `DeviceState::last_submission`. That
        // field is what a `ReadbackDesc::after` of `None` waits on, and this
        // command buffer does not complete until the compositor has taken the
        // drawable — so recording it would make every screenshot wait a whole
        // vsync for work it does not care about. The seam's wording is
        // "everything *submitted* to this device", and a present is not a
        // `Device::submit`.
        Ok(())
    }

    /// Builds a swapchain without touching the table it will be stored in.
    fn build_swapchain(
        &self,
        desc: &SwapchainDesc<'_>,
        kind: &SurfaceKindRef,
    ) -> Result<SwapchainEntry, SurfaceError> {
        let caps = match kind {
            SurfaceKindRef::Layer(layer) => layer_surface_caps(layer),
            SurfaceKindRef::Offscreen => offscreen_surface_caps(),
        };
        if !caps.formats.contains(&desc.format) {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
                "SwapchainDesc::format is {:?}, which this surface does not offer; \
                 SurfaceCaps::formats has {:?}",
                desc.format, caps.formats
            ))));
        }
        if !caps.composite_alpha.contains(&desc.composite_alpha) {
            return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
                "SwapchainDesc::composite_alpha is {:?}, and this surface offers only {:?}",
                desc.composite_alpha, caps.composite_alpha
            ))));
        }
        let extent = resolve_extent(desc.extent, self.inner.caps.limits.max_image_2d)?;
        if extent.clamped {
            crcbl_core::log::warn!(
                "crcbl-mtl: swapchain extent {:?} clamped to {:?} by this device's texture limit",
                desc.extent,
                extent.configured
            );
        }
        let present_mode = resolve_present_mode(desc.present_mode, &caps);
        let image_count = desc
            .image_count
            .clamp(caps.min_image_count, caps.max_image_count);

        let target = match kind {
            SurfaceKindRef::Layer(layer) => {
                self.configure_layer(layer, desc, extent.configured, image_count, present_mode);
                SwapchainTarget::Layer {
                    layer: layer.clone(),
                    drawable: None,
                    rows: None,
                    // Fresh, which is the whole of how the present numbering
                    // restarts across a reconfigure: that call replaces the
                    // entry with what this function returns, so there is no
                    // reset to forget to write. `crcbl_mtl::present` says so
                    // from the other side.
                    presents: Arc::default(),
                }
            }
            SurfaceKindRef::Offscreen => {
                self.build_offscreen_ring(desc, extent.configured, image_count)?
            }
        };
        crcbl_core::log::info!(
            "crcbl-mtl: swapchain {}x{} {:?}, {image_count} image(s), {present_mode:?}",
            extent.configured.0,
            extent.configured.1,
            desc.format,
        );
        Ok(SwapchainEntry {
            owner: self.inner.id,
            surface: desc.surface,
            target,
            format: desc.format,
            extent: extent.configured,
            acquired: None,
            next_index: 0,
            image_count,
        })
    }

    /// Puts a `CAMetalLayer` into the state this swapchain describes.
    ///
    /// Every property here is one the seam's descriptor names, and each is set
    /// on every configure rather than only when it changed: a reconfigure is the
    /// seam's single call for resize, format change and present-mode change at
    /// once, and a layer that kept an old value because nothing noticed it move
    /// is the bug that shape exists to prevent.
    fn configure_layer(
        &self,
        layer: &CAMetalLayer,
        desc: &SwapchainDesc<'_>,
        extent: (u32, u32),
        image_count: u32,
        present_mode: PresentMode,
    ) {
        layer.setDevice(Some(&self.inner.raw));
        // The one format decision, from the one table. See the module docs.
        layer.setPixelFormat(conv::pixel_format(desc.format));
        // **Not framebuffer-only.** The default is `true`, which makes a
        // drawable's texture unusable as a copy source — and `crcbl screenshot`
        // copies a presented frame out through exactly that path, as does the
        // seam's `copy_image_to_buffer` on an `AcquiredFrame::image`. The cost
        // is that Core Animation cannot pick the most compressed layout for the
        // drawable; the alternative is a swapchain image the engine cannot read.
        layer.setFramebufferOnly(false);
        // `CompositeAlpha::Opaque` is the only mode `layer_surface_caps` offers
        // and `build_swapchain` refused anything else, so this is unconditional
        // rather than a match with one arm.
        layer.setOpaque(true);
        layer.setMaximumDrawableCount(to_ns(u64::from(image_count)));
        layer.setDisplaySyncEnabled(matches!(present_mode, PresentMode::Fifo));
        layer.setDrawableSize(CGSize::new(f64::from(extent.0), f64::from(extent.1)));
        if let Some(label) = desc.label {
            layer.setName(Some(&NSString::from_str(label)));
        }
    }

    /// The offscreen ring: a swapchain-shaped rotation of plain textures.
    ///
    /// Built through [`create_image`](crcbl_hal::Device::create_image) and
    /// [`create_image_view`](crcbl_hal::Device::create_image_view) rather than
    /// straight onto `MTLTextureDescriptor`, so the ring gets the same limit and
    /// format validation every other image does instead of a second copy of it
    /// that can drift.
    fn build_offscreen_ring(
        &self,
        desc: &SwapchainDesc<'_>,
        extent: (u32, u32),
        image_count: u32,
    ) -> Result<SwapchainTarget, SurfaceError> {
        let mut images: Vec<ImageHandle> = Vec::with_capacity(image_count as usize);
        let mut views: Vec<ImageViewHandle> = Vec::with_capacity(image_count as usize);
        for index in 0..image_count {
            let label = desc.label.map(|label| format!("{label} [{index}]"));
            let made = self
                .create_image(&ImageDesc {
                    label: label.as_deref(),
                    // `TRANSFER_SRC` is what makes this a screenshot target and
                    // `SAMPLED` what makes it a tonemap input; `PRESENT` says
                    // what it is. The same set `crcbl-vk`'s ring asks for.
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::TRANSFER_SRC
                        | ImageUsage::TRANSFER_DST
                        | ImageUsage::SAMPLED
                        | ImageUsage::PRESENT,
                    format: desc.format,
                    extent: Extent3d::d2(extent.0, extent.1),
                    mip_levels: 1,
                    samples: 1,
                    image_type: ImageType::D2,
                })
                .and_then(|image| {
                    images.push(image);
                    self.create_image_view(&ImageViewDesc {
                        label: label.as_deref(),
                        image,
                        format: desc.format,
                        view_type: ImageViewType::D2,
                        range: ImageSubresourceRange::all(desc.format),
                    })
                    .map(|view| (image, view))
                });
            match made {
                Ok((image, view)) => {
                    views.push(view);
                    self.mark_swapchain_owned(image, view);
                }
                Err(error) => {
                    // Everything made so far is undone: the rows are the
                    // swapchain's and nothing else will ever name them.
                    self.unwind_ring(images, views);
                    return Err(SurfaceError::Hal(error));
                }
            }
        }
        Ok(SwapchainTarget::Offscreen { images, views })
    }

    /// Frees the rows of a ring that failed part-way through being built.
    ///
    /// The two vectors are not necessarily the same length — the view is what
    /// fails after its image was already made — so they are drained
    /// independently rather than zipped.
    fn unwind_ring(&self, images: Vec<ImageHandle>, views: Vec<ImageViewHandle>) {
        let mut state = self.state();
        for view in views {
            self.remove_swapchain_view(&mut state, view);
        }
        for image in images {
            self.remove_swapchain_image(&mut state, image);
        }
    }

    /// Frees every pool row a swapchain owned.
    ///
    /// Two things about the signature are load-bearing rather than stylistic.
    /// Taking the entry **by value** means the caller has already removed it
    /// from the table, so no path frees the rows while something can still hand
    /// them out. Taking the guard **by reference** rather than re-locking is
    /// what stops this deadlocking: every caller already holds the device lock,
    /// [`Mutex`](std::sync::Mutex) is not reentrant, and a version that called
    /// `self.state()` itself would hang on a resize with no diagnostic at all.
    fn release_swapchain_rows(&self, state: &mut DeviceState, entry: SwapchainEntry) {
        match entry.target {
            SwapchainTarget::Layer { rows, .. } => {
                if let Some(rows) = rows {
                    self.remove_swapchain_rows(state, rows);
                }
            }
            SwapchainTarget::Offscreen { images, views } => {
                for view in views {
                    self.remove_swapchain_view(state, view);
                }
                for image in images {
                    self.remove_swapchain_image(state, image);
                }
            }
        }
    }
}

/// Asks `drawable` to report into `presents` under `present_id` once it has
/// been shown.
///
/// **This is the whole of present feedback on Metal**, and the only part of the
/// capability that is not plain Rust: everything the number is then compared
/// against lives in [`PresentLedger`], where it is testable without a display.
///
/// Apple documents the handler as running once the drawable has been presented
/// *or dropped*, which is the seam's guarantee exactly — "the numbered present
/// is no longer waiting to happen" — rather than a claim that the pixels were
/// seen. `presentedTime` is deliberately not read: it would oblige a timestamp
/// the seam has no method for, and would pull a second `objc2-metal` feature in
/// for nothing.
fn attach_presented_handler(
    drawable: &ProtocolObject<dyn CAMetalDrawable>,
    presents: &Arc<PresentLedger>,
    present_id: u64,
) {
    let presents = Arc::clone(presents);
    let handler = RcBlock::new(move |_drawable: NonNull<ProtocolObject<dyn MTLDrawable>>| {
        presents.record_shown(present_id);
    });
    // SAFETY: three conditions, and the first is the one `objc2` names.
    //
    // 1. **The pointer is a valid block.** It is `RcBlock`'s own, taken from a
    //    live `RcBlock` that outlives this statement; the block's signature is
    //    the `MTLDrawablePresentedHandler` `objc2-metal` declares, so the
    //    argument and return encodings match what Metal will call it with.
    //
    // 2. **The block outlives this call.** `addPresentedHandler:` stores the
    //    block to run later, and the Objective-C convention for a stored block
    //    is that the callee copies it — which for a block already on the heap
    //    is a retain. That is convention rather than a documented sentence in
    //    Apple's reference, and it is the one assumption here that is: every
    //    Metal sample passes a *stack* block to this method, which would be a
    //    use-after-free the moment the frame returned if the copy did not
    //    happen. Our block being heap-allocated to begin with makes the retain
    //    the whole of it.
    //
    // 3. **It is sound to run on Core Animation's thread.** The block captures
    //    an `Arc<PresentLedger>` and a `u64` and nothing else. `PresentLedger`
    //    is `Send + Sync` by derivation — a `Mutex` and a `Condvar` over two
    //    integers, with no `unsafe impl` anywhere and no Objective-C object
    //    inside it — which is what makes touching it from a thread this crate
    //    did not create, concurrently with a waiter, defined. The drawable the
    //    handler is passed is ignored rather than retained, so nothing here
    //    extends a drawable's life past the pool's expectations.
    unsafe { drawable.addPresentedHandler(RcBlock::as_ptr(&handler)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    use crcbl_core::Handle;
    use crcbl_hal::{
        Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
        CommandEncoderDesc, Features, ImageAspect, ImageBarrier, ImageSubresourceLayers, Instance,
        LoadOp, MemoryLocation, Offset3d, QueueKind, ReadbackDesc, ReadbackHandle, Rect2d,
        RenderPassDesc, ResourceState, StoreOp, SubmitInfo,
    };
    use objc2_metal::MTLPixelFormat;
    use objc2_quartz_core::CALayer;

    use crate::MetalInstance;
    use crate::instance::tests::{desc as device_desc, open as open_instance};

    /// The size every offscreen test configures at.
    ///
    /// 64 texels wide is `64 × 4 = 256` bytes a row for a four-byte format,
    /// which is a comfortable stride for the buffer↔image copy; four rows is
    /// enough that a row-stride mistake shows up rather than being invisible in
    /// a single row.
    const EXTENT: (u32, u32) = (64, 4);

    /// Bytes one full copy of [`EXTENT`] occupies at four bytes a texel.
    const EXTENT_BYTES: usize = 64 * 4 * 4;

    /// The clear colour, and why the expected bytes are what they are.
    ///
    /// [`Format::Rgba8Unorm`] applies no transfer function on either read or
    /// write, so a component `c` is stored as `round(c × 255)`. Every channel
    /// differs and none is zero, so a channel swizzle and a buffer nothing ever
    /// wrote are both distinguishable from a pass. The same derivation
    /// `crcbl_mtl::device`'s clear tests use.
    const CLEAR: [f32; 4] = [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0];

    /// What [`CLEAR`] must read back as.
    const CLEAR_TEXEL: [u8; 4] = [0x11, 0x22, 0x33, 0xFF];

    /// What the staging buffer is filled with first, so "the copy never ran"
    /// and "the clear never ran" are different failures.
    const POISON: u8 = 0xA5;

    fn open_device() -> (MetalInstance, crate::MetalDevice) {
        let instance = open_instance();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "a Mac has at least one adapter");
        let device = instance
            .open_device(&device_desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        (instance, device)
    }

    /// An offscreen surface on `instance`.
    fn offscreen_surface(instance: &MetalInstance) -> SurfaceHandle {
        // SAFETY: `SurfaceTarget::Offscreen` names no platform object at all,
        // so the trait's obligations about live handles are vacuous for it.
        unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("an offscreen surface needs no window system")
    }

    /// A swapchain descriptor over `surface`, in a format its caps offer.
    fn swapchain_desc(
        surface: SurfaceHandle,
        format: Format,
        images: u32,
    ) -> SwapchainDesc<'static> {
        SwapchainDesc {
            label: Some("crcbl-mtl test swapchain"),
            surface,
            format,
            extent: EXTENT,
            image_count: images,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        }
    }

    /// A detached `CAMetalLayer` and the target naming it.
    ///
    /// Core Animation vends layers without a window server, an `NSView` or a
    /// run loop, so everything short of `nextDrawable` — which is the one thing
    /// that needs a real display — is exercisable in CI on a layer built here.
    fn detached_layer() -> Retained<CAMetalLayer> {
        CAMetalLayer::new()
    }

    /// The `SurfaceTarget` naming `layer`.
    fn appkit_target(layer: &CAMetalLayer) -> SurfaceTarget {
        SurfaceTarget::AppKit {
            layer: core::ptr::NonNull::from(&**layer).cast(),
        }
    }

    /// Blocks until `readback` is ready, or fails the test.
    fn drain(device: &crate::MetalDevice, readback: ReadbackHandle, size: usize) -> Vec<u8> {
        use crcbl_hal::Device as _;
        let mut out = vec![0u8; size];
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = device
                .poll_readback(readback, &mut out)
                .expect("the readback resolves");
            if state.is_ready() {
                return out;
            }
            assert!(
                Instant::now() < deadline,
                "the readback never became ready; the submission it waits on has not completed"
            );
            std::thread::yield_now();
        }
    }

    // --- the extent rule, with no hardware in the loop ----------------------

    /// **Obligation 4.** Zero is refused, and the message names the rule so the
    /// fix is obvious from the log rather than from this file.
    ///
    /// Red when `resolve_extent` clamps a zero up to one, or returns `Ok` for
    /// any of the three shapes below.
    #[test]
    fn a_zero_metal_extent_is_a_caller_error_not_a_fallback() {
        for requested in [(0, 720), (1280, 0), (0, 0)] {
            let error = resolve_extent(requested, 16384).expect_err("zero is refused");
            let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
                panic!("{requested:?} gave the wrong variant: {error:?}");
            };
            assert!(message.contains("do not create one yet"), "{message}");
        }
    }

    /// **Obligation 1.** The shell's size is used unchanged, and no clamp is
    /// reported for a size that needed none.
    ///
    /// Red when the ceiling is applied to a size below it, or when `clamped` is
    /// set unconditionally.
    #[test]
    fn an_extent_inside_the_texture_limit_is_the_shells_untouched() {
        let decision = resolve_extent((1280, 720), 16384).expect("a real size is not a bug");
        assert_eq!(decision.configured, (1280, 720));
        assert!(!decision.clamped, "nothing forced a change");
    }

    /// **Obligation 2.** A request past the device's texture limit is clamped
    /// on the axis that overran, and the clamp is reported.
    ///
    /// Red when the oversized axis passes through — which is the value Metal
    /// raises on rather than returning nil — or when the axis that fitted is
    /// clamped too, or when `clamped` stays false.
    #[test]
    fn an_extent_past_the_texture_limit_clamps_on_the_axis_that_overran() {
        let decision = resolve_extent((40_000, 720), 16384).expect("not zero");
        assert_eq!(decision.configured, (16384, 720));
        assert!(decision.clamped, "the disagreement must be visible");
    }

    // --- what the two surfaces offer ---------------------------------------

    /// **The sRGB decision, in the caps.** A `CAMetalLayer` is offered an sRGB
    /// format first so [`SurfaceCaps::preferred_format`] picks it, and is never
    /// offered the RGBA8 pair Core Animation rejects.
    ///
    /// Red when [`LAYER_FORMATS`] loses its sRGB entry — `preferred_format`
    /// then returns `Bgra8Unorm`, the linear format whose missing encode is the
    /// bug `crcbl-wgpu` shipped — or when `Rgba8UnormSrgb` is added to it,
    /// which would hand a caller a format `setPixelFormat:` raises on.
    #[test]
    fn the_layer_offers_srgb_first_and_never_the_rgba8_pair_core_animation_rejects() {
        let caps = SurfaceCaps {
            formats: LAYER_FORMATS.to_vec(),
            ..offscreen_surface_caps()
        };
        assert_eq!(caps.preferred_format(), Some(Format::Bgra8UnormSrgb));
        for format in LAYER_FORMATS {
            assert!(
                !matches!(format, Format::Rgba8Unorm | Format::Rgba8UnormSrgb),
                "CAMetalLayer::pixelFormat does not accept {format:?}"
            );
        }
        // And the format that list names really is Metal's sRGB one, from the
        // single table — not a second mapping written here that could drift.
        assert_eq!(
            conv::pixel_format(Format::Bgra8UnormSrgb),
            MTLPixelFormat::BGRA8Unorm_sRGB
        );
    }

    /// The failure `Instance::surface_caps` calls out by name: an empty
    /// `formats` collides with `preferred_format`'s documented meaning, and an
    /// empty `present_modes` breaks the promise that `Fifo` is always there.
    ///
    /// Red when either list is emptied, when `Fifo` is dropped from one, or
    /// when a min/max image count pair is written the wrong way round.
    #[test]
    fn neither_caps_list_is_ever_empty_and_both_always_offer_fifo() {
        let offscreen = offscreen_surface_caps();
        // The real thing rather than a hand-assembled stand-in: a stand-in
        // would go on passing after `layer_surface_caps` itself emptied a list.
        let layer = layer_surface_caps(&detached_layer());
        for caps in [&offscreen, &layer] {
            assert!(!caps.formats.is_empty(), "{caps:?}");
            assert!(!caps.present_modes.is_empty(), "{caps:?}");
            assert!(!caps.composite_alpha.is_empty(), "{caps:?}");
            assert!(caps.supports_present_mode(PresentMode::Fifo), "{caps:?}");
            assert!(caps.min_image_count >= 1, "{caps:?}");
            assert!(caps.min_image_count <= caps.max_image_count, "{caps:?}");
        }
        // The ring has no opinion about its size, exactly as Wayland has none.
        assert_eq!(offscreen.current_extent, None);
        // Neither has a layer nothing has laid out yet. **`Some((0, 0))` here
        // would be the sentinel-leak failure obligation 4 names**: a zero size
        // is the layer saying "not yet", and passing it through would have a
        // caller configure a swapchain at nothing.
        assert_eq!(
            layer.current_extent, None,
            "a freshly made layer has 0x0 bounds, which is an absence and not a size"
        );
    }

    /// The seam documents a fallback, not a failure.
    ///
    /// Red when an unavailable mode is passed through (the first assertion) or
    /// when an available one is downgraded anyway (the second).
    #[test]
    fn an_unavailable_present_mode_falls_back_to_fifo_rather_than_failing() {
        let caps = offscreen_surface_caps();
        assert!(
            !caps.supports_present_mode(PresentMode::Mailbox),
            "{caps:?}"
        );
        assert_eq!(
            resolve_present_mode(PresentMode::Mailbox, &caps),
            PresentMode::Fifo
        );
        assert_eq!(
            resolve_present_mode(PresentMode::Immediate, &caps),
            PresentMode::Immediate
        );
    }

    // --- the offscreen ring, end to end ------------------------------------

    /// **Present feedback is advertised, and an offscreen ring still answers a
    /// wait at once.**
    ///
    /// The two halves are one claim. `crcbl_mtl::adapter` reports
    /// [`Features::PRESENT_FEEDBACK`] for every device, because
    /// `addPresentedHandler:` is a plain drawable method with no query behind
    /// it — and a device whose swapchain is the ring has no drawable to
    /// observe. So the seam's "nothing to wait for" answer has to come from the
    /// *swapchain*, and this is where that is checked. A ring that blocked
    /// instead would cost `GpuContext::acquire` a whole present timeout on
    /// every frame of every offscreen run, which is `crcbl screenshot` and the
    /// golden-image e2e.
    ///
    /// **This is the half of the capability an automated run can execute**: no
    /// window server and no shader. The `addPresentedHandler:` path itself is
    /// covered by nothing, anywhere — `docs/backlog.md` records it as a gap.
    ///
    /// Red when the flag stops being reported (first assertion). Red when the
    /// ring is given a ledger that a present records into, or when the
    /// `SwapchainTarget::Offscreen` arm of `wait_until_presented_impl` stops
    /// returning early: both leave the wait blocking on a frame no handler will
    /// ever report, so it sits out `PRESENT_WAIT` and comes back
    /// [`SurfaceError::Timeout`].
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn an_offscreen_ring_advertises_present_feedback_and_still_answers_at_once() {
        use crcbl_hal::Device as _;
        /// Long enough that a wait which really blocked is unmistakable in the
        /// elapsed time, short enough not to stall the suite when one does.
        const PRESENT_WAIT: Duration = Duration::from_secs(2);

        let (instance, device) = open_device();
        assert!(
            device.caps().features.contains(Features::PRESENT_FEEDBACK),
            "every Metal device can attach a presented handler to a drawable"
        );
        let surface = offscreen_surface(&instance);
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 2))
            .expect("a ring of two images");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        // A *numbered* present, so the id below is one the caller has really
        // spent rather than one nothing ever reached.
        device.acquire_next_frame(swapchain).expect("a ring image");
        device
            .present(
                queue,
                &PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: Some(1),
                },
            )
            .expect("presenting a ring image advances the cursor");

        let started = Instant::now();
        device
            .wait_until_presented(swapchain, 1, PRESENT_WAIT)
            .expect("a ring has no display behind it to wait for");
        device
            .wait_until_presented(swapchain, 99, PRESENT_WAIT)
            .expect("nor for an id nothing ever presented");
        assert!(
            started.elapsed() < PRESENT_WAIT,
            "an offscreen wait blocked for {:?}; it must answer immediately",
            started.elapsed()
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// An offscreen surface configures, acquires, presents and destroys — the
    /// whole cycle, with the implicit-acquire shape the seam specifies.
    ///
    /// Red when the extent reported is not the one configured (obligation 3),
    /// when either semaphore comes back `Some` (this backend has no WSI
    /// semaphore to give), or when the ring cursor does not advance.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn an_offscreen_surface_configures_a_swapchain_and_hands_out_frames() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let caps = instance
            .surface_caps(surface, instance.adapters()[0].id)
            .expect("an offscreen surface reports caps on every adapter");
        let format = caps
            .preferred_format()
            .expect("the list is never empty, which is the point of the caps contract");

        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, format, 2))
            .expect("a ring of two images at 64x4");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let first = device.acquire_next_frame(swapchain).expect("a ring image");
        assert_eq!(first.extent, EXTENT, "obligation 3: the configured size");
        assert_eq!(first.index, 0);
        assert!(first.acquire_semaphore.is_none(), "{first:?}");
        assert!(first.present_semaphore.is_none(), "{first:?}");
        assert!(!first.suboptimal, "nothing has changed under it");
        device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: None,
                },
            )
            .expect("presenting a ring image advances the ring");

        let second = device.acquire_next_frame(swapchain).expect("a ring image");
        assert_eq!(second.index, 1, "the ring cursor must advance");
        assert_ne!(
            second.image, first.image,
            "two ring positions are two images"
        );
        device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: None,
                },
            )
            .expect("presented");

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// The ring comes back round to the same handles, which is what makes it a
    /// ring rather than an allocator.
    ///
    /// Red when the cursor never wraps (the index assertion), and red when the
    /// handles are reissued per trip rather than owned for the swapchain's life
    /// (the handle assertion) — which is the behaviour the *layer* path has and
    /// this one deliberately does not.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn an_offscreen_ring_comes_back_round_to_the_same_handles() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let count = 3;
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, count))
            .expect("a ring of three");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let mut seen = Vec::new();
        for _ in 0..=count {
            let frame = device.acquire_next_frame(swapchain).expect("a ring image");
            seen.push((frame.index, frame.image, frame.view));
            device
                .present(
                    queue,
                    &crcbl_hal::PresentInfo {
                        swapchain,
                        waits: &[],
                        present_id: None,
                    },
                )
                .expect("presented");
        }
        let indices: Vec<u32> = seen.iter().map(|entry| entry.0).collect();
        assert_eq!(indices, vec![0, 1, 2, 0], "the ring must wrap, not grow");
        assert_eq!(
            (seen[0].1, seen[0].2),
            (seen[3].1, seen[3].2),
            "position 0 must be the same image and view on the second trip"
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// **The frame that proves the whole path.** A clear into an acquired ring
    /// image, copied out and read back byte for byte.
    ///
    /// This is the swapchain equivalent of MTL3's clear test, and it needs no
    /// shader — so unlike the draw tests it runs wherever `crcbl-mtl`'s suite
    /// runs, including the CI runner whose paravirtual GPU cannot execute a
    /// shader program.
    ///
    /// Red when the acquired view does not name the acquired image (the bytes
    /// stay [`POISON`]), when the ring's images are made without
    /// `COLOR_ATTACHMENT` or `TRANSFER_SRC` (the pass or the copy fails), when
    /// the clear colour is swizzled (the texel differs channelwise), and when
    /// the readback is observed before the submission completed (`POISON`
    /// again).
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn an_offscreen_frame_clears_and_reads_back_through_the_seams_own_path() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 2))
            .expect("a ring at 64x4");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("crcbl-mtl swapchain readback"),
                size: EXTENT_BYTES as u64,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a host-readable staging buffer");
        device
            .write_buffer(staging, 0, &vec![POISON; EXTENT_BYTES])
            .expect("a HostReadback buffer is Shared and so is writable");

        let frame = device.acquire_next_frame(swapchain).expect("a ring image");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("crcbl-mtl swapchain clear"),
            queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("swapchain clear"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(frame.extent.0, frame.extent.1),
        });
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                frame.image,
                ImageSubresourceRange::all(Format::Rgba8Unorm),
                ResourceState::ColorAttachment,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: staging,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: frame.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(frame.extent.0, frame.extent.1),
        });
        let commands = encoder.finish().expect("the recording is complete");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("the queue accepts it");
        device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: frame.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("presented");

        let request = device
            .request_readback(&ReadbackDesc {
                label: Some("the swapchain frame"),
                buffer: staging,
                offset: 0,
                size: EXTENT_BYTES as u64,
                after: None,
            })
            .expect("a HostReadback buffer, in range");
        let bytes = drain(&device, request, EXTENT_BYTES);
        let expected: Vec<u8> = CLEAR_TEXEL
            .iter()
            .copied()
            .cycle()
            .take(EXTENT_BYTES)
            .collect();
        assert_eq!(bytes, expected, "the acquired image is not the cleared one");

        device.destroy_readback(request);
        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// The seam says a caller must not destroy a swapchain image, and this is
    /// what makes ignoring that a no-op rather than a ring with a hole in it.
    ///
    /// Red when the `swapchain_owned` guard is removed from `destroy_image` or
    /// `destroy_image_view`: the row goes, and the next acquire of the same
    /// position hands back a handle that no longer resolves.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_swapchain_owned_image_survives_a_caller_destroying_it() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 1))
            .expect("a ring of one");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let frame = device.acquire_next_frame(swapchain).expect("a ring image");
        device.destroy_image(frame.image);
        device.destroy_image_view(frame.view);
        device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: None,
                },
            )
            .expect("presented");

        let again = device.acquire_next_frame(swapchain).expect("a ring image");
        assert_eq!(
            (again.image, again.view),
            (frame.image, frame.view),
            "one image means one position, so this is the same row"
        );
        // Resolving it is the assertion: a destroyed row fails lookup here.
        device
            .inner
            .image_raw(again.image)
            .expect("the swapchain still owns its image");
        device
            .inner
            .view_raw(again.view)
            .expect("the swapchain still owns its view");

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// A present with no acquire behind it is a caller bug and is named as one.
    ///
    /// Red when `present` returns `Ok` for a swapchain with nothing
    /// outstanding, which is the shape that silently drops a frame.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn presenting_a_metal_swapchain_without_acquiring_is_refused() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 2))
            .expect("a ring");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let error = device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: None,
                },
            )
            .expect_err("nothing was acquired");
        assert!(
            matches!(
                error,
                SurfaceError::Hal(HalError::InvalidDescriptor(ref message))
                    if message.contains("acquire_next_frame")
            ),
            "{error:?}"
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// A reconfigure resizes in place: the swapchain handle survives, the frame
    /// comes back at the new size, and the old rows stop resolving.
    ///
    /// Red when `reconfigure_swapchain` leaves the old ring in place (the
    /// extent assertion), and red when it leaves the old rows in the pools (the
    /// stale-handle assertion) — which is the leak the seam's "do not hold a
    /// view across a reconfigure" rule exists to make detectable.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_reconfigure_resizes_in_place_and_retires_the_old_rows() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let mut desc = swapchain_desc(surface, Format::Rgba8Unorm, 2);
        let swapchain = device.create_swapchain(&desc).expect("a ring");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        let before = device.acquire_next_frame(swapchain).expect("a ring image");
        assert_eq!(before.extent, EXTENT);
        device
            .present(
                queue,
                &crcbl_hal::PresentInfo {
                    swapchain,
                    waits: &[],
                    present_id: None,
                },
            )
            .expect("presented");

        desc.extent = (32, 8);
        device
            .reconfigure_swapchain(swapchain, &desc)
            .expect("a resize keeps the handle");

        let after = device.acquire_next_frame(swapchain).expect("a ring image");
        assert_eq!(after.extent, (32, 8), "obligation 3 after a resize");
        assert_eq!(after.index, 0, "a reconfigured ring starts over");
        assert!(
            device.inner.view_raw(before.view).is_err(),
            "a view held across a reconfigure must stop resolving"
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// A reconfigure naming a different surface is refused rather than
    /// retargeting the swapchain behind the caller's back.
    ///
    /// Red when `reconfigure_swapchain_impl` stops comparing the surface: the
    /// call then succeeds and the swapchain quietly belongs to a window nobody
    /// asked it to.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_reconfigure_naming_another_surface_is_refused() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let first = offscreen_surface(&instance);
        let second = offscreen_surface(&instance);
        let swapchain = device
            .create_swapchain(&swapchain_desc(first, Format::Rgba8Unorm, 2))
            .expect("a ring");

        let error = device
            .reconfigure_swapchain(swapchain, &swapchain_desc(second, Format::Rgba8Unorm, 2))
            .expect_err("that is destroy-and-create, not a reconfigure");
        assert!(
            matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(_))),
            "{error:?}"
        );

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(first);
        instance.destroy_surface(second);
    }

    /// A format the surface does not offer is refused by name, with the list it
    /// does offer in the message.
    ///
    /// Red when `build_swapchain` stops checking: `MTLTextureDescriptor` would
    /// then accept a depth format as a colour ring and the failure would arrive
    /// at the first render pass instead.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_format_the_surface_does_not_offer_is_refused_by_name() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let caps = instance
            .surface_caps(surface, instance.adapters()[0].id)
            .expect("caps");
        assert!(
            !caps.formats.contains(&Format::D32Float),
            "a depth format is not a presentation format: {caps:?}"
        );

        let error = device
            .create_swapchain(&swapchain_desc(surface, Format::D32Float, 2))
            .expect_err("D32Float is not in the offered list");
        let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
            panic!("wrong variant");
        };
        assert!(message.contains("D32Float"), "{message}");

        instance.destroy_surface(surface);
    }

    /// A zero extent is refused all the way through the seam, not only in the
    /// helper.
    ///
    /// Red when `build_swapchain` stops calling `resolve_extent`, or clamps a
    /// zero up to one.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_zero_extent_metal_swapchain_is_refused_end_to_end() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let surface = offscreen_surface(&instance);
        let mut desc = swapchain_desc(surface, Format::Rgba8Unorm, 2);
        desc.extent = (0, 0);

        let error = device
            .create_swapchain(&desc)
            .expect_err("a minimized window means 'not yet'");
        assert!(
            matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(_))),
            "{error:?}"
        );

        instance.destroy_surface(surface);
    }

    // --- obligation 3, both halves -----------------------------------------

    /// A surface from another *instance* is foreign, and the check survives two
    /// instances whose pools issue identical bits.
    ///
    /// The second instance is given its own surface first, so the slot the
    /// first instance's handle names is **occupied** in the second's pool.
    /// Without the instance tag in the handle it would resolve there and the
    /// call would succeed against the wrong surface — which is exactly the bug
    /// `crcbl_mtl::device`'s handle-tagging section describes.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_surface_from_another_instance_is_foreign() {
        let first = open_instance();
        let second = open_instance();
        let theirs = offscreen_surface(&first);
        let _ours = offscreen_surface(&second);

        let error = second
            .surface_caps(theirs, second.adapters()[0].id)
            .expect_err("that surface belongs to the other instance");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "surface"),
            "{error:?}"
        );
    }

    /// A swapchain from another *device* is foreign on every entry point that
    /// takes one.
    ///
    /// Red when the device tag stops riding in the swapchain handle: the second
    /// device's pool resolves the first's handle to its own swapchain and
    /// acquires from the wrong ring.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_swapchain_from_another_device_is_foreign() {
        use crcbl_hal::Device as _;
        let (instance, first) = open_device();
        let second = instance
            .open_device(&device_desc(instance.adapters()[0].id))
            .expect("a second device on the same adapter");
        let surface = offscreen_surface(&instance);

        let theirs = first
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 2))
            .expect("a ring");
        let _ours = second
            .create_swapchain(&swapchain_desc(surface, Format::Rgba8Unorm, 2))
            .expect("a ring on the second device too");

        let error = second
            .acquire_next_frame(theirs)
            .expect_err("that swapchain belongs to the other device");
        assert!(
            matches!(
                error,
                SurfaceError::Hal(HalError::ForeignObject { kind, .. }) if kind == "swapchain"
            ),
            "{error:?}"
        );
    }

    /// A destroyed surface handle stops resolving immediately, and an unknown
    /// adapter is answered before the surface is even looked at.
    ///
    /// Red when `destroy_surface` leaves the row behind (the first assertion),
    /// and red when `surface_caps` checks the surface first and reports a stale
    /// handle for what is actually an adapter the caller invented (the second).
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_stale_surface_and_an_unknown_adapter_are_told_apart() {
        let instance = open_instance();
        let surface = offscreen_surface(&instance);
        let past_the_end = crcbl_hal::AdapterId(instance.adapters().len() as u32);

        let error = instance
            .surface_caps(surface, past_the_end)
            .expect_err("there is no adapter one past the last");
        assert!(
            matches!(error, HalError::NoSuchAdapter(id) if id == past_the_end.0),
            "{error:?}"
        );

        instance.destroy_surface(surface);
        let error = instance
            .surface_caps(surface, instance.adapters()[0].id)
            .expect_err("that surface is gone");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );
    }

    /// A device asked to be compatible with its own instance's surface opens;
    /// one asked about a handle nobody issued does not.
    ///
    /// Red when the surface slice leaves `open_device`'s old blanket refusal in
    /// place — every windowed device would then fail to open, which is the
    /// whole point of this slice.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_device_opens_against_its_own_instances_surface() {
        let instance = open_instance();
        let surface = offscreen_surface(&instance);
        let mut desc = device_desc(instance.adapters()[0].id);
        desc.compatible_surface = Some(surface);
        instance
            .open_device(&desc)
            .expect("every Metal device can present to every surface of its instance");

        // `Handle::from_bits` is the only way to build one nobody issued, and
        // it carries no owner tag, so it is stale rather than foreign.
        desc.compatible_surface =
            Some(Handle::from_bits(1 << 32).expect("generation 1 is non-zero"));
        let error = instance
            .open_device(&desc)
            .expect_err("no instance issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );

        instance.destroy_surface(surface);
    }

    // --- the CAMetalLayer path, short of a drawable -------------------------

    /// A detached `CAMetalLayer` becomes a surface, and its caps are the
    /// layer's rather than the ring's.
    ///
    /// Red when the AppKit arm of `create_surface` goes back to refusing, and
    /// red when the two caps functions are swapped — the ring's image counts
    /// and format list differ from the layer's, and `maximumDrawableCount`
    /// raises outside 2...3.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_detached_layer_becomes_a_surface_whose_caps_are_the_layers() {
        let instance = open_instance();
        let layer = detached_layer();
        // SAFETY: `layer` is a live `CAMetalLayer` this function just created
        // and holds a `Retained` to, so it outlives the surface below, and this
        // thread owns it.
        let surface = unsafe { instance.create_surface(&appkit_target(&layer)) }
            .expect("a CAMetalLayer is a Metal surface");

        let caps = instance
            .surface_caps(surface, instance.adapters()[0].id)
            .expect("every Metal adapter can drive a layer");
        assert_eq!(caps.preferred_format(), Some(Format::Bgra8UnormSrgb));
        assert_eq!(
            (caps.min_image_count, caps.max_image_count),
            (MIN_LAYER_DRAWABLES, MAX_LAYER_DRAWABLES),
            "CAMetalLayer::maximumDrawableCount raises outside this range"
        );
        assert!(caps.supports_present_mode(PresentMode::Fifo));
        assert!(caps.supports_present_mode(PresentMode::Immediate));
        assert!(
            !caps.supports_present_mode(PresentMode::Mailbox),
            "displaySyncEnabled has no third state, and mailbox promises no tearing"
        );

        instance.destroy_surface(surface);
    }

    /// **The sRGB check, on the object that would have been wrong.**
    /// Configuring a swapchain writes the format `conv`'s table names, the size
    /// the shell asked for, and the pacing the descriptor named — all read back
    /// off the layer.
    ///
    /// Red, one assertion each, when: `setPixelFormat:` is given the linear
    /// `BGRA8Unorm` instead of the sRGB one, which is the missing-encode bug
    /// `crcbl-wgpu` shipped and which no image comparison in this crate would
    /// catch; `setDrawableSize:` is skipped, leaving a detached layer at 0×0 so
    /// `nextDrawable` returns nil forever; `setFramebufferOnly:` is left at its
    /// `true` default, which makes `crcbl screenshot`'s copy out of a presented
    /// frame illegal; `setMaximumDrawableCount:` is left unclamped or unset;
    /// `setDisplaySyncEnabled:` ignores the present mode.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn configuring_a_layer_writes_the_format_the_table_says_and_the_size_asked_for() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let layer = detached_layer();
        // SAFETY: as above — a live layer this function owns and outlives the
        // surface.
        let surface = unsafe { instance.create_surface(&appkit_target(&layer)) }
            .expect("a CAMetalLayer is a Metal surface");

        // `image_count: 1` is below what Core Animation accepts, so this also
        // proves the clamp: an unclamped 1 raises inside `setMaximumDrawableCount:`.
        let mut desc = swapchain_desc(surface, Format::Bgra8UnormSrgb, 1);
        let swapchain = device.create_swapchain(&desc).expect("a layer swapchain");

        assert_eq!(
            layer.pixelFormat(),
            conv::pixel_format(Format::Bgra8UnormSrgb),
            "the layer's format must come from the one table"
        );
        assert_eq!(
            layer.pixelFormat(),
            MTLPixelFormat::BGRA8Unorm_sRGB,
            "an sRGB swapchain must encode on write, or every frame is too dark"
        );
        assert_eq!(
            (layer.drawableSize().width, layer.drawableSize().height),
            (f64::from(EXTENT.0), f64::from(EXTENT.1)),
            "obligation 1: the shell's size is what the layer is configured at"
        );
        assert!(
            !layer.framebufferOnly(),
            "a framebuffer-only drawable cannot be copied out of"
        );
        assert!(layer.isOpaque(), "CompositeAlpha::Opaque is what was asked");
        assert_eq!(
            layer.maximumDrawableCount(),
            MIN_LAYER_DRAWABLES as usize,
            "a request of one must clamp up rather than raise"
        );
        assert!(
            layer.displaySyncEnabled(),
            "Fifo is displaySyncEnabled = true"
        );

        // A reconfigure is the seam's one call for a mode change too.
        desc.present_mode = PresentMode::Immediate;
        desc.extent = (32, 16);
        device
            .reconfigure_swapchain(swapchain, &desc)
            .expect("a mode change keeps the handle");
        assert!(
            !layer.displaySyncEnabled(),
            "Immediate is displaySyncEnabled = false"
        );
        assert_eq!(
            (layer.drawableSize().width, layer.drawableSize().height),
            (32.0, 16.0),
            "a reconfigure resizes the layer too"
        );
        // And the layer's own size is now what `current_extent` reports, which
        // is the cross-check obligation 4 asks for.
        let caps = instance
            .surface_caps(surface, instance.adapters()[0].id)
            .expect("caps");
        assert_eq!(caps.current_extent, Some((32, 16)));

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    /// An object that is not a `CAMetalLayer` is refused rather than raising at
    /// the first `nextDrawable`.
    ///
    /// Red when the `isKindOfClass` check is removed: the surface is created,
    /// and the failure arrives much later as an Objective-C exception, which
    /// aborts the process rather than returning an `Err`.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_layer_that_is_not_a_metal_layer_is_refused_rather_than_raising() {
        let instance = open_instance();
        let plain = CALayer::new();
        let target = SurfaceTarget::AppKit {
            layer: core::ptr::NonNull::from(&*plain).cast(),
        };
        // SAFETY: `plain` is a live `CALayer` this function owns — a real
        // Objective-C object, which is what makes the retain sound. It is the
        // *wrong class*, which is what this test is about, and that is a check
        // rather than a safety condition.
        let error = unsafe { instance.create_surface(&target) }
            .expect_err("a CALayer is not a CAMetalLayer");
        let HalError::InvalidDescriptor(message) = error else {
            panic!("wrong variant");
        };
        assert!(message.contains("CAMetalLayer"), "{message}");
    }

    /// **A real drawable, acquired and presented.**
    ///
    /// `nextDrawable` is the one call in this module that needs a display
    /// server to answer: a detached layer in a CI container has no surface to
    /// vend a drawable from, and the call returns nil (reported here as
    /// [`SurfaceError::Timeout`]) or blocks. Everything else on the layer path
    /// — creation, the caps, every property the configure writes, the
    /// wrong-class refusal — is asserted ungated above, so what this adds is
    /// exactly the drawable.
    ///
    /// Feature-gated *and* `#[ignore]`d, the shape
    /// `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` already
    /// uses; `tests/run-mtl-e2e.sh` is the only thing that turns it on.
    ///
    /// Red when `nextDrawable`'s result is dropped rather than retained (the
    /// present finds nothing outstanding), when the drawable's texture is not
    /// registered as the frame's image (the two handles fail to resolve), when
    /// the extent is remembered instead of read off the texture, and when the
    /// rows outlive the present — which is what the second cycle checks, since
    /// a layer holding every drawable cannot vend another.
    ///
    /// # It is also the only check `addPresentedHandler:` has anywhere
    ///
    /// Each cycle numbers its present and then waits for that number, so a
    /// handler that is never attached, or one attached to a drawable whose
    /// report never arrives, fails here as [`SurfaceError::Timeout`] rather
    /// than passing quietly. Waiting on the frame just presented is what the
    /// seam tells a *frame loop* not to do — it drains the pipeline — and is
    /// exactly right for a test, which wants the narrowest window in which the
    /// report must appear.
    ///
    /// Nothing automated runs this: the `mtl-e2e` job excludes it by name
    /// because a headless runner's detached `CAMetalLayer` vends no drawable at
    /// all. **A person on a real Mac running `tests/run-mtl-e2e.sh` is the only
    /// thing that has ever executed the present-feedback path**, and if a
    /// detached layer turns out to vend drawables whose handlers never fire,
    /// this is where that will be discovered — as a timeout, with the wait
    /// naming the id it gave up on.
    #[cfg(feature = "mtl-e2e")]
    #[test]
    #[ignore = "needs a real drawable; a CI container's detached layer vends none"]
    fn a_layer_swapchain_acquires_a_drawable_and_presents_it() {
        use crcbl_hal::Device as _;
        let (instance, device) = open_device();
        let layer = detached_layer();
        // SAFETY: a live layer this function owns and outlives the surface.
        let surface = unsafe { instance.create_surface(&appkit_target(&layer)) }
            .expect("a CAMetalLayer is a Metal surface");
        let swapchain = device
            .create_swapchain(&swapchain_desc(surface, Format::Bgra8UnormSrgb, 3))
            .expect("a layer swapchain");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue exists");

        // Ten display periods at 60 Hz, so a frame that is genuinely on its way
        // has room even on a busy machine, and one that will never be reported
        // still fails the test in well under a second.
        let present_wait = Duration::from_millis(160);

        // More cycles than the layer has drawables: a backend that never gives
        // one back blocks here rather than finishing, which is the failure
        // `crcbl-wgpu`'s own e2e docs describe.
        let mut previous = None;
        for cycle in 0..(MAX_LAYER_DRAWABLES + 2) {
            let frame = device
                .acquire_next_frame(swapchain)
                .unwrap_or_else(|error| panic!("cycle {cycle} found no drawable: {error:?}"));
            assert_eq!(frame.extent, EXTENT, "obligation 3, off the drawable");
            assert!(frame.acquire_semaphore.is_none(), "{frame:?}");
            assert!(frame.present_semaphore.is_none(), "{frame:?}");
            device
                .inner
                .view_raw(frame.view)
                .expect("an acquired view resolves until its present");
            // Ids strictly increase across a swapchain's presents, and the
            // first must not be zero — the seam spells "unnumbered" that way.
            let present_id = u64::from(cycle) + 1;
            device
                .present(
                    queue,
                    &crcbl_hal::PresentInfo {
                        swapchain,
                        waits: &[],
                        present_id: Some(present_id),
                    },
                )
                .expect("presented");
            device
                .wait_until_presented(swapchain, present_id, present_wait)
                .unwrap_or_else(|error| {
                    panic!("present {present_id} was never reported as shown: {error:?}")
                });
            assert!(
                device.inner.view_raw(frame.view).is_err(),
                "the seam says an acquired image is valid *until* the present"
            );
            if let Some(previous) = previous {
                assert_ne!(
                    previous, frame.index,
                    "the reported index must move between frames"
                );
            }
            previous = Some(frame.index);
        }

        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }
}
