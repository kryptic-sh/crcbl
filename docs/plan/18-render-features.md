# Topic 18 — Render Features: Lighting, Shadows + Post-Processing

The visual-credibility layer on top of the stage 2/3 renderer: **two complete
lighting paths** (ray traced and rasterised), shadow maps, and the
post-processing stack (HDR, tonemapping, AA, bloom). Ray-traced lighting,
rasterised lighting, shadows and the HDR/tonemap/FXAA core are all **MVP**;
bloom follows at P10; TAA is post-MVP.

## Lighting: two complete paths (MVP)

**Ray-traced lighting is MVP, and so is a full rasterised twin of everything it
does.** `LightingPath` (see [39-capabilities.md](39-capabilities.md)) selects
between them per device, and the selection degrades: a device without
`RAY_QUERY` and `ACCELERATION_STRUCTURE` gets `Rasterised` and a complete
picture that merely looks worse.

This is a deliberate and expensive choice. It is made because **ray tracing is
Vulkan and D3D12 only** — WebGPU has no ray tracing at all, and Slang cannot yet
emit ray tracing for the Metal target, so macOS, iOS and every browser run the
rasterised path. Those are not edge platforms, so the raster twin is not a
degraded fallback nobody looks at; it is the path most players will see.

**Correction (2026-08-23): the arithmetic behind "expensive" has changed, and
the conclusion has not.** `crcbl-dx12` was deferred on 2026-08-21 along with
`crcbl-mtl` (see [09-backends-metal-dx12.md](09-backends-metal-dx12.md)), so of
the two backends that can ray trace, one is parked. Every backend under active
work — `crcbl-vk` and `crcbl-webgpu` — would run the rasterised path on every
device except Vulkan hardware with `RAY_QUERY`. That makes the raster twin
**more** clearly the right call, not less: it is no longer the path most players
will see, it is very nearly the only path.

What it does change is P7C's own justification, which this table does not carry
and should not be read as carrying. A ray-traced path is a whole second lighting
implementation reaching one of four backends while two of the other three are
deferred, and whether that is worth building next is a scheduling question for
the roadmap rather than a design question for this document. `docs/backlog.md`
is where that decision belongs.

| Effect              | `RayTraced`                         | `Rasterised`                                     |
| ------------------- | ----------------------------------- | ------------------------------------------------ |
| Global illumination | ray-traced GI                       | irradiance probes + baked/ambient                |
| Reflections         | ray-traced reflections              | screen-space reflections, probe fallback         |
| Shadows             | ray-traced shadows, all light types | CSM for sun, single map for spot, cube for point |
| Ambient occlusion   | ray-traced AO                       | screen-space AO                                  |

Rules that keep the two from diverging into two renderers:

- **One material model, one BRDF, one set of inputs.** Both paths shade with the
  same material table (topic 37) and the same lighting maths; what differs is
  how visibility and incoming radiance are _gathered_, never how they are
  _shaded_.
- **One tonemapped output target.** The post stack below runs identically after
  either path, so nothing downstream branches on `LightingPath`.
- **Golden images per path**, and a documented pair-wise comparison: the two
  paths are not expected to match pixel for pixel, but a scene that reads
  correctly on one and wrongly on the other is a defect in whichever is wrong.
  The comparison is a human-reviewed reference, not an automatic tolerance.
- **Acceleration structures are built regardless of who consumes them** where
  the device supports them — BLAS per mesh asset at bake or load, TLAS refit per
  frame from the same instance data the cull pass reads. Topic 24 and topic 13
  are the other potential consumers; neither is MVP and neither may assume the
  structure exists.

## Many lights: the list and how it is gathered (decided 2026-08-13)

The table above names shadows for spot and point lights, and when this section
was written **the engine had exactly one light** — a single `DirectionalLight`
(direction, colour) in the frame block. There was no light list, no light
culling and no count budget specified anywhere, so the shadow rows above were
not implementable as written. This section is that missing half, and it is
built: `crcbl_render::light` turns a `DirectionalLight` and each `Light::Point`
/ `Light::Spot` into `GpuLight` rows, and `crcbl_render::light_grid` runs the
compute pass that assigns them to the froxel grid `mesh.slang`'s fragment stage
indexes.

### Clustered forward

Lights live in an SSBO of rows, exactly as instances and materials already do,
and a **compute pass assigns them to a froxel grid** — screen tiles by depth
slices — which the fragment stage indexes by its own position. This is the same
discipline §3.3 already uses for instances: a compute pass produces compacted
lists, the draw reads them, the CPU uploads deltas and nothing else.

Chosen over the alternatives for reasons that are about this engine rather than
taste:

- **Tiled / Forward+** is simpler and degrades badly with depth range: a tile
  spanning a near wall and a far skybox gathers every light in between. The
  samples that motivate lighting (lantern, towers) are exactly that shape.
- **Deferred** conflicts with two rules already locked here: "one material
  model, one BRDF, one set of inputs" shared with the ray-traced twin, and the
  post stack running identically after either path. It also fights MSAA and
  transparency, and it would make the raster path structurally unlike the
  ray-traced one, which is the divergence this topic exists to prevent.
- **Clustered forward needs nothing of a device** — a compute pass and two
  storage buffers — so it is the same code on all four backends, which is the
  constraint every other path in this engine is held to.

### What it costs and what is budgeted

- **A light is a row**: position, radius, colour premultiplied by intensity,
  type, and for a spot its direction and cone angles. A directional light is a
  row too, flagged as affecting every cluster, so the sun stops being a special
  case in the shader.
- **A cluster holds a bounded number of light indices.** Overflow is **counted
  and reported**, never silently dropped — topic 40's counters are where that
  number surfaces, and a scene that overflows should be visible in the debug
  panel rather than mysteriously dark.
- **Shadowed lights are a subset, and a small one.** Shadow atlas space is the
  scarce resource: the sun's cascades, then a stated number of spot maps and
  point cube maps, chosen by a rule the frame can state (nearest, brightest,
  largest screen influence — the rule is the next slice's decision and belongs
  in this file when taken). An unshadowed light still lights; it just does not
  occlude.

### Shadowed lights: the three decisions (taken 2026-08-13)

The list exists now, so these are settled.

- **Point lights use six atlas tiles, not a cube map.** The sun already renders
  into a tile atlas, so six tiles reuse one allocator, one image, one sampler
  and one barrier story; a cube map is a second image type, a second view type
  the seam would have to carry, and a second sampling path. What that costs is
  hardware filtering across a face seam, which a tile atlas cannot do —
  mitigated by a border of padding per tile and by the fact that PCF already
  samples within a face. **A cube map is the better answer only if seam
  artefacts turn up in practice**, and then it is a contained change to the
  sampling side.
- **Shadowed lights are chosen by projected screen influence**, radius over
  distance to the eye — the same metric family LOD selection already uses, so
  there is one notion of "how much does this matter on screen" rather than two.
  Ties break by light index so a frame's selection is stable rather than
  order-dependent, and hysteresis on the selection is owed for the same reason
  it was owed for LOD: a light drifting across the cutoff should not flicker its
  shadow on and off.
- **The atlas is a fixed tile grid.** The sun's cascades take the first tiles,
  and the rest are handed out one per spot and six per point until they run out.
  A light that gets no tile **still lights and simply does not occlude**, which
  is the honest degradation and is what makes the budget a quality knob rather
  than a correctness cliff.

**Order of work: spot before point**, even though point is the MVP row and spot
is polish. A spot is one tile and one matrix — the sun's machinery with a
different projection — and a point light is six of exactly that plus face
selection. Building the simpler one first de-risks the harder one and neither is
wasted.

### A fourth, taken 2026-08-13 once spot shadows had landed

- **One cull per point light, not one per face.** Spot shadows made the cost of
  a shadowed light visible: each one needs its own `DrawGen`, and a `DrawGen` is
  roughly five megabytes — most of it per-instance LOD hysteresis state that is
  device-local and lives for the renderer's life. Six of those per point light
  is thirty megabytes for one light, which is not a budget, it is a leak with a
  schedule.

  A point light's six faces share a frustum in the only sense that matters: the
  union of them is the light's sphere, and the sphere is what the cull already
  tests against. So a point light gets **one** `DrawGen` culling against its
  radius, and its six faces each draw that one visible set through a different
  matrix into a different tile. A face therefore draws instances behind it,
  which the rasteriser discards — conservative, and the trade is six dispatches'
  worth of memory against a little wasted vertex work on five faces.

  This is also the shape a tighter per-face cull would refine later without
  moving anything: the visible set is already per-light, and narrowing it is a
  change to one dispatch rather than a change to how many there are.

### A fifth, taken 2026-08-14: the sun's bias is denominated in texels

The sun's shadow comparison used to be biased in **shadow-clip depth**, on the
argument that an orthographic projection distributes depth linearly so one
number means one world distance everywhere in the map. True, and exactly the
defect: the distance it meant was that number times the cascade's whole depth
range, which is `2 · radius + CASTER_REACH`. On the outer cascade that is 88 m,
of which 40 is caster reach — so a bias of 0.0094 in clip was **0.83 m of world
slack** against walls 0.15 m thick, and it grew whenever a scene needed casters
to stand further off the near plane.

`sun_visibility` now does what `punctual_visibility` already did: it offsets the
world position towards the light before projecting, by a multiple of the world
footprint of one texel of the cascade the fragment landed in
(`2 · radius / TILE`). The two light types are biased in one unit, and the
number scales with the map's resolution instead of with a caster budget. It also
means a **near** cascade is biased proportionally less than a far one; the old
denomination had that backwards, giving cascade 0 the larger world slack of the
two because its depth range is a larger multiple of its texel.

Measured on radv (and confirmed on llvmpipe) through `apps/lantern`'s 1280×960
review frames, which is the tree's grazing-sun fixture — `N·L` of 0.30 on the
floor, a slope of 3.17, and a 0.15 m shell for the slack to show past:

| Artefact                              | Before  | After   |
| ------------------------------------- | ------- | ------- |
| Lit strip at the `-x` wall's foot     | 0.601 m | 0.375 m |
| Lit band down the back wall's left    | 0.579 m | 0.368 m |
| Sawtooth band at the back wall's head | 0.113 m | 0.051 m |

The sawtooth also fell from 112 luma above the correctly shadowed wall to 5.7,
which is the part of it a reviewer sees: a bright cornice became a dotted line.

