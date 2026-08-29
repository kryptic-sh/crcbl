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

| Area                    | Here                                                 | Owner                                                                            |
| ----------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------- |
| Geometry and visibility | **ahead**                                            | [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md), [25-lod.md](25-lod.md) |
| Shadows                 | behind, ladder written                               | [45-shadows.md](45-shadows.md)                                                   |
| Ambient occlusion       | behind, ladder written                               | [46-ambient-occlusion.md](46-ambient-occlusion.md)                               |
| Reflections             | comparable for screen space                          | [47-reflections.md](47-reflections.md)                                           |
| Antialiasing            | behind, ladder written                               | [49-antialiasing.md](49-antialiasing.md)                                         |
| Irradiance probes       | first rung only                                      | [50-irradiance-probes.md](50-irradiance-probes.md)                               |
| **Materials**           | **far behind**, ladder in §2                         | [37-materials.md](37-materials.md), and §2 below                                 |
| **Texture filtering**   | **a chain, trilinear, 8× anisotropic, uncompressed** | §2's filtering subsection                                                        |
| **Transparency**        | **absent**, argued                                   | §3 below                                                                         |
| **Volumetrics**         | height fog and a froxel column                       | [51-volumetrics.md](51-volumetrics.md), and §4 below                             |
| Global illumination     | behind                                               | §5 below                                                                         |
| Post-processing         | behind                                               | [48-post-processing.md](48-post-processing.md), §6                               |
| Upscaling               | spatial half built, temporal blocked                 | [15-windowing.md](15-windowing.md), §7                                           |
| Decals                  | absent, planned                                      | [33-decals.md](33-decals.md)                                                     |
| Particles               | simulated, never drawn                               | [20-particles.md](20-particles.md)                                               |
| Sky and atmosphere      | a gradient, no atmosphere                            | §8 below                                                                         |

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
- **A physically based BRDF**, not a Blinn-Phong lobe with a roughness slider
  bolted on: Cook-Torrance with Trowbridge-Reitz `D`, Smith height-correlated
  visibility and Schlick's `F`, over glTF's own metallic-roughness pair —
  `shaders/mesh.slang`'s `ggx_lobe`. `f0` interpolates from the dielectric 0.04
  to the base colour on `metallic` and the diffuse albedo scales down by it, so
  a conductor reflects and does not scatter. This is the same BRDF Unreal, Unity
  HDRP and Filament shade with, and [44-lighting.md](44-lighting.md) carries the
  derivation. **The BRDF is not this engine's PBR gap** — §2 and §5 are, and
  that document's ladder is what closes them.

## 2. Materials — the largest gap

**What a current engine ships:** a metallic-roughness texture set — base colour,
normal, occlusion-roughness-metallic packed, emissive — with alpha modes
(opaque, mask, blend), and a tangent frame to sample the normal map in.

**Where this one is**, read out of `crcbl_shaders::mesh::GpuMaterial`:
`base_color`, `base_color_texture`, `metallic`, `roughness`, `emissive`,
`tiling`, `tile_metres`. One texture, and it is the base colour.
`crcbl_shaders::mesh::MeshVertex` is position, normal, colour and one UV — **no
tangent**. `emissive` is a factor with no page behind it, and there is no alpha
mode of any kind.

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
   data. **The vertex route is the one to take, and the reason is mirrored
   UVs**: the derivative route cannot recover the handedness glTF stores in a
   tangent's `w`, so every mirrored shell on a character lights inside out.
   Corrected 2026-08-27 — this rung used to argue determinism instead, and that
   argument was wrong on its face: `shaders/mesh.slang`'s `geometric_normal_of`
   already takes `ddx`/`ddy` in the fragment stage, its result drives the shadow
   slope bias, and the cross-backend goldens hold over it.
2. **A second and third texture page**, on the base-colour page's own pattern —
   `crcbl_render::scene::PageDesc` already owns layer 0 as the neutral texel,
   and a normal page's neutral is `(0.5, 0.5, 1.0)` rather than white. **Linear,
   not sRGB**: `crcbl_render::forward`'s `BASE_COLOR_PAGE_FORMAT` is
   `Rgba8UnormSrgb` because a base-colour texel is a colour, and a normal,
   roughness, metalness or occlusion texel is a number. Decoding one through the
   sRGB curve is wrong by a gamma and looks merely "shinier than intended",
   which is why it survives review — see [44-lighting.md](44-lighting.md)'s
   rung 2.
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

### Texture filtering — mips, anisotropy, compression

