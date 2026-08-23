# Roadmap — Interleaved Build Order (canonical)

This is the **canonical build order** for the engine. The numbered stage docs
(`01`–`10`) are topical deep-dives — architecture, tasks, risks per subsystem;
this roadmap interleaves _slices_ of those stages with the samples that consume
them.

Two hard rules govern the ordering:

1. **Engine base before any sample.** Phases 0–4 build the complete core (window
   → render → sim → physics slice → UI slice) before the first line of sample
   code. No sample ever starts before every module it needs exists.
2. **Component → system → sample.** After the base, work proceeds in pulls:
   build the component(s), build the system(s) on them, then ship the sample
   that proves them. The sample is the phase's exit criterion.

**Wasm is early** (phase 5, right after the first playable): every sample from
breakout onward ships as a browser demo on GitHub Pages. The demo site is the
engine's public face and its continuous cross-backend regression test.

## Status (as of 2026-08-21)

**`crcbl-mtl` and `crcbl-dx12` are deferred.** Work on both stopped by the
owner's decision; `docs/plan/09-backends-metal-dx12.md` carries the terms and
`docs/backlog.md` the two `DEFERRED` entries. Neither crate is deleted and
neither CI job is removed, so a regression on either still shows — what stops is
new implementation: closing capability rows, chasing the WARP device removal,
and writing Metal features no runner here can execute. Everything the section
below reports about either backend is the state at the point work stopped, not
work in progress. **`parity_blockers()` therefore cannot reach empty**, and
`every_parity_blocker_is_on_a_deferred_backend` in `crcbl-hal/src/capability.rs`
is what says so rather than letting a non-zero count read as work somebody is
about to pick up.

Read that test for what it actually asserts, because it is stronger than "the
count is not zero": **every remaining blocker is on a deferred backend, so
neither `crcbl-vk` nor `crcbl-webgpu` has one.** The two backends carrying a
shipping platform are at parity as the model defines it, and the residue is
parked work on the two that are not. The test also carries a vacuity guard — it
fails if the blocker set empties, because at that point its first assertion
would be true and meaningless and this note would be the thing to delete.
`the_parity_blockers_are_exactly_the_reviewed_list` is the other half: the set
cannot change without somebody editing `REVIEWED_BLOCKERS` by hand, which is
what stops a row leaving because a kind was widened rather than because the work
landed.

Effort goes to `crcbl-vk` and `crcbl-webgpu`, the two backends carrying a
shipping platform each — Linux and Windows through Vulkan, the browser through
WebGPU.

## Status (as of 2026-08-20)

**Since 2026-08-15 the work has been backend parity and the viewer sample**, and
neither has a phase row of its own: parity cuts across P2, P10 and P14, and the
viewer is `sample/05-viewer.md`'s own milestones.

- **Parity stopped being a claim and became a mechanism.** `crcbl-hal` carries
  an exhaustive `Capability` enum that every backend answers for through a
  `match`, so a capability added to one backend fails to compile on the others.
  `crates/crcbl/tests/hal_seam_e2e.rs` drives that enum in **both** directions —
  a declared capability must work, a declared refusal must refuse with the
  documented error — and prints which capabilities the device withheld, so a row
  that ran nowhere cannot read as a row that passed. Real divergence goes on
  `REVIEWED_BLOCKERS`, and a snapshot test refuses to let that list drift
  without a human editing it.

- **Rows closed, mostly on D3D12 and Metal.** D3D12 gained mesh pipelines and
  both mesh draws, root constants, query heaps, timelines, image-to-image and
  depth-plane copies, an MSAA resolve, a zero fill, and a device-removal report
  that carries its DRED breadcrumbs. Metal gained push constants, occlusion
  query sets, a bindless descriptor array as an argument buffer, and a GPU-side
  draw count packed by a compute kernel rather than through an indirect command
  buffer — the ICB route hung the GPU three times and was abandoned, and that is
  written up in `docs/backlog.md`. WebGPU gained streamed query sets and the
  stencil reference.

- **Four breaking changes at the seam, each because a backend could not answer
  truthfully.** Timestamp results are nanoseconds rather than ticks plus a
  period; `MultisampleState::mask` is gone; `StencilState::reference` is gone
  and `set_stencil_reference` is the only channel; a pass takes its timestamps
  in its descriptor. Each is in `CHANGELOG.md` with the backend that forced it.

- **Two browser defects fixed**: frames came back dark because the canvas was
  not encoded as sRGB, and cancelling a readback was reported as a device error
  rather than as the ordinary thing it is.

- **`apps/viewer` — milestones 1 to 3 of `sample/05-viewer.md`.** It loads a
  `.glb` through the asset seam, frames and orbits it, draws it over a
  screen-space infinite grid, and lists what the document holds and what the
  conversion skipped (`I`). Wireframe (`W`), world-space normals (`N`) and a
  runtime exposure on `-`/`=` and on a menu slider are the debug views.
  **Re-export the file and the frame becomes the new document**, which is V-F4's
  artist loop. `crcbl-render` gained `OrbitCamera` and the grid out of it, and
  `crcbl-ui` gained menu sliders.

  This is _not_ P9's hot reload: `crate::watch` polls one path from the sample's
  own tick, and `crcbl-assets` still has no reload of its own. The Fetch source
  and the RON scene format are also still P9's remainder.

- **NaN robustness, found by the viewer opening files nobody curated.** A `NaN`
  vertex no longer shrinks a bounding box, displaces a cluster sphere, or
  shrinks a physics BVH node — `glam`'s `Vec3::min`/`max` are bare comparisons
  that yield the right-hand side on a `NaN`, which is not `f32::min`.

- **A newer WARP does not fix D3D12 mesh shading** (2026-08-20). The
  redistributable `Microsoft.Direct3D.WARP` 1.0.20 was researched for two
  release notes that land on the symptom and had never been run. It loaded —
  both dx12 steps log `driver="D3D12 UMD 1.0.20.0"` against the OS WARP's
  `10.0.26100.33158` — and the device was removed exactly as before, from `Map`,
  with no debug-layer error and no DRED breadcrumb. The crate's own D3D12 suite
  passed with both mesh flags reported, so it is `render_e2e`'s frame alone, and
  a symptom that survives two WARP versions is far more likely ours than
  Microsoft's. That narrows the open decision rather than settling it.

- **Three test arms that were asserted only in the source now run**
  (2026-08-20). A test branching on what the device reports covers both arms in
  its text and one arm in any run: the bindless refusal, the `update_bind_group`
  refusal and the timestamp-set refusal each keyed on a feature every adapter
  here reports, so each ran nowhere while a doc, a test name and a backlog entry
  all said both were covered. Reaching them needed no hardware — opening a
  device _without_ the feature is the same move `mesh.rs` already used for
  `GeometryPath::IndirectPerBatch` — and each now carries a guard that the
  subtraction happened, because without one the test asserts the capable
  device's answer under the lesser arm's name.

- **quarry's gate has its pixels** (2026-08-20). Six goldens, one per
  `GeometryPath` at each end of the fixed dolly, blessed on an RX 7900 XTX and
  checked against lavapipe locally — max channel delta 1, nothing over
  `Tolerance::RASTERISER`. And a LOD view that tints each cluster by the DAG
  level it was decimated to, which is what makes "one mesh spans several levels
  across its own surface" a thing to look at rather than only a thing to assert:
  the mesh path draws a mosaic, the two indirect paths draw one flat grey
  because they select per instance. What the sample still owes is the window,
  the screen-error heatmap, and the skinned case — the last behind skeletal
  animation, which is also what `puppet` waits on.

### As of 2026-08-15

**Since S3, the work has been the render side and two fixtures rather than a
fifth game**, and none of it has a phase row of its own because it was pulled
forward out of P7, P9 and P14. What landed:

