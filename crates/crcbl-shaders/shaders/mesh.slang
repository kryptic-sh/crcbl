// crcbl-targets: spirv, wgsl, msl, dxil

// Milestones 3 and 4 of `docs/plan/02-vulkan-backend.md`'s ladder: a
// depth-tested mesh with a perspective camera in a uniform buffer, lit by a
// clustered light list.
//
// **The shading model is physically based, not the rung's Blinn-Phong.** Rung 4
// asks for "Lambert+Blinn — enough to see geometry properly; real material model
// comes with stage 3/5", and the real one arrived: Lambert diffuse plus one
// Cook-Torrance GGX lobe — Trowbridge-Reitz `D`, Smith height-correlated `V`,
// Schlick `F` — driven by a glTF metallic-roughness material row. `ggx_lobe` is
// the lobe and carries the derivation; `GpuMaterial` is the row. Blinn's
// exponent-and-strength pair is what this replaced and appears in this file only
// where a comment says so.
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
// is a single `Texture2DArray` and `base_color_layer(material)` names a layer
// of it — as `normal_textures` and `normal_layer(material)` are the same pair
// for §2's normal page. Only one of the two models is here, and the reason is that only
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

// The vertex pool's two streams, and the arithmetic that reads one back.
//
// `docs/plan/43-render-standards.md` §2's 2026-08-30 layout: the pool is one
// storage buffer holding `POSITION_WORDS` words of position for every vertex
// and then `ATTRIBUTE_WORDS` words of attributes for every vertex, and
// `FrameUniforms::vertex_pool` says where the second region starts. Everything
// below is spelled character for character in `mesh_cluster.slang` and — the
// pure functions — in `skinning.slang`, because there is no shared header and
// the compile script hashes one source per artifact;
// `crcbl_shaders::mesh::every_shader_decodes_a_vertex_the_same_way` is what
// holds the copies equal.
//
// A drift here is not a compile error anywhere: every file builds, and the
// shaders simply read the same bytes as different numbers.

/// Words one vertex spends in the pool's **position** region, matching
/// `crcbl_shaders::mesh::POSITION_STRIDE`.
static const uint POSITION_WORDS = 3;

/// Words one vertex spends in the pool's **attribute** region, matching
/// `crcbl_shaders::mesh::ATTRIBUTE_STRIDE`.
static const uint ATTRIBUTE_WORDS = 5;

/// An orthonormal tangent frame — the three vectors a QTangent stands in for,
/// matching `crcbl_shaders::vertex::TangentFrame`.
struct TangentFrame
{
    float3 tangent;
    float3 bitangent;
    float3 normal;
};

/// One `snorm16` lane read back the way `crcbl_shaders::vertex`'s
/// `dequantise_snorm` does: `n / 32767`, clamped at the one end the division
/// can pass, which is what makes both `+1` and `-1` exactly representable.
float dequantise_snorm(int lane)
{
    return max(float(lane) / 32767.0, -1.0);
}

/// The four `snorm16` lanes of a packed pair of words, low lane first.
float4 unpack_snorm16x4(uint low, uint high)
{
    return float4(dequantise_snorm(asint(low << 16) >> 16),
                  dequantise_snorm(asint(low) >> 16),
                  dequantise_snorm(asint(high << 16) >> 16),
                  dequantise_snorm(asint(high) >> 16));
}

/// The two `unorm16` lanes of one word, low lane first.
float2 unpack_unorm16x2(uint word)
{
    return float2(float(word & 0xffffu), float(word >> 16)) / 65535.0;
}

/// One `rgba8` word as linear floats, red in the low byte.
float4 unpack_rgba8(uint word)
{
    return float4(float(word & 0xffu),
                  float((word >> 8) & 0xffu),
                  float((word >> 16) & 0xffu),
                  float(word >> 24)) / 255.0;
}

/// `q v q⁻¹` for a unit `q`, in the two cross products that form costs rather
/// than the two quaternion multiplications it is written as —
/// `crcbl_shaders::vertex`'s `rotate`, term for term.
float3 rotate_by(float4 q, float3 v)
{
    float3 t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

/// The frame a QTangent's four lanes stand for, mirroring
/// `crcbl_shaders::vertex::QTangent::decode`.
///
/// Renormalised first: four independently rounded lanes are not a unit
/// quaternion, and rotating by one that is not scales every vector it produces.
/// The bitangent is `handedness * cross(normal, tangent)` rather than a third
/// rotation of `(0, 1, 0)`, because the quaternion was extracted from the
/// right-handed frame either way and only this recovers a left-handed one.
TangentFrame decode_qtangent(float4 lanes)
{
    float4 q = normalize(lanes);
    TangentFrame basis;
    basis.tangent = rotate_by(q, float3(1.0, 0.0, 0.0));
    basis.normal = rotate_by(q, float3(0.0, 0.0, 1.0));
    basis.bitangent = cross(basis.normal, basis.tangent) * (lanes.w < 0.0 ? -1.0 : 1.0);
    return basis;
}

/// One vertex, decoded out of the pool's two regions.
///
/// Not the memory layout: the pool holds `POSITION_WORDS` words of position for
/// every vertex and then `ATTRIBUTE_WORDS` words of attributes for every
/// vertex, and `load_vertex` is what turns a pair of those into this.
struct MeshVertex
{
    float3 position;
    TangentFrame basis;
    float2 uv0;
    float4 color;
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
/// shader has any use for. The region holds two point cubes and two spots, which
/// is what the 2026-08-26 re-tiling of the atlas bought without growing it.
static const uint SHADOW_LIGHT_TILES = 14;

/// Tiles one point light's shadow map is, matching
/// `crcbl_shaders::mesh::SHADOW_POINT_FACES`.
///
/// Six atlas tiles rather than a cube map — one image, one sampler, one barrier
/// story — and `point_face` below is the convention that says which is which.
static const uint SHADOW_POINT_FACES = 6;

/// Slots the shadow atlas has, matching
/// `crcbl_shaders::mesh::SHADOW_ATLAS_TILES`.
///
/// The cascades and the light region together, which is what sizes
/// `FrameUniforms::shadow_atlas_rect` — so this is a layout number and the same
/// drift test that pins `SHADOW_CASCADES` pins it.
static const uint SHADOW_ATLAS_TILES = 16;

/// How many clipmap levels the irradiance volume's header carries room for,
/// matching `crcbl_shaders::probe::PROBE_LEVELS`.
///
/// `docs/plan/50-irradiance-probes.md`'s layered density: a few concentric
/// levels of one grid, each twice the last, so a fragment near the middle reads
/// dense probes and one far out reads sparse ones. Sizes
/// `FrameUniforms::probe_level_origin` and its reciprocal twin, so this is a
/// layout number and the same drift test that pins `SHADOW_CASCADES` pins it.
static const uint PROBE_LEVELS = 4;

/// The depth one tile of the shadow atlas is reset to before it is redrawn,
/// matching `crcbl_shaders::mesh::SHADOW_ATLAS_CLEAR_DEPTH`.
///
/// The reversed-Z far plane, which is what `crcbl_hal::depth::CLEAR` is and what
/// the pass loads the whole attachment with on a frame that redraws every tile.
/// `depthClearVertexMain` is what writes it over one tile on a frame that does
/// not, and the two have to be the same value or a held tile and a cleared tile
/// would answer differently where nothing was drawn.
static const float SHADOW_ATLAS_CLEAR_DEPTH = 0.0;

/// Which atlas tile light tile `tile` of the light region was rendered into.
///
/// The cascades take tiles `0 .. SHADOW_CASCADES` and the lights take the rest,
/// which is the same arithmetic `crcbl_render::shadow::light_tile` performs on
/// the writing side. One statement of the split, read from both ends.
uint light_tile(uint tile)
{
    return SHADOW_CASCADES + tile;
}

/// Where `tile_uv` — a point in the map at `rect`, `0..1` on both axes — lands
/// in the atlas as a whole.
///
/// A scale and an offset, so this knows nothing about how the atlas is divided
/// and a map of any size addresses through the same two instructions. That is
/// `docs/plan/45-shadows.md`'s atlas rung: the cascades and the light maps share
/// one sampling path whatever size their tiles are, where a grid derived from an
/// index could only ever produce one size.
float2 atlas_uv(float4 rect, float2 tile_uv)
{
    return rect.zw + tile_uv * rect.xy;
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
    /// Flat ambient term in `rgb`. The irradiance grid below is **added** to it
    /// rather than replacing it.
    ///
    /// **Ambient is not a light and is not a row**: it stands in for every
    /// bounce the direct terms do not carry, so it has no position, no cluster
    /// and no shadow. The sun used to sit beside it here as a direction and a
    /// colour, and it is a row in `lights` now — see that binding.
    ///
    /// **`w` is the normals view's switch** — `NORMALS_VIEW` below — and this is
    /// the lane it rides in because it is the only spare one every writer of this
    /// block already leaves at exactly zero. `camera_position.w` is written `1.0`
    /// by both of them, so a switch there would have turned the debug view on for
    /// every frame the engine has ever drawn; a `float4` of its own at the end of
    /// the block would move no offset either, and was not taken because the block
    /// is constructed field-by-field by a test outside this crate's workspace
    /// neighbourhood — `crates/crcbl/tests/forward_e2e/depth_probe.rs` — which a
    /// new member breaks. A lane every writer already zeroes is the change that
    /// costs nothing anywhere.
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
    /// light. `w`: how many of them it is moved along the receiver's own
    /// geometric normal, per unit of `sin(acos(Ng·L))` — the normal-offset
    /// bias, and `shadow_normal_offset` is what argues the direction.
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
    /// `xyz`: how many probes **each level** of the irradiance clipmap holds on
    /// each axis — the same count every level, which is what makes a coarser
    /// level cover more world rather than hold more probes. `w`: how many rows
    /// `probes` holds across every level, and the bound every fetch below is
    /// clamped against. Matching `crcbl_shaders::probe::ProbeVolume`.
    ///
    /// **The rows below were appended after these**, each on
    /// `light_view_proj`'s terms: no existing member's offset moved, so every
    /// golden blessed before it existed still matched. The clipmap is the one
    /// change that did not keep that discipline — growing this group from three
    /// rows to `2 · PROBE_LEVELS + 2` moved every member after it — and it could
    /// not: what moved is the *shape* of a member the block already had, and
    /// nothing after it moved relative to anything else. No golden moved either,
    /// because the offsets are a contract between this file and
    /// `crcbl_shaders::mesh::FRAME_UNIFORMS_SIZE` and the two changed together.
    ///
    /// All zero for a scene with no probes, and `probe_irradiance` then returns
    /// exactly zero with no branch anywhere — see it.
    uint4 probe_counts;
    /// `x`: how many clipmap levels are live, never more than `PROBE_LEVELS`.
    /// `y`: how many rows one level holds, which is also where each next level's
    /// rows begin. `z` and `w` are the sixteen-byte row's padding.
    uint4 probe_levels;
    /// Where probe `(0, 0, 0)` of each level stands, in world space; `w` unused.
    ///
    /// **Uploaded rather than derived here.** Where a level stands is one rule,
    /// `crcbl_shaders::probe::ProbeVolume::level_origin` is where it is written,
    /// and this stage reads the answer — exactly as `probe_octahedral.slang`
    /// reads a direction table rather than evaluating the octahedral mapping a
    /// second time. The rows past `probe_levels.x` are zeroed and nothing reads
    /// them.
    float4 probe_level_origin[PROBE_LEVELS];
    /// One over each level's spacing on each axis; `w` unused.
    ///
    /// Inverted on the host so this stage multiplies rather than divides. An
    /// axis whose reciprocal is zero collapses onto probe 0, which is what the
    /// grid of a scene with no probes carries.
    float4 probe_level_inv_spacing[PROBE_LEVELS];
    /// `docs/plan/25-lod.md`'s selection numbers, as `draw_gen.slang`'s
    /// `DrawGenParams::lod_params` carries them: `x` pixels per unit of length
    /// one unit from the eye, `y` the budget a group's projected error must
    /// exceed to expand, `z` the budget an expanded one is held down to, `w`
    /// unread padding.
    ///
    /// **Read by nothing in this file.** It is declared because this block and
    /// `mesh_cluster.slang`'s are one buffer, and *that* file's mesh stage reads
    /// all three for the screen-error heatmap — it projects a group's error
    /// itself and anchors the ramp on the two budgets. A block short of this row
    /// here would be one whose size the two stages disagree about.
    ///
    /// **Last in the block**, on `probe_origin`'s terms exactly: no existing
    /// member's offset moves, so every golden blessed before this member existed
    /// still matches.
    float4 lod_params;
    /// Exponential height fog: `x` the density at the reference height, per
    /// world unit travelled; `y` the height over which that density falls by a
    /// factor of `e`; `z` the reference height itself; `w` unread.
    ///
    /// **A zero density is exactly no fog.** `fog_optical_depth` is then
    /// exactly zero, `fog_transmittance` of zero is exactly one, and the
    /// composite below is `lit * t + fog * (1 - t)` — which at `t == 1` returns
    /// `lit` bit for bit, where a `lerp` would return `lit + (fog - lit)` and
    /// lose the low bits of an HDR value far from the fog colour. So this is a
    /// feature made of data with no branch selecting it, on the probe grid's
    /// terms, and no golden blessed before it existed moves.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset changes.
    float4 fog_params;
    /// The radiance the fog scatters towards the eye, in `rgb`; `w` unread.
    ///
    /// Pre-tonemap and unclamped like everything else this stage writes: it is
    /// a radiance rather than a display colour, so a bright sky's fog may sit
    /// above one and the bloom chain may see it.
    float4 fog_color;
    /// The red channel of the sky's L1 irradiance, packed as `GpuProbe::sh_r`
    /// is: the linear band in `xyz` and the constant band in `w`, so
    /// `sky_irradiance` evaluates it as `dot(row, float4(N, 1))`.
    ///
    /// A distant environment reaches a diffuse surface in exactly this shape,
    /// which is why the sky and the irradiance grid share it. The two are
    /// **added** rather than chosen between: a scene may have a sky above it
    /// and bounce light around it, and both are the same term.
    ///
    /// **Zero is exactly off.** Three zero rows make three zero dot products,
    /// so the sum the ambient term starts from is the one it was before a sky
    /// existed. `crcbl_shaders::sky::SkyGradient::irradiance` is the host side
    /// that fills these, and `SkyGradient::BLACK` projects to exactly zero.
    ///
    /// **Last in the block**, on `lod_params`' terms.
    float4 sky_sh_r;
    /// The green channel of the same. See `sky_sh_r`.
    float4 sky_sh_g;
    /// The blue channel of the same. See `sky_sh_r`.
    float4 sky_sh_b;
    /// World → clip **as the previous frame's camera saw it**: the camera-side
    /// twin of `GpuInstance::previous_transform`, and the other half of what a
    /// motion vector is made of.
    ///
    /// `crcbl_render::ForwardRenderer` advances it once per frame, at the same
    /// boundary `crcbl_render::InstancePool::rotate` settles a transform's
    /// previous value on, and the first frame carries its own `view_proj` — a
    /// camera that has not moved yet has not moved.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset moves, so every golden blessed before this member existed still
    /// matches.
    float4x4 previous_view_proj;
    /// Where the pool's **attribute** region begins, as a word index; `yzw`
    /// unread padding. See `crcbl_shaders::mesh::FrameUniforms::vertex_pool`,
    /// and `load_vertex` above, which is the only reader.
    uint4 vertex_pool;
    /// Where atlas slot `i`'s map is in the shadow atlas: a scale into the image
    /// in `xy` and an offset in `zw`, so a point `t` of that map's own `0..1`
    /// space is at `zw + t * xy`. `atlas_rect` is what reads it.
    ///
    /// **What replaced a tile index into a fixed grid**,
    /// `docs/plan/45-shadows.md`'s atlas rung: this file used to derive a cell
    /// from the index, so every map was one size and could only be that size.
    /// Reading the rectangle the host allocated lets a far or small light take a
    /// halving of a cell with no second sampling path here.
    ///
    /// Indexed by **slot** rather than by light, on `light_view_proj`'s terms:
    /// the cascades are the first `SHADOW_CASCADES` rows and `light_tile` of a
    /// light's tile is its own. A slot no map was rendered into carries a zero
    /// rectangle and is read by nothing, because the rows that could name it
    /// carry `NO_SHADOW_TILE`.
    ///
    /// **Last in the block**, on `lod_params`' terms: no existing member's
    /// offset moves.
    float4 shadow_atlas_rect[SHADOW_ATLAS_TILES];
};

/// What `FrameUniforms::ambient`'s `w` must reach for `fragmentMain` to draw the
/// normals view instead of the shaded picture.
///
/// **A threshold rather than an equality**, because the switch rides in a float
/// lane: `crcbl_shaders::mesh::FrameUniforms::NORMALS_VIEW_ON` is the `1.0` the
/// host writes and `..._OFF` is the `0.0` every other frame carries, and half way
/// between them is the one comparison that cannot be decided differently by two
/// targets' float rules. Off is the default everywhere — see the Rust constants,
/// which is where the two values are named.
static const float NORMALS_VIEW = 0.5;

/// What `FrameUniforms::ambient`'s `w` must reach for the LOD view.
///
/// The same lane as `NORMALS_VIEW`, one sentinel further out:
/// `crcbl_shaders::mesh::FrameUniforms::LOD_VIEW_ON` is the `2.0` the host
/// writes. `fragmentMain` tests this **before** the normals threshold, because
/// a `2.0` clears both and the outer one has to win.
///
/// The tint itself is produced in `mesh_cluster.slang`, which is the only stage
/// that knows a cluster's level; this file's job is to show the colour it was
/// handed rather than to shade it. On the two indirect paths there is no cluster
/// — selection is per instance — so `vertexMain` writes one flat colour, and
/// that difference is the thing worth seeing: the mesh path is a mosaic and the
/// indirect paths are not.
///
/// **This threshold is the whole set of overlays, not only the tint.**
/// `mesh_cluster.slang` carries a third sentinel above it —
/// `crcbl_shaders::mesh::FrameUniforms::HEATMAP_VIEW_ON`, the screen-error
/// heatmap — and a `3.0` clears this comparison too. That is deliberate rather
/// than incidental: what this file does for every overlay is the same thing,
/// pass the colour the geometry stage chose through unshaded, so it needs a
/// threshold per *kind of treatment* and not one per overlay. The stage that
/// picks between the overlays is the one that knows the cluster.
static const float LOD_VIEW = 1.5;

/// The switch for the **occlusion view**, which draws the ambient-occlusion
/// channel alone as grey.
///
/// The same lane again, one sentinel further out:
/// `crcbl_shaders::mesh::FrameUniforms::OCCLUSION_VIEW_ON` is the `4.0` the host
/// writes, and this threshold has to lie above the heatmap's `3.0` so the
/// outermost test wins. `the_occlusion_view_threshold_lies_above_the_heatmap` is
/// what holds the interleaving.
///
/// **A `4.0` also clears `LOD_VIEW`, and the vertex and mesh stages act on
/// that** — they write the tint or heatmap overlay into `output.color`. Nothing
/// reads it: this view's fragment branch returns before the interpolated colour
/// is touched. The overlay is computed and discarded, which costs one lerp per
/// vertex on a view that exists to be looked at, and is the price of one lane
/// carrying five states instead of a second lane carrying one.
static const float OCCLUSION_VIEW = 3.5;

/// The switch for the **motion view**, which draws the motion vector this
/// fragment wrote to `FragmentOutput::motion` instead of shading it.
///
/// The same lane once more, one sentinel further out again:
/// `crcbl_shaders::mesh::FrameUniforms::MOTION_VIEW_ON` is what the host
/// writes, and this threshold has to lie above the occlusion view's so the
/// outermost test wins.
/// `the_motion_view_threshold_lies_above_the_occlusion_view` is what holds the
/// interleaving.
///
/// A sentinel this far out clears every threshold below it, and the vertex and
/// mesh stages act on that exactly as they do for the occlusion view: the
/// overlay they write into `output.color` is computed and discarded, because
/// this view's fragment branch returns before the interpolated colour is
/// touched.
static const float MOTION_VIEW = 4.5;

/// The switch for the **bent-normal view**, which draws the direction the
/// occlusion channel carries beside its scalar.
///
/// The same lane once more and the outermost sentinel there is:
/// `crcbl_shaders::mesh::FrameUniforms::BENT_NORMAL_VIEW_ON` is what the host
/// writes, and this threshold lies above the motion view's so this test wins.
/// `the_bent_normal_view_threshold_lies_above_the_motion_view` is what holds
/// the interleaving.
///
/// **It draws the channel and not the shading's use of it**, which is the
/// occlusion view's rule exactly: `gba` passes straight through, so the picture
/// is the conventional `n * 0.5 + 0.5` the normals view already uses and a
/// pixel with no direction reads as the mid grey the sentinel encodes to.
/// Drawing the fallback instead would show this file's own shading normal
/// wherever the channel said nothing, which is a picture of `bent_normal_at`
/// rather than of what the occlusion pass produced.
static const float BENT_NORMAL_VIEW = 5.5;

/// How far the motion view stretches a motion vector before centring it on
/// grey.
///
/// `crcbl_shaders::mesh::MOTION_VIEW_SCALE` is the host's copy and
/// `the_motion_view_scale_is_the_one_the_host_declares` holds the two equal.
/// A motion of `0.5 / MOTION_VIEW_SCALE` of the frame in one frame lands on the
/// end of the ramp; the `Rgba16Float` scene target carries anything past that
/// unclamped, and only the tonemap's `saturate` on the way to the swapchain
/// flattens it.
///
/// **A pixel at rest encodes to one half in both channels, give or take one
/// half-float step.** The two clip positions are the same vertex through equal
/// matrices, but they are two multiplication chains and not one, so the
/// subtraction comes out a last bit either side of zero rather than at it —
/// and the colour export rounds that into the step below one half, never the
/// step above. `crates/crcbl/tests/mesh_e2e/motion.rs`'s `REST_TOLERANCE` is
/// that one step measured, and why nothing there is an exact comparison.
static const float MOTION_VIEW_SCALE = 8.0;

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's "transform, mesh id, material
/// id, flags", plus the sector id its 2026-07-27 correction adds. This shader
/// reads the transform, the mesh id and the material id — see the Rust struct,
/// which carries the full account of what each field is for and which slice
/// consumes it.
struct GpuInstance
{
    /// Model → sector. Rotation, translation and scale, **non-uniform
    /// included**: the raster stages carry a normal through `normal_basis`, the
    /// upper-left 3×3's cofactor matrix, rather than through the 3×3 itself.
    float4x4 transform;
    /// Where this instance was **last frame**: the same model → sector matrix,
    /// one frame behind, so a fragment can say where it came from.
    ///
    /// Written by `InstancePool` rather than by the caller, exactly as the
    /// liveness bit is. On the frame an instance moves this holds the transform
    /// it moved *from*; on every other frame it equals `transform`, which is
    /// zero motion — including the frame an instance is created, because a
    /// spawn did not travel from anywhere.
    ///
    /// **Nothing reads it yet.** Temporal antialiasing, temporal reflections,
    /// temporal upscaling and per-object motion blur each need a motion vector,
    /// and a motion vector needs this and a motion-vector target that no pass
    /// here writes. The slot is here ahead of them because widening the record
    /// later moves every offset below it in four shader copies and re-blesses
    /// every golden, and none of that gets cheaper by waiting.
    float4x4 previous_transform;
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
    /// that walks the array — is what reads it. Bit 1 is
    /// `INSTANCE_BASE_VERTEX_OVERRIDE`, which the vertex stage below reads. A
    /// draw addresses the instance it was told to, so nothing here asks about
    /// the liveness bit.
    uint flags;
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh entry's when `INSTANCE_BASE_VERTEX_OVERRIDE` is set in
    /// `flags` and ignored when it is not. A deformed instance names its
    /// **source** mesh above and carries the base of the region a skinning
    /// dispatch wrote it into here, so the bucket, the level tables and the
    /// bounding box all still resolve through the mesh it was made from.
    uint base_vertex;
    /// The pool vertex the **previous frame's** deformation of this instance was
    /// written into, read **only** when `INSTANCE_BASE_VERTEX_OVERRIDE` is set
    /// in `flags` and ignored when it is not — and then it is the half of the
    /// skinning region the frame before this one filled, not the half the field
    /// above names.
    ///
    /// The vertex stage below takes a fragment's previous clip position from
    /// the vertex at this base. Without it the previous transform would be
    /// applied to *this* frame's deformed vertex, and a limb that swung would
    /// report the motion of the body it hangs off.
    uint previous_base_vertex;
    /// Pads the record to a multiple of the transform's 16-byte alignment.
    ///
    /// **Spelled out rather than left to the layout rules**, because the four
    /// targets do not agree about the tail: `std430` and Metal round the struct
    /// up to that alignment and DXIL's structured-buffer stride does not, so a
    /// record ending at the field above is 160 bytes in SPIR-V, WGSL and MSL and
    /// 152 in DXIL — one buffer, two element sizes, and nothing that reports it.
    /// `DrawConstants` below carries the same pair for `std140`'s version of the
    /// same rule.
    uint pad1;
    uint pad2;
};


/// `GpuInstance::flags` bit 1: resolve this instance's base vertex out of
/// `GpuInstance::base_vertex` rather than out of the mesh entry the draw
/// selected, matching `crcbl_shaders::mesh::GpuInstance::BASE_VERTEX_OVERRIDE`.
///
/// **A bit rather than a sentinel in the field**: zero is a legal base vertex,
/// so no in-range value is free to mean "no override", and an element nothing
/// has written has to read as "no override" rather than as "draw out of vertex
/// zero". The Rust constant carries the rest of that argument.
static const uint INSTANCE_BASE_VERTEX_OVERRIDE = 2;

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
    /// The scale and offset every `unorm16` UV lane of this mesh was quantised
    /// against, matching `crcbl_shaders::mesh::GpuMesh::uv_range`.
    ///
    /// Four scalars rather than two `float2`s: `std430` aligns a `float2` to
    /// eight, which after nine floats would leave a hole the CPU side has to
    /// reproduce. `load_vertex` takes the four as one `float4`.
    float uv_scale_u;
    float uv_scale_v;
    float uv_offset_u;
    float uv_offset_v;
    /// Per-mesh bits, matching `crcbl_shaders::mesh::GpuMesh::flags`. Today
    /// there is one — `MESH_AUTHORED_TANGENTS` — and `vertexMain` is what
    /// carries it to the fragment stage.
    uint flags;
};

/// `GpuMesh::flags` bit 0: this mesh's vertices carry a real tangent frame,
/// matching `crcbl_shaders::mesh::GpuMesh::MESH_AUTHORED_TANGENTS`.
///
/// **What it selects is which frame a normal map is sampled in.** Set, and the
/// fragment stage perturbs in the interpolated vertex frame — the QTangent the
/// importer built from the document's own `TANGENT`, whose `w` is the only
/// thing that recovers the handedness of a mirrored UV shell. Clear, and it
/// perturbs in the screen-space derivative frame instead, because a mesh with
/// no authored tangent carries `crcbl_shaders::vertex::orthonormal_basis`'
/// stand-in — a frame that is orthonormal and agrees with no UV
/// parameterisation at all, so a normal map read through it is rotated by an
/// arbitrary angle per vertex.
///
/// `docs/plan/43-render-standards.md` §2's rung 1 in one bit.
static const uint MESH_AUTHORED_TANGENTS = 1;

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
    /// The base-colour page layer in the **low** sixteen bits and the normal
    /// page layer in the high sixteen, matching
    /// `crcbl_shaders::mesh::GpuMaterial`'s `base_color_texture` and
    /// `normal_texture`.
    ///
    /// **Two layers to a word, and that is what makes the row 64 bytes.**
    /// `docs/plan/43-render-standards.md` §2 sizes it for four page rows, the
    /// alpha cutoff and flags; written as four separate `uint`s the row needs
    /// eighteen words and `std430` rounds it to eighty. A page layer index is
    /// bounded by the device's maximum array layers, so sixteen bits is a limit
    /// nothing reaches — the host's `MATERIAL_STRIDE` carries the arithmetic and
    /// `MAX_PAGE_LAYER` the bound. `base_color_layer` and `normal_layer` below
    /// are the two readers, so no call site does the shift by hand.
    ///
    /// Layer 0 by convention is the page's neutral — a white texel on the
    /// base-colour page and `(0.5, 0.5, 1.0)` on the normal one — so a material
    /// that wants no texture leaves both halves at zero.
    uint color_normal_pages;
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
    /// How `fragmentMain` derives this material's sampling UV: `TILING_AUTHORED`
    /// reads the vertex UV, `TILING_PHYSICAL` derives it from the surface's
    /// world-space extent. Matching `crcbl_shaders::mesh::GpuMaterial::tiling`.
    ///
    /// Zero is `TILING_AUTHORED`, so a row nobody wrote and every material
    /// authored before this field existed samples the vertex UV exactly as it
    /// always did — the physical branch is added, not substituted.
    uint tiling;
    /// How many world-space metres one repeat of the texture spans under
    /// `TILING_PHYSICAL`; unread under `TILING_AUTHORED`. Matching
    /// `crcbl_shaders::mesh::GpuMaterial::tile_metres`.
    ///
    /// `physical_tile_uv` clamps it off zero before dividing, so a physical row
    /// left at zero collapses the surface onto one texel rather than dividing by
    /// zero.
    float tile_metres;
    /// Linear radiance this surface emits, in `r`, `g` and `b` — added to the
    /// shaded colour and scaled by nothing.
    ///
    /// **Three scalars rather than a `float3`**, for the reason the padding they
    /// replaced spelled out: under `std430` a `float3` is 16-byte aligned, so one
    /// declared here would sit at offset 48 and take the row to 64 while the CPU
    /// wrote it at 36. Scalars keep every earlier member's offset and keep the
    /// stride at `crcbl_shaders::mesh::MATERIAL_STRIDE`. `emissive_of` is what
    /// reads them back as a vector.
    ///
    /// **The three words this occupies were the row's padding**, which is why
    /// nothing moved and why a material nobody has written emits nothing: zero
    /// added to a colour is that colour, on every target, exactly.
    ///
    /// Not a factor and a separate strength, which is how glTF splits it
    /// (`emissiveFactor` and `KHR_materials_emissive_strength`): their product is
    /// a linear radiance and this is that product, because the scene target is
    /// `Rgba16Float` and has the range to hold it. The bloom chain is what turns
    /// a value above one into a glow.
    float emissive_r;
    float emissive_g;
    float emissive_b;
    /// The metallic-roughness-occlusion page layer in the low sixteen bits and
    /// the emissive page layer in the high sixteen, on `color_normal_pages`'
    /// terms.
    ///
    /// **Declared and read by nothing.** §2's table lays four page rows out at
    /// once and this slice wires the normal row alone; every producer writes
    /// zero here, and the rungs that spend the two halves are the packed ORM
    /// texture and the emissive page. They are in the layout now because the
    /// row's stride is mirrored in five declarations and one pinned offsets
    /// test, and widening it twice is that work twice.
    uint mro_emissive_pages;
    /// glTF's `normalTexture.scale`: the factor the decoded tangent-space
    /// normal's `x` and `y` are multiplied by before it is renormalised, with
    /// `z` left alone — glTF 2.0 §3.9.3. One is the map as authored, zero is a
    /// flat surface. Read only where `normal_layer` is non-zero.
    float normal_scale;
    /// The alpha below which a masked material discards, glTF's `alphaCutoff`.
    /// **Declared and read by nothing** — §3's alpha-mask rung is what spends
    /// it, and it needs a mode bit in `flags` beside it before a cutoff means
    /// anything.
    float alpha_cutoff;
    /// Per-material bits, **all unassigned**. The word §2's table names beside
    /// the cutoff, and where its alpha modes will land.
    uint flags;
};