**What stopped it going further is the dunes patch, and that is worth writing
down.** With the constant term at half a texel the strip measures 0.140 m — a
quarter of what it was — but `crcbl_render::scene::demo`'s dunes patch then
self-shadows in a cross-hatch on its own triangulation. That patch is an
analytic height field sampled onto one-metre quads and shaded with the field's
_exact_ normal, so `tan(acos(N·L))` describes a surface the triangle underneath
does not have, and the slope term is too small by however much the two disagree.
Covering it takes about five texels of constant bias that no slope can predict,
and every scene pays for it. **The trade this table records is superseded by the
sixth decision below**, which removed the cross-hatch it is a schedule of; it is
kept because the sixth's own table is read against it. All at a slope
coefficient of 2:

| Constant, in texels | lantern's strip | dunes' valley floor |
| ------------------- | --------------- | ------------------- |
| 0.5                 | 0.140 m         | heavy cross-hatch   |
| 1                   | 0.160 m         | cross-hatch         |
| 2                   | 0.203 m         | cross-hatch         |
| 3                   | 0.244 m         | faint cross-hatch   |
| 4                   | 0.289 m         | a trace             |
| 5                   | 0.330 m         | clean               |
| 6 (shipped then)    | 0.375 m         | clean               |

Five is where the trace stops being visible in the dunes frame; six is that with
margin, and the margin costs four and a half centimetres of lantern's strip.

The next move on this path is a bias driven from the **geometric** normal, or a
normal-offset bias — either would let the constant fall back and take the strip
with it.

### A sixth, taken 2026-08-14: the slope is read off the facet, not the shading normal

That next move, built. `mesh.slang`'s `geometric_normal_of` takes the normal of
the triangle the rasteriser actually drew —
`cross(ddx(world_position), ddy(world_position))`, whose two derivatives span
the facet's tangent plane — and `shadow_slope` reads `tan(acos(Ng·L))` off it
for both `sun_visibility` and `punctual_visibility`. The facet is computed in
`fragmentMain` and passed in rather than taken inside those functions, because
`ddx`/`ddy` exist only in a fragment stage and a function that silently depends
on where it is called from is worse than a parameter.

**The sign is aligned to the shading normal rather than hard-coded, and that is
load-bearing.** Which way the bare cross product points depends on the target's
screen-space Y direction and on the primitive's winding, and this file compiles
to four targets. Measured through the SPIR-V artifact on radv by writing
`0.5 + 0.5 * dot(normalize(cross(ddx, ddy)), N)` to the lit target: a scene of
flat slabs read exactly 0 everywhere, so the bare cross product is
**anti-parallel** to the authored normal there, and negating it read exactly 1.
A hard-coded `cross(ddx, ddy)` would have been the negation of a bias on that
target. The other three were not measured.

**Both light types read the same normal.** Acne is a property of the triangle
rasterised into a map, and a punctual map rasterises the same triangles the
sun's does, so a light type reading a different normal for the same surface
would need bias constants of its own for no reason anyone could state.
`PUNCTUAL_DEPTH_BIAS_TEXELS` and `PUNCTUAL_SLOPE_BIAS_TEXELS` are unchanged and
no punctual golden moved: `Scene::SpotShadow` and `Scene::PointShadow` receive
on a flat floor, where the facet and the shading normal are the same vector.

What it removed from the dunes is the **broad cross-hatch** over the valley
floors. What is left at a low constant is a different artefact — a dotted
hairline along a facet _seam_, where two triangles of different slope are biased
by different amounts and the texel their shared edge falls in stores the steeper
one's depth. No slope read off either facet predicts the other's, so a constant
is still what covers it; it is simply a much smaller one. The re-measured trade,
slope coefficient still 2, on radv:

| Constant, in texels | lantern's strip | dunes, shading normal | dunes, facet normal |
| ------------------- | --------------- | --------------------- | ------------------- |
| 0                   | 0.128 m         | heavy cross-hatch     | seam on most edges  |
| 0.5                 | 0.149 m         | —                     | seam on many edges  |
| 1                   | 0.170 m         | —                     | seam on some edges  |
| 2                   | 0.213 m         | faint cross-hatch     | a few isolated dots |
| 3 (shipped)         | 0.256 m         | —                     | clean               |
| 6 (shipped before)  | 0.382 m         | clean                 | clean               |

Graded from the 1280×960 frame and from the golden's own 256×192, which is where
the aliasing is worst. The lantern column does not depend on which normal is
read: that room is built of flat slabs, so its frames at 6.0 are
**byte-identical** either way, which is also the cleanest evidence that the
change does nothing except where the two normals disagree.

**Three is shipped with no margin above it**, deliberately unlike the six it
replaces. Six was one over the first clean value because what it covered was an
unexplained shortfall; three covers a bounded, understood quantity, so margin in
it is lantern's strip bought back for nothing.

Re-measured through `apps/lantern`'s 1280×960 review frames, on the same
fixtures as the fifth decision's table:

| Artefact                            | Texels off `N` | Texels off `Ng` |
| ----------------------------------- | -------------- | --------------- |
| Lit strip at the `-x` wall's foot   | 0.382 m        | 0.256 m         |
| Lit band down the back wall's left  | 0.373 m        | 0.244 m         |
| Cornice lift over the shadowed wall | 61 luma        | 21 luma         |

The two "texels" columns are this session's own re-measurement of the shipped
tree either side of the change, taken as the half-fall of a luma profile walked
out from the wall; the fifth decision's 0.375 m and 0.368 m are the same
artefacts measured by that session and are quoted here unchanged. The cornice
figure is a **peak** luma over the correctly shadowed wall below it, and it does
not reproduce the fifth decision's 5.7: on the shipped tree, before this change,
the band under the ceiling peaks 61 luma over the wall at 2.96 m and 0.08 m
tall, which is nearer that decision's _before_ figure than its after. Whatever
statistic produced 5.7 is not recorded and was not recovered, so the two rows
above are a matched pair and the 5.7 is not comparable with either.

Goldens moved: `apps/lantern/tests/golden/room.png` (145 of 49152 pixels, all in
the three fixtures above plus the metal block's and plinth's contact shadows)
and `crates/crcbl/tests/golden/dunes.png` (19 pixels, all along shadow
terminators on the dune flanks). Every other golden in the tree still matches
within tolerance and was left alone.

## The BRDF the "one material model" rule names (decided 2026-08-13)

The rule at the top of this file — "one material model, one BRDF, one set of
inputs" — had no content behind it. `mesh.slang` shaded with Lambert plus a
Blinn-Phong lobe whose exponent and strength were two `static const` floats,
`SPECULAR_POWER = 32.0` and `SPECULAR_STRENGTH = 0.35`, so there was exactly one
material in the engine however many rows the table held. That is the state the
SSR row above cannot be built on: screen-space reflections have to know which
pixels reflect and how sharply, and nothing in the engine could say.

So the material row grew `metallic` and `roughness` — glTF's own two, under
their own names — and the lobe became **one Cook-Torrance GGX lobe** driven by
them: Trowbridge-Reitz `D`, Smith height-correlated visibility, Schlick's
Fresnel, Lambert diffuse. The two constants are deleted, not left beside it.

Why GGX now rather than a roughness-driven Blinn later:

- **The rule is the reason.** A roughness-driven Blinn would be a second
  material model, and P7C's ray-traced twin shades with the same one — so it
  would be written once and rewritten once, and in between the two paths would
  be comparable only by eye.
- **glTF already speaks it.** The importer reads `metallicFactor` and
  `roughnessFactor` off the accessor it was already holding for the base colour,
  so an imported material means what its author meant with no mapping in
  between.
- **It costs nothing a device has to have.** The lobe is dot products, a square
  root and two divides. No `pow` with a variable exponent and no trigonometry,
  for the reason the rest of `mesh.slang` avoids them: a platform's
  transcendental functions differ in the last place between the four targets and
  this file's determinism argument is that it does not use any.

Two consequences worth stating before somebody meets them:

- **A metal has no ambient term, and that is the model rather than a bug.**
  Ambient scales the diffuse albedo, and a conductor's diffuse albedo is zero —
  what a metal owes the room is a reflection, not a scatter. So a fully metallic
  surface out of every light's reach is **black** until it has something to
  reflect, and the two rows above that give it one are exactly SSR and
  irradiance probes. Nothing regresses today: `GpuMaterial::UNTINTED` is
  `metallic 0.0` and no scene in the tree sets one higher.
- **The engine's Lambert term carries no `1 / pi`, so neither does the specular
  one.** Trowbridge-Reitz normalises to `alpha2 / (pi * shape * shape)` and
  Lambert to `albedo / pi`; this engine's diffuse is a bare `albedo * N·L`, the
  convention where a light's intensity has absorbed the reciprocal. The `pi` is
  therefore folded out of `D` as well, so the ratio between the two lobes is the
  physical one. Writing the textbook `D` against this diffuse would put every
  highlight a factor of `pi` under the surface it sits on, which is not a look
  anyone chose — and it is what keeps a roughness of a half close to the Blinn
  exponent of 32 it replaced.

## Shadows (MVP — lands with P7)

- **Sun: cascaded shadow maps** (CSM), 2–3 cascades, stable (texel-snapped)
  projections, PCF filtering (3×3 MVP). One directional light with shadows was
  the MVP contract — it's what makes 3D scenes read as 3D — and the light list
  above has since taken the engine past it: `crcbl_render::shadow` budgets the
  sun's cascades, a spot's cone and a point's faces out of one atlas.
- **GPU-driven all the way**: shadow pass reuses the stage 3 compute culling
  (one cull dispatch per cascade against the same instance/geometry pools,
  indirect draws into depth-only pipelines). No CPU re-traversal per cascade —
  the shadow cost scales like the main pass, by design.
- Render graph: cascades = depth targets owned by the graph; barriers/layout
  automatic like every pass. **The debug half of this row is unbuilt**: there is
  no cascade-split overlay and no shadow-map inspector panel.
  `ForwardRenderer::debug_view` offers three views — the LOD screen-error
  heatmap, the DAG-level tint and world-space normals — and none of them is
  about shadows.
