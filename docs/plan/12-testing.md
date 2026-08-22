# Topic 12 — Test Infrastructure

Every subsystem ships with unit tests **and** e2e tests. Test infra is built in
P0 alongside the workspace, not retrofitted. The CLI/headless pillar (topic 11)
is the e2e substrate: if it can't be tested without a GUI, it's built wrong.

## Test taxonomy (what each crate owes)

| Level          | Scope                                                                                                                            | Runner                              |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Unit           | Pure logic in-crate: math, pools, rebase, graph compile, TOI solvers, replication encode/decode                                  | `cargo nextest`, per crate          |
| Property       | Invariant-heavy code: `WorldPos` rebase, BVH after random churn, undo inverses, snapshot roundtrip                               | `proptest` or a seeded loop         |
| Integration    | Crate pairs through public APIs: ECS↔net replication, scene→server instantiation, HAL graph on NullBackend                       | `tests/` dirs                       |
| **Sim e2e**    | Full headless server+client, input scripts, N ticks, state-hash assert (`apps/sim`)                                              | nextest, headless — runs everywhere |
| **Render e2e** | Offscreen render → readback → golden-image compare (`crcbl screenshot`)                                                          | needs GPU or software rasterizer    |
| **Editor e2e** | Command sequences against headless editor server: edit → save → reload → verify; random-command + full-undo = initial-state-hash | nextest, headless                   |
| Sample e2e     | Each sample's determinism script + golden frames — samples are test fixtures, not just demos                                     | CI per sample                       |

**Property tests are not all `proptest`, and that is fine.** `proptest` is a
dependency of `crcbl-core` alone, where it guards `WorldPos` rebase and pool
handle invalidation. Everywhere else the property suites are hand-written loops
over a seeded generator — `crates/crcbl-phys/tests/broadphase_churn.rs` and
`crates/crcbl-phys/tests/dynamics.rs` are the two the anchor list below points
at. What a property test owes is a generator, a shrink story and a seed you can
replay, not a particular crate; and where the input is a long _sequence_ of
operations checked against a brute-force oracle, an explicit loop states the
sequence more directly than a strategy combinator would. Reach for `proptest`
when the interesting part is generating one value; write the loop when the
interesting part is the order of many.

## Infrastructure (P0, then grows)

- **Runner**: `cargo nextest` workspace-wide. E2E suites are gated **per
  crate**, never workspace-wide: `vk-e2e`, `wgpu-e2e`, `mtl-e2e`, `wayland-e2e`,
  `x11-e2e`, `win32-e2e`, `cli-e2e` and `render-e2e` are each declared by the
  crate that owns the hardware or window system its suite needs. One workspace
  `e2e` feature could only ever mean "all of it", which on every real machine is
  a superset of what is actually present; a per-crate name says which loader,
  compositor or GPU a run is claiming, so a CI job can select exactly the suites
  its runner can honour and a developer can turn on the one backend they have.
  `crcbl-dx12` has no feature at all, and that is argued rather than overlooked
  — `crates/crcbl-dx12/tests/run-dx12-e2e.sh` makes the case: D3D12 ships in
  Windows and WARP ships with it, so unlike Metal (no software rasteriser
  exists) and Vulkan (no loader on a bare machine) there is no Windows machine
  where that suite _cannot_ run. Hiding it behind a feature would take working
  coverage away from the ordinary Windows job in exchange for nothing.
- **Nothing may skip silently — and `--all-features` is not what prevents it.**
  Every gated suite is `#[ignore]`d _on top of_ its feature;
  `crates/crcbl-shell/tests/wayland_e2e.rs`'s header says why. So
  `cargo nextest run --workspace --all-features` is precisely the run that does
  not execute them, deliberately: that run has to stay green on a machine with
  no compositor and no GPU, which is every CI runner except the one job that
  provides one. Turning the features on and calling it coverage would only move
  the trap, since a feature-gated test on a machine that cannot host it either
  fails or is skipped anyway. The counter-measure is each suite's own harness
  (`run-<suite>-e2e.sh` / `.ps1`): it turns the ignored set on, then parses
  nextest's own summary line out of a colour-stripped copy of the log — CI sets
  `CARGO_TERM_COLOR: always`, so the counts arrive wrapped in escapes — and
  fails when the count is zero, which is the gate having stopped gating because
  a feature or an `#[ignore]` no longer matches the tests. Reading nextest's
  reported total rather than counting lines of its output is deliberate: a line
  count silently picks up headers and lands a number close enough to look right.
