# Topic 39 — Device Capabilities and Graceful Degradation

How the engine decides what a device can do, what it does when the answer is
"less than you asked for", and where a game or a player overrides either.

**This topic supersedes the two-valued renderer tier.**
`03-gpu-driven-rendering.md` introduced `Tier A` / `Tier B` as a shorthand for
"native" versus "WebGPU", and that shorthand stopped describing reality: Metal
has multi-draw-indirect and no GPU-side count, D3D12 has both in the API and
neither written yet, `wgpu` on native reports very nearly the full native set,
and WebGPU in a browser has none of it. Ray tracing and mesh shaders add two
more independent axes. Two buckets cannot hold that, and forcing a device into
the wrong one is a lie the renderer then acts on.

## The rule

**A missing feature degrades by default. A game may declare one required, and
then its absence is a named, loud failure at device creation.**

Everything else in this document follows from that sentence. There is no third
behaviour: nothing silently renders differently without a log line, and nothing
refuses to start unless someone asked it to.

## Three layers

### 1. Capabilities are the truth

`crcbl_hal::Features` (bitflags) and `crcbl_hal::Limits` (numeric ceilings),
bundled as `DeviceCaps`. A backend reports what the device actually has. This
already exists and is the right shape; what changes is what sits on top.

**`Features::TIER_A` is removed.** It is a composite — descriptor indexing,
buffer device address, draw-indirect-count, multi-draw-indirect, compute and
timeline semaphores — and `crcbl-vk` demands the whole set or refuses the
device. That is all-or-nothing, it is the opposite of the rule above, and it is
why Metal reports the lesser tier over one flag while having the rest.

New flags this topic adds, for the features moved into the MVP:

| Flag                     | What it gates                             |
| ------------------------ | ----------------------------------------- |
| `MESH_SHADER`            | mesh + amplification/task stages          |
| `RAY_QUERY`              | inline ray queries from any stage         |
| `RAY_TRACING_PIPELINE`   | ray generation / hit / miss shader stages |
| `ACCELERATION_STRUCTURE` | BLAS/TLAS build and refit                 |

### 2. Derived path selectors are what the renderer branches on

A single tier cannot express the combinations, but the renderer does not need
the whole capability set at each branch either — it needs to know which of a
small number of paths to compile and record. Each selector is derived from
`Features`, each is one shader-permutation axis, and each is one golden-image
axis:

| Selector       | Values                                                | Derived from                          |
| -------------- | ----------------------------------------------------- | ------------------------------------- |
| `GeometryPath` | `MeshShader` \| `IndirectCount` \| `IndirectPerBatch` | `MESH_SHADER`, `DRAW_INDIRECT_COUNT`  |
| `BindingModel` | `Bindless` \| `ArrayPages`                            | `DESCRIPTOR_INDEXING`                 |
| `LightingPath` | `RayTraced` \| `Rasterised`                           | `RAY_QUERY`, `ACCELERATION_STRUCTURE` |

Selectors are ordered best-first and resolve downward: a device without
`MESH_SHADER` but with `DRAW_INDIRECT_COUNT` lands on `IndirectCount`; without
either it lands on `IndirectPerBatch`. **Degradation is always monotonic** —
there is no capability whose absence selects a path that needs more than the one
above it.

### 3. Named profiles are for humans, never for code

"Native baseline" and "web baseline" name useful points in the space, for CI job
names, documentation and log lines. **Nothing branches on a profile name.** A
profile is a description after the fact, not an input.

## The request: `required` versus `preferred`

**This distinction already exists in the seam and is spelled correctly.**
`DeviceDesc` carries `required_features` and `optional_features`;
`DeviceCaps::missing(required)` answers "which of these is absent", and
`request_device` is documented to fail naming exactly what an adapter lacks. No
new type is needed — an earlier draft of this document proposed a
`FeatureRequest` struct, which would have duplicated what is there.

**What is wrong is the default.** `DeviceDesc::default()` sets
`required_features: Features::TIER_A` — every caller demands descriptor
indexing, buffer device address, draw-indirect-count, multi-draw-indirect,
compute and timeline semaphores, or gets no device at all. That is the
all-or-nothing behaviour this topic exists to replace, and it is why Metal is
refused over one absent flag while having the rest.

The default becomes **only what nothing can work without** — compute and a
timeline semaphore — with everything else optional. A game whose whole look is
ray traced puts `RAY_QUERY` in `required_features` and gets a named failure
rather than a picture that is quietly a different game.

Worth noting the samples already do the right thing: `apps/*/src/gpu.rs` pass
`optional_features: Features::TIER_A`, so they degrade today. It is the seam's
own default that does not.

