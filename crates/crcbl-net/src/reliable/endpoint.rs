//! One peer's end of a reliability-layer link.
//!
//! An [`Endpoint`] is driven entirely by its caller, as a [`crate::Transport`]
//! is: datagrams in through [`Endpoint::receive_datagram`], datagrams out by
//! calling [`Endpoint::poll_outgoing`] until it returns `None`, messages out
//! through [`Endpoint::recv`]. Time comes from the injected [`Clock`] and
//! nowhere else, so a [`crate::ManualClock`] makes every timer here — resend,
//! keepalive, timeout — exact and reproducible.
//!
//! # For the `Transport` wrapper
//!
//! [`Endpoint::send`] on [`Channel::Reliable`] is `send_reliable`, on
//! [`Channel::UnreliableSequenced`] is `send_unreliable`, and its errors are
//! already [`TransportError`]s. [`Endpoint::recv_reliable`] is
//! `recv_reliable`; [`Endpoint::recv`] drains reliable deliveries first, as
//! `recv` must. [`EndpointState::is_connected`] is `is_connected`.
//!
//! # Bounded against a dead or hostile peer
//!
//! Every queue here has a cap and a named constant: reliable messages in
//! flight ([`RELIABLE_WINDOW`], [`MAX_RELIABLE_BYTES_IN_FLIGHT`]), unreliable
//! payloads awaiting a poll ([`MAX_QUEUED_UNRELIABLE`]), deliveries awaiting
//! [`Endpoint::recv`] ([`MAX_DELIVERED_MESSAGES`], [`MAX_DELIVERED_BYTES`]),
//! and reassembly, which shares the in-flight byte cap. A full send side is
//! [`TransportError::Backpressure`]; a full receive side refuses the reliable
//! packet unacknowledged, so the backpressure reaches the sender as resends.

use std::collections::VecDeque;
use std::time::Duration;

use super::ack::{ACK_BITS, Arrival, ReceivedWindow};
use super::fragment::{Filed, Reassembly};
use super::outgoing::SendBuffer;
use super::packet::{
    MAX_RELIABLE_MESSAGE_BYTES, MAX_UNRELIABLE_PAYLOAD, PacketAcks, PacketBody, PacketDecodeError,
    PacketHeader, decode_packet, encode_packet,
};
use super::rtt::RttEstimator;
use super::sequence::{HALF_RANGE, sequence_greater_than};
use super::window::{StatsWindow, WindowCounts};
use crate::{Clock, MessageKind, TransportError};

/// Reliable messages a sender may have unacknowledged, and so the width of the
/// receiver's reorder window.
///
/// A power of two so window slots stay distinct across the message id's wrap,
/// and far inside half the id space so ids in the window always compare
/// correctly. At a message per tick it is several seconds of commands, more
/// than any session survives stalled.
pub const RELIABLE_WINDOW: usize = 256;

/// Reliable payload bytes a sender may hold unacknowledged, which is also the
/// receiver's reassembly budget — the one bound is what keeps the other from
/// ever refusing an honest peer (see the `fragment` module).
///
/// A handful of largest messages, so a join-in-progress snapshot does not stop
/// the commands queued behind it.
pub const MAX_RELIABLE_BYTES_IN_FLIGHT: usize = 4 * MAX_RELIABLE_MESSAGE_BYTES;

/// Unreliable payloads queued and not yet polled out. A caller that polls
/// every frame never comes near it; one that stopped polling hears about it
/// as [`TransportError::Backpressure`].
pub const MAX_QUEUED_UNRELIABLE: usize = 256;

/// Delivered messages waiting for [`Endpoint::recv`] at which reliable
/// fragments start being refused. The queue can pass it by what reassembly
/// held at that moment — at most [`RELIABLE_WINDOW`] messages — and no
/// further.
pub const MAX_DELIVERED_MESSAGES: usize = 1024;

