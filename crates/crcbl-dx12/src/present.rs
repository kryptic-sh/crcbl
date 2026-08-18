//! Everything about a D3D12 swapchain that is arithmetic rather than DXGI.
//!
//! The flip-model format set, the buffer-count clamp, the frame latency, what a
//! [`PresentMode`] becomes as a sync interval, the timeout a wait is given, and
//! the ledger [`Device::wait_until_presented`](crcbl_hal::Device::wait_until_presented)
//! is answered from. **Not one `windows` type appears in this file**, which is
//! why it is `#[cfg(any(target_os = "windows", test))]` while the rest of the
//! backend is Windows-only: off Windows it exists in the test build and nowhere
//! else, purely so these assertions run on a machine before CI does.
//!
//! `crcbl_mtl::present` is the model, and it was written for the same reason:
//! the half of a present-feedback implementation that can be checked without a
//! display is the half worth keeping separate.
//!
//! # DXGI's waitable object carries no id, so the id is kept here
//!
//! `IDXGISwapChain2::GetFrameLatencyWaitableObject` hands back a handle that is
//! signalled when the swapchain has room for another frame — the second of the
//! three shapes
//! [`Features::PRESENT_FEEDBACK`](crcbl_hal::Features::PRESENT_FEEDBACK) names,
//! "one hands out a waitable object with no number at all". So
//! [`PresentInfo::present_id`](crcbl_hal::PresentInfo::present_id) has to be
//! recorded on this side of the seam, and [`PresentLedger`] is that record.
//!
//! What the ledger decides is **whether there is anything to wait for**, which
//! is the seam's three immediate answers minus the one about the capability
//! flag (this backend always has it). What it deliberately does *not* try to
//! decide is when a particular numbered frame reached the display: DXGI cannot
//! answer that, and the seam says so — its guarantee is "the weakest of the
//! three", and for D3D12 that is "fewer than
//! [`SetMaximumFrameLatency`](frame_latency) presents are still outstanding".
//!
//! # The ledger is per swapchain, and resets with it
//!
//! One [`PresentLedger`] belongs to one `SwapchainEntry`, and
//! `reconfigure_swapchain` replaces it with a fresh one — which is where
//! `PresentInfo::present_id`'s "starts over when the swapchain is
//! reconfigured" comes from, by construction rather than by a line of code.
//! `ResizeBuffers` is the call underneath that, and it is the exact moment the
//! waitable object's own count is no longer about the frames the caller
//! numbered.

use core::time::Duration;

use crcbl_hal::{CompositeAlpha, Format, PresentMode, SurfaceCaps};

/// Formats a flip-model swapchain can be configured with, best first.
///
/// **This is a refusal list as much as an offer.** `DXGI_SWAP_EFFECT_FLIP_DISCARD`
/// accepts four back-buffer formats and rejects everything else at
/// `CreateSwapChainForHwnd`, so reporting the seam's whole
/// [`Format`](crcbl_hal::Format) set here would be an offer the create call
/// cannot keep — the failure `Instance::surface_caps` calls out by name, one
/// step later than an empty list.
///
/// The sRGB spellings are on the list and the buffers are **not** created with
/// them: a flip-model back buffer is created linear and its render target view
/// names the sRGB format, which is the one cast D3D12 permits on a swapchain
/// buffer without a typeless resource. [`buffer_format`] is that mapping, and
/// it is why this list may say `Bgra8UnormSrgb` honestly.
///
/// sRGB comes first because
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// prefers it anyway and the engine presents an sRGB swapchain.
pub(crate) const FLIP_MODEL_FORMATS: &[Format] = &[
    Format::Bgra8UnormSrgb,
    Format::Rgba8UnormSrgb,
    Format::Bgra8Unorm,
    Format::Rgba8Unorm,
    Format::Rgb10a2Unorm,
    Format::Rgba16Float,
];

/// Whether a flip-model swapchain can be configured with `format`.
pub(crate) fn is_flip_model_format(format: Format) -> bool {
    FLIP_MODEL_FORMATS.contains(&format)
}

