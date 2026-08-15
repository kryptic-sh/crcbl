//! The round trip, driven from JavaScript, so that a browser can be asked
//! whether it works.
//!
//! Everything else in this crate is checked without a browser: the encoding
//! against a committed fixture, the transport against a synthetic
//! `WebAssembly.Memory`, the reply format against the same fixture read the
//! other way. **None of that can call `navigator.gpu`**, which is the one thing
//! this slice added and the one thing no node tool can reach. So there has to be
//! an entry point a page can drive end to end — encode a request, let the
//! replayer answer it, read what came back — and this module is it.
//!
//! # What it is, plainly
//!
//! An observation point, not a backend. It owns a [`StreamChannel`] because
//! nothing else does yet: `crcbl::backend`'s registry entry for
//! [`BackendKind::WebGpu`](crcbl_hal::BackendKind::WebGpu) still refuses, so no
//! engine code calls [`install`] and the seven transport
//! exports answer `0` on every frame of every demo. **When the backend arrives
//! and installs its own channel, this module has done its job and goes**, taking
//! its exports with it — and it refuses rather than fights on the way,
//! because [`install`] will not replace a live channel.
//!
//! # Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_gpu_probe_adapters`](shim::__crcbl_web_gpu_probe_adapters) | `() -> i32` | Encode one enumeration and register its wait. `1`, or `0` if there was no room or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_state`](shim::__crcbl_web_gpu_probe_state) | `() -> i32` | Drain whatever JS has committed and answer one of the `PROBE_*` codes. |
//! | [`__crcbl_web_gpu_probe_text_ptr`](shim::__crcbl_web_gpu_probe_text_ptr) | `() -> i32` | Where the adapter's name, or the reason there is none, starts. |
//! | [`__crcbl_web_gpu_probe_text_len`](shim::__crcbl_web_gpu_probe_text_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_features_lo`](shim::__crcbl_web_gpu_probe_features_lo) | `() -> i32` | Low 32 bits of the granted adapter's [`Features`]. |
//! | [`__crcbl_web_gpu_probe_features_hi`](shim::__crcbl_web_gpu_probe_features_hi) | `() -> i32` | High 32 bits of the same. |
//! | [`__crcbl_web_gpu_probe_max_image_2d`](shim::__crcbl_web_gpu_probe_max_image_2d) | `() -> i32` | The granted adapter's [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d). |
//! | [`__crcbl_web_gpu_probe_device`](shim::__crcbl_web_gpu_probe_device) | `() -> i32` | Encode one device request for the adapter that was granted, and register its wait. `1`, or `0` if nothing has been granted yet, there was no room, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_device_state`](shim::__crcbl_web_gpu_probe_device_state) | `() -> i32` | Drain, and answer one of the `DEVICE_*` codes. |
//! | [`__crcbl_web_gpu_probe_device_reason_ptr`](shim::__crcbl_web_gpu_probe_device_reason_ptr) | `() -> i32` | Where the reason no device opened starts. Empty when one did. |
//! | [`__crcbl_web_gpu_probe_device_reason_len`](shim::__crcbl_web_gpu_probe_device_reason_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_device_features_lo`](shim::__crcbl_web_gpu_probe_device_features_lo) | `() -> i32` | Low 32 bits of the **opened device's** [`Features`]. |
//! | [`__crcbl_web_gpu_probe_device_features_hi`](shim::__crcbl_web_gpu_probe_device_features_hi) | `() -> i32` | High 32 bits of the same. |
//! | [`__crcbl_web_gpu_probe_device_max_image_2d`](shim::__crcbl_web_gpu_probe_device_max_image_2d) | `() -> i32` | The opened device's [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d). |
//! | [`__crcbl_web_gpu_probe_surface`](shim::__crcbl_web_gpu_probe_surface) | `(i32) -> i32` | Encode one [`CreateSurface`](crate::Command::CreateSurface) against [`PROBE_SURFACE`], naming the canvas that `canvas_id` is the page's registry key for. `1`, or `0` if the probe is re-entered or another channel is installed. |
//!
//! **`state` before `ptr`, always** — the log queue's rule and for its reason:
//! a `state` call decodes a buffer and clones a string out of it, so it
//! allocates, and an allocation may grow wasm memory and detach a `Uint8Array`
//! built before the call. The pointers, the lengths and the six numbers
//! allocate nothing.
//!
//! **Either `state` drains for both probes.** There is one channel and one
//! committed reply buffer, so the first of the two calls in a frame decodes it
//! and hands each probe its own answer; the second finds nothing left and
//! reports what its probe now holds. The consequence worth stating: a buffer
//! that will not decode is reported by whichever was asked first, as that
//! probe's `*_UNDECODABLE`, and the other reports the state it was already in.
//! Dropping the other probe's answer instead would leave a command waiting for
//! ever, which is the one thing this channel must never do.
//!
//! # The device this asks for, and why it asks for so little
//!
//! [`probe_device_desc`] requires [`Features::COMPUTE`](crcbl_hal::Features::COMPUTE)
//! — core WebGPU, so every browser can satisfy it — and asks for **nothing
//! optional**. That is not timidity: it is what makes the answer checkable. A
//! device opened with no optional features and no requested limits is the
//! specification's own default, so the page can open a second one for itself
//! and compare, and the result differs from the *adapter's* capabilities on any
//! machine whose adapter reports more than the floor. A request that asked for
//! everything the adapter had would produce a device whose capabilities equal
//! the adapter's, and a backend that reported the adapter's record for its
//! device would then pass.
//!
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) is
//! deliberately *not* what this uses: it requires
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE), which WebGPU
//! does not have, so it is the refusal case rather than the opening one. See
//! [`crate::device`].
//!
//! # The surface probe is one export, and that is the command's shape
//!
//! [`__crcbl_web_gpu_probe_surface`](shim::__crcbl_web_gpu_probe_surface) has no
//! `state`, no codes of its own and nothing to absorb, because
//! [`CreateSurface`](crate::Command::CreateSurface) has **no entry on the reply
//! channel**: identity is positional, so wasm names the handle itself and moves
//! on, and there is nothing for a browser to send back. A state machine here
//! would have one state and a poll would have nothing to poll for, so the honest
//! shape is one call that encodes one command and answers whether it went.
//!
//! That also decides where a failure surfaces. A canvas id the page has not
//! registered, or a canvas that will not give up a `webgpu` context, is a
//! `SurfaceError` **thrown out of the replayer in JS** — the far side cannot be
//! told, so the near side is. `web/engine/gpu-replay.js` argues that choice
//! where it is made.
//!
//! # Why three numbers and not the whole of `AdapterInfo`
//!
//! These exist for the browser gate, and a gate check is only worth its line if a
//! browser can **corroborate** it — the adapter-name check compares what wasm
//! received against what `navigator.gpu` tells the same page, which is what makes
//! it evidence rather than a constant. Two of the seven fields on the wire have
//! that property: the feature set, which the page can rebuild from
//! `adapter.features`, and the limits, which the page can read off
//! `adapter.limits`. Both vary per machine and per browser.
//!
//! The other five do not. `vendor_id`, `device_id`, `device_type` and `driver`
//! are the documented absences — a browser has nothing to disagree with — and
//! `id` is `0` by construction. Exporting them would add checks that can only
//! restate a constant, so they are held by `cargo test` and by
//! `web/tools/gpu-replay.mjs` instead, where the whole record is compared field
//! for field.
//!
//! `max_image_2d` is one limit of nineteen for the same reason: it is
//! `maxTextureDimension2D`, which differs between a phone and a desktop, so it
//! catches a limits block that crossed as zeros. What holds the other eighteen is
//! the mapping check the gate runs in-page against the live adapter, plus the
//! committed fixture.
//!
//! The device's three numbers are the same three for the same reasons, and one
//! more: **they are what says the device's capabilities are not a copy of the
//! adapter's.** A page can open its own default device and read `device.features`
//! and `device.limits.maxTextureDimension2D` off it, and both differ from the
//! adapter's whenever the adapter reports anything above the specification's
//! floor.
//!
//! `i32` pairs rather than one `i64` because the whole of this ABI is
//! `(i32, …) -> i32`, which `docs/plan/41-webgpu-stream.md` sets as the
//! convention and which needs no `BigInt` on the JS side.
//!
//! `web/engine/gpu-probe.js` is the page's half, and
//! `web/tools/browser-e2e.mjs` is what drives it in a real browser.