/// The same cap in payload bytes; the overshoot is at most
/// [`MAX_RELIABLE_BYTES_IN_FLIGHT`].
pub const MAX_DELIVERED_BYTES: usize = MAX_RELIABLE_BYTES_IN_FLIGHT;

/// Time without an ack-eliciting packet after which a keepalive goes out, so
/// the peer's timeout and this side's round-trip estimate stay fed. It runs
/// under steady unreliable traffic too: those packets are never acked
/// promptly, so they measure nothing. netcode.io's send rate for an idle
/// connection is the same ten a second.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(100);

/// Silence after which the peer is declared gone.
///
/// Between ENet's `ENET_PEER_TIMEOUT_MINIMUM` and `ENET_PEER_TIMEOUT_MAXIMUM`,
/// and many keepalive intervals long: a burst of loss or a stalled frame
/// cannot reach it, a peer whose process died does.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(10);

// A timeout a few keepalives long would fire on ordinary loss.
const _: () = assert!(PEER_TIMEOUT.as_millis() >= 50 * KEEPALIVE_INTERVAL.as_millis());

/// Copies of the disconnect packet sent on [`Endpoint::disconnect`]. It is
/// never acknowledged, so redundancy is the only delivery it gets; the count is
/// netcode.io's `NETCODE_NUM_DISCONNECT_PACKETS`.
pub const DISCONNECT_REDUNDANCY: u8 = 10;

/// Sent packets remembered for acknowledgement. A power of two for the same
/// reason as [`RELIABLE_WINDOW`], and wider than the most packets the
/// in-flight caps let out before the first ack can return.
const SENT_PACKET_WINDOW: usize = 1024;

/// Ack-eliciting packets received before the current ack field is snapshotted
/// for its own ack packet. Half the bitfield, so a burst longer than the field
/// is still acknowledged whole, by overlapping snapshots, rather than only its
/// tail.
const ACK_SNAPSHOT_INTERVAL: u16 = ACK_BITS / 2;

/// Snapshots held for sending; the oldest is dropped past this, and a
/// dropped one costs resends, never correctness.
const MAX_ACK_SNAPSHOTS: usize = 32;

/// A sent packet is declared lost once a packet this many sequences newer is
/// acknowledged: RFC 9002's `kPacketThreshold`. It feeds the loss estimate
/// only; resends run on the timeout.
const PACKET_LOSS_THRESHOLD: u16 = 3;

/// Weight of each packet's fate in the smoothed loss estimate.
const LOSS_SMOOTHING: f32 = 1.0 / 32.0;

/// How far behind the newest received packet the unreliable channel's
/// latest-wins mark may fall before it is dragged along. Without it, a
/// channel idle for half the sequence space would have its mark compare as
/// newer than every fresh packet, and drop them all.
const UNRELIABLE_REACH: u16 = HALF_RANGE / 2;

const _: () = assert!(RELIABLE_WINDOW.is_power_of_two() && RELIABLE_WINDOW < HALF_RANGE as usize);
const _: () = assert!(SENT_PACKET_WINDOW.is_power_of_two());
const _: () = assert!(SENT_PACKET_WINDOW > ACK_BITS as usize);

/// Which of the two channels a message travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Reliable-ordered: resent until acknowledged, delivered once and in
    /// order, fragmented when large.
    Reliable,
    /// Unreliable-sequenced: sent once, never fragmented, and dropped on
    /// arrival if anything newer on it was already delivered.
    UnreliableSequenced,
}

impl From<Channel> for MessageKind {
    fn from(channel: Channel) -> Self {
        match channel {
            Channel::Reliable => Self::Reliable,
            Channel::UnreliableSequenced => Self::Unreliable,
        }
    }
}

/// A message the peer sent, delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The channel it came on.
    pub channel: Channel,
    /// Its bytes, reassembled if it was split.
    pub payload: Vec<u8>,
}

