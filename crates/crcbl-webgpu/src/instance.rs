//! Adapter enumeration over the stream: ask on one frame, answer on a later
//! one.
//!
//! [`Instance::adapters`](crcbl_hal::Instance::adapters) is a synchronous call
//! returning a `Vec`. Nothing on this seam can be: the command is written during
//! a frame and replayed when the frame ends, WebGPU's own
//! `navigator.gpu.requestAdapter()` is a promise on top of that, and the answer
//! comes back through the reply channel a frame or more later. [`AdapterProbe`]
//! is the state between those two moments — the sequence the request was
//! assigned, and what eventually named it.
//!
//! # The trait is still not here, and this is not a step towards faking it
//!
//! **There is no `impl Instance` in this crate**, and there deliberately is not.
//! What changed is the reason. The channel now carries the whole of
//! [`AdapterInfo`], so
//! [`AdapterProbe::adapters`] answers `Vec<AdapterInfo>` — the real return type
//! of [`Instance::adapters`](crcbl_hal::Instance::adapters) — rather than the
//! pairs it could honestly manage before. The trait's other five methods are
//! what is missing: `backend` is answerable, but `create_surface`,
//! `destroy_surface`, `surface_caps` and `request_device` have no commands and
//! no replies, and three of the four cannot return "not yet" — they return a
//! handle, a [`SurfaceCaps`](crcbl_hal::SurfaceCaps) or a
//! [`PendingDevice`](crcbl_hal::PendingDevice), and there is nothing truthful to
//! put in any of them. An impl now would be four stubs and one real method.
//!
//! # What this reports is what the *browser* said, not what this crate can do
//!
//! The [`DeviceCaps`](crcbl_hal::DeviceCaps) in an answered probe is
//! `web/engine/gpu-replay.js`'s reading of `adapter.features` and
//! `adapter.limits`. It is not a promise that this backend can execute those
//! capabilities: there is no `create_query_set` command yet, so a probe on a
//! browser with `timestamp-query` reports
//! [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) while
//! nothing here could serve it.
//!
//! That is the right split while there is no device — the wire's job is to
//! carry the browser's answer intact — but it stops being right the moment an
//! `impl Instance` exists, because a capability on the HAL seam is a promise
//! about what a caller may *ask for*. The impl must intersect this set with what
//! the stream can encode, the way `crcbl-wgpu`'s `hal_features_for` withholds
//! the query features its backend refuses.
//!
//! # Worked exchange
//!
//! ```
//! use std::rc::Rc;
//! use crcbl_hal::{AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits};
//! use crcbl_webgpu::instance::AdapterProbe;
//! use crcbl_webgpu::web::StreamChannel;
//! use crcbl_webgpu::{Command, ReplyWriter, decode_stream};
//!
//! let channel = Rc::new(StreamChannel::new());
//!
//! // The frame that asks. One call encodes the command and registers the wait.
//! let probe = AdapterProbe::request(&channel).expect("a fresh channel has room");
//! let sequence = probe.sequence().expect("a fresh request is waiting");
//!
//! // What JS replays, and what it answers with. The three fields WebGPU cannot
//! // supply are the values that mean absent, not plausible-looking numbers.
//! let commands = channel.encode(|stream| decode_stream(stream.bytes())).unwrap()?;
//! assert_eq!(commands, vec![Command::EnumerateAdapters]);
//! let granted = AdapterInfo {
//!     id: AdapterId(0),
//!     name: "Apple M2".into(),
//!     vendor_id: 0,
//!     device_id: 0,
//!     device_type: DeviceType::Other,
//!     driver: String::new(),
//!     backend: BackendKind::WebGpu,
//!     caps: DeviceCaps { features: Features::COMPUTE, limits: Limits::minimum() },
//! };
//! let mut replies = ReplyWriter::new();
//! replies.adapter(sequence, &granted);
//!
//! // A later frame, where the answer lands.
//! let mut probe = probe;
//! let decoded = crcbl_webgpu::decode_replies(replies.bytes())?;
//! assert!(probe.absorb(&decoded));
//! assert_eq!(probe.adapters(), vec![granted]);
//! # Ok::<(), crcbl_webgpu::DecodeError>(())
//! ```

use crcbl_hal::AdapterInfo;

use crate::reply::Reply;
use crate::web::StreamChannel;
use crate::writer::StreamWriter;

