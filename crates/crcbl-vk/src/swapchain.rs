//! Swapchains, the offscreen image ring, and the extent rule.
//!
//! # The extent rule, discharged
//!
//! `crcbl-hal`'s [`swapchain`](crcbl_hal::swapchain) module states four
//! numbered obligations. This is the first backend to meet them against a real
//! window system, and each is discharged in a named place:
//!
//! 1. **The shell's size is authoritative.** [`resolve_swapchain_extent`] starts
//!    from [`SwapchainDesc::extent`] and never reads `currentExtent`.
//! 2. **`currentExtent` is an optional cross-check.**
//!    [`resolve_current_extent`] maps Vulkan's `0xFFFFFFFF` sentinel — which is
//!    what Wayland reports, always — to `None`, so it cannot escape into the
//!    seam.
//! 3. **The caller reads the size from the shell.** Nothing here ever
//!    substitutes one for the other; `apps/sandbox` logs the disagreement.
//! 4. **A zero extent is the caller's problem.** A zero-extent swapchain is
//!    forbidden in Vulkan and is refused here with a descriptor error naming the
//!    rule, rather than guessed at.
//!
//! ## Where obligation 1 meets Vulkan
//!
//! Obligation 1 is about the *request*, and obligation 2 is what P1.1 added to
//! it. `imageExtent` must lie within `minImageExtent..=maxImageExtent`, and on
//! X11 the server reports `minImageExtent == maxImageExtent == currentExtent` —
//! so when the shell's size and the server's disagree, **there is no legal
//! swapchain at the shell's size**, and clamping is forced rather than chosen.
//!
//! This backend therefore clamps, logs loudly when clamping changed anything,
//! and reports the extent it actually configured on every
//! [`AcquiredFrame::extent`]. The seam originally had nowhere to put that
//! number; the field exists because this backend needed it.
//!
//! # Offscreen is a swapchain too
//!
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) produces a
//! surface with no `VkSurfaceKHR` at all, and its "swapchain" is a ring of plain
//! images this module allocates. Acquire rotates the ring and present advances
//! it. That is what makes `crcbl screenshot` and the P1 golden-image e2e run
//! through the *same* code path as a window, and it is the only Vulkan CI can
//! run without a compositor.

use ash::vk;

use crcbl_hal::{
    AcquiredFrame, CompositeAlpha, Format, HalError, ImageHandle, ImageViewHandle, PresentMode,
    SemaphoreHandle, SurfaceCaps, SurfaceError, SwapchainDesc,
};

use crate::conv;

/// Maps `VkSurfaceCapabilitiesKHR::currentExtent` onto the seam's
/// `Option<(u32, u32)>`.
///
/// **Obligation 2.** `(0xFFFFFFFF, 0xFFFFFFFF)` is the platform saying "you
/// choose — the compositor is not going to tell you", which is what Wayland
/// reports unconditionally. It is not a value; it is the absence of one, and
/// letting it through would produce a four-billion-pixel swapchain request on
/// exactly one backend.
#[must_use]
pub(crate) fn resolve_current_extent(extent: vk::Extent2D) -> Option<(u32, u32)> {
    // The spec defines the sentinel as *both* components being `0xFFFFFFFF`.
    // Checking either is the defensive reading and costs nothing.
    if extent.width == u32::MAX || extent.height == u32::MAX {
        None
    } else {
        Some((extent.width, extent.height))
    }
}

/// What [`resolve_swapchain_extent`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExtentDecision {
    /// The extent the swapchain is actually created at.
    pub(crate) configured: (u32, u32),
    /// Whether the surface's permitted range forced a change. Always worth a
    /// log line: it means the shell and the window system disagree, and on X11
    /// it means the shell's event is a frame behind.
    pub(crate) clamped: bool,
}