/// Where the link stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointState {
    /// Exchanging packets.
    Connected,
    /// [`Endpoint::disconnect`] was called and the disconnect packets are
    /// still going out.
    Disconnecting,
    /// Closed from this side, every disconnect packet sent.
    Disconnected,
    /// The peer said it was leaving.
    PeerDisconnected,
    /// The peer went silent for [`PEER_TIMEOUT`] — gone without saying so.
    TimedOut,
}

impl EndpointState {
    /// Whether messages can still be sent and received.
    #[must_use]
    pub fn is_connected(self) -> bool {
        self == Self::Connected
    }
}

/// Why a datagram was not taken.
///
/// None of them changes the endpoint's state, and none acknowledges the
/// packet: a refused reliable fragment is resent by an honest peer.
#[derive(Debug, thiserror::Error)]
pub enum ReceiveError {
    /// Not a packet of ours.
    #[error(transparent)]
    Decode(#[from] PacketDecodeError),
    /// The link is no longer connected.
    #[error("the link is closed")]
    Closed,
    /// A reliable message id beyond anything an honest sender, held to
    /// [`RELIABLE_WINDOW`], could have reached.
    #[error("reliable message {message_id} is outside the window at {next_expected}")]
    OutOfWindow { message_id: u16, next_expected: u16 },
    /// A fragment disagreeing with an earlier one of the same message about
    /// how many pieces it has.
    #[error("message {message_id} was {expected} fragments, now claims {found}")]
    FragmentCountMismatch {
        message_id: u16,
        expected: usize,
        found: usize,
    },
    /// Holding this fragment would pass the reassembly budget.
    #[error("reassembly holds {buffered} of {limit} bytes")]
    ReassemblyFull { buffered: usize, limit: usize },
    /// The application has left [`MAX_DELIVERED_MESSAGES`] or
    /// [`MAX_DELIVERED_BYTES`] unread, so reliable fragments are refused
    /// until it reads; the sender feels it as resends and then as its own
    /// backpressure.
    #[error("{messages} delivered messages ({bytes} bytes) are waiting to be read")]
    DeliveriesFull { messages: usize, bytes: usize },
}

/// What the netgraph reads: the link's measured health and its counters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EndpointStats {
    /// Smoothed round trip, once one has been measured.
    pub rtt: Option<Duration>,
    /// Mean deviation of the round trip: jitter.
    pub rtt_variance: Duration,
    /// The current resend timeout, before backoff.
    pub rto: Duration,
    /// Smoothed fraction of sent packets lost, 0 to 1.
    pub packet_loss: f32,
    /// Packets sent, every kind.
    pub packets_sent: u64,
    /// Packets received and accepted.
    pub packets_received: u64,
    /// Sent packets the peer acknowledged.
    pub packets_acked: u64,
    /// Sent packets declared lost.
    pub packets_lost: u64,
    /// Reliable fragments sent again after their timeout.
    pub resends: u64,
    /// Datagram bytes sent.
    pub bytes_sent: u64,
    /// Datagram bytes received and accepted.
    pub bytes_received: u64,
    /// Unreliable messages dropped on arrival: stale, or no room to deliver.
    pub unreliable_dropped: u64,
    /// The same counts over the last [`super::STATS_WINDOW`] alone — where a
    /// rate, or a loss figure that has not been smoothing for minutes, is
    /// read from. See [`super::window`].
    pub recent: WindowCounts,
}

/// What the endpoint remembers about a packet it sent.
#[derive(Debug, Clone, Copy)]
struct SentPacket {
    sequence: u16,
    sent_at: Duration,
    /// Whether the peer acks it promptly, which is what makes its round trip a
    /// fair sample.
    ack_eliciting: bool,
    /// The reliable fragment it carried, as (message id, index).
    fragment: Option<(u16, u8)>,
    acked: bool,
    /// Counted as acknowledged or lost; each packet is counted once.
    counted: bool,
}

