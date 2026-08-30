# Topic 51 — Volumetrics: height fog, the froxel column, and light shafts

Written 2026-08-27, when the arithmetic landed and the passes did not; the
plumbing landed the same day. Its place in the set is
[18-render-features.md](18-render-features.md)'s index; what a current engine
ships and where this one stands against it is
[43-render-standards.md](43-render-standards.md) §4, which now points here for
the froxel half.

## Where this is

`atlas_uv`, `tile_pcf` and the cascade walk are `mesh.slang`'s, copied once —
there being no `#include` — and held to it by `crcbl_shaders::volumetric`'s
`both_shaders_spell_the_same_atlas_walk`, which compares the two bodies letter
for letter with comments dropped and the uniform block's name normalised.
`shadow_slope` did not come with them, and neither did either bias or the
`n_dot_l` early return: all three are about a receiving _facet_, and a froxel is
a volume of air. Biasing one would push its scattering out of the shadow it
stands in, which reads as a lit rim along every shaft.

The punctual copy is guarded the same way:
`both_shaders_spell_the_same_punctual_light` holds `struct GpuLight`,
`punctual_falloff`, `spot_cone` and the list walk's load-bearing lines to
`mesh.slang`'s, and `crcbl_shaders::light`'s constants guard names
`volumetric.slang` beside the two files that already carried the kinds and the
stride. `volumetric_punctual_visibility` is `mesh.slang`'s `punctual_visibility`
on the cascade walk's terms — both biases dropped, the `n_dot_l` return with
them, the `w` test and the far-plane test kept, because those are what a
perspective map needs that an orthographic one does not.

The answer leaves the scatter pass in a **second buffer**, a `float4` per froxel
— the glow in `rgb`, the sun's visibility in `w` — and that is the design
decision this rung turned on; see below.

**What rung 2 left**: a light whose radius is shorter than the slice it sits in
is missed when the midpoint is outside it, which the far slices' length makes
ordinary for a small light far away. Rung 3's jitter along the slice is what
recovers it; `docs/backlog.md` carries it.

## The decisions

### The scattering target is a storage buffer on the existing froxel grid, not a 3D texture

A current engine writes its scattering into a 3D texture over the frustum,
because that buys a hardware trilinear filter on the lookup and a place to
reproject the previous frame into. Neither is free here:

- **The transient pool has no volume.** `crcbl_render::transient`'s
  `TransientImageDesc` has no depth field at all and `TransientPool::image`
  hard-codes `ImageType::D2` and `ImageViewType::D2`. A 3D target means widening
  that description and every literal that builds one, and it means the engine's
  first 3D image on four backends — which is exactly the shape of gap that let a
  read-only depth attachment reach `crcbl-dx12` as a refusal after passing all
  three cross-target clippy runs.
- **The grid it would duplicate already exists as a buffer.**
  `crcbl_render::light_grid` keeps `FROXEL_CAPACITY` froxels of `CLUSTER_STRIDE`
  words in one device-local storage buffer, and `light_cluster.slang` fills it.
  A second structure over the same subdivision, addressed by the same
  `froxel_of`, is a second thing to size, to barrier and to keep in step.

So rung 1 writes **one storage buffer, indexed by the froxel id `froxel_of`
already computes**, four floats per froxel: in-scattered radiance in `xyz` and
extinction in `w`. What that gives up is named rather than hidden — the
composite reads the nearest froxel instead of a filtered one, so a slow pan
across a shaft steps rather than slides. Rung 3 below is where that is bought
back, and it is the rung that needs the 3D image.

### The composite is its own fullscreen pass, not a term in `mesh.slang`

Tempting, because the fragment stage already computes `froxel_of` and already
applies the height fog. Two reasons not to:

- **The storage-buffer budget.** `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE`
  is 8, the number a WebGPU device guarantees, and it is a sum over every bind
  group layout in one pipeline layout. `crcbl_render::forward`'s mesh layout
  makes four of its storage buffers fragment-visible today — materials, the
  light list, the light grid and the probe table — so a fifth fits, but the same
  entries are `VERTEX`-visible on the non-mesh-shader path and that stage is
  where the headroom is not.
- **Where fog already sits is a known defect.** `docs/backlog.md` records that
  `mesh.slang` fogs the radiance it finishes and `ssr_blur.slang` adds the
  reflection afterwards, so a reflection arrives unfogged. Adding scattering in
  the same place would inherit that ordering and make it worse — the shaft would
  be behind the reflection as well as the fog. A pass after the reflection
  resolve fixes both at once for the volumetric term, and is the place the
  height fog should eventually move to as well.

