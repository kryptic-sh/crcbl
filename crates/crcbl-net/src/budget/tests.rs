//! The budget's promises, each checked end to end where it can be: a server
//! half (diff, fit, retain) and a client half (decode, apply, ack) driven tick
//! by tick, so "the client holds X" is read off a real client baseline rather
//! than inferred from what the encoder chose.

use super::*;
use crate::delta::{BaselineStore, decode_delta, encode_delta, encode_entity_entry};
use crate::messages::SystemSnapshot;
use crate::reliable::MAX_UNRELIABLE_PAYLOAD;
use crate::transport::MAX_IN_MEMORY_MESSAGE_BYTES;
use crate::types::SectorId;

/// The system every fixture entity lives in.
const SYSTEM: u32 = 7;

/// Bytes of every fixture entity's component: the tick it was written at,
/// then padding.
const COMPONENT_BYTES: usize = 20;

/// What one fixture entry costs in an encoded delta.
const ENTRY_BYTES: usize = ENTITY_ENTRY_HEADER_BYTES + COMPONENT_BYTES;

/// Entries a [`budget_for`] budget holds beside a one-system delta's framing.
const ENTRIES_PER_SNAPSHOT: usize = 8;

/// Entities changing every tick in the rotation fixtures: several snapshots'
/// worth, so most of them are held back every tick.
const CHANGING: u64 = 50;

/// The budget of a one-system delta with room for `entries` fixture entries.
fn budget_for(entries: usize) -> usize {
    DELTA_HEADER_BYTES + SYSTEM_HEADER_BYTES + entries * ENTRY_BYTES
}

/// Ticks within which every one of `changing` entities ships when
/// [`ENTRIES_PER_SNAPSHOT`] fit a snapshot and all are equally relevant.
fn rotation_bound(changing: u64) -> u64 {
    changing.div_ceil(ENTRIES_PER_SNAPSHOT as u64)
}

/// A component recording the tick it was written at.
fn component(written: u64) -> Vec<u8> {
    let mut data = written.to_le_bytes().to_vec();
    data.resize(COMPONENT_BYTES, 0xC5);
    data
}

/// The tick a [`component`] was written at.
fn written_at(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[..8].try_into().expect("a fixture component"))
}

/// The server's state at `tick`: entities `1..=count` in [`SYSTEM`], each
/// holding the component `written(entity)` says.
fn state(
    tick: u64,
    entities: impl IntoIterator<Item = u64>,
    written: impl Fn(u64) -> u64,
) -> Baseline {
    let mut data = Vec::new();
    for entity in entities {
        encode_entity_entry(&mut data, entity, &component(written(entity)));
    }
    Baseline::from_snapshot(
        TickId::from_raw(tick),
        &[SystemSnapshot {
            system_id: SYSTEM,
            data,
        }],
        Trust::Authenticated,
    )
    .expect("a fixture state is valid")
}

/// Every entity in `baseline`'s fixture system, with the tick its component
/// was written at.
fn held(baseline: &Baseline) -> HashMap<EntityBits, u64> {
    baseline
        .iter_entities()
        .map(|(_, entity, data)| (entity, written_at(data)))
        .collect()
}

/// One client's session, both ends: the server's baselines, acks and
/// accumulator, and the client's reconstructed baseline.
struct Session {
    budget: usize,
    store: BaselineStore,
    accumulator: PriorityAccumulator,
    last_acked: Option<TickId>,
    client: Baseline,
}

