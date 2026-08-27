# Topic 45 — Shadows: cascades, atlas tiles, bias and the filter ladder

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

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

### A seventh, taken 2026-08-28: the slope moves the receiver sideways, not towards the light

The ladder's first rung, built. `mesh.slang`'s `shadow_slope` is gone and
`shadow_normal_offset` stands in its place: where the slope term used to scale a
move **towards the light** by `tan(acos(Ng·L))`, the receiver is now moved
**along its own geometric normal** by `sin(acos(Ng·L))`, and only the constant
term still travels light-ward. Both `sun_visibility` and `punctual_visibility`
carry the change, on the sixth decision's terms exactly — acne is a property of
the triangle rasterised into a map, and a light type biased differently for the
same surface would need constants of its own for no reason anyone could state.

**Sideways is the whole of it.** A move towards the light raises the depth the
fragment compares with, so enough of it to clear acne on a grazing surface is
also enough to lift a shadow off its caster; the two are one number pulled in
opposite directions, and the fifth decision's table is a schedule of that trade.
A move along the normal leaves the compared depth alone and changes _which
texel_ is read, so what it buys is a sample belonging to the receiver rather
than to the facet climbing across it.

**And the sine is bounded where the tangent was not.** `tan` runs to infinity as
a surface turns edge-on and needed `SHADOW_SLOPE_BIAS_CLAMP` — a number chosen
to stop an unbounded one. `sin` is at most one by construction, so the clamp
went with it and the worst case is now a quantity rather than a choice.

Measured on radv through `apps/lantern`'s 1280×960 review frame, walking the
floor out from the `-x` wall along `room::SHADED_FLOOR`'s own line, and through
`crcbl_render::scene::demo`'s dunes patch:

| Artefact                                  | Depth-biased slope | Normal offset |
| ----------------------------------------- | ------------------ | ------------- |
| Peak luma in the strip at the wall's foot | 140.3              | 51.0          |
| Lit strip's half-fall width               | 0.391 m            | none          |
| Cornice lift over the shadowed back wall  | 78.3 luma          | 11.7 luma     |
| Dark pixels in the dunes' valley floor    | 60                 | 24            |

**51.0 is the shadowed floor's own value**, which is what makes the second
column say the leak is _gone_ rather than narrowed: the profile never rises
above the shadow it is walking through, so there is no half-fall left to
measure. The dunes count is pixels sitting more than ten luma below the median
of their own neighbourhood, which is what a self-shadowing dot is and a smooth
Lambert gradient is not; at the golden's own 256×192 it reads 5 before and 3
after.

**The two counts moved, and both were swept rather than kept.**
`crcbl_render::shadow::NORMAL_OFFSET_TEXELS` is two and `DEPTH_BIAS_TEXELS` fell
from three to one — the constant is what the offset earned back, which is what
the fifth decision's closing line predicted it would. Each doc carries its own
sweep. The ceiling on the offset is the thinnest wall in the tree rather than a
frame: an offset moves a receiver bodily, and two texels of the outer cascade is
125 mm against `apps/lantern`'s 150 mm shell, where three would be 187.5 mm and
through it. No leak was seen at two; the bound is why three is not shipped.

**What it cost is recorded rather than hidden.** The brass block's foot in
`apps/lantern` picks up a scalloped fringe a couple of pixels deep, on the
period of the shadow texel, where the offset walks a receiver near a silhouette
across the edge of its own caster. That is the standard cost of this direction,
it is bounded by the offset itself, and it is a tenth the size of the strip it
replaced. The rungs above — a rotated Poisson kernel, then PCSS — are what
soften it.

Goldens re-blessed: `cube`, `cube_97x61`, `dunes`, `spot_shadow` and
`point_shadow` in `crates/crcbl/tests/golden/`, and `room.png` and `live.png` in
`apps/lantern/tests/golden/`. Every other golden in the tree still matches and
was left alone, which is the evidence that nothing outside a shadow moved.
`crcbl_shaders::mesh`'s `both_shadow_lookups_offset_along_the_facet_normal` is
what holds the direction after the re-bless, because a re-blessed golden cannot.

### The quality ladder, taken 2026-08-27

What ships, first, because the ladder is only readable against it: **stable
sphere-fitted cascades snapped to texels** — `crates/crcbl-render/src/shadow.rs`
fits a sphere around the eye rather than a box around the frustum, so rotating
the camera cannot change a cascade's extent, and quantises the light-space
origin to whole texels — **3×3 hardware PCF through a comparison sampler**,
which is `mesh.slang`'s `tile_pcf` taking nine `SampleCmpLevelZero` taps one
atlas texel apart and dividing by nine, the texel-denominated bias of the fifth
decision, the geometric normal of the sixth, the **normal-offset** direction of
the seventh, and the **2026-08-26 re-tiling** that bought a second point light
by shrinking `SHADOW_TILE` rather than growing the image.

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

**Those numbers are from before the seventh decision**, which re-blessed `cube`
and moved exactly the shadow edges the diff is made of. Whether the failure
survived it is a question only a Pages run answers — this machine's adapter is
not SwiftShader — and `docs/backlog.md` is where that is tracked.

The ladder, in the order it should be climbed:

- ~~**Normal-offset bias.**~~ **Built 2026-08-28** — the seventh decision above
  has the frames and the two sweeps. It earned back exactly what the fifth
  decision's closing line said it would: the constant term fell from three
  texels to one, and lantern's wall-foot strip went with it.
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
