//! The snapshot budget — which changes one snapshot carries when they do not
//! all fit.
//!
//! A snapshot rides the unreliable channel, and on a network transport that
//! channel never fragments: [`crate::reliable::MAX_UNRELIABLE_PAYLOAD`] is one
//! datagram. A diff against the client's baseline can be longer than that —
//! a join, or any sector with more moving entities than a datagram holds — so
//! the server does not send the diff as it stands. It fits it:
//!
//! * [`snapshot_budget`] turns a transport's unreliable limit into the most
//!   encoded delta bytes one snapshot may take.
//! * [`PriorityAccumulator::fit`] keeps the updates that matter most within
//!   it and holds the rest back. One accumulator exists per (client, sector),
//!   held by [`crate::SessionManager`] beside that sector's baselines.
//! * [`Fitted::baseline_after`] is the baseline the client holds once it
//!   applies what was sent — what the server retains for that tick, in place
//!   of the full state it did not send.
//!
//! # The priority accumulator
//!
//! After the TRIBES Engine Networking Model (Frohnmayer & Gift): each tick
//! every pending update's priority grows by its relevance, the highest are
//! packed until the packet is full, and a packed update's priority returns to
//! zero. Staleness is what the accumulation measures — an update held back
//! `k` ticks carries `k` × its relevance — so what matters ships every tick
//! and the long tail rotates through the room left over, each part of it in
//! turn. An update is one entity's entry in one system: the delta's own
//! granularity, since whole components travel on change.
//!
//! Relevance is [`DEFAULT_RELEVANCE`] for every update until something
//! supplies more: `fit` takes it as a function of the system and entity, so a
//! game's interest model can plug in without the accumulator changing. It is
//! a [`NonZeroU32`] because a relevance of zero would never accumulate, and an
//! update that never accumulates is one held back for ever.
//!
//! # Why holding an update back loses nothing
//!
//! The ack-baseline rule in `docs/notes/simulation.md` (_What the deleted
//! 23-netcode plan left behind_) does the work: every snapshot is a diff
//! against what the client has acknowledged, and a value the client does not
//! hold still differs from its baseline, so it is included again next tick.
//! An update held back is exactly that case. What makes it hold is the
//! baseline the server retains — [`Fitted::baseline_after`], never the full
//! state — because retaining the full state would record the held-back
//! update as delivered, and the next diff would call it unchanged.
//!
//! * **A held-back change is never read as unchanged or removed.** It simply
//!   is not in the delta; the client keeps its old value, and the retained
//!   baseline keeps it too.
//! * **Removals are packed before any change**, in entity order. A removal
//!   costs a bare id, so every one ships unless the tick's removals alone
//!   overflow the budget; the overflow is deferred to the next snapshot the
//!   same way — the entity stays in the retained baseline, so the next diff
//!   removes it again. A deferred removal is late, never lost.
//! * **A keyframe is fitted like a delta**: its entries are all additions, so
//!   the ones held back are added by the deltas that follow once the client
//!   acknowledges it. For a join that loses nothing — the client held nothing.
//!   A recovery keyframe (acks stalled, or the client's baseline evicted)
//!   resets a client whose state is already known to be out of step, and it
//!   withdraws what it holds back until those deltas add it again: a
//!   partial keyframe cannot say "keep the rest", and state never rides the
//!   reliable channel.
//! * **What cannot be held back** is the delta's framing — its header and one
//!   header per system — and any single update too long to fit beside it.
//!   Such a snapshot could never be sent whole or in part, so `fit` refuses it
//!   with [`BudgetTooSmall`] rather than leave one entity stale for good in
//!   silence.
//!
//! # Determinism
//!
//! The same delta, accumulator and budget give the same snapshot, byte for
//! byte. [`Baseline`] keeps entities in hash maps whose iteration order is
//! seeded per map, so `fit` sorts every list by entity bits before choosing,
//! and breaks ties in priority by entity bits, then system id. Without that,
//! which entities a client sees fresh on a given tick would differ run to run
//! for one server state. The determinism rules in `docs/notes/simulation.md`
//! (same input, same hash) bind the simulation; holding its wire output to
//! the same standard is what lets a test, or a recorded session, reproduce
//! what a client was sent.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use crcbl_core::TickId;

use crate::auth::AUTH_OVERHEAD;
use crate::delta::{
    Baseline, BaselineDecodeError, DELTA_HEADER_BYTES, Delta, DeltaCodec,
    ENTITY_ENTRY_HEADER_BYTES, MAX_DELTA_BYTES, REMOVED_ENTRY_BYTES, SYSTEM_HEADER_BYTES, Trust,
    encoded_delta_len,
};
use crate::types::EntityBits;