- **`crcbl-jobs`** (P5B's crate) — a work-stealing `Pool`, `Threads`/`Inline`
  spawners, mailboxes, a ring and `par_for`. `apps/horde` steers its crowd on it
  and takes `--workers`. The Web Worker backend is the part still missing.
- **`crcbl-scene`** — glTF import, QEM edge-collapse simplification, meshlet
  build and a cluster DAG, with runtime cut selection in `crcbl-shaders`. This
  is most of topic 25, and `crcbl lod` ships it as a subcommand.
- **`crcbl-assets`** — `AssetSource`, `DirSource`, `AssetRegistry`. The Fetch
  source, hot reload and the RON scene format are P9's remainder.
- **`crcbl-mtl` and `crcbl-dx12`** — P14's two backends, each with a real-device
  CI job and its own harness.
- **P7's GPU-driven half**, meshlet geometry as a real path rather than a
  degradation, SSAO, SSR, a point/spot shadow atlas, L1 irradiance probes, GGX
  materials and a scene-description API.
- **Two acceptance fixtures, not games**: `apps/hud` and `apps/lantern`, both
  published to the demo site. `apps/bare` is a third, and is the engine used as
  a plain library with the loop written by hand.
- **The browser entry point moved into the engine** (`crcbl::web_exports!`), and
  the samples' remaining copy-paste is inventoried in the 2026-08-15 section at
  the end of this file.

The subsections below are the record of each phase as it closed, newest first.

**S3 is complete: the demo site carries six demos, and the scale sample has its
numbers.** Horde — one arena, a crowd that converges, a gun that aims itself,
three enemy kinds, XP gems and a "pick 1 of 3" level-up — is playable natively
and in a browser, with five spatial cues, the longest run kept in `~/.config` or
the Origin Private File System, and `web/run-browser-e2e.sh` driving it in a
real Chromium for 26/26 checks. Breakout, flappy and asteroids are 26/26
alongside it.

It was built **before** P7 and P8 rather than behind them, which is a departure
from the phase table below, and the point of doing it that way was to find out
what those phases actually have to buy. The measurement is in
[sample/03-horde.md](sample/03-horde.md) with the conditions on every table, and
it is unambiguous: **the render side carries ten thousand and the tick does
not.** CPU frame time is 0.096 ms at one thousand enemies and 0.120 ms at ten
thousand — flat, which is the exit criterion — while the tick is 14.66 ms with
the crowd spread and 84 ms once it has converged on the player, against a 16.67
ms budget. So P8 (`crcbl-jobs`, the parallel schedule) is worth the whole of the
gap and P7 (GPU culling, indirect draws, instance deltas) can return at most 0.7
% of a frame to this sample. The roadmap had the dependency the other way round.

**S2 is complete: the demo site carries three games.** Asteroids — a ship that
turns, thrusts and wraps, bullets that never miss, rocks in three sizes that
split twice, waves that grow — is playable natively and in a browser, with three
spatial cues, a best score in `~/.config` or the Origin Private File System, and
`web/run-browser-e2e.sh` driving it in a real Chromium for 26/26 checks.
Breakout and flappy are 26/26 alongside it.

It is the **churn** sample and it earned the name: an 18,000-tick soak asserts
the entity and collider accounting on every tick while the game spawns and
destroys constantly, and `hundreds_of_spawns_and_deaths_leak_nothing` is what
found that `GameModule::tick` runs after the ECS sweep. What the sample was
built to produce was a findings note — nine places the engine resisted a game
that was neither breakout nor flappy, three of them the second game's findings
coming round again. What became of all three lists is below; what is still open
out of them is in `docs/backlog.md`.

**P4B is complete: pixel art has a pipeline, and both games are drawn with it.**
Eleven slices merged, from a format nothing could read to two retrofitted
samples. `crcbl-sprite` gained a reader — `decode_png`, `read_aseprite_json`,
`load` — so a baked sheet stopped being write-only, and `Playback`, a bare tick
cursor that answers `frame_index` as a closed form so catching up after a stall
lands where tick-by-tick would. `crcbl-render` gained `texture::upload_texture`
(format-agnostic, replacing `ui_pass`'s private R8-only helper whose row pitch
was correct only because `R8Unorm` makes texels and bytes the same number),
`SpriteRenderer` and `sprite.slang` (one instance per sprite, batched by sheet
in submission order, constants from a uniform buffer on every tier so there is
no second `.slang` to keep in step), `NineSliceSource::expand`, `LayerStack` and
`Parallax`, and `button_skin` — which draws a skinned button through the sprite
pass, because the UI atlas is a single-channel glyph mask and a skin is RGBA
colour art. `crcbl crpix` turns PNG frames into a `.crpix` sheet, for art
authored anywhere else.

**`SampleMode::Pixel` is sharp bilinear, not nearest**, and that is the decision
the pipeline turns on. Nearest at a non-integer scale — a 320-wide field across
a 1366-wide canvas — makes some art pixels four screen pixels across and their
neighbours five, and the unevenness crawls as the sprite moves. Instead the
linear blend is squeezed into a band one fragment wide at each texel boundary
and the sprite's screen rectangle is snapped to whole device pixels.

**Both samples are retrofitted, and `ForwardRenderer` is gone from both
frames**, taking the HDR scene target, the depth buffer and the tonemap pass
with it — the forward pass drew exactly one instance, and in each game that
instance was the player. Flappy has a bird whose three-frame flap restarts on a
rising vertical velocity rather than free-running, a three-sliced pipe, and
hills and a ground band on parallax layers; breakout has four bevelled brick
rows, a paddle, a ball, and a nine-sliced stone court whose wall faces land
exactly on the colliders the ball bounces off. Every picture in both is `.crpix`
text under `apps/*/assets/`, baked to PNG + sidecar by a `build.rs`; nothing
baked is committed, so the text is the only source of truth.

**The pictures are checked against a real driver, not only asserted.** Six
golden images run the real passes through `crcbl-vk` and read the pixels back:
`sprite`, `sprite_alpha`, `sprite_pixel`, `sprite_smooth`, `sprite_nine_slice`
and `button_skin_widths`. `sprite_pixel` is the one that pins the sharp-bilinear
arithmetic — a `Pixel` sprite at a whole scale is exactly flat inside each
texel, which no recorder assertion can tell you.

**S1B finding 1 is closed by this work**, and is struck through in the findings
list below. The other five stand.

**S1B is complete: the demo site carries two games.** Flappy — a bird under
gravity, one button, and an endless procession of procedurally placed pipes — is
playable natively and in a browser, and `web/run-browser-e2e.sh` drives it in a
real Chromium for 19/19 checks the same way it drives breakout. What the sample
was built to produce is the findings note below: six places the engine resisted
a game that was not breakout, and a list of the seams that did not.

**P5 is complete.** Nine slices merged: polled device creation across the HAL
seam (P5.4), AudioWorklet output (P5.5), the wasm32 dependency graph (P5.6),
browser storage over fetch and OPFS (P5.7), the JS shim, wasm entry point and
Pages deploy (P5.8), WGSL across the seam (P5.9), a Tier B constants path
(P5.10), wgpu present + offscreen (P5.11), the cross-backend image gate (P5.12),
and the headless-browser gate (P5.13).

**The gate is closed: the demos render and play in a browser.**
`web/run-browser-e2e.sh` drives a real Chromium over the built site and reports
19/19 for each — the page boots, opens a WebGPU device, configures a swapchain,
takes a real click and a real `Space` key, starts the game, and draws distinct
frames with no device errors. It runs once per demo (`CRCBL_WEB_E2E_DEMO`), so a
failure names the game rather than the site.

Two blockers stood in the way and both are recorded below rather than forgotten:
naga rejected every SPIR-V artifact the engine shipped (closed by P5.9 carrying
WGSL across the seam), and Dawn — stricter than naga about WGSL's uniformity
rule — rejected the UI shader for sampling a texture under a branch on a
varying, which invalidated the whole frame's command buffer and left the canvas
black while the game ran normally. Both were found by building the check before
believing the code.

**The HAL seam is now frozen**, on the roadmap's own criterion: two backends
implement it, and `crcbl screenshot` renders the same scene through `crcbl-vk`
and `crcbl-wgpu` to byte-identical PNGs on one driver, one channel level apart
across two.

**A full-workspace code review** (2026-08-01) read every line of the tree and
its findings were fixed across eight commits before this phase resumed — among
them a BVH refit that hid untouched colliders from every query, an unsound
`Send`/`Sync` on the audio mixer, three allocation bombs reachable from ordinary
files, path traversal out of the storage root, an unauthenticated ack that could
permanently desync a client, and a `crcbl-wgpu` that could not complete a frame.
Breakout's determinism tests never launched the ball, which is why three
gameplay bugs had survived; they do now.

**P0 and P1 are complete and merged to `main`.** The `crcbl screenshot` CLI
subcommand (offscreen render → PNG), explicit `PipelineLayoutHandle` on
`bind_group`/`push_constants`, `BindGroupDesc::variable_count`, render-graph
sub-resource vocabulary, and three HAL seam findings closed on 2026-07-31. P2a
is complete: per-entity `Transform` replication (`SystemTrait::replicate` seam,
`PhysicsSystem` impl) and real client-side interpolation landed on 2026-07-31.
P2b is complete: protocol negotiation and reconnect, condition simulation,
per-sector ack-baseline streams, replication integration, hostile-input
hardening, per-client ingress limits, and decoder fuzzing in CI have landed. P2c
is complete: `crcbl-store` (StorageSource, settings, saves, replay), GameModule
API + static binding, and `.crpl` replay writer/reader with FileTransport. **P3
L0 is done**: overlap queries, dynamic BVH refit, swept-sphere-vs-capsule TOI,
trigger-volume support, ECS component types (RigidBody, Transform,
ColliderComponent), and PhysicsSystem (SystemTrait impl bridging entities to the
spatial world). **P4 is done**: `crcbl-ui` draw-list, glyph atlas text,
label/button widgets, HUD skeleton, draw-list snapshot hash tests; `crcbl-store`
crash ring buffer; `crcbl replay` CLI. **P4A audio is done**: device seam
(cpal), audio thread, mixer/voices, WAV and QOA decoders, full spatial cue
grammar rules 1–4, and golden-buffer test.

The phase table below is the plan; this section is the record. Where the two
disagree about what was built, this section is right and the phase row says what
was intended.

| Phase              | Status   | Landed as                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------ | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **P0** — base      | **done** | `f922ca3`, `3198f7a`, `6dd4b46`, `84af231`, `c058f45`, `bad7186`, `a991e42`, `063fd99`, `421ce69`, `f06e6cd`, `36dd636`, `e094d39`                                                                                                                                                                                                                                                                                                                                                                                    |
| **P1** — Vulkan    | **done** | `91fd871`, `236f19b`, `c6dc4a4`, `a54990d`, `dc36d32`, `8a4e303`, `cbd6153`                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| **P2** — sim core  | **done** | `7b8efb5` scaffold, `f8e8117` net, `9e55569` ecs, `9d31e2d` input, `2b2e5bd` server/client, `d4d0330` sim harness; `7f2d920` interpolation; P2b through `f1e625c`, `e036705`, `ec0597e`, `fb7e7bf`, `9dcc30d`, `b6c9d7d`, `27390fd`, `cb3b110`, `b37e4d5`, `53405f6`, `4156345`; P2c through `4ff402e`, `fa16710`, `c4688f1`, `83362f3`, `1270c72`                                                                                                                                                                    |
| **P3** — phys L0   | **done** | `5665da2` overlaps, `cbfd6b1` dynamic BVH refit, `b1924dd` swept-capsule TOI + triggers, `c4db85d` ECS components, `b765640` PhysicsSystem, `a2be1f6` integrator, `6b30532` force providers, `60dd95e` integration loop, `05dd23a` property tests                                                                                                                                                                                                                                                                     |
| **P4** — UI L0     | **done** | `49ec170` draw-list, `b40ca95` label/button/HUD, `ab17700` `0352112` `510ce31` triangulation, `9f65472` snapshot tests; `1270c72` replay; `264a7fd` crash ring                                                                                                                                                                                                                                                                                                                                                        |
| **P4A** — audio    | **done** | `6bd33b2` device seam, `a7e94c2` mixer/voices/golden, `912234f` WAV, `2abbd2d` QOA, `916d51f` cue grammar rules 1–4, `bf9a245` clippy                                                                                                                                                                                                                                                                                                                                                                                 |
| **P4B** — sprites  | **done** | `5f0c723` texture upload, `3f7cbd6` sheet reader, `0f2f434` sprite pass, `44837fc` flappy cue counting, `ed92170` sharp bilinear + sprite goldens, `250f287` clip playback, `acd75be` nine-slice + layers, `2d65fb4` skinned buttons, `216ea85` `crcbl crpix`, `37ce45d` flappy art, `7e2458c` breakout art + wing beat                                                                                                                                                                                               |
| **S1** — breakout  | **done** | `d747a84` scaffold, `71d931c` paddle+input, `5989f1b` ball+physics, `495a7fb` bricks+scoring, `ee3bea5` audio+HUD, `fa4e20b` spatial panning, `3bcf327` client interpolation, `a6c2e6b` high score, `ecfd85a` determinism tests, `ab71b1e` input fix, `968b65a` launch fix, `e3fd64d` persistence test, `5f47a12` UI compositing pass                                                                                                                                                                                 |
| **S1B** — flappy   | **done** | `1de08a6` wgpu device errors, `8411987` bird + gravity + flap, `86bb004` seeded course + treadmill, `0c1991b` collision + score + restart, `69e6624` scrolling camera + HUD, `c898e1a` audio + best score, `c0446be` demo-site publish                                                                                                                                                                                                                                                                                |
| **P5** — wasm      | **done** | `a61bd26` polled device creation, `b932e28` AudioWorklet output, `325b8ba` wasm32 dependency graph, `9c1b48e` fetch + OPFS storage, `c5c2a13` JS shim + entry point + Pages deploy, `fd0bc23` WGSL across the seam, `df45682` Tier B constants, `33d4dc0` wgpu push-constant capability, `afb4579` wgpu present + offscreen, `e52e28c` cross-backend gate, `4d7c7c8` uniform-control-flow shader fix, `ed3e726` headless-browser gate. Earlier: `84e531b` WebShell, `afd63bf` wasm32 target, `f7d28ad` WGSL artifacts |
| **S2** — asteroids | **done** | `da3096d` the game core, `c0f87ef` art, rotation and menus, `485e8aa` audio, the best score and the browser demo                                                                                                                                                                                                                                                                                                                                                                                                      |
| **S3** — horde     | **done** | `63873a7` the core loop, `67c3207` art, experience and the level-up screen, and this slice: audio, the longest run, the browser demo and the scale measurement                                                                                                                                                                                                                                                                                                                                                        |

### What exists now

- **Workspace + CI**: every crate under `crates/`, every sample under `apps/`.
  `.github/workflows/ci.yml` is the list of required jobs and the only one worth
  trusting — fmt, a `shell` lint, clippy `-D warnings`, rustdoc `-D warnings`,
  `cargo-machete`, `cargo-deny`, a `wasm32` check, nextest on Linux and
  cross-platform (macOS + Windows), coverage, a weekly advisory cron, a decoder
  fuzz job, the shader-manifest check, and an e2e job per platform seam (Wayland
  under nested sway, X11 under Xvfb, Win32, Vulkan on lavapipe and again on
  Windows, Metal, D3D12, and the CLI scaffold). The `wgpu e2e` and
  `cross-backend image compare (vk vs wgpu)` jobs this list used to name went
  with `crcbl-wgpu` in `6b5e17a`; the browser's own gate is the Pages
  workflow's, and `web/run-cross-backend-e2e.sh` is what replaced the compare.
- **`crcbl-core`**: `Handle`/`Pool`, sector-tiled `WorldPos` (`I64Vec3` sectors,
  2^20 m cells), `FrameArena`, `FrameClock` with an injected `TimeSource`, the
  input vocabulary, `SurfaceTarget`, logging.
- **`crcbl-hal`**: the backend seam — object-safe traits, POD descriptors,
  handle-based resources, reversed-Z defaults, request/poll readback, and a
  recording `NullBackend` that is a test tool rather than a stub.
- **`crcbl-shell`**: the platform-agnostic seam plus `HeadlessShell`, and
  **backends for Wayland, X11, Win32, AppKit and the browser canvas**. The two
  Linux ones are built on our own protocol codegen (`crcbl-wl-scanner`) and
  hand-written `dlopen`'d FFI. Windows, monitors, input, XKB keymaps, pointer
  lock and raw motion, fractional scale, clipboard. XDND on X11 is the one
  deliberate gap — `x11/mod.rs` removes `ShellCaps::DRAG_DROP` by hand; see
  [15-windowing.md](15-windowing.md).
- **`crcbl-vk`**: loader through present on real hardware — device, swapchain,
  pipelines, bind groups, samplers, deletion queue, timestamp queries.
  Validation is enabled and provably non-vacuous.
- **`crcbl-render`**: the render graph — passes declare reads and writes;
  compile is pure and testable with no GPU; it is the only thing that emits a
  barrier in a frame, and it can dump its own pass order and barriers as text.
  Grown well past that since: the forward pass and its clustered light grid,
  mesh/instance/cluster pools, GPU culling and draw generation, the material
  table, shadows, SSAO, SSR, irradiance probes, the sprite and UI passes, the
  menu, transients and per-pass timing. `crcbl-render/src/lib.rs`'s module list
  is the current inventory.
- **`crcbl-shaders`**: Slang sources with **four committed artifact sets** —
  SPIR-V, WGSL, DXIL and MSL, plus the cluster blobs — drift caught by a SHA-256
  manifest over the _sources_ that verifies with no compiler installed.
- **`crcbl-golden`**: the image comparator, tolerance calibrated by measurement
  against radv-vs-lavapipe rather than guessed.
- **`crcbl-cli`** (`new`, `run`, `build`, `screenshot`, `replay`, `crpix`,
  `lod`) and **`apps/sandbox`**, which draws a reversed-Z lit spinning cube
  through the graph into an HDR target on both Wayland and X11.
- **`crcbl-jobs`**: the work-stealing `Pool`, the `Spawn` seam with `Threads`
  and `Inline`, mailboxes, a ring and `par_for`. No Web Worker backend yet —
  `Threads` is `cfg(not(target_arch = "wasm32"))`.
- **`crcbl-scene`**: glTF import and validation, QEM edge-collapse
  simplification, meshlet build, the cluster DAG and LOD resolve.
- **`crcbl-assets`**: `AssetSource`, `DirSource` and `AssetRegistry`. Hot reload
  is still future tense in the crate's own docs.
- **`crcbl-mtl` and `crcbl-dx12`**: the Metal and D3D12 backends, each with a
  real-device CI job and its own e2e harness.
- **`apps/breakout`** (S1, 12 slices merged): paddle movement via `ActionMap`,
  server-authoritative over `InMemoryTransport`, `PhysicsSystem` with
  swept-sphere CCD (ball, paddle, 40-brick grid, left/right/top walls), game
  state machine (Waiting→Playing→Won/Lost), 3 lives + fall-out detection,
  spatial audio panning (`compute_cue` per ball position), client-interpolated
  ball state with replication drift detection, high score persistence via
  `crcbl_store::write_atomic`, on-screen HUD via `UiRenderer` (glyph atlas
  texture, per-frame vertex/index upload, alpha-blended compositing pass), and
  its own tests including scripted-input determinism and persistence RT.
- **`crcbl-wgpu` is deleted** (2026-08-21, `6b5e17a`). This list carried two
  entries for it — the wgpu 30 backend and its cross-platform `cell` module —
  long after the crate, its suite, both its CI jobs, the `GpuBackend::Wgpu` and
  `BackendKind::Wgpu` variants and `CRCBL_GPU=wgpu` all went. `wgpu` and
  `gpu-allocator` are at zero occurrences in `Cargo.lock`. What replaced it is
  `crcbl-webgpu` below; see "The WebGPU backend replaced `wgpu`" further down
  this file, which recorded the deletion while this section went on describing
  the crate as present.
- **`crcbl-shell`** (P5): `WebShell` backend — single-canvas window,
  `extern "C"` entry points for JS→wasm input (resize/key/pointer/frame), event
  queue over `RefCell<VecDeque>`, `SurfaceTarget::Web { canvas_id }`. Registered
  in backend table as non-auto (`CRCBL_SHELL=web`). Caps exclude `MULTI_WINDOW`,
  `POINTER_WARP`, `CLIPBOARD`, `DRAG_DROP`, `EVENT_WAIT`. 8 unit tests. JS shim
  counterpart (P5.3) follows.
- **Polled device creation** (P5.4): `Instance::request_device` returns a
  `Box<dyn PendingDevice>` whose `poll` yields `Pending` or `Ready(device)`,
  following the `request_readback`/`poll_readback` idiom the seam already had.
  `wgpu`'s `requestDevice` is a promise and the browser main thread cannot block
  on it, so before this `crcbl-wgpu` returned `Unsupported` on wasm32 and
  nothing could render in a browser. Everything decidable synchronously (unknown
  adapter, missing features, foreign surface) is still an error from the
  request. `create_device` remains as a provided blocking wrapper and is
  `#[cfg]`-ed out on wasm32, so a browser build that reaches for it fails to
  compile rather than at run time; the ~35 native call sites are untouched.
  Vulkan's first poll is ready (no faked latency); the null backend models both
  outcomes and gained `set_device_latency` beside `set_readback_latency`, so the
  polled path is testable with no GPU. `crcbl::backend::request_open` and
  `GpuContext::request_open` are the browser-shaped entry points.
- **AudioWorklet output** (P5.5): `crcbl-audio` gained a web output and gated
  `cpal` to non-wasm targets — its wasm dependency graph is now zero third-party
  crates. The seam is a pure pull (`render(frames)` into wasm-owned planar
  buffers), which needs no `SharedArrayBuffer` and so survives Pages' inability
  to set COOP/COEP, as the 2026-07-27 correction in
  [10-wasm-webgpu.md](10-wasm-webgpu.md) required. The source is always driven
  at the fixed 48 kHz internal rate and resampled to the device rate with phase
  and carry frames across blocks — passing a 44.1 kHz device rate down would
  detune every asset by 147 cents and corrupt the pitch cues rules 3 and 4
  depend on. An underrun returns a short block and is counted, never a repeat.
- **Browser storage** (P5.7): `crcbl-store::web` — `FetchSource` (pre-load as
  the intended mode, request/poll underneath for anything discovered at run
  time; a miss returns `StorageError::Pending`) and `OpfsStorage`. OPFS has no
  `rename`, so `write_atomic`'s guarantee is split honestly rather than quietly
  weakened: generation ping-pong with SHA-256-framed records keeps "no torn
  value", "no older value after a newer one" and "nothing left behind by a
  failure", and drops durability-on-return — `write` returns when queued, and
  the answerable form is `queued + in_flight == 0`. Nothing needs an OPFS sync
  access handle, so the engine may run on the main thread or in a Worker; the
  shim declares which and a shim that never drains fills the queue and is
  refused rather than silently dropping writes. One `canonical_key` guards every
  key from the engine and from the shim's manifest, with a charset that excludes
  every URL metacharacter, so no key can escape the asset root.
- **wasm32 dependency graph** (P5.6): `apps/breakout` and the `crcbl` umbrella
  build for `wasm32-unknown-unknown`. `crcbl-vk` moved to a
  `cfg(not(target_arch = "wasm32"))` dependency of the umbrella (`ash` reaches
  `libloading`, which has no wasm build), and its registry entry is `#[cfg]`-ed
  out to match; the browser backend — `crcbl-wgpu` when this was written,
  `crcbl-webgpu` since it was deleted — becomes the auto-selectable one on wasm
  only, so native selection order is unchanged. `crcbl::screenshot` is
  native-only — every step of it blocks. `getrandom`'s `wasm_js` backend is
  enabled from `crcbl-server`'s wasm target section, so `wasm-bindgen`/`js-sys`
  stay out of native binaries. CI's wasm32 job now checks and clippies the whole
  workspace minus `crcbl-vk`, with `apps/breakout` first and by name.
- **JS shim, wasm entry point, Pages deploy** (P5.8): `apps/breakout` is a
  library with two front ends — `main.rs` (argv, exit codes) and `src/web.rs`
  (`cdylib`, `extern "C"`, driven by `requestAnimationFrame`). Start-up is
  polled: `PendingLoop` turns `wait_for_configure` and `Gpu::open`'s two blocks
  inside out, so the device promise is polled across rAF frames instead of being
  waited on inside the loop that resolves it. The clock is the browser's
  (`Loop::set_frame_step`, clamped) because `Instant::now` panics on
  `wasm32-unknown-unknown`, and so does `crcbl_core::log::init_logging` — the
  browser build queues log lines in wasm and the page drains them. The high
  score moved to `OpfsStorage` on wasm32. `web/` holds the shim: canvas/DPI/
  input, the AudioWorklet feed (shape B, `postMessage`, no `SharedArrayBuffer`),
  fetch pre-load, OPFS restore/drain, plus the site's index and the breakout
  page. `.github/workflows/pages.yml` builds on PRs and deploys on main, with
  `pages: write`/`id-token: write` scoped to the deploy job only.
  `web/tools/check-exports.mjs` is the gate that can run without a browser: it
  compares what Rust declares, what the shim calls, and what the artifact
  exports, and fails on any of the three disagreeing. **`wasm-bindgen` is
  adopted as a build tool only** — no `#[wasm_bindgen]`, no crate depends on it,
  the version comes out of `Cargo.lock` — because `wgpu`→`web-sys` leaves ~320
  unresolvable `__wbindgen_placeholder__` imports that only its CLI can link.
  The gate this slice could not close — no browser had ever loaded the page — is
  closed by P5.13.
- **Web key ABI correction** (P5.8): `crcbl-shell`'s `__crcbl_web_key` takes two
  `(ptr, len)` pairs and the module had no way for a browser to obtain an
  address inside wasm memory, so the entry point was specified but not callable.
  `__crcbl_web_key_scratch_ptr`/`_capacity` publish a wasm-owned scratch, the
  same shape `crcbl-store`'s fetch and OPFS ABIs already used. Found by writing
  the shim; a test now drives a key event through the real path.
- **`crcbl-shaders`** (P5.3): WGSL artifacts committed alongside SPIR-V —
  `compile-shaders.sh` produces `wgsl/*.wgsl` per shader; `Shader::wgsl()`
  returns the source for wgpu backends. The manifest tracks every format. (DXIL
  and MSL joined them with P14; see the `crcbl-shaders` bullet above.)
- **`crcbl-ecs`** (P2a): system-owned-array ECS — `World`, `System<T>` (dense
  SoA + sparse entity→index), `Schedule` (ordered tick sequence), `Inspector`
  (per-system stats). Entity lifecycle: spawn, deferred despawn, generational
  sweep across all systems.
- **`crcbl-net`** (P2a): transport seam and replication protocol — `Transport`
  trait (reliable/unreliable channels, non-blocking), `InMemoryTransport` (SPSC
  pair for single-player and tests), `SnapshotWriter`/`SnapshotReader`
  (per-system snapshot encoding). **P2b additions**: protocol handshake +
  schema-hash gate, session management + reconnect, `ConditionSimulator`
  loss/reorder wrapper, hardened wire codec, ack-baseline delta encoding with
  entity-removal and keyframe fallback, sector-scoped envelopes, per-client
  ingress limits, and decoder fuzzing in CI.
- **`crcbl-input`** (P2a): device-agnostic action system — `ActionMap`
  (bindings→actions, WASD composite, mouse motion/scroll), `ButtonAction` with
  just-pressed/just-released edges, `InputTickState` for client→server tick
  snapshots.
- **`crcbl-server`** (P2a): authoritative fixed-tick server — drains client
  inputs, advances the ECS schedule, emits per-tick snapshots over the
  transport. Headless-runnable; no render dependency.
- **`crcbl-client`** (P2a): rendering client — sends input each tick, buffers
  the two most recent snapshots for interpolation, handles snapshot reordering.
- **`crcbl-sim`** (P2a): determinism harness — runs N ticks of a seed-generated
  world using `ManualTime`, prints a state hash. Same input → same hash across
  runs, verified.
- **`crcbl-phys`** (P3 L0): query + kinematics pillar — `PhysicsWorld` with
  sphere/box/capsule colliders, static BVH with O(log n) refit, overlap queries
  (sphere + AABB), ray-vs-shape and swept-sphere TOI (including exact capsule),
  trigger-volume support, ECS component types (`RigidBody`, `Transform`,
  `ColliderComponent`), `PhysicsSystem` (full integration loop: force providers,
  SemiImplicitEuler integration, collider sync), L1 dynamics (terminal velocity
  emergence, determinism, bounded energy drift), and 1000-body stress test with
  state-hash determinism.
- **`crcbl-ui`** (P4): immediate-mode draw list with rect/text/outline commands,
  CPU triangulation into screen-space vertex+index buffers, baked-in monospace
  glyph atlas (95 ASCII glyphs at 8×13 px), `FontAtlas` text layout (greedy
  line-break), `Label`/`Button` widgets with state-based styling,
  `Hud`/`HudPanel` anchored overlay container, draw-list snapshot hash tests.
  **S1 completion** added `UiRenderer` in `crcbl-render` (GPU pipeline, glyph
  atlas texture upload, per-frame vertex/index buffer write, alpha-blended
  compositing pass on top of the tonemap target) and the `ui.slang` shader in
  `crcbl-shaders` (screen-space vertex pulling, push-constant viewport
  transform, glyph atlas sampling).
- **`crcbl-sprite`** (P4B): the pixel-art pipeline's data half — the `.crpix`
  parser and baker (`crpix`, `bake`, `colours`, `trace`) that turn hand-written
  text into a PNG strip plus an Aseprite-schema sidecar, and behind the `load`
  feature the reader that takes them back apart (`decode_png` normalising RGB,
  palette, grey and grey+alpha to packed RGBA8; `read_aseprite_json` as §7's
  exact inverse; `load` as the pair). `Sheet`, `Frame`, `Clip`, `NineSlice`,
  `SampleMode`, and `Playback` — a tick cursor that holds neither sheet nor
  clip. No renderer dependency: `docs/specs/crcbl/pix.md` is the format's spec
  and it stops at the two baked files.
- **`crcbl-render` sprite stack** (P4B): `SpriteRenderer` — instanced,
  world-space, alpha-blended quads cut out of registered sheets, one instance
  per sprite from `SV_InstanceID`, batched by sheet in submission order and
  nothing sorting behind the caller. `SampleMode::Pixel` is sharp bilinear
  rather than nearest, carried on the instance rather than in the sampler, so
  there is one sampler and one pipeline. `NineSliceSource::expand` turns stored
  insets into up to nine quads that tile the target exactly, stretching and
  never tiling; `LayerStack`/`Parallax` band them back to front; `button_skin`
  makes a skinned `crcbl-ui` button nine of them. `texture::upload_texture` is
  the shared format-agnostic staging upload underneath, and the UI pass now uses
  it too.
- **`crcbl-audio`** (P4A): `AudioStream` device seam (cpal native, null for CI),
  audio thread polling at hardware sample rate, a `Mixer` of `Voice`s (looping,
  volume, pan, varispeed), stereo f32 internal format, WAV decoder (mono/stereo
  8/16/ 24/32-bit PCM + float), QOA decoder, `SpatialCue` compute with
  deterministic cue grammar rules 1–4 (ITD + ILD for pan, rear attenuation +
  pitch for behind, elevation pitch for above/below, distance rolloff), and
  golden-buffer test.

  P4A shipped the `Mixer` **unusable**, which is what finding 5 below was really
  reporting: `play` took `&mut self` while `AudioStream::open` consumed its
  source, so once the stream was running nothing could reach the mixer to play
  anything through it. All four samples wrote their own queue instead. The S3
  follow-up slice fixed the shape — `play(&self) -> VoiceId`, `stop`, `set_mix`,
  `AudioSource for Arc<T>`, `VoiceMix::from(&SpatialCue)`, and `SoundBank`
  buffers shared rather than copied per voice — and migrated all four samples
  onto it. `MAX_VOICES`, priority and stealing are still not in the crate;
  `docs/backlog.md` carries that.

**`cargo nextest run --workspace --all-features --locked`: 3453 passed, 223
skipped** (measured 2026-08-15) — the skips are the `#[ignore]`d e2e harnesses
below, which each runner opts into by name. Treat the figure as the last
measurement rather than a property: it moves with every slice, and the harness
prints its own total, so re-run it rather than adjusting it here. The whole
workspace suite passes with no Vulkan driver present at all, and the shell
suites pass under 32-way CPU contention. The browser half adds two checks that
need no browser: `web/tools/check-exports.mjs` (Rust's declared symbols == the
artifact's exports == what the shim calls) and `web/tools/smoke.mjs`, which
instantiates the deployed artifact under node with every import stubbed and
drives the documented boot order.

### What the second, third and fourth games found

Every sample after breakout carried the same exit criterion — _a findings note
listing every place the engine's API resisted a game that was not the last one,
even if the list is empty, because empty is itself the result_. None of the
three lists was empty, and each answered a different question. The second told
us whether a seam had been designed or merely fitted to breakout. The third told
us whether the gaps the second found were being closed, and whether the copies
made to work around them stayed in step — both answers were no. The fourth told
us what happens to a duplication nobody closed on the deadline it was given: the
browser entry point missed two deadlines by name and was written four times, and
the audio and persistence copies drifted into three spellings while it waited.

**That method is the part worth keeping, and it is why the lists are not
reproduced here.** Every finding they raised is now either closed in the tree or
carried in `docs/backlog.md` as an open entry, and a section that restated them
would be a second copy of the backlog going stale beside it — which is what the
"status correction" banner this replaces was admitting. `git log` holds each
list as it was written.

**What closed.** The three seams the lists kept naming: `crcbl::web_exports!`
and `impl_web_pending!` for the browser entry point,
`crcbl::store::record::Record` with `Backing::platform` for "one number kept
between sessions", and `crcbl_audio::mixer`'s `play`/`cue`/`set_listener` for
"play this buffer once, panned". With them went `SpriteRenderer`'s instanced
pass and its `batch_count`, `PhysicsWorld::broadphase_stats`, the
`overlap_sphere_into` allocation, the server's tick/sweep order, `Sprite`'s
`#[non_exhaustive]`, and the wasm32 rustdoc gate.

**What is still open** lives in `docs/backlog.md` and nowhere else: music
streaming and ducking, the voice limit and stealing, the absent entity-shaped
overlap query, angular velocity, the BVH-traversal-order neighbour sum, horde's
arena-size decision, the unrun windowed native path, and the fact that every
performance number in this project was taken on an offscreen image ring rather
than a real swapchain.

**The one claim the lists made that this document still makes:** the seams
themselves survived four unrelated games — the action map, the render graph, the
shell seam, the polled browser start-up, `crcbl-store`'s atomic write,
`crcbl-audio`'s cue grammar and null stream, the sprite pass, the layer stack
and the menu system each took the fourth game with no change and no argument.
Every gap the three lists found was in the convenience layer or in something
never built.

### Known gaps, carried forward deliberately

- **XDND on X11** is not implemented (`ShellCaps::DRAG_DROP` is honestly clear
  there); owed before the editor's asset browser at P12. **And it is not the
  only one**: `08-editor.md` claims OS file drop is "editor work, not seam work"
  because the shell carries file-list mimes from day one, and that is false on
  three of four desktop backends — Win32 publishes `text/uri-list` as a
  registered format rather than reading `CF_HDROP`, macOS reads only
  `public.file-url`, and X11 has no XDND at all. `docs/backlog.md` carries the
  detail.
- **Render scale has no renderer half.** `15-windowing.md` locks two display
  modes and defines borderless as "internal render target at chosen resolution,
  upscale-blit to native surface"; `18-render-features.md` orders the post chain
  around it. `ShellCaps::HW_UPSCALE` exists and `crcbl-render` contains no
  upscale path at all. Owed before borderless means what the doc says.
- **Two shader stops, both closed, both worth remembering (P5.9, P5.13).** Every
  SPIR-V artifact the engine ships declares `OpCapability DrawParameters` —
  Slang emits it because `SV_VertexID` lowers to
  `gl_VertexIndex - gl_BaseVertex` — and naga does not implement it, so
  `crcbl-wgpu` had never created a shader module on any target. The WGSL to
  sidestep it had been committed since P5.3 and was consumed by nothing, because
  `ShaderModuleDesc` carried only SPIR-V. P5.9 gave the seam a WGSL half.

  Then a browser found the second one. Dawn enforces WGSL's uniformity rule
  where naga does not, and both UI shaders sampled the glyph atlas inside
  `if (input.uv.x > 0.0 || input.uv.y > 0.0)` — a branch on a varying:

  ```
  error: 'textureSample' must only be called from uniform control flow
  note: control flow depends on possibly non-uniform value
  ```

  The consequence was total rather than cosmetic: the invalid module invalidated
  the `ui compositing` pipeline, that invalidated the whole `breakout frame`
  command buffer, and `Queue.submit` dropped it — discarding the forward and
  tonemap passes too, so **the canvas stayed black while the game ran
  normally**. Hoisting the sample above the branch and selecting afterwards is
  the same image with no non-uniform sample; Vulkan's output is unchanged,
  verified by the cross-backend PNGs staying byte-identical.

  The lesson is the one this project keeps re-learning: **a second
  implementation is the only thing that finds this class of bug.** Both stops
  were invisible to a green native run, and neither would have been found by
  reading the code.

- **RenderDoc capture** has not been verified by hand; every object and pass
  carries a debug label and `DEBUG_MARKERS` is requested.
- **A green local Vulkan run is weaker than CI's.** Synchronisation validation
  has three reaches, and many installed layers report hazards at record time and
  within one submission but **not across submissions** — which is where every
  cross-frame hazard lives. `run-vk-e2e.sh` measures its own reach, prints it,
  and says so loudly when the run is weaker than the CI job it stands in for.
  The real gate for that bug class is the no-GPU cross-frame graph suite, which
  runs everywhere.

### Standing requirements every sample inherits

These are not items on a list that gets ticked once. They are properties a
sample is expected to have, and a new sample that lacks one is incomplete rather
than pending. `sample/00-samples-overview.md` carries them as rules 4 and 11;
they are restated here because the roadmap is what a phase is planned from.

**1. Every sample ships the debug overlay, and switching it on is one thing.** A
modular panel: frame timing and FPS on every sample always, network stats on
every sample that has a connection, and each further system contributing its own
module rather than the overlay knowing about it. Modular is the load-bearing
word — a sample turns the panel on and gets whatever the systems it uses
registered, which is why this does not become a per-sample HUD written twelve
times the way `web.rs` was written twice.

The frame-timing core was **pulled forward out of P10** and is **built**, for
the reason it was pulled: every sample that already existed wanted it and two
more were planned before P10. `crcbl-ui`'s `debug` module is the panel —
`DebugModule`, `DebugSection`, `DebugRow`, `FrameStats` — `crcbl-render`'s
`FrameTimings` implements `DebugModule` for the GPU's pass timings, and
`HostedGame::debug_sections` is the one hook a sample adds its own through.
Every sample contributes: horde a scene section and an audio one, asteroids the
field and churn counts its soak test asserts plus its held-engine cue, flappy
the treadmill's course and entity counts plus `Audio::plays`, breakout the
bricks left and the ball's ramped speed.

What stays at P10 is the rest of [07-ui-debug.md](07-ui-debug.md)'s debug suite
— inspector, console, culling stats, debug-draw controls, UI inspector — and the
network module is still [23-netcode.md](23-netcode.md)'s netgraph (RTT, jitter,
loss, send/recv bandwidth, snapshot size, resend counts, tick-lead), which lands
with it. Both docs already name these; this is the same work with an earlier
start and a stated obligation on samples, not a second plan.

Breakout and flappy were the retrofit consumers, and they are the check that the
panel is genuinely modular: neither has a network module to show, because both
run over `InMemoryTransport`, so a panel that cannot render without one is
broken. It renders — and each sample's
`the_overlay_is_composed_of_exactly_the_modules_*_has` asserts the exact section
list, so a module that appeared without a system behind it would fail. Flappy's
`Audio::plays` counter existed for exactly this and is now what its audio
section reports. Breakout contributes no audio section at all, because its
`Audio` keeps no counter — the honest answer, and a second shape of the same
modularity claim.

**2. Every sample that should have pixel art uses the sprite system.** Authored
as `.crpix` text in the repository, baked at build time to PNG + sidecar by the
sample's `build.rs`, drawn through `SpriteRenderer` with `SampleMode::Pixel`.
Nothing baked is committed, so the text a reviewer reads is the picture that
loads — `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input.

"Should have" is the whole of the judgement. A sample whose subject is not
pictures is exempt: **hud** is a widget gallery, **viewer** opens arbitrary
glTF, **sparks** is a particle workbench, and none of them wants a sprite sheet
in place of what they exist to show. Every other sample on the ladder is 2D or
has 2D chrome, and for those "we will draw untextured quads for now" is no
longer an available answer — it was the answer breakout and flappy both gave,
and P4B is the cost of unwinding it twice.

The engine-side gaps this obligation is currently paying for are in
`docs/backlog.md`: `NineSliceSource` has no texels-to-units scale, and the bake
half of `build.rs` is copied per sample. Both are owed before the third sample,
for the ordinary reason — two consumers is evidence, three is a habit.

### How work proceeds (read this before starting a phase)

Phases are cut into **slices** small enough to review in one sitting. Each slice
is a branch, verified in full, merged fast-forward into `main`, pushed, and
watched through CI before the next one starts. A slice is not done when it
compiles; it is done when the gates below are green and CI agrees.

**The gates.** Every one of these must pass before a merge, and CI runs all of
them:

```
cargo build --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo machete
cargo deny --all-features check
```

**The e2e harnesses.** These need real display servers or a GPU, so they are
feature-gated and `#[ignore]`d — a plain `nextest` run skips them, which is why
each has a script that sets the environment up and **fails if zero tests ran**:

**No test counts in this table on purpose.** Each script prints its own total
and fails at zero, so the number is something a run tells you; one written here
would be wrong within a slice and would still read as verified.

| Harness                                       | What it needs            |
| --------------------------------------------- | ------------------------ |
| `crates/crcbl-shell/tests/run-wayland-e2e.sh` | nested headless sway     |
| `crates/crcbl-shell/tests/run-x11-e2e.sh`     | Xvfb                     |
| `crates/crcbl-vk/tests/run-vk-e2e.sh`         | any Vulkan ICD           |
| the `wgpu-e2e` suite (deleted 2026-08-21)     | a wgpu adapter, Xvfb     |
| `crates/crcbl-mtl/tests/run-mtl-e2e.sh`       | a Metal device (macOS)   |
| `crates/crcbl-dx12/tests/run-dx12-e2e.sh`     | a D3D12 device (Windows) |
| `web/run-cross-backend-e2e.sh`                | both backends            |
| `crates/crcbl/tests/run-render-e2e.sh`        | a GPU the renderer opens |
| `crates/crcbl-cli/tests/run-cli-e2e.sh`       | nothing                  |
| `apps/lantern/tests/run-lantern-golden.sh`    | a GPU (lighting goldens) |
| `web/run-browser-e2e.sh`                      | Chrome + Xvfb            |

`web/run-browser-e2e.sh` is the P5 gate itself and needs no GPU: it serves the
built site, drives it in a real browser over the DevTools protocol, sends a real
click and a real Space key, and **reads the canvas back** to prove the frame is
neither blank nor still. Its header carries the measured table of which
display/adapter combinations can report canvas pixels at all — three of the four
obvious ones cannot, silently — and it runs a known-colour clear as a control
before it believes any render result.

The cross-backend row counts **comparisons**, not `#[test]`s: it renders one
frame through each backend per size and compares the pair with `crcbl-golden`'s
measured tolerance, and it fails when zero comparisons ran for the same reason
the others fail when zero tests ran. It compares a browser's readback against a
live native render now — `--reference vk` on Linux and Windows,
`--reference mtl` on macOS — so the two sides come from different
implementations rather than from two ICDs, which is what the tolerance was
measured against. CI has only lavapipe under Vulkan and says so.

**Two runs that are not optional**, because both have caught bugs a normal run
could not:

```
# No GPU at all — the plain `test (linux)`, macOS and Windows CI condition.
VK_ICD_FILENAMES=/nonexistent.json cargo nextest run --workspace --all-features --locked

# Under contention — a shared CI runner is far slower than a dev box.
for i in $(seq 1 32); do (while :; do :; done) & done
./crates/crcbl-shell/tests/run-x11-e2e.sh
jobs -p | xargs -r kill
```

**Lessons this project has already paid for.** Each of these was a real CI
failure that a green local run had reported as fine:

- **The dev machine is the unusual one.** A long uptime hid a signed-arithmetic
  bug in the X11 event clock; a local GPU hid a missing-ICD failure; a Linux
  checkout hid a CRLF hash mismatch on Windows. If a test can only fail on a
  machine unlike this one, it is not yet a test.
- **A check that cannot run is not a check.** Sync validation was enabled for
  two phases without actually being on; a golden gate conditional on an optional
  shader compiler would have run nowhere; a test-count guard silently matched
  nothing because CI colours its output. Verify the checker, not just the code —
  the pattern is to break the thing deliberately and confirm the test notices.
- **A flaky test is a bug.** `.config/nextest.toml` says so and CI adds no
  retries. Two "flakes" turned out to be a phantom key-release under load and a
  missing cross-frame barrier.
- **Poll for the condition, never sleep**, and demand a _new_ event rather than
  accepting a stale one. Both harnesses do this; new tests must too.

**Conventions.** Conventional Commits; stage explicit paths, never `git add -A`
blindly; nothing above `crcbl-hal` may name a Vulkan type; every `unsafe` block
carries a `SAFETY:` comment naming its invariant; and a seam gap found mid-slice
gets recorded in the relevant crate's docs rather than worked around silently.

## Phase table

| Phase      | Build                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | From stage        | Gate / deliverable                                                                                                                                             |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **P0** ✅  | Workspace, CI, **test infra** (nextest, coverage, NullBackend suite — topic 12), `crcbl-cli` scaffold (`new`/`run`/`build` — topic 11), `crcbl-core` (handles, pools, `WorldPos`, clock, input), HAL seam + NullBackend, `crcbl-shell` (topic 15): Wayland (wayr) + X11 backends, window loop                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | 1, 11, 12, 15     | Window opens; CI green                                                                                                                                         |
| **P1** ✅  | `crcbl-vk`: device, swapchain, frames-in-flight, render graph, pipelines; milestone ladder → lit mesh; ortho/2D path; offscreen render + `crcbl screenshot` + lavapipe golden-image e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 2, 11, 12         | Lit spinning mesh, zero validation errors, golden frame in CI                                                                                                  |
| **P2** ✅  | `crcbl-ecs` (system-owned arrays), `crcbl-net` (transport trait, in-memory), `crcbl-server`/`crcbl-client`, replication, interpolation, tick determinism; `crcbl sim --hash` harness; **action-map input layer** kb/mouse on its own input thread w/ stacked tick states (topics 19+21); **tick-id protocol + client tick alignment** (topic 21); `crcbl-store`: `StorageSource`, settings layers, save/load container over snapshots + atomic writes (topic 14); `GameModule` API + static binding (topic 16); `.crpl` replay writer/reader + FileTransport playback (topic 22); protocol foundations: handshake+schema-hash gate, session reconnect, condition simulator, decode hardening, sector-scoped envelope + ack-baseline deltas (topic 23)                                                                                                                                                                                                                                                                                                                                                                                                                     | 4, 11, 12, 14, 16 | Server-simulated entities render via interpolation; save→load→hash green                                                                                       |
| **P3** ✅  | `crcbl-phys` slice 1 (L0): box/sphere colliders, BVH, ray/segment, swept-sphere TOI + contact normals                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 5                 | Physics unit+property suite + debug draw                                                                                                                       |
| **P4** ✅  | `crcbl-ui` slice 1: draw-list pass, glyph atlas text, label/button, HUD basics; draw-list snapshot tests; black-box replay ring + crash dump + `crcbl replay` CLI (topic 22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 7, 12             | Text + score HUD renders through engine                                                                                                                        |
| **P4A** ✅ | `crcbl-audio`: device seam (cpal), audio thread, mixer/buses, WAV/QOA, voices, **full spatial cue grammar** (topic 13), server-event wiring, golden-buffer e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 13                | Grammar-trainer orbit test audible + green e2e                                                                                                                 |
| **P4B** ✅ | `crcbl-sprite` load half (`decode_png`, `read_aseprite_json`, `load`, `Playback`); `crcbl-render` sprite stack: `texture::upload_texture`, `SpriteRenderer` + `sprite.slang` (sharp-bilinear `SampleMode::Pixel`), `NineSliceSource::expand`, `LayerStack`/`Parallax`, `button_skin`; `crcbl crpix` (PNG frames → `.crpix`); both samples retrofitted off `ForwardRenderer`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | 3, 6, 11, 12      | Sprite goldens through a real driver; breakout + flappy drawn as `.crpix` art                                                                                  |
| **S1** ✅  | **Sample: breakout** — first playable; all deps (P0–P4A) exist. **12 slices merged**: scaffold, paddle+input, ball+physics (swept-sphere CCD), bricks+scoring+game states, audio (sine-wave bounce/brick-break), spatial panning via `compute_cue`, client-interpolated ball state with drift detection, high score persistence via `crcbl_store::write_atomic`, determinism tests, input-queue fix, launch-speed fix, persistence RTT, render-graph UI compositing pass (glyph atlas, per-frame vertex/index upload, alpha-blended on-screen HUD with score/lives/state).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | —                 | Winnable/losable native game with on-screen HUD                                                                                                                |
| **S1B** ✅ | **Sample: flappy** — one-button side-scroller, deliberately smaller than breakout. Needs no engine work past P4A; it exists to answer whether the engine can host a _second_ game without breakout's shape having leaked into the API, and to prove the demo site carries more than one. See [sample/12-flappy.md](sample/12-flappy.md). **Seven slices merged**, listed in the status table above.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                 | **Done.** Playable native + browser; the findings note is above                                                                                                |
| **P5** ✅  | **Wasm early**: `crcbl-wgpu` Tier B backend, canvas/rAF platform, Slang→WGSL, `FetchSource`-lite, AudioWorklet output; cross-backend image compare (vk↔wgpu); **GitHub Pages demo site + CI deploy**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 10 (part), 12, 13 | **breakout playable in browser at the Pages URL**                                                                                                              |
| **P5B** 🟡 | **Built except the browser half.** `crcbl-jobs` ships the spawn seam, mailboxes, the ring, the work-stealing pool and `par_for`, and `apps/horde` is adopted onto it with `--workers`. Owed: the **Web Worker** spawner (`Threads` is `cfg(not(wasm32))`), adoption by the other three samples, and the cross-origin isolation gate. Originally: **`crcbl-jobs` + worker parity** (topic 21), moved ahead of P6–P8: the spawn seam (native `std::thread`, wasm Web Worker, single-thread fallback), latest-wins mailboxes, accumulate-then-swap input ring, work-stealing pool, `par_for` deterministic mode; **adopted by the four existing samples**; cross-origin isolation gate on Pages                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 21, 10, 19        | threads-1-vs-N hash test green in CI; a sample's sim runs off the main thread in a browser; `crossOriginIsolated` asserted by the browser gate                 |
| **P5C** ✅ | **`crcbl-shell` Win32 + AppKit backends** (topic 15), moved out of P14: hand-written `extern "system"` FFI and Objective-C runtime FFI, window lifecycle, the two display modes, monitor/DPI enumeration, input, clipboard and drag-drop — each half landing with an end-to-end suite against a real desktop in CI, the treatment Wayland and X11 got at P0.5/P0.6. **Eight slices merged.** Four shipping defects that only an out-of-process suite could reach: `TranslateMessage` called nowhere, so typing produced no text on Windows; `wait_events` waking instantly forever on a queue bit a pump cannot clear; AppKit's presentation options repositioning a borderless window to its creation frame; and `setStyleMask:` taking the first responder, so F11 cost a game its keyboard                                                                                                                                                                                                                                                                                                                                                                             | 15, 12            | **Done.** A window opens, flips mode and reports injected input on both, proven by `win32 e2e (real desktop)` and the AppKit session target — not by compiling |
| **P6**     | Phys slice 2: dynamic BVH churn, overlap queries, L1 integrator start (thrust, damping)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 5                 |                                                                                                                                                                |
| **S2** ✅  | **Sample: asteroids** — first sample built after the standing requirements, so it is the first to get `.crpix` art and the debug panel from its first slice rather than as a retrofit. **Three slices merged**, listed in the status table above.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | —                 | **Done.** Native + published web demo, sprite art, debug panel on; the findings note is above                                                                  |
| **P6A**    | Wasm module host (topic 16): `wasmtime` behind `WasmHost` seam (NaN canonicalization, fuel limits), Rust guest SDK, component schemas                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | 16                | breakout-as-`.wasm` equivalence gate: state hash == static build, native + browser                                                                             |
| **P7** 🟡  | **Most of it has landed** — the GPU-driven half, the capability seam (`TIER_A`/`RendererTier` are gone, `GeometryPath`/`BindingModel`/`LightingPath` derive from `Features`), meshlet geometry as a real path rather than a degradation, and QEM cluster LOD with runtime cut selection and a `crcbl lod` subcommand. Owed: the bindless material page, and P7B/P7C's lighting work. Originally: GPU-driven rendering full: geometry pools, instance deltas, GPU culling; **the capability seam** — `Features`/`Limits` truth, a `DeviceDesc` default that degrades, derived `GeometryPath`/`BindingModel`/`LightingPath`, `TIER_A` and `RendererTier` removed (topic 39); **meshlet geometry as the primary path** plus both indirect fallbacks (topic 3 §3.5); **QEM cluster hierarchy + per-cluster LOD** (topic 25, moved out of wave 1); **HDR target + tonemap + FXAA; sun CSM shadows** (topic 18)                                                                                                                                                                                                                                                                 | 3, 25, 39         | 10k-instance sandbox, flat CPU cost, and **every path renders it** with a golden image per selected combination                                                |
| **P7B**    | **The rasterised lighting twin, complete** (topic 18): spot and point shadows, screen-space AO, screen-space reflections, irradiance probes. Every one of the four is built and gated by a golden in `crates/crcbl/tests/render_e2e.rs`; the row stays unticked because the exit criterion is lantern's and `apps/lantern`'s room has no spot light, so the sample renders sun and point shadows only. Lands **before** the ray-traced path — it is what macOS, iOS and every browser run                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | 18                | lantern (13) renders the scene complete on `LightingPath::Rasterised`                                                                                          |
| **P7C**    | **Ray-traced lighting** (topic 18): acceleration structures (BLAS bake/load, TLAS refit), ray-traced shadows, AO, reflections and global illumination. **Vulkan only while the deferral holds** — WebGPU has no ray tracing and Slang cannot yet emit it for Metal, so macOS, iOS and browsers stay on P7B's path with no engine branch, and `crcbl-dx12`, the other backend that could ray trace, was deferred on 2026-08-21. That leaves a second complete lighting implementation reaching one of the four backends; whether it is still the right thing to build next is an open scheduling question, not a settled row                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | 18, 39            | lantern (13) renders the same scene on both paths, human-compared                                                                                              |
| **P8**     | Phys slice 3: batch queries at scale, sleeping/islands pressure; the ECS parallel schedule and the physics broadphase adopted onto `crcbl-jobs` (moved to P5B)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 5                 |                                                                                                                                                                |
| **S3** ✅  | **Sample: horde** — the scale sample, and the one built **out of order**: the plan puts it after P7 and P8 and it was built on what exists instead, because milestone 3 is "raise counts until a budget breaks, file engine findings" and that is worth more before the two phases than after them. **Three slices merged**, listed in the status table above. See [sample/03-horde.md](sample/03-horde.md) for the measured budgets.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | —                 | 10k enemies @60; perf numbers recorded; web demo (smaller budget)                                                                                              |
| **P9** 🟡  | **Part built**: `crcbl-assets` (`AssetSource`, `DirSource`, `AssetRegistry`) and glTF import in `crcbl-scene`. Owed: the Fetch source, hot reload, the RON scene format, material templates and `crcbl import`. Originally: Assets + scenes: `AssetSource` (Dir/Fetch), glTF import, RON scene format, hot reload; **material templates+instances + render↔surface link + `mat check` lint** (topic 37); `crcbl import`; glTF corpus e2e                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 6, 11, 12         | Sponza-class scene through full path                                                                                                                           |
| **P10**    | UI slice 2 + debug tools: widget set, panels/splitters, the rest of the profiler HUD (its frame-timing/FPS core is pulled forward before S2 — see the standing requirements), inspector, console, **UI inspector**; music streaming + ducking + cue inspector; audio occlusion; **bloom** (18); **gamepad evdev+web + rebind UI** (19); **UI focus + spatial pad/kb navigation** (7 — arrows/WASD 1:1 dpad); **time-scrub debugger + replay browser + CI determinism verifier** (22)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | 7, 13             | Debug overlay live over loaded scene                                                                                                                           |
| **S4** 🟡  | `apps/hud` is **shipped** — native, a menu, a published browser demo and a CI gate step. Owed: **viewer**. Originally: **Samples: hud complete + viewer** (hud skeleton exists since P4 as the UI fixture; viewer = native tool, web build stretch)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                 | Widget gallery + themes golden-framed; arbitrary glTF opens, panels work                                                                                       |
| **S4C** 🟡 | `apps/quarry` is **built and published** — the face bakes into meshlet clusters and coarsens into a QEM DAG whose cut mixes levels, all three `GeometryPath` values draw it (forced by subtracting features from one adapter), six goldens are committed, the three topic 25 overlays are built, the tiling case is asserted bit-for-bit, and `/demos/quarry/` renders on `IndirectPerBatch` in a browser. `docs/plan/sample/14-quarry.md`'s Measured section carries the numbers. Owed: the **skinned prop**, which waits on skeletal animation (wave 1), and the two **human** reviews the exit criteria ask for — the seam review and the three-way comparison. Originally: **Sample: quarry** (14) — geometry acceptance fixture. Meshlet clusters, QEM cluster LOD, and all three `GeometryPath` values on one dense scene. Runs beside P7 rather than after it: the fixture is how P7's paths are proven                                                                                                                                                                                                                                                            | —                 | Golden frames per `GeometryPath`; three-way comparison recorded; no LOD popping on any path                                                                    |
| **S4B** 🟡 | `apps/lantern` is **built and published** — the room, effect toggles, a golden suite and a browser demo on the raster path. Owed: the both-paths comparison, which waits on P7C. Originally: **Sample: lantern** (13) — lighting acceptance fixture. The same scene under both `LightingPath` values with every effect toggleable; the per-camera toggle layer's first consumer                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | —                 | Golden frames per lighting path; the human-reviewed pair comparison recorded; web demo renders the complete raster picture                                     |
| **P11**    | Phys slice 4 (L1 full): sector frames + SOI, gravity/drag/atmosphere, Kepler on-rails, bubbles, timewarp                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 5                 | Physics stage exit nears                                                                                                                                       |
| **S5**     | **Sample: orbit** — physics acceptance test                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | —                 | Full mission; **flashiest web demo**                                                                                                                           |
| **P12**    | Editor: edit-mode schedule, viewport+picking (phys raycast), outliner, properties, gizmos, undo commands, play mode; `crcbl scene`/`edit --serve` CLI protocol + command/undo property suite                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | 8, 11, 12         | Scene authored start-to-finish in editor **and** modifiable headless via CLI                                                                                   |
| **S6**     | **Sample: towers** (flagship) — solo loop first, then editor-authored map                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | —                 | Solo TD on editor-built map; web demo                                                                                                                          |
| **P13**    | **UDP + own reliability layer** (acks/resend/fragmentation/tokens) (topic 23); **LAN host discovery + lobby browser** — hosts announce on the local network, clients enumerate; session trust plumbing (topic 27, PSK-shaped); quantization + priority/budget encoder; dedicated headless server; **live spectator relay + broadcast delay** (22). **WebTransport and WebSocket are removed** — see the 2026-08-09 LAN correction below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 10 (rest), 23, 27 | Two native clients find a LAN-hosted server through the lobby browser and play; bracket (16) drives the reliable request/response half                         |
| **S6B**    | **Sample: shard** (15) milestone 1 — the MMO-style web slice. Single player, one zone, rasterised lighting, `IndirectPerBatch` + `ArrayPages`. The web flagship, and the first browser figure taken from 3D content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                 | A full session plays in a browser; browser budget and peak wasm memory recorded                                                                                |
| **S6+**    | **towers co-op** — marquee demo. **Native LAN**, not mixed native/browser: a browser cannot host, cannot discover LAN hosts, and cannot reach a LAN server from an HTTPS page                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                 | 4-player LAN co-op session found through the lobby browser                                                                                                     |
| **S7**     | **Sample: bracket** (16) — matchmaking, rating and signed results as a service, with no game attached. Native and local; the web build runs client and server in-process                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 23, 27            | Synthetic population converges to true skill within a recorded tolerance; a forged result is rejected                                                          |
| **P14** ✅ | **Done.** Both backends ship with a real-device CI job and an e2e harness each, and `render_e2e` compares their frames against a lavapipe-blessed reference. Metal + DX12 backends — the HAL half only; the Win32/AppKit **shell** backends moved to P5C. Metal is macOS/iOS's **only** path (2026-08-05: no MoltenVK, no Vulkan on Apple)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | 9                 | Sandbox + samples on macOS/Windows                                                                                                                             |
| post       | **Wave 1**: skeletal animation (17) + **player kit** (30 — puppet adopts it) → **puppet** sample; particles/VFX (20) → **sparks** workbench + towers/horde/shard retrofit; gamepad XInput/GameController (19); ~~on-screen touch controls~~ (**shipped early** as `crcbl_ui::touch`'s `TouchStick`/`TouchButton`, feeding `ActionMap::virtual_stick`/`virtual_button`; horde is the consumer); TAA + auto-exposure (18). Also wave 1: projected+parallax decals (33); UI drag-drop capability (34). Then **Wave 2**: **prediction + lag comp** (26), navigation (24), HLOD sector proxies (25), **contact solver L2+L3** (36) → **ragdolls** (35) → **arena** (forcing function for 26+24, fairness harness, **LAN only**), L3 joints, QUIC native, packaging, **shard milestone 2** (persistent LAN world: sector streaming + interest management at scale). **FPS-era** → **breach** milestone 2 onward (5v5 comp shooter, sample 11, **native only**): ballistic penetration + kinetic impact (28), first-person rendering (29), player kit (30), visibility culling + integrity gate (31), VOIP (32), decal carve tier (33), grid-inventory kit (34), weapon kit (38) | 5, post           |                                                                                                                                                                |

## Demo site (GitHub Pages)

- `gh-pages` deploy workflow: on main push, build all wasm-ready samples
  (`wasm32-unknown-unknown` + bindgen), assemble static site (index page listing
  demos + engine README blurb), deploy to `https://crcbl.kryptic.sh/`.
- Every wasm sample = a Pages demo from the moment it exists. The site was
  planned to grow one demo per S-phase and grows one per **acceptance fixture**
  too: `hud` and `lantern` are published and neither is an S-phase game. Broken
  wasm build = broken CI = blocked merge — the browser target can't rot.
- The demo site's pages are rendered from one layout (`web/templates/` +
  `web/pages/`, built by `web/tools/build-pages.mjs`), the same shape the org
  site uses. Adding a demo is a content file and one line in the demo bar, not
  an edit to every existing page — the property S1B first exercised and S2
  measured: no existing page needed editing, and `build-pages.mjs` fails a demo
  page that does not `<!--include-->` the shared window. There are, however,
  **four** places a new demo has to be named that nothing keeps in step — the
  bar in `build-pages.mjs`, the wasm artifacts in `build.sh`, the game's two
  assertions in `web/tools/browser-e2e.mjs`'s `EXPECTATIONS`, and a per-demo
  step in `.github/workflows/pages.yml` (the gate reads one canvas, so it runs
  once per demo). S3 found the last two by hitting them; `web/README.md`'s
  "Adding a demo" now lists all six pieces. See S2 finding 9.
- The demos are a **subdomain, not a subpath under the org site**, and that is
  deliberate. `www.kryptic.sh` is prose built from one template by a stdlib
  Python script; this site is build output — `cargo build --target wasm32`,
  committed shader artifacts, and a headless-Chromium gate. Folding it into the
  org site would put that whole toolchain into the website's CI and make every
  demo change wait on a second repo's deploy. The subdomain keeps the pipeline
  where the code is. `www.kryptic.sh/crcbl/` remains the project's landing page
  and links here.
- `web/CNAME` carries the domain into the published artifact and the deploy
  workflow fails without it: a missing `CNAME` file silently reverts the custom
  domain on the next deploy, so it is checked rather than assumed.

## Cross-cutting tracks

The pillars deliver in slices across every phase (their own docs carry the slice
tables):

- **CLI/headless** ([11-cli-headless.md](11-cli-headless.md)) — `crcbl` binary
  grows a phase at a time; every subsystem is scriptable the phase it lands.
- **Testing** ([12-testing.md](12-testing.md)) — unit + property + e2e per
  subsystem, in the same phase as the subsystem, never later. Golden
  images/buffers, determinism hashes, lavapipe CI.
- **Audio** ([13-audio.md](13-audio.md)) — spatial cue grammar lands P4A, before
  the first sample; every sample ships with directional sound.
- **Persistence** ([14-persistence.md](14-persistence.md)) — settings +
  save/load ride the P2 snapshot machinery; profiles at P4 (breakout high
  score); OPFS wasm at P5; settings UI at P10; co-op world save/resume proven in
  towers (S6).
- **Debug tools** (in [07-ui-debug.md](07-ui-debug.md)) — each system lands with
  its overlay/inspector hooks, and contributes a **module** to the one debug
  panel every sample switches on. Frame timing and FPS are unconditional; the
  netgraph appears when the sample has a connection. See the standing
  requirements above; the frame-timing core was pulled forward out of P10 and is
  built, and every sample contributes its own modules through it. The netgraph
  and the rest of the debug suite are still P10.
- **Profiling and benchmarking** ([40-profiling.md](40-profiling.md)) — CPU
  spans and counters have **landed** beside the per-pass GPU timestamps
  (`crcbl_core::trace`, `crcbl_render::FrameCounters`, `CullStatsRing`);
  `crcbl bench` arrives with the job system that first needs proving, and the
  panel's perf rows land with the UI slice that owns the panel. **The tooling
  goes in before the optimisation phase, not with it** — a profiler bolted on
  afterwards never covers the code written before it, which is the same argument
  §1.3 makes for putting timestamp queries in the seam at P0. CI publishes
  benchmark numbers and deliberately does **not** gate on them; a shared runner
  is far noisier than a dev box and a perf gate people learn to ignore is worse
  than none.
- **Pixel art** ([specs/crcbl/pix.md](../specs/crcbl/pix.md)) — `.crpix` text
  baked at build time and drawn through `SpriteRenderer`. Every sample that
  should have pixel art uses it; see the standing requirements above.
- **Our own WebGPU** (see the 2026-08-15 section at the end of this file) — in
  progress, and out of the browser: a wasm build links `crcbl-webgpu` and
  nothing else, which took `wasm-bindgen` out of the toolchain with it.
  `crcbl-wgpu` was the last place the engine drew through someone else's
  abstraction, and it is gone — deleted in `6b5e17a`, so there is no
  `CRCBL_GPU=wgpu` any more. The section carries the survey, the architectural
  decision it needed first, and what the replacement did _not_ buy.
- **Sample→engine seams** (see the 2026-08-15 section at the end of this file) —
  a standing sweep rather than a phase. When two samples carry the same
  machinery, it belongs behind an engine seam; when they carry the same
  _content_, it does not. The forcing argument is not tidiness: a hand-copied
  capability list left `apps/hud` opening a device without present feedback,
  which four other samples have a test against. A copy in every sample is a bug
  that can be present in only some of them.

## Notes

- Stage-doc numbering (01–10) is **topic identity, not order**. Topics 11–13
  (CLI, testing, audio) are cross-cutting tracks. Where this roadmap and a stage
  doc disagree on sequencing, the roadmap wins.
- Physics slices P3/P6/P8/P11 are the demand-driven delivery from
  [05-physics.md](05-physics.md); the slice↔sample mapping there matches the
  S-phases here.
- Sample docs numbered in build order: 01 breakout, 02 asteroids, 03 horde, 04
  hud (P4 skeleton → P10 complete), 05 viewer, 06 orbit, 07 towers, 08 arena, 09
  puppet (post-MVP wave 1 — animation/input/shadows showcase). Numbers allocated
  after the ladder was written keep the next free number rather than renumbering
  the rest: 12 flappy, then 13 lantern, 14 quarry, 15 shard, 16 bracket.
- HAL freeze moves in practice to P5 exit: the seam isn't frozen until _two_
  backends implement it — earlier and stronger than the old "freeze at stage 2
  exit", superseding it. The second backend is `crcbl-webgpu`; this rule named
  `wgpu` until 2026-08-23, and `crcbl-wgpu` was deleted on 2026-08-21.

## Corrections (design review, 2026-07-27)

- **P2 is split into three gated sub-phases.** As written it held ECS,
  transport, server/client, replication, interpolation, determinism harness,
  input thread + tick stack, tick-id protocol, `crcbl-store`, `GameModule` API,
  `.crpl` writer/reader, and the whole protocol foundation — more scope than
  P0+P1 combined behind one modest exit, with several architecture-shaping
  decisions riding along invisibly:
  - **P2a — sim core**: `crcbl-ecs`, `crcbl-server`/`crcbl-client`,
    interpolation, tick-id protocol + client tick alignment, input thread +
    `InputTickState`, determinism harness (`crcbl sim --hash`). _Exit_:
    server-simulated entities render via interpolation; hash stable.
  - **P2b — protocol foundations**: transport trait + InMemory, handshake +
    schema-hash gate, session/reconnect, sector-scoped envelope, **ack-baseline
    deltas incl. entity-removal encoding and delta-base-too-old keyframe
    fallback**, encoded-space change detection, condition simulator, decode
    hardening + fuzz corpus. _Exit_: replication survives scripted loss/reorder
    with state equality.
  - **P2c — durability + modules**: `crcbl-store` (settings, saves as sector-set
    snapshots), `GameModule` API + static binding, `.crpl` tick-linear replay
    writer/reader + `FileTransport`. _Exit_: save→load→hash green; a recorded
    session replays identically.
- **P4A gate wording**: the audible gate is **cue grammar rules 1–4**; occlusion
  (rule 5) needs the physics BVH and lands at P10 per topic 13's own delivery
  table. A gate must be checkable when it's claimed.
- **HDR from P1**: render to `RGBA16F` + trivial tonemap from the first lit mesh
  so breakout/asteroids goldens and web demos aren't re-blessed wholesale when
  18's stack lands at P7.

## Correction (priority, 2026-08-03)

**`crcbl-jobs` moves out of P8 into its own phase, P5B, ahead of P6–P8, and the
four existing samples adopt it there rather than waiting for a fifth.**

### Why it moves

The status section above already made the case and the phase table had not
caught up: horde's measurement is that **the render side carries ten thousand
and the tick does not** — CPU frame time flat at 0.096 ms → 0.120 ms from one
thousand to ten thousand enemies, against a tick of 14.66 ms spread and 84 ms
converged on a 16.67 ms budget. So P8's job half is worth the whole of that gap
and P7's GPU-driven work can return at most 0.7 % of a frame. Leaving the job
system behind two phases that cannot pay for themselves was the roadmap having
the dependency backwards.

### Why the samples adopt it in the same phase

Because the seam is the deliverable, not the pool. `docs/plan/21-jobs.md`'s
2026-08-03 correction records the measurement that decides this:
**`std::thread::spawn` compiles on `wasm32-unknown-unknown` and returns
`UNSUPPORTED_PLATFORM` at run time**, so every thread in the topology has to
start through a spawn seam rather than through `std::thread`. A seam with no
consumers is a guess; four games driving it is what proves the shape before
P6–P8 build on top of it. This is the same rule the rest of this file applies —
a seam is not frozen until two samples have used it — and here there are four
available.

### What P8 keeps

The physics half: batch queries at scale, sleeping and islands. It adopts the
pool P5B built rather than building one.

### The gate that is not ours to pass

Cross-origin isolation. Pages cannot set COOP/COEP, so `SharedArrayBuffer` — and
with it the shared-memory input ring — needs the `coi-serviceworker` shim. If
that cannot be had, the demos run single-threaded through the fallback and
native keeps the full topology. **P5B is designed to survive that outcome**,
which is why the seam and the fallback come before the worker backend.

## Correction (priority, 2026-08-04)

**The `crcbl-shell` Win32 and AppKit backends move out of P14 into their own
phase, P5C. P14 keeps the Metal and DX12 HAL backends.**

### Why they move

Topic 15 scheduled them for P14 on one argument: "before that they'd be
compile-verified-only anyway (gpur lesson: that's not support)". That argument
was about the _HAL_ — a GPU backend with no device to run on really is only
compiled. It does not transfer to the shell, and P0.6 is the evidence: the X11
backend was verified against a real X server in CI from its first slice, with no
renderer in the room, because a window system is reachable from a test process
that never draws anything. GitHub's `windows-latest` and `macos-latest` runners
are desktops. So the choice was never "wait for a GPU" — it was "leave two of
five backends untested for ten phases", which is the shape `open()` returning
`NoBackend` on both platforms has been reporting honestly all along.

### What this phase can prove, and what it cannot

Everything the seam is: lifecycle, both display modes, monitors and DPI, input,
clipboard, drag-drop — each against a real desktop, the same way
`tests/wayland_e2e.rs` and `tests/x11_e2e.rs` do it.

**Not** the sample-level passes. The Linux suites drive a running game and press
F11 at it, which needs a renderer; `crcbl-vk` reaches Windows through the Vulkan
loader but has no software device on either runner, and macOS has no Vulkan at
all — permanently, per the 2026-08-05 decision that Apple platforms are Metal
only, so this waits on `crcbl-mtl`. So the F11-at-a-running-game pass has no
equivalent here, and that is a coverage gap to state rather than to approximate
— it comes back at P14, when there is something to draw with.

### What P14 keeps

Metal and DX12, and the sample/editor bring-up on both platforms. It inherits a
shell that already works there instead of writing one. (The MoltenVK spike gate
it also kept was cancelled on 2026-08-05 — Apple platforms are Metal only.)

## Progress (P7's GPU-driven half, 2026-08-10)

**§3.3 is wired end to end and the frame draws through it.** The forward frame
is now `cull` → `draw-args` → `forward` → `tonemap`, with every barrier computed
by the render graph rather than written by hand. What landed, in order:

1. **Geometry and instance pools** (§3.1, §3.2) with timeline-gated upload.
2. **A GPU mesh table** — `{base_vertex, base_index, index_count, bounds}` per
   resident mesh, indexed by `GpuInstance::mesh`, which stopped being reserved.
3. **Frustum culling in a compute pass**, checked against a CPU reference cull
   that is the oracle rather than a second implementation nobody reads.
4. **Instance liveness** — `GpuInstance::flags` bit 0, so a removed slot stops
   being drawn instead of being culled on stale contents.
5. **Indirect draw generation**, per bucket rather than per instance, with both
   `GeometryPath` arms run on real hardware.

**Metal compute was the blocker and is fixed.** `crcbl-mtl` refused every
dispatch; `ComputePipelineDesc` now carries the workgroup size Metal needs at
the call, and the macOS CI job runs the dispatch and indirect-dispatch tests on
a real device.

Two things worth carrying forward, both in `docs/backlog.md`:

- **A golden is necessary and not sufficient at this stage.** Breaking
  `first_index` to zero left the cube golden bit-identical; only the argument
  readback caught it. Draw-generation changes need the arguments compared, not
  just the picture.
- **The cross-backend gate caught its first real divergence** — a pooled mesh at
  a non-zero base vertex rendered correctly on wgpu and corrupted on Vulkan from
  one source, because the four targets fold the base into `SV_VertexID`
  differently. The rule that came out of it (every draw passes zero for both
  bases) is in `mesh.slang`'s header with the measured table.

§3.2's material table has since landed with both halves — `MaterialTable` is the
SSBO, `GpuMaterial` is a row, and a row's `base_color_texture` selects a layer
of a `Texture2DArray` page the fragment stage samples. It is
`BindingModel::ArrayPages` on every backend; `Bindless` is unimplemented and
`docs/backlog.md` holds what is left of it.

Still owed by P7 **as of that date**: meshlet geometry as the primary path
(§3.5), QEM cluster LOD (topic 25), the bindless form of the material page, and
the P7B/P7C lighting work. Three of the four have since closed — meshlet
geometry draws through a real mesh pipeline rather than degrading and saying so,
QEM cluster LOD shipped with `crcbl-scene`'s simplifier, the cluster DAG and
runtime cut selection, and **P7B is complete**: spot and point shadows, SSAO,
SSR and irradiance probes are each built and each gated by a golden in
`crates/crcbl/tests/render_e2e.rs`. What remains is the bindless page and P7C.
(This line said "the lighting work" until 2026-08-23, contradicting the P7B row
in this file's own phase table.)

## Correction (capabilities, scope and networking, 2026-08-09)

Four decisions taken together. They interlock, so they are recorded together.

### 1. The two-valued renderer tier is replaced by device capabilities

`Tier A` / `Tier B` was written when the only implementations in view were
Vulkan and WebGPU, and it stopped describing reality: Metal has
multi-draw-indirect and no GPU-side count, D3D12 has both in the API and neither
written, `wgpu` on **native** reports very nearly the full native set — it is
not a Tier B backend and the plan was wrong to call it one — and only WebGPU in
a browser is the thing "Tier B" ever meant.

**[39-capabilities.md](39-capabilities.md) is now canonical.** `Features` and
`Limits` are the truth; `GeometryPath`, `BindingModel` and `LightingPath` are
derived selectors the renderer branches on; profile names are for humans and
nothing branches on them. `Features::TIER_A` and `RendererTier` are removed.

**A missing feature degrades by default; a game may declare one required**, and
then its absence is a named failure at device creation rather than a quietly
different picture. This lands at P7, while `RendererTier` is still consumed only
by log lines, `Debug` impls and one device request — after P7 it would not be
cheap.

### 2. Ray tracing and mesh shaders move into the MVP

Both were "extensions later, keep the HAL open to them". Both are now MVP:

- **Mesh shaders are the primary geometry path** (topic 3 §3.5). Every native
  backend has them and Slang emits them for all three. The indirect paths are
  what a device without them falls back to.
- **Ray-traced lighting is MVP, and so is a complete rasterised twin** (topic
  18). RT is **Vulkan and D3D12 only** — WebGPU has none, and Slang cannot yet
  emit ray tracing for Metal — so macOS, iOS and every browser run the raster
  path. That is not an edge case, it is most players, which is why the twin is
  MVP rather than a fallback.
- **QEM auto-LOD moves out of wave 1 into P7** (topic 25). Per-cluster selection
  needs a generated cluster hierarchy to select between; without it the meshlet
  path is the culling win without the detail win.

### 3. Multiplayer is LAN, web builds are single player

Native sessions are direct-connect by IP or found through a lobby browser over
local-network host discovery. **No hosted infrastructure exists anywhere in the
project.**

**WebTransport and WebSocket are removed from the plan.** Their only purpose was
browser clients reaching hosted servers, and there are none. The transport
surface is UDP and in-memory.

Browser multiplayer was examined rather than assumed absent, and the finding is
structural: a browser cannot listen on a socket, cannot discover hosts on a
local network, and cannot open an insecure connection to a LAN address from an
HTTPS page. WebRTC with manually exchanged connection codes is the one route
that survives the no-infrastructure constraint; it is **deferred, with its costs
recorded in `docs/backlog.md`**, not refused.

Consequences: towers co-op is native LAN rather than mixed native/browser; arena
is LAN, so prediction is validated against **injected** latency only; topic 27's
hosted tier 3 and `crcbl-mint` leave the plan.

### 4. Four samples added; two flagships build web slice first

lantern (13) and quarry (14) are acceptance fixtures for lighting and geometry —
every path proven correct before a game depends on it. shard (15) is an
MMO-style flagship and bracket (16) is matchmaking with no game attached.

**breach and shard each ship a reduced single-player web slice before their
native game exists**, because the browser runs the fallback paths and a fallback
proven after the fact is a fallback nobody proved. **breach's competitive game
is native only** — anti-cheat, raw mouse input and an unreliable channel are
things a browser cannot honestly provide.

Two new standing sample rules follow: every sample runs on every path its device
offers and reports which it took, and between them the samples cover every
engine feature. See
[sample/00-samples-overview.md](sample/00-samples-overview.md).

## Progress (test infrastructure and a re-measurement, 2026-08-10)

**The testing standard is written down and the backends follow it.** Naming and
placement had never been recorded, which is why six crates drifted from a
convention the other eighteen kept; `docs/plan/12-testing.md` now states the
rules and three of its own statements that the tree had already replaced with
better mechanisms have been retired. What changed in the tree:

- **`crcbl-mtl` and `crcbl-dx12` mark their device tests `#[ignore]`**, and both
  harnesses select `--run-ignored only`, so the count each guards on is the
  number of tests that touched a GPU rather than that plus arithmetic. Measured
  in CI: 70 on the Metal runner, 73 on the D3D12 one, against `crcbl-vk`'s 74 —
  three implementations of one seam, now comparable. Their tests stay in `src/`
  because moving them would mean widening two backends' public APIs to host
  tests; `crcbl-dx12` exports one item and `crcbl-mtl` two.
- **One shared guard reads nextest's summary.** Five of the eight harnesses read
  a cancelled `2/15 tests run` as a healthy fifteen. `tools/nextest-summary.sh`
  is now the only copy, and a `shell` CI job exercises it against synthetic
  cancelled, zero, absent and colour-wrapped summaries — the shapes a real suite
  cannot be asked to produce.
- **`render_e2e` covers all three scenes**, so `sprite.slang` and `ui.slang` are
  compared across every backend rather than only vk-versus-wgpu. Metal and D3D12
  drew both for the first time and matched a lavapipe-blessed reference at max
  channel delta 1 with no pixel over tolerance.
- **`crcbl-wgpu` honours the seam's third obligation.** It had no owner tagging
  at all, so a handle crossing devices was undefined on the backend the wasm
  build uses.

**A number the plan was reasoning from is stale.** `sample/03-horde.md` recorded
a converged ten thousand at 84 ms against a 16.67 ms budget and concluded P8 was
worth the whole gap. That table was taken **single-threaded**, and
`steer_enemies` went onto `par_for` afterwards without anything re-running it.
Re-measured varying only `--workers`: converged ten thousand goes 37.4 ms → 7.9
ms, a 4.71× speed-up, **inside budget**. The section's own prediction was "to
something like 6 ms if it scaled"; it scaled.

So **P8's headline claim for this sample is already met**, by `par_for` alone
and without the ECS schedule running systems in parallel. The two
single-threaded wins that section named as prerequisites — `overlap_sphere`
allocating, and no `body_mut` — are both taken. This does not retire P8, whose
batch-query and islands work stands on its own; it retires horde as the argument
for it, the same way the sample's own measurement retired horde as the argument
for P7.

## Sample→engine seams: what the samples still copy (2026-08-15)

**New work, added because the pattern keeps costing something.** The browser
entry point was copied five times before `a1f285e` moved it into
`crcbl::web_exports!`. While that was landing, `apps/hud` was found opening its
device without `PRESENT_FEEDBACK` or `PRESENT_TIMING` — a hand-copied capability
list that had drifted from `GpuContextDesc::default`, leaving the closed pacing
loop unreachable and `display_timing` answering `Unknown` forever. Four samples
carry a test against exactly that; hud had none, so it shipped the bug the test
exists to catch (fixed in `9d2b3f9`).

That is the cost this track is about: **a copy in every sample is a bug that can
be present in some of them.** Two surveys of `apps/*` produced the slices below.

### The slices, ranked

Five of the seven landed and are not restated here — `git log` has each. What
they became: `SpriteRenderer::register_baked`, `crcbl::impl_game_gpu!` with a
separate `impl_polled_gpu!`, `impl_polled_bundle!`, `Backing::platform` in
`crcbl-store`, and `crcbl::engine`'s `open_shell`, `DEFAULT_WINDOW_SIZE`,
`requested_window_size` and `log_first_configure`. Three lessons out of them are
worth more than the code and are kept where they can act:

- **rustc suppresses its own lints inside an external macro's expansion**, so
  moving hand-written forwards into a macro removed the only thing catching
  them: each forward is `Self::method(self)`, which resolves to the _trait_
  method when the bundle has no inherent one — infinite recursion rather than a
  compile error, and `unconditional_recursion` silent about it. Measured by
  deleting an inherent `counters`: it warns before the move and compiles clean
  after. Both macros therefore open with a `const _` block coercing each
  inherent method to a function pointer in a scope where the trait is not
  imported, so path syntax cannot reach the trait method and a missing one is
  `E0599`. This applies to any future forwarding macro.
- **A generated `desc` would have made five guard tests vacuous.** The hud bug
  that started this track came from a hand-copied `desc`, and the obvious fix
  was to generate it — but doing so makes the hud shape unrepresentable, which
  turns the five tests that assert against it into tests of the generator. With
  every sample guarded the safety argument was already spent, so
  `impl_polled_bundle!` takes `desc` by name and all five tests still run.
- **Read the `map_err` call sites, not the declaration.** A blocker recorded on
  this track said each game had its own error enum whose `NoWindowSystem`
  variant no generic bound could name. Every sample's error type is in fact a
  type alias for `crcbl::engine::LoopError<TheirGameError>`.

**`with_shell` and `open_the_window` stay.** `open_the_window`'s title, app id
and error type are the game's, and a wrapper taking all three needs six
positional arguments with two adjacent `&str`s among them. `with_shell` looks
like the others and is not: `apps/horde` builds its clock from
`!options.real_clock()` rather than from `headless`, and `apps/lantern` opens
its window through a different signature. Extracting it would need a callback
per difference.

The two that did not land:

- **`web_exports!`'s residue.** The impl half shipped as
  `crcbl::impl_web_pending!` — carrying the same `const _` coercion guard, and
  working only while `WebPending` is not imported in the sample, which is why
  the six lost that import and each says so. What is left is the ten literal
  symbol names, which need a proc macro that can build identifiers:
  `concat_idents!` is unstable and the names must stay per-sample so two demos
  in one browser cannot collide. **That means a new dependency, which is the
  repo owner's call** (see Decisions).

- **`crcbl_audio::CueDeck`** — the plumbing only: the stream-open-with-null-
  fallback, the unknown-id guard, the cue→`VoiceMix` conversion and the `plays`
  counter with its `id - 1` indexing, all repeated across four samples. **Not
  the sound design**, which is supposed to differ, and not the per-sample debug
  sections. **Blocked on the `CueGrammar` decision** below — build the seam
  first and it gets built around a parameter that is about to disappear.

### Decisions this needs, which are the owner's and not a refactor

- **`CueGrammar` on the `Mixer`.** Every call site passes
  `&CueGrammar::default()`, in the samples and in `crcbl-audio`'s own tests.
  **This entry used to add that nothing in the workspace ever constructs a
  non-default one, and that was never true**: `distance_rolloff_reaches_zero` in
  `crcbl-audio/src/spatial.rs` builds one with its own rolloff, and has since
  2026-07-31 — before this was written. It varies the grammar without varying it
  per call site, which is a weaker argument for moving it than the one that
  stood here, and the decision is the owner's on those terms. Moving it beside
  the listener collapses `cue(emitter, &grammar)` to `cue(emitter)`.
  Deliberately not taken when the listener moved, because "this mixer's grammar"
  is a bigger claim than "this mixer's listener". Slice 7 waits on it.
- **A proc-macro dependency for identifier concatenation**, for slice 6. Ten
  lines per sample against one new crate in the tree.
- **Does `Summary` keep re-flattening `RunSummary`?** Every sample re-declares
  the same eight fields and copies them one by one (~223 lines total), and drift
  is already visible in the doc comments. The engine went the _other_ way for
  arguments — `Common` is a field on each game's `Options` "so adding a shared
  flag reaches four games without touching four structs" — and `Summary` does
  the opposite with no stated reason. The counter-argument is real: flattening
  is what lets `main.rs` write `summary.frames` rather than
  `summary.run.frames`.

### Deliberately not extracted

Recorded so they are not re-proposed. Each was checked against the rule that DRY
is about duplicated _knowledge_, not duplicated shape.

- **The `DebugModule` impls.** Same shape, genuinely different numbers, and each
  sample's doc argues against sharing. This is the seam working.
- **Action sets and key bindings.** A game is entitled to rebind alone.
- **`horde/src/controls.rs`.** Its shared parts are _already_ engine —
  `TouchStick`, `PauseControl`, `CONTROL_STYLE`. What is left is horde's.
- **`HudStrings` and `draw_hud`.** The keys and strings are content. See the
  correction in `docs/backlog.md` about what its caching does and does not buy.
- **`fn still`** in three `art.rs` files — it returns a _game-local_ struct, so
  sharing it would put a per-sample type in the engine.

## Structured logging: the engine's own macros (2026-08-15)

**Done.** `tracing` was queued and then declined by the repo owner in favour of
a minimal implementation inside the engine, which is what shipped:
`crcbl_core`'s `error!`/`warn!`/`info!`/`debug!`/`trace!`, forwarding to one
`log_at!`, with a wall-clock banner at start-up and seconds-since-start on every
line — on the browser queue as well as on stderr.

**Two things the work found that the plan had wrong.**

The first question below — whether `tracing` would subsume `crcbl_core::trace` —
turned out not to arise, but its answer still holds and is why the profiler was
left alone: the trace ring is drained per frame into the debug panel, and the
log is read by a person afterwards.

**The dependency cannot be dropped, and the survey implied it could.** The
evidence given here was that `wgpu`, `naga` and `gpu-allocator` all report
through the `log` facade — but two of those three left with `crcbl-wgpu` on
2026-08-21 and are at zero occurrences in `Cargo.lock`. `naga` survives only as
`crcbl-shaders`' WGSL validator, so "a device that would not open" is no longer
a diagnostic anything here can emit. **The conclusion still holds on the
browser-queue argument below, which is the load-bearing one**; a reader
reopening this decision should weigh that and not the dead crates. So the sink
still implements `log::Log`, and the macros dispatch through `log::logger()`
rather than reaching for this crate's own static. That last part is not a style
choice: `wasm32` installs `crcbl::web`'s queue instead of the stderr sink, and a
version that bypassed the facade compiled everywhere while silently dropping
every engine log line in the browser. `web/tools/smoke.mjs` caught it.

What did change is that the backend and platform crates no longer name `log` at
all — `crcbl-dx12`, `crcbl-mtl`, `crcbl-render`, `crcbl-shell`, `crcbl-store`
and `crcbl-vk`, the list having also carried `crcbl-wgpu` until that crate was
deleted — and `crcbl::log` is the engine's module rather than the facade
re-exported — the call sites did not move, because the path is the same either
way.

The survey it was planned from follows.

### What logs today

The engine logs through the **`log` facade**, and `crcbl_core::log` supplies the
sink — a small `log::Log` writing to stderr, with `env_logger`-style filtering
read from `CRCBL_LOG` and longest-prefix directive matching. Its module doc
states the reason it is hand-written: "every framework that does more brings a
runtime, a feature matrix, and opinions about async." That argument is what this
slice has to answer, not ignore.

Call sites split three ways, and only the first is in scope:

- **`log::{trace,debug,info,warn,error}!` across `crates/` and `apps/`** — this
  is the migration target. `debug`, `info` and `warn` each carry roughly a
  hundred call sites; `error` is far fewer and `trace` is used once.
- **`println!`/`eprintln!` in `crcbl-cli` and the samples' `main.rs`** — this is
  **program output**, not logging, and it must stay exactly as it is. A CLI that
  routed its results through a log filter would have `CRCBL_LOG=off` silence the
  answer the user asked for.
- **`println!`/`eprintln!` in the e2e harnesses** — test progress, read by the
  harness scripts. Also out of scope.

### The three questions the slice has to answer first

1. ~~**Does `tracing` replace `crcbl_core::trace`, or sit beside it?**~~
   **Answered 2026-08-15: it sits beside it, and the boundary is what the output
   is read by.**

   The two look alike and are not the same kind of thing. `crcbl_core::trace` is
   a **profiler buffer**: a fixed-capacity per-thread ring
   (`RECORDS_PER_THREAD`, `MAX_TRACKS`) that `drain()`s a `Snapshot` **every
   frame, in-process**, into the debug panel and the headless summary —
   `crcbl::perf` is written around exactly that, down to "half a frame is not a
   frame" when a drain splits the frame span. It counts what it had to drop so a
   hole in the trace is _reported_ rather than silently absent, and it costs one
   relaxed atomic load when disabled.

   `tracing` is a **diagnostic facade**: a global subscriber, structured fields,
   filtering by target, output a human or a CI log reads after the fact.
   Reproducing the profiler inside it would mean writing that ring and the drop
   accounting again as a `Layer`, and being careful about allocation per span to
   keep the frame cost — which is building the thing that already exists, in a
   harder place.

   **So the rule is: read by the frame that produced it → `crcbl_core::trace`;
   read by a person or a log afterwards → `tracing`.** That keeps this slice to
   the `log::*` sites and leaves `crcbl::perf` untouched, which is also what
   makes it a slice rather than a rewrite.

2. **What happens to `crcbl_core::log::capture`?** It hands a test the records
   the engine emitted, so a log line that is the only evidence of a decision can
   be asserted on. It has real consumers across the workspace, and every one of
   them is a test that goes silently vacuous if the replacement subscriber
   collects nothing. Whatever replaces it must be shown to fail when the log
   line is removed.
3. **How far does the dependency reach?** `tracing` itself is small, but
   `EnvFilter` — the piece that would replace `CRCBL_LOG` parsing — lives in
   `tracing-subscriber` and brings more. Some crates here are deliberately
   dependency-light (`crcbl-sprite` builds with none at all), so "which crates
   get the facade" is a decision, not a sweep.

### What makes it cheap

`tracing` ships a `log` compatibility layer, so **existing `log::*` call sites
keep working while the sink moves**. That makes this a two-step slice with a
green tree in the middle: swap the subscriber first and prove the existing call
sites still arrive, then convert call sites where a span or structured fields
buy something. Converting all of them mechanically buys nothing on its own —
`tracing`'s value is in the fields and the spans, and a `tracing::info!` with
one interpolated string is a `log::info!` with a longer import.

### Not yet decided

Whether the browser build takes the same path. `crcbl_core::log` queues lines in
wasm for the page to drain, because `Instant::now` panics on
`wasm32-unknown-unknown`; the sink's timestamps use `Instant` today. Any
subscriber that stamps time needs the same treatment, and the browser gate is
what would catch it not having it.

## The WebGPU backend replaced `wgpu` (2026-08-15 → 2026-08-21)

The 530-line plan that stood here is spent and has been deleted rather than
annotated — `git log` has it. What it planned is in the tree: `crcbl-webgpu`
implements all four HAL traits, `crcbl-wgpu` was deleted in `6b5e17a`, `wgpu`
and `gpu-allocator` are at zero occurrences in `Cargo.lock`, `wasm-bindgen` and
`web-sys` reach the wasm32 graph from nowhere
(`cargo tree --target wasm32-unknown-unknown -i wasm-bindgen` prints nothing;
their only reverse dependency is `cpal`, which is native), `getrandom` was
replaced by `crcbl-rand`, and `web/tools/check-exports.mjs`'s
`ALLOWED_IMPORT_MODULES` is the empty set with the build enforcing it.

The gate the plan called its finish line runs on three operating systems:
`pages.yml`'s `render-harness` job renders all eleven `Scene` variants in a real
browser and compares each against the shared golden, then holds the same
readbacks against a _live_ native render through `--reference vk` and
`--reference mtl`. That is wider than what it replaced — the old cross-backend
script compared three scenes across two abstractions over one driver.

The one rule out of that plan that still binds future work moved into
`crates/crcbl/tests/render_e2e.rs`'s module docs, where the code it constrains
lives: **do not split the golden references per backend**, and why
`max_channel_delta` rather than a mean-error budget. What the plan got wrong,
and the three loose ends it left, are in `docs/backlog.md`.
