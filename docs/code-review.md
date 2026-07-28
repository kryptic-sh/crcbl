# Code Review

Scope: local commits `9380b4f..1fdfde8`, with emphasis on P2b protocol hardening
and the roadmap status update.

## Findings

### High — lost ACK permanently wedges snapshot replication

`crates/crcbl-client/src/lib.rs:168-185` accepts only a non-keyframe whose
`baseline_tick` equals the client's current baseline and emits an ACK only when
applying a snapshot. `crates/crcbl-server/src/lib.rs:121-129` keeps encoding
every later snapshot against `SessionManager::last_acked_tick()`.

Concrete failure: the client applies keyframe tick 1, but its unreliable ACK is
lost. The server retains tick 1 but has no acknowledged baseline, so tick 2 and
every later tick are sent as keyframes. This case recovers. The permanent wedge
occurs after an ACK has once succeeded: client applies tick 2 and ACKs it;
server receives that ACK and starts sending deltas based on tick 2. If the
tick-3 ACK is lost, the client advances to tick 3, while the server continues
sending ticks 4+ against tick 2. The client rejects all of them because its
baseline is tick 3, and rejected packets produce no ACK. The server therefore
never advances its baseline and the session cannot recover without an external
keyframe/reset.

The P2b exit criterion says replication survives scripted loss/reorder with
state equality. Recovery needs either cumulative acknowledgement semantics that
let a later accepted snapshot advance the server safely, client
retention/application of the referenced baseline, or a server keyframe fallback
when ACK progress stalls.

### High — roadmap marks P2b complete while required P2b gates remain absent

`docs/plan/ROADMAP.md:23-25` and `docs/plan/ROADMAP.md:36` declare P2b complete.
The same roadmap defines P2b as including per-client message/byte-rate limits
and a fuzz corpus in CI, but no rate limiter exists under `crates/crcbl-net`,
and `.github/workflows/cron.yml:22` explicitly defers `cargo-fuzz` net decode
hardening to P13. The delta transport also has no sector identifier: `Delta` in
`crates/crcbl-net/src/delta.rs:283-287` contains only tick/baseline/systems,
while `crcbl-server` sends that raw delta and `crcbl-client` reconstructs every
accepted snapshot as `SectorId::ZERO` at
`crates/crcbl-client/src/lib.rs:187-193`. That does not implement the roadmap's
per-`(client, sector)` baseline streams.

Concrete failure: a nonzero-sector snapshot loses its sector identity during
delta encoding and is reconstructed as sector zero. Separately, a client can
submit unlimited individually valid packets because only packet size is bounded.
These are explicitly listed P2b requirements, so advancing the canonical status
to P2c is inaccurate.

### Medium — public delta application permits impossible entity lifecycle operations

`crates/crcbl-net/src/delta.rs:494-517` rejects duplicate IDs within a delta but
does not validate an operation against the current baseline. `DeltaCodec::apply`
at `crates/crcbl-net/src/delta.rs:443-453` therefore allows `added` to overwrite
an entity that already exists, `modified` to create an absent entity, and
`removed` to silently ignore an absent entity.

Concrete failure: a hostile non-keyframe delta with a matching baseline tick
places an existing entity in `added`; application succeeds and replaces its
component bytes. Likewise, placing a never-seen entity in `modified` creates it.
Both malformed lifecycle encodings become accepted state transitions instead of
decode/application errors, weakening schema validation and making protocol
corruption indistinguishable from valid replication.

### Medium — component oversize reports the wrong error and hides validation intent

`crates/crcbl-net/src/delta.rs:500-514` combines component-size validation and
duplicate-ID validation in one condition, returning
`BaselineDecodeError::DuplicateEntity` for an oversized unique component.

Concrete failure: call `encode_delta` or `DeltaCodec::apply` with one unique
entity whose component is `MAX_COMPONENT_BYTES + 1`. The result is
`DuplicateEntity(entity_bits)`, despite no duplicate. Callers cannot distinguish
malformed size from entity conflict, metrics classify the packet incorrectly,
and tests matching the intended `ComponentTooLarge` variant would fail.