/// The relevance of an update nothing has said more about: the unit every
/// tick of staleness adds.
pub const DEFAULT_RELEVANCE: NonZeroU32 = NonZeroU32::MIN;

/// The most encoded delta bytes one snapshot may take on a transport whose
/// unreliable channel accepts `max_unreliable_message_bytes`.
///
/// Sealing adds [`AUTH_OVERHEAD`], so that comes off first — the same reason
/// [`MAX_DELTA_BYTES`] subtracts it from the in-memory limit — and the result
/// never exceeds `MAX_DELTA_BYTES`, which the encoder enforces whatever the
/// transport.
#[must_use]
pub const fn snapshot_budget(max_unreliable_message_bytes: usize) -> usize {
    let budget = max_unreliable_message_bytes.saturating_sub(AUTH_OVERHEAD);
    if budget < MAX_DELTA_BYTES {
        budget
    } else {
        MAX_DELTA_BYTES
    }
}

/// A snapshot whose unsheddable part — the delta's framing, plus the one
/// update too long to fit beside it — does not fit the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the smallest snapshot carrying what cannot be held back encodes to {size} bytes, \
     past its {budget}-byte budget"
)]
pub struct BudgetTooSmall {
    /// Encoded length of the smallest snapshot that would carry it.
    pub size: usize,
    /// The budget it was fitted to.
    pub budget: usize,
}

/// A delta fitted to a budget, and what was held back to fit it.
#[derive(Debug, Clone)]
pub struct Fitted {
    /// What to send: the diff with the held-back entries taken out and every
    /// list in entity order.
    pub delta: Delta,
    /// Added or modified entries held back for a later snapshot.
    pub shed: usize,
    /// Removals deferred to a later snapshot because the tick's removals alone
    /// overflowed the budget.
    pub deferred_removals: usize,
}

impl Fitted {
    /// Whether nothing was held back, so the snapshot is the whole diff.
    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.shed == 0 && self.deferred_removals == 0
    }

    /// The baseline the client holds once it applies [`Fitted::delta`] to
    /// `previous` — the baseline the delta was encoded against, `None` for a
    /// keyframe.
    ///
    /// This, and not the state the delta was diffed from, is what the server
    /// retains for the tick whenever something was held back: the held-back
    /// updates have to go on differing from it. It is computed by
    /// [`DeltaCodec::apply`], the client's own path, so the two cannot
    /// disagree about what a delta leaves behind.
    ///
    /// # Errors
    ///
    /// Whatever [`DeltaCodec::apply`] refuses — in practice, a `previous` that
    /// is not the baseline the delta names.
    pub fn baseline_after(
        &self,
        previous: Option<&Baseline>,
    ) -> Result<Baseline, BaselineDecodeError> {
        let mut baseline = match previous {
            Some(previous) if !self.delta.is_keyframe => previous.clone(),
            // A keyframe replaces everything, so it applies to nothing — the
            // empty baseline a client starts from.
            _ => Baseline::from_snapshot(TickId::ZERO, &[], Trust::Authenticated)?,
        };
        DeltaCodec::apply(&self.delta, &mut baseline, Trust::Authenticated)?;
        Ok(baseline)
    }
}

/// One (client, sector)'s accumulated priorities: the updates the last fit
/// held back, each with the priority it carries into the next.
///
/// See the [module docs](self) for the model.
#[derive(Debug, Default)]
pub struct PriorityAccumulator {
    /// `(entity_bits, system_id) → priority` for every update held back last
    /// fit. An update absent here has priority zero: it was packed, or it was
    /// not pending.
    held: HashMap<(EntityBits, u32), u64>,
}

/// One added or modified entry competing for room.
struct Candidate {
    priority: u64,
    entity_bits: EntityBits,
    system_id: u32,
    system_index: usize,
    bytes: usize,
}

impl PriorityAccumulator {
    /// An accumulator holding nothing back.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The priority the update for `entity_bits` in `system_id` carries into
    /// the next fit: zero unless the last fit held it back.
    #[must_use]
    pub fn priority(&self, system_id: u32, entity_bits: EntityBits) -> u64 {
        self.held
            .get(&(entity_bits, system_id))
            .copied()
            .unwrap_or(0)
    }

