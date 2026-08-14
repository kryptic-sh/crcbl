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

The table above names shadows for spot and point lights, and **the engine has
exactly one light** — a single `DirectionalLight` (direction, colour) in the
frame block. There was no light list, no light culling and no count budget
specified anywhere, so the shadow rows above were not implementable as written.
This section is that missing half.

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
  samples that motivate lighting (lumen, towers) are exactly that shape.
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

Measured on radv (and confirmed on llvmpipe) through `apps/lumen`'s 1280×960
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

| Constant, in texels | lumen's strip | dunes' valley floor |
| ------------------- | ------------- | ------------------- |
| 0.5                 | 0.140 m       | heavy cross-hatch   |
| 1                   | 0.160 m       | cross-hatch         |
| 2                   | 0.203 m       | cross-hatch         |
| 3                   | 0.244 m       | faint cross-hatch   |
| 4                   | 0.289 m       | a trace             |
| 5                   | 0.330 m       | clean               |
| 6 (shipped then)    | 0.375 m       | clean               |

Five is where the trace stops being visible in the dunes frame; six is that with
margin, and the margin costs four and a half centimetres of lumen's strip.

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

| Constant, in texels | lumen's strip | dunes, shading normal | dunes, facet normal |
| ------------------- | ------------- | --------------------- | ------------------- |
| 0                   | 0.128 m       | heavy cross-hatch     | seam on most edges  |
| 0.5                 | 0.149 m       | —                     | seam on many edges  |
| 1                   | 0.170 m       | —                     | seam on some edges  |
| 2                   | 0.213 m       | faint cross-hatch     | a few isolated dots |
| 3 (shipped)         | 0.256 m       | —                     | clean               |
| 6 (shipped before)  | 0.382 m       | clean                 | clean               |

Graded from the 1280×960 frame and from the golden's own 256×192, which is where
the aliasing is worst. The lumen column does not depend on which normal is read:
that room is built of flat slabs, so its frames at 6.0 are **byte-identical**
either way, which is also the cleanest evidence that the change does nothing
except where the two normals disagree.

**Three is shipped with no margin above it**, deliberately unlike the six it
replaces. Six was one over the first clean value because what it covered was an
unexplained shortfall; three covers a bounded, understood quantity, so margin in
it is lumen's strip bought back for nothing.

Re-measured through `apps/lumen`'s 1280×960 review frames, on the same fixtures
as the fifth decision's table:

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

Goldens moved: `apps/lumen/tests/golden/room.png` (145 of 49152 pixels, all in
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
  projections, PCF filtering (3×3 MVP). One directional light with shadows is
  the MVP contract — it's what makes 3D scenes read as 3D.
- **GPU-driven all the way**: shadow pass reuses the stage 3 compute culling
  (one cull dispatch per cascade against the same instance/geometry pools,
  indirect draws into depth-only pipelines). No CPU re-traversal per cascade —
  the shadow cost scales like the main pass, by design.
- Render graph: cascades = depth targets owned by the graph; barriers/layout
  automatic like every pass. Debug: cascade-split visualization overlay +
  shadow-map inspector panel (topic 7 debug tools).
- Skinned casters (topic 17) come free via the skinned-output pool region.
- **Spot-light shadows** (single map) when towers wants them (tower projectiles
  at night — optional polish); **point-light** (cube maps) are MVP now, because
  the raster twin has to cover every light type ray-traced shadows cover;
  static-geometry caching (cached cascades / shadow atlases) post-MVP when a
  sample's perf numbers demand it.
- **Under `LightingPath::RayTraced` this whole section is bypassed**, not
  augmented: shadows come from ray queries against the TLAS, for every light
  type, with no cascades and no shadow atlas. The two are alternatives, which is
  what keeps the raster path from acquiring ray-traced special cases.
- Path note: identical on every `GeometryPath` — depth pass plus whatever emit
  tail the device selected; nothing in the shadow path depends on the binding
  model.

## Screen-space AO: what the one-line row was missing (decided 2026-08-13)

The table above says "screen-space AO" and nothing else, and — exactly as the
shadow rows were not implementable before the light list existed — that row sits
on three things this engine does not have:

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
  inside the image already. `crcbl-vk`'s `depth_probe` is what asks whether the
  clamp is there: it binds the same one-texel image to a frame of `MESH_EXTENT`,
  darkens the light list until ambient is the whole of the colour, and asserts
  the ambient term arrived.

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

## Screen-space reflections (decided 2026-08-14)

The third P7B row. SSR runs after the forward pass, so the only per-pixel data
downstream are the depth buffer and the `Rgba16Float` scene colour — and a
reflection needs to know which surfaces reflect and how sharply, which is `F0`
and `roughness`.

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
  `a = roughness`.** Both values are bounded in `0..=1`, which is the argument
  the AO transient already makes for its single channel; a dielectric's `0.04`
  quantises to `10/255` and a roughness drives a lobe width, and neither error
  is resolvable in a reflection that is itself an approximation.
  `max_color_attachments` is 4 on the minimum capability profile.
- **It carries the material, not the shading.** Baking the evaluated
  environment-specular weight would be one channel cheaper and would freeze a
  shading decision into a buffer. This topic's first rule is one material model,
  one BRDF, **one set of inputs** — `F0` and `roughness` are the inputs, and
  storing them is what lets the irradiance-probe row evaluate the same lobe
  rather than inherit SSR's version of it.
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
  window got bigger. `lumen`'s panel is where it showed — the reflection its
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
- **A ray that hits nothing returns zero, and zero is correct.** The reflection
  is additive, so a miss adds nothing and the surface looks as it does today.
  That is why the table's Reflections cell says "screen-space reflections, probe
  fallback" — the fallback is the next row.
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
- **The roughness gate makes most of every existing frame identically zero.**
  With the cutoff at 0.5, `GpuMaterial::UNTINTED`'s 0.5 gives exactly zero on
  every target, a pixel weighted exactly zero is bit-identical across four
  rasterisers with no argument required, and the blur pass returns such a
  pixel's scene colour untouched rather than adding a filtered zero to it.

  **Noted when the blur was built (2026-08-14):** the blur widens the lobe a
  single ray can honestly stand for, so it is the natural moment to raise that
  cutoff — and raising it past a rough conductor at 0.55 takes `UNTINTED` in as
  well, because no monotone ramp passes 0.55 and stops at 0.5. That trades this
  claim away, so it was **kept out of the blur slice** and is its own decision;
  `docs/backlog.md` carries what it costs, measured.

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

The first slice does **sharp mirror reflections only, and the rough end is zero
rather than wrong**. A single ray cannot represent a wide lobe, and the failure
mode of pretending otherwise is a sharp reflection on a rough surface, which
reads as a bug on sight. The roughness fade is not a gate bolted on for the
goldens — it is the statement that this pass is valid only where the lobe is
narrow.

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
- **The second weight is on the march's own roughness ramp, not on the
  reflectivity attachment.** The march already computes `1 - roughness/cutoff`
  and writes it into the alpha of the image the blur reads, so the blur weighs
  taps by how near that is to the centre's own value without a second read of
  the attachment and a second copy of the cutoff. Over the centre's own value
  rather than a tuned tolerance: a tap on a surface too rough to reflect at all
  then weighs exactly nothing, which is the case a matt floor under a metal
  block is.
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
of `lumen`'s room the blur changed, llvmpipe and radv disagree by at most 8,
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

## Post-processing stack

Pipeline order (all at internal render resolution, before the topic 15
render-scale upscale; UI composites after, at native resolution):

```
scene (HDR RGBA16F) → bloom (down/upsample chain) → exposure + tonemap → FXAA → [upscale] → UI
```

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) later.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure.
- **AA (MVP)**: **FXAA** — cheap, single pass, no history. **TAA post-MVP**
  (needs motion vectors in the G-pass + history management + the ghosting fight;
  motion vectors slot into the instance path when TAA lands — the instance
  format reserves the prev-transform slot **now** so TAA is additive later).
  MSAA rejected (fights deferred-ish/HDR pipelines and the browser; FXAA→TAA is
  the path).
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
  `apps/lumen`'s `--no-shadows` / `--no-ao` / `--no-reflections` drive it.
