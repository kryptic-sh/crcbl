//! The shell's side of `wl_data_device`: the protocol calls for the
//! clipboard and drag-and-drop.
//!
//! The bookkeeping between those calls is in [`super::data`], which can be
//! tested without a compositor; this module is the half that needs the
//! connection.

use core::ffi::c_int;
use std::ffi::CString;
use std::os::fd::AsRawFd;

use crate::{
    ClipboardContent, ClipboardRequestId, MimeType, PhysicalPoint, ReceivedMime, ShellError,
    ShellEvent, WindowId,
};

use super::data::{self, Delivery, Resolution, Transfer};
use super::ffi::{self, WlProxy};
use super::protocol::wayland::{wl_data_device, wl_data_offer, wl_data_source};
use super::{RawEvent, WaylandShell, WlWindow, fd, keymap};

impl WaylandShell {
    /// The seat whose `wl_data_device` is `proxy`.
    fn data_seat(&self, proxy: usize) -> Option<usize> {
        self.seats.iter().position(|seat| {
            seat.data
                .as_ref()
                .is_some_and(|device| device.proxy as usize == proxy)
        })
    }

    /// The seat whose `wl_data_source` is `proxy`.
    fn source_seat(&self, proxy: usize) -> Option<usize> {
        self.seats.iter().position(|seat| {
            seat.data
                .as_ref()
                .and_then(|device| device.source.as_ref())
                .is_some_and(|source| source.proxy as usize == proxy)
        })
    }