    /// Fit `delta` to `budget` encoded bytes.
    ///
    /// Every added or modified entry's priority grows by `relevance` of it;
    /// removals are packed first, then entries in descending priority, each
    /// one that fits what is left; packed entries return to priority zero and
    /// the rest keep what they have accumulated. A delta that already fits is
    /// returned whole, in entity order. Priorities saturate rather than wrap.
    ///
    /// # Errors
    ///
    /// [`BudgetTooSmall`] when the delta's framing, or its framing plus any
    /// single entry, exceeds `budget`: that entry could never be sent. The
    /// accumulator is left as it was.
    pub fn fit(
        &mut self,
        mut delta: Delta,
        budget: usize,
        relevance: impl Fn(u32, EntityBits) -> NonZeroU32,
    ) -> Result<Fitted, BudgetTooSmall> {
        for system in &mut delta.systems {
            system
                .added
                .sort_unstable_by_key(|entity| entity.entity_bits);
            system
                .modified
                .sort_unstable_by_key(|entity| entity.entity_bits);
            system.removed.sort_unstable();
        }

        let framing = SYSTEM_HEADER_BYTES
            .saturating_mul(delta.systems.len())
            .saturating_add(DELTA_HEADER_BYTES);
        let largest_entry = delta
            .systems
            .iter()
            .flat_map(|system| {
                let removal = (!system.removed.is_empty()).then_some(REMOVED_ENTRY_BYTES);
                system
                    .added
                    .iter()
                    .chain(&system.modified)
                    .map(|entity| ENTITY_ENTRY_HEADER_BYTES + entity.data.len())
                    .chain(removal)
            })
            .max()
            .unwrap_or(0);
        let unsheddable = framing.saturating_add(largest_entry);
        if unsheddable > budget {
            return Err(BudgetTooSmall {
                size: unsheddable,
                budget,
            });
        }

        if encoded_delta_len(&delta).is_some_and(|len| len <= budget) {
            // Everything pending is packed, so nothing carries a priority on.
            self.held.clear();
            return Ok(Fitted {
                delta,
                shed: 0,
                deferred_removals: 0,
            });
        }

        let mut room = budget - framing;
        let mut kept: HashSet<(usize, EntityBits)> = HashSet::new();

        let mut removals: Vec<(EntityBits, u32, usize)> = delta
            .systems
            .iter()
            .enumerate()
            .flat_map(|(index, system)| {
                system
                    .removed
                    .iter()
                    .map(move |&entity_bits| (entity_bits, system.system_id, index))
            })
            .collect();
        removals.sort_unstable();
        let mut deferred_removals = 0;
        for (entity_bits, _, system_index) in removals {
            if REMOVED_ENTRY_BYTES <= room {
                room -= REMOVED_ENTRY_BYTES;
                kept.insert((system_index, entity_bits));
            } else {
                deferred_removals += 1;
            }
        }

        let mut candidates: Vec<Candidate> = delta
            .systems
            .iter()
            .enumerate()
            .flat_map(|(system_index, system)| {
                system
                    .added
                    .iter()
                    .chain(&system.modified)
                    .map(move |entity| (system_index, system.system_id, entity))
            })
            .map(|(system_index, system_id, entity)| Candidate {
                priority: self
                    .priority(system_id, entity.entity_bits)
                    .saturating_add(u64::from(relevance(system_id, entity.entity_bits).get())),
                entity_bits: entity.entity_bits,
                system_id,
                system_index,
                bytes: ENTITY_ENTRY_HEADER_BYTES + entity.data.len(),
            })
            .collect();
        candidates.sort_unstable_by_key(|candidate| {
            (
                Reverse(candidate.priority),
                candidate.entity_bits,
                candidate.system_id,
            )
        });

        let mut held = HashMap::new();
        for candidate in candidates {
            if candidate.bytes <= room {
                room -= candidate.bytes;
                kept.insert((candidate.system_index, candidate.entity_bits));
            } else {
                held.insert(
                    (candidate.entity_bits, candidate.system_id),
                    candidate.priority,
                );
            }
        }
        let shed = held.len();
        self.held = held;

        for (index, system) in delta.systems.iter_mut().enumerate() {
            let keep = |entity_bits: EntityBits| kept.contains(&(index, entity_bits));
            system.added.retain(|entity| keep(entity.entity_bits));
            system.modified.retain(|entity| keep(entity.entity_bits));
            system.removed.retain(|&entity_bits| keep(entity_bits));
        }
        debug_assert!(encoded_delta_len(&delta).is_some_and(|len| len <= budget));

        Ok(Fitted {
            delta,
            shed,
            deferred_removals,
        })
    }
}

#[cfg(test)]
mod tests;