/// One peer's end of a link. See the module docs.
pub struct Endpoint<C: Clock> {
    protocol_id: u32,
    clock: C,
    state: EndpointState,
    next_sequence: u16,
    received: ReceivedWindow,
    sent: Vec<Option<SentPacket>>,
    largest_acked: Option<u16>,
    /// The oldest sent sequence not yet judged for loss.
    loss_scan_from: u16,
    rtt: RttEstimator,
    outgoing: SendBuffer,
    unreliable_queue: VecDeque<Vec<u8>>,
    reassembly: Reassembly,
    /// Latest-wins mark: the newest packet whose unreliable payload was
    /// delivered, or the floor [`UNRELIABLE_REACH`] dragged it to.
    unreliable_mark: Option<u16>,
    delivered: VecDeque<Delivery>,
    delivered_bytes: usize,
    ack_snapshots: VecDeque<PacketAcks>,
    /// Ack-eliciting packets received since the current ack field last went
    /// out.
    unacked_eliciting: u16,
    /// When an ack-eliciting packet last went out: the round-trip estimate
    /// is only fed by those.
    last_probe_sent: Duration,
    /// Whether [`Self::request_keepalive`] asked for a keepalive that has not
    /// gone out yet.
    probe_requested: bool,
    last_received: Duration,
    disconnects_left: u8,
    stats: EndpointStats,
    /// What `stats` counts, bucketed by when, for [`EndpointStats::recent`].
    window: StatsWindow,
}

impl<C: Clock> Endpoint<C> {
    /// A connected endpoint speaking `protocol_id`, timed by `clock`.
    ///
    /// Both ends of a link must be given the same protocol id; each refuses
    /// every datagram carrying another. The peer-timeout clock starts now.
    pub fn new(protocol_id: u32, clock: C) -> Self {
        Self::starting_at(protocol_id, clock, 0, 0)
    }

    /// The same, with the first packet sequence and first reliable message id
    /// chosen, so tests can start next to the wrap. The peer must start its
    /// reliable ids at the same `message_id`.
    pub(crate) fn starting_at(protocol_id: u32, clock: C, sequence: u16, message_id: u16) -> Self {
        let now = clock.now();
        Self {
            protocol_id,
            clock,
            state: EndpointState::Connected,
            next_sequence: sequence,
            received: ReceivedWindow::default(),
            sent: vec![None; SENT_PACKET_WINDOW],
            largest_acked: None,
            loss_scan_from: sequence,
            rtt: RttEstimator::default(),
            outgoing: SendBuffer::new(message_id),
            unreliable_queue: VecDeque::new(),
            reassembly: Reassembly::new(message_id),
            unreliable_mark: None,
            delivered: VecDeque::new(),
            delivered_bytes: 0,
            ack_snapshots: VecDeque::new(),
            unacked_eliciting: 0,
            last_probe_sent: now,
            probe_requested: false,
            last_received: now,
            disconnects_left: 0,
            stats: EndpointStats::default(),
            window: StatsWindow::default(),
        }
    }

    /// Where the link stands, as of the last [`Self::update`].
    #[must_use]
    pub fn state(&self) -> EndpointState {
        self.state
    }

    /// The netgraph's inputs, with [`EndpointStats::recent`] read at the
    /// clock's now.
    #[must_use]
    pub fn stats(&self) -> EndpointStats {
        EndpointStats {
            rtt: self.rtt.smoothed(),
            rtt_variance: self.rtt.variance(),
            rto: self.rtt.rto(),
            recent: self.window.read(self.clock.now()),
            ..self.stats
        }
    }

