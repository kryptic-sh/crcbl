//! The clipboard: owning the `CLIPBOARD` selection, answering conversions, and
//! reading someone else's.
//!
//! [`selection`] owns the state machines and the reasoning;
//! this is the half that talks to the server. The division is deliberate — the
//! part that is hardest to get right is the part with no X server in it.
//!
//! # Obligations 4, 5 and 6, and which of them X11 got for free
//!
//! P0.5c predicted all three would cost X11 nothing. Two did:
//!
//! * **Obligation 5** — "a read that cannot be answered *yet* is held, never
//!   answered empty". X11 has no focus gate: `ConvertSelection` may be issued
//!   by any window at any time. And the "is there anything there at all"
//!   question has a *synchronous* answer, `GetSelectionOwner`, so
//!   [`clipboard_request`](crate::Shell::clipboard_request) never has to hold
//!   anything for want of knowing — it either answers
//!   [`Empty`](crate::ClipboardContent::Empty) on the spot or starts a real
//!   conversation. The obligation is discharged by answering, exactly as the
//!   trait's own wording anticipated.
//! * **Obligation 6** — "a refusal for want of user interaction is named".
//!   `SetSelectionOwner` takes a timestamp and no serial, and the server checks
//!   nothing about where that timestamp came from. There is no rule, so
//!   [`ShellError::NeedsUserInteraction`](crate::ShellError::NeedsUserInteraction)
//!   is never returned here — which the trait says is "obviously correct rather
//!   than obviously incomplete".
//!
//! **Obligation 4 did not.** See [`selection`] for the
//! deadline an `INCR` transfer needs and why there is nothing else that could
//! end one.
//!
//! # A claim needs a timestamp, and a client often has to manufacture one
//!
//! ICCCM requires `SetSelectionOwner` to quote a timestamp and forbids
//! `CurrentTime`, because two clients claiming the selection are ordered by
//! their timestamps and `CurrentTime` means "whenever the server got round to
//! it" — so a client using it can silently steal a selection a slower client
//! claimed first, and can never tell that it did.
//!
//! The rule with teeth is the *other* half, and it is what makes this
//! non-trivial: **the server ignores a `SetSelectionOwner` whose timestamp is
//! older than the current owner's.** Silently — there is no error and no
//! reply. So a client that quotes the last event it happened to receive loses
//! its own copy command whenever another application took the clipboard more
//! recently than that, which for a game that has been idle is *most of the
//! time*. P0.6 found this the hard way: the end-to-end test in which a peer
//! releases the selection and this backend then claims it failed roughly one
//! run in three, entirely on the ordering of two millisecond stamps.
//!
//! There is no request for "what time is it". The idiom every toolkit uses,
//! and what [`X11Shell::refresh_server_time`] does, is to **provoke** one: a
//! zero-length append to a property on our own window changes nothing and
//! generates a `PropertyNotify`, which the server stamps with its current time.
//! One round trip, taken only when the clipboard is being claimed, and
//! `CurrentTime` remains the fallback for a server that does not answer within
//! the deadline — where a claim that is ignored is worse than a claim that
//! races.

use super::{
    ClipboardContent, ClipboardRequestId, MimeType, Read, ReceivedMime, ShellEvent, Step, Write,
    X11Shell, ffi, read_wire, selection, window,
};

impl X11Shell {
    /// Someone else took the selection.
    ///
    /// Our offers are dead the instant this arrives — a `SelectionRequest` for
    /// them will never come again, and answering one we somehow still received
    /// would be answering for data the user has replaced.
    pub(super) fn handle_selection_clear(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionClearEvent>(raw) else {
            return;
        };
        if event.selection != self.conn.atoms.clipboard {
            return;
        }
        self.offers.clear();
        self.owner_window = 0;
        // Every `INCR` transfer we were feeding is now for data we no longer
        // own. Dropping them is correct: the peer sees the property stop
        // changing and times out, which is the same outcome as us disappearing.
        self.writes.clear();
    }

