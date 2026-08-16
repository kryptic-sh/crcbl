//! The device request over the stream: ask on one frame, open on a later one.
//!
//! [`Instance::request_device`](crcbl_hal::Instance::request_device) is already
//! the shape this seam needs — it starts the request and hands back a
//! [`PendingDevice`](crcbl_hal::PendingDevice) to poll — and that shape exists
//! *because* of this backend: a browser cannot block on `requestDevice`, which
//! resolves on a later turn of the event loop. So this is the call the polled
//! HAL was designed for, and [`DeviceProbe`] is the state between the frame that
//! asked and the frame that was answered.
//!
//! # The probe is the state `impl PendingDevice` polls
//!
//! [`WebGpuPendingDevice`](crate::hal::WebGpuPendingDevice) now exists, in
//! [`crate::hal`], and this probe is the state it holds:
//! [`PendingDevice::poll`](crcbl_hal::PendingDevice::poll) answers
//! [`DeviceRequestState`](crcbl_hal::DeviceRequestState), whose `Ready` arm
//! carries a `Box<dyn Device>` — the [`WebGpuDevice`](crate::hal::WebGpuDevice)
//! built around the caps [`Opened`](DeviceProbe::Opened) names. The state
//! machine stayed exposed here rather than folding into the impl, exactly as
//! [`AdapterProbe`](crate::instance::AdapterProbe) did: `poll` is
//! [`absorb`](DeviceProbe::absorb) plus a match on this enum, and holding the
//! two apart lets each be tested without the other.
//!
//! # The features the caller asked for, and what a browser can do with them
//!
//! [`DeviceDesc`] names two [`Features`] words and the
//! whole of both crosses the wire. **Nothing is filtered on this side**, because
//! WebGPU's vocabulary lives with the replayer — `web/engine/gpu-replay.js`
//! owns the `GPUFeatureName` ⇄ [`Features`] mapping in both directions, and a
//! second copy of it in Rust is a second thing to drift. What the replayer does
//! with each word is the difference between the two:
//!
//! * **`required_features`**: every bit must be satisfiable. Four of them are
//!   core WebGPU and need no name at all; four have a `GPUFeatureName`; the
//!   other nineteen have nothing behind them in WebGPU and **fail the request**,
//!   answered by a [`Reply::DeviceFailed`] whose
//!   [`unsupported`](Reply::DeviceFailed::unsupported) is exactly the gap.
//!   Dropping them instead would open a device missing something the caller said
//!   it could not run without — which is the one thing `required` means.
//! * **`optional_features`**: asked for where the adapter has them, silently
//!   absent where it does not. That is what optional means, and it is why the
//!   answer carries the device's own [`DeviceCaps`]: "check
//!   [`Device::caps`](crcbl_hal::Device::caps) afterwards to find out" is the
//!   HAL's own instruction.
//!
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) is worth
//! knowing about here: it requires
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE), and WebGPU
//! has no semaphores at all. The engine's default headless descriptor is
//! therefore one a browser refuses — loudly, naming that flag — rather than one
//! that opens a device pretending to have it.
//!
//! # A failed request is not a lost device
//!
//! [`Device::take_error`](crcbl_hal::Device::take_error) exists because WebGPU
//! reports failures out of band: an object is handed back and the reason arrives
//! later on the device's error channel. Two different events hide behind that,
//! and only the first is this slice's:
//!
//! | Event | WebGPU | Where it lands |
//! | --- | --- | --- |
//! | the request failed | `requestDevice()` rejects, or this backend refuses to ask | [`Reply::DeviceFailed`] → [`DeviceProbe::Failed`], and an `Err` from `poll` once the trait exists |
//! | a device was lost, or a call was invalid | `GPUDevice.lost`, `uncapturederror` | `take_error`, on a device that is open — **not built yet**, because nothing here holds a device |
//!
//! The second needs a reply of its own and a device to attach it to, and it is
//! what `docs/plan/41-webgpu-stream.md`'s error-attribution section is about.
//! Reporting a lost device as a failed request would be wrong in both
//! directions: the request succeeded, and the device that was lost had already
//! been used.
//!
//! # Worked exchange
//!
//! ```
//! use std::rc::Rc;
//! use crcbl_hal::{AdapterId, DeviceCaps, DeviceDesc, Features, Limits};
//! use crcbl_webgpu::device::DeviceProbe;
//! use crcbl_webgpu::web::StreamChannel;
//! use crcbl_webgpu::{Command, ReplyWriter, decode_stream};
//!
//! let channel = Rc::new(StreamChannel::new());
//!
//! // The frame that asks. Compute is core WebGPU, so this one is satisfiable;
//! // the optional word is what the device may or may not come back with.
//! let desc = DeviceDesc {
//!     label: Some("engine"),
//!     adapter: AdapterId(0),
//!     required_features: Features::COMPUTE,
//!     optional_features: Features::TIMESTAMP_QUERY,
//!     compatible_surface: None,
//! };
//! let probe = DeviceProbe::request(&channel, &desc).expect("a fresh channel has room");
//! let sequence = probe.sequence().expect("a fresh request is waiting");
//!
//! // What JS replays. The feature words are the caller's, unfiltered.
//! let commands = channel.encode(|stream| decode_stream(stream.bytes())).unwrap()?;
//! assert_eq!(commands, vec![Command::RequestDevice {
//!     adapter: AdapterId(0),
//!     label: Some("engine".into()),
//!     required_features: Features::COMPUTE,
//!     optional_features: Features::TIMESTAMP_QUERY,
//!     compatible_surface: None,
//! }]);
//!
//! // …and what it answers with, a frame or more later: the device's own
//! // capabilities, which are not the adapter's.
//! let granted = DeviceCaps {
//!     features: Features::COMPUTE | Features::TIMESTAMP_QUERY,
//!     limits: Limits::minimum(),
//! };
//! let mut replies = ReplyWriter::new();
//! replies.device(sequence, &granted);
//!
//! let mut probe = probe;
//! let decoded = crcbl_webgpu::decode_replies(replies.bytes())?;
//! assert!(probe.absorb(&decoded));
//! assert_eq!(probe.caps(), Some(granted));
//! # Ok::<(), crcbl_webgpu::DecodeError>(())
//! ```

