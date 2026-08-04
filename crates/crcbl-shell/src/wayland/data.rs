//! `data-device` state: the offers a peer made us, the selection we published,
//! and the drag currently over a window.
//!
//! The *protocol calls* stay in [`super`], next to the connection that owns
//! every proxy. What lives here is the bookkeeping between them — which is the
//! half that can be reasoned about, and tested, without a compositor.
//!
//! # One implementation, two triggers
//!
//! `docs/plan/15-windowing.md` states that clipboard and drag-and-drop "share
//! one implementation with two triggers" on Wayland and X11, and this module is
//! where that stops being an aspiration. A `wl_data_offer` reached by
//! `wl_data_device.selection` and one reached by `wl_data_device.enter` are the
//! same object with the same `receive` request; the only differences are what
//! starts the transfer (a [`clipboard_request`](crate::Shell::clipboard_request)
//! versus a `drop`) and what the bytes become at the end (a
//! [`ClipboardData`](crate::ShellEvent::ClipboardData) versus one
//! [`DroppedFile`](crate::ShellEvent::DroppedFile) per parsed URI). So [`Offer`]
//! serves both, and [`super::WaylandShell`] runs one transfer list.

use super::ffi::WlProxy;
use crate::{MimeType, PhysicalPoint, WindowId};

/// A `wl_data_offer` a peer made, and the formats it advertises.
///
/// The mime strings are the **peer's** spelling, kept verbatim: they are what
/// `wl_data_offer.receive` has to quote back, and
/// [`ReceivedMime`](crate::ReceivedMime) exists because they arrive at runtime
/// and are not this engine's to canonicalize.
#[derive(Debug)]
pub(super) struct Offer {
    /// The proxy, or null once it has been destroyed.
    pub proxy: *mut WlProxy,
    /// Every `wl_data_offer.offer` seen, in arrival order.
    pub mimes: Vec<String>,
}

impl Offer {
    /// The largest format list one offer will hold. Real offers carry fewer
    /// than a dozen formats; a hostile compositor can send `wl_data_offer.offer`
    /// without bound, and every mime is a `String` this client allocates.
    pub(super) const MAX_OFFER_MIMES: usize = 32;

    pub(super) const fn new(proxy: *mut WlProxy) -> Self {
        Self {
            proxy,
            mimes: Vec::new(),
        }
    }

    /// Records one announced format, dropping it once [`MAX_OFFER_MIMES`] is
    /// reached. Dropping rather than destroying the offer keeps a real offer
    /// claimable; it is only its format list that is capped.
    pub(super) fn note_mime(&mut self, mime: String) {
        if self.mimes.len() < Self::MAX_OFFER_MIMES {
            self.mimes.push(mime);
        }
    }

    /// The spelling this offer uses for `want`, if it has one.
    ///
    /// Two passes, and the order is the point. An exact match wins, so asking
    /// for `text/plain;charset=utf-8` from a peer that offers both spellings
    /// gets the one that was asked for. Otherwise any *equivalent* spelling
    /// will do — [`MimeType::recognize`] is what makes a peer offering bare
    /// `text/plain` (which X11 clients and GTK both do) satisfy a request for
    /// UTF-8 text, instead of the paste silently finding nothing.
    pub(super) fn pick(&self, want: MimeType) -> Option<&str> {
        let exact = self.mimes.iter().find(|mime| *mime == want.as_str());
        exact
            .or_else(|| {
                self.mimes
                    .iter()
                    .find(|mime| MimeType::recognize(mime) == Some(want))
            })
            .map(String::as_str)
    }
}