- **Device** is wired to `DeviceCaps` and **removes nothing**, which is a fact
  about these three effects rather than an unfinished clamp — the AO section
  above says it of the occlusion pair in as many words, the reflection pair's
  module says it of itself, and a device too small for the shadow atlas fails to
  build the renderer rather than degrading past it. The first real rule arrives
  with the ray-traced variants, which `LightingPath` selects.
- **Camera stack** is a field nothing writes: there is no render-stack RON, and
  nothing in the workspace reads or writes RON at all.
- **`[engine.video]`** is a field nothing writes either, and it is closer than
  the row above: `crcbl_store::settings::SettingsStack` reads that namespace
  today. What is missing is a schema and a startup that builds a stack — nothing
  in `crates/` or `apps/` constructs one. See `docs/backlog.md`.

## Interactions (kept honest)

- Render-scale upscale (topic 15) happens **after** tonemap+AA: post chain costs
  scale with internal res (the whole point of render scale).
- UI renders after upscale at native res (crisp text regardless of 3D scale) —
  this ordering is the reason the UI pass was kept separate in stage 7.
- Debug overlays (debug draw, gizmos) render pre-tonemap in HDR (they're in the
  world) except UI-space panels.
- Golden-image tests (topic 12): shadows and each post pass get dedicated golden
  frames; tonemap changes are the classic "everything shifted" diff — the
  `--bless` flow exists for exactly this.

## Delivery

| Slice                                                                 | Phase    |
| --------------------------------------------------------------------- | -------- |
| HDR target + exposure/tonemap pass + FXAA                             | P7       |
| Sun CSM (culling-integrated, PCF), cascade debug overlay              | P7       |
| Rasterised twin: spot + point shadows, SSAO, SSR, irradiance probes   | P7B      |
| Acceleration structures: BLAS bake/load, TLAS refit, `crcbl as stats` | P7C      |
| Ray-traced shadows + AO                                               | P7C      |
| Ray-traced reflections                                                | P7C      |
| Ray-traced global illumination                                        | P7C      |
| Bloom chain                                                           | P10      |
| Auto-exposure, TAA (motion vectors), shadow atlases                   | post-MVP |

**P7B and P7C are new phases** carrying the raster twin and the ray-traced path
respectively; the roadmap's phase table is authoritative for their ordering. The
raster twin lands **first**, deliberately: it is the path macOS, iOS and every
browser use, so it is the one whose absence would block more platforms than it
unblocked, and it is the reference the ray-traced path is reviewed against.

Sample impact: horde (S3) onward renders shadowed + tonemapped; orbit's planet
terminator and towers' map lighting are the showcase beneficiaries; **`lumen`
(sample 13) is the dedicated lighting sample** and exists to show the same scene
under both paths side by side. Exit criteria of the other samples inherit
"shadows on, stack on" implicitly via the phase gates.

## Risks

- **CSM artifact whack-a-mole** (peter-panning, acne, cascade seams): budget it;
  stable snapping + slope-scaled bias + debug overlay from day one; artifacts
  are visible in golden frames.
- **Post-stack perf in a browser**: each pass is simple, but measure — the horde
  web demo budget (S3) includes the stack.
- **TAA later ≠ never**: prev-transform slot reserved now is the cheap
  insurance; everything else about TAA stays post-MVP.
