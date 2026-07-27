//! Surfaces, swapchains, and the acquire/present pair.
//!
//! # The acquire shape, and why it is this one
//!
//! `docs/plan/10-wasm-webgpu.md` lists "swapchain acquire is implicit/async" as
//! a constraint the HAL must already satisfy: *"HAL surface API shaped so
//! 'acquire' can be trivial (WebGPU `getCurrentTexture`)"*. The two APIs to
//! reconcile:
//!
//! | | Vulkan | WebGPU |
//! | --- | --- | --- |
//! | Call | `vkAcquireNextImageKHR(sc, timeout, sem, fence, &idx)` | `ctx.getCurrentTexture()` |
//! | Returns | an image index | a texture |
//! | Sync | caller supplies a semaphore to be signalled | none; implicit |
//! | Present | `vkQueuePresentKHR` waits on a semaphore | implicit at rAF end |
//!
//! A literal port of the Vulkan signature — `acquire_next_image(&self, swapchain,
//! semaphore)` — forces the caller to create and rotate binary semaphores it
//! has no use for on Tier B, and forces the wgpu backend to invent and signal
//! fake ones.
//!
//! **The fix: the swapchain owns its synchronisation.** [`Device::acquire_next_frame`](crate::Device::acquire_next_frame)
//! returns an [`AcquiredFrame`] carrying the image *and* an
//! `Option<SemaphoreHandle>` pair, which the backend supplies from its own
//! per-image ring:
//!
//! * `crcbl-vk` returns `Some(..)` for both. The renderer splices
//!   `acquire_semaphore` into its submit's waits and `present_semaphore` into
//!   its signals.
//! * `crcbl-wgpu` returns `None` for both. The same renderer code splices
//!   nothing.
//!
//! The renderer is written once, with no tier branch, and the WebGPU
//! implementation is three lines. This is also why [`AcquiredFrame`] hands back
//! an [`ImageHandle`] rather than an index the caller must map: WebGPU has no
//! stable image index to give, only a texture per frame.
//!
//! # Reconfigure, don't recreate
//!
//! [`Device::reconfigure_swapchain`](crate::Device::reconfigure_swapchain) mirrors WebGPU's `configure()` and covers
//! resize, present-mode change and format change with one call. Vulkan
//! implements it by recreating; wgpu implements it by reconfiguring. Callers
//! never destroy and recreate a swapchain to resize, so the handle stays stable
//! across a resize storm and nothing above has to re-fetch it.
//!
//! # Offscreen is a swapchain too
//!
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) produces a
//! surface whose "swapchain" is a ring of plain images. `crcbl screenshot` and
//! the P1 golden-image e2e therefore run through the *same* acquire/present
//! path as a real window, instead of a second, less-exercised one.

use crcbl_core::Handle;

use crate::{Format, ImageHandle, SemaphoreHandle};

/// Marker type for surface handles. Uninhabited.
#[derive(Debug)]
pub enum Surface {}
/// Marker type for swapchain handles. Uninhabited.
#[derive(Debug)]
pub enum Swapchain {}

/// A presentation surface, created from a
/// [`SurfaceTarget`](crcbl_core::SurfaceTarget).
pub type SurfaceHandle = Handle<Surface>;
/// A swapchain configured on a [`Surface`].
pub type SwapchainHandle = Handle<Swapchain>;

/// How presented images are paced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PresentMode {
    /// Wait for vblank, queue depth 1. Always supported; the only mode WebGPU
    /// has.
    #[default]
    Fifo,
    /// Wait for vblank, but tear rather than stall if a frame is late.
    FifoRelaxed,
    /// Replace the queued image with the newest one. Low latency, no tearing,
    /// uncapped GPU work. Preferred by `crcbl-vk` where available.
    Mailbox,
    /// Present immediately; tears. For latency measurement and uncapped
    /// benchmarking.
    Immediate,
}