/// A drag currently held over this client.
#[derive(Debug)]
pub(super) struct Drag {
    /// The offer the compositor handed over with `wl_data_device.enter`.
    pub offer: Offer,
    /// The window under the pointer, or `None` when the drag is over a surface
    /// that did not opt into drops.
    ///
    /// `None` is not "we lost the window": it is the recorded decision to
    /// refuse, which is what makes
    /// [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops)
    /// load-bearing rather than advisory. The drag is still tracked, because
    /// the offer still has to be destroyed on `leave` either way.
    pub window: Option<WindowId>,
    /// Where in that window, in device pixels.
    pub position: PhysicalPoint,
    /// The mime we told the source we would accept, if any.
    ///
    /// `None` means we sent `accept(null)` — the source shows a "no drop"
    /// cursor and, per protocol, must not be asked for data afterwards.
    pub mime: Option<String>,
}

/// A selection this client published.
#[derive(Debug)]
pub(super) struct Source {
    /// The `wl_data_source`, or null once cancelled.
    pub proxy: *mut WlProxy,
    /// The bytes behind each offered format.
    ///
    /// Owned, not borrowed: [`ClipboardOffer`](crate::ClipboardOffer) hands
    /// over a `&[u8]` that lives for the call, while the compositor may ask for
    /// the bytes minutes later. Every platform's clipboard write is "claim the
    /// selection now, produce the bytes on demand", so the shell holds them —
    /// which is exactly what [`clipboard`](crate::clipboard) says the
    /// synchronous write means.
    pub payload: Vec<(String, Vec<u8>)>,
}

impl Source {
    /// The bytes to answer a `wl_data_source.send` for `mime` with.
    ///
    /// Matches the peer's spelling first and an equivalent one second, for the
    /// same reason [`Offer::pick`] does: a peer is entitled to ask for
    /// `text/plain` after we advertised `text/plain;charset=utf-8`, and
    /// answering an empty transfer would be indistinguishable from an empty
    /// clipboard.
    pub(super) fn bytes_for(&self, mime: &str) -> Option<&[u8]> {
        if let Some((_, bytes)) = self.payload.iter().find(|(offered, _)| offered == mime) {
            return Some(bytes);
        }
        let wanted = MimeType::recognize(mime)?;
        self.payload
            .iter()
            .find(|(offered, _)| MimeType::recognize(offered) == Some(wanted))
            .map(|(_, bytes)| bytes.as_slice())
    }
}

/// One `wl_data_device`, which is one seat's clipboard and drag endpoint.
#[derive(Debug)]
pub(super) struct Device {
    pub proxy: *mut WlProxy,
    /// Offers announced by `wl_data_device.data_offer` that no `selection` or
    /// `enter` has claimed yet.
    ///
    /// The protocol introduces the object first and says what it is for
    /// afterwards, with the `wl_data_offer.offer` events for its mime types in
    /// between. A list rather than a single slot because nothing in the
    /// protocol forbids two introductions before either is claimed.
    pub incoming: Vec<Offer>,
    /// The current clipboard offer, if this seat's keyboard focus has given us
    /// one.
    pub selection: Option<Offer>,
    /// The drag in progress over one of our surfaces.
    pub drag: Option<Drag>,
    /// The selection we published, if we own it.
    pub source: Option<Source>,
    /// Whether what [`selection`](Self::selection) says is *current*.
    ///
    /// # The focus gate, as a boolean
    ///
    /// `wl_data_device.selection` is sent only to the client that has keyboard
    /// focus on this seat, and it is sent again whenever focus arrives — so a
    /// client learns the clipboard on focus and its knowledge goes stale on
    /// focus loss, when another client may change the selection without us
    /// hearing about it.
    ///
    /// This is the difference between "the clipboard is empty" and "I have not
    /// been told what is on the clipboard", which the seam's obligation 5 turns
    /// on: `false` means a read must be **held**, not answered. Set by any
    /// `selection` event — including the null one that means the clipboard was
    /// cleared, which is a positive statement of emptiness — and cleared when
    /// this seat's keyboard focus leaves us.
    pub selection_seen: bool,
}