    /// Seats in the order a clipboard operation for `window` should try them.
    ///
    /// The clipboard belongs to a **seat**, and the seam names a *window*. The
    /// bridge is focus: the seat that is typing into this window is the one
    /// whose Ctrl+C this is. Seats that are not focused on it come after, so a
    /// single-seat session — every desktop — behaves identically either way,
    /// and a multi-seat one prefers the right clipboard instead of the first.
    pub(super) fn seats_for_window(&self, window: WindowId) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.seats.len())
            .filter(|index| self.seats[*index].data.is_some())
            .collect();
        order.sort_by_key(|index| {
            let seat = &self.seats[*index];
            match (seat.keyboard_focus, seat.pointer_focus) {
                (Some(focused), _) if focused == window => 0,
                (_, Some(hovered)) if hovered == window => 1,
                _ => 2,
            }
        });
        order
    }

    /// What this shell currently knows about the clipboard, for `window` and
    /// `mime`.
    ///
    /// The three-way answer the seam's obligation 5 turns on:
    /// [`Ready`](Resolution::Ready) starts a transfer,
    /// [`Empty`](Resolution::Empty) is an answer, and
    /// [`Unknown`](Resolution::Unknown) means **hold** — the compositor has not
    /// told us what is on the clipboard, which is not the same as it being
    /// empty.
    pub(super) fn resolve_selection(&self, window: WindowId, mime: MimeType) -> Resolution {
        let mut known = false;
        for index in self.seats_for_window(window) {
            let Some(device) = self.seats[index].data.as_ref() else {
                continue;
            };
            if !device.selection_seen {
                continue;
            }
            known = true;
            if let Some(offer) = device.selection.as_ref()
                && let Some(spelling) = offer.pick(mime)
            {
                return Resolution::Ready {
                    offer: offer.proxy,
                    spelling: spelling.to_string(),
                };
            }
        }
        if known {
            Resolution::Empty
        } else {
            Resolution::Unknown
        }
    }

    /// Queues one [`ShellEvent::ClipboardData`].
    pub(super) fn answer_read(
        &mut self,
        window: WindowId,
        request: ClipboardRequestId,
        mime: ReceivedMime,
        content: ClipboardContent,
    ) {
        self.queue.push_back(ShellEvent::ClipboardData {
            window,
            request,
            mime,
            content,
        });
    }

    /// Starts, answers or keeps holding every read that is waiting for the
    /// clipboard to become readable.
    ///
    /// Called from [`pump`](crate::Shell::pump) and whenever a `selection` event
    /// arrives, which is the moment a held read most often becomes answerable.
    pub(super) fn resolve_held_reads(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let now = ffi::monotonic_nanos();
        for read in core::mem::take(&mut self.held) {
            // The window went away while its read was outstanding. Obligation 4
            // is about answering an accepted request; an answer naming a
            // destroyed window would break obligation 1 instead.
            if self.window(read.window).is_err() {
                continue;
            }
            match self.resolve_selection(read.window, read.mime) {
                Resolution::Ready { offer, spelling } => {
                    let delivery = Delivery::Clipboard {
                        window: read.window,
                        request: read.request,
                        mime: ReceivedMime::new(&spelling),
                    };
                    if self.start_receive(offer, &spelling, delivery).is_err() {
                        self.answer_read(
                            read.window,
                            read.request,
                            ReceivedMime::new(&spelling),
                            ClipboardContent::Unavailable,
                        );
                    }
                }
                Resolution::Empty => self.answer_read(
                    read.window,
                    read.request,
                    ReceivedMime::from(read.mime),
                    ClipboardContent::Empty,
                ),
                Resolution::Unknown if now < read.deadline_nanos => self.held.push(read),
                Resolution::Unknown => {
                    // Out of time: no window of ours has had keyboard focus for
                    // the whole wait, so there is nothing to read from and
                    // never was anything to report. `Unavailable`, not `Empty`
                    // — there may well be something on the clipboard.
                    crcbl_core::log::debug!(
                        "{:?}: the clipboard never became readable; \
                         no window of this client had keyboard focus",
                        read.window
                    );
                    self.answer_read(
                        read.window,
                        read.request,
                        ReceivedMime::from(read.mime),
                        ClipboardContent::Unavailable,
                    );
                }
            }
        }
    }

    /// Sends a `wl_data_offer` destructor, if the offer still has a proxy.
    pub(super) fn destroy_offer(&mut self, offer: &data::Offer) {
        if offer.proxy.is_null() {
            return;
        }
        // SAFETY: the proxy is a live `wl_data_offer` created by the compositor
        // on this connection, and `destroy` is that interface's own destructor.
        unsafe { self.conn.release(offer.proxy, wl_data_offer::destroy) };
    }

    /// Routes one `data-device` event.
    ///
    /// Every path that can see a [`RawEvent::SourceSend`] either adopts its
    /// descriptor or closes it; see that variant.
    #[expect(
        clippy::too_many_lines,
        reason = "the data-device routing table; see process_raw"
    )]
    pub(super) fn process_data(&mut self, event: RawEvent) {
        match event {
            RawEvent::DataOffer { device, offer } => {
                let proxy = offer as *mut WlProxy;
                let Some(index) = self.data_seat(device) else {
                    // The seat went away between the event and the drain.
                    self.destroy_offer(&data::Offer::new(proxy));
                    return;
                };
                if let Some(device) = self.seats[index].data.as_mut() {
                    let evicted = device.push_incoming(data::Offer::new(proxy));
                    if let Some(evicted) = evicted {
                        self.destroy_offer(&evicted);
                    }
                }
            }
            RawEvent::OfferMime { offer, mime } => {
                for seat in &mut self.seats {
                    if let Some(device) = seat.data.as_mut()
                        && let Some(offer) = device.announced_mut(offer)
                    {
                        offer.note_mime(mime);
                        return;
                    }
                }
            }
            RawEvent::Selection { device, offer } => {
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                // "The client must destroy the previous selection data_offer,
                // if any, upon receiving this event" — and the *new* offer was
                // announced first, so both are in hand at once.
                let (previous, claimed) = match self.seats[index].data.as_mut() {
                    Some(device) => (device.selection.take(), device.claim(offer)),
                    None => return,
                };
                if let Some(previous) = previous {
                    self.destroy_offer(&previous);
                }
                if let Some(device) = self.seats[index].data.as_mut() {
                    device.selection = claimed;
                    // Including for the null offer: "the clipboard was cleared"
                    // is knowledge, and it is what lets a read be answered
                    // `Empty` rather than held.
                    device.selection_seen = true;
                }
                // A read that was waiting for exactly this can now be answered.
                self.resolve_held_reads();
            }
            RawEvent::DragEnter {
                device,
                serial,
                surface,
                x,
                y,
                offer,
            } => self.drag_entered(device, serial, surface, x, y, offer),
            RawEvent::DragMotion {
                device, x, y, time, ..
            } => {
                let _ = time;
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                let scale = self.seats[index]
                    .data
                    .as_ref()
                    .and_then(|device| device.drag.as_ref())
                    .and_then(|drag| drag.window)
                    .and_then(|window| self.window(window).ok())
                    .map_or(1.0, WlWindow::scale_factor);
                if let Some(drag) = self.seats[index]
                    .data
                    .as_mut()
                    .and_then(|device| device.drag.as_mut())
                {
                    drag.position = PhysicalPoint::new(
                        keymap::fixed_to_f64(x) * scale,
                        keymap::fixed_to_f64(y) * scale,
                    );
                }
            }
            RawEvent::DragLeave { device } => {
                let Some(index) = self.data_seat(device) else {
                    return;
                };
                // `None` when the drag already became a transfer at `drop`; a
                // compositor is entitled to send `leave` after one.
                let drag = self.seats[index]
                    .data
                    .as_mut()
                    .and_then(|device| device.drag.take());
                if let Some(drag) = drag {
                    self.destroy_offer(&drag.offer);
                }
            }
            RawEvent::DragDrop { device } => self.drag_dropped(device),
            RawEvent::SourceSend { source, mime, fd } => {
                let Some(file) = fd::adopt(fd) else { return };
                let Some(index) = self.source_seat(source) else {
                    // Our source is already gone; closing the descriptor is the
                    // end-of-file that tells the peer there is nothing coming.
                    return;
                };
                let bytes = self.seats[index]
                    .data
                    .as_ref()
                    .and_then(|device| device.source.as_ref())
                    .and_then(|source| source.bytes_for(&mime))
                    .map(<[u8]>::to_vec);
                let Some(bytes) = bytes else {
                    crcbl_core::log::debug!(
                        "a peer asked for {mime}, which this selection does not offer"
                    );
                    return;
                };
                // Never written here: the peer may be reading slowly, or may be
                // *this process* pasting its own selection, in which case the
                // read that would drain the pipe cannot happen until this
                // function has returned. See `fd`.
                self.writes
                    .push(fd::Writing::new(file, bytes, ffi::monotonic_nanos()));
                self.service_transfers();
            }
            RawEvent::SourceCancelled { source } => {
                let Some(index) = self.source_seat(source) else {
                    return;
                };
                // Another client took the selection. The source is dead and the
                // protocol says to destroy it; the bytes go with it, because
                // nothing will ever ask for them again.
                self.clear_source(index);
            }
            _ => {}
        }
    }

    /// Tells a drag source that nothing here suits us.
    ///
    /// `wl_data_offer.accept` with a **null** mime type is the protocol's "no",
    /// and it has to be sent even for an offer this backend is not tracking: a
    /// drag that gets no answer at all leaves the source's cursor promising a
    /// drop that will never happen.
    fn refuse_drag(&mut self, proxy: *mut WlProxy, serial: u32) {
        if proxy.is_null() {
            return;
        }
        // SAFETY: `proxy` is the `wl_data_offer` the compositor created and
        // named in the `enter` event being handled, so it is live; `accept`
        // takes a nullable string.
        unsafe { wl_data_offer::accept(proxy, serial, None) };
        self.conn.flush();
    }

    /// A drag arrived over one of our surfaces.
    fn drag_entered(
        &mut self,
        device: usize,
        serial: u32,
        surface: usize,
        x: i32,
        y: i32,
        offer: usize,
    ) {
        let proxy = offer as *mut WlProxy;
        let Some(index) = self.data_seat(device) else {
            self.refuse_drag(proxy, serial);
            // Refusing is not enough: the offer was announced for this `enter`,
            // but with the seat gone it can never be claimed, so nothing will
            // ever destroy it but this.
            self.destroy_offer(&data::Offer::new(proxy));
            return;
        };
        let Some(offer) = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.claim(offer))
        else {
            self.refuse_drag(proxy, serial);
            // Same shape as the seat-gone path: the `enter` named an offer that
            // was never announced, so it is unclaimable and refusing alone
            // would leak the proxy.
            self.destroy_offer(&data::Offer::new(proxy));
            return;
        };
        // A window that did not ask for drops is not a drop target: the source
        // is told we accept nothing, which is what makes its cursor say so, and
        // the payload is never read. `WindowDesc::accept_drops` is opt-in
        // precisely so a game does not show a drop cursor it will ignore.
        let target = self
            .window_by_proxy(surface, |window| window.surface as usize)
            .filter(|window| self.window(*window).is_ok_and(|state| state.accept_drops));
        let scale = target
            .and_then(|window| self.window(window).ok())
            .map_or(1.0, WlWindow::scale_factor);
        // `text/uri-list` is the only drag format this seam can express: a
        // `DroppedFile` carries a path. A drag of text or of an image is
        // refused rather than half-accepted.
        let mime = target
            .and(offer.pick(MimeType::UriList))
            .map(str::to_string);

        if offer.proxy.is_null() {
            // Nothing to answer on. The null check used to guard only the
            // version query, and then `accept` and `set_actions` dereferenced
            // the same pointer two lines later — the generated wrappers read
            // the proxy before marshalling anything.
            return;
        }
        // SAFETY: the offer proxy is live on this connection.
        let version = unsafe { ffi::proxy_version(offer.proxy) };
        let encoded = mime.as_deref().and_then(|mime| CString::new(mime).ok());
        // SAFETY: the offer is live (checked just above); `accept` takes a
        // nullable string, and a null one is the protocol's own way of saying
        // "nothing here suits me".
        unsafe {
            wl_data_offer::accept(offer.proxy, serial, encoded.as_deref());
            if version >= 3 {
                // Only `copy`: see `data::ACTION_COPY`. A source that offers
                // only `move` gets no action, which is a refused drop rather
                // than a promise this engine cannot keep.
                let action = if encoded.is_some() {
                    data::ACTION_COPY
                } else {
                    0
                };
                wl_data_offer::set_actions(offer.proxy, action, action);
            }
        }
        self.conn.flush();

        // A second `enter` without `leave` violates the protocol, but it must
        // not leak: the previous drag's offer is destroyed before being
        // replaced, exactly as `leave` would have done.
        let previous = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.drag.take());
        if let Some(previous) = previous {
            self.destroy_offer(&previous.offer);
        }

        if let Some(device) = self.seats[index].data.as_mut() {
            device.drag = Some(data::Drag {
                offer,
                window: target,
                position: PhysicalPoint::new(
                    keymap::fixed_to_f64(x) * scale,
                    keymap::fixed_to_f64(y) * scale,
                ),
                mime,
            });
        }
    }

    /// The drag was released. Starts the read that becomes
    /// [`ShellEvent::DroppedFile`].
    fn drag_dropped(&mut self, device: usize) {
        let Some(index) = self.data_seat(device) else {
            return;
        };
        let Some(drag) = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.drag.take())
        else {
            return;
        };
        let (Some(window), Some(mime)) = (drag.window, drag.mime.clone()) else {
            // Refused on `enter`, so there is nothing to read and the offer is
            // ours to dispose of.
            self.destroy_offer(&drag.offer);
            return;
        };
        let delivery = Delivery::Drop {
            window,
            position: drag.position,
            device,
            offer: drag.offer.proxy as usize,
        };
        if self
            .start_receive(drag.offer.proxy, &mime, delivery)
            .is_err()
        {
            self.destroy_offer(&drag.offer);
        }
    }

    /// Asks a peer for `mime` over a fresh pipe, and records what the bytes
    /// become.
    ///
    /// The pipe's write end goes to the peer and is dropped here immediately:
    /// the descriptor is duplicated across the socket by the compositor, and a
    /// client that kept its own copy open would keep the pipe from ever
    /// reaching end-of-file — which is the only signal that a transfer is
    /// complete.
    pub(super) fn start_receive(
        &mut self,
        offer: *mut WlProxy,
        mime: &str,
        delivery: Delivery,
    ) -> Result<(), ShellError> {
        let encoded = CString::new(mime)
            .map_err(|_| ShellError::Backend("a mime type with a NUL byte".to_string()))?;
        let (reader, writer) = std::io::pipe()
            .map_err(|error| ShellError::Backend(format!("cannot create a pipe: {error}")))?;
        // SAFETY: the offer is live on this connection, and the descriptor is
        // valid for the duration of the call — which is all `receive` needs,
        // since the compositor duplicates it out of the message.
        unsafe { wl_data_offer::receive(offer, &encoded, writer.as_raw_fd()) };
        self.conn.flush();
        drop(writer);
        self.transfers.push(Transfer {
            reading: fd::Reading::new(
                std::fs::File::from(std::os::fd::OwnedFd::from(reader)),
                ffi::monotonic_nanos(),
            ),
            delivery,
        });
        Ok(())
    }

    /// Destroys the selection source this seat published, if any.
    fn clear_source(&mut self, index: usize) {
        let source = self.seats[index]
            .data
            .as_mut()
            .and_then(|device| device.source.take());
        let Some(source) = source else { return };
        if source.proxy.is_null() {
            return;
        }
        // SAFETY: the proxy is a live `wl_data_source` this backend created,
        // and `destroy` is its own destructor.
        unsafe { self.conn.release(source.proxy, wl_data_source::destroy) };
    }

    /// Tears down one seat's `wl_data_device` and everything hanging off it.
    ///
    /// Order matters: the offers and the source are objects the *device* owns,
    /// so they go first, exactly as a pointer's constraints go before the
    /// pointer.
    pub(super) fn remove_data_device(&mut self, index: usize) {
        let Some(device) = self.seats[index].data.take() else {
            return;
        };
        // A drop whose device is going away can never be finished, and its
        // offer would outlive the device that owns it. Both end here, while the
        // device is still alive to destroy the offer against.
        let doomed: Vec<usize> = self
            .transfers
            .iter()
            .filter(|transfer| {
                matches!(transfer.delivery, Delivery::Drop { device: owner, .. }
                    if owner == device.proxy as usize)
            })
            .filter_map(|transfer| transfer.delivery.drop_offer())
            .collect();
        self.transfers.retain(|transfer| {
            !matches!(transfer.delivery, Delivery::Drop { device: owner, .. }
                if owner == device.proxy as usize)
        });
        let offers = device
            .incoming
            .iter()
            .chain(device.selection.iter())
            .chain(device.drag.as_ref().map(|drag| &drag.offer))
            .map(|offer| offer.proxy)
            .chain(doomed.into_iter().map(|offer| offer as *mut WlProxy))
            .collect::<Vec<_>>();
        for proxy in offers {
            self.destroy_offer(&data::Offer::new(proxy));
        }
        if let Some(source) = device.source.as_ref() {
            // SAFETY: a live `wl_data_source` this backend created.
            unsafe { self.conn.release(source.proxy, wl_data_source::destroy) };
        }
        // SAFETY: the device is live and `release` is `wl_data_device`'s own
        // destructor — which arrived in version 2, so an older one is dropped
        // client-side instead.
        unsafe {
            self.conn
                .release_since(device.proxy, 2, wl_data_device::release);
        }
        self.conn.flush();
    }

    /// The descriptors a blocking wait should include; see
    /// [`Conn::drain`](super::Conn::drain).
    ///
    /// **Reads only.** A pending write is nearly always writable — that is what
    /// "pending" means for one whose peer is merely slow — so putting it in the
    /// poll would make every wait return immediately and turn an idle editor
    /// into a spin loop for the length of the transfer. Writes are covered by
    /// [`TRANSFER_POLL`](super::TRANSFER_POLL) instead, which is a bound on the
    /// sleep rather than an invitation to skip it.
    pub(super) fn transfer_fds(&self) -> Vec<c_int> {
        self.transfers
            .iter()
            .map(|transfer| transfer.reading.raw_fd())
            .collect()
    }

    /// Moves every transfer as far as it will go without blocking, and turns
    /// the finished ones into events.
    ///
    /// # Why this is a loop
    ///
    /// The two ends of a transfer can both be this process — pasting our own
    /// selection is the ordinary case, and it is the one that deadlocks a
    /// backend that reads on the event-loop thread. Writing fills the pipe,
    /// reading empties it, and neither can finish alone, so the pass repeats
    /// while *anything* moved. It terminates because every iteration that
    /// repeats has moved bytes and no payload is infinite.
    pub(super) fn service_transfers(&mut self) {
        while !self.transfers.is_empty() || !self.writes.is_empty() {
            let now = ffi::monotonic_nanos();
            let mut moved = false;

            // Writes first: a read of our own selection cannot progress until
            // the bytes it is waiting for have been put in the pipe.
            moved |= self.writes.service(now);

            let mut finished = Vec::new();
            let mut index = 0;
            while index < self.transfers.len() {
                let (state, progressed) = self.transfers[index].reading.service(now);
                moved |= progressed;
                match state {
                    fd::State::Pending => index += 1,
                    fd::State::Done => finished.push((self.transfers.remove(index), true)),
                    fd::State::Failed => finished.push((self.transfers.remove(index), false)),
                }
            }
            for (transfer, complete) in finished {
                self.deliver(transfer, complete);
            }
            if !moved {
                break;
            }
        }
    }

    /// Turns one finished transfer into the events it promised.
    fn deliver(&mut self, transfer: Transfer, complete: bool) {
        let Transfer { reading, delivery } = transfer;
        let bytes = reading.take();
        match delivery {
            Delivery::Clipboard {
                window,
                request,
                mime,
            } => {
                // A transfer that completed reports its bytes even when there
                // are none of them: a peer is entitled to publish an empty
                // selection, and `Bytes(vec![])` says the read worked. One that
                // broke — the peer went away mid-write, or never wrote at all —
                // is `Unavailable`, which is emphatically not `Empty`: there
                // *is* something on the clipboard, we just could not get it.
                let content = if complete {
                    ClipboardContent::Bytes(bytes)
                } else {
                    ClipboardContent::Unavailable
                };
                self.answer_read(window, request, mime, content);
            }
            Delivery::Drop {
                window,
                position,
                offer,
                ..
            } => {
                let offer = offer as *mut WlProxy;
                if complete {
                    let time = self.time.event_time_now();
                    // One event per file, which is what `DroppedFile`
                    // specifies; a URI that is not a local path produces none.
                    for path in crate::parse_uri_list(&bytes) {
                        self.queue.push_back(ShellEvent::DroppedFile {
                            window,
                            time,
                            path,
                            position: Some(position),
                        });
                    }
                }
                // SAFETY: the offer is live — nothing destroys it between the
                // `drop` event and here — and both requests belong to its
                // interface.
                unsafe {
                    // `finish` tells the source the transfer is over so it can
                    // release its own copy. It arrived with the actions in
                    // version 3, and calling it on an older offer is a protocol
                    // error that disconnects the client.
                    if !offer.is_null() && ffi::proxy_version(offer) >= 3 && complete {
                        wl_data_offer::finish(offer);
                    }
                    self.conn.release(offer, wl_data_offer::destroy);
                }
                self.conn.flush();
            }
        }
    }
}
