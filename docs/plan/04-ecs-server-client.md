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
  permanent single-player path. Wasm constraint noted: the future network
  transport must have a WebSocket/WebTransport impl (stage 10), so the trait is
  message-oriented and async-agnostic — no UDP assumptions in the interface.

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
- Interest management (per-client visibility) — post-MVP; the snapshot writer
  API takes a client id now so it can be added without resurfacing every system.
- Transport trait: reliable-ordered channel + unreliable-sequenced channel
  semantics in the interface (in-memory impl trivially provides both; a future
  UDP/QUIC/WebTransport impl maps them properly).

## Tasks

1. `crcbl-ecs`: entity pool, system trait + registration, schedule, removal
   sweep, inspector registry. Unit-tested without any renderer.
2. `crcbl-net`: message types, transport trait, `InMemoryTransport`, snapshot
   writer/reader.
3. `crcbl-server`: tick loop on the fixed-timestep accumulator (stage 1), input
   queue, snapshot emission. Headless `server` binary target proving no render
   deps.
4. `crcbl-client`: connection state machine, input send, snapshot apply,
   interpolation buffer, feed to render instance array.
5. Sandbox becomes client+server: moving entities simulated on the server,
   rendered via interpolation. Kill-switch flag to run the server at 5 Hz to
   _see_ interpolation working (debug tools principle).

## Exit criteria

- Sandbox: N moving entities server-simulated, client-rendered, smooth at
  mismatched tick/render rates.
- `crcbl-server` compiles with no dependency on `crcbl-render`/`crcbl-vk`
  (enforced in CI via `cargo tree` check).
- Tick determinism smoke test: same input script → same state hash over 1000
  ticks (foundation for future rollback/replay debugging).
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