use std::cell::RefCell;
use std::rc::Rc;

use crcbl_hal::{AdapterId, DeviceDesc, Features, SurfaceHandle};

use crate::device::DeviceProbe;
use crate::instance::AdapterProbe;
use crate::web::{StreamChannel, install};

/// [`AdapterProbe::Unasked`], or no channel to ask through.
pub const PROBE_UNASKED: u32 = 0;
/// [`AdapterProbe::Waiting`] — the request is out and unanswered.
pub const PROBE_WAITING: u32 = 1;
/// [`AdapterProbe::Granted`]; the text is the adapter's name.
pub const PROBE_GRANTED: u32 = 2;
/// [`AdapterProbe::Refused`]; the text is the reason.
pub const PROBE_REFUSED: u32 = 3;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the text is the [`DecodeError`](crate::DecodeError).
///
/// Distinct from [`PROBE_REFUSED`] because the two blame opposite halves: a
/// refusal is a browser with no GPU, and this is the format's two hand-written
/// sides having drifted.
pub const PROBE_UNDECODABLE: u32 = 4;

/// [`DeviceProbe::Unasked`], or no adapter to open.
pub const DEVICE_UNASKED: u32 = 0;
/// [`DeviceProbe::Waiting`] — `requestDevice` has not settled.
///
/// [`DeviceRequestState::Pending`](crcbl_hal::DeviceRequestState::Pending) seen
/// through the ABI, and the ordinary answer on every frame between the ask and
/// the answer.
pub const DEVICE_WAITING: u32 = 1;
/// [`DeviceProbe::Opened`]; the three numeric exports carry the device's own
/// capabilities and the reason is empty.
pub const DEVICE_OPENED: u32 = 2;
/// [`DeviceProbe::Failed`]; the reason says what the browser refused, or what
/// this backend refused to ask it for.
pub const DEVICE_FAILED: u32 = 3;
/// The committed reply buffer would not decode; the reason is the
/// [`DecodeError`](crate::DecodeError). [`PROBE_UNDECODABLE`]'s twin, and
/// distinct from [`DEVICE_FAILED`] for its reason: a refusal is a browser, and
/// this is the format's two hand-written sides having drifted.
pub const DEVICE_UNDECODABLE: u32 = 4;