impl Session {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            store: BaselineStore::new(64),
            accumulator: PriorityAccumulator::new(),
            last_acked: None,
            client: Baseline::from_snapshot(TickId::ZERO, &[], Trust::Authenticated)
                .expect("empty snapshot is valid"),
        }
    }

    /// One tick at [`DEFAULT_RELEVANCE`]: see [`Session::step_with`].
    fn step(&mut self, current: Baseline, delivered: bool) -> Fitted {
        self.step_with(current, delivered, |_, _| DEFAULT_RELEVANCE)
    }

    /// One tick: diff `current` against the acked baseline, fit it, encode it
    /// and retain what it leaves the client holding; if `delivered`, the
    /// client decodes and applies it and its ack lands before the next tick.
    fn step_with(
        &mut self,
        current: Baseline,
        delivered: bool,
        relevance: impl Fn(u32, EntityBits) -> NonZeroU32,
    ) -> Fitted {
        let previous = self.last_acked.and_then(|tick| self.store.get(tick));
        let delta = DeltaCodec::encode_from_baseline(SectorId::ZERO, &current, previous);
        let fitted = self
            .accumulator
            .fit(delta, self.budget, relevance)
            .expect("every fixture entry fits an empty snapshot");
        let bytes = encode_delta(&fitted.delta).expect("a fitted delta encodes");
        assert!(
            bytes.len() <= self.budget,
            "a {}-byte snapshot for a {}-byte budget",
            bytes.len(),
            self.budget
        );
        let retained = if fitted.is_whole() {
            current
        } else {
            fitted
                .baseline_after(previous)
                .expect("the fitted delta applies to its own baseline")
        };
        let tick = retained.tick;
        self.store.insert(retained);

        if delivered {
            let decoded =
                decode_delta(&bytes, Trust::Authenticated).expect("the client decodes it");
            DeltaCodec::apply(&decoded, &mut self.client, Trust::Authenticated)
                .expect("the client applies it");
            self.last_acked = Some(tick);
        }
        fitted
    }
}

/// Entities an update in `fitted` names, removals included.
fn shipped(fitted: &Fitted) -> Vec<EntityBits> {
    fitted
        .delta
        .systems
        .iter()
        .flat_map(|system| {
            system
                .added
                .iter()
                .chain(&system.modified)
                .map(|entity| entity.entity_bits)
                .chain(system.removed.iter().copied())
        })
        .collect()
}

fn removed(fitted: &Fitted) -> Vec<EntityBits> {
    fitted
        .delta
        .systems
        .iter()
        .flat_map(|system| system.removed.iter().copied())
        .collect()
}

#[test]
fn the_budget_is_the_transports_unreliable_limit_less_the_seal() {
    assert_eq!(
        snapshot_budget(MAX_UNRELIABLE_PAYLOAD),
        MAX_UNRELIABLE_PAYLOAD - AUTH_OVERHEAD
    );
    assert_eq!(
        snapshot_budget(MAX_IN_MEMORY_MESSAGE_BYTES),
        MAX_DELTA_BYTES
    );
    assert_eq!(
        snapshot_budget(usize::MAX),
        MAX_DELTA_BYTES,
        "the encoder's own cap bounds a transport that takes more"
    );
    assert_eq!(snapshot_budget(AUTH_OVERHEAD - 1), 0);
}

/// **A diff that fits is sent whole**, and leaves nothing accumulating.
#[test]
fn a_diff_that_fits_is_sent_whole() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    let fitted = session.step(state(1, 1..=4, |_| 1), true);
    assert!(fitted.is_whole(), "{fitted:?}");
    assert_eq!(held(&session.client).len(), 4);
    let fitted = session.step(state(2, 1..=4, |_| 2), true);
    assert!(fitted.is_whole());
    assert!((1..=4).all(|entity| session.accumulator.priority(SYSTEM, entity) == 0));
}

