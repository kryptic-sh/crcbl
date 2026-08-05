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
//! # The swapchain owns the views too
//!
//! [`AcquiredFrame::view`] is there for the same reason as the two semaphores,
//! and it was added for the same reason: P1.1 found that *every* caller had to
//! build the identical per-image view cache, because
//! [`CommandEncoder::begin_render_pass`](crate::CommandEncoder::begin_render_pass)
//! needs an [`ImageViewHandle`] and acquire yielded only an [`ImageHandle`].
//! That is duplicated bookkeeping in every consumer *and* an index-keyed
//! invalidation rule every backend has to document — to save one field.
//!
//! The swapchain owns its images, so it is the natural owner of their views.
//! A backend creates one view per ring image at configure time, reissues them
//! on [`reconfigure`](crate::Device::reconfigure_swapchain), and destroys them
//! with the swapchain. The view covers the whole image with the swapchain's own
//! format, which is what a colour attachment wants; a caller that needs a
//! different view of a swapchain image — an sRGB reinterpretation, say — still
//! makes its own with [`Device::create_image_view`](crate::Device::create_image_view).
//!
//! # Reconfigure, don't recreate
//!
//! [`Device::reconfigure_swapchain`](crate::Device::reconfigure_swapchain) mirrors WebGPU's `configure()` and covers
//! resize, present-mode change and format change with one call. Vulkan
//! implements it by recreating; wgpu implements it by reconfiguring. Callers
//! never destroy and recreate a swapchain to resize, so the handle stays stable
//! across a resize storm and nothing above has to re-fetch it.
//!
//! # The extent: one request, one answer — implementation obligations
//!
//! Two things claim to know how big a swapchain should be, and they disagree in
//! practice:
//!
//! * the **shell**'s `WindowState::size()`, delivered as a
//!   `ShellEvent::Resized` when the window system configures the window; and
//! * [`SurfaceCaps::current_extent`], queried here.
//!
//! Vulkan makes the disagreement structural rather than hypothetical.
//! `VkSurfaceCapabilitiesKHR::currentExtent` is a real size on X11 (the server
//! knows), and is deliberately `0xFFFFFFFF, 0xFFFFFFFF` on Wayland, which means
//! "you choose — the compositor is not going to tell you". WebGPU has no such
//! query at all. A seam that left this unstated would get one answer from
//! `crcbl-vk` and a different one from `crcbl-wgpu`, and the bug would look
//! like a window that renders at the wrong size on exactly one backend.
//!
//! ## What P1.1 changed, and why
//!
//! These obligations originally said the shell's size was simply what the
//! swapchain *is* configured at, and that a backend "must not fail a create or
//! reconfigure merely because the two differ". Driving them against real
//! Vulkan proved that unimplementable on X11: `imageExtent` must lie within
//! `minImageExtent..=maxImageExtent`, and an X server reports
//! `minImageExtent == maxImageExtent == currentExtent`. So when a resize event
//! is one frame behind — which is the *normal* state during a resize —
//! **there is no legal swapchain at the shell's size at all**. A backend can
//! only clamp, and the old rule left it no way to say so, which meant a caller
//! could render a frame at a size the swapchain did not have.
//!
//! The rule below is the one that survives contact: the shell's size is the
//! **request**, and the extent that came back is on [`AcquiredFrame`].
//!
//! ## The obligations
//!
//! 1. **The shell's size is the request, and the only thing a backend may
//!    start from.** [`SwapchainDesc::extent`] is what the caller asks for. A
//!    backend **must not** substitute [`SurfaceCaps::current_extent`] for it,
//!    and **must not** fail a create or reconfigure merely because the two
//!    differ.
//! 2. **A backend clamps into the platform's permitted range, and reports what
//!    it configured.** Where the window system pins that range — X11 pins it to
//!    `currentExtent` exactly — clamping is forced, not chosen. The configured
//!    size is then reported on every [`AcquiredFrame::extent`], and a backend
//!    that had to clamp **should** say so once in a log line, because it means
//!    the shell and the window system are out of step.
//! 3. **A caller renders at [`AcquiredFrame::extent`], not at the size it asked
//!    for.** That is the render area, the viewport and the scissor. Using the
//!    requested size instead is the bug this field exists to prevent, and it
//!    only appears while a window is being dragged. The two are equal on every
//!    platform that does not pin a range, so this costs nothing to obey.
//! 4. **`current_extent` is a cross-check, and it is optional.** Report it when
//!    the platform supplies a real one; report `None` when it does not.
//!    `0xFFFFFFFF` is **not** a value to pass through — it is the platform
//!    saying "no opinion", so a Vulkan backend maps it to `None` and never lets
//!    that sentinel escape into the seam. Nothing above the seam needs to read
//!    it; obligation 3 already gives a caller the number it acts on.
//! 5. **A zero or absent extent is the caller's problem, not a fallback.** An
//!    unconfigured or minimized window has no size, a zero-extent swapchain is
//!    forbidden in Vulkan, and the answer is to *not create one yet* — never to
//!    guess from `current_extent`.
//!
//! The corollary for [`SurfaceTarget`](crcbl_core::SurfaceTarget): no variant
//! carries a size, including
//! [`Offscreen`](crcbl_core::SurfaceTarget::Offscreen), which used to and no
//! longer does. A target names handles; handles survive a resize; the extent
//! travels in the descriptor. That is what makes "re-query the target after a
//! mode change" a complete rule rather than one with an exception in it.
//!
//! # Offscreen is a swapchain too
//!
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) produces a
//! surface whose "swapchain" is a ring of plain images. `crcbl screenshot` and
//! the P1 golden-image e2e therefore run through the *same* acquire/present
//! path as a real window, instead of a second, less-exercised one.

