# Stage 9 — Metal + DX12 Backends

Implement the frozen HAL on Metal (macOS) and DX12 (Windows). The renderer, ECS,
UI, and editor don't change — that's the point of the seam. Vulkan also runs
natively on Windows, so DX12 is there for the Xbox door and for first-class
Windows GPU debugging — **not** old-iGPU coverage; see the 2026-07-27 correction
below, which retracts the original justification. Metal is the only path on
macOS.

> **Capability correction, 2026-08-09.** The "tier flags audit" below asserts
> both backends are Tier A. That framing is superseded by
> [39-capabilities.md](39-capabilities.md): there are no tiers, a backend
> reports what the device has, and the renderer picks a path from that. Two
> specifics this document got wrong are recorded there in full — **Metal has no
> draw-indirect-count at all** (the count lives in GPU memory and Metal's only
> count-reading execution needs its commands to already exist; `wgpu` reached
> the same conclusion independently), and **Metal's bindless story is
> unsettled** — `crcbl-mtl` withdrew `DESCRIPTOR_INDEXING` at MTL6 because bind
> groups are flat argument tables, and getting it back needs Slang emitting
> argument-buffer-shaped MSL rather than a flag being flipped.
>
> **Ray tracing on Metal is blocked upstream.** Metal has ray tracing; Slang
> does not yet emit it for the Metal target. So the MVP's ray-traced lighting
> path is Vulkan and D3D12 only, and Apple platforms run the rasterised twin —
> with no engine branch, because the capability model absorbs it. Hand-writing
> MSL for those shaders was considered and declined; the reasoning is in
> topic 39.

## Order

**Metal first.** It's the mandatory one — as of the 2026-08-05 decision it is
the _only_ Apple path, so macOS and iOS have no GPU without it — and the API
distance is larger, so it flushes out HAL leaks DX12 wouldn't. Then DX12, which
maps near-1:1 to the Vulkan-shaped HAL.

~~An escape hatch worth timeboxing first: **MoltenVK spike (2–3 days)**~~ —
**cancelled 2026-08-05, see the correction at the bottom of this file.** Apple
platforms are Metal only; the spike will not be run and there is no gate to
clear.

## Shared prerequisites

- **Shader cross-compilation**: Slang emits Metal (MSL) and DXIL directly — this
  was the reason for choosing Slang in stage 2. Build pipeline grows per-backend
  artifact outputs keyed by the same shader hash.
- **Capability audit** (superseding the original "both are Tier A"): each
  backend reports the features it actually has — bindless via Metal argument
  buffers or DX12 descriptor heaps, indirect via ICBs or `ExecuteIndirect`, mesh
  shaders on both, ray tracing on DX12 only. A capability that does not map
  cleanly is reported **clear** and the renderer selects a lesser path; it is
  never emulated behind the seam and never resolved with backend-specific
  renderer code. See [39-capabilities.md](39-capabilities.md).
- **CI**: macOS + Windows runners build + run graph-compile tests; on-hardware
  smoke (render one frame, hash the readback) on self-hosted/manual runners —
  compile-verified-only backends are a known trap (gpur lesson).

## crcbl-mtl (Metal, via `objc2-metal`)

Mapping notes:

| HAL concept           | Metal                                                                                                     |
| --------------------- | --------------------------------------------------------------------------------------------------------- |
| Device/Queue          | `MTLDevice` / `MTLCommandQueue`                                                                           |
| Swapchain             | `CAMetalLayer` + drawable pacing                                                                          |
| Bindless              | Argument buffers (tier 2) — resource heaps + `useResource` residency management (the real work)           |
| Buffer device address | `gpuAddress` (Metal 3)                                                                                    |
| Draw indirect count   | **Not available.** Reported clear; the renderer selects another `GeometryPath` — see the correction above |
| Timeline semaphore    | `MTLSharedEvent`                                                                                          |
| Barriers/sync2        | Mostly implicit; hazard tracking off + explicit fences/`memoryBarrier` to match graph semantics           |
| Timestamps            | `sampleTimestamps` / counter sample buffers                                                               |

Known risk areas: residency management for bindless (Metal makes you say what's
resident), drawable acquisition semantics vs the Vulkan-shaped swapchain API,
private vs shared storage mapping onto HAL memory types.

## crcbl-dx12 (via `windows-rs`)

Mapping notes:

| HAL concept           | DX12                                                                                                    |
| --------------------- | ------------------------------------------------------------------------------------------------------- |
| Device/Queue          | `ID3D12Device` / command queues                                                                         |
| Swapchain             | DXGI flip-model                                                                                         |
| Bindless              | Shader-visible descriptor heap, SM6.6 dynamic resources (`ResourceDescriptorHeap[i]`) — near-direct fit |
| Buffer device address | GPU virtual addresses — direct fit                                                                      |
| Draw indirect count   | `ExecuteIndirect` with count buffer — direct fit                                                        |
| Timeline semaphore    | `ID3D12Fence` — direct fit                                                                              |
| Barriers              | Enhanced barriers (near-sync2); legacy resource states only if driver support forces it                 |
| Timestamps            | Query heaps                                                                                             |