impl Device {
    /// The largest number of announced-but-unclaimed offers one `Device` will
    /// hold. The protocol permits several introductions before any is claimed,
    /// and real compositors hold at most a couple — so this is generous — but a
    /// hostile compositor can announce offers and never claim them, and
    /// `incoming` is the one quantity in this backend with no other bound.
    /// Beyond the cap the oldest pending offer is returned for the caller to
    /// destroy, which also bounds `Sink::objects` (every announced offer is
    /// watched there until destroyed).
    pub(super) const MAX_PENDING_OFFERS: usize = 8;

    pub(super) const fn new(proxy: *mut WlProxy) -> Self {
        Self {
            proxy,
            incoming: Vec::new(),
            selection: None,
            drag: None,
            source: None,
            selection_seen: false,
        }
    }

    /// Records an announced offer, returning the oldest pending one to destroy
    /// when the list is already at [`MAX_PENDING_OFFERS`].
    ///
    /// FIFO eviction: a real compositor claims offers in the order it announced
    /// them, so the oldest is the one most likely to have been superseded — and
    /// evicting the newest would make the claimed offer the one we destroyed.
    pub(super) fn push_incoming(&mut self, offer: Offer) -> Option<Offer> {
        if self.incoming.len() >= Self::MAX_PENDING_OFFERS {
            let evicted = self.incoming.remove(0);
            self.incoming.push(offer);
            return Some(evicted);
        }
        self.incoming.push(offer);
        None
    }

    /// Takes the announced offer named by `proxy` out of the pending list.
    ///
    /// `None` for the null proxy a `selection` event carries when the clipboard
    /// was cleared, and for an offer this client never saw announced — which is
    /// what a compositor sends after we destroyed one.
    pub(super) fn claim(&mut self, proxy: usize) -> Option<Offer> {
        if proxy == 0 {
            return None;
        }
        let index = self
            .incoming
            .iter()
            .position(|offer| offer.proxy as usize == proxy)?;
        Some(self.incoming.remove(index))
    }

    /// The announced offer named by `proxy`, for the `offer` events that arrive
    /// before it is claimed.
    pub(super) fn announced_mut(&mut self, proxy: usize) -> Option<&mut Offer> {
        // A claimed offer keeps receiving `offer` events on some compositors —
        // the protocol permits the mime list to be extended — so all three
        // places an offer can live are searched.
        if let Some(index) = self
            .incoming
            .iter()
            .position(|offer| offer.proxy as usize == proxy)
        {
            return self.incoming.get_mut(index);
        }
        if self
            .selection
            .as_ref()
            .is_some_and(|offer| offer.proxy as usize == proxy)
        {
            return self.selection.as_mut();
        }
        self.drag
            .as_mut()
            .map(|drag| &mut drag.offer)
            .filter(|offer| offer.proxy as usize == proxy)
    }
}

/// What a completed transfer turns into.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Delivery {
    /// The answer to a [`clipboard_request`](crate::Shell::clipboard_request).
    Clipboard {
        window: WindowId,
        request: crate::ClipboardRequestId,
        /// The peer's spelling of the format, which is what
        /// [`ClipboardData`](crate::ShellEvent::ClipboardData) carries.
        mime: crate::ReceivedMime,
    },
    /// A drop, which becomes one
    /// [`DroppedFile`](crate::ShellEvent::DroppedFile) per local path in the
    /// `text/uri-list`.
    Drop {
        window: WindowId,
        position: PhysicalPoint,
        /// The `wl_data_device` the drag arrived on.
        ///
        /// Recorded so that tearing that device down — an unplugged seat — can
        /// find the transfers it invalidates. Without it the offer below would
        /// outlive the device that owns it, and the `finish` at the end of the
        /// transfer would name an object the compositor has already reclaimed.
        device: usize,
        /// The offer to `finish` and destroy once the bytes are in.
        offer: usize,
    },
}

impl Delivery {
    /// The window whose event this becomes.
    pub(super) const fn window(&self) -> WindowId {
        match self {
            Self::Clipboard { window, .. } | Self::Drop { window, .. } => *window,
        }
    }