    /// Queue `payload` on `channel`.
    ///
    /// # Errors
    ///
    /// [`TransportError::Disconnected`] once the link is not connected;
    /// [`TransportError::MessageTooLarge`] for a reliable message past
    /// [`MAX_RELIABLE_MESSAGE_BYTES`], or an unreliable one past
    /// [`MAX_UNRELIABLE_PAYLOAD`] — that channel never fragments;
    /// [`TransportError::Backpressure`] when the channel's queue is full.
    /// Nothing is queued on any error.
    pub fn send(&mut self, channel: Channel, payload: Vec<u8>) -> Result<(), TransportError> {
        if !self.state.is_connected() {
            return Err(TransportError::Disconnected);
        }
        match channel {
            Channel::Reliable => self.outgoing.push(payload),
            Channel::UnreliableSequenced => {
                if payload.len() > MAX_UNRELIABLE_PAYLOAD {
                    return Err(TransportError::MessageTooLarge {
                        size: payload.len(),
                        limit: MAX_UNRELIABLE_PAYLOAD,
                    });
                }
                if self.unreliable_queue.len() >= MAX_QUEUED_UNRELIABLE {
                    return Err(TransportError::Backpressure);
                }
                self.unreliable_queue.push_back(payload);
                Ok(())
            }
        }
    }

    /// The next delivered message, reliable ones first.
    pub fn recv(&mut self) -> Option<Delivery> {
        let position = self
            .delivered
            .iter()
            .position(|delivery| delivery.channel == Channel::Reliable)
            .unwrap_or(0);
        self.take_delivery(position)
    }

    /// The next delivered reliable message, leaving unreliable ones queued.
    pub fn recv_reliable(&mut self) -> Option<Delivery> {
        let position = self
            .delivered
            .iter()
            .position(|delivery| delivery.channel == Channel::Reliable)?;
        self.take_delivery(position)
    }

    fn take_delivery(&mut self, position: usize) -> Option<Delivery> {
        let delivery = self.delivered.remove(position)?;
        self.delivered_bytes -= delivery.payload.len();
        Some(delivery)
    }

    /// Have the next [`Self::poll_outgoing`] with nothing else ack-eliciting
    /// to send emit a keepalive, without waiting for [`KEEPALIVE_INTERVAL`].
    ///
    /// For a transport that has just keyed a link and wants the peer to see a
    /// packet under the new key at once: the first keepalive is what tells a
    /// server the client really holds the key its hello offered.
    pub fn request_keepalive(&mut self) {
        self.probe_requested = true;
    }

    /// Close the link from this side: queued traffic is dropped and the next
    /// [`DISCONNECT_REDUNDANCY`] polls emit disconnect packets. The peer sees
    /// [`EndpointState::PeerDisconnected`], not a timeout.
    pub fn disconnect(&mut self) {
        if self.state.is_connected() {
            self.state = EndpointState::Disconnecting;
            self.disconnects_left = DISCONNECT_REDUNDANCY;
            self.outgoing.clear();
            self.unreliable_queue.clear();
            self.ack_snapshots.clear();
        }
    }

    /// Advance the timers: a peer silent for [`PEER_TIMEOUT`] is declared
    /// [`EndpointState::TimedOut`]. [`Self::poll_outgoing`] calls this
    /// itself.
    pub fn update(&mut self) {
        let now = self.clock.now();
        if self.state.is_connected() && now.saturating_sub(self.last_received) >= PEER_TIMEOUT {
            self.state = EndpointState::TimedOut;
        }
    }