use crcbl_hal::{DeviceCaps, DeviceDesc, Features};

use crate::reply::Reply;
use crate::web::StreamChannel;
use crate::writer::StreamWriter;

/// One device request, from the frame that asked to the frame that was
/// answered.
///
/// The counterpart of [`AdapterProbe`](crate::instance::AdapterProbe), and a
/// state rather than a future for its reason: there is no executor here. The
/// engine holds one, hands [`absorb`](Self::absorb) whatever
/// [`drain_replies`](crate::web::StreamChannel::drain_replies) produced, and
/// reads the outcome when it has one.
///
/// **Not [`Eq`]**, because [`Opened`](Self::Opened) holds
/// [`Limits`](crcbl_hal::Limits) and two of its fields are `f32`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum DeviceProbe {
    /// Nothing has been asked. The state a probe is constructed in, and the one
    /// [`request`](Self::request) leaves it in if the channel had no room.
    #[default]
    Unasked,
    /// The command is on the stream and its answer has not arrived.
    ///
    /// [`DeviceRequestState::Pending`](crcbl_hal::DeviceRequestState::Pending),
    /// and the ordinary state for every frame between the ask and the answer:
    /// `requestDevice` is a promise, and nothing on this seam can hurry it.
    Waiting {
        /// Sequence of the [`RequestDevice`](crate::Command::RequestDevice)
        /// command, which is what the reply will name.
        sequence: u64,
    },
    /// The browser opened a device.
    Opened {
        /// What *that device* can do — not what the adapter could. See
        /// [`Reply::Device`].
        caps: DeviceCaps,
    },
    /// No device is coming.
    Failed {
        /// What the browser said, or what this backend refused to ask for. For
        /// a log or a banner; never a code to branch on.
        reason: String,
        /// Which required features could not be satisfied, empty when the
        /// failure was not about features. The half of the answer that *is* for
        /// branching on, and what
        /// [`HalError::UnsupportedFeatures`](crcbl_hal::HalError::UnsupportedFeatures)
        /// would carry.
        unsupported: Features,
    },
}