**Every downgrade is logged once, at device creation, naming the feature and the
path it selected.** A silently absent feature reporting as success is the same
defect class as an unimplemented hook returning `Ok` — see the verification
rules in `12-testing.md`.

## Toggles: three layers, one resolution point

Every feature is switchable, and the switch exists at three levels because they
answer different questions:

| Layer            | Home                                       | Question it answers               |
| ---------------- | ------------------------------------------ | --------------------------------- |
| **Settings**     | `[engine.video]` (`14-persistence.md`)     | what does this _player_ want?     |
| **Per camera**   | render-stack RON (`18-render-features.md`) | what does this _view_ need?       |
| **Programmatic** | game code, at any time                     | what does this _moment_ call for? |

Resolution order, applied in one place:

```
camera stack declares what the view wants
  → [engine.video] clamps it downward as a quality setting
  → programmatic override may set it either way
  → device capability clamps it downward, last and absolutely
```

The per-camera layer exists because it is genuinely per view: a
render-to-texture camera feeding a security monitor or a planar reflection does
not want reflections of its own, and that is a property of the camera rather
than of the player's hardware or preference. The programmatic layer is the same
escape hatch `GpuContext::set_pacing` already provides — a game with its own
quality logic can drive it directly.

**Capability clamps last and cannot be overridden upward.** Asking for a feature
the device lacks is what `required` is for; it is not something a toggle can
force.

**Built 2026-08-14 for topic 18's three raster effects**, in
`crcbl_render::effects`: `RenderEffects` is the effect set, `EffectRequest`
carries the three requested layers, and `EffectRequest::resolve` is the one
place the order is applied. `ForwardRenderer` holds one request and resolves it
once per `begin_frame`.

Two of the four layers are shaped and have no source in the tree — the camera
stack (there is no render-stack RON, and nothing in the workspace reads RON) and
`[engine.video]` (`crcbl_store::settings` reads the namespace; nothing builds a
stack at startup). They are fields nothing but a test writes, and they are
present because the _order_ is the thing being built: a resolution point missing
two of its inputs cannot be shown to apply them in the right order.

The device layer is wired to `DeviceCaps` and currently **removes nothing**, and
that is a statement about the three effects rather than a stub — see topic 18's
"Where the toggles live", which argues it per effect. The first rule that fires
arrives with the ray-traced variants, which `LightingPath` already selects.

## Feature matrix

Two different questions, deliberately separated — a plan that conflates them
reads as though the work is done:

- **API** — can the platform express this at all?
- **crcbl** — has this backend implemented it yet?

| Feature               | Vulkan     | Metal             | D3D12      | wgpu (native) | WebGPU   |
| --------------------- | ---------- | ----------------- | ---------- | ------------- | -------- |
| Compute               | yes / yes  | yes / yes         | yes / owed | yes / yes     | yes      |
| Descriptor indexing   | yes / yes  | yes / withdrawn   | yes / owed | yes / —       | **no**   |
| Buffer device address | yes / yes  | yes / —           | yes / —    | **no**        | **no**   |
| Multi-draw-indirect   | yes / yes  | yes / yes         | yes / owed | yes / —       | **no**   |
| Draw-indirect-count   | yes / yes  | **no**            | yes / owed | yes / —       | **no**   |
| Mesh shaders          | yes / owed | yes / owed        | yes / owed | yes / —       | **no**   |
| Ray query / RT        | yes / owed | yes / **blocked** | yes / owed | yes / —       | **no**   |
| Timestamp query       | yes / yes  | refused           | yes / owed | yes / —       | yes      |
| Push constants        | yes / yes  | yes / yes         | yes / owed | yes / —       | proposed |
| Persistent mapping    | yes / yes  | yes / yes         | yes / yes  | **no**        | **no**   |

Three entries need their reasons stated rather than left as a cell:

- **Draw-indirect-count on Metal is not a gap, it is absent from the API.** The
  count lives in GPU memory and Metal's only count-reading execution is
  `executeCommandsInBuffer:` over an `MTLIndirectCommandBuffer` whose commands
  must already exist. `wgpu` reached the same conclusion independently — its
  `MULTI_DRAW_INDIRECT_COUNT` is documented as D3D12 and Vulkan only, and its
  Metal backend contains no multi-draw code at all. Two implementations, one
  answer. **Metal reports the flag clear and lands on a different
  `GeometryPath`.** With mesh shaders as the primary path this affects only the
  fallback.
- **Ray tracing on Metal is blocked on the shader compiler, not the API.** Metal
  has ray tracing; Slang does not yet emit it for the Metal target (its Metal
  support covers vertex, fragment, compute, mesh and amplification).
  Hand-written MSL was considered and declined — see below.
