//! XDND, the receiving half: a file dragged out of another application becomes
//! [`ShellEvent::DroppedFile`].
//!
//! # What this implements, and what it deliberately does not
//!
//! **Receiving only.** These windows are drop *targets*; nothing here starts a
//! drag. `docs/plan/15-windowing.md` scopes drag-and-drop to "file paths in
//! (viewer/editor import)", and a source is the other half of a protocol nobody
//! in this engine asks for — it needs a pointer grab, a drag icon, an action
//! menu and an offer this seam has no vocabulary to express.
//!
//! # Decision: version 5, refusing anything below 3
//!
//! [`VERSION`] is what `XdndAware` publishes: 5, which is the current revision
//! of the specification and the version GTK and Qt both announce. The
//! negotiated version is `min(source, ours)` — [`negotiate`] — which is the
//! rule the specification puts on both ends, and the reason a version-3 source
//! still works against a version-5 target. The clamp runs in the other
//! direction too: a hypothetical version-6 source is spoken to as 5, because 5
//! is what this target knows.
//!
//! [`MIN_VERSION`] is the floor, and it is the floor because it is the oldest
//! version whose message layout this code is written against and tested at:
//! `XdndEnter` with up to three type atoms inline and a flag pointing at
//! `XdndTypeList` for the rest, `XdndPosition` carrying both the timestamp the
//! payload's `ConvertSelection` has to quote and an action word, and
//! `XdndStatus` answering with an action. An older source predates parts of
//! that layout, so a target that read those words anyway would be reading
//! whatever the source happened to leave in them.
//!
//! So below the floor the drop is **refused**, and refused loudly rather than
//! silently mishandled: one `XdndStatus` with the accept bit clear, which is a
//! protocol-legal answer at every version — the source shows a "no drop" cursor
//! and the user learns immediately — plus an `XdndFinished` if the source drops
//! regardless. Nothing hangs; see [`X11Shell::handle_xdnd_drop`].
//!
//! # Decision: `text/uri-list`, and only that
//!
//! [`DroppedFile`](ShellEvent::DroppedFile) carries a `PathBuf`, so a drag of
//! text or of an image is something this seam cannot report. The type is picked
//! out of `XdndEnter`'s list — the three inline atoms, or the source's
//! `XdndTypeList` property when there are more than three — and a drag with no
//! `text/uri-list` in it is refused at `XdndPosition` rather than accepted and
//! dropped on the floor. The Wayland backend refuses the same drag at
//! `wl_data_device.enter` for the same reason.
//!
//! The bytes are decoded by [`parse_uri_list`](crate::parse_uri_list), which
//! all three backends share: percent-decoding and the `file://` prefix are
//! wrong in the same way on every platform, so they are wrong in one place or
//! in three.
//!
//! # Decision: the payload rides the clipboard's transfer machine
//!
//! A drop's data arrives through `ConvertSelection` on `XdndSelection` — the
//! same request, the same `SelectionNotify`, the same ICCCM `INCR` handshake
//! and the same stall deadline as a paste. So it is the same
//! [`Read`], distinguished by [`Delivery`]; see that type for why two state
//! machines would have been the wrong answer.
//!
//! # Decision: a zero-area rectangle, so every motion is reported
//!
//! `XdndStatus` answers with a rectangle the source may suppress further
//! `XdndPosition` messages inside. This target answers with a **zero-area** one
//! and sets the "send them anyway" bit, because `XdndDrop` carries no
//! coordinates at all: the position in [`DroppedFile`](ShellEvent::DroppedFile)
//! is the last `XdndPosition`, and a target that let the source go quiet would
//! report where the pointer entered the window rather than where the file was
//! let go. The cost is one client message per pointer motion during a drag,
//! which is the rate the pointer already generates.

use crate::{ClipboardContent, PhysicalPoint, ShellEvent, WindowId};

use super::{Delivery, Read, X11Shell, ffi};

