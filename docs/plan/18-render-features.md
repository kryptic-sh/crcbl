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
  constant rotation table indexed by `pixel.xy & 3`, and a 4×4 box blur.** Not
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
  tile, so it both removes the banding and divides an isolated flipped sample by
  sixteen.
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
  the fix and is deliberately deferred to the slice after the first frame
  exists.

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