/// The base-colour page layer a material row names — the low half of
/// `GpuMaterial::color_normal_pages`.
///
/// A function rather than a mask spelled at each reader, for `emissive_of`'s
/// reason: the pairing is the one thing about the row that is invisible in the
/// field declarations, and a shift written by hand in two places is two places
/// to get the halves the wrong way round.
uint base_color_layer(GpuMaterial material)
{
    return material.color_normal_pages & 0xffffu;
}

/// The normal page layer a material row names — the high half of
/// `GpuMaterial::color_normal_pages`.
///
/// **Zero means the material has no normal map**, and `fragmentMain` tests it
/// rather than trusting layer 0's neutral texel. An eight-bit unorm has no
/// exact `0.5`, so that texel decodes to a tangent-space normal about a fifth
/// of a degree off flat; perturbing by it would move every golden in the tree
/// that draws a lit surface, for a picture nobody asked to change.
/// `crcbl_shaders::mesh::a_neutral_normal_texel_is_not_exactly_flat` is that
/// error measured on the host.
uint normal_layer(GpuMaterial material)
{
    return material.color_normal_pages >> 16;
}

/// The emissive radiance of a material row, as a vector.
///
/// The three scalars exist for the layout reason `GpuMaterial::emissive_r`
/// gives; every reader wants them together, and this is the one place that
/// reassembles them so no caller reorders the channels by hand.
float3 emissive_of(GpuMaterial material)
{
    return float3(material.emissive_r, material.emissive_g, material.emissive_b);
}

/// `GpuMaterial::tiling`: sample the base-colour texture at the vertex's own UV.
///
/// Zero, matching `crcbl_shaders::mesh::GpuMaterial::TILING_AUTHORED`, so it is
/// the value an unwritten row and every `..UNTINTED` spread already carry.
static const uint TILING_AUTHORED = 0;

/// `GpuMaterial::tiling`: derive the UV from the surface's world-space extent, so
/// the texture repeats once per `tile_metres` — see `physical_tile_uv`. Matching
/// `crcbl_shaders::mesh::GpuMaterial::TILING_PHYSICAL`.
static const uint TILING_PHYSICAL = 1;

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
    /// Towards the light for a directional; the cone axis for a spot; the
    /// direction a rectangle emits along. `w` is the cosine of a spot's outer
    /// half-angle, and a rectangle's half-extent along its `v` axis.
    float4 direction;
    /// A rectangle's unit `u` axis in `xyz` and its half-extent along it in `w`.
    /// Unread by every other kind. The `v` axis is
    /// `cross(tangent.xyz, direction.xyz)`, which is unit because those two are.
    float4 tangent;
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
    /// What this light may contribute: `FLAG_FILL`, or zero for an ordinary one.
    uint flags;
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

/// `GpuLight::kind` for a rectangular area light, shaded through the linearly
/// transformed cosine `ltc_matrix` holds — `crcbl_shaders::light::KIND_RECT` is
/// the map of which of the row's vectors means what for one.
static const uint KIND_RECT = 3;

/// `GpuLight::flags` for a light that lights but does not shadow and adds no
/// specular, matching `crcbl_shaders::light::FLAG_FILL`.
///
/// **Only the specular half is enforced here.** The shadow half is
/// `crcbl_render::shadow::Selection`'s: it is the only thing that hands a light
/// a tile, so a fill light's row already carries `NO_SHADOW_TILE` and the walk
/// below needs no second test for it.
static const uint FLAG_FILL = 1;

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

/// The vertex pool, read as raw words: both streams of every resident mesh.
///
/// **One binding for two streams, and that is forced rather than chosen.** This
/// layout already binds `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE` storage
/// buffers in its vertex stage — every one a WebGPU device guarantees — so a
/// second vertex binding is a renderer that cannot be built in a browser. Two
/// regions of one buffer cost nothing two buffers would have saved: the memory
/// system sees the same two contiguous runs either way, and a pass that wants
/// geometry alone still reads twelve bytes a vertex.
[[vk::binding(1, 0)]]
StructuredBuffer<uint> vertices;

/// Where vertex `at`'s position sits, and nothing else.
///
/// The twelve bytes stream 0 exists to make cheap: a depth prepass or a shadow
/// cascade reads a contiguous run of these and never touches the attribute
/// region at all.
float3 load_position(uint at)
{
    uint word = at * POSITION_WORDS;
    return float3(asfloat(vertices[word]),
                  asfloat(vertices[word + 1]),
                  asfloat(vertices[word + 2]));
}

/// The whole of vertex `at`, with its coordinates put back through the mesh's
/// own range — `range.xy` the scale, `range.zw` the offset, which is
/// `crcbl_shaders::vertex::UvRange::decode`.
MeshVertex load_vertex(uint at, float4 range)
{
    uint word = frame.vertex_pool.x + at * ATTRIBUTE_WORDS;
    MeshVertex vertex;
    vertex.position = load_position(at);
    vertex.basis = decode_qtangent(unpack_snorm16x4(vertices[word], vertices[word + 1]));
    vertex.uv0 = range.zw + range.xy * unpack_unorm16x2(vertices[word + 2]);
    vertex.color = unpack_rgba8(vertices[word + 4]);
    return vertex;
}

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
/// `base_color_layer(material)`.
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

/// The one sampler the page is read through: nearest, **wrapped**.
///
/// Nearest because these are material *pages* rather than photographs — there
/// is no mip chain (§3.2's mip generation is a compute pass of its own) and a
/// filtered read of a page with no mips is a shimmer, not a smoother picture.
/// One sampler and no choice, for `sprite.slang`'s reason: a shader cannot ask
/// which filter it got, so a second one would be a second pipeline.
///
/// **`Repeat`, not `ClampToEdge`**, and that is what `TILING_PHYSICAL` needs: a
/// physical UV runs to `world_extent / tile_metres`, well past `0..1` on any
/// face larger than one tile, and only a wrapping sampler makes the page tile
/// across it rather than smearing its edge row. `TILING_AUTHORED` is
/// unaffected — its UV is the vertex's, which the meshes author in `0..1`, so a
/// wrapped and a clamped read return the same texel for it. `crcbl_render`'s
/// forward pass is what sets the address mode and says so.
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
/// `ssao_upsample.slang` left it: one `Rgba8Unorm` channel the size of the
/// frame, `r` the visibility — `1.0` where nothing occludes — and `gba` the
/// **bent direction**, in world space and encoded `xyz * 0.5 + 0.5`.
///
/// # The direction says where the light still comes from
///
/// The scalar can only dim the ambient term. The bent direction is the average
/// unblocked direction over the hemisphere the same horizon sweep measured, so
/// the environment can be sampled *along* it — see `bent_normal_at` and the
/// irradiance sum in `fragmentMain`, which is the whole of what the widened
/// target buys. `ssao.slang`'s header is where the encoding and the sweep's
/// by-product are argued.
///
/// **A zero-length direction is the sentinel for "no bent direction here"**,
/// and `bent_normal_at` answers it with the fragment's own shading normal —
/// which is what makes the 1×1 placeholder below and a pixel the gather had
/// nothing to say about one case with one answer.
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
/// occludes nothing — and, in the three channels beside it, the direction that
/// steers nothing. So this file has no branch for the feature being off.
///
/// **Last of the set, and that is structural.** `mesh_cluster.slang` declares up
/// to 19 and mirrors 20 and 21; 22 is past all of them, so it needs no mirror
/// there and no Metal argument-table index moves. See `SHADOW_ATLAS_BINDING`'s
/// note in `crcbl_render::forward` on why the list stays ascending.
[[vk::binding(22, 0)]]
Texture2D<float4> ambient_occlusion;

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

/// The split-sum `DFG` table: the **scale and bias on `f0`** whose sum is how
/// much of the light arriving at a GGX lobe that lobe hands back.
/// `crcbl_shaders::dfg` is where it is integrated and committed, and
/// `crcbl_shaders::dfg::pair_texels` is the image `crcbl_render::forward`
/// uploads into this slot.
///
/// # Both channels, because two terms read it
///
/// Multi-scatter energy compensation wants only the sum — the directional
/// albedo — and read that alone until the area lights arrived. An area light's
/// specular is the polygon integral times `f0 · scale + bias`, which is the
/// pair, so the slot now carries the pair and the compensation adds the two
/// channels itself. One image and one set of four `Load`s serve both, which is
/// `docs/plan/44-lighting.md`'s own "one table serves both rungs" holding by
/// construction rather than by two uploads of the same bytes agreeing.
///
/// # Two bytes of fixed point per channel, not a half float
///
/// `Rgba8Unorm`, each channel's high byte then its low, decoded by
/// `decode_dfg_pair` below. Each stored number is a *share of arriving light*
/// and therefore lives in `[0, 1]`, where a step of `1 / 65535` is finer
/// everywhere than half precision — which near one is `2^-11`, thirty times
/// coarser. `crcbl_shaders::dfg::PAIR_TEXEL_BYTES` argues the rest of it, and
/// `ssr.slang` reads the same encoding out of its own copy.
///
/// # Fetched by texel and filtered here, not by a sampler
///
/// Four `Load`s and a bilinear blend written out, rather than a `SampleLevel`
/// with a linear filter. A filter is fixed-function arithmetic whose weights
/// four rasterisers compute independently, and this engine's goldens are
/// compared across all four with no tolerance; the blend below is ordinary
/// float multiplies and adds, which they agree on. It also costs no sampler,
/// and therefore moves no index of Metal's sampler argument table.
///
/// **Last of the set, for the reason `ambient_occlusion` and `probes` both
/// give**: `crcbl-mtl` numbers a resource by counting the same-table entries of
/// the layout list below it, and Slang numbers a stage's arguments by
/// declaration order, so the two agree only while both ascend. 25 is past
/// everything `mesh_cluster.slang` declares — that file reaches 24 — so it needs
/// no mirror there and no index it already owns moves.
[[vk::binding(25, 0)]]
Texture2D<float4> specular_dfg;

/// §2's second texture page: the **normal** map, one `Texture2DArray` indexed
/// by `normal_layer(material)`.
///
/// The base-colour page's own shape, one binding higher, read through the same
/// UV and the *same sampler* — `base_color_sampler`, so a normal map gets the
/// trilinear anisotropic filtering the colour page already has without a second
/// sampler, a second layout entry, or a second index in Metal's sampler
/// argument table.
///
/// # `Rgba8Unorm`, and the missing `Srgb` is the whole point
///
/// `crcbl_render::forward` creates this image as `Rgba8Unorm` where it creates
/// the base-colour page as `Rgba8UnormSrgb`, and
/// `docs/plan/44-lighting.md`'s rung 2 is where that is argued: a base-colour
/// texel is a *colour* and glTF defines it as sRGB-encoded, where a normal texel
/// is a *number* — three components of a direction — and pushing it through the
/// sRGB decode is wrong by a gamma. The failure is quiet, which is why it
/// survives review everywhere it happens: the decoded normal simply leans
/// further from vertical than the author drew, and the surface reads as
/// "bumpier than intended" rather than as broken.
/// `crcbl_render::forward::the_two_page_formats_differ` is the guard.
///
/// **Appended past `specular_dfg`, never inserted**, for
/// `SHADOW_ATLAS_BINDING`'s reason exactly: `crcbl-mtl` gives a resource the
/// next index in its Metal argument table by counting the same-table entries of
/// the layout list, and Slang numbers a stage's arguments by declaration order,
/// so the two agree only while both ascend. 26 is past everything
/// `mesh_cluster.slang` declares — that file reaches 24 — so it needs no mirror
/// there and no index anything already owns moves.
[[vk::binding(26, 0)]]
Texture2DArray<float4> normal_textures;

/// `docs/plan/44-lighting.md`'s rung 5: the linearly transformed cosine fit of
/// the GGX lobe, four matrix entries per texel over the same `(N·V, roughness)`
/// grid `specular_dfg` is on. `crcbl_shaders::ltc` is where it is fitted and
/// committed, and `crcbl_shaders::ltc::texels` is the image
/// `crcbl_render::forward` uploads into this slot.
///
/// # Half floats, where the table beside it is fixed point
///
/// `Rgba16Float`. The `DFG` pair holds shares of arriving light, which live in
/// `[0, 1]` where a fixed-point step beats half precision; these are matrix
/// entries that change sign and run over two orders of magnitude, and half
/// floats spend their precision relatively rather than absolutely.
/// `crcbl_shaders::ltc::LTC_TEXEL_BYTES` argues it in full.
///
/// # Fetched by texel and filtered here, for `specular_dfg`'s reason
///
/// Four `Load`s and a bilinear blend written out. The two tables share a grid,
/// so `table_tap` computes one set of texel coordinates and weights and both
/// reads use it — which is the whole reason `crcbl_shaders::ltc` was fitted on
/// `crcbl_shaders::dfg`'s axes rather than on the paper's own.
///
/// **Appended past `normal_textures`, never inserted**, for the reason every
/// binding above it gives: `crcbl-mtl` numbers a resource by counting the
/// same-table entries of the layout list, and Slang numbers a stage's arguments
/// by declaration order, so the two agree only while both ascend. 27 is past
/// everything `mesh_cluster.slang` declares, so it needs no mirror there and no
/// index anything already owns moves.
[[vk::binding(27, 0)]]
Texture2D<float4> ltc_matrix;

/// `docs/plan/45-shadows.md`'s screen-space contact shadows, as
/// `contact_shadows.slang` left them: one `R8Unorm` channel the size of the
/// frame, `1.0` where the sun reaches the surface unobstructed.
///
/// # What it is, and what the cascades cannot say
///
/// `cascade_visibility` below biases its comparison by whole cascade texels,
/// because that is the resolution the evidence has — and a bias large enough to
/// stop acne is large enough to detach the shadow from the thing casting it.
/// This channel is the same question asked of the *depth prepass*, at frame
/// resolution, over a ray a fraction of a metre long: the sliver where a foot
/// meets the floor, which no atlas texel is small enough to hold.
///
/// # Fetched by pixel, clamped, and multiplied into the sun alone
///
/// `contact_at` reads it exactly as `occlusion_at` reads `ambient_occlusion`,
/// and that binding's declaration argues every part of it: a `Load` at
/// `SV_Position.xy` needs no sampler and no texel-centre arithmetic, and the
/// clamp against `GetDimensions` is what lets the off-switch be one texel rather
/// than a frame-sized clear — an unclamped `Load` reads a 1×1 placeholder as
/// *total* shadow at every pixel but one.
///
/// It multiplies `sun_visibility` and nothing else. A punctual light's contact
/// is a march of its own along a different direction, and this channel was
/// marched towards the sun; `docs/plan/45-shadows.md` carries that as the rung
/// above this one.
///
/// **Appended past `ltc_matrix`, never inserted**, for the reason every binding
/// above it gives: `crcbl-mtl` numbers a resource by counting the same-table
/// entries of the layout list, and Slang numbers a stage's arguments by
/// declaration order, so the two agree only while both ascend. 28 is past
/// everything `mesh_cluster.slang` declares, so it needs no mirror there and no
/// index anything already owns moves.
[[vk::binding(28, 0)]]
Texture2D<float> contact_shadow;

/// `docs/plan/50-irradiance-probes.md`'s per-probe **visibility map**: one layer
/// per row of `probes`, `PROBE_VISIBILITY_EXTENT` texels square, `r` the mean
/// distance from that probe to the nearest static surface in a direction and `g`
/// the mean of its square. `crcbl_shaders::probe_visibility` owns the layout,
/// the octahedral mapping and the Chebyshev bound this file evaluates against
/// it, and `crcbl_render::probe_visibility` is what captures it.
///
/// # What it is for
///
/// Majercik et al. 2019's contribution, and the one thing that makes a probe
/// grid stop leaking: a fragment further from a probe than the wall that probe
/// can see in the fragment's own direction is *behind* that wall, and takes none
/// of the probe's light. `probe_irradiance` weighs each of its eight corners by
/// it. Without it a room's probes light the room next door through the wall
/// between them, which is the defect the whole rung is about.
///
/// # Read by four `Load`s and a blend written out, for `specular_dfg`'s reason
///
/// A hardware filter is fixed-function arithmetic whose weights four rasterisers
/// compute independently, and this engine's goldens are compared across all
/// four. It also means the image's format need not be filterable — `rg32float`
/// is not, without WebGPU's `float32-filterable` — and it costs no sampler, so
/// no index of Metal's sampler argument table moves.
///
/// **A `float4` element on a two-channel image**, read as `.xy`: the third and
/// fourth channels of such a fetch are the format's own zero and one on every
/// backend, and declaring the wider element is what `ambient_occlusion` does
/// over its own narrower use.
///
/// # Bound on every path, and a one-texel image is the off switch
///
/// `crcbl_render::forward` binds a 1×1×1 image holding
/// `crcbl_shaders::probe_visibility::FAR` when nothing has been captured or when
/// `r_probe_visibility` is off. No surface is further away than that, so every
/// probe keeps all of its weight and this file has no branch for the feature
/// being absent — the same shape as the white 1×1 image `ambient_occlusion`
/// falls back to. The fetch is clamped against `GetDimensions` for that
/// binding's reason, which is what makes the one-texel image readable by the
/// same code as a captured one.
///
/// **Appended past `contact_shadow`, never inserted**, for the reason every
/// binding above it gives: `crcbl-mtl` numbers a resource by counting the
/// same-table entries of the layout list, and Slang numbers a stage's arguments
/// by declaration order, so the two agree only while both ascend. 29 is past
/// everything `mesh_cluster.slang` declares, so it needs no mirror there and no
/// index anything already owns moves.
[[vk::binding(29, 0)]]
Texture2DArray<float4> probe_visibility;