/// The format the **back buffers** are created with for a swapchain the caller
/// asked for in `format`.
///
/// The sRGB variants lose their encoding here and get it back on the view.
/// `DXGI_SWAP_EFFECT_FLIP_DISCARD` rejects `_SRGB` back buffers outright, and
/// the sanctioned way to still present sRGB is the one every D3D12 sample uses:
/// a linear buffer with an sRGB render target view over it.
///
/// Everything else is its own buffer format, so this is identity for the linear
/// half of [`FLIP_MODEL_FORMATS`].
pub(crate) fn buffer_format(format: Format) -> Format {
    match format {
        Format::Bgra8UnormSrgb => Format::Bgra8Unorm,
        Format::Rgba8UnormSrgb => Format::Rgba8Unorm,
        other => other,
    }
}

/// Fewest back buffers a flip-model swapchain may have.
pub(crate) const MIN_BUFFERS: u32 = 2;
/// Most back buffers DXGI will accept, `DXGI_MAX_SWAP_CHAIN_BUFFERS`.
pub(crate) const MAX_BUFFERS: u32 = 16;

/// The back-buffer count a request of `requested` is configured at.
///
/// The seam documents [`SwapchainDesc::image_count`](crcbl_hal::SwapchainDesc::image_count)
/// as a request the backend clamps, which is what this is. Two is not a
/// preference: `DXGI_SWAP_EFFECT_FLIP_DISCARD` refuses a single-buffered
/// swapchain, so a caller asking for one image gets two and renders correctly
/// rather than getting `E_INVALIDARG`.
pub(crate) fn buffer_count(requested: u32) -> u32 {
    requested.clamp(MIN_BUFFERS, MAX_BUFFERS)
}

/// Formats an **offscreen ring** can be created with, best first.
///
/// Nothing here is a DXGI restriction: an offscreen ring is a set of plain
/// `ID3D12Resource` textures this backend creates itself, so
/// `DXGI_SWAP_EFFECT_FLIP_DISCARD`'s refusal list has no say and the sRGB
/// spellings are the resources' own formats rather than a view's cast. What the
/// list *is* is the set `crcbl-vk`'s offscreen ring offers, in its order, so a
/// harness that renders the same scene through both backends and compares the
/// frames gets the same channel order from
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// rather than a swizzled picture on one of them.
pub(crate) const OFFSCREEN_FORMATS: &[Format] = &[
    Format::Rgba8UnormSrgb,
    Format::Bgra8UnormSrgb,
    Format::Rgba8Unorm,
    Format::Bgra8Unorm,
    Format::Rgba16Float,
];

/// Whether an offscreen ring can be created with `format`.
pub(crate) fn is_offscreen_format(format: Format) -> bool {
    OFFSCREEN_FORMATS.contains(&format)
}

/// Fewest images an offscreen ring may have.
///
/// One, unlike [`MIN_BUFFERS`]: the two-buffer floor is flip-discard's, and a
/// ring of plain images has no compositor to hold one of them. A screenshot
/// that wants exactly one target should get one.
pub(crate) const MIN_RING_IMAGES: u32 = 1;

/// Most images an offscreen ring may have.
///
/// Not a platform limit — there is no DXGI object to have one — but a cap on
/// how much a caller's `image_count` can allocate by accident. The same eight
/// `crcbl-vk`'s offscreen caps report.
pub(crate) const MAX_RING_IMAGES: u32 = 8;

/// The image count an offscreen ring request of `requested` is configured at.
pub(crate) fn ring_image_count(requested: u32) -> u32 {
    requested.clamp(MIN_RING_IMAGES, MAX_RING_IMAGES)
}

/// The ring image after `current`, in a ring of `count`.
///
/// The whole of what makes a ring a ring, and it lives here rather than beside
/// the entry it mutates for this module's own reason: it is arithmetic, so it
/// is checkable on a machine with no D3D12 at all. Getting it wrong is a
/// screenshot harness reading back the frame before the one it drew, which is a
/// picture that looks plausible.
///
/// A `count` of zero cannot arise — [`ring_image_count`] clamps to
/// [`MIN_RING_IMAGES`] — and is answered with zero rather than a division by
/// zero, because "the ring has no images" is a state whose right cursor is the
/// one an acquire will reject.
pub(crate) fn next_ring_image(current: u32, count: u32) -> u32 {
    if count == 0 {
        return 0;
    }
    (current + 1) % count
}