### The two media are one medium, and only one of them may charge for absorption

Height fog's `optical_depth` and the froxel column's accumulated `transmittance`
are the same air integrated twice. Compositing both multiplies the transmittance
twice and darkens the frame by the square of what the medium actually does —
which reads as "the fog got thicker when volumetrics were enabled" and is the
failure this section exists to name in advance.

The rule: **when a froxel column is present it owns the transmittance**, and
`mesh.slang`'s height-fog composite is off for that frame. The froxel medium's
extinction is seeded from the same `fog` rows the fragment stage would have
read, so switching between them changes the sampling, never the medium.

### Slice thickness is computed, never assumed

The depth split is exponential —
`slice_near(k) = CLUSTER_NEAR * (CLUSTER_FAR / CLUSTER_NEAR)^(k / CLUSTER_DEPTH_SLICES)`
— so a slice's thickness is `slice_near(k + 1) - slice_near(k)` and the slices
differ by four orders of magnitude across the frustum. `integrate_slice` takes
that thickness as an argument for this reason; handing it a constant is the
mistake that makes the near field vanish and the far field glow.

Two corrections ride on top of it, and both are real rather than polish:

- **The thickness is along the view ray, not along `z`.** A froxel at the corner
  of the frame is longer than one at the centre by the secant of its angle from
  the axis, and at a wide field of view that is tens of per cent — a brightening
  toward the edges of the frame if it is dropped.
- **The last slice is unbounded.** `light_cluster.slang` gives its far side
  `FLT_MAX` so a light past `CLUSTER_FAR` is still listed. A thickness of
  `FLT_MAX` is an optical depth of infinity; the column's last slice takes the
  camera's far plane instead, and `crcbl_shaders::fog::MAX_OPTICAL_DEPTH` is the
  ceiling either way.

### The light loop will exist twice, and the guard has to say so

There is no `#include` in these shaders. `mesh.slang` evaluates a light's
attenuation, its cone and its shadow tile in the fragment stage, and the
scattering pass needs the same three answers at a froxel centre. That is a
second copy, and this workspace's rule for a second copy is a drift guard that
enumerates every source carrying it — `crcbl_shaders::sky`'s
`the_shader_spells_the_same_gradient` is the pattern, and it splits on the
function's own signature so a copy that was edited in one file and not the other
fails on the host with no GPU involved.

**This is the largest single cost of the froxel row** and it is why rung 1 below
is the sun alone: the cascaded directional shadow is one lookup shared by every
froxel in a column, the punctual set is where the copy gets wide, and a shaft of
sunlight through a window is the thing anyone means by volumetrics.

Rung 2 made the copy, and the guard is `crcbl_shaders::volumetric`'s
`both_shaders_spell_the_same_punctual_light`: the row struct and the two
functions compared as bodies with comments dropped, and the walk's own lines —
the count clamp, the row fetch, the two `reach` multiplies — compared as text,
because the walk is inline in both files rather than a function either could
name. The shadow tile — the third of the three answers above — joined the same
guard when the lamps started to occlude: `point_face` and `light_tile` as
bodies, the two tile checks and the map's frustum test as lines both files
spell, and the unbiased lookup as a line only `volumetric.slang` does.

### The froxel's visibility leaves the scatter pass in a buffer of its own

`volumetric_composite.slang` integrates the last partial slice along the pixel's
own ray, which is what keeps the frame off the slice boundaries — and that
integral needs the same scattering source `scatterMain` gave the whole froxel.
With the sun occluded, that source now depends on a cascade lookup. Three ways
to get it there, and only one of them is cheap:

- **Walk the atlas again in the composite.** One 3×3 PCF kernel per pixel, which
  at any real resolution is the cost of the opaque shadow pass a second time —
  on a feature whose entire argument is that it shades per froxel instead of per
  pixel. It also puts a second copy of the cascade walk in the tree.
- **Recover it from the column buffer.** `scatterMain` writes `source * (1 - T)`
  and `T`, so the source is a division away — by `1 - T`, which goes to zero
  exactly where a thin slice makes the answer meaningless.
- **Write it down.** One `float` per froxel, in a buffer the scan does not
  touch: a fraction of the column buffer beside it, one extra storage binding on
  each of the two layouts, and the cascade walk exists in exactly one shader.

The third. `crcbl_shaders::volumetric::LIGHTING_STRIDE` carries the argument
where the size is defined — and since rung 2 the punctual glow rides the same
row, for the same reason: the composite's partial slice owes its froxel's whole
source, and the list walk is the scatter pass's to make once.

## The rungs

