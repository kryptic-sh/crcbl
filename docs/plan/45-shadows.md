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
  shadow on and off. Since 2026-08-31 that metric is `shadow::coverage` and the
  hysteresis is this module's own: `HOLD_RATIO` on the ranking and
  `LEVEL_HOLD_RATIO` on the tile size. It is not [25-lod.md](25-lod.md)'s helper
  — that was tried and refused, and
  [43-render-standards.md](43-render-standards.md)'s row (f) says why — but it
  is the same rule with the same band, and the two constants name each other.
- **The atlas is a fixed tile grid** — and the rung that replaces the grid is
  pulled forward below, 2026-08-30. The sun's cascades take the first tiles, and
  the rest are handed out one per spot and six per point until they run out. A
  light that gets no tile **still lights and simply does not occlude**, which is
  the honest degradation and is what makes the budget a quality knob rather than
  a correctness cliff.

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
  tile rather than growing the image. **Cadence landed 2026-08-31** as item 3 of
  the atlas rung below, and it is keyed to the frame index exactly as this
  paragraph asked: the near cascade every frame, each one out at twice the
  period, and a moving light or a camera cut resetting it. A shadow pass is a
  whole geometry pass and there are as many as there are cascades and lit tiles,
  which is why it was the largest geometry saving short of occlusion culling.
  Static caching — the map's geometry rendered once per cascade and only dynamic
  instances re-drawn — is still the rung above this one, and still unbuilt: what
  item 5 caches is the whole map, not its static half.