- Skinned casters (topic 17) come free via the skinned-output pool region.
- **Spot and point shadows are built, and not as this line first described
  them.** The 2026-08-13 decision above replaced the cube map with six atlas
  tiles, so a point light is `crcbl_render::shadow`'s `POINT_FACES` tiles of the
  grid the cascades already sit in rather than a second image type and a second
  sampling path. What a frame budgets is therefore tiles and cull slots —
  `LIGHT_TILES` and `LIGHT_SLOTS` in that module, which the 2026-08-26 re-tiling
  of the atlas widened to two point cubes and two spot cones by shrinking the
  tile rather than growing the image. Static-geometry caching (cached cascades)
  stays post-MVP, for when a sample's perf numbers demand it.
- **Under `LightingPath::RayTraced` this whole section is bypassed**, not
  augmented: shadows come from ray queries against the TLAS, for every light
  type, with no cascades and no shadow atlas. The two are alternatives, which is
  what keeps the raster path from acquiring ray-traced special cases.
- Path note: identical on every `GeometryPath` — depth pass plus whatever emit
  tail the device selected; nothing in the shadow path depends on the binding
  model.

### The quality ladder, taken 2026-08-27

What ships, first, because the ladder is only readable against it: **stable
sphere-fitted cascades snapped to texels** — `crates/crcbl-render/src/shadow.rs`
fits a sphere around the eye rather than a box around the frustum, so rotating
the camera cannot change a cascade's extent, and quantises the light-space
origin to whole texels — **3×3 hardware PCF through a comparison sampler**,
which is `mesh.slang`'s `tile_pcf` taking nine `SampleCmpLevelZero` taps one
atlas texel apart and dividing by nine, the texel-denominated bias of the fifth
decision, the geometric-normal slope of the sixth, and the **2026-08-26
re-tiling** that bought a second point light by shrinking `SHADOW_TILE` rather
than growing the image.

**The re-tiling has a measured cost, and it is not hypothetical.** Since that
change the `cube` browser-path golden fails on linux and windows: **64 grossly
wrong pixels** against the budget of 49 that
`crcbl_golden::Tolerance::RASTERISER`'s `max_gross_ratio` of one in a thousand
allows a 256×192 frame, at a **max channel delta of 216**, with an SSIM of
**0.998945** — which clears that tolerance's floor of 0.99, so the picture is
structurally the same picture and the failure is localised rather than a frame
that stopped drawing. The diff is scattered noise along shadow edges: the cube's
face gradients, and the pyramid's edges. macOS passes. **This is unresolved**,
and it is recorded here as evidence that the tile is now the binding constraint
on shadow quality — every map is 768 texels a side where it was 1024 — not as a
defect with a fix attached.

The ladder, in the order it should be climbed:

- **Normal-offset bias.** The fifth decision's own closing line named it and it
  is still the cheapest real win: offsetting the receiver **along its normal**
  before projecting moves the sample sideways across the map rather than moving
  the surface towards the light, so it removes acne without buying the
  peter-panning a depth bias buys. What it earns back is the constant term the
  sixth decision's table prices in centimetres of lantern's lit strip.
- **Cascade cross-fade.** The switch between cascades is hard today, so wherever
  two cascades meet there is a seam — and the fifth decision made it _more_
  visible, not less, by biasing a near cascade proportionally less than a far
  one. A band of overlap blended by the split distance is the standard answer,
  and it costs a second `tile_pcf` inside the band and nothing outside it.
- **A rotated Poisson-disc PCF kernel.** A wider penumbra at the same tap count,
  which a 3×3 box cannot trade for at any price. **The rotation must be an
  integer-indexed constant table**, for the reason the AO section gives at
  length: a per-pixel float hash amplifies by construction exactly the driver
  differences a golden cannot absorb, and a shadow comparison is every bit as
  binary as an AO sample.
- **PCSS, or contact hardening.** A blocker search over the map, then a filter
  whose width comes from how far the blockers it found are — the industry
  standard soft shadow, and the first rung here that costs a **second sampling
  loop** rather than a different kernel in the one loop that exists. It is also
  the first that makes the tile resolution above bind harder, since a blocker
  search reads a neighbourhood the re-tiling already made coarser.

Refused, with the reasons:

- **VSM and EVSM.** Storing moments makes a shadow map filterable, so it can be
  blurred, mipped and sampled cheaply — and it **light-leaks through thin
  geometry**, because two depths summarised into one distribution admit a
  receiver between them. A leak is a **correctness** artefact rather than a
  quality one: it puts light where the scene has none, which reads as a hole in
  a wall and not as a soft edge. Every rung above trades quality for cost; this
  one trades the thing the pass exists to compute.
- **Virtual shadow maps.** The modern answer, and far too large to sit inside a
  quality pass: it is a page table, a feedback buffer saying which pages a frame
  actually sampled, and a cache keeping the pages a static scene did not
  invalidate. That is a topic, not a rung — it would **replace** the fixed tile
  grid this module is built on rather than improve it — so if it is ever wanted
  it gets its own document.

## Screen-space AO: what the one-line row was missing (decided 2026-08-13)

> **Built 2026-08-14; what follows is the survey that preceded it.** Two of the
> three blockers below have since gone and the section says so further down —
> `forward.rs` has a depth prepass (`prepass_groups`), and the ambient term is
> separable. Kept because the survey is why the row was not implementable
> earlier, not because any of it still blocks.

The table above says "screen-space AO" and nothing else, and — exactly as the
shadow rows were not implementable before the light list existed — that row sat
on three things this engine did not have:

- **There is no depth prepass, and the depth buffer is built never to be read.**
  `TransientImageDesc::scene_depth` carries `DEPTH_STENCIL_ATTACHMENT` and
  nothing else, and its doc says "never sampled"; the forward pass attaches it
  with `clear_depth`, which discards on store.
- **The ambient term is unseparable after the forward pass.** `mesh.slang`
  computes `albedo * (ambient + direct) + gloss` in one line to one target, so
  anything downstream can only scale all three — and AO must darken ambient
  alone.
- **`LightingPath` has no consumer.** It is read by `Debug` impls, one log line
  and adapter tests. Only `GeometryPath` branches anything, so "the rasterised
  twin's AO" cannot be gated on the selector this file names.

So **the AO row is a depth-prepass row**, and the prepass is the structural
part. SSR will want the same depth, so it is a P7B cost that AO happens to pay
first.

### The decisions

- **Add a depth prepass, and it is unusually cheap here.** `shadow_pipeline` is
  already the depth-only twin of the colour pipeline, built from the same
  modules and the same layout; driven with the camera's bind group and the
  camera's already-generated draws it _is_ a scene depth prepass — no new
  pipeline, no new shader, no new bind group. `PassBuilder::depth_read` and
  `DepthStencilState::equal_depth_read_only` both already exist with no
  production caller, and `graph_compile.rs` already asserts the
  `DepthStencilWrite → DepthStencilRead` barrier for exactly this pair.
- **Reconstruct normals from depth; do not add a normal attachment.** Under this
  ordering an attachment is not merely MRT bandwidth — the prepass has no colour
  target at all, so it would mean a third geometry pipeline per `GeometryPath`,
  a new fragment entry point compiled to four targets, and a new `VertexOutput`
  consumer, for a buffer one pass reads. Use the four-tap closest-neighbour
  reconstruction rather than the two-tap `ddx`/`ddy` one: the naive version
  straddles the depth discontinuity at every silhouette and draws a dark rim
  around every object. Escalating later is contained to the prepass pipeline and
  the AO shader's first ten lines, and the code should say so.
- **AO is produced before the forward pass and consumed inside it**, as an
  integer texel fetch by `SV_Position.xy` multiplying `frame.ambient.rgb` alone.
  The shader already indexes a screen-space structure that way — `froxel_of`
  takes `SV_Position.xy` — and a `Load` needs no sampler, no UV and no
  filtering, which is one less thing for four backends to disagree about.
  **Multiplying the tonemap's input is refused**, and refused in writing because
  the one-line row invites it: it darkens direct light and highlights.
- **Classic normal-oriented hemisphere SSAO, eight samples, a sixteen-entry
  constant rotation table indexed by `pixel.xy & 3`, and a 4×4 blur over the
  result** — a box in the first slice, depth-weighted in the second (below). Not
  GTAO yet — its horizon integral is several times the work for quality nobody
  can resolve at the goldens' 256×192, and CI's rasterisers are software.
  Upgrading is a change to one function in one shader, the same shape
  `tonemap.slang` already documents for its curve; the pass, the resource, the
  binding and the test are unchanged.
- **The rotation comes from an integer-indexed constant table, never a float
  hash**, and **the blur is not optional**. This is the determinism rule and it
  is why the design looks conservative. Each AO sample is a binary depth
  comparison, so one sample landing on the threshold resolves differently on two
  drivers and swings that pixel by an eighth — far past
  `Tolerance::RASTERISER`'s delta of 2. Interleaved-gradient noise and
  `frac(sin(dot(…)))` hashes amplify float differences _by construction_, which
  is the opposite of what a golden needs; an integer index into a constant array
  is bit-identical by inspection. The blur's footprint is exactly the noise
  tile, so it removes the banding, and where all sixteen of its taps count it
  divides an isolated flipped sample by sixteen — which the depth-weighted
  kernel below is precise about, because it is no longer sixteen everywhere.
- **The golden is not the instrument.** An AO pass writing a constant 1.0 draws
  a perfectly plausible frame. The check is a **structural ratio**, in the shape
  `SPOT_SHADOW_RATIO` already uses: a band inside a concave corner must be
  measurably darker than a band on the same surface, at the same camera
  distance, with the same normal and the same distance from every light, outside
  the corner. That survives one-level driver drift and fails a no-op pass, an
  inverted normal, and a result that never reaches the shading line.