- **A harness over a crate whose tests are mixed selects `--run-ignored only`,
  not `all`.** Where the suite is its own target — `vk_e2e`, `wgpu_e2e`,
  `wayland_e2e` — the two flags pick the same tests, because everything in that
  binary needs the hardware. `crcbl-mtl` and `crcbl-dx12` keep their tests in
  `src/` alongside pure ones (the next section says why they must), so
  `--run-ignored all` there had the harness run the whole crate and made the
  guarded count "unit tests plus device tests". A run in which **every device
  test had vanished** would still report a healthy total and clear the zero
  check — the same check-that-cannot-fail shape the guard exists to catch, one
  level up. `--run-ignored only` selects exactly the tests that need the device,
  which is what makes the number mean something. It also requires the placement
  rule below to actually hold: the flag is only as good as the `#[ignore]`s.
- **The cut-short run is the same trap wearing a healthy number**, and one
  sourced guard is what catches it. nextest prints `<n> tests run:` for a
  complete run and `<ran>/<total> tests run:` for one it cancelled, so a guard
  matching the digits immediately before the words reads `2/15 tests run` as a
  healthy fifteen — thirteen tests that never executed, reported as a pass. Most
  of the bash harnesses did exactly that, and the rest rejected it while
  reporting "zero tests run" about a run that had run some, so one sourced
  helper — `tools/nextest-summary.sh` — now owns the whole of it: strip the
  colour, find the summary, name the cancelled shape, fail on zero. Every bash
  harness **that drives nextest** sources it, and
  `tools/nextest-summary-test.sh` feeds it each shape — complete, cancelled,
  zero, absent, colour-wrapped, repeated — and asserts what it does with them,
  which is the thing an inline copy per harness could not have. The PowerShell
  harnesses keep their own copies of the same logic, which `docs/backlog.md`
  records as the remaining place it can drift.
- **The harness that drives no nextest guards on its own count instead, and that
  is the right shape rather than a gap.** `web/run-cross-backend-e2e.sh` never
  invokes nextest: it holds the browser harness's readbacks against a frame a
  native backend rendered in the same run, so there is no summary line to parse.
  What it guards is the number the run is _supposed_ to produce — it refuses an
  empty scene list outright and fails when a scene produced no comparison. (This
  paragraph described the _deleted_ per-scene-per-size script under the
  surviving script's name until 2026-08-23: `6b5e17a` rewrote the paths here and
  left the prose attached to a different program.) The property is the same one
  `nextest-summary.sh` protects: a run that silently did less work than it
  claimed cannot pass.
- **Software GPU in CI**: render e2e runs on **lavapipe** (Vulkan) and wgpu's
  GL/software fallbacks — every commit exercises real render paths without
  hardware runners. The mac and Windows arms did not wait for a scheduled
  hardware job in the end: `mtl e2e` on `macos-latest` and `dx12 e2e` on
  `windows-latest` run per commit like everything else, the first against the
  runner's real GPU and the second pinned to WARP.
- **Golden images**: `crcbl screenshot` output vs checked-in references; compare
  with per-pixel tolerance + SSIM-style metric (rasterizers differ slightly);
  regenerate via `--bless` flag; diffs uploaded as CI artifacts on failure.
- **Determinism harness**: same input script → same state hash, asserted across
  runs and (same-binary) across CI jobs; any nondeterminism source (time, RNG,
  iteration order) must be injected/seeded — enforced from P2, in every sim e2e
  by default.
- **Frame-poll discipline** for anything async (swapchain warm-up, asset loads):
  poll for the condition with deadline, never fixed sleeps (slow-CI flake
  lesson, learned elsewhere the hard way).
