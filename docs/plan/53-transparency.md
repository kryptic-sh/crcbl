# Topic 53 — Blended transparency: the sorted pass and its keys

Written 2026-09-06, when the five decisions this rung was held on were taken.
Its place in the set is [18-render-features.md](18-render-features.md)'s index;
what a current engine ships and where this one stands against it is
[43-render-standards.md](43-render-standards.md) §3, which now points here.

It is the last rung of the raster ladder, and the first that touches the
**structure** of the frame rather than what a pass computes: every other rung so
far has added a pass over the whole picture or a term inside one, and this one
adds a second draw of the scene whose order is data the GPU produces.

## Where this is

`crates/crcbl-render/src/forward.rs` builds no pipeline with a `BlendState` at
all. Every blended pipeline in the render crate composites rather than shades —
`crcbl_render::sprite_pass` and `crcbl_render::ui_pass` in straight and
premultiplied alpha, `crcbl_render::debug_draw` in straight alpha,
`crcbl_render::grid` premultiplied, and `crcbl_render::bloom`'s upsample
additive because the add has to be the blender's. So the engine can draw a
translucent sprite and cannot draw a translucent _surface_.

The first step of the ladder is taken: alpha-mask landed 2026-09-05 — a
`discard`, no sorting, no new pass, and what foliage actually wants.
`crcbl_scene::gltf_import` already recognises `AlphaMode::Blend` and warns that
it is **drawing the material opaque**, naming the count; that warning is the
thing this topic deletes.

## The decisions

Five, all taken 2026-09-06, each with the answer the industry converged on.

### 1. Order-independent transparency is refused for now, and the sort is built

Per-object back-to-front sorted alpha blending is the default in Unreal, Unity
(both HDRP and URP), Godot 4 and Frostbite. Weighted-blended OIT (McGuire and
Bavoil 2013) is an **opt-in approximation** in every one of them, not the
default path, and it is an approximation that cannot be blessed against a
reference — which this workspace's golden discipline has to decide about before
such a pass is built, not after.

So the sort is what gets built, and OIT stays an unscheduled opt-in effect. The
sorted pass is also what OIT would need anyway for the cases it is worse at, so
nothing here is thrown away if the refusal is ever revisited.

**What the sort cannot fix is per-object granularity**, which is what every
engine's artists then work around: two interpenetrating blended surfaces have no
correct object order. That is the known limit of the shipped answer rather than
a defect of this one, and it is the reason foliage takes MASK rather than BLEND.

### 2. Global order is one indirect call per blended slot

This engine's submission is GPU-driven: draw order comes out of
`draw_gen.slang`'s per-bucket runs and the CPU never sees a list to sort. So
"sort back to front on the CPU" is not a step the frame has, and the order has
to be produced where the draws are.

**The CPU records a fixed number of blended draw slots at a fixed stride**, one
per `Capacities::blended` — a new capacity beside `instances` and `materials` —
and the sort writes each slot's indirect argument structure in depth order. A
slot no blended object landed in gets an **instance count of zero**, which draws
nothing.

That shape is chosen because it is the one that works on **every** `EmitTail`,
including `EmitTail::PerBatch`, which is what a browser runs: WebGPU has no
`draw_indexed_indirect_count`, so a variable number of draws cannot come out of
GPU memory there. A fixed record with zeroed slots is the same picture rather
than an approximation of it — exactly the argument `EmitTail::PerBatch`'s own
documentation already makes for empty buckets — and a zero-count indirect draw
is free on every driver this engine targets. It is what Bevy's sorted
transparent phase and wgpu's indirect batching do for the same reason.

`EmitTail::Count` **may additionally bound the loop** with a real count, since
that tail has the verb; it is an optimisation on one path and never a second
picture.

### 3. Blended surfaces cast no shadows

Translucent shadow casting is off by default in Unreal and off by default in
Unity, and for the same reason both give: a shadow map holds one depth and one
occluder, and a surface light passes through has neither.