- **The atlas proper, pulled forward from post-MVP on 2026-08-30** because the
  user wants shadows that hold up on a scene with many lights sooner than the
  ladder had it. What the atlas holds is `SHADOW_ATLAS_COLUMNS` by
  `SHADOW_ATLAS_ROWS` cells of `SHADOW_TILE` texels and `LIGHT_SLOTS` cull
  slots, and so two shadowed point lights and two spots beside the sun; a fifth
  light lights and does not occlude. That capacity is unchanged by the items
  below that have landed — they made the atlas divisible, not larger. The rung
  makes the atlas an **allocator rather than a grid**, in the shape Doom 2016
  and Unity HDRP ship. **All five landed 2026-08-31.** Item 2 is what first
  spends the allocator's levels, and 3 and 5 spend the frame.
  1. **Variable tile sizes — the allocator landed 2026-08-31.**
     `crcbl_render::shadow::AtlasAllocator` is a quadtree over each of the
     atlas's root cells: `allocate(level)` hands out a whole cell or a halving
     of it down to `MIN_TILE`, `release` merges four free children back into
     their parent so the space comes back at the size it was, and the order is
     deterministic — lowest free node of the level asked for, subdividing the
     lowest free node of the finest coarser level. A forest of per-cell
     quadtrees rather than one tree over the image, because the atlas is neither
     square by construction nor a power of two in cells; ask every root for its
     whole self and the layout is the old grid, texel for texel, which is what
     shipped. **`MIN_TILE` is a floor nothing has measured** — no light asks for
     a sub-cell map until item 2 exists — so the halvings are a starting point
     for that rung to sweep, not a finding.
  2. **A priority per light per frame — landed 2026-08-31.**
     `crcbl_render::shadow::coverage` is how much of the frame's **height** a
     light's shadow map covers on screen: the map's own footprint over the
     distance to the eye, through the projection's own scale with no viewport
     height in it. **A fraction of the frame rather than a count of pixels**, so
     a scene allocates the same tiles at every extent and a golden at 256x192 is
     evidence about the 1080p frame. **The map's footprint rather than the
     light's sphere**, so a narrow cone is not demoted for being narrow: two
     lights of one radius spread their texels over world regions that differ by
     the cone's tangent, and the narrow one already has the finer texels.

     One scorer decides both questions: the ranking that hands out runs of tiles
     and the size of the tiles in that run. `WHOLE_CELL_COVERAGE` is the anchor
     — a quarter of the frame's height earns a whole root cell — and every
     threshold below it is that halved, because a level of the quadtree is a
     halving of the tile's side. `LEVEL_HOLD_RATIO` is the band:
     [25-lod.md](25-lod.md)'s "switch-up and switch-down differ" at this
     module's other boundary, deliberately the same fifth
     `ForwardRenderer::lod_hold_ratio` opens, so a light on a threshold does not
     halve and double its map every frame. A light with no history starts at the
     coarsest level and climbs, which is the ladder with no band at all — so a
     frame's first answer is reproducible.

     **The anchor is the conservative end of a sweep bounded by the fixtures
     that must not move**, and that is written down rather than hidden. Measured
     on the tree's own fixtures, which the metric makes independent of the
     extent they render at: `Scene::PointShadow` 3.06, `Scene::SpotShadow` 1.41,
     `apps/lantern`'s lamp 1.06 at the worst phase of its orbit, its corner
     downlight 0.37 — and the downlight is what binds, clearing the anchor by
     half as much again. The parameter-free rule of one shadow texel per screen
     pixel would demote every one of them at the 256x192 the goldens are blessed
     at; `docs/backlog.md` carries what that costs and what it buys.

     **The bias had to move with it**, and that is the half a golden could not
     have caught: `mesh.slang`'s `SHADOW_TILE` was the whole cell's side and
     every bias in that file was denominated in it, so a demoted light would
     have been biased by a footprint four times too small per two levels.
     `tile_texels(rect)` — the reciprocal of `atlas_step` — is the map's own
     side, read out of the rectangle item 4 already hands over, and the constant
     is gone from the shader.

     Checked by `crates/crcbl/tests/mesh_e2e/shadow_tiles.rs`: two lights of one
     shape over one floor, one at a whole cell and one at a quarter of its side,
     with the demoted light's pool measured for self-shadowing dots — 22 in 2941
     pixels correctly biased against 195 in 2828 with the bias taken from the
     cell, on radv at 1280x960; 23 in 2940 against 200 in 2823 on lavapipe.

     **Priced 2026-08-31**: four spot lights over a six-by-six field of dunes
     patches, the same rig drawn from a camera at 22 and at 88 world units, so
     the far camera is that rig two ladder rungs down. `PassStats` over 48
     recorded frames after the warm-up, the `shadow` pass alone — on an RX 7900
     XTX 0.032 ms p50 whole against 0.030 demoted, and on llvmpipe 3.254 against
     2.695. A sixth off on the software tier and a few per cent on the hardware
     one, and the saving is the tile's _and_ the cut's together, since the far
     camera also selects a coarser cut. The sun's two cascades are whole cells
     in both columns and are most of what is left, which is why four demoted
     lights do not halve the pass — on radv a repeat put the two 2 µs apart on a
     pass of thirty, which is why the test reports the pair and asserts no
     ordering between them. The browser tier is unmeasured.

  3. **A budget in tiles and in rendered faces per frame — landed 2026-08-31.**
     `crcbl_render::shadow::Cadence::schedule` is the whole of it: which of the
     atlas's **groups** a frame redraws, out of the ones whose maps the image no
     longer holds. A group is a cascade or a light slot's whole run of tiles —
     what one cull covers, since a point light's `POINT_FACES` faces draw one
     visible set through six matrices, so redrawing three of them and holding
     three is the same cull and the same six visible sets at half the draws.

     **Keyed to the frame index and to nothing else.** No clock, no iteration
     order and no address is in the schedule, so a golden at a stated frame is
     the same golden on every run, every driver and every backend — the property
     the rung above asks for by name and the one this would be worthless
     without.

     **The tier is the ladder item 2 already built.** A light's tier is the
     quadtree level `tile_level` gave it, so `coverage` decides how often a map
     is redrawn exactly as it decides how large that map is — one scorer, read
     twice, and no second notion of how much a light matters. A cascade's tier
     is its own index, which is the cadence rung's own words: the near cascade
     every frame, each one out at twice the period. `r_shadow_cadence` caps the
     ladder.

     **Cadence is the outer gate and staleness the inner one**, and the code
     says so: a group whose inputs moved but whose turn has not come holds the
     texels it has and is redrawn on its turn. The alternative — staleness
     overriding the cadence — is a schedule that binds only while nothing moves,
     which is the case that never needed bounding.

     **`r_shadow_faces` bounds the tiles a frame redraws**, and the due groups
     are admitted in the group index **rotated by the frame** rather than in
     tier order. Tier order would starve every coarse map outright: a tier-0
     group is due on every frame, so the budget would be spent before a tier-1
     group was ever reached, for ever. Rotating serves them round-robin and the
     ladder still comes out of it — a group due twice as often asks twice as
     often and is served twice as often. A group that does not fit is skipped
     and the walk continues, which is `Selection::update`'s rule at this
     boundary; a frame that admitted nothing takes the first candidate anyway,
     so a budget smaller than the largest map is a bounded overshoot rather than
     a tile nothing ever draws.

     **The reset is derived, not thresholded.** A map whose own centre has moved
     further than its own reach — a light past its radius, a cascade's eye past
     the sphere `Cascades::far` fitted it to — covers ground it was never drawn
     for, which is worse than a shadow that lags. Such a group bypasses the
     period and is offered the budget first;
     `ForwardRenderer::shadow_cadence_reset` is the readback. A frame whose
     atlas was **relaid** — a light gained, lost or resized a tile, or
     `RenderEffects::SHADOWS` moved — resets wholesale and spends no budget,
     because the only clear this seam has covers the whole image and there is
     nothing left to hold.

     **What made holding a tile possible is the clear item 5 could not make.**
     `mesh.slang`'s `depthClearVertexMain` and
     `MeshModules::depth_clear_pipeline` reset one tile with a primitive over
     its own viewport — `CompareOp::Always` and depth writes on, at
     `SHADOW_ATLAS_CLEAR_DEPTH`, which a const assertion holds to
     `crcbl_hal::depth::CLEAR`. A frame that keeps a tile therefore **loads**
     the attachment and resets what it redraws; a frame that keeps nothing
     clears it, which is byte for byte the recording that shipped. That is also
     the second half of the DECIDED rule below: the atlas's unit is now the
     group, so a lamp that swings costs its own tiles and not every tile.
     Per-_face_ inside a point light's cube is still not the unit and does not
     need to be.

     **Both limits default to what shipped**, so every golden in the tree is the
     frame it was blessed from: `r_shadow_cadence` is one frame and
     `r_shadow_faces` is the whole atlas. `ForwardRenderer::set_shadow_cadence`
     pins the pair per renderer — the two variables are process-global and the
     shadow pass is in every frame's pass list, so a test that moved them while
     drawing would change the frame every other test draws.

     Checked without a device by `crcbl_render::shadow::cadence`'s own tests —
     the ladder halving, the hold capping it, the budget binding, nothing
     starving, a forced group bypassing the period — and by
     `crcbl_render::forward`'s
     `the_far_cascade_is_held_on_the_frames_between_its_turns`,
     `a_frame_that_keeps_a_tile_loads_the_attachment_and_resets_the_rest`,
     `the_face_budget_bounds_a_frame_and_starves_no_map`,
     `a_light_that_jumps_past_its_own_radius_resets_the_cadence` and
     `a_point_light_costs_the_budget_every_face_it_draws` against the null
     backend's recorded stream — the last of those because every other check
     here is built on the sun and a spot, which own one map each, so a face
     count that answered one for every group satisfied all of them and a point
     light's cube is what binds on the difference. On a device by
     `crates/crcbl/tests/mesh_e2e/shadow_cache.rs`'s
     `a_frame_that_kept_a_tile_draws_the_map_it_redrew`, which is the half no
     host test can reach: a frame drawn under a budget of one tile has to be the
     frame a renderer meeting the scene for the first time draws. On radv at
     256×192 the caster's move changes 1432 pixels, the frame after the run
     differs from the reference in 0, and the frame that held the lamp's map
     lags it by 1023 — against 755 differing when the tile clear is removed and
     268 when the load is replaced by a whole-attachment clear.

     **Priced 2026-08-31**: the same six-by-six field of dunes patches and four
     spot lights the rung above is priced on, one camera, with a patch nudged
     each frame so every map is out of date on every frame — the case a budget
     exists to bound. `PassStats` over 48 recorded frames after the warm-up, the
     `shadow` pass alone:

     | Tier                | Every map every frame (6 tiles) | Budget of 2 tiles          |
     | ------------------- | ------------------------------- | -------------------------- |
     | RX 7900 XTX (radv)  | p50 0.016 ms, p95 0.018 ms      | p50 0.007 ms, p95 0.008 ms |
     | llvmpipe (lavapipe) | p50 3.833 ms, p95 5.025 ms      | p50 1.727 ms, p95 2.414 ms |

     A little over half the pass on both, which is what two tiles out of six
     buys once the clear and the pass's own fixed cost are paid. The absolute
     figures move with what else is on the machine — a repeat of the hardware
     column on a quieter run read 0.030 ms against 0.013 ms — which is why the
     two arms are drawn on alternating frames of one run and why no ordering is
     asserted.

     The browser tier is unmeasured. `docs/backlog.md` carries what a default
     other than "every map every frame" would cost and what it would need.

  4. **The shader reads a rect per light — landed 2026-08-31.**
     `FrameUniforms::shadow_atlas_rect` is one `float4` per atlas slot (a scale
     into the image in `xy`, an offset in `zw`), and `mesh.slang`'s `atlas_uv`
     is `rect.zw + tile_uv * rect.xy` where it used to derive a cell from a
     grid; `atlas_step` divides the atlas's texel size by the rectangle's, which
     is what a smaller tile stepping further per texel means and the whole of
     why one sampling path covers every size. `volumetric.slang` carries the
     same row and the same three functions, held to `mesh.slang`'s by
     `both_shaders_spell_the_same_atlas_walk`, and `SHADOW_ATLAS_COLUMNS` and
     `SHADOW_ATLAS_ROWS` are gone from both — the grid is a host fact only now.

     **The rectangle rides beside the per-slot matrix, not in the light's row**,
     and the reason is the point light: it owns `SHADOW_POINT_FACES` maps and
     one row cannot carry six rectangles. `GpuLight::shadow_tile` is therefore
     still the index — but into a _pool the allocator hands out from_ rather
     than into a fixed grid, which is what this item was for.

  5. **Static caching — landed 2026-08-31, at whole-atlas granularity.**
     `crcbl_render::shadow::Selection` holds its tiles across frames: a slot
     that wants the size it already has keeps the same texels, and only a slot
     whose map is gone or whose level changed hands its tile back to
     `AtlasAllocator::release`, which is what gives that function its first
     caller in the renderer. `ForwardRenderer::begin_frame` then compares
     everything the atlas is drawn from — every view's block and every cull's
     frustum, the selection eye, the instance count and `InstancePool::revision`
     — against the reading the image was last _drawn_ from, and a frame that
     matches records no cull and no pass. `ForwardRenderer::shadow_atlas_cached`
     is the readback.

     **The record is committed by the pass's own body, not by the call that
     records it**, and that is the subtle half: a frame whose graph is refused,
     or built and dropped, has left the image exactly as it was, and a renderer
     that had already moved its record on would then hold a map nothing drew.
     `a_refused_frame_leaves_the_shadow_atlas_where_it_was` is the case.

     **What did not land is the tile as the unit.** The DECIDED rule below says
     "a lamp that swings costs exactly its own tiles"; what shipped is that one
     changed input redraws every tile. The obstacle is the clear: this seam can
     only clear a depth attachment pass-wide, and the region-bounded forms are
     not portable — Metal's load action and WebGPU's `loadOp` have no render
     area, so a partial clear would keep a tile on Vulkan and erase it on Metal.
     Per-tile needs either a `clear_attachment`-with-rects call at the HAL seam
     or a depth-writing clear quad scissored to the tile. `docs/backlog.md`
     carries both, and the second half of the same entry: the camera is an input
     to every tile — cascades are fitted to it and every shadow cull selects
     detail at the camera's pixels — so the cache only hits on a frame the eye
     did not move in.

  **DECIDED 2026-08-30, the user's rule for this rung:** the atlas is **dynamic
  and cached** — every light re-renders its tiles whenever it or an instance it
  covers moves, and a light whose tiles would come out the same is not
  re-rendered at all, so a scene full of still fixtures costs the frame nothing
  and a lamp that swings costs exactly its own tiles. That makes items 3 and 5
  the rule rather than an option: the cadence tiers exist to bound the worst
  case when everything moves, and the cache is what makes the common case cheap.
  Both halves are built as of 2026-08-31: item 5 landed the cache at whole-atlas
  granularity, and item 3's per-tile clear — `mesh.slang`'s
  `depthClearVertexMain` — moved its unit to the group, so a lamp that swings
  now costs its own tiles and not every tile. Per-_face_ inside a point light's
  cube is deliberately not the unit; the cadence's unit is the cull, and
  `docs/backlog.md` says why. A cached tile is not a bake — it is re-rendered
  the frame its inputs change, and nothing about it survives a load. The budget
  row — how many shadowed local lights a frame renders and the atlas's size on
  each of the three quality tiers — is drafted in
  [39-capabilities.md](39-capabilities.md)'s tier table as starting values to
  sweep on each tier's hardware.

  Checked by the goldens that stand — the sun's cascades and the four lights
  lantern shadows must not move — plus a scene with more lights than tiles that
  reads every tile allocated, the priority order observed, and the cost row
  `40-profiling.md`'s baseline supplies.

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

