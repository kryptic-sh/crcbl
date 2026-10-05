//! Re-simulating a recorded run: a fresh host fed a recording's simulation
//! sets and its peers' input, checked against its state hashes tick by tick.
//!
//! A recording's output entries are what a viewer plays back; they need no
//! simulation. A re-simulation instead runs the ticks again, so it reproduces
//! the recorded state only from the recorded inputs: the applied `Flags::SIM`
//! sets, which [`Host::replay_sim_record`] schedules, and what the module was
//! handed of its peers ([`Host::peer_input_record`]) — the roster and each
//! peer's frames, handed to the module through the step a live tick takes, so
//! a module reading [`PeerInputs::iter`](super::PeerInputs::iter) reads what
//! it read live — each peer's player included, which a re-simulation takes
//! from the peer's recorded join.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use crcbl_core::{PlayerId, TickId};
use crcbl_net::ConsoleSet;

use super::{AppliedSimSet, Host, PeerFrames, PeerId, RosterChange, TickInputs};
use crate::sim_hash::hash_world;

/// Why [`Host::resimulate`] did not reproduce a recording.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResimError {
    /// A recorded set this host refuses — a name its registry does not know,
    /// or a value its kind refuses — found before any tick runs.
    SetRefused {
        /// The tick it was recorded at.
        tick: TickId,
        /// The variable, as recorded.
        name: String,
        /// The registry's reason.
        reason: String,
    },
    /// A recorded set, state hash or tick of peer input for a tick this host
    /// has already passed, so it can neither apply nor compare it.
    TickPassed {
        /// The tick it was recorded at.
        tick: TickId,
        /// The tick this host stood at.
        host_tick: TickId,
    },
    /// A recorded roster change that cannot follow the ones before it, found
    /// before any tick runs.
    RosterRefused {
        /// The tick it was recorded at.
        tick: TickId,
        /// The change.
        change: RosterChange,
        /// What it breaks.
        fault: RosterFault,
    },
    /// Frames recorded for a peer the recorded roster cannot have handed
    /// them to, found before any tick runs.
    FramesRefused {
        /// The tick they were recorded at.
        tick: TickId,
        /// The peer they name.
        peer: PeerId,
        /// What they break.
        fault: FramesFault,
    },
    /// The first recorded state hash this host does not reproduce.
    Diverged {
        /// The tick whose end state differs.
        tick: TickId,
        /// The hash the recording holds.
        recorded: u64,
        /// The hash this host reached.
        resimulated: u64,
    },
}

/// Why a recorded roster change cannot follow the ones before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RosterFault {
    /// A join of a peer the roster already holds.
    AlreadyAdmitted,
    /// A join of a peer whose session ended earlier in the recording: a host
    /// never numbers two sessions alike.
    Reused,
    /// A change for a peer the roster does not hold.
    NotAdmitted,
    /// A loss of a peer already lost.
    AlreadyLost,
    /// A resume of a peer that is not lost.
    NotLost,
    /// A join naming a player another admitted peer is: a host holds one
    /// session a player.
    PlayerAdmitted,
}

/// Why recorded frames cannot have been handed to the peer they name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FramesFault {
    /// The roster does not hold the peer at that tick.
    NotAdmitted,
    /// The peer is lost at that tick, and a lost peer is handed nothing.
    Lost,
    /// The tick records the peer's frames twice.
    Twice,
}

impl fmt::Display for RosterFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AlreadyAdmitted => "the peer is already admitted",
            Self::Reused => "the peer's number belonged to a session that ended",
            Self::NotAdmitted => "the peer is not admitted",
            Self::AlreadyLost => "the peer is already lost",
            Self::NotLost => "the peer is not lost",
            Self::PlayerAdmitted => "another admitted peer is the same player",
        })
    }
}

impl fmt::Display for FramesFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAdmitted => "the peer is not admitted",
            Self::Lost => "the peer is lost",
            Self::Twice => "the tick records them twice",
        })
    }
}

impl fmt::Display for ResimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetRefused { tick, name, reason } => write!(
                f,
                "the set of `{name}` recorded for tick {} is refused: {reason}",
                tick.get()
            ),
            Self::TickPassed { tick, host_tick } => write!(
                f,
                "recorded for tick {}, which this host had passed at tick {}",
                tick.get(),
                host_tick.get()
            ),
            Self::RosterRefused {
                tick,
                change,
                fault,
            } => write!(
                f,
                "the roster change {change:?} recorded for tick {} is refused: {fault}",
                tick.get()
            ),
            Self::FramesRefused { tick, peer, fault } => write!(
                f,
                "the frames of {peer:?} recorded for tick {} are refused: {fault}",
                tick.get()
            ),
            Self::Diverged {
                tick,
                recorded,
                resimulated,
            } => write!(
                f,
                "diverged at tick {}: recorded state hash {recorded:016x}, re-simulated \
                 {resimulated:016x}",
                tick.get()
            ),
        }
    }
}