    /// Take one datagram from the peer.
    ///
    /// # Errors
    ///
    /// See [`ReceiveError`]. An error leaves the endpoint as it was; the
    /// datagram may be dropped and the link carries on.
    pub fn receive_datagram(&mut self, datagram: &[u8]) -> Result<(), ReceiveError> {
        if !self.state.is_connected() {
            return Err(ReceiveError::Closed);
        }
        let (header, body) = decode_packet(datagram, self.protocol_id)?;
        if self.received.classify(header.sequence) == Arrival::Duplicate {
            return Ok(());
        }
        // The body goes first: a refused fragment must leave the packet
        // unrecorded, or the ack for it would tell the sender to stop
        // resending what was never kept.
        let ack_eliciting = match body {
            PacketBody::Ack => false,
            PacketBody::Keepalive => true,
            PacketBody::Reliable(fragment) => {
                if self.deliveries_full() {
                    return Err(ReceiveError::DeliveriesFull {
                        messages: self.delivered.len(),
                        bytes: self.delivered_bytes,
                    });
                }
                if self.reassembly.file(&fragment)? == Filed::Stored {
                    self.flush_reliable();
                }
                true
            }
            PacketBody::Unreliable(payload) => {
                self.accept_unreliable(header.sequence, payload);
                false
            }
            PacketBody::Disconnect => {
                self.state = EndpointState::PeerDisconnected;
                false
            }
        };

        let now = self.clock.now();
        self.received.record(header.sequence);
        self.drag_unreliable_mark();
        if let Some(acks) = header.acks {
            self.process_acks(acks, now);
        }
        self.last_received = now;
        self.stats.packets_received += 1;
        self.stats.bytes_received += datagram.len() as u64;
        self.window
            .count(now, |recent| recent.bytes_received += datagram.len() as u64);
        if ack_eliciting {
            self.unacked_eliciting += 1;
            if self.unacked_eliciting >= ACK_SNAPSHOT_INTERVAL {
                if let Some(acks) = self.received.acks() {
                    if self.ack_snapshots.len() >= MAX_ACK_SNAPSHOTS {
                        self.ack_snapshots.pop_front();
                    }
                    self.ack_snapshots.push_back(acks);
                }
                self.unacked_eliciting = 0;
            }
        }
        Ok(())
    }

    /// The next datagram to put on the wire, or `None` when there is nothing
    /// to send now. Call until `None` each time the caller's loop runs.
    ///
    /// In priority order: disconnect packets while disconnecting; snapshotted
    /// acks for a burst; queued unreliable payloads; reliable fragments due
    /// for their first send or a resend; an ack for anything received that
    /// asked for one; and a keepalive once nothing ack-eliciting has gone out
    /// for [`KEEPALIVE_INTERVAL`].
    pub fn poll_outgoing(&mut self) -> Option<Vec<u8>> {
        self.update();
        let now = self.clock.now();
        match self.state {
            EndpointState::Connected => {}
            EndpointState::Disconnecting => {
                self.disconnects_left = self.disconnects_left.saturating_sub(1);
                if self.disconnects_left == 0 {
                    self.state = EndpointState::Disconnected;
                }
                return Some(self.emit(PacketBody::Disconnect, now));
            }
            EndpointState::Disconnected
            | EndpointState::PeerDisconnected
            | EndpointState::TimedOut => return None,
        }

        if let Some(acks) = self.ack_snapshots.pop_front() {
            let header = self.header_with(Some(acks));
            let datagram = encode_packet(self.protocol_id, &header, &PacketBody::Ack);
            return Some(self.record_sent(datagram, false, None, now));
        }
        if let Some(payload) = self.unreliable_queue.pop_front() {
            return Some(self.emit(PacketBody::Unreliable(&payload), now));
        }

        let header = self.header_with(self.received.acks());
        if let Some(due) = self.outgoing.next_due(now, &self.rtt) {
            let fragment = (due.fragment.message_id, due.fragment.index);
            let resend = due.resend;
            let datagram = encode_packet(
                self.protocol_id,
                &header,
                &PacketBody::Reliable(due.fragment),
            );
            if resend {
                self.stats.resends += 1;
                self.window.count(now, |recent| recent.resends += 1);
            }
            self.unacked_eliciting = 0;
            return Some(self.record_sent(datagram, true, Some(fragment), now));
        }

        if self.unacked_eliciting > 0 {
            return Some(self.emit(PacketBody::Ack, now));
        }
        if self.probe_requested || now.saturating_sub(self.last_probe_sent) >= KEEPALIVE_INTERVAL {
            return Some(self.emit(PacketBody::Keepalive, now));
        }
        None
    }

    // ── private helpers ──────────────────────────────────────────────────

