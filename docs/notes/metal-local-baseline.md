# Metal hardware baseline and performance — 2026-09-10

The current Metal renderer was exercised offscreen on an Apple M3 Pro. This
report records the baseline, the fixes it exposed, and the scope of the measured
performance improvement. Measurements started at
`c2515247cfc9353cb5894e491aa87b8e45aee7b0`.

## Environment

- Apple M3 Pro, macOS 26.5.2; Integrated, Bindless, Rasterised.
- Rust 1.97.0, cargo-nextest 0.9.140.
- Metal Toolchain 17F109 (`com.apple.dt.toolchain.Metal.32023.883`).
- API validation interposed `MTLDebugDevice`; shader validation was enabled for
  correctness runs. Performance runs disabled both validation modes.
- Timestamp counter set and stage-boundary sampling were available. Both
  correlated clocks advanced over 50 ms, with a CPU/GPU tick ratio of 1.

All runs were offscreen. The live drawable test was excluded. These results do
not establish window/presentation behavior or results on other Metal devices.

## Baseline findings and fixes

### Render state and empty work

The original hardware harness passed 69 tests, and all 59 renderer goldens
passed with API and shader validation in logging mode. The historical UI frame
failure did not reproduce. Logging mode nevertheless exposed 5,929 redundant
compute-pipeline binds, 11,572 redundant buffer binds, 146 redundant raster
setters, and state overwritten before a draw consumed it.

Packing dispatches now skip unchanged bindings. Render replay applies pipeline,
viewport, scissor and stencil state when a draw consumes it, and skips unchanged
raster state. Caches remain local to one encoder and recorded resources remain
retained through replay.

Strict validation also exposed zero-instance culling passes whose closures
returned without dispatching. The graph now omits those passes while retaining
counter clearing and argument generation, including after populated frames.

One ordinary unit test opened a real device without a hardware annotation; it
now belongs to the hardware suite. The harness accepts the ordinary device-open
validation report, so a filtered run need not include a separate reporting test.

### Timestamp correctness

The HAL seam initially failed because a clear-only render pass's closing sample
preceded its opening sample. Independent native probes showed that:

- Clear-only passes write vertex-stage timestamps but no fragment timestamps.
- An absent fragment stage leaves zeros in a fresh sample buffer and the
  previous draw's timestamps in a reused buffer.
- Empty compute encoders produce no samples, even with a memory barrier or
  resource declaration. A one-thread no-op dispatch preserves the samples.
- A zero-count indirect dispatch still produces samples.

Both vertex end and fragment end now write the render pass's closing slot.
Vertex completion supplies a fresh end when no fragment stage runs; fragment
completion overwrites it when present. The permanent hardware regression
alternates draw/clear/draw/clear in one sample buffer, with an independent
fragment-start sample. Removing either closing write was compiled and observed
to fail the corresponding case.

A lazily cached no-op kernel runs only for timed compute encoders with no
recorded dispatch. Zero raw timestamps remain zero during CPU clock conversion.
The HAL seam now verifies ordered samples and agreement between CPU and GPU
resolve paths. The previously unrun Metal timestamp divergence was retired only
after that hardware proof. Devices without stage-boundary sampling remain
excluded from the hardware regression.

### Profiling accounting

An eight-second Metal System Trace explained why summed pass timings were much
larger than observed frame throughput. Pairing intervals by API encoder ID gave:

| Pass    | Paired encoders | Median outer span | Median summed active stages |
| ------- | --------------: | ----------------: | --------------------------: |
| Forward |           3,658 |       3,450.75 us |                   591.08 us |
| SSAO    |           3,660 |       2,844.85 us |                   334.27 us |
| SSR     |           3,657 |       3,681.83 us |                   296.13 us |

Vertex-start to fragment-end includes scheduling gaps, and pass spans overlap.
`FrameTimings::total_nanos` remains a diagnostic sum, labeled accordingly.
`elapsed_nanos` records the outer measured interval instead; incomplete or
backwards timestamp pairs supply no budget sample. GPU budget reporting uses
this elapsed interval, not the sum. It includes gaps/contention and excludes
untimed leading/trailing work; it does not claim active GPU work or throughput.

CPU profiling also found retirement waits inside reported CPU active time. Only
the blocking calls in `GpuContext::retire_to` now receive idle spans;
command-buffer destruction remains CPU work. Regressions cover overlapping
pairs, invalid samples, budget selection, and retirement exclusions.

## Performance improvement

The forward renderer emits one indirect argument per bucket. Its producer clears
both the instance count and draw-count flag; a surviving instance increments the
former and sets the latter. Empty buckets therefore already have zero instances.
Metal's count emulation was repacking arguments that this producer had already
made safe to draw unconditionally.

`Device::preferred_geometry_path` defaults to the capability-based choice. Metal
prefers `IndirectPerBatch` when its ceiling is `IndirectCount`, avoiding that
packing work. Mesh remains preferred if supported. No capability is removed:
generic counted draws retain their implementation and HAL tests. The renderer
contains no backend-specific selection branch. Diagnostics report its actual
path separately from the capability ceiling.

The baseline for this comparison includes the correctness/binding fixes above
but precedes the geometry preference. The after build also includes the timing
accounting fixes. Identical commands were run in six alternating before/after
pairs per viewport. Pair zero was warmup; all remaining five pairs were
included. All processes exited successfully. No GPU tests or compilation ran
concurrently.