- **Coverage**: `cargo llvm-cov` in CI with per-crate thresholds — gate on
  meaningful floors (core/phys/net high; backend crates measured but gated
  looser since e2e covers them), trend tracked, not vanity-chased.

## Naming and placement

None of this was written down, which is why it drifted. All of it is what the
workspace already does; the rules below were read off the tree, not invented for
it.

**A test's name is a prose sentence in `snake_case`, stating the claim.** Not
`test_foo`, not the function under test — the thing that is true if the test
passes.
`a_pipeline_without_depth_state_binds_the_devices_default_rather_than_nil`
(`crcbl-mtl`), `a_metal_indirect_draws_stride_is_only_checked_when_it_is_used`
(`crcbl-mtl`), `the_engine_passes_offer_every_shader_artifact_they_have`
(`crcbl-render`). Names that long are not decoration: a failing e2e run on a
runner you cannot attach a debugger to gives you the name and a diff, and a name
that is a sentence has already told you which half of the claim broke. The
backend crates carry the longest names, and `crcbl-dx12`, `crcbl`,
`crcbl-golden`, `crcbl-mtl` and `crcbl-shell` are the longest of them. The
crates that have drifted below the convention are recorded here as known-drifted
rather than left to be inferred: `crcbl-ecs` (whose names run to a handful of
words), `crcbl-net`, `crcbl-input`, `crcbl-phys`, `crcbl-audio` and
`crcbl-store`. They are not wrong so much as terse, and nothing enforces the
difference; renaming them is a task nobody has taken.

**A test that exists on more than one backend names the backend or its API.**
`no_two_formats_share_a_metal_format` in `crcbl-mtl/src/conv.rs` against
`no_two_formats_share_a_dxgi_format` in `crcbl-dx12/src/conv.rs`;
`reported_limits_come_from_metal_and_agree_with_the_features` against
`reported_limits_come_from_d3d12_and_agree_with_the_features`;
`a_device_reports_metal_and_one_graphics_queue` against
`a_device_reports_dx12_and_one_graphics_queue`. The convention exists where the
claims genuinely differ per backend, and it is what makes a runner log legible.
It was applied late: names across `crcbl-vk`, `crcbl-mtl`, `crcbl-dx12` and
`crcbl-wgpu` were once _verbatim identical_ in two or three of those crates at
the same time, and the crate prefix in nextest's output was the only thing
telling them apart — a prefix a grep, a bug report or a CI annotation usually
does not carry. They now differ by the backend word alone, which is the property
worth keeping: a search for one finds the other, and neither can be read as the
wrong backend's result.

The one name left deliberately bare is
`a_device_outlives_the_instance_that_made_it` in
`crates/crcbl-hal/tests/seam_from_outside.rs`, which asserts the obligation
against the null backend. Once the three backend copies took prefixes, the
unprefixed name belongs to the test that genuinely is about no backend.

**A test that is deliberately backend-agnostic takes no prefix, and says so.**
`crates/crcbl/tests/render_e2e.rs` is the exemplar: every test in it opens
whatever `crcbl::backend::open` selects, so `CRCBL_GPU` decides which backend
draws and the file is the same suite on all of them. Its header argues that
explicitly rather than leaving it to look like an oversight — the point of one
shared suite is that a golden blessed on one backend keeps being re-derived on
another, which a per-backend copy would not do. Absence of a prefix has to be
readable as a decision, so write the sentence that makes it one.

**Placement follows what the test needs, not which directory looks tidier.** The
rule the workspace actually holds to: _a test lives in `src/` if it can pass on
a machine with no GPU and no loader; a test that needs a live device is
`#[ignore]`d._ Measured rather than asserted — every one of `crcbl-vk`'s `src/`
tests passes with `VK_DRIVER_FILES` pointed at a manifest that does not exist,
and none of them carries `#[ignore]`. The converse is just as deliberate: every
test in `crates/crcbl-vk/tests/vk_e2e/` is `#[ignore]`d, and across the agnostic
suites exactly one is not —
`the_rotation_frame_of_reference_agrees_with_the_shaders` in
`crates/crcbl/tests/sprite_e2e/sprite/rotation.rs`. It is pure and could live in
`src/`, and it stays in the e2e binary on purpose: it pins the frame of
reference its neighbours' pixel assertions are written in, so a sign error there
would relabel every expected colour consistently and the whole sweep would pass
while asserting the mirror image. It belongs next to what it protects.

