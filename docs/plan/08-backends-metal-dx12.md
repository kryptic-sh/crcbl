# Stage 8 — Metal + DX12 Backends

Implement the frozen HAL on Metal (macOS) and DX12 (Windows). The renderer, ECS,
UI, and editor don't change — that's the point of the seam. Vulkan also runs
natively on Windows, so DX12's practical role is old-Intel-iGPU coverage and
keeping the door open for Xbox; Metal is the only path on macOS.

## Order

**Metal first.** It's the mandatory one (no Vulkan on macOS without MoltenVK)
and the API distance is larger — it flushes out HAL leaks DX12 wouldn't. Then
DX12, which maps near-1:1 to the Vulkan-shaped HAL.

An escape hatch worth timeboxing first: **MoltenVK spike (2–3 days)** — run
`crcbl-vk` on MoltenVK. If the tier-A feature set (descriptor indexing,
BDA-equivalent, indirect count) works acceptably, native Metal can be
deprioritized post-MVP and macOS ships on MoltenVK meanwhile. Decision gate, not
a plan change.

## Shared prerequisites

- **Shader cross-compilation**: Slang emits Metal (MSL) and DXIL directly — this
  was the reason for choosing Slang in stage 2. Build pipeline grows per-backend
  artifact outputs keyed by the same shader hash.
- **Tier flags audit**: both backends are Tier A (bindless: Metal argument
  buffers tier 2 / DX12 descriptor heaps; indirect: ICBs / ExecuteIndirect). Any
  tier-A capability that doesn't map cleanly gets resolved via the tier system,
  not backend-specific renderer code.
- **CI**: macOS + Windows runners build + run graph-compile tests; on-hardware
  smoke (render one frame, hash the readback) on self-hosted/manual runners —
  compile-verified-only backends are a known trap (gpur lesson).

## crcbl-mtl (Metal, via `objc2-metal`)

Mapping notes:

| HAL concept           | Metal                                                                                           |
| --------------------- | ----------------------------------------------------------------------------------------------- |
| Device/Queue          | `MTLDevice` / `MTLCommandQueue`                                                                 |
| Swapchain             | `CAMetalLayer` + drawable pacing                                                                |
| Bindless              | Argument buffers (tier 2) — resource heaps + `useResource` residency management (the real work) |
| Buffer device address | `gpuAddress` (Metal 3)                                                                          |
| Draw indirect count   | ICBs or indirect command encoding — closest-fit chosen during implementation                    |
| Timeline semaphore    | `MTLSharedEvent`                                                                                |
| Barriers/sync2        | Mostly implicit; hazard tracking off + explicit fences/`memoryBarrier` to match graph semantics |
| Timestamps            | `sampleTimestamps` / counter sample buffers                                                     |

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

1. MoltenVK spike + decision gate.
2. Slang → MSL/DXIL build outputs + shader hash plumbing.
3. `crcbl-mtl`: bring-up ladder (clear → triangle → sandbox → editor), then
   tier-A features, then perf pass vs Vulkan baseline.
4. `crcbl-dx12`: same ladder.
5. Windowing: winit already covers macOS/Windows; verify DPI + resize behavior
   per platform.
6. CI matrix + on-hardware smoke runs.
7. Perf validation: stage 3 exit-criteria scene within ~15% of the Vulkan
   numbers on comparable hardware (flag, investigate, document if not).

## Exit criteria

- Sandbox + editor run natively on macOS (Metal or MoltenVK per gate decision)
  and Windows (DX12 and Vulkan — both, since `crcbl-vk` should just work on
  Windows and is the better-tested path).
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
