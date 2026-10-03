//! Re-simulating a recorded run: a fresh host fed a recording's simulation
//! sets, checked against its state hashes tick by tick.
//!
//! A recording's output entries are what a viewer plays back; they need no
//! simulation. A re-simulation instead runs the ticks again, so it reproduces
//! the recorded state only from the recorded inputs — today the applied
//! `Flags::SIM` sets, which [`Host::replay_sim_record`] schedules. Peers' input
//! frames are not recorded, so a host whose module reads them
//! ([`PeerInputs::iter`](super::PeerInputs::iter)) re-simulates as if every
//! peer sent nothing, and diverges wherever one did.

use std::fmt;

use crcbl_core::TickId;
use crcbl_net::ConsoleSet;

use super::{AppliedSimSet, Host};
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
    /// A recorded set or state hash for a tick this host has already passed,
    /// so it can neither apply nor compare it.
    TickPassed {
        /// The tick it was recorded at.
        tick: TickId,
        /// The tick this host stood at.
        host_tick: TickId,
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

impl Host {
    /// Re-simulate a recording: schedule its `sets` through
    /// [`replay_sim_record`](Self::replay_sim_record), then run tick by tick to
    /// the last of its `hashes`, comparing this host's state hash
    /// ([`hash_world`]) at the end of each tick one names. Answers the tick it
    /// stopped at.
    ///
    /// A host built like the recorded one — the same world, module and
    /// registry, from the same tick — and given its applied sets in order and
    /// the hashes `hash_world` took at the end of those ticks reproduces every
    /// one. A recording with no hashes runs no ticks.
    ///
    /// # Errors
    ///
    /// Every set is checked before any tick runs, so a set this host refuses
    /// ([`ResimError::SetRefused`]) or one for a tick it has passed
    /// ([`ResimError::TickPassed`]) schedules nothing. A hash for a tick
    /// already passed is [`ResimError::TickPassed`] too, so `hashes` must be
    /// in tick order. The first hash not reproduced stops the run there
    /// ([`ResimError::Diverged`]), leaving this host at that tick.
    pub fn resimulate(
        &mut self,
        sets: impl IntoIterator<Item = (TickId, ConsoleSet)>,
        hashes: impl IntoIterator<Item = (TickId, u64)>,
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
        self.replay_sim_record(record);

        for (tick, recorded) in hashes {
            while self.tick_id() < tick {
                // One period past the last update is exactly one more tick: the
                // clock's remainder is under a period, and its catch-up cap is
                // at least one.
                self.update(self.now + self.clock.tick_dt());
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
}
