# Stage 4 — ECS + Server→Client Core

The simulation half of the engine: system-owned-array ECS, fixed-tick
authoritative server, transport seam with the in-memory implementation, and
replication to a rendering client. After this stage the sandbox is a real
(single-player) client connected to a real server in the same process.

## Goals

- ECS in the chosen shape: systems own arrays of the objects registered to them
  — not archetype soup, not objects with component bags.
- Server owns all gameplay state; client owns presentation state. Enforced by
  crate boundaries, not discipline.
- Transport is a trait from day one; in-memory channel is the first impl and the
  permanent single-player path. The trait is message-oriented and async-agnostic
  — no UDP assumptions in the interface. (This was originally motivated by a
  browser transport; browsers have no network transport at all now — see topic
  23's LAN correction — but the shape is right regardless and is what lets
  in-memory and UDP share one interface.)

## ECS model (`crcbl-ecs`)

- `Entity` = generational id (from `crcbl-core::Pool`). An entity is _only_ an
  id — no storage of its own.
- A **System** owns `Vec`-backed dense arrays (SoA) of the data for entities
  registered with it, plus a sparse entity→index map. Registration =
  `system.attach(entity, data)`; systems iterate their own arrays linearly —
  cache-friendly by construction, mirrors the GPU-side SoA layout from stage 3
  (render instance array is literally the render system's array).
- Cross-system access: systems expose typed queries by entity id (sparse map
  lookup) — used for cold paths; hot paths keep data in their own arrays.
- Ordering: explicit system schedule (a `Vec` of stages run in order). No
  automatic dependency inference in MVP — declared order, asserted conflicts in
  debug.
- Entity destruction: deferred to end-of-tick; systems get a removal sweep.
  Generational ids make stale references safe.
- Debug hook (principle 4): every system reports
  `(name, entity_count, tick_time)` to the inspector registry; per-system
  debug-draw callback slot.

## Server→client architecture

```
crates/crcbl-server   — simulation: ECS schedule, fixed tick, authoritative state
crates/crcbl-client   — presentation: interpolation, prediction hooks, render-system feed
crates/crcbl-net      — transport trait, in-memory impl, replication protocol
```

- **Fixed-tick server** (e.g. 30/60 Hz, configurable): consumes input messages,
  runs the ECS schedule, emits snapshots. Never blocks on the client;
  headless-runnable (dedicated server binary is free).
- **Client**: sends inputs, receives snapshots, interpolates between the last
  two for rendering (render clock trails server clock by one tick). Prediction
  is a hook, not an MVP feature — the interpolation buffer is designed so
  client-side prediction can slot in post-MVP.
- **Single player** = both in one process, `InMemoryTransport` (SPSC message
  queues). Same codepath as multiplayer; only the transport differs.

## Replication (`crcbl-net`)

- Message model: `ClientToServer::{Input, Command}` /
  `ServerToClient::{Snapshot, Event}`; all POD-serializable (`bincode`-style,
  zero-alloc hot path).
- Snapshots: per-system replication — each server system with replicated state
  provides `replicate(&self, out: &mut SnapshotWriter)`; the client-side twin
  consumes it. Wire model = **ack-baseline deltas** (topic 23): each client's
  snapshot is a delta vs their last-acked snapshot — only-on-change and
  loss-safe by construction, no game-code sync logic ever (game code writes
  values; declaring the schema is its entire netcode surface). Full state on
  join. Dirty flags are a server-side encoding accelerator only.
- Interest management is **per sector, and per sector only**. `SectorId`
  envelopes scope every snapshot; the server keeps a baseline store and an ack
  cursor per sector, and the client holds the set of sectors it is subscribed
  to. Finer per-client visibility was supposed to cost nothing later because
  "the snapshot writer API takes a client id" — it does not:
  `crcbl_ecs::SystemTrait::replicate` takes a byte sink and nothing else. That
  reservation was never made, so narrowing below the sector still means
  resurfacing every system that replicates.
- Transport trait: reliable-ordered channel + unreliable-sequenced channel
  semantics in the interface (in-memory impl trivially provides both; the UDP
  impl at P13 maps them properly).

## Tasks

1. **The headless `server` binary target was never built**: `crcbl-server` is a
   library with no `[[bin]]`, so the "proves no render deps" half of that task
   is carried by the manifest alone — see the exit criterion below.
2. **The sandbox never became client+server, and does not need to.** Sample rule
   2 (`docs/plan/sample/00-samples-overview.md`) made the split non-optional for
   every sample instead, so breakout onward each run a `GameModule` on a
   `Server` against an `InMemoryTransport` — a stronger proof than one app would
   have been, since it is now the shape a sample cannot opt out of. The 5 Hz
   kill switch generalised with it: `--tick-hz` is a flag on the shared
   `crcbl::args::Common`, not a sandbox debug toggle.

## Exit criteria

- N moving entities server-simulated, client-rendered, smooth at mismatched
  tick/render rates — met by every sample from breakout on, at any `--tick-hz`.
- `crcbl-server` compiles with no dependency on `crcbl-render`/`crcbl-vk`. **The
  property holds and nothing enforces it**: its `Cargo.toml` names `crcbl-core`,
  `crcbl-ecs`, `crcbl-net` and `crcbl-rand` and no renderer, but there is no
  `cargo tree` check in `ci.yml` — the only `cargo tree` in the workflows is a
  comment in `cron.yml` about something else. So the guarantee is the
  manifest's, which a future edit can break silently. Adding the check is what
  would make this an exit criterion rather than an observation, and it has to be
  shown to fail before it is trusted.
- Tick determinism smoke test: same input → same state hash over 1000 ticks
  (foundation for future rollback/replay debugging). Half met.
  `crcbl_server::sim_hash::hash_world` is the hash and `crcbl sim` is the
  harness that runs it, defaulting to 1000 ticks — but its world comes from
  `--seed` and there is **no input script**, because no scene format and no RON
  reader exist to carry one. So the loop is shown deterministic; the same loop
  _driven by recorded input_ is not.
- Inspector registry lists all systems with live counts (consumed by stage 7
  UI).

## Risks

- **ECS shape fights replication.** Mitigation: per-system replicate() keeps
  both aligned — the system that owns the array owns its wire format.
- **Prediction scope creep.** Interpolation-only for MVP. Hooks, not
  implementations.
- **Determinism drift (floats).** Only chase determinism to the level the smoke
  test needs (same binary, same machine). Cross-platform determinism is
  explicitly out of scope.

## Correction (design review, 2026-07-27)

**Interpolation buffer depth**: "render clock trails server clock by one tick"
is too shallow — one tick (16.7 ms) survives zero jitter and no dropped
snapshots. The design is the industry norm (Valve's Source networking documents
it): **~100 ms / two snapshot intervals plus a jitter margin**, with the buffer
**jitter-adaptive** (grows under measured jitter, shrinks when calm). 26 already
assumes this number; this doc is the P2 implementation reference and now carries
it.
