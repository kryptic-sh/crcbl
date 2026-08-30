# Topic 40 — Profiling, Benchmarking and Perf Tooling

Measuring the engine: where a frame's time goes on the CPU and the GPU, a
benchmark harness that produces numbers worth comparing, and the debug panel
rows that make a slowdown visible while you play rather than after you export.

**The tooling lands before the perf work, not with it.** A profiler bolted on
afterwards is one that never covers the code written before it —
[01-foundations.md](01-foundations.md) §1.3 already makes that argument for
putting timestamp queries in the seam at P0, and it applies to every other span
this topic adds. The optimisation phase this feeds is deliberately later; what
this topic delivers is the ability to know what to optimise, and to prove
afterwards that it worked.

## What already exists (do not rebuild it)

- **Per-pass GPU timestamps.** `crcbl_render::timing` — `PassTimers`,
  `FrameTimings`, `PassTiming` — wired into `CompiledGraph::execute` rather than
  bolted on. The report is **frames latent by design**: a ring of query sets,
  one more than the frames in flight, resolved only when a slot comes back
  round, so there is no fence, no wait and no `wait_idle`. The latency _is_ the
  synchronisation.
- **Per-pass distributions over a run**, since 2026-08-28.
  `crcbl_render::PassStats` takes every distinct `FrameTimings` the timers
  resolve and keeps a rolling `crcbl_core::stats::Window` per pass label, which
  `Loop::finish` reports as the `gpu passes` line: p50, p95 and share of the p50
  total for each label, over the last `DEFAULT_FRAME_WINDOW` frames. It replaces
  a line that printed the newest latent `FrameTimings` verbatim — one arbitrary
  frame of the run, which is the shape of measurement this topic's "percentiles,
  not means" decision exists to refuse, and which forced
  [45-shadows.md](45-shadows.md)'s eleventh decision to be medians of five
  hand-run binaries. **A label is summed within the frame rather than tracked
  per occurrence**: `lantern` renders two views, so `shadow`, `forward` and
  `tonemap` each appear twice in its report and the cull passes once per
  cascade, and the occurrence count is on the row.
- **A pass's span includes its barriers**, because `crcbl-hal`'s encoder scope
  rules put query writes outside any pass. That is the more useful number: a
  pass whose barriers cost more than its draws is a real finding, and one that
  hid its barriers in a neighbour's bucket would not surface it.
- **Degrading rather than breaking**: a device without
  `Features::TIMESTAMP_QUERY` gets no timers and an empty report, which is what
  browsers actually do.
- **`Features::DEBUG_MARKERS`** for capture tools.
- **`crcbl-cli`** — every verb `crates/crcbl-cli/src/args.rs`'s `Command` parses
  (`crcbl bench` among them, below), `--json` output on all of them, and a
  report module. [11-cli-headless.md](11-cli-headless.md) owns the list.

## What is missing

1. **The ECS schedule, physics and asset upload are unspanned.** They are not
   phases the engine's loop has — they live inside a game's `tick` — so
   `crcbl::perf`'s span vocabulary, which names the loop's own phases, does not
   reach them.
2. **The benchmark scenarios that need a device are not written.**
   `crcbl bench --scenario jobs|phys` opens none, so its environment block
   carries no adapter, backend or driver version, and `apps/horde`'s ad-hoc
   flags (`--wall-clock`, `--fps 0`, `--tick-hz 1`, `--frames`, `--prefill`) are
   still how the numbers in [sample/03-horde.md](sample/03-horde.md) get
   produced.
3. **No baseline or regression detection.** Nothing stores a previous run to
   compare against, so "is this slower than last week" is unanswerable.
   **Scheduled 2026-08-30 as the next slice of this topic**, because the render
   ladders now land a pass a day and nothing measures what each one costs: the
   per-pass GPU timestamps `PassStats` already collects are written by
   `crcbl bench` to a baseline file **per machine** (never committed, never
   compared across hosts), a rerun reports each pass as a ratio against it, and
   a pass above 1.15× is the red line — **locally**, on the developer's own
   adapter, which is the only place the 2026-08-13 decision below allows a
   timing to fail. Three tiers are what a rung is priced on: the desktop
   adapter, lavapipe, and the browser, and `43-render-standards.md`'s delivery
   table carries the rule that a rung's cost row is filled before it is called
   built.
