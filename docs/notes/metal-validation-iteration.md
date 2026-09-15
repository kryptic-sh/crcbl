# Metal validation iteration — 2026-09-10

This iteration makes the mesh and forward suites fail on Metal API warnings and
shader faults. It preserves the shared image goldens and the renderer's existing
geometry preference. The preceding performance and hosted-driver investigation
is recorded in [the baseline report](metal-local-baseline.md).

## Reproduced failures and fixes

Readback marker nodes declared compute passes but only retained an image handle
for a later copy. Strict validation rejected their empty compute encoders. The
mesh, skinned-motion and reflectivity fixtures now declare copy passes; the
resource states and actual readbacks are unchanged.

The reflectivity fixture exposed five geometry buffers only to the vertex stage,
although its generated fragment signature includes them. Its layout now grants
both vertex and fragment visibility, matching the production mesh layout.

Deferred raster pipelines exposed an argument-ordering problem: eagerly replayed
bind groups could write a native slot that another group or inline constant
replaced before the draw. Arguments now materialize at the consuming draw, after
the pipeline. Replay selects the final writer for each stage, resource table and
physical slot before touching the binding cache or Metal. Losing logical
bindings remain retained for later pipeline or argument changes. A regression
covers a group followed by two successive inline-constant draws on the same
native slot.

The shadow-atlas debug view replaces every display pixel. Previously, tonemap
and optional ground-grid passes first wrote that same image, producing a
redundant Store-to-DontCare transition. Atlas frames now omit those two passes.
HDR scene generation, the returned scene image, resolve and render-scale upscale
retain their contracts. Existing atlas pixel and border tests exercise the
change.

## What a green validation run means

Metal shader validation can recover from an invalid read by substituting zero;
command-buffer completion alone does not prove a shader ran without findings.
The strict renderer, mesh and forward gates explicitly set:

```sh
export CRCBL_GPU=mtl
export MTL_DEBUG_LAYER=1
export MTL_DEBUG_LAYER_ERROR_MODE=assert
export MTL_DEBUG_LAYER_WARNING_MODE=assert
export MTL_SHADER_VALIDATION=1
export MTL_SHADER_VALIDATION_ENABLE_ERROR_REPORTING=1
export MTL_SHADER_VALIDATION_REPORT_TO_STDERR=1
export MTL_SHADER_VALIDATION_ABORT_ON_FAULT=1
```

The hardware harness also defaults all three shader reporting/termination
settings to `1` and prints their values. It retains API `nslog` defaults because
several device probes intentionally end empty encoders or test load-only passes.
Those diagnostics are separate from the strict renderer gates. The device report
checks validation-layer interposition and failed submissions; the runner
enforces fatal shader findings.

## Local verification

Apple M3 Pro, macOS 26.5.2, Rust 1.97.0 and nextest 0.9.140. GPU runs were
offscreen; the live drawable test was excluded.

| Check                                                   | Result                                |
| ------------------------------------------------------- | ------------------------------------- |
| Renderer goldens, strict API and shader validation      | 59 passed                             |
| Mesh suite, strict API and shader validation            | 93 passed                             |
| Forward suite, strict API and shader validation         | 37 passed                             |
| Hardware suite, API logging and fatal shader validation | 72 passed                             |
| Metal host tests                                        | 115 passed, 62 hardware tests ignored |
| Renderer host tests                                     | 595 passed                            |
| Metal and renderer clippy, warnings denied              | Passed                                |

With the strict environment above, run the GPU suites using:

```sh
crates/crcbl/tests/run-render-e2e.sh --test-threads 1
crates/crcbl/tests/run-mesh-e2e.sh --test-threads 1
crates/crcbl/tests/run-forward-e2e.sh --test-threads 1
```

For the hardware harness, use its API logging defaults or explicitly set both
API violation modes to `nslog`, preserving fatal shader validation:

```sh
MTL_DEBUG_LAYER_ERROR_MODE=nslog MTL_DEBUG_LAYER_WARNING_MODE=nslog \
  crates/crcbl-mtl/tests/run-mtl-e2e.sh --no-fail-fast \
  -E 'not test(a_layer_swapchain_acquires_a_drawable_and_presents_it)'
```

These results establish correctness on the tested hardware and settings. They do
not measure a throughput improvement or native window/presentation behavior.

## Hosted verification

The updated hardware harness also passes 72/72 tests on the macOS 26 Apple
Paravirtual device, with API logging and all fatal shader reporting defaults
([job 102831154895](https://github.com/digitaloten/crcbl/actions/runs/34464924574/job/102831154895)).
The captured test summary and harness report confirm the test count, validation
interposition and zero failed submissions; this result is not inferred solely
from the diagnostic workflow's status.

The strict mesh suite passes 93/93 on macOS 15 in the same diagnostic run. Its
captured terminal summary was preserved even though the combined mesh/forward
job later hit its 20-minute limit during the forward suite. A partial forward
run is not counted as a pass.

A forward-only retry then passes 37/37 with API assertions and fatal shader
validation on macOS 15
([run 34466958609](https://github.com/digitaloten/crcbl/actions/runs/34466958609)).
All hosted iteration runs used the exact crate source tree at `f0cc6fa0`; later
changes at this validation checkpoint only update CI configuration and reports.