/// Resolves the extent a swapchain is created at.
///
/// **Obligations 1 and 4.** The requested extent is the shell's and is where
/// this starts; `currentExtent` is never consulted. Clamping to
/// `minImageExtent..=maxImageExtent` is not a substitution — it is the only
/// range in which a `VkSwapchainKHR` legally exists, and on X11 that range is a
/// single point.
///
/// # Errors
///
/// A zero extent, which obligation 4 makes the caller's problem: an
/// unconfigured or minimized window has no size, and the answer is not to
/// create a swapchain yet.
pub(crate) fn resolve_swapchain_extent(
    requested: (u32, u32),
    capabilities: &vk::SurfaceCapabilitiesKHR,
) -> Result<ExtentDecision, SurfaceError> {
    if requested.0 == 0 || requested.1 == 0 {
        return Err(SurfaceError::Hal(HalError::InvalidDescriptor(format!(
            "SwapchainDesc::extent is {requested:?}; a zero-extent swapchain is \
             forbidden in Vulkan, and an unconfigured or minimized window means \
             'do not create one yet' rather than 'pick something'"
        ))));
    }

    let min = capabilities.min_image_extent;
    let max = capabilities.max_image_extent;
    // A driver reporting a zero maximum is reporting nonsense; treating it as
    // "no limit" beats producing a zero-extent swapchain from a clamp.
    let max_width = if max.width == 0 { u32::MAX } else { max.width };
    let max_height = if max.height == 0 {
        u32::MAX
    } else {
        max.height
    };

    let configured = (
        requested.0.clamp(min.width.max(1), max_width),
        requested.1.clamp(min.height.max(1), max_height),
    );
    Ok(ExtentDecision {
        configured,
        clamped: configured != requested,
    })
}

/// Clamps a requested image count into what the surface accepts.
#[must_use]
pub(crate) fn resolve_image_count(
    requested: u32,
    capabilities: &vk::SurfaceCapabilitiesKHR,
) -> u32 {
    let max = if capabilities.max_image_count == 0 {
        u32::MAX
    } else {
        capabilities.max_image_count
    };
    requested.clamp(capabilities.min_image_count.max(1), max)
}

/// The capabilities an offscreen "surface" reports.
///
/// There is no window system to ask, so this is a statement of what the ring
/// supports rather than a query. `Fifo` because the seam promises it always
/// exists; `Rgba8UnormSrgb` first because
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// picks the first sRGB format and a golden image wants a display-referred one.
/// `current_extent: None` because — exactly like Wayland — nothing here has an
/// opinion about the size.
#[must_use]
pub(crate) fn offscreen_surface_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
            Format::Rgba16Float,
        ],
        present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: 1,
        max_image_count: 8,
        current_extent: None,
    }
}

/// The per-image synchronisation a windowed swapchain owns.
///
/// The seam's decision that "the swapchain owns its synchronisation" is what
/// keeps `crcbl-wgpu`'s implementation three lines, and this is the Vulkan
/// half of the bargain: binary semaphores nothing above the seam ever creates,
/// handed out through [`AcquiredFrame`].
#[derive(Debug)]
pub(crate) struct FrameSync {
    /// Signalled by `vkAcquireNextImageKHR`. One per *ring slot*, rotated, with
    /// a fence per slot so a slot is never reused while its acquire is still
    /// pending — which is a validation error, and the one every hand-rolled
    /// swapchain gets wrong first.
    pub(crate) acquire: Vec<vk::Semaphore>,
    /// Fence paired with `acquire[i]`, so reuse of slot `i` can be checked.
    pub(crate) acquire_fence: Vec<vk::Fence>,
    /// Whether `acquire_fence[i]` was actually armed by a *successful* acquire.
    ///
    /// A failed `vkAcquireNextImageKHR` signals neither the semaphore nor the
    /// fence, so waiting on an unarmed slot blocks forever. Created `false`
    /// rather than creating the fences signalled, which would say "armed" about
    /// a slot that had never been used.
    pub(crate) acquire_armed: Vec<bool>,
    /// Handles for `acquire`, so the seam can name them.
    pub(crate) acquire_handles: Vec<SemaphoreHandle>,
    /// Signalled by the submission that writes the image, waited by present.
    /// One per *image*, because an image cannot be re-acquired until its
    /// present has completed.
    pub(crate) present: Vec<vk::Semaphore>,
    /// Handles for `present`.
    pub(crate) present_handles: Vec<SemaphoreHandle>,
    /// Next ring slot to use.
    pub(crate) next_slot: usize,
}