### An eighth, taken 2026-08-28: the cascade switch is a band, not an edge

**Where two cascades meet, both are sampled and the answers are mixed by
distance.** `mesh.slang`'s cascade lookup is now `cascade_visibility`, which
answers for one named cascade, and `sun_visibility` calls it twice inside a band
at the outer edge of the cascade it selected: `CASCADE_FADE_FRACTION` of that
cascade's reach, a tenth, over which `lerp` walks the answer from the near
cascade's to the far one's.

**Everything a cascade decides changes across the switch**, which is why the
switch was visible. The near cascade's texel is a sixth of the outer one's here
— 10.6 mm against 62.5 mm — so both biases, denominated in texels since the
fifth decision, are six times larger on the far side; and the maps are different
maps, so a shadow edge lands in a different place. The fifth decision made this
_sharper_ knowingly: biasing a near cascade proportionally less than a far one
is the whole point of denominating in texels, and the seam is its bill.

**The seam was measured before it was fixed, and it is not the scene's own
contrast.** The cascades are spheres about the eye, so cascade 0 meets cascade 1
in a circle on `apps/lantern`'s floor at an eye-distance of 4.088 m. Walking
that circle in the fixed-camera frame at 1280×960 and reading the luma 6 cm
either side of it, 51 samples:

| Circle                  | Mean step | p90   |
| ----------------------- | --------- | ----- |
| 3.6 m (no boundary)     | 2.82      | 4.74  |
| **4.088 m (the split)** | **11.28** | 41.04 |
| 4.5 m (no boundary)     | 3.70      | 16.22 |

