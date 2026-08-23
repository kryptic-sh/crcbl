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
- A headless sim runnable at any worker count. **There is no `crcbl sim`
  subcommand** — `crcbl-cli` does not parse the word, and the determinism
  harness is `apps/sim`, whose flags are `--ticks`, `--tick-rate` and `--seed`.
  A worker count is what it would gain to make the killer test below runnable;
  nothing exposes one yet.

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

| Slice                                                                                                                                                  | Phase                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| **Seams reserved**: ECS access declarations, mailbox between sim/client (exists as interpolation buffer), audio ring (exists), `tick(dt)` runner shape | P2 (design constraint, near-zero code)                                                  |
| **Tick-id protocol + client tick alignment** (lead + rate-based drift correction) — cheap now, misery to retrofit                                      | P2 (with the replication protocol)                                                      |
| Input thread + stacked `InputTickState` (accumulate-then-swap, last-N ring)                                                                            | P2 (with the action layer; single-thread runner inlines it on wasm)                     |
| `crcbl-jobs`: pool, `par_for` (both modes), mailbox/ring primitives + property tests                                                                   | **Built** at P5B — `spawn`, `pool`, `deque`, `mailbox`, `ring`; adopted by `apps/horde` |
| ECS parallel schedule (startup DAG, debug access asserts)                                                                                              | P8                                                                                      |
| Pipeline-thread formalization (named threads, timeline profiler view)                                                                                  | P8                                                                                      |
| Physics/anim/VFX `par_for` adoption                                                                                                                    | P8 → wave 1 as each system scales                                                       |
| wasm-threads pool re-enable (SharedArrayBuffer)                                                                                                        | post-MVP                                                                                |

The P2 line matters most: it costs almost nothing and prevents the classic
retrofit disaster. P5B, not P8, is where threads actually switched on — the
2026-08-03 correction moved it and the crate shipped there — with horde's
numbers as before/after proof.

~~The Web Worker spawner is the one piece of this that is genuinely still
missing.~~ **Landed 2026-08-23.** `crcbl_jobs::spawn` is still `#[cfg]`-split —
native gets `Threads` — but `wasm32` now gets `crcbl_jobs::workers::Workers`
rather than `Inline`. It cannot start a thread itself, because no wasm module
can; it **queues** each request and a page drains the queue through the
`__crcbl_web_jobs_*` exports, which keeps the engine's exports-plus-polling ABI
intact rather than adding the first import to a browser artifact.

Two things about it decide how the rest of this document reads. `Workers`
answers `Spawn::threaded` **false** until a page announces itself, so an
artifact loaded by a page with no shim degrades exactly as `Inline` did — which
is every page today, because **nothing implements the shim yet**. And `Pool`
itself cannot be driven from the browser's main thread at all: `par_for` and
`pool::work` both take a `std::sync::Mutex` and wait on a `Condvar`, which lower
to `Atomics.wait` and throw there. The topology this document settled — the game
worker owns the pool, the main thread forwards and presents — is therefore the
only arrangement that runs, not the preferred one.

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

## Correction (priority, 2026-08-03)

**This topic is moved up: the wasm target is to reach thread-topology parity
with native, rather than staying single-threaded as a post-MVP note.** What
follows is what was measured before committing, because two of the three
findings change the shape of the work rather than its schedule.

### Finding 1 — a threaded wasm artifact builds today

`RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals" cargo +nightly build --target wasm32-unknown-unknown -Z build-std=std,panic_abort`
compiles clean on `nightly-2026-07-02`, with one warning:
`unstable feature specified for -Ctarget-feature: atomics`.

**Cost: it is nightly, and `rust-toolchain.toml` pins an exact stable on
purpose** — its own comment calls a floating channel a broken promise, because
every clippy job runs `-D warnings` and a new release turns CI red on an
untouched repository. A threaded wasm build therefore needs a _second_, pinned
nightly for that target only, in the shape the `decoder-fuzz` job already uses.

**Correction (2026-08-23): that command's artifact is not usable by a worker, so
this finding read stronger than it was.** Built exactly as above,
`crcbl_horde.wasm` has **zero imports** and **exports** its memory. A worker can
only attach to a memory the host constructs and the module imports, so nothing
about that artifact is threaded beyond the atomic instructions being legal. The
link arguments are the missing half:

```
-C link-arg=--shared-memory  -C link-arg=--import-memory
-C link-arg=--max-memory=1073741824
-C link-arg=--export=__wasm_init_tls   -C link-arg=--export=__tls_base
-C link-arg=--export=__tls_size        -C link-arg=--export=__tls_align
-C link-arg=--export=__stack_pointer   -C link-arg=--export=__heap_base
```