/// The XDND version this target publishes in `XdndAware`.
///
/// The current revision of the specification, and the one GTK and Qt announce.
/// See the [module docs](self).
pub const VERSION: u8 = 5;

/// The oldest source version this target will talk to.
///
/// The oldest whose message layout this code reads and is tested at; see the
/// [module docs](self) for which fields that is about and why refusing an older
/// source is better than guessing at them.
pub const MIN_VERSION: u8 = 3;

/// `XdndStatus`'s "I will accept a drop here" bit.
const STATUS_ACCEPT: u32 = 1;

/// `XdndStatus`'s "send `XdndPosition` for every motion" bit.
///
/// Set always; see the [module docs](self) for the position this preserves.
const STATUS_WANT_POSITION: u32 = 1 << 1;

/// `XdndFinished`'s "the drop was taken" bit.
const FINISHED_ACCEPTED: u32 = 1;

/// The version `XdndFinished` grew its accepted flag and action word in.
///
/// Deliberately not [`VERSION`], which is the same number today and is a
/// different fact: this one is about the *message*, and stays where it is if
/// this target ever publishes 6. Below it both words are reserved and a source
/// requires them zero.
const FINISHED_FIELDS_FROM: u8 = 5;

/// The version a source announced, out of `XdndEnter`'s second word.
#[must_use]
pub const fn offered_version(word: u32) -> u8 {
    // The high byte. The rest of the word is flags, of which only bit 0 is
    // defined.
    (word >> 24) as u8
}

/// Whether the source's type list is too long to have fitted in `XdndEnter`.
///
/// Bit 0 of the same word. When it is set the full list is in the source
/// window's `XdndTypeList` property and the three inline atoms are only its
/// first three.
#[must_use]
pub const fn has_type_list(word: u32) -> bool {
    word & 1 != 0
}

/// The version to speak to a source that announced `offered`, or `None` to
/// refuse it.
///
/// `min(source, ours)`, which is what the specification requires of both ends,
/// with the floor the [module docs](self) argue for.
#[must_use]
pub const fn negotiate(offered: u8) -> Option<u8> {
    if offered < MIN_VERSION {
        return None;
    }
    Some(if offered < VERSION { offered } else { VERSION })
}

/// Two signed 16-bit coordinates packed into one message word, x first.
///
/// XDND's own encoding for both the pointer position and the status rectangle.
#[must_use]
pub const fn pack_point(x: i16, y: i16) -> u32 {
    ((x as u16 as u32) << 16) | (y as u16 as u32)
}

/// The inverse of [`pack_point`].
///
/// The halves are **signed**: a drag over a monitor left of or above the
/// primary one has negative root coordinates, and reading them as unsigned puts
/// the pointer 65 000 pixels away.
#[must_use]
pub const fn unpack_point(word: u32) -> (i16, i16) {
    (
        ((word >> 16) & 0xffff) as u16 as i16,
        (word & 0xffff) as u16 as i16,
    )
}

/// The three type atoms `XdndEnter` carries inline.
///
/// Zeros are padding for a source with fewer than three types, not atoms.
#[must_use]
pub fn inline_types(data: &[u32; 5]) -> Vec<u32> {
    data[2..5]
        .iter()
        .copied()
        .filter(|atom| *atom != 0)
        .collect()
}

