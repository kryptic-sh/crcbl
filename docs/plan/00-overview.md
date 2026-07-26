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

| Decision     | Choice                                                                           | Rationale                                                                                                                                                                                                                                                                                 |
| ------------ | -------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Vulkan layer | `ash` (raw Vulkan)                                                               | GPU-driven design needs bindless, indirect draws, fine-grained sync. wgpu abstracts these away.                                                                                                                                                                                           |
| Windowing    | From scratch (`crcbl-shell`, topic 15), zero 3rd-party                           | Own wire-protocol Wayland+X11 clients at P0 (wayr donor), own JS-shim canvas P5, hand-rolled Win32/AppKit FFI P14. Platform-agnostic Shell trait; SurfaceTarget = only sanctioned leak. Two modes only: windowed (aspect-lockable) + borderless w/ render scale. No exclusive fullscreen. |
| GUI          | Own immediate-mode GUI, **CSS-subset styled**                                    | DOM-like blocks/spans, flexbox-subset layout, `.css` stylesheets with id/class selectors + hot reload. ALL engine UI (editor, debug tools, HUDs) built this way; editor and game share one draw path.                                                                                     |
| Math         | `glam`                                                                           | SIMD, ecosystem standard, no reason to hand-roll.                                                                                                                                                                                                                                         |
| Scene format | Open sources (glTF/WAV/PNG + LFS), own cooked; scene = `.scn/` dir of RON chunks | **LOCKED.** Per-system chunk files: dirty-chunk saves (no full rewrite) + clean git diffs + conflict-free merges. Deterministic writer, stable IDs, journal autosave, `crcbl bake` for web. RON for entity data, TOML for flat config.                                                    |
| ECS          | System-owned arrays                                                              | Systems track arrays of the objects belonging to them (SoA), not objects with components attached.                                                                                                                                                                                        |
| Networking   | Server-authoritative, transport seam                                             | In-memory channel for single player; real network transport is the same interface. Multiplayer is first-class.                                                                                                                                                                            |
| Wasm target  | WebGPU via `wgpu` as portability backend (Tier B)                                | Browser has no Vulkan. `crcbl-wgpu` covers wasm + doubles as native fallback tier. Perf tier stays ash/mtl/dx12.                                                                                                                                                                          |
| Physics      | From scratch (`crcbl-phys`), layered L0–L3                                       | First-class pillar. Sector-tiled `WorldPos` (galaxy scale), simulator-grade forces, CCD. f64, same-binary determinism.                                                                                                                                                                    |
| Audio        | From scratch (`crcbl-audio`), stylized spatializer                               | First-class pillar, lands before first sample. Deterministic ITD/ILD/pitch/occlusion cue grammar — learnable player skill, not realistic HRTF. cpal/AudioWorklet at the device seam only.                                                                                                 |
| CLI/headless | `crcbl` binary drives everything                                                 | Editor edits are transport commands → CLI is just another client. Scriptable scenes, sims, screenshots; agent/CI-friendly.                                                                                                                                                                |
| Testing      | Unit + property + e2e per subsystem, same phase                                  | Golden images (lavapipe CI) + golden audio buffers + determinism hashes. No subsystem lands untested.                                                                                                                                                                                     |
| Persistence  | `crcbl-store`: saves = snapshots, settings = TOML                                | Save game = replication snapshot in versioned container (one serialization path, shared with play-mode restore + join-in-progress); layered settings; RON profiles; async `StorageSource` (platform dirs / OPFS wasm); atomic writes.                                                     |
| Game modules | Wasm FFI (`crcbl-mod`, topic 16)                                                 | Game logic = wasm modules, any wasm language; engine owns all state (modules ~stateless → hot reload/saves/replication free); static+wasm dual binding, one API; wasmtime behind seam (browser: nested instantiate).                                                                      |

## Core design principles

1. **GPU-bound by default.** Same discipline as the infr Vulkan backend:
   anything that can run on the GPU runs on the GPU. Minimize host↔GPU round
   trips. Persistent mapped buffers, bindless descriptors, multi-draw-indirect,
   compute-driven culling. The CPU records a nearly constant-size command stream
   regardless of scene size.
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

| Stage | Doc                                                      | Theme                                                      |
| ----- | -------------------------------------------------------- | ---------------------------------------------------------- |
| 1     | [01-foundations.md](01-foundations.md)                   | Workspace, crates, core types, HAL seam, window/event loop |
| 2     | [02-vulkan-backend.md](02-vulkan-backend.md)             | Vulkan device, swapchain, render graph, first triangle     |
| 3     | [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md) | Bindless, geometry pools, indirect draws, GPU culling      |
| 4     | [04-ecs-server-client.md](04-ecs-server-client.md)       | ECS, tick loop, transport seam, replication                |
| 5     | [05-physics.md](05-physics.md)                           | From-scratch physics: sector space, forces/orbits, CCD     |
| 6     | [06-assets-scenes.md](06-assets-scenes.md)               | glTF import, scene format, asset ids, hot reload           |
| 7     | [07-ui-debug.md](07-ui-debug.md)                         | Immediate-mode GUI, debug draw, profiler, inspector        |
| 8     | [08-editor.md](08-editor.md)                             | Scene editor built on the engine, gizmos, play-in-editor   |
| 9     | [09-backends-metal-dx12.md](09-backends-metal-dx12.md)   | Metal and DX12 implementations of the HAL                  |
| 10    | [10-wasm-webgpu.md](10-wasm-webgpu.md)                   | Wasm target: wgpu backend, WebTransport, browser platform  |