impl DeviceProbe {
    /// Ask the browser to open `desc`, on this frame's stream.
    ///
    /// `None` when the channel would not take the request — a full waiting set,
    /// or a buffer already borrowed — and nothing is encoded then. Same
    /// contract, and the same reason, as
    /// [`AdapterProbe::request`](crate::instance::AdapterProbe::request): a
    /// command whose sequence nothing waits on turns the frame's *entire* reply
    /// buffer into a
    /// [`DecodeError::UnexpectedSequence`](crate::DecodeError::UnexpectedSequence).
    ///
    /// Nothing here validates `desc` against an adapter. It cannot: this side
    /// has no adapter to check against unless the caller kept one, and the
    /// features are checked where the WebGPU vocabulary lives — see the [module
    /// docs](self).
    #[must_use]
    pub fn request(channel: &StreamChannel, desc: &DeviceDesc<'_>) -> Option<Self> {
        let sequence =
            channel.encode_awaited(|stream: &mut StreamWriter| stream.request_device(desc))?;
        Some(Self::Waiting { sequence })
    }

    /// The sequence this is waiting on, or `None` if it is not waiting.
    #[must_use]
    pub const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Whether an answer has arrived — either way round.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        matches!(self, Self::Opened { .. } | Self::Failed { .. })
    }

    /// What the opened device can do, or `None` if none was opened.
    ///
    /// [`Device::caps`](crcbl_hal::Device::caps) as it will be answered once
    /// there is a device to answer it, and the value a caller reads to find out
    /// which of its optional features it actually got.
    #[must_use]
    pub const fn caps(&self) -> Option<DeviceCaps> {
        match self {
            Self::Opened { caps } => Some(*caps),
            _ => None,
        }
    }

    /// Why no device was opened, and which required features were missing.
    ///
    /// `None` while the request is unasked, waiting, or settled the other way.
    /// The pair an `Err` from [`PendingDevice::poll`](crcbl_hal::PendingDevice::poll)
    /// would be built from: an empty [`Features`] word is a failure that was not
    /// about features, which is
    /// [`HalError::DeviceLost`](crcbl_hal::HalError::DeviceLost) rather than
    /// [`HalError::UnsupportedFeatures`](crcbl_hal::HalError::UnsupportedFeatures).
    #[must_use]
    pub fn failure(&self) -> Option<(&str, Features)> {
        match self {
            Self::Failed {
                reason,
                unsupported,
            } => Some((reason, *unsupported)),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there.
    ///
    /// `true` when this call settled the probe. Everything not naming this
    /// probe's sequence is left alone rather than consumed —
    /// [`AdapterProbe::absorb`](crate::instance::AdapterProbe::absorb)'s rule,
    /// and it is what lets both probes read the same drained buffer.
    pub fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::Device { caps } => Self::Opened { caps: *caps },
            Reply::DeviceFailed {
                reason,
                unsupported,
            } => Self::Failed {
                reason: reason.clone(),
                unsupported: *unsupported,
            },
            // A reply of another shape naming this sequence is a bug in the
            // replayer, and it settles rather than waits: the sequence has been
            // answered, a second answer to it is refused, so nothing else is
            // coming.
            other => Self::Failed {
                reason: format!(
                    "the replayer answered the device request with {}",
                    other.name()
                ),
                unsupported: Features::empty(),
            },
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crcbl_hal::{AdapterId, Limits};

    use super::*;
    use crate::{Command, decode_stream};

    fn channel() -> Rc<StreamChannel> {
        Rc::new(StreamChannel::new())
    }

    /// A descriptor a browser can actually satisfy: `COMPUTE` is core WebGPU.
    fn desc() -> DeviceDesc<'static> {
        DeviceDesc {
            label: Some("engine"),
            adapter: AdapterId(0),
            required_features: Features::COMPUTE,
            optional_features: Features::TIMESTAMP_QUERY,
            compatible_surface: None,
        }
    }

    fn opened() -> DeviceCaps {
        DeviceCaps {
            features: Features::COMPUTE | Features::TIMESTAMP_QUERY,
            limits: Limits {
                max_image_2d: 8192,
                ..Limits::minimum()
            },
        }
    }

    #[test]
    fn a_request_encodes_the_whole_descriptor_and_registers_exactly_one_wait() {
        let channel = channel();
        let probe = DeviceProbe::request(&channel, &desc()).expect("a fresh channel has room");
        assert_eq!(channel.waiting_replies(), 1);
        let commands = channel
            .encode(|stream| decode_stream(stream.bytes()))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode");
        assert_eq!(
            commands,
            vec![Command::RequestDevice {
                adapter: AdapterId(0),
                label: Some("engine".into()),
                required_features: Features::COMPUTE,
                optional_features: Features::TIMESTAMP_QUERY,
                compatible_surface: None,
            }]
        );
        assert_eq!(probe.sequence(), Some(0));
        assert!(!probe.is_settled());
        assert_eq!(probe.caps(), None);
        assert_eq!(probe.failure(), None);
    }

    /// **The features a browser cannot satisfy are still what crosses.** The
    /// engine's own default descriptor is the case: filtering them here would
    /// turn a request the browser must refuse into one it silently grants.
    #[test]
    fn a_required_feature_webgpu_cannot_satisfy_is_encoded_rather_than_dropped() {
        let channel = channel();
        let desc = DeviceDesc::for_adapter(AdapterId(0));
        assert!(
            desc.required_features
                .contains(Features::TIMELINE_SEMAPHORE),
            "the default descriptor is what makes this case reachable"
        );
        DeviceProbe::request(&channel, &desc).expect("room");
        let commands = channel
            .encode(|stream| decode_stream(stream.bytes()))
            .expect("not borrowed")
            .expect("the writer's own bytes decode");
        let [
            Command::RequestDevice {
                required_features, ..
            },
        ] = commands.as_slice()
        else {
            panic!("one RequestDevice");
        };
        assert_eq!(*required_features, desc.required_features);
    }

    #[test]
    fn a_device_reply_settles_the_probe_and_a_failure_settles_it_the_other_way() {
        let channel = channel();
        let mut probe = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        assert!(probe.absorb(&[(sequence, Reply::Device { caps: opened() })]));
        assert!(probe.is_settled());
        assert_eq!(probe.caps(), Some(opened()));
        assert_eq!(probe.failure(), None);

        let mut refused = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = refused.sequence().expect("a fresh request waits");
        assert!(refused.absorb(&[(
            sequence,
            Reply::DeviceFailed {
                reason: "no WebGPU feature satisfies TIMELINE_SEMAPHORE".into(),
                unsupported: Features::TIMELINE_SEMAPHORE,
            },
        )]));
        assert!(refused.is_settled());
        assert_eq!(refused.caps(), None);
        assert_eq!(
            refused.failure().map(|(_, missing)| missing),
            Some(Features::TIMELINE_SEMAPHORE)
        );
    }

    /// **The device's capabilities are the device's**, and a probe that dropped
    /// them on the way through `absorb` would satisfy every assertion about
    /// settling.
    #[test]
    fn an_opened_probe_carries_the_capabilities_the_reply_named() {
        let channel = channel();
        let mut probe = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");

        let caps = DeviceCaps {
            features: Features::COMPUTE | Features::TEXTURE_COMPRESSION_BC,
            limits: Limits {
                max_image_2d: 4096,
                timestamp_period_ns: 0.0,
                ..Limits::minimum()
            },
        };
        assert!(probe.absorb(&[(sequence, Reply::Device { caps })]));
        let carried = probe.caps().expect("the probe opened");
        assert_eq!(carried, caps);
        assert!(carried.supports(Features::TEXTURE_COMPRESSION_BC));
        assert!(
            !carried.supports(Features::TIMESTAMP_QUERY),
            "an optional feature the device did not grant must not read as granted"
        );
        assert_eq!(carried.limits.max_image_2d, 4096);
    }

    #[test]
    fn a_probe_ignores_replies_that_do_not_name_its_sequence() {
        let channel = channel();
        let mut probe = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        assert!(!probe.absorb(&[(sequence + 1, Reply::Device { caps: opened() })]));
        assert_eq!(probe.sequence(), Some(sequence));
        assert!(!probe.is_settled());
    }

    #[test]
    fn a_settled_probe_does_not_absorb_again() {
        let channel = channel();
        let mut probe = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        probe.absorb(&[(sequence, Reply::Device { caps: opened() })]);
        assert!(!probe.absorb(&[(
            sequence,
            Reply::DeviceFailed {
                reason: "second thoughts".into(),
                unsupported: Features::empty(),
            },
        )]));
        assert_eq!(probe.caps(), Some(opened()));
    }

    #[test]
    fn a_reply_of_the_wrong_shape_settles_the_probe_with_a_reason_naming_it() {
        let channel = channel();
        let mut probe = DeviceProbe::request(&channel, &desc()).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        assert!(probe.absorb(&[(
            sequence,
            Reply::NoAdapter {
                reason: "not an answer to this".into(),
            },
        )]));
        let (reason, unsupported) = probe.failure().expect("settled as a failure");
        assert!(reason.contains("NoAdapter"), "{reason}");
        assert_eq!(
            unsupported,
            Features::empty(),
            "a replayer bug is not a feature gap"
        );
    }

    /// Both probes read the same drained buffer, so each must take its own
    /// answer and leave the other's alone — a frame that carried both would
    /// otherwise strand one of them.
    #[test]
    fn an_adapter_answer_and_a_device_answer_in_one_frame_reach_their_own_probes() {
        use crate::instance::AdapterProbe;

        let channel = channel();
        let mut adapters = AdapterProbe::request(&channel).expect("room");
        let mut device = DeviceProbe::request(&channel, &desc()).expect("room");
        let adapter_sequence = adapters.sequence().expect("waiting");
        let device_sequence = device.sequence().expect("waiting");
        assert_ne!(adapter_sequence, device_sequence);

        let frame = vec![
            (device_sequence, Reply::Device { caps: opened() }),
            (
                adapter_sequence,
                Reply::NoAdapter {
                    reason: "gone".into(),
                },
            ),
        ];
        assert!(adapters.absorb(&frame));
        assert!(device.absorb(&frame));
        assert!(adapters.is_settled());
        assert_eq!(device.caps(), Some(opened()));
    }

    #[test]
    fn a_request_is_refused_once_the_waiting_set_is_full_and_encodes_nothing() {
        let channel = channel();
        for _ in 0..crate::tag::MAX_WAITING_REPLIES {
            assert!(DeviceProbe::request(&channel, &desc()).is_some());
        }
        let full = channel
            .encode(|stream| stream.bytes().len())
            .expect("not borrowed");
        assert_eq!(DeviceProbe::request(&channel, &desc()), None);
        assert_eq!(
            channel.encode(|stream| stream.bytes().len()),
            Some(full),
            "a refused request must not leave a command on the stream"
        );
    }
}