/// **Nothing held back is lost: every update reaches the client within the
/// rotation bound.** Fifty entities change every tick and eight fit a
/// snapshot, so each is held back most ticks — yet the accumulator ships
/// every one at least every `ceil(50 / 8)` ticks, and the client never holds
/// a value older than that. Staleness is what the accumulation measures;
/// without it the same eight would win every tick.
#[test]
fn every_update_reaches_the_client_within_the_rotation_bound() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    let bound = rotation_bound(CHANGING);
    let mut last_shipped: HashMap<EntityBits, u64> = HashMap::new();
    for tick in 1..=6 * bound {
        let fitted = session.step(state(tick, 1..=CHANGING, |_| tick), true);
        assert!(fitted.shed > 0, "the fixture must overflow the budget");
        for entity in shipped(&fitted) {
            last_shipped.insert(entity, tick);
        }
        if tick < bound {
            continue;
        }
        for entity in 1..=CHANGING {
            let shipped_at = last_shipped.get(&entity).copied().unwrap_or(0);
            assert!(
                tick - shipped_at < bound,
                "entity {entity} last shipped at tick {shipped_at}, {} ticks before \
                 tick {tick}; the bound is {bound}",
                tick - shipped_at
            );
            let written = held(&session.client)[&entity];
            assert!(
                tick - written < bound,
                "at tick {tick} the client holds entity {entity} as of tick {written}"
            );
        }
    }
}

/// **The client converges on the server once changes stop**, even with
/// snapshots lost along the way: what was held back, and what was sent and
/// lost, still differs from the acked baseline, so it ships.
#[test]
fn the_client_converges_once_changes_stop() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    let churn = 4 * rotation_bound(CHANGING);
    for tick in 1..=churn {
        session.step(state(tick, 1..=CHANGING, |_| tick), tick % 3 != 0);
    }
    let settled = state(churn, 1..=CHANGING, |_| churn);
    let mut converged_at = None;
    for tick in churn + 1..=churn + 4 * rotation_bound(CHANGING) {
        session.step(state(tick, 1..=CHANGING, |_| churn), true);
        if session.client.state_hash() == settled.state_hash() {
            converged_at = Some(tick);
            break;
        }
    }
    let converged_at = converged_at.expect("the client never converged on the server");
    assert!(
        converged_at - churn <= rotation_bound(CHANGING) + 1,
        "converged {} ticks after changes stopped",
        converged_at - churn
    );
}

/// **A held-back update is never sent as a removal or a revert.** Every
/// entity exists on the server throughout, so no delta may remove one, the
/// client never loses one it was given, and the value it holds only moves
/// forward in time. The budget leaves room over that no entry fits, where an
/// encoder could squeeze in a cheaper wrong answer.
#[test]
fn a_held_back_update_is_never_sent_as_a_removal_or_a_revert() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT) + ENTRY_BYTES / 2);
    let mut seen: HashMap<EntityBits, u64> = HashMap::new();
    for tick in 1..=4 * rotation_bound(CHANGING) {
        let fitted = session.step(state(tick, 1..=CHANGING, |_| tick), tick % 4 != 0);
        assert_eq!(removed(&fitted), [], "tick {tick} removed a live entity");
        let now = held(&session.client);
        for (entity, &before) in &seen {
            let after = now.get(entity).copied();
            assert!(
                after.is_some_and(|after| after >= before),
                "at tick {tick} entity {entity} went from tick {before} to {after:?}"
            );
        }
        seen = now;
    }
}

/// **Removals ship before any change.** The tick's changes alone overflow
/// the budget; five entities are also destroyed, and all five removals are in
/// that tick's snapshot.
#[test]
fn removals_ship_before_any_change() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    for tick in 1..=2 * rotation_bound(CHANGING) {
        session.step(state(tick, 1..=CHANGING, |_| tick), true);
    }
    let tick = 2 * rotation_bound(CHANGING) + 1;
    let survivors = 1..=CHANGING - 5;
    let fitted = session.step(state(tick, survivors, |_| tick), true);
    assert!(fitted.shed > 0, "the changes must overflow the budget");
    assert_eq!(fitted.deferred_removals, 0);
    assert_eq!(
        removed(&fitted),
        (CHANGING - 4..=CHANGING).collect::<Vec<_>>()
    );
    assert!((CHANGING - 4..=CHANGING).all(|entity| !held(&session.client).contains_key(&entity)));
}

