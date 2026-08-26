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

**`Features::TIER_A` is removed — and it is gone.** It was a composite —
descriptor indexing, buffer device address, draw-indirect-count,
multi-draw-indirect, compute and timeline semaphores — that `crcbl-vk` demanded
whole or refused the device over. That was all-or-nothing, the opposite of the
rule above, and why Metal reported the lesser tier over one flag while having
the rest. `TIER_A`, `TIER_B` and `RendererTier` now name nothing in the
workspace; what stands in its place is `Features::GPU_DRIVEN`, the data-layout
bundle, which is asked for as _optional_ and never as required.

New flags this topic adds, for the features moved into the MVP — **all built, in
`crcbl_hal::caps`**, with `TASK_SHADER` beside `MESH_SHADER`:

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
axis. **All three are built, in `crcbl_hal::caps`:**

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

**What was wrong was the default.** `DeviceDesc::default()` set
`required_features: Features::TIER_A` — every caller demanded descriptor
indexing, buffer device address, draw-indirect-count, multi-draw-indirect,
compute and timeline semaphores, or got no device at all. That was the
all-or-nothing behaviour this topic exists to replace, and why Metal was refused
over one absent flag while having the rest.

The default becomes **only what nothing can work without** — compute and a
timeline semaphore — with everything else optional. A game whose whole look is
ray traced puts `RAY_QUERY` in `required_features` and gets a named failure
rather than a picture that is quietly a different game.

**Built as written.** `DeviceDesc::for_adapter` requires
`Features::COMPUTE.union(Features::TIMELINE_SEMAPHORE)` and asks for
`Features::GPU_DRIVEN` as optional, so the device opens on whatever the adapter
has and the selectors decide what to record. The doc comment on
`required_features` carries the ray-traced-game example above, so the rule is
stated where a caller reads it rather than only here.

The samples always did the right thing and still do: `GpuContextDesc::default`
puts `GPU_DRIVEN`, `MESH_SHADER`, `TIMESTAMP_QUERY`, `DEBUG_MARKERS`,
`PRESENT_FEEDBACK` and `PRESENT_TIMING` in `optional_features` and requires none
of them, so they degrade. It was the seam's own default that did not.

**Every downgrade is logged once, at device creation, naming the feature and the
path it selected.** A silently absent feature reporting as success is the same
defect class as an unimplemented hook returning `Ok` — see the verification
rules in `12-testing.md`. Built: `crcbl_hal::downgrades` compares what was asked
for against what the device **granted** — not against what the adapter could
have given — and `GpuContext::open` logs the difference once, saying nothing
when the device granted the lot.

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

**One of the four layers still has no source in the tree, and it is not
`[engine.video]`** — corrected 2026-08-27, this paragraph having claimed two.
The camera stack is the one: there is no render-stack RON and nothing in the
workspace reads RON, so `EffectRequest::camera` is written by a renderer per
view and by nothing else. `[engine.video]` **is** wired — `GpuContext::open`
reads the player's file through `SettingsSource`, `crcbl::settings::VIDEO_KEYS`
is the one place a key is spelled, and `GpuContext::effect_request` hands the
layer to a renderer built on that context, so every sample and every `crcbl new`
scaffold gets it without asking.
[18-render-features.md](18-render-features.md)'s "Where the toggles live"
already said so; this document had not caught up.

The device layer is wired to `DeviceCaps` and currently **removes nothing**, and
that is a statement about the three effects rather than a stub — see topic 18's
"Where the toggles live", which argues it per effect. The first rule that fires
arrives with the ray-traced variants, which `LightingPath` already selects.

## The graphics settings catalogue (LOCKED 2026-08-27)

**Why this is here and not in topic 15 or topic 18.** The graphics-quality keys
are not properties of a window, so topic 15 is the wrong home; and topic 18 owns
what each effect _is_, not what a player is allowed to do to it. What decides a
quality key's behaviour is the four-layer resolution order, and that order lives
here. So [15-windowing.md](15-windowing.md) keeps the display half of the
catalogue — mode, monitor, surface extent, present mode, render scale — this
document keeps the quality half, and [14-persistence.md](14-persistence.md)
keeps the file both are written to.

**What is missing from the seam is not its shape, it is its width.** The
layering is built and already documented in
[18-render-features.md](18-render-features.md): camera stack → `[engine.video]`
→ programmatic override → device capability, resolved in one place by
`EffectRequest::resolve`. Nothing below proposes a second layering. What the
seam carries today is four **boolean** effect bits, and a quality menu needs
rungs.

### Rule 1: the clamp survives the widening, and "downward" is an explicit order

