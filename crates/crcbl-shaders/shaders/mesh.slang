// crcbl-targets: spirv, wgsl, msl, dxil

// Milestones 3 and 4 of `docs/plan/02-vulkan-backend.md`'s ladder: a
// depth-tested mesh with a perspective camera in a uniform buffer, lit by a
// single directional light with Lambert diffuse and Blinn-Phong specular.
//
// Milestone 5 — the orthographic camera — needs **nothing** from this file. That
// is the point of rung 5: "proving the 2D story (z = z-index) is just a
// projection matrix swap". The shader multiplies by `view_proj`; whether that
// matrix came from a perspective or an orthographic constructor is a decision
// made on the CPU, in `crcbl_render::camera`, and this file cannot tell.
//
// # Still no vertex input state
//
// Same as `triangle.slang`: `crcbl-hal`'s `GraphicsPipelineDesc` has no
// vertex-buffer layout and there is no `bind_vertex_buffer`, so geometry arrives
// through a storage buffer the vertex stage indexes with `SV_VertexID`. What is
// new here is that the draw is **indexed** — `draw_indexed(0..36, 0, 0..1)` — so
// `SV_VertexID` is the value read out of the index buffer rather than a running
// counter. Vertex pulling and the post-transform vertex cache are orthogonal,
// and this is where that stops being a claim.
//
// # Matrix layout, and why no `-matrix-layout-*` flag is needed
//
// A `float4x4` in a constant buffer here comes out of `slangc` decorated
// `RowMajor` with `MatrixStride 16`, and `mul(m, v)` comes out as
// `OpVectorTimesMatrix`. Composing those two: the SPIR-V matrix column `j` is
// the HLSL matrix's *row* `j`, and the `RowMajor` decoration then puts the HLSL
// matrix's *column* 0 in the first sixteen bytes. That is exactly
// `glam::Mat4::to_cols_array`, so `crcbl_shaders::mesh::FrameUniforms` writes a
// `glam` matrix straight through with no transpose.
//
// `-matrix-layout-column-major` was tried and is a no-op for this file — the
// emitted SPIR-V is byte-identical with and without it — so the compile script
// stays flagless. The reason this is written down rather than left to be
// rediscovered is that a transposed matrix produces a *plausible* picture, not
// an obviously broken one; `crcbl-vk`'s e2e suite pins it by asserting which
// cube face lands where on screen, which a transpose could not survive.
//
// # The model matrix moved into an instance array
//
// `docs/plan/03-gpu-driven-rendering.md` §3.2: the per-object transform is a
// `GpuInstance` in a storage buffer written by delta upload, not a field of the
// frame's uniform block. `FrameUniforms` below therefore holds only what is
// genuinely per *frame*, and the vertex stage reads its transform out of
// `instances`.
//
// # Every draw passes zero for both of its bases, and the shader adds them back
//
// `SV_VertexID` and `SV_InstanceID` are HLSL semantics, and `slangc` does not
// lower either of them the same way on the four targets this file declares.
// Measured with slangc 2026.14, from this crate's own committed artifacts:
//
// * **SPIR-V** gives both of them HLSL's meaning by subtracting.
//   `spirv-dis spirv/mesh.spv` shows an `OpISub` of `VertexIndex` against
//   `BaseVertex` and another of `InstanceIndex` against `BaseInstance`, so both
//   ids count from the start of the *draw*.
// * **HLSL, and so DXIL**, is where those semantics come from: `slangc` emits
//   `SV_VertexID` and `SV_InstanceID` unchanged, and D3D12 excludes
//   `BaseVertexLocation` and `StartInstanceLocation` from them. Same meaning as
//   the SPIR-V, which is what the SPIR-V is subtracting to reproduce.
// * **WGSL** emits `@builtin(vertex_index)` and `@builtin(instance_index)` raw,
//   and WebGPU defines both to *include* the draw's `baseVertex` and
//   `firstInstance`.
// * **MSL** passes `[[vertex_id]]` and `[[instance_id]]` straight through, and
//   the Metal Shading Language spec says each "includes the base vertex value"
//   / "includes the base instance value" when one is given. So MSL agrees with
//   WGSL and not with SPIR-V.
//
// A draw's bases therefore reach this shader as two different numbers depending
// on the target, and no source using either id can be correct on all four while
// a base is non-zero. `sprite.slang` hit the instance half of this first and
// its header carries the full diagnosis; the resolution here is the same one,
// applied to both ids: **every draw this pass records passes zero for its base
// vertex and its base instance**, which is the one value all four lowerings
// agree on, and the real bases arrive as data.
//
// That is what lets a mesh at a non-zero `MeshRange::base_vertex` draw its own
// vertices. Until it did, the pool's base vertex travelled through
// `draw_indexed`'s own argument and the `OpISub` above took it straight back
// out — correct for the cube at base 0, and another mesh's geometry for every
// resident after it.
//
// # The base vertex comes from the mesh table, per instance
//
// `docs/plan/03-gpu-driven-rendering.md` §3.3's cull pass has to emit draws for
// the instances it keeps, which means the GPU must be able to resolve an
// instance's geometry with no CPU in the loop. So the pool's ranges are a
// storage buffer of their own — `meshes` below — and `GpuInstance::mesh` is an
// index into it. The vertex stage reads its own base vertex from there rather
// than being handed one per draw.
//
// The two are not the same reach even before culling exists: a per-draw block
// can only say one thing per draw, so every instance in a draw had to share a
// mesh. Resolved per instance, one draw covers instances of *different* meshes
// — which is the shape §3.3 emits and the reason this is the arrangement now
// rather than after the cull pass is written.
//
// # The instance comes from the visible list, and the draw says where its run
// starts
//
// Since 2026-08 the draws are `draw_gen.slang`'s: one per bucket, `first
// instance` zero and an instance count the GPU wrote. `SV_InstanceID` is then
// `0..instance_count` on all four targets — which is what the base being zero
// buys — and it walks the bucket's run of `visible_instances` rather than
// naming an instance itself.
//
// So `DrawConstants` carries the run's *start*, decided when the bucket table
// is built, and the instance index arrives from a buffer the cull pass filled.
// A draw therefore covers however many instances survived culling without the
// CPU learning the number, which is what §3.3 is for.
//
// # The material table now has both halves, and its texture side is one page
//
// §3.2 asks for "a bindless texture array (`BindingModel::Bindless`) or texture
// array pages (`ArrayPages`)". This file has the second: `base_color_textures`
// is a single `Texture2DArray` and `GpuMaterial::base_color_texture` names a
// layer of it. Only one of the two models is here, and the reason is that only
// one of them every device can run — `crcbl-hal` derives `BindingModel` from
// `Features::DESCRIPTOR_INDEXING`, `crcbl-mtl` withdraws that feature, and a
// layer index needs nothing beyond a dynamically indexed sample. A device that
// *does* report the feature runs this same declaration; what it does not get
// yet is one array holding every texture in the world, which is P3's.
//
// Colour space is the one thing a reader has to take on trust from outside this
// file, so it is written down in both places: the page is created as
// `Rgba8UnormSrgb` by `crcbl_render::forward`, which is the format glTF defines
// base-colour textures to be in, so the sampler returns linear light and the
// multiply against `base_color` — which is linear, like every colour in this
// pass — is a multiply of two values in the same space. The tonemap pass and
// the swapchain's sRGB encode are what turn the product into pixels.

/// One vertex of a hardcoded mesh, matching `crcbl_shaders::mesh::MeshVertex`.
///
/// `float4`s rather than packed `float3`/`float2` members: under `std430` a
/// `float3` is 16-byte aligned *and* 12 bytes wide and a `float2` is 8-byte
/// aligned, so a struct mixing them has padding the CPU side has to reproduce
/// exactly. Four `float4`s are 64 bytes with no padding anywhere, which is a
/// layout that cannot drift. The unused `w` components stay unused; P9's real
/// vertex format packs them (topic 03 §3.1).
struct MeshVertex
{
    float4 position;
    float4 normal;
    float4 color;
    /// Base-colour texture coordinates in `xy`; `zw` unused.
    ///
    /// A whole `float4` for two numbers, deliberately: a `float2` here would be
    /// eight bytes followed by eight of tail padding, because the struct's own
    /// alignment is the `float4`s' 16 — so it would cost the same and put a
    /// hole in a layout whose whole point is not having one.
    float4 uv;
};

/// How many cascades the sun's shadow map is split into, matching
/// `crcbl_shaders::mesh::SHADOW_CASCADES`.
///
/// `docs/plan/18-render-features.md`'s shadow section asks for "2–3 cascades".
/// It is one number here and one on the CPU, checked against each other by that
/// module's `the_cascade_count_matches_the_one_mesh_slang_declares`, because a
/// shader reading three matrices out of a block the CPU wrote two into reads the
/// next member as a matrix.
///
/// **Four is the ceiling** and it is `cascade_far` below that sets it: the split
/// distances are one `float4`, so a fifth cascade would have nowhere to say
/// where it ends.
static const uint SHADOW_CASCADES = 2;

/// How many **tiles** the atlas has for shadowed lights beside the cascades,
/// matching `crcbl_shaders::mesh::SHADOW_LIGHT_TILES`.
///
/// `docs/plan/18-render-features.md`'s 2026-08-13 decision: the atlas is a fixed
/// tile grid, the cascades take the first tiles, the rest go one per shadowed
/// spot and `SHADOW_POINT_FACES` per shadowed point, and a light that gets none
/// still lights without occluding. Sizes `FrameUniforms::light_view_proj`, so
/// this is a layout number and the same drift test that pins `SHADOW_CASCADES`
/// pins it.
///
/// **Not a count of lights**: `GpuLight::shadow_tile` is an index into this
/// region and a point light occupies six of it, so how many lights a frame can
/// shadow is a host-side budget (`crcbl_render::shadow::LIGHT_SLOTS`) that no
/// shader has any use for.
static const uint SHADOW_LIGHT_TILES = 6;

/// Tiles one point light's shadow map is, matching
/// `crcbl_shaders::mesh::SHADOW_POINT_FACES`.
///
/// Six atlas tiles rather than a cube map — one image, one sampler, one barrier
/// story — and `point_face` below is the convention that says which is which.
static const uint SHADOW_POINT_FACES = 6;

/// Tiles across the shadow atlas, matching
/// `crcbl_shaders::mesh::SHADOW_ATLAS_COLUMNS`.
static const uint SHADOW_ATLAS_COLUMNS = 4;

/// Tiles down it, matching `crcbl_shaders::mesh::SHADOW_ATLAS_ROWS`.
static const uint SHADOW_ATLAS_ROWS = 2;

/// The side of one tile in texels, matching `crcbl_shaders::mesh::SHADOW_TILE`.
///
/// Read here because every shadow bias in this file is denominated in *tile
/// texels* — see `PUNCTUAL_DEPTH_BIAS_TEXELS` — and the world footprint of one
/// texel is what that converts through, for a cascade as much as for a cone.
static const float SHADOW_TILE = 1024.0;

