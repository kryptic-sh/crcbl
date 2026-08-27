# Topic 51 — Volumetrics: height fog, the froxel column, and light shafts

Written 2026-08-27, when the arithmetic landed and the passes did not; the
plumbing landed the same day. Its place in the set is
[18-render-features.md](18-render-features.md)'s index; what a current engine
ships and where this one stands against it is
[43-render-standards.md](43-render-standards.md) §4, which now points here for
the froxel half.

## Where this is

**Built.** Exponential height fog — `crcbl_shaders::fog`, an exponential
constructed out of operations IEEE-754 pins down, two rows at the end of
`FrameUniforms`, the composite in `mesh.slang`'s fragment stage, and
`ForwardRenderer::set_fog` to switch it on. It answers absorption along the view
ray and nothing else: a surface behind fog is dimmed toward the fog colour, and
no light in the scene puts anything _into_ that fog.

**Built.** The scattering model — `crcbl_shaders::volumetric`. `phase` is
Henyey-Greenstein, the angular half; `integrate_slice` is what one slice of a
column owes the composite, the radiance it adds and the fraction it transmits,
with the self-attenuation term that makes slicing not change the picture.

**Built.** The frame that carries a column — rung 1a. `volumetric.slang` cuts
the frustum into the same froxels `light_cluster.slang` does and writes what
each slab of air scatters and transmits, then scans each column into an
exclusive prefix; `volumetric_composite.slang` reads that prefix, integrates the
last partial slice along the pixel's own ray and composites over the frame.
`crcbl_render::volumetric` owns the three passes and
`RenderEffects::VOLUMETRIC_FOG` switches them on — off by default, and the frame
block's fog density is zeroed when they run, so the medium is charged once.

**Built.** The sun in the medium — rung 1b-i. `scatterMain` and the composite's
partial slice both add `sun_radiance * phase(anisotropy, cos_theta)` to the
environment term, along the segment each is integrating, with the sun's
direction and the medium's anisotropy carried in the params block.
`crcbl_render::Fog` gained `sun_scattering` and `anisotropy` and both default to
zero, so the term is additive over rung 1a rather than a mode: a scene that sets
neither renders bit for bit what it did before, which is what makes rung 1a's
three tests this feature's off-switch.

**Not built.** The occlusion — rung 1b-ii. Nothing in `scatterMain` reads a
shadow cascade, so every froxel is lit as though it saw the sun. That draws the
glow around a light without the shafts between the things blocking it, which is
the half of the effect anyone actually means by the word. What it needs is the
cascade atlas and a comparison sampler bound to a compute pass, and five
functions copied out of `mesh.slang` — `atlas_uv`, `light_tile`, `tile_pcf`, the
cascade walk and the `SHADOW_*` constants — under a drift guard that enumerates
the copy. `shadow_slope` does not come with them: a participating medium has no
facet to bias against.

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

## The rungs

| Rung                                                  | What it buys                                                                     | What it costs                                                                                                   |
| ----------------------------------------------------- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **1a — the column** _(built)_                         | The frame a shaft will be drawn into, proved against the closed form             | The froxel buffer, a scatter pass, an integrate pass and a composite pass                                       |
| **1b-i — the sun in the medium** _(built)_            | The sun's glow through the air, and the forward lobe that makes it a direction   | The phase function copied into both shaders, and the sun's direction in the params block                        |
| **1b-ii — the shaft**                                 | The dark between the shafts: the sun occluded per froxel, not per pixel          | The cascade lookup copied once into `scatterMain`, and the drift guard that enumerates the copy                 |
| **2 — punctual lights**                               | Glow around every point and spot light, cones made visible                       | The froxel light list in the scatter pass, and the attenuation and cone copy that goes with it                  |
| **3 — a 3D target and temporal reprojection**         | The filtered lookup, and enough samples per froxel to stop a shaft from crawling | A volume in the transient pool, the engine's first 3D image on four backends, and a history buffer to reproject |
| **4 — a density field rather than a constant medium** | Fog banks, ground mist, a medium that is somewhere rather than everywhere        | A source for the field, which is a content question and not only a rendering one                                |

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
- **The buffer holds what the host says it should.** Not built: a readback of
  the scattering buffer compared per froxel against `crcbl_shaders::volumetric`
  on the CPU. The frame-level equality above is the cheaper statement of the
  same thing and is what rung 1a shipped with; a per-froxel readback is what
  will separate a wrong scatter from a wrong scan once the two stop being
  algebraically equal at rung 1b.
- **An isotropic medium does not care which way the sun points.** _Built_ —
  `an_isotropic_medium_scatters_the_sun_the_same_way_whichever_way_it_points`
  reverses the sun at `g = 0` and demands every background texel come back byte
  for byte. It is what says the sun's direction reaches the picture through the
  phase function and through nothing else; a Lambert term or a dot product
  folded into the radiance draws a picture that still looks like fog and fails
  this. The mesh's own texels are excluded, because the same light shades the
  cube and that surface should change.
- **A forward lobe brightens the frame the sun is ahead of.** _Built_ —
  `a_forward_lobe_brightens_the_frame_the_sun_is_ahead_of` puts the sun on the
  camera's own forward axis and then reverses it, so every background texel's
  scattering angle flips from near zero to near `pi`, and demands the first be
  brighter at **every** texel rather than on average. The ratio it holds to was
  swept rather than chosen, and it does not move with density: a background
  column runs to `CLUSTER_FAR` and is opaque, so what those texels carry is the
  scattering source itself.
- **A froxel in shadow scatters nothing.** The observable that separates a real
  shaft from a uniform glow: the same medium, the same light, an occluder moved
  in and out, and the column behind the occluder going dark. Without it the pass
  is an expensive way to reproduce height fog.
- **Switching volumetrics on does not darken an unlit frame.** The two-media
  rule above, stated as a test: a scene with no lights and the same fog rows
  renders the same whichever path carries the transmittance.