    fn header_with(&self, acks: Option<PacketAcks>) -> PacketHeader {
        PacketHeader {
            sequence: self.next_sequence,
            acks,
        }
    }

    /// Send a body that borrows nothing from `self`, with the current acks.
    fn emit(&mut self, body: PacketBody<'_>, now: Duration) -> Vec<u8> {
        let header = self.header_with(self.received.acks());
        let datagram = encode_packet(self.protocol_id, &header, &body);
        self.unacked_eliciting = 0;
        let ack_eliciting = matches!(body, PacketBody::Keepalive);
        self.record_sent(datagram, ack_eliciting, None, now)
    }

    /// Remember a packet just built, and advance the sequence.
    fn record_sent(
        &mut self,
        datagram: Vec<u8>,
        ack_eliciting: bool,
        fragment: Option<(u16, u8)>,
        now: Duration,
    ) -> Vec<u8> {
        let sequence = self.next_sequence;
        let slot = &mut self.sent[usize::from(sequence) % SENT_PACKET_WINDOW];
        // The packet a full window ago is out of reach of any ack now: if it
        // was never acknowledged, it is lost.
        if let Some(evicted) = slot.take()
            && !evicted.counted
        {
            count_fate(&mut self.stats, &mut self.window, now, true);
        }
        *slot = Some(SentPacket {
            sequence,
            sent_at: now,
            ack_eliciting,
            fragment,
            acked: false,
            counted: false,
        });
        self.next_sequence = sequence.wrapping_add(1);
        if ack_eliciting {
            self.last_probe_sent = now;
            self.probe_requested = false;
        }
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += datagram.len() as u64;
        self.window
            .count(now, |recent| recent.bytes_sent += datagram.len() as u64);
        datagram
    }

    fn process_acks(&mut self, acks: PacketAcks, now: Duration) {
        for sequence in acks.acknowledged() {
            let Some(record) = self.sent[usize::from(sequence) % SENT_PACKET_WINDOW].as_mut()
            else {
                continue;
            };
            if record.sequence != sequence || record.acked {
                continue;
            }
            record.acked = true;
            if !record.counted {
                record.counted = true;
                count_fate(&mut self.stats, &mut self.window, now, false);
            }
            if record.ack_eliciting {
                self.rtt.sample(now.saturating_sub(record.sent_at));
            }
            if let Some((message_id, index)) = record.fragment {
                self.outgoing.ack(message_id, index);
            }
            if self
                .largest_acked
                .is_none_or(|largest| sequence_greater_than(sequence, largest))
            {
                self.largest_acked = Some(sequence);
            }
        }
        self.detect_losses(now);
    }

    /// Declare lost every packet [`PACKET_LOSS_THRESHOLD`] or more behind the
    /// largest acknowledged that was never acknowledged itself.
    fn detect_losses(&mut self, now: Duration) {
        let Some(largest) = self.largest_acked else {
            return;
        };
        // Anything further back than the window was judged when it was
        // evicted.
        let oldest_held = self.next_sequence.wrapping_sub(SENT_PACKET_WINDOW as u16);
        if usize::from(self.next_sequence.wrapping_sub(self.loss_scan_from)) > SENT_PACKET_WINDOW {
            self.loss_scan_from = oldest_held;
        }
        let limit = largest.wrapping_sub(PACKET_LOSS_THRESHOLD);
        while !sequence_greater_than(self.loss_scan_from, limit) {
            let sequence = self.loss_scan_from;
            if let Some(record) = self.sent[usize::from(sequence) % SENT_PACKET_WINDOW].as_mut()
                && record.sequence == sequence
                && !record.counted
            {
                record.counted = true;
                count_fate(&mut self.stats, &mut self.window, now, true);
            }
            self.loss_scan_from = sequence.wrapping_add(1);
        }
    }