impl std::error::Error for ResimError {}

/// One admitted peer of a recording's roster.
#[derive(Debug)]
struct Seat {
    peer: PeerId,
    /// The player its join named.
    player: Option<PlayerId>,
    /// Whether its link is up.
    connected: bool,
}

/// The roster a recording's changes build, one change at a time.
#[derive(Debug, Default)]
struct Roster {
    /// Every admitted peer in admission order.
    peers: Vec<Seat>,
    /// Every peer that ever joined.
    joined: BTreeSet<PeerId>,
}

impl Roster {
    fn apply(&mut self, change: RosterChange) -> Result<(), RosterFault> {
        let peer = change.peer();
        let index = self.peers.iter().position(|seat| seat.peer == peer);
        match (change, index) {
            (RosterChange::Joined(..), Some(_)) => return Err(RosterFault::AlreadyAdmitted),
            (RosterChange::Joined(_, player), None) => {
                if !self.joined.insert(peer) {
                    return Err(RosterFault::Reused);
                }
                // A player whose session is lost comes back on a fresh one
                // only once the host has ended the old, so the roster never
                // holds one player twice. A join that names nobody is not
                // compared.
                if player.is_some() && self.peers.iter().any(|seat| seat.player == player) {
                    return Err(RosterFault::PlayerAdmitted);
                }
                self.peers.push(Seat {
                    peer,
                    player,
                    connected: true,
                });
            }
            (_, None) => return Err(RosterFault::NotAdmitted),
            (RosterChange::Lost(_), Some(index)) => {
                if !self.peers[index].connected {
                    return Err(RosterFault::AlreadyLost);
                }
                self.peers[index].connected = false;
            }
            (RosterChange::Resumed(_), Some(index)) => {
                if self.peers[index].connected {
                    return Err(RosterFault::NotLost);
                }
                self.peers[index].connected = true;
            }
            (RosterChange::Left(_) | RosterChange::Ended(_), Some(index)) => {
                self.peers.remove(index);
            }
        }
        Ok(())
    }

    /// What the module is handed at a tick that records `recorded` for its
    /// peers: every admitted peer in admission order, with its recorded
    /// frames or with nothing.
    fn hand(
        &self,
        tick: TickId,
        mut recorded: Vec<PeerFrames>,
    ) -> Result<Vec<PeerFrames>, ResimError> {
        let refuse = |peer, fault| ResimError::FramesRefused { tick, peer, fault };
        for (index, frames) in recorded.iter().enumerate() {
            if recorded[..index]
                .iter()
                .any(|seen| seen.peer == frames.peer)
            {
                return Err(refuse(frames.peer, FramesFault::Twice));
            }
            match self.peers.iter().find(|seat| seat.peer == frames.peer) {
                None => return Err(refuse(frames.peer, FramesFault::NotAdmitted)),
                Some(seat) if !seat.connected => {
                    return Err(refuse(frames.peer, FramesFault::Lost));
                }
                Some(_) => {}
            }
        }
        Ok(self
            .peers
            .iter()
            .map(|seat| {
                recorded
                    .iter()
                    .position(|frames| frames.peer == seat.peer)
                    .map_or_else(
                        || PeerFrames::none(seat.peer),
                        |at| recorded.swap_remove(at),
                    )
            })
            .collect())
    }

    /// Every admitted peer's player, in admission order: what the module is
    /// handed beside [`hand`](Self::hand)'s frames.
    fn players(&self) -> Vec<Option<PlayerId>> {
        self.peers.iter().map(|seat| seat.player).collect()
    }
}

/// One recorded tick of peer input, checked and laid out as the module is
/// handed it.
struct PlannedTick {
    tick: TickId,
    roster: Vec<RosterChange>,
    /// Every admitted peer after `roster`, in admission order.
    peers: Vec<PeerFrames>,
    /// The player of each of `peers`, by index.
    players: Vec<Option<PlayerId>>,
}

/// What the module is handed at a tick the recording has no entry for: the
/// roster as the latest planned tick left it, each peer handed nothing.
#[derive(Default)]
struct Idle {
    peers: Vec<PeerFrames>,
    players: Vec<Option<PlayerId>>,
}