Cross-cutting topic docs (identity, no ordering implied):

| Topic | Doc                                            | Theme                                                                   |
| ----- | ---------------------------------------------- | ----------------------------------------------------------------------- |
| 11    | [11-cli-headless.md](11-cli-headless.md)       | `crcbl` CLI: headless engine/editor control, scripting                  |
| 12    | [12-testing.md](12-testing.md)                 | Test infra: unit/property/e2e, golden images, determinism               |
| 13    | [13-audio.md](13-audio.md)                     | Spatial cue grammar, mixer, occlusion, audio testing                    |
| 14    | [14-persistence.md](14-persistence.md)         | Save games (snapshot-based), settings layers, profiles                  |
| 15    | [15-windowing.md](15-windowing.md)             | Own windowing: wire-protocol backends, 2 modes, agnostic seam           |
| 16    | [16-wasm-modules.md](16-wasm-modules.md)       | Game logic as wasm modules: FFI ABI, any language, modding              |
| 17    | [17-animation.md](17-animation.md)             | Skeletal animation: cooked clips, state machine, GPU skinning           |
| 18    | [18-render-features.md](18-render-features.md) | Shadows (CSM) + post stack: HDR, tonemap, FXAA, bloom                   |
| 19    | [19-input.md](19-input.md)                     | Device-agnostic action input: kb/mouse/pad/touch, one config            |
| 20    | [20-particles.md](20-particles.md)             | GPU-resident particles/VFX: compute sim, RON effects, workbench         |
| 21    | [21-jobs.md](21-jobs.md)                       | Threading: pipeline threads + job pool, mailboxes, tick sync            |
| 22    | [22-replay.md](22-replay.md)                   | State recording: replays, black-box debug, spectating                   |
| 23    | [23-netcode.md](23-netcode.md)                 | Transports (UDP+own reliability, WebTransport/WS), protocol foundations |
| 24    | [24-navigation.md](24-navigation.md)           | Navmesh gen (Recast-lineage, sector-tiled), A\*+funnel, crowds          |
| 25    | [25-lod.md](25-lod.md)                         | LOD: hand-first + QEM auto fallback, GPU selection in cull pass         |
| 26    | [26-prediction.md](26-prediction.md)           | Client prediction/rollback + query-only lag comp, fairness harness      |

Sequencing is the [ROADMAP](ROADMAP.md)'s job: phases P0–P4A build the full
engine base (window → render → sim → physics slice → UI slice → audio) before
the first sample; wasm + the GitHub Pages demo site land immediately after the
first sample (P5). Stages 1–8 + topics 11–14 slices are the MVP; Metal/DX12 (9)
and the WebTransport half of 10 complete cross-platform. Stage 10's constraints
(renderer Tier B, async assets, message-shaped transport, `tick(dt)` loop) are
baked in from the start — wasm is a first-class target, not a port.

Each roadmap S-phase is proven by a **sample project** — small complete
games/tools in `apps/`, numbered in build order: see
[sample/00-samples-overview.md](sample/00-samples-overview.md) (breakout,
asteroids, horde, viewer, orbit, flagship co-op tower defense, post-MVP arena).
Every game sample ships as a browser demo on the Pages site.

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
  the puppet sample (09) as forcing function & acceptance test.
- Scripting-as-text: game logic is wasm modules (topic 16); Lua VM template
  covers script-style workflows post-MVP.
- Audio: reverb zones, portal/room-graph propagation, doppler, surround — the
  cue grammar (incl. occlusion) and mixing are MVP; see
  [13-audio.md](13-audio.md).
- Physics L2 contact solver is stretch (non-gating); L3 constraints/joints are
  post-MVP — see the layer table in [05-physics.md](05-physics.md). L0/L1/CCD
  are MVP.
- Real network transport for native (QUIC/UDP) — the seam exists from stage 4;
  single player over the in-memory transport exercises the whole path. Stage 10
  ships WebTransport/WebSocket for the browser, which becomes the protocol base
  for native QUIC later.
- Ray tracing, mesh shaders (extensions later; keep the HAL open to them).
- Xbox/console targets — the only unique DX12 value; stage 9 covers desktop
  Windows via DX12 and macOS via Metal.

## Repo conventions

- Crate/dir/repo name `crcbl`; "Crucible" is the display name (README only).
- Cargo workspace, crates under `crates/`, apps under `apps/`.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, `cargo test`
  green at every stage exit.