Eight of those 51 samples are the switch's own — a step the band moves, as
against a shadow edge that crosses the circle and steps either way. On those
eight the step **fell from 33.70 to 5.76 on average, and from 63.56 to 17.74 at
worst**; the single largest step on the circle, 78.0 at 279°, is a shadow edge
and is unmoved, which is what says the metric is reading the right thing. Both
control circles came back **byte-identical**: the band changes nothing away from
the boundary.

**The tenth is a knee, not a guess.** Two metrics over the same eight angles —
the residual step across the boundary, and the steepest centimetre of a radial
walk from 3.1 m to 4.6 m, which is what sees a gradient that is merely narrower
rather than gone:

| Fraction | Residual step, mean / max | Steepest cm, mean / max |
| -------- | ------------------------- | ----------------------- |
| 0        | 33.70 / 63.56             | 14.91 / 22.59           |
| 0.05     | 5.95 / 13.89              | 7.82 / 12.56            |
| **0.1**  | **5.76 / 17.74**          | **6.76 / 12.56**        |
| 0.2      | 7.52 / 19.56              | 6.82 / 11.07            |
| 0.3      | 8.17 / 20.19              | 6.74 / 11.07            |

A twentieth already removes the step but leaves the ramp steeper; a fifth and
above flatten the ramp no further and start giving the residual step back, since
a wider band hands more of the near cascade's frame to the coarser map. The
tenth is where both curves stop moving. The 0.3 row is the one to trust least:
its band opens at 2.86 m and the floor only comes into frame between 2.83 m and
3.05 m at these angles, so part of its ramp is off the bottom of the image.

**What it costs is a second `tile_pcf` — nine more comparison taps — for the
fragments inside the band, and nothing at all for the ones outside it.** A band
is a shell rather than a volume, so the share of the frame that pays is smaller
than the fraction suggests, and the outermost cascade has nothing to fade into
and never pays. That share was not measured on its own, though the pass it lives
in now is — see the eleventh decision, which prices the whole forward pass with
`crcbl_render::PassTimers`.

Goldens re-blessed: `room.png` and `live.png` in `apps/lantern/tests/golden/`,
at 674 of 49 152 pixels differing (SSIM 0.995797) and 8714 of 691 200 (SSIM
0.997066). **Every golden in `crates/crcbl/tests/golden/` still matches** — 32
of 32 render-e2e scenes, unchanged — which is the shape of a fix that only
touches a cascade boundary: those scenes are small enough that the boundary
falls outside them. `crcbl_shaders::mesh`'s
`the_cascade_fade_grows_towards_the_outer_cascade` is what holds the band's
direction and its position, both of which draw a plausible frame when they are
wrong.