Location is a poor rule to reach for on the Metal and D3D12 backends in
particular, and a future reader should not "fix" them by moving files.
`crcbl-dx12/src/lib.rs` re-exports exactly one item — `Dx12Instance`.
`Dx12Device` is a `pub struct` inside the private `device` module and is never
re-exported at all. `crcbl-mtl/src/lib.rs` re-exports two, `MetalDevice` and
`MetalInstance`. Every one of `crcbl-dx12`'s tests and every one of
`crcbl-mtl`'s therefore lives in `src/`, written against `pub(crate)` surface —
a large private surface in `crcbl-dx12` alone. Neither crate has a Rust target
under `tests/`; those directories hold harness scripts only. Moving those suites
out would mean widening two backends' public APIs for no reason but to host
tests, which is the opposite of what `seam_from_outside.rs` exists to check.

**So those two crates keep the `#[ignore]` half of the rule instead of the
directory half, and now they actually do.** Both once kept every test unmarked —
`crcbl-mtl` marked only the handful that make the GPU execute a shader, and
`crcbl-dx12` carried no `#[ignore]` anywhere — which is what let their harnesses
run the whole crate and guard on a count that mixed pure tests with device ones.
Every test whose body causes a real device, instance or adapter to be created is
now `#[ignore]`d with a reason naming its harness, in the form `crcbl-vk` and
`crcbl-wgpu` use; every test that passes with no GPU is not. The split was read
off the bodies, tracing `instance::tests::open`, `device::tests::open_device`
and `instance::tests::pinned_adapter` through each module's local helpers, so in
both crates the split falls out of what a test's body does rather than out of
where the file sits. Calls that look like hardware and are not stayed on the
pure side, each because it can pass on a machine with no GPU:
`crcbl-mtl/src/fault.rs`'s two tests, which build a synthetic `NSError` and
assert how the encoder states are worded; and
`neither_caps_list_is_ever_empty_and_both_always_offer_fifo` in
`crcbl-mtl/src/swapchain.rs`, which makes a detached `CAMetalLayer` — Core
Animation vends one without a window server, a display or an `MTLDevice`.
`crcbl-dx12/src/descriptor.rs` went the other way for the same reason: its
address arithmetic is asserted against descriptor heaps `CreateDescriptorHeap`
really allocated, so it needs the device even though the claim is arithmetic.

The consequence to know when reading a CI log: the
`cargo nextest run --workspace --all-features` sweep on `macos-latest` and
`windows-latest` now runs each crate's pure tests only. The device tests run in
the `mtl e2e` and `dx12 e2e` jobs, and — for D3D12 on an unpinned adapter — in
`test-cross-platform`'s "DX12 adapter report" step, which passes
`--run-ignored all` for exactly that reason: the adapter line it publishes is
printed by a device test, and without the flag the step would run the pure
tests, print nothing, and still pass its `--no-tests fail` check.

**Filenames name the subject, never the taxonomy tier.**

- `<platform>_e2e.rs` for a hardware or window-system suite: `wayland_e2e.rs`,
  `x11_e2e.rs`, `win32_e2e.rs`, `wgpu_e2e.rs`, `render_e2e.rs`, `cli_e2e.rs`,
  and `vk_e2e/` where the suite is large enough to want modules.
- `seam_from_outside.rs` for a test that exercises a crate's public surface from
  an integration binary. `crcbl-hal` and `crcbl-shell` both use it, and both
  headers give the same reason: an in-crate test can reach private items, so it
  cannot prove anything about what the crate exposes.
- `run-<suite>-e2e.sh` / `.ps1` for a harness. A helper that is **sourced rather
  than run** drops the `run-` prefix, because it exports into the caller's shell
  and exits it on failure — `crates/crcbl-vk/tests/vulkan-icd.sh` and
  `crates/crcbl-shell/tests/sway-session.sh`, both of which say so in their
  first lines.

