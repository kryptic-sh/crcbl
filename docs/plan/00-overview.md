# Crucible (`crcbl`) — Engine Plan Overview

Cross-platform GPU rendering/game engine in Rust, everything from scratch as a
learning exercise. Vulkan first (Linux), Metal and DX12 later, behind a hard
backend seam. Wasm/WebGPU arrives **early** — every sample publishes as a
browser demo on GitHub Pages. Server→client architecture from day one.
First-class pillars beyond rendering: from-scratch **physics** (galaxy-scale
sector-tiled space, simulator-grade dynamics, swept CCD), from-scratch **audio**
(learnable spatial cue grammar, esports-legible), **CLI/headless** control of
engine and editor, and **test infra** (unit + e2e per subsystem). 3D-first; 2D
is an orthographic projection with `z` as z-index.

> **Build order lives in [ROADMAP.md](ROADMAP.md)** — it interleaves component →
> system → sample slices across these docs and is canonical where orderings
> disagree. Stage/topic numbers below are document identity, not sequence.

## Locked decisions

| Decision     | Choice                                                                           | Rationale                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------ | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Vulkan layer | `ash` (raw Vulkan)                                                               | GPU-driven design needs bindless, indirect draws, fine-grained sync. wgpu abstracts these away.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Windowing    | From scratch (`crcbl-shell`, topic 15) — _bindings, not frameworks_              | Wayland/X11 protocol layer + codegen ours over libwayland-client/libxcb connections (the Vulkan WSI ABI requires real driver-visible objects — topic 15); own JS-shim canvas P5; Win32/AppKit at **P5C** (moved out of P14 on 2026-08-04). Platform-agnostic Shell trait; SurfaceTarget = only sanctioned leak. Two modes only: windowed (aspect-lockable) + borderless w/ render scale — the two modes exist in `crates/crcbl-shell`, but **render scale is unbuilt on both sides** (2026-08-27): `render_scale` appears nowhere under `crates/`, in the shell or the renderer. The `crcbl-shell` fractional scale is the compositor's HiDPI factor, a different quantity. See ROADMAP's known gaps. No exclusive fullscreen.           |
| GUI          | Own immediate-mode GUI, **CSS-subset styled**                                    | DOM-like blocks/spans, flexbox-subset layout, `.css` stylesheets with id/class selectors + hot reload. ALL engine UI (editor, debug tools, HUDs) built this way; editor and game share one draw path.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Math         | `glam`                                                                           | SIMD, ecosystem standard, no reason to hand-roll.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Scene format | Open sources (glTF/WAV/PNG + LFS), own cooked; scene = `.scn/` dir of RON chunks | **LOCKED.** Per-system chunk files: dirty-chunk saves (no full rewrite) + clean git diffs + conflict-free merges. Deterministic writer, stable IDs, journal autosave, `crcbl bake` for web. RON for entity data, TOML for flat config. **The TOML half is built and the RON half has no implementation** (2026-08-27): `crcbl-store` reads and writes `settings.toml` through the `toml` crate, while nothing in the workspace can read or write RON — no `ron` dependency, no hand-written reader, and not one `.ron` file in the tree. `crcbl-scene` says the `.scn/` directory arrives "when step 4 lands". Picking the RON implementation is an open decision in `docs/backlog.md`, and it also blocks `crcbl save dump` (topic 14). |
| ECS          | System-owned arrays                                                              | Systems track arrays of the objects belonging to them (SoA), not objects with components attached.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Networking   | Server-authoritative, transport seam                                             | In-memory channel for single player; real network transport is the same interface. Multiplayer is first-class.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Wasm target  | WebGPU through our own command stream                                            | Browser has no Vulkan. **`crcbl-webgpu` is the backend, and it is not a `wgpu` wrapper**: wasm serialises HAL calls into a buffer it owns and JS decodes and replays them against WebGPU (topic 41). This row named `wgpu` and `crcbl-wgpu` until 2026-08-23; that crate was deleted on 2026-08-21 and with it the native triage backend the row promised, so there is **no** portability fallback on native — `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` are the native set. Capability decides the path (topic 39). Web builds are single player; no browser networking.                                                                                                                                                                 |
| Physics      | From scratch (`crcbl-phys`), layered L0–L3                                       | First-class pillar. Sector-tiled `WorldPos` (galaxy scale), simulator-grade forces, CCD. f64, same-binary determinism.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Audio        | From scratch (`crcbl-audio`), stylized spatializer                               | First-class pillar, lands before first sample. Deterministic ITD/ILD/pitch/occlusion cue grammar — learnable player skill, not realistic HRTF. cpal/AudioWorklet at the device seam only.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| CLI/headless | `crcbl` binary drives everything                                                 | Editor edits are transport commands → CLI is just another client. Scriptable scenes, sims, screenshots; agent/CI-friendly.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Testing      | Unit + property + e2e per subsystem, same phase                                  | Golden images (lavapipe CI) + golden audio buffers + determinism hashes. No subsystem lands untested.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Persistence  | `crcbl-store`: saves = snapshots, settings = TOML                                | Save game = replication snapshot in versioned container (one serialization path, shared with play-mode restore + join-in-progress); layered settings; RON profiles; async `StorageSource` (platform dirs / OPFS wasm); atomic writes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Game modules | Wasm FFI (`crcbl-mod`, topic 16)                                                 | Game logic = wasm modules, any wasm language; engine owns all state (modules ~stateless → hot reload/saves/replication free); static+wasm dual binding, one API; wasmtime behind seam (browser: nested instantiate). **The decision is locked and none of it is built** (2026-08-27): the `crates/` tree has no `crcbl-mod` in it, and no manifest in the workspace depends on `wasmtime`. Every sample in `apps/` is a static Rust binary linked against `crcbl`, so nothing exercises the guest path yet.                                                                                                                                                                                                                              |