### A ninth, taken 2026-08-28: a rotated disc, and the count that makes it quiet

**The filter is a 32-tap Vogel disc of radius two tile texels, turned by one of
sixteen rotations that an ordered-dither matrix picks off the fragment's pixel
coordinate.** It replaces the 3×3 box, whose nine taps sat on the grid they
sampled and could not reach further than one texel without the count growing as
the square of the radius.

**The rotation is integer-indexed, which the ladder asked for and is worth
restating.** The usual spelling hashes the pixel coordinate into a float angle
and rotates by its sine and cosine. Both halves are wrong here: a hash is float
arithmetic whose low bits differ between drivers by construction, and `sin` and
`cos` are specified to no accuracy anyone can quote. A shadow comparison is
binary, so a tap that lands the other side of an edge is a whole tap of
difference rather than a rounding one, and a per-driver re-bless is what that
buys. The angles are therefore a constant table and the index is
`SHADOW_DITHER`'s.

**The taps are a Vogel spiral, not a Poisson set, and that is a deliberate
departure from the rung above.** A Poisson set has no closed form — it comes out
of dart-throwing, so it arrives as two dozen constants whose provenance is a
program nobody kept. The spiral's radius is `sqrt((i + 0.5) / n)` and its angle
`i π (3 - sqrt 5)`, so `crcbl_shaders::mesh`'s
`the_shadow_disc_is_the_vogel_spiral_it_claims_to_be` re-derives every literal
rather than pinning a copy of itself. Its spectral behaviour is slightly worse
and its auditability is not comparable.

**A wider filter buys acne back, and the depth constant is what pays.** A tap
two texels out reads a depth two texels' worth of the receiver's slope away, and
an offset along the normal cannot help — it moves every tap together. Counted as
the seventh decision counted them, on the dunes patch at 1280×960:

| Reach, in texels | Depth bias | Dark pixels | Strip at the wall's foot |
| ---------------- | ---------- | ----------- | ------------------------ |
| 1 (the box)      | 1.0        | 24          | none                     |
| 1.5              | 1.0        | 25          | none                     |
| 2                | 1.0        | 47          | none                     |
| **2**            | **1.5**    | **25**      | **none**                 |
| 2                | 2.0        | 24          | 0.015 m                  |
| 3                | 1.0        | 155         | 0.003 m                  |

So `DEPTH_BIAS_TEXELS` rose from one to one and a half, which is where the count
returns to the box's own and the strip has not yet come back. Three texels of
reach is off the table at any bias.

**What sets the tap count is dither, and two scenes had to be asked.**
`apps/lantern`'s wall shaft is a penumbra magnified across a hundred pixels:
twelve taps put a plain 4-pixel stipple through it, sixteen still showed one,
and by twenty-four it was gone. The dunes terrain is harder — one large smooth
surface in partial shadow, where a dither reads as texture rather than as noise
— and there it was measured as the RMS of each pixel about its own 5×5
neighbourhood over a patch that is one gradient:

| Taps         | Grain over the dunes patch |
| ------------ | -------------------------- |
| 9 (the box)  | 0.827                      |
| 16           | 1.459                      |
| 24           | 1.099                      |
| 32 (shipped) | 0.918                      |

Thirty-two is where what the filter adds falls back to what the box already had.
**Narrowing the disc does not substitute for taps**: twenty-four over a
1.5-texel disc measured 1.129, slightly worse than the same count over two, so
the count is the only knob this has.

**The dither matrix beat the obvious lattice, measurably.** `(3x + 4y) mod 16`
keeps neighbours three and four rotations apart and needs no table at all; it
was tried first. Because that difference is _constant_, what it leaves on a
smooth surface is a set of diagonal stripes, and it measured a fifth grainier
than the ordered matrix at the same tap count — 1.187 against 1.099. The
matrix's differences vary, so its texture has no direction to it.

**`volumetric.slang` took the same filter**, dithered by the froxel column's
grid position where a fragment uses its pixel. A shaft of light filtered on a
different disc from the surface behind it would have two penumbrae where the
scene has one. The two copies are held together by `crcbl_shaders::volumetric`'s
`both_shaders_spell_the_same_atlas_walk`, which now compares the four tables as
text as well as the walk.

**Nothing in the tree times any of this.** Thirty-two taps against nine is the
cost, twice over — the fragment path and the froxel pass — and there is no
shadow-pass timing to price it, so the count was chosen on the picture alone.
`docs/backlog.md` carries that as the gap it is, and `SHADOW_TAPS` is the first
constant a graphics-quality setting should reach for.

Goldens re-blessed: `dunes` in `crates/crcbl/tests/golden/` — 7085 of 49 152
pixels differing, **none of them grossly**, at an SSIM of 0.999351, which is the
shape of every edge softening slightly rather than anything moving — and
`room.png` and `live.png` in `apps/lantern/tests/golden/`. Every other golden in
the tree still matches.

### A tenth, taken 2026-08-28: the filter's width comes from the blocker

The ninth decision gave the filter a shape and a fixed reach of two tile texels.
Two texels is 21 mm of penumbra on `apps/lantern`'s near cascade and 125 mm on
its outer one, and that is the whole of it — the same width under a caster
resting on the floor as under one thrown from the top of a wall. Real shadows do
not work that way, and the frame says so: the far boundary of the brass block's
shadow, which is thrown from the block's top edge onto the outer cascade,
arrives quantised to the 62.5 mm texels the map stored it in. It reads as a
**sawtooth**, because a two-texel filter cannot cover an edge whose true
penumbra wants twenty.

