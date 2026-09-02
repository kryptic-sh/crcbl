// crcbl-targets: spirv, wgsl, msl, dxil

// One face of one probe's depth cube: the static geometry drawn from the
// probe's own position, writing **how far away it is** rather than a depth.
//
// `docs/plan/50-irradiance-probes.md`'s raster producer. A probe's visibility
// map wants the distance to the nearest surface in every direction, and the
// cheapest thing that answers that for every direction at once is the same
// thing a reflection capture is: six 90° views from the probe, tiled into one
// atlas. `probe_octahedral.slang` is what folds those six tiles into the
// octahedral layer `mesh.slang` reads, and `crcbl_render::probe_capture` is
// what places the tiles and builds the matrices.
//
// # Distance, not depth
//
// The colour target is `R32Float` and holds `length(world - probe)`. A depth
// buffer holds the projected `z`, which is the distance along the *face's
// axis*, and turning that back into a distance in the direction of a given
// texel means inverting the projection in the resolve — one more place the
// face's near and far planes have to be agreed on, for a value the fragment
// stage already has in hand. The depth attachment still exists and still
// resolves which surface is nearest, because at a fixed pixel the two orders
// agree: the ray through that pixel is one ray, and along it `z` and distance
// are both monotonic.
//
// # Two-sided
//
// No culling, matching what this capture replaced: a probe standing inside
// geometry sees that geometry's back faces, and the distance to them is
// exactly what says it is inside.

/// One tile of the atlas: where its face looks from, and where "from" is.
struct CaptureFace
{
    /// World → this face's clip space. A 90° reversed-Z perspective down one
    /// of the six axes — `crcbl_render::probe_capture::face_matrix`, which is
    /// `crcbl_render::shadow::point_matrix`'s shape and its face order.
    ///
    /// Stored as `glam::Mat4::to_cols_array` writes it and read with no
    /// transpose, on `mesh.slang`'s header's terms.
    float4x4 view_proj;
    /// The probe's world position in `xyz`. `w` is unused padding, because a
    /// uniform block's members are `float4`-aligned anyway.
    float4 origin;
};

/// The tile being drawn. One dynamic offset per tile, which is how a device
/// with no push constants selects it — see `crcbl_hal::BindingKind::UniformBuffer`.
[[vk::binding(0, 0)]]
ConstantBuffer<CaptureFace> capture;

/// Every occluding triangle in **world space**, three floats a vertex and
/// three vertices a triangle, in no order.
///
/// A soup rather than the vertex pool and an index buffer: the pool's meshes
/// are cluster DAGs whose level-0 triangles have to be walked to be drawn at
/// all, and `crcbl_render::probe_visibility::Occluders` has already done that
/// walk on the host for its own reasons. Pulling from a flat buffer here means
/// this pipeline needs no instance table, no material table and no cull.
[[vk::binding(1, 0)]]
StructuredBuffer<float> positions;

/// What the vertex stage hands the fragment stage: the clip position the
/// rasteriser needs, and the world position the distance is measured from.
struct CaptureVertex
{
    float4 position : SV_Position;
    float3 world : WORLD;
};

[shader("vertex")]
CaptureVertex vertexMain(uint vertex: SV_VertexID)
{
    uint at = vertex * 3u;
    float3 world = float3(positions[at], positions[at + 1u], positions[at + 2u]);

    CaptureVertex output;
    output.world = world;
    output.position = mul(capture.view_proj, float4(world, 1.0));
    return output;
}

[shader("fragment")]
float4 fragmentMain(CaptureVertex input) : SV_Target
{
    // Interpolated **world position** and then a length, rather than an
    // interpolated distance: distance is not a linear function of position, so
    // a varying carrying it would be wrong everywhere but the three corners.
    float reach = length(input.world - capture.origin.xyz);
    return float4(reach, 0.0, 0.0, 0.0);
}