    /// The offer this transfer is reading from, for a drop.
    ///
    /// `None` for a clipboard read: that transfer's offer is the seat's
    /// selection, which the device still owns and will destroy itself.
    pub(super) const fn drop_offer(&self) -> Option<usize> {
        match self {
            Self::Drop { offer, .. } => Some(*offer),
            Self::Clipboard { .. } => None,
        }
    }
}

/// A pipe being drained, and what to do with the bytes.
#[derive(Debug)]
pub(super) struct Transfer {
    pub reading: super::fd::Reading,
    pub delivery: Delivery,
}

/// A clipboard read that has been accepted and cannot be answered yet.
///
/// The seam's obligation 5 made into state: the compositor has not told us what
/// is on the clipboard — because no window of ours has had keyboard focus since
/// the shell started, or since focus left — so the request waits rather than
/// being answered [`Empty`](crate::ClipboardContent::Empty), which would be
/// indistinguishable from a clipboard that really is empty.
#[derive(Clone, Debug)]
pub(super) struct HeldRead {
    pub window: WindowId,
    pub request: crate::ClipboardRequestId,
    pub mime: MimeType,
    /// `CLOCK_MONOTONIC` nanoseconds after which this is reported
    /// [`Unavailable`](crate::ClipboardContent::Unavailable).
    ///
    /// A read that waits forever is a UI stuck on "pasting…" forever, so the
    /// wait is bounded exactly as a transfer's is.
    pub deadline_nanos: u64,
}

/// What a seat currently knows about the clipboard, for one requested format.
#[derive(Debug)]
pub(super) enum Resolution {
    /// There is an offer and it has the format: read it.
    Ready {
        offer: *mut super::WlProxy,
        /// The peer's spelling, which `receive` has to quote back.
        spelling: String,
    },
    /// The clipboard is known, and holds nothing in this format.
    Empty,
    /// Nothing is known yet — hold the request. See
    /// [`Device::selection_seen`].
    Unknown,
}

/// `wl_data_device_manager.dnd_action`, as this backend uses it.
///
/// Only `copy` is ever asked for or accepted. `move` would mean promising the
/// source that its own copy may be deleted, which an engine dropping an asset
/// into a scene must never say, and `ask` requires a menu the shell has no way
/// to draw.
pub(super) const ACTION_COPY: u32 = 1;

#[cfg(test)]
mod tests {
    use core::ptr;

    use super::*;

    fn offer_with(mimes: &[&str]) -> Offer {
        Offer {
            proxy: ptr::null_mut(),
            mimes: mimes.iter().map(|mime| (*mime).to_string()).collect(),
        }
    }

    #[test]
    fn an_exact_spelling_wins_and_an_equivalent_one_still_matches() {
        // A peer that offers both must be asked for the one we named.
        let both = offer_with(&["text/plain", "text/plain;charset=utf-8"]);
        assert_eq!(
            both.pick(MimeType::TextUtf8),
            Some("text/plain;charset=utf-8")
        );

        // The case `MimeType::recognize` exists for, and the one that makes a
        // paste from GTK or from an X11 client work at all: only the bare
        // spelling is offered.
        let bare = offer_with(&["TARGETS", "text/plain", "text/html"]);
        assert_eq!(
            bare.pick(MimeType::TextUtf8),
            Some("text/plain"),
            "asking for utf-8 text must accept the peer's spelling of it"
        );
        assert_eq!(bare.pick(MimeType::CrcblRon), None);
        assert_eq!(bare.pick(MimeType::UriList), None);

        // A format the engine has no variant for is matched literally.
        let foreign = offer_with(&["application/vnd.some-editor.scene+json"]);
        assert_eq!(
            foreign.pick(MimeType::Other("application/vnd.some-editor.scene+json")),
            Some("application/vnd.some-editor.scene+json")
        );
        assert_eq!(foreign.pick(MimeType::Other("image/png")), None);
        assert_eq!(offer_with(&[]).pick(MimeType::TextUtf8), None);
    }