/// The present modes an offscreen ring offers.
///
/// Both are honest and neither does anything: "presenting" a ring image is a
/// cursor bump with no display behind it, so nothing paces it either way.
/// [`PresentMode::Fifo`] is there because the seam requires every surface to
/// have it; [`PresentMode::Immediate`] because a headless caller asking not to
/// wait for a vblank is asking for exactly what this already does, and refusing
/// it would push a screenshot tool into `Fifo` for no reason.
pub(crate) fn offscreen_present_modes() -> Vec<PresentMode> {
    vec![PresentMode::Fifo, PresentMode::Immediate]
}

/// The mode an offscreen ring is configured at, given what was asked for.
///
/// The seam's fallback rule, as [`resolve_present_mode`] applies it to a
/// windowed swapchain.
pub(crate) fn resolve_offscreen_present_mode(requested: PresentMode) -> PresentMode {
    if offscreen_present_modes().contains(&requested) {
        requested
    } else {
        PresentMode::Fifo
    }
}

/// What an offscreen "surface" reports it can do.
///
/// There is no window system to ask, so this is a statement about the ring
/// rather than a query — the same shape and the same reasoning as
/// `crcbl-vk`'s own `offscreen_surface_caps`. `current_extent` is `None`
/// because nothing here has an opinion about the size, which is exactly what
/// the seam's obligation 4 asks a backend to say when the platform does not
/// supply one.
pub(crate) fn offscreen_surface_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: OFFSCREEN_FORMATS.to_vec(),
        present_modes: offscreen_present_modes(),
        // The one mode a ring of plain images composites in: nothing composites
        // it at all, and a caller reading these bytes back gets the alpha
        // channel the render pass wrote.
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: MIN_RING_IMAGES,
        max_image_count: MAX_RING_IMAGES,
        current_extent: None,
    }
}

/// The value `SetMaximumFrameLatency` is given for a swapchain of `buffers`
/// back buffers.
///
/// One fewer than the buffers, because that is how many frames can be queued
/// while the caller still owns one to render into — a latency equal to the
/// buffer count would let the queue fill and leave `acquire_next_frame`
/// handing back a buffer the display is reading.
///
/// It is also the number the waitable object counts against: a wait returns
/// once fewer than this many presents are outstanding, so the **first**
/// `latency` waits on a fresh swapchain return immediately and prove nothing.
/// That is not a defect to code around; it is the pipeline filling.
pub(crate) fn frame_latency(buffers: u32) -> u32 {
    buffers.saturating_sub(1).max(1)
}

/// How a present is paced: what `Present`'s two arguments become.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pacing {
    /// `IDXGISwapChain::Present`'s `SyncInterval`. One vblank, or zero.
    pub(crate) sync_interval: u32,
    /// Whether the present carries `DXGI_PRESENT_ALLOW_TEARING`.
    ///
    /// **Only ever true beside a zero sync interval.** DXGI rejects the flag on
    /// a synchronised present, which is why the two travel together in one
    /// struct rather than as two independent knobs a call site could pair
    /// wrongly.
    pub(crate) allow_tearing: bool,
}

/// What `Present` is called with for a resolved [`PresentMode`].
///
/// `Mailbox` and `FifoRelaxed` are here for totality and are never resolved to
/// — see [`present_modes`], which does not offer them — and both fall to the
/// synchronised present, which is the safe direction for a mode DXGI cannot
/// express.
pub(crate) fn pacing(mode: PresentMode) -> Pacing {
    match mode {
        PresentMode::Immediate => Pacing {
            sync_interval: 0,
            allow_tearing: true,
        },
        PresentMode::Fifo | PresentMode::FifoRelaxed | PresentMode::Mailbox => Pacing {
            sync_interval: 1,
            allow_tearing: false,
        },
    }
}