| Rung                                                         | What it buys                                                                                                                                                                                                                                                                                                           | What it costs                                                                                                                                              |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **1a — the column** _(built)_                                | The frame a shaft will be drawn into, proved against the closed form                                                                                                                                                                                                                                                   | The froxel buffer, a scatter pass, an integrate pass and a composite pass                                                                                  |
| **1b-i — the sun in the medium** _(built)_                   | The sun's glow through the air, and the forward lobe that makes it a direction                                                                                                                                                                                                                                         | The phase function copied into both shaders, and the sun's direction in the params block                                                                   |
| **1b-ii — the shaft** _(built)_                              | The dark between the shafts: the sun occluded per froxel, not per pixel                                                                                                                                                                                                                                                | The cascade lookup copied once into `scatterMain`, a visibility buffer, and the drift guard over the copy                                                  |
| **2 — punctual lights** _(built)_                            | Glow around every point and spot light, cones made visible                                                                                                                                                                                                                                                             | The froxel light list in the scatter pass, and the attenuation and cone copy that goes with it                                                             |
| **3 — a 3D target, a coarser grid and a depth-aware lookup** | The filtered lookup, and a grid an eighth of the frame across with sample count as a quality tier — the industry's answer is temporal reprojection, and it is refused here (2026-08-30): a history buffer makes the frame a function of how many frames preceded it, which every golden in the tree is built not to be | A volume in the transient pool, the engine's first 3D image on four backends, and `46-ambient-occlusion.md`'s shared depth-aware upsample in the composite |
| **4 — a density field rather than a constant medium**        | Fog banks, ground mist, a medium that is somewhere rather than everywhere                                                                                                                                                                                                                                              | A source for the field, which is a content question and not only a rendering one                                                                           |

Rung 1 is what `43-render-standards.md`'s delivery table means by "froxel
volumetric fog". The rest is listed so the first rung can be built without
closing them off, not because any is scheduled.

## What each rung is checked by

The model's own properties are already pinned on the host, in
`crcbl_shaders::volumetric`: the phase function integrates to one over the
sphere at every anisotropy, its lobe mirrors with the sign of `g`, and a
homogeneous column cut into 1, 2, 7, 64 and 512 slices composites to the same
radiance. Six sabotages were red-checked against those eight tests, and the
naive `source * thickness` — the form a froxel pass reaches for first — fails
exactly one of them.

What the passes owe on top of that is the part the host cannot see:

- **The column is the closed form while the albedo is one.** _Built_ —
  `the_froxel_volume_integrates_the_same_medium_the_closed_form_does` in
  `crcbl`'s `mesh_e2e` draws the fixture cube twice under the same medium, once
  through each path, and compares the transmittance texel by texel. It is the
  rung-1a gate and it needs no golden image at all. It caught the mistake it was
  written for: the two shaders disagreed by one cell about where a slice begins,
  which drew a plausibly foggy frame and moved the transmittance by `0.13`
  against the `0.008` the tile-centre quantisation accounts for.
- **Switching the effect on with no medium is exactly the identity.** _Built_ —
  `the_froxel_volume_is_exactly_the_identity_at_zero_density`, byte for byte
  against the analytic path's own zero-density frame. Three passes run whatever
  the medium is, so "nobody set a fog" has to be a value rather than an absence.
- **Doubling the medium's density squares the column's transmittance.** _Built_
  — `doubling_the_density_squares_the_transmittance_through_the_froxel_volume`,
  the same law `doubling_the_fog_density_squares_the_transmittance` holds the
  closed form to, now measured through the froxel path. A linear falloff passes
  a golden and fails this.
- **The buffer holds what the host says it should.** _Built_ —
  `crates/crcbl/tests/mesh_e2e/froxels.rs`'s
  `the_froxel_column_is_the_scan_of_the_slabs_the_medium_scatters` copies the
  parameter block, the column and the lighting back, rebuilds every slab out of
  the block the two shaders were handed, scans it on the host and compares the
  result froxel by froxel. The frame-level equality above is the cheaper
  statement of part of the same thing; this is what separates a wrong scatter
  from a wrong scan, and what names the froxel rather than the frame. Two
  mistakes put into `integrateMain` in turn — the scan made inclusive, and its
  last slice dropped — failed here at froxel 0 and at froxel 284; the rendered
  checks failed on both too, but on a different one each time and neither said
  which pass had moved.

  It does **not** reach the composite sourcing its partial slice from a constant
  rather than from the froxel's own visibility. That is work the composite does
  per pixel and stores in no buffer, so there is nothing to read back;
  `the_composite_scatters_its_partial_slice_through_the_froxel_s_visibility`
  remains the text guard standing in for it, and `docs/backlog.md` carries the
  frame that would catch it.