4. **No trace export.** Nothing a real profiler UI can open.
5. **No memory or occupancy accounting.** Pool residency, buffer bytes,
   descriptor counts, staging-ring pressure — all invisible.
6. **The job-system counters reach no trace and no panel row.** `Pool::stats` is
   read only by `crcbl bench --scenario jobs`; nothing puts the numbers on the
   trace or in a panel row, and `crcbl-jobs` still has no spans — it does not
   depend on `crcbl-core`, so the dependency the span module's placement
   decision anticipated has not been added yet.

## Decisions (taken 2026-08-13, so they are not re-argued)

- **Trace export is Chrome Trace Event JSON**, which Perfetto and
  `chrome://tracing` both open. It is text, it needs no dependency, and
  `crcbl-cli` already has JSON machinery. **Tracy is not adopted**: it is a
  client library and therefore a new dependency and a user decision, and its
  wire protocol is not something to hand-roll. If Tracy is wanted later it is an
  optional feature over the same span data, not a second instrumentation pass.
- **Spans are always compiled and runtime-gated**, not feature-gated out. A
  profiler you must rebuild to use is one nobody turns on mid-investigation, and
  a build that changes what it measures is the classic way to measure the wrong
  thing. The cost when disabled is one relaxed atomic load per span. A
  compile-time switch exists for shipping builds and is off by default in `dev`.

  **And it must not be a Cargo feature** — decided 2026-08-13, after building
  one and finding out what it did. CI's only two workspace test runs both pass
  `--all-features`, so an additive `trace-off` feature is _on_ in CI and every
  test then asserts the compiled-out arm: a green light on code CI never ran.
  Feature unification means a top-level binary could not turn it back off
  either. When the switch earns its place it is `--cfg crcbl_trace_off` with a
  `build.rs` declaring `cargo::rustc-check-cfg`, which does not unify —
  `--all-features` keeps testing the real code and a shipping build sets
  `RUSTFLAGS`. Until there is a shipping build to serve there is no switch at
  all: the runtime gate's measured cost when off is one plain byte load and a
  tail jump, which is the number this bullet is asking for.

- **Percentiles, not means.** A benchmark reports p50, p95, p99 and max. Frame
  time is a tail problem — a mean hides exactly the stutter a player notices,
  and this project has already recorded a case where a within-arm spread was
  wider than the between-arm difference being claimed.
- **A benchmark pins everything it can and records everything it cannot.** Fixed
  seed, fixed tick rate, fixed frame count, explicit warm-up, headless, named
  adapter and backend; the output carries adapter name, driver version, backend,
  `GeometryPath`/`BindingModel`/`LightingPath`, build profile and the commit. A
  number without those is not comparable to another number.
- **CI does not gate on absolute timings.** A shared runner is far slower and
  far noisier than a dev box — the roadmap already says so — so CI runs the
  benchmark to prove it _runs_ and publishes the numbers as an artifact.
  Regression comparison happens against a baseline recorded on a known machine.
  A perf gate on CI hardware would fail for reasons that have nothing to do with
  the commit, and a gate people learn to ignore is worse than none.
- **The GPU report stays frames-latent.** No benchmark mode "reads it properly"
  by stalling: a stall changes the thing being measured. A benchmark runs long
  enough that latency does not matter.

## The span API

One shape, used everywhere, so a trace has one timeline:

- A scoped CPU span with a static name, opened and closed by RAII, nesting
  freely; the frame is the outermost.
- Spans carry the thread they ran on, so the job system's workers appear as
  their own tracks.
- GPU spans come from `crcbl_render::timing`'s existing per-pass timestamps and
  are emitted onto their own track, aligned to the CPU timeline by one
  calibration point per frame rather than by assuming the two clocks agree.
- **Counters are spans' siblings**, not log lines: a named `u64` sampled per
  frame — draws, instances submitted and drawn, clusters, triangles, pool bytes
  resident, staging bytes in flight, jobs run, steals.

### Where it lives (taken 2026-08-13)

**`crcbl_core::trace`**, beside `crcbl_core::time` and `crcbl_core::log`. Those
are the same kind of thing — the facilities every other crate reaches for and
none of them owns — and `crcbl-core` is already the bottom of the graph: it
depends on nothing of ours, and every crate that has to open a span depends on
it already, except `crcbl-jobs`, which gains it.