```sh
CRCBL_TRACE=1 MTL_DEBUG_LAYER=0 MTL_SHADER_VALIDATION=0 \
  target/release/sandbox --headless --backend mtl --frames 2400 \
  --size 256x192 --fps 0 --no-debug-overlay
# Repeat at --size 1280x720 and alternate the before/after release executables.
```

| Viewport | Before median | After median | Result                                         |
| -------- | ------------: | -----------: | ---------------------------------------------- |
| 256x192  |      1.3981 s |     1.2909 s | 7.7% less elapsed time; 8.3% higher throughput |
| 1280x720 |      4.8819 s |     4.8766 s | Effectively unchanged                          |

These are elapsed process times for a fixed workload, including startup. The
sample's fixed simulation step is not measured CPU frame time. The improvement
is specific to this workload/device; fragment shading dominates the 720p run.

Binding cleanup alone showed no reliable throughput gain, although a 240-frame
validation log shrank from 12,740,309 bytes to 4,191 bytes. Scratch-page sharing
and smaller packing workgroups were also tried, passed goldens, and removed
because their measurements did not establish an improvement.

## Verification

```sh
cargo nextest run --locked -p crcbl-hal -p crcbl-mtl -p crcbl-render \
  -p crcbl-ui --all-features --no-fail-fast
cargo nextest run --locked -p crcbl --lib --all-features --no-fail-fast
crates/crcbl-mtl/tests/run-mtl-e2e.sh --no-fail-fast --test-threads=1 \
  -E 'not test(a_layer_swapchain_acquires_a_drawable_and_presents_it)'
CRCBL_GPU=mtl MTL_DEBUG_LAYER=1 MTL_SHADER_VALIDATION=1 \
  MTL_DEBUG_LAYER_ERROR_MODE=nslog MTL_DEBUG_LAYER_WARNING_MODE=nslog \
  crates/crcbl/tests/run-hal-seam-e2e.sh --no-fail-fast --test-threads=1
CRCBL_GPU=mtl MTL_DEBUG_LAYER=1 MTL_SHADER_VALIDATION=1 \
  MTL_DEBUG_LAYER_ERROR_MODE=assert MTL_DEBUG_LAYER_WARNING_MODE=assert \
  crates/crcbl/tests/run-render-e2e.sh --no-fail-fast --test-threads=1
```

Results: 1,147 HAL/Metal/render/UI host tests; 375 engine tests; 72 offscreen
Metal hardware tests; 28 HAL seam tests; and 59 strict renderer goldens passed.
The PR branch was rechecked on upstream `bfab434a`; the broader mesh-frame suite
also passed 93 tests with its current CI-style API logging settings (shader
validation unset). That is separate from mesh/task-shader hardware proof.
Formatting, all-target/all-feature clippy, and documentation checks passed with
warnings denied for the modified crates. All 45 committed MSL artifacts compiled
with Metal Toolchain 17F109, with 13 writable-resource vertex warnings and nine
unused-variable warnings in generated code, and no compilation errors.

The renderer CI step now uses the same strict API and shader-validation
settings. The hosted runner remains an independent device check.

API additions: the device preference method has a default implementation.
`FrameTimings` struct-literal callers must supply `elapsed_nanos` or use
`..Default::default()`.

References:
[Apple stage-boundary sampling](https://developer.apple.com/documentation/metal/sampling-gpu-data-into-counter-sample-buffers)
and
[Dawn Metal command encoding](https://raw.githubusercontent.com/google/dawn/main/src/dawn/native/metal/CommandBufferMTL.mm).
The local reproductions and regression tests establish the fixes described here.

## Hosted shader-validation control

PR #17 exposed a hosted-runner difference: on macOS 26's Apple Paravirtual GPU,
enabling shader validation makes valid sampled textures report usage-flag
mismatches and return zero. Both renderer geometry paths fail the same three
representative scenes with validation enabled and pass with it disabled.

A standalone Swift/Metal control reproduces the failure without crcbl: sample
one white R8Unorm texel from a ShaderRead texture into a red RGBA8 output pixel.
The shader reports a usage mismatch and writes black on macOS 26 with validation
([run 34462175408](https://github.com/digitaloten/crcbl/actions/runs/34462175408));
the same runner writes the expected red pixel with validation disabled
([run 34462431533](https://github.com/digitaloten/crcbl/actions/runs/34462431533)).
The identical validated control passes on macOS 15
([run 34462666976](https://github.com/digitaloten/crcbl/actions/runs/34462666976))
and on the local M3 Pro with macOS 26.5.2. This isolates the failure to the
hosted macOS 26 environment rather than establishing a failure on all macOS 26
GPUs.

The Metal renderer CI job is pinned to macOS 15, where all 59 renderer goldens
pass with API assertions and shader validation, error reporting, stderr
reporting and abort-on-fault enabled
([run 34463283160](https://github.com/digitaloten/crcbl/actions/runs/34463283160)).
These extra shader settings matter: default shader fault handling can zero-fill
an invalid read while the command buffer still completes successfully. The
renderer gate now makes such findings fatal. The hardware harness retains its
API logging mode because several probes deliberately end unused encoders.

The full macOS 15 job also exposed a separate native indirect-command-buffer
failure: three hardware probes abort inside Apple's `IOGPUMetalResource`
initialization even with the harness's existing validation settings
([run 34464266030](https://github.com/kryptic-sh/crcbl/actions/runs/34464266030)).
The remaining 69 hardware tests pass and all 45 committed MSL artifacts compile.
Hardware probes therefore retain a separate macOS 26 job, where all 72 offscreen
tests passed; renderer coverage runs on macOS 15. No hardware probes are removed
and neither job disables shader validation.
