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

| Area                    | Here                                                   | Owner                                                                            |
| ----------------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------- |
| Geometry and visibility | **ahead**                                              | [03-gpu-driven-rendering.md](03-gpu-driven-rendering.md), [25-lod.md](25-lod.md) |
| Shadows                 | behind, ladder written                                 | [45-shadows.md](45-shadows.md)                                                   |
| Ambient occlusion       | behind, ladder written                                 | [46-ambient-occlusion.md](46-ambient-occlusion.md)                               |
| Reflections             | comparable for screen space                            | [47-reflections.md](47-reflections.md)                                           |
| Antialiasing            | behind, ladder written                                 | [49-antialiasing.md](49-antialiasing.md)                                         |
| Irradiance probes       | visibility maps and the clipmap built, updater owed    | [50-irradiance-probes.md](50-irradiance-probes.md)                               |
| **Materials**           | **far behind**, ladder in §2                           | [37-materials.md](37-materials.md), and §2 below                                 |
| **Texture filtering**   | **a chain, trilinear, 8× anisotropic, uncompressed**   | §2's filtering subsection                                                        |
| **Transparency**        | **absent**, argued                                     | §3 below                                                                         |
| **Volumetrics**         | height fog and a froxel column                         | [51-volumetrics.md](51-volumetrics.md), and §4 below                             |
| Global illumination     | behind                                                 | §5 below                                                                         |
| Post-processing         | behind                                                 | [48-post-processing.md](48-post-processing.md), §6                               |
| Upscaling               | spatial half built, temporal its own rung              | [15-windowing.md](15-windowing.md), §7                                           |
| Decals                  | absent, planned                                        | [33-decals.md](33-decals.md)                                                     |
| Particles               | simulated and drawn as instances, no pass of their own | [20-particles.md](20-particles.md)                                               |
| Sky and atmosphere      | a gradient, no atmosphere                              | §8 below                                                                         |

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

**Where this one was**, read out of `crcbl_shaders::mesh::GpuMaterial` on
2026-08-27: `base_color`, `base_color_texture`, `metallic`, `roughness`,
`emissive`, `tiling`, `tile_metres`. One texture, and it was the base colour.
`crcbl_shaders::mesh::MeshVertex` was position, normal, colour and one UV — **no
tangent**. `emissive` was a factor with no page behind it, and there was no
alpha mode of any kind.

So: **no normal mapping.** That was the single largest visual gap in this
renderer, and it was larger than every rung of the AO, shadow and AA ladders put
together — a surface could only be as detailed as its triangles, which is why
the samples all read as flat-shaded greybox no matter how good the lighting on
them got.

**Rung 1 and the normal half of rung 2 landed 2026-08-30** and the gap is closed
to that depth: the tangent frame is real, the normal page is bound, and a glTF
material's `normalTexture` reaches the fragment stage. What is still missing
from the list above is the rest of the texture set — the packed
metallic-roughness-occlusion page, the emissive page and the alpha modes, whose
columns in the row exist and are read by nothing — and the "Landed" paragraph
below says exactly what shipped.

**What it would take**, in the order the dependencies fall:

1. **A tangent frame.** Either a tangent in `MeshVertex` — the layout decided
   below carries it as a QTangent, which glTF feeds directly — or screen-space
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

**Decided 2026-08-30: the widening is spent once, into the layout below.** There
is no `.crcblmesh` on disk to migrate — zero tracked files — so the format is
born v0 under [06-assets-scenes.md](06-assets-scenes.md)'s pre-1.0 rule and the
goldens re-bless once. It is the first row of the foundations block in this
document's delivery table, because every rung above it and the position-only
prepass are cheaper on it than on a wider copy of today's vertex.

| Stream         | Member         | Encoding                                                                                     | Bytes |
| -------------- | -------------- | -------------------------------------------------------------------------------------------- | ----- |
| 0 (position)   | `position`     | `float3`                                                                                     | 12    |
| 1 (attributes) | `qtangent`     | `snorm16x4` — the tangent frame as a quaternion, sign in `w`                                 | 8     |
| 1              | `uv0`          | `unorm16x2`, a per-mesh scale and offset in the mesh table row                               | 4     |
| 1              | `uv1`          | `unorm16x2`, the same way                                                                    | 4     |
| 1              | `color`        | `rgba8`                                                                                      | 4     |
| `GpuMaterial`  | four page rows | base colour, normal, metallic-roughness-occlusion, emissive, plus the alpha cutoff and flags | 64    |