/// The descriptor [`shim::__crcbl_web_gpu_probe_device`] asks with.
///
/// Requires only [`Features::COMPUTE`], which core WebGPU grants with no
/// `GPUFeatureName` behind it, and asks for nothing optional — see the [module
/// docs](self#the-device-this-asks-for-and-why-it-asks-for-so-little) for why
/// the emptiness is the point rather than a placeholder.
#[must_use]
pub const fn probe_device_desc(adapter: AdapterId) -> DeviceDesc<'static> {
    DeviceDesc {
        label: Some("crcbl-webgpu probe"),
        adapter,
        required_features: Features::COMPUTE,
        optional_features: Features::empty(),
        compatible_surface: None,
    }
}

/// The surface [`shim::__crcbl_web_gpu_probe_surface`] creates, every time.
///
/// One fixed handle rather than one drawn from a pool, because the probe has no
/// pool to draw from: it is an observation point, and identity on this stream is
/// positional — wasm picks the id, JS files the context under it. Index `0` with
/// generation `1`, the smallest bit pattern
/// [`Handle::from_bits`](crcbl_core::Handle::from_bits) accepts.
///
/// Asking twice therefore names this same surface twice, and the replayer's
/// table takes the second context in the first's place rather than growing.
pub const PROBE_SURFACE: SurfaceHandle = match SurfaceHandle::from_bits(1 << 32) {
    Some(surface) => surface,
    // Generation `1`, written into the high half above, so this arm is the
    // literal being wrong rather than a case a caller can reach.
    None => panic!("generation 1 is not zero"),
};

thread_local! {
    /// The probe's own channel and its state. Thread-local for
    /// [`crate::web`]'s reason: whichever thread the engine runs on is the one
    /// the shim calls.
    static PROBE: RefCell<Probe> = const { RefCell::new(Probe::new()) };
}

/// The channel the probe installed, the two calls it is waiting on, and the
/// last thing each has to say.
#[derive(Debug)]
struct Probe {
    /// Held for as long as the probe exists, because
    /// [`install`] keeps only a [`Weak`](std::rc::Weak):
    /// dropping this is what puts every transport export back to `0`.
    channel: Option<Rc<StreamChannel>>,
    state: AdapterProbe,
    /// The adapter's name, the reason there is none, or a decode error.
    text: String,
    device: DeviceProbe,
    /// Why no device opened, or a decode error. Its own string rather than a
    /// share of [`text`](Self::text): the two probes settle at different times
    /// and each export reads the text belonging to its own `state` call, so one
    /// buffer would mean whichever ran last.
    reason: String,
}

impl Probe {
    const fn new() -> Self {
        Self {
            channel: None,
            state: AdapterProbe::Unasked,
            text: String::new(),
            device: DeviceProbe::Unasked,
            reason: String::new(),
        }
    }

    /// Install a channel if this probe has none, and hand it back.
    ///
    /// `None` when a channel is already installed by something that is not this
    /// probe — which is the engine having grown a real backend, and the point at
    /// which this module should be deleted rather than made to share.
    fn channel(&mut self) -> Option<&Rc<StreamChannel>> {
        if self.channel.is_none() {
            let channel = Rc::new(StreamChannel::new());
            if !install(&channel) {
                return None;
            }
            self.channel = Some(channel);
        }
        self.channel.as_ref()
    }