- **An isotropic medium does not care which way the sun points.** _Built_ —
  `an_isotropic_medium_scatters_the_sun_the_same_way_whichever_way_it_points`
  reverses the sun at `g = 0` and demands every background texel come back byte
  for byte. It is what says the sun's direction reaches the picture through the
  phase function and through nothing else; a Lambert term or a dot product
  folded into the radiance draws a picture that still looks like fog and fails
  this. The mesh's own texels are excluded, because the same light shades the
  cube and that surface should change — and so are the cascades, because a
  shadow moves when the sun does and would answer "the frame changed" for a
  reason that is not the lobe.
- **A forward lobe brightens the frame the sun is ahead of.** _Built_ —
  `a_forward_lobe_brightens_the_frame_the_sun_is_ahead_of` puts the sun on the
  camera's own forward axis and then reverses it, so every background texel's
  scattering angle flips from near zero to near `pi`, and demands the first be
  brighter at **every** texel rather than on average. The ratio it holds to was
  swept rather than chosen, and it does not move with density: a background
  column runs to `CLUSTER_FAR` and is opaque, so what those texels carry is the
  scattering source itself.
- **A froxel in shadow scatters nothing.** _Built_ —
  `the_medium_behind_an_occluder_loses_the_sun_and_keeps_the_sky` draws one
  scene twice, with the cascades on and off, so the only difference between the
  two frames is whether a froxel may see what stands in front of it. It asks
  four things, and each rules out a different way of being wrong: no background
  texel is brighter, a large patch is meaningfully darker, the darkening is
  **not** the same everywhere — an occluder has an edge and a dimmer does not —
  and nothing goes to black, because the environment term is not occluded.
- **A medium with no sun in it does not notice the cascades.** _Built_ —
  `shadowing_the_froxels_leaves_a_sunless_medium_exactly_alone`, byte for byte.
  The off-switch, and it is what says the visibility multiplies the sun term and
  only the sun term: a factor that reached the environment term or the
  transmittance moves a bit here while still drawing a shaft that looks right in
  the test above.
- **Switching volumetrics on does not darken an unlit frame.** The two-media
  rule above, stated as a test: a scene with no lights and the same fog rows
  renders the same whichever path carries the transmittance.
- **The glow lanes hold the froxel's list walked at its midpoint.** _Built_ —
  `crates/crcbl/tests/mesh_e2e/froxels.rs`'s
  `the_glow_in_the_buffer_is_the_froxel_s_list_walked_at_its_midpoint` puts a
  point light and a spot light in the column's medium, walks **every** light in
  the scene from each slab's midpoint on the host — the rows the renderer
  uploads, through `mesh.slang`'s falloff and cone and the medium's phase — and
  demands the buffer's `rgb` froxel by froxel. Every light rather than the
  froxel's list, so a light the clustering pass left out of a froxel it reaches
  fails here too. Non-vacuous by construction: some froxels glow and some are
  exactly dark, and the spot's radius holds froxels both in its beam and out of
  it. Dropping the cone reddened it at froxel 96; dropping the falloff window
  reddened it and the frame test below.
- **A lamp in the medium glows, and only where the lamp is.** _Built_ —
  `a_lamp_in_the_medium_glows_and_only_where_the_lamp_is` draws the fixture in a
  black, sunless medium with one green lamp ahead of the eye, at
  `light_scattering` zero and then two, and asks three things of the background:
  no texel darkens, the brightest lifts by a measured amount, and three quarters
  of them — the columns the lamp's radius never reaches — do not move byte for
  byte. The last is the falloff window's claim, and a falloff without it lights
  the whole frame a little: 4096 of 39110 texels unmoved where the fixture
  measures 33713.
- **A lamp's glow stops at the wall its map holds.** _Built_ —
  `crates/crcbl/tests/mesh_e2e/froxels.rs`'s
  `a_lamp_s_glow_stops_at_the_wall_its_map_holds` reads the column twice, with
  the atlas drawn into and with it left at its reversed-Z clear, and compares
  the glow lanes froxel by froxel. It first asks that the clear reads as lit —
  the sun lane exactly one everywhere — because the glow test above rests on
  that; then that no froxel brightens, that some lose more than half their glow
  to a map, and that most glowing froxels keep it to the digit, which is what
  separates a map from a dimmer. Returning `1.0` from the lookup left 0 of 105
  glowing froxels occluded and reddened it; the glow test stayed green, as it
  should, because it was drawn against the clear.