    /// Another client — or one of our own windows — wants our selection.
    ///
    /// This is the owner half of the conversation, and on X11 it can arrive on
    /// the very connection a read of ours is waiting on. Answering it is
    /// therefore never allowed to block; the only round trip below is a
    /// `ChangeProperty`, which does not have a reply.
    pub(super) fn handle_selection_request(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionRequestEvent>(raw) else {
            return;
        };
        let property = self.answer_selection_request(&event);
        // The reply is a `SelectionNotify` we construct and send ourselves —
        // there is no request for it. `property == 0` is the refusal, and the
        // requestor must get one either way: a peer that receives no answer at
        // all waits until *its* timeout, which is the failure this whole
        // protocol makes it easy to cause and hard to notice.
        let notify = ffi::SelectionNotifyEvent {
            response_type: ffi::event::SELECTION_NOTIFY,
            pad0: 0,
            sequence: 0,
            time: event.time,
            requestor: event.requestor,
            selection: event.selection,
            target: event.target,
            property,
            pad1: [0; 8],
        };
        self.conn.send_event(event.requestor, 0, &notify);
        self.conn.flush();
    }

    /// Writes the answer to a conversion request, and reports the property it
    /// went into — or `0` for "we cannot do that".
    fn answer_selection_request(&mut self, event: &ffi::SelectionRequestEvent) -> u32 {
        if event.selection != self.conn.atoms.clipboard || self.offers.is_empty() {
            return 0;
        }
        // A `property` of `None` is a pre-ICCCM client asking us to choose;
        // the convention is to use the target atom as the property name.
        let property = if event.property == 0 {
            event.target
        } else {
            event.property
        };

        if event.target == self.conn.atoms.timestamp {
            // ICCCM: the answer is the timestamp we *acquired* the selection
            // with, as a 32-bit `INTEGER`. Not the current server time — a peer
            // uses this to tell one ownership from the next.
            let acquired = [self.owner_time];
            self.conn.set_property(
                event.requestor,
                property,
                ffi::value::ATOM_INTEGER,
                32,
                &window::words_to_bytes(&acquired),
            );
            return property;
        }

        if event.target == self.conn.atoms.targets {
            let targets = self.advertised_targets();
            self.conn.set_property(
                event.requestor,
                property,
                ffi::value::ATOM_ATOM,
                32,
                &window::words_to_bytes(&targets),
            );
            return property;
        }

        let Some(mime) = self.conn.atoms.mime_for_target(event.target) else {
            return 0;
        };
        let Some((_, bytes)) = self.offers.iter().find(|(offered, _)| *offered == mime) else {
            return 0;
        };

        if bytes.len() <= self.conn.max_property_bytes {
            self.conn
                .set_property(event.requestor, property, event.target, 8, bytes);
            return property;
        }

        // `INCR`: the property is written with type `INCR` and a *size
        // estimate* as its value, and the data follows in chunks the requestor
        // pulls by deleting the property. Selecting `PropertyChange` on the
        // requestor's window is what lets us see those deletions — without it
        // the transfer starts and never advances.
        let estimate = [u32::try_from(bytes.len()).unwrap_or(u32::MAX)];
        self.conn.set_property(
            event.requestor,
            property,
            self.conn.atoms.incr,
            32,
            &window::words_to_bytes(&estimate),
        );
        self.select_property_changes(event.requestor);
        self.writes.push(Write::new(
            event.requestor,
            property,
            event.target,
            bytes.clone(),
            self.conn.max_property_bytes,
            ffi::monotonic_nanos(),
        ));
        property
    }

    /// The `TARGETS` list, in preference order.
    ///
    /// `TARGETS` itself is in it, which ICCCM requires and which half the
    /// clients in the wild forget: a peer enumerating what is on the clipboard
    /// looks for it, and a list that omits it reads as a client that does not
    /// support the query at all.
    fn advertised_targets(&self) -> Vec<u32> {
        // `TIMESTAMP` too: ICCCM requires *every* selection owner to answer it,
        // and a peer that uses it to detect an ownership change (which is what
        // it is for) reads an owner that omits it as broken.
        let mut targets = vec![self.conn.atoms.targets, self.conn.atoms.timestamp];
        for (mime, _) in &self.offers {
            if *mime == MimeType::TextUtf8 {
                // All four spellings, because an X11 peer may ask for any of
                // them and mean the same thing — see [`atoms`](super::atoms).
                targets.extend_from_slice(&self.conn.atoms.text_targets());
            } else {
                let atom = self.conn.atoms.for_mime(*mime);
                if atom != 0 {
                    targets.push(atom);
                }
            }
        }
        // `dedup` alone only removes *consecutive* duplicates, so two
        // `TextUtf8` offers put four non-adjacent repeats into `TARGETS`. The
        // order is the preference order a peer reads, so the first occurrence
        // is the one that stays.
        let mut unique = Vec::with_capacity(targets.len());
        for atom in targets {
            if atom != 0 && !unique.contains(&atom) {
                unique.push(atom);
            }
        }
        unique
    }

