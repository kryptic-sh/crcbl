# Stage 1 — Foundations

Workspace layout, core crates, the HAL seam, and a window with an event loop.
Nothing draws yet; everything after this stage has a place to live.

## Goals

- Cargo workspace with the full crate skeleton so later stages add code, not
  structure.
- `crcbl-hal` trait surface defined well enough that stage 2 (Vulkan) and stage
  9 (Metal/DX12) implement the same contract.
- Window + event loop + swapchain-ready surface handle on Linux.
- CI: fmt, clippy `-D warnings`, tests on Linux from day one.

## Workspace layout

```
crcbl/
├── Cargo.toml              # workspace root
├── crates/
│   ├── crcbl-core/         # ids, handles, arenas, slotmaps, time, logging
│   ├── crcbl-hal/          # backend seam: traits + POD descriptors only
│   ├── crcbl-vk/           # stage 2: ash implementation of the HAL
│   ├── crcbl-render/       # render graph, frame loop, meshes, materials
│   ├── crcbl-ecs/          # stage 4: system-owned arrays
│   ├── crcbl-net/          # stage 4: transport seam, replication
│   ├── crcbl-phys/         # stage 5: physics — queries, forces, CCD
│   ├── crcbl-scene/        # stage 6: scene format, glTF import
│   ├── crcbl-ui/           # stage 7: immediate-mode GUI
│   ├── crcbl-audio/        # topic 13: mixer + spatial cue grammar
│   ├── crcbl-store/        # topic 14: saves, settings, profiles
│   ├── crcbl-cli/          # topic 11: `crcbl` binary — headless control
│   └── crcbl/              # umbrella: re-exports, engine setup helpers
├── apps/
│   ├── sandbox/            # dev playground, first window lives here
│   └── editor/             # stage 8
└── docs/plan/
```

Empty crates are created in this stage with only their public seam types where
those are already known (`crcbl-hal` especially). Don't stub speculative APIs
elsewhere — an empty `lib.rs` is fine.

## Tasks

### 1.1 Workspace + tooling

- Workspace `Cargo.toml`, shared `[workspace.dependencies]` (glam, winit, ash,
  thiserror, log).
- `rustfmt.toml`, `deny.toml` (match gpur conventions),
  `.github/workflows/ci.yml` running fmt + clippy + test on Linux.
- Workspace lints: `unsafe_op_in_unsafe_fn`, `missing_debug_implementations`
  where sane. `crcbl-vk` is the only crate expected to hold nontrivial unsafe.

### 1.2 crcbl-core

- `Handle<T>`: 32-bit index + 32-bit generation, typed. Slotmap-style arena
  (`Pool<T>`) that recycles slots and invalidates stale handles.
- **`WorldPos` sector-tiled position** (physics pillar, foundational):
  `{ sector: IVec3, local: DVec3 }` — sparse 3D sector grid, f64 local offset,
  exact rebase on sector crossing. All simulation positions use this from day
  one; plain `Vec3` is only ever camera-relative render space. Retrofitting
  galaxy-scale coordinates is a rewrite — so they land here, in stage 1, even
  though physics proper is stage 5.
- Frame-scoped bump allocator for per-frame transient data.
- `Instant`-based frame clock: fixed-timestep accumulator (server tick) +
  variable render dt, since stage 4 needs the split and the loop shape should
  exist before code grows around a naive loop.
- Logging setup (`log` + env-filter style init).

### 1.3 crcbl-hal — the backend seam

Define the trait surface. Shape it like Vulkan (the lowest common denominator of
vk/mtl/dx12 is "Vulkan-flavored"): explicit passes, explicit sync at the graph
level, bindless-capable descriptor model.

Core objects (traits or handle-based, decided here):

- `Instance` → `Adapter` enumeration → `Device` + `Queue`.
- `Surface` + `Swapchain` (created from a raw-window-handle).
- Resources: `Buffer`, `Image`, `Sampler` — created from POD descriptor structs
  (`BufferDesc { size, usage, memory }`).
- `ShaderModule` (SPIR-V in; Metal/DX12 backends consume SPIR-V via
  cross-compilation — see stage 9).
- `Pipeline` (graphics + compute) from POD state descriptions.
- `CommandEncoder`: render pass scope, compute scope, copies, `draw_indirect` /
  `draw_indexed_indirect` / `dispatch_indirect` from day one — GPU-driven
  rendering is the point, indirect is not an afterthought.
- Timestamp queries (debug principle: profiling hooks in the seam itself).

Explicitly **not** in the HAL: render graph, frame pacing, materials. Those live
in `crcbl-render`, above the seam.

Deliverable check: a `NullBackend` (no-op impl) in `crcbl-hal` tests proving the
seam compiles as a trait object / generic and nothing leaks backend types.

### 1.4 Window + event loop

- `apps/sandbox`: winit window, event loop, raw-window-handle plumbed to where
  the HAL surface will be created.
- Input event normalization into engine types (`crcbl-core::input`): keyboard,
  mouse, resize, DPI. winit types stop at the app boundary.

## Exit criteria

- `cargo build --workspace` + clippy + fmt green in CI.
- Sandbox opens a window on Linux/Wayland and X11, handles resize + close.
- `crcbl-hal` seam reviewed against both the Vulkan plan (stage 2) and a skim of
  Metal/DX12 docs — no obviously vk-only concept in the trait names.
- `NullBackend` test passes.

## Risks

- **Over-designing the HAL before the Vulkan impl exists.** Mitigation: the seam
  is allowed to change during stage 2; it freezes at stage 2 exit, not stage 1
  exit.
- **winit API churn.** Pin the version workspace-wide; upgrade deliberately.