A separate `crcbl-trace` crate was the alternative and was declined: it would
buy separation nothing needs, and a new crate is structure to carry forever for
a module that is a few hundred lines. If the trace machinery ever grows a
serialiser and a wire format big enough to want its own compile unit, moving it
is a re-export away — which is not true of the dependency edge, and the
dependency edge is the part that has to be right now.

> **The `--cfg crcbl_trace_off` switch is still unbuilt**, which is what the
> decision above asks for until there is a shipping build to serve.

## `crcbl bench`

A CLI subcommand beside `screenshot`, headless, one scenario per invocation:

- **Scenarios are named and fixed**, and live with the samples that own them —
  horde's 10 000 enemies, the dunes patch at several camera distances, the
  sprite and UI scenes. A scenario names its seed, its frame count and its
  warm-up.
- **Output is JSON by default** (the CLI's existing `--json` shape) with a human
  summary on request. It carries the statistics above plus the environment
  block, and it is the format the baseline is stored in.
- **`crcbl bench --compare <baseline>`** reports per-metric deltas and flags
  anything outside a stated threshold. The threshold is per metric and recorded
  in the scenario, because a 5 % move in a 0.1 ms pass is noise and a 5 % move
  in frame time is not.
- **`crcbl bench --trace <path>`** writes the Chrome Trace JSON for one run.

## Debug panel: the perf rows

[07-ui-debug.md](07-ui-debug.md) owns the panel; these are the rows this topic
adds, and they are what makes a slowdown visible without exporting anything:

- **Frame**: CPU frame time and GPU frame time side by side with p50/p95, and
  which of the two is the budget — the single most useful row, and the reason it
  is first: "GPU-bound" is the first question and nothing answered it. It shows
  two rolling distributions rather than pairing one frame's two numbers, because
  the GPU report is frames latent and a single-frame pairing would be wrong by
  exactly that offset. Every row below it is still owed.
- **Pass list**: the existing per-pass GPU times, sorted by cost, with the
  frame's total and each pass's share. **The _panel_ row does not have this**:
  what the overlay shows is still `FrameTimings`' own `DebugModule`, one latent
  frame at a time, which is the right thing for a live row and the wrong thing
  for a comparison.
- **CPU breakdown**: tick, schedule, physics, upload, record, present-wait.
- **Counters**: draws, instances submitted vs drawn (the culling win, visible),
  clusters drawn and their level histogram, triangles.
- **Memory**: pool bytes resident and capacity, staging bytes in flight,
  descriptor counts.
- **Jobs**: worker utilisation, tasks run, steals, longest queue.
- **A freeze toggle**, so a spike can be read rather than chased.

Rows follow the existing `DebugModule`/`DebugSection` shape. Note the recorded
hazard: labels share one namespace across modules and nothing detects a
collision, so a test that searches the draw list by label text can silently read
the wrong row.

## Risks

- **Instrumentation that changes what it measures.** Mitigation: the disabled
  cost is one atomic load; the enabled cost is measured and reported by a
  benchmark of the profiler itself, which is the only honest way to know.
- **Numbers nobody can compare.** Mitigation: the environment block is
  mandatory, and a comparison against a baseline from different hardware is
  refused rather than printed.
- **A perf gate that cries wolf.** Mitigation: CI does not gate on timings, as
  decided above.
- **Counters that lie by omission.** A counter that stops being incremented when
  a path changes reads as an improvement. Mitigation: counters that must move
  are asserted in tests the way the culling counts already are.

## Delivery

| Slice                                                                                                       | Phase                                                            |
| ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| `crcbl bench` with fixed scenarios, warm-up, percentiles, JSON output                                       | The sample-owned scenarios, which need a device, are not written |
| Trace export (Chrome Trace JSON) + job-system tracks                                                        | P8                                                               |
| Baseline storage + `--compare` + thresholds, per machine, 1.15× per pass — the next slice here (2026-08-30) | P8                                                               |
| Memory/occupancy accounting and its rows                                                                    | P9 (assets and pools are what make it interesting)               |
| The rest of the debug panel's perf rows; freeze toggle                                                      | P10 (with the UI slice that owns the panel)                      |
| Tracy or another external profiler, if wanted                                                               | later, on demonstrated need and a dependency decision            |