A file named for a tier — `integration.rs` — tells a reader what the build
system already knows and nothing about what is inside. Anything under `tests/`
is an integration target by definition, so the name is spent on a fact you get
for free, and the file becomes the place unrelated tests accumulate because
nothing in its name says they do not belong. `crcbl-server`'s integration target
was the one file carrying it, and it is now
`crates/crcbl-server/tests/client_server_session.rs` — named for the handshake,
resume and replication it actually asserts. No file in the workspace carries a
tier name today, which is the state to keep rather than a rule with nothing left
to point at.

**Every file in a `tests/` directory carries a `//!` header** saying what it
covers _and why it is a separate target_ — the second half is the one that gets
skipped, and it is the one a reader needs.
`crates/crcbl-shell/tests/appkit_session.rs` explains that `libtest` always runs
a body on a spawned thread and AppKit is main-thread-only, which is why it is a
`harness = false` target; `crates/crcbl/tests/render_e2e.rs` explains why the
test is in `crcbl` and not in `crcbl-mtl`. Both are facts no reader could
reconstruct from the code. The rule now holds with no exceptions: every module
under `crates/crcbl-vk/tests/vk_e2e/` carries one, and so does
`crates/crcbl-net/fuzz/tests/corpus.rs`, which were the two gaps this section
used to record. A submodule's header says what _that_ module owns rather than
repeating the suite preamble its root already carries — `vk_e2e/main.rs` states
the feature gate, the `#[ignore]` convention, the offscreen path and the
validation-report assertion once, for all of them.

## Per-subsystem e2e anchors (the non-negotiables)

- `crcbl-core`: property tests on `WorldPos` rebase round-trips and pool handle
  invalidation.
- `crcbl-hal`/`crcbl-vk`/`crcbl-webgpu`: graph-compile unit suite on
  NullBackend; triangle/mesh golden images on lavapipe; identical-scene
  cross-backend image compare, the browser against a live native render — the
  regression net that catches a divergence no single backend's golden can.
- `crcbl-ecs`+`net`: replication roundtrip (server tick → snapshot → client
  state == server state); churn soak with leak assert.
- `crcbl-phys`: analytic cases with known answers (orbit period, terminal
  velocity within tolerance — see orbit sample exit criteria), bullet-
  through-paper CCD suite, BVH property tests, determinism hash 1000-tick
  replays.
- `crcbl-scene`: glTF corpus (Khronos samples subset vendored) load-and- count
  asserts; scene save→load→hash roundtrip.
- `crcbl-ui`: draw-list snapshot tests (widget tree → draw-command list compare
  — no GPU needed); hit-test unit grid.
- Editor: random-command/undo property test (stage 8 exit criterion) runs
  headless via the command protocol.
- Samples: every sample CI-runs its input-script determinism check + at least
  one golden frame.

## Delivery

| Slice                                                     | Roadmap phase       |
| --------------------------------------------------------- | ------------------- |
| nextest + CI skeleton, coverage wiring, NullBackend suite | P0                  |
| lavapipe render e2e + golden-image tooling (`--bless`)    | P1                  |
| Determinism harness + sim e2e pattern                     | P2                  |
| Phys analytic/property/CCD suites                         | P3, grows P6/P8/P11 |
| Cross-backend image compare (vk↔wgpu)                     | P5                  |
| glTF corpus + scene roundtrip                             | P9                  |
| UI draw-list snapshots                                    | P4, P10             |
| Editor command/undo property suite                        | P12                 |

## Exit criteria (MVP)

- CI runs unit+property+integration+sim e2e on every push; render e2e on
  lavapipe. `--all-features` is the workspace run, and every hardware suite is
  additionally driven by its own harness, each of which fails on a zero or
  cut-short count — no silently-skipped e2e.
- Every crate has both unit and e2e coverage per the anchor list; coverage
  floors enforced.
- A rendering change that shifts output must touch a golden image (blessed
  intentionally) — unreviewed visual drift is impossible.