/// One adapter enumeration, from the frame that asked to the frame that was
/// answered.
///
/// A state rather than a future: there is no executor here and nothing to poll
/// against. The engine holds one of these, hands
/// [`absorb`](Self::absorb) whatever
/// [`drain_replies`](crate::web::StreamChannel::drain_replies) produced, and
/// reads the outcome when it has one.
///
/// **Not [`Eq`]**, for [`Reply`]'s reason: a granted probe holds
/// [`Limits`](crcbl_hal::Limits), and two of its fields are `f32`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AdapterProbe {
    /// Nothing has been asked. The state a probe is constructed in, and the one
    /// [`request`](Self::request) leaves it in if the channel had no room.
    #[default]
    Unasked,
    /// The command is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`EnumerateAdapters`](crate::Command::EnumerateAdapters)
        /// command, which is what the reply will name.
        sequence: u64,
    },
    /// The browser granted an adapter.
    Granted {
        /// Everything it said about it.
        ///
        /// [`id`](crcbl_hal::AdapterInfo::id) is always `0` in a browser: see
        /// [`adapters`](Self::adapters). [`name`](crcbl_hal::AdapterInfo::name)
        /// may be empty — a browser is allowed to grant an adapter and decline
        /// to name it, and the canonical corpus carries that case — and three
        /// more fields are always absent, which
        /// [`reply`](crate::reply) lists.
        info: AdapterInfo,
    },
    /// No adapter is coming.
    ///
    /// Ordinarily because the browser granted none — WebGPU present with no GPU
    /// behind it is a real machine, not a corner. It is also where a replayer
    /// that answered the enumeration with a reply that is not an enumeration
    /// answer lands, because the practical consequence is the same one and the
    /// reason says which happened.
    Refused {
        /// What the browser said, or what the replayer did instead. For a log
        /// or a banner; never a code to branch on.
        reason: String,
    },
}

