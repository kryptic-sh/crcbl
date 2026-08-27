# Topic 43 — What a current engine ships, and where this one stands

A survey, not a plan to build all of it. Every other topic in this directory
argues one decision at length; this one exists because nothing was answering the
question a newcomer asks first — **what does this renderer not do that a
shipping engine in 2026 does?** — and the answer was spread across a 100 KB
topic document, a backlog and the absence of any document at all.

Written 2026-08-27, against the tree at that date. Every "where this one is" row
below was **read out of the code**, not recalled: the file is named so the claim
can be checked and so it fails
[`tools/check-doc-citations.sh`](../../tools/check-doc-citations.sh) if the file
moves.

## How to read this

The comparand is the feature set common to Unreal 5, Unity HDRP and Godot 4 —
not the frontier of any one of them. A row marked _missing_ is missing; a row
marked _refused_ has a reason written down in the topic that owns it, and
re-proposing it means arguing with that reason rather than with this table.

**This engine is not uniformly behind.** Its geometry and visibility path is
ahead of two of the three comparands, and that is worth stating first because
every gap below is easier to read against it.

| Area                    | Here                            | Owner                                                                            |
| ----------------------- | ------------------------------- | -------------------------------------------------------------------------------- |
| Geometry and visibility | **ahead**                       | [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md), [25-lod.md](25-lod.md) |
| Shadows                 | behind, ladder written          | [45-shadows.md](45-shadows.md)                                                   |
| Ambient occlusion       | behind, ladder written          | [46-ambient-occlusion.md](46-ambient-occlusion.md)                               |
| Reflections             | comparable for screen space     | [47-reflections.md](47-reflections.md)                                           |
| Antialiasing            | behind, ladder written          | [49-antialiasing.md](49-antialiasing.md)                                         |
| Irradiance probes       | first rung only                 | [50-irradiance-probes.md](50-irradiance-probes.md)                               |
| **Materials**           | **far behind, nothing written** | [37-materials.md](37-materials.md), and §2 below                                 |
| **Transparency**        | **absent, nothing written**     | §3 below                                                                         |
| **Volumetrics**         | **absent, nothing written**     | §4 below                                                                         |
| Global illumination     | behind                          | §5 below                                                                         |
| Post-processing         | behind                          | [48-post-processing.md](48-post-processing.md), §6                               |
| Upscaling               | absent, contract without a pass | [15-windowing.md](15-windowing.md), §7                                           |
| Decals                  | absent, planned                 | [33-decals.md](33-decals.md)                                                     |
| Particles               | simulated, never drawn          | [20-particles.md](20-particles.md)                                               |
| Sky and atmosphere      | absent                          | §8 below                                                                         |

## 1. What is already at or above the standard

Stated first, and with the same evidence discipline as the gaps.

- **GPU-driven submission.** Cull, draw-argument generation and per-bucket runs
  are compute passes writing indirect arguments the CPU never reads —
  `crates/crcbl-shaders/shaders/cull.slang` and `draw_gen.slang`. Unity HDRP
  does not do this at all; Unreal does it inside Nanite and not for the general
  path.
- **Mesh shaders with a cluster DAG and screen-space error LOD**, and an
  indirect-draw fallback that draws the same picture — `mesh_shader.slang`,
  `mesh_cluster.slang`, [25-lod.md](25-lod.md). The comparand here is Nanite,
  and the honest comparison is that this is the same shape at a fraction of the
  scope: no software rasteriser for sub-pixel triangles, no streaming.
- **Clustered forward lighting** with a froxel grid built on the GPU —
  `light_cluster.slang`. Standard, and correctly so.
- **Four backends behind one seam** with byte-comparable goldens across them,
  including a browser. None of the three comparands can produce a WebGPU frame
  that matches its native frame to a channel delta of two.
- **Reversed-Z, an HDR scene target, and linear lighting from the first pass.**
  All three are table stakes and all three are here.

## 2. Materials — the largest gap, and the one nothing yet plans

**What a current engine ships:** a metallic-roughness texture set — base colour,
normal, occlusion-roughness-metallic packed, emissive — with alpha modes
(opaque, mask, blend), and a tangent frame to sample the normal map in.

