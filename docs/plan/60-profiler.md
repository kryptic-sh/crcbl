# Topic 60 — The profiler: what the CPU, the GPU and the driver can tell, read by crcbl itself

Written 2026-10-28, at the owner's request, after a day spent finding out why
the mesh-shader path is slower than IndirectCount on an RX 7900 XTX with nothing
better than per-pass timestamps, hand-written mirrored-run scripts and guesses
(`docs/backlog.md`, _D3D12 mesh shading (shipped 2026-10-28)_). The owner's
constraints: **crcbl's own profiler, no external tool** — it talks to the GPU
through the driver itself, the way vendor profilers do — and it is built so the
editor ([08-editor.md](08-editor.md)) can host it later. Nothing in this
document past _Where this is_ is built.

## Where this is

Built, and the profiler is built on it rather than beside it (the rules each
piece was built under are in [tooling notes](../notes/tooling.md), _What the
deleted 40-profiling plan left behind_ and _Profiling and benchmarking:
decisions taken 2026-08-13_):

- **CPU spans**: `crcbl_core::trace`, runtime-gated by `CRCBL_TRACE`, with
  `crcbl::perf`'s vocabulary for the loop's phases; `crcbl_core::stats`'
  percentiles.
- **GPU pass timings**: `crcbl_render::timing`, a timestamp pair per render
  graph pass, frames latent, a pass's span including its barriers;
  `crcbl_render::PassStats` and the debug overlay's per-pass p50/p95 table.
- **Counters the engine knows**: `crcbl_render::counters` (`FrameCounters`),
  `crcbl_render::cull_stats` (`CullStatsRing`), `crcbl_jobs::Pool::stats`.
- **Benchmarks**: `crcbl bench --scenario jobs|phys` with warm-up,
  p50/p95/p99/max, `--json` and an environment block.
- **Pipeline statistics as compiled**: `CRCBL_VK_SHADER_STATS=1`
  (`crcbl_vk::shader_stats`, 2026-10-28) logs each Vulkan pipeline's per-stage
  statistics through `VK_KHR_pipeline_executable_properties`.
- **Names on the GPU's own record**: `Features::DEBUG_MARKERS` labels every pass
  through `VK_EXT_debug_utils`, D3D12's `BeginEvent` and Metal's debug groups.
- **Crash breadcrumbs**: D3D12's DRED is always on (`crcbl_dx12::dred`).

What is missing, each a gap this session hit:

1. **No way to count work per pass.** `QueryKind::PipelineStatistics` exists and
   nothing can fill one: `CommandEncoder` has no verb that records into it, and
   `Device::query_results` reads one `u64` per query where a statistics query is
   eleven (D3D12), up to fourteen with the mesh stages
   (`D3D12_QUERY_DATA_PIPELINE_STATISTICS1`) or eight (Metal). The
   `PipelineStatisticsQuery` divergence rows say so. Task and mesh invocation
   counts per pass would have answered in one run what took five experiments.
2. **No hardware counters.** Nothing reads how busy a shader stage, a cache or
   the memory bus was. Which counters exist is per vendor and per API (below).
3. **No sub-pass timing.** A pass is one span; which draw inside it costs is
   invisible.
4. **No capture, no export, no viewer.** Spans go to a text summary
   (`Snapshot::report`); nothing writes a trace a viewer opens (topic 40's piece
   4), and there is no recorded capture to look at after the run.
5. **A/B by hand.** Every comparison today is a scratch shell script running
   mirrored pairs and a Python fold of the logs. The method is settled (mirrored
   order, per-run p50s, ranges, "within spread" as a verdict); the tool is not.