/// How the surface's alpha channel composites with the desktop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompositeAlpha {
    /// Alpha ignored; the surface is opaque. The engine's default.
    #[default]
    Opaque,
    /// Alpha is premultiplied into the colour channels.
    PreMultiplied,
    /// Alpha is separate from the colour channels.
    PostMultiplied,
    /// Let the window system decide.
    Inherit,
}

/// What a surface supports on a given adapter.
///
/// Queried with [`Instance::surface_caps`](crate::Instance::surface_caps)
/// *before* a device exists, because present-queue selection on Vulkan needs
/// the answer at device-creation time.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceCaps {
    /// Formats the surface can be configured with, best first.
    pub formats: Vec<Format>,
    /// Present modes available. Always contains [`PresentMode::Fifo`].
    pub present_modes: Vec<PresentMode>,
    /// Composite-alpha modes available.
    pub composite_alpha: Vec<CompositeAlpha>,
    /// Fewest images the surface will accept.
    pub min_image_count: u32,
    /// Most images the surface will accept.
    pub max_image_count: u32,
    /// Current size in pixels, if the window system reports one.
    pub current_extent: Option<(u32, u32)>,
}

/// Configuration for a swapchain.
///
/// Passed to both [`Device::create_swapchain`](crate::Device::create_swapchain)
/// and [`Device::reconfigure_swapchain`](crate::Device::reconfigure_swapchain) —
/// one struct, so a resize cannot accidentally change something it did not mean
/// to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapchainDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Surface to present to.
    pub surface: SurfaceHandle,
    /// Image format. Must be one of [`SurfaceCaps::formats`].
    ///
    /// The engine presents an **sRGB** swapchain and renders to an
    /// [`Rgba16Float`](Format::Rgba16Float) offscreen target that a tonemap pass
    /// resolves into it — HDR from P1, per `docs/plan/ROADMAP.md`. The swapchain
    /// format is therefore a display format, never a shading one.
    pub format: Format,
    /// Size in pixels. In borderless mode this is the *native* surface size;
    /// render scale is a renderer feature applied to an offscreen target
    /// (topic 15), not a smaller swapchain.
    pub extent: (u32, u32),
    /// Requested image count. The backend clamps to
    /// [`SurfaceCaps::min_image_count`]/[`max`](SurfaceCaps::max_image_count).
    pub image_count: u32,
    /// Requested pacing. The backend falls back to [`PresentMode::Fifo`] if
    /// unavailable, which is always supported.
    pub present_mode: PresentMode,
    /// Desktop compositing mode.
    pub composite_alpha: CompositeAlpha,
}

/// A swapchain image ready to be rendered into.
///
/// See the module docs for why the semaphores are optional and owned by the
/// swapchain rather than supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcquiredFrame {
    /// Image to render into. Valid until the matching
    /// [`present`](crate::Device::present).
    ///
    /// Its state on acquire is *undefined*: the first barrier touching it must
    /// use [`ResourceState::Undefined`](crate::ResourceState::Undefined) as the
    /// source, which discards any previous contents. Loading a swapchain image
    /// is never correct.
    pub image: ImageHandle,
    /// Index within the swapchain's image ring. For debug output and for
    /// indexing per-image caches; do not use it to guess how many images exist.
    pub index: u32,
    /// Wait on this before the first submission that writes the image, if
    /// present. `None` on backends with an implicit acquire.
    pub acquire_semaphore: Option<SemaphoreHandle>,
    /// Signal this from the last submission that writes the image, and pass it
    /// to [`present`](crate::Device::present), if present. `None` on backends
    /// with an implicit present.
    pub present_semaphore: Option<SemaphoreHandle>,
    /// The swapchain still works but no longer matches the surface exactly —
    /// usually a resize that arrived mid-frame. Render this frame, then
    /// reconfigure. Ignoring it is legal and merely looks slightly wrong for one
    /// frame; treating it as fatal is a bug.
    pub suboptimal: bool,
}

/// Parameters for a present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentInfo<'a> {
    /// Swapchain to present.
    pub swapchain: SwapchainHandle,
    /// Waited on before the image is handed to the compositor. Normally exactly
    /// the [`AcquiredFrame::present_semaphore`], or empty when that was `None`.
    pub waits: &'a [SemaphoreHandle],
}