/// Which atlas tile light tile `tile` of the light region was rendered into.
///
/// The cascades take tiles `0 .. SHADOW_CASCADES` and the lights take the rest,
/// which is the same arithmetic `crcbl_render::shadow::light_tile` performs on
/// the writing side. One statement of the split, read from both ends.
uint light_tile(uint tile)
{
    return SHADOW_CASCADES + tile;
}

/// Where `tile_uv` — a point in tile `tile`, `0..1` on both axes — lands in the
/// atlas as a whole.
///
/// The grid is row-major, so tile `i` is at column `i % SHADOW_ATLAS_COLUMNS`
/// and row `i / SHADOW_ATLAS_COLUMNS`. Both the cascades and the light maps
/// address through this, which is what makes "the cascades are the first tiles"
/// a fact about one function rather than an agreement between two.
float2 atlas_uv(uint tile, float2 tile_uv)
{
    float2 cell = float2(float(tile % SHADOW_ATLAS_COLUMNS), float(tile / SHADOW_ATLAS_COLUMNS));
    return (cell + tile_uv) / float2(float(SHADOW_ATLAS_COLUMNS), float(SHADOW_ATLAS_ROWS));
}

/// Everything the frame needs that is not geometry, matching
/// `crcbl_shaders::mesh::FrameUniforms`.
///
/// A uniform buffer rather than push constants: `Features::PUSH_CONSTANTS` is
/// absent on Tier B, and `crcbl-hal`'s `PushConstantRange` docs are explicit
/// that the substitute is "a *data-layout* decision the renderer makes up front,
/// not something a backend can paper over per draw". So the camera lives here
/// from milestone 3, which is also what `docs/plan/02-vulkan-backend.md` §2.5
/// asks for by name.
struct FrameUniforms
{
    /// World → clip. Reversed-Z: 1.0 at the near plane, 0.0 at infinity.
    float4x4 view_proj;
    /// World-space eye position in `xyz`; `w` unused.
    float4 camera_position;
    /// Flat ambient term in `rgb`; `w` unused. The irradiance grid below is
    /// **added** to it rather than replacing it.
    ///
    /// **Ambient is not a light and is not a row**: it stands in for every
    /// bounce the direct terms do not carry, so it has no position, no cluster
    /// and no shadow. The sun used to sit beside it here as a direction and a
    /// colour, and it is a row in `lights` now — see that binding.
    float4 ambient;
    /// World → cascade `i`'s shadow clip, one matrix per cascade.
    ///
    /// Orthographic and reversed-Z like every other projection here, so a
    /// caster at the light's near plane stores 1.0 and the far plane stores 0.0
    /// — which is why the comparison below is `Greater` and not `Less`.
    ///
    /// **The shadow pass reads this block through `view_proj`, not through
    /// here.** Cascade `i` is rendered by binding a second copy of this whole
    /// struct whose `view_proj` *is* `shadow_view_proj[i]`, which is what lets
    /// the depth-only pipeline reuse `vertexMain` and `mesh_cluster.slang`'s
    /// mesh stage unchanged rather than being a second transform path that could
    /// disagree with the first. This array is the *fragment* stage's copy, for
    /// putting a shaded point back into the map's space.
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    /// Component `i` is how far from the eye cascade `i` reaches, in world
    /// units; a fragment picks the first cascade whose distance covers it.
    ///
    /// Only the first `SHADOW_CASCADES` components are read. The rest are
    /// written as the last real split so an out-of-range read — which cannot
    /// happen, but would be silent if it did — selects the widest cascade rather
    /// than a zero that shadows everything.
    float4 cascade_far;
    /// `x`: one shadow-atlas texel in `u`. `y`: one texel in `v`. `z`: how many
    /// **cascade texels** the sun's shadow comparison is moved towards the
    /// light. `w`: how many more of them it moves per unit of `tan(acos(Ng·L))`,
    /// the rasterised facet's slope — see `shadow_slope`.
    ///
    /// Both texel sizes are carried even where the grid is square and they are
    /// equal: the kernel below steps in tile space and scales back by the grid's
    /// two extents, and the grid stops being square the moment a point light's
    /// six tiles arrive. Deriving one from the other is how the PCF kernel ends
    /// up sampling a rectangle.
    ///
    /// **`z` and `w` are counts of texels, not depths.** `sun_visibility` is
    /// what converts them, through the world footprint of one texel of whichever
    /// cascade the fragment landed in — the same denomination
    /// `PUNCTUAL_DEPTH_BIAS_TEXELS` carries for a cone and a cube face, so the
    /// sun and the punctual lights are biased in one unit rather than two. They
    /// are the sun's numbers all the same: a cascade's footprint is a different
    /// arithmetic from a frustum's, and only the cascades read these.
    float4 shadow_params;
    /// The froxel grid's extent: `x` froxels across the screen, `y` down it, `z`
    /// depth slices, `w` how many pixels wide and tall one tile is.
    ///
    /// The slice count is `CLUSTER_DEPTH_SLICES` for a perspective camera and
    /// `1` for an orthographic one, which has no view depth to slice by. The
    /// tile size is `crcbl_shaders::light::CLUSTER_TILE_PIXELS` at any viewport
    /// the grid buffer can hold and is doubled where it cannot, which is what
    /// keeps a very large frame coarser rather than out of bounds. The
    /// distribution of the slices over depth is a constant this file and
    /// `light_cluster.slang` each declare, so there is nothing there for the two
    /// to disagree about.
    ///
    /// This slot held `docs/plan/25-lod.md`'s selection numbers until the light
    /// list arrived, by which time no stage read them: the projection moved into
    /// `draw_gen.slang`, which owns the hysteresis state it needs, and
    /// `mesh_cluster.slang`'s amplification stage reads that pass's answer rather
    /// than the numbers. A dead `float4` in a block every pipeline binds is not
    /// somewhere to leave a value nothing reads.
    uint4 cluster_grid;
    /// World → light tile `i`'s shadow clip, one matrix per tile of the atlas's
    /// light region.
    ///
    /// **Perspective and reversed-Z**, unlike `shadow_view_proj` above: a spot
    /// is a cone, a cone is a frustum, a point light's face is a 90° one, and the
    /// orthographic box the cascades use is neither. Reversed all the same, so
    /// the same `Greater` comparison sampler reads all three.
    ///
    /// **Indexed by tile rather than by light**, which is what lets one light own
    /// six: `GpuLight::shadow_tile` names the first of a light's tiles and
    /// `point_visibility` adds the face it selected. A tile no light holds is
    /// read by nothing, because the rows that would name it carry
    /// `NO_SHADOW_TILE`. **Last in the block rather than beside the cascades**,
    /// so adding it moved no existing member's offset — which is what let the
    /// cascade goldens stay byte-identical across the change that introduced it.
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    /// Where probe `(0, 0, 0)` of the irradiance grid is, in world space; `w`
    /// unused. Matching `crcbl_shaders::probe::ProbeVolume`.
    ///
    /// **These three are last in the block**, on `light_view_proj`'s terms
    /// exactly: no existing member's offset moves, so every golden blessed
    /// before the grid existed still matches.
    float4 probe_origin;
    /// One over the grid's spacing on each axis; `w` unused.
    ///
    /// Inverted on the host so this stage multiplies rather than divides. An
    /// axis whose reciprocal is zero collapses onto probe 0, which is what the
    /// grid of a scene with no probes carries.
    float4 probe_inv_spacing;
    /// `xyz`: how many probes the grid holds on each axis. `w`: their product,
    /// which is how many rows `probes` holds and the bound every fetch below is
    /// clamped against.
    ///
    /// All zero for a scene with no probes, and `probe_irradiance` then returns
    /// exactly zero with no branch anywhere — see it.
    uint4 probe_counts;
};

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's "transform, mesh id, material
/// id, flags", plus the sector id its 2026-07-27 correction adds. This shader
/// reads the transform, the mesh id and the material id — see the Rust struct,
/// which carries the full account of what each field is for and which slice
/// consumes it.
struct GpuInstance
{
    /// Model → sector. Rigid (rotation + translation), so its upper-left 3×3
    /// transforms normals correctly without an inverse-transpose.
    float4x4 transform;
    /// Which mesh to draw: an index into `meshes` below. The vertex stage
    /// resolves this instance's base vertex through it, which is what lets one
    /// draw cover instances of different meshes.
    uint mesh;
    /// Which material to shade with: an index into `materials` below. The
    /// vertex stage passes it through to the fragment stage, which multiplies
    /// that row's base colour into the interpolated albedo, so two instances of
    /// the same mesh differing only here are two colours.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. Bit 0 says the element is a live instance rather than
    /// a removed one's leftovers, and `cull.slang` — which is the only thing
    /// that walks the array — is what reads it. A draw addresses the instance
    /// it was told to, so nothing here does.
    uint flags;
};

/// One resident mesh's range in the geometry pools **and its local-space
/// bounds**, matching `crcbl_shaders::mesh::GpuMesh`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.1's three integers, in the buffer
/// the GPU resolves them out of. Two of the three are `draw_indexed`'s own
/// arguments and are read by nothing here yet; they are in the record because
/// §3.3's cull pass builds its draws out of exactly these numbers, and a table
/// carrying only what today's vertex stage reads would have to change layout
/// the day it does.
///
/// The bounds are §3.3's other half, and they are here rather than in a table
/// of their own because they have the same lifetime as the range: both are
/// decided when the mesh is suballocated, both are cleared when it is freed,
/// and a second table would be a second thing to keep in step. `cull.slang`
/// reads them; nothing in this file does.
///
/// # Nine scalars, and not a `float3` in sight
///
/// Under `std430` a `float3` is 16-byte aligned, so a `float3 bounds_min` after
/// three `uint`s would land at offset 16 with four bytes of padding in front of
/// it and the struct would be 48 bytes rather than 36. A struct of scalars has
/// no padding anywhere and a stride that is exactly the sum of its members —
/// which is the same reason the three `uint`s above are three `uint`s.
struct GpuMesh
{
    /// This mesh's first vertex in the pool, added to every index a draw of it
    /// reads. The pool stores indices mesh-relative, so this is the only thing
    /// that says where the mesh actually is.
    uint base_vertex;
    /// This mesh's first index in the index pool.
    uint base_index;
    /// How many indices the mesh has. Zero for an entry naming no mesh — see
    /// `crcbl_render::MeshPool`, which clears an entry when its mesh is freed.
    uint index_count;
    /// Lowest corner of the mesh's **local-space** axis-aligned bounding box.
    float min_x;
    float min_y;
    float min_z;
    /// Highest corner of the same box.
    float max_x;
    float max_y;
    float max_z;
};