/// What a swapchain handle resolves to.
#[derive(Debug)]
pub(crate) struct SwapchainEntry {
    /// Device that created it, per obligation 3.
    pub(crate) owner: u64,
    /// The raw surface, kept so `destroy_swapchain` can release it even after
    /// the caller has already destroyed the surface handle — which is exactly
    /// what obligation 2 permits a caller to do.
    pub(crate) surface_raw: vk::SurfaceKHR,
    /// Null for an offscreen ring.
    pub(crate) raw: vk::SwapchainKHR,
    /// The extent this swapchain was **actually** configured at, which is not
    /// always the one that was asked for — see the module docs, and
    /// `crcbl-hal`'s obligations 2 and 3. Reported on every
    /// [`AcquiredFrame::extent`].
    pub(crate) extent: (u32, u32),
    /// Images, owned by the driver for a real swapchain and by us for a ring.
    pub(crate) images: Vec<vk::Image>,
    /// One whole-image view per image, owned by the swapchain and handed out
    /// through [`AcquiredFrame::view`]. The seam puts these here so no caller
    /// has to build the same per-image cache — see `crcbl-hal`'s swapchain
    /// module.
    pub(crate) views: Vec<vk::ImageView>,
    /// Seam handles for `views`, reissued on every reconfigure alongside the
    /// image handles.
    pub(crate) view_handles: Vec<ImageViewHandle>,
    /// Memory backing `images`, empty for a real swapchain.
    pub(crate) memory: Vec<vk::DeviceMemory>,
    /// Seam handles for `images`, reissued on every reconfigure.
    pub(crate) image_handles: Vec<ImageHandle>,
    /// `None` for an offscreen ring, which has an implicit acquire.
    pub(crate) sync: Option<FrameSync>,
    /// Image index handed out by the last acquire, if one is outstanding.
    pub(crate) acquired: Option<u32>,
    /// Ring cursor for the offscreen case.
    pub(crate) next_offscreen: u32,
    /// `vkQueuePresentKHR` returned `SUBOPTIMAL_KHR`.
    ///
    /// The seam's `present` has nowhere to report that — it is not an error and
    /// `AcquiredFrame::suboptimal` is the only field that carries it — so it is
    /// remembered here and folded into the *next* acquire, which is where the
    /// caller already handles it.
    pub(crate) pending_suboptimal: bool,
}

impl SwapchainEntry {
    /// Whether this is the offscreen ring rather than a real WSI swapchain.
    pub(crate) fn is_offscreen(&self) -> bool {
        self.raw == vk::SwapchainKHR::null()
    }

    /// Builds the [`AcquiredFrame`] for image `index`.
    pub(crate) fn frame(&self, index: u32, slot: Option<usize>, suboptimal: bool) -> AcquiredFrame {
        let sync = self.sync.as_ref();
        AcquiredFrame {
            image: self.image_handles[index as usize],
            view: self.view_handles[index as usize],
            extent: self.extent,
            index,
            acquire_semaphore: sync
                .zip(slot)
                .map(|(sync, slot)| sync.acquire_handles[slot]),
            present_semaphore: sync.map(|sync| sync.present_handles[index as usize]),
            suboptimal,
        }
    }
}