**The `[engine.video]` layer may only ever remove quality, and an absent key
removes nothing.** That is the load-bearing property of the whole seam, and it
is the property a level-valued key is most likely to break. It is stated once,
here; the catalogue's second rule — that a key is named before it is implemented
— is stated once in [14-persistence.md](14-persistence.md).

For a boolean it is obvious: `crcbl::settings::video_effects` starts from
`RenderEffects::all` and clears a bit only for a key that is present and
`false`, so `true` and absent read identically and a settings file can never
switch an effect _on_. Its tests are named for that — an absent key clamps
nothing, and a key set to `true` reads the same as no key at all.

For a level it needs saying, because "less" is not something a value type knows:

- **Every enumerated key declares an explicit total order, lowest quality first,
  and that order is data — never the declaration order of a Rust enum.** A
  variant reordered or inserted in the middle of a `#[derive]`d enum would
  silently redefine every player's clamp, and nothing would fail. The order is
  therefore a table beside the key, in the same place the key is spelled, and it
  is what a test asserts against — the same discipline
  `crcbl::settings::VIDEO_KEYS`' tests already apply by writing the key/effect
  pairs out longhand rather than looping over the table that is their own
  oracle.
- **Clamping is `min` under that order.** The camera asked for GTAO, the
  player's file says SSAO, the frame gets SSAO. The camera asked for SSAO, the
  file says GTAO, the frame still gets SSAO — the file cannot promote.
- **An absent key is not the lowest rung.** It is "the player has not asked",
  which under `min` means the identity: no clamp. A key holding a value this
  layer cannot read is the same answer, and warns naming the key, which is what
  `video_effects` already does for a non-boolean.
- **`off` is a rung, not a separate mechanism.** It is simply the lowest one, so
  a boolean key is the two-rung case of the general rule rather than a different
  kind of key. That is what lets `VIDEO_KEYS` widen from `(&str, RenderEffects)`
  pairs to something carrying a level without the four existing keys changing
  meaning.

This is a widening of the one table in `crates/crcbl/src/settings.rs` and of
`RenderEffects` itself, since a bitflag cannot hold a rung. Both are the same
slice; neither is started here.

### The preset is the key players actually use, and it is not a fifth layer

`quality` — `"low"` | `"medium"` | `"high"` | `"ultra"` | `"custom"`.

**A preset is a writer, not a reader.** Selecting one writes every individual
key it covers into the user file; the resolution order never sees the preset at
all. That is what keeps it from becoming a fifth layer with its own precedence
question, and it is why **touching any individual key sets
`quality = "custom"`** — the preset value is then a label describing what was
last written, which is exactly true and cannot drift, because nothing reads it
back.

The alternative — a preset that is consulted at resolve time for keys the file
does not mention — was considered and declined: it makes an absent key mean
something, which is the one thing rule 1 forbids.

### The catalogue

Rungs are written lowest-first, which is the clamp order for that key. **The
"Today" column is per row on purpose**; of the whole table, four rows are
implemented as booleans and the rest have no reader and no renderer half.