- **AO is always on, and the off-switch is data rather than a branch.** There is
  no device fact to gate on — every backend has a fullscreen draw, a sampled
  `D32Float` and an `R8Unorm` target — and inventing a capability that is really
  a performance opinion is what topic 39 exists to prevent. A renderer-owned 1×1
  `R8Unorm` cleared to 1.0, bound when the AO passes are not added, is the
  `shadow_placeholder` pattern already in the tree, and it makes a later quality
  knob a two-line change rather than a shader permutation.

  **Qualified when the switch was built (2026-08-14): the 1×1 form is not free,
  and what it costs is a line in the shader rather than a pass.** `mesh.slang`
  fetches this channel with a `Load` at the fragment's own pixel, chosen in that
  file for having no sampler, no UV and no texel-centre arithmetic for four
  backends to disagree about. A `Load` outside a texture's extent yields
  **zero**, not its one texel, so an unclamped fetch reads a 1×1 image as total
  occlusion everywhere but the origin: the first AO-off frame drawn that way was
  black wherever ambient was the whole of the light, on real hardware, with
  nothing reporting an error.

  So the fetch is clamped — `min` against `ambient_occlusion.GetDimensions()` —
  and with that, the paragraph above ships as written. A frame with AO off
  records **no occlusion pass at all** and takes no frame-sized image out of the
  transient pool; every property the placeholder was chosen for survives, and
  the clamp costs nothing on a frame-sized channel, where every fragment is
  inside the image already. `crcbl`'s `forward_e2e::depth_probe` is what asks
  whether the clamp is there, on every backend: it binds the same one-texel
  image to a frame of `MESH_EXTENT`, darkens the light list until ambient is the
  whole of the colour, and asserts the ambient term arrived.

  A frame-sized transient cleared to 1.0 by an `ssao-none` pass was what shipped
  first, on 2026-08-14, before the shader could be edited. It was correct and
  strictly more expensive, and it is gone.

### Risks this carries

- **The forward pass keeps clearing and writing depth**, and the first slice
  took that deliberately. `LoadOp::Load` with `Greater` — which an earlier draft
  of this section implied — **cannot work**: the prepass has already written the
  identical depth and `Greater` rejects equality, so every fragment dies and the
  frame is black. `GreaterOrEqual` works and is the version that buys the
  overdraw win, but it reintroduces the invariance risk below. Clearing is the
  only zero-risk form, and it is why `spot.png` moved by exactly zero pixels
  when AO landed.
- **Depth invariance**, which is what the overdraw win costs. `GreaterOrEqual`
  in the forward pass needs its `SV_Position.z` bit-identical to the prepass's.
  Same module, same entry point, same matrix, but two pipelines can be compiled
  differently and a marginally farther fragment is rejected, which looks like
  holes. Nothing in the shaders carries an invariance decoration. The four CI
  rasterisers are the measurement; the zero-risk fallback is to keep the forward
  pass writing depth and forgo the overdraw win, and **that fallback is taken by
  saying so in the code, never by re-blessing a golden around it**.
- **A `Load` on a depth texture with no sampler** is the corner this engine has
  already been bitten in once, over `DepthTexture2D` versus `Texture2D<float>`.
- **The box blur bleeds AO across silhouettes** as a halo. A bilateral blur is
  the fix and was deliberately deferred to the slice after the first frame
  exists — the section below is that slice.

### The depth-weighted blur (decided 2026-08-13)

The first slice shipped the box, and the risk above is what it cost: a box
kernel averages a foreground pixel's occlusion with a background that is not the
same surface, and the far plane is written "fully unoccluded", so every
silhouette in the frame carries a bright fringe exactly one kernel deep.
Replacing the kernel is a change to `ssao_blur.slang` and its bind group, which
is what the first slice said it would be.

- **Weight on view-space Z, never on the raw reversed-Z delta.** A depth
  difference is not a distance: the same one-metre gap is an enormous reversed-Z
  delta in front of the eye and almost none near the far plane, so a tolerance
  on the stored value would be a different filter in every part of the frame.
  The blur unprojects, exactly as `ssao.slang` does.
- **So the blur binds the same `SsaoParams` block**, rather than growing one of
  its own: `inv_proj` and the radius are already written there once per frame.
  The consequence is that the blur's bind group names a per-frame buffer, so its
  cache became a ring for the reason the occlusion pass's already was — a single
  cache keyed on the views hands the even frames' block to the odd ones. The
  helper both passes share now keys on every view it was given rather than on
  one, because the blur's group names two transients and is stale when either
  moves.
- **The weight is a ramp and never a cut.** `if (abs(dz) < threshold)` would put
  a _binary_ decision on the output pixel, which is precisely what the rotation
  table spends its whole argument keeping off the input samples: two drivers
  resolve the borderline case differently and the entire pixel jumps.
- **The tolerance is derived from the AO radius, not a new uniform field.** The
  radius is the only length these two passes have — `ssao.slang` gathers within
  it and its falloff is at full strength inside it — so it is already this
  pair's answer to "are these two pixels near enough to be occluding each
  other". A knob nobody adjusts would be a fourth thing the Rust mirror has to
  agree about for a number that is not free to move.
- **The far plane is the halo's mechanism, and its test is the one comparison
  that stays.** A far tap gets no weight and a far centre returns 1.0 unchanged,
  as `ssao.slang` already does at the same pixel. That test compares against an
  exact constant rather than between two computed depths, so two drivers either
  both take it or neither does.
- **What the division by sixteen is worth now, in writing.** It is the full
  sixteen wherever every tap counts, which is any surface facing the camera and
  therefore most of a frame — and it falls towards one exactly where taps are
  rejected, at a silhouette and at the far plane. The trade is deliberate: the
  taps a box spent there were a halo in every frame, and what is given up is
  margin against a driver disagreement that may be in none of them.
- **The observable is in `Scene::Cube`, and it is not the scene named after
  AO.** `Scene::Ao` looks into a closed trough, so every pixel of that frame is
  geometry: it has no far plane to bleed and no silhouette to bleed across, and
  the kernel change moves it by one channel level in a couple of hundred pixels.
  The cube frame has the plain pyramid's underside — one flat normal pointing
  down, one flat albedo, and no direct light on it, so its pixels are the
  ambient term times the occlusion and nothing else. The band along its
  silhouette measures about a thirteenth over the band two rows in with a box
  kernel and about a fortieth with this one, on both of the rasterisers it was
  run on.

### GTAO, taken 2026-08-27: the ground the refusal stood on has moved

The decision above says "Not GTAO yet" and
`crates/crcbl-shaders/shaders/ssao.slang`'s header says the same in its own
words — the horizon integral is several times the work for quality nobody can
resolve at the goldens' 256×192, and CI's rasterisers are software. **That is a
cost argument, and it never weighed the thing this section spends its longest
paragraph on.**

What ships sums **binary** depth comparisons. This section's own determinism
rule is that one such comparison landing on the threshold resolves differently
on two drivers and swings that pixel by an eighth, which is why the rotation is
a table and why the blur is not optional. GTAO's horizon-visibility integral is
**continuous**: a driver disagreeing in the last bit moves a horizon angle by a
hair and the occlusion with it, where a binary sum cliffs. **So GTAO degrades
gracefully exactly where the shipped pass cliffs, which makes it better for the
goldens rather than worse** — and that, rather than a quality opinion, is what
reopens it.

**The upgrade is contained where the shader's header already says it is.**
`occlusion_at` becomes the integral, and `ROTATIONS` becomes a slice-offset
table indexed the same way, because the rule that a rotation is an
integer-indexed constant and never a float hash survives the technique change
untouched. The pass, the `R8Unorm` resource, the binding, `ssao_blur.slang` and
the structural-ratio test do not move.

**Bent normals are the second half, and they are what make this worth more than
a quality bump.** A scalar occlusion can only _scale_ the ambient term; a bent
normal is a direction the ambient term can be sampled _along_, which is exactly
the hook the irradiance-probe section left open — `probe_irradiance` already
takes a normal and would take that one. It is also the honest route to specular
occlusion, which the SSR section refuses outright and refuses **correctly**: a
scalar AO is the wrong term for a reflection, and a bent normal with a cone
angle is the right one. That refusal stands until this pair exists.

**SSAO stays as the cheap tier** rather than being deleted, on the antialiasing
ladder's own FXAA-under-SMAA pattern: eight taps and a comparison is a real
budget on a software rasteriser and on a small device, and the two techniques
share the pass, the resource, the blur and the test.

Refused, with the reasons:

- **HBAO and HBAO+.** They read the same depth and are superseded by GTAO on it,
  so building one is a step onto a rung that is already obsolete. Nothing is
  learned on the way up that the destination does not already contain.
- **Any AO that needs a normal attachment before the SSR section's escalation
  clause actually fires.** That clause names its own trigger — a fringe of
  unrelated colour one pixel deep at silhouettes — and the attachment is its
  remedy, not AO's wish. For AO a wrong reconstructed normal costs a pixel an
  eighth of its occlusion, which is the budget this section already declined to
  spend a colour target on.

## Screen-space reflections (decided 2026-08-14)

The third P7B row. SSR runs after the forward pass, so the only per-pixel data
are the depth buffer, the `Rgba16Float` scene colour and the reflectivity
attachment — and a reflection needs to know its coloured `F0` and whether its
lobe is narrow enough for a screen-space ray.

### The AO section's refusal of an attachment does not transfer

The paragraph above refuses a normal attachment because "the prepass has no
colour target at all, so it would mean a third geometry pipeline per
`GeometryPath`, a new fragment entry point compiled to four targets, and a new
`VertexOutput` consumer". **Every clause of that is a fact about the depth
prepass**, which is built from the shadow pipeline with no fragment stage and no
colour targets. On the **forward** pass none of it holds: both forward pipelines
already take one `ColorTargetState` array and both name the same fragment entry,
so a second target is one array element and no new pipeline, no new entry point
and no new interpolant. Recorded here so the refusal is not applied by analogy
to a pass it was never about.

### The decision

- **One new colour attachment on the forward pass: `Rgba8Unorm`, `rgb = F0`,
  `a = sharpness`.** Sharpness is the clamped screen-march ramp
  `1 - roughness / ROUGHNESS_CUTOFF`: zero means the surface keeps its probe
  environment but cannot honestly launch one screen-space ray. Encoding the
  endpoint rather than reconstructing it from quantised roughness is
  load-bearing — `0.5` may round to either neighbouring byte, while zero
  survives every `Rgba8Unorm` backend exactly. `max_color_attachments` is 4 on
  the minimum capability profile.
- **It carries the two values the downstream reflection pair consumes.** `F0`
  colours Fresnel for every surface. Sharpness gates the march and controls how
  strongly the blur moves from the direct centre fallback to filtered SSR. The
  original roughness remains in the material row for the forward GGX lobe; the
  attachment does not pretend a single screen-space ray can evaluate a broad
  lobe.
- **The normal stays reconstructed from depth**, sharing the AO pass's four-tap
  function — but see the escalation clause below, because the cost of a wrong
  normal is not the same for the two features.
