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
// The draw is still one instance, so `SV_InstanceID` is 0 and the cube is
// instance 0. That is the whole of what a non-indirect draw can address here,
// and the reason is a target disagreement rather than a shortcut: the emitted
// SPIR-V, DXIL and MSL all give `SV_InstanceID` HLSL's meaning — the SPIR-V
// subtracts `BaseInstance` from `InstanceIndex` to get there — while the
// emitted WGSL indexes with `@builtin(instance_index)` raw, which in WebGPU
// starts at the draw's `first_instance`. So a draw's base instance reaches this
// shader as two different numbers depending on the target, and it is not used.
// §3.3's indirect path does not need it: the cull pass writes the visible
// instance ids the draw reads.

/// One vertex of a hardcoded mesh, matching `crcbl_shaders::mesh::MeshVertex`.
///
/// Three `float4`s rather than a packed `float3` triple: under `std430` a
/// `float3` is 16-byte aligned *and* 12 bytes wide, so a struct of them has
/// padding the CPU side has to reproduce exactly. Three `float4`s are 48 bytes
/// with no padding anywhere, which is a layout that cannot drift. The `w`
/// components are unused; P9's real vertex format packs them (topic 03 §3.1).
struct MeshVertex
{
    float4 position;
    float4 normal;
    float4 color;
};

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
};

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance`.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.2's "transform, mesh id, material
/// id, flags", plus the sector id its 2026-07-27 correction adds. The three ids
/// are written by the CPU and read by nothing yet — see the Rust struct, which
/// carries the full account of what each is for and which slice consumes it.
struct GpuInstance
{
    /// Model → sector. Rigid (rotation + translation), so its upper-left 3×3
    /// transforms normals correctly without an inverse-transpose.
    float4x4 transform;
    /// Which mesh to draw. **Reserved**: the draw resolves its mesh range on
    /// the CPU, and the GPU-side mesh table is §3.3's.
    uint mesh;
    /// Which material to shade with. **Reserved**: the material table is the
    /// other half of §3.2 and is not built.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. **Reserved**: no bit is defined. It costs nothing —
    /// the struct is 16-byte aligned, so without it the three ids would be
    /// followed by four bytes of padding instead.
    uint flags;
};

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame;

[[vk::binding(1, 0)]]
StructuredBuffer<MeshVertex> vertices;

[[vk::binding(2, 0)]]
StructuredBuffer<GpuInstance> instances;

struct VertexOutput
{
    float4 position : SV_Position;
    float3 world_position : POSITION0;
    float3 world_normal : NORMAL0;
    float4 color : COLOR0;
};

[shader("vertex")]
VertexOutput vertexMain(uint index: SV_VertexID, uint instance_index: SV_InstanceID)
{
    MeshVertex vertex = vertices[index];
    GpuInstance instance = instances[instance_index];

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
    return output;
}

/// Blinn-Phong specular exponent.
///
/// One constant rather than a material parameter: `docs/plan/02-vulkan-backend.md`
/// §2.5 rung 4 is "enough to see geometry properly; real material model comes
/// with stage 3/5", and a one-field material system would be a guess at the
/// shape `docs/plan/37-materials.md` owns.
static const float SPECULAR_POWER = 32.0;

/// How much of the light colour the specular lobe contributes.
static const float SPECULAR_STRENGTH = 0.35;

[shader("fragment")]
float4 fragmentMain(VertexOutput input) : SV_Target
{
    // Interpolation denormalises, so both of these are renormalised per
    // fragment. Skipping it makes large triangles darken towards their centres.
    float3 normal = normalize(input.world_normal);
    float3 to_light = normalize(frame.light_direction.xyz);
    float3 to_eye = normalize(frame.camera_position.xyz - input.world_position);

    // Lambert.
    float diffuse = max(dot(normal, to_light), 0.0);

    // Blinn: the half-vector form, not Phong's reflection vector. Cheaper, and
    // it is what every later specular model in this engine is a refinement of.
    float3 half_vector = normalize(to_light + to_eye);
    float specular = pow(max(dot(normal, half_vector), 0.0), SPECULAR_POWER);
    // No light on the back of a surface, whatever the half-vector says.
    specular *= step(0.0, diffuse) * diffuse;

    float3 lit = input.color.rgb * (frame.ambient.rgb + frame.light_color.rgb * diffuse)
               + frame.light_color.rgb * (specular * SPECULAR_STRENGTH);

    // Deliberately **not** clamped: the target is `Rgba16Float` and the tonemap
    // pass is what maps this into the swapchain. A `saturate` here would throw
    // away the very range HDR-from-P1 exists to carry — see `tonemap.slang`.
    return float4(lit, input.color.a);
}