impl Host {
    /// Re-simulate a recording: schedule its `sets` through
    /// [`replay_sim_record`](Self::replay_sim_record), then run tick by tick to
    /// the last of its `hashes`, handing the module its peers' recorded
    /// `inputs` — [`peer_input_record`](Self::peer_input_record) — and
    /// comparing this host's state hash ([`hash_world`]) at the end of each
    /// tick one names. Answers the tick it stopped at.
    ///
    /// A host built like the recorded one — the same world, module and
    /// registry, from the same tick — and given its applied sets in order,
    /// its input record and the hashes `hash_world` took at the end of those
    /// ticks reproduces every one. A recording with no hashes runs no ticks.
    ///
    /// The peers the module is handed come from `inputs` alone, starting from
    /// no peers: the ticks run through the step a live tick takes once it has
    /// read its transports, without reading them, so this host's own
    /// transports, if it has any, are neither read nor admitted while it
    /// runs. While it is recording ([`record_peer_inputs`](Self::record_peer_inputs)),
    /// it records the replayed input again.
    ///
    /// # Errors
    ///
    /// Every set and every tick of input is checked before any tick runs, so a
    /// set this host refuses ([`ResimError::SetRefused`]), a roster change
    /// that cannot follow the ones before it ([`ResimError::RosterRefused`]),
    /// frames for a peer the roster cannot hand them to
    /// ([`ResimError::FramesRefused`]) or a set or input for a tick it has
    /// passed ([`ResimError::TickPassed`]) runs nothing. Ticks of input must
    /// be in tick order, one a tick. A hash for a tick already passed is
    /// [`ResimError::TickPassed`] too, so `hashes` must be in tick order. The
    /// first hash not reproduced stops the run there
    /// ([`ResimError::Diverged`]), leaving this host at that tick.
    pub fn resimulate(
        &mut self,
        sets: impl IntoIterator<Item = (TickId, ConsoleSet)>,
        hashes: impl IntoIterator<Item = (TickId, u64)>,
        inputs: impl IntoIterator<Item = TickInputs>,
    ) -> Result<TickId, ResimError> {
        let mut record = Vec::new();
        for (tick, set) in sets {
            // A set applies at the start of its tick, so the tick this host
            // stands at — already started and finished — is passed too.
            if tick <= self.tick_id() {
                return Err(ResimError::TickPassed {
                    tick,
                    host_tick: self.tick_id(),
                });
            }
            let checked = self
                .sim
                .check(&set)
                .map_err(|reason| ResimError::SetRefused {
                    tick,
                    name: set.name,
                    reason,
                })?;
            record.push(AppliedSimSet { tick, set: checked });
        }
        let mut plan = self.plan_inputs(inputs)?;
        self.replay_sim_record(record);

        let mut idle = Idle::default();
        for (tick, recorded) in hashes {
            while self.tick_id() < tick {
                self.step_replayed(&mut plan, &mut idle);
            }
            if self.tick_id() != tick {
                return Err(ResimError::TickPassed {
                    tick,
                    host_tick: self.tick_id(),
                });
            }
            let resimulated = hash_world(&self.world, tick);
            if resimulated != recorded {
                return Err(ResimError::Diverged {
                    tick,
                    recorded,
                    resimulated,
                });
            }
        }
        Ok(self.tick_id())
    }

    /// Check every tick of `inputs` against the roster the ones before it
    /// built, and lay each out as the module is handed it.
    fn plan_inputs(
        &self,
        inputs: impl IntoIterator<Item = TickInputs>,
    ) -> Result<VecDeque<PlannedTick>, ResimError> {
        let mut roster = Roster::default();
        let mut plan = VecDeque::new();
        let mut previous = self.tick_id();
        for input in inputs {
            // Like a set, a tick's input is read at its start.
            if input.tick <= previous {
                return Err(ResimError::TickPassed {
                    tick: input.tick,
                    host_tick: previous,
                });
            }
            previous = input.tick;
            for &change in &input.roster {
                roster
                    .apply(change)
                    .map_err(|fault| ResimError::RosterRefused {
                        tick: input.tick,
                        change,
                        fault,
                    })?;
            }
            let peers = roster.hand(input.tick, input.peers)?;
            plan.push_back(PlannedTick {
                tick: input.tick,
                roster: input.roster,
                peers,
                players: roster.players(),
            });
        }
        Ok(plan)
    }

    /// Run one more tick from `plan`: its planned input if it has one, and
    /// `idle` — the roster as the last planned tick left it — if not.
    fn step_replayed(&mut self, plan: &mut VecDeque<PlannedTick>, idle: &mut Idle) {
        // One period past the last update is exactly one more tick: the
        // clock's remainder is under a period, and its catch-up cap is at
        // least one.
        self.now += self.clock.tick_dt();
        self.clock.update(self.now);
        while self.clock.consume_tick() {
            let tick = self.clock.tick();
            if plan.front().is_some_and(|planned| planned.tick == tick)
                && let Some(PlannedTick {
                    roster,
                    peers,
                    players,
                    ..
                }) = plan.pop_front()
            {
                self.step(roster, &peers, &players);
                *idle = Idle {
                    peers: peers
                        .into_iter()
                        .map(|frames| PeerFrames::none(frames.peer))
                        .collect(),
                    players,
                };
            } else {
                self.step(Vec::new(), &idle.peers, &idle.players);
            }
        }
    }
}