/// One drag currently over this client, in whatever state it has reached.
///
/// **At most one.** A drag is driven by a pointer grab the source holds, and
/// there is one core pointer — so a second `XdndEnter` before a `XdndLeave` is
/// a protocol violation rather than a second drag, and replacing the state is
/// the only sane response to one.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    /// The source window. Every reply is addressed here, never to the window
    /// the messages arrive on.
    pub source: u32,
    /// The negotiated protocol version; see [`negotiate`].
    pub version: u8,
    /// The window under the drag, or `None` when there is not one that could
    /// take a drop — an unsupported version, a window that is not ours, or a
    /// window that did not ask for drops.
    pub window: Option<WindowId>,
    /// The atom the source spelled `text/uri-list` as, or `0` when the drag
    /// carries no format this seam can report.
    pub target: u32,
    /// The target window's origin on the root, latched once at `XdndEnter`.
    ///
    /// `XdndPosition` reports root coordinates and
    /// [`DroppedFile`](ShellEvent::DroppedFile) wants window-relative ones, so
    /// something has to translate — and `TranslateCoordinates` is a round trip.
    /// Taking it once per drag rather than once per motion is the difference
    /// between a drag that tracks the pointer and one that lags it; a window
    /// that moved mid-drag would offset the reported position, and a user with
    /// one pointer cannot drag a file and a window at the same time.
    pub origin: (i32, i32),
    /// The last position reported, in the window's device pixels.
    pub position: PhysicalPoint,
}

impl Drag {
    /// Whether a drop here would be taken.
    #[must_use]
    pub const fn accepts(&self) -> bool {
        self.window.is_some() && self.target != 0
    }
}

impl X11Shell {
    /// Publishes `XdndAware` on a window that asked for drops.
    ///
    /// **This property is the whole of the opt-in.** A source looks for it and
    /// addresses nothing else, so a window without it is never sent an
    /// `XdndEnter` and never sees a drop cursor —
    /// [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops) being off
    /// by default is enforced by simply not writing it, which is the same
    /// system-level gate `DragAcceptFiles` gives the Win32 backend rather than
    /// the recorded refusal the Wayland one has to keep.
    ///
    /// The value is one word holding [`VERSION`], typed `ATOM` — which it is
    /// not, and the specification says so anyway; every implementation writes
    /// and reads it that way and one that used `CARDINAL` would be unreadable
    /// by all of them.
    pub(super) fn set_xdnd_aware(&self, xid: u32) {
        let version = [u32::from(VERSION)];
        self.conn.set_property(
            xid,
            self.conn.atoms.xdnd_aware,
            ffi::value::ATOM_ATOM,
            32,
            &super::window::words_to_bytes(&version),
        );
    }

    /// Routes one XDND client message, or answers `false` if it is not one.
    pub(super) fn handle_xdnd_message(&mut self, event: &ffi::ClientMessageEvent) -> bool {
        let atoms = self.conn.atoms;
        if event.type_ == atoms.xdnd_enter {
            self.handle_xdnd_enter(event);
        } else if event.type_ == atoms.xdnd_position {
            self.handle_xdnd_position(event);
        } else if event.type_ == atoms.xdnd_leave {
            self.handle_xdnd_leave(event);
        } else if event.type_ == atoms.xdnd_drop {
            self.handle_xdnd_drop(event);
        } else {
            return false;
        }
        true
    }

    /// A drag entered one of our windows.
    ///
    /// Everything that decides whether the drop can be taken is known here —
    /// the version, the window, and the types — so this is where the answer is
    /// worked out; `XdndPosition` only reports it, once per motion.
    fn handle_xdnd_enter(&mut self, event: &ffi::ClientMessageEvent) {
        let source = event.data[0];
        let offered = offered_version(event.data[1]);
        let Some(version) = negotiate(offered) else {
            crcbl_core::log::warn!(
                "an XDND source announced version {offered}, below the {MIN_VERSION} this \
                 backend speaks; the drop will be refused"
            );
            self.drag = Some(Drag {
                source,
                // Clamped rather than kept: every reply below version 5 has its
                // extra words zeroed anyway, so this only has to be a version
                // this code can reason about.
                version: MIN_VERSION,
                window: None,
                target: 0,
                origin: (0, 0),
                position: PhysicalPoint::ORIGIN,
            });
            return;
        };

        // A window that did not ask for drops is not a drop target, and one
        // that is not ours at all is a message we should never have received.
        let window = self
            .window_by_xid(event.window)
            .filter(|window| self.window(*window).is_ok_and(|state| state.accept_drops));

        // The types: three inline, or the source's own property when it has
        // more than three. Read from the *source* window, which is another
        // client's — an ordinary `GetProperty`, and the one round trip this
        // handler takes.
        let offered_types = if has_type_list(event.data[1]) {
            self.conn
                .get_property_words(source, self.conn.atoms.xdnd_type_list)
        } else {
            inline_types(&event.data)
        };
        let uri_list = self.conn.atoms.mime_uri_list;
        let target = if window.is_some() && offered_types.contains(&uri_list) {
            uri_list
        } else {
            0
        };

        let origin = window
            .and_then(|window| self.window(window).ok())
            .and_then(|state| self.origin_now(state.id))
            .unwrap_or((0, 0));

        self.drag = Some(Drag {
            source,
            version,
            window,
            target,
            origin,
            position: PhysicalPoint::ORIGIN,
        });
    }