- **The pass is the composite.** It reads scene colour, depth and reflectivity
  and writes their sum into a second `Rgba16Float` transient, which `add_passes`
  returns in place of the scene colour. One pass, no blend state, no feedback
  loop. A frame that does not add it returns the old id and the picture is
  bit-identical — the same data-not-a-branch off-switch AO has, needing no
  placeholder because nothing in `mesh.slang` reads the result.

### What is refused

- **Packing into the scene target's alpha.** Nothing reads it today — the
  tonemap samples `rgb` and writes a literal 1.0 — but one channel cannot carry
  a coloured `F0` and a roughness, so the packing is a scalar-reflectance design
  wearing a bandwidth argument. It also takes the name away from a channel that
  already has one, and that transparency will want.
- **A material-id channel with the pass reading the table itself.** Exactly
  right for untextured materials and exactly wrong for textured ones: the
  fragment stage multiplies the row by the vertex colour and the page texel, and
  a metal's base colour **is** its `F0`.
- **A G-buffer.** This attachment moves no shading: the forward pass still
  evaluates the whole BRDF, still reads the froxel list, still writes to
  target 0. The line to hold is that the attachment gains a field when a pass
  reads it, never because a G-buffer "should have" one.
- **Reading last frame's colour with reprojection.** Motion vectors are
  post-MVP, and a history buffer makes a golden a function of how many frames
  were drawn before it.
- **A planar reflection pass.** It would give a perfect mirror with no march,
  and it is per-plane, a second geometry pass per mirror, and useless on
  anything curved. It is the right answer for the render-to-texture camera this
  document already names, and belongs in that section.

### The escalation clause, written before it is needed

Reconstructed normals are exact on a plane and wrong on a one-pixel rim at every
silhouette, where the four-tap reconstruction keeps whichever neighbour is
nearer and at an edge that neighbour is on the other surface. **For AO a wrong
normal costs a pixel an eighth of its occlusion; for SSR a wrong normal is a
wrong ray, and a wrong ray fetches an arbitrary colour.** So: if a fringe of
unrelated colour one pixel deep appears at silhouettes, the fix is a second
attachment carrying the view-space normal, **not** a tuning of the march. That
escalation is contained to the fragment stage's return struct, one target state,
one transient and the SSR shader's first ten lines, and it moves no golden
because only the SSR pass reads it.

### The march

Screen-space DDA over the projected segment, fixed pixel stride, no jitter, no
refinement pass.

- **Screen space, not view space.** A world-unit step is tens of pixels near the
  eye and a fraction of one far away, so the same constants would be a different
  tracer in a room and on a planet. A pixel step is a pixel step everywhere, and
  it makes the loop bound a property of the screen rather than of the scene's
  scale — which matters because CI's rasterisers are software and the loop bound
  is the whole cost.
- **Amended when it was built (2026-08-14): the _reach_ is a share of the frame,
  not a fixed pixel count.** The paragraph above is right about the step and
  about the loop bound, and a first cut took it literally — sixty-four taps two
  pixels apart, so a reflection could reach 128 pixels whatever the resolution.
  That has the mirror image of the defect the paragraph refuses, one level up:
  the same scene at five times the resolution grows five times as many pixels
  between a surface and what it reflects, so the reflection got _shorter_ as the
  window got bigger. `lantern`'s panel is where it showed — the reflection its
  golden asserts at 256×192 was simply absent from the 1280×960 review frame of
  the same room. `ssr.slang` therefore derives its stride from
  `REACH_FRACTION * min(width, height) / MAX_STEPS`: the stride is still a fixed
  number of pixels along one ray, the loop bound is still a constant, the cost
  is still the same at every resolution, and a reflection is now the same share
  of the frame at every resolution rather than the same number of pixels.
- **The segment is clipped to the viewport before the walk**, so every tap is
  in-bounds by construction and a ray leaving the screen stops being a branch.
  It ends at the clipped endpoint with a **border ramp** on its weight; a hard
  stop draws a visible line where reflections end.
- **A ray that hits nothing returns the probe environment.** The same L1 table
  used for diffuse irradiance is decoded back to approximate directional
  radiance, multiplied by Fresnel, and blended against a hit by confidence. A
  zero probe volume returns exact zero and preserves the old hit multiplication
  order. This is why the table's Reflections cell says "screen-space
  reflections, probe fallback" rather than claiming screen space is complete.
- **Behind an object, the depth buffer has no information, and this is where the
  plausible wrong answer lives.** A tap says the ray is behind the _front_
  surface, not how thick that surface is. A tap counts as a hit only within a
  thickness bound; past it the tap is **no evidence and the march continues**.
  Treating any "behind" as a hit is the classic SSR smear — it reflects the
  nearest foreground object into every distant reflection and reads as a comet
  tail off every silhouette. **The thickness is derived from the ray's own depth
  advance per step**, floored by a constant, rather than being a fourth number
  the Rust mirror has to agree about.
- **No binary-search refinement.** The crossing is interpolated linearly between
  the last two taps' depth deltas. A bisection is a cascade of binary
  comparisons; an interpolation is arithmetic on two values already fetched.
- Three more, each hiding a wrong answer: **start the ray off the surface**
  along the normal or the first tap self-intersects; **clip against the near
  plane** or a ray pointing towards the camera crosses `w <= 0` and every
  projected coordinate after it is nonsense; and **fade rays pointing back at
  the viewer**, which have almost no on-screen evidence to find.

### Determinism: the goldens cannot carry this one

**The AO argument does not transfer, and the difference is quantitative.** That
pass can say a flipped sample costs an eighth and the blur then divides it by
sixteen. A march has no such denominator: the first tap whose comparison flips
**is** the answer. Two drivers disagreeing in the last bit can tap a
neighbouring pixel at the crossing, or miss the crossing entirely at the last
step — the second costs the whole reflection at that pixel.

What is still worth doing, and is not decoration:

- **No jitter of any kind**, for the rotation table's reason applied to a case
  where it matters more. Stepping artefacts get lived with or blurred; they do
  not get dithered away.
- **Every weight is continuous and goes to zero exactly where the decision is
  fragile.** A hit at the last step is at maximum distance and its fade is near
  zero; a hit near the border is on the border ramp; a tap that barely satisfies
  the thickness bound is on that ramp's low end. **The pixels where two drivers
  can disagree are, by construction, the pixels whose reflection is multiplied
  by almost nothing.** That is inspectable rather than measured.
- **The roughness gate makes the screen march identically absent on most
  surfaces.** With the cutoff at 0.5, `GpuMaterial::UNTINTED`'s 0.5 encodes
  sharpness as exact zero in `Rgba8Unorm` on every target. Such a pixel still
  receives probe environment specular, but it returns before any projected-ray
  setup or depth tap. `Scene::Probes` explicitly disables reflections because
  its absolute Rust mirror predicts diffuse irradiance alone.

  **Resolved when probe specular landed (2026-08-14):** the cutoff stays at 0.5
  and gates only the screen march. A rough conductor therefore receives the
  broad, low-frequency probe environment without pretending one projected ray
  represents its lobe, while `UNTINTED` retains an exact-zero march endpoint.
  Raising the cutoff is unnecessary unless a later fixture specifically needs
  sharper SSR on rougher surfaces.

**And the honest part**: those reduce the exposure, they do not bound it. There
is no argument that puts SSR under `Tolerance::RASTERISER` in general. So a
golden stays a review aid, every real check is a **structural ratio between two
blocks of one frame** (which one-driver drift moves together), and a fixture's
reflections must come from **large, low-frequency reflected content** — if the
reflected surface is a flat lit floor, picking the neighbouring tap changes
nothing. **If a golden flaps between CI's legs, the resolution is not to widen
the tolerance and not to re-bless per driver**: it is to make that fixture's
reflected content flatter, or to drop that golden and keep the ratio. Written
down before the first flap, because widening a tolerance will look like a
one-line fix at the time.

### Roughness

The screen-space half does **sharp mirror reflections only**. A single ray
cannot represent a wide lobe, and the failure mode of pretending otherwise is a
sharp reflection on a rough surface, which reads as a bug on sight. The
sharpness ramp is therefore a statement that the march is valid only where the
lobe is narrow; it does not gate probe environment specular, whose low-frequency
L1 result is more honest for a broad lobe than one ray.

The blur that follows is the AO blur's kernel, not a mip chain: proven in this
tree against real silhouettes, and gaining one factor — taps are weighted by how
close their roughness is to the centre's, so a mirror beside a rough metal does
not average the two. **Cone tracing over a colour mip chain is refused for this
row**: it needs mip generation on an `Rgba16Float` target and a `SampleLevel` at
a computed LOD, which is a filtered read whose level four rasterisers select
arithmetic for — the thing the AO pair spent its design avoiding. It is the
better technique and upgrading is contained to the blur pass, which the code
should say.

**Built 2026-08-14, and four things about it were not in the paragraph above.**

- **The blur had to become the composite, and the march had to stop being it.**
  A pass that adds the reflection to the scene colour leaves nothing to filter
  but the whole frame. So `ssr.slang` writes the reflection alone into an
  `Rgba16Float` transient of its own and `ssr_blur.slang` writes the sum — which
  also means the off-switch is now the pair rather than the one pass.
- **The second weight is the sharpness ramp carried through the reflection.**
  `mesh.slang` computes `saturate(1 - roughness/cutoff)` before `Rgba8Unorm`
  storage, and the march copies that value into the reflection alpha. Zero
  sharpness returns the probe fallback before march setup and the blur
  composites that centre value directly. Positive sharpness uses
  `lerp(centre, filtered, sqrt(sharpness))`, so approaching the cutoff is
  continuous while a half-sharp reflection retains enough filtering to remove
  the march's fixed-stride stepping. The linear share was measured at 8.46–8.48
  levels of mean row bend across lavapipe, WARP and Metal against the fixture's
  limit of 8; the square-root share measures 4.82 on local lavapipe.
- **The depth tolerance is the march's `THICKNESS_FLOOR` times a small
  multiplier, and the multiplier is not decoration.** `DEPTH_TOLERANCE_RADII`'s
  shape, but a floor-thickness is a much shorter length than the AO radius: at
  one of them the filter switches itself off on a floor seen at a shallow angle,
  which is exactly where a reflection lives, and the stepping survives it. Eight
  keeps the kernel at full strength across such a surface and still falls to
  nothing across a silhouette.