So a blended bucket is **excluded from `depth_partitions`** — the partition
`ForwardRenderer` uses for both the depth prepass and the shadow atlas — and
from the reflective shadow map with it. The consequence is stated rather than
hidden: a pane of glass throws no shadow and contributes no bounce, and a scene
that needs one uses a MASK material or a separate opaque proxy, which is what
the same scene does in every engine named above.

It also falls out of an interaction rule that predates this document: the depth
prepass, SSAO and the Hi-Z pyramid all read a **single opaque depth**, and a
transparent surface has no single depth. Keeping blended geometry out of the
depth partition is what keeps all three reading what they were written against.

### 4. The pass runs after `ssr` and after `volumetric-composite`, and the blended fragment applies the fog itself

The blended pass is recorded after both, which settles two interactions at once.

- **SSR.** The SSR row ([rendering notes](../notes/rendering.md)) refuses SSR on
  transparency with its reason — a transparent surface writing the reflectivity
  attachment overwrites the opaque `F0` behind it while the scene colour there
  is a blend — and Unreal makes SSR on translucency a separate opt-in for the
  same reason. Running after `ssr` means a blended surface is neither an SSR
  source nor an SSR receiver, and the write mask below is what enforces the
  first half.
- **Volumetric fog.** `volumetric-composite` applies the froxel volume to the
  opaque frame. A blended surface drawn afterwards would be un-fogged, floating
  in front of air that everything else is behind. So the **blended fragment
  samples the integrated volume at its own depth and applies the fog itself** —
  which is Frostbite's and Unreal's shipped form ("translucency samples the
  volumetric fog texture"), and is cheap here because the froxel buffer is
  already resident and `froxel_of` is already in `mesh.slang`.

### 5. BLEND is exclusive with MASK, and the mode field carries six values

glTF 2.0 gives a material a single `alphaMode`, one of `OPAQUE`, `MASK` and
`BLEND`. So the engine's alpha mode is one of three and never two, and BLEND
composes only with `doubleSided`: **three alpha modes × two sides = six modes**,
where there are four today.

That widens two constants that are documented as a pair and are currently
exactly full:

- `GpuMaterial::MODE_MASK` gains an `ALPHA_MODE_BLEND` bit —
  `ALPHA_MODE_MASK | ALPHA_MODE_BLEND | DOUBLE_SIDED`, three bits carrying six
  legal values.
- `GpuInstance::MATERIAL_MODE_MASK` widens from `0b11` to `0b111` at the same
  `MATERIAL_MODE_SHIFT` of 2, taking bit 4, which is free — bits 0 and 1 are
  `LIVE` and `BASE_VERTEX_OVERRIDE`.

The two "legal" values the three bits can spell and the specification cannot —
mask and blend together — are refused where the mode is built, in
`crcbl_scene::gltf_import` and in `MaterialTable`, rather than defended in the
shader. `draw_gen.slang`'s `instance_material_mode` compares the whole field, so
it needs the width and nothing else.

## The sort key

The sort is a **GPU radix sort over one key per live blended instance**, run in
compute before the blended pass records, and the ordering it produces is **back
to front**: the farthest blended fragment must be composited first.

**The key is 64 bits, not 32.** The tempting layout — depth quantised to 24 bits
with the instance index in the low 8 — does not work, because the index has to
name an entry of the instance pool and eight bits names 256 of them, orders of
magnitude short of `Capacities::instances`' own default. Narrowing the depth
field to make room is the wrong trade in the other direction: 24 bits of
quantised depth is already near the point where two distinct surfaces share a
bucket at scene scale.

So a key is a pair, packed into 64 bits and sorted whole:

| Bits    | Field                                                                              |
| ------- | ---------------------------------------------------------------------------------- |
| 63 – 32 | View depth, quantised to 32 bits and **inverted**, so ascending order is far-first |
| 31 – 0  | The instance-pool index                                                            |

Sorting on the whole 64 bits is the same thing as sorting on the tuple
`(depth, instance)`, and that second element is the point: **ties are broken
deterministically**. Two coplanar blended surfaces at the same quantised depth
would otherwise land in whatever order the radix passes happened to leave them,
which differs by workgroup scheduling and therefore by driver — and this
workspace's goldens are built not to be a function of that. With the index in
the low word the order is a function of the scene alone, and the same frame
sorts identically on radv, on lavapipe and in a browser.

The depth is the instance's **origin** in view space, not a per-fragment or
per-triangle depth: the granularity is per object, which is decision 1's stated
limit written into the key.

## What changes, crate by crate

Every symbol named here exists today unless the line says it is new.

### `crcbl-hal`

`ColorTargetState` gains two constructors beside `opaque`:

- **one that takes a blend** — `ColorTargetState::blended(format, blend)`,
  `write_mask: ColorWrites::ALL`. `BlendState::alpha` (`SrcAlpha`,
  `OneMinusSrcAlpha`) is already in the crate and is what this pass uses.
- **one with an empty write mask** — a target the pipeline declares and does not
  write, `blend: None` and `write_mask: ColorWrites::empty()`.

The second is what makes decision 4's SSR half enforceable rather than a
convention. `MeshModules::COLOR_TARGETS` is three targets — the shaded colour,
[18-render-features.md](18-render-features.md)'s reflectivity channel, and
[43-render-standards.md](43-render-standards.md) §9's motion vector — and a
pipeline must declare all three because the attachments are all three. The
blended pipeline declares the same three and writes only the first: reflectivity
stays the opaque surface's `F0` behind the glass, and the motion vector stays
the opaque surface's, which is what the passes reading them were written
against.

No new capability. Blending on a single colour target is core on all four
backends and needs no `Features` bit.

### `crcbl-scene`

`gltf_import` stops folding `AlphaMode::Blend` into the opaque arm. Today's
match reads `AlphaMode::Opaque | AlphaMode::Blend => (UNTINTED.alpha_cutoff, 0)`
and a warning below it counts BLEND materials off the JSON and says they are
being drawn opaque. Blend gains its own arm setting `ALPHA_MODE_BLEND`, and the
warning is deleted with it. The JSON-side count stays useful as the place a
document naming an `alphaMode` the specification does not have is rejected: the
comment there records why it reads the JSON rather than the `gltf` crate's own
accessor, which unwraps a `Checked` and panics on such a document. That reason
does not change.

### `crcbl-render`

- **`Capacities::blended`** — new: the number of blended draw slots the frame
  records, sized like `instances` is, and the number the sort's output buffer
  and the indirect argument buffer are both cut to.
- **A `SidedPipelines` of its own**, beside the four the renderer already holds
  (`mesh_pipeline`, `shadow_pipeline`, `depth_masked_pipeline`, `rsm_pipeline`).
  Same entry point and same modules as `mesh_pipeline`; what differs is the
  colour-target array above and the depth state — `GreaterOrEqual` against the
  prepass's depth with **writes off**, since a blended surface must not occlude
  the blended surface behind it.
- **`blended_partitions`** — new, beside `depth_partitions` and
  `sided_partitions`: the blended buckets split by side alone, under the two
  halves of that pipeline pair. `depth_partitions` and `sided_partitions` each
  gain the complementary filter, so a blended bucket reaches neither the depth
  passes (decision 3) nor the opaque colour pass.
- **`RENDER_PASSES`** gains two terms — the sort's compute dispatches and the
  blended render pass — on the rule that constant states: it is a ceiling every
  term contributes its own widest case to. A frame whose scene has no blended
  material records neither, exactly as a frame that appends no debug geometry
  records no debug pass.
- **No `RenderEffects` bit.** Whether a surface is translucent is a property of
  the scene's materials, not an effect a camera stack switches on, and the same
  argument already governs the double-sided twin. The pass is recorded when the
  scene has a blended bucket and not otherwise.

### `crcbl-shaders`

- **`blend_sort.slang`** — new: the radix sort. Key extraction from the instance
  pool, the radix passes, and the scatter that writes each sorted slot's
  indirect argument structure and zeroes the rest.
- **`mesh.slang`** — the fragment stage gains the fog application of decision 4
  and writes only `SV_Target0` on the blended path. Nothing in the shading
  itself changes: a blended fragment shades through the same froxel grid and the
  same BRDF as an opaque one, which is exactly the argument topic 44 gave for
  clustered forward (recorded in the [rendering notes](../notes/rendering.md))
  over deferred in the first place.
- **`draw_gen.slang`** — `instance_material_mode` reads the widened field.

## The fixture

`crates/crcbl/tests/render_e2e.rs` gains a scene on `Scene::AlphaMask`'s shape,
and its claims are relations between bands rather than absolute colours, as that
test's are.

The scene is **two blended quads of different colours over a lit floor, hung at
different depths and overlapping in screen space, plus a band of floor no quad
ever covers**. That last band is the control, and it is what the whole fixture
turns on:

- **The blend happened.** The band under one quad alone must lie between that
  quad's own colour and the floor's — a quad drawn opaque passes no such
  relation, and a quad not drawn at all reads as the control band exactly.
- **The order is back to front.** The band where the two quads overlap must be
  nearer the **near** quad's colour than the far one's. This is the claim the
  sort exists for, and it is the one a frame with a blend state and no sort
  fails: reversing the key's inversion flips exactly this band and leaves every
  other claim green.
- **The floor is still there.** The control band must be within tolerance of the
  same floor in a frame with no blended material at all, which is what says the
  blended pass wrote where it was asked to and nowhere else.
- **Nothing was occluded.** The far quad's own band must not be the clear: depth
  writes left on would have the near quad reject it.
- **The fog reached it.** With `VOLUMETRIC_FOG` on, every blended band moves
  toward the fog's colour by the amount the opaque floor at that depth moved.
  Decision 4's claim, and it is the one that would silently pass if the pass
  were recorded before `volumetric-composite` instead of after — so it is
  measured against the floor rather than against a constant.

Each claim is to be red-checked the way `Scene::AlphaMask`'s six bands were,
before any of them is trusted: the tree sabotaged one change at a time, this
test alone selected, and the line the run printed recorded in the test's own
documentation.

## What this unblocks

Four things wait on a blended pass:

- **Glass and water surfaces** at all — there is no other way to draw one.
- **Foliage that is not a cutout.** MASK covers the leaf card; a soft edge does
  not.
- **[20-particles.md](20-particles.md)'s blended particle buckets**, which need
  the same sorted-slot machinery and can take it rather than grow a second one.
- **The mirrors sample's water**, once the reflection ladder's planar rung lands
  (`docs/backlog.md`, _Planar reflections, the reflection ladder's rung 2, are
  unbuilt_): a planar reflection on an opaque plane is a floor, and on a blended
  one it is water.

## Delivery

| Rung                                               | What it buys                                                                                     | What it costs                                                                                                                                      |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Blended transparency with GPU-sorted keys**      | Glass, water and soft-edged foliage — the last thing a current engine draws that this one cannot | `Capacities::blended`, a `SidedPipelines`, `blended_partitions`, `blend_sort.slang`, two `ColorTargetState` constructors, and one more render pass |
| **Order-independent transparency** _(unscheduled)_ | Interpenetrating blended surfaces, which per-object order cannot get right                       | A pass that is an approximation with no reference to bless against — decision 1's refusal, and the reason it is a separate row                     |

The first row is [43-render-standards.md](43-render-standards.md)'s delivery
table's entry for this topic, and it is **priced before it is called built** on
that table's rule: the sort and the blended pass each write their millisecond
cost on the desktop adapter, on lavapipe and in the browser into this document,
read off `crcbl_render::PassStats`. The browser figure is the one that decides
whether `Capacities::blended`'s default is the right size, since the fixed-slot
shape of decision 2 pays for its slots whether or not they draw.