**Where this one is**, read out of `crcbl_shaders::mesh::GpuMaterial`:
`base_color`, `base_color_texture`, `metallic`, `roughness`, `tiling`,
`tile_metres`. One texture, and it is the base colour.
`crcbl_shaders::mesh::MeshVertex` is position, normal, colour and one UV — **no
tangent**. There is no emissive term anywhere in `shaders/mesh.slang`, and no
alpha mode of any kind.

So: **no normal mapping.** That is the single largest visual gap in this
renderer, and it is larger than every rung of the AO, shadow and AA ladders put
together — a surface here can only be as detailed as its triangles, which is why
the samples all read as flat-shaded greybox no matter how good the lighting on
them gets.

**What it would take**, in the order the dependencies fall:

1. **A tangent frame.** Either a fourth channel in `MeshVertex` — the `uv`
   member already wastes `zw`, so a packed tangent costs no stride at all, which
   is the cheap answer and the one glTF feeds directly — or screen-space
   derivatives of position and UV in the fragment stage, which costs no vertex
   data and is wrong on mirrored UVs. **The vertex route is the one to take**,
   and the reason is this file's own recurring one: derivatives differ in the
   last place across four rasterisers, and this workspace's goldens are
   cross-backend.
2. **A second and third texture page**, on the base-colour page's own pattern —
   `crcbl_render::scene::PageDesc` already owns layer 0 as the neutral texel,
   and a normal page's neutral is `(0.5, 0.5, 1.0)` rather than white.
3. **Emissive**, which is a factor and a page and one add before the tonemap —
   and which the bloom chain already existing makes worth more than it costs.
   **The factor half is built (2026-08-27)**: `GpuMaterial::emissive` is a
   linear radiance in the three words the row already padded with, glTF's factor
   times `KHR_materials_emissive_strength` fills it on import, and `mesh.slang`
   adds it last and unclamped. The emissive _page_ is not, and waits on the
   second texture page rung above it.
4. **Alpha modes.** `MASK` is a `discard` against a cutoff and is nearly free;
   `BLEND` is §3 and is not.

**Decision needed:** widening `MeshVertex` moves every mesh in every golden and
every `.crcblmesh` on disk. That is a version bump of the mesh format and a
re-bless of the whole suite, and it is the user's call when to spend it — see
[06-assets-scenes.md](06-assets-scenes.md) for the format's own compatibility
rule.

## 3. Transparency — absent, structurally

**What a current engine ships:** a sorted alpha-blended pass after the opaque
one, usually forward-shaded even in a deferred engine, plus alpha-to-coverage or
an order-independent scheme for foliage and hair.

**Where this one is:** `crates/crcbl-render/src/forward.rs` builds no pipeline
with a `BlendState` at all. The only blending in the render crate is
`crcbl_render::sprite_pass` and `crcbl_render::ui_pass`, both
`BlendState::alpha()` and both 2D compositing rather than shading. So the engine
can draw a translucent sprite and cannot draw a translucent _surface_.

**Why it is not simply "add a blend state":**

- The clustered forward path is the easy half — a transparent pass shades with
  the same froxel grid and the same BRDF, which is exactly the argument
  [44-lighting.md](44-lighting.md) gives for choosing clustered forward over
  deferred in the first place.
- **Sorting is the hard half**, and per-object sorting is what every engine
  actually ships and what every engine's artists then work around. This engine's
  submission is GPU-driven and its draw order comes out of `draw_gen.slang`'s
  per-bucket runs, so "sort back to front on the CPU" is not a step it has.
- **The interactions are already written down and are the reason to do this
  deliberately.** SSR on transparency is refused in
  [47-reflections.md](47-reflections.md) with the reason — a transparent surface
  writing the reflectivity attachment overwrites the opaque `F0` behind it while
  the scene colour there is a blend. The same argument applies to the depth
  prepass, to SSAO and to the Hi-Z pyramid: **all four read a single opaque
  depth**, and a transparent surface has no single depth.

**The order that keeps each step honest:** alpha-mask first (a `discard`, no
sorting, no new pass, and it is what foliage actually wants), then a blended
pass with GPU-sorted keys, and only then an order-independent scheme if the
sorting proves insufficient. Weighted-blended OIT is the cheap candidate and it
is a _approximation_ that cannot be blessed against a reference — which this
workspace's golden discipline should decide about before it is built, not after.