## Core design principles

1. **GPU-bound by default.** Same discipline as the infr Vulkan backend:
   anything that can run on the GPU runs on the GPU. Minimize host↔GPU round
   trips. Persistent mapped buffers, bindless descriptors, multi-draw-indirect,
   compute-driven culling. The CPU records a nearly constant-size command stream
   regardless of scene size.

   **Three of those four are native-only, and the browser gets none of them.**
   WebGPU has compute-driven culling and nothing else on that list: no binding
   arrays, no multi-draw-indirect or GPU-side count, no persistent mapping (a
   buffer is mapped for one transfer and unmapped again; the WebGPU spec has no
   always-mapped storage buffer), and no buffer device address anywhere. _This
   list named `wgpu`'s `MAPPABLE_PRIMARY_BUFFERS` feature flag until 2026-08-27;
   `wgpu` is not in the tree and its feature names are not our contract — the
   constraint is WebGPU's, and it is what `crcbl-webgpu` has to live inside._
   The principle is a **native** principle; what the browser runs is the
   degraded path, and which path a device takes is decided by capability rather
   than by platform — see [39-capabilities.md](39-capabilities.md).

2. **Backend seam is a trait boundary, not a compile flag.** `crcbl-hal` defines
   the contract; `crcbl-vk` is one implementation. Renderer code above the seam
   never names a Vulkan type.
3. **Server is the game.** The server owns simulation state and is
   authoritative. The client renders, predicts, and sends inputs. Single player
   = server + client in one process over an in-memory channel.
4. **Debug tools are load-bearing.** Debug draw, GPU timers, frame profiler, and
   entity inspector are built alongside each system, not bolted on.
5. **The editor is a game.** The scene editor is a client of the engine using
   the same renderer, GUI, ECS, and server loop as a shipped game.
6. **Physics is simulation, simulation is the server.** Physics systems are
   ordinary server-side ECS systems (SoA arrays, replicated results,
   deterministic per-tick hash). The world is sector-tiled from the core types
   up — galaxy-scale coordinates are foundational, not a retrofit.
7. **Nothing is GUI-only.** Every engine and editor capability works headless
   through the `crcbl` CLI — same command protocol the GUI uses. If it only
   works with a window, it's an architecture regression.
8. **Audio is information.** The spatializer is a deterministic cue grammar
   players can learn and exploit; sounds are replicated server events rendered
   client-side, exactly like graphics.
9. **Untested is unfinished.** Each subsystem ships unit + e2e coverage in the
   same roadmap phase; samples double as CI fixtures (determinism scripts,
   golden frames, golden audio).
10. **Game logic is a guest.** Gameplay code lives in wasm modules behind a flat
    FFI (any wasm language); the engine owns all state, so hot reload, saves,
    replication, and sandboxed modding come free. One API, two bindings: static
    for dev, wasm for shipping/mods.

## Stages