**What a current engine ships:** every texture carries a full mip chain, built
offline in linear light and shipped inside the asset; the sampler is trilinear
with 8× to 16× anisotropy behind a settings row; and the bytes are
block-compressed — BC7 for colour, BC5 for a two-channel normal, BC4 for a
single-channel mask — with ETC2 or ASTC standing in on a device without BC.
Filtering is not a technique a player names, which is why a survey walks past
it: it is the difference between a floor that shimmers as the camera moves and
one that does not.

**Where this one is**, read out of `crcbl_render::forward` and
`crcbl_render::texture`: the base-colour page is uploaded by
`upload_texture_mip_layers` with every layer's whole chain, its sampler is
`Linear` on all three filters over that chain at
`ForwardRenderer::anisotropy_for`'s anisotropy — `DEFAULT_ANISOTROPY`, eight,
clamped to the device where it was granted `Features::SAMPLER_ANISOTROPY`, one
where it was not — and the page is `Rgba8UnormSrgb`, uncompressed. A minified
texel is therefore a trilinear blend of the two levels nearest its footprint
along the footprint's long axis, which `tests/tiling_e2e.rs`'s grazing floor
holds against the isotropic control. `ForwardRenderer::set_anisotropy` moves it
at runtime — a new sampler, and each frame slot's mesh-layout groups rebuilt at
that slot's own `begin_frame`. What is still missing is the player's say — the
anisotropy is the engine's default until a settings row turns the knob — and
compression: `Format` holds BC1 through BC7 behind
`Features::TEXTURE_COMPRESSION_BC` and nothing in the renderer asks.

**What it would take**, in the order the dependencies fall — and none of it
waits on this section's stride decision, which is what makes it the cheapest
unblocked rung on this page:

1. **The chain, built on the host and uploaded whole — built 2026-08-29.**
   `crcbl_render::mip` owns the filter `crcbl_scene::gltf_render` used to keep
   for itself — `resample`, an alpha-weighted box in linear light — and `chain`
   is that filter run once per level down to one texel.
   `ForwardRenderer::with_scene` builds every layer's chain and
   `upload_texture_mip_layers` records one copy per level of every layer;
   `tests/forward_e2e/page.rs` reads the levels back on every backend and
   compares them with the host's bytes. **On the host, not in a compute pass** —
   [06-assets-scenes.md](06-assets-scenes.md) and
   [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md) both named a compute
   pass, and both are corrected (2026-08-29). Three reasons, each sufficient on
   its own: a compute pass over an sRGB page needs a `UNORM` view alias over the
   image, which is `ImageDesc::view_formats`, which does not exist, and
   `docs/backlog.md` records that WebGPU refuses the reinterpretation without
   it; a host filter is adds and one divide per texel, so the page's bytes are
   identical on all four backends and the goldens hold, where a device-built
   chain is four drivers' rounding; and offline is what every current engine
   does anyway — the mips ship in the asset, and a compute pass is for a texture
   the frame itself produced, of which this engine has none. Two rules the
   filter keeps: average in linear light and re-encode, never average the
   encodings (the importer's own comment says what that costs); and weight by
   alpha so a transparent texel does not bleed. A non-colour page, when this
   section lands one, averages its numbers plainly and **renormalises a normal
   after averaging** — the mean of unit vectors is shorter than one, and the
   length it lost is the roughness that [44-lighting.md](44-lighting.md)'s rung
   4 exists to put back.
2. **The sampler: trilinear and anisotropic, with the player's key and its row —
   built 2026-08-29.** `mag`, `min` and `mip` are `Linear` and `lod_max` covers
   the chain in `ForwardRenderer::with_scene`. Five goldens moved and were
   re-blessed on radv at `Tolerance::RASTERISER`: `cube`, `cube_97x61` and
   `lights` in `crates/crcbl/tests/golden`, `room` and `live` in
   `apps/lantern/tests/golden`. One fixture moved with them: the lights scene's
   sun key is halved in `dim_sun`, because the probe reads each quadrant's
   brightest pixel and on the textured pyramid that pixel is the checker's white
   texel — a flat quarter under the green pool while the page sampled nearest,
   one point on a bilinear ramp now, where the sun out-shone the pool by a step.
   The anisotropy is `ForwardRenderer::anisotropy_for`'s: `DEFAULT_ANISOTROPY` —
   eight — clamped to `Limits::max_sampler_anisotropy` where the device was
   granted `Features::SAMPLER_ANISOTROPY`, and one where it was not. Granted,
   not supported: the feature is optional at every open site, so
   `GpuContextDesc`'s default optional features, `OffscreenSetup`'s and the
   scaffold's all name it. The renderer's knob is in:
   `ForwardRenderer::set_anisotropy`, clamped to `1.0..=max_sampler_anisotropy`
   where the feature was granted and to one where it was not, creates the
   sampler and lets each frame slot rebuild its mesh-layout groups at its own
   `begin_frame` — the moment that slot's previous submission has retired — so a
   frame in flight keeps the sampler it was recorded with, and the replaced
   sampler is destroyed once no slot names it. `tests/tiling_e2e.rs` draws a
   grazing greybox floor through that knob at one and at the default and holds
   the far band's line contrast apart on every backend — the one observation of
   a sampler's anisotropy any backend allows. The key is in:
   `[engine.video] anisotropic_filtering`, read by
   `crcbl::settings::anisotropic_filtering` — `1` off, up to
   `MAX_ANISOTROPIC_FILTERING` (the desktop preset's sixteen), the engine's
   default when absent — written by `set_anisotropic_filtering`, carried by
   `GpuContext::anisotropic_filtering`, and handed to the knob by `apps/viewer`
   at open and on reload, beside `render_scale`. It is the first key that may
   ask for more than the engine's default, on the ground that the device's own
   ceiling bounds the spend and the knob clamps to it. The row is
   `apps/options`'s `ANISOTROPY`: `menu::ANISOTROPIES` steps `1`, `2`, `4`, `8`,
   `16` on `FRAME_CAPS`' pattern, `RESET` puts it back to the default, and it
   wears `NEXT_START_MARK` since that screen draws no page. **On WebGPU the
   reported limit is one**, by a decision `docs/backlog.md` carries — the API
   has no query for the ceiling — so the browser filters isotropically until
   that decision is revisited; the specification's own text is that an ask above
   the platform's maximum is clamped and never refused, which is the ground for
   reporting the desktop figure there instead.

   **What this costs the goldens is bounded and stated.** The specification
   bounds the level-of-detail computation rather than fixing it and leaves the
   anisotropic footprint to the implementation, so a minified textured surface
   is the one place four rasterisers are _permitted_ to differ by more than a
   last bit. The frames that hold a textured layer re-bless under
   `Tolerance::RASTERISER`, and every exactness claim about a page — that its
   bytes reach the device whole, that a row's layer index selects the layer —
   stays on a magnified or `SampleLevel` read, where the filter is a bilinear
   blend of four known texels and the answer is arithmetic. The trilinear slice
   measured which goldens carry one — the five above — and no other frame moved
   past the tolerance. The anisotropic slice measured the other half, 8× on radv
   against the 1× references: every one of the thirteen render scenes drew the
   same frame — the demo page is magnified everywhere they show it — lantern's
   `room` moved 360 pixels by at most thirteen, inside the tolerance, and `live`
   moved past it. So the browser at one against the desktop's eight costs
   nothing on the shared set, `room` and `live` are re-blessed at eight, and the
   browser compares neither.

3. **`texture_quality` as a `lod_min` clamp**, which is all the cheap form of
   that key means: the top level or two of every chain go unread, the memory
   they cost stays until residency exists, and the picture is what a smaller
   page would draw. Topic 25's streaming is the expensive form and is not this
   rung.
4. **Block compression, at the bake tool and not before.** The device half is
   done — `Format::Bc7RgbaUnormSrgb` for colour, `Bc5RgUnorm` for the
   two-channel normal [44-lighting.md](44-lighting.md)'s rung 2 asks for,
   `Bc4RUnorm` for a mask or a lone roughness — and the encoder is the whole of
   the work: the workspace has none, and one is a new dependency and therefore
   the user's decision. **The standard answer is KTX2 carrying Basis
   Universal**, which is what glTF's `KHR_texture_basisu` names and what
   three.js, Babylon and Godot import — one asset that transcodes at load to BC7
   on a desktop and to ETC2 or ASTC on a device without BC. That matters here
   because the browser is a first-class target and BC is an optional WebGPU
   feature, so a BC-only pipeline ships uncompressed to part of the site. The
   `gltf` crate this workspace resolves exposes no feature for that extension,
   so the importer would read it out of the document's JSON by hand; and
   `Format` has no ETC2 or ASTC variant, which is a seam change on the day a
   device without BC is targeted. Until then a page is uncompressed and four
   times its BC7 size — a memory figure rather than a picture, which is why this
   rung is last.

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

## 4. Volumetrics — height fog and the sun's shaft through the froxel column

> **The ladder moved to [51-volumetrics.md](51-volumetrics.md) on 2026-08-27**,
> where the rungs, the decisions the froxel pass has to make before it is
> written, and what each rung is checked by all live. What follows is this
> topic's own account: how far behind the industry this area is, and why.

**What a current engine ships:** exponential height fog with a single scattering
term, and froxel-based volumetric lighting — a 3D texture over the view frustum,
scattering integrated along each froxel column, applied as one composite over
the scene.

**Where this one is:** exponential height fog, built 2026-08-27 and off unless a
caller asks; and the froxel column that carries the sun's shaft, behind
`RenderEffects::VOLUMETRIC_FOG` — three passes over the clustering pass's own
subdivision, proved against the closed form, with the sun scattering into it
through a Henyey-Greenstein lobe and occluded per froxel by the same cascades
the surfaces are shadowed by. Rung 1 of [51-volumetrics.md](51-volumetrics.md)
is closed. What is open above it is punctual lights in the medium, a 3D target
with temporal reprojection, and a density field — none scheduled, and each
argued there.

**Why this is the cheapest large win on the list.** The froxel grid volumetric
fog wants is the froxel grid `light_cluster.slang` **already builds** — same
frustum subdivision, same light list per cell, and the light culling that is the
expensive part of a volumetric pass is already paid for by the opaque shading.
Those three passes landed 2026-08-27 — `crcbl_render::volumetric`, over a
storage buffer on the grid the light pass already fills rather than the 3D
texture a current engine uses. [51-volumetrics.md](51-volumetrics.md) says why,
and what that choice gives up.

**Height fog alone is cheaper still** — one term in the tonemap's input, no new
pass, no new resource — and it is most of the perceived benefit in an outdoor
scene. It landed first, 2026-08-27, for the same reason FXAA landed before SMAA
— `ForwardRenderer::set_fog` switches it on.

**The `exp` this needs looked like a blocker and is not one — decided and
answered 2026-08-27.** The analytic exponential-height-fog integral is `exp`
twice over, once for the density falloff with height and once for the
transmittance along the ray, and this workspace's shading rule is that no
transcendental may reach a colour, because four platforms' implementations of
them differ in the last place. `log2` inside `froxel_of` is not a precedent for
it: that result is floored into an integer slice, and the fog's is a colour.

**Two things this section previously asserted turned out to be wrong, and both
mattered to the decision.** The first is that "a cross-backend golden has no
tolerance to absorb that": every golden in this tree is compared under
`crcbl_golden::Tolerance::RASTERISER`, and `Tolerance::EXACT` appears in no
image test at all — only in `compare-png`'s and `compare-readback`'s argument
parsing. The rule is therefore not a consequence of an exact compare; it stands
on its own reasoning, which is better stated as **keeping the ceiling on a
disagreement known** rather than absorbed. The second is that the three exits
below were the only ones.

**The fourth exit is `crcbl_shaders::fog`, and it is cheaper than all three.**
An exponential built out of nothing but the operations the rule already permits
— range reduction against a two-part `ln 2`, a Taylor kernel in Horner form over
the reciprocal factorials, and `2^-n` written straight into an IEEE exponent
field. Every step is an operation IEEE-754 specifies exactly, so the only
freedom a compiler has left is whether to contract a multiply and an add, which
is worth a unit in the last place; measured against `f64::exp` over its whole
domain the construction is within two. There is no fit to transcribe wrongly, no
table to cook, no binding to add and no exception to declare. The three exits it
replaces were: a rational fit the way the ACES tonemap fits the RRT; the fog
goldens carrying a tolerance where no other shading path does; or marching the
froxel grid so no closed form appears at all.

So height fog was a **slice** again, and it is **built (2026-08-27)**. The
arithmetic and its proof first — `crcbl_shaders::fog`, with `optical_depth`
checked against Simpson quadrature and the saturation that keeps a camera below
the fog plane from producing infinities — then the frame: two `float4` rows at
the end of `FrameUniforms`, the Slang mirror in `mesh.slang`, the composite over
the finished radiance, and `ForwardRenderer::set_fog`.

**The observable is the law, not the difference.** Uniform fog's optical depth
is `density * distance`, so doubling the density **squares** the transmittance
at every texel at once, whatever that texel's distance —
`doubling_the_fog_density_squares_the_transmittance` recovers the transmittance
two independent ways per texel and holds them to it. A linear falloff fails it;
so does the height sign inverted, against
`raising_the_reference_plane_thickens_the_fog`. Both were red-checked by
sabotage on real hardware rather than assumed.

The froxel row below still buys the scattering the closed form cannot, and it
now inherits a transmittance function rather than needing one.

**The froxel row's arithmetic landed 2026-08-27**, on height fog's pattern: the
model and its proof first, the passes after. `crcbl_shaders::volumetric` is
`phase` — Henyey-Greenstein, the angular half that makes fog glow around a light
rather than uniformly — and `integrate_slice`, which is what one slice of a
froxel column owes the composite: the radiance it adds and the fraction it
transmits, in one closed form.

Neither reaches for a transcendental. The exponential is `fog::exp_neg`, and the
phase function's three-halves power is written `d * sqrt(d)`, because IEEE-754
requires a correctly rounded `sqrt` and specifies nothing about `pow` — a fourth
escape from the shading rule, beside the cooked table (`crcbl_shaders::dfg`),
the IEEE construction (`crcbl_shaders::fog`) and the host-side projection (the
sky's spherical harmonics).

**The observable is that slicing does not change the picture.**
`splitting_a_slice_composites_to_the_same_radiance` cuts a homogeneous column
into 1, 2, 7, 64 and 512 slices and holds them all to the same radiance and the
same transmittance. That is what the self-attenuation term inside the slice buys
and it is the only test in the module the naive `source * thickness` fails —
which matters, because that form reads correctly and is what a froxel pass
reaches for first. Its failure direction is the visible one: more slices, more
light.

The frame followed — the scattering target, the pass that fills it, the scan
along `z`, the composite, the Slang mirror of both functions and the drift guard
that holds it to this module — as [51-volumetrics.md](51-volumetrics.md)'s rungs
1a through 1b-ii, all built by 2026-08-28. What is left of §4 is that document's
rungs 2 to 4: punctual lights in the medium, a 3D target with temporal
reprojection, and a density field.

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

  **The prefiltered half exists since 2026-08-29** as
  `crcbl_shaders::sky_prefilter`: a gradient is linear in its three colours, so
  its convolution against the lobe is two weights over `(|R.y|, roughness)` and
  one committed table serves every sky. **And it is read since the same day**:
  `ssr.slang`'s miss fallback takes the sky through the table at the surface's
  roughness (`sky_prefiltered`), so a rough metal's reflection of the sky is the
  cone it gathers rather than the one mirror direction. **And the `DFG` half
  since the same day**: `crcbl_shaders::dfg::pair_texels` is the table's two
  channels as a second image beside the sky's, and `f0 · scale + bias` at
  `(N·V, roughness)` scales the environment in place of Schlick along the
  reflected direction. The rung is built.

  **The determinism rule does not block it**, which is worth stating because it
  blocks so much else on this page. Karis's split-sum needs a `DFG` table over
  `(N·V, roughness)` and a roughness-indexed radiance mip chain; both are
  **baked at build time and committed like a shader artifact**, so the run-time
  cost is a multiply and two fetches and four backends read the same bytes.
  Baking a transcendental into a table is the general escape, and
  [44-lighting.md](44-lighting.md)'s rung 3 is where it is written down.

- **Multi-scatter energy compensation is in the frame (2026-08-27)**, and this
  entry stays on the page because the rung above still reads its table.
  Single-scatter GGX drops every microfacet bounce after the first, so a rough
  conductor rendered too dark by an amount that varies with roughness and `N·V`
  and that no constant factor could absorb — 0.317 of the light at the roughest
  row, seen head on. `crcbl_shaders::dfg` cooks `tables/dfg.bin`, `mesh.slang`
  binds it at binding 25 and multiplies the specular lobe by
  `1 + f0 (1 / E - 1)`. Fdez-Agüera's closed form, so the same table serves the
  specular-IBL rung above unchanged. [44-lighting.md](44-lighting.md)'s rung 1.
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

| Stage          | Industry            | Here                                      |
| -------------- | ------------------- | ----------------------------------------- |
| Auto-exposure  | histogram, GPU      | **built 2026-08-29**, histogram and roll  |
| Tonemap curve  | ACES or AgX         | **built 2026-08-27**, ACES; clamp default |
| Bloom          | Karis-average chain | **built**, off unless a view asks         |
| Colour grading | 3D LUT              | **missing**                               |
| Depth of field | gather or scatter   | **missing**                               |
| Motion blur    | per-object          | **missing**, blocked — §9                 |
| Lens artefacts | CA, vignette, grain | **missing**                               |
| HDR display    | scRGB or HDR10      | **missing** — sRGB swapchain only         |

**The tonemap curve was the one to take first and it was nearly free.** Taken
2026-08-27: `shaders/tonemap.slang` carries Stephen Hill's ACES fit behind a
selector in its block, and [48-post-processing.md](48-post-processing.md)
records why the clamp is still what a view gets unless it asks — and why the
curve is ACES rather than AgX, which needs transcendentals this workspace's
goldens cannot absorb. What is left of this row is deciding which stacks default
to the curve, which is a re-bless rather than a design.

**Auto-exposure was second and it was taken 2026-08-29.**
`shaders/exposure.slang` is three compute entry points — a clear, a histogram of
the finished frame's luminance, and a serial reduce over the bins — and
`crcbl_shaders::exposure` is the same arithmetic on the CPU that `crcbl`'s
`mesh_e2e` checks the bins against. Nothing is read back: the reduce writes one
float into a device-local buffer the tonemap binds, and the frame that was
measured is the frame that applies it.

The binning is integer arithmetic on the exponent field rather than a `log2`,
for §5's reason — the exponent of an IEEE-754 float _is_ the floor of its base-2
logarithm, so the bins are identical on all four backends where the
transcendental would not be.

**Adaptation followed the same day.** The reduce no longer writes what it
measured; it writes a step toward it from what the frame before was exposed by,
which is the slot behind it in the same ring —
`crcbl_render::ExposureAdaptation` is what a view hands in, with its own frame
delta, and the two rates differ because a real eye adapts down faster than it
adapts back up. The step is linear rather than the `1 - exp(-rate * delta)`
every engine writes, for §5's reason again: an exposure multiplies every texel
of the frame, so no transcendental may reach it. A view that asks for nothing
gets a blend of one, which is the target itself and the picture the pass drew
before adaptation existed.

What is left of the row is **the histogram's cost**: the pass takes one global
atomic per texel with no workgroup-local tile in front of it, and nothing in
this tree has profiled it — `docs/backlog.md` carries what that would take.

## 7. Upscaling and render scale

**What a current engine ships:** an internal render resolution decoupled from
the window, and a temporal upscaler — DLSS, FSR 3, XeSS, or Unreal's TSR.

**Where this one is:** the spatial half is **built (2026-08-27)** and the
temporal half is blocked. `ForwardRenderer::set_render_scale` sizes an internal
target at a fraction of the caller's extent — down to `MIN_RENDER_SCALE`, a
quarter in each dimension — and `shaders/upscale.slang` reconstructs it into the
caller's own target as the last pass of the frame, after the tonemap and after
FXAA. Every stage of [48-post-processing.md](48-post-processing.md)'s chain now
genuinely runs at the internal extent, which is the whole point of the ordering
that document had been asserting for a pass that did not exist. At full scale
there is no second image and no pass: the earlier stage writes the caller's
target directly, the same additive-zero shape the FXAA rung landed in.

**A player can now ask for it**, as of 2026-08-28: `[engine.video] render_scale`
is read by `crcbl::settings::video` into a `VideoSettings` beside the effect
bits, surfaced as `GpuContext::render_scale`, and handed to the renderer by
`apps/viewer`. It obeys the same clamp-downward rule the effect keys do — an
absent key is `1.0`, which is the whole extent and no pass — and the reader
clamps to the renderer's own `MIN_RENDER_SCALE..=1.0` so a typed-in extra digit
cannot ask for a target larger than the surface. The writer arrived the same day
— `crcbl::settings::set_render_scale` beside the reader, saved through
`SettingsSource::save` — and `apps/options` carries it, since 2026-08-29, on a
`RENDER SCALE` groove that runs linearly from `MIN_RENDER_SCALE` to the whole
extent, wearing `NEXT_START_MARK` because that screen draws no scene.

**The filter is Catmull-Rom**, sixteen taps, and it is Mitchell-Netravali at
`B = 0, C = 0.5` — interpolating, so a texel that survives the scale reaches the
frame unchanged, and a partition of unity by exact identity rather than by
tolerance. Its outer lobes are negative, which is what buys the acutance back
and is also why the reconstructed frame carries _more_ neighbour-to-neighbour
difference than the full-resolution render on a scene whose detail is one hard
silhouette. Multiplies and adds only, so no transcendental reaches a colour and
the pass can be blessed on all four backends.

**Bilinear was the alternative and is the worse one at the same cost class**: a
blit is one tap against sixteen, but a settings menu's resolution slider is
judged entirely on how the frame looks at 0.5, and bilinear at 0.5 is visibly
mushy where Catmull-Rom is merely soft. The tap count is on an image, not a
scene — it does not scale with anything the game does.

A _temporal_ upscaler is §9's blocker again and is not a near-term row. What
this rung deliberately does **not** do is jitter, accumulate, or ask for a
history buffer; it is a spatial reconstruction of one frame, and swapping it for
FSR 3 or TSR later replaces the pass without moving the seam around it.

## 8. Sky, atmosphere and environment

**What a current engine ships:** a sky pass — at minimum a cubemap or a
gradient, usually a physically-based atmosphere — that also feeds the ambient
and specular IBL terms.

**Where this one is:** closed at the gradient rung. The sky lights the scene, is
what a missed reflection falls back to, and is drawn behind the frame.

**Built 2026-08-27:** `crcbl_shaders::sky::SkyGradient` — zenith, horizon and
ground blended by a smoothstep in the direction's `y` — with `radiance` for what
a ray leaving the scene sees and `irradiance` for the same field as an L1
`GpuProbe`, which is the record `mesh.slang` already unpacks for probes. The
projection is closed form: azimuthal symmetry collapses the sphere integral to
two moments of the blend, and the horizontal bands are zero.

**The blend is a cubic and not a `pow`, deliberately.** A hand-tuned sky usually
tightens its horizon band with an exponent, and §4's rule forbids a
transcendental that reaches a colour — a sky being nothing but colour. A
smoothstep is multiplies and adds, so this rung needed neither
`crcbl_shaders::fog`'s construction nor `dfg`'s cooked table. A gradient wanting
a tighter horizon than a cubic gives spends a colour band on it.

**The first consumer landed the same day.** `ForwardRenderer::set_sky` carries a
`Sky`, three rows at the end of the frame block carry its projection, and
`mesh.slang`'s `sky_irradiance` adds it to the ambient sum beside
`probe_irradiance` — added, not chosen between, because both are the same term.
`Sky::NONE` projects to every coefficient zero, so the rung arrived switched off
and no golden moved.

**The second consumer landed the same day.** `ssr.slang` adds the sky along the
reflected ray to the probe environment a missed march falls back to, out of
three rows at the end of `SsrParams` — `sky_radiance` at first, and since
2026-08-29 `sky_prefiltered`, the same rows weighted by
`crcbl_shaders::sky_prefilter`'s table at the surface's roughness. Those rows
carry the **gradient** and not its L1 projection, which is the one place the two
blocks disagree on purpose: an ambient term wants the environment's
cosine-weighted integral and L1 _is_ that integral, while a reflection wants
radiance along one direction, and rebuilding that from four coefficients would
blur a gradient the pass can evaluate exactly.

So §5's half of this rung is closed — the environment SSR falls back to is no
longer the probe grid alone.

**The third consumer landed 2026-08-27 too, and it is the half that shows.**
`crcbl_render::sky_pass` draws `sky.slang`'s full-screen triangle at the
reversed-Z far plane against the depth the forward pass stored, tested
`GreaterOrEqual` with writes off — so the hardware that rejected the hidden
fragments is what selects the background, and the pass binds no depth texture,
no sampler and has no `discard`. It carries the gradient rather than the
projection, on `SsrParams`' reasoning. A frame whose sky is `Sky::NONE` adds no
pass at all, which is why the rung landed without moving a golden.

**What is left here** is everything above a gradient: a cubemap or a
physically-based atmosphere, and the specular IBL term §5 wants. Those are their
own rungs and are not blocked by anything this one left behind.

This matters more than it sounds because of §5: **the environment term SSR falls
back to and the ambient term a metal needs are the same term a sky would
provide.** A gradient sky and an irradiance/radiance pair generated from it
would close part of §5 and all of §8 at once, which is why it is grouped here
rather than filed as scenery.

## 9. The one blocker that gates five features

**Motion vectors, and the previous-frame transform they need.** The transform
half was **built 2026-08-27**: `crcbl_shaders::mesh::GpuInstance` now carries
`previous_transform` beside `transform`, `INSTANCE_STRIDE` is 160, and
`crcbl_render::InstancePool` fills it — a rewrite carries where the instance
came from, an insert is at rest because a spawn did not travel from anywhere,
and a rotate puts back at rest whatever moved a frame ago and has not moved
again. So every instance can now say where it was last frame.

What is still missing is the **pass**: a motion-vector target, the subtraction
that writes it, and the previous frame's view-projection in the frame block.
Until those exist the five features below are still blocked — but on a pass each
of them needs anyway rather than on a format nothing could change cheaply.

That absence blocks, in the order they would be wanted:

1. **TAA** — [49-antialiasing.md](49-antialiasing.md)'s ladder names it exactly.
2. **Temporal SSR**, which is what makes a rough reflection quiet.
3. **Temporal upscaling** — every one of DLSS, FSR 3, XeSS and TSR takes motion
   vectors as a required input. There is no non-temporal upscaler worth shipping
   above a plain blit.
4. **Per-object motion blur.**
5. **SSGI's accumulation**, per §5.

**The cheap insurance was taken on 2026-08-27**, which is what this section
recommended most strongly and for the reason it gave: widening `GpuInstance` is
cheap while four shader copies declare it and expensive once more shaders index
past `INSTANCE_STRIDE` — the same argument `GpuInstance::sector` is already in
the record on. The record grew by 64 bytes an instance and no frame moved, which
is the shape a reservation is supposed to have.

The field is **populated rather than reserved**, and deliberately: a slot
holding whatever the slot held last is a slot whose first reader debugs the
pool. The pool owns it the way it owns the liveness bit, so no caller can leave
it stale.

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
- **A second material model** — one BRDF, and every path shades with it. That
  refuses each of anisotropic GGX, clearcoat, sheen and subsurface by name:
  every one is a real term in a modern material system and every one is a second
  lobe, so the first of them arrives with a `MATERIAL_STRIDE` widening and can
  bring the rest. [44-lighting.md](44-lighting.md) prices them.
- **Parallax occlusion mapping** — a per-pixel march with a dependent texture
  read, for an effect normal mapping already approximates. It is a rung above
  normal maps rather than beside them, and there is no normal map yet.

## Delivery

Ordered by benefit per unit of work, which is not the order of the sections
above.

| Rung                                                                              | Why here                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~Reserve the previous-transform slot in `GpuInstance`~~                          | §9 — **built 2026-08-27**: the field, the stride, four shader copies and the pool that fills it; no frame moved                                                                                                                                                                                                                                                                                                                  |
| ~~Exponential height fog~~                                                        | §4 — **built 2026-08-27**: `crcbl_shaders::fog` answers the `exp` question, two rows of the frame block carry it, `set_fog` switches it on                                                                                                                                                                                                                                                                                       |
| ~~Multi-scatter energy compensation~~                                             | §5 — **built 2026-08-27**, both halves: the cooked table and the multiply on the lobe                                                                                                                                                                                                                                                                                                                                            |
| ~~The `anisotropic_filtering` row~~                                               | §2's filtering subsection — **built 2026-08-29**: the chain, the trilinear sampler, the device's anisotropy, `set_anisotropy`, the settings key and `apps/options`'s `ANISOTROPY` row; the WebGPU ceiling decision is `docs/backlog.md`'s                                                                                                                                                                                        |
| **Normal maps: tangent, page, sampling**                                          | §2 — the largest visual gap, and the rest of the material set follows the same road                                                                                                                                                                                                                                                                                                                                              |
| **Emissive page** (the factor shipped 2026-08-27)                                 | §2 — rides the second texture page rung                                                                                                                                                                                                                                                                                                                                                                                          |
| **Alpha-mask materials**                                                          | §3 — a `discard`, no sorting, and it is what foliage wants                                                                                                                                                                                                                                                                                                                                                                       |
| ~~Render scale and a blit~~                                                       | §7 — **built 2026-08-27**, Catmull-Rom, one pass                                                                                                                                                                                                                                                                                                                                                                                 |
| ~~A gradient sky feeding ambient and the SSR fallback~~                           | §8 — **built 2026-08-27**: the gradient, its L1 projection, the ambient it feeds, the environment a missed reflection falls back to, and the depth-tested pass that draws it behind the frame                                                                                                                                                                                                                                    |
| ~~The sun's shaft through the froxel column~~                                     | §4 — **built 2026-08-28**: the buffer, the scatter, the scan, the composite, the sun's phase-scattered radiance and the cascade lookup that occludes it — [51-volumetrics.md](51-volumetrics.md) rungs 1a through 1b-ii                                                                                                                                                                                                          |
| ~~Auto-exposure~~                                                                 | §6 — **built 2026-08-29**: the histogram, the reduce, the buffer the tonemap reads, the `AUTO_EXPOSURE` bit a view asks with, and the temporal roll between one frame's exposure and the next                                                                                                                                                                                                                                    |
| **Blended transparency with GPU-sorted keys**                                     | §3 — the first rung here that touches the frame's structure                                                                                                                                                                                                                                                                                                                                                                      |
| ~~Specular IBL: prefiltered radiance and a BRDF LUT~~                             | §5 — **built 2026-08-29**, both halves: `crcbl_shaders::sky_prefilter` is the gradient convolved against the lobe as one committed table, `ssr.slang`'s miss fallback reads the sky through it at the surface's roughness, and `crcbl_shaders::dfg`'s pair scales the environment by `f0 · scale + bias` in place of Schlick — mirrors unchanged, rough metals darker at grazing angles by the energy one direction over-counted |
| **Specular antialiasing (roughness regularisation)**                              | §2's normal maps first — the aliasing it removes is the one no AA rung can                                                                                                                                                                                                                                                                                                                                                       |
| **Block-compressed pages: KTX2 and Basis at the bake, BC7/BC5/BC4 on the device** | §2's filtering subsection — a bake-tool rung, gated on an encoder the workspace does not have and the user has not chosen                                                                                                                                                                                                                                                                                                        |
| Colour grading, DOF, lens artefacts                                               | §6 — polish, after the curve exists to grade against                                                                                                                                                                                                                                                                                                                                                                             |
| SSGI, temporal SSR, TAA, temporal upscaling                                       | §9's slot first; each is its own rung after it                                                                                                                                                                                                                                                                                                                                                                                   |