## 4. Volumetrics — absent, and the structure it needs already exists

**What a current engine ships:** exponential height fog with a single scattering
term, and froxel-based volumetric lighting — a 3D texture over the view frustum,
scattering integrated along each froxel column, applied as one composite over
the scene.

**Where this one is:** no fog of any kind. `grep -i fog` over
`crates/crcbl-shaders/shaders/` returns nothing.

**Why this is the cheapest large win on the list.** The froxel grid volumetric
fog wants is the froxel grid `light_cluster.slang` **already builds** — same
frustum subdivision, same light list per cell, and the light culling that is the
expensive part of a volumetric pass is already paid for by the opaque shading.
What is missing is a 3D scattering target, an integration pass along `z`, and a
composite. Three passes and no new culling.

**Height fog alone is cheaper still** — one term in the tonemap's input, no new
pass, no new resource — and it is most of the perceived benefit in an outdoor
scene. It should land first for the same reason FXAA landed before SMAA.

**But it is not free the way this section assumed, and whoever takes it should
know that before starting (2026-08-27).** The analytic exponential-height-fog
integral is `exp` twice over — once for the density falloff with height and once
for the transmittance along the ray — and this workspace's shading rule is that
no transcendental function may reach a colour, because four platforms'
implementations of them differ in the last place and a cross-backend golden has
no tolerance to absorb that. `log2` inside `froxel_of` is not a precedent for
it: that result is floored into an integer slice, and the fog's is a colour.

So height fog is a **decision**, not a slice, and it has three honest exits:
approximate the exponential with a rational fit the way the ACES tonemap
approximates the RRT — the same trick, and the reason the tonemap could be
blessed on all four backends; accept the fog term as the first shading path
whose goldens carry a tolerance rather than an exact compare, and say so where
the bless flow will meet it; or march the froxel grid instead, where the
integration is a sum over slices and no closed form appears. The third is the
most work and the only one that needs no exception. Nothing here changes the
ranking — fog is still the cheapest large win — but the row below buys a
determinism argument along with the pass.

## 5. Global illumination