/// The matrix that carries a mesh-space normal into `GpuInstance::transform`'s
/// space, built out of that transform's own upper-left 3×3.
///
/// **The cofactor matrix**: row `i` is the cross product of the other two rows
/// of `basis`, in cyclic order, and that matrix is `det(basis)` times the
/// inverse transpose. The inverse transpose is what a normal needs and the 3×3
/// is not — a normal is *perpendicular* to a surface, and only a transform that
/// preserves angles carries a perpendicular the way it carries a tangent. The
/// determinant is a scalar factor, and `fragmentMain` normalises what it is
/// handed, so a positive one never reaches the shading.
///
/// **The identity is its own cofactor matrix, exactly** — the cross products of
/// the axis triple are the triple back, with nothing to round — and so is a
/// uniform scale, and an axis-aligned scale on an axis-aligned normal, once
/// `fragmentMain` has normalised. Those are the shapes this engine's own scenes
/// place instances at, which is why this replaced the bare 3×3 with every
/// golden still matching; `crcbl_shaders::mesh`'s
/// `the_normal_basis_keeps_a_normal_perpendicular_under_a_non_uniform_scale`
/// asserts each of them.
///
/// **A cofactor matrix rather than the inverse transpose itself**, for two
/// reasons that are not style. It needs no division, so a nearly singular
/// transform yields a small vector rather than an enormous one; and where the
/// determinant is negative — a mirroring transform, which flips the winding —
/// the cofactor flips the normal onto the side the flipped winding put the
/// surface on, where the inverse transpose alone leaves it pointing into the
/// object. This is `graphitemaster/normals_revisited`'s result.
///
/// A transform whose determinant is *zero* has flattened the mesh into a plane,
/// and this returns a zero row for each axis that went with it. Those faces
/// have no area in the flattened mesh and raster no fragments, so the
/// `normalize` in `fragmentMain` is never reached with the zero.
///
/// **Not a second matrix in `GpuInstance`.** An inverse transpose stored per
/// instance is more of the buffer that the cull pass streams and the instance
/// pool re-uploads on every move, for a value that is a function of a matrix
/// already in the record. Computing it here spends three cross products instead
/// of that bandwidth, and it cannot fall out of step with the transform it is
/// built from. `mesh_cluster.slang` and `skinning.slang` carry this function
/// too, and `crcbl_shaders::mesh`'s
/// `every_shader_that_transforms_a_normal_builds_it_with_one_function` is what
/// holds every spelling of it equal.
float3x3 normal_basis(float3x3 basis)
{
    return float3x3(cross(basis[1], basis[2]),
                    cross(basis[2], basis[0]),
                    cross(basis[0], basis[1]));
}

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
    /// This vertex's clip position — the same value as `position` above,
    /// carried as an ordinary interpolant so the fragment stage can divide it
    /// itself.
    ///
    /// **`SV_Position` is not that value.** By the time a fragment stage reads
    /// it the rasteriser has already divided by `w` and mapped the result into
    /// pixels, and this block carries no viewport extent to undo that with.
    ///
    /// **The divide belongs in the fragment stage**, which is why this and
    /// `previous_clip_position` both cross the boundary undivided. Interpolation
    /// is linear in clip space and the perspective divide is not, so a divide
    /// performed at the corners and interpolated across the primitive names the
    /// screen position of a point that is not this fragment — and the
    /// subtraction below would then be wrong everywhere except at the corners.
    float4 clip_position : TEXCOORD2;
    /// Where this vertex was last frame, in last frame's clip space:
    /// `FrameUniforms::previous_view_proj` applied to
    /// `GpuInstance::previous_transform`.
    ///
    /// Undivided, on `clip_position`'s terms. `motion_vector` below is what
    /// subtracts the two, and it is the only reader of either.
    float4 previous_clip_position : TEXCOORD3;
    /// This vertex's surface tangent in world space — the QTangent's tangent
    /// through the instance transform's **bare 3×3**.
    ///
    /// **The bare 3×3 and not `normal_basis`**, which is `skinning.slang`'s
    /// account and holds here for the same reason: a tangent lies *in* the
    /// surface, so the transform itself is what carries it, where the cofactor
    /// matrix is what a perpendicular needs. Sending both through one matrix is
    /// the mistake `normal_basis` exists to prevent, and what it looks like is a
    /// normal map lit from the wrong side on every non-uniformly scaled
    /// instance.
    ///
    /// Interpolated, and therefore neither unit length nor perpendicular to
    /// `world_normal` by the time a fragment reads it — `fragmentMain`
    /// re-orthogonalises with the Gram-Schmidt projection that `skinning.slang`
    /// applies for the same reason. Read only where `FRAME_AUTHORED_TANGENTS`
    /// is set; a mesh without it carries `orthonormal_basis`' stand-in here,
    /// which is a direction the UV parameterisation never agreed to.
    float3 world_tangent : TEXCOORD4;
    /// Which frame this primitive's normal map is sampled in, and which way its
    /// bitangent points: `FRAME_AUTHORED_TANGENTS` and `FRAME_LEFT_HANDED`.
    ///
    /// # `nointerpolation`, for `material`'s reason exactly
    ///
    /// Neither bit is a quantity. A handedness is `+1` or `-1` and a value
    /// between them is a bitangent pointing nowhere, which is precisely what a
    /// mirrored UV seam would produce if the sign were interpolated across the
    /// triangle that straddles it. The provoking vertex's frame is the one the
    /// whole primitive is shaded in, which is the same reading that makes the
    /// mesh flag meaningful at all: a mesh either shipped tangents or did not.
    nointerpolation uint frame : TEXCOORD5;
};

/// `VertexOutput::frame` bit 0: this primitive's mesh carries authored
/// tangents, so the fragment stage perturbs in the interpolated vertex frame.
///
/// `GpuMesh::flags`' `MESH_AUTHORED_TANGENTS` carried across the stage
/// boundary. It is copied into a varying rather than re-read from the table
/// because the fragment stage has no mesh index — the instance's row is the
/// vertex stage's lookup, and `mesh_cluster.slang`'s mesh stage's.
static const uint FRAME_AUTHORED_TANGENTS = 1;

/// `VertexOutput::frame` bit 1: the vertex frame is **left-handed**, so the
/// bitangent is `-cross(normal, tangent)`.
///
/// The sign glTF stores in its tangent's `w` and `QTangent` stores in the sign
/// of its own, carried rather than recomputed — `skinning.slang` gives the
/// argument: handedness is a property of the UV parameterisation, which no
/// transform changes, and re-deriving it from vectors a non-uniform scale has
/// moved would let a nearly degenerate instance mirror a frame the asset never
/// mirrored.
static const uint FRAME_LEFT_HANDED = 2;

/// The `VertexOutput::frame` word for one drawn vertex: the mesh's tangent bit
/// and the vertex's own handedness.
///
/// One function so the two raster paths cannot spell it differently — the same
/// obligation `normal_basis` and `decode_qtangent` are under, and
/// `crcbl_shaders::mesh::every_shader_builds_the_frame_word_the_same_way` is
/// what holds them equal.
uint frame_word(uint mesh_flags, TangentFrame basis)
{
    uint word = (mesh_flags & MESH_AUTHORED_TANGENTS) != 0u ? FRAME_AUTHORED_TANGENTS : 0u;
    // `decode_qtangent` put the sign in the bitangent it reconstructed, so this
    // reads it back off the frame rather than out of the lanes a second time.
    return dot(cross(basis.normal, basis.tangent), basis.bitangent) < 0.0
        ? (word | FRAME_LEFT_HANDED)
        : word;
}

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
    // **Unless the instance overrides it.** A skinned instance's vertices are a
    // run of the pool a compute dispatch deformed them into, which belongs to no
    // mesh entry and changes every frame, so the instance carries the base and
    // goes on naming its source mesh everywhere else. Without the bit the
    // bucket stays authoritative, and it has to: a DAG's level is chosen per
    // instance on the GPU and the base belongs to the *selected* level, which is
    // the entry `draw_gen.slang` put in this block.
    uint base_vertex = (instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) != 0u
        ? instance.base_vertex
        : mesh.base_vertex;
    MeshVertex vertex = load_vertex(
        index + base_vertex,
        float4(mesh.uv_scale_u, mesh.uv_scale_v, mesh.uv_offset_u, mesh.uv_offset_v));
    // **And where this vertex was**, which for a deformed instance is not this
    // vertex at all: the run above is the pose *this* frame's dispatch wrote,
    // so putting it through the previous transform says a swinging limb moved
    // exactly as its body did. The override arm names the half the previous
    // frame's dispatch filled — `GpuInstance::previous_base_vertex` — and
    // without the bit it is the base the line above already read from, so a
    // rigid draw names the same vertex rather than a second one.
    uint previous_base = (instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) != 0u
        ? instance.previous_base_vertex
        : base_vertex;
    float3 previous_position = load_position(index + previous_base);

    float4 world = mul(instance.transform, float4(vertex.position, 1.0));

    VertexOutput output;
    output.position = mul(frame.view_proj, world);
    output.world_position = world.xyz;
    // **Through the cofactor matrix, not through the 3×3 itself.** The 3×3 is
    // the transform a tangent takes; a normal is the perpendicular to those,
    // and a non-uniform scale moves the two apart. `normal_basis` is the matrix
    // that keeps them together, and it is built here rather than uploaded, so
    // `GpuInstance` grows nothing.
    output.world_normal = mul(normal_basis((float3x3)instance.transform), vertex.basis.normal);
    // **And the tangent through the bare 3×3, not the cofactor** — the pair
    // `skinning.slang` argues at length: a tangent lies in the surface and is
    // carried by the transform, a normal is perpendicular to it and is not.
    // The handedness rides beside them as a bit rather than a third vector,
    // because the bitangent is one cross product away from these two.
    output.world_tangent = mul((float3x3)instance.transform, vertex.basis.tangent);
    output.frame = frame_word(mesh.flags, vertex.basis);
    // **No cluster here, and that is the honest picture.** These two paths
    // select one level per *instance*, so there is neither a per-cluster level
    // to tint by nor a per-cluster projected error to shade by; a flat grey says
    // so for both overlays, and standing it beside the mesh path's mosaic is the
    // comparison they exist to make.
    output.color = frame.ambient.w >= LOD_VIEW
        ? float4(0.45, 0.45, 0.48, 1.0)
        : vertex.color;
    // The instance names its material exactly as it names its mesh, so two
    // instances of one mesh can be two colours with no CPU in the loop and no
    // second draw. Passed on rather than resolved here: the table is the
    // fragment stage's to read now, for the reason its binding gives.
    output.material = instance.material;
    output.uv = vertex.uv0;
    // Both clip positions, undivided — see `VertexOutput::clip_position`. The
    // previous one is last frame's vertex under this instance's previous
    // transform seen through last frame's camera, which is the whole of the
    // motion: where the surface went, where the object went and where the
    // viewer went, in one subtraction.
    output.clip_position = output.position;
    output.previous_clip_position = mul(
        frame.previous_view_proj,
        mul(instance.previous_transform, float4(previous_position, 1.0)));
    return output;
}

/// The depth-only vertex stage: stream 0's twelve bytes, the instance's
/// transform, and nothing else.
///
/// `docs/plan/43-render-standards.md` §2 split the pool into a position region
/// and an attribute region so the passes that need a clip position and nothing
/// more could stop paying for a whole record. This is the pass that spends it.
/// `crcbl_render::forward`'s depth pipeline — the shadow cascades and the depth
/// prepass, which are one pipeline driven with two frame blocks — has **no
/// fragment stage and no colour target**, so every varying `vertexMain` fills is
/// written and then dropped. Filling them costs the attribute region's
/// `ATTRIBUTE_WORDS` and a second position load for the motion vector, on top of
/// the `POSITION_WORDS` this one reads; here the fetch is stream 0 alone.
///
/// **Only `SV_Position`, and nothing on this path wants more.** The atlas and
/// the prepass image carry depth alone, and there is no alpha-mask cutout to
/// serve: a cutout needs a fragment stage to `discard` in, this pipeline binds
/// none, and §2's rung 4 will want its own entry point and its own pipeline
/// rather than a UV bolted onto this one.
///
/// **The clip position is the same two multiplies in the same order as
/// `vertexMain`'s**, deliberately. The prepass writes the depth the occlusion
/// pair samples and the cascades write the depth the forward pass compares
/// against, so a transform that rounded differently here would move shadows and
/// occlusion with nothing in the frame to point at.
///
/// The mesh-shader path is untouched: where the device has a mesh stage the
/// depth pipeline still runs `mesh_cluster.slang`'s `meshMain`, which reads a
/// whole vertex through its own `load_vertex`. A depth-only mesh stage is its
/// own rung — that module emits the varyings one `VertexOutput` declares, and
/// splitting it splits the declaration too.
[shader("vertex")]
float4 depthVertexMain(uint index: SV_VertexID, uint instance_id: SV_InstanceID) : SV_Position
{
    GpuInstance instance = instances[visible_instances[draw.base + instance_id]];
    GpuMesh mesh = meshes[draw.mesh];
    // `vertexMain`'s base-vertex rule, and it must stay the same rule: a skinned
    // instance's vertices are the run this frame's dispatch deformed them into,
    // which belongs to no mesh entry, so the instance carries the base and the
    // bucket's mesh is authoritative without the bit.
    uint base_vertex = (instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) != 0u
        ? instance.base_vertex
        : mesh.base_vertex;
    float4 world = mul(instance.transform, float4(load_position(index + base_vertex), 1.0));
    return mul(frame.view_proj, world);
}