impl AdapterProbe {
    /// Ask the browser what it will grant, on this frame's stream.
    ///
    /// `None` when the channel would not take the request — a bounded waiting
    /// set that is full, or a buffer already borrowed. Nothing is encoded then,
    /// which is the whole reason this goes through
    /// [`encode_awaited`](StreamChannel::encode_awaited): a command on the
    /// stream whose sequence nothing waits on turns the frame's *entire* reply
    /// buffer into a [`DecodeError::UnexpectedSequence`](crate::DecodeError).
    #[must_use]
    pub fn request(channel: &StreamChannel) -> Option<Self> {
        let sequence = channel.encode_awaited(StreamWriter::enumerate_adapters)?;
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
        matches!(self, Self::Granted { .. } | Self::Refused { .. })
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there.
    ///
    /// `true` when this call settled the probe. Everything not naming this
    /// probe's sequence is left alone rather than consumed: the reply buffer is
    /// the whole engine's, and a probe that swallowed a readback would be a
    /// dropped answer, which is a command that waits for ever.
    pub fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::Adapter { info } => Self::Granted { info: info.clone() },
            Reply::NoAdapter { reason } => Self::Refused {
                reason: reason.clone(),
            },
            // A reply of another shape naming this sequence is a bug in the
            // replayer rather than a browser without a GPU, and it is settled
            // rather than left waiting: nothing else is coming, because the
            // sequence has been answered and a second answer to it is refused.
            other => Self::Refused {
                reason: format!(
                    "the replayer answered the enumeration with {}",
                    other.name()
                ),
            },
        };
        true
    }

    /// What [`Instance::adapters`](crcbl_hal::Instance::adapters) would answer.
    ///
    /// The real return type now that the channel carries the whole of
    /// [`AdapterInfo`], rather than the pair of id and name it could honestly
    /// manage before. Empty while the request is in flight *and* when the
    /// browser refused, which are different facts:
    /// [`is_settled`](Self::is_settled) is what tells them apart.
    ///
    /// At most one entry, because WebGPU has no enumeration API to have more:
    /// `navigator.gpu.requestAdapter()` grants one adapter or none, and the id
    /// is its position in that list of one.
    ///
    /// Cloned rather than borrowed because that is what the trait returns, and
    /// because an enumeration happens once at startup — the same trade
    /// [`AdapterInfo`] itself documents for its two `String`s.
    #[must_use]
    pub fn adapters(&self) -> Vec<AdapterInfo> {
        match self {
            Self::Granted { info } => vec![info.clone()],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crcbl_hal::{AdapterId, BackendKind, DeviceCaps, DeviceType, Features, Limits};

    use super::*;
    use crate::{Command, decode_stream};

    fn channel() -> Rc<StreamChannel> {
        Rc::new(StreamChannel::new())
    }

    /// An adapter shaped the way the browser's replayer builds one: the three
    /// fields WebGPU cannot supply are the values that mean absent.
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
                features: Features::COMPUTE,
                limits: Limits::minimum(),
            },
        }
    }

    #[test]
    fn a_request_encodes_one_command_and_registers_exactly_one_wait() {
        let channel = channel();
        let probe = AdapterProbe::request(&channel).expect("a fresh channel has room");
        assert_eq!(channel.waiting_replies(), 1);
        let commands = channel
            .encode(|stream| decode_stream(stream.bytes()))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode");
        assert_eq!(commands, vec![Command::EnumerateAdapters]);
        assert_eq!(probe.sequence(), Some(0));
        assert!(!probe.is_settled());
        assert!(probe.adapters().is_empty());
    }

    #[test]
    fn an_adapter_reply_settles_the_probe_and_a_refusal_settles_it_the_other_way() {
        let channel = channel();
        let mut probe = AdapterProbe::request(&channel).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        let llvmpipe = granted("llvmpipe");
        assert!(probe.absorb(&[(
            sequence,
            Reply::Adapter {
                info: llvmpipe.clone(),
            },
        )]));
        assert!(probe.is_settled());
        assert_eq!(probe.adapters(), vec![llvmpipe]);

        let mut refused = AdapterProbe::request(&channel).expect("room");
        let sequence = refused.sequence().expect("a fresh request waits");
        assert!(refused.absorb(&[(
            sequence,
            Reply::NoAdapter {
                reason: "requestAdapter() resolved null".into(),
            },
        )]));
        assert!(refused.is_settled());
        // Settled and empty are different facts, and both are true here.
        assert!(refused.adapters().is_empty());
    }

    /// The reply buffer belongs to the whole engine, so a probe must take its
    /// own answer and leave the rest alone — including when its own answer is
    /// not in this frame's buffer at all.
    #[test]
    fn a_probe_ignores_replies_that_do_not_name_its_sequence() {
        let channel = channel();
        let mut probe = AdapterProbe::request(&channel).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        assert!(!probe.absorb(&[(
            sequence + 1,
            Reply::Adapter {
                info: granted("someone else's adapter"),
            },
        )]));
        assert_eq!(probe.sequence(), Some(sequence));
        assert!(!probe.is_settled());
    }

    /// A settled probe stays settled: the sequence has been answered and the
    /// channel refuses a second answer to it, so a later buffer naming it again
    /// must not be able to rewrite the outcome.
    #[test]
    fn a_settled_probe_does_not_absorb_again() {
        let channel = channel();
        let mut probe = AdapterProbe::request(&channel).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        probe.absorb(&[(
            sequence,
            Reply::Adapter {
                info: granted("first"),
            },
        )]);
        assert!(!probe.absorb(&[(
            sequence,
            Reply::Adapter {
                info: granted("second"),
            },
        )]));
        assert_eq!(probe.adapters(), vec![granted("first")]);
    }

    /// **The capabilities reach the caller, not just the name.** The whole of
    /// this slice is that a probe answers what a renderer selects its paths on,
    /// and a `Granted` that dropped `caps` on the way through `absorb` would
    /// satisfy every assertion above.
    #[test]
    fn a_granted_probe_carries_the_capabilities_the_reply_named() {
        let channel = channel();
        let mut probe = AdapterProbe::request(&channel).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");

        let mut info = granted("timestamping");
        info.caps.features = Features::COMPUTE | Features::TIMESTAMP_QUERY;
        info.caps.limits.max_image_2d = 16384;
        info.caps.limits.timestamp_period_ns = 1.0;
        assert!(probe.absorb(&[(sequence, Reply::Adapter { info: info.clone() })]));

        let adapters = probe.adapters();
        assert_eq!(adapters, vec![info]);
        let caps = adapters[0].caps;
        assert!(caps.supports(Features::TIMESTAMP_QUERY));
        assert_eq!(caps.limits.max_image_2d, 16384);
        // Derived from the features that crossed rather than stored, which is
        // the thing a renderer actually reads.
        assert_eq!(caps.binding_model(), crcbl_hal::BindingModel::ArrayPages);
        assert_eq!(
            caps.geometry_path(),
            crcbl_hal::GeometryPath::IndirectPerBatch
        );
    }

    /// The wrong reply shape is settled rather than left waiting for an answer
    /// that can no longer come, and the reason says what happened.
    #[test]
    fn a_reply_of_the_wrong_shape_settles_the_probe_with_a_reason_naming_it() {
        let channel = channel();
        let mut probe = AdapterProbe::request(&channel).expect("room");
        let sequence = probe.sequence().expect("a fresh request waits");
        assert!(probe.absorb(&[(
            sequence,
            Reply::ReadbackPending {
                readback: crcbl_core::Handle::from_bits((1 << 32) | 1).expect("a real handle"),
            },
        )]));
        let AdapterProbe::Refused { reason } = &probe else {
            panic!("a reply that cannot be an enumeration answer settles as a refusal");
        };
        assert!(reason.contains("ReadbackPending"), "{reason}");
    }

    /// The bound exists so a channel nobody is answering stops accepting
    /// requests rather than growing a set nothing drains.
    #[test]
    fn a_request_is_refused_once_the_waiting_set_is_full_and_encodes_nothing() {
        let channel = channel();
        for _ in 0..crate::tag::MAX_WAITING_REPLIES {
            assert!(AdapterProbe::request(&channel).is_some());
        }
        let full = channel
            .encode(|stream| stream.bytes().len())
            .expect("not borrowed");
        assert_eq!(AdapterProbe::request(&channel), None);
        assert_eq!(
            channel.encode(|stream| stream.bytes().len()),
            Some(full),
            "a refused request must not leave a command on the stream"
        );
    }
}