    fn accept_unreliable(&mut self, sequence: u16, payload: &[u8]) {
        let fresh = self
            .unreliable_mark
            .is_none_or(|mark| sequence_greater_than(sequence, mark));
        let room = self.delivered.len() < MAX_DELIVERED_MESSAGES
            && self.delivered_bytes + payload.len() <= MAX_DELIVERED_BYTES;
        if !fresh || !room {
            self.stats.unreliable_dropped += 1;
            return;
        }
        self.unreliable_mark = Some(sequence);
        self.delivered_bytes += payload.len();
        self.delivered.push_back(Delivery {
            channel: Channel::UnreliableSequenced,
            payload: payload.to_vec(),
        });
    }

    /// Keep the latest-wins mark within [`UNRELIABLE_REACH`] of the newest
    /// packet received.
    fn drag_unreliable_mark(&mut self) {
        if let (Some(mark), Some(newest)) = (self.unreliable_mark, self.received.newest())
            && newest.wrapping_sub(mark) > UNRELIABLE_REACH
            && sequence_greater_than(newest, mark)
        {
            self.unreliable_mark = Some(newest.wrapping_sub(UNRELIABLE_REACH));
        }
    }

    /// Whether the application has left the delivery queue at its cap.
    fn deliveries_full(&self) -> bool {
        self.delivered.len() >= MAX_DELIVERED_MESSAGES
            || self.delivered_bytes >= MAX_DELIVERED_BYTES
    }

    /// Move every complete in-order reliable message into the delivery queue.
    ///
    /// Unconditionally, which is what keeps the reorder window honest: a
    /// complete message is acknowledged, so the sender releases it and moves
    /// its own window on, and a receiver still holding it would soon be
    /// offered ids past the end of its window. The cap is enforced one step
    /// earlier instead — see [`ReceiveError::DeliveriesFull`] — so the queue
    /// can pass its cap by at most what reassembly held.
    fn flush_reliable(&mut self) {
        while let Some(payload) = self.reassembly.pop_ready() {
            self.delivered_bytes += payload.len();
            self.delivered.push_back(Delivery {
                channel: Channel::Reliable,
                payload,
            });
        }
    }

    // ── test access ──────────────────────────────────────────────────────

    /// Bytes held across every buffer that grows with traffic: reliable
    /// messages in flight, queued unreliable payloads, reassembly and
    /// deliveries.
    #[cfg(test)]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.outgoing.bytes()
            + self.unreliable_queue.iter().map(Vec::len).sum::<usize>()
            + self.reassembly.buffered_bytes()
            + self.delivered_bytes
    }

    /// The ack field the next packet out would carry.
    #[cfg(test)]
    pub(crate) fn current_acks(&self) -> Option<PacketAcks> {
        self.received.acks()
    }

    /// Deliveries waiting for [`Self::recv`].
    #[cfg(test)]
    pub(crate) fn delivered_len(&self) -> usize {
        self.delivered.len()
    }

    /// Reliable messages not yet fully acknowledged.
    #[cfg(test)]
    pub(crate) fn reliable_in_flight(&self) -> usize {
        self.outgoing.len()
    }
}

/// Count one sent packet as acknowledged or lost, at `now`, and fold it into
/// the loss estimate.
fn count_fate(stats: &mut EndpointStats, window: &mut StatsWindow, now: Duration, lost: bool) {
    if lost {
        stats.packets_lost += 1;
        window.count(now, |recent| recent.packets_lost += 1);
    } else {
        stats.packets_acked += 1;
        window.count(now, |recent| recent.packets_acked += 1);
    }
    let sample = if lost { 1.0 } else { 0.0 };
    stats.packet_loss += (sample - stats.packet_loss) * LOSS_SMOOTHING;
}

impl<C: Clock> std::fmt::Debug for Endpoint<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("protocol_id", &self.protocol_id)
            .field("state", &self.state)
            .field("next_sequence", &self.next_sequence)
            .field("reliable_in_flight", &self.outgoing.len())
            .field("delivered", &self.delivered.len())
            .finish_non_exhaustive()
    }
}