/// The stage that resets one tile of the shadow atlas to
/// `SHADOW_ATLAS_CLEAR_DEPTH`, so a pass that keeps some tiles can still redraw
/// others.
///
/// `docs/plan/45-shadows.md`'s cadence rung needs a tile to be cleared on its
/// own, and the render-pass load operation cannot do it: a `LoadOp::Clear`
/// covers the whole attachment, and the region-bounded forms are not portable —
/// Metal's load action and WebGPU's `loadOp` have no render area, so a partial
/// clear would keep a tile on one backend and erase it on another. A primitive
/// covering the tile's own viewport, drawn through a pipeline that always passes
/// the depth test and writes, is the portable statement of the same thing.
///
/// **No inputs at all**, which is what makes it a clear rather than a draw: the
/// three corners come from `SV_VertexID` — `ssao.slang`'s over-sized triangle,
/// the same three — and the depth is the constant above rather than anything
/// projected. `crcbl_render::forward`'s shadow pass has already set the viewport
/// and the scissor to the tile, so the primitive covers that tile and no other.
[shader("vertex")]
float4 depthClearVertexMain(uint index: SV_VertexID) : SV_Position
{
    float2 corner = float2(float((index << 1) & 2), float(index & 2));
    return float4(
        corner * float2(2.0, -2.0) + float2(-1.0, 1.0),
        SHADOW_ATLAS_CLEAR_DEPTH,
        1.0);
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

/// The levels an `Rgba8Unorm` channel has above zero, which is what the
/// reflectivity attachment's alpha is quantised to before it is stored.
///
/// The alpha carries the lobe's roughness for `ssr.slang`, and a raw store
/// would hand the output merger a value such as `0.5 * 255 = 127.5` — a
/// rounding tie, which every API leaves to the implementation, so one backend
/// would reload `GpuMaterial::UNTINTED` as 127 and another as 128, and the
/// sharpness ramp `ssr.slang` derives from the reload would be zero on one and
/// not on the other. `floor(x + 0.5)` is arithmetic every backend defines, so
/// what is handed over is a level exactly and the merger has no tie to break.
/// `crcbl_shaders::ssr::REFLECTIVITY_LEVELS` is the Rust mirror, and
/// `crcbl_shaders::ssr::stored_roughness` is this rounding on the CPU.
static const float REFLECTIVITY_LEVELS = 255.0;

/// What the reflectivity attachment holds where nothing reflects: no `F0` and
/// a fully rough surface, so `ssr.slang` marches nothing from the pixel and
/// reads no mirror in it. `crcbl_shaders::ssr::NO_REFLECTION` is the Rust
/// mirror — the clear `crcbl_render::forward` loads the attachment with — and
/// the debug views below write it.
static const float4 NO_REFLECTION = float4(0.0, 0.0, 0.0, 1.0);

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

/// The smallest directional albedo `specular_compensation` will divide by.
///
/// The table never comes near it — its floor is a rough conductor seen head on,
/// which hands back about a third — so this is not a tuning constant but a
/// guard: it is what stops an unbound, unwritten or corrupt image from turning
/// one multiply into an infinity that survives the tonemap as a white pixel.
/// `crcbl_shaders::dfg::energy_compensation` clamps to the same value, and its
/// doc says why the *upper* half of that clamp is the load-bearing one.
static const float MIN_DIRECTIONAL_ALBEDO = 1e-4;

/// Where in a `(N·V, roughness)` table the four texels around a point are, and
/// how they are weighted.
///
/// **One tap for two tables.** `specular_dfg` and `ltc_matrix` are the same
/// square on the same axes at the same texel centres — which is why
/// `crcbl_shaders::ltc` was fitted on `crcbl_shaders::dfg`'s grid rather than on
/// the paper's own — so the addressing is computed once here and both reads
/// spend four `Load`s and a blend and nothing else.
struct TableTap
{
    /// The lower texel on each axis.
    int2 lo;
    /// The upper one, clamped to the edge rather than wrapped.
    int2 hi;
    /// How far between them the point sits, per axis.
    float2 weight;
};

/// Where `(n_dot_v, roughness)` lands in either table.
///
/// The addressing a clamped linear sampler would use: both tables are baked on
/// texel *centres*, so an axis maps to `value * size - 0.5` and both ends clamp
/// to the edge texel. `crcbl_shaders::dfg::sample` and
/// `crcbl_shaders::ltc::sample` are the same filter on the CPU.
///
/// The extent comes from `GetDimensions` rather than from a constant mirrored
/// out of Rust: the size is a property of the image, so asking it is one fewer
/// number for two files to disagree about. `the_grid_is_the_dfg_table_s_grid`
/// is what says asking one of the two is asking both.
TableTap table_tap(float n_dot_v, float roughness)
{
    uint width;
    uint height;
    specular_dfg.GetDimensions(width, height);
    float2 extent = float2(float(width), float(height));
    float2 scaled = float2(saturate(n_dot_v), saturate(roughness)) * extent - 0.5;
    float2 low = clamp(floor(scaled), float2(0.0, 0.0), extent - 1.0);
    float2 high = min(low + 1.0, extent - 1.0);

    TableTap tap;
    tap.lo = int2(low);
    tap.hi = int2(high);
    tap.weight = clamp(scaled - low, 0.0, 1.0);
    return tap;
}

/// One texel of `specular_dfg` as the two numbers it encodes.
///
/// Each channel pair arrives as the stored bytes over 255, a conversion every
/// backend defines exactly; multiplying back by 255 recovers the bytes, and
/// `high * 256 + low` over 65535 is the fixed point the cook wrote.
/// `crcbl_shaders::dfg::decode_albedo` is the same arithmetic on the CPU, once
/// per channel, and `ssr.slang`'s `decode_fixed_pair` is the other copy of this
/// one.
float2 decode_dfg_pair(float4 texel)
{
    return float2(texel.r * 65280.0 + texel.g * 255.0, texel.b * 65280.0 + texel.a * 255.0)
         / 65535.0;
}

/// The split-sum pair — a scale and a bias on `f0` — at `tap`.
///
/// Bilinear over the four texels around the point, written out rather than asked
/// of a sampler: a hardware filter's weights are fixed-function arithmetic four
/// rasterisers compute independently, and the blend below is ordinary multiplies
/// and adds, which they agree on.
float2 dfg_at(TableTap tap)
{
    float2 top = lerp(decode_dfg_pair(specular_dfg.Load(int3(tap.lo.x, tap.lo.y, 0))),
                      decode_dfg_pair(specular_dfg.Load(int3(tap.hi.x, tap.lo.y, 0))),
                      tap.weight.x);
    float2 bottom = lerp(decode_dfg_pair(specular_dfg.Load(int3(tap.lo.x, tap.hi.y, 0))),
                         decode_dfg_pair(specular_dfg.Load(int3(tap.hi.x, tap.hi.y, 0))),
                         tap.weight.x);
    return lerp(top, bottom, tap.weight.y);
}

/// The four entries of the linearly transformed cosine's inverse matrix at
/// `tap`, in `crcbl_shaders::ltc`'s order.
///
/// `dfg_at`'s filter over the other table; the texels are half floats rather
/// than fixed-point pairs, so there is nothing to decode.
float4 ltc_at(TableTap tap)
{
    float4 top = lerp(ltc_matrix.Load(int3(tap.lo.x, tap.lo.y, 0)),
                      ltc_matrix.Load(int3(tap.hi.x, tap.lo.y, 0)),
                      tap.weight.x);
    float4 bottom = lerp(ltc_matrix.Load(int3(tap.lo.x, tap.hi.y, 0)),
                         ltc_matrix.Load(int3(tap.hi.x, tap.hi.y, 0)),
                         tap.weight.x);
    return lerp(top, bottom, tap.weight.y);
}

/// What the specular lobe is multiplied by to put back the light a single
/// scatter dropped.
///
/// `1 + f0 * (1 / E - 1)` per channel, where `E` is the directional albedo —
/// `dfg_at`'s two channels added, which is the table at `f0 = 1`. A GGX
/// lobe follows light off the microsurface once and loses everything that bounces
/// again, so it returns `E` of what arrived and the shortfall grows with
/// roughness — two thirds of it on the roughest conductor this table holds. The
/// missing `1 - E` leaves after further bounces, each tinting it by the
/// surface's own reflectance, so the total wanted is `E + f0 (1 - E)` and this
/// is that over `E`. `crcbl_shaders::dfg::energy_compensation` carries the same
/// derivation and the same clamp.
///
/// **At most one below, never a dimming**: the albedo is clamped to one before
/// it is inverted, so a polished surface — where `E` is one and the table's
/// Monte Carlo noise can put it a hair above — comes back with the factor
/// exactly one and the smooth half of every material set is untouched.
float3 specular_compensation(float3 f0, float directional_albedo)
{
    float albedo = clamp(directional_albedo, MIN_DIRECTIONAL_ALBEDO, 1.0);
    float gain = 1.0 / albedo - 1.0;
    return float3(1.0, 1.0, 1.0) + f0 * gain;
}

/// The `pi` this file's two lobes folded out of themselves, put back once at the
/// end of a polygon integral.
///
/// `ggx_lobe`'s header has the convention: neither lobe carries its `1 / pi`,
/// because a light's intensity has absorbed it. The published edge fit below
/// carries a `1 / (2 pi)` instead — it is written to hand a shader the
/// Lambertian *response* rather than the irradiance — so this engine multiplies
/// it back. One constant, in one place, rather than three altered coefficients
/// nobody could check against the paper. `crcbl_shaders::ltc::LOBE_PI` is the
/// same number on the host.
static const float LOBE_PI = 3.14159265;

/// A spherical polygon mid-integration: up to five corners, and how many of them
/// survived the horizon.
///
/// Five because a quad clipped against a plane is a triangle, a quad or a
/// pentagon.
struct LtcPolygon
{
    /// The corners, relative to the shading point and already transformed.
    float3 corner[5];
    /// How many of them are real.
    int count;
};

/// The shading frame `crcbl_shaders::ltc`'s fit is expressed in: the normal at
/// `+z` and the view in the `+xz` half-plane, as the rows of a world-to-local
/// matrix.
///
/// **The degenerate case is a view exactly along the normal**, where the
/// tangent has no direction to point in and `normalize` would hand back a
/// nothing. It is not a corner case to be ignored: it is the middle of every
/// surface facing the camera. The fallback is any perpendicular, and it is
/// exactly right rather than merely safe — the table's head-on column has no
/// shear at all, so at that incidence the lobe is rotationally symmetric about
/// the normal and every choice of tangent gives the same answer.
float3x3 ltc_shading_frame(float3 normal, float3 to_eye, float n_dot_v)
{
    float3 across = to_eye - normal * n_dot_v;
    float span = length(across);
    float3 seed = abs(normal.z) < 0.9 ? float3(0.0, 0.0, 1.0) : float3(1.0, 0.0, 0.0);
    float3 tangent = span > 1e-4 ? across / span : normalize(cross(seed, normal));
    float3 bitangent = cross(normal, tangent);
    return float3x3(tangent, bitangent, normal);
}

/// The inverse of the fitted transform, from the four entries `ltc_at` read.
///
/// The middle entry is one because the integral below normalises every
/// transformed corner, so a positive scale of the whole matrix cannot change its
/// answer and the fit's fifth number was spent normalising the other four.
/// `crcbl_shaders::ltc::inverse_transform` is the same matrix on the host.
float3x3 ltc_transform(float4 entry)
{
    return float3x3(entry.x, 0.0, entry.y,
                    0.0, 1.0, 0.0,
                    entry.z, 0.0, entry.w);
}

/// One edge's contribution to the clamped-cosine integral: the `z` of the two
/// corners' cross product, scaled by `theta / (2 pi sin theta)`.
///
/// **The rational is the paper's own, and it is why this term is legal here.**
/// The exact factor is an `acos` over a `sin`, and
/// `docs/plan/44-lighting.md`'s rule is that no transcendental may reach a
/// colour; Heitz, Dupuy, Hill and Neubelt's shader replaces it with two
/// polynomials, a divide and a square root, every one of which IEEE-754
/// specifies. That is the same ground `crcbl_shaders::fog`'s exponential stands
/// on and the fourth escape from the rule this workspace uses.
///
/// The obtuse branch is the fit's own: the polynomial covers `cos theta >= 0`,
/// and past a right angle the identity
/// `theta / sin theta = pi / sin theta - (pi - theta) / sin(pi - theta)` reaches
/// it with the same polynomial evaluated at `|cos theta|` — which is the
/// `0.5 / sqrt(1 - x^2)` term once the `2 pi` is folded in.
///
/// Only the `z` of the cross product is wanted: the receiver's normal is `+z` in
/// this frame, and the edge term is that cross dotted with it.
/// `crcbl_shaders::ltc::edge_weight` is the same fit on the host.
float ltc_edge(float3 first, float3 second)
{
    float cosine = clamp(dot(first, second), -1.0, 1.0);
    float y = abs(cosine);
    float numerator = 0.8543985 + (0.4965155 + 0.0145206 * y) * y;
    float denominator = 3.4175940 + (4.1616724 + y) * y;
    float fit = numerator / denominator;
    // `0.5 / sqrt` rather than `rsqrt`: IEEE-754 requires a correctly rounded
    // square root and a correctly rounded divide, and specifies nothing at all
    // about a reciprocal square root — which is the difference between a term
    // four backends agree on and one they nearly do.
    float weight = cosine > 0.0 ? fit : 0.5 / sqrt(max(1.0 - cosine * cosine, 1e-7)) - fit;
    return (first.x * second.y - first.y * second.x) * weight;
}

/// `polygon` clipped against the `z > 0` half-space.
///
/// Transcribed from the paper's `ClipQuadToHorizon`: the configuration is the
/// four corners' sign bits, and each case replaces the corners below the horizon
/// with the points where their edges cross it — `-b.z * a + a.z * b`, which is
/// the crossing scaled by the two heights and therefore needs no divide. Two of
/// the sixteen configurations are opposite corners above with the other two
/// below, which a planar quad cannot be in, and they return nothing.
///
/// `crcbl_shaders::ltc`'s `clip_to_horizon` is the same table on the host, and
/// `the_polygon_integral_agrees_with_a_hemisphere_quadrature` is what says the
/// table is right — it fires rays at a rectangle and knows nothing about any of
/// these cases.
LtcPolygon ltc_clip(LtcPolygon polygon)
{
    float3 l0 = polygon.corner[0];
    float3 l1 = polygon.corner[1];
    float3 l2 = polygon.corner[2];
    float3 l3 = polygon.corner[3];
    float3 l4 = float3(0.0, 0.0, 0.0);

    int config = 0;
    config += l0.z > 0.0 ? 1 : 0;
    config += l1.z > 0.0 ? 2 : 0;
    config += l2.z > 0.0 ? 4 : 0;
    config += l3.z > 0.0 ? 8 : 0;

    int count = 0;
    if (config == 1)
    {
        count = 3;
        l1 = -l1.z * l0 + l0.z * l1;
        l2 = -l3.z * l0 + l0.z * l3;
    }
    else if (config == 2)
    {
        count = 3;
        l0 = -l0.z * l1 + l1.z * l0;
        l2 = -l2.z * l1 + l1.z * l2;
    }
    else if (config == 3)
    {
        count = 4;
        l2 = -l2.z * l1 + l1.z * l2;
        l3 = -l3.z * l0 + l0.z * l3;
    }
    else if (config == 4)
    {
        count = 3;
        l0 = -l3.z * l2 + l2.z * l3;
        l1 = -l1.z * l2 + l2.z * l1;
    }
    else if (config == 6)
    {
        count = 4;
        l0 = -l0.z * l1 + l1.z * l0;
        l3 = -l3.z * l2 + l2.z * l3;
    }
    else if (config == 7)
    {
        count = 5;
        l4 = -l3.z * l0 + l0.z * l3;
        l3 = -l3.z * l2 + l2.z * l3;
    }
    else if (config == 8)
    {
        count = 3;
        l0 = -l0.z * l3 + l3.z * l0;
        l1 = -l2.z * l3 + l3.z * l2;
        l2 = l3;
    }
    else if (config == 9)
    {
        count = 4;
        l1 = -l1.z * l0 + l0.z * l1;
        l2 = -l2.z * l3 + l3.z * l2;
    }
    else if (config == 11)
    {
        count = 5;
        l4 = l3;
        l3 = -l3.z * l2 + l2.z * l3;
        l2 = -l2.z * l1 + l1.z * l2;
    }
    else if (config == 12)
    {
        count = 4;
        l1 = -l1.z * l2 + l2.z * l1;
        l0 = -l0.z * l3 + l3.z * l0;
    }
    else if (config == 13)
    {
        count = 5;
        l4 = l3;
        l3 = l2;
        l2 = -l2.z * l1 + l1.z * l2;
        l1 = -l1.z * l0 + l0.z * l1;
    }
    else if (config == 14)
    {
        count = 5;
        l4 = -l0.z * l3 + l3.z * l0;
        l0 = -l0.z * l1 + l1.z * l0;
    }
    else if (config == 15)
    {
        count = 4;
    }

    // The unused corners are set to the first rather than left as they were, so
    // the unrolled sum below closes the ring by adding an edge of zero length
    // instead of normalising whatever the clip happened to leave.
    if (count <= 3)
    {
        l3 = l0;
        l4 = l0;
    }
    else if (count == 4)
    {
        l4 = l0;
    }

    LtcPolygon clipped;
    clipped.corner[0] = l0;
    clipped.corner[1] = l1;
    clipped.corner[2] = l2;
    clipped.corner[3] = l3;
    clipped.corner[4] = l4;
    clipped.count = count;
    return clipped;
}

/// The clamped-cosine integral over the spherical polygon `corners` subtends at
/// the shading point, under `transform`.
///
/// `transform` is the world-to-local frame alone for the diffuse term and that
/// frame under the fitted matrix for the specular one — which is the whole of
/// what a linearly transformed cosine is. A polygon covering the hemisphere
/// answers `LOBE_PI`; one entirely below the horizon, or wound so the receiver
/// is behind it, answers zero.
///
/// `crcbl_shaders::ltc::polygon_irradiance` is the same integral on the host.
float ltc_irradiance(float3x3 transform, float3 corners[4])
{
    LtcPolygon polygon;
    for (int corner = 0; corner < 4; ++corner)
    {
        polygon.corner[corner] = mul(transform, corners[corner]);
    }
    polygon.corner[4] = float3(0.0, 0.0, 0.0);
    polygon.count = 4;
    polygon = ltc_clip(polygon);
    if (polygon.count == 0)
    {
        return 0.0;
    }
    for (int at = 0; at < 5; ++at)
    {
        polygon.corner[at] = normalize(polygon.corner[at]);
    }

    float sum = ltc_edge(polygon.corner[0], polygon.corner[1]);
    sum += ltc_edge(polygon.corner[1], polygon.corner[2]);
    sum += ltc_edge(polygon.corner[2], polygon.corner[3]);
    if (polygon.count >= 4)
    {
        sum += ltc_edge(polygon.corner[3], polygon.corner[4]);
    }
    if (polygon.count == 5)
    {
        sum += ltc_edge(polygon.corner[4], polygon.corner[0]);
    }
    // One-sided: a negative sum is a polygon wound the other way round, which
    // is a surface behind the panel. `crcbl_render::RectLight` carries the
    // winding that makes "in front" the positive one.
    return max(sum, 0.0) * LOBE_PI;
}

/// A rectangle light's four corners, relative to the shading point.
///
/// The row carries the `u` axis and both half-extents; `v` is
/// `cross(tangent.xyz, direction.xyz)`, which is unit because those two are unit
/// and perpendicular. The order is what makes `ltc_irradiance` positive for a
/// receiver the rectangle faces — see `crcbl_shaders::light::KIND_RECT`.
void rect_corners(GpuLight light, float3 world_position, out float3 corners[4])
{
    float3 across = light.tangent.xyz * light.tangent.w;
    float3 down = cross(light.tangent.xyz, light.direction.xyz) * light.direction.w;
    float3 centre = light.position.xyz - world_position;
    corners[0] = centre - across - down;
    corners[1] = centre + across - down;
    corners[2] = centre + across + down;
    corners[3] = centre - across + down;
}

/// `log2(e)`, mirroring `crcbl_shaders::fog::LOG2_E`.
///
/// The four fog constants and the two kernels below are the Slang half of that
/// module. There is no `#include` in these shaders, so the copy is the only way
/// to share the arithmetic, and
/// `crcbl_shaders::fog::the_shader_spells_the_same_constants` is what stops the
/// two drifting.
static const float FOG_LOG2_E = 1.44269504;

/// `ln 2` with its low mantissa bits cleared, so `n * FOG_LN2_HI` is exact.
/// `crcbl_shaders::fog::LN2_HI`.
static const float FOG_LN2_HI = 0.69311523;

/// What `FOG_LN2_HI` dropped. `crcbl_shaders::fog::LN2_LO`.
static const float FOG_LN2_LO = 3.1946183e-5;

/// The largest magnitude `fog_exp_neg` takes before it saturates, chosen so the
/// reduced exponent stays normal at both ends.
/// `crcbl_shaders::fog::MAX_ARGUMENT`.
static const float FOG_MAX_ARGUMENT = 87.0;

/// Optical depth past which `fog_optical_depth` stops counting.
/// `crcbl_shaders::fog::MAX_OPTICAL_DEPTH`.
static const float FOG_MAX_OPTICAL_DEPTH = 32.0;

/// Below this reduced height difference the ratio below takes its series.
/// `crcbl_shaders::fog::SERIES_CUTOFF`.
static const float FOG_SERIES_CUTOFF = 0.125;

/// Reciprocal factorials, `1/i!`. `crcbl_shaders::fog::KERNEL`.
static const float FOG_KERNEL[8] = {
    1.0, 1.0, 0.5, 0.16666667, 0.041666668, 0.008333334, 0.0013888889, 0.00019841270
};

/// The same one factorial along, `1/(i+1)!`. `crcbl_shaders::fog::RATIO_KERNEL`.
static const float FOG_RATIO_KERNEL[5] = {
    1.0, 0.5, 0.16666667, 0.041666668, 0.008333334
};

/// `e^-x`, without calling a transcendental.
///
/// **This is why fog could be built at all.** This workspace's rule is that no
/// transcendental may reach a colour — four platforms' implementations of them
/// differ in the last place — and `docs/plan/43-render-standards.md` §4 held
/// exponential height fog as a decision rather than a slice for exactly that
/// reason. The way out is to build the function from the operations the rule
/// permits: range reduction against a two-part `ln 2`, a Taylor kernel in
/// Horner form over the reciprocal factorials, and `2^-n` written straight into
/// an IEEE exponent field. Every step is an operation IEEE-754 specifies
/// exactly, so the only freedom a compiler keeps is whether to contract a
/// multiply and an add — one unit in the last place, four orders of magnitude
/// inside what the goldens compare under.
///
/// `crcbl_shaders::fog::exp_neg` is the same arithmetic on the host, measured
/// there against `f64::exp`.
float fog_exp_neg(float x)
{
    float clamped = clamp(x, -FOG_MAX_ARGUMENT, FOG_MAX_ARGUMENT);

    // e^-x = 2^-n * e^-r, with |r| at most half of ln 2.
    float n = floor(clamped * FOG_LOG2_E + 0.5);
    float reduced = (clamped - n * FOG_LN2_HI) - n * FOG_LN2_LO;

    float u = -reduced;
    float kernel = FOG_KERNEL[7];
    for (int term = 6; term >= 0; --term)
    {
        kernel = kernel * u + FOG_KERNEL[term];
    }

    // 2^-n by exponent field. The clamp above is what keeps this inside the
    // normal range, so no bias arithmetic is needed for the subnormal end.
    return kernel * asfloat(uint(127 - int(n)) << 23);
}

/// `(1 - e^-d) / d`, which tends to one as `d` does.
///
/// The direct form loses every significant bit as `d` shrinks, its numerator
/// being a difference of two nearly equal numbers; below the cutoff this takes
/// the series that difference converges to, which has no cancellation in it.
/// A level ray — the commonest one in a frame — is exactly the case that needs
/// it. `crcbl_shaders::fog::one_minus_exp_over`.
float fog_one_minus_exp_over(float d)
{
    if (abs(d) < FOG_SERIES_CUTOFF)
    {
        float u = -d;
        float series = FOG_RATIO_KERNEL[4];
        for (int term = 3; term >= 0; --term)
        {
            series = series * u + FOG_RATIO_KERNEL[term];
        }
        return series;
    }
    return (1.0 - fog_exp_neg(d)) / d;
}

/// The optical depth of exponential height fog along one ray.
///
/// `height_a` and `height_b` are the segment's two ends measured from the fog
/// reference plane and `distance` is its length, so the integral of a density
/// that falls off as `e^(-h/falloff)` is
///
/// ```text
/// tau = density * distance * e^-a * (1 - e^-d) / d
/// ```
///
/// with `a = height_a / falloff` and `d = (height_b - height_a) / falloff`.
/// Factored so the leading exponential comes out front, which is what puts a
/// level ray on the series branch above rather than on a division by nearly
/// zero. Symmetric in the two ends, as the integral it evaluates is.
///
/// A `falloff` of zero or less is fog that does not thin with height, and gives
/// `density * distance` — the limit the height form approaches as the falloff
/// grows, so the branch is a continuation and not a special case.
/// `crcbl_shaders::fog::optical_depth`.
float fog_optical_depth(float density, float falloff, float height_a, float height_b,
                        float distance)
{
    if (falloff <= 0.0)
    {
        return clamp(density * distance, 0.0, FOG_MAX_OPTICAL_DEPTH);
    }

    float a = height_a / falloff;
    float d = (height_b - height_a) / falloff;
    float tau = density * distance * fog_exp_neg(a) * fog_one_minus_exp_over(d);
    return clamp(tau, 0.0, FOG_MAX_OPTICAL_DEPTH);
}

/// How much of a surface's radiance survives the fog between it and the eye.
///
/// Exactly one where the density is zero, which is what makes the composite in
/// `fragmentMain` an identity on an unfogged frame.
/// `crcbl_shaders::fog::transmittance`.
float fog_transmittance(float optical_depth)
{
    return fog_exp_neg(max(optical_depth, 0.0));
}

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

/// How far along its own normal a receiver is moved before its shadow lookup,
/// per texel of footprint: `sin(acos(Ng·L))`.
///
/// # Sideways across the map, not towards the light
///
/// This is the **normal-offset bias**, and the direction is the whole of it. A
/// bias along `to_light` raises the depth a fragment compares with, so enough of
/// it to clear acne on a grazing surface is also enough to lift the contact
/// point of a shadow off its caster — peter-panning, and the two are the same
/// number pulling in opposite directions. Moving along the *normal* instead
/// leaves the compared depth where it was and moves which texel is read, so what
/// it buys is a sample that belongs to the receiver rather than to the facet
/// climbing across it, and a contact shadow keeps its contact.
///
/// # Why the sine, and why nothing clamps it
///
/// A surface square to the light has no depth slope across a texel and needs no
/// offset; one edge-on to it needs a full texel of lateral travel, and the
/// lateral component of a step along the normal is `sin` of the angle between
/// them. So the term is `sin(acos(Ng·L))`, which is **bounded by one** — and
/// that is the second thing this direction buys. `tan`, which is what a
/// slope-scaled *depth* bias is denominated in, runs to infinity as a surface
/// turns edge-on and had to be clamped; the clamp is gone with it, and the
/// worst case is now a bounded quantity rather than a number chosen to stop an
/// unbounded one.
///
/// One function for the sun and for the punctual lights, because it is one piece
/// of knowledge: what an offset in texels has to be multiplied by.
///
/// `geometric_normal` is `geometric_normal_of`'s, and passing the *shading*
/// normal here is the defect that function exists to remove: the lateral travel
/// that escapes a texel is a fact about the triangle the rasteriser drew into
/// the map, not about the field the corners interpolate. A facet turned away
/// from the light saturates to a zero cosine and therefore to a full texel of
/// offset, which is the conservative answer: such a fragment is at or past the
/// terminator of the surface that shadows it, and it is where acne is worst.
float shadow_normal_offset(float3 geometric_normal, float3 to_light)
{
    float cosine = saturate(dot(geometric_normal, to_light));
    return sqrt(saturate(1.0 - cosine * cosine));
}

/// How many taps the shadow filter takes.
///
/// Thirty-two, against the nine a 3×3 box took, and the count is **not** what
/// widened the penumbra — `SHADOW_FILTER_TEXELS` is. What it buys is the number
/// of values the filter can return: `SHADOW_TAPS + 1` of them, and the rotation
/// below turns the gap between two of those values into a dither pattern rather
/// than a band. So the count is what sets how coarse that dither is, and the
/// two scenes that see it disagree about how many are enough.
///
/// `apps/lantern`'s wall shaft is a penumbra magnified across a hundred pixels:
/// twelve taps put a plain 4-pixel stipple through it, sixteen still showed one,
/// and by twenty-four it was gone. The dunes terrain is the harder case — one
/// large smooth surface in partial shadow, where a dither reads as texture — and
/// there it is measured rather than looked at, as the RMS of each pixel about
/// its own 5×5 neighbourhood over a patch that is one gradient:
///
/// | Taps          | Grain over the dunes patch |
/// | ------------- | -------------------------- |
/// | 9 (the box)   | 0.827                      |
/// | 16            | 1.459                      |
/// | 24            | 1.099                      |
/// | 32 (shipped)  | 0.918                      |
///
/// Thirty-two is where what this filter adds falls back to what the box it
/// replaced already had. Widening or narrowing the disc does not move that
/// number — 24 taps over a 1.5-texel disc measured 1.129, slightly worse than
/// the same taps over two texels — so the count is the only knob it has.
///
/// **The count was chosen on the picture alone**, and it stays that way: what
/// moved once it was measured is the *pass*, not the count. `crcbl_render`'s
/// `PassTimers` prices the forward pass this filter runs in — 0.221 ms at
/// 1920x1080 on an RX 7900 XTX against 0.303 ms without the probe below — and
/// `docs/plan/45-shadows.md`'s eleventh decision has both adapters. Nothing
/// prices thirty-two taps against sixteen, which is what a graphics-quality
/// tier would need, and this is still the first constant such a setting should
/// reach for.
///
/// What a frame actually pays is lower than this count, and it is
/// [`SHADOW_PROBE_TAPS`] that decides where: the disc is taken in full only
/// where a shorter probe of it could not settle the fragment.
static const uint SHADOW_TAPS = 32;

/// How far the filter reaches from the fragment's own texel, in tile texels.
///
/// The tap positions below are a unit disc, so this is the disc's radius and
/// the whole of the penumbra's width. Two texels against the box's one, which on
/// the outer cascade is 125 mm of penumbra and on the near one 21 mm.
///
/// **A wider reach buys acne back**, because a tap two texels out reads a depth
/// two texels' worth of slope away from the receiver's own. Three texels put the
/// dunes patch's self-shadowing dots up from 24 to 155; two put them at 47,
/// which is what `crcbl_render::shadow::DEPTH_BIAS_TEXELS`' half-texel rise
/// paid back.
static const float SHADOW_FILTER_TEXELS = 2.0;

/// The tap positions, a **Vogel disc** of [`SHADOW_TAPS`] points on the unit
/// disc.
///
/// # The formula, because a table of magic numbers is not checkable
///
/// Tap `i` of `n` is at radius `sqrt((i + 0.5) / n)` and angle
/// `i * π (3 - sqrt 5)` — the sunflower spiral, whose radius is the one that
/// gives every tap the same share of the disc's area and whose angle is the
/// golden angle. Each literal below is that expression evaluated, and
/// `crcbl_shaders::mesh`'s `the_shadow_disc_is_the_vogel_spiral_it_claims_to_be`
/// re-derives every one of them rather than trusting the transcription.
///
/// **It is not a Poisson disc**, which is what
/// `docs/plan/45-shadows.md`'s ladder asked for, and the difference is
/// deliberate. A Poisson set has no closed form: it comes out of dart-throwing
/// or best-candidate, so it arrives as two dozen constants whose provenance is a
/// program nobody kept. A Vogel spiral has slightly worse spectral behaviour and
/// a generator any reader can evaluate, and with the rotation below it is what
/// the technique actually needs — taps that do not line up between neighbouring
/// fragments.
static const float2 SHADOW_DISC[SHADOW_TAPS] = {
    float2(+0.125000, +0.000000), float2(-0.159645, +0.146248),
    float2(+0.024436, -0.278438), float2(+0.201222, +0.262459),
    float2(-0.369268, -0.065318), float2(+0.349802, -0.222516),
    float2(-0.117002, +0.435242), float2(-0.223136, -0.429634),
    float2(+0.484115, +0.176798), float2(-0.503641, +0.207896),
    float2(+0.242788, -0.518824), float2(+0.179414, +0.572001),
    float2(-0.540757, -0.313380), float2(+0.634370, -0.139464),
    float2(-0.387146, +0.550675), float2(-0.089440, -0.690200),
    float2(+0.549072, +0.462758), float2(-0.738878, +0.030555),
    float2(+0.538955, -0.536332), float2(-0.036058, +0.779792),
    float2(-0.512818, -0.614527), float2(+0.812360, +0.109302),
    float2(-0.688311, +0.478909), float2(+0.188086, -0.836061),
    float2(+0.435033, +0.759191), float2(-0.850448, -0.271316),
    float2(+0.826102, -0.381680), float2(-0.357888, +0.855156),
    float2(-0.319407, -0.888034), float2(+0.849909, +0.446688),
    float2(-0.944035, +0.248845), float2(+0.536596, -0.834530),
};

/// How many of [`SHADOW_DISC`]'s taps the filter reads before it decides whether
/// to read the rest.
///
/// # Why there is a probe at all
///
/// Thirty-two taps is what a **penumbra** costs, and almost none of a frame is
/// penumbra. A shadowed surface away from any edge answers zero at all
/// thirty-two; a lit one answers one at all thirty-two; both spend the whole
/// filter to learn what five taps already said. So the disc is preceded by a
/// probe of five taps spread over that same disc, and taken in full only where
/// the five disagree.
///
/// The five are **read again** by the loop rather than carried into its sum.
/// That costs five taps on a penumbra fragment and buys the property that makes
/// the change reviewable: where the probe disagrees, this filter returns
/// bit-for-bit what it returned before the probe existed, so every frame that
/// moved is a frame the probe called unanimous.
///
/// # What it changes
///
/// A probe of five can be unanimous where the disc of thirty-two is not — a
/// caster thinner than the gap between two probe taps that also misses the
/// middle one — and there the filter answers a flat zero or one where it
/// answered a fraction. That is a visual change rather than a free
/// optimisation, and `docs/plan/45-shadows.md`'s eleventh decision is where the
/// frames it moved are recorded.
static const uint SHADOW_PROBE_TAPS = 5;

/// Which of [`SHADOW_DISC`]'s taps the probe reads: the innermost, and four
/// near the rim.
///
/// The four rim taps are chosen for their **angles**. The spiral turns by the
/// golden angle at each step, so every *second* tap turns by twice it, which is
/// 85 degrees short of a whole turn — taps 23, 25, 27 and 29 therefore sit at
/// 283, 198, 113 and 28 degrees, three gaps of 85 degrees and one of 105, at
/// radii from 0.857 to 0.960. Four taps a quarter turn apart is what makes the
/// probe a ring rather than a direction, and tap 0 at radius 0.125 is what a
/// caster small enough to sit inside that ring falls on.
///
/// `crcbl_shaders::mesh`'s `the_shadow_probe_is_a_ring_about_a_centre`
/// re-derives both halves from the disc rather than pinning a copy of the list.
static const uint SHADOW_PROBE_INDEX[SHADOW_PROBE_TAPS] = { 0u, 23u, 25u, 27u, 29u };

/// The sixteen rotations the disc is taken through, as `(cos, sin)` pairs of
/// `2π k / 16`.
///
/// **A table rather than a `sincos`, and integer-indexed rather than hashed.**
/// The usual spelling of this technique hashes the pixel coordinate to a float
/// angle and rotates by its sine and cosine. Both halves of that are wrong here:
/// a hash multiplies a large coordinate by a large constant and keeps the
/// fraction, which is arithmetic whose low bits differ between drivers by
/// construction, and `sin`/`cos` are specified to no accuracy anyone can quote.
/// This file's shading rule refuses a transcendental for exactly that reason,
/// and a shadow comparison is binary — a tap that lands the other side of an
/// edge is a whole tap of difference, not a rounding one. So the angles are
/// constants and the index is an integer.
static const float2 SHADOW_ROTATIONS[16] = {
    float2(+1.000000, +0.000000), float2(+0.923880, +0.382683),
    float2(+0.707107, +0.707107), float2(+0.382683, +0.923880),
    float2(+0.000000, +1.000000), float2(-0.382683, +0.923880),
    float2(-0.707107, +0.707107), float2(-0.923880, +0.382683),
    float2(-1.000000, +0.000000), float2(-0.923880, -0.382683),
    float2(-0.707107, -0.707107), float2(-0.382683, -0.923880),
    float2(-0.000000, -1.000000), float2(+0.382683, -0.923880),
    float2(+0.707107, -0.707107), float2(+0.923880, -0.382683),
};

/// Which rotation each pixel of a 4×4 cell takes: the **ordered-dither matrix**,
/// read row-major.
///
/// The point of a permutation rather than a straight lattice is that
/// neighbouring pixels must land far apart in the rotation table *without* the
/// difference being the same everywhere. `(3x + 4y) mod 16` keeps neighbours
/// three and four apart and was tried first; because that difference is
/// constant, the dither it leaves on a large smooth surface is a set of diagonal
/// stripes, and it measured a fifth grainier than this matrix on the dunes
/// terrain — 1.187 against 1.099. The ordered matrix's differences vary, so what
/// it leaves is a texture with no direction to it.
///
/// This is the classic recursive construction, in which each quadrant's entries
/// are placed as far from its neighbours' as the level above allows.
///
/// `crcbl_shaders::mesh`'s `the_shadow_dither_covers_every_rotation` is what
/// holds the properties this rests on.
static const uint SHADOW_DITHER[16] = {
    0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u,
};

/// The world distance in front of a cascade's near plane that still writes
/// depth, and therefore the part of that cascade's depth range no receiver
/// occupies.
///
/// The same number is `crcbl_render::shadow`'s `CASTER_REACH`, which takes it
/// from here — a caster standing between the sun and the cascade's sphere is
/// outside the sphere and must still darken what is inside it, so the light's
/// box is pulled back this far. What the sampling side needs it for is the
/// **inverse** of that arithmetic: a cascade of radius `r` is an orthographic
/// box `2 r + SHADOW_CASTER_REACH` deep, and that is the factor turning a
/// difference of two shadow-clip depths back into metres, which is the only
/// unit a penumbra can be measured in.
static const float SHADOW_CASTER_REACH = 40.0;

/// The tangent of the sun's angular radius, which is what makes a penumbra
/// widen with distance from its caster.
///
/// A blocker `s` metres above a receiver casts a penumbra `s` times this wide,
/// because the sun is a disc rather than a point and the geometry of the
/// half-shadow is a similar triangle. That is the whole of contact hardening:
/// the number does not depend on the scene, only on how large the light looks
/// from it.
///
/// # The physical number is a no-op here, and that was measured
///
/// **The real sun's is 0.004634** — half of a 0.5310° disc, tangent taken. A
/// cascade texel is `2 r` over the cascade's own `tile_texels`: 10.6 mm on the
/// near cascade of `apps/lantern`'s split and 62.5 mm on the outer one, so the
/// physical sun
/// needs 4.6 m of blocker separation before its penumbra reaches even
/// [`SHADOW_FILTER_TEXELS`] on the near cascade, and 27 m on the outer one.
/// Nothing in either test scene casts that far. Rendered at the physical value
/// against the same frame with this at zero, `apps/lantern` differed in **36
/// bytes of 4,915,200 by one** and the dunes terrain was byte-identical.
///
/// So this is a **softness knob whose unit happens to be an angle**, not a
/// measurement of the sun, and the shipped value is a sun about four times
/// oversized — `atan(0.02)` is 1.15°, against the real disc's 0.27° radius.
/// What buys the honesty back is that everything downstream of it *is*
/// physical: the estimate is a similar triangle over a real blocker height, so
/// the width still tracks the scene rather than a fudge per material.
///
/// # The sweep that picked it
///
/// The artefact this rung exists to remove is the staircase on
/// `apps/lantern`'s far shadow boundary. The outer cascade's texels are 62.5 mm
/// and the edge is quantised to them, which reads as a sawtooth because a
/// two-texel filter cannot cover a penumbra that wants twenty. Measured as the
/// RMS departure of that edge from the straight line it should be, each row
/// smoothed by nine pixels so the filter's own dither cancels:
///
/// | This constant       | Edge wobble | Dunes acne | Dunes grain |
/// | ------------------- | ----------- | ---------- | ----------- |
/// | 0 (a fixed filter)  | 1.58 px     | 24 dots    | 0.918       |
/// | 0.010               | 1.13 px     | 24 dots    | 0.918       |
/// | 0.015               | 0.50 px     | 24 dots    | 0.918       |
/// | 0.020 (shipped)     | 0.58 px     | 24 dots    | 0.918       |
/// | 0.030               | 1.07 px     | 25 dots    | 0.942       |
/// | 0.050               | 1.08 px     | 23 dots    | 1.005       |
/// | 0.100               | 1.08 px     | 43 dots    | 1.222       |
///
/// The minimum is a tie between 0.015 and 0.02 — 0.08 px apart, which is under
/// what one scene resolves — and the wider of the two is taken because the
/// effect it buys away from this one edge is larger at no measured cost. Past
/// 0.03 the number stops moving because the estimate saturates at
/// [`SHADOW_SEARCH_TEXELS`], and 1.08 px is what the fully clamped filter
/// leaves; acne and grain keep climbing anyway, which is the ninth decision's
/// finding that a wider disc buys acne back.
static const float SHADOW_SUN_TAN_RADIUS = 0.02;

/// How far the blocker search reaches, in tile texels, and therefore the widest
/// penumbra the filter is allowed to draw.
///
/// The two are one number because a penumbra wider than the search is a penumbra
/// sized from blockers the search did not look at: the filter would spread taps
/// across texels nothing measured, and the width would be whatever the nearest
/// blocker inside the search happened to be. Clamping the filter to the search
/// makes the estimate's domain and the filter's domain the same set of texels.
///
/// Eight, four times [`SHADOW_FILTER_TEXELS`], and it binds: the sweep in
/// [`SHADOW_SUN_TAN_RADIUS`] stops moving past 0.03 because `apps/lantern`'s
/// far shadow boundary has saturated here. That is the intended shape — the
/// clamp is what stops a distant caster asking for a filter the atlas cannot
/// pay for — and 500 mm of penumbra on the outer cascade is already more than
/// any of that sweep's frames wanted.
static const float SHADOW_SEARCH_TEXELS = 8.0;

/// How many taps the blocker search takes.
///
/// Half the filter's, and they are buying a different thing: the filter's count
/// sets how many values it can return, and this one only has to find *a*
/// representative blocker depth. An average over sixteen is already stable
/// enough that the radius it produces does not flicker between neighbouring
/// pixels, and the rotation below is shared with the filter so the two discs
/// turn together.
static const uint SHADOW_SEARCH_TAPS = 16;

/// The blocker search's tap positions: a **Vogel disc** of [`SHADOW_SEARCH_TAPS`]
/// points, generated exactly as [`SHADOW_DISC`] is and re-derived by the same
/// test.
///
/// Its own table rather than a stride through [`SHADOW_DISC`], because a Vogel
/// spiral's radii depend on the count it was generated for: taking every other
/// point of a thirty-two point disc gives sixteen points whose radii still run
/// `sqrt((i + 0.5) / 32)`, which is a disc with a hole in its rim rather than a
/// smaller disc.
static const float2 SHADOW_SEARCH_DISC[SHADOW_SEARCH_TAPS] = {
    float2(+0.176777, +0.000000), float2(-0.225772, +0.206826),
    float2(+0.034558, -0.393771), float2(+0.284571, +0.371173),
    float2(-0.522223, -0.092374), float2(+0.494695, -0.314685),
    float2(-0.165466, +0.615525), float2(-0.315561, -0.607594),
    float2(+0.684642, +0.250030), float2(-0.712256, +0.294009),
    float2(+0.343354, -0.733729), float2(+0.253730, +0.808932),
    float2(-0.764746, -0.443186), float2(+0.897134, -0.197232),
    float2(-0.547507, +0.778772), float2(-0.126487, -0.976090),
};

/// Which of [`SHADOW_ROTATIONS`] this pixel takes, as a `(cos, sin)` pair.
///
/// A function rather than two lines in each caller because the blocker search
/// and the filter it sizes have to agree: a search that walked a differently
/// turned disc would measure a penumbra the filter then does not draw, and the
/// disagreement would be a per-pixel one, which reads as noise rather than as a
/// wrong number. Sharing the lookup makes agreement structural.
///
/// `pixel` is the fragment's `SV_Position.xy`. Only its low two bits per axis
/// are read, so the value's magnitude and its sub-pixel fraction never enter the
/// arithmetic.
float2 shadow_rotation(float2 pixel)
{
    uint2 cell = uint2(pixel) & 3u;
    return SHADOW_ROTATIONS[SHADOW_DITHER[cell.y * 4u + cell.x]];
}

/// Where atlas slot `tile`'s map is in the image, out of the frame block:
/// `FrameUniforms::shadow_atlas_rect`, and that member is where the layout is
/// argued.
///
/// A function rather than the subscript written out at each of the three places
/// that want it, so the block member is named once and
/// `crcbl_shaders::volumetric`'s drift test has one body to compare.
float4 atlas_rect(uint tile)
{
    return frame.shadow_atlas_rect[tile];
}

/// One *atlas* texel as a step in a tile's own `0..1` space.
///
/// The map at `rect` is `rect.xy` of the atlas across, so dividing the atlas's
/// texel size by it is what a fixed grid's multiply by its column and row counts
/// used to be. It is also the whole of why a tile may now be smaller than its
/// neighbour: a half-sized map steps twice as far per texel, and every clamp and
/// every spoke below is written in these steps rather than in the atlas's.
float2 atlas_step(float4 rect)
{
    return frame.shadow_params.xy / rect.xy;
}

/// How many texels the map at `rect` is across.
///
/// [`atlas_step`]'s reciprocal, written as one divide: the map is `rect.x` of
/// the atlas wide and one atlas texel is `frame.shadow_params.x` of it, so their
/// ratio is the map's own side. Square, because a tile of the quadtree is.
///
/// **Every shadow bias in this file is denominated in the texels of the map
/// being sampled**, and until `docs/plan/45-shadows.md`'s priority rung that was
/// one number for every map — a whole root cell — declared here as
/// `SHADOW_TILE`. A light demoted to a halving of a cell has half the texels
/// across and twice the world footprint per texel, so biasing it by the cell's
/// side under-biases it by exactly the factor it was demoted by, which is acne
/// on that light's receivers and on no other's. Reading the side out of the
/// rectangle the host allocated is what makes one pair of bias constants mean
/// the same thing at every size.
float tile_texels(float4 rect)
{
    return rect.x / frame.shadow_params.x;
}

/// Whether `rect` names a map at all.
///
/// **A rectangle of zeros is what the host writes for a slot no map was
/// rendered into** — `crcbl_render::shadow::TileRect::EMPTY`, whose `to_uv` is
/// four zeros, and every row of the block is that on a frame with no cascade
/// and no shadowed light. So this is host data that is legitimately zero, and
/// every arithmetic below that touches a rectangle **divides by it**:
/// `tile_texels` by `rect.x` and `atlas_step` by `rect.xy`.
///
/// `0.0 / 0.0` is a `NaN`, and a `NaN` does not merely produce a wrong number —
/// it walks past the guard that would have caught it, because every comparison
/// against a `NaN` is false. `cascade_visibility`'s "outside this cascade's
/// box" test is `any(abs(ndc.xy) > 1.0) || ndc.z <= 0.0`, and with a `NaN`
/// `ndc` both halves are false and the fragment goes on to sample the atlas at
/// `NaN` coordinates. What that draws is the driver's business: on llvmpipe
/// (LLVM 20.1.2) it is zero visibility and a **black frame**, which is how
/// `crates/crcbl/tests/forward_e2e/depth_probe.rs` found it, and on llvmpipe
/// (LLVM 22.1.8) and radv it is not.
///
/// So every caller answers **lit** as soon as it sees this, before it divides —
/// on the same reasoning the past-the-far-plane return already gives: a map
/// that was never rendered knows nothing, and inventing a shadow from it is
/// worse than admitting it.
bool atlas_rect_is_empty(float4 rect)
{
    return !(rect.x > 0.0);
}

/// One tap of the shadow filter: `spoke` turned by `rotation`, clamped into the
/// map at `rect`, and compared against `reference`.
///
/// One tap for the cascades and for the light maps, which is what keeps "a tap
/// never leaves its own tile" a property of one function. The clamp is half a
/// texel inside the tile: the comparison sampler filters a 2×2 neighbourhood
/// around each tap, so a tap exactly on the edge would still reach across it and
/// read the neighbouring map's depths as if they were this one's.
///
/// `rect` and `texel_step` are parameters rather than a slot index this would
/// look up: every tap [`tile_pcf`] takes — its probe's and its disc's alike —
/// reads one map, so the pair is the same for all of them and the block read
/// and the divide happen once per map rather than once per tap.
///
/// It is a function rather than four lines written twice because
/// [`tile_pcf`]'s probe and its disc take exactly the same tap; two copies of
/// this arithmetic would be two places a clamp could be fixed in one of.
float tile_tap(float4 rect, float2 texel_step, float2 tile_uv, float2 spoke, float2 rotation,
               float reference)
{
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The rotation, written out: a complex multiply by `(cos, sin)`.
    float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                           spoke.x * rotation.y + spoke.y * rotation.x);
    float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
    return shadow_atlas.SampleCmpLevelZero(shadow_sampler, atlas_uv(rect, tap), reference);
}