/// Builds the create-info for a WSI swapchain.
///
/// Separate from the call so the *policy* — usage flags, transform, clipping,
/// the old-swapchain handoff — is one readable block rather than an argument
/// list at a call site.
pub(crate) fn swapchain_create_info<'a>(
    surface: vk::SurfaceKHR,
    desc: &SwapchainDesc<'_>,
    capabilities: &vk::SurfaceCapabilitiesKHR,
    extent: (u32, u32),
    image_count: u32,
    present_mode: vk::PresentModeKHR,
    old: vk::SwapchainKHR,
) -> vk::SwapchainCreateInfoKHR<'a> {
    // `TRANSFER_SRC` so `crcbl screenshot` can copy a presented frame out
    // without a second, differently-configured swapchain. Every driver supports
    // it on a swapchain image in practice, and `supported_usage_flags` is
    // checked rather than assumed.
    let mut usage = vk::ImageUsageFlags::COLOR_ATTACHMENT;
    if capabilities
        .supported_usage_flags
        .contains(vk::ImageUsageFlags::TRANSFER_SRC)
    {
        usage |= vk::ImageUsageFlags::TRANSFER_SRC;
    }

    // `IDENTITY` if the surface offers it, the current transform otherwise.
    // Anything else means the driver rotates the image for us, which is a
    // phone concern the engine does not have yet — but passing an unsupported
    // transform is a hard error, so this is a check, not an assumption.
    let transform = if capabilities
        .supported_transforms
        .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
    {
        vk::SurfaceTransformFlagsKHR::IDENTITY
    } else {
        capabilities.current_transform
    };

    vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(conv::format(desc.format))
        .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
        .image_extent(vk::Extent2D {
            width: extent.0,
            height: extent.1,
        })
        .image_array_layers(1)
        .image_usage(usage)
        // One queue family: the MVP submits everything to graphics
        // (`docs/plan/02-vulkan-backend.md` §2.1, stated not assumed), so
        // `EXCLUSIVE` is correct and is the faster of the two.
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(transform)
        .composite_alpha(conv::composite_alpha(desc.composite_alpha))
        .present_mode(present_mode)
        // The compositor may obscure part of the window; the engine never reads
        // a presented image back through the swapchain, so discarding those
        // pixels is free.
        .clipped(true)
        .old_swapchain(old)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// X11: the server knows the size, and pins the permitted range to it.
    fn x11_capabilities(width: u32, height: u32) -> vk::SurfaceCapabilitiesKHR {
        vk::SurfaceCapabilitiesKHR {
            min_image_count: 2,
            max_image_count: 4,
            current_extent: vk::Extent2D { width, height },
            min_image_extent: vk::Extent2D { width, height },
            max_image_extent: vk::Extent2D { width, height },
            supported_transforms: vk::SurfaceTransformFlagsKHR::IDENTITY,
            current_transform: vk::SurfaceTransformFlagsKHR::IDENTITY,
            supported_composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            supported_usage_flags: vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::TRANSFER_SRC,
            ..Default::default()
        }
    }

    /// Wayland: no opinion about the size, and a range wide enough to say so.
    fn wayland_capabilities() -> vk::SurfaceCapabilitiesKHR {
        vk::SurfaceCapabilitiesKHR {
            min_image_count: 2,
            max_image_count: 0,
            current_extent: vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            },
            min_image_extent: vk::Extent2D {
                width: 1,
                height: 1,
            },
            max_image_extent: vk::Extent2D {
                width: 16384,
                height: 16384,
            },
            supported_transforms: vk::SurfaceTransformFlagsKHR::IDENTITY,
            current_transform: vk::SurfaceTransformFlagsKHR::IDENTITY,
            supported_composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            supported_usage_flags: vk::ImageUsageFlags::COLOR_ATTACHMENT,
            ..Default::default()
        }
    }

    /// Obligation 2, stated as the property that matters: the sentinel is the
    /// absence of a value, and it must not travel.
    #[test]
    fn the_sentinel_is_absence_not_a_size() {
        assert_eq!(
            resolve_current_extent(wayland_capabilities().current_extent),
            None
        );
        assert_eq!(
            resolve_current_extent(x11_capabilities(1280, 720).current_extent),
            Some((1280, 720))
        );
        // Half a sentinel is not a thing any driver should produce, and is
        // still not a usable size.
        assert_eq!(
            resolve_current_extent(vk::Extent2D {
                width: u32::MAX,
                height: 720
            }),
            None
        );
    }

    /// Obligation 1 on Wayland, where it holds exactly: the shell's size is
    /// used unchanged, whatever the surface says.
    #[test]
    fn wayland_configures_at_the_shells_size_untouched() {
        let decision = resolve_swapchain_extent((1280, 720), &wayland_capabilities())
            .expect("a real size is not a caller bug");
        assert_eq!(decision.configured, (1280, 720));
        assert!(!decision.clamped);
    }

    /// Obligation 1 on X11, where Vulkan bends it. The server pins the range to
    /// its own idea of the size, so a shell event that has not arrived yet
    /// produces a clamp — and the clamp is *reported*, because silently
    /// rendering at a size nobody asked for is the bug this flag exists to
    /// surface.
    #[test]
    fn x11_clamps_to_the_servers_range_and_says_so() {
        let capabilities = x11_capabilities(800, 600);
        let decision = resolve_swapchain_extent((1280, 720), &capabilities).expect("not zero");
        assert_eq!(
            decision.configured,
            (800, 600),
            "there is no legal swapchain at any other size"
        );
        assert!(decision.clamped, "the disagreement must be visible");

        // Agreement is the common case and must not report a clamp.
        let decision = resolve_swapchain_extent((800, 600), &capabilities).expect("not zero");
        assert_eq!(decision.configured, (800, 600));
        assert!(!decision.clamped);
    }

    /// Obligation 4: a zero extent is refused, and the error says why rather
    /// than guessing a size from `currentExtent`.
    #[test]
    fn a_zero_extent_is_a_caller_error_not_a_fallback() {
        for requested in [(0, 720), (1280, 0), (0, 0)] {
            let error = resolve_swapchain_extent(requested, &x11_capabilities(1280, 720))
                .expect_err("zero is refused");
            let text = error.to_string();
            assert!(
                matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(_))),
                "{requested:?} gave {text}"
            );
        }
        // And the message names the rule, so the fix is obvious from the log.
        let error =
            resolve_swapchain_extent((0, 0), &wayland_capabilities()).expect_err("zero is refused");
        let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
            panic!("wrong variant");
        };
        assert!(message.contains("do not create one yet"), "{message}");
    }

    /// A driver reporting a zero maximum is reporting nonsense; the clamp must
    /// not turn that into a zero-extent swapchain.
    #[test]
    fn a_nonsense_maximum_does_not_produce_a_zero_extent() {
        let capabilities = vk::SurfaceCapabilitiesKHR {
            min_image_extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
            max_image_extent: vk::Extent2D {
                width: 0,
                height: 0,
            },
            ..wayland_capabilities()
        };
        let decision = resolve_swapchain_extent((640, 480), &capabilities).expect("not zero");
        assert_eq!(decision.configured, (640, 480));
        assert!(!decision.clamped);
    }

    #[test]
    fn image_counts_clamp_into_the_surfaces_range() {
        let capabilities = x11_capabilities(64, 64);
        assert_eq!(resolve_image_count(3, &capabilities), 3);
        assert_eq!(
            resolve_image_count(1, &capabilities),
            2,
            "below the minimum"
        );
        assert_eq!(
            resolve_image_count(9, &capabilities),
            4,
            "above the maximum"
        );
        // Wayland's "no maximum" must not clamp to zero.
        assert_eq!(resolve_image_count(3, &wayland_capabilities()), 3);
        assert_eq!(resolve_image_count(99, &wayland_capabilities()), 99);
    }

    /// The offscreen ring reports itself the same way Wayland does — no opinion
    /// about the extent — so the caller's code path is identical.
    #[test]
    fn the_offscreen_ring_has_no_opinion_about_its_size_either() {
        let caps = offscreen_surface_caps();
        assert_eq!(caps.current_extent, None);
        assert!(caps.supports_present_mode(PresentMode::Fifo));
        assert!(
            caps.preferred_format()
                .is_some_and(|format| format.is_srgb()),
            "a golden image wants a display-referred format"
        );
        assert!(caps.min_image_count >= 1);
    }

    /// `TRANSFER_SRC` is what `crcbl screenshot` copies a presented frame out
    /// with, and a driver that does not offer it must not be asked for it.
    #[test]
    fn swapchain_usage_asks_for_transfer_src_only_when_offered() {
        let desc = SwapchainDesc {
            label: None,
            surface: crcbl_core::Handle::from_bits(1 << 32).expect("non-zero generation"),
            format: Format::Bgra8UnormSrgb,
            extent: (64, 64),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        };
        let offered = swapchain_create_info(
            vk::SurfaceKHR::null(),
            &desc,
            &x11_capabilities(64, 64),
            (64, 64),
            2,
            vk::PresentModeKHR::FIFO,
            vk::SwapchainKHR::null(),
        );
        assert!(
            offered
                .image_usage
                .contains(vk::ImageUsageFlags::TRANSFER_SRC)
        );

        let withheld = swapchain_create_info(
            vk::SurfaceKHR::null(),
            &desc,
            &wayland_capabilities(),
            (64, 64),
            2,
            vk::PresentModeKHR::FIFO,
            vk::SwapchainKHR::null(),
        );
        assert!(
            !withheld
                .image_usage
                .contains(vk::ImageUsageFlags::TRANSFER_SRC)
        );
        assert!(
            withheld
                .image_usage
                .contains(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        );
        assert_eq!(withheld.clipped, vk::TRUE);
        assert_eq!(withheld.image_array_layers, 1);
        assert_eq!(withheld.image_sharing_mode, vk::SharingMode::EXCLUSIVE);
    }

    /// A surface that cannot do `IDENTITY` gets its own current transform back,
    /// because passing an unsupported one is a hard error rather than a hint.
    #[test]
    fn an_unsupported_identity_transform_falls_back_to_the_current_one() {
        let mut capabilities = wayland_capabilities();
        capabilities.supported_transforms = vk::SurfaceTransformFlagsKHR::ROTATE_90;
        capabilities.current_transform = vk::SurfaceTransformFlagsKHR::ROTATE_90;
        let desc = SwapchainDesc {
            label: None,
            surface: crcbl_core::Handle::from_bits(1 << 32).expect("non-zero generation"),
            format: Format::Bgra8UnormSrgb,
            extent: (64, 64),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        };
        let info = swapchain_create_info(
            vk::SurfaceKHR::null(),
            &desc,
            &capabilities,
            (64, 64),
            2,
            vk::PresentModeKHR::FIFO,
            vk::SwapchainKHR::null(),
        );
        assert_eq!(info.pre_transform, vk::SurfaceTransformFlagsKHR::ROTATE_90);
    }
}