    #[test]
    fn a_source_answers_a_peers_spelling_of_a_format_it_published() {
        let source = Source {
            proxy: ptr::null_mut(),
            payload: vec![
                (MimeType::TextUtf8.as_str().to_string(), b"plain".to_vec()),
                (
                    MimeType::CrcblRon.as_str().to_string(),
                    b"(node:1)".to_vec(),
                ),
            ],
        };
        assert_eq!(
            source.bytes_for("text/plain;charset=utf-8"),
            Some(&b"plain"[..])
        );
        assert_eq!(
            source.bytes_for("text/plain"),
            Some(&b"plain"[..]),
            "a peer may ask for the spelling it knows, not the one we advertised"
        );
        assert_eq!(source.bytes_for("UTF8_STRING"), Some(&b"plain"[..]));
        assert_eq!(
            source.bytes_for("application/x-crcbl+ron"),
            Some(&b"(node:1)"[..])
        );
        // A format that was never offered gets nothing, not the first entry.
        assert_eq!(source.bytes_for("image/png"), None);
    }

    #[test]
    fn an_announced_offer_is_claimed_exactly_once() {
        let mut device = Device::new(ptr::null_mut());
        // Two proxies, distinguished only by address, which is how the
        // dispatcher names them.
        let first = 0x1000 as *mut WlProxy;
        let second = 0x2000 as *mut WlProxy;
        device.incoming.push(Offer::new(first));
        device.incoming.push(Offer::new(second));

        device
            .announced_mut(first as usize)
            .expect("announced")
            .mimes
            .push("text/plain".to_string());
        let claimed = device.claim(first as usize).expect("claimed");
        assert_eq!(claimed.mimes, ["text/plain"]);
        assert!(
            device.claim(first as usize).is_none(),
            "claiming twice would destroy an offer somebody else owns"
        );
        assert_eq!(device.incoming.len(), 1);

        // The null proxy of a cleared selection is not an offer at all.
        assert!(device.claim(0).is_none());

        // A claimed offer is still reachable for late `offer` events.
        device.selection = Some(claimed);
        assert!(device.announced_mut(first as usize).is_some());
        assert!(device.announced_mut(0xdead).is_none());
    }

    #[test]
    fn push_incoming_evicts_the_oldest_offer_at_the_cap() {
        let mut device = Device::new(ptr::null_mut());
        let proxies: Vec<*mut WlProxy> = (0..Device::MAX_PENDING_OFFERS)
            .map(|i| ((i + 1) * 0x1000) as *mut WlProxy)
            .collect();
        for &proxy in &proxies {
            device.push_incoming(Offer::new(proxy));
        }
        // The call past the cap returns the first offer pushed, for the caller
        // to destroy.
        let evicted = device
            .push_incoming(Offer::new(0xdead as *mut WlProxy))
            .expect("past the cap");
        assert_eq!(evicted.proxy, proxies[0]);
        assert_eq!(device.incoming.len(), Device::MAX_PENDING_OFFERS);
        // The evicted offer is gone — claiming it would destroy one somebody
        // else owns — while the next-oldest still claims.
        assert!(device.claim(proxies[0] as usize).is_none());
        assert!(device.claim(proxies[1] as usize).is_some());
    }

    #[test]
    fn note_mime_caps_the_format_list() {
        let mut offer = Offer::new(ptr::null_mut());
        let mimes: Vec<String> = (0..Offer::MAX_OFFER_MIMES + 8)
            .map(|i| format!("text/plain;charset=utf-8;part={i}"))
            .collect();
        for mime in &mimes {
            offer.note_mime(mime.clone());
        }
        assert_eq!(offer.mimes.len(), Offer::MAX_OFFER_MIMES);
        assert_eq!(offer.mimes, mimes[..Offer::MAX_OFFER_MIMES]);
    }
}
