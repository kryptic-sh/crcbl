# Crucible (`crcbl`) — Engine Plan Overview

Cross-platform GPU rendering/game engine in Rust. Vulkan first (Linux), Metal
and DX12 later, behind a hard backend seam. Wasm/WebGPU is a supported target —
its constraints shape earlier stages (renderer tiers, async IO, transport seam).
Server→client architecture from day one. From-scratch physics is a first-class
pillar: galaxy-scale sector-tiled space, simulator-grade dynamics, swept
collision (CCD). 3D-first; 2D is an orthographic projection with `z` as z-index.

## Locked decisions

| Decision     | Choice                                               | Rationale                                                                                                              |
| ------------ | ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Vulkan layer | `ash` (raw Vulkan)                                   | GPU-driven design needs bindless, indirect draws, fine-grained sync. wgpu abstracts these away.                        |
| Windowing    | `winit`                                              | Proven cross-platform; needed for Metal/DX12 stages anyway.                                                            |
| GUI          | Own immediate-mode GUI                               | Editor is built on the engine; game GUI and editor GUI share one draw path.                                            |
| Math         | `glam`                                               | SIMD, ecosystem standard, no reason to hand-roll.                                                                      |
| Scene format | glTF 2.0 for meshes/materials; own format for scenes | **Revisitable** — flagged for discussion. glTF in from any DCC; engine-native scene format maps directly onto ECS.     |
| ECS          | System-owned arrays                                  | Systems track arrays of the objects belonging to them (SoA), not objects with components attached.                     |
| Networking   | Server-authoritative, transport seam                 | In-memory channel for single player; real network transport is the same interface. Multiplayer is first-class.         |
| Wasm target  | WebGPU via `wgpu` as portability backend (Tier B)    | Browser has no Vulkan. `crcbl-wgpu` covers wasm + doubles as native fallback tier. Perf tier stays ash/mtl/dx12.       |
| Physics      | From scratch (`crcbl-phys`), layered L0–L3           | First-class pillar. Sector-tiled `WorldPos` (galaxy scale), simulator-grade forces, CCD. f64, same-binary determinism. |

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

Stages 1–8 are the MVP. Stages 9–10 make it cross-platform. Ordering within a
stage is suggested, not sacred; stages themselves are dependency-ordered
(physics slots after ECS because physics systems _are_ server systems; stages
6–8 may interleave with physics L2 stretch work). Stage 10's constraints
(renderer Tier B, async assets, message-shaped transport, `tick(dt)` loop) are
baked into stages 1/3/4/6 — wasm is a first-class target, not a port.

Each stage exit is proven by a **sample project** — small complete games/tools
in `apps/`, laddered against the stages: see
[sample/00-samples-overview.md](sample/00-samples-overview.md) (breakout,
asteroids, viewer, horde, orbit, flagship co-op tower defense, post-MVP arena).

## MVP feature → stage map

| MVP feature                        | Stage(s) |
| ---------------------------------- | -------- |
| Render engine                      | 2, 3     |
| Physics (L0+L1+CCD, sector space)  | 1, 5     |
| Scene loader                       | 6        |
| ECS (system-owned arrays)          | 4        |
| Scene editor                       | 8        |
| Immediate-mode GUI (editor + game) | 7        |
| Editor built on the engine         | 8        |
| Debug tools throughout             | 2–8      |
| Server→client from day 1           | 1, 4     |
| 3D-first, 2D as ortho projection   | 2, 3     |

## Out of MVP scope (explicitly)

- Audio, animation blending/state machines, scripting.
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
