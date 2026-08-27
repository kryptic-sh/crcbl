# Topic 51 — Volumetrics: height fog, the froxel column, and light shafts

Written 2026-08-27, when the arithmetic landed and the passes did not. Its place
in the set is [18-render-features.md](18-render-features.md)'s index; what a
current engine ships and where this one stands against it is
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

**Not built.** Everything that puts either of those on a GPU per froxel: the
buffer, the two compute passes, the composite. Nothing in any shader reads
`crcbl_shaders::volumetric` yet.

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
| **1 — the sun's shaft**                               | Light shafts from the directional light, the effect anyone means by the word     | The froxel buffer, a scatter pass, an integrate pass, a composite pass, and the cascade lookup copied once      |
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

- **The buffer holds what the host says it should.** A readback of the
  scattering buffer for a scene with one directional light and a known medium,
  compared per froxel against `crcbl_shaders::volumetric` on the CPU. This is
  the rung-1 gate and it needs no golden image at all.
- **Doubling the medium's density squares the column's transmittance**, the same
  law `doubling_the_fog_density_squares_the_transmittance` holds the closed form
  to, now measured through the froxel path. A linear falloff passes a golden and
  fails this.
- **A froxel in shadow scatters nothing.** The observable that separates a real
  shaft from a uniform glow: the same medium, the same light, an occluder moved
  in and out, and the column behind the occluder going dark. Without it the pass
  is an expensive way to reproduce height fog.
- **Switching volumetrics on does not darken an unlit frame.** The two-media
  rule above, stated as a test: a scene with no lights and the same fog rows
  renders the same whichever path carries the transmittance.