/// **Removals past the budget are deferred, not lost.** Destroying far more
/// entities than one snapshot has room to name defers the overflow, and the
/// snapshots after remove the rest.
#[test]
fn removals_past_the_budget_are_deferred_not_lost() {
    const ENTITIES: u64 = 400;
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    // Nothing changes after tick 1, so the rotation delivers everything and
    // stops.
    let mut tick = 1;
    while held(&session.client).len() < ENTITIES as usize {
        session.step(state(tick, 1..=ENTITIES, |_| 1), true);
        tick += 1;
    }
    let fitted = session.step(state(tick, [], |_| 1), true);
    assert!(fitted.deferred_removals > 0, "{fitted:?}");
    assert_eq!(
        removed(&fitted).len() + fitted.deferred_removals,
        ENTITIES as usize
    );
    let per_snapshot = (ENTRIES_PER_SNAPSHOT * ENTRY_BYTES / REMOVED_ENTRY_BYTES) as u64;
    let mut snapshots = 1;
    while !held(&session.client).is_empty() {
        tick += 1;
        snapshots += 1;
        assert!(
            snapshots <= ENTITIES.div_ceil(per_snapshot),
            "the removals still had not all arrived after {snapshots} snapshots"
        );
        session.step(state(tick, [], |_| 1), true);
    }
}

/// **The encoded snapshot never exceeds the budget**, whatever the mix of
/// entry sizes: entries from empty to most of the budget, added, modified and
/// removed, every tick. `Session::step` asserts the length of every encoded
/// snapshot.
#[test]
fn the_encoded_snapshot_never_exceeds_the_budget() {
    // xorshift64 (Marsaglia), for arbitrary but reproducible sizes.
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let budget = budget_for(ENTRIES_PER_SNAPSHOT);
    let largest = budget - DELTA_HEADER_BYTES - SYSTEM_HEADER_BYTES - ENTITY_ENTRY_HEADER_BYTES;
    let mut session = Session::new(budget);
    for tick in 1..=200_u64 {
        let mut data = Vec::new();
        for entity in 1..=40_u64 {
            // Some entities come and go, so there are additions and removals.
            if next() % 5 == 0 {
                continue;
            }
            let len = (next() % (largest as u64 + 1)) as usize;
            encode_entity_entry(&mut data, entity, &vec![tick as u8; len]);
        }
        let current = Baseline::from_snapshot(
            TickId::from_raw(tick),
            &[SystemSnapshot {
                system_id: SYSTEM,
                data,
            }],
            Trust::Authenticated,
        )
        .expect("valid");
        session.step(current, next() % 4 != 0);
    }
}

/// **The same inputs give the same snapshot, byte for byte.** Two servers
/// build the same states in separately seeded hash maps; every snapshot they
/// fit is identical.
#[test]
fn the_same_inputs_give_byte_identical_snapshots() {
    let budget = budget_for(ENTRIES_PER_SNAPSHOT);
    let mut left = Session::new(budget);
    let mut right = Session::new(budget);
    for tick in 1..=3 * rotation_bound(CHANGING) {
        let a = left.step(state(tick, 1..=CHANGING, |_| tick), true);
        let b = right.step(state(tick, (1..=CHANGING).rev(), |_| tick), true);
        assert_eq!(
            encode_delta(&a.delta).expect("encodes"),
            encode_delta(&b.delta).expect("encodes"),
            "tick {tick}"
        );
    }
}

/// **Ties go to the lower entity bits**, so which of several equally urgent
/// updates ships is decided by the state and not by map order.
#[test]
fn ties_go_to_the_lower_entity_bits() {
    let mut session = Session::new(budget_for(2));
    let fitted = session.step(state(1, [9, 3, 5, 1], |_| 1), true);
    assert_eq!(shipped(&fitted), [1, 3]);
    assert_eq!(session.accumulator.priority(SYSTEM, 5), 1);
    assert_eq!(session.accumulator.priority(SYSTEM, 1), 0, "packed resets");
}

