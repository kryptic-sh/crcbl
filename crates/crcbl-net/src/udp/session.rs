//! A keyed link: the sealer, opener and endpoint one peer's traffic goes
//! through once the hello is done.

use std::net::{SocketAddr, UdpSocket};

use crate::Clock;
use crate::reliable::Endpoint;
use crate::seal::{Opener, SealError, Sealer};

/// Datagrams a [`Session`] refused, by why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Refusals {
    /// Failed [`Opener::open`]: forged, tampered, replayed, too old for the
    /// replay window, or not sealed at all.
    pub(crate) unopened: u64,
    /// Opened, then refused by [`Endpoint::receive_datagram`] — a closed link
    /// or a full delivery queue, which an honest peer's resend recovers.
    pub(crate) refused: u64,
}

/// One peer's end of a keyed link.
pub(crate) struct Session<C: Clock> {
    sealer: Sealer,
    opener: Opener,
    pub(crate) endpoint: Endpoint<C>,
    pub(crate) refusals: Refusals,
}

impl<C: Clock> Session<C> {
    pub(crate) fn new(sealer: Sealer, opener: Opener, endpoint: Endpoint<C>) -> Self {
        Self {
            sealer,
            opener,
            endpoint,
            refusals: Refusals::default(),
        }
    }

    /// Open one datagram from the peer and give the packet to the endpoint.
    ///
    /// A datagram that does not open is dropped and counted, never passed on
    /// and never an error: anyone can send one, so none of them may be able
    /// to end the link. See [`super`]'s docs.
    pub(crate) fn receive(&mut self, datagram: &[u8]) {
        match self.opener.open(datagram) {
            Ok(packet) => self.receive_packet(&packet),
            Err(_) => self.refusals.unopened += 1,
        }
    }

    /// Give an already opened packet to the endpoint: the one a listener
    /// opened to confirm the handshake.
    pub(crate) fn receive_packet(&mut self, packet: &[u8]) {
        if self.endpoint.receive_datagram(packet).is_err() {
            self.refusals.refused += 1;
        }
    }

    /// Seal and send everything the endpoint has due, to `peer` through
    /// `socket`, counting each datagram the socket would not take in
    /// `send_failures`.
    ///
    /// # Errors
    ///
    /// [`SealError::CounterExhausted`] once the sealer has no counter left;
    /// the link is then over, and a reconnect is the rekey.
    pub(crate) fn flush(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        send_failures: &mut u64,
    ) -> Result<(), SealError> {
        while let Some(packet) = self.endpoint.poll_outgoing() {
            let datagram = self.sealer.seal(&packet)?;
            send(socket, &datagram, peer, send_failures);
        }
        Ok(())
    }

    /// Send what is still due, then close the link and send the disconnect
    /// packets, so the peer sees a disconnect rather than a timeout and the
    /// reliable messages queued before the close go out ahead of it.
    pub(crate) fn close(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        send_failures: &mut u64,
    ) -> Result<(), SealError> {
        if self.endpoint.state().is_connected() {
            self.flush(socket, peer, send_failures)?;
            self.endpoint.disconnect();
        }
        self.flush(socket, peer, send_failures)
    }
}

/// Put one datagram on the wire.
///
/// A datagram the socket refuses — its buffer full, the route gone — is lost
/// exactly as one dropped on the wire is, and the protocol already recovers
/// from that: the reliable channel resends, the unreliable one never needed
/// it, and a link whose datagrams never arrive times out. So a refusal is
/// counted rather than raised, and the caller's link carries on.
pub(crate) fn send(socket: &UdpSocket, datagram: &[u8], to: SocketAddr, failures: &mut u64) {
    if socket.send_to(datagram, to).is_err() {
        *failures += 1;
    }
}
