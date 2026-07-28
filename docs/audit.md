# Audit — P2a / P2b (`crcbl-ecs`, `-net`, `-input`, `-server`, `-client`, `apps/sim`)

**Last updated:** 2026-07-28, after P2b (`1ab4669`..`fb7e7bf`). **Method:** the
eight gates and the harnesses from the ROADMAP's "How work proceeds" section,
plus throwaway probe tests and Miri. Every finding was produced by running
something, not by reading.

**Verdict:** **no blocking defects.** All eight gates pass, 823 tests. The
previous round's soundness defect is properly fixed, and P2b's replication is
_stronger than its own tests demonstrate_ — I verified by probe that it survives
loss, reorder and duplication with full state equality, none of which the
shipped tests actually assert. The open items are coverage and claim accuracy,
not correctness.

**Resolved since the last round** (detail pruned): the raw-byte `hash_state`
that read uninitialized padding (Miri-confirmed UB) and hashed pointers for heap
types; the silent abstention of custom `SystemTrait` impls; and, before that,
the `Cargo.lock`/rustdoc gate failures, the misleading `interpolate()` docs, and
the inflated test count.

---

## Open findings

### 1. The replication test asserts far less than its header claims

`crates/crcbl-net/tests/replication.rs` opens with _"replication survives
scripted loss/reorder with state equality … after N ticks the client's
reconstructed state matches the server's."_ Of those three claims:

| Claim          | Status                                                 |
| -------------- | ------------------------------------------------------ |
| loss           | exercised (`loss_rate: 0.15`)                          |
| reorder        | **never exercised** — `reorder_window` is not set once |
| state equality | **not asserted** — the only check is `> 0`             |

```rust
assert!(client.baseline.entity_count() > 0, "…");
```

That passes with one entity out of four, or with every value wrong.

**Both missing properties do in fact hold.** Probes I added and ran:
`entity_count_for(1) == 4` after 80 lossy ticks, and again after 120 ticks with
`loss_rate: 0.10, reorder_window: 4, duplicate_rate: 0.10`. So this is a
test-strength gap rather than a bug — the code earns a stronger assertion than
it is being given.

**Fix:** replace the `> 0` check with the equality the header promises, and add
the reorder/duplication case. Both are ~10 lines against the existing harness.

### 2. The wired server and client have no integration coverage

`ec0597e` wires `crcbl-server` and `crcbl-client` to the P2b codec, but nothing
drives them together. The "integration" test defines its own `Server`/`Client`
structs locally and tests the codec against those; outside their own crates, the
only reference to either crate anywhere is `apps/sim` importing `sim_hash`.

So the shipped wiring — the thing a game actually links — is exercised by unit
tests only, while the integration test validates a parallel hand-rolled driver.
If the two drift, nothing notices.

**Fix:** point the integration test at the real crates, or add one that does.

### 3. The determinism harness now works, but is aimed at an inert world

The hash is capable after the `ComponentHash` fix. What it hashes has not caught
up: `System<T>::tick()` is still a documented no-op, and `apps/sim::build_world`
registers only `System<f32>` storage systems, so `crcbl-sim --ticks 1000` runs a
thousand ticks in which nothing mutates. The output remains a pure function of
`(seed, ticks)`.

The harness can now _detect_ divergence; it is not yet _exposed_ to any. A
nondeterminism source — map iteration order, float accumulation order, thread
interleaving — has nothing to perturb.

**Fix:** give the sim at least one system with real per-tick behaviour over its
component data. Until then, treat a green `crcbl-sim` as "the plumbing works",
not "the simulation is deterministic".

### 4. Minor: the NaN comment overstates `to_bits()`

`component_hash.rs` says `to_bits()` "handles NaN deterministically (canonical
payload)". It does not canonicalise:

```
NAN=0x7fc00000  -NAN=0xffc00000  0.0/0.0=0x7fc00000
```

Sign-differing NaNs hash differently. Harmless for same-binary determinism (the
same computation yields the same bits), but two equivalent code paths producing
differently-signed NaNs would read as divergence. Reword, or canonicalise
explicitly if NaN components are ever expected.

---

## What the last two rounds got right

- **The `ComponentHash` fix is the right shape.** No `unsafe` anywhere in
  `crcbl-ecs`; `f32`/`f64` go through `to_bits()`; and the bound sits where it
  bites — `System::<Padded>` and `System::<String>` are rejected at
  `register_system` with
  `the trait bound 'Padded: ComponentHash' is not satisfied`. Unsound component
  types are now a compile error rather than a silent wrong answer.
- **Abstention was made visible, as recommended.**
  `SystemTrait::contributes_to_hash` plus `Schedule::non_contributing_systems`,
  surfaced as a `sim: WARNING — N system(s) do not contribute…` line. A system
  that hashes nothing now announces itself.
- **Decode hardening is genuine.** A hand-built hostile corpus (empty, unknown
  tags, truncated bodies, `0xFFFFFFFF` declared lengths, all-`0xFF` blobs) plus
  a random fuzz test, run through every decoder, asserting no panic. This is
  what `plan/23-netcode.md` asks for and it was not skimped.
- **The replication implementation is solid**, per finding 1 — it survives
  conditions its tests never subject it to.

---

## The pattern worth carrying forward

The correctness problems are gone; what persists is **documentation and tests
claiming more than they establish**. A test file header promising
loss/reorder/equality while asserting `> 0`; a comment promising canonical NaN;
an "integration" test integrating mocks. Earlier rounds had the same shape at
higher severity — a hash that hashed nothing, an interpolator whose docs claimed
lerping.

The repo's rule still applies, one level up from correctness: **a claim that no
test establishes is a claim to delete or a test to write.** Where the code is
genuinely good — as it is here — the cheaper of the two is usually the test, and
it converts an unverified assertion into a guarantee.

Worth noting the trajectory: round 1 was blocking CI failures, round 2 was
soundness, round 3 is assertion strength. Each round the findings get less
severe, which is the direction it should go.