/// The present modes a DXGI flip-model swapchain genuinely offers.
///
/// * [`PresentMode::Fifo`] always — `Present(1, 0)`, which the seam requires
///   every surface to have.
/// * [`PresentMode::Immediate`] **only when the factory reports tearing**.
///   `Present(0, 0)` on a flip-model swapchain does not tear: it replaces the
///   queued frame and still waits for the vblank, so offering `Immediate` for
///   it would be a mode that does not do what its name says. Tearing needs
///   `DXGI_PRESENT_ALLOW_TEARING`, which needs
///   `IDXGIFactory5::CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING)`
///   *and* the swapchain to carry `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`.
///
/// [`PresentMode::Mailbox`] and [`PresentMode::FifoRelaxed`] are deliberately
/// absent: DXGI has neither. A deeper flip-discard queue is still FIFO, and
/// `FifoRelaxed`'s "tear rather than stall" has no DXGI spelling at all —
/// `ALLOW_TEARING` is unconditional tearing, not a late-frame fallback.
pub(crate) fn present_modes(tearing: bool) -> Vec<PresentMode> {
    let mut out = vec![PresentMode::Fifo];
    if tearing {
        out.push(PresentMode::Immediate);
    }
    out
}

/// The mode a swapchain is configured at, given what was asked for.
///
/// The seam's rule: "the backend falls back to [`PresentMode::Fifo`] if
/// unavailable, which is always supported".
pub(crate) fn resolve_present_mode(requested: PresentMode, tearing: bool) -> PresentMode {
    if present_modes(tearing).contains(&requested) {
        requested
    } else {
        PresentMode::Fifo
    }
}

/// The millisecond argument a `WaitForSingleObject*` gets for a seam timeout.
///
/// Shared by the two waits this backend blocks on — the swapchain's
/// frame-latency waitable object, and the fence event a semaphore wait arms in
/// `crate::device` — because the clamp and the rounding below are properties of
/// the *conversion*, not of what is being waited for.
///
/// The clamp is the point. `INFINITE` is `u32::MAX` milliseconds, so a
/// [`Duration`] long enough to saturate the conversion would turn a bounded
/// wait into one that never returns — the opposite of what a timeout is for. A
/// millisecond short of that is 49 days, which no caller distinguishes from
/// forever and which the caller's own deadline outlives anyway.
///
/// Rounded up, not down: a sub-millisecond timeout that truncated to zero would
/// make the wait a poll, and the seam's `Timeout` would then be reported for a
/// frame — or a semaphore value — that was merely not up *yet*.
pub(crate) fn timeout_millis(timeout: Duration) -> u32 {
    let millis = timeout.as_millis();
    let rounded = if timeout.subsec_nanos().is_multiple_of(1_000_000) {
        millis
    } else {
        millis + 1
    };
    u32::try_from(rounded)
        .unwrap_or(u32::MAX - 1)
        .min(u32::MAX - 1)
}

/// What a wait for a given present id has to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PresentWait {
    /// The id names a present this swapchain object was never given, so there
    /// is nothing to block on. The seam's immediate `Ok(())`.
    NothingToWaitFor,
    /// Block on the swapchain's frame-latency waitable object.
    Block,
}

/// The presents this swapchain object has been given a number for.
///
/// One `u64`, because that is the whole of what DXGI leaves this side to keep:
/// there is no per-frame report to match against, only "the swapchain has room
/// again". `crcbl-vk` keeps the same single number for the same question and
/// hands it to `vkWaitForPresentKHR`; `crcbl-mtl` keeps a second one because
/// its presented handler reports an id back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PresentLedger {
    /// The highest id this swapchain object has taken for a present that went
    /// out. An id above it names a frame the swapchain was never given.
    committed: u64,
}