So `tile_pcf` takes its radius as a parameter and the sun measures one per
fragment — **PCSS**, the industry's standard soft shadow, and the first rung on
this ladder that costs a second sampling loop rather than a different kernel in
the one that exists.

**The estimate.** `sun_penumbra_texels` in `mesh.slang` reads sixteen depths
straight out of the map over eight tile texels, keeps the ones nearer the light
than the receiver — reversed-Z, so nearer is _larger_ — and averages them. The
difference between that average and the receiver's own depth is a height, once
the cascade's box has been un-projected: an orthographic cascade of radius `r`
is `2 r + SHADOW_CASTER_REACH` deep and spreads that range linearly over `0..1`,
so one unit of clip depth is the whole of it. A sun of angular radius `θ` turns
a height into a half-shadow's width by a similar triangle, and dividing by the
cascade's texel footprint puts it in the unit the filter wants.

**`Load`, not a sampler**, which is also why the rung needs no new descriptor.
The atlas's only sampler is the comparison one, and a comparison sampler cannot
return a depth; a filtering one would average four depths across a silhouette
and report a blocker at a height nothing in the scene occupies.

**The result is clamped at both ends, and both ends are load-bearing.** Below at
`SHADOW_FILTER_TEXELS`, because a contact is not a sub-texel-sharp edge but a
quantised one, and a radius under the width the ninth decision was tuned around
trades its dither back for the staircase it removed. Above at
`SHADOW_SEARCH_TEXELS`, because a filter wider than the search that sized it
spreads taps over texels nothing measured. The fallback when no tap found a
blocker is the _lower_ clamp rather than "lit": sixteen points over eight texels
is a sparser look than thirty-two over two, so a thin caster can fall between
search taps, and answering fully lit there would leak light through it. The
search sizes the filter; it never replaces it.

#### The physical sun is a no-op at this atlas, and that was measured

The real sun is a 0.531° disc, so `tan` of its angular radius is 0.004634. A
cascade texel is `2 r / SHADOW_TILE`: 10.6 mm on lantern's near cascade and 62.5
mm on its outer one. At the physical value a blocker needs **4.6 m** of
separation before its penumbra reaches even two texels on the near cascade, and
**27 m** on the outer one — and nothing in either test scene casts that far.
Rendered at the physical value against the same frame with the constant at zero,
`apps/lantern` differed in **36 bytes of 4,915,200, each by one**, and the dunes
terrain was byte-identical.

That is worth stating plainly rather than discovering later: **shipping the
physical number would buy sixteen taps and no picture.** So
`SHADOW_SUN_TAN_RADIUS` is a softness knob whose unit happens to be an angle,
and the shipped 0.02 is a sun about four times oversized — 1.15° against the
real disc's 0.27°. What keeps it honest is that everything downstream of it is
physical: the width still comes from a real blocker height through a similar
triangle, so it tracks the scene rather than being a fudge per material.

#### The sweep that picked it

Measured as the RMS departure of lantern's far shadow boundary from the straight
line it should be, each row smoothed by nine pixels so the filter's own dither
cancels — the naive per-pixel metrics all move the wrong way here, because the
wider filter's dither dominates them while the artefact the eye reads is the
low-frequency sawtooth. Beside it, the dunes terrain's acne and grain from the
ninth decision's harness:

| `SHADOW_SUN_TAN_RADIUS` | Edge wobble | Dunes acne | Dunes grain |
| ----------------------- | ----------- | ---------- | ----------- |
| 0 (a fixed filter)      | 1.58 px     | 24 dots    | 0.918       |
| 0.010                   | 1.13 px     | 24 dots    | 0.918       |
| 0.015                   | 0.50 px     | 24 dots    | 0.918       |
| **0.020 (shipped)**     | **0.58 px** | 24 dots    | 0.918       |
| 0.030                   | 1.07 px     | 25 dots    | 0.942       |
| 0.050                   | 1.08 px     | 23 dots    | 1.005       |
| 0.100                   | 1.08 px     | 43 dots    | 1.222       |

The minimum is a tie between 0.015 and 0.02 — 0.08 px apart, under what one
scene resolves — and the wider of the two is taken because the effect it buys
away from this one edge is larger at no measured cost on the other scene. Past
0.03 the number stops moving: the estimate has saturated at
`SHADOW_SEARCH_TEXELS`, and 1.08 px is what the fully clamped filter leaves.
Acne and grain keep climbing past that anyway, which is the ninth decision's
finding that a wider disc buys acne back.

**The contact end was checked separately**, because a rung called contact
hardening that softens contacts has failed at the thing it is named for: the
brass block's foot line is unmoved between the fixed filter and the shipped
value, which is the lower clamp doing its job.

**The sun only.** `punctual_visibility` passes `SHADOW_FILTER_TEXELS` and always
will from here: a spot's and a point face's maps are perspective projections, so
a difference of two of their depths is not a distance until it is un-projected,
and neither light has an angular radius to scale one by. `volumetric.slang`
passes the constant too, for a different reason — a blocker search sizes a
penumbra from how far a _surface_ stands below its caster, and a froxel is a
volume of air with no surface to stand on. That is the same reasoning that drops
both biases there.