    /// The pointer moved while the drag is over us.
    ///
    /// Answered every time, because a source that hears nothing treats the
    /// target as busy and shows no drop cursor at all.
    fn handle_xdnd_position(&mut self, event: &ffi::ClientMessageEvent) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if drag.source != event.data[0] {
            return;
        }
        let (root_x, root_y) = unpack_point(event.data[2]);
        drag.position = PhysicalPoint::new(
            f64::from(i32::from(root_x) - drag.origin.0),
            f64::from(i32::from(root_y) - drag.origin.1),
        );
        let (source, accept) = (drag.source, drag.accepts());
        self.send_status(source, event.window, accept);
    }

    /// The drag left, without dropping. Nothing is owed but forgetting it.
    fn handle_xdnd_leave(&mut self, event: &ffi::ClientMessageEvent) {
        if self.drag.is_some_and(|drag| drag.source == event.data[0]) {
            self.drag = None;
        }
    }

    /// The button came up. Fetches the payload, or declines and says so.
    ///
    /// **An `XdndFinished` is owed on every path.** A source blocks its own
    /// drag loop until it arrives — that is how it knows when it may release
    /// the data it was offering — so a target that stays silent on a refusal
    /// leaves the other application wedged until its own timeout. Every branch
    /// below either sends one now or hands one to
    /// [`finish_drop`](Self::finish_drop), and the one way an accepted drop can
    /// end without reaching either — its window being destroyed mid-transfer —
    /// goes through [`abandon_drop`](Self::abandon_drop).
    fn handle_xdnd_drop(&mut self, event: &ffi::ClientMessageEvent) {
        let Some(drag) = self.drag.take() else {
            // A drop with no enter: nothing was ever accepted, and the source
            // is still owed an answer. Version 0 fields, which every version
            // reads as "not taken".
            self.send_finished(event.data[0], event.window, MIN_VERSION, false);
            return;
        };
        if drag.source != event.data[0] {
            // Not the drag we are tracking. Answer the *sender*, and keep the
            // one we have.
            self.send_finished(event.data[0], event.window, MIN_VERSION, false);
            self.drag = Some(drag);
            return;
        }
        let (Some(window), true) = (drag.window, drag.accepts()) else {
            self.send_finished(drag.source, event.window, drag.version, false);
            return;
        };
        let Ok(xid) = self.window(window).map(|state| state.id) else {
            self.send_finished(drag.source, event.window, drag.version, false);
            return;
        };

        // The specification requires the conversion to quote *this* timestamp:
        // the source keeps its offer alive against the moment of the drop, and
        // a conversion stamped with anything else may be refused as stale.
        //
        // It is **not** latched into `last_server_time`, which every other
        // handler does with the stamp its event carried. Those stamps are the
        // server's; this one is a number another client wrote into a message,
        // and `last_server_time` is what a `SetSelectionOwner` quotes — where a
        // value from the future silently takes the clipboard from whoever
        // holds it.
        let time = event.data[2];
        let stamp = self.time.event_time(time);
        let property = self.conn.atoms.crcbl_xdnd;
        let selection = self.conn.atoms.xdnd_selection;
        self.convert(xid, selection, drag.target, property, time);
        self.reads.push(Read::for_drop(
            window,
            drag.target,
            selection,
            property,
            Delivery::Drop {
                source: drag.source,
                version: drag.version,
                position: drag.position,
                time: stamp,
            },
            ffi::monotonic_nanos(),
        ));
    }

    /// Turns a finished drop transfer into events, and answers the source.
    ///
    /// One [`DroppedFile`](ShellEvent::DroppedFile) per local path, which is
    /// what that event specifies and what both other backends emit; a URI that
    /// is not a local file produces none.
    ///
    /// The accepted flag reports whether the **transfer** succeeded, not how
    /// many paths came out of it. A source that offered a `text/uri-list` of
    /// nothing but `https:` URLs was answered correctly and told so; telling it
    /// the drop failed would invite a source that had offered a move to put the
    /// file back.
    pub(super) fn finish_drop(
        &mut self,
        window: WindowId,
        delivery: &Delivery,
        content: &ClipboardContent,
    ) {
        let Delivery::Drop {
            source,
            version,
            position,
            time,
        } = *delivery
        else {
            return;
        };
        let xid = self.window(window).map_or(0, |state| state.id);
        if let ClipboardContent::Bytes(bytes) = content {
            for path in crate::parse_uri_list(bytes) {
                self.queue.push_back(ShellEvent::DroppedFile {
                    window,
                    time,
                    path,
                    position: Some(position),
                });
            }
        }
        self.send_finished(
            source,
            xid,
            version,
            matches!(content, ClipboardContent::Bytes(_)),
        );
        if xid != 0 {
            self.conn.delete_property(xid, self.conn.atoms.crcbl_xdnd);
            self.conn.flush();
        }
    }

    /// Releases a source whose drop can no longer be answered.
    ///
    /// Called for every read a destroyed window was carrying; the clipboard
    /// ones have nobody waiting, so they are a no-op here.
    pub(super) fn abandon_drop(&self, xid: u32, delivery: &Delivery) {
        let Delivery::Drop {
            source, version, ..
        } = *delivery
        else {
            return;
        };
        self.send_finished(source, xid, version, false);
    }

    /// Forgets a drag over a window that has gone away.
    ///
    /// The same obligation `forget_selection_state` discharges for a read: a
    /// drop answered after [`WindowDestroyed`](ShellEvent::WindowDestroyed)
    /// would name a handle the consumer has been told is stale.
    pub(super) fn forget_drag(&mut self, window: WindowId) {
        if self.drag.is_some_and(|drag| drag.window == Some(window)) {
            self.drag = None;
        }
    }

    /// Answers an `XdndPosition`.
    fn send_status(&self, source: u32, xid: u32, accept: bool) {
        let mut flags = STATUS_WANT_POSITION;
        let mut action = 0;
        if accept {
            flags |= STATUS_ACCEPT;
            // Copy, and only copy. This engine reads the path a source handed
            // it; it never takes ownership of the file, so answering `move`
            // would be a promise to delete something.
            action = self.conn.atoms.xdnd_action_copy;
        }
        // The two rectangle words, both zero: an empty rectangle the pointer
        // can never be inside, so the source keeps sending positions. See the
        // module docs for the position that preserves.
        let rectangle = pack_point(0, 0);
        self.send_xdnd(
            source,
            self.conn.atoms.xdnd_status,
            [xid, flags, rectangle, rectangle, action],
        );
    }

    /// Tells the source the drop is over, however it went.
    fn send_finished(&self, source: u32, xid: u32, version: u8, accepted: bool) {
        let (flags, action) = if version >= FINISHED_FIELDS_FROM && accepted {
            (FINISHED_ACCEPTED, self.conn.atoms.xdnd_action_copy)
        } else {
            (0, 0)
        };
        self.send_xdnd(
            source,
            self.conn.atoms.xdnd_finished,
            [xid, flags, action, 0, 0],
        );
    }

    /// Sends one XDND client message to the source.
    ///
    /// The event mask is `0`, which delivers to the client that created the
    /// window — the only routing that reaches a source whose window is
    /// unmapped, which a drag proxy's is.
    fn send_xdnd(&self, source: u32, type_: u32, data: [u32; 5]) {
        if source == 0 || type_ == 0 {
            return;
        }
        let message = ffi::ClientMessageEvent {
            response_type: ffi::event::CLIENT_MESSAGE,
            format: 32,
            sequence: 0,
            window: source,
            type_,
            data,
        };
        self.conn.send_event(source, 0, &message);
        self.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_announced_version_is_the_high_byte_and_the_flag_is_bit_zero() {
        // The one word that carries both, and reading it wrong is a target that
        // negotiates version 0 with every source there is.
        assert_eq!(offered_version(5 << 24), 5);
        assert_eq!(
            offered_version((5 << 24) | 1),
            5,
            "the flag is not the version"
        );
        assert!(has_type_list((5 << 24) | 1));
        assert!(!has_type_list(5 << 24));
    }

    #[test]
    fn a_version_is_clamped_down_and_never_up() {
        // `min(source, ours)` in both directions: an older source is spoken to
        // in its own version, and a newer one in ours.
        assert_eq!(negotiate(VERSION), Some(VERSION));
        assert_eq!(negotiate(3), Some(3));
        assert_eq!(negotiate(4), Some(4));
        assert_eq!(
            negotiate(9),
            Some(VERSION),
            "we do not speak a version we do not know"
        );
        // And below the floor there is no version to speak — see the module
        // docs for why guessing at `XdndPosition`'s missing words is worse.
        assert_eq!(negotiate(2), None);
        assert_eq!(negotiate(0), None);
    }

    #[test]
    fn a_packed_point_round_trips_and_keeps_its_sign() {
        // A drag over a monitor left of the primary one has negative root
        // coordinates. Read as unsigned they put the pointer 65 000 pixels off
        // the far side, which lands the drop outside every window.
        for point in [
            (0, 0),
            (1920, 1080),
            (-1, -1),
            (-1920, 5),
            (i16::MIN, i16::MAX),
        ] {
            assert_eq!(
                unpack_point(pack_point(point.0, point.1)),
                point,
                "{point:?}"
            );
        }
        // x is the *high* half; swapping them is a drop on the wrong axis.
        assert_eq!(pack_point(7, 9), (7 << 16) | 9);
    }

    #[test]
    fn the_inline_types_are_three_words_and_padding_is_not_an_atom() {
        // A source with one type leaves the other two words zero, and
        // `XCB_ATOM_NONE` is 0 — so a target that kept them would look for a
        // uri-list among atoms that do not exist.
        assert_eq!(inline_types(&[1, 2, 30, 31, 32]), vec![30, 31, 32]);
        assert_eq!(inline_types(&[1, 2, 30, 0, 0]), vec![30]);
        assert!(inline_types(&[1, 2, 0, 0, 0]).is_empty());
    }

    #[test]
    fn a_drag_with_no_uri_list_and_a_drag_on_no_window_both_refuse() {
        // `accepts` is what `XdndStatus` reports and what `XdndDrop` branches
        // on, so both halves of the refusal have to be in it.
        let window = {
            let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
            pool.insert(0).cast()
        };
        let base = Drag {
            source: 42,
            version: VERSION,
            window: Some(window),
            target: 77,
            origin: (0, 0),
            position: PhysicalPoint::ORIGIN,
        };
        assert!(base.accepts());
        assert!(
            !Drag { target: 0, ..base }.accepts(),
            "no text/uri-list on offer"
        );
        assert!(
            !Drag {
                window: None,
                ..base
            }
            .accepts(),
            "not a window of ours"
        );
    }
}