/// One material's shading factors **and its base-colour texture**, matching
/// `crcbl_shaders::mesh::GpuMaterial`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's material table, which "holds
/// texture indices + factors". That section pairs it with "a bindless texture
/// array (`BindingModel::Bindless`) or texture array pages (`ArrayPages`)", and
/// the index below is an `ArrayPages` **page layer**: `base_color_textures` is
/// one `Texture2DArray` and the index selects a slice of it. That is the model
/// every device can run — a descriptor array needs `DESCRIPTOR_INDEXING`, which
/// Metal does not report — so there is one lookup here rather than a
/// permutation, which is the same plan's "the lesser path is a constraint on
/// data layout, not a separate renderer".
struct GpuMaterial
{
    /// Linear RGBA factor multiplied into the vertex albedo and the texel.
    float4 base_color;
    /// Which layer of `base_color_textures` this material samples.
    ///
    /// Layer 0 by convention holds a white texel, so a material that wants no
    /// texture leaves this at zero and the multiply below changes nothing —
    /// `crcbl_render`'s forward pass is what fills that layer and says so.
    uint base_color_texture;
    /// How metallic the surface is: 0 a dielectric, 1 a conductor. glTF's
    /// `metallicFactor`.
    ///
    /// Read twice by `fragmentMain`: `F0` interpolates between the dielectric
    /// 0.04 and the base colour on it, and the diffuse albedo is scaled *down*
    /// by it — so a conductor reflects and does not scatter, and has no ambient
    /// term either. See the accumulation loop.
    float metallic;
    /// How rough it is: 0 a mirror, 1 fully diffuse. glTF's `roughnessFactor`.
    ///
    /// Squared into the GGX `alpha` rather than used directly, which is the
    /// usual perceptual parameterisation, and clamped away from zero — the
    /// normal distribution divides by `alpha` squared.
    float roughness;
    /// Pads the row to 32 bytes, which is what `std430` rounds it to anyway:
    /// the struct's alignment is the `float4`'s 16, so a 28-byte row would be
    /// read at a stride of 32 by a shader while the CPU wrote it at 28. Spelled
    /// out rather than implied, exactly as `DrawConstants` below spells its own.
    ///
    /// **There were three of these**, and the two shading factors above went
    /// into two of them — so the row gained a material model without the table's
    /// stride moving and without a single golden shifting for a layout reason.
    uint pad0;
};

/// One irradiance probe, matching `crcbl_shaders::probe::GpuProbe`.
///
/// `docs/plan/18-render-features.md`'s L1 spherical-harmonic probe: four
/// coefficients per colour channel, packed `(L1x, L1y, L1z, L0)` so that the
/// irradiance a normal receives is `dot(sh, float4(N, 1))` and nothing else. No
/// `pow` and no trigonometry — the rule `ggx_lobe` is already held to.
///
/// The coefficients are **irradiance**, not radiance: the clamped-cosine
/// convolution is a per-band scale and the host folded it in when it filled the
/// row. `crcbl_shaders::probe` is where that arithmetic lives and where it is
/// checked against the literature it comes from.
struct GpuProbe
{
    /// The red channel's four coefficients.
    float4 sh_r;
    /// The green channel's.
    float4 sh_g;
    /// The blue channel's.
    float4 sh_b;
};

/// One light, matching `struct GpuLight` in `light_cluster.slang` and
/// `crcbl_shaders::light::GpuLight`.
struct GpuLight
{
    /// World-space position in `xyz`, influence radius in `w`.
    float4 position;
    /// Colour premultiplied by intensity in `rgb`; `w` unused.
    float4 color;
    /// Towards the light for a directional; the cone axis for a spot. `w` is the
    /// cosine of a spot's outer half-angle.
    float4 direction;
    uint kind;
    /// The cosine of a spot's inner half-angle.
    float cos_inner;
    /// The **first** entry of `FrameUniforms::light_view_proj` this light
    /// occludes through, or `NO_SHADOW_TILE` if it was given none.
    ///
    /// A spot occludes through that one tile; a point light through the
    /// `SHADOW_POINT_FACES` tiles from here, and `point_visibility` adds the face
    /// it selected. The atlas holds `SHADOW_LIGHT_TILES` of them and a scene may
    /// want more than they hold, so a row saying "no tile" is the ordinary case:
    /// `docs/plan/18-render-features.md`'s rule is that such a light still lights
    /// and simply does not occlude.
    uint shadow_tile;
    uint pad1;
};

/// `GpuLight::shadow_tile` for a light with no shadow map of its own.
///
/// Deliberately not zero, which is a real tile. The shading loop compares
/// against `SHADOW_LIGHT_TILES` rather than against this value, so a row carrying
/// any other out-of-range tile is refused the same way — and a point light's
/// whole run is checked, because this value plus a face index would otherwise
/// wrap into the region.
static const uint NO_SHADOW_TILE = 0xffffffffu;

/// `GpuLight::kind` for the sun: no position, no falloff, and in every froxel.
static const uint KIND_DIRECTIONAL = 0;

/// `GpuLight::kind` for a point light.
static const uint KIND_POINT = 1;

/// `GpuLight::kind` for a spot light.
static const uint KIND_SPOT = 2;

/// How many light indices one froxel's list holds, matching
/// `crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY`.
static const uint CLUSTER_LIGHT_CAPACITY = 16;

/// Words one froxel occupies in `cluster_lights`, matching
/// `crcbl_shaders::light::CLUSTER_STRIDE`.
static const uint CLUSTER_STRIDE = 17;

/// The statistics word the clustering pass counts dropped assignments into,
/// matching `crcbl_shaders::light::CLUSTER_OVERFLOW_WORD`.
///
/// Declared here and read by nothing: this file is the other half of the grid's
/// contract, and the constant is one of the numbers `crcbl_shaders::light`
/// checks both declarations of.
static const uint CLUSTER_OVERFLOW_WORD = 2;

/// Depth slices a perspective grid has, matching
/// `crcbl_shaders::light::CLUSTER_DEPTH_SLICES`.
static const uint CLUSTER_DEPTH_SLICES = 24;

/// The view depth the first slice starts at, matching
/// `crcbl_shaders::light::CLUSTER_NEAR`.
static const float CLUSTER_NEAR = 0.1;

/// The view depth the last slice's resolution ends at, matching
/// `crcbl_shaders::light::CLUSTER_FAR`.
static const float CLUSTER_FAR = 1000.0;

/// Where this draw's run of visible instances starts and what geometry it
/// draws, matching `crcbl_shaders::mesh::DrawConstants`.
///
/// One block per draw, reached through a dynamic offset — the same mechanism,
/// for the same reason, as `sprite.slang`'s `SpriteConstants::base`. The header
/// says why the first number cannot be the draw's own base instance.
struct DrawConstants
{
    /// This draw's bucket's first slot in `visible_instances`, added to
    /// `SV_InstanceID` — which counts from zero on every target, because every
    /// draw this pass records passes zero for its first instance.
    uint base;
    /// Which mesh this draw's geometry is, as an index into `meshes`.
    ///
    /// **The bucket's mesh, not the instance's**, and the two are the same thing
    /// for every instance a bucket can hold: an indexed draw has one index range
    /// and `draw_gen.slang` takes it from this same entry, so a draw already
    /// cannot cover two meshes' triangles. What the distinction buys is
    /// `docs/plan/25-lod.md`'s uniform cut — the level a DAG is drawn at is a
    /// mesh of its own, selected per instance on the GPU, and the instance goes
    /// on naming level 0 because that is the entry the cull pass reads a
    /// bounding box out of.
    uint mesh;
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size and which naga enforces where a driver would not.
    uint pad0;
    uint pad1;
};

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame;

[[vk::binding(1, 0)]]
StructuredBuffer<MeshVertex> vertices;

[[vk::binding(2, 0)]]
StructuredBuffer<GpuInstance> instances;

[[vk::binding(3, 0)]]
ConstantBuffer<DrawConstants> draw;

[[vk::binding(4, 0)]]
StructuredBuffer<GpuMesh> meshes;

/// The per-bucket runs of surviving instance indices `draw_gen.slang` scatters
/// into. Read-only here: this stage picks its instance out of a run and edits
/// nothing.
[[vk::binding(5, 0)]]
StructuredBuffer<uint> visible_instances;

/// The material table §3.2 asks for, indexed by `GpuInstance::material`.
///
/// Read in the **fragment** stage. It was the vertex stage until 2026-08, and
/// the move is not a correctness fix: the factor is one value per instance and
/// interpolation is linear, so scaling the corners and scaling the interpolated
/// result are the same picture — which is exactly what makes the move safe to
/// make on its own, with every golden still matching. What it bought is that
/// the fragment stage can reach the table at all, which the texture half of
/// §3.2 needs and a per-vertex read can never provide: a texture fetch is not
/// constant across a primitive, so nothing it feeds can be folded into a
/// varying. That half is here now — see `base_color_textures` below.
/// `VertexOutput::material` is the id that got it here, and its declaration is
/// where the per-target risk is written down.
[[vk::binding(6, 0)]]
StructuredBuffer<GpuMaterial> materials;

/// §3.2's texture side: **one `ArrayPages` page**, indexed by
/// `GpuMaterial::base_color_texture`.
///
/// A `Texture2DArray` — one image, one descriptor, a layer per material texture
/// — rather than an array *of* descriptors, and that is the whole binding-model
/// decision. A descriptor array is `BindingModel::Bindless` and needs
/// `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws; a layer index
/// is an ordinary dynamically-indexed sample that every target has always had,
/// so this one declaration runs on all four and the layout that binds it sets
/// no `BindingFlags` at all.
///
/// **The layers are sRGB-encoded and the sampler is what decodes them.**
/// `crcbl_render::forward` creates the image as `Rgba8UnormSrgb`, and the
/// format is the whole of the decode: the sampler returns linear light, which
/// is the space `base_color` and the vertex albedo are in, so the three can be
/// multiplied. Nothing here converts anything — a `pow` in this file would mean
/// the format was wrong. sRGB is also the format glTF requires of a base-colour
/// texture, where its base-colour *factor* is linear; the two spaces meeting in
/// one product is the trap this arrangement removes.
[[vk::binding(7, 0)]]
Texture2DArray<float4> base_color_textures;

/// The one sampler the page is read through: nearest, clamped.
///
/// Nearest because these are material *pages* rather than photographs — there
/// is no mip chain (§3.2's mip generation is a compute pass of its own) and a
/// filtered read of a page with no mips is a shimmer, not a smoother picture.
/// One sampler and no choice, for `sprite.slang`'s reason: a shader cannot ask
/// which filter it got, so a second one would be a second pipeline.
[[vk::binding(8, 0)]]
SamplerState base_color_sampler;

