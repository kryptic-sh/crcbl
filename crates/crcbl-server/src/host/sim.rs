//! A host's tick boundary for `Flags::SIM` console sets: who may make one,
//! when it applies, and the stream that records it.
//!
//! **The host's console is the authority, and so is the listen host's own
//! player.** A set arrives from one of two places: the host's own console
//! ([`Host::submit_console_set`](super::Host::submit_console_set)) — the
//! keyboard of a listen host, or a dedicated server's operator — or a peer's
//! sealed `ClientToServer::Command`. Only a peer added with
//! [`Host::add_host_player`](super::Host::add_host_player) is the host: a
//! listen host's own player, connected over its in-memory pair. Every other
//! peer is refused, with the reason sent back, so a dedicated server — which
//! adds no host player — takes sets from its console alone.
//!
//! **A set applies at the start of the next tick, in the order the host read
//! them**: sets from the console between two ticks, then each peer's in the
//! order its messages were drained, peers in admission order. Nothing applies
//! when the line is typed or the message read, so a tick never sees a value
//! change part-way through.
//!
//! **Every applied set is recorded with its tick** ([`AppliedSimSet`]), and a
//! fresh host handed that record
//! ([`Host::replay_sim_record`](super::Host::replay_sim_record)) applies each
//! entry at the start of the tick it names — before any live set of that
//! tick — through the same checks, so the same world and module reach the same
//! state. [`Host::resimulate`](super::Host::resimulate) does that from a
//! replay file's sets, checked against its state hashes.

use std::collections::VecDeque;

use crcbl_console::{Registry, SimSet, SimVars};
use crcbl_core::TickId;
use crcbl_net::{ConsoleOutcome, ConsoleReply, ConsoleSet};

use super::PeerId;

/// The refusal a peer that is not the host is sent.
pub(super) const NOT_THE_HOST: &str =
    "only the host may set simulation variables, and this client is not the host";

/// The refusal for a set made of a host with no simulation variables.
const NO_SIM_VARIABLES: &str = "this host takes no simulation variables";

/// One set a host applied, as its replay stream records it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedSimSet {
    /// The tick whose start it was applied at — the first tick that read it.
    pub tick: TickId,
    /// What was set.
    pub set: SimSet,
}

/// Where a set came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The host's own console.
    Console,
    /// A peer's command.
    Peer(PeerId),
}

/// The registry a host checks sets against, the values its module reads, and
/// the sets waiting for, and recorded at, its tick boundary.
#[derive(Debug, Default)]
pub(super) struct SimConsole {
    /// `None` until the game hands one over; every set is refused until then.
    registry: Option<Registry>,
    vars: SimVars,
    /// Live sets, in the order the host read them.
    queue: Vec<(Origin, ConsoleSet)>,
    /// A record being replayed, in tick order.
    scheduled: VecDeque<AppliedSimSet>,
    /// Every set applied, in the order applied.
    record: Vec<AppliedSimSet>,
    /// Answers to the console's own sets, waiting to be taken.
    console_replies: Vec<ConsoleReply>,
}

impl SimConsole {
    /// Take sets against `registry`, every variable starting at its default.
    pub(super) fn set_registry(&mut self, registry: Registry) {
        self.vars = SimVars::new(&registry);
        self.registry = Some(registry);
    }

    pub(super) const fn vars(&self) -> &SimVars {
        &self.vars
    }

    pub(super) fn record(&self) -> &[AppliedSimSet] {
        &self.record
    }

    /// Queue `set` for the next tick boundary.
    pub(super) fn submit(&mut self, origin: Origin, set: ConsoleSet) {
        self.queue.push((origin, set));
    }

    /// Schedule a record to replay, each entry at the start of its tick.
    pub(super) fn schedule(&mut self, record: impl IntoIterator<Item = AppliedSimSet>) {
        self.scheduled.extend(record);
        // Stable, so two sets recorded in one tick keep their order.
        self.scheduled
            .make_contiguous()
            .sort_by_key(|applied| applied.tick);
    }

    pub(super) fn take_console_replies(&mut self) -> Vec<ConsoleReply> {
        std::mem::take(&mut self.console_replies)
    }

    /// The tick boundary of `tick`: the record's entries for it, then every
    /// live set, each checked, applied and recorded or refused. The console's
    /// answers are kept; the peers' are returned for the host to send.
    pub(super) fn begin_tick(
        &mut self,
        tick: TickId,
        is_host_player: impl Fn(PeerId) -> bool,
    ) -> Vec<(PeerId, ConsoleReply)> {
        while let Some(next) = self.scheduled.front() {
            if next.tick > tick {
                break;
            }
            let Some(applied) = self.scheduled.pop_front() else {
                break;
            };
            let set = ConsoleSet {
                name: applied.set.name().to_owned(),
                value: applied.set.value_text(),
            };
            let reply = if applied.tick < tick {
                refusal(
                    set,
                    format!(
                        "recorded for tick {}, which this host had passed when the record \
                         was handed over",
                        applied.tick.get()
                    ),
                )
            } else {
                self.apply(set, tick)
            };
            self.console_replies.push(reply);
        }

        let mut to_peers = Vec::new();
        for (origin, set) in std::mem::take(&mut self.queue) {
            let reply = match origin {
                Origin::Peer(peer) if !is_host_player(peer) => {
                    refusal(set, NOT_THE_HOST.to_owned())
                }
                Origin::Console | Origin::Peer(_) => self.apply(set, tick),
            };
            match origin {
                Origin::Console => self.console_replies.push(reply),
                Origin::Peer(peer) => to_peers.push((peer, reply)),
            }
        }
        to_peers
    }

    /// Check `set` against the registry, answering why it is refused.
    pub(super) fn check(&self, set: &ConsoleSet) -> Result<SimSet, String> {
        let Some(registry) = &self.registry else {
            return Err(NO_SIM_VARIABLES.to_owned());
        };
        registry
            .sim_set(&set.name, &set.value)
            .map_err(|fault| fault.message().to_owned())
    }

    /// Check `set` against the registry and, when it passes, apply and record
    /// it at `tick`.
    fn apply(&mut self, set: ConsoleSet, tick: TickId) -> ConsoleReply {
        let checked = match self.check(&set) {
            Ok(checked) => checked,
            Err(reason) => return refusal(set, reason),
        };
        if let Err(fault) = self.vars.apply(&checked) {
            return refusal(set, fault.message().to_owned());
        }
        let reply = ConsoleReply {
            name: checked.name().to_owned(),
            value: set.value,
            outcome: ConsoleOutcome::Applied(tick),
        };
        self.record.push(AppliedSimSet { tick, set: checked });
        reply
    }
}

fn refusal(set: ConsoleSet, reason: String) -> ConsoleReply {
    ConsoleReply {
        name: set.name,
        value: set.value,
        outcome: ConsoleOutcome::Refused(reason),
    }
}