- **Persistent mapped buffers are a native-only design principle.**
  `00-overview.md`'s first core design principle names them alongside bindless
  descriptors and multi-draw-indirect; `wgpu` exposes them as
  `MAPPABLE_PRIMARY_BUFFERS`, native only. On the browser path every upload goes
  through a staging copy. This was asserted as a principle and never given a
  Tier B answer; the staging path is that answer, stated here.

## Worked example: ray tracing on Apple

This is the case the whole design is for, so it is written out.

Ray tracing is MVP and is Vulkan and D3D12 only. macOS and iOS fall on the same
side of the line as the browser, and the 2026-08-05 platform decision makes
Metal the only Apple path, so there is no second route.

**Nothing in the engine has an Apple branch.** `crcbl-mtl` reports `RAY_QUERY`
clear, `LightingPath` resolves to `Rasterised`, the engine logs the downgrade
once, and the raster stack — which exists anyway, for the browser — draws the
frame. No `#[cfg]`, no platform special case, no second plan.

When Slang's Metal ray-tracing support lands, `crcbl-mtl` starts reporting the
flag and the existing path lights up **with no engine change at all**. That is
the argument for capability-driven degradation in one worked case, and it is why
the tier model had to go: under a tier, the same situation is a device wearing
the wrong label.

**Considered and declined: hand-writing MSL for the ray-tracing shaders.** Metal
has its own ray tracing API and the shaders could be written by hand. Declined
because **MSL cannot be validated anywhere except on a Mac** — `xcrun metal` is
macOS-only and `MTLDevice::newLibraryWithSource:` needs a real device, so on the
development machine there is no way to syntax-check it, and no open-source tool
parses MSL at all. A hand-written twin would be the `ui_tier_b.slang`
duplication — since deleted, precisely because a manually synced twin is what it
was — but in a language nothing local can read, guarded by an end-to-end suite
whose draw tests are currently quarantined and which has never run on real Apple
hardware. The drift would be undetectable. **Do not revisit without either a
Metal compiler that runs off macOS or a differential gate that runs on real
Apple hardware.**

**Tracked externally:** Slang's Metal ray-tracing support is listed by upstream
as in progress. This is a dependency on someone else's roadmap, which is exactly
the situation the project's from-scratch policy exists to avoid, so it is
recorded rather than assumed: re-check at each Slang version bump — the pin
lives in `crates/crcbl-shaders/tools/compile-shaders.sh` — and note that
contributing the target upstream is a legitimate option if it stalls.

## Testing

- **Every selector value must be executed by something.** A path that no device
  in CI selects is a path that compiles and is never run; name it in
  `docs/backlog.md` as a coverage gap rather than letting a green suite imply
  otherwise. The Tier B arms of the indirect-draw tests are the existing
  instance of this — lavapipe reports the higher capability, so the fallback is
  compiled and unrun.
- **The downgrade log line is an assertion target**, not decoration: an e2e that
  forces a feature off must see the engine say so.
- **`required` must be shown to fail.** A device request naming a feature the
  null backend does not report has to produce the named error; a `required` that
  cannot fail is not a gate.
- Golden images are per `(GeometryPath, BindingModel, LightingPath)` combination
  that a backend actually selects, not per backend.

## Delivery

| Slice                                                                     | Phase |
| ------------------------------------------------------------------------- | ----- |
| Remove `Features::TIER_A` and `RendererTier`; fix `DeviceDesc::default()` | P7    |
| Derived path selectors + the resolution point + downgrade logging         | P7    |
| `MESH_SHADER` / `RAY_QUERY` / `ACCELERATION_STRUCTURE` flags reported     | P7    |
| Toggle layering (settings ← camera stack ← programmatic)                  | P7    |
| Settings-screen exposure of the video toggles                             | P10   |

**The timing is deliberate and it is now.** `RendererTier` is consumed by log
lines, `Debug` impls, tests and one device request; nothing in the renderer
branches on it, because P7 has not landed. Changing it before P7 is nearly free
and changing it afterwards is not.

## Risks

- **Selector creep.** Three selectors are tractable; one per feature is a
  combinatorial mess in shader permutations and golden images. A new selector
  needs a real second path behind it, not a capability that could have been a
  uniform.
- **Untested fallbacks.** The whole point is that lesser paths work, and the
  usual failure is that nobody runs them. See the testing rule above; this is
  the risk most likely to be realised.
- **A `preferred` set that is really required.** If the engine cannot in fact
  render without something, it belongs in `required` — a downgrade path that was
  never going to work is worse than an honest refusal.