/// The sun's cascaded shadow map: `SHADOW_CASCADES` square tiles **side by side
/// in one 2D image**, tile `i` holding cascade `i`.
///
/// # An atlas, not a `Texture2DArray`
///
/// One layer per cascade is the textbook shape and it is not the shape here,
/// because of what is on the *writing* side: the shadow pass renders cascade `i`
/// by setting a viewport over tile `i` of a single attachment, and a
/// `crcbl_render::graph` render pass attaches an image, not one of its layers.
/// An array would need per-layer attachment views the graph does not build.
///
/// Bindings 9 to 14 are `mesh_cluster.slang`'s, and this file's `fragmentMain`
/// is the fragment stage of *that* pipeline too — so the numbering here skips
/// past them rather than colliding with them. See that file's header.
///
/// # `DepthTexture2D`, and not `Texture2D<float>`
///
/// The two are the same HLSL declaration and the same D3D12 SRV, and they are
/// **not** the same WGSL. `Texture2D<float>` lowers to `texture_2d<f32>`, which
/// `textureSampleCompareLevel` does not accept — WebGPU's comparison sample
/// takes a `texture_depth_2d` and nothing else, so the artifact compiled but
/// could never have been created into a pipeline. `DepthTexture2D` is Slang's
/// name for that type and it lowers correctly on all four: `texture_depth_2d`,
/// `depth2d<float, access::sample>` in MSL, an `OpTypeImage` with `Depth = 1` in
/// SPIR-V, and the same `Texture2D` at `t#` in HLSL.
///
/// It is the same distinction the seam grew `SampleType::Depth` for, one layer
/// down: a depth image is a different *kind* of sampled image on the two targets
/// that have a binding model, and identical on the two that do not.
[[vk::binding(15, 0)]]
DepthTexture2D shadow_atlas;

/// The comparison sampler the atlas is read through — **hardware PCF**.
///
/// `SamplerComparisonState`, not `SamplerState`: each `SampleCmpLevelZero`
/// below returns the *filtered fraction of four texels that passed the depth
/// test*, so the 3×3 kernel is nine hardware-bilinear comparisons rather than
/// nine raw fetches the shader compares and averages itself. Created with
/// `CompareOp::Greater` and linear filtering by `crcbl_render::forward`, and
/// `Greater` is the reversed-Z spelling of "the receiver is nearer the light
/// than whatever was stored" — see `crcbl_hal::SamplerDesc::compare`, which
/// carries the same warning.
[[vk::binding(16, 0)]]
SamplerComparisonState shadow_sampler;

/// `docs/plan/18-render-features.md`'s light list: every light in the frame, one
/// row each, **the sun included**.
///
/// The sun was `FrameUniforms::light_direction` and `light_color` until the list
/// existed, and moving it here is the point rather than a side effect: a
/// directional row is flagged as affecting every froxel, so the loop below has
/// one shape and no "and also the sun" tail. What that buys is that a second
/// directional light is data, and adding one changes nothing in this file.
///
/// Bindings 17 to 19 are `mesh_cluster.slang`'s, declared by the mesh and
/// amplification stages of the pipeline this file's `fragmentMain` completes, so
/// the numbering skips past them exactly as it skipped 9 to 14 above.
[[vk::binding(20, 0)]]
StructuredBuffer<GpuLight> lights;

/// The froxel grid `light_cluster.slang` filled: `CLUSTER_STRIDE` words per
/// froxel, the count first and then that many indices into `lights`.
///
/// Indexed by *this fragment's own position* — its pixel gives the tile, its
/// view depth gives the slice — which is the whole of what clustered forward
/// buys over looping the scene's lights.
[[vk::binding(21, 0)]]
StructuredBuffer<uint> cluster_lights;

/// `docs/plan/18-render-features.md`'s screen-space ambient occlusion, as
/// `ssao_blur.slang` left it: one `R8Unorm` channel the size of the frame, `1.0`
/// where nothing occludes.
///
/// # Fetched by pixel, not sampled, and clamped to the image
///
/// A `Load` at `SV_Position.xy` — the same value `froxel_of` above is indexed by,
/// and for the same reason. When the occlusion pair ran, this image is exactly
/// the size of the frame being drawn, so there is a texel per fragment and no
/// interpolation to want: a `Load` needs no sampler, no UV and no texel-centre
/// arithmetic, which is one less thing for four backends to disagree about.
///
/// **The coordinate is clamped against `GetDimensions` all the same**, and that
/// clamp is what lets the off-switch below be one texel rather than a frame-sized
/// clear. A `Load` outside a texture's extent yields *zero* rather than the
/// nearest texel, so an unclamped fetch reads a 1×1 placeholder as total
/// occlusion at every pixel but one — a frame with no ambient term at all, black
/// wherever ambient is the whole of the light. That was found on hardware, with
/// nothing anywhere reporting an error. A frame-sized channel is unaffected by
/// the clamp: every fragment of the frame is inside the image already.
///
/// # It is bound on every path, and is `1.0` when the passes are not there
///
/// `crcbl_render::forward` binds a 1×1 white image when it adds no AO pass, on
/// the terms `shadow_atlas`' placeholder is bound: the slot has to be filled
/// because Slang's Metal backend materialises every global into every entry
/// point, and the honest filler for "no occlusion was computed" is the value that
/// occludes nothing. So this file has no branch for the feature being off.
///
/// **Last of the set, and that is structural.** `mesh_cluster.slang` declares up
/// to 19 and mirrors 20 and 21; 22 is past all of them, so it needs no mirror
/// there and no Metal argument-table index moves. See `SHADOW_ATLAS_BINDING`'s
/// note in `crcbl_render::forward` on why the list stays ascending.
[[vk::binding(22, 0)]]
Texture2D<float> ambient_occlusion;

/// `docs/plan/18-render-features.md`'s irradiance grid, row by row — the
/// `FrameUniforms::probe_*` header above says where they are.
///
/// **Appended after the occlusion channel, never inserted**, which is the same
/// rule that put that one last: `crcbl-mtl` gives a resource the next index in
/// its Metal argument table by counting the same-table entries of the layout
/// list, and Slang numbers a stage's arguments by declaration order, so the two
/// agree only while both ascend. 23 is past everything `mesh_cluster.slang`
/// declares as well — that file reaches 21 — so it needs no mirror there and no
/// index it already owns moves.
///
/// **Bound on every path, and a scene with no probes binds a single zeroed
/// row.** A storage buffer has to have a size, so the table is at least one row
/// whatever the scene, and `probe_irradiance` reads that row and adds zero. The
/// off-switch is the grid's counts being zero rather than a branch here — the
/// same shape as the white 1×1 image `ambient_occlusion` falls back to.
[[vk::binding(23, 0)]]
StructuredBuffer<GpuProbe> probes;

struct VertexOutput
{
    float4 position : SV_Position;
    float3 world_position : POSITION0;
    float3 world_normal : NORMAL0;
    float4 color : COLOR0;
    /// This primitive's material row, straight off `GpuInstance::material`.
    ///
    /// # `nointerpolation`, and why it is the whole of this varying's risk
    ///
    /// An index is not a quantity: row 1 is not "between" rows 0 and 2, and
    /// interpolating it produces a fragment that reads a row no vertex named.
    /// So this is `nointerpolation` and the value the fragment stage sees is
    /// the provoking vertex's — which is also the only reading under which
    /// moving the multiply across the stage boundary leaves the picture alone.
    ///
    /// The header's account of `SV_VertexID` and `SV_InstanceID` is why that
    /// sentence is not enough on its own: this file's four targets have twice
    /// disagreed about what an integer the vertex stage hands the fragment
    /// stage means, and each time the disagreement was found by rendering one
    /// scene through two of them rather than by reading the emitted code. A
    /// flat integer varying is the same class — `spirv` decorates it `Flat`,
    /// WGSL writes `@interpolate(flat)`, MSL writes `[[flat]]`, DXIL carries an
    /// interpolation mode per signature element — and the four spellings agree
    /// only as long as each is actually emitted.
    ///
    /// What a wrong lowering looks like here is deliberately loud rather than
    /// subtle: `crates/crcbl/tests/golden/cube.png` draws its two instances in
    /// unlike colour families, so a fragment that resolved the wrong row is a
    /// smear between them and not a shade of the right answer.
    nointerpolation uint material : TEXCOORD0;
    /// This fragment's base-colour texture coordinates, straight off the
    /// vertex. Interpolated, unlike `material` above: a UV *is* a quantity, and
    /// the point of it is to vary across the primitive.
    float2 uv : TEXCOORD1;
};

[shader("vertex")]
VertexOutput vertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID)
{
    GpuInstance instance = instances[visible_instances[draw.base + instance_id]];
    // The mesh names its vertices, and it is the bucket's mesh — the same entry
    // `draw_gen.slang` took this draw's index range from, so the base vertex and
    // the indices cannot come from two different meshes. Nothing on this path
    // came from the draw call itself, which is what §3.3's cull pass needs in
    // order to emit draws the CPU never looked at.
    GpuMesh mesh = meshes[draw.mesh];
    MeshVertex vertex = vertices[index + mesh.base_vertex];

    float4 world = mul(instance.transform, float4(vertex.position.xyz, 1.0));

    VertexOutput output;
    output.position = mul(frame.view_proj, world);
    output.world_position = world.xyz;
    // The transform is rigid, so its 3×3 part is orthonormal and is its own
    // inverse-transpose. The renderer states that as an obligation on
    // `GpuInstance::transform` rather than shipping a second matrix per
    // instance that nothing needs until non-uniform scale exists.
    output.world_normal = mul((float3x3)instance.transform, vertex.normal.xyz);
    output.color = vertex.color;
    // The instance names its material exactly as it names its mesh, so two
    // instances of one mesh can be two colours with no CPU in the loop and no
    // second draw. Passed on rather than resolved here: the table is the
    // fragment stage's to read now, for the reason its binding gives.
    output.material = instance.material;
    output.uv = vertex.uv.xy;
    return output;
}

/// The smallest roughness the specular lobe is evaluated at.
///
/// `alpha` is the roughness squared and the normal distribution divides by
/// `alpha` squared, so a roughness of zero is a division by zero — a mirror is
/// a delta function and a delta function has no finite value to sample.
/// Clamping rather than guarding the divide keeps the lobe *narrow* at the
/// limit instead of flattening it, which is what a reader authoring a polished
/// material expects to happen.
///
/// The value is the usual one: at this roughness a highlight is a couple of
/// pixels across at any resolution this engine renders at, so lowering it
/// further buys aliasing rather than sharpness.
static const float MIN_ROUGHNESS = 0.045;

/// The roughness where SSR's sharpness ramp reaches zero.
///
/// This mirrors `ssr.slang`'s constant. The attachment stores the resulting
/// ramp, rather than this value, because zero round-trips exactly through
/// `Rgba8Unorm`.
static const float ROUGHNESS_CUTOFF = 0.5;

/// The dielectric normal-incidence reflectance every non-metal is given.
///
/// Four per cent covers the range real insulators sit in closely enough that a
/// material column for it would be a knob with no picture behind it. A metal
/// interpolates away from it towards its own base colour — see `f0_of` below.
static const float DIELECTRIC_F0 = 0.04;

