# Topic 21 — Job System + Threading Model (`crcbl-jobs`)

Performance-first threading: **every subsystem that can run independently gets
its own thread and never waits on a slower one; the main thread's only job is
converging published states into frame dispatch.** Two-level model: long-lived
pipeline threads for independent subsystems, plus a work-stealing job pool for
data-parallel bursts inside them.

## Design philosophy

1. **Isolation is already built.** The system-owned-array ECS means each system
   writes only its own SoA arrays — data races are designed out at the
   architecture level, not locked out at runtime. The job system exploits what
   the ECS shape guarantees.
2. **Never block on slower producers.** Cross-thread communication is
   **latest-wins mailboxes** (triple-buffered SPSC): a producer publishes
   complete states at its own cadence; consumers grab the newest complete one
   and go. A slow system publishes less often — it never stalls anyone. (The
   audio thread already works exactly this way; this generalizes it.)
3. **The main thread converges, nothing else.** It pumps the shell, snapshots
   the latest published states (interpolated sim, UI draw list, VFX spawn
   params, debug draw), feeds the render graph, submits. Target KPI: main-thread
   wait time ≈ 0 — it's a _collector_, and the profiler treats any wait on it as
   a regression.
4. **Determinism survives parallelism.** The server tick must produce the same
   hash single-threaded and multi-threaded — parallel-for uses fixed chunking
   and fixed-order reductions; scheduling order is never observable in results.
   This is CI-enforced (the killer test below).

## Thread topology

```
[OS main]  shell pump (OS requires it: AppKit/Win32 message pump, canvas)
    │           └─ raw events → input ring; resize/surface state
[input]    action-resolution thread (topic 19): consumes raw ring continuously,
    │      maintains held state + accumulates edges; **stacked tick states**
    │      published for pollers (see below)
[converge] frame assembly + render graph submit (may == OS main; target: wait≈0)
[sim]      server fixed-tick loop: ECS schedule (parallel via pool), physics
    │           └─ publishes: snapshots (the stage-4 replication buffer IS the mailbox)
[audio]    DSP callback thread (exists — topic 13; SPSC command ring)
[net]      transport IO (socket read/write, WebTransport later)
[io × N]   asset loading/decode workers (AssetSource is async already)
[pool × M] work-stealing job workers, M = cores − pinned threads
```

- The **server/client split is already a pipeline**: sim thread ticks at 60 Hz
  publishing snapshots; the converge thread renders at any rate, interpolating
  the last two — the existing interpolation buffer _is_ the latest-wins mailbox
  between them. Single-player just means both live in one process; this topic
  makes the threading explicit, not new.
- Cadences are independent by design: sim 60 Hz, render uncapped/vsync, audio 48
  kHz blocks, IO as-fast-as-disk, net as-fast-as-wire. **The graphics tick is
  fully independent of the physics tick** — render never waits on sim, never
  runs sim work, and a heavy physics tick manifests as increased interpolation
  staleness (visible in the profiler), never as a render hitch.
- Dedicated server build = sim + net + io threads only; no converge/render.
- On Linux the input thread can own the Wayland/X11 socket poll directly (fds
  are thread-safe to read); on Windows/macOS/web the OS pump stays on main and
  forwards through the raw ring — same downstream shape either way.

### Input: its own thread, stacked states, poll-only consumers

The input thread runs the topic 19 action layer continuously (timestamps
preserved, patterns evaluated between ticks — a 3 ms tap is never lost to a 16
ms tick):

- **Stacked tick state**: edges (pressed/released this window) accumulate into
  the _pending_ buffer; at each sim tick boundary the sim thread swaps it and
  reads a complete, immutable `InputTickState` (held set + ordered edge list +
  resolved action values). Accumulate-then-swap — never latest-wins, because
  edges must not be droppable.
- **Consumers poll, never subscribe**: sim systems read the tick's state;
  UI/converge read a continuously updated _live_ view (sub-tick hover and drag
  responsiveness); replays inject `InputTickState`s directly. No callbacks, no
  event handlers in game code.