- **The cutoff did not move with it.** The paragraph above pairs the blur with a
  cutoff a rough conductor clears, and those are two changes with very different
  blast radii: the filter moves the frames that already reflect, and the cutoff
  puts `GpuMaterial::UNTINTED` — nearly every surface in the engine — into the
  march. They were split, and the second is its own slice with its own decision
  to record. See `docs/backlog.md`.

**What the blur is measurably worth, and what it is not.** The stepping the
march leaves is gone: `render_e2e`'s `Scene::Ssr` band bends 17.7 levels per row
with a kernel that keeps only its centre tap and 2.8 with the real one, and that
is asserted. Cross-driver divergence fell where it mattered — on the 192 pixels
of `lantern`'s room the blur changed, llvmpipe and radv disagree by at most 8,
where the unfiltered march's worst in the panel's band was 66. **The roughness
weight, though, is not separable by any assertion this tree's fixtures
support**: no fixture puts a mirror-sharp surface beside a rough one at the same
depth, which is the case it exists for. It is kept on the construction argument,
and `docs/backlog.md` carries that as a coverage gap rather than as a claim.

### What is left to later rows

No temporal anything. No Hi-Z traversal. No back-face or thickness buffer. No
half-resolution SSR — half-res AO is already owed and unmeasured, and a second
unmeasured quality-for-speed trade should not land before the timers have been
pointed at the first. No `LightingPath` gate, which still has no consumer. **No
specular occlusion**: AO scales the ambient term alone, a highlight is an image
of a light, and a reflection is an image of the room in one direction — none of
the three take the same occlusion factor, and if one is wanted it is its own
term and its own decision. And **SSR on transparency is out, with the
interaction recorded now**: a transparent surface writing the reflectivity
attachment would overwrite the opaque `F0` behind it while the scene colour at
that pixel is a blend. Every SSR has this; writing it down is what stops the
transparency row rediscovering it as a bug.

### Taken 2026-08-27: Hi-Z marching, and a colour pyramid that may already exist

The row above names "No Hi-Z traversal", and the roughness section concedes that
cone tracing is the better technique. This is the quality-and-performance pass
that collects both, and it is one slice because they share the march.

- **Hi-Z marching replaces the fixed-stride DDA.** A min-max depth pyramid lets
  a ray climb to whichever level its current cell is empty at and step across
  the whole of it, so a march crosses a frame in `O(log n)` steps where a fixed
  stride spends a constant number of taps at a constant spacing. That buys both
  ends of the trade the current march makes: the cost falls, and the reach stops
  being a fixed share of the frame, so a reflection can find something the far
  side of a room. **The determinism cost is real and is stated rather than waved
  at**: a pyramid is a reduction, so its levels are float arithmetic four
  rasterisers perform independently and the goldens have to absorb the
  difference. The mitigation is the one the current march already uses — no
  jitter of any kind, and a loop bound that is a constant — under this section's
  standing rule that every real check is a structural ratio between two blocks
  of one frame.
- **Cone tracing over a colour mip chain, for roughness.** `ssr.slang`'s header
  refuses it on two costs: building a colour pyramid, and a `SampleLevel` at a
  computed LOD. Half of that has changed under it — **the bloom downsample chain
  is already a colour pyramid of the scene**, built every frame a view asks for
  bloom, so the pass that would have had to build one may be able to borrow it.
  That is flagged as a thing to **verify**, not as a saving already banked: the
  chain's format, its extents, the mip it stops at and its lifetime inside the
  graph all have to agree with what a cone trace wants, and it is drawn only
  when the bloom bit is on — which no view in this workspace but the
  `Scene::Bloom` fixture sets. The computed-LOD read is untouched by any of that
  and remains the harder half of the refusal.
- **Temporal accumulation is still blocked, and blocked on the antialiasing
  section's blocker exactly**: motion vectors, and a prev-transform slot in
  `GpuInstance` that does not exist. **One blocker, two features.** Whoever pays
  for TAA's instance widening pays for temporal SSR in the same change, which is
  the strongest argument for taking that slot once rather than twice.
- **Planar reflections stay refused for this row**, and stay the right answer
  for the render-to-texture mirror this document already names. Nothing above
  weakens that paragraph: a planar pass is per-plane, is a second geometry pass
  per mirror, and is useless on anything curved.
- **Ray-traced reflections stay at P7C**, unchanged. This slice improves the
  raster twin; it does not move the row the twin exists beside.

## Post-processing stack

Pipeline order (all at internal render resolution, before the topic 15
render-scale upscale; UI composites after, at native resolution):

```
scene (HDR RGBA16F) → bloom (down/upsample chain) → exposure + tonemap → FXAA → [upscale] → UI
```

**`[upscale]` has no implementation on either side of the seam, verified
2026-08-27.** [15-windowing.md](15-windowing.md) defines borderless as an
internal render target upscale-blitted to the native surface, and
`ShellCaps::HW_UPSCALE` reports what a window system will do for free — but
`crcbl-render` has no upscale pass, no render-scale knob and no internal target
whose extent differs from the swapchain's. So every stage of this chain runs at
native resolution today, and the ordering is a contract for a pass that does not
exist rather than a description of a frame. Whoever builds it inherits the two
interactions below unchanged.

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) later.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure. **The pass is built and the curve is not**:
  `tonemap.slang` is exposure-and-clamp, chosen at P1 so display-referred
  content reached the swapchain unchanged and no golden would be re-blessed
  twice — once when the pass landed and again when this row does. That file says
  in as many words that topic 18's stack "replaces exactly one function" in it,
  so what is owed here is the operator, not the pass, the transient or the
  `TonemapParams` block. Fixed exposure is a runtime uniform now; auto-exposure
  (histogram, GPU reduce) is still later.
- **AA (MVP)**: **FXAA**, then SMAA 1x, with TAA post-MVP and MSAA priced rather
  than rejected — the whole ladder, what each rung costs in this tree and what
  is refused are the **Antialiasing** section below. **Unbuilt**: there is no
  `fxaa.slang`, no resolve pass in the graph and no `RenderEffects` bit for one,
  so every frame this engine draws is unresolved.

  **The prev-transform slot is not reserved**, whatever this row claimed until
  2026-08-27. `crcbl_shaders::mesh::GpuInstance` carries `transform`, `mesh`,
  `material`, `sector`, `flags` and `base_vertex` and nothing else, so the
  instance format is a widening TAA still has to pay for. The cheap insurance
  was never taken; the reason to take it — that `INSTANCE_STRIDE` is cheap to
  extend now and expensive once §3.3's shaders index past it — is the one
  `GpuInstance::sector` is already in the record on.

- **Bloom (P10)**: physically-plausible threshold-free downsample chain (Karis
  average), 5–6 mips, tent upsample, additive with scalar. Cheap, huge
  perceived-quality win — timed with the UI/debug polish phase so the profiler
  HUD can show its cost honestly.
- Stack is data-driven per camera (RON: which passes, parameters) —
  games/samples tune without engine edits; settings UI (topic 14 P10) exposes
  quality toggles.

### Where the toggles live

Every feature in this document is switchable at three layers, resolved in one
place, per [39-capabilities.md](39-capabilities.md):

```
camera stack (this RON) declares what the view wants
  → [engine.video] clamps it downward as a player quality setting
  → programmatic override may set it either way
  → device capability clamps it downward, last and absolutely
```

The per-camera layer is the one this topic owns, and it is genuinely per view: a
render-to-texture camera driving a security monitor, a planar reflection, or a
weapon-scope PiP (topic 29) does not want reflections or GI of its own, and that
is a property of the camera rather than of the player's hardware.

**Built 2026-08-14, and two of the four layers have no source in the tree.**
`crcbl_render::effects` is the resolution point: `RenderEffects` is the effect
set, `EffectRequest` carries the three requested layers, and
`EffectRequest::resolve` applies the whole order in one place.
`ForwardRenderer::begin_frame` resolves once per frame and freezes the answer,
so the half of a frame that parametrises the shadow culls and the half that
dispatches them cannot disagree.

- **Programmatic** is wired: `ForwardRenderer::set_effect_request`, and
  `apps/lantern`'s `--no-shadows` / `--no-ao` / `--no-reflections` drive it.
  There is no `--no-bloom`, and there is nothing for one to turn off: bloom is
  the one effect **not** in `RenderEffects::DEFAULT_STACK`, so a view that has
  declared no render stack — which is every view in this workspace but the
  `Scene::Bloom` fixture — is not drawing it to begin with. The reason is on
  that constant: the other three approximate light transport present in the
  scene, and a camera given no stack has been given no lens.
- **Device** is wired to `DeviceCaps` and **removes nothing**, which is a fact
  about these three effects rather than an unfinished clamp — the AO section
  above says it of the occlusion pair in as many words, the reflection pair's
  module says it of itself, and a device too small for the shadow atlas fails to
  build the renderer rather than degrading past it. The first real rule arrives
  with the ray-traced variants, which `LightingPath` selects.
- **Camera stack** is a field nothing writes: there is no render-stack RON, and
  nothing in the workspace reads or writes RON at all.
- **`[engine.video]`** is wired: `GpuContext` reads the player's settings file
  while it opens — `SettingsSource::Platform` by default, so every sample and
  the `crcbl new` scaffold get it without asking — and
  `GpuContext::effect_request` hands the layer to a renderer built on that
  context. `crcbl::settings`' `VIDEO_KEYS` is the one place a key is spelled,
  and a key that is absent clamps nothing, because this layer may only remove.

## Antialiasing

The stack's AA slot, and the ladder that runs through it. **Nothing in this
engine resolves an edge today** — no `fxaa.slang`, no resolve pass in the graph,
no `RenderEffects` bit, and `crcbl_hal::MultisampleState`'s default is one
sample — so that row is a contract for a pass that does not exist rather than a
description of a frame.

### FXAA 3.11 first

One fullscreen pass over the tonemapped image: a luma edge detect, a subpixel
blend along the edge it found, no history, no new attachment and no change to
any pass in front of it. It is the cheapest thing that removes the staircase,
and it is the tier that stays after the rung above it lands.

**Its template is `crates/crcbl-shaders/shaders/bloom_composite.slang` and not
`crates/crcbl-shaders/shaders/tonemap.slang`**, which is worth saying because
the obvious answer is the wrong one. The tonemap is a 1:1 `Load` at an integer
pixel and deliberately samples no neighbour — that is the whole of its
determinism argument. The bloom composite already carries both halves FXAA
needs: the same fullscreen triangle out of `SV_VertexID`, and a neighbourhood
gathered around a UV through an `inv_source` texel-size uniform its Rust mirror
writes once per frame. An `fxaa.slang` is that file with the tent replaced by
the edge detect.