6. **No memory accounting** (topic 40's piece 5): how much of the GPU's memory
   each category holds, against the budget the driver reports.

### What the drivers here expose, measured 2026-10-28

On the Windows desktop (RX 7900 XTX and Raphael iGPU, AMD 25.10.36), from
`vkEnumerateDeviceExtensionProperties`:

| Extension                                     | Present | What it is for                                     |
| --------------------------------------------- | ------- | -------------------------------------------------- |
| `VK_KHR_performance_query`                    | **no**  | the cross-vendor hardware counter API              |
| `VK_AMD_gpa_interface`                        | yes     | AMD's own counter and trace interface              |
| `VK_KHR_pipeline_executable_properties`       | yes     | per-stage compiled statistics (used, above)        |
| `VK_AMD_shader_info`                          | yes     | per-stage register use and disassembly             |
| `VK_KHR_calibrated_timestamps`                | yes     | one GPU/CPU clock pair, to align the two timelines |
| `VK_EXT_memory_budget`                        | yes     | the driver's per-heap budget and use               |
| `VK_AMD_buffer_marker`, `VK_EXT_device_fault` | yes     | where a hung or lost device was                    |
| `VK_EXT_debug_utils` (instance)               | yes     | labels and object names                            |

Not measured here, and to be measured before a slice relies on them:
`VK_KHR_performance_query` on NVIDIA and on Mesa's ANV and RADV (the office
laptop has an Intel RPL-U and an NVIDIA MX550); D3D12 has no cross-vendor
counter API at all (vendors ship their own libraries); Metal has `MTLCounterSet`
(timestamp, stage utilisation, statistics) where the device lists it, and CI's
Mac lists none; WebGPU has timestamps and nothing else.

## What a profiler here has to do

The whole feature set, so the slices below build toward one design rather than a
pile of tools, and so the editor finds what it needs when it arrives. Each row
says which rung delivers it.

### Data it collects

| Source                       | What                                                                                                                | Rung              |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------- |
| CPU spans                    | every thread's scoped spans, the loop's phases, jobs as their workers' tracks                                       | have, P2          |
| GPU pass timings             | per render-graph pass, per queue, frames latent                                                                     | have              |
| GPU sub-pass timings         | per draw/dispatch call inside a pass, on request                                                                    | P4                |
| Pipeline statistics          | per pass: vertex/primitive counts, each stage's invocations (task and mesh included), clipping                      | P1                |
| Hardware counters            | per pass, then per call: stage busy, waves/occupancy, cache hit rates, memory traffic                               | P5                |
| Compiled pipeline statistics | per pipeline and stage: registers, local memory, scratch, instruction counts                                        | have (Vulkan), P6 |
| Memory                       | GPU memory by category (buffers, images, transients, pools) against the driver budget; CPU allocations by subsystem | P7                |
| Synchronisation              | CPU waits on fences, present waits, queue submissions and their overlap                                             | P2                |
| Frame pacing                 | present timing, frame intervals, missed vblanks                                                                     | have (partly), P2 |
| Engine counters              | `FrameCounters`, cull stats, pool stats, any subsystem's named counters                                             | have, P2          |
| Device health                | device-lost breadcrumbs (DRED, `VK_EXT_device_fault`, buffer markers) attached to the capture                       | P8                |

### How it captures

- **Always available, off by default, near-free when off.** The rule for spans
  stands (an atomic load when disabled, never a rebuild to turn on). GPU queries
  cost per pass, so the GPU half is enabled per capture, and a capture says what
  was on.
- **A rolling window and a triggered capture.** The last N frames are kept in a
  ring; a capture freezes a window (N frames back, or the next N) on a trigger:
  a key, a console command, the CLI, the editor, or a condition (a frame over a
  budget, a hitch).
- **Replay where counting needs several passes.** A hardware counter set can
  exceed what one pass can sample, so a capture of counters re-runs the same
  frames once per group. The headless loop is deterministic by construction
  (fixed clock, seeded simulation), so a recorded frame range replays to the
  same work; a live game captures the counter groups on consecutive frames and
  says so.
- **No stalls.** Results stay frames latent, as timing's do; a capture waits for
  its own results to arrive rather than flushing the GPU.
- **Honest about its own cost.** A capture records what it enabled and its
  measured overhead, and a profile mode that perturbs the frame (statistics
  queries, per-call timestamps) is marked as such in every view.

### What it shows

- A **timeline**: CPU threads and GPU queues on one time axis, aligned by a
  calibrated clock pair per capture (not by assuming the clocks agree), with
  spans, passes, calls, counters as tracks, markers for frames and triggers.