/// Rotated-disc PCF over tile `tile`, at `tile_uv` in that tile's own `0..1`
/// space, dithered by the fragment's pixel coordinate.
///
/// [`SHADOW_PROBE_TAPS`] taps decide whether the other twenty-seven are worth
/// taking: a neighbourhood the probe finds wholly lit or wholly shadowed
/// returns without them. That is where nearly all of this filter's cost goes
/// on a real frame, and it is the only part of this function that is not what
/// the ninth and tenth decisions shipped.
///
/// `radius` is the disc's reach in tile texels, and it is a parameter rather
/// than [`SHADOW_FILTER_TEXELS`] because the sun's penumbra is not a constant
/// width — `sun_penumbra_texels` measures one per fragment. Everything that
/// reads a map the sun does not own passes the constant, which is what it was
/// before this became a parameter. The probe reaches as far as the disc does,
/// so a wide penumbra is probed wide and a contact is probed tight.
///
/// `pixel` is the fragment's `SV_Position.xy`, and it is a parameter for
/// `geometric_normal_of`'s reason: a shader stage that has no such thing must
/// not be able to call this by accident.
float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)
{
    float2 rotation = shadow_rotation(pixel);
    float4 rect = atlas_rect(tile);
    if (atlas_rect_is_empty(rect))
    {
        return 1.0;
    }
    float2 texel_step = atlas_step(rect);

    float probe = 0.0;
    for (uint spot = 0u; spot < SHADOW_PROBE_TAPS; ++spot)
    {
        probe += tile_tap(rect, texel_step, tile_uv,
                          SHADOW_DISC[SHADOW_PROBE_INDEX[spot]] * radius, rotation, reference);
    }
    // Unanimity, tested at both ends rather than by a tolerance: a comparison
    // sampler returns exactly zero or exactly one wherever the four texels it
    // filters agree, whatever weights it gave them, so the sum of five is an
    // exact integer on every fragment this early-out is meant for.
    if (probe <= 0.0)
    {
        return 0.0;
    }
    if (probe >= float(SHADOW_PROBE_TAPS))
    {
        return 1.0;
    }

    float visibility = 0.0;
    for (uint index = 0u; index < SHADOW_TAPS; ++index)
    {
        visibility += tile_tap(rect, texel_step, tile_uv, SHADOW_DISC[index] * radius, rotation,
                               reference);
    }
    return visibility / float(SHADOW_TAPS);
}

/// How wide the sun's penumbra is at this fragment, in texels of cascade
/// `cascade`, ready for [`tile_pcf`]'s `radius`.
///
/// # The estimate
///
/// This is PCSS's first half: a **blocker search**. It reads
/// [`SHADOW_SEARCH_TAPS`] depths straight out of the map over
/// [`SHADOW_SEARCH_TEXELS`], keeps the ones nearer the light than this fragment
/// — reversed-Z, so nearer is *larger* — and averages them. That average is how
/// far above the receiver the thing shadowing it stands, and
/// [`SHADOW_SUN_TAN_RADIUS`] turns a height into a half-shadow's width.
///
/// **`Load`, not a sampler.** A blocker depth is a stored value, and filtering
/// four of them would average across a silhouette and report a blocker at a
/// height nothing in the scene occupies. It is also why this needs no second
/// sampler object: the atlas's only sampler is the comparison one the filter
/// uses, and a comparison sampler cannot return a depth.
///
/// # Why the result is clamped at both ends
///
/// Below, at [`SHADOW_FILTER_TEXELS`]: a contact is not a sub-texel-sharp edge,
/// it is a *quantised* one, and letting the radius fall under the filter width
/// the rest of this file was tuned around trades the ninth decision's dither for
/// the staircase it removed. Above, at [`SHADOW_SEARCH_TEXELS`]: a wider filter
/// than the search that sized it spreads taps over texels nothing measured.
///
/// The fallback when no tap found a blocker is that same lower clamp rather than
/// "lit". The search is sixteen points over eight texels and the filter is
/// thirty-two over two, so the filter is the denser look at the neighbourhood
/// the receiver actually touches: a thin caster can fall between search taps,
/// and answering `1.0` there would leak light through it. The search's job is to
/// size the filter, never to replace it.
float sun_penumbra_texels(uint cascade, float2 tile_uv, float reference, float2 rotation)
{
    float2 texel = frame.shadow_params.xy;
    float4 rect = atlas_rect(cascade);
    float2 texel_step = atlas_step(rect);
    float2 tile_min = float2(0.5, 0.5) * texel_step;
    // The atlas's extent in texels, which is what turns a normalised atlas
    // coordinate into the integer `Load` wants.
    float2 extent = float2(1.0, 1.0) / texel;

    float sum = 0.0;
    float found = 0.0;
    for (uint index = 0u; index < SHADOW_SEARCH_TAPS; ++index)
    {
        float2 spoke = SHADOW_SEARCH_DISC[index] * SHADOW_SEARCH_TEXELS;
        float2 turned = float2(spoke.x * rotation.x - spoke.y * rotation.y,
                               spoke.x * rotation.y + spoke.y * rotation.x);
        float2 tap = clamp(tile_uv + turned * texel_step, tile_min, 1.0 - tile_min);
        // `min` against the last texel rather than trust to the clamp above: that
        // one is in tile space and half a texel inside, so the product below
        // lands inside the atlas by construction — but it is a float product,
        // and an out-of-range `Load` is undefined rather than clamped.
        int2 coord = int2(min(atlas_uv(rect, tap) * extent, extent - float2(1.0, 1.0)));
        float depth = shadow_atlas.Load(int3(coord, 0));
        if (depth > reference)
        {
            sum += depth;
            found += 1.0;
        }
    }

    if (found <= 0.0)
    {
        return SHADOW_FILTER_TEXELS;
    }

    // Shadow-clip depth back to metres. The cascade's box is `2 * radius` deep
    // through the sphere plus `SHADOW_CASTER_REACH` in front of it, and an
    // orthographic projection spreads that range linearly over `0..1`, so one
    // unit of `ndc.z` is the whole of it. `cascade_matrix` on the host is where
    // those two ends are set.
    float radius = frame.cascade_far[cascade];
    float separation = (sum / found - reference) * (2.0 * radius + SHADOW_CASTER_REACH);
    float texel_world = 2.0 * radius / tile_texels(rect);
    return clamp(separation * SHADOW_SUN_TAN_RADIUS / texel_world,
                 SHADOW_FILTER_TEXELS,
                 SHADOW_SEARCH_TEXELS);
}

/// What fraction of a cascade's reach the fade into the next one takes up.
///
/// # Why there is a band at all
///
/// Cascade selection is a step function of distance to the eye, and everything
/// a cascade decides changes across that step: the map's resolution — a texel of
/// the near cascade is a sixth of the outer one's here — and therefore both
/// biases, which are denominated in texels and so are six times larger on the
/// far side. Two cascades asked about the same fragment do not answer the same,
/// and where the step falls that difference is a visible ring: a shadow edge
/// jumps sideways, and a surface biased out of its own shadow on one side is
/// back in it on the other.
///
/// The fifth decision's texel denomination made that ring *sharper*, not
/// blunter, and knowingly: it is the price of biasing a near cascade
/// proportionally less than a far one, which is the whole point of denominating
/// in texels. So the band is what pays it back — over the last tenth of a
/// cascade's reach both cascades are sampled and the two answers are mixed by
/// distance, and the ring becomes a gradient no wider than the band.
///
/// # What it costs, and where
///
/// A second `cascade_visibility`, which is a second `tile_pcf` — nine more
/// comparison taps — for the fragments inside the band and nothing at all for
/// the ones outside it. A tenth of a cascade's reach is a shell, not a volume,
/// so the fraction of the frame that pays is smaller than a tenth; and the last
/// cascade has nothing to fade into, so it never pays.
///
/// # The number
///
/// A tenth. Measured on lantern's fixed camera at 1280×960, walking the floor
/// circle where cascade 0 meets cascade 1 — an eye-distance of 4.088 m — and
/// reading the luma either side of it six centimetres apart: the step averaged
/// **11.28** over the ring with no band, against **2.82** and **3.70** on
/// control circles at 3.6 m and 4.5 m where no boundary falls, so the seam was
/// four times the scene's own contrast at that scale. The sweep is in
/// `docs/plan/45-shadows.md`'s eighth decision.
static const float CASCADE_FADE_FRACTION = 0.1;