    /// The owner answered one of our `ConvertSelection`s.
    pub(super) fn handle_selection_notify(&mut self, raw: &[u8]) {
        let Some(event) = read_wire::<ffi::SelectionNotifyEvent>(raw) else {
            return;
        };
        // Which of the two conversions this answers. A read has exactly one
        // outstanding at a time, so the phase is enough to route it — and it
        // has to be the *phase* rather than the property, because both use the
        // same property by design.
        let is_targets = event.target == self.conn.atoms.targets;
        let Some(index) = self.reads.iter().position(|read| {
            self.windows
                .get(read.window.cast())
                .is_some_and(|window| window.id == event.requestor)
                && (read.state == selection::ReadState::AwaitingTargets) == is_targets
        }) else {
            return;
        };

        let now = ffi::monotonic_nanos();
        let xid = event.requestor;
        // A refused conversion writes no property, so there is nothing to read.
        let value = (event.property != 0)
            .then(|| self.conn.get_property(xid, event.property))
            .flatten()
            // A type-less read is the property having vanished between the
            // notify and the read — a peer that reset itself — which
            // `get_property` now returns as `Some((0, …))` rather than `None`.
            // Treat it as the absence the `None` arm below already handles.
            .filter(|(type_, _, _)| *type_ != 0);

        if is_targets {
            // The atom list, or empty for a peer with no `TARGETS` support —
            // which `on_targets` treats as "ask directly" rather than as a
            // failure.
            let offered: Vec<u32> = value.map_or_else(Vec::new, |(_, format, bytes)| {
                if format == 32 {
                    bytes
                        .chunks_exact(4)
                        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
                        .collect()
                } else {
                    Vec::new()
                }
            });
            let step = self.reads[index].on_targets(&offered, now);
            self.settle(index, step);
            return;
        }

        let (is_incr, bytes) = match value {
            Some((type_, _, bytes)) => (type_ == self.conn.atoms.incr, bytes),
            // Either the owner refused, or the property vanished between the
            // notify and the read — which a peer that reset itself can cause.
            // There is nothing to read and nothing coming.
            None => {
                let step = self.reads[index].on_selection_notify(0, event.target, false, &[], now);
                self.settle(index, step);
                return;
            }
        };
        let step = self.reads[index].on_selection_notify(
            event.property,
            event.target,
            is_incr,
            &bytes,
            now,
        );
        self.settle(index, step);
    }

    /// A property changed — the whole of an `INCR` transfer, in both
    /// directions.
    pub(super) fn handle_property_notify(&mut self, raw: &[u8]) {
        /// `XCB_PROPERTY_NEW_VALUE`.
        const NEW_VALUE: u8 = 0;
        /// `XCB_PROPERTY_DELETE`.
        const DELETED: u8 = 1;
        let Some(event) = read_wire::<ffi::PropertyNotifyEvent>(raw) else {
            return;
        };
        self.last_server_time = event.time;
        // The timestamp probe is the only writer of `CRCBL_TIMESTAMP`, so its
        // notify is the probe's answer — the signal `refresh_server_time` waits
        // for. It must not depend on the stamp: at ≥1 kHz event rates the
        // server may give the probe the same millisecond as the previous event,
        // and `last_server_time` would then never change.
        if event.atom == self.conn.atoms.crcbl_timestamp {
            self.timestamp_probe_seen = true;
        }

        // Not a selection at all: the window manager rewriting `_NET_WM_STATE`
        // is how a fullscreen request is *answered*, and it arrives here
        // because `PropertyChange` is one event mask covering both. Without
        // this the effective mode would only ever be read at map time, and a
        // fullscreen toggle would update `requested_mode` and nothing else.
        if event.atom == self.conn.atoms.net_wm_state
            && let Some(window) = self.window_by_xid(event.window)
        {
            self.refresh_effective_mode(window);
            return;
        }

        // Reading: a new value on *our* window's property is the next chunk.
        if event.state == NEW_VALUE
            && let Some(index) = self.reads.iter().position(|read| {
                read.state == selection::ReadState::Incremental
                    && read.property == event.atom
                    && self
                        .windows
                        .get(read.window.cast())
                        .is_some_and(|window| window.id == event.window)
            })
        {
            let bytes = match self.conn.get_property(event.window, event.atom) {
                Some((_, _, bytes)) => bytes,
                // A failed read is not the end of the transfer — the terminator is a
                // type-less property, and `get_property` returns that as `Some((0, …))`.
                // A null reply or an over-cap chunk means no progress: the read is left
                // in place and `service_transfers` abandons it as stalled (`Unavailable`)
                // instead of a truncated paste being reported complete.
                None => return,
            };
            let step = self.reads[index].on_chunk(&bytes, ffi::monotonic_nanos());
            self.settle(index, step);
            return;
        }

        // Writing: the peer deleted the property, which is its acknowledgement
        // and its request for the next chunk.
        if event.state == DELETED
            && let Some(index) = self
                .writes
                .iter()
                .position(|write| write.requestor == event.window && write.property == event.atom)
        {
            self.push_chunk(index);
        }
    }