**What a current engine ships:** at minimum a probe-based irradiance volume with
runtime updates (Unity's APV, Godot's SDFGI, Unreal's Lumen), plus screen-space
GI to catch what the probes are too coarse for.

**Where this one is:** L1 spherical-harmonic irradiance probes —
`shaders/probe.slang`, `compute_probe.slang` — decoded per pixel, and a flat
`frame.ambient` term underneath them. That is a real irradiance volume and it is
the right first rung.

**The gaps, in order of how much they cost:**

- **No specular IBL.** L1 irradiance answers "how much light arrives at this
  normal", which is a diffuse question. A rough metal needs prefiltered
  _radiance_ at a mip chosen by roughness plus a split-sum BRDF lookup, and this
  engine has neither. What it does instead is decode the L1 irradiance as
  approximate directional radiance for SSR's fallback — documented in
  `shaders/ssr.slang`, honest about being approximate, and the reason a metal
  out of the march's reach looks flat.
- **No screen-space GI.** The engine already marches screen space for
  reflections and already has a Hi-Z pyramid to march it with, so SSGI is closer
  than it looks: the same march with a cosine-distributed ray instead of a
  mirror one, accumulated. What it does **not** have is the temporal
  accumulation SSGI needs to be quiet — see the blocker in §9.
- **No lightmaps and no baked GI**, which is a deliberate absence rather than a
  gap: this engine's scenes are described in RON and imported from glTF with no
  bake step, and adding one is a pipeline decision rather than a render feature.

**Which trace family this engine can afford (2026-08-27).** "Ray marching" names
three techniques, and only one of them answers GI:

1. **Screen-space marching** — SSR, GTAO, and the SSGI rung above. Marches the
   depth buffer, needs no acceleration structure, runs on every target. Its
   limit is structural rather than a quality setting: the depth buffer holds
   only what is on screen, so off-screen geometry, backfaces and anything
   occluded contribute nothing. Excellent at contact scale, and on its own not
   global illumination at all.
2. **Signed-distance-field marching** (sphere tracing) — per-mesh distance
   fields in a BVH plus a coarse global field for the far term, with a cache
   holding surface radiance. This is what Unreal's Lumen software path and
   Godot's SDFGI are, and it is the family that actually removes the off-screen
   limit without ray-tracing hardware. It costs a bake and a volume texture per
   mesh, it has no answer for skinned or deforming geometry, and a coarse field
   loses thin geometry and contact detail — which is why Lumen is a **hybrid**
   (screen trace near, mesh SDF mid, global field far) rather than one march.
3. **Voxel cone tracing** — march a voxel mip with a widening cone. Cheaper than
   an SDF for the diffuse term, leaks through thin walls, and pays a
   revoxelisation cost every time the scene moves. Largely superseded by 2.

**"Faster than ray tracing" is the wrong reason to pick a march.** On a device
with ray-tracing hardware, Lumen's hardware path is both faster and more
accurate than its software path; the march exists for **reach**. Reach is
exactly what decides it here — WebGPU has no ray tracing at all and the browser
is a first-class target in [10-wasm-webgpu.md](10-wasm-webgpu.md), so on that
target a march is not the cheap option, it is the only one. The second reason is
this workspace's own rule: a march is adds and compares, so it carries no
transcendental into a colour and can be blessed on all four backends. That is
the argument the Hi-Z SSR rung already landed on.

So the ordering is cheapest real win first: **screen-space contact shadows**
(one march on the depth prepass, and the contact gap no shadow bias can close),
then **SSGI over the Hi-Z pyramid already built**, then the **cone trace over a
colour pyramid** that §9's delivery table already carries, and only then **mesh
plus global SDF** — which is the Lumen-class answer, and is a bake pipeline, a
volume-texture budget, a BVH and an honestly-documented skinned-geometry
exclusion rather than a slice.

## 6. Post-processing

Pipeline as it stands, verified in `crcbl_render::forward`'s pass list: scene →
bloom → exposure and tonemap → FXAA → UI.

| Stage          | Industry            | Here                                        |
| -------------- | ------------------- | ------------------------------------------- |
| Auto-exposure  | histogram, GPU      | **missing** — exposure is a runtime uniform |
| Tonemap curve  | ACES or AgX         | **built 2026-08-27**, ACES; clamp default   |
| Bloom          | Karis-average chain | **built**, off unless a view asks           |
| Colour grading | 3D LUT              | **missing**                                 |
| Depth of field | gather or scatter   | **missing**                                 |
| Motion blur    | per-object          | **missing**, blocked — §9                   |
| Lens artefacts | CA, vignette, grain | **missing**                                 |
| HDR display    | scRGB or HDR10      | **missing** — sRGB swapchain only           |

**The tonemap curve was the one to take first and it was nearly free.** Taken
2026-08-27: `shaders/tonemap.slang` carries Stephen Hill's ACES fit behind a
selector in its block, and [48-post-processing.md](48-post-processing.md)
records why the clamp is still what a view gets unless it asks — and why the
curve is ACES rather than AgX, which needs transcendentals this workspace's
goldens cannot absorb. What is left of this row is deciding which stacks default
to the curve, which is a re-bless rather than a design.

**Auto-exposure is second**, and it is a histogram compute pass plus a reduce —
the engine has compute, has readback-free ring buffers, and has the profiler to
show what it costs.

## 7. Upscaling and render scale

**What a current engine ships:** an internal render resolution decoupled from
the window, and a temporal upscaler — DLSS, FSR 3, XeSS, or Unreal's TSR.

**Where this one is:** [48-post-processing.md](48-post-processing.md) documents
an `[upscale]` stage between the resolve and the UI, and verified 2026-08-27
there is **no upscale pass, no render-scale knob, and no internal target whose
extent differs from the swapchain's**. The ordering is a contract for a pass
that does not exist.

**Render scale without an upscaler is still worth having** — a bilinear or
Catmull-Rom blit from a smaller internal target is the single largest
performance knob a settings menu can offer, it is one pass, and it is what
[15-windowing.md](15-windowing.md)'s borderless definition already assumes.
**That should land with the settings work**, because a graphics settings menu
whose resolution slider does nothing is the wrong first impression.

A _temporal_ upscaler is §9's blocker again and is not a near-term row.

## 8. Sky, atmosphere and environment

**What a current engine ships:** a sky pass — at minimum a cubemap or a
gradient, usually a physically-based atmosphere — that also feeds the ambient
and specular IBL terms.

**Where this one is:** nothing. The background is the scene target's clear
colour, and ambient is a flat constant plus probes.

This matters more than it sounds because of §5: **the environment term SSR falls
back to and the ambient term a metal needs are the same term a sky would
provide.** A gradient sky and an irradiance/radiance pair generated from it
would close part of §5 and all of §8 at once, which is why it is grouped here
rather than filed as scenery.

## 9. The one blocker that gates five features

**Motion vectors, and the previous-frame transform they need.**
`crcbl_shaders::mesh::GpuInstance` carries `transform`, `mesh`, `material`,
`sector`, `flags` and `base_vertex` — no previous transform, verified
2026-08-27. Nothing in the engine can say where a pixel was last frame.

That single absence blocks, in the order they would be wanted:

1. **TAA** — [49-antialiasing.md](49-antialiasing.md)'s ladder names it exactly.
2. **Temporal SSR**, which is what makes a rough reflection quiet.
3. **Temporal upscaling** — every one of DLSS, FSR 3, XeSS and TSR takes motion
   vectors as a required input. There is no non-temporal upscaler worth shipping
   above a plain blit.
4. **Per-object motion blur.**
5. **SSGI's accumulation**, per §5.

**The cheap insurance was never taken and the price only goes up.** Widening
`GpuInstance` is cheap now and expensive once more shaders index past
`INSTANCE_STRIDE` — the same argument `GpuInstance::sector` is already in the
record on. **Reserving the slot is a smaller decision than any of the five
features and unblocks all of them**, and it is the recommendation this document
makes most strongly.

## 10. What this document refuses to re-open

Each of these has its reason written down where the technique is owned. They are
listed here so a survey of gaps does not read as a list of things to build.

- **Deferred shading and visibility buffers** — [44-lighting.md](44-lighting.md)
  chose clustered forward, and the reason is transparency and MSAA, both of
  which §3 and that document's MSAA section still stand on.
- **VSM and EVSM shadow maps** — they light-leak through thin geometry, which is
  a correctness artefact rather than a quality one.
- **Virtual shadow maps** — the modern answer, and a topic rather than a rung:
  it replaces the fixed tile grid rather than improving it.
- **HBAO and HBAO+** — superseded by GTAO on the same input.
- **Float-hash rotations and interleaved-gradient noise**, anywhere — they
  amplify by construction the driver differences this workspace's goldens cannot
  absorb.
- **A second material model** — one BRDF, and every path shades with it.

## Delivery

Ordered by benefit per unit of work, which is not the order of the sections
above.

| Rung                                                     | Why here                                                                                 |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| **Reserve the previous-transform slot in `GpuInstance`** | §9 — smallest change on this page, unblocks five features, gets more expensive with time |
| **Exponential height fog**                               | §4 — one term, no new pass, but `exp` needs a determinism answer first                   |
| **Normal maps: tangent, page, sampling**                 | §2 — the largest visual gap, and the rest of the material set follows the same road      |
| **Emissive page** (the factor shipped 2026-08-27)        | §2 — rides the second texture page rung                                                  |
| **Alpha-mask materials**                                 | §3 — a `discard`, no sorting, and it is what foliage wants                               |
| **Render scale and a blit**                              | §7 — the settings menu's largest knob, one pass                                          |
| **A gradient sky feeding ambient and the SSR fallback**  | §8 — closes part of §5 at the same time                                                  |
| **Froxel volumetric fog**                                | §4 — the culling is already paid for                                                     |
| **Auto-exposure**                                        | §6 — a histogram and a reduce                                                            |
| **Blended transparency with GPU-sorted keys**            | §3 — the first rung here that touches the frame's structure                              |
| **Specular IBL: prefiltered radiance and a BRDF LUT**    | §5 — what makes a rough metal read as metal                                              |
| Colour grading, DOF, lens artefacts                      | §6 — polish, after the curve exists to grade against                                     |
| SSGI, temporal SSR, TAA, temporal upscaling              | §9's slot first; each is its own rung after it                                           |