- The stack also retains the last N tick states (ring) — prediction rollback
  (arena, post-MVP) replays exact inputs per tick from here for free.

### Tick sync: client and server run the same clock

Server and client physics/sim ticks are **the same fixed Hz and the same tick
numbering** — synchronized, not merely similar:

- The server owns tick N and the tick rate; every snapshot and event carries its
  tick id (already in the stage 4 protocol).
- The client estimates server time (EWMA over snapshot arrival + RTT/2 from the
  transport's ping) and runs its local tick counter at `server_tick + lead`,
  where lead ≈ ½RTT + jitter margin — just enough that a client input for tick N
  _arrives before_ the server simulates N.
- Drift correction is **rate-based, never stepping**: the client micro-scales
  its tick clock (±0.5%) to converge on the target offset — no teleporting
  simulation time, no dropped or doubled ticks under normal jitter. In-memory
  transport degenerates to lead = 0, same code.
- The server keeps a per-client input jitter buffer keyed by target tick; late
  inputs get the stage-4 policy (apply-next MVP; prediction era makes this the
  rollback trigger).
- This alignment is the **precondition for client prediction** (arena):
  predicted tick N compares against authoritative tick N — same number, same dt,
  same input. Landing it here, before prediction exists, is deliberate:
  interpolation-only MVP works without it, but retrofitting tick alignment under
  a shipped protocol is misery.
- Graphics remains outside the sync entirely: it interpolates between whatever
  synced ticks exist, at any frame rate.

## The two levels

### Pipeline threads (subsystem-level)

Long-lived, named, pinned-count. Communication rules — the only three allowed
primitives, all lock-free in steady state:

| Primitive             | Use                                                | Semantics                                                         |
| --------------------- | -------------------------------------------------- | ----------------------------------------------------------------- |
| **Mailbox (SPSC ×3)** | states: snapshots, UI draw lists, pose palettes    | latest-wins, never blocks                                         |
| **Ring (SPSC)**       | streams: input events, audio commands, net packets | bounded, overflow = counted + policy (drop-oldest or grow-in-dev) |
| **Job handle**        | one-shot results: asset decode, bake, screenshot   | poll or continuation, no join-on-main                             |

No shared mutable state across pipeline threads. No mutexes in the frame path (a
mutex in a hot path is a review-rejectable smell; init/teardown may lock
freely).

### Job pool (data-parallel bursts)

Work-stealing deque pool (own implementation — it's ~500 lines, a classic, and a
learning goal fits the project charter). Used _inside_ a stage:

- `par_for(range, chunk, f)` over SoA arrays — physics integration, broadphase
  refit, anim pose evaluation (one job per character), VFX emitter accumulation,
  scene chunk parsing.
- `scope(|s| …)` fork-join for divide-and-conquer (BVH build).
- **Deterministic mode** (sim-side default): fixed chunk boundaries, results
  written to pre-assigned slots, tree-ordered reductions — worker count and
  steal order cannot affect output. Render/client-side jobs may use the relaxed
  mode (order-free, faster).
- ECS parallel schedule: systems declare access at registration (own arrays =
  write; cross-system queries = read, already explicit in the ECS design). The
  scheduler derives the conflict DAG **at startup, not per tick** and runs
  non-conflicting systems concurrently on the pool; declared order is preserved
  only where a dependency exists. Debug builds assert undeclared access (the P2
  "asserted conflicts" hook grows teeth here).

## Degradation (wasm + low-core, mandated by stage 10)

The whole model collapses cleanly: pipeline stages become sequential calls in
the `tick(dt)` driver, mailboxes become plain double-buffers, `par_for` runs
inline. **Same code, zero cfg in systems** — parallelism is a property of the
runner, not the logic. wasm-threads (SharedArrayBuffer) later turns the pool
back on in browsers that allow it; nothing above the runner changes. Low-core
devices: pool shrinks, pipeline threads can share cores — correctness identical
by the determinism rule.

## Memory

- Per-thread bump allocators (stage 1 frame allocator, per-stage instance, reset
  at each stage's own cadence boundary).
- Mailbox states are owned buffers swapped by pointer — publish = pointer swap +
  release fence, never a copy of the payload.
- Job payloads allocate from the spawning stage's bump arena; pool workers never
  allocate (violations assert in debug).

## Debug + profiling (topic 7)

- **Thread timeline** in the profiler HUD: spans per thread, job pool lanes,
  mailbox publish/consume markers — the "who waited on whom" view.
- Convergence report: per-frame main-thread wait breakdown (target ≈ 0;
  regressions visible as a red bar, not a vibe).
- Ring overflow counters + mailbox staleness (consumer using an old state
  because producer is behind) surfaced in the inspector.
- `crcbl sim --threads N` — run headless sim at any worker count.

## Testing (topic 12)

- **The killer test**: determinism harness runs the same input script at
  `--threads 1`, `2`, `N` → identical state hash per tick. Runs in CI on every
  push; any parallel change that breaks it is caught immediately.
- Mailbox/ring property tests (publish/consume interleavings, wraparound,
  overflow policies) + loom-style exhaustive tests for the primitives only
  (they're small; the primitives are the only unsafe concurrency surface).
- TSAN job in scheduled CI.
- Perf regression: horde benchmark records per-thread utilization; a change that
  grows main-thread wait fails the recorded budget.

## Delivery

| Slice                                                                                                                                                  | Phase                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| **Seams reserved**: ECS access declarations, mailbox between sim/client (exists as interpolation buffer), audio ring (exists), `tick(dt)` runner shape | P2 (design constraint, near-zero code)                                  |
| **Tick-id protocol + client tick alignment** (lead + rate-based drift correction) — cheap now, misery to retrofit                                      | P2 (with the replication protocol)                                      |
| Input thread + stacked `InputTickState` (accumulate-then-swap, last-N ring)                                                                            | P2 (with the action layer; single-thread runner inlines it on wasm)     |
| `crcbl-jobs`: pool, `par_for` (both modes), mailbox/ring primitives + property tests                                                                   | P8 (horde is the forcing function — 10k bodies + 10k instances want it) |
| ECS parallel schedule (startup DAG, debug access asserts)                                                                                              | P8                                                                      |
| Pipeline-thread formalization (named threads, timeline profiler view)                                                                                  | P8                                                                      |
| Physics/anim/VFX `par_for` adoption                                                                                                                    | P8 → wave 1 as each system scales                                       |
| wasm-threads pool re-enable (SharedArrayBuffer)                                                                                                        | post-MVP                                                                |

The P2 line matters most: it costs almost nothing and prevents the classic
retrofit disaster. P8 is where threads actually switch on, with horde's numbers
as before/after proof.

## Risks

- **Determinism vs parallelism is the whole game.** Deterministic mode is the
  sim default and CI-enforced from the first parallel commit; relaxed mode is
  opt-in and client-only. Any "just use atomics" shortcut in sim systems fails
  the killer test by construction.
- **Over-threading small workloads**: `par_for` has a serial cutoff (chunking
  heuristic + profiler evidence); a 200-entity system runs inline.
- **Own work-stealing pool bugs**: primitives kept tiny, loom-tested, TSAN'd;
  the pool is the only novel concurrency code — everything else is SPSC or
  immutable publishing.
- **Main-thread creep** (systems sneaking work into converge): the wait≈0 KPI +
  timeline view make creep visible; review rule — converge assembles, never
  computes.

## Correction (design review, 2026-07-27)

**"Rate-based, never stepping" cannot converge after a real discontinuity.**
±0.5% slew at 60 Hz corrects ~0.3 ticks/second; a 500 ms offset (route change,
wifi→wired, resume from suspend) would take ~100 s to converge while every input
misses its target tick. Every deployed clock-sync design (NTP being canonical)
is hybrid. Corrected policy: **slew below a threshold (~50 ms), step above it**,
with a defined sim-side policy for the stepped interval (drop or fast-forward
the skipped ticks, logged and surfaced in the netgraph). Absolutism replaced by
a documented threshold.