    /// Encode one enumeration and register its wait.
    fn request(&mut self) -> bool {
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(state) = AdapterProbe::request(channel) else {
            return false;
        };
        self.state = state;
        self.text.clear();
        true
    }

    /// Encode one device request for the adapter that was granted.
    ///
    /// `false` when nothing has been granted yet, which is an ordering rule
    /// rather than a failure: [`DeviceDesc::adapter`](crcbl_hal::DeviceDesc)
    /// names an adapter from an enumeration, so there has to have been one.
    fn request_device(&mut self) -> bool {
        let Some(adapter) = self.granted().map(|info| info.id) else {
            return false;
        };
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(state) = DeviceProbe::request(channel, &probe_device_desc(adapter)) else {
            return false;
        };
        self.device = state;
        self.reason.clear();
        true
    }

    /// Encode one [`CreateSurface`](crate::Command::CreateSurface) against
    /// [`PROBE_SURFACE`], naming the canvas `canvas_id` is the page's key for.
    ///
    /// [`encode`](StreamChannel::encode) and never
    /// [`encode_awaited`](StreamChannel::encode_awaited): nothing answers this
    /// command, so a registered wait would hold a slot in a bounded set for a
    /// reply that is never coming.
    fn request_surface(&mut self, canvas_id: u32) -> bool {
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| stream.create_surface(PROBE_SURFACE, canvas_id))
            .is_some()
    }

    /// Drain what JS has committed and hand **both** probes their answers.
    ///
    /// The error, if the buffer would not decode, for the caller to report as
    /// its own probe's `*_UNDECODABLE`. One drain for the two of them because
    /// there is one buffer: absorbing into only the probe that asked would drop
    /// the other's answer, and a dropped reply is a command that waits for ever.
    fn drain(&mut self) -> Option<crate::DecodeError> {
        let channel = self.channel.as_ref()?;
        // `None` is the inbox being borrowed, which nothing here can cause; it
        // reads as "no replies this frame", which is also what an ordinary
        // frame answers.
        match channel.drain_replies() {
            Some(Ok(replies)) => {
                self.state.absorb(&replies);
                self.device.absorb(&replies);
                None
            }
            Some(Err(error)) => Some(error),
            None => None,
        }
    }

    /// Drain, absorb, and report where the enumeration has got to.
    fn state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.text = error.to_string();
            return PROBE_UNDECODABLE;
        }
        match &self.state {
            AdapterProbe::Unasked => PROBE_UNASKED,
            AdapterProbe::Waiting { .. } => PROBE_WAITING,
            AdapterProbe::Granted { info } => {
                self.text.clone_from(&info.name);
                PROBE_GRANTED
            }
            AdapterProbe::Refused { reason } => {
                self.text.clone_from(reason);
                PROBE_REFUSED
            }
        }
    }

    /// Drain, absorb, and report where the device request has got to.
    fn device_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.reason = error.to_string();
            return DEVICE_UNDECODABLE;
        }
        match &self.device {
            DeviceProbe::Unasked => DEVICE_UNASKED,
            DeviceProbe::Waiting { .. } => DEVICE_WAITING,
            DeviceProbe::Opened { .. } => {
                self.reason.clear();
                DEVICE_OPENED
            }
            DeviceProbe::Failed { reason, .. } => {
                self.reason.clone_from(reason);
                DEVICE_FAILED
            }
        }
    }

    /// What the granted adapter said about itself, or `None` if none was.
    ///
    /// The numeric exports read through this rather than each reaching into the
    /// enum, so "not granted" is answered in one place instead of three.
    const fn granted(&self) -> Option<&crcbl_hal::AdapterInfo> {
        match &self.state {
            AdapterProbe::Granted { info } => Some(info),
            _ => None,
        }
    }

    /// What the opened device said about itself, or `None` if none opened.
    ///
    /// **Not [`granted`](Self::granted)'s `caps`**, and the whole reason the
    /// device has numeric exports of its own: WebGPU grants a device what was
    /// asked for, which is less than the adapter has.
    const fn opened(&self) -> Option<crcbl_hal::DeviceCaps> {
        self.device.caps()
    }
}

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`. None of these is `unsafe`: none
/// dereferences a pointer the caller supplied.
pub mod shim {
    use super::{DEVICE_UNASKED, PROBE, PROBE_UNASKED};

    /// Ask the browser what it will grant.
    ///
    /// `1` when one enumeration is on the stream with its wait registered; `0`
    /// when there was no room, when the probe is re-entered, or when another
    /// channel is already installed.
    ///
    /// Calling it twice asks twice: each request is its own sequence, and the
    /// second one replaces the first probe's state, so the first answer arrives
    /// naming a sequence nothing is waiting for any more.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_adapters() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request()),
            Err(_) => 0,
        })
    }

    /// Drain the committed replies and report where the enumeration has got to.
    ///
    /// One of the `PROBE_*` codes. **May allocate**, so any view onto wasm
    /// memory is built after it rather than before.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.state(),
            Err(_) => PROBE_UNASKED,
        })
    }

    /// Where the text belonging to the last
    /// [`__crcbl_web_gpu_probe_state`] starts. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_text_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.text.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that text is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_text_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.text.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the browser to open the adapter it granted.
    ///
    /// `1` when one device request is on the stream with its wait registered;
    /// `0` when no adapter has been granted yet, when there was no room, when
    /// the probe is re-entered, or when another channel is already installed.
    ///
    /// **The enumeration has to have been answered first.** The descriptor names
    /// an [`AdapterId`](crcbl_hal::AdapterId) from an enumeration, so there is
    /// nothing to name until one has come back — and `0` here while
    /// [`__crcbl_web_gpu_probe_state`] still answers
    /// [`PROBE_WAITING`](super::PROBE_WAITING) is that ordering, not a failure.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_device()),
            Err(_) => 0,
        })
    }

    /// Drain the committed replies and report where the device request has got
    /// to.
    ///
    /// One of the `DEVICE_*` codes. **May allocate**, on
    /// [`__crcbl_web_gpu_probe_state`]'s terms and for its reason — and, like
    /// it, this is the call that drains for *both* probes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.device_state(),
            Err(_) => DEVICE_UNASKED,
        })
    }

    /// Where the reason belonging to the last
    /// [`__crcbl_web_gpu_probe_device_state`] starts. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_reason_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.reason.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that reason is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_reason_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.reason.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Reads one number off the granted adapter, or `0`.
    ///
    /// **`0` is a legal value for every one of them** — an adapter may
    /// genuinely have no optional features — so it is not a failure code, and
    /// these are only meaningful once [`__crcbl_web_gpu_probe_state`] has
    /// answered [`PROBE_GRANTED`](super::PROBE_GRANTED). Allocates nothing.
    fn granted_u32(read: impl FnOnce(&crcbl_hal::AdapterInfo) -> u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.granted().map_or(0, read),
            Err(_) => 0,
        })
    }

    /// Low 32 bits of the granted adapter's
    /// [`Features`](crcbl_hal::Features). `0` when nothing has been granted,
    /// which is also a legal value for it — read it only once
    /// [`__crcbl_web_gpu_probe_state`] has answered
    /// [`PROBE_GRANTED`](super::PROBE_GRANTED).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_features_lo() -> u32 {
        granted_u32(|info| info.caps.features.bits() as u32)
    }

    /// High 32 bits of the same word, on the same terms. Split because the whole
    /// of this ABI is `(i32, …) -> i32`; see the [module docs](super).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_features_hi() -> u32 {
        granted_u32(|info| (info.caps.features.bits() >> 32) as u32)
    }

    /// The granted adapter's
    /// [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d) —
    /// `maxTextureDimension2D` as the browser reported it. `0` when nothing has
    /// been granted, on the terms [`__crcbl_web_gpu_probe_features_lo`] states.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_max_image_2d() -> u32 {
        granted_u32(|info| info.caps.limits.max_image_2d)
    }

    /// Ask the page to make a surface out of one of its canvases.
    ///
    /// `1` when one [`CreateSurface`](crate::Command::CreateSurface) is on the
    /// stream; `0` when the probe is re-entered, or when another channel is
    /// already installed.
    ///
    /// **THE ONE EXPORT HERE WITH NO `state` BESIDE IT.** Its two neighbours ask
    /// a question and poll for the answer; this one only tells. `create_surface`
    /// makes no round trip — see the [module
    /// docs](super#the-surface-probe-is-one-export-and-that-is-the-commands-shape)
    /// — so `1` says the command was encoded and reached the shim's buffer, and
    /// nothing more. **Whether the page could resolve the canvas is the page's
    /// to report**, and it reports it by throwing out of the replay.
    ///
    /// `canvas_id` is a parameter rather than a constant of this module's
    /// because the value is the page's fact and not wasm's:
    /// [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget) is an integer key into
    /// the shell's JS-side canvas registry, and nothing here knows what the
    /// shell registered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface(canvas_id: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_surface(canvas_id)),
            Err(_) => 0,
        })
    }

    /// [`granted_u32`] for the device that opened, on the same terms: `0` is a
    /// legal value for each of these, so they are read only once
    /// [`__crcbl_web_gpu_probe_device_state`] has answered
    /// [`DEVICE_OPENED`](super::DEVICE_OPENED). Allocates nothing.
    fn opened_u32(read: impl FnOnce(crcbl_hal::DeviceCaps) -> u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.opened().map_or(0, read),
            Err(_) => 0,
        })
    }

    /// Low 32 bits of the **opened device's**
    /// [`Features`](crcbl_hal::Features) — what WebGPU granted, which is what
    /// was asked for and not everything the adapter had.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_features_lo() -> u32 {
        opened_u32(|caps| caps.features.bits() as u32)
    }

    /// High 32 bits of the same word.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_features_hi() -> u32 {
        opened_u32(|caps| (caps.features.bits() >> 32) as u32)
    }

    /// The opened device's
    /// [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d) — the limit the
    /// *device* was created with, which is the specification's default unless
    /// something asked for more, and therefore not the adapter's ceiling.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_max_image_2d() -> u32 {
        opened_u32(|caps| caps.limits.max_image_2d)
    }
}

#[cfg(test)]
mod tests {
    use crcbl_hal::{AdapterInfo, BackendKind, DeviceCaps, DeviceType, Limits};

    use super::shim::{
        __crcbl_web_gpu_probe_adapters, __crcbl_web_gpu_probe_device,
        __crcbl_web_gpu_probe_device_features_hi, __crcbl_web_gpu_probe_device_features_lo,
        __crcbl_web_gpu_probe_device_max_image_2d, __crcbl_web_gpu_probe_device_reason_len,
        __crcbl_web_gpu_probe_device_reason_ptr, __crcbl_web_gpu_probe_device_state,
        __crcbl_web_gpu_probe_features_hi, __crcbl_web_gpu_probe_features_lo,
        __crcbl_web_gpu_probe_max_image_2d, __crcbl_web_gpu_probe_state,
        __crcbl_web_gpu_probe_surface, __crcbl_web_gpu_probe_text_len,
        __crcbl_web_gpu_probe_text_ptr,
    };
    use super::*;
    use crate::web::shim::{
        __crcbl_web_gpu_reply_buffer, __crcbl_web_gpu_reply_commit, __crcbl_web_gpu_stream_len,
        __crcbl_web_gpu_stream_ptr, __crcbl_web_gpu_stream_release,
    };
    use crate::{Command, ReplyWriter, decode_stream, tag};

    /// The text the last `state` call left, read the way JS reads it.
    fn text() -> String {
        let len = __crcbl_web_gpu_probe_text_len() as usize;
        let ptr = __crcbl_web_gpu_probe_text_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::text`, which nothing
        // between the two calls above can have moved — neither export allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's text is a Rust String")
    }

    /// The reason the last `device_state` call left, read the way JS reads it.
    fn device_reason() -> String {
        let len = __crcbl_web_gpu_probe_device_reason_len() as usize;
        let ptr = __crcbl_web_gpu_probe_device_reason_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::reason`, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's reason is a Rust String")
    }

    /// Reads the frame out through the transport exports, as the shim does, and
    /// returns what it decoded.
    fn take_frame() -> Vec<Command> {
        let len = __crcbl_web_gpu_stream_len() as usize;
        assert!(len >= tag::HEADER_BYTES, "no channel is installed");
        let ptr = __crcbl_web_gpu_stream_ptr();
        // SAFETY: the pair the two calls above just handed out, and nothing
        // encodes between them.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        let commands = decode_stream(bytes).expect("the writer's own bytes decode");
        assert_eq!(__crcbl_web_gpu_stream_release(), 1);
        commands
    }

    /// Hands `bytes` to wasm the way `putReplyStream` does.
    fn deliver(bytes: &[u8]) {
        let len = u32::try_from(bytes.len()).expect("a test buffer fits");
        let ptr = __crcbl_web_gpu_reply_buffer(len);
        assert!(!ptr.is_null(), "wasm would not take the replies");
        // SAFETY: `ptr` and `len` are what the call above just returned, and
        // nothing has called back into wasm since.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len as usize) };
        assert_eq!(__crcbl_web_gpu_reply_commit(len), 1);
    }

    /// An adapter with a feature set spanning both halves of the `u64` and a
    /// `max_image_2d` no default would produce, so the three numeric exports
    /// cannot pass by reading zero or by swapping their halves.
    fn granted(name: &str) -> AdapterInfo {
        AdapterInfo {
            id: AdapterId(0),
            name: name.into(),
            vendor_id: 0,
            device_id: 0,
            device_type: DeviceType::Other,
            driver: String::new(),
            backend: BackendKind::WebGpu,
            caps: DeviceCaps {
                // `COMPUTE` is bit 8, `RAY_QUERY` bit 24, `ACCELERATION_STRUCTURE`
                // bit 26 — all in the low word, which is why the corpus in
                // `tests/replies` carries the high-word case the enum cannot
                // reach yet.
                features: Features::COMPUTE | Features::RAY_QUERY,
                limits: Limits {
                    max_image_2d: 16384,
                    ..Limits::minimum()
                },
            },
        }
    }

    /// The whole exchange through the exports alone, which is what the browser
    /// gate does — with the replayer replaced by a `ReplyWriter`, because a
    /// `cargo test` has no `navigator.gpu` and that is the entire reason the
    /// browser gate exists.
    #[test]
    fn the_exports_carry_a_request_out_and_an_adapter_back() {
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_WAITING);

        // The sequence is not on the wire; it is the base plus the position,
        // and this frame holds exactly one command.
        let commands = take_frame();
        assert_eq!(commands, vec![Command::EnumerateAdapters]);

        let info = granted("Cherry MX Blue GPU");
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &info);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
        assert_eq!(text(), "Cherry MX Blue GPU");
    }

    /// **The capabilities reach the page too, not only the name.** This is the
    /// half the browser gate corroborates against `navigator.gpu`, so the three
    /// exports have to answer the adapter that was granted rather than zeros.
    #[test]
    fn the_numeric_exports_answer_the_granted_adapters_capabilities() {
        // Nothing granted yet: the documented `0`, which is also why these are
        // read only after `state` said `GRANTED`.
        assert_eq!(__crcbl_web_gpu_probe_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_max_image_2d(), 0);

        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let info = granted("capable");
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &info);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);

        let bits = info.caps.features.bits();
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_features_lo()),
            bits & 0xFFFF_FFFF
        );
        assert_eq!(u64::from(__crcbl_web_gpu_probe_features_hi()), bits >> 32);
        assert_eq!(
            __crcbl_web_gpu_probe_max_image_2d(),
            info.caps.limits.max_image_2d
        );
    }

    /// A refusal has no adapter, so the numbers must stay at their "nothing
    /// granted" value rather than keeping whatever a previous probe left.
    #[test]
    fn a_refusal_leaves_the_numeric_exports_at_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.no_adapter(0, "no GPU here");
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_REFUSED);
        assert_eq!(__crcbl_web_gpu_probe_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_max_image_2d(), 0);
    }

    #[test]
    fn a_browser_that_grants_nothing_comes_back_as_a_refusal_with_its_reason() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);

        let mut replies = ReplyWriter::new();
        replies.no_adapter(0, "requestAdapter() resolved null");
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_REFUSED);
        assert_eq!(text(), "requestAdapter() resolved null");
    }

    /// The state a drifted format lands in, and it must not read as a browser
    /// without a GPU: those blame opposite halves of the build.
    #[test]
    fn a_reply_answering_a_command_nobody_asked_is_undecodable_rather_than_refused() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);

        let mut replies = ReplyWriter::new();
        replies.adapter(9_999, &granted("an answer to nothing"));
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNDECODABLE);
        assert!(text().contains("9999"), "{}", text());
    }

    /// The capabilities an opened device answers with — deliberately *less*
    /// than [`granted`]'s adapter, which is what a WebGPU device is: the
    /// features that were asked for, and the specification's default limits.
    fn device_caps() -> DeviceCaps {
        DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits {
                max_image_2d: 8192,
                ..Limits::minimum()
            },
        }
    }

    /// Enumerates, grants `info`, and leaves the probe with an adapter.
    fn grant(info: &AdapterInfo) {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.adapter(0, info);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
    }

    /// The device half of the exchange, through the exports alone — the second
    /// round trip the browser gate drives.
    #[test]
    fn the_exports_carry_a_device_request_out_and_the_devices_own_capabilities_back() {
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        grant(&granted("Cherry MX Blue GPU"));

        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::RequestDevice {
                adapter: AdapterId(0),
                label: Some("crcbl-webgpu probe".into()),
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            }]
        );

        // Sequence 1: the enumeration spent 0.
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
        assert_eq!(device_reason(), "");
        let bits = device_caps().features.bits();
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_device_features_lo()),
            bits & 0xFFFF_FFFF
        );
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_device_features_hi()),
            bits >> 32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_max_image_2d(),
            device_caps().limits.max_image_2d
        );
    }

    /// **The device's numbers are not the adapter's**, and the two sets of
    /// exports must not read the same store. The corpus here is built so that
    /// every one of the three differs.
    #[test]
    fn the_device_exports_do_not_answer_with_the_adapters_capabilities() {
        let adapter = granted("capable");
        grant(&adapter);
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);

        assert_ne!(
            adapter.caps.features,
            device_caps().features,
            "the corpus would not notice a copy otherwise"
        );
        assert_ne!(
            adapter.caps.limits.max_image_2d,
            device_caps().limits.max_image_2d
        );
        assert_eq!(
            __crcbl_web_gpu_probe_features_lo(),
            adapter.caps.features.bits() as u32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_features_lo(),
            device_caps().features.bits() as u32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_max_image_2d(),
            adapter.caps.limits.max_image_2d
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_max_image_2d(),
            device_caps().limits.max_image_2d
        );
    }

    /// The descriptor names an adapter, so there has to be one. Nothing may be
    /// encoded before there is.
    #[test]
    fn a_device_request_before_an_adapter_is_granted_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_device(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        // Not even a channel: refusing before installing one is what keeps the
        // "another channel is installed" answer meaningful.
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        // …and it is still refused while the enumeration is in flight.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_device(), 0);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
    }

    /// A refusal carries what the browser said and leaves the numbers at their
    /// "nothing opened" value rather than at whatever a previous answer left.
    #[test]
    fn a_refused_device_request_reports_its_reason_and_no_capabilities() {
        grant(&granted("has an adapter, will not open it"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);

        let mut replies = ReplyWriter::new();
        replies.device_failed(
            1,
            "no WebGPU feature satisfies Features(TIMELINE_SEMAPHORE)",
            Features::TIMELINE_SEMAPHORE,
        );
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_FAILED);
        assert!(
            device_reason().contains("TIMELINE_SEMAPHORE"),
            "{}",
            device_reason()
        );
        assert_eq!(__crcbl_web_gpu_probe_device_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_max_image_2d(), 0);
    }

    /// **One buffer, two probes.** Both answers arrive in the same frame and
    /// whichever export is asked first is what decodes it — so the other's
    /// answer has to have been absorbed by then rather than dropped with the
    /// buffer.
    #[test]
    fn one_drain_hands_both_probes_their_own_answer() {
        // Ask for both before either is answered, so the two replies land
        // together.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &granted("both at once"));
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);

        // The device request first: a second enumeration puts the adapter probe
        // back to `Waiting`, and the descriptor has no adapter to name then.
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame().len(), 2);

        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        replies.adapter(2, &granted("answered second"));
        deliver(replies.bytes());

        // The device is asked first, so it is the call that drains. The
        // adapter's answer must survive that.
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
        assert_eq!(text(), "answered second");
    }

    /// A drifted format lands on whichever probe asked first, and must not read
    /// as a browser that refused a device.
    #[test]
    fn a_reply_answering_a_device_request_nobody_made_is_undecodable_rather_than_failed() {
        grant(&granted("adapter"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);

        let mut replies = ReplyWriter::new();
        replies.device(9_999, &device_caps());
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNDECODABLE);
        assert!(device_reason().contains("9999"), "{}", device_reason());
    }

    /// How many sequences the probe's own channel is still waiting on.
    ///
    /// Reached through the thread-local rather than through an export, because
    /// there is no export for it: what it is here to observe is a *negative* —
    /// see the test below.
    fn waiting_replies() -> usize {
        PROBE.with(|probe| {
            probe
                .borrow()
                .channel
                .as_ref()
                .map_or(0, |channel| channel.waiting_replies())
        })
    }

    /// The surface half, which is one export and one command: the page's canvas
    /// id goes out and the handle wasm named goes with it.
    #[test]
    fn the_surface_export_encodes_one_create_surface_naming_the_canvas() {
        assert_eq!(__crcbl_web_gpu_probe_surface(7), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateSurface {
                surface: PROBE_SURFACE,
                canvas_id: 7,
            }]
        );
    }

    /// **Nothing waits on it**, and that is the difference from its two
    /// neighbours rather than an omission: `create_surface` has no reply, so a
    /// registered wait would hold a slot in a bounded set for ever.
    #[test]
    fn the_surface_request_registers_no_wait_because_nothing_answers_it() {
        assert_eq!(__crcbl_web_gpu_probe_surface(7), 1);
        assert_eq!(waiting_replies(), 0);
        assert_eq!(take_frame().len(), 1);

        // The same channel, one command later, does register one — so the zero
        // above is this command's shape and not a counter that never moves.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(waiting_replies(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
    }

    /// It needs no adapter and no device, which is what lets the browser gate
    /// drive it as its own group: `Instance::create_surface` is an instance
    /// method, and the seam lets a caller make a surface before any device
    /// exists.
    #[test]
    fn a_surface_request_needs_neither_an_adapter_nor_a_device() {
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_surface(3), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateSurface {
                surface: PROBE_SURFACE,
                canvas_id: 3,
            }]
        );
    }

    /// The probe must not take a channel from an engine that has one, because
    /// replacing it would strand the frame the shim is part-way through.
    #[test]
    fn the_probe_refuses_when_another_channel_is_already_installed() {
        let engine = Rc::new(StreamChannel::new());
        assert!(install(&engine));
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 0);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
    }
}