| Stage | Doc                                                      | Theme                                                          |
| ----- | -------------------------------------------------------- | -------------------------------------------------------------- |
| 1     | [01-foundations.md](01-foundations.md)                   | Workspace, crates, core types, HAL seam, window/event loop     |
| 2     | [02-vulkan-backend.md](02-vulkan-backend.md)             | Vulkan device, swapchain, render graph, first triangle         |
| 3     | [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md) | Bindless, geometry pools, indirect draws, GPU culling          |
| 4     | [04-ecs-server-client.md](04-ecs-server-client.md)       | ECS, tick loop, transport seam, replication                    |
| 5     | [05-physics.md](05-physics.md)                           | From-scratch physics: sector space, forces/orbits, CCD         |
| 6     | [06-assets-scenes.md](06-assets-scenes.md)               | glTF import, scene format, asset ids, hot reload               |
| 7     | [07-ui-debug.md](07-ui-debug.md)                         | Immediate-mode GUI, debug draw, profiler, inspector            |
| 8     | [08-editor.md](08-editor.md)                             | Scene editor built on the engine, gizmos, play-in-editor       |
| 9     | [09-backends-metal-dx12.md](09-backends-metal-dx12.md)   | Metal and DX12 implementations of the HAL — **deferred**       |
| 10    | [10-wasm-webgpu.md](10-wasm-webgpu.md)                   | Wasm target: WebGPU backend, browser platform, capability gaps |

> **Stage 9 is deferred as of 2026-08-21.** `crcbl-mtl` and `crcbl-dx12` stay in
> the tree and their CI jobs keep running and passing, but no new backend work
> is done on them. Development is on `crcbl-vk` and `crcbl-webgpu`, which are
> the two backends that can be exercised in full both locally and in CI — Vulkan
> against real hardware here and lavapipe on Linux and Windows, WebGPU in a real
> browser on all three runners. See
> [09-backends-metal-dx12.md](09-backends-metal-dx12.md) for what that parks and
> what it costs.

Cross-cutting topic docs (identity, no ordering implied):