- A **frame view**: the render graph as it ran — passes, their resources and
  barriers, each with its time, statistics and counters.
- **Tables**: per pass and per call, with p50/p95/p99/max over the captured
  frames, sortable; per pipeline, its compiled statistics.
- **Derived metrics**, computed from counters by per-vendor formulas kept in one
  table (utilisation, occupancy, bandwidth), each naming the counters it used.
- **Comparison**: two captures, or two builds, side by side, per pass and per
  counter, as ratios with ranges, and a verdict ("within spread", "slower,
  ranges apart") rather than a bare percentage.
- **Baselines**: a capture or a scenario's summary stored per machine and
  compared later; never across machines (the existing rule).

### Where it is used from

- **The engine API** (`crcbl::profile`): start, stop, trigger, read; what every
  other interface calls.
- **The console**: `profile.capture <frames>`, `profile.counters <set>`,
  `profile.save <path>`.
- **The HUD**: the debug overlay's existing rows, fed from the same data.
- **The CLI**: `crcbl profile run` (a sample or scenario, headless, N frames, to
  a capture file), `crcbl profile report <capture>`,
  `crcbl profile compare <a> <b>`, and `--ab <exe-a> <exe-b>` running mirrored
  pairs itself.
- **The editor**: a profiler panel reading the same captures, live from the
  editor's own engine instance or from a game process over a socket.
- **Remote**: a running game (another machine, a phone) serves its captures over
  a socket the editor or the CLI connects to; the protocol carries the capture
  format, nothing viewer-specific.

### What it writes

- **A capture file of its own** (`.crcblprof`): versioned, self-describing
  (adapter, driver, backend, the capability selectors, build profile, commit,
  the counter set and units), holding every track above. The format is the
  contract between producers and viewers, so the editor never needs the engine
  that recorded it.
- **Chrome Trace Event JSON** for Perfetto and `chrome://tracing` (the decision
  of 2026-08-13 stands), lossy by design: spans, passes, counters.
- **Summary JSON**: per pass, per counter, percentiles and the environment
  block, the format baselines and comparisons read (`crcbl bench`'s, extended).

## The decisions

1. **One profiler, crcbl's own, reading the driver directly.** No external
   profiler is installed, launched or required (owner, 2026-10-28). Labels stay
   on the GPU's own record (`DEBUG_MARKERS`), which costs nothing and keeps the
   engine legible to any tool a user brings, but nothing here depends on one.
2. **Producers and viewers are separate, and the capture format is the seam
   between them.** The engine produces captures; the HUD, the CLI and the editor
   are viewers of the same format. That is what makes the editor panel a viewer
   to write rather than a second profiler, and remote profiling a transport
   rather than a feature.
3. **The HAL grows the verbs, the backends fill them, and a missing one is a
   divergence row.** Pass-scoped statistics writes beside `PassTimestampWrites`;
   results of a declared width; a counter-set description per device (names,
   units, how many per pass); per-call timestamps. A backend without one says so
   in `crcbl_hal::DIVERGENCES` and degrades to an empty track, as timestamps do
   on a device without `TIMESTAMP_QUERY`.
4. **Vulkan, D3D12 and Metal, with Metal after the other two** (owner,
   2026-10-28: Metal waits until the owner works on a Mac, which is also the
   first time a Metal device that lists counter sets is at hand; CI's Mac lists
   none). Every rung is designed for all three from the start, so the seam does
   not have to change when Metal arrives; Vulkan and D3D12 are built and proved
   first. WebGPU keeps its timestamps and gets nothing more, because the
   browser's API has nothing more (a divergence row, not a gap).
5. **Counters through each vendor's own interface, transcribed, not linked.**
   `VK_KHR_performance_query` where a driver has it; `VK_AMD_gpa_interface` on
   AMD's Vulkan driver, which has nothing else, its structures transcribed from
   AMD's published GPU Performance API sources (MIT) rather than linking that
   library; Metal's counter sets. D3D12 has no cross-vendor counter API. On AMD,
   the GPU Performance API reaches D3D12 counters through the driver's own
   extension interface, which is the same kind of thing as the Vulkan one and is
   transcribed the same way if its published definitions allow it (checked when
   P5 starts, not assumed). NVIDIA's and Intel's D3D12 counters come only
   through their libraries (the Nsight Perf SDK, Intel's Metrics Discovery),
   each a new dependency and the owner's call; until then those adapters get
   D3D12's statistics and timings, which every vendor has.
6. **Every number is checked against something that knows the answer.**
   Statistics against fixtures with known counts (a triangle is three vertex
   invocations and one primitive; a cluster dispatch is its cluster count of
   mesh workgroups); counters by relations the hardware must satisfy (a stage
   cannot be busier than the frame, waves launched match invocations over the
   wave width) and against the timings they explain. Each check is shown red
   before it is trusted, as every test here is.
7. **Thread traces are not in scope.** Instruction-level timing (AMD's SQTT,
   NVIDIA's trace) is what a vendor's own profiler is for and reimplementing one
   is out of proportion; counters per pass and per call answer the questions
   this engine has had. Revisit if a question needs one.

## The rungs

Each rung is usable on its own and proved before the next starts.

- **P1 — Pipeline statistics per pass.** The seam: a pass-scoped statistics
  write (`PassStatisticsWrites`, beside `PassTimestampWrites`), a query result
  of a declared width, the stage set reported per device (task and mesh
  invocations where the device has them: Vulkan's `VK_EXT_mesh_shader` bits,
  D3D12's `PIPELINE_STATISTICS1` under `MeshShaderPipelineStatsSupported`).
  Vulkan and D3D12 now, Metal's statistic counter set when the Mac work starts,
  WebGPU a divergence. `crcbl_render::timing` records them per pass beside its
  timestamps when asked. Proved by a fixture with known counts on every backend
  that has it; first used on the mesh path's depth passes.
- **P2 — The capture.** The ring of frames, the trigger, the capture file, the
  calibrated clock pair, CPU spans and GPU passes on one timeline, engine
  counters and synchronisation as tracks; `crcbl profile run|report` and
  `profile.capture`. Chrome trace export from the same data.
- **P3 — Comparison.** `crcbl profile compare` and `--ab`: mirrored pairs,
  per-run percentiles, ranges, verdicts, the environment refusal; summaries as
  baselines per machine.
- **P4 — Per-call timings.** Timestamps around each draw and dispatch of a pass
  on request (a profile mode, marked as perturbing), mapped back to the bucket
  or the cluster partition that recorded them.
- **P5 — Hardware counters.** The counter-set description, per-pass sampling,
  replay across groups; `VK_AMD_gpa_interface` first (the desktop here), AMD's
  D3D12 extension interface beside it, then `VK_KHR_performance_query` (measured
  on the office laptop first); derived metrics. Metal's counter sets with the
  Mac work; NVIDIA's and Intel's D3D12 counters on the dependency decision.
- **P6 — Compiled pipeline statistics everywhere it exists**: Vulkan's into the
  capture rather than the log, `VK_AMD_shader_info`'s register counts and
  disassembly per pipeline on demand; D3D12 and Metal have no driver query for
  it, which is a divergence, not a gap to paper over.
- **P7 — Memory.** Per-category GPU memory against `VK_EXT_memory_budget` and
  DXGI's `QueryVideoMemoryInfo`, transient and pool occupancy, CPU allocations
  by subsystem.
- **P8 — Device health in the capture**: a lost device's breadcrumbs (DRED,
  `VK_EXT_device_fault`, buffer markers) saved with the frames before it.
- **P9 — Viewers**: the HUD's rows from captures, then the editor's panel
  (timeline, frame view, tables, comparison), then remote capture over a socket.

## How it is checked

- Each rung's numbers against known answers (decision 6), mutation-proven.
- The profiler's own cost measured by a benchmark of the profiler, with every
  track off and on (the existing rule), and recorded in the capture.
- A capture recorded on one machine opens in a viewer built without a GPU: the
  format is tested by round trip and by a fixture capture committed beside the
  reader.
- The first real use is the open question that prompted this: the mesh path's
  task stage on the RX 7900 XTX (`docs/backlog.md`), answered with P1's
  invocation counts and P5's counters.