What it costs here, item by item, because none of it is hypothetical:

- One `.slang` source and **four committed artifact sets** — SPIR-V, WGSL, MSL
  and DXIL — each hashed into the manifest `crates/crcbl-shaders/tools/` writes
  and `--check` gates.
- A params mirror under `crates/crcbl-shaders/src/`, on
  `crcbl_shaders::bloom::BloomParams`'s terms: one block, declared once,
  agreeing with the source member for member.
- **A fifth `RenderEffects` bit, which is not free.**
  `crates/crcbl-render/src/effects.rs`'s `NAMES` table is as long as the type
  has flags, so an unnamed fifth effect does not compile, and
  `every_effect_is_named_exactly_once_and_the_row_prints_them` pins the exact
  string `DEFAULT_STACK.row()` produces — the row every sample's summary line
  and debug panel print. Putting the bit in the default stack changes that
  string, so the assertion moves deliberately rather than by surprise.
- A pass in `crates/crcbl-render/src/forward.rs` shaped like the tonemap block
  it follows: a pipeline, a layout, a params buffer per frame in flight and a
  bind-group ring keyed on the views it reads. `RENDER_PASSES` grows a term and
  `fullscreen_passes` grows a branch, which is what keeps the frame's timer
  count matching the frame.
- **A re-bless of every golden the bit is on for.** FXAA moves every edge in
  every frame it runs on, so there is no additive-zero property to land it
  behind — the probe and bloom slices had one and this does not. The switch
  therefore decides how much of the suite moves, and the honest default is the
  one that moves it exactly once.

### SMAA 1x second, and it is the real industry standard step

**When FXAA's over-blur of text and thin geometry starts showing, the engine
reaches for SMAA 1x** — not for TAA, and not for a wider FXAA preset. Three
passes: an edge detection, a blend-weight calculation that looks the detected
pattern up in a precomputed **area** table and a **search** table, and a
neighbourhood blend that applies the weights. Each is the fullscreen shape the
tier below establishes, so the pass machinery is the same machinery a third
time.

Two things about it are specific to this tree:

- **The lookup tables are a data cost, not a computation.** They are on the
  order of 160 KB and have to arrive as **committed bytes** with a generator and
  a `--check` mode behind them, on `cook-clusters`' precedent and hashed the way
  `spirv/manifest.txt` hashes an artifact. Deriving them at start-up instead
  would put a table four rasterisers computed independently underneath every
  golden in the suite, which is the read this file's determinism arguments spend
  their whole length avoiding.
- **It is historyless, so it is deterministic by construction**, and that is
  what makes it golden-safe where TAA is not. Its inputs are one frame's pixels
  and two constant tables; no frame it draws is a function of how many frames
  preceded it.

FXAA does not leave when SMAA arrives. **It stays as the cheap tier**, on the
terms `RenderEffects` already gives the other pairs: a tier that is off is a
frame with fewer passes, not a shader branch.

### TAA is specified, still post-MVP, and the blocker is named exactly

TAA needs four things this tree does not have:

- **A per-frame subpixel jitter on the projection**, which changes the camera
  matrix every golden in the suite is drawn through.
- **Motion vectors**, which is a second colour target on the forward pass and a
  velocity per fragment. The SSR section's escalation clause is the shape — one
  target state, one transient, the fragment stage's return struct — but a
  velocity is not reconstructible from depth the way a normal is, so it is a
  real widening rather than a contained one.
- **A history target with neighbourhood clamping**, which makes a frame a
  function of how many frames were drawn before it. That is the property this
  document already refuses in writing for SSR history and again for DDGI.
- **A prev-transform slot in `GpuInstance`, which does not exist.** The AA row
  in the stack above carries that correction and the arithmetic behind it, and
  nothing here repeats it.

`crcbl_render::skinning`'s `SkinnedRegion::previous_base` is the half of the
reservation that **was** taken: topic 17's 2026-07-27 correction double-buffers
the skinned-output pool region from day one and a frame alternates which run it
writes. It has no reader outside that module's tests, because there is no pass
to read it. So the animation side of TAA is paid for and the instance side is
not, which is the honest state of the row.

### A seventh, taken 2026-08-27: MSAA is reopened, priced, and still not the default

The AA row rejected MSAA for fighting "deferred-ish/HDR pipelines", and **that
is deferred-renderer reasoning applied to a renderer that is not deferred**.
This engine is clustered forward, and the "Clustered forward" section above
rejected deferred partly _because_ deferred fights MSAA. A rejection cannot be
inherited from the argument it was the counterweight to.

The seam already carries it. `crates/crcbl-hal/src/pipeline.rs`'s
`MultisampleState` has `samples` and `alpha_to_coverage`, every pipeline in the
tree takes one, and its `Default` says in as many words that MSAA is available
and never the default. So the honest position is not "rejected" but **viable and
priced**, and the price is specific:

- **The depth prepass has to be multisampled too.** The forward pass attaches
  the depth the prepass wrote, and a single-sample depth image cannot be
  attached beside a multisampled colour target.
- **Both screen-space passes read that depth, and each wants one sample of it.**
  `ssao.slang` reconstructs a normal from four neighbouring depths and
  `ssr.slang` marches it tap by tap. So MSAA buys either a depth **resolve**
  before those passes — a pass and an image the frame does not have — or
  per-sample versions of both, which is the occlusion pair and the reflection
  pair rewritten.

That is why it is not the default, and it is a reason rather than a refusal.
**MSAA is the right answer for a forward renderer doing little screen-space
work**; FXAA and then SMAA are the right answer for this one for exactly as long
as SSAO and SSR are in the stack. A view that drops both — which the per-camera
effect layer above already allows — is a view where the arithmetic flips, and
the reader holding that view is the one who should make the call.

### What is refused

- **DLSS.** Single-vendor and closed: it runs on one hardware line behind an
  SDK, where every other path in this engine is held to being the same code on
  all four backends. A quality tier that exists on one adapter is a second
  renderer wearing a capability flag.
- **FSR 2 and FSR 3.** Temporal, so they inherit **every** TAA blocker above —
  the jitter, the motion vectors, the instance slot — and add a history of their
  own on top. Being vendor-neutral answers the objection to DLSS and touches
  none of the reasons TAA is post-MVP.
- **Any AA that resolves after the UI pass.** The UI composites at native
  resolution after the upscale seam, deliberately, so its text is rasterised
  sharp. Running an edge filter over it afterwards blurs glyphs that were never
  aliased, which is a regression with a quality setting's name on it.

## Interactions (kept honest)

- Render-scale upscale (topic 15) happens **after** tonemap+AA: post chain costs
  scale with internal res (the whole point of render scale). Both of these first
  two bullets are ordering rules for an upscale that does not exist — see the
  note under the pipeline order above.
- UI renders after upscale at native res (crisp text regardless of 3D scale) —
  this ordering is the reason the UI pass was kept separate in stage 7.
- Debug overlays (debug draw, gizmos) render pre-tonemap in HDR (they're in the
  world) except UI-space panels. §3.6's debug draw layer is itself unbuilt, so
  this rule binds whoever builds it.
- Golden-image tests (topic 12): shadows and each post pass get dedicated golden
  frames; tonemap changes are the classic "everything shifted" diff — the
  `--bless` flow exists for exactly this.

## Delivery

| Slice                                                                                                                                         | Phase                                                                                                                                                                                     |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| HDR target + exposure/tonemap pass                                                                                                            | P7 — built at P1. The **filmic curve** is still owed; see the post stack                                                                                                                  |
| Antialiasing rung 1: **FXAA 3.11**                                                                                                            | P7 — unbuilt, and it is the whole of the stack's AA row today                                                                                                                             |
| Sun CSM (culling-integrated, 3×3 PCF)                                                                                                         | P7 — built. The **cascade debug overlay** is not                                                                                                                                          |
| Shadow ladder rung 1: **normal-offset bias**                                                                                                  | P7 — the fifth decision named it and the sixth's constants are what it buys back                                                                                                          |
| Rasterised twin: spot + point shadows, SSAO, SSR, irradiance probes                                                                           | P7B — **complete**, each gated by a golden in `crates/crcbl/tests/render_e2e.rs`                                                                                                          |
| Acceleration structures: BLAS bake/load, TLAS refit, `crcbl as stats`                                                                         | P7C                                                                                                                                                                                       |
| Ray-traced shadows + AO                                                                                                                       | P7C                                                                                                                                                                                       |
| Ray-traced reflections                                                                                                                        | P7C                                                                                                                                                                                       |
| Ray-traced global illumination                                                                                                                | P7C                                                                                                                                                                                       |
| Bloom chain                                                                                                                                   | **Built 2026-08-23** (P10) — off unless a view asks; see `RenderEffects::DEFAULT_STACK`                                                                                                   |
| The render quality pass: **SMAA 1x**, **GTAO + bent normals**, **Hi-Z + cone-traced SSR**, shadow cross-fade → rotated Poisson PCF → **PCSS** | P10, with the bloom chain, for the reason that row gives: the profiler HUD is what shows a quality rung's cost honestly. Each rung's section above says what it costs and what it refuses |
| MSAA                                                                                                                                          | **No phase, and not a rejection** — viable and priced by the seventh decision, and not the default for exactly as long as SSAO and SSR read a single-sample depth                         |
| Auto-exposure; TAA (jitter, motion vectors, the `GpuInstance` slot); temporal SSR; shadow atlases                                             | post-MVP. The instance slot is **one blocker for two features** — see the antialiasing and reflection sections                                                                            |

**P7B and P7C are new phases** carrying the raster twin and the ray-traced path
respectively; the roadmap's phase table is authoritative for their ordering. The
raster twin lands **first**, deliberately: it is the path macOS, iOS and every
browser use, so it is the one whose absence would block more platforms than it
unblocked, and it is the reference the ray-traced path is reviewed against.

Sample impact: horde (S3) onward renders shadowed + tonemapped; orbit's planet
terminator and towers' map lighting are the showcase beneficiaries; **`lantern`
(sample 13) is the dedicated lighting sample** and exists to show the same scene
under both paths side by side. Exit criteria of the other samples inherit
"shadows on, stack on" implicitly via the phase gates.

## Risks