/// The Cook-Torrance specular lobe, `pi * D * V * F`, without the `N·L` the
/// accumulation multiplies in.
///
/// Trowbridge-Reitz `D`, Smith height-correlated `V` (the visibility form, so
/// the `1 / (4 N·L N·V)` of the microfacet denominator is already inside it),
/// and Schlick's `F`. One lobe for every light kind, which is topic 18's "one
/// material model, one BRDF, one set of inputs" holding by construction.
///
/// **The `pi` is the diffuse term's, folded out of here rather than into it.**
/// Trowbridge-Reitz normalises to `alpha2 / (pi * shape * shape)` and Lambert
/// to `albedo / pi`, and this engine's diffuse is a bare `albedo * N·L` — the
/// convention where a light's intensity has absorbed the `1 / pi`. Writing the
/// textbook `D` against that diffuse would put every highlight a factor of
/// `pi` under the surface it sits on, which is not a look anyone chose. So the
/// two lobes share one convention: neither carries its `1 / pi`, and the ratio
/// between them is the physical one. It is also what keeps a roughness of a
/// half close to the Blinn exponent of 32 this replaced, so the goldens moved
/// for the shape of the lobe rather than for its strength.
///
/// **No `pow` and no trigonometry anywhere in here**, which is the same rule
/// `OPEN_BOX_FACES` is built under on the host: a platform's transcendental
/// functions differ in the last place between targets, and this file's whole
/// determinism argument is that it does not use any. Schlick's fifth power is
/// two squarings and a multiply; the rest is dot products, a square root and
/// two divides.
///
/// `alpha2` rather than the roughness, because the caller has clamped and
/// squared it once for the whole fragment rather than once per light.
float3 ggx_lobe(float alpha2, float3 f0, float n_dot_l, float n_dot_v, float n_dot_h,
                float v_dot_h)
{
    // Trowbridge-Reitz. The denominator is `alpha2` at `n_dot_h == 1` and grows
    // from there, so it is only zero when `alpha2` is — which `MIN_ROUGHNESS`
    // is what rules out. The `max` is the belt on those braces, in the shape
    // every other divide in this file guards itself with.
    float shape = n_dot_h * n_dot_h * (alpha2 - 1.0) + 1.0;
    float d = alpha2 / max(shape * shape, 1e-8);

    // Smith height-correlated visibility. Both square roots are of a value at
    // least `alpha2`, and the sum is at least that times `n_dot_l + n_dot_v` —
    // so the guard here is against a fragment where both cosines are zero at
    // once, which is a surface exactly edge-on to the eye and the light.
    float lambda_v = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - alpha2) + alpha2);
    float lambda_l = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - alpha2) + alpha2);
    float visibility = 0.5 / max(lambda_v + lambda_l, 1e-6);

    // Schlick, as repeated multiplication.
    float grazing = 1.0 - v_dot_h;
    float grazing2 = grazing * grazing;
    float3 fresnel = f0 + (float3(1.0, 1.0, 1.0) - f0) * (grazing2 * grazing2 * grazing);

    return d * visibility * fresnel;
}

/// Ceiling on the slope factor every shadow bias in this file is scaled by.
///
/// `tan(acos(Ng·L))` runs to infinity as a surface turns edge-on to the light,
/// and an unbounded bias there is peter-panning — the contact point of a shadow
/// slides away from its caster. Bounded rather than removed because the same
/// term is what keeps a *steeply* lit surface free of acne; the clamp is the
/// standard pairing.
///
/// It binds more often now that the slope is read off the facet rather than off
/// the shading normal: a fragment whose triangle faces away from the light while
/// its interpolated normal still faces it gets the clamp, where before it got a
/// finite slope. `shadow_slope` argues why that is the answer wanted there.
///
/// **A bound on the tangent, and on nothing else** — it is not denominated in
/// depth, in texels or in world units, which is why one number serves
/// `sun_visibility` and `punctual_visibility` even though what each multiplies
/// it by is different. Five is a surface about 79° from the light: past that the
/// diffuse term this multiplies is under a fifth of its peak, so a shadow that
/// detaches there costs less picture than the acne the unclamped term prevents.
static const float SHADOW_SLOPE_BIAS_CLAMP = 5.0;

/// The normal of the triangle the rasteriser actually drew, from the
/// screen-space derivatives of the world position.
///
/// The two derivatives of a perspective-correct varying span the facet's tangent
/// plane, so their cross product is that facet's normal — *exactly*, for the
/// planar triangle it came from, where `shading_normal` is only what the corners'
/// own normals interpolate to. That difference is the whole reason this exists:
/// every shadow bias below is a distance the surface climbs across one shadow
/// texel, and a shading normal that describes a smooth field rather than the
/// facet under it makes that distance too small by however much the two disagree.
///
/// **A fragment stage only**, which is why the visibility functions take the
/// result rather than computing it: `ddx`/`ddy` are a quad's finite differences
/// and have no meaning in any other stage. Both are taken from the same
/// primitive — a quad belongs to one triangle, helper lanes included — so a
/// fragment on a silhouette gets its own triangle's facet and not a blend of two.
///
/// **The sign comes from `shading_normal`, not from the cross product's own
/// handedness.** Which way `cross(ddx, ddy)` points depends on the target's
/// screen-space Y direction and on the primitive's winding, and this file
/// compiles to four targets; the shading normal already carries the side the
/// author meant, so aligning to it is correct on all of them by construction
/// where a hard-coded sign would be correct on some. A facet in the other
/// hemisphere would turn every bias below into its own negation.
///
/// It is not a hypothetical sign, either. Measured through the SPIR-V artifact
/// on radv by writing `0.5 + 0.5 * dot(normalize(cross(ddx, ddy)), N)` to the
/// lit target: every fragment of a scene of flat slabs read exactly 0, so the
/// bare cross product is **anti-parallel** to the authored normal there, and
/// negating it read exactly 1. The other three targets were not measured.
float3 geometric_normal_of(float3 world_position, float3 shading_normal)
{
    float3 facet = cross(ddx(world_position), ddy(world_position));
    float extent = length(facet);
    if (extent < 1e-12)
    {
        // Nothing to read: a primitive whose world position does not vary across
        // the quad has no tangent plane, and the shading normal is the honest
        // answer rather than a normalised zero.
        return shading_normal;
    }
    facet /= extent;
    return dot(facet, shading_normal) < 0.0 ? -facet : facet;
}

/// How far the rasterised surface climbs across one shadow texel, per texel of
/// footprint: `tan(acos(Ng·L))`, bounded by `SHADOW_SLOPE_BIAS_CLAMP`.
///
/// One function for the sun and for the punctual lights, because it is one piece
/// of knowledge — what a bias in texels has to be multiplied by — and the clamp
/// belongs with it rather than at each call site.
///
/// `geometric_normal` is `geometric_normal_of`'s, and passing the *shading*
/// normal here is the defect that function exists to remove. A facet turned away
/// from the light gives a non-positive cosine and therefore the clamp, which is
/// the conservative answer: such a fragment is at or past the terminator of the
/// surface that shadows it, and it is where acne is worst.
float shadow_slope(float3 geometric_normal, float3 to_light)
{
    float cosine = saturate(dot(geometric_normal, to_light));
    float slope = sqrt(saturate(1.0 - cosine * cosine)) / max(cosine, 1e-4);
    return min(slope, SHADOW_SLOPE_BIAS_CLAMP);
}

/// 3×3 PCF over tile `tile`, at `tile_uv` in that tile's own `0..1` space.
///
/// One kernel for the cascades and for the light maps, which is what keeps "a
/// tap never leaves its own tile" a property of one function. The clamp is half
/// a texel inside the tile: the comparison sampler filters a 2×2 neighbourhood
/// around each tap, so a tap exactly on the edge would still reach across it and
/// read the neighbouring map's depths as if they were this one's.
float tile_pcf(uint tile, float2 tile_uv, float reference)
{
    float2 texel = frame.shadow_params.xy;
    // The grid's two extents, which is what turns an *atlas* texel into a step
    // in tile space: a tile is `1 / SHADOW_ATLAS_COLUMNS` of the atlas wide and
    // `1 / SHADOW_ATLAS_ROWS` of it tall, so scaling back up by them lands on
    // exactly one shadow-map texel on either axis.
    float2 grid = float2(float(SHADOW_ATLAS_COLUMNS), float(SHADOW_ATLAS_ROWS));
    float2 tile_min = float2(0.5, 0.5) * texel * grid;
    float visibility = 0.0;
    for (int y = -1; y <= 1; ++y)
    {
        for (int x = -1; x <= 1; ++x)
        {
            float2 offset = float2(float(x), float(y)) * texel * grid;
            float2 tap = clamp(tile_uv + offset, tile_min, 1.0 - tile_min);
            visibility +=
                shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv(tile, tap), reference);
        }
    }
    return visibility / 9.0;
}

/// The fraction of the sun this fragment can see: `1.0` fully lit, `0.0` fully
/// shadowed, and the values between are the PCF filter.
///
/// `to_light` is the unit vector towards the sun and `n_dot_l` the *unclamped*
/// Lambert term. A surface facing away from the light is already black from the
/// diffuse term, and asking the shadow map about it is both wasted work and the
/// place self-shadowing acne is worst, so it returns early — the caller
/// multiplies the result into a diffuse term that is zero there anyway.
///
/// `geometric_normal` is `geometric_normal_of`'s and drives the slope term
/// alone. The early return above stays on `n_dot_l`, which is the *shading*
/// normal's: what it says is "this fragment's diffuse term is already zero", and
/// that is a fact about the normal the shading used.
float sun_visibility(float3 world_position, float3 to_light, float n_dot_l,
                     float3 geometric_normal)
{
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    // The first cascade whose reach covers this fragment. Distance to the eye
    // rather than view-space z: the splits are radii around the camera, so a
    // fragment at the edge of the screen and one straight ahead at the same
    // distance land in the same cascade and the seam is a sphere rather than a
    // plane the camera can rotate.
    float eye_distance = length(world_position - frame.camera_position.xyz);
    uint cascade = SHADOW_CASCADES - 1;
    for (uint index = 0; index < SHADOW_CASCADES; ++index)
    {
        if (eye_distance < frame.cascade_far[index])
        {
            cascade = index;
            break;
        }
    }

    // The bias, and it is `punctual_visibility`'s exactly: the world position is
    // moved towards the light before projecting, by a multiple of the world
    // footprint of one texel of *this cascade's* map. A cascade is an
    // orthographic box of `2 * radius` fitted to `SHADOW_TILE` texels, and its
    // radius is the split distance the selection above read — the same
    // arithmetic `crcbl_render::shadow::cascade_matrix` snaps its origin to.
    //
    // **A number in texels rather than in shadow-clip depth**, which is the
    // whole of this: acne is the map quantising a receiver's own depth away
    // across one texel, so the error to cover scales with the texel's footprint
    // and with the surface's slope. A constant added in clip instead scales with
    // the cascade's whole depth range — `2 * radius` plus `CASTER_REACH`, tens
    // of metres of it belonging to casters standing off the near plane — so the
    // same number meant 0.83 m of world slack on the outer cascade and lit a
    // strip along the foot of every wall.
    //
    // What the two counts are and why they are what they are lives on the host,
    // in `crcbl_render::shadow`: the slope term is this kernel's reach, and the
    // constant is what a texel's own quantisation costs on top of it. The slope
    // is read off the **facet**, so a surface shaded with a normal its triangles
    // do not have gets the bias its triangles need — which is what let that
    // constant halve. `crcbl_render::shadow::DEPTH_BIAS_TEXELS` has the numbers.
    float texel_world = 2.0 * frame.cascade_far[cascade] / SHADOW_TILE;
    float offset = texel_world
                 * (frame.shadow_params.z
                    + frame.shadow_params.w * shadow_slope(geometric_normal, to_light));

    float4 clip = mul(frame.shadow_view_proj[cascade],
                      float4(world_position + to_light * offset, 1.0));
    // Orthographic, so `w` is 1 and the divide is a formality — written out
    // anyway because nothing in this file may assume which constructor built a
    // matrix, which is `Projection`'s whole point on the CPU side.
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0)
    {
        // Outside this cascade's box, or beyond its far plane. Lit: a shadow map
        // knows nothing about what it never rendered, and inventing a shadow
        // there is a black band across everything past the last split.
        return 1.0;
    }

    // Y-up NDC to top-left texture space, which is the flip `crcbl_render`'s
    // projections push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);

    // The offset is already in `ndc.z`, and it moved the reference *up* —
    // reversed-Z, so a larger depth is nearer the light, which is the direction
    // that stops a surface shadowing itself. Moving the world position along
    // `to_light` cannot move `tile_uv`: the projection's own depth axis is that
    // direction, so the sample stays in the texel it belonged to and only the
    // depth compared there changes. That is what makes this the same thing a
    // clip-space bias did, denominated in something that means a distance.
    return tile_pcf(cascade, tile_uv, ndc.z);
}