| Key                     | Domain (lowest rung first)                         | Today                                                                                                                                                                                                                                                                                                                     |
| ----------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `quality`               | `low` \| `medium` \| `high` \| `ultra` \| `custom` | **Nothing.** No preset, and no settings screen to select one from.                                                                                                                                                                                                                                                        |
| `anti_aliasing`         | `off` \| `fxaa` \| `smaa`                          | **The shader exists and nothing above it does**, checked 2026-08-27: an `fxaa.slang` and a matching Rust params module sit in `crcbl-shaders`, that crate's module list names neither, `crcbl-render` has no resolve pass, and `RenderEffects` still carries four bits and no AA one. The `smaa` rung has nothing at all. |
| `ambient_occlusion`     | `off` \| `ssao` \| `gtao`                          | **Built as a boolean.** `RenderEffects::AMBIENT_OCCLUSION` and the `ambient_occlusion` key are the `off`/`ssao` rungs; topic 18 declines GTAO for now and says why.                                                                                                                                                       |
| `shadow_quality`        | `off` \| `low` \| `medium` \| `high`               | **Built as a boolean.** `RenderEffects::SHADOWS` and the `shadows` key are `off` versus everything else; the atlas has no quality rungs.                                                                                                                                                                                  |
| `shadow_distance`       | metres, a scalar                                   | **Nothing.** Distinct from `shadow_quality` because it trades range for resolution rather than buying either.                                                                                                                                                                                                             |
| `reflections`           | `off` \| `ssr`                                     | **Built as a boolean**, and the domain is honestly two rungs — a ray-traced rung arrives with `LightingPath::RayTraced`, not before.                                                                                                                                                                                      |
| `texture_quality`       | `low` \| `medium` \| `high`                        | **Nothing.** It is a streaming and residency decision, and topic 25 owns the mechanism it would drive.                                                                                                                                                                                                                    |
| `anisotropic_filtering` | `1` \| `2` \| `4` \| `8` \| `16`                   | **Nothing.** A sampler parameter with no key and no per-material plumbing.                                                                                                                                                                                                                                                |
| `draw_distance`         | metres, a scalar                                   | **Nothing.**                                                                                                                                                                                                                                                                                                              |
| `lod_bias`              | signed scalar; negative is more detail             | **Nothing.** Note that a _negative_ bias buys quality, so this key's clamp order runs the opposite way to its numeric order — the explicit-order rule above is why that is expressible at all.                                                                                                                            |
| `particle_quality`      | `low` \| `medium` \| `high`                        | **Nothing.** Topic 20 owns the system.                                                                                                                                                                                                                                                                                    |
| `decal_density`         | `low` \| `medium` \| `high`                        | **Nothing.** Topic 33 owns the system.                                                                                                                                                                                                                                                                                    |
| `bloom`                 | `off` \| `on`                                      | **Built as a boolean.** `RenderEffects::BLOOM`, and it is the one effect not in `RenderEffects::DEFAULT_STACK` — a view given no stack has been given no lens.                                                                                                                                                            |
| `motion_blur`           | `off` \| `on`                                      | **Nothing.** No pass, and no motion vectors to build one from.                                                                                                                                                                                                                                                            |
| `film_grain`            | `off` \| `on`                                      | **Nothing.**                                                                                                                                                                                                                                                                                                              |
| `chromatic_aberration`  | `off` \| `on`                                      | **Nothing.**                                                                                                                                                                                                                                                                                                              |
| `depth_of_field`        | `off` \| `on`                                      | **Nothing.**                                                                                                                                                                                                                                                                                                              |

**The last five rows exist as keys for a reason worth stating.** Motion blur,
film grain, chromatic aberration, depth of field and bloom are _lens_ effects —
properties of a camera rather than of the scene's light transport, which is the
line `RenderEffects::DEFAULT_STACK` is already drawn along. They are also the
effects players most reliably want off, and a game that ships them without
switches is one players patch around. Naming the keys now costs nothing and
means the switch exists on the day the pass does.

### Audio bus gains are not capability-clamped

[13-audio.md](13-audio.md) adds an `[engine.audio]` section whose keys are
per-bus linear gains, and they do **not** join the order above. The reason is
not that audio matters less: it is that the fourth layer has nothing to say. A
device removes a _render_ feature — there are adapters with no ray query and no
mesh shader, and `DeviceCaps` reports them — but there is no audio device that
takes away the ability to multiply a sample by 0.5. The DSP core is pure `f32`
block processing that runs identically on native and wasm, by design.

So an audio volume resolves through a shorter chain, stated in topic 13, and
this document's job is only to say that the chain is genuinely shorter rather
than merely unimplemented — a distinction worth keeping, because "no clamp yet"
and "no clamp ever" are read the same way by anyone skimming.

### Considered and declined

- **A per-monitor or per-adapter quality profile.** Declined for the reason
  [15-windowing.md](15-windowing.md) gives at length: it adds a second axis to
  the key space before anyone has asked for one, and its index is a monitor or
  adapter name that is neither unique nor stable. The `quality` preset covers
  the actual use — "make this machine's settings sensible in one click" —
  without persisting a matrix.