**Sixty-four holds four page rows only because two share a word.** Written as
four separate `uint`s beside the factors, the alpha cutoff, the flags and glTF's
`normalTexture.scale`, the row wants eighteen words — seventy-two, which
`std430` rounds to eighty. So the four layer indices ride two per word, sixteen
bits each: `color_normal_pages` and `mro_emissive_pages` on the wire,
`base_color_texture`, `normal_texture`, `metallic_roughness_occlusion_texture`
and `emissive_texture` as ordinary fields on the host, with
`crcbl_shaders::mesh::MAX_PAGE_LAYER` the bound. A page layer index is limited
by the device's maximum array layers, which no target here reports above a few
thousand, so the sixteen bits are not a limit anything reaches — and bit-packing
a small integer is what the vertex stream above is made of.

Stream 0 is what the depth prepass and every shadow pass will fetch — twelve
bytes a vertex where they decode the whole record today, and there are as many
of those passes a frame as there are cascades and lit tiles. Stream 1 is the
forward pass's, twenty bytes against sixty-four. The QTangent replaces the
normal _and_ the tangent: a normalised quaternion recovers both, its sign
carries glTF's handedness, and a mesh that ships no tangent takes the derivative
frame as its fallback until the importer fills one (which is the MikkTSpace
call, `docs/backlog.md`'s).

**Landed 2026-08-30.** `crcbl_shaders::vertex` carries the arithmetic —
`QTangent`, `UvRange`, `orthonormal_basis`, the `rgba8` pair — and
`crcbl_shaders::mesh::MeshVertex` is the two streams, built only through
`from_normal` and `from_frame`. Three things were decided in the building:

- The attribute stream is twenty bytes, not the thirty-two an earlier draft
  quoted beside the table — the rows sum to twenty, and the arithmetic won.
- The two streams are two regions of one storage buffer rather than two
  bindings: the raster path's vertex stage already binds
  `PORTABLE_STORAGE_BUFFERS_PER_STAGE` storage buffers, so a ninth would be a
  renderer no browser can build. The boundary is the pool's vertex capacity in
  words and travels in `FrameUniforms::vertex_pool.x`, and in
  `skinning::Params::attribute_base` for the pass that binds no frame block.
- The `UvRange` rides in the mesh table row (`GpuMesh::uv_range`), not in the
  draw constants: both geometry paths already fetch that row, and a cluster DAG
  needs one range for every level because the mesh path resolves level 0's.

The three shader copies (`mesh`, `mesh_cluster`, `skinning`) decode it with one
block of arithmetic that `every_shader_decodes_a_vertex_the_same_way` holds
equal, and no golden moved. The `TANGENT` reader that layout was waiting for
landed with rungs 1 and 2 below, and the depth-only entry point it was waiting
for landed 2026-08-31: `depthVertexMain` reads stream 0 and writes `SV_Position`
alone, and `crcbl_render::forward`'s depth pipeline — the shadow cascades' and
the depth prepass's — names it. What it still owes — a writer for `uv1`, and the
same split for the mesh-shader path's geometry stage — is `docs/backlog.md`'s.

**The page allocator, generalised (the foundations block's fourth row).** One
array-image allocator — `crcbl_render::scene::PageDesc` generalised, with a
per-row index and rectangle — serves what the base-colour page serves today and
everything that follows the same road: the normal, metallic-roughness-occlusion
and emissive pages of this section, and the decal atlases. Four page indices in
the material row is what the 64-byte `GpuMaterial` above is sized for.

**DECIDED 2026-08-31, when the shadow atlas's rectangle was built: the shadow
atlas is not a caller of this.** It has its own allocator,
`crcbl_render::shadow::AtlasAllocator`, and the two have nothing in common but
the word. `PageDesc` describes _layers_ of an array image, whole and uploaded
once at scene build, indexed by layer number; the shadow atlas allocates and
frees _rectangles_ of one image every frame, at four sizes, with a quadtree and
a merge. Sharing them would mean writing the rectangle allocator, giving it one
caller, and then bending `PageDesc` — which has no allocation, no free and no
rectangle today — around it: an abstraction with one real user, which this
repo's own rule refuses. Row (d) is therefore about the pages and the decal
atlases; it is not blocked on the shadow rung and the shadow rung is not blocked
on it. If decals arrive and want rectangles too, that is the second caller and
the moment to extract one.

**Normal maps landed 2026-08-30 — rung 1, and rung 2's normal page.**
`crcbl_scene`'s importer reads the `TANGENT` accessor and glTF's `normalTexture`
(index, `texCoord` 0 and `scale`); a primitive that shipped tangents is marked
with `GpuMesh::MESH_AUTHORED_TANGENTS` and everything the engine authors itself
is not. The fragment stage picks a frame from that one bit — the mesh's own
frame carried through the vertex stage, or Schüler's screen-space frame out of
`ddx`/`ddy` — and perturbs the shading normal through it, and the result is what
lighting, SSR and the `Normals` view read. `GpuMaterial` is 64 bytes with all
four page columns; only the normal one is wired. Four things were decided in the
building:

- **The screen-space frame applies the determinant's sign rather than inheriting
  it.** The textbook cotangent frame comes out scaled by the UV Jacobian's
  determinant, and that determinant is negative both for a mirrored
  parameterisation _and_ for a target whose screen-space `y` runs the other way
  — which radv's does, as `geometric_normal_of` already measured. A frame that
  kept the sign would light a normal map from the wrong side on one backend and
  the right side on another. Applying it makes the frame always right-handed
  about the normal, which is the same statement as rung 1's: this route cannot
  recover handedness, and a mirrored shell needs the authored tangent.
- **A material with no normal texture gets the interpolated normal back
  exactly**, by testing the layer index in the shader rather than by trusting
  layer 0's neutral texel. Eight bits cannot encode the identity: `0x80` decodes
  to `1 / 255` off flat, which is enough to move every golden in the tree drawn
  without a normal map. None moved.
- **The normal page gets its own mip filter.** `crcbl_render::mip::normal_chain`
  averages the decoded vectors with no transfer curve and no alpha weight and
  renormalises, and copies a cell that covers exactly one source texel byte for
  byte rather than round-tripping it — a decode, renormalise and re-encode moves
  nearly every texel of a real map, because most eight-bit normals are not unit
  vectors.
- **The four page indices are packed two per word.** Eighteen plain words is
  seventy-two bytes, which `std430` rounds to eighty; sixteen bits each is
  sixteen words exactly. See the paragraph above the layout table.
- **The browser decides the shape of the fragment stage.** Both of
  `shading_normal_of`'s branches are uniform across a quad in fact — the
  material row and the frame word are both `nointerpolation` — and WGSL's
  uniformity analysis can prove neither, because both trace back to the
  instance's material index. So the derivatives are taken above the early return
  rather than after it, and the page is read with `SampleGrad` and those
  derivatives rather than with an implicit-LOD `Sample`. Same level, same
  filtering, and the module parses; SPIR-V, MSL and DXIL accepted every
  intermediate form of it without a word, and only the browser gate ever said
  otherwise. `fxaa.slang`'s `tap` had already paid for this lesson once.

**What it costs, on the three tiers.** Measured on the raster tier:
`apps/viewer` headless on the shelf's SciFiHelmet (2048² pages, authored
tangents) at 1920×1080 for 900 frames on an RX 7900 XTX under radv, the forward
pass is **0.061 ms p50 / 0.064 ms p95** with the page sampled and **0.055 /
0.057 ms** with `shading_normal_of` short-circuited to the interpolated normal —
about **0.006 ms, a tenth of that pass** and two and a bit per cent of the
frame's 0.254 ms of GPU work. That is one anisotropic fetch from an `Rgba8Unorm`
array image plus a normalise and a 3×3 per fragment. The derivatives the browser
made unconditional are in both of those figures; a third run that returns before
taking them at all reads **0.056 / 0.058 ms**, which is _above_ the run that
takes them, so the four derivative operations on every fragment in the frame
cost less than this measurement can separate from run-to-run noise. An unmarked
mesh pays those same derivatives plus the frame's arithmetic and no fetch, which
no scene in this tree exercises at a size worth measuring and which is
**estimated** to land under the sampled figure, since it trades the fetch's
latency for ALU already counted. The page's memory is exactly the base-colour
page's — one square extent shared by both, `layers × extent² × 4` bytes and a
third again for the chain, so 21 MiB a layer at 2048² — and layer 0 is a full
layer of one repeated texel, which `docs/backlog.md` has. **The browser tier
pays the same arithmetic and more bytes**: WebGPU has `dpdx`/`dpdy` and a
seventeenth sampled-texture binding is inside every portability floor this
engine holds to, but the page is uncompressed `RGBA8` until the KTX2/BC5 rung
lands, where a desktop would spend one byte a texel instead of four — which is
why the compression rung is the one the material ladder waits on. Estimated; no
browser measurement was taken. **The ray-tracing tier cannot use half of this**:
a hit shader has no quad and therefore no derivatives, so a traced hit can only
be shaded through an authored tangent frame, and an unmarked mesh reached by a
ray has no normal mapping at all. That is an argument for MikkTSpace ahead of
§5's rung, and it is in the backlog with the rest.

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
that slot's own `begin_frame` — and the player has the say through
`crcbl::settings`' `ANISOTROPIC_FILTERING_KEY`, which `apps/options`' menu
writes from `ANISOTROPIES`. What is still missing is compression: `Format` holds
BC1 through BC7 behind `Features::TEXTURE_COMPRESSION_BC` and nothing in the
renderer asks.

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
with a `BlendState` at all. Every blended pipeline in the render crate
composites rather than shades: `crcbl_render::sprite_pass` and
`crcbl_render::ui_pass` in straight and premultiplied alpha,
`crcbl_render::debug_draw` in straight alpha, `crcbl_render::grid`
premultiplied, and `crcbl_render::bloom`'s upsample additive because the add has
to be the blender's. So the engine can draw a translucent sprite and cannot draw
a translucent _surface_.

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
the surfaces are shadowed by, and every point and spot light glowing in it
through the froxel's own cluster list since 2026-08-29. Rungs 1 and 2 of
[51-volumetrics.md](51-volumetrics.md) are closed. What is open above them is a
3D target with temporal reprojection, and a density field — neither scheduled,
and each argued there.

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

`crcbl_shaders::volumetric` is `phase` — Henyey-Greenstein, the angular half
that makes fog glow around a light rather than uniformly — and
`integrate_slice`, which is what one slice of a froxel column owes the
composite: the radiance it adds and the fraction it transmits, in one closed
form.

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

What is left of §4 is [51-volumetrics.md](51-volumetrics.md)'s rungs 3 and 4: a
3D target with temporal reprojection, and a density field.

## 5. Global illumination

**What a current engine ships:** at minimum a probe-based irradiance volume with
runtime updates (Unity's APV, Godot's SDFGI, Unreal's Lumen), plus screen-space
GI to catch what the probes are too coarse for.

**Where this one is:** L1 spherical-harmonic irradiance probes — the row layout
in `crcbl_shaders::probe`, the read in `mesh.slang`'s `probe_irradiance` and
`ssr.slang`'s `probe_level_environment`, and the visibility maps captured by
`probe_capture.slang` and `probe_octahedral.slang` — decoded per pixel, and a
flat `frame.ambient` term underneath them. (This named `shaders/probe.slang` and
`compute_probe.slang` until 2026-09-04. The first has never existed; the second
is the HAL's compute-capability probe, which draws nothing and has no bearing on
irradiance at all.) That is a real irradiance volume and it is the right first
rung.

**The gaps, in order of how much they cost:**

- **The split-sum escape, and why the determinism rule does not block it.**
  Worth stating because that rule blocks so much else on this page: Karis's
  split-sum needs a `DFG` table over `(N·V, roughness)` and the environment
  prefiltered against the lobe at every roughness; both are **baked at build
  time and committed like a shader artifact**, so the run-time cost is a
  multiply and two fetches and four backends read the same bytes. Both tables
  are cooked: `crcbl_shaders::dfg` writes `tables/dfg.bin`, and the radiance
  half is **not** the mip chain this bullet used to name — a gradient sky is
  linear in its three colours, so `crcbl_shaders::sky_prefilter` reduces the
  whole prefilter to a 64-square two-channel table in `tables/sky_prefilter.bin`
  with the sky's colours left as run-time parameters. What that rung still owes
  is the image upload and the shader's specular ambient term. Baking a
  transcendental into a table is the general escape, and
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
  accumulation SSGI needs to be quiet — see §9.
- **No lightmaps, no baked GI, and no GI at all below the ray-tracing tier — the
  user's rules of 2026-08-30**: the sun and every scene light are dynamic,
  nothing bakes a lighting result, and a device without hardware ray tracing
  (every browser, lavapipe) runs the raster stack. **Amended the same day**:
  that tier carries one bounce after all —
  [50-irradiance-probes.md](50-irradiance-probes.md)'s visibility-gated probe
  volume, updated every frame from the sun's reflective shadow map and weighted
  at shading by a per-probe depth map so it does not leak. On `crcbl-vk`,
  `crcbl-dx12` and `crcbl-mtl` the same `GpuProbe` rows are filled by inline ray
  queries in compute instead — fixed pattern, no history on either tier.

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

**SSGI is withdrawn from that ordering, 2026-08-30.** It was the non-RT tier's
only bounce; [50-irradiance-probes.md](50-irradiance-probes.md)'s rebuilt probe
volume is now a leak-free bounce on every tier, and a screen-space term on top
of it would add a view-dependent, off-screen-blind estimate of the same quantity
for a pass of its own. Contact shadows **shipped 2026-09-01** and are off this
queue — `crcbl_render::contact_shadows`, off by default; the ranking argument
that put them first is kept above as the reasoning, not as a plan. The cone
trace and the SDF path stay where they were as the RT tier's raster
alternatives, not scheduled.

## 6. Post-processing

Pipeline as it stands. The post chain proper is scene → bloom → exposure and
tonemap → antialiasing resolve → render-scale upscale → UI, and the frame in
front of it has grown a depth prepass, the background and sky, the occlusion
chain (`ssao`, `ssao-blur`, `ssao-blur-2`, `ssao-upsample`), the contact shadow
march, the froxel volume, the Hi-Z pyramid the reflection march climbs with
`ssr` and `ssr-blur` behind it, and debug draw. The ground grid is **not** in
front of the post chain: it draws directly after the tonemap, which is where
`crcbl_render::forward`'s own pass-list test places it.
`crcbl_render::forward`'s section comments are the list and
`crcbl::screenshot`'s expected pass list is what goes red when it moves; this
sentence used to enumerate five passes and claim to be verified, which is how it
went three passes stale without anyone noticing.

| Stage          | Industry            | Here                                      |
| -------------- | ------------------- | ----------------------------------------- |
| Auto-exposure  | histogram, GPU      | **built 2026-08-29**, histogram and roll  |
| Tonemap curve  | ACES or AgX         | **built 2026-08-27**, ACES; clamp default |
| Bloom          | Karis-average chain | **built**, off unless a view asks         |
| Colour grading | 3D LUT              | **missing**                               |
| Depth of field | gather or scatter   | **missing**                               |
| Motion blur    | per-object          | **missing** — its own rung, §9            |
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

A _temporal_ upscaler is one of §9's own rungs and is not a near-term row. What
this rung deliberately does **not** do is jitter, accumulate, or ask for a
history buffer; it is a spatial reconstruction of one frame, and swapping it for
FSR 3 or TSR later replaces the pass without moving the seam around it.

## 8. Sky, atmosphere and environment

**What a current engine ships:** a sky pass — at minimum a cubemap or a
gradient, usually a physically-based atmosphere — that also feeds the ambient
and specular IBL terms.

**Where this one is:** closed at the gradient rung. The sky lights the scene, is
what a missed reflection falls back to, and is drawn behind the frame.

The gradient is `crcbl_shaders::sky::SkyGradient` — zenith, horizon and ground
blended by a smoothstep in the direction's `y` — with `radiance` for what a ray
leaving the scene sees and `irradiance` for the same field as an L1 `GpuProbe`,
which is the record `mesh.slang` already unpacks for probes. The projection is
closed form: azimuthal symmetry collapses the sphere integral to two moments of
the blend, and the horizontal bands are zero.

**The blend is a cubic and not a `pow`, deliberately.** A hand-tuned sky usually
tightens its horizon band with an exponent, and §4's rule forbids a
transcendental that reaches a colour — a sky being nothing but colour. A
smoothstep is multiplies and adds, so this rung needed neither
`crcbl_shaders::fog`'s construction nor `dfg`'s cooked table. A gradient wanting
a tighter horizon than a cubic gives spends a colour band on it.

**A consumer takes the gradient, not its L1 projection**, which is the one place
the two blocks disagree on purpose: an ambient term wants the environment's
cosine-weighted integral and L1 _is_ that integral, while a reflection wants
radiance along one direction, and rebuilding that from four coefficients would
blur a gradient the pass can evaluate exactly. The sky pass reads it on the same
terms: `sky.slang`'s full-screen triangle draws at the reversed-Z far plane
against the depth the forward pass stored, tested `GreaterOrEqual` with writes
off — so the hardware that rejected the hidden fragments is what selects the
background, and the pass binds no depth texture, no sampler and has no
`discard`. A frame whose sky is `Sky::NONE` adds no pass at all.

**What is left here** is everything above a gradient: a cubemap or a
physically-based atmosphere. Those are their own rungs and are not blocked by
anything this one left behind.

This matters more than it sounds because of §5: **the environment term SSR falls
back to and the ambient term a metal needs are the same term a sky would
provide.** A gradient sky and an irradiance/radiance pair generated from it
would close part of §5 and all of §8 at once, which is why it is grouped here
rather than filed as scenery.

**DECIDED 2026-08-30 — the atmosphere is Hillaire's, and it replaces the
gradient as the default sky.** "A Scalable and Production Ready Sky and
Atmosphere Rendering Technique" (Hillaire, EGSR 2020): a transmittance LUT and a
multiple-scattering LUT that depend on the planet and not on the sun, cooked
once and committed like `dfg.bin`; and a small sky-view LUT that depends on the
sun's direction. **The sky-view LUT is computed on the host**, with the tree's
own `fog::exp_neg` construction rather than libm, and uploaded when the sun
moves — so §4's rule holds (no transcendental reaches a colour in a shader), the
result is bit-identical on every platform, and the per-frame GPU cost on every
tier is one sampled-image fetch for the sky and the L1 projection of the same
LUT for the ambient term, which is what makes time-of-day free. Recomputing the
LUT is amortised over frames when the sun moves continuously. `SkyGradient`
stays as the constructor for a scene that wants a flat sky and as the fixture
the goldens already carry; a scene that sets an atmosphere gets it instead. An
analytic fit (Preetham) was considered and declined: cheaper to compute, visibly
wrong at low sun, and it saves a LUT the tree already knows how to cook.

## 9. Motion vectors, and the five rungs that read them

**The convention, which a consumer must not have to guess**: texture-coordinate
space, current minus previous, `+y` down — so a history buffer is read at
`uv - motion`. It is written on `MOTION_FORMAT` and on
`TransientImageDesc::motion`, and `DebugView::Motion` is what makes it visible,
encoding the vector as `motion * MOTION_VIEW_SCALE + 0.5`.
`crates/crcbl/tests/mesh_e2e/motion.rs` is the observer: a still scene reads
rest, a moved instance reads its own motion while its neighbour reads rest, a
panning camera moves every covered pixel, and the frame after a move is back at
rest. `crates/crcbl/tests/mesh_e2e/skinned_motion.rs` is the second observer: a
cube whose palette carried it across the frame reads that displacement while its
instance transform never changed, which is
`crcbl_shaders::mesh::GpuInstance::previous_base_vertex` — the pool vertex the
frame before deformed it into — being read on the override arm.

A rigid body's motion, a deformed surface's and a skinned object's own travel
are all three in the target. **The one thing it does not carry** is
`docs/backlog.md`'s: the camera's motion where the sky shows through.

The five rungs that read it, each now blocked on its own work only, in the order
they would be wanted:

1. **TAA** — [49-antialiasing.md](49-antialiasing.md)'s ladder names it exactly.
2. **Temporal SSR**, which is what makes a rough reflection quiet.
3. **Temporal upscaling** — every one of DLSS, FSR 3, XeSS and TSR takes motion
   vectors as a required input. There is no non-temporal upscaler worth shipping
   above a plain blit.
4. **Per-object motion blur.**
5. **SSGI's accumulation**, per §5.

**Widening `GpuInstance` is cheap while four shader copies declare it and
expensive once more shaders index past `INSTANCE_STRIDE`** — the same argument
`GpuInstance::sector` is already in the record on, and the reason a slot this
section wanted was taken early rather than when its first reader arrived.

A slot of that kind is **populated rather than reserved**, and deliberately: a
slot holding whatever the slot held last is a slot whose first reader debugs the
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
  normal maps rather than beside them.

## Delivery

Ordered by benefit per unit of work, which is not the order of the sections
above.

**A rung is priced before it is called built (rule, 2026-08-30).** Every row
below that lands writes its cost into the plan that owns it: milliseconds per
pass on the desktop adapter, on lavapipe and in the browser, read off
`crcbl_render::PassStats` and the per-machine baseline
[40-profiling.md](40-profiling.md) schedules — not a sentence saying it is
cheap. The software and browser tiers pay for every pass at tens of times the
desktop cost, and they are the tiers every golden runs on, so the desktop number
alone is not a price. The same rule holds the forward pass to
[44-lighting.md](44-lighting.md)'s attachment budget.

**Every `forward` figure below has the pass's full-extent clears in it, and this
is what they cost (measured 2026-09-05).** The pass opens by clearing the scene
colour, the reflectivity and the motion targets over the whole extent, as
`LoadOp::Clear`s fused into its begin: putting a timestamp around them would
mean giving them a pass and a second full-target write of their own, so the
millisecond figures and the shares quoted for `forward` — here, in
[44-lighting.md](44-lighting.md) and in [47-reflections.md](47-reflections.md) —
include them. What separates them from the draw is a second configuration rather
than a second timestamp, and `crates/crcbl/tests/mesh_e2e/depth_only.rs`
measures one beside its field: the same extent, the same effect stack, an empty
draw list. At 640x480 over 48 recorded frames that floor is a `forward` p50 of
**0.009 ms on an RX 7900 XTX** and **0.258 ms on lavapipe** — medians of three
runs each, spread 0.009–0.010 and 0.256–0.268 — against the same runs' loaded
field at 0.135 ms and 29.255 ms. Subtracting the floor is what turns `forward`
into the draw's own cost, and no share quoted here or elsewhere has had it
subtracted.

| Rung                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Why here                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **THE LIGHTING ORDER, decided 2026-08-30** — the raster items interleave with the foundations rather than waiting behind them What is left of it: the atmosphere → CMAA2. Everything ahead of those has landed — (a) vertex v2, normal maps, LTC area lights and the fill flag, the shadow atlas allocator with (f) and (e) beside it, contact shadows, the AO tint and bent normals, and the probe volume's visibility, its clipmap and both RSM producers (2026-09-04). (b), (d) and (g) land where the first rung that needs them does; (c) when the RT tier's updater is next. The user's rule for the order: best-looking for the performance, cheapest real win first |
| **FOUNDATIONS BLOCK (a)–(g), 2026-08-30** — a foundation is scheduled before any feature rung that would be cheaper with it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | The seven rows below, in order; each is priced like any rung. (a) is this section's stride decision and gates the material ladder; the rest are the early-leverage items written into their own plans the same day                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| **(a) Vertex v2 — the layout and the 64-byte `GpuMaterial` landed 2026-08-30, the depth-only entry point 2026-08-31**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | §2 — `depthVertexMain` is what spends stream 0: the shadow atlas and the depth prepass fetch twelve bytes a vertex instead of the whole record plus a second position for the motion vector. Priced on both measurable tiers over a field of 144 dunes patches at 640x480 — the depth prepass costs 20.6/20.4 ms against the full stage's 22.9/24.2 ms on lavapipe, and 0.073-0.074 ms either way on an RX 7900 XTX, which is not vertex-fetch bound at that scale. The mesh-shader path's geometry stage still reads a whole vertex; the material row widened with the normal-map rung below, and three of its four page columns are laid out and unread                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **(b) The render stack as RON, per camera**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | [48-post-processing.md](48-post-processing.md) — the pass list a camera runs is data, so a demo, a test and a settings preset compose it without a build                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| **(c) Acceleration structures and inline ray queries on the seam** (vk, dx12, mtl)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | §5 — the GI's engine on the ray-tracing tier: a build and refit, a `Capability` the device reports, and the hit-shading reads. Decided 2026-08-30 in `docs/backlog.md`: GI is hardware ray tracing only; the other tiers run the raster stack                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| **(d) The page allocator, generalised**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | §2 — one array-image allocator behind base colour, the material pages and decals. The shadow atlas is not a caller: it got its own quadtree on 2026-08-31, and §2's paragraph says why                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| **(e) The debug draw layer — geometry built 2026-08-31, text is its own rung**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | [07-ui-debug.md](07-ui-debug.md) — lines, boxes, spheres and frusta the culling, LOD and atlas rungs are debugged through. `crcbl_render::debug_draw` is the immediate-mode buffer and the pass; it lands before the tonemap, in HDR, per [18-render-features.md](18-render-features.md)'s interaction rule, and a frame that appends nothing records no pass at all — every golden was left where it was blessed. Priced with 1024 boxes at 256x192 over 48 recorded frames: 0.030/0.031 ms on an RX 7900 XTX against the forward pass's 0.013/0.014, and 1.865/2.187 ms on lavapipe against its 0.134/0.159. **World-anchored text is not built** — it needs a glyph atlas and a rasteriser seam, `crcbl-ui` owns one, and `docs/backlog.md` carries what the slice needs                                                                                                                                                                                                                                                                                                                      |
| **(f) The shared importance and hysteresis helper — REFUSED as stated, 2026-08-31**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | [25-lod.md](25-lod.md) + [45-shadows.md](45-shadows.md) — one scorer for cluster selection and shadow-tile priority. Built as one scorer _per plan_, not one across the two, because the two are not one function: `crcbl_render::shadow::coverage` divides by the distance to the light's **centre**, an angular radius, while `GroupCost::projected_error` divides by the distance to the group's sphere's **surface** and answers infinity inside it, a worst-case error. One helper would need a flag choosing the denominator, which is the shape this workspace refuses. The hysteresis is further apart still: the LOD band is a boolean two-budget rule per (instance, group) with its state in a device-local buffer, and the shadow band is a multi-level ladder walked on the CPU per light. What is shared is the number — `LEVEL_HOLD_RATIO` is deliberately the same fifth `ForwardRenderer::lod_hold_ratio` opens, and each names the other. What the shadow side genuinely deduplicated is its own: `coverage` is the single number both the tile ranking and the tile size read |
| **(g) Quality presets low / medium / high — the seam landed 2026-08-31**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | [39-capabilities.md](39-capabilities.md) — every rung's rows behind three names, which is what the three-tier pricing rule reports against. `crcbl::settings::presets` is the module and `quality` is the console command: a tier is a **writer** of individual `[engine.video]` keys through `crcbl::settings::apply`, never a layer the resolution order consults, and its label is derived from what the readers answer rather than stored. What it covers is three keys — the render scale, the antialiasing tier and the fog switch — because those are the only rows of that document's tier table this tree has a key for; `medium` and `high` consequently write the same values, and `medium_and_high_hold_the_same_values_until_a_key_separates_them` is the tripwire for the day one of the shadow-atlas, probe-volume, SSR or ray-tracing rows grows one. Nothing selects a tier at start-up, so no golden moved. What each tier should spend on the knobs the table is silent about — the shadow cadence, the occlusion pair, anisotropy — is `docs/backlog.md`'s                   |
| **The viewer as the PBR showcase** — native drag-and-drop, a bundled shelf of Khronos CC0 models picked from a list, Suzanne on open ([sample/05-viewer.md](sample/05-viewer.md) milestone 4)                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | **The shelf and Suzanne-on-open shipped 2026-08-30** (`apps/viewer/src/shelf.rs`, nine models, Suzanne the default); drag-and-drop works on both hosts bar X11, which `sample/05-viewer.md` records. What is _not_ drawing is the full material set: base colour and the normal map are read, and `mesh.slang`'s `mro_emissive_pages` is laid out and sampled nowhere — the same gap row (a) of this table names                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| **LTC area lights and the fill flag — rectangles landed 2026-08-31**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | [44-lighting.md](44-lighting.md)'s rung 5 — `crcbl_shaders::ltc` and its cooked `tables/ltc.bin`, a `Light::Rect` in the rows and the froxel grid, and the polygon integral in `mesh.slang`. Priced there on both measurable tiers: a rectangle costs 3.7× a point light on radv and 2.3× on lavapipe with a full froxel of them at 1080p. What it left — sphere, tube and disc shapes, textured lights, and the browser frame — is `docs/backlog.md`'s (`fill` reached `PointLight` and `SpotLight` on 2026-09-02)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| **Emissive page** (the factor shipped 2026-08-27)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | §2 — rides the second texture page rung                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| **Alpha-mask materials**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | §3 — a `discard`, no sorting, and it is what foliage wants                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Blended transparency with GPU-sorted keys**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | §3 — the first rung here that touches the frame's structure                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| **Specular antialiasing (roughness regularisation)**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | §2's normal maps first — the aliasing it removes is the one no AA rung can                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| **Block-compressed pages: KTX2 and Basis at the bake, BC7/BC5/BC4 on the device**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | §2's filtering subsection — a bake-tool rung, gated on an encoder the workspace does not have and the user has not chosen. **It is the bandwidth rung** (2026-08-30): four to six times less texture traffic, which is what makes normal maps and the material pages affordable in the browser at all, so the encoder decision is what the material ladder above waits on                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Colour grading, DOF, lens artefacts                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | §6 — polish, after the curve exists to grade against                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| **CMAA2 in SMAA's place, then MSAA 2×/4×/8×**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | [49-antialiasing.md](49-antialiasing.md)'s eighth decision — the settings row first, the depth resolve second                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| **The probe volume's remaining half: scrolling and recapture** — the visibility maps and the clipmap landed 2026-09-02, the sun's RSM updater 2026-09-04 with lantern's and shard's bakes gone with it, and the punctual producer the same day                                                                                                                                                                                                                                                                                                                                                                                                                              | [50-irradiance-probes.md](50-irradiance-probes.md)'s decision of 2026-08-30 — leak-free, one bounce, dynamic sun and lamps on every tier. Both producers are built, so lantern's coloured wall tints the plaster beside it again; what the volume still cannot do is move, so a probe outside cascade 0's sphere gathers nothing. The traced updater rides on (c)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| SSGI, temporal SSR, TAA, temporal upscaling                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | §9's pass is built; each is its own rung now                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