/// One cascade's answer about one fragment: `1.0` outside its box, and the PCF
/// filter over its map inside it.
///
/// Split out of `sun_visibility` so the cross-fade there can ask two cascades
/// the same question and mix the answers. Everything that depends on *which*
/// cascade is here, and everything that depends on the *fragment* is in the
/// caller — which is what makes calling it twice with a different first
/// argument the whole of the fade.
float cascade_visibility(uint cascade, float3 world_position, float3 to_light,
                         float3 geometric_normal, float2 pixel)
{
    // The bias, and it is `punctual_visibility`'s exactly: two moves of the
    // world position before it is projected, each a multiple of the world
    // footprint of one texel of *this cascade's* map. A cascade is an
    // orthographic box of `2 * radius` fitted to `tile_texels` of its own
    // rectangle, and its radius is the split distance the caller's selection
    // read — the same arithmetic `crcbl_render::shadow::cascade_matrix` snaps
    // its origin to. The side comes out of the rectangle rather than out of a
    // constant because a map is no longer always a whole root cell; see
    // `tile_texels`.
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
    // **The two moves are in different directions and cover different things.**
    // Along the normal is `shadow_normal_offset`'s, and it is what the slope
    // costs: it walks the lookup sideways across the map instead of raising the
    // depth it compares, so a grazing surface is read where it belongs and a
    // contact shadow keeps its contact. Along the light is the constant, and it
    // is the facet *seam* — two triangles of different slope biased by different
    // amounts, where the texel their shared edge falls in stores the steeper
    // one's depth and no offset read off either facet predicts the other's.
    //
    // What the two counts are and why they are what they are lives on the host,
    // in `crcbl_render::shadow`: `DEPTH_BIAS_TEXELS` and `NORMAL_OFFSET_TEXELS`
    // have the numbers and the frames they were read off.
    float4 rect = atlas_rect(cascade);
    if (atlas_rect_is_empty(rect))
    {
        // No map in this slot, so nothing here was ever rendered. **Before the
        // divide**, which is the whole point: see `atlas_rect_is_empty`.
        return 1.0;
    }
    float texel_world = 2.0 * frame.cascade_far[cascade] / tile_texels(rect);
    float3 biased = world_position
                  + geometric_normal
                        * (texel_world * frame.shadow_params.w
                           * shadow_normal_offset(geometric_normal, to_light))
                  + to_light * (texel_world * frame.shadow_params.z);

    float4 clip = mul(frame.shadow_view_proj[cascade], float4(biased, 1.0));
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

    // Both moves are already in `ndc`, and they land in different components.
    // The one along `to_light` moved the reference *up* — reversed-Z, so a
    // larger depth is nearer the light, which is the direction that stops a
    // surface shadowing itself — and it cannot move `tile_uv`, because the
    // projection's own depth axis is that direction. The one along the normal is
    // the opposite: it is perpendicular to that axis in the amount
    // `shadow_normal_offset` returns, so what it moves is `tile_uv` and the
    // texel this fragment reads. That is the difference between raising a
    // comparison and taking a different one.
    // The filter's width, which for the sun is not a constant: a contact is
    // sharp and a shadow thrown from height is soft, and `sun_penumbra_texels`
    // is what reads the difference out of the map. The rotation is taken once
    // and handed to both so the disc the search walks is the disc the filter
    // draws.
    float2 rotation = shadow_rotation(pixel);
    float radius = sun_penumbra_texels(cascade, tile_uv, ndc.z, rotation);
    return tile_pcf(cascade, tile_uv, ndc.z, pixel, radius);
}

/// The fraction of the sun this fragment can see: `1.0` fully lit, `0.0` fully
/// shadowed, and the values between are the PCF filter and the cascade fade.
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
                     float3 geometric_normal, float2 pixel)
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

    float visibility =
        cascade_visibility(cascade, world_position, to_light, geometric_normal, pixel);
    if (cascade + 1 >= SHADOW_CASCADES)
    {
        // The last cascade has nothing to fade into. Everything past its reach
        // selects it too — the loop's fallback — and gets its answer unmixed,
        // which the box test inside turns into "lit" soon enough.
        return visibility;
    }

    // The band, at the outer edge of this cascade's reach. `reach` is strictly
    // positive here: the loop only leaves a cascade selected whose far distance
    // *exceeds* this fragment's, and the one case where it does not — nothing
    // matched — returned above.
    float reach = frame.cascade_far[cascade];
    float band = reach * CASCADE_FADE_FRACTION;
    float blend = saturate((eye_distance - (reach - band)) / band);
    if (blend <= 0.0)
    {
        return visibility;
    }

    // The next cascade is guaranteed to contain this fragment, which is what
    // makes the mix well defined rather than a blend with the `1.0` an outside
    // fragment gets: the cascades are spheres about the same eye with strictly
    // growing radii — `crcbl_render::shadow::cascade_matrix` fits every one of
    // them to `eye` — so anything inside cascade `i` is inside cascade `i + 1`
    // by construction, not by a margin someone has to maintain.
    float next =
        cascade_visibility(cascade + 1, world_position, to_light, geometric_normal, pixel);
    return lerp(visibility, next, blend);
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
/// half-angle `θ` at axial depth `d` is `2 d tan θ` over the map's own
/// `tile_texels`, and for a cascade of radius `r` is `2 r` over the same. A
/// surface at slope `s` to the light climbs `s` times that across one texel,
/// which is the term below.
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

/// The **normal-offset** part, per unit of `sin(acos(Ng·L))` — how far along its
/// own normal a receiver is moved, in the same texels.
///
/// It replaces a slope-scaled *depth* bias of the same count, for
/// `shadow_normal_offset`'s reasons. **The count was carried over rather than
/// re-swept**, and what says it is still enough is the observable
/// `PUNCTUAL_DEPTH_BIAS_TEXELS` describes: `Scene::SpotShadow`'s and
/// `Scene::PointShadow`'s two geometry paths still agree pixel for pixel at
/// four, where a bias too small makes them disagree by however much acne there
/// is. Both goldens moved and were re-blessed after the frames were compared;
/// the shadows stayed attached to their casters, which is the other end.
///
/// `sin` is smaller than the `tan` it replaced at every angle, so this is not a
/// number that grew when its meaning changed. Whether a smaller one would now
/// do is unmeasured.
static const float PUNCTUAL_NORMAL_OFFSET_TEXELS = 4.0;

/// The fraction of a punctual light's map this fragment can see: `1.0` fully
/// lit, `0.0` fully shadowed, the values between the PCF filter.
///
/// The half a spot and a point light's face have in common, which is all of it
/// but two numbers: `tile` is the light tile to sample and `map_world` the world
/// width the whole map covers at this receiver's depth — `2 d tan θ` for a
/// frustum of half-angle `θ`. One texel of it is that over `tile_texels` of the
/// map's own rectangle, which is what `PUNCTUAL_DEPTH_BIAS_TEXELS` multiplies,
/// and taking the divide **here** rather than at the two call sites is what
/// keeps a demoted map biased by its own side: the caller knows the frustum and
/// only this function knows which rectangle the map landed in.
/// `to_light` is the unit vector towards the light and `n_dot_l` the unclamped
/// Lambert term, both already computed by the shading loop.
///
/// **`geometric_normal` drives the offset here for the same reason it does in
/// `sun_visibility`, and the consistency is deliberate.** Acne is a property of
/// the triangle that was rasterised into the map, and a punctual map rasterises
/// the same triangles the sun's does — so a light type reading a different normal
/// for the same surface would need a bias pair of its own for no reason anyone
/// could state. It moves no fixture the two constants above were set from:
/// `Scene::SpotShadow` and `Scene::PointShadow` receive on a flat floor, where
/// the facet and the shading normal are the same vector.
float punctual_visibility(uint tile, float3 world_position, float3 to_light, float n_dot_l,
                          float map_world, float3 geometric_normal, float2 pixel)
{
    uint atlas = light_tile(tile);
    float4 rect = atlas_rect(atlas);
    if (atlas_rect_is_empty(rect))
    {
        // `cascade_visibility`'s guard, for its reason: a row naming a slot the
        // host laid no map into divides by zero here otherwise.
        return 1.0;
    }
    float texel_world = map_world / tile_texels(rect);
    // Two moves, in two directions, on `sun_visibility`'s terms exactly: along
    // the normal by what the receiver's slope costs in lateral travel, and along
    // the light by the constant that covers a facet seam.
    float3 biased = world_position
                  + geometric_normal
                        * (texel_world * PUNCTUAL_NORMAL_OFFSET_TEXELS
                           * shadow_normal_offset(geometric_normal, to_light))
                  + to_light * (texel_world * PUNCTUAL_DEPTH_BIAS_TEXELS);

    float4 clip = mul(frame.light_view_proj[tile], float4(biased, 1.0));
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
    // The constant width, not the sun's measured one: a punctual map is a
    // perspective projection, so a difference of two of its depths is not a
    // distance until it is un-projected, and its light has no angular radius to
    // scale one by. Contact hardening for these is its own rung.
    return tile_pcf(atlas, tile_uv, ndc.z, pixel, SHADOW_FILTER_TEXELS);
}

/// The fraction of a shadowed spot this fragment can see.
///
/// `tile` is the light's `shadow_tile` and the caller has already checked it
/// names a real light tile. The rest is `punctual_visibility`'s.
float spot_visibility(GpuLight light, uint tile, float3 world_position, float3 to_light,
                      float n_dot_l, float3 geometric_normal, float2 pixel)
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
    return punctual_visibility(tile, world_position, to_light, n_dot_l, 2.0 * tan_outer * axial,
                               geometric_normal, pixel);
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
                       float n_dot_l, float3 geometric_normal, float2 pixel)
{
    if (n_dot_l <= 0.0)
    {
        return 1.0;
    }

    float3 from_light = world_position - light.position.xyz;
    // A face is a 90° frustum, so its half-angle's tangent is 1 and the axial
    // depth is the largest component of the offset — which is the same
    // `2 d tan θ` a spot computes, with both terms known rather than read off
    // the row.
    float axial = max(max(abs(from_light.x), abs(from_light.y)), abs(from_light.z));
    return punctual_visibility(base + point_face(from_light), world_position, to_light, n_dot_l,
                               2.0 * axial, geometric_normal, pixel);
}

/// Karis' quartic window on a light's reach: `1` at the light, **exactly zero**
/// at `radius`, and smooth between.
///
/// Split out of `punctual_falloff` because an area light wants the window and
/// not the inverse square beside it: a rectangle's own polygon integral already
/// shrinks with distance, because that is what a solid angle does, so
/// multiplying by `1 / d²` a second time would darken it by the square of its
/// distance twice over. What both kinds share is the hard cut at `radius`, and
/// `light_cluster.slang` culls both against that same number — so a light is
/// either in a froxel's list or contributing nothing to it, with no radius at
/// which the two disagree.
float range_window(float distance, float radius)
{
    float ratio = distance / max(radius, 1e-6);
    float window = saturate(1.0 - ratio * ratio * ratio * ratio);
    return window * window;
}