    /// Writes the next `INCR` chunk of an outgoing transfer.
    fn push_chunk(&mut self, index: usize) {
        let now = ffi::monotonic_nanos();
        let Some(write) = self.writes.get_mut(index) else {
            return;
        };
        let (requestor, property, target) = (write.requestor, write.property, write.target);
        let Some(chunk) = write.next_chunk(now) else {
            self.writes.remove(index);
            return;
        };
        let finished = chunk.is_empty();
        // A copy, because `set_property` borrows the connection and `write`
        // borrows `self.writes`. Bounded by the chunk size, which is the
        // server's own request limit.
        let chunk = chunk.to_vec();
        self.conn
            .set_property(requestor, property, target, 8, &chunk);
        self.conn.flush();
        if finished {
            self.writes.remove(index);
        }
    }

    /// Applies a [`Step`] to the read at `index`, emitting it if it finished.
    fn settle(&mut self, index: usize, step: Step) {
        match step {
            Step::AcknowledgeChunk => {
                let (xid, property) = {
                    let read = &self.reads[index];
                    let xid = self
                        .windows
                        .get(read.window.cast())
                        .map_or(0, |window| window.id);
                    (xid, read.property)
                };
                if xid != 0 {
                    // Deleting the property is the acknowledgement *and* the
                    // request for the next chunk. Both halves of the `INCR`
                    // handshake are this one call.
                    self.conn.delete_property(xid, property);
                    self.conn.flush();
                }
            }
            Step::Convert(target) => {
                let (xid, property) = {
                    let read = &self.reads[index];
                    let xid = self
                        .windows
                        .get(read.window.cast())
                        .map_or(0, |window| window.id);
                    (xid, read.property)
                };
                if xid == 0 {
                    let read = self.reads.remove(index);
                    self.emit_answer(&read, ClipboardContent::Unavailable);
                    return;
                }
                self.convert(xid, target, property);
            }
            Step::Done(content) => {
                let read = self.reads.remove(index);
                self.emit_answer(&read, content);
            }
        }
    }

    /// Emits the one [`ClipboardData`](ShellEvent::ClipboardData) a read owes.
    ///
    /// Obligation 4's "exactly once" is structural here: a [`Read`] is removed
    /// from the list in the same expression that produces the event, so there
    /// is no path that emits twice and none that drops one silently.
    fn emit_answer(&mut self, read: &Read, content: ClipboardContent) {
        let mime = match content {
            // The peer chose the spelling, so report the peer's — which is why
            // `ReceivedMime` is a separate type from `MimeType`.
            ClipboardContent::Bytes(_) => self
                .conn
                .atoms
                .spelling_of(read.answered_target)
                .map_or_else(|| ReceivedMime::from(read.mime), ReceivedMime::new),
            // There is no peer spelling for a non-answer, so the seam specifies
            // the format that was *asked* for.
            ClipboardContent::Empty | ClipboardContent::Unavailable => {
                ReceivedMime::from(read.mime)
            }
        };
        self.queue.push_back(ShellEvent::ClipboardData {
            window: read.window,
            request: read.request,
            mime,
            content,
        });
    }