/// **Relevance decides who ships first.** One entity far more relevant than
/// the rest ships every tick while the others rotate behind it.
#[test]
fn a_more_relevant_update_ships_every_tick() {
    const URGENT: EntityBits = CHANGING;
    let relevance = |_: u32, entity: EntityBits| {
        if entity == URGENT {
            NonZeroU32::MAX
        } else {
            DEFAULT_RELEVANCE
        }
    };
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    for tick in 1..=4 * rotation_bound(CHANGING) {
        let fitted = session.step_with(state(tick, 1..=CHANGING, |_| tick), true, relevance);
        assert!(
            shipped(&fitted).contains(&URGENT),
            "tick {tick} held back the urgent entity"
        );
    }
}

/// **A keyframe is fitted too, and the deltas after it add the rest.** A
/// client joining a sector with more entities than a snapshot holds gets the
/// most urgent in its keyframe; nothing it holds is withdrawn, since it held
/// nothing, and it converges.
#[test]
fn a_joining_client_gets_a_fitted_keyframe_and_the_rest_after() {
    let mut session = Session::new(budget_for(ENTRIES_PER_SNAPSHOT));
    let fitted = session.step(state(1, 1..=CHANGING, |_| 1), true);
    assert!(fitted.delta.is_keyframe);
    assert_eq!(shipped(&fitted).len(), ENTRIES_PER_SNAPSHOT);
    assert_eq!(held(&session.client).len(), ENTRIES_PER_SNAPSHOT);

    let settled = state(1, 1..=CHANGING, |_| 1);
    for tick in 2..=rotation_bound(CHANGING) {
        let before = held(&session.client).len();
        let fitted = session.step(state(tick, 1..=CHANGING, |_| 1), true);
        assert!(!fitted.delta.is_keyframe);
        assert_eq!(removed(&fitted), []);
        assert!(held(&session.client).len() > before);
    }
    assert_eq!(session.client.state_hash(), settled.state_hash());
}

/// **What cannot be held back is refused with its size**: an entry too long
/// to fit beside the framing could never be sent, and the accumulator is left
/// as it was.
#[test]
fn an_update_that_could_never_fit_is_refused_with_its_size() {
    let budget = budget_for(2);
    let mut accumulator = PriorityAccumulator::new();
    let mut session = Session::new(budget);
    // Accumulate some priority first, to show a refusal does not touch it.
    session.step(state(1, 1..=4, |_| 1), true);
    accumulator.held.clone_from(&session.accumulator.held);
    assert!(!accumulator.held.is_empty());

    let mut data = Vec::new();
    let too_long =
        budget - DELTA_HEADER_BYTES - SYSTEM_HEADER_BYTES - ENTITY_ENTRY_HEADER_BYTES + 1;
    encode_entity_entry(&mut data, 1, &vec![0; too_long]);
    let current = Baseline::from_snapshot(
        TickId::from_raw(2),
        &[SystemSnapshot {
            system_id: SYSTEM,
            data,
        }],
        Trust::Authenticated,
    )
    .expect("valid");
    let delta = DeltaCodec::encode_from_baseline(SectorId::ZERO, &current, None);
    let refused = accumulator
        .fit(delta, budget, |_, _| DEFAULT_RELEVANCE)
        .expect_err("an entry past the budget cannot be fitted");
    assert_eq!(
        refused,
        BudgetTooSmall {
            size: budget + 1,
            budget
        }
    );
    assert_eq!(accumulator.held, session.accumulator.held);

    // The same entry one byte shorter fits.
    let mut data = Vec::new();
    encode_entity_entry(&mut data, 1, &vec![0; too_long - 1]);
    let current = Baseline::from_snapshot(
        TickId::from_raw(2),
        &[SystemSnapshot {
            system_id: SYSTEM,
            data,
        }],
        Trust::Authenticated,
    )
    .expect("valid");
    let delta = DeltaCodec::encode_from_baseline(SectorId::ZERO, &current, None);
    assert!(
        accumulator
            .fit(delta, budget, |_, _| DEFAULT_RELEVANCE)
            .is_ok()
    );
}
