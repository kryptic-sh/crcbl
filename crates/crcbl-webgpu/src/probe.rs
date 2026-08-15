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
//! its four exports with it — and it refuses rather than fights on the way,
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
//! | [`__crcbl_web_gpu_probe_features_lo`](shim::__crcbl_web_gpu_probe_features_lo) | `() -> i32` | Low 32 bits of the granted adapter's [`Features`](crcbl_hal::Features). |
//! | [`__crcbl_web_gpu_probe_features_hi`](shim::__crcbl_web_gpu_probe_features_hi) | `() -> i32` | High 32 bits of the same. |
//! | [`__crcbl_web_gpu_probe_max_image_2d`](shim::__crcbl_web_gpu_probe_max_image_2d) | `() -> i32` | The granted adapter's [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d). |
//!
//! **`state` before `ptr`, always** — the log queue's rule and for its reason:
//! `state` decodes a buffer and clones a string out of it, so it allocates, and
//! an allocation may grow wasm memory and detach a `Uint8Array` built before the
//! call. `ptr` and `len` allocate nothing, and neither do the three below them.
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
//! `i32` pairs rather than one `i64` because the whole of this ABI is
//! `(i32, …) -> i32`, which `docs/plan/41-webgpu-stream.md` sets as the
//! convention and which needs no `BigInt` on the JS side.
//!
//! `web/engine/gpu-probe.js` is the page's half, and
//! `web/tools/browser-e2e.mjs` is what drives it in a real browser.

use std::cell::RefCell;
use std::rc::Rc;

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

thread_local! {
    /// The probe's own channel and its state. Thread-local for
    /// [`crate::web`]'s reason: whichever thread the engine runs on is the one
    /// the shim calls.
    static PROBE: RefCell<Probe> = const { RefCell::new(Probe::new()) };
}

/// The channel the probe installed, the enumeration it is waiting on, and the
/// last thing it has to say.
#[derive(Debug)]
struct Probe {
    /// Held for as long as the probe exists, because
    /// [`install`] keeps only a [`Weak`](std::rc::Weak):
    /// dropping this is what puts every transport export back to `0`.
    channel: Option<Rc<StreamChannel>>,
    state: AdapterProbe,
    /// The adapter's name, the reason there is none, or a decode error.
    text: String,
}

impl Probe {
    const fn new() -> Self {
        Self {
            channel: None,
            state: AdapterProbe::Unasked,
            text: String::new(),
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

    /// Drain what JS has committed, absorb this probe's own answer, and report.
    fn state(&mut self) -> u32 {
        if let Some(channel) = self.channel.as_ref() {
            // `None` is the inbox being borrowed, which nothing here can cause;
            // it reads as "no replies this frame", which is also what an
            // ordinary frame answers.
            match channel.drain_replies() {
                Some(Ok(replies)) => {
                    self.state.absorb(&replies);
                }
                Some(Err(error)) => {
                    self.text = error.to_string();
                    return PROBE_UNDECODABLE;
                }
                None => {}
            }
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
}

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`. None of these is `unsafe`: none
/// dereferences a pointer the caller supplied.
pub mod shim {
    use super::{PROBE, PROBE_UNASKED};

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
}

#[cfg(test)]
mod tests {
    use crcbl_hal::{
        AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits,
    };

    use super::shim::{
        __crcbl_web_gpu_probe_adapters, __crcbl_web_gpu_probe_features_hi,
        __crcbl_web_gpu_probe_features_lo, __crcbl_web_gpu_probe_max_image_2d,
        __crcbl_web_gpu_probe_state, __crcbl_web_gpu_probe_text_len,
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