    /// Starts a read, or answers it immediately.
    ///
    /// The synchronous half is `GetSelectionOwner`: X11 can say whether there
    /// is anything on the clipboard at all without asking anyone to convert
    /// anything, so "nobody owns the selection" is answered as
    /// [`Empty`](ClipboardContent::Empty) on the spot rather than held. That is
    /// obligation 5 discharged by answering, and it is the case the trait's own
    /// documentation names X11 for.
    pub(super) fn start_read(
        &mut self,
        window: super::WindowId,
        mime: MimeType,
    ) -> ClipboardRequestId {
        let request = ClipboardRequestId(self.next_request_id);
        self.next_request_id += 1;

        let xid = self.window(window).map_or(0, |state| state.id);
        let candidates = self.conn.atoms.candidates_for(mime);
        if xid == 0 || candidates.is_empty() || self.conn.selection_owner() == 0 {
            self.queue.push_back(ShellEvent::ClipboardData {
                window,
                request,
                mime: ReceivedMime::from(mime),
                content: ClipboardContent::Empty,
            });
            return request;
        }

        // A dedicated property per shell rather than per request: X11 allows
        // only one outstanding conversion per (requestor, property) pair, and
        // the seam already serialises reads through `pump`. Using the target
        // atom as the property — which some clients do — would collide the
        // moment two windows read the same format at once.
        let property = self.conn.atoms.crcbl_selection;
        // Step one is always `TARGETS`: a mime type is one string here and up
        // to four atoms on the wire, and only the owner knows which it has.
        self.convert(xid, self.conn.atoms.targets, property);
        self.reads.push(Read::new(
            request,
            window,
            mime,
            candidates,
            property,
            ffi::monotonic_nanos(),
        ));
        request
    }

    /// Issues one `ConvertSelection` into `property`.
    ///
    /// The property is deleted first, because a stale value from a previous
    /// conversion would otherwise be indistinguishable from this one's answer —
    /// the owner writes the property and *then* sends the notify, so a
    /// requestor that skipped this can read the wrong bytes and never know.
    fn convert(&self, xid: u32, target: u32, property: u32) {
        self.conn.delete_property(xid, property);
        // SAFETY: the connection and window are live, and every atom came from
        // the interned table.
        unsafe {
            (self.conn.lib.convert_selection)(
                self.conn.raw(),
                xid,
                self.conn.atoms.clipboard,
                target,
                property,
                self.last_server_time,
            );
        }
        self.conn.flush();
    }

    /// Learns the server's current time, by provoking an event that carries
    /// one.
    ///
    /// See the [module docs](self) for why a client cannot simply ask. The
    /// zero-length append is a no-op on the property — `PropMode::Append` of
    /// nothing — and exists purely for the `PropertyNotify` it generates, which
    /// [`handle_property_notify`](Self::handle_property_notify) latches into
    /// `last_server_time` on the way past.
    ///
    /// The probe's own notify is the answer, not the stamp it carries: at ≥1
    /// kHz event rates the server may timestamp it with the same millisecond as
    /// the previous event, so the loop waits for the event to arrive rather
    /// than for `last_server_time` to change.
    ///
    /// Bounded, and draining through the ordinary path: any other event that
    /// arrives while waiting is handled exactly as it would have been, and is
    /// delivered on the next [`pump`](crate::Shell::pump) rather than lost.
    pub(super) fn refresh_server_time(&mut self, xid: u32) {
        /// Long enough for a loaded server, short enough that a copy never
        /// feels slow. A local X socket answers in microseconds.
        const DEADLINE: core::time::Duration = core::time::Duration::from_millis(250);

        // The probe's own notify is the answer even when the server stamps it
        // with the same millisecond as the previous event — at that rate
        // `last_server_time` cannot change, and the loop would otherwise burn
        // its whole deadline. `handle_property_notify` sets the flag for
        // exactly this event, so the reset is what arms the wait.
        self.timestamp_probe_seen = false;
        // SAFETY: the connection and window are live. Appending zero elements
        // is well-defined and leaves the property's value untouched — the
        // event is the whole point.
        unsafe {
            (self.conn.lib.change_property)(
                self.conn.raw(),
                ffi::value::PROP_MODE_APPEND,
                xid,
                self.conn.atoms.crcbl_timestamp,
                ffi::value::ATOM_STRING,
                8,
                0,
                core::ptr::null(),
            );
        }
        self.conn.flush();

        let deadline =
            ffi::monotonic_nanos() + u64::try_from(DEADLINE.as_nanos()).unwrap_or(u64::MAX);
        loop {
            self.drain();
            if self.timestamp_probe_seen || self.lost.is_some() {
                return;
            }
            if ffi::monotonic_nanos() >= deadline {
                crcbl_core::log::warn!(
                    "the X server did not answer a timestamp probe; using CurrentTime"
                );
                return;
            }
            let _ = ffi::poll_readable(self.conn.fd, 5);
        }
    }