/// How far a punctual light's shadow comparison is moved towards the light, in
/// **tile texels** at the receiver.
///
/// # Why texels rather than depth
///
/// Acne is the shadow map quantising a receiver's own depth away across one
/// texel, so the error to cover is a *world distance*: the footprint of one
/// texel at the receiver, times how fast the surface climbs across it. Nothing
/// in that depends on how a projection distributes depth, and a bias denominated
/// in clip depth picks up the projection's range as a factor it has no business
/// carrying — which for the cascades meant tens of metres of `CASTER_REACH`
/// setting the size of a light leak. `sun_visibility` carries the same
/// denomination for the same reason, so the sun and the punctual lights are
/// biased in one unit.
///
/// A perspective map has a second reason on top of that one: under reversed-Z
/// its depth resolution piles up at the near plane, so a clip-depth constant
/// that stops acne on a receiver near the light is a shadow detached from its
/// caster on one far from it — not the same knob spelled differently, but a
/// different knob at every depth.
///
/// So the offset is applied to the **world position, before projecting**, which
/// makes it independent of the projection entirely, and it is denominated in the
/// footprint of one shadow-map texel at the receiver — which for a frustum of
/// half-angle `θ` at axial depth `d` is `2 d tan θ / SHADOW_TILE`, and for a
/// cascade of radius `r` is the constant `2 r / SHADOW_TILE`. A surface at slope
/// `s` to the light climbs `s` times that across one texel, which is the term
/// below.
///
/// **That is also why one pair of numbers covers a point light's faces as well
/// as a spot's cone**, whose fields of view are nothing like each other: a face
/// is a 90° frustum, so its `tan θ` is 1 and one of its texels covers a much
/// larger patch of world than a narrow spot's does — and the footprint is
/// computed rather than assumed, so a constant in texels means the same thing in
/// both. Each caller of `punctual_visibility` passes its own footprint; these
/// two say how many of them to move.
///
/// # Where the numbers came from, and what says they are right
///
/// **The observable is `crcbl`'s
/// `the_spot_shadow_scene_draws_the_same_frame_on_every_geometry_path`**, and
/// that is not the obvious one, so it is worth saying why it works. A device
/// with an amplification stage rejects the floor's clusters before they reach
/// the spot's map — its bounding radius is not scaled by the instance transform
/// and the floor is a scaled cube — so on that path the receiver is not in its
/// own shadow map at all and cannot self-shadow. On the indirect path it is. So
/// the two paths agree pixel for pixel exactly when the bias is enough, and
/// disagree by however much acne there is when it is not.
///
/// Measured on lavapipe at 256x192, with `Scene::SpotShadow`:
///
/// * `0` and `0`: the two paths differ over 4.13% of the frame, max channel
///   delta 88 — moiré across the whole lit pool.
/// * `0.5` and `1.0`: still 2.70%, max channel delta 12.
/// * `1.0` and `2.0`: they agree, and every golden holds.
///
/// The shipped pair is double the smallest that worked, for margin on hardware
/// this has not run on. The other end is checked by the golden rather than by a
/// number: too much bias detaches a shadow from its caster, and
/// `Scene::SpotShadow` stands its pyramid **on** the floor so the contact point
/// is in the frame. `Scene::PointShadow` stands both of its casters on the same
/// floor for the same reason, and its two geometry paths agree at these numbers
/// unchanged.
static const float PUNCTUAL_DEPTH_BIAS_TEXELS = 2.0;

/// The slope-scaled part, per unit of `tan(acos(N·L))`. See
/// `PUNCTUAL_DEPTH_BIAS_TEXELS` for how it was chosen.
static const float PUNCTUAL_SLOPE_BIAS_TEXELS = 4.0;

/// The fraction of a punctual light's map this fragment can see: `1.0` fully
/// lit, `0.0` fully shadowed, the values between the PCF filter.
///
/// The half a spot and a point light's face have in common, which is all of it
/// but two numbers: `tile` is the light tile to sample and `texel_world` the
/// world footprint of one of its texels at this receiver — see
/// `PUNCTUAL_DEPTH_BIAS_TEXELS`, which is what that footprint is multiplied by.
/// `to_light` is the unit vector towards the light and `n_dot_l` the unclamped
/// Lambert term, both already computed by the shading loop.
///
/// **`geometric_normal` drives the slope here for the same reason it does in
/// `sun_visibility`, and the consistency is deliberate.** Acne is a property of
/// the triangle that was rasterised into the map, and a punctual map rasterises
/// the same triangles the sun's does — so a light type reading a different normal
/// for the same surface would need a bias pair of its own for no reason anyone
/// could state. It moves no fixture the two constants above were set from:
/// `Scene::SpotShadow` and `Scene::PointShadow` receive on a flat floor, where
/// the facet and the shading normal are the same vector.
float punctual_visibility(uint tile, float3 world_position, float3 to_light, float n_dot_l,
                          float texel_world, float3 geometric_normal)
{
    float offset = texel_world
                 * (PUNCTUAL_DEPTH_BIAS_TEXELS
                    + PUNCTUAL_SLOPE_BIAS_TEXELS * shadow_slope(geometric_normal, to_light));

    float4 clip = mul(frame.light_view_proj[tile], float4(world_position + to_light * offset, 1.0));
    // **`w` first, and this is the difference from the cascades.** An
    // orthographic matrix has `w == 1` everywhere; a perspective one has `w`
    // equal to the depth in front of the light, so a receiver *behind* the light
    // gets a negative `w` and a divide that mirrors it into the frustum. Without
    // this, a surface behind a spot samples the map as though it were in front
    // of it.
    if (clip.w <= 0.0)
    {
        return 1.0;
    }
    float3 ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0 || ndc.z > 1.0)
    {
        // Outside this map's own frustum, nearer than its near plane or past its
        // far one. Lit, on `sun_visibility`'s terms: a map knows nothing about
        // what it never rendered. A cone's falloff has already closed by the
        // frustum's edge and a face's neighbour covers what it does not, so this
        // path costs no picture.
        return 1.0;
    }

    // Y-up NDC to top-left texture space, the flip `crcbl_render`'s projections
    // push into the viewport rather than into the matrix.
    float2 tile_uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    return tile_pcf(light_tile(tile), tile_uv, ndc.z);
}

/// The fraction of a shadowed spot this fragment can see.
///
/// `tile` is the light's `shadow_tile` and the caller has already checked it
/// names a real light tile. The rest is `punctual_visibility`'s.
float spot_visibility(GpuLight light, uint tile, float3 world_position, float3 to_light,
                      float n_dot_l, float3 geometric_normal)
{
    if (n_dot_l <= 0.0)
    {
        // Facing away: black from the diffuse term already, and the place
        // self-shadowing acne is worst. `sun_visibility`'s reason exactly.
        return 1.0;
    }

    // The cone's outer half-angle, as a tangent, out of the cosine the row
    // carries: `tan(acos(c)) == sqrt(1 - c²) / c`. `Light::row` refuses a
    // non-positive one on the way in and `Selection` refuses a cone at or past
    // 80°, so the divide has a real denominator.
    float cos_outer = light.direction.w;
    float tan_outer = sqrt(saturate(1.0 - cos_outer * cos_outer)) / max(cos_outer, 1e-4);
    // Depth along the cone's axis, which is what the projection's `w` will be —
    // computed here with one dot product because the offset has to be applied
    // *before* the matrix, and a radial distance would be up to `1 / cos_outer`
    // too large at the rim.
    float axial = max(dot(world_position - light.position.xyz, normalize(light.direction.xyz)), 0.0);
    return punctual_visibility(tile, world_position, to_light, n_dot_l,
                               2.0 * tan_outer * axial / SHADOW_TILE, geometric_normal);
}

/// Which of a point light's six faces `from_light` — the direction from the
/// light towards the surface — belongs to.
///
/// **The cube-map convention, and it is a convention rather than a derivation**:
/// the largest component of the direction picks the axis and its sign picks the
/// half, giving `+X, -X, +Y, -Y, +Z, -Z` as faces 0 to 5. The only thing that has
/// to agree with it is `crcbl_render::shadow::face_axis`, which builds the six
/// matrices in that same order — a shader picking face 3 for a direction the host
/// built face 2's matrix for samples a map of somewhere else entirely, and draws
/// a frame with shadows in it.
///
/// The ties — a direction exactly on a face diagonal — go to the earlier face,
/// and either answer is right there: the two faces' frusta meet at that edge, so
/// the point projects to the boundary of both.
uint point_face(float3 from_light)
{
    float3 axis = abs(from_light);
    if (axis.x >= axis.y && axis.x >= axis.z)
    {
        return from_light.x >= 0.0 ? 0u : 1u;
    }
    if (axis.y >= axis.z)
    {
        return from_light.y >= 0.0 ? 2u : 3u;
    }
    return from_light.z >= 0.0 ? 4u : 5u;
}

/// The fraction of a shadowed **point** light this fragment can see.
///
/// `base` is the light's `shadow_tile`: the first of the `SHADOW_POINT_FACES`
/// light tiles it holds, and the face selected below is added to it. The caller
/// has already checked that the whole run is inside the light region.
float point_visibility(GpuLight light, uint base, float3 world_position, float3 to_light,
                       float n_dot_l, float3 geometric_normal)
{
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    float3 from_light = world_position - light.position.xyz;
    // A face is a 90° frustum, so its half-angle's tangent is 1 and the axial
    // depth is the largest component of the offset — which is the same
    // `2 d tan θ / SHADOW_TILE` a spot computes, with both terms known rather
    // than read off the row.
    float axial = max(max(abs(from_light.x), abs(from_light.y)), abs(from_light.z));
    return punctual_visibility(base + point_face(from_light), world_position, to_light, n_dot_l,
                               2.0 * axial / SHADOW_TILE, geometric_normal);
}

