# Audit — P2a / P2b (`crcbl-ecs`, `-net`, `-input`, `-server`, `-client`, `apps/sim`)

**Last updated:** 2026-07-28, after the fix round in `4e55d49`. **Method:** the
eight gates and the harnesses from the ROADMAP's "How work proceeds" section,
plus throwaway probe tests and Miri. Every finding was produced by running
something, not by reading.

**Verdict:** **no blocking defects.** All eight gates pass, 826 tests. All four
findings from the previous round were addressed, and the two that leave residue
share one root cause: **the crates do not expose enough state for a test to
assert what its own header claims.** That is a design gap, not carelessness —
and it is the last thing standing between these tests and the properties they
describe.

**Resolved in `4e55d49`** (detail pruned): the replication test's `> 0` check
became an entity-count equality and gained a loss+reorder+duplication case; the
integration test now drives the real `crcbl_server::Server` and
`crcbl_client::Client` instead of local mocks; `apps/sim` gained a
`CounterSystem` with real per-tick behaviour that feeds the hash; and the NaN
comment was corrected to say `to_bits()` preserves the payload rather than
canonicalising it. Earlier rounds' UB, gate failures and vacuous hash remain
fixed.

---

## Open findings

### 1. The server↔client integration test asserts liveness, not data flow

`crates/crcbl-server/tests/integration.rs` now uses the real crates, which was
the point. But after a 20-tick exchange it asserts only:

```rust
assert!(server.is_connected());
assert!(client.is_connected());
assert!(server.tick_id().get() > 0, "server tick id must advance beyond zero");
```

A server that emitted nothing, or a client that discarded every packet, passes
both — the transport stays connected and the clock still advances. The file's
header claims more: _"the server emits delta-encoded snapshots, the client
applies them, acks, and the server advances its baseline accordingly."_ None of
those three is checked, and the test's own comment concedes it verifies "the
plumbing runs without panic".

**The wiring does work.** Holding one end of the transport myself, the server
emitted 5 non-empty payloads over 5 ticks. So this is again a missing assertion
rather than a bug.

**Root cause, and the actual fix:** `Client::baseline` is private and the client
exposes no accessor for received state — `interpolate()` is still a stub, and
`world()` is not where snapshots land. A test therefore _cannot_ observe that
anything arrived. Add one observable (`last_applied_tick()`, or a baseline
entity count), then assert it. Without that, this test cannot be strengthened at
all.

### 2. "State equality" is asserted at entity-count level only

The replication tests now check `entity_count_for(1) == n_entities` under loss,
and under loss+reorder+duplication. That is a real improvement over `> 0` and it
covers the case the header promised.

It still compares **counts, not values**: a client that reconstructed four
entities with wrong component bytes passes. The header says "the client's
reconstructed state matches the server's".

Same root cause as finding 1: `Baseline` stores
`HashMap<u32, HashMap<u64, Vec<u8>>>` privately and exposes only
`entity_count`/`entity_count_for`/`system_count`. There is no way to compare
per-entity bytes from a test.

**Fix:** expose the per-entity data (or a hash of it) and assert value equality.
A `Baseline::state_hash()` would be enough, would reuse the determinism work
already done, and would make "state equality" mean what it says.

---

## What this round got right

- **It used the real crates, as asked.** Finding 2 of the last round said the
  integration test validated a hand-rolled driver; it now constructs
  `Server::new(world, transport, 60)` and `Client::new(...)` and runs them
  against each other. The structural criticism was taken, not worked around.
- **The reorder case was added rather than the claim deleted.** A new
  `replication_with_loss_reorder_and_duplication` exercises `reorder_window: 4`
  and `duplicate_rate: 0.10` — the harder of the two options, and the one that
  turns an unverified header into a tested property.
- **`apps/sim` is no longer inert.** `CounterSystem` implements `SystemTrait`
  with a real `tick()` that mutates its component data, overrides `hash_state`,
  and returns `true` from `contributes_to_hash`. Two runs at
  `--ticks 10 --seed 1` produce `6c6e1fd85865ba46` both times; 20 ticks produce
  a different hash. The determinism harness is now pointed at something that
  actually moves.
- **The NaN comment was corrected precisely**, including the reasoning for why
  payload preservation is sufficient for same-binary determinism, rather than
  simply deleting the claim.

---

## The pattern worth carrying forward

Three rounds in, every correctness problem has been fixed and stayed fixed. What
remains is narrower and more interesting: **tests are written to the limit of
what the API lets them observe, and the docs are written to what the author
knows.** The gap between them is where these findings live.

The fix is not more discipline in the tests — both remaining findings are
currently _unwriteable_ as stronger assertions. It is to expose one honest
observable per subsystem: a `last_applied_tick()` on the client, a
`state_hash()` on `Baseline`. Each is a few lines, and each converts a paragraph
of prose into a property that CI enforces.

Trajectory across rounds: blocking CI failures → soundness/UB → assertion
strength → observability. Each round the findings get less severe and more
structural, which is the right direction.