/// How bright a punctual light is at `distance`, reaching **exactly zero** at
/// `radius`.
///
/// Inverse-square under `range_window`, which is where the cut at `radius` and
/// its reason live.
///
/// The `1` in the denominator keeps the singularity at zero distance finite;
/// it is the standard form, and it costs a light its physical units in exchange
/// for never producing an infinity a tonemap has to survive.
float punctual_falloff(float distance, float radius)
{
    return range_window(distance, radius) / (distance * distance + 1.0);
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

/// Texels along one side of a visibility map's interior — the part that holds
/// directions. `crcbl_shaders::probe_visibility::SIDE`, and
/// `the_visibility_map_constants_match_the_ones_the_shaders_declare` is what
/// holds this file, `ssr.slang` and that module to one another.
static const float PROBE_VISIBILITY_SIDE = 16.0;

/// Texels of wrapped border on each side of that interior, which is what lets
/// the blend below read four ordinary neighbours everywhere.
/// `crcbl_shaders::probe_visibility::BORDER`.
static const float PROBE_VISIBILITY_BORDER = 1.0;

/// How far along its own normal a shaded point is moved before its distance to a
/// probe is measured, in world units.
/// `crcbl_shaders::probe_visibility::SURFACE_BIAS`, where the reason is argued:
/// without it a floor is exactly as far from the probe above it as the floor
/// that probe recorded, and shadows itself.
static const float PROBE_SURFACE_BIAS = 0.05;

/// The share of its trilinear weight a fully occluded corner keeps.
/// `crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT`, where the reason is
/// argued: it is what stops the weighted sum's divisor reaching zero, and what
/// makes a fragment whose every corner is occluded fall back to the plain
/// trilinear result rather than to black.
static const float PROBE_OCCLUDED_WEIGHT = 0.0001;

/// How much of a clipmap level's half-extent the blend into the next level out
/// occupies. `crcbl_shaders::probe::LEVEL_BAND`, where the reason is argued: a
/// point on level `k`'s boundary stands at exactly half of level `k+1`'s
/// half-extent, so a band wider than half an extent would still be fading level
/// `k` where the fragment beside it has already gone wholly to level `k+1` —
/// which is the step this blend exists to remove.
static const float PROBE_LEVEL_BAND = 0.25;

/// `+1` for a positive or zero value and `-1` for a negative one — the
/// octahedral mapping's `signNotZero`.
///
/// Not `sign`, which answers zero for zero: the fold below multiplies by this,
/// and a zero factor would collapse a direction lying in an axis plane onto the
/// square's centre.
float sign_not_zero(float value)
{
    return value >= 0.0 ? 1.0 : -1.0;
}

/// A unit direction as a point of the octahedral square, each component in
/// `-1..1` — `crcbl_shaders::probe_visibility::oct_encode`, and the same
/// paper's: Cigolle et al. 2014, *A Survey of Efficient Representations for
/// Independent Unit Vectors*.
///
/// **The fold is on `+Y`/`-Y`**, because this engine's world is `+Y` up: the
/// seam then runs around and below the horizon rather than through the ceiling
/// and the floor, which are the two directions a probe in a room has the most to
/// say about.
///
/// The `max` in the divisor is not defensive about a caller — the direction from
/// a probe to a biased shading point is never zero — it is what keeps the
/// division finite for the degenerate volume, where every probe stands at the
/// grid's origin and a fragment may sit exactly on one.
float2 oct_encode(float3 direction)
{
    float norm = abs(direction.x) + abs(direction.y) + abs(direction.z);
    float2 p = direction.xz / max(norm, 1e-20);
    if (direction.y < 0.0)
    {
        p = float2((1.0 - abs(p.y)) * sign_not_zero(p.x),
                   (1.0 - abs(p.x)) * sign_not_zero(p.y));
    }
    return p;
}

/// The two depth moments probe `index`'s map holds along `direction`, blended
/// bilinearly — `crcbl_shaders::probe_visibility::ProbeVisibility::moments`.
///
/// This body is character for character the one `ssr.slang` carries, along with
/// `probe_chebyshev`, `probe_weight`, `oct_encode` and `sign_not_zero`: that
/// pass weighs the reflection's probe fallback by the same bound, and
/// `crcbl_shaders::probe_visibility`'s
/// `the_shaders_weigh_a_probe_the_way_this_module_does` is what holds the two
/// copies together.
///
/// `table_tap`'s addressing and `table_tap`'s reason: a texel holds the value at
/// its own centre, so a direction's place in the interior is
/// `uv * SIDE + BORDER - 0.5`, and the two neighbours either side of it are
/// inside the bordered layer whatever the direction. The clamp is against
/// `GetDimensions` rather than against those constants, on `occlusion_at`'s
/// terms: it makes the fetch a fact about the image, which is what lets the
/// one-texel off-switch be read by this same code.
float2 probe_moments(uint index, float3 direction)
{
    uint width;
    uint height;
    uint layers;
    probe_visibility.GetDimensions(width, height, layers);
    float2 extent = float2(float(width), float(height));
    float2 scaled = (oct_encode(direction) * 0.5 + 0.5) * PROBE_VISIBILITY_SIDE
                  + PROBE_VISIBILITY_BORDER - 0.5;
    float2 low = clamp(floor(scaled), float2(0.0, 0.0), extent - 1.0);
    float2 high = min(low + 1.0, extent - 1.0);
    float2 weight = clamp(scaled - low, 0.0, 1.0);
    int layer = int(min(index, max(layers, 1u) - 1u));

    float2 c00 = probe_visibility.Load(int4(int(low.x), int(low.y), layer, 0)).xy;
    float2 c10 = probe_visibility.Load(int4(int(high.x), int(low.y), layer, 0)).xy;
    float2 c01 = probe_visibility.Load(int4(int(low.x), int(high.y), layer, 0)).xy;
    float2 c11 = probe_visibility.Load(int4(int(high.x), int(high.y), layer, 0)).xy;
    return lerp(lerp(c00, c10, weight.x), lerp(c01, c11, weight.x), weight.y);
}

/// The Chebyshev bound on the probe at `probe_position`'s visibility of a
/// surface at `world_position` facing `normal` — `probe_weight` without its
/// floor, and `crcbl_shaders::probe_visibility::chebyshev` applied to the
/// moments this probe's map holds along that direction.
///
/// **The Chebyshev bound**, cubed: for a depth distribution of mean `μ` and
/// variance `σ²`, the share of it at least `d - μ` beyond the mean is at most
/// `σ² / (σ² + (d - μ)²)`, which reads as a visibility that is one in front of
/// the mean surface and falls off behind it at a width the texel's own
/// disagreement sets. Donnelly & Lauritzen 2006 is where the bound entered
/// graphics and Majercik et al. 2019 §3 is where it is applied to a probe, cube
/// and all.
///
/// The query point is moved off the surface by `PROBE_SURFACE_BIAS` first; that
/// constant is where the reason is.
///
/// **The floor lives in `probe_weight`, not here, and the split is what makes
/// the bound callable without it.** `PROBE_OCCLUDED_WEIGHT` is a property of the
/// weighted mean's divisor — it is what keeps the sum of eight corner weights
/// off zero — so a gather that has no divisor must not pay it:
/// `docs/plan/50-irradiance-probes.md`'s reflective-shadow-map updater sums
/// thousands of texels into one probe, where a floor under every wall-hidden
/// texel is not a rounding error but that many ten-thousandths of flux leaking
/// through the wall. Such a pass calls this; the trilinear gather calls
/// `probe_weight`.
float probe_chebyshev(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    float3 to_probe = probe_position - (world_position + normal * PROBE_SURFACE_BIAS);
    float to_surface = length(to_probe);
    // The map is indexed by the direction the probe looks *out* along.
    float2 moments = probe_moments(index, -to_probe);
    // Clamped because the two moments are blended independently and a texel
    // between two disagreeing neighbours can land a hair under `mean²`, which
    // is a negative variance and a visibility above one.
    float variance = max(moments.y - moments.x * moments.x, 0.0);
    float behind = to_surface - moments.x;
    float bound = variance / (variance + behind * behind);
    return to_surface <= moments.x ? 1.0 : bound * bound * bound;
}

/// The share of its trilinear weight the probe at `probe_position` keeps for a
/// surface at `world_position` facing `normal` —
/// `crcbl_shaders::probe_visibility::ProbeVisibility::weight`.
///
/// `probe_chebyshev` floored at `PROBE_OCCLUDED_WEIGHT`, which is what keeps the
/// gather's divisor off zero; `probe_irradiance` is where what that buys is
/// argued, and `probe_chebyshev` is where the bound itself is.
float probe_weight(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    return max(probe_chebyshev(index, probe_position, world_position, normal),
               PROBE_OCCLUDED_WEIGHT);
}

/// Which row of the probe table a grid cell is, clamped into the buffer.
///
/// The clamp is against `probe_counts.w` — the product the host wrote — for the
/// reason `ambient_occlusion`'s `GetDimensions` clamp exists: it makes the fetch
/// a fact about the buffer rather than a promise about whoever filled the
/// header. `max(total, 1)` is what keeps the empty grid on row 0, which every
/// table has because a buffer of no bytes is not a buffer.
///
/// **It is also the layer of `probe_visibility`**, which is what makes the row
/// and the map one probe: `crcbl_render::probe_capture` writes its layers in the
/// same order the table's rows are in — `x`-fastest within a level, and the
/// levels one after another finest first, which is what `probe_levels.y` steps
/// over here.
uint probe_row(uint level, uint3 cell)
{
    uint index = frame.probe_levels.y * level
               + (cell.z * frame.probe_counts.y + cell.y) * frame.probe_counts.x + cell.x;
    return min(index, max(frame.probe_counts.w, 1u) - 1u);
}

/// One corner of the trilinear gather: its coefficients already scaled by the
/// weight it keeps, and that weight beside them.
///
/// Both halves ride through the same seven blends below, so what comes out is
/// `Σ trilinear·visibility·coefficients` over the eight corners and
/// `Σ trilinear·visibility` — a weighted mean waiting for its divisor.
/// `crcbl_shaders::probe::WeightedProbe` is the host mirror.
struct WeightedProbe
{
    /// The row, every coefficient already multiplied by `weight`.
    GpuProbe sh;
    /// What this corner counts for: its visibility, never below
    /// `PROBE_OCCLUDED_WEIGHT`.
    float weight;
};

/// Two corners blended coefficient by coefficient and weight by weight — one
/// axis of the trilinear interpolation below.
WeightedProbe lerp_probe(WeightedProbe a, WeightedProbe b, float t)
{
    WeightedProbe blended;
    blended.sh.sh_r = lerp(a.sh.sh_r, b.sh.sh_r, t);
    blended.sh.sh_g = lerp(a.sh.sh_g, b.sh.sh_g, t);
    blended.sh.sh_b = lerp(a.sh.sh_b, b.sh.sh_b, t);
    blended.weight = lerp(a.weight, b.weight, t);
    return blended;
}

/// The corner at cell `cell` of clipmap level `level`: its row, scaled by what
/// that probe's visibility map says a surface at `world_position` facing
/// `normal` keeps of it.
///
/// `origin` and `spacing` are the level's own, which the caller reads and
/// inverts once rather than eight times here.
WeightedProbe probe_corner(uint level, uint3 cell, float3 origin, float3 spacing,
                           float3 world_position, float3 normal)
{
    uint row = probe_row(level, cell);
    GpuProbe stored = probes[row];
    float3 at = origin + float3(cell) * spacing;
    float weight = probe_weight(row, at, world_position, normal);

    WeightedProbe corner;
    corner.sh.sh_r = stored.sh_r * weight;
    corner.sh.sh_g = stored.sh_g * weight;
    corner.sh.sh_b = stored.sh_b * weight;
    corner.weight = weight;
    return corner;
}

/// The irradiance one clipmap level of `docs/plan/18-render-features.md`'s probe
/// grid gives a surface at `world_position` facing `normal`, in linear RGB.
///
/// Mirrored on the host by `crcbl_shaders::probe::level_irradiance_at`, which is
/// where the spherical harmonics are checked against the literature they come
/// from.
///
/// # A grid of nothing evaluates to zero, and there is no branch
///
/// A scene with no probes carries counts of zero and a reciprocal spacing of
/// zero. Then `last` is zero on every axis, the grid coordinate is zero, both
/// corners of every axis are probe 0, and that row is the zeroed one every table
/// starts as — so every weighted coefficient is exactly zero, the divisor is
/// positive, `0 / w` is zero, and `frame.ambient.rgb + 0.0` is
/// `frame.ambient.rgb` **exactly**, on every target. That is what let the grid
/// land with no golden re-blessed, and what lets the visibility test below join
/// it on the same terms.
///
/// # Interpolated coefficients, not interpolated evaluations
///
/// The eight corners are blended as *coefficients* and evaluated once, so the
/// whole lookup is seven blends, three dot products, one divide and one clamp
/// whatever the grid's size — where evaluating each corner first would be
/// twenty-four dot products. It is also the more correct of the two: irradiance
/// is linear in the coefficients until the clamp, and clamping once at the end
/// is clamping the interpolated field rather than eight samples of it.
///
/// The clamp itself is not defensive. L1 is a truncated series and a truncated
/// series rings: an environment concentrated in one direction evaluates negative
/// on the far side, and a surface there would *subtract* light.
///
/// # Each corner is weighed by whether it can see the surface
///
/// `docs/plan/50-irradiance-probes.md`'s rung, and the one thing that makes a
/// probe grid stop leaking: a corner's weight is its trilinear share times
/// `probe_weight`, which is the Chebyshev bound of that probe's visibility map
/// in the direction of this fragment. A probe on the far side of a wall is
/// further away than the wall it recorded, so it keeps nothing and its light
/// does not come through. The coefficients ride through the same seven blends
/// already scaled by that weight and the weights ride beside them, so the sum is
/// a weighted mean and the divide at the end is what makes it one — the
/// evaluation order above is untouched.
///
/// **Every corner occluded is the plain trilinear result, not black.** The
/// weight never falls below `PROBE_OCCLUDED_WEIGHT`, so the divisor cannot reach
/// zero; and when *every* corner is at that floor it divides straight back out,
/// leaving exactly the unweighted blend. A fragment inside geometry, which is
/// the case that reaches it, keeps the light it had before this rung rather than
/// becoming a hole in the room.
///
/// # Outside the level clamps to its surface
///
/// The same choice the shadow sampler makes at a cascade's edge: an environment
/// does not stop at the last probe, and fading to zero there would put a seam in
/// mid-air. `probe_irradiance` is what makes that clamp invisible where a finer
/// level ends and a coarser one goes on.
float3 probe_level_irradiance(uint level, float3 world_position, float3 normal)
{
    // The last addressable probe on each axis. `max(…, 0)` is what keeps an axis
    // with no probes — or with exactly one — addressing probe 0 rather than
    // probe -1.
    float3 last = max(float3(frame.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    // Where this level stands and how far apart its probes are, read rather than
    // derived: `crcbl_shaders::probe::ProbeVolume` wrote these rows, so there is
    // one place the clipmap's geometry is decided.
    float3 origin = frame.probe_level_origin[level].xyz;
    float3 inv = frame.probe_level_inv_spacing[level].xyz;
    float3 grid = clamp((world_position - origin) * inv, float3(0.0, 0.0, 0.0), last);
    float3 base = floor(grid);
    float3 f = grid - base;

    uint3 lo = uint3(base);
    // `min` against the axis's last probe rather than `+ 1` outright: at the far
    // face there is no next probe, and `f` is zero there, so the corner is
    // weighed out rather than wrapped around.
    uint3 hi = uint3(min(base + 1.0, last));

    // The spacing the header carries the reciprocal of, so a corner's world
    // position can be named. An axis with no reciprocal has no step to take and
    // every probe on it stands at the origin's coordinate — which is the
    // degenerate volume, and `ProbeVolume::position` on the host agrees.
    float3 spacing = float3(inv.x != 0.0 ? 1.0 / inv.x : 0.0,
                            inv.y != 0.0 ? 1.0 / inv.y : 0.0,
                            inv.z != 0.0 ? 1.0 / inv.z : 0.0);

    // Seven blends over the coefficients and their weights. The far corner's
    // weight reaches exactly zero at the boundary where the cell index changes —
    // `lerp(a, b, 0.0)` is `a` — so a fragment landing in cell `i` at `f≈1` and
    // one landing in cell `i+1` at `f≈0` compute the same value, which is the
    // whole of this grid's determinism argument.
    WeightedProbe x00 = lerp_probe(probe_corner(level, uint3(lo.x, lo.y, lo.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, lo.y, lo.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x10 = lerp_probe(probe_corner(level, uint3(lo.x, hi.y, lo.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, hi.y, lo.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x01 = lerp_probe(probe_corner(level, uint3(lo.x, lo.y, hi.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, lo.y, hi.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe x11 = lerp_probe(probe_corner(level, uint3(lo.x, hi.y, hi.z), origin, spacing, world_position, normal),
                                   probe_corner(level, uint3(hi.x, hi.y, hi.z), origin, spacing, world_position, normal), f.x);
    WeightedProbe y0 = lerp_probe(x00, x10, f.y);
    WeightedProbe y1 = lerp_probe(x01, x11, f.y);
    WeightedProbe cell = lerp_probe(y0, y1, f.z);

    float4 basis = float4(normal, 1.0);
    return max(float3(dot(cell.sh.sh_r, basis), dot(cell.sh.sh_g, basis), dot(cell.sh.sh_b, basis))
                   / cell.weight,
               float3(0.0, 0.0, 0.0));
}

/// How far out of the **finest** clipmap level `world_position` stands, in that
/// level's own half-extents: zero at its centre and one on its boundary.
///
/// `crcbl_shaders::probe::ProbeVolume::level_reach` is the host mirror, and this
/// body is character for character the one `ssr.slang` carries — that pass
/// evaluates the same clipmap, and `crcbl_shaders::probe`'s
/// `the_shaders_pick_a_level_the_way_this_module_does` is what holds the two
/// copies together.
///
/// **Only the finest level is measured**, because every level is concentric with
/// it and twice as wide: level `k`'s reach is this one halved `k` times, which
/// is what lets `probe_level_of` walk the levels without touching their rows.
///
/// **An axis with no extent is contained everywhere.** A grid one probe deep, or
/// one whose reciprocal spacing is zero, has no boundary on that axis to be
/// outside of — it clamps onto probe 0 for every point — so it contributes
/// nothing here rather than dividing by its zero extent.
float probe_level_reach(float3 world_position, float3 origin, float3 inv_spacing, float3 last)
{
    float reach = 0.0;
    for (uint axis = 0; axis < 3; ++axis)
    {
        if (last[axis] == 0.0 || inv_spacing[axis] == 0.0)
            continue;
        float grid = (world_position[axis] - origin[axis]) * inv_spacing[axis];
        reach = max(reach, abs(2.0 * grid / last[axis] - 1.0));
    }
    return reach;
}

/// Which clipmap level a point of `probe_level_reach` `reach` reads, and what
/// share of the read belongs to that level rather than to the next one out: `x`
/// the level, `y` the share.
///
/// `crcbl_shaders::probe::ProbeVolume::level_of` is the host mirror, and this
/// body is `ssr.slang`'s character for character.
///
/// The finest level whose reach is inside one is the level, and the share ramps
/// from zero on that level's boundary to one `PROBE_LEVEL_BAND` of an extent
/// inside it. The coarsest level has nothing to blend towards and takes the
/// whole share, which is the clamp-at-the-edge the single grid always did.
///
/// **The two sides of a level switch meet.** On level `k`'s boundary the share
/// is exactly zero, so the read there is wholly level `k+1`; a point a hair
/// further out picks level `k+1`, whose own reach is half of one — past the
/// band, so its share is exactly one and the read is wholly level `k+1` again.
/// That is the same shape of argument the trilinear cell index already rests on.
float2 probe_level_of(float reach, uint levels)
{
    for (uint level = 0; level + 1 < levels; ++level)
    {
        float at = reach * exp2(-float(level));
        if (at < 1.0)
            return float2(float(level), saturate((1.0 - at) / PROBE_LEVEL_BAND));
    }
    return float2(float(levels - 1), 1.0);
}

/// The irradiance the whole clipmap gives a surface at `world_position` facing
/// `normal`, in linear RGB.
///
/// Mirrored on the host by `crcbl_shaders::probe::irradiance_at`. The finest
/// level that contains the point, faded into the next one out over a band at its
/// edge — `probe_level_reach` and `probe_level_of` are the rule and
/// `probe_level_irradiance` is the read within a level.
///
/// # A one-level volume is exactly one gather
///
/// The coarsest level takes the whole share, so a volume of one level evaluates
/// `probe_level_irradiance` of level 0 and nothing else — the arithmetic this
/// function was before the clipmap, unchanged and in the same order. That is
/// what let the levels land with no golden re-blessed, and it is also why a
/// scene that wants one grid pays nothing for the ones it did not ask for.
///
/// # Two terms rather than a `lerp`
///
/// `coarse * (1 - share) + fine * share` returns `fine` **exactly** at a share
/// of one and `coarse` exactly at zero, where `lerp` would return
/// `coarse + (fine - coarse)` and lose the low bits of either end. That
/// exactness is what makes the switch at a level's boundary meet, and it is the
/// same spelling `ssr.slang` composites its screen hit with, for the same
/// reason.
float3 probe_irradiance(float3 world_position, float3 normal)
{
    float3 last = max(float3(frame.probe_counts.xyz) - 1.0, float3(0.0, 0.0, 0.0));
    uint levels = clamp(frame.probe_levels.x, 1u, PROBE_LEVELS);
    float reach = probe_level_reach(world_position,
                                    frame.probe_level_origin[0].xyz,
                                    frame.probe_level_inv_spacing[0].xyz,
                                    last);
    float2 pick = probe_level_of(reach, levels);
    uint level = uint(pick.x);
    float share = pick.y;

    float3 fine = probe_level_irradiance(level, world_position, normal);
    if (share >= 1.0)
        return fine;
    float3 coarse = probe_level_irradiance(level + 1, world_position, normal);
    return coarse * (1.0 - share) + fine * share;
}

/// The irradiance the gradient sky gives a surface facing `normal`.
///
/// `probe_irradiance`'s last three lines with the frame's rows in place of a
/// cell's, because the sky is the same kind of thing: a distant environment
/// projected onto L1. `crcbl_shaders::sky::SkyGradient::irradiance` is the host
/// side that fills the rows, in closed form.
///
/// The `max` is `probe_irradiance`'s and is not defensive: L1 is a truncation
/// and a truncated series rings, so an environment concentrated in one
/// direction evaluates negative on the far side — a surface that would subtract
/// light. It costs nothing here, since a sky with no rows evaluates to exactly
/// zero and `max(0, 0)` is zero.
float3 sky_irradiance(float3 normal)
{
    float4 basis = float4(normal, 1.0);
    return max(float3(dot(frame.sky_sh_r, basis),
                      dot(frame.sky_sh_g, basis),
                      dot(frame.sky_sh_b, basis)),
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
    /// `a` is the lobe's roughness, quantised to `REFLECTIVITY_LEVELS` here so
    /// the store has no rounding tie to break.
    ///
    /// Roughness rather than the sharpness ramp it used to hold. `ssr.slang`
    /// derives the ramp from it, which keeps the cutoff one constant in one
    /// file; and the roughness itself is what the split-sum's prefiltered
    /// environment is indexed by, where the ramp is zero for every surface
    /// rougher than the cutoff — exactly the surfaces that lookup is for.
    float4 reflectivity : SV_Target1;
    /// The motion vector, into a two-channel half-float target —
    /// `crcbl_render::forward::MOTION_FORMAT` names the format and
    /// `crcbl_render::TransientImageDesc::motion` describes the image.
    ///
    /// **Current minus previous, in texture coordinates, `+y` down**, which is
    /// `motion_vector`'s convention and stated there in full. Written on every
    /// path through this stage, the debug views included: a member left
    /// unwritten on one branch is an undefined texel in a target a later pass
    /// reprojects with.
    float2 motion : SV_Target2;
};

/// `TILING_PHYSICAL`'s sampling UV: the surface's world-space position along the
/// two axes of its dominant face plane, divided by `tile_metres`.
///
/// The texture then repeats once per `tile_metres` of surface, so one texture
/// cell measures one `tile_metres` however large the face is — a 2 m face under
/// a 1 m tile shows a 2×2 grid, a 1 m face shows one. That is the whole of what
/// physical tiling buys over the authored UV, which a mesh fixes to `0..1` and
/// so stretches one tile across a face of any size.
///
/// **The plane is chosen by the dominant normal axis**, the ordinary planar
/// projection: a face whose normal points mostly along X is sampled by its world
/// `z,y`, one along Y by `x,z`, one along Z by `x,y`. That is exact for the
/// axis-aligned faces a greybox is built from, and continuous enough elsewhere
/// that a grid still reads as a ruler. The projection is unsigned — which world
/// axis maps to `u` versus `v`, and its direction, do not matter to a symmetric
/// grid, and the cell *count* a surface shows is `extent / tile_metres` either
/// way, which is what the tiling is judged by.
///
/// `tile_metres` is clamped off zero so a physical row that never set it — the
/// zero a `default`-constructed row carries — collapses onto one texel rather
/// than dividing by zero.
float2 physical_tile_uv(float3 world_position, float3 normal, float tile_metres)
{
    float3 axis = abs(normal);
    float2 planar;
    if (axis.x >= axis.y && axis.x >= axis.z)
    {
        planar = world_position.zy;
    }
    else if (axis.y >= axis.z)
    {
        planar = world_position.xz;
    }
    else
    {
        planar = world_position.xy;
    }
    return planar / max(tile_metres, 1e-4);
}

/// Some unit vector perpendicular to `normal`, for a fragment whose real
/// tangent could not be recovered.
///
/// Duff et al., "Building an Orthonormal Basis, Revisited" (JCGT 6:1, 2017) —
/// `crcbl_shaders::vertex::orthonormal_basis`' first return value. The sign of
/// `normal.z` is what makes it stable: the division is by `sign + normal.z`,
/// which the naive form lets reach zero at the south pole and which this one
/// holds at a magnitude of at least one everywhere. The host writes the sign
/// with `f32::copysign`, which differs from the comparison here only at a
/// negative zero — where both denominators are one all the same, so neither
/// divides by zero and the frame either produces is equally arbitrary.
///
/// **Arbitrary about the normal, and that is the point.** It agrees with no UV
/// parameterisation, so a normal map read through it is rotated by an angle
/// nobody chose — which is exactly the right answer for the two cases that reach
/// it, a surface whose UV does not vary at all and a tangent that interpolated
/// to nothing. Both are frames that could not be measured, and a finite
/// arbitrary one beats a `NaN` that turns the pixel black.
float3 orthonormal_tangent(float3 normal)
{
    float sign_z = normal.z >= 0.0 ? 1.0 : -1.0;
    float a = -1.0 / (sign_z + normal.z);
    return float3(1.0 + sign_z * normal.x * normal.x * a,
                  sign_z * normal.x * normal.y * a,
                  -sign_z * normal.x);
}

/// The tangent frame a fragment gets when its mesh shipped no `TANGENT`:
/// Schüler's cotangent frame, from the screen-space derivatives of world
/// position and UV.
///
/// Christian Schüler, "Followup: Normal Mapping Without Precomputed Tangents"
/// (ShaderX5, 2006). The two position derivatives span the facet's tangent plane
/// and the two UV derivatives say how the parameterisation runs across it, so
/// inverting that 2×2 Jacobian gives the direction in which `u` increases —
/// which is what a tangent *is*:
///
/// ```text
/// T ∝ ∂v/∂y · ∂p/∂x − ∂v/∂x · ∂p/∂y
/// B ∝ ∂u/∂x · ∂p/∂y − ∂u/∂y · ∂p/∂x
/// ```
///
/// The division by the Jacobian's determinant is left out, because both vectors
/// are normalised below and a positive scale falls out of that — but its **sign**
/// is not a scale and is applied. That is the one line the textbook form leaves
/// out and the one this file cannot do without: a determinant is negative when
/// the parameterisation is mirrored *or* when the target's screen-space `y` runs
/// the other way, and the four targets here do not agree about the second.
/// `geometric_normal_of` below measured that on radv — the bare
/// `cross(ddx, ddy)` is anti-parallel to the authored normal there — and a frame
/// that inherited the sign would light a normal map from the wrong side on that
/// backend and the right one on another.
///
/// **The frame this returns is therefore always right-handed about `normal`,
/// and that is the limitation rather than a defect.** It is the whole of
/// `docs/plan/43-render-standards.md` §2's rung 1: a screen-space frame cannot
/// tell a mirrored UV shell from a flipped screen basis, so it cannot recover
/// the handedness glTF stores in a tangent's `w`, and the left half of a
/// mirrored character lights inside out through it. What it is for is the mesh
/// that has no vertex frame at all — every mesh this engine authors itself, and
/// every imported primitive with no `TANGENT` accessor.
///
/// `normal` is the surface normal the frame is built around; the returned
/// tangent and bitangent are unit length and perpendicular to it, and the
/// normal is passed through unchanged. The bitangent completes the frame with a
/// cross product rather than being taken from the measurement: with the sign
/// applied the two agree in direction, and a skewed parameterisation — `u` and
/// `v` not at right angles on the surface — loses its shear here, which is what
/// every implementation that renormalises does.
///
/// **A fragment stage only**, on `geometric_normal_of`'s terms: `ddx`/`ddy` are
/// a quad's finite differences and mean nothing in any other stage.
TangentFrame derivative_frame(float3 dpdx, float3 dpdy, float2 duvdx, float2 duvdy, float3 normal)
{
    float determinant = duvdx.x * duvdy.y - duvdy.x * duvdx.y;
    float winding = determinant < 0.0 ? -1.0 : 1.0;
    float3 tangent = (duvdy.y * dpdx - duvdx.y * dpdy) * winding;

    TangentFrame basis;
    basis.normal = normal;
    // Gram-Schmidt before the normalise, for the reason `skinning.slang` gives:
    // the measured tangent lies in the facet's plane and `normal` is the
    // interpolated shading normal, which on a curved surface is not that plane's
    // perpendicular.
    tangent = tangent - normal * dot(normal, tangent);
    float length_squared = dot(tangent, tangent);
    // A guarded normalise rather than a bare `rsqrt`, whose value is undefined
    // for a primitive with no UV variation at all — a surface every one of whose
    // corners carries the same coordinate, which is an ordinary thing for a
    // greybox face to be, and one whose determinant is exactly zero. The stand-in
    // is `orthonormal_basis`' own construction, which is arbitrary about the
    // normal and therefore no worse than a frame that could not be measured;
    // what matters is that no `NaN` reaches the shading, because a `NaN` normal
    // is a black pixel that survives every later multiply.
    basis.tangent = length_squared > 1e-16 ? tangent * rsqrt(length_squared)
                                           : orthonormal_tangent(normal);
    basis.bitangent = cross(normal, basis.tangent);
    return basis;
}

/// The shading normal: the surface normal this material's normal map perturbs,
/// or the surface normal itself where the material names no map.
///
/// `docs/plan/43-render-standards.md` §2's rung 2, and the whole of what the
/// normal page buys.
///
/// # Two uniform branches, and both of them are free
///
/// A material row is a `nointerpolation` index and `VertexOutput::frame` is a
/// `nointerpolation` word, so **both** tests below are constant across a quad —
/// a quad belongs to one primitive, helper lanes included, which is the same
/// fact `geometric_normal_of` rests on. So neither is a divergent branch on any
/// target: every lane of the quad takes the same arm, the derivative operations
/// above them are still evaluated in lockstep, and a select written branch-free
/// would spend the page fetch and the frame on every untextured surface in the
/// scene for a picture identical to the branch's.
///
/// The first branch is also what makes "no normal map" **exact**. Layer 0 of the
/// page is the neutral texel, but an eight-bit unorm has no exact `0.5`, so that
/// texel decodes to a tangent-space normal about a fifth of a degree off
/// vertical — enough to move every golden in the tree that draws a lit surface.
/// Returning `normal` untouched returns the same bits the shading used before
/// this function existed. See `normal_layer`.
///
/// # The decode is glTF's, term for term
///
/// `normalize((<sampled> * 2 - 1) * float3(scale, scale, 1))` — glTF 2.0
/// §3.9.3's own definition of `normalTexture.scale`, which multiplies the `x`
/// and `y` of the decoded normal and leaves `z` alone. The texel arrives linear
/// because the page is an `Rgba8Unorm` image and not an sRGB one; see
/// `normal_textures`, where the colour-space decision is argued.
///
/// # WGSL cannot see that the branches are uniform, and says so
///
/// Every fact above is a fact about the hardware, and WGSL's uniformity
/// analysis proves none of it: it reports the fragment input itself as possibly
/// non-uniform, and `layer` reaches this function through the material row that
/// input indexes, so both tests below are non-uniform as far as it can tell.
/// Two rules then bite, and both of them fail
/// `CreateShaderModule` in the browser while SPIR-V, MSL and DXIL compile the
/// same source without a word — `fxaa.slang`'s `tap` carries the same scar.
///
/// * A derivative is legal only in uniform control flow, so `ddx` and `ddy` are
///   taken at this function's top level, **above** the early return rather than
///   after it.
/// * An implicit-LOD sample is legal only in uniform control flow, so the page
///   is read with `SampleGrad` and the derivatives already in hand. That is the
///   same level `Sample` would have chosen — it is the same gradient — so the
///   filtering is unchanged and the branch is kept.
///
/// The two scalars rather than the material row are the same accommodation one
/// step earlier: Slang's WGSL backend passes a struct by pointer, and a pointer
/// into the material array is one more thing the analysis has to reason about.
/// This function reads two of the row's fields, so it takes two values.
float3 shading_normal_of(uint layer,
                         float normal_scale,
                         VertexOutput input,
                         float3 normal,
                         float2 uv)
{
    // Above the early return, not below it, and above the frame's branch rather
    // than inside the arm that uses them: a derivative is legal only in uniform
    // control flow, and both tests below are on the material row. What that
    // costs is four derivative operations on a surface with no normal map, which
    // §2's pricing paragraph measured at less than the noise between two runs;
    // what it buys is the module parsing at all in the browser.
    float3 dpdx = ddx(input.world_position);
    float3 dpdy = ddy(input.world_position);
    float2 duvdx = ddx(uv);
    float2 duvdy = ddy(uv);

    if (layer == 0u)
    {
        return normal;
    }

    TangentFrame basis;
    if ((input.frame & FRAME_AUTHORED_TANGENTS) != 0u)
    {
        // **Gram-Schmidt, because interpolation is not orthogonality.** The
        // tangent crossed the stage boundary as an ordinary varying and the
        // normal through the cofactor matrix, so the two arrive neither unit
        // length nor perpendicular; removing the normal's component of the
        // tangent is the projection `skinning.slang` applies for the same reason
        // and the one MikkTSpace's own reconstruction makes.
        basis.normal = normal;
        float3 tangent = input.world_tangent - normal * dot(normal, input.world_tangent);
        float length_squared = dot(tangent, tangent);
        // A tangent that collapsed — parallel to the normal after a degenerate
        // transform, or interpolated to nothing across a seam — has no direction
        // left to recover, and the arbitrary frame is finite where a normalised
        // zero is not.
        basis.tangent = length_squared > 1e-16 ? tangent * rsqrt(length_squared)
                                               : orthonormal_tangent(normal);
        // **The handedness is the asset's, carried rather than recomputed** —
        // `FRAME_LEFT_HANDED`, which is glTF's tangent `w` and the sign
        // `QTangent` keeps in its own. It is a property of the UV
        // parameterisation, which no transform changes.
        basis.bitangent = cross(basis.normal, basis.tangent)
            * ((input.frame & FRAME_LEFT_HANDED) != 0u ? -1.0 : 1.0);
    }
    else
    {
        basis = derivative_frame(dpdx, dpdy, duvdx, duvdy, normal);
    }

    float3 texel =
        normal_textures.SampleGrad(base_color_sampler, float3(uv, float(layer)), duvdx, duvdy).xyz;
    float3 tangent_space = texel * 2.0 - 1.0;
    tangent_space.xy *= normal_scale;
    // Normalised after the tilt and again after the change of basis: the first
    // is glTF's own definition, the second is because the frame's three axes are
    // orthonormal only to the precision the interpolation and the projection
    // above left them with.
    tangent_space = normalize(tangent_space);
    return normalize(tangent_space.x * basis.tangent
                     + tangent_space.y * basis.bitangent
                     + tangent_space.z * basis.normal);
}

/// How long a decoded bent direction has to be to be a direction at all.
///
/// **`ssao.slang`'s constant, and it has to be that one.** That file and the
/// two filters behind it carry the argument in full: a real direction survives
/// `Rgba8Unorm` as a unit vector to within about a hundredth, the zero sentinel
/// decodes to under a hundredth, and half way between them is a threshold no
/// rounding on any target can reach from either side. A number of this file's
/// own would be the consumer disagreeing with the producer about whether a
/// pixel was handed a direction.
///
/// `crcbl_shaders::ssao::BENT_NORMAL_MIN_LENGTH` is the Rust mirror and
/// `the_bent_normal_length_matches_the_constant_the_shaders_declare` is what
/// holds all four declarations together.
static const float BENT_NORMAL_MIN_LENGTH = 0.5;

/// The occlusion channel under `position`: the visibility in `r` and the
/// encoded bent direction in `gba`.
///
/// **The fetch is clamped to the image's extent**, which is what makes the
/// renderer's 1×1 white off-switch read as "nothing occludes" instead of as
/// total occlusion — `ambient_occlusion`'s own declaration argues that at
/// length. It costs nothing on a frame-sized channel, where every fragment is
/// inside the image already.
///
/// Shared by the shading below and by the two occlusion views, so a view is the
/// buffer the shading reads rather than a second reading of it that could clamp
/// differently.
float4 occlusion_at(float2 position)
{
    uint width;
    uint height;
    ambient_occlusion.GetDimensions(width, height);
    int2 texel = min(int2(position), int2(width, height) - 1);
    return ambient_occlusion.Load(int3(texel, 0));
}

/// The direction the ambient term is sampled along: the channel's bent
/// direction where there is one, and `shading_normal` where there is not.
///
/// **The fallback is this fragment's own normal and not a reconstruction.** The
/// sentinel means the gather had no preferred direction to report — an
/// unoccluded surface, a march too short to run, slices that cancelled — or
/// that no occlusion pass ran at all, and in every one of those cases the
/// honest answer is the direction the surface already faces. That is also the
/// frame this file drew before the channel was widened, which is what lets the
/// feature be data rather than a branch on a switch.
///
/// **Normalised after the decode**, because `Rgba8Unorm` quantises: the channel
/// holds a unit vector to within about a hundredth, and an irradiance evaluated
/// along a short normal is an irradiance scaled by a rounding step.
float3 bent_normal_at(float4 occlusion, float3 shading_normal)
{
    float3 decoded = occlusion.gba * 2.0 - 1.0;
    return length(decoded) < BENT_NORMAL_MIN_LENGTH ? shading_normal : normalize(decoded);
}

/// The contact-shadow channel under `position`, in `0..=1`, one meaning the sun
/// reaches this surface unobstructed.
///
/// **The fetch is clamped to the image's extent** for `occlusion_at`'s reason
/// exactly, and here the consequence is louder: unclamped, the renderer's 1×1
/// white off-switch would read as total shadow at every pixel but one, and a
/// frame drawn with the effect *off* would lose its sun entirely.
float contact_at(float2 position)
{
    uint width;
    uint height;
    contact_shadow.GetDimensions(width, height);
    int2 texel = min(int2(position), int2(width, height) - 1);
    return contact_shadow.Load(int3(texel, 0));
}

/// The scalar visibility `occlusion_at` hands back, turned into the per-channel
/// one a coloured surface actually has.
///
/// Jimenez et al. 2016, "Practical Realtime Strategies for Accurate Indirect
/// Occlusion" — the GTAO course notes — fit a cubic in the visibility whose
/// three coefficients are affine in the surface's albedo, and the six numbers
/// below are that fit transcribed rather than re-derived. What it answers is the
/// thing a scalar occlusion term states wrongly: a horizon integral measures how
/// much of the room is *hidden* from a point, but light does not stop at the
/// first thing it meets — it goes on bouncing between the surface and the
/// neighbours occluding it, and each bounce leaves tinted by what it bounced
/// off. So the light reaching the bottom of a crevice is both more than the
/// scalar says and the colour of the crevice, and the redder the wall the redder
/// what survives.
///
/// **Never darker than what it was handed.** The `max` is part of the published
/// fit and not a guard bolted onto it: for a dark albedo the cubic runs *below*
/// the visibility across nearly the whole range — there is no bounce light off a
/// surface that absorbs everything, so there is nothing for the fit to add — and
/// the `max` is what returns the scalar term unchanged instead of inventing
/// occlusion the pass never measured. This can therefore only ever add ambient,
/// which is the whole of what it claims to do.
///
/// **Exactly one at full visibility, and that is enforced rather than
/// observed.** `crcbl_render::forward` binds a 1×1 white image when it adds no
/// occlusion pass, so with the effect off *every* fragment in the frame arrives
/// here at full visibility, and the frame it draws has to be the frame it drew
/// before this function existed. The published fit does not quite give that on
/// its own: its three coefficients sum to `0.9996 + 0.0005 * albedo`, a hair
/// under one for a dark albedo — where the `max` returns the one — and a hair
/// over it above an albedo of 0.8, where nothing would catch it.
///
/// The `min` closes that end, and it is the one place this function departs
/// from the paper. It is a correction rather than a taste: nothing occludes the
/// fragment, so there is no bounce for the tint to add, and a multiplier above
/// one there would be inventing light out of a least-squares residual. It costs
/// nothing anywhere else, because the cubic clears one at no other visibility
/// in the range. `crcbl_shaders::mesh`'s
/// `the_multi_bounce_tint_leaves_an_unoccluded_fragment_alone` is what holds
/// the identity, and it fails if the `min` goes.
///
/// **Every tier runs it.** It reads nothing the occlusion pass does not already
/// write and adds no pass of its own, so there is no bandwidth for a quality
/// knob to buy back and no `RenderEffects` bit selecting it.
/// `docs/plan/46-ambient-occlusion.md`'s low tier is what asked for it, and the
/// tier split there is about the *widened* target bent normals need — which this
/// does not touch.
float3 multi_bounce_occlusion(float visibility, float3 albedo)
{
    float3 a = 2.0404 * albedo - 0.3324;
    float3 b = -4.7951 * albedo + 0.6417;
    float3 c = 2.7552 * albedo + 0.6903;
    float3 tinted = ((visibility * a + b) * visibility + c) * visibility;
    return min(1.0, max(visibility, tinted));
}

/// This fragment's motion vector: where it is now minus where it was last
/// frame, in texture coordinates.
///
/// # The convention, stated once so no consumer has to guess it
///
/// **Current minus previous, in UV space, `+y` down.** A pass holding the
/// previous frame therefore reads its history at `uv - motion`, and a surface
/// travelling right and down the screen has both components positive. The
/// `float2(0.5, -0.5)` is the whole of the mapping: an NDC `x` of `-1..1` is a
/// `u` of `0..1`, and `v` runs the other way because a framebuffer's rows start
/// at the top.
///
/// Both positions arrive undivided and are divided here rather than at the
/// corners — `VertexOutput::clip_position` argues that at length.
///
/// # A previous position behind the eye writes zero
///
/// Its `w` is zero or negative, so the divide is either a division by zero or a
/// point mirrored through the eye onto the wrong side of the screen. Neither is
/// a motion, and a NaN or a wild vector here is one a temporal consumer would
/// reproject with rather than reject. Zero is the honest answer and it is the
/// same "nothing moved here" the attachment is cleared to, so a consumer needs
/// no second rule for it.
float2 motion_vector(float4 current, float4 previous)
{
    if (previous.w <= 0.0)
    {
        return float2(0.0, 0.0);
    }
    return (current.xy / current.w - previous.xy / previous.w) * float2(0.5, -0.5);
}

[shader("fragment")]
FragmentOutput fragmentMain(VertexOutput input)
{
    // Interpolation denormalises, so this and `to_eye` below are renormalised per
    // fragment. Skipping it makes large triangles darken towards their centres —
    // and would make the normals view below read a shortened normal as a darker
    // colour, which is a picture of the interpolation rather than of the surface.
    //
    // **The surface's normal, not the shading normal.** A normal map perturbs
    // this into `normal` below; what stays with this name is what the geometry
    // said, which is what the shadow biases and the tiling projection want — see
    // `geometric_normal_of` and `physical_tile_uv`, the two readers that must
    // not follow a texture.
    float3 vertex_normal = normalize(input.world_normal);

    // **Computed before the first branch, because every branch writes it.** The
    // debug views below return early and each one owes this target a value —
    // see `FragmentOutput::motion`.
    float2 motion = motion_vector(input.clip_position, input.previous_clip_position);

    // **The motion view, outermost because its sentinel is.** The vector this
    // fragment is about to write, drawn instead of the shaded picture: `r` and
    // `g` are the two components stretched by `MOTION_VIEW_SCALE` and centred
    // on grey, and `b` is zero so the channels that carry something are the two
    // the eye separates. A pixel at rest is grey in `rg` to within one
    // half-float step — `MOTION_VIEW_SCALE` says why not exactly — and it is
    // the only observer the subtraction has until a pass reads the attachment.
    // **The bent-normal view, outermost because its sentinel is.** The direction
    // the occlusion channel carries under this fragment, drawn as the normals
    // view draws a normal — `n * 0.5 + 0.5`, so +X is red, +Y green and +Z blue
    // — which is the encoding the channel already holds, passed through. A
    // fragment with no bent direction is the mid grey the zero sentinel encodes
    // to, and a frame drawing without `RenderEffects::AMBIENT_OCCLUSION` is
    // that grey everywhere, because the 1×1 placeholder holds the sentinel too.
    if (frame.ambient.w >= BENT_NORMAL_VIEW)
    {
        FragmentOutput bent;
        // Straight into the `Rgba16Float` scene target, on the occlusion view's
        // terms: every channel is in `[0, 1]`, and the tonemap resolves that as
        // the identity at the default exposure.
        bent.lit = float4(occlusion_at(input.position.xy).gba, 1.0);
        // Not reflective, for the normals view's reason: `ssr.slang` must march
        // nothing off a debug picture.
        bent.reflectivity = NO_REFLECTION;
        bent.motion = motion;
        return bent;
    }

    if (frame.ambient.w >= MOTION_VIEW)
    {
        FragmentOutput moved;
        moved.lit = float4(motion * MOTION_VIEW_SCALE + 0.5, 0.0, 1.0);
        // Not reflective, for the normals view's reason: `ssr.slang` must march
        // nothing off a debug picture.
        moved.reflectivity = NO_REFLECTION;
        moved.motion = motion;
        return moved;
    }

    // **The normals view, and it is *world*-space normals.**
    //
    // The mapping is the conventional `n * 0.5 + 0.5`: +X is red, +Y green, +Z
    // blue, and a face whose winding was inverted reads as the complement of the
    // colour it should have — which is the whole diagnostic. Its usefulness rests
    // on a face keeping one colour while the camera orbits, so that "this face is
    // inverted" is a thing a modeller *sees* rather than infers from a picture
    // that re-colours whenever they move. View-space normals answer the other
    // question — is this normal smooth — by re-colouring on purpose, and they are
    // not what this file could answer anyway: the block carries `view_proj` and
    // no separate view matrix, so there is no rotation here to take a normal into
    // eye space with.
    //
    // The branch is on a **uniform**, so it is wave-uniform on every target and
    // returning before the texture fetch below costs no derivative anything else
    // needs.

    // **The occlusion view, outermost because its sentinel is.** The
    // ambient-occlusion channel drawn as grey and nothing else — one is white,
    // fully occluded is black — which is how the two techniques in
    // `docs/plan/46-ambient-occlusion.md`'s ladder are actually compared. A
    // composited frame shows occlusion times albedo times ambient, and a
    // difference in the first is not separable from a difference in the other
    // two by looking.
    //
    // **On a frame drawing without `RenderEffects::AMBIENT_OCCLUSION` this is
    // white everywhere**, because that is the 1×1 image the renderer binds
    // instead of a computed channel. That is the honest picture and not a bug to
    // paper over: "no occlusion was computed" and "nothing occludes here" are the
    // same value by construction, and the ambient term multiplies by it either
    // way.
    if (frame.ambient.w >= OCCLUSION_VIEW)
    {
        FragmentOutput occlusion;
        float value = occlusion_at(input.position.xy).r;
        // Straight into the `Rgba16Float` scene target, on the normals view's
        // terms: the tonemap resolves it as `saturate(colour * exposure)`, the
        // identity on `[0, 1]` at the default exposure, so the channel reaches
        // the swapchain unchanged. Grey rather than a ramp, because the eye
        // reads a gradient in luminance and a false-colour ramp would put a
        // visible edge where the ramp changes hue and none where the occlusion
        // steps.
        occlusion.lit = float4(value, value, value, 1.0);
        // Not reflective, for the normals view's reason: `ssr.slang` must march
        // nothing off a debug picture.
        occlusion.reflectivity = NO_REFLECTION;
        occlusion.motion = motion;
        return occlusion;
    }

    if (frame.ambient.w >= LOD_VIEW)
    {
        FragmentOutput tint;
        // Straight through, exactly as the normals view does: the colour was
        // chosen by the stage that knew the level and this one must not shade
        // it, or two levels with similar tints become one under a light.
        tint.lit = float4(input.color.rgb, 1.0);
        tint.reflectivity = NO_REFLECTION;
        tint.motion = motion;
        return tint;
    }

    // **The material row and this fragment's shading normal, resolved before the
    // normals view rather than after it.** That view exists to answer "which way
    // is this surface facing", and once a normal map is in the frame the honest
    // answer is the perturbed normal the lighting actually shades with — a view
    // that drew the interpolated one would be a picture of the mesh rather than
    // of the material, and would say nothing at all about whether a normal map
    // reached the device the right way up.
    //
    // `material` is flat, so every fragment of a primitive reads the row its
    // instance named.
    GpuMaterial material = materials[input.material];
    // The UV is the vertex's under `TILING_AUTHORED` and derived from the
    // surface's world extent under `TILING_PHYSICAL` — the added branch that
    // makes a tile measure `tile_metres` of surface. Resolved here because both
    // pages are sampled through it: the base colour below, and the normal map
    // inside `shading_normal_of`.
    //
    // **Projected on the surface normal and not on the shading one**, which
    // would be circular — the projection chooses which world plane the UV comes
    // from, and a texture that moved the plane it is sampled through would
    // shimmer wherever the perturbation crossed an axis.
    float2 uv = input.uv;
    if (material.tiling == TILING_PHYSICAL)
    {
        uv = physical_tile_uv(input.world_position, vertex_normal, material.tile_metres);
    }
    float3 normal =
        shading_normal_of(normal_layer(material), material.normal_scale, input, vertex_normal, uv);

    if (frame.ambient.w >= NORMALS_VIEW)
    {
        FragmentOutput normals;
        // Straight into the `Rgba16Float` scene target, which the tonemap then
        // resolves as `saturate(colour * exposure)` — the identity on `[0, 1]` at
        // the default exposure, so the encoded normal reaches the swapchain
        // exactly. A caller who has moved the exposure has asked for the picture
        // to be scaled and gets that here too; the operator is monotonic per
        // channel either way, so which channel dominates never changes.
        // **The shading normal, so a normal map shows up here.** `normal` is
        // what `shading_normal_of` returned above, which is the vector the
        // light loop below would have used — a view that drew the interpolated
        // one instead would be a picture of the mesh and would say nothing
        // about whether the page reached the device the right way up.
        normals.lit = float4(normal * 0.5 + 0.5, 1.0);
        // Not reflective, so `ssr.slang` marches nothing off these pixels and
        // `ssr_blur.slang` composites nothing onto them. A zero `F0` still leaves
        // Schlick's grazing tail, so a renderer running `RenderEffects::REFLECTIONS`
        // over a scene with probes in it adds a faint environment along the
        // silhouette — the one place this view is not the encoded normal alone.
        normals.reflectivity = NO_REFLECTION;
        normals.motion = motion;
        return normals;
    }

    float3 to_eye = normalize(frame.camera_position.xyz - input.world_position);
    // The facet under the shading normal, taken once for the fragment and read
    // by the shadow biases alone — never by the shading, which is the authored
    // normal's job. `geometric_normal_of` says why it cannot live inside them.
    float3 geometric_normal = geometric_normal_of(input.world_position, vertex_normal);

    // Both halves of §3.2's "texture indices + factors", multiplied together.
    // The sample is why the lookup had to move to this stage at all: a texture
    // fetch is not constant across a primitive, so nothing it feeds could have
    // been folded into a varying at the corners.
    //
    // The texel arrives **linear** — the page is an sRGB-format image, so the
    // sampler decoded it — which is the space `base_color` and `input.color`
    // are in. See the binding above.
    //
    // The UV was resolved above the normals view, because the normal page is
    // sampled through it too. The wrapped sampler is what lets the physical UV,
    // which runs past `0..1`, repeat rather than smear.
    float4 texel = base_color_textures.Sample(
        base_color_sampler, float3(uv, float(base_color_layer(material))));
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

    // Both tables, read once for the fragment through one set of texel
    // coordinates. The pair is what an area light scales its polygon integral
    // by and what the multi-scatter compensation below adds together, so a
    // fragment with no area light in its froxel pays exactly what it paid
    // before this rung: four `Load`s and a blend.
    TableTap table = table_tap(n_dot_v, roughness);
    float2 dfg = dfg_at(table);
    float3 lobe_energy = f0 * dfg.x + dfg.y;

    // Accumulated rather than branched on, which is the whole of what making the
    // sun a row bought: one loop shades a directional, a point, a spot and a
    // rectangle, and topic 18's "one material model, one BRDF" holds by
    // construction instead of by four copies of the same maths agreeing.
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
        else if (light.kind == KIND_RECT)
        {
            // The **window and not the inverse square**: the polygon integral
            // below already shrinks with distance, because that is what a solid
            // angle does. `range_window` says what would happen if both were
            // applied. `to_light` is the direction to the rectangle's centre,
            // which nothing in the shading of one reads — it is here for the
            // shadow selection below, which refuses a rectangle a tile.
            float3 offset = light.position.xyz - input.world_position;
            float distance = length(offset);
            to_light = offset / max(distance, 1e-6);
            reach = range_window(distance, light.position.w);
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

        // Read by the shadow biases below and by the punctual shading; a
        // rectangle replaces both terms outright and keeps this only for the
        // biases.
        float n_dot_l = dot(normal, to_light);
        float diffuse;
        float3 specular;
        if (light.kind == KIND_RECT)
        {
            // **The rung's whole shading, in four lines.** The clamped-cosine
            // integral over the rectangle is the diffuse term outright — a
            // Lambert lobe *is* a clamped cosine, so there is no approximation
            // in that half at all. The specular one is the same integral over
            // the same rectangle seen through the fitted transform, scaled by
            // the energy the lobe returns, which is the pair already read
            // above. `docs/plan/44-lighting.md`'s rung 5 is the argument and
            // `crcbl_shaders::ltc` is the fit.
            float3 corners[4];
            rect_corners(light, input.world_position, corners);
            float3x3 to_local = ltc_shading_frame(normal, to_eye, n_dot_v);
            diffuse = ltc_irradiance(to_local, corners);
            specular = ltc_irradiance(mul(ltc_transform(ltc_at(table)), to_local), corners)
                     * lobe_energy;
        }
        else
        {
            // Lambert.
            diffuse = max(n_dot_l, 0.0);

            // Cook-Torrance, one GGX lobe, driven by the row's own two factors
            // — where this was a Blinn power and a strength constant no caller
            // could vary. The half-vector form survives the change: it is what
            // the microfacet model is written in, rather than an approximation
            // of it.
            float3 half_vector = normalize(to_light + to_eye);
            specular = ggx_lobe(alpha2, f0, diffuse, n_dot_v,
                                max(dot(normal, half_vector), 0.0),
                                max(dot(to_eye, half_vector), 0.0));
            // No light on the back of a surface, whatever the half-vector says.
            // The `N·L` the lobe left out is the same one Lambert already
            // carries, so both terms take it from here and a back-facing
            // fragment loses both.
            specular *= diffuse;
        }

        // **A fill light lights and does not glint.** The other half of the
        // flag — that it casts no shadow — is `crcbl_render::shadow::Selection`
        // refusing it a tile, so nothing below has to test for it a second
        // time. `crcbl_shaders::light::FLAG_FILL` argues why it is a flag
        // rather than a kind of light.
        if ((light.flags & FLAG_FILL) != 0u)
        {
            specular = float3(0.0, 0.0, 0.0);
        }

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
            reach = sun_visibility(input.world_position, to_light, n_dot_l, geometric_normal,
                                   input.position.xy)
                  // The contact term, and it multiplies where the cascades
                  // *replace*: the atlas answers for everything a cascade texel
                  // can resolve and this answers for the sliver under it, so the
                  // two are independent occluders of the same light rather than
                  // two opinions about one. `contact_shadow`'s declaration says
                  // why it is the sun's alone.
                  * contact_at(input.position.xy);
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
                                          n_dot_l, geometric_normal, input.position.xy);
            }
        }
        else if (light.shadow_tile < SHADOW_LIGHT_TILES)
        {
            reach *= spot_visibility(light, light.shadow_tile, input.world_position, to_light,
                                     n_dot_l, geometric_normal, input.position.xy);
        }
        diffuse *= reach;
        specular *= reach;

        direct += light.color.rgb * diffuse;
        gloss += light.color.rgb * specular;
    }

    // **Multi-scatter energy compensation, applied to the summed lobe rather
    // than inside the loop.** The factor depends on the material and on the view
    // — `f0`, `roughness`, `N·V` — and on nothing a light carries, so it is the
    // same number for every light in the froxel and one multiply outside the
    // loop is the whole of it. `specular_compensation` carries the derivation;
    // `docs/plan/44-lighting.md`'s rung 1 is what asked for it.
    //
    // **The diffuse term is left alone**, and that is the model rather than an
    // omission: what the lobe dropped is light that went on bouncing *within the
    // microsurface* and left it as specular, so putting it back anywhere else
    // would be inventing energy in a term that never lost any.
    gloss *= specular_compensation(f0, dfg.x + dfg.y);

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
    float4 occlusion_texel = occlusion_at(input.position.xy);
    float occluded = occlusion_texel.r;

    // **The direction the ambient term is sampled along.** The same sweep that
    // measured how much of the room is hidden also reported which way what is
    // left of it lies, and a surface in a corner is lit by the opening rather
    // than by the wall its normal points at. `bent_normal_at` is where the
    // sentinel — no direction here, or no occlusion pass at all — comes back as
    // this fragment's own shading normal, which is the term this line had
    // before the channel was widened.
    float3 bent_normal = bent_normal_at(occlusion_texel, normal);

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
    //
    // The gradient sky is a third term on those same terms — `docs/plan/43-render-standards.md`
    // §8. It is added and not chosen between, because a scene may have a sky
    // above it and bounce light around it, and both reach a diffuse surface as
    // an L1 environment. A frame with no sky adds exactly zero.
    //
    // **Both directional terms take the bent normal, and the flat one cannot.**
    // `frame.ambient.rgb` is a constant over the sphere — there is no direction
    // to sample it along — while the sky and the probe grid are L1 environments
    // evaluated at a normal, which is exactly the hook a bent direction is for.
    float3 irradiance = frame.ambient.rgb + sky_irradiance(bent_normal)
                      + probe_irradiance(input.world_position, bent_normal);

    // **The ambient scales the diffuse albedo, so a metal has no ambient
    // term** — and that is the model rather than an omission. A conductor
    // reflects its surroundings instead of scattering them, so what it owes the
    // room is a *reflection*, and `ssr.slang` is what supplies it: the probe
    // grid above is the diffuse half of one design, an `ssr.slang` miss returns
    // those same probes as the specular half, and `ssr_blur.slang` is what adds
    // the result on top of this pass's output. So a fully metallic surface out
    // of every light's reach is the environment rather than black, and black
    // only under a zeroed probe volume with nothing on screen for it to
    // reflect. `apps/lantern/src/room.rs`'s `MIRROR` and `ROUGH_METAL` rows are
    // conductors, and `apps/lantern/tests/golden.rs`'s
    // `zero_probes_only_remove_the_ssr_and_rough_fallbacks` is what measures
    // those two shares apart on them.
    //
    // **Occlusion scales the probes too**, and that is the same statement as
    // above rather than a new one: how much of the surrounding room a surface
    // can see is what modulates the environment reaching it, and the grid is an
    // environment.
    //
    // **And it reaches the ambient as a tint rather than as one number.** The
    // scalar is how much of the room is hidden; what a coloured surface loses to
    // that is not grey, because the light still arriving has bounced off its
    // neighbours and carries their colour. `multi_bounce_occlusion` is the
    // published fit for that and carries the derivation, including why an
    // unoccluded fragment comes back all but unchanged.
    //
    // **Unconditional, on every tier.** It is arithmetic in a pass every tier
    // already runs — no second target, no second pass — so there is nothing for
    // a quality knob to buy back, and `docs/plan/46-ambient-occlusion.md`'s tier
    // split is about the widened target bent normals want rather than about
    // this.
    float3 ambient_visibility = multi_bounce_occlusion(occluded, diffuse_albedo);

    float3 lit = diffuse_albedo * (irradiance * ambient_visibility + direct) + gloss;

    // **Emission is added last and scaled by nothing.** Not by the occlusion,
    // which asks how much of the room reaches a surface and has no bearing on
    // what the surface produces; not by the albedo, which is what a surface does
    // to light arriving at it. A lamp's filament is as bright in a corner as in
    // the open, and a black emissive surface is still black — the two are
    // independent, which is why this is a sum and not a factor.
    //
    // Unclamped for `output.lit`'s reason below: a value above one is what the
    // bloom chain reads as a glow, and clamping here would throw exactly that
    // away.
    lit += emissive_of(material);

    // **Exponential height fog, and it is the last thing that touches the
    // radiance.** Everything above answers what left the surface; this answers
    // how much of it survived the air on the way to the eye, so it composites
    // over the finished sum — the emission included, since a lamp seen through
    // fog is as fogged as the wall beside it.
    //
    // **Written as two multiplies and not as a `lerp`.** At zero density the
    // transmittance is exactly one, and `lit * 1 + fog * 0` returns `lit` bit
    // for bit where `lerp(fog, lit, 1)` returns `fog + (lit - fog)` and loses
    // the low bits of any HDR value far from the fog colour. That is what makes
    // the whole feature additive-zero: a frame with no fog configured is the
    // frame this engine drew before fog existed, and every golden blessed then
    // still matches. `docs/plan/43-render-standards.md` §4.
    //
    // The ray is eye to fragment, and both ends are measured from the fog's
    // reference plane. Unbranched for the probe grid's reason: a feature made
    // of data has nothing to select.
    float3 fog_offset = frame.camera_position.xyz - input.world_position;
    float fog_depth = fog_optical_depth(frame.fog_params.x, frame.fog_params.y,
                                        frame.camera_position.y - frame.fog_params.z,
                                        input.world_position.y - frame.fog_params.z,
                                        length(fog_offset));
    float fog_survives = fog_transmittance(fog_depth);
    lit = lit * fog_survives + frame.fog_color.rgb * (1.0 - fog_survives);

    FragmentOutput output;
    // Deliberately **not** clamped: the target is `Rgba16Float` and the tonemap
    // pass is what maps this into the swapchain. A `saturate` here would throw
    // away the very range HDR-from-P1 exists to carry — see `tonemap.slang`.
    output.lit = float4(lit, albedo.a);
    // `roughness` is the clamped lobe input, stored as the level it will
    // reload as — `REFLECTIVITY_LEVELS` says why the rounding happens here.
    output.reflectivity =
        float4(f0, floor(roughness * REFLECTIVITY_LEVELS + 0.5) / REFLECTIVITY_LEVELS);
    output.motion = motion;
    return output;
}