impl PresentLedger {
    /// Takes `present_id` as the number for a present that has just succeeded,
    /// and says whether it was taken.
    ///
    /// `false` means the present goes out unnumbered and nothing will be able
    /// to wait for it. Two ids get that answer, and they are the two
    /// `crcbl_mtl::present` refuses for the same reasons:
    ///
    /// * **Zero**, which numbers nothing. `PresentInfo::present_id` is an
    ///   `Option` so no sentinel is needed, but a caller can still pass
    ///   `Some(0)`, and `committed` starts at zero — treating it as a real id
    ///   would make an untouched ledger claim to have presented something.
    /// * **One that does not follow the last**, which the seam calls a caller
    ///   bug: ids must strictly increase across a swapchain's presents. The
    ///   answer is to refuse the number rather than to move `committed` down,
    ///   because a `committed` that went backwards would report every id
    ///   between the two as never presented and answer their waits at once.
    pub(crate) fn record_present(&mut self, present_id: u64) -> bool {
        if present_id == 0 || present_id <= self.committed {
            return false;
        }
        self.committed = present_id;
        true
    }

    /// Whether a wait for `present_id` has anything to block on.
    ///
    /// The seam's "no record of this id" case is not rare and costs a whole
    /// timeout when it is missed: `Device::present` can fail after the caller
    /// has already spent an id, and a reconfigure builds a ledger that has seen
    /// none of the old ones. Both land above `committed` and are answered here.
    ///
    /// An id **below** `committed` that was never itself taken — a caller that
    /// skipped a number — blocks, which is later than the truth and never
    /// earlier. That is the safe direction for a wait whose promise is "no
    /// longer waiting to happen".
    pub(crate) fn plan(self, present_id: u64) -> PresentWait {
        if present_id == 0 || present_id > self.committed {
            PresentWait::NothingToWaitFor
        } else {
            PresentWait::Block
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every format offered is one `CreateSwapChainForHwnd` will accept**,
    /// and the sRGB half is offered because the *view* carries the encoding.
    ///
    /// The failure this guards is the one `Instance::surface_caps` names: a
    /// backend that answers with formats the create call rejects has moved the
    /// refusal one call later, where a caller has already picked one.
    ///
    /// Red when a format outside the four flip-model back-buffer layouts is
    /// added to the list, and red when `buffer_format` stops stripping sRGB —
    /// the second assertion then finds `Bgra8UnormSrgb` claiming to be its own
    /// buffer format.
    #[test]
    fn every_offered_format_has_a_legal_flip_model_buffer_format() {
        assert!(!FLIP_MODEL_FORMATS.is_empty(), "nothing to check");
        // The four layouts `DXGI_SWAP_EFFECT_FLIP_DISCARD` accepts, named here
        // rather than derived from the list under test so this is a comparison
        // and not a tautology.
        const BUFFERS: &[Format] = &[
            Format::Bgra8Unorm,
            Format::Rgba8Unorm,
            Format::Rgb10a2Unorm,
            Format::Rgba16Float,
        ];
        for &format in FLIP_MODEL_FORMATS {
            let buffer = buffer_format(format);
            assert!(
                BUFFERS.contains(&buffer),
                "{format:?} would be created as {buffer:?}, which flip-discard rejects"
            );
            assert!(
                !buffer.is_srgb(),
                "{format:?} kept its sRGB encoding on the buffer: {buffer:?}"
            );
            assert!(is_flip_model_format(format), "{format:?}");
        }
        assert!(
            !is_flip_model_format(Format::Rgba32Float),
            "a format flip-discard rejects is reported as offered"
        );
        // The preference order the engine reads through
        // `SurfaceCaps::preferred_format`.
        assert!(
            FLIP_MODEL_FORMATS[0].is_srgb(),
            "the engine presents an sRGB swapchain, so sRGB is offered first"
        );
    }

    /// **An offscreen ring is not a flip-model swapchain, and its caps say so
    /// in the two places that matter.**
    ///
    /// The seam's own floor first: `Fifo` must be offered, the format list and
    /// the mode list must both be non-empty — the failure
    /// `Instance::surface_caps` calls out by name — and `preferred_format` must
    /// find something, because `crcbl::screenshot` calls exactly that and has
    /// nothing to fall back on.
    ///
    /// Then the two differences from [`FLIP_MODEL_FORMATS`] and
    /// [`MIN_BUFFERS`], which are what makes this a separate set rather than a
    /// reuse: a ring may be one image deep, and it may hold `Rgb10a2Unorm`
    /// never — that format is on the flip-model list and off this one, so a
    /// list that had quietly been aliased to the other would fail here.
    ///
    /// Red when `offscreen_surface_caps` starts reporting
    /// `FLIP_MODEL_FORMATS`, when `MIN_RING_IMAGES` is raised to flip-discard's
    /// two, or when `Fifo` is dropped from `offscreen_present_modes`.
    #[test]
    fn an_offscreen_ring_reports_its_own_caps_and_not_the_flip_model_ones() {
        let caps = offscreen_surface_caps();
        assert!(!caps.formats.is_empty(), "a surface offers something");
        assert!(
            caps.present_modes.contains(&PresentMode::Fifo),
            "the seam requires Fifo of every surface: {:?}",
            caps.present_modes
        );
        assert_eq!(
            caps.preferred_format(),
            Some(Format::Rgba8UnormSrgb),
            "crcbl::screenshot takes the preferred format and it decides the channel order"
        );
        assert_eq!(caps.current_extent, None, "nothing here has an opinion");
        assert_eq!(caps.min_image_count, 1, "a ring may be one image deep");
        assert!(caps.max_image_count >= caps.min_image_count);

        // The list is its own, not flip-discard's. `Rgb10a2Unorm` is the
        // witness: a flip-model back buffer may be one and a ring image is not
        // offered as one.
        assert!(
            !caps.formats.contains(&Format::Rgb10a2Unorm),
            "the offscreen list was aliased to the flip-model one: {:?}",
            caps.formats
        );
        for &format in OFFSCREEN_FORMATS {
            assert!(is_offscreen_format(format), "{format:?}");
        }
        assert!(
            !is_offscreen_format(Format::D32Float),
            "a depth format is not a colour ring image"
        );

        // The clamp, at both ends and in the middle.
        assert_eq!(ring_image_count(0), MIN_RING_IMAGES);
        assert_eq!(ring_image_count(1), 1);
        assert_eq!(ring_image_count(2), 2);
        assert_eq!(ring_image_count(u32::MAX), MAX_RING_IMAGES);

        // And the cursor wraps rather than running off the end. A ring that
        // never advanced hands the same image to every frame; one that
        // advanced without wrapping indexes past `images` on the trip after
        // the first, which is the acquire error rather than a wrong picture.
        let count = ring_image_count(2);
        let mut cursor = 0;
        let mut walk = vec![cursor];
        for _ in 0..4 {
            cursor = next_ring_image(cursor, count);
            walk.push(cursor);
        }
        assert_eq!(walk, vec![0, 1, 0, 1, 0], "a two-image ring must wrap");
        assert_eq!(
            next_ring_image(0, 1),
            0,
            "a one-image ring's cursor wraps onto itself"
        );
        assert_eq!(
            next_ring_image(7, MAX_RING_IMAGES),
            0,
            "the last image of the deepest ring wraps to the first"
        );
        // Unreachable through `ring_image_count`, and answered rather than
        // dividing by zero.
        assert_eq!(next_ring_image(0, 0), 0);

        // And the mode resolution falls back rather than configuring one the
        // caps did not offer.
        assert_eq!(
            resolve_offscreen_present_mode(PresentMode::Immediate),
            PresentMode::Immediate
        );
        assert_eq!(
            resolve_offscreen_present_mode(PresentMode::Mailbox),
            PresentMode::Fifo,
            "a mode the caps do not offer falls back to the one every surface has"
        );
        for mode in caps.present_modes.iter().copied() {
            assert_eq!(
                resolve_offscreen_present_mode(mode),
                mode,
                "an offered mode must survive resolution"
            );
        }
    }

    /// The two clamps that keep a descriptor D3D12 would refuse from reaching
    /// it.
    ///
    /// Red when `buffer_count` stops clamping at either end — one back buffer
    /// is `E_INVALIDARG` from `CreateSwapChainForHwnd`, and so is seventeen.
    /// Red when `frame_latency` can answer zero, which
    /// `SetMaximumFrameLatency` rejects.
    #[test]
    fn the_buffer_count_and_the_frame_latency_stay_inside_what_dxgi_accepts() {
        assert_eq!(buffer_count(0), MIN_BUFFERS);
        assert_eq!(buffer_count(1), MIN_BUFFERS);
        assert_eq!(buffer_count(2), 2);
        assert_eq!(buffer_count(3), 3);
        assert_eq!(buffer_count(u32::MAX), MAX_BUFFERS);

        for buffers in [MIN_BUFFERS, 3, 4, MAX_BUFFERS] {
            let latency = frame_latency(buffers);
            assert!(
                latency >= 1,
                "{buffers} buffers gave a latency of {latency}"
            );
            assert!(
                latency < buffers,
                "{buffers} buffers gave a latency of {latency}, which leaves none to draw into"
            );
        }
        // The clamp's own floor, in case a future caller reaches this directly.
        assert_eq!(frame_latency(0), 1);
        assert_eq!(frame_latency(1), 1);
    }

    /// **Tearing is probed, not assumed, and `Immediate` is offered only when
    /// the probe said yes.**
    ///
    /// The pairing is the assertion: `DXGI_PRESENT_ALLOW_TEARING` is illegal
    /// beside a non-zero sync interval, so a mode that carried the flag *and*
    /// waited for a vblank would fail every present with `E_INVALIDARG`.
    ///
    /// Red when `present_modes` starts offering `Immediate` unconditionally
    /// (the second block finds it in the list a factory without tearing
    /// produced), and red when `pacing` pairs the flag with a sync interval of
    /// one.
    #[test]
    fn immediate_is_offered_only_where_tearing_is_and_never_pairs_with_a_vblank() {
        let without = present_modes(false);
        assert_eq!(without, vec![PresentMode::Fifo]);
        let with = present_modes(true);
        assert!(with.contains(&PresentMode::Fifo), "{with:?}");
        assert!(with.contains(&PresentMode::Immediate), "{with:?}");
        assert!(
            !with.contains(&PresentMode::Mailbox) && !with.contains(&PresentMode::FifoRelaxed),
            "DXGI has neither of those: {with:?}"
        );

        // Every mode the seam has, so the pairing rule is checked over all of
        // them rather than over the two that are offered.
        for mode in [
            PresentMode::Fifo,
            PresentMode::FifoRelaxed,
            PresentMode::Mailbox,
            PresentMode::Immediate,
        ] {
            let pacing = pacing(mode);
            assert!(
                !pacing.allow_tearing || pacing.sync_interval == 0,
                "{mode:?} asks to tear on a synchronised present: {pacing:?}"
            );
        }
        assert_eq!(pacing(PresentMode::Immediate).sync_interval, 0);
        assert_eq!(pacing(PresentMode::Fifo).sync_interval, 1);

        // And the resolution falls back rather than configuring a mode the
        // machine cannot present in.
        assert_eq!(
            resolve_present_mode(PresentMode::Immediate, false),
            PresentMode::Fifo
        );
        assert_eq!(
            resolve_present_mode(PresentMode::Immediate, true),
            PresentMode::Immediate
        );
        assert_eq!(
            resolve_present_mode(PresentMode::Mailbox, true),
            PresentMode::Fifo,
            "DXGI has no mailbox, whatever the factory says about tearing"
        );
    }

    /// A timeout never becomes `INFINITE`, and never becomes a poll.
    ///
    /// Both directions are failures a caller cannot see: a wait that turned
    /// into `INFINITE` hangs the frame loop on a stalled compositor, and one
    /// that truncated to zero reports `SurfaceError::Timeout` for every frame
    /// while the display is perfectly healthy.
    ///
    /// Red when the `u32::MAX - 1` clamp is dropped (the first assertion sees
    /// `INFINITE`), and red when the round-up is dropped (the third sees zero).
    #[test]
    fn a_timeout_is_never_infinite_and_never_rounds_down_to_a_poll() {
        const INFINITE: u32 = u32::MAX;
        assert_ne!(
            timeout_millis(Duration::from_secs(u64::from(u32::MAX))),
            INFINITE,
            "a long timeout became a wait that never returns"
        );
        assert_ne!(timeout_millis(Duration::MAX), INFINITE);
        assert_eq!(timeout_millis(Duration::from_nanos(1)), 1);
        assert_eq!(timeout_millis(Duration::from_micros(1500)), 2);
        // Whole milliseconds are not rounded up, or every timeout would drift.
        assert_eq!(timeout_millis(Duration::from_millis(50)), 50);
        assert_eq!(timeout_millis(Duration::from_secs(1)), 1_000);
        // Zero is a genuine poll and stays one: the caller asked for no wait.
        assert_eq!(timeout_millis(Duration::ZERO), 0);
    }

    /// **The seam's immediate answer, which is the case that costs a whole
    /// timeout when it is missing.**
    ///
    /// An id above `committed` names a frame this swapchain was never given: a
    /// present that failed after the caller spent the id, or any id at all
    /// before the first present. Nothing will ever complete it, so the wait has
    /// to return rather than block on a waitable object that answers a
    /// different question.
    ///
    /// Red when `plan`'s `present_id > self.committed` guard is dropped: every
    /// assertion here then answers `Block`.
    #[test]
    fn an_id_this_dxgi_swapchain_was_never_given_is_answered_at_once() {
        let mut ledger = PresentLedger::default();
        assert_eq!(ledger.plan(1), PresentWait::NothingToWaitFor);
        assert_eq!(ledger.plan(u64::MAX), PresentWait::NothingToWaitFor);
        assert_eq!(
            ledger.plan(0),
            PresentWait::NothingToWaitFor,
            "zero numbers nothing"
        );

        assert!(ledger.record_present(1));
        assert!(ledger.record_present(2));
        assert_eq!(ledger.plan(2), PresentWait::Block);
        assert_eq!(
            ledger.plan(3),
            PresentWait::NothingToWaitFor,
            "the frame after the last present has not been presented"
        );
    }

    /// **An id that does not follow the last is refused rather than renumbering
    /// the swapchain.**
    ///
    /// Letting `committed` move down would be worse than ignoring the id: every
    /// id between the old and new values would then be reported as never
    /// presented, so waits for frames that really are on their way would return
    /// at once and the pacing loop would spin.
    ///
    /// Red when `record_present` drops the `present_id <= self.committed`
    /// guard — id 3 then walks `committed` back to 3 and the last assertion
    /// finds present 5 unwaitable.
    #[test]
    fn a_d3d12_present_id_that_does_not_follow_the_last_is_refused() {
        let mut ledger = PresentLedger::default();
        assert!(!ledger.record_present(0), "zero numbers nothing");
        assert!(ledger.record_present(5));
        assert!(!ledger.record_present(5), "a repeat is a caller bug");
        assert!(!ledger.record_present(3), "so is going backwards");
        assert!(ledger.record_present(6), "and the next real id still works");
        assert_eq!(ledger.plan(5), PresentWait::Block);
        assert_eq!(ledger.plan(6), PresentWait::Block);
        assert_eq!(ledger.plan(7), PresentWait::NothingToWaitFor);
    }

    /// A rebuilt swapchain remembers none of the old one's presents.
    ///
    /// `reconfigure_swapchain` calls `ResizeBuffers` and replaces the ledger,
    /// so this pins the property that makes that correct: the engine's own
    /// counter does **not** restart, so the first id asked of a fresh ledger is
    /// a large one and has to be answered rather than waited for. Blocking
    /// instead would sit on the waitable object for a frame the old swapchain
    /// presented.
    ///
    /// Red for any future ledger that carried state across a reconfigure, and
    /// red if `Default` ever starts at a non-zero `committed`.
    #[test]
    fn a_fresh_ledger_answers_every_id_the_old_dxgi_swapchain_had_presented() {
        let mut old = PresentLedger::default();
        for id in 1..=100 {
            assert!(old.record_present(id));
        }
        assert_eq!(old.plan(100), PresentWait::Block);

        let rebuilt = PresentLedger::default();
        assert_eq!(rebuilt.plan(100), PresentWait::NothingToWaitFor);
        assert_eq!(
            rebuilt.plan(101),
            PresentWait::NothingToWaitFor,
            "nor the id the engine's counter has got to"
        );
    }
}