use crcbl_core::Handle;

use crate::{Format, ImageHandle, ImageViewHandle, SemaphoreHandle};

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
///
/// # Decision: WSI vocabulary is correct here
///
/// P0's seam review flagged [`min_image_count`](Self::min_image_count) /
/// [`max_image_count`](Self::max_image_count) as Vulkan-shaped, since WebGPU
/// has no notion of a swapchain image count. Recorded so it is not
/// re-litigated: *surface capabilities are inherently a WSI concept* — this
/// whole struct describes what a window system will accept — so WSI vocabulary
/// is the right vocabulary in it. A Tier B backend reports the range its
/// platform actually offers (`2..=2` for a WebGPU canvas, which has one
/// implicit ring) and the caller's clamp code is unchanged. The alternative,
/// removing the fields, would take a real knob away from the three backends
/// that have one.
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
    ///
    /// **A cross-check, never the source of truth** — see the [module
    /// docs](self#the-extent-one-request-one-answer--implementation-obligations).
    /// The shell's `WindowState::size()` is what a swapchain is configured at;
    /// this is what the *surface* believes, which on Wayland is nothing at all
    /// (`None`, never the `0xFFFFFFFF` sentinel) and on X11 can be a frame
    /// behind a resize.
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
    /// Size in pixels, **from the shell** — the *request*.
    ///
    /// A backend starts here and never substitutes
    /// [`SurfaceCaps::current_extent`] for it (obligation 1), but it may have
    /// to clamp into a range the window system pins, in which case the size it
    /// actually configured comes back on [`AcquiredFrame::extent`] (obligations
    /// 2 and 3). **Render at that, not at this.** See the [module
    /// docs](self#the-extent-one-request-one-answer--implementation-obligations)
    /// for why the rule is stated rather than left to each backend, and note
    /// that a zero extent is a caller bug — an unconfigured or minimized window
    /// means "do not create a swapchain yet", not "pick something".
    ///
    /// In borderless mode this is the *native* surface size; render scale is a
    /// renderer feature applied to an offscreen target (topic 15), not a
    /// smaller swapchain.
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
    /// A whole-image view of [`image`](Self::image), in the swapchain's format
    /// — what a [`ColorAttachment`](crate::ColorAttachment) needs.
    ///
    /// **Owned by the swapchain**, like the semaphores: do not destroy it, and
    /// do not hold it across a
    /// [`reconfigure`](crate::Device::reconfigure_swapchain), which reissues
    /// every one. See the [module docs](self#the-swapchain-owns-the-views-too)
    /// for why this is here rather than left to each caller.
    pub view: ImageViewHandle,
    /// The size this swapchain was actually configured at, in pixels.
    ///
    /// **Obligation 3**: this is the render area, the viewport and the scissor
    /// — not [`SwapchainDesc::extent`], which is only what was *asked for*.
    /// The two differ exactly when the window system pinned a range the
    /// request fell outside of, which on X11 is any frame where a resize event
    /// has not caught up yet. See the [module
    /// docs](self#the-extent-one-request-one-answer--implementation-obligations).
    pub extent: (u32, u32),
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
    ///
    /// **This is the only place the seam carries "suboptimal".** A present can
    /// discover it too — `vkQueuePresentKHR` returns `VK_SUBOPTIMAL_KHR` — and
    /// [`Device::present`](crate::Device::present) has nowhere to put it, since
    /// it is not an error. That is deliberate rather than an oversight: a
    /// backend remembers it and folds it into the *next* acquire, which is
    /// where the caller already handles it, and which costs one `bool` instead
    /// of a second reporting channel every consumer would have to branch on.
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
    /// A number for this present, so it can be waited for afterwards with
    /// [`Device::wait_until_presented`](crate::Device::wait_until_presented).
    ///
    /// **Must strictly increase** across the presents of one swapchain, and
    /// starts over when the swapchain is
    /// [reconfigured](crate::Device::reconfigure_swapchain) — a backend is
    /// free to key state off the number, so a repeated or a going-backwards
    /// one is a caller bug. The engine uses its submission counter, which is
    /// monotonic already.
    ///
    /// `None` numbers nothing and is always legal: a caller that will never
    /// wait pays nothing for the seam existing, and a backend with no way to
    /// number a present ignores this field. It is an `Option` rather than a
    /// reserved `0` because a sentinel is a value someone eventually passes by
    /// accident.
    pub present_id: Option<u64>,
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

    /// The Tier B shape from the module docs, asserted against the backend that
    /// models it rather than against a hand-built literal: an implicit acquire
    /// returns `None` for both semaphores, so the renderer's splice becomes an
    /// empty slice rather than a branch. The image, the view and the extent are
    /// **not** optional — every backend has all three, which is the whole
    /// reason they sit beside the semaphores rather than being left to the
    /// caller — and here they are checked by *using* them.
    ///
    /// The Tier A half of the same contract, and the ring's rotation and
    /// reconfigure behaviour, are the null backend's own
    /// `swapchain_acquire_matches_the_tiers_sync_model`.
    #[test]
    fn an_implicit_acquire_frame_carries_no_semaphores() {
        use crate::null::NullInstance;
        use crate::{
            AdapterId, ClearValue, ColorAttachment, CommandEncoderDesc, DeviceDesc, Features,
            Instance, LoadOp, QueueKind, Rect2d, RenderPassDesc, StoreOp, SubmitInfo,
            SurfaceTarget,
        };

        const EXTENT: (u32, u32) = (1280, 720);

        let instance = NullInstance::tier_b();
        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing that has to outlive the surface.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("an offscreen surface always works");
        let device = instance
            .create_device(&DeviceDesc {
                required_features: Features::COMPUTE,
                ..DeviceDesc::for_adapter(AdapterId(0))
            })
            .expect("the tier B preset has compute");
        let surface_caps = instance
            .surface_caps(surface, AdapterId(0))
            .expect("surface caps");
        let format = surface_caps
            .preferred_format()
            .expect("a format is offered");
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("implicit acquire"),
                surface,
                format,
                extent: EXTENT,
                image_count: surface_caps.min_image_count,
                present_mode: surface_caps.choose_present_mode(&[PresentMode::Fifo]),
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("a swapchain");

        let frame = device.acquire_next_frame(swapchain).expect("acquire");
        assert!(
            frame.acquire_semaphore.is_none(),
            "a backend with an implicit acquire has nothing to wait on"
        );
        assert!(
            frame.present_semaphore.is_none(),
            "and nothing to signal into the present"
        );
        // Obligation 3: this is the number a caller renders at.
        assert_eq!(frame.extent, EXTENT);

        // The view is a live object beside the optional semaphores, not a
        // placeholder — so hand it to the thing it exists for. The null backend
        // records a validation error for a dead attachment view rather than
        // failing the call, which is what `assert_valid` below reads.
        let queue = device.queue(QueueKind::Graphics).expect("a graphics queue");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("implicit acquire"),
            queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("clear"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::default(),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(frame.extent.0, frame.extent.1),
        });
        encoder.end_render_pass();
        let commands = encoder.finish().expect("recording succeeded");
        instance.recorder().assert_valid();
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");

        // The renderer's splice: `None` collects to an empty slice, so the
        // present is the same code as Tier A's with nothing in it.
        let waits: Vec<SemaphoreHandle> = frame.present_semaphore.into_iter().collect();
        assert!(waits.is_empty(), "the splice is an empty slice");
        device
            .present(
                queue,
                &PresentInfo {
                    swapchain,
                    waits: &waits,
                    present_id: None,
                },
            )
            .expect("presenting with no waits is the Tier B path, not an error");

        device.destroy_command_buffer(commands);
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
    }

    #[test]
    fn default_present_mode_is_the_universally_supported_one() {
        assert_eq!(PresentMode::default(), PresentMode::Fifo);
        assert!(caps().supports_present_mode(PresentMode::Fifo));
        assert_eq!(CompositeAlpha::default(), CompositeAlpha::Opaque);
    }
}
