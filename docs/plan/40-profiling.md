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
- **A pass's span includes its barriers**, because `crcbl-hal`'s encoder scope
  rules put query writes outside any pass. That is the more useful number: a
  pass whose barriers cost more than its draws is a real finding, and one that
  hid its barriers in a neighbour's bucket would not surface it.
- **Degrading rather than breaking**: a device without
  `Features::TIMESTAMP_QUERY` gets no timers and an empty report, which is what
  browsers actually do.
- **`Features::DEBUG_MARKERS`** for capture tools.
- **`crcbl-cli`** with `new` and `screenshot`, `--json` output, and a report
  module.

## What is missing

The GPU side is genuinely good and the rest is not there at all:

1. **No CPU-side spans.** Nothing measures the tick, the ECS schedule, physics,
   asset upload, culling's CPU half or the shell's frame. The GPU report says
   which pass cost what; nothing says whether the frame was GPU-bound at all.
2. **No benchmark harness.** `apps/horde` has ad-hoc flags (`--wall-clock`,
   `--fps 0`, `--tick-hz 1`, `--frames`, `--prefill`) and the numbers in
   [sample/03-horde.md](sample/03-horde.md) were produced by hand. There is no
   fixed scenario set, no warm-up, no statistics beyond a mean, and no
   machine-readable output.
3. **No baseline or regression detection.** Nothing stores a previous run to
   compare against, so "is this slower than last week" is unanswerable.
4. **No trace export.** Nothing a real profiler UI can open.
5. **No memory or occupancy accounting.** Pool residency, buffer bytes,
   descriptor counts, staging-ring pressure — all invisible.
6. **No job-system instrumentation.** `crcbl-jobs` has a work-stealing deque and
   exposes no worker utilisation, steal counts or queue depth, so the phase that
   adopts it cannot show it helped.
7. **Counters are piecemeal.** `SceneStats`, `visible_count` and each sample's
   own rows exist; there is no one place a frame's draw count, instance count,
   cluster count or triangle count is reported.
8. **The culling stats never leave the GPU.** An earlier draft of this file
   listed "the culling-stats readback on a delayed ring" under what already
   exists. It does not: `DrawGen::visible_count` is a `DeviceLocal` buffer with
   `TRANSFER_SRC`, and `crcbl-hal` has the poll-shaped
   `request_readback`/`poll_readback` the frame loop is allowed to use — but
   there is no staging buffer, no copy inside the frame graph, and no consumer.
   Every read of it in the tree is a test copying it back by hand outside the
   frame loop. **Until that ring is built, the culling win and the cluster
   counts cannot be reported at all** — which is why the counters row says
   `indirect` rather than a number wherever a `ForwardRenderer` is in the frame.
   Building it is its own slice: a `HostReadback` buffer per frame in flight, a
   copy the graph schedules rather than the hand-written barriers
   `screenshot.rs` uses, and four-backend verification.

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
  which of the two is the budget — the single most useful row, because
  "GPU-bound" is the first question and nothing answers it today.
- **Pass list**: the existing per-pass GPU times, sorted by cost, with the
  frame's total and each pass's share.
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

| Slice                                                                            | Phase                                                     |
| -------------------------------------------------------------------------------- | --------------------------------------------------------- |
| CPU spans + counters + the always-on/runtime-gate decision; frame CPU vs GPU row | P7 (the GPU half already exists and is unmatched)         |
| `crcbl bench` with fixed scenarios, warm-up, percentiles, JSON output            | P8 (the job system is the first thing that needs proving) |
| Trace export (Chrome Trace JSON) + job-system tracks                             | P8                                                        |
| Baseline storage + `--compare` + thresholds                                      | P8                                                        |
| Memory/occupancy accounting and its rows                                         | P9 (assets and pools are what make it interesting)        |
| The rest of the debug panel's perf rows; freeze toggle                           | P10 (with the UI slice that owns the panel)               |
| Tracy or another external profiler, if wanted                                    | later, on demonstrated need and a dependency decision     |
