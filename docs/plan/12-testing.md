# Topic 12 — Test Infrastructure

Every subsystem ships with unit tests **and** e2e tests. Test infra is built in
P0 alongside the workspace, not retrofitted. The CLI/headless pillar (topic 11)
is the e2e substrate: if it can't be tested without a GUI, it's built wrong.

## Test taxonomy (what each crate owes)

| Level          | Scope                                                                                                                            | Runner                              |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Unit           | Pure logic in-crate: math, pools, rebase, graph compile, TOI solvers, replication encode/decode                                  | `cargo nextest`, per crate          |
| Property       | Invariant-heavy code: `WorldPos` rebase, BVH after random churn, undo inverses, snapshot roundtrip                               | `proptest`, in-crate                |
| Integration    | Crate pairs through public APIs: ECS↔net replication, scene→server instantiation, HAL graph on NullBackend                       | `tests/` dirs                       |
| **Sim e2e**    | Full headless server+client, input scripts, N ticks, state-hash assert (`crcbl sim --hash`)                                      | nextest, headless — runs everywhere |
| **Render e2e** | Offscreen render → readback → golden-image compare (`crcbl screenshot`)                                                          | needs GPU or software rasterizer    |
| **Editor e2e** | Command sequences against headless editor server: edit → save → reload → verify; random-command + full-undo = initial-state-hash | nextest, headless                   |
| Sample e2e     | Each sample's determinism script + golden frames — samples are test fixtures, not just demos                                     | CI per sample                       |

## Infrastructure (P0, then grows)

- **Runner**: `cargo nextest` workspace-wide; e2e suites feature-gated
  (`--features e2e`) so plain `nextest run` stays fast — but CI always runs
  `--all-features` (a plain run silently skipping e2e is a known trap).
- **Software GPU in CI**: render e2e runs on **lavapipe** (Vulkan) and wgpu's
  GL/software fallbacks — every commit exercises real render paths without
  hardware runners. Hardware jobs (real GPU, later mac/win) are scheduled, not
  per-commit.
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

## Per-subsystem e2e anchors (the non-negotiables)

- `crcbl-core`: property tests on `WorldPos` rebase round-trips and pool handle
  invalidation.
- `crcbl-hal`/`crcbl-vk`/`crcbl-wgpu`: graph-compile unit suite on NullBackend;
  triangle/mesh golden images on lavapipe + wgpu; identical- scene cross-backend
  image compare (vk vs wgpu within tolerance) — the tier system's regression
  net.
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
  lavapipe; full suite `--all-features` (no silently-skipped e2e).
- Every crate has both unit and e2e coverage per the anchor list; coverage
  floors enforced.
- A rendering change that shifts output must touch a golden image (blessed
  intentionally) — unreviewed visual drift is impossible.
- All e2e drivable locally with one command per suite
  (`cargo nextest run --all-features`, `crcbl`-based scripts for the rest).