- **CSM artifact whack-a-mole** (peter-panning, acne, cascade seams): this risk
  arrived, and the fifth and sixth decisions above are the record of fighting it
  — bias denominated in tile texels, slope read off the geometric normal rather
  than the shading one. Stable snapping and the golden frames did the work; the
  debug overlay that was supposed to help is still unbuilt. **It has not
  finished arriving**: the shadow ladder's own decision records a `cube` golden
  that has failed on linux and windows since the 2026-08-26 re-tiling and is
  unresolved, which is why normal-offset bias is a P7 row above rather than part
  of the P10 quality pass.
- **Post-stack perf in a browser**: each pass is simple, but measure — the horde
  web demo budget (S3) includes the stack. The quality pass adds passes to it:
  SMAA 1x is three where FXAA is one, and a Hi-Z march builds a pyramid before
  it walks one.
- **An AA rung re-blesses the suite, and there is no additive-zero form of it.**
  The probe and bloom slices could land switched off and move nothing; FXAA
  moves every edge in every frame the bit is on for. So the risk is not the
  filter, it is deciding once which goldens carry it and re-blessing them once
  rather than a scene at a time.
- **A Hi-Z pyramid puts a reduction underneath the reflection goldens.** Every
  level is float arithmetic four rasterisers perform independently, where the
  fixed-stride march reads the prepass directly. The SSR section's standing rule
  — structural ratios rather than tolerances, and never a per-driver re-bless —
  is what has to absorb it, and it was written before this rung was scheduled.
- **TAA later ≠ never**: reserving the prev-transform slot now was the cheap
  insurance and it was not taken — see the AA row above. What has changed is the
  price of not taking it: **temporal SSR is blocked on the same slot**, so the
  widening is owed to two features rather than one, and `SkinnedRegion`'s
  double-buffered half of the reservation goes on costing memory nothing reads.

## Irradiance probes: the design (2026-08-14)

The capability table's `Rasterised` twin of ray-traced global illumination, and
what was P7B's last unbuilt row. **It is built now, as designed here** —
`crcbl_render::probe` owns the `ProbeTable` and the `ProbeGrid` that rides in
the frame uniforms, `crcbl_shaders::probe` fixes the row layout both sides
write, `mesh.slang`'s `probe_irradiance` does the interpolation and the three
dot products, `SceneDesc::probes` is where an application hands the volume over,
and `apps/lantern`'s `bounce` module bakes the sun's first bounce into one. The
paragraphs below describe what shipped rather than what was intended.

### A static grid of L1 spherical-harmonic probes, and no new pass

An irradiance volume: a uniform grid of probes in a read-only storage buffer,
trilinearly interpolated in the shader. The diffuse half is **added** to
`frame.ambient` in `mesh.slang`; the specular half is what a miss returns inside
`ssr.slang`.

**It adds no render pass** — `RENDER_PASSES`, `MAX_PASSES` and
`fullscreen_passes` are all unchanged. That is the single strongest reason to
prefer it here over anything that needs a pass of its own.

L1 is four coefficients per channel, packed so that evaluating irradiance for a
normal is **three dot products** against `float4(N, 1)`. No `pow`, no
trigonometry — the rule `ggx_lobe` is already held to, and the rule this file's
determinism argument rests on.

### Rejected, for reasons about this tree rather than about the technique

- **DDGI** (Majercik et al. 2019) needs ray tracing, which nothing here has, and
  temporal accumulation, which this document already refuses in writing for SSR
  history: a golden must not be a function of how many frames preceded it.
  Either one is fatal alone.
- **Light-field probes** (McGuire et al. 2017) are the correct answer to light
  leaking and cost a per-probe octahedral depth map. There is no leaking defect
  yet to justify them.
- **SH L2** (Ramamoorthi & Hanrahan 2001) captures ~99% of diffuse irradiance
  against L1's ~87%, for 27 floats against 12 — a difference
  `Tolerance::RASTERISER` cannot resolve. Escalating later is contained to one
  constant and one function.
- **The Valve ambient cube** (McTaggart 2004) is close, and never rings
  negative. L1 is 12 floats to its 18 and needs no per-axis select; L1's ringing
  costs one `max(…, 0)` at this grid density. Recorded as the drop-in if ringing
  is ever seen.
- **A 3D texture with hardware trilinear.** `ImageType::D3` exists in the seam
  but nothing outside `null`'s tests has ever created one, `texture.rs` knows
  only the `D2Array` upload path, and hardware filter weights are vendor tables
  — the exact class of filtered read the AO and SSR designs spent their
  determinism arguments avoiding. An 8-tap manual lerp over a cache-resident
  table costs less and risks nothing.
- **A compute pass writing a probe volume.** One of the two reasons given here
  has gone: it was that `crcbl-wgpu` refused `BindingKind::StorageImage`
  outright, and that crate was deleted 2026-08-21 — `crcbl-webgpu` answers
  `Capability::StorageImageBinding` with `Support::Yes`. So the browser tier
  could run it; a storage buffer would be permitted, but the
  temporal-accumulation refusal above applies regardless.
- **Prefiltered radiance cubemaps** — the only thing that puts a recognisable
  room in a mirror — need cube arrays, mip chains and `SampleLevel` at a
  computed LOD, which `ssr.slang` refuses in writing. Deferred with a named
  trigger.

### The irradiance is authored, not baked and not computed at runtime

`SceneDesc.probes`, filled by the application — for `apps/lantern`, computed
analytically from the room's own dimension constants so that moving a wall moves
the probes.

**Not a bake tool yet, for a concrete reason:** a gather bake means casting rays
at scene triangles, and this tree has **no ray-triangle intersector and no
BVH**. `crcbl-phys`'s `query` module has ray-vs-sphere, ray-vs-AABB and
ray-vs-capsule and nothing else. Writing both, plus an artifact format and a
manifest entry, is its own topic-sized piece of work and must not be smuggled
into this row. The precedent for when it comes is real — `cook-clusters` is a
committed-artifact generator with a `--check` mode, and `spirv/manifest.txt` is
how such an artifact is hashed.

### Additive, which is what makes it safe to land empty

```
float3 irradiance = frame.ambient.rgb + probe_irradiance(world_position, normal);
```

A scene with no probes uploads a volume of zeroes, and `x + 0 == x` exactly on
every target — so the frame is **bit-identical and there is no branch
anywhere**. It is the same argument this file already makes for the sun becoming
a row: the sum starts at zero and `0 + x` is `x` exactly. An author who wants
probes to be the whole ambient sets `DirectionalLight::ambient` to zero.

AO still scales the diffuse environment alone, so the **no specular occlusion**
refusal above is untouched.

### The specular half goes where the SSR design already left room for it

`ssr.slang` fills the space a screen-space miss leaves with probe radiance:

```
hit = hit_color * fresnel * confidence;
fallback = probe_radiance(world_position, reflection_dir) * fresnel * (1 - confidence);
reflection = hit + fallback;
```

`confidence` is the existing hit weight. Written as two terms rather than the
algebraically equivalent `lerp`: with a zero probe volume, `fallback` is exactly
zero and the pre-probe `hit_color * fresnel * confidence` multiplication order
is unchanged, so existing SSR hits remain bit-identical instead of moving by a
half-float rounding level.

The table stores _irradiance_ coefficients with the diffuse clamped-cosine
transfer already folded in. Specular needs radiance, so `probe_radiance` divides
the constant band by `π` and the linear band by `2π/3` before evaluating the L1
basis. Directly dotting the stored rows would brighten a constant environment by
`π` and distort its directional term.

Three things follow: no double-counting by construction, the zero-probe case
stays bit-identical, and **a fully metallic surface stops being black**, because
a conductor's only non-direct light is a reflection and now it has one
everywhere the surface faces rather than only where the march lands. This also
holds at and above `ROUGHNESS_CUTOFF`: zero sharpness returns the probe term
before screen-march setup, and the blur composites that centre value directly.
Positive sharpness blends continuously from it into filtered SSR.

**The honest limit:** an L1 probe is a very blurry environment. On a rough metal
it is close to right; on a mirror it is a smooth gradient where a room should
be. That is "not black", which is what was wanted — it is not "a mirror".

### The data path adds no device requirement

A read-only storage-buffer fetch in a fragment stage, which is what the material
table, the light list and the froxel grid already are. The seam permits it
explicitly: only _writable_ storage bindings of host-visible buffers are
refused. The mesh binding is appended **after** `AMBIENT_OCCLUSION_BINDING`,
never inserted, because `crcbl-mtl` numbers Metal arguments by counting layout
entries while Slang numbers by declaration order and the two agree only while
both ascend. As with occlusion, the new index is past everything
`mesh_cluster.slang` declares, so that file needs no mirror.

**No new `Features` flag and no new selector** — topic 39 requires a real second
path behind a selector, not a capability that could have been a uniform.

### Determinism: the SSR argument does not transfer, and does not need to

The evaluation is three dot products, seven lerps and one `max`. There is **no
comparison between two fetched values anywhere in it** — SSR's exposure is that
the first tap whose comparison flips _is_ the answer, and a probe lookup has no
tap and no flip. The one discontinuous quantity is the grid cell index, and it
is provably not a hazard: the trilinear weight on the far corner reaches exactly
zero at the boundary where the index changes, so a driver landing in cell `i` at
`f≈1` and one landing in cell `i+1` at `f≈0` compute the same value.

So a probe golden goes under `Tolerance::RASTERISER` like every other 3D golden.

### Testing

- **CPU, no GPU**: encoding round-trip and a stride assertion; and a Rust mirror
  of the SH evaluation **checked against known values from the literature** — a
  constant environment of radiance `L` integrates to irradiance `π·L`, and the
  L1 band's transfer coefficient is `2π/3`. A transcription slip there would
  pass every other test in this tree.
- **The bit-identity gate**: slice 1 re-blesses nothing. If any golden moves,
  the additive-zero property is broken. Shown red by perturbing one coefficient.
- **`Scene::Probes`**: the open box with ambient at zero and the sun right down,
  so every pixel is the probe term and nothing else — the anti-vacuity condition
  `Scene::Ao` already relies on. Two probes with opposite-coloured L1, and the
  observable is a **ratio between two blocks of one frame**, which is the form
  this document mandates for anything a tolerance cannot bound. A flat ambient
  gives ratio 1.0 and a zero volume gives a black frame, so it fails in both
  directions rather than asserting what unfinished code already returns.