- All e2e drivable locally with one command per suite — `run-<suite>-e2e.sh` or
  `.ps1` for anything needing a device or a window system, plain
  `cargo nextest run --workspace --all-features` for everything else.

## Corrections (2026-08-09)

- **"wgpu's GL/software fallbacks" do not run in CI.** The infrastructure
  section says render e2e runs on lavapipe _and_ those fallbacks; nothing
  exercises a GL device anywhere. **The action this asked for is moot**: GL was
  reachable only through `crcbl-wgpu`, which was deleted 2026-08-21, so there is
  no suite left to point at a GL device and no GL path in the tree at all.
  `docs/backlog.md` carries the OpenGL decline and its reasoning. The claim is
  what has to go, not the coverage.
- **Shader artifacts are validated one target in four.** `spirv-val` runs on the
  SPIR-V; the WGSL, MSL and DXIL emitted from the same source are checked by
  nothing, which is how a shader Dawn rejects passed every gate and shipped a
  black canvas. The rules that close it are in
  [02-vulkan-backend.md](02-vulkan-backend.md)'s shader-portability section;
  they belong to this topic's anchor list too.

  **Closed, 2026-08-15 — all four are validated now.** `spirv-val` still runs on
  the SPIR-V from `crates/crcbl-shaders/tools/compile-shaders.sh`;
  `crates/crcbl-shaders/tests/wgsl_validation.rs` runs naga over every committed
  `wgsl/*.wgsl`, and its header records the `var<uniform>` with no binding
  decoration that shipped for months because nothing looked; the DXIL comes from
  a pinned `dxc` whose version is checked and whose container is then asserted
  to be **signed**, since an unsigned one commits happily and is refused by
  every real driver; and the MSL is compiled with `xcrun metal -c` in `ci.yml`'s
  `mtl e2e` job, which is the only place on the matrix that can — and which
  counts what it compiled, so a glob matching nothing fails rather than exits 0.

- **The cross-backend image compare is the only detector for a whole bug class**
  — a shader whose _semantics_ differ per target, which no lint can find. It
  currently covers two backends and one scene. Extending it to every engine
  shader and every backend is a testing deliverable, not a rendering one.

  **Closed wider than this asked, and re-read 2026-08-23.**
  `web/run-cross-backend-e2e.sh` now holds every scene the browser draws against
  a live native render — `--reference vk`, and `--reference mtl` on macOS — so
  the pair is two genuinely separate implementations rather than two
  abstractions over one Vulkan driver. This entry claimed it still drove `vk`
  against `wgpu` over three scenes at several sizes; that was the deleted
  script, and Metal is no longer outside the compare.

## Superseded (2026-08-10)

The corrections above are open gaps: the doc claims coverage the tree does not
have, and closing them means changing the tree. These three are the opposite
shape — the tree deliberately chose a better mechanism and the doc kept
describing the old one. They are rewritten in place above rather than footnoted,
because a rule left standing with its retraction in a later section still reads
as a rule, and someone would eventually "restore" the code to it. What they used
to say, and why the replacement won:

- **"e2e suites feature-gated (`--features e2e`)."** There is no workspace-wide
  `e2e` feature and there should not be one. Per-crate features name the
  specific loader, compositor or GPU each suite needs, plus `crcbl-dx12` with
  none at all on the deliberate grounds that WARP ships with Windows. A single
  flag meaning "all of it" could never be true of a real machine.
- **"CI always runs `--all-features` (a plain run silently skipping e2e is a
  known trap)."** The trap is real and the counter-measure named here was
  backwards. Double-gating — feature _and_ `#[ignore]` — makes `--all-features`
  the run that skips them, which is what keeps the workspace run green on a
  machine with no hardware. Each harness parsing nextest's own count and failing
  on zero is the check that actually cannot pass while testing nothing, and the
  harnesses that also reject a cancelled run are the only ones fully closing it.
- **"Property → `proptest`, in-crate."** `proptest` is a dependency of
  `crcbl-core` and of no other crate. The phys property suites this document's
  own anchor list points at are seeded loops in `tests/`, checked against a
  brute-force oracle — the right shape when the generated thing is a long
  sequence of operations rather than one value.