    /// Fails every read and write that has stopped making progress.
    ///
    /// Obligation 4's "within a bounded time", and the one part of the X11
    /// clipboard that is *harder* than Wayland's: there is no descriptor to
    /// poll and no protocol event for "the peer gave up", so a deadline is the
    /// only thing that can end a stalled transfer. Called once per
    /// [`pump`](crate::Shell::pump).
    pub(super) fn service_transfers(&mut self) {
        let now = ffi::monotonic_nanos();
        let mut index = 0;
        while index < self.reads.len() {
            if self.reads[index].is_stalled(now) {
                let read = self.reads.remove(index);
                crcbl_core::log::warn!(
                    "clipboard {} stalled for {:?}; answering Unavailable",
                    read.request,
                    selection::TIMEOUT
                );
                self.emit_answer(&read, ClipboardContent::Unavailable);
                continue;
            }
            index += 1;
        }
        self.writes.retain(|write| !write.is_stalled(now));
    }

    /// Selects `PropertyChange` on another client's window.
    ///
    /// Needed for the *sending* half of an `INCR` transfer: the requestor
    /// acknowledges each chunk by deleting the property, and without this
    /// selection those deletions are invisible and the transfer stops after one
    /// chunk. Never issued for one of our own windows — see the guard inside.
    pub(super) fn select_property_changes(&self, xid: u32) {
        // Our own windows already select `PROPERTY_CHANGE` — it is in
        // `WINDOW_EVENT_MASK`, set at creation and never changed — so an
        // `INCR` transfer between two of our windows needs no mask change,
        // and must not get one: `ChangeWindowAttributes(EVENT_MASK, …)`
        // *replaces* the whole mask, and the old call stripped every input
        // event off a window pasting our own oversized offer, permanently.
        // A foreign requestor keeps the replace, which is the only way to
        // add `PROPERTY_CHANGE` to a window we know nothing about.
        if self.window_by_xid(xid).is_some() {
            return;
        }
        let values = [ffi::event_mask::PROPERTY_CHANGE];
        // SAFETY: the connection is live and `xid` is the requestor from a
        // `SelectionRequest`, which the server vouched for. The value list has
        // exactly the one word the `EVENT_MASK` bit declares. A window that has
        // since been destroyed yields an asynchronous `BadWindow`, which is the
        // correct outcome — the transfer then times out.
        unsafe {
            (self.conn.lib.change_window_attributes)(
                self.conn.raw(),
                xid,
                ffi::cw::EVENT_MASK,
                values.as_ptr().cast::<core::ffi::c_void>(),
            );
        }
    }
}

impl super::Conn {
    /// Who owns the `CLIPBOARD` selection, or `0` for nobody.
    ///
    /// The synchronous question X11 can answer and Wayland cannot; see
    /// [`start_read`](X11Shell::start_read).
    pub(super) fn selection_owner(&self) -> u32 {
        // SAFETY: the connection is live; a null error pointer discards the
        // error and a null reply is handled below.
        let reply = unsafe {
            let cookie = (self.lib.get_selection_owner)(self.raw(), self.atoms.clipboard);
            (self.lib.get_selection_owner_reply)(self.raw(), cookie, core::ptr::null_mut())
        };
        if reply.is_null() {
            return 0;
        }
        // SAFETY: `reply` is a live reply this call owns.
        let owner = unsafe { (*reply).owner };
        // SAFETY: freed exactly once.
        unsafe { ffi::free_reply(reply) };
        owner
    }

    /// Claims or releases the `CLIPBOARD` selection.
    ///
    /// Returns whether the server agrees we own it. The read-back is not
    /// paranoia: `SetSelectionOwner` is silently ignored when the timestamp is
    /// older than the current owner's, which is exactly the race the timestamp
    /// rule exists for, and a client that assumed success would then answer
    /// `SelectionRequest`s that never arrive.
    pub(super) fn set_selection_owner(&self, owner: u32, time: u32) -> bool {
        // SAFETY: the connection is live and `owner` is one of our windows or
        // `XCB_NONE`.
        unsafe { (self.lib.set_selection_owner)(self.raw(), owner, self.atoms.clipboard, time) };
        self.flush();
        self.selection_owner() == owner
    }
}