/// How bright a punctual light is at `distance`, reaching **exactly zero** at
/// `radius`.
///
/// Inverse-square with Karis' quartic window on top. The window is not
/// cosmetic: `light_cluster.slang` culls a light against this same radius, so a
/// falloff still contributing there would leave a hard edge wherever a froxel
/// stopped listing it — the seam that reads as a shading bug. Zero at the radius
/// is what makes the cull and the shading the same statement.
///
/// The `1` in the denominator keeps the singularity at zero distance finite;
/// it is the standard form, and it costs a light its physical units in exchange
/// for never producing an infinity a tonemap has to survive.
float punctual_falloff(float distance, float radius)
{
    float ratio = distance / max(radius, 1e-6);
    float window = saturate(1.0 - ratio * ratio * ratio * ratio);
    return window * window / (distance * distance + 1.0);
}

/// How much of a spot light this direction is inside its cone for: `1` within
/// the inner angle, `0` outside the outer one, smooth between.
///
/// `to_light` points at the light and `axis` points along the cone, so the
/// cosine wanted is of the angle between the axis and the direction *away* from
/// the light — hence the negation, which is the sign error this whole engine
/// spells out rather than leaves to be rediscovered.
float spot_cone(float3 to_light, float3 axis, float cos_outer, float cos_inner)
{
    float cosine = dot(-to_light, normalize(axis));
    // `max` rather than a guard: a caller writing the two cosines the wrong way
    // round would otherwise divide by a negative and light the outside of the
    // cone. `crcbl_render::SpotLight` refuses that pairing on the way in; this
    // is the shader-side half of the same statement.
    return saturate((cosine - cos_outer) / max(cos_inner - cos_outer, 1e-4));
}

/// Which froxel a fragment at `pixel` and view depth `depth` belongs to.
///
/// **The depth is `clip.w`, not the depth buffer.** Under reversed-Z the buffer
/// runs 1.0 at the near plane to 0.0 at infinity, so an index taken from
/// `SV_Position.z` would run backwards *and* be hyperbolically distributed — a
/// numbering that looks plausible and puts every fragment in the wrong list.
///
/// The slice mapping is the inverse of `light_cluster.slang`'s exponential
/// split, and both are written from the same four constants rather than from a
/// scale and a bias passed between them; `crcbl_shaders::light`'s
/// `the_slice_mapping_round_trips` is what says the pair really are inverses.
uint froxel_of(float2 pixel, float depth)
{
    uint grid_x = max(frame.cluster_grid.x, 1u);
    uint grid_y = max(frame.cluster_grid.y, 1u);
    uint slices = max(frame.cluster_grid.z, 1u);
    uint tile_pixels = max(frame.cluster_grid.w, 1u);

    // Clamped as well as divided: the grid is `ceil(extent / tile)` wide, so the
    // last column and row are part-empty and a pixel in the part that is not
    // there would index the next row's froxel.
    uint tile_x = min(uint(pixel.x) / tile_pixels, grid_x - 1);
    uint tile_y = min(uint(pixel.y) / tile_pixels, grid_y - 1);

    float scale = float(CLUSTER_DEPTH_SLICES) / log2(CLUSTER_FAR / CLUSTER_NEAR);
    float bias = -scale * log2(CLUSTER_NEAR);
    // Clamped *before* the logarithm as well as after: a fragment on the eye
    // plane has zero depth, and `log2(0)` is an infinity whose conversion to an
    // integer is undefined rather than merely wrong.
    float slice = floor(log2(max(depth, CLUSTER_NEAR)) * scale + bias);
    uint index = uint(clamp(slice, 0.0, float(slices - 1)));

    return (index * grid_y + tile_y) * grid_x + tile_x;
}

/// One row of the probe table, by grid cell, clamped into the buffer.
///
/// The clamp is against `probe_counts.w` — the product the host wrote — for the
/// reason `ambient_occlusion`'s `GetDimensions` clamp exists: it makes the fetch
/// a fact about the buffer rather than a promise about whoever filled the
/// header. `max(total, 1)` is what keeps the empty grid on row 0, which every
/// table has because a buffer of no bytes is not a buffer.
GpuProbe probe_at(uint3 cell)
{
    uint index = (cell.z * frame.probe_counts.y + cell.y) * frame.probe_counts.x + cell.x;
    return probes[min(index, max(frame.probe_counts.w, 1u) - 1u)];
}

/// Two probes blended coefficient by coefficient — one axis of the trilinear
/// interpolation below.
GpuProbe lerp_probe(GpuProbe a, GpuProbe b, float t)
{
    GpuProbe blended;
    blended.sh_r = lerp(a.sh_r, b.sh_r, t);
    blended.sh_g = lerp(a.sh_g, b.sh_g, t);
    blended.sh_b = lerp(a.sh_b, b.sh_b, t);
    return blended;
}