`SHADOW_CASTER_REACH` moved into `crcbl_shaders::mesh` and
`crcbl_render::shadow`'s `CASTER_REACH` now takes it from there. The sampling
side inverts the box the host builds, and two declarations would let a matrix
and its inverse drift apart — which would scale every penumbra by the ratio
between them, and no frame says how wide a penumbra should be.

Guards: `the_blocker_search_sizes_the_filter_from_what_is_nearer_the_light` pins
the reversed-Z sense of "blocker", the depth-range factor and the clamp's order,
each of which draws a plausible frame when it is wrong;
`the_shadow_discs_are_the_vogel_spirals_they_claim_to_be` now re-derives the
search's sixteen-point table as well as the filter's thirty-two, separately,
because a Vogel spiral's radii depend on the count it was generated for. All
four were verified by sabotage with the artifacts regenerated, so what fired was
the assertion and not `build.rs`'s manifest check.

Goldens re-blessed: `room.png` and `live.png` in `apps/lantern/tests/golden/`.
Every scene in `crates/crcbl/tests/render_e2e.rs` still matches at 256×192 — the
penumbra change falls under the rasteriser tolerance at that size — and so does
every browser-path golden.

**Sixteen taps on top of thirty-two, on the fragment path only.** What that
costs was not separated out, but the pass it is in is timed: the eleventh
decision below has the forward pass's own milliseconds on two adapters, and the
instrument was in the tree the whole time this said otherwise.

### An eleventh, taken 2026-08-28: the disc is taken only at an edge

The ninth and tenth decisions left a sun-lit fragment reading **48 texels of the
atlas** where it read 9, and a froxel reading 32. Both counts were chosen on the
picture, and the Pages browser gate — the only instrument in the tree that
touches this — ran out of wall clock on two demos rather than reporting a
number. `docs/backlog.md` carried four ways to answer that, and this is the
first of them: **the filter's own early-out**, which is the standard
optimisation for a wide PCF kernel and was simply missing.

**The shape.** `tile_pcf` now takes `SHADOW_PROBE_TAPS` taps first —
`SHADOW_PROBE_INDEX` names which of the disc's thirty-two — and returns a flat
`0.0` or `1.0` the moment those agree. So a fragment away from any shadow edge
costs 5 taps rather than 32, a sun-lit one 21 rather than 48, and a froxel 5
rather than 32. Where the probe disagrees the disc is taken in full and the
probe's five are **read again** rather than carried into the sum: that costs
five taps at an edge, 37 against 32, and it buys the property that makes the
change reviewable — a fragment the probe could not decide is shaded bit for bit
as it was before the probe existed, so every texel that could move is a texel
the probe called unanimous.

**Which five, and why those.** The four rim taps are chosen for their _angles_.
The disc is a Vogel spiral, so consecutive taps turn by the golden angle and
every _second_ tap turns by twice it — 85 degrees short of a whole turn. Taps
23, 25, 27 and 29 therefore sit at 283, 198, 113 and 28 degrees, three gaps of
85 and one of 105, at radii from 0.857 to 0.960 of the disc's reach. Four taps
about a quarter turn apart is what makes the probe a _ring_ rather than a
direction: a probe bunched on one side calls a fragment lit with a caster
standing in the half it never looked at. Tap 0, at radius 0.125, is what a
caster small enough to fit inside that ring falls on.

The probe scales with `radius`, so a wide penumbra is probed wide and a contact
is probed tight — the tenth decision's per-fragment width is what sizes both.

**Unanimity is an exact test, not a tolerance.** A comparison sampler filters a
2×2 neighbourhood, so a tap can return a fraction; but wherever those four
texels _agree_ the weighted sum is exactly 0 or exactly 1 whatever weights the
driver chose, and a sum of five such taps is an exact integer. So the two arms
are `probe <= 0.0` and `probe >= float(SHADOW_PROBE_TAPS)` and neither carries a
margin.

**What it changed on the shipped scenes: nothing.** Every golden in the tree
still matches, on two adapters that rasterise differently — radv on an RX 7900
XTX and llvmpipe, the CI-class software rasteriser. `render_e2e`'s 32,
`mesh_e2e`'s 29, `forward_e2e`'s 15, `apps/lantern`'s 7, `apps/quarry`'s 23 and
every app golden beside them, none re-blessed. `lantern` also drew its 42
browser checks green through the WGSL path. So on these frames the probe and the
disc agree everywhere, and the entry this closes was right that it _could_
differ — a caster thinner than the gap between two rim taps that also misses the
middle one — and wrong that it would.

**Both arms were sabotage-verified on hardware**, with the artifacts regenerated
inside the loop so that what fired was a golden and not `build.rs`'s manifest
check. Returning `0.5` from the shadowed arm reddens four `render_e2e` goldens —
`cube`, `lights`, and the two shadow scenes' path comparisons — and returning
`0.5` from the lit arm reddens four others — `spot`, `bloom`, the resolve check
and `point_shadow`. Neither arm is dead code, and the suite can see both.

**`tile_tap` is new and is a move, not a change.** The rotate, the clamp and the
compare were four lines the probe and the disc would otherwise each carry; they
are one function now, called from both, and
`both_shaders_spell_the_same_atlas_walk` holds its text against
`volumetric.slang`'s copy the way it already held `tile_pcf`'s.

