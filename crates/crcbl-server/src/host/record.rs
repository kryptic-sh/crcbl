//! What a host's module is handed of its peers, tick by tick — the roster's
//! changes and each peer's frames — and the record of it a re-simulation
//! feeds back.
//!
//! **One step for both.** A live tick reads its peers' transports and moves
//! the frames each one's session queued into [`PeerFrames`]; a re-simulation
//! builds the same from a recording ([`Host::resimulate`](super::Host::resimulate)).
//! Either way the module is handed them by the same `Host::step`, which is
//! also what records them — so what is recorded is what the module read,
//! after the host's checks and its per-tick cap, and a replayed tick reaches
//! the module by the code a live one does.
//!
//! **The roster is recorded beside the frames**, because
//! [`PeerInputs::iter`](super::PeerInputs::iter) lists every admitted peer in
//! admission order, a lost one with nothing: a module that reads it reacts to
//! who is in the session, not only to what they sent. Each change is recorded
//! at the tick whose module first saw it — a join, a loss or a departure the
//! host noticed while reading its transports at that tick, or a kick or a
//! shutdown the game made between that tick and the one before.

use crcbl_core::TickId;

use super::{PeerEvent, PeerId};

/// A change to a host's roster, as its input record carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterChange {
    /// A new session was admitted, last in admission order
    /// ([`PeerEvent::Joined`]).
    Joined(PeerId),
    /// The peer's link dropped; it keeps its place, handed nothing
    /// ([`PeerEvent::Lost`]).
    Lost(PeerId),
    /// A lost peer came back to the same session ([`PeerEvent::Resumed`]).
    Resumed(PeerId),
    /// The host ended the session ([`PeerEvent::Left`]).
    Left(PeerId),
    /// The game ended the session: [`Host::kick`](super::Host::kick), or
    /// [`Host::shutdown`](super::Host::shutdown) for each peer it held. These
    /// raise no [`PeerEvent`], and change the roster all the same.
    Ended(PeerId),
}

impl RosterChange {
    /// The peer it changed for.
    #[must_use]
    pub const fn peer(self) -> PeerId {
        match self {
            Self::Joined(peer)
            | Self::Lost(peer)
            | Self::Resumed(peer)
            | Self::Left(peer)
            | Self::Ended(peer) => peer,
        }
    }

    /// The change `event` makes to the roster — none for a
    /// [`PeerEvent::Reaccepted`], whose session keeps its place and its link.
    const fn of(event: PeerEvent) -> Option<Self> {
        match event {
            PeerEvent::Joined(peer) => Some(Self::Joined(peer)),
            PeerEvent::Lost(peer) => Some(Self::Lost(peer)),
            PeerEvent::Resumed(peer) => Some(Self::Resumed(peer)),
            PeerEvent::Left(peer) => Some(Self::Left(peer)),
            PeerEvent::Reaccepted(_) => None,
        }
    }
}

/// One peer's input for one tick, as the module is handed it through
/// [`PeerInputs::iter`](super::PeerInputs::iter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerFrames {
    /// The peer.
    pub peer: PeerId,
    /// Its frames for this tick, in tick order: each the tick its client
    /// stamped on it and its bytes.
    pub frames: Vec<(TickId, Vec<u8>)>,
    /// How many further frames the host's per-tick cap refused.
    pub dropped: u32,
}

impl PeerFrames {
    /// `peer`, handed nothing.
    #[must_use]
    pub const fn none(peer: PeerId) -> Self {
        Self {
            peer,
            frames: Vec::new(),
            dropped: 0,
        }
    }

    /// Whether the peer was handed nothing: no frames, and none refused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty() && self.dropped == 0
    }
}

/// What a host's module was handed of its peers at one tick: the roster's
/// changes since the tick before, in the order the host applied them, and the
/// input of every peer handed any. A peer in the roster with no entry here
/// was handed nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickInputs {
    /// The tick whose module read it.
    pub tick: TickId,
    /// The roster's changes, in the order applied.
    pub roster: Vec<RosterChange>,
    /// Each peer handed anything, in admission order.
    pub peers: Vec<PeerFrames>,
}

/// The host's session changes: the events the game reads with
/// [`Host::events`](super::Host::events) and, while the host records its
/// peers' input, the roster's changes and the ticks recorded so far.
#[derive(Debug, Default)]
pub(super) struct PeerLog {
    events: Vec<PeerEvent>,
    record: Option<InputRecord>,
}

/// A host's input record, while it keeps one.
#[derive(Debug, Default)]
struct InputRecord {
    /// The roster's changes since the last step.
    roster: Vec<RosterChange>,
    ticks: Vec<TickInputs>,
}

impl PeerLog {
    /// Raise `event` for the game, and record its change to the roster.
    pub(super) fn push(&mut self, event: PeerEvent) {
        self.events.push(event);
        if let (Some(record), Some(change)) = (self.record.as_mut(), RosterChange::of(event)) {
            record.roster.push(change);
        }
    }

    /// Record that the game ended `peer`'s session, which raises no event.
    pub(super) fn ended(&mut self, peer: PeerId) {
        if let Some(record) = self.record.as_mut() {
            record.roster.push(RosterChange::Ended(peer));
        }
    }

    /// Take the events since the last call, oldest first.
    pub(super) fn drain(&mut self) -> std::vec::Drain<'_, PeerEvent> {
        self.events.drain(..)
    }

    /// Start recording, with `roster` — the peers already in session — as
    /// the changes the first recorded tick opens with. Does nothing while
    /// already recording.
    pub(super) fn start_recording(&mut self, roster: impl IntoIterator<Item = RosterChange>) {
        if self.record.is_none() {
            self.record = Some(InputRecord {
                roster: roster.into_iter().collect(),
                ticks: Vec::new(),
            });
        }
    }

    /// The roster's changes since the last step, for the live path to hand
    /// the next one — none while not recording.
    pub(super) fn take_roster(&mut self) -> Vec<RosterChange> {
        self.record
            .as_mut()
            .map(|record| std::mem::take(&mut record.roster))
            .unwrap_or_default()
    }

    /// Record what the module was handed at `tick`, while recording: the
    /// roster's changes and every peer handed anything. A tick with neither
    /// leaves no entry.
    pub(super) fn record_tick(
        &mut self,
        tick: TickId,
        roster: Vec<RosterChange>,
        peers: &[PeerFrames],
    ) {
        let Some(record) = self.record.as_mut() else {
            return;
        };
        let peers: Vec<PeerFrames> = peers
            .iter()
            .filter(|peer| !peer.is_empty())
            .cloned()
            .collect();
        if roster.is_empty() && peers.is_empty() {
            return;
        }
        record.ticks.push(TickInputs {
            tick,
            roster,
            peers,
        });
    }

    /// Take every tick recorded since the last take, in tick order, leaving
    /// the record empty and still recording.
    pub(super) fn take_recorded(&mut self) -> Vec<TickInputs> {
        self.record
            .as_mut()
            .map(|record| std::mem::take(&mut record.ticks))
            .unwrap_or_default()
    }

    /// Every tick recorded so far, in tick order.
    pub(super) fn recorded(&self) -> &[TickInputs] {
        self.record
            .as_ref()
            .map_or(&[], |record| record.ticks.as_slice())
    }
}