/// The irradiance `docs/plan/18-render-features.md`'s probe grid gives a surface
/// at `world_position` facing `normal`, in linear RGB.
///
/// Mirrored on the host by `crcbl_shaders::probe::irradiance_at`, which is where
/// the spherical harmonics are checked against the literature they come from.
///
/// # A grid of nothing evaluates to zero, and there is no branch
///
/// A scene with no probes carries counts of zero and a reciprocal spacing of
/// zero. Then `last` is zero on every axis, the grid coordinate is zero, both
/// corners of every axis are probe 0, and that row is the zeroed one every table
/// starts as — so the three dot products are zero and `frame.ambient.rgb + 0.0`
/// is `frame.ambient.rgb` **exactly**, on every target. That is what let the
/// grid land with no golden re-blessed.
///
/// # Interpolated coefficients, not interpolated evaluations
///
/// The eight corners are blended as *coefficients* and evaluated once, so the
/// whole lookup is seven blends, three dot products and one clamp whatever the
/// grid's size — where evaluating each corner first would be twenty-four dot
/// products. It is also the more correct of the two: irradiance is linear in the
/// coefficients until the clamp, and clamping once at the end is clamping the
/// interpolated field rather than eight samples of it.
///
/// The clamp itself is not defensive. L1 is a truncated series and a truncated
/// series rings: an environment concentrated in one direction evaluates negative
/// on the far side, and a surface there would *subtract* light.
///
/// # Outside the grid clamps to its surface
///
/// The same choice the shadow sampler makes at a cascade's edge: an environment
/// does not stop at the last probe, and fading to zero there would put a seam in
/// mid-air. What it costs is that a room's lighting extends past its probes,
/// which is a gradient rather than an artefact.
float3 probe_irradiance(float3 world_position, float3 normal)
{
    // The last addressable probe on each axis. `max(…, 0)` is what keeps an axis
    // with no probes — or with exactly one — addressing probe 0 rather than
    // probe -1.
    float3 last = max(float3(frame.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    float3 grid = clamp((world_position - frame.probe_origin.xyz) * frame.probe_inv_spacing.xyz,
                        float3(0.0, 0.0, 0.0), last);
    float3 base = floor(grid);
    float3 f = grid - base;

    uint3 lo = uint3(base);
    // `min` against the axis's last probe rather than `+ 1` outright: at the far
    // face there is no next probe, and `f` is zero there, so the corner is
    // weighed out rather than wrapped around.
    uint3 hi = uint3(min(base + 1.0, last));

    // Seven blends over the coefficients. The far corner's weight reaches
    // exactly zero at the boundary where the cell index changes — `lerp(a, b,
    // 0.0)` is `a` — so a fragment landing in cell `i` at `f≈1` and one landing
    // in cell `i+1` at `f≈0` compute the same value, which is the whole of this
    // grid's determinism argument.
    GpuProbe x00 = lerp_probe(probe_at(uint3(lo.x, lo.y, lo.z)), probe_at(uint3(hi.x, lo.y, lo.z)), f.x);
    GpuProbe x10 = lerp_probe(probe_at(uint3(lo.x, hi.y, lo.z)), probe_at(uint3(hi.x, hi.y, lo.z)), f.x);
    GpuProbe x01 = lerp_probe(probe_at(uint3(lo.x, lo.y, hi.z)), probe_at(uint3(hi.x, lo.y, hi.z)), f.x);
    GpuProbe x11 = lerp_probe(probe_at(uint3(lo.x, hi.y, hi.z)), probe_at(uint3(hi.x, hi.y, hi.z)), f.x);
    GpuProbe y0 = lerp_probe(x00, x10, f.y);
    GpuProbe y1 = lerp_probe(x01, x11, f.y);
    GpuProbe cell = lerp_probe(y0, y1, f.z);

    float4 basis = float4(normal, 1.0);
    return max(float3(dot(cell.sh_r, basis), dot(cell.sh_g, basis), dot(cell.sh_b, basis)),
               float3(0.0, 0.0, 0.0));
}

/// The forward pass's two colour attachments, one field per target.
///
/// This was a bare `float4` until `docs/plan/18-render-features.md`'s
/// screen-space reflections, which run after this pass and therefore have only
/// the depth buffer and the scene colour to work from — neither of which says
/// which surfaces reflect or how sharply. **Both of the numbers that do are
/// already computed below** for the GGX lobe and were being thrown away, so what
/// the second target costs is a write rather than an evaluation.
///
/// # One entry point, still
///
/// A struct return is not a second fragment stage: `fragmentMain` is named by
/// the vertex pipeline and by the mesh-shader pipeline alike, and both gained
/// one element in the same `ColorTargetState` array. That is the design's own
/// claim in `docs/plan/18-render-features.md` — the AO section's refusal of an
/// attachment was about the depth *prepass*, which has no fragment stage and no
/// colour target at all — and it holds here rather than being assumed.
struct FragmentOutput
{
    /// The shaded colour, into the `Rgba16Float` scene target.
    float4 lit : SV_Target0;
    /// The reflectivity channel, into an `Rgba8Unorm` target: `rgb` is `F0` and
    /// `a` is SSR's already-evaluated sharpness ramp.
    ///
    /// The ramp is stored rather than roughness: `Rgba8Unorm` cannot preserve the
    /// cutoff exactly, while its zero endpoint does. `ssr.slang` therefore reads
    /// alpha directly and a material at the cutoff skips marching on every
    /// backend.
    float4 reflectivity : SV_Target1;
};

[shader("fragment")]
FragmentOutput fragmentMain(VertexOutput input)
{
    // Interpolation denormalises, so both of these are renormalised per
    // fragment. Skipping it makes large triangles darken towards their centres.
    float3 normal = normalize(input.world_normal);
    float3 to_eye = normalize(frame.camera_position.xyz - input.world_position);
    // The facet under the shading normal, taken once for the fragment and read
    // by the shadow biases alone — never by the shading, which is the authored
    // normal's job. `geometric_normal_of` says why it cannot live inside them.
    float3 geometric_normal = geometric_normal_of(input.world_position, normal);

    // The material row, resolved here rather than at the corners. `material` is
    // flat, so every fragment of a primitive reads the row its instance named.
    GpuMaterial material = materials[input.material];
    // Both halves of §3.2's "texture indices + factors", multiplied together.
    // The sample is why the lookup had to move to this stage at all: a texture
    // fetch is not constant across a primitive, so nothing it feeds could have
    // been folded into a varying at the corners.
    //
    // The texel arrives **linear** — the page is an sRGB-format image, so the
    // sampler decoded it — which is the space `base_color` and `input.color`
    // are in. See the binding above.
    float4 texel = base_color_textures.Sample(
        base_color_sampler, float3(input.uv, float(material.base_color_texture)));
    float4 albedo = input.color * material.base_color * texel;

    // The row's two shading factors, clamped once for the fragment rather than
    // once per light. `roughness` is squared into GGX's `alpha` — the
    // perceptual parameterisation glTF authors in — and `alpha` is squared
    // again inside the lobe, so this is the one place the chain starts.
    float metallic = saturate(material.metallic);
    float roughness = clamp(material.roughness, MIN_ROUGHNESS, 1.0);
    float alpha = roughness * roughness;
    float alpha2 = alpha * alpha;

    // **A conductor has no diffuse lobe**: what a metal does with light is
    // reflect it, and its reflectance is coloured where a dielectric's is the
    // flat `DIELECTRIC_F0`. Both halves are one interpolation each, and the
    // pair is the whole of what `metallic` means.
    float3 f0 = lerp(float3(DIELECTRIC_F0, DIELECTRIC_F0, DIELECTRIC_F0), albedo.rgb, metallic);
    float3 diffuse_albedo = albedo.rgb * (1.0 - metallic);

    // Constant across the fragment's lights, so it is taken once. Clamped off
    // zero because it is a factor of the visibility term's denominator: a
    // surface exactly edge-on to the eye is a fragment the rasteriser can still
    // hand over, and its lobe has to be a finite number.
    float n_dot_v = max(dot(normal, to_eye), 1e-4);

    // This fragment's own froxel, and the only lights it will look at. The view
    // depth is recomputed from the frame's matrix rather than read off
    // `input.position.w`: the four targets this file compiles to have twice
    // disagreed about what a value the rasteriser hands the fragment stage
    // means — see the header on `SV_VertexID` — and `SV_Position`'s `w` in a
    // fragment is a reciprocal on some of them and not others. A matrix row is
    // the same arithmetic everywhere.
    float view_depth = mul(frame.view_proj, float4(input.world_position, 1.0)).w;
    uint base = froxel_of(input.position.xy, view_depth) * CLUSTER_STRIDE;
    // Clamped against the budget as well as read from the buffer: the count is
    // written by a pass that already refuses to exceed it, and this is what
    // stops a corrupt or never-written grid from walking into the next froxel's
    // list.
    uint count = min(cluster_lights[base], CLUSTER_LIGHT_CAPACITY);

    // Accumulated rather than branched on, which is the whole of what making the
    // sun a row bought: one loop shades a directional, a point and a spot, and
    // topic 18's "one material model, one BRDF" holds by construction instead of
    // by three copies of the same maths agreeing.
    float3 direct = float3(0.0, 0.0, 0.0);
    float3 gloss = float3(0.0, 0.0, 0.0);
    for (uint slot = 0; slot < count; ++slot)
    {
        GpuLight light = lights[cluster_lights[base + 1 + slot]];

        float3 to_light;
        // What scales this light's direct terms: the shadow map for the sun, the
        // distance falloff and cone for a punctual light. One name for both, so
        // the two arrive at the same two multiplies below.
        float reach;
        if (light.kind == KIND_DIRECTIONAL)
        {
            to_light = normalize(light.direction.xyz);
            reach = 1.0;
        }
        else
        {
            float3 offset = light.position.xyz - input.world_position;
            float distance = length(offset);
            to_light = offset / max(distance, 1e-6);
            reach = punctual_falloff(distance, light.position.w);
            if (light.kind == KIND_SPOT)
            {
                reach *= spot_cone(to_light, light.direction.xyz, light.direction.w, light.cos_inner);
            }
        }

        // Lambert.
        float n_dot_l = dot(normal, to_light);
        float diffuse = max(n_dot_l, 0.0);

        // Cook-Torrance, one GGX lobe, driven by the row's own two factors —
        // where this was a Blinn power and a strength constant no caller could
        // vary. The half-vector form survives the change: it is what the
        // microfacet model is written in, rather than an approximation of it.
        float3 half_vector = normalize(to_light + to_eye);
        float3 specular = ggx_lobe(alpha2, f0, diffuse, n_dot_v,
                                   max(dot(normal, half_vector), 0.0),
                                   max(dot(to_eye, half_vector), 0.0));
        // No light on the back of a surface, whatever the half-vector says. The
        // `N·L` the lobe left out is the same one Lambert already carries, so
        // both terms take it from here and a back-facing fragment loses both.
        specular *= diffuse;

        // The shadow maps, and the *only* place they enter the shading. The sun
        // occludes through its cascades, a spot through the one tile `Selection`
        // gave it and a point light through whichever of its six the surface is
        // on; a light with no tile — every light past the budget — keeps the
        // `reach` its falloff and cone already produced, which is
        // `docs/plan/18-render-features.md`'s "still lights, just does not
        // occlude" holding by construction.
        //
        // The sun *replaces* `reach` where a spot *multiplies* it, and the
        // asymmetry is not an oversight: a directional light has no falloff and
        // no cone, so its `reach` is the constant 1.0 set above and the two
        // spellings are the same arithmetic. A spot's is the product of its
        // window and its cone, and the shadow is a third factor on it.
        if (light.kind == KIND_DIRECTIONAL)
        {
            reach = sun_visibility(input.world_position, to_light, n_dot_l, geometric_normal);
        }
        else if (light.kind == KIND_POINT)
        {
            // **The whole run has to be inside the region, not just its first
            // tile**: a point light's faces are `shadow_tile .. + 6`, so a row
            // whose base sat in the last five tiles would read a matrix past the
            // end of the array. `Selection` never emits one; this is what makes
            // that a fact about the shader rather than a promise about the host.
            //
            // Written as a subtraction on the two constants rather than as
            // `shadow_tile + SHADOW_POINT_FACES <= SHADOW_LIGHT_TILES`, because
            // `NO_SHADOW_TILE` is `0xffffffff` and that sum wraps — an unshadowed
            // point light would pass the test and sample tile 5.
            if (light.shadow_tile <= SHADOW_LIGHT_TILES - SHADOW_POINT_FACES)
            {
                reach *= point_visibility(light, light.shadow_tile, input.world_position, to_light,
                                          n_dot_l, geometric_normal);
            }
        }
        else if (light.shadow_tile < SHADOW_LIGHT_TILES)
        {
            reach *= spot_visibility(light, light.shadow_tile, input.world_position, to_light,
                                     n_dot_l, geometric_normal);
        }
        diffuse *= reach;
        specular *= reach;

        direct += light.color.rgb * diffuse;
        gloss += light.color.rgb * specular;
    }

    // **Screen-space occlusion scales the ambient term and nothing else**, and
    // the refusal is as load-bearing as the multiply. Ambient stands in for every
    // bounce the direct terms do not carry, so how much of the surrounding room a
    // surface can see is exactly what modulates it. Applying the same factor to
    // `direct` would darken light that has a *measured* path to the surface —
    // already occluded by the shadow maps above — and applying it to `gloss`
    // would dim a highlight, which is a mirror image of the light rather than of
    // the room. Multiplying the tonemap pass's input instead is the same mistake
    // one pass later and is refused in `docs/plan/18-render-features.md` in
    // writing, because the one-line feature row invites it.
    //
    // **The fetch is clamped to the image's extent**, which is what makes the
    // renderer's 1×1 white off-switch read as "nothing occludes" instead of as
    // total occlusion — the binding's own declaration argues that at length. It
    // costs nothing on a frame-sized channel, where every fragment is inside the
    // image already.
    uint occlusion_width;
    uint occlusion_height;
    ambient_occlusion.GetDimensions(occlusion_width, occlusion_height);
    int2 occlusion_texel = min(int2(input.position.xy),
                               int2(occlusion_width, occlusion_height) - 1);
    float occluded = ambient_occlusion.Load(int3(occlusion_texel, 0));

    // Ambient is not the sun and keeps its full value in shadow, which is what
    // stops a shadowed surface from being black. The association here is the one
    // the single-light form had — the sum starts at zero and `0 + x` is `x`
    // exactly — so converting the sun from a field to a row is arithmetically a
    // no-op and the goldens are what say so.
    //
    // `docs/plan/18-render-features.md`'s irradiance grid, **added** to the flat
    // term rather than replacing it: the sum starts at the ambient and a grid of
    // nothing contributes exactly zero, so the whole feature is data and there
    // is no branch anywhere selecting it. An author who wants the probes to be
    // the whole of the environment sets `DirectionalLight::ambient` to zero.
    float3 irradiance = frame.ambient.rgb + probe_irradiance(input.world_position, normal);

    // **The ambient scales the diffuse albedo, so a metal has no ambient
    // term** — and that is the model rather than an omission. A conductor
    // reflects its surroundings instead of scattering them, so what it owes the
    // room is a *reflection*, and `ssr.slang` is what supplies it: the probe
    // grid above is the diffuse half of one design, an `ssr.slang` miss returns
    // those same probes as the specular half, and `ssr_blur.slang` is what adds
    // the result on top of this pass's output. So a fully metallic surface out
    // of every light's reach is the environment rather than black, and black
    // only under a zeroed probe volume with nothing on screen for it to
    // reflect. `apps/lumen/src/room.rs`'s `MIRROR` and `ROUGH_METAL` rows are
    // conductors, and `apps/lumen/tests/golden.rs`'s
    // `zero_probes_only_remove_the_ssr_and_rough_fallbacks` is what measures
    // those two shares apart on them.
    //
    // **Occlusion scales the probes too**, and that is the same statement as
    // above rather than a new one: how much of the surrounding room a surface
    // can see is what modulates the environment reaching it, and the grid is an
    // environment.
    float3 lit = diffuse_albedo * (irradiance * occluded + direct) + gloss;

    FragmentOutput output;
    // Deliberately **not** clamped: the target is `Rgba16Float` and the tonemap
    // pass is what maps this into the swapchain. A `saturate` here would throw
    // away the very range HDR-from-P1 exists to carry — see `tonemap.slang`.
    output.lit = float4(lit, albedo.a);
    // `roughness` is the clamped lobe input. Store SSR's already-evaluated
    // sharpness ramp: its zero endpoint survives `Rgba8Unorm` exactly, unlike
    // the cutoff roughness it replaces.
    float sharpness = saturate(1.0 - roughness / ROUGHNESS_CUTOFF);
    output.reflectivity = float4(f0, sharpness);
    return output;
}