Guard: `the_shadow_probe_is_a_ring_about_a_centre` re-derives the index list
against the disc it indexes — in range, no repeats, exactly one tap inside a
quarter of the reach, the rest past four fifths of it, and no angular gap wider
than a third of a turn — rather than pinning the four indices shipped. Every one
of those failures is quiet: a bunched probe, a missing centre tap and an index
past the end of the table all draw a frame, and none of them draws one a golden
attributes to the probe.

#### What it saved, measured

**The instrument was already in the tree.** `crcbl_render::PassTimers` brackets
every pass in the graph with a GPU timestamp pair, `apps/lantern` builds one,
and `crcbl::engine`'s `finish` logs the whole per-pass report at `info`. The
backlog's first answer — "a timestamp around the forward pass" — did not need
building; it needed running. `lantern --headless --frames N --size WxH` under
`RUST_LOG=info` prints it, and the shadow filter's cost is in the **`forward`**
row, not the `shadow` one: `shadow` is the atlas draw, which this decision does
not touch and which did not move.

Five runs each on an RX 7900 XTX (radv, Mesa 26.2.1),
`--size 1920x1080 --frames 400`, and three each on that machine's llvmpipe (LLVM
22.1.8) at `--size 960x720 --frames 120`. The figure is the room view's
`forward` row; lantern draws a second view for the wall monitor, which is small
and moved with it. Medians:

| Adapter           | `forward`, disc only | `forward`, with the probe | Cut |
| ----------------- | -------------------- | ------------------------- | --- |
| radv, 1920×1080   | 0.303 ms             | 0.221 ms                  | 27% |
| llvmpipe, 960×720 | 11.281 ms            | 8.135 ms                  | 28% |

**Reproducible since 2026-08-28, and confirmed.** Those medians were taken by
hand from five runs because the exit log printed one arbitrary latent frame;
`crcbl_render::PassStats` now prints a p50 and a p95 per label over the last 120
frames of the same run, and it sums a label across the frame rather than
reporting each view separately. On the same machine and extent one run reports
`forward` at **0.230 ms p50 / 0.243 ms p95** — both views together, against the
0.221 ms this table's room view alone — and 18 labels for 0.990 ms of p50, where
the old line listed 53 rows. See [40-profiling.md](40-profiling.md).

**The two agree to a percentage point, and that answers the open question.** The
backlog asked whether the 48 taps cost what they cost because they are taps or
because of the branch divergence they add on a CPU rasteriser, where a lane runs
every tap the widest fragment in its group takes. A SIMD GPU and a scalar
software rasteriser cutting by the same share says taps, not divergence.

For scale, the same frame's whole report on radv at 1920×1080 — frame 397, 53
passes over both views, 0.986 ms of GPU time — puts `ssao` at 0.255 ms and 25.9%
of it, `forward` at 0.199 ms, `ssr` at 0.099 ms, `shadow` at 0.070 ms and the
five Hi-Z levels at 0.018 ms between them. **GTAO is the most expensive pass in
this frame**, which is a finding for
[46-ambient-occlusion.md](46-ambient-occlusion.md) rather than for this page.

What is still true is that **nothing runs this on its own**: the numbers above
came from a hand-run binary, there is no harness that records a pass's cost and
fails on a regression, and the browser gate still reports wall clock per demo
rather than per pass. `docs/backlog.md` carries that as what is left.

### The quality ladder, taken 2026-08-27

What ships, first, because the ladder is only readable against it: **stable
sphere-fitted cascades snapped to texels** — `crates/crcbl-render/src/shadow.rs`
fits a sphere around the eye rather than a box around the frustum, so rotating
the camera cannot change a cascade's extent, and quantises the light-space
origin to whole texels — **hardware PCF through a comparison sampler**, which is
`mesh.slang`'s `tile_pcf` taking its `SampleCmpLevelZero` taps over a rotated
disc and averaging them, the texel-denominated bias of the fifth decision, the
geometric normal of the sixth, the **normal-offset** direction of the seventh,
the **cascade cross-fade** of the eighth, the **rotated disc** of the ninth —
which is what the box in the sentence above became — and the **2026-08-26
re-tiling** that bought a second point light by shrinking `SHADOW_TILE` rather
than growing the image.

**The tile is now the binding constraint on shadow quality**: the 2026-08-26
re-tiling bought a second point light by shrinking `SHADOW_TILE` rather than
growing the image, so every map is 768 texels a side where it was 1024.

The ladder, in the order it should be climbed:

- **The tile resolution itself** is `docs/backlog.md`'s question rather than a
  rung, and it is what the tenth decision's PCSS ran into: the _physical_ sun's
  penumbra buys nothing at a 768-texel tile, which is why the shipped angle is
  an artistic one and says so.
- **Screen-space contact shadows — DECIDED 2026-08-30: on for the medium and
  high tiers, off on low.** A short march along the light's direction through
  the depth prepass, per fragment, that closes the contact gap no bias and no
  filter can: the sliver where a foot meets the floor or a book meets a shelf,
  finer than any atlas texel. `docs/plan/43-render-standards.md` §7 ranks it the
  cheapest real win among the screen-space marches, and it needs nothing the
  tree lacks — the prepass is there, the Hi-Z pyramid is there. The user took
  the recommendation as given: not a settings row of its own but a tier item —
  its own `RenderEffects` bit, in `DEFAULT_STACK`, that foundation (g)'s low
  preset clears; until the presets exist it is simply on. Priced on the three
  tiers before it counts, per the standing rule, and a screen-space term, so it
  leaves no trace on a tile and stacks with every rung above.

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