With those, the module imports `env.memory`, and the bootstrap was **run**
rather than designed: under `node:worker_threads`, three workers instantiated
the same module against one shared memory and each executed Rust — shared-heap
allocation, a shared `AtomicU32`, and a per-worker `thread_local` all behaved,
and the main thread's own `thread_local` survived. Each worker needs its
`__stack_pointer` set and `__wasm_init_tls` called before it runs anything. Do
not expect either omission to announce itself. Skipping `__wasm_init_tls`
sometimes traps and sometimes does not: `__tls_base`'s initial value is a layout
accident, measured at 1048576 in one build — where it collides with the initial
`__stack_pointer` and the corruption trapped — and at zero in another, where
every worker's thread-locals aliased one harmless address and a
`const`-initialised `thread_local!` read and wrote it without complaint.
Skipping the `__stack_pointer` write is silently wrong in the same way. Both
have to be gated by observing separation directly.

**Two facts from that session belong in this plan, not only in the backlog.**
First, `Mutex` and `Condvar` work across workers, which is what decides whether
the pool needs redesigning: a worker blocked in `Condvar::wait` was woken with
the right count by three others, and a `wait_timeout(500 ms)` with nothing to
satisfy it returned in 500 ms reporting `timed_out` rather than in microseconds
— so it is a real futex wait, and `Pool` needs no change to run on workers.
Second, omitting the `__stack_pointer` export is a **silent** failure: every
worker keeps the main thread's stack, a closure that merely allocates still
returns the right answer every time, and the damage appears only where a worker
writes a large stack array. Any gate over this has to make a worker use its
stack.

### Finding 2 — `std::thread::spawn` compiles on wasm and fails at run time

This is the one that matters. In `library/std/src/sys/thread/mod.rs`, the arm
`all(target_family = "wasm", target_feature = "atomics")` takes **only `sleep`**
from `thread/wasm.rs`; `Thread`, `available_parallelism`, `yield_now` and
`set_name` all come from `thread/unsupported.rs`, where `Thread::new` returns
`Err(io::Error::UNSUPPORTED_PLATFORM)` and `available_parallelism` returns
`Err`.

So enabling atomics buys shared memory and working atomic primitives — and no
way to start a thread. A wasm module cannot create its own worker: only the host
can instantiate the module again against the shared `WebAssembly.Memory`.

**Consequence for the engine's shape.** `crcbl-jobs` must not spawn through
`std::thread`. It needs a spawn seam — native behind `std::thread`, wasm behind
a JS worker shim the `web/` half owns — and every subsystem thread in the
topology above starts through that seam. Designing it in from the start is
cheap; retrofitting it after `std::thread::spawn` is written through the crate
is not, which is the whole reason this was measured before any code.

### Finding 3 — the ceiling, which parity cannot reach

Three gaps stay open however much work is done, and they are properties of the
platform:

1. **The main thread cannot block.** `Atomics.wait` throws on the browser's main
   thread. The native topology's converge thread waits on mailboxes; the
   browser's main thread may not, so converge has to live in a worker and the
   main thread becomes a pure event forwarder.
2. **A dedicated worker has no `requestAnimationFrame`.** Moving render to a
   worker (which `OffscreenCanvas` allows) gives up rAF's vsync alignment,
   because rAF is a `Window` API. Presenting from a worker is timer- or
   message-driven, which is not the same clock native gets.
3. **`SharedArrayBuffer` still needs COOP/COEP**, which GitHub Pages cannot set
   — see topic 10's 2026-07-27 correction. The `coi-serviceworker` shim is the
   only route, and it costs a reload on first load plus CORP on every
   cross-origin subresource.

**So "parity" is precise rather than total**: the _thread topology_ — sim,
audio, io, net and a work-stealing pool, each publishing through latest-wins
mailboxes — can match native. _Loop ownership and presentation timing_ cannot,
and no amount of threading changes that: the browser owns the outer loop, which
is the constraint that already forbids a `crcbl::run(game)`.

### Order

1. The spawn seam in `crcbl-jobs`, with the native backend and a single-thread
   fallback. Everything else depends on its shape.
2. Cross-origin isolation on the Pages deploy, proved by the browser gate
   asserting `crossOriginIsolated === true` before anything relies on it.
3. The wasm worker backend behind the seam, and the pinned nightly for it.
4. Subsystem threads, in the order topic 21 already lists.

Step 2 is a gate rather than a step: if isolation cannot be had on Pages, the
demos stay single-threaded through the fallback and native keeps the topology —
which is exactly what the seam in step 1 exists to make survivable.

## Topology on the web, settled (2026-08-03)

The browser's split, decided rather than discovered, and it maps onto the
topology above with **one** substitution:

```
[OS main]   DOM/canvas/input events  ─┐  raw events → input ring (accumulate-then-swap)
            rAF + render submit       │
              ▲ latest-wins snapshot  │
              │                       ▼
[game]      the whole simulation topology: spawns and owns sim, io, net and the
   worker   pool workers; blocks freely on `Atomics.wait`; publishes snapshots
```

**Main is the converge thread, not merely an event forwarder.** The table above
already allows this — `[converge] … (may == OS main; target: wait≈0)` — and in
the browser it is the right half of the choice, because the alternative is
moving the canvas to a worker with `OffscreenCanvas` and thereby **giving up
`requestAnimationFrame`**, which is a `Window` API a dedicated worker does not
have. Presenting from a worker means presenting on a timer, which is not the
clock native gets.

Main blocking is forbidden; **main reading is not.** `Atomics.wait` throws on
the main thread, but `load`, `store`, `compareExchange` and `notify` are all
allowed there, so a latest-wins triple-buffered read — grab the newest complete
snapshot, never wait — is legal on main by construction. The mailbox discipline
this topic already specifies is exactly what makes the browser case work; it was
not chosen for that reason, and it pays for itself anyway.

**The game thread owns the topology, not main.** Nested workers are permitted,
so the game worker spawns sim, io, net and the pool itself. Main therefore knows
nothing about the thread graph: it forwards events and it presents. That is what
keeps the browser's main thread from becoming a second scheduler that has to be
kept in step with the real one.

**Two directions, two disciplines, and they are not interchangeable:**

- **main → game (input)**: the raw event ring is **accumulate-then-swap**, never
  latest-wins, because edges must not be droppable — a 3 ms tap must survive a
  16 ms tick. Main writes without blocking; the game thread waits when idle.
- **game → main (snapshots)**: **latest-wins**, because a stale frame is better
  than a stalled one and the consumer must never block.

**Without cross-origin isolation there is no ring**, and the fallback is
`postMessage` — which is what `crcbl-audio`'s worklet feed already does, and
which changes the shape rather than only the speed. That is why the isolation
gate is a gate.

## Corrections (2026-08-09)

- **`ring` cannot implement drop-oldest, and the primitives table promises it.**
  The overflow policy is listed as "counted + policy (drop-oldest or
  grow-in-dev)". Drop-oldest is impossible from the producer: the read cursor
  belongs to the consumer, and a producer advancing it would be a second writer
  to it — which is precisely what makes an SPSC ring cheap. `push` hands the
  item back and counts the refusal, leaving the policy to the caller. If a
  consumer ever genuinely needs drop-oldest, the honest options are a
  consumer-side drain-and-discard or an MPSC design, not a flag on this one.
  **The table is wrong as written.**
- **The delivery table still says P8.** `crcbl-jobs` moved to **P5B** by the
  2026-08-03 correction at the top of this file, and the samples adopt it there.
  The table below that correction was never updated. _(Fixed 2026-08-15; the
  crate is built and the row says so.)_
- **loom and TSAN are specified; Miri is what runs.** The testing section asks
  for loom-style exhaustive tests on the primitives and a TSAN job in scheduled
  CI. Neither exists. What exists is a **weekly**
  `cargo miri test -p crcbl-jobs` in `cron.yml`, which models memory ordering
  more thoroughly than any test on x86-64 can — a `Release` store and a
  `Relaxed` one compile identically there, so Miri is load-bearing rather than
  supplementary. One gap follows: an ordering regression can sit on `main` for
  up to a week.

  This bullet used to name a second gap — that nothing runs the primitives on a
  weakly-ordered machine, and an aarch64 runner would be independent evidence.
  **That was tested on 2026-08-23 and it is not true that a runner would give
  evidence.** `build + test (macos-latest)` is aarch64 and already runs the two
  concurrent tests. A throwaway branch weakened both orderings at once — the
  ring's `tail` store `Release` → `Relaxed`, the mailbox's handoff swap `AcqRel`
  → `Relaxed` — and the whole run went **green**, with both tests logging `PASS`
  rather than being skipped. Twenty thousand iterations on Apple silicon
  surfaced nothing. So the aarch64 leg is reassurance, and Miri is not merely
  load-bearing but the only thing holding these orderings. If that coverage is
  wanted for real the instrument is a targeted stress harness — many short runs
  under interleaving pressure, with a failure counter — not another runner.

- ~~Finding 1's threaded-wasm measurement is not currently reproducible.~~
  **Reproducible since 2026-08-22**, when `rust-src` was added to
  `nightly-2026-07-02`, and reproduced on 2026-08-23. It is now a command anyone
  can run rather than a measurement someone once took:
  `./web/build.sh --threads` builds every demo crate that way and gates each
  artifact's worker-capable surface with
  `web/tools/check-exports.mjs --threads`. No CI job runs it — no runner has the
  nightly — so it is a local gate, in the same position as
  `web/run-browser-e2e.sh`.