| Topic | Doc                                                | Theme                                                                     |
| ----- | -------------------------------------------------- | ------------------------------------------------------------------------- |
| 11    | [11-cli-headless.md](11-cli-headless.md)           | `crcbl` CLI: headless engine/editor control, scripting                    |
| 12    | [12-testing.md](12-testing.md)                     | Test infra: unit/property/e2e, golden images, determinism                 |
| 13    | [13-audio.md](13-audio.md)                         | Spatial cue grammar, mixer, occlusion, audio testing                      |
| 14    | [14-persistence.md](14-persistence.md)             | Save games (snapshot-based), settings layers, profiles                    |
| 15    | [15-windowing.md](15-windowing.md)                 | Own windowing: wire-protocol backends, 2 modes, agnostic seam             |
| 16    | [16-wasm-modules.md](16-wasm-modules.md)           | Game logic as wasm modules: FFI ABI, any language, modding                |
| 17    | [17-animation.md](17-animation.md)                 | Skeletal animation: cooked clips, state machine, GPU skinning             |
| 18    | [18-render-features.md](18-render-features.md)     | Render features index: how the techniques interact, deliver and risk      |
| 19    | [19-input.md](19-input.md)                         | Device-agnostic action input: kb/mouse/pad/touch, one config              |
| 20    | [20-particles.md](20-particles.md)                 | GPU-resident particles/VFX: compute sim, RON effects, workbench           |
| 21    | [21-jobs.md](21-jobs.md)                           | Threading: pipeline threads + job pool, mailboxes, tick sync              |
| 22    | [22-replay.md](22-replay.md)                       | State recording: replays, black-box debug, spectating                     |
| 23    | [23-netcode.md](23-netcode.md)                     | Transports (UDP + own reliability, LAN discovery), protocol foundations   |
| 24    | [24-navigation.md](24-navigation.md)               | Navmesh gen (Recast-lineage, sector-tiled), A\*+funnel, crowds            |
| 25    | [25-lod.md](25-lod.md)                             | LOD: hand-first + QEM auto fallback, GPU selection in cull pass           |
| 26    | [26-prediction.md](26-prediction.md)               | Client prediction/rollback + query-only lag comp, fairness harness        |
| 27    | [27-auth.md](27-auth.md)                           | Trust tiers (open/PSK/token), identity, ranked chain, crcbl-mint          |
| 28    | [28-ballistics.md](28-ballistics.md)               | Penetrating sweeps: material energy loss, ricochet, media drag            |
| 29    | [29-fp-rendering.md](29-fp-rendering.md)           | First-person: viewmodel pass, ADS cameras, PiP optics, kill-cam POV       |
| 30    | [30-player-kit.md](30-player-kit.md)               | Optional player kit: predicted movement, GTA-style 3P cam, 1P binding     |
| 31    | [31-vis-culling.md](31-vis-culling.md)             | Optional anti-wallhack: PVS + ray envelopes, leak auditor                 |
| 32    | [32-voip.md](32-voip.md)                           | Voice: team/direct + proximity, Opus, gate-safe (no positions)            |
| 33    | [33-decals.md](33-decals.md)                       | Decals: projected/parallax/carve-volume tiers, impact + decoration        |
| 34    | [34-inventory.md](34-inventory.md)                 | UI drag-drop + optional grid-inventory kit (looting, slots)               |
| 35    | [35-ragdolls.md](35-ragdolls.md)                   | Ragdolls: server settles / client performs, anim→physics handoff          |
| 36    | [36-contact-solver.md](36-contact-solver.md)       | Physics L2/L3: substepped impulses, islands, sleeping, joints             |
| 37    | [37-materials.md](37-materials.md)                 | Material authoring: templates+instances, render↔surface link, lint        |
| 38    | [38-weapons.md](38-weapons.md)                     | Weapon kit: attachments, server-authoritative fire, recoil patterns       |
| 39    | [39-capabilities.md](39-capabilities.md)           | Device capabilities, graceful degradation, path selectors, feature matrix |
| 40    | [40-profiling.md](40-profiling.md)                 | Profiling, benchmarking: CPU/GPU spans, counters, trace export, perf rows |
| 41    | [41-webgpu-stream.md](41-webgpu-stream.md)         | The encoding `crcbl-webgpu` speaks — slice 2 of stage 10's WebGPU track   |
| 42    | [42-steam.md](42-steam.md)                         | Steamworks (`crcbl-steam`): hand-written FFI over the SDK's flat C API    |
| 43    | [43-render-standards.md](43-render-standards.md)   | What a current engine ships and where this one stands — the gap survey    |
| 44    | [44-lighting.md](44-lighting.md)                   | Lighting: the two paths, the light list, clustered forward, the BRDF      |
| 45    | [45-shadows.md](45-shadows.md)                     | Shadows: cascades, atlas tiles, bias, and the filter quality ladder       |
| 46    | [46-ambient-occlusion.md](46-ambient-occlusion.md) | Ambient occlusion: SSAO, its depth-weighted blur, and GTAO next           |
| 47    | [47-reflections.md](47-reflections.md)             | Screen-space reflections: the Hi-Z march, roughness, determinism          |
| 48    | [48-post-processing.md](48-post-processing.md)     | The post stack: pipeline order, HDR, tonemap, bloom, where toggles live   |
| 49    | [49-antialiasing.md](49-antialiasing.md)           | Antialiasing: FXAA, SMAA 1x, TAA's blocker, and MSAA repriced             |
| 50    | [50-irradiance-probes.md](50-irradiance-probes.md) | Irradiance probes: the static L1 grid, the diffuse GI twin                |

Sequencing is the [ROADMAP](ROADMAP.md)'s job: phases P0–P4A build the full
engine base (window → render → sim → physics slice → UI slice → audio) before
the first sample; wasm + the GitHub Pages demo site land immediately after the
first sample (P5). Stages 1–8 + topics 11–14 slices are the MVP; Metal/DX12 (9)
complete cross-platform. Stage 10's constraints (the browser's reduced
capability set, async assets, message-shaped transport, `tick(dt)` loop) are
baked in from the start — wasm is a first-class target, not a port.

Each roadmap S-phase is proven by a **sample project** — small complete
games/tools in `apps/`, numbered in build order: see
[sample/00-samples-overview.md](sample/00-samples-overview.md), which is the
canonical list and now runs to sixteen documents (breakout, asteroids, horde,
hud, viewer, orbit, towers, arena, puppet, sparks, breach, flappy, lantern,
quarry, shard, bracket).

