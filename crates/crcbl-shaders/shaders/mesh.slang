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
    /// World-space direction **towards** the light in `xyz`, normalized; `w`
    /// unused. Towards rather than "the direction light travels" because that is
    /// the vector both the Lambert and the Blinn half-vector terms want, and
    /// negating it in the shader is a sign error waiting to happen.
    float4 light_direction;
    /// Light colour premultiplied by intensity in `rgb`; `w` unused.
    float4 light_color;
    /// Flat ambient term in `rgb`; `w` unused. A real irradiance probe is P7.
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
    /// `x`: one shadow-atlas texel in `u`. `y`: one texel in `v`. `z`: the
    /// constant depth bias. `w`: the slope-scaled coefficient the bias adds on
    /// top of it, per unit of `tan(acos(N·L))`.
    ///
    /// The two texel sizes are not equal: the atlas is `SHADOW_CASCADES` square
    /// tiles side by side, so `x` is `1 / (tile * cascades)` and `y` is
    /// `1 / tile`. Deriving one from the other is how the PCF kernel ends up
    /// sampling a rectangle.
    float4 shadow_params;
    /// `docs/plan/25-lod.md`'s selection numbers. `x`: how many pixels one unit
    /// of length subtends one unit from the eye. `y`: the pixel budget a group's
    /// projected error is compared against. `z`, `w`: unread padding `std140`'s
    /// sixteen-byte `float4` alignment requires.
    ///
    /// **Declared here and read by nothing**: no stage in this file selects a
    /// level. It is here because this block and `mesh_cluster.slang`'s must be
    /// the same bytes — that file's amplification stage is the one that reads it,
    /// out of the same buffer this one's vertex stage reads `view_proj` from.
    float4 lod_params;
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
    /// Pads the row to 32 bytes, which is what `std430` rounds it to anyway:
    /// the struct's alignment is the `float4`'s 16, so a 20-byte row would be
    /// read at a stride of 32 by a shader while the CPU wrote it at 20. Spelled
    /// out rather than implied, exactly as `DrawConstants` below spells its own.
    uint pad0;
    uint pad1;
    uint pad2;
};

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

/// Blinn-Phong specular exponent.
///
/// One constant rather than a material column, **even now that there is a table
/// to put it in**: `docs/plan/02-vulkan-backend.md` §2.5 rung 4 is "enough to
/// see geometry properly; real material model comes with stage 3/5", so how
/// specular is parameterised at all is part of the shading model
/// `docs/plan/37-materials.md` owns. `GpuMaterial` carries what §3.2 asks a
/// material table to carry and nothing a caller would not vary.
static const float SPECULAR_POWER = 32.0;

/// How much of the light colour the specular lobe contributes.
static const float SPECULAR_STRENGTH = 0.35;

/// Ceiling on the slope-scaled part of the depth bias, in shadow-clip depth.
///
/// `tan(acos(N·L))` runs to infinity as a surface turns edge-on to the light,
/// and an unbounded bias there is peter-panning — the contact point of a shadow
/// slides away from its caster. Bounded rather than removed because the same
/// term is what keeps a *steeply* lit surface free of acne; the clamp is the
/// standard pairing.
static const float SHADOW_SLOPE_BIAS_CLAMP = 5.0;

/// The fraction of the sun this fragment can see: `1.0` fully lit, `0.0` fully
/// shadowed, and the values between are the PCF filter.
///
/// `n_dot_l` is the *unclamped* Lambert term. A surface facing away from the
/// light is already black from the diffuse term, and asking the shadow map about
/// it is both wasted work and the place self-shadowing acne is worst, so it
/// returns early — the caller multiplies the result into a diffuse term that is
/// zero there anyway.
float sun_visibility(float3 world_position, float n_dot_l)
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

    float4 clip = mul(frame.shadow_view_proj[cascade], float4(world_position, 1.0));
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
    float2 texel = frame.shadow_params.xy;

    // Slope-scaled bias: `tan(acos(x)) == sqrt(1 - x²) / x`, computed that way
    // rather than through two transcendentals.
    float cosine = saturate(n_dot_l);
    float slope = sqrt(saturate(1.0 - cosine * cosine)) / max(cosine, 1e-4);
    float bias = frame.shadow_params.z
               + frame.shadow_params.w * min(slope, SHADOW_SLOPE_BIAS_CLAMP);
    // **Added, not subtracted.** Reversed-Z: a larger depth is nearer the
    // light, so moving the receiver's reference *up* is moving it towards the
    // light, which is the direction that stops a surface shadowing itself.
    float reference = ndc.z + bias;

    // 3×3 PCF, in tile space so a tap never leaves this cascade for its
    // neighbour's. The clamp is half a texel inside the tile: the comparison
    // sampler filters a 2×2 neighbourhood around each tap, so a tap exactly on
    // the edge would still reach across it.
    float2 tile_min = float2(0.5, 0.5) * texel * float2(float(SHADOW_CASCADES), 1.0);
    float visibility = 0.0;
    for (int y = -1; y <= 1; ++y)
    {
        for (int x = -1; x <= 1; ++x)
        {
            // The offset is applied in *tile* space and the tile is
            // `1 / SHADOW_CASCADES` of the atlas wide, so the u step is the
            // atlas texel scaled back up — one shadow-map texel either way.
            float2 offset = float2(float(x), float(y)) * texel
                          * float2(float(SHADOW_CASCADES), 1.0);
            float2 tap = clamp(tile_uv + offset, tile_min, 1.0 - tile_min);
            float2 atlas_uv =
                float2((float(cascade) + tap.x) / float(SHADOW_CASCADES), tap.y);
            visibility += shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv, reference);
        }
    }
    return visibility / 9.0;
}

[shader("fragment")]
float4 fragmentMain(VertexOutput input) : SV_Target
{
    // Interpolation denormalises, so both of these are renormalised per
    // fragment. Skipping it makes large triangles darken towards their centres.
    float3 normal = normalize(input.world_normal);
    float3 to_light = normalize(frame.light_direction.xyz);
    float3 to_eye = normalize(frame.camera_position.xyz - input.world_position);

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

    // Lambert.
    float n_dot_l = dot(normal, to_light);
    float diffuse = max(n_dot_l, 0.0);

    // Blinn: the half-vector form, not Phong's reflection vector. Cheaper, and
    // it is what every later specular model in this engine is a refinement of.
    float3 half_vector = normalize(to_light + to_eye);
    float specular = pow(max(dot(normal, half_vector), 0.0), SPECULAR_POWER);
    // No light on the back of a surface, whatever the half-vector says.
    specular *= step(0.0, diffuse) * diffuse;

    // The cascaded shadow map, and the *only* place it enters the shading.
    // Topic 18's rule that both lighting paths share one BRDF is what makes it
    // one multiplier on the sun's direct terms rather than a branch: ambient is
    // not the sun and keeps its full value in shadow, which is what stops a
    // shadowed surface from being black.
    float visibility = sun_visibility(input.world_position, n_dot_l);
    diffuse *= visibility;
    specular *= visibility;

    float3 lit = albedo.rgb * (frame.ambient.rgb + frame.light_color.rgb * diffuse)
               + frame.light_color.rgb * (specular * SPECULAR_STRENGTH);

    // Deliberately **not** clamped: the target is `Rgba16Float` and the tonemap
    // pass is what maps this into the swapchain. A `saturate` here would throw
    // away the very range HDR-from-P1 exists to carry — see `tonemap.slang`.
    return float4(lit, albedo.a);
}