impl SurfaceCaps {
    /// Picks a swapchain format: the first sRGB format the surface offers, or
    /// the first format at all.
    ///
    /// sRGB by preference because the tonemap pass writes display-referred
    /// values and hardware sRGB encode is free. Returns `None` only if the
    /// surface offers nothing, which means the backend should have failed
    /// earlier.
    #[must_use]
    pub fn preferred_format(&self) -> Option<Format> {
        self.formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .or_else(|| self.formats.first().copied())
    }

    /// Whether `mode` is available.
    #[must_use]
    pub fn supports_present_mode(&self, mode: PresentMode) -> bool {
        self.present_modes.contains(&mode)
    }

    /// Picks the first available mode from `preferences`, falling back to
    /// [`PresentMode::Fifo`].
    ///
    /// The `[Mailbox, Fifo]` preference from
    /// `docs/plan/02-vulkan-backend.md` §2.2, expressed once here instead of
    /// re-derived in every backend.
    #[must_use]
    pub fn choose_present_mode(&self, preferences: &[PresentMode]) -> PresentMode {
        preferences
            .iter()
            .copied()
            .find(|mode| self.supports_present_mode(*mode))
            .unwrap_or(PresentMode::Fifo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> SurfaceCaps {
        SurfaceCaps {
            formats: vec![Format::Bgra8Unorm, Format::Bgra8UnormSrgb],
            present_modes: vec![PresentMode::Fifo, PresentMode::Mailbox],
            composite_alpha: vec![CompositeAlpha::Opaque],
            min_image_count: 2,
            max_image_count: 4,
            current_extent: Some((1920, 1080)),
        }
    }

    #[test]
    fn preferred_format_prefers_srgb_even_when_listed_second() {
        assert_eq!(caps().preferred_format(), Some(Format::Bgra8UnormSrgb));
    }

    #[test]
    fn preferred_format_falls_back_to_the_first_listed() {
        let linear_only = SurfaceCaps {
            formats: vec![Format::Rgba16Float, Format::Bgra8Unorm],
            ..caps()
        };
        assert_eq!(linear_only.preferred_format(), Some(Format::Rgba16Float));

        let empty = SurfaceCaps {
            formats: vec![],
            ..caps()
        };
        assert_eq!(empty.preferred_format(), None);
    }

    #[test]
    fn present_mode_choice_follows_the_plans_preference_order() {
        let caps = caps();
        assert_eq!(
            caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]),
            PresentMode::Mailbox
        );
        // Immediate is absent here, so the next preference wins.
        assert_eq!(
            caps.choose_present_mode(&[PresentMode::Immediate, PresentMode::Fifo]),
            PresentMode::Fifo
        );
        // Nothing available at all still yields Fifo, which every surface has.
        assert_eq!(
            caps.choose_present_mode(&[PresentMode::Immediate]),
            PresentMode::Fifo
        );
    }

    /// The Tier B shape from the module docs: a backend with an implicit
    /// acquire returns `None` for both semaphores, and the renderer's splice
    /// becomes a no-op rather than a branch.
    #[test]
    fn an_implicit_acquire_frame_carries_no_semaphores() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let frame = AcquiredFrame {
            image: pool.insert(0).cast(),
            index: 0,
            acquire_semaphore: None,
            present_semaphore: None,
            suboptimal: false,
        };
        let waits: Vec<_> = frame.acquire_semaphore.into_iter().collect();
        let signals: Vec<_> = frame.present_semaphore.into_iter().collect();
        assert!(waits.is_empty());
        assert!(signals.is_empty());
    }

    #[test]
    fn default_present_mode_is_the_universally_supported_one() {
        assert_eq!(PresentMode::default(), PresentMode::Fifo);
        assert!(caps().supports_present_mode(PresentMode::Fifo));
        assert_eq!(CompositeAlpha::default(), CompositeAlpha::Opaque);
    }
}