**What is in `apps/` today, read from the tree on 2026-08-27**, is fourteen of
those sixteen — `breakout`, `asteroids`, `flappy`, `horde`, `hud`, `lantern`,
`viewer`, `orbit`, `puppet`, `sparks`, `quarry`, `shard`, `breach` and `bracket`
— beside three that are not samples at all: `apps/bare` (the engine used as a
plain library, loop hand-written), `apps/sandbox` (the dev playground the first
window lived in) and `apps/render-harness` (drives the backend-agnostic golden
scenes through a browser). The two still missing are **towers** and **arena**,
which is worth knowing because exit criteria all over these documents name
towers as the thing that proves them. _This paragraph said `sim` and listed six
samples, until 2026-08-27. That app was real — a headless simulation harness —
and was deleted on 2026-08-23; the CLI verb `crcbl sim` outlived it and is what
that work is reached through now._

Every sample ships as a browser demo on the Pages site: `web/demos/` holds
fourteen, one for each sample app above, with a page apiece under `web/pages/`.
`bare` and `sandbox` have no browser presence at all; `render-harness` has one
that is not a demo — `web/harness/`, driven by `web/run-render-harness-e2e.sh`.

## MVP feature → stage map

| MVP feature                             | Doc(s)  |
| --------------------------------------- | ------- |
| Render engine                           | 2, 3    |
| Physics (L0+L1+CCD, sector space)       | 1, 5    |
| Audio (spatial cue grammar + mixing)    | 13      |
| CLI/headless engine + editor control    | 11      |
| Test infra (unit + e2e everywhere)      | 12      |
| Persistence (saves/settings/profiles)   | 14      |
| Own windowing (2 modes, render scale)   | 15      |
| Game modules (API + wasm host)          | 16      |
| Shadows + post stack (HDR/tonemap/FXAA) | 18      |
| Action-based input (kb/mouse in MVP)    | 19      |
| Wasm target + Pages demo site           | 10      |
| Scene loader                            | 6       |
| ECS (system-owned arrays)               | 4       |
| Scene editor                            | 8       |
| Immediate-mode GUI (editor + game)      | 7       |
| Editor built on the engine              | 8       |
| Debug tools throughout                  | 2–8, 13 |
| Server→client from day 1                | 1, 4    |
| 3D-first, 2D as ortho projection        | 2, 3    |

## Out of MVP scope (explicitly)

- Skeletal animation: fully designed (topic 17), scheduled post-MVP wave 1 with
  the puppet sample (09) as forcing function & acceptance test. **It started
  early and is partly built** (2026-08-27): `crcbl-anim` ships `Skeleton`,
  `Clip`, `Pose`, `Palette` and `BlendSpace1d`, and `apps/puppet` blends a
  locomotion set by speed. Its own crate docs draw the line — no state machine,
  no root motion, no GPU skinning — so what is out of MVP scope is now the upper
  half of topic 17, not all of it.
- Scripting-as-text: game logic is wasm modules (topic 16); Lua VM template
  covers script-style workflows post-MVP.
- Audio: reverb zones, portal/room-graph propagation, doppler, surround — the
  cue grammar (incl. occlusion) and mixing are MVP; see
  [13-audio.md](13-audio.md).
- Physics L2 contact solver is MVP-stretch (non-gating); L2 + L3 joints get a
  full design in [36-contact-solver.md](36-contact-solver.md) and land wave 2
  (ragdolls, 35, are their flagship consumer). L3 constraints/joints are
  post-MVP — see the layer table in [05-physics.md](05-physics.md). L0/L1/CCD
  are MVP.
- Real network transport for native (QUIC/UDP) — the seam exists from stage 4;
  single player over the in-memory transport exercises the whole path. Native
  sessions are LAN over UDP (P13); browsers have no network transport at all —
  see topic 23's LAN correction.
- ~~Ray tracing, mesh shaders (extensions later; keep the HAL open to them).~~
  **Both moved into the MVP on 2026-08-09** — see the ROADMAP correction. Mesh
  shaders are the primary geometry path (topic 3 §3.5); ray-traced lighting is
  MVP alongside a complete rasterised twin (topic 18), and is Vulkan and D3D12
  only.
- Xbox/console targets — the only unique DX12 value; stage 9 covers desktop
  Windows via DX12 and macOS via Metal.

## Repo conventions

- Crate/dir/repo name `crcbl`; "Crucible" is the display name (README only).
- Cargo workspace, crates under `crates/`, apps under `apps/`.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, `cargo test`
  green at every stage exit.