Known risk areas: swapchain/present quirks (fullscreen transitions, tearing
flags), root signature design for the bindless model, debug layer (DRED,
GPU-based validation) integration into the same log path as Vulkan validation.

## Tasks

1. ~~MoltenVK spike + decision gate.~~ Cancelled; see the 2026-08-05 correction.
2. Slang → MSL/DXIL build outputs + shader hash plumbing.
3. `crcbl-mtl`: bring-up ladder (clear → triangle → sandbox → editor), then
   tier-A features, then perf pass vs Vulkan baseline.
4. `crcbl-dx12`: same ladder.
5. ~~Windowing: crcbl-shell Win32 + AppKit backends land here (topic 15).~~
   **Moved to P5C** (ROADMAP's 2026-08-04 correction) and shipped. What still
   waits for this phase is the sample-level pass — pressing F11 at a running
   game needs something to draw with.
6. CI matrix + on-hardware smoke runs.
7. Perf validation: stage 3 exit-criteria scene within ~15% of the Vulkan
   numbers on comparable hardware (flag, investigate, document if not).

## Exit criteria

- Sandbox + editor run natively on macOS (**Metal** — the gate is closed, see
  the 2026-08-05 correction) and Windows (DX12 and Vulkan — both, since
  `crcbl-vk` should just work on Windows and is the better-tested path).
- Same RenderDoc/Xcode-GPU-capture debuggability: named objects, per-pass timers
  feeding the same profiler HUD.
- Zero renderer/game/editor code changes attributable to backend differences
  (HAL fixes allowed; they're the seam doing its job).

## Risks

- **Metal residency + argument buffer debugging.** Budget the majority of Metal
  time here; Xcode GPU capture early and often.
- **Real-hardware access.** Compile-green ≠ works (gpur lesson). Schedule actual
  mac/Windows hardware time before declaring the stage done.
- **HAL freeze pressure.** Metal will find seam leaks; fix them as HAL changes
  with Vulkan re-verified, never as `#[cfg]` in the renderer.

## Corrections (design review, 2026-07-27)

- **FFI policy is not a contradiction with 15**: the rule is _bindings, not
  frameworks_. `ash`, `objc2`, `windows-rs` are thin bindings to APIs the
  OS/driver requires by ABI and are fine; winit/SDL-class frameworks are not.
  See 15's revised dependency-line section.
- **DX12's justification is corrected**: it is _not_ old-Intel-iGPU coverage —
  this backend is specced Tier A with SM6.6 dynamic resources, which those GPUs
  don't support (they're what `crcbl-wgpu` exists for). DX12 is here for (a) the
  Xbox door and (b) first-class Windows GPU debugging/vendor tooling. No Tier B
  DX12 path is planned.

## Correction (platform decision, 2026-08-05)

**Apple platforms are Metal only. The MoltenVK spike is cancelled, and there is
no longer a decision gate in front of this phase.**

### What changed

The order section above timeboxed a 2–3 day MoltenVK spike as an escape hatch:
if `crcbl-vk` ran acceptably on MoltenVK, native Metal could be deprioritized
post-MVP and macOS could ship on MoltenVK meanwhile. That hatch is closed by
decision rather than by measurement. `crcbl-vk` is not expected to run on macOS
or iOS, and `crcbl-mtl` is the only Apple path.

The wider matrix this belongs to: Vulkan for Windows, Linux and Android; Metal
for macOS and iOS; DX12 as the second Windows path, for the reasons the
2026-07-27 correction already gives (the Xbox door and first-class Windows GPU
debugging — **not** old-iGPU coverage). An OpenGL/GLES backend was considered
and declined in the same pass; `docs/backlog.md` carries that entry and its
reasons.

### Why, and what it costs

Two GPU paths on the platform with the least CI capacity is the expensive
outcome, and it is the one the hatch led to: every macOS bug report would have
started with "which backend were you on". iOS settles the question anyway —
there is no Vulkan loader or ICD story there at all, MoltenVK is linked directly
into the application — so choosing Metal for macOS as well makes the whole Apple
side a single backend.

The cost is that **`crcbl-mtl` is load-bearing rather than an optimisation**.
Until it can present a frame, macOS has no native GPU path at all: `crcbl-wgpu`
is the only thing that runs there, at Tier B. Nothing about this phase's ladder
changes, but its first two rungs — clear, then swapchain — are now the ones
holding up the platform.

### The technical question the spike would have answered

Worth keeping, because native Metal has to answer the same one. `crcbl-vk`
requires `Features::TIER_A` outright rather than degrading, that set includes
`DRAW_INDIRECT_COUNT`, and `crates/crcbl-vk/src/adapter.rs` reads it straight
off `VkPhysicalDeviceVulkan12Features`. Metal has no native indirect-count draw
— which is exactly why `crcbl-mtl` reports Tier B today, and why the
indirect-command-buffer work in this phase's tier-A step is what moves it.
MoltenVK would have met the same wall from the other side; the mapping table
above already names ICBs as the closest fit.