- **A user-defined post-chain order.** The post stack's order is fixed by
  [18-render-features.md](18-render-features.md) — bloom, then exposure and
  tonemap, then AA, then upscale, then UI — and each step of it is a correctness
  argument, not a preference: AA on tonemapped output, upscale after the passes
  whose cost is meant to scale with the internal extent, UI at native
  resolution. Letting a settings file reorder that produces frames nobody can
  reason about and goldens nobody can bless. Per-camera stack authoring (topic
  18's RON) remains the sanctioned way to change what a _view_ runs, and it is a
  developer-facing mechanism, not a player-facing one.
- **A settings-file migration format of the catalogue's own.** Declined:
  [14-persistence.md](14-persistence.md) already defines what happens to a key
  that is unknown, unreadable or absent, and the answer — warn, clamp nothing,
  keep going — is the whole migration story a clamp-only layer needs. A key that
  is renamed is a key that stops clamping, which is the safe direction, and that
  is precisely why renaming one is still a compatibility break worth avoiding.

## Feature matrix

Two different questions, deliberately separated — a plan that conflates them
reads as though the work is done:

- **API** — can the platform express this at all?
- **crcbl** — has this backend implemented it yet?

> **This matrix is a design record, not the live answer.** What each backend
> reports today is `crcbl_hal::Capability`, answered through an exhaustive
> `match` per backend and driven in both directions by
> `crates/crcbl/tests/hal_seam_e2e.rs`; `DIVERGENCES` and `REVIEWED_BLOCKERS` in
> `crcbl-hal/src/capability.rs` are what a human has reviewed as still owed.
> Read those before acting on a cell here. **And no `owed` cell on Metal or
> D3D12 is a task**: both backends were deferred 2026-08-21 — see
> `docs/plan/09-backends-metal-dx12.md`.
>
> Corrected 2026-08-23, having gone stale in both directions: the table carried
> a `wgpu (native)` column for a crate deleted 2026-08-21, and still showed
> `owed` for D3D12 push constants, D3D12 mesh shaders and D3D12 timestamp query,
> and `refused` for Metal timestamp query, all of which had landed. The
> paragraph under it already contradicted the timestamp cell in so many words.

| Feature               | Vulkan     | Metal             | D3D12      | WebGPU   |
| --------------------- | ---------- | ----------------- | ---------- | -------- |
| Compute               | yes / yes  | yes / yes         | yes / yes  | yes      |
| Descriptor indexing   | yes / yes  | yes / withdrawn   | yes / yes  | **no**   |
| Buffer device address | yes / yes  | yes / —           | yes / yes  | **no**   |
| Multi-draw-indirect   | yes / yes  | yes / yes         | yes / yes  | **no**   |
| Draw-indirect-count   | yes / yes  | yes / yes         | yes / yes  | **no**   |
| Mesh shaders          | yes / yes  | yes / owed        | yes / owed | **no**   |
| Ray query / RT        | yes / owed | yes / **blocked** | yes / owed | **no**   |
| Timestamp query       | yes / yes  | yes / owed        | yes / yes  | yes      |
| Push constants        | yes / yes  | yes / yes         | yes / yes  | proposed |
| Persistent mapping    | yes / yes  | yes / yes         | yes / yes  | **no**   |

The D3D12 column moved because `crcbl-dx12`'s `adapter.rs` now reports those
flags — descriptor indexing on binding tier 3, and the rest unconditionally,
each with its reason written on `features_of`. Push constants landed as root
constants and timestamp query as query heaps, so both cells moved; the
`Limits::timestamp_period_ns` the latter was once said to need no longer exists
either, the seam returning nanoseconds and each backend converting where it
knows how. Mesh shading is the one D3D12 row still open, and it is parked rather
than owed. **A withheld flag is not an oversight there, it is the rule of this
document applied to a backend mid-build.**

Vulkan's mesh-shader cell moved for the whole feature, not just the report:
`crcbl-vk` reads `PhysicalDeviceMeshShaderFeaturesEXT`, `create_mesh_pipeline`
refuses on a device without the flag, and `draw_mesh_tasks` is wired through the
seam. Its ray-tracing cell stays `owed` for the opposite reason — the adapter
reports `RAY_QUERY`, `RAY_TRACING_PIPELINE` and `ACCELERATION_STRUCTURE`, but
`crcbl-hal` has no acceleration-structure API for anything to build one through,
so nothing can use them yet.

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
  otherwise. **Subtraction is the mechanism when no adapter here selects the
  lesser path**: `crates/crcbl-vk/tests/vk_e2e/draw_gen.rs` opens a device
  without `DRAW_INDIRECT_COUNT` to reach `IndirectPerBatch` and without
  `MESH_SHADER` to reach `IndirectCount`, and asserts the arm is on the path it
  is about before comparing frames — an arm that silently ran the same code
  twice would otherwise pass.
- **The downgrade log line is an assertion target**, not decoration: an e2e that
  forces a feature off must see the engine say so.
- **`required` must be shown to fail.** A device request naming a feature the
  null backend does not report has to produce the named error; a `required` that
  cannot fail is not a gate.
- Golden images are per `(GeometryPath, BindingModel, LightingPath)` combination
  that a backend actually selects, not per backend.

## Delivery

The model itself is built — the flags, the selectors, the resolution point and
the downgrade log, in `crcbl_hal::caps`, `crcbl_hal::downgrades` and
`crcbl_render::effects`. **The settings layer is built too**, corrected
2026-08-27: `crcbl::settings::video_effects` maps four `[engine.video]` keys to
`RenderEffects` bits and `GpuContext::open` reads the player's file while it
opens, so the layer has a real source in every sample rather than only in a
test.

What is not built is the **width** of that layer and the screen in front of it.
Four boolean keys is the entire settings surface today; the catalogue above
names the rest and marks each one unimplemented, and exposing any of them on a
settings screen is still P10 work.

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
