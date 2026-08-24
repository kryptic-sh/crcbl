// crcbl-targets: spirv, wgsl, msl, dxil

// GPU skinning as a compute prepass: a joint palette and the bind-pose vertex
// pool in, skinned vertices written back into a *transient region of that same
// pool*.
//
// `docs/plan/17-animation.md`'s "GPU skinning" section is the design, and one
// sentence of it shapes every decision in this file: "Renderer consumes them
// like any static mesh — **vertex pulling and the GPU-driven pipeline don't
// know skinning exists**." So what this writes is `MeshVertex`, the identical
// struct `mesh.slang` pulls with `SV_VertexID`, byte for byte and field for
// field — and a skinned draw is an ordinary draw whose base vertex points at
// the region this pass filled. Nothing downstream grows a skinning branch:
// not the vertex stage, not the cull pass, not the draw generator, not the
// shadow passes that read the same pool.
//
// That is also why `color` and `uv` are copied through unchanged rather than
// left alone. The destination is a *different* range of the pool from the
// source, so a field this pass does not write is a field holding whatever the
// allocator last put there.
//
// # Nothing dispatches this yet
//
// There is no render-graph pass and no host-side consumer anywhere in the tree.
// This slice is the shader and the layout `crcbl_shaders::skinning` pins against
// it, deliberately and only that; the pass, the pool's transient region and the
// prev/current ping-pong that topic 17's 2026-07-27 correction asks for are the
// slices after it. The shader is finished — it is not a stub — but a reader
// looking for the thing that runs it will not find one.
//
// # Compute, and every backend can run it
//
// Topic 17 again: "The browser gets the same compute path — skinning is just a
// compute shader, no bindless needed — so wasm gets animated characters day
// one. Skinning is independent of every path selector in
// `docs/plan/39-capabilities.md`." Nothing here needs a feature: a uniform
// block, three storage buffers and a `[numthreads]`, which is the same shape
// `clear_counters.slang` has and the smallest shape a dispatch can have. There
// is no mesh stage, no subgroup op, no wave intrinsic and no 16-bit type
// anywhere below, so the WGSL artifact is a faithful translation rather than an
// approximation of one.
//
// # One dispatch per skinned range
//
// `SkinParams` describes exactly one contiguous run of vertices: where its bind
// pose is in the pool, where its skinned copy goes, where its palette starts.
// A frame with three animated characters is three dispatches.
//
// The GPU-driven form — one dispatch over a table of ranges, with the range
// resolved from the workgroup id — is what §3.3's other passes look like and is
// deliberately *not* here. It needs a range table, which is a pool-side
// structure the slice that allocates the transient region owns; guessing its
// shape now would be guessing at a layout nothing yet writes.
//
// # Input and output share one binding
//
// `vertices` is bound once, read at `input_base + i` and written at
// `output_base + i`, rather than as a read-only binding and a writable one over
// the same buffer. WebGPU forbids the second arrangement outright: a buffer
// range used as writable storage may not appear in any other way in the same
// usage scope, so the two-binding spelling is a `createBindGroup` failure in
// the browser rather than a stylistic alternative to this one.
//
// The two ranges must not overlap — a vertex read after its own slot has been
// written is one invocation's output feeding another's input, with no ordering
// anywhere to say which. That is the caller's obligation, it is a property of
// how the pool suballocates rather than anything a shader could test, and
// `crcbl_shaders::skinning::Params::to_bytes` is where it is actually enforced:
// it refuses a block whose two ranges intersect.

/// The lengths and offsets one dispatch cannot derive for itself, matching
/// `crcbl_shaders::skinning::Params`.
struct SkinParams
{
    /// Vertices in this range. Invocations at or past it do nothing — see
    /// `computeMain`, where the whole of the tail-invocation story is.
    uint vertex_count;
    /// First vertex of the **bind-pose** run, as an index into the vertex pool.
    uint input_base;
    /// First vertex of the **skinned** run, as an index into the same pool.
    ///
    /// The transient region topic 17 asks for. It is a different range of one
    /// buffer rather than a second buffer, which is what lets a skinned mesh be
    /// drawn, culled and shadowed by passes that only ever knew about the pool.
    uint output_base;
    /// First entry of this range's run of `SkinBinding`s.
    ///
    /// Its own base rather than `input_base` reused: the skin bindings are a
    /// buffer holding only skinned geometry, so a pool full of static meshes
    /// costs nothing there. Making one index serve both would force that buffer
    /// to be as long as the vertex pool.
    uint binding_base;
    /// First matrix of this range's joint palette.
    ///
    /// One buffer holds every animated instance's palette — topic 17's "joint
    /// palettes (SSBO, all animated instances)" — so a range names where its
    /// own begins.
    uint joint_base;
    /// Joints in this range's palette. Every index is clamped against it.
    ///
    /// Never zero: `crcbl_shaders::skinning::Params::to_bytes` refuses a block
    /// that says so, which is what makes `joint_count - 1` below safe to write.
    uint joint_count;
};

/// One vertex, matching `struct MeshVertex` in `mesh.slang` and
/// `crcbl_shaders::mesh::MeshVertex`.
///
/// Both the input and the output layout, and that is the point of the whole
/// pass: what this writes is what the vertex stage pulls, so the renderer needs
/// no second vertex format and no branch to choose between them.
struct MeshVertex
{
    float4 position;
    float4 normal;
    float4 color;
    /// Base-colour texture coordinates in `xy`; `zw` unused.
    float4 uv;
};

/// A vertex's four joints and their four weights, matching
/// `crcbl_shaders::skinning::SkinBinding`.
///
/// # Why the joint indices are `uint`s
///
/// glTF stores `JOINTS_0` as unsigned bytes or shorts and
/// `crcbl_scene::GltfPrimitive::joints` widens either to `[u16; 4]`, so the
/// obvious packing is two `uint`s holding four 16-bit halves. It buys **nothing
/// at all**: `std430` aligns the `float4` beside them to 16, so a `uint2` here
/// is followed by eight bytes of padding and the struct is 32 bytes either way.
/// The packed spelling would cost the same memory, plus a shift and a mask per
/// joint per vertex, plus a rule the CPU side has to reproduce exactly — and
/// WGSL has no 16-bit integer type to unpack into, so the browser would do that
/// arithmetic in 32-bit registers regardless.
///
/// Four `uint`s is therefore the cheaper spelling as well as the plainer one,
/// and it is a layout that cannot drift.
struct SkinBinding
{
    /// Which four joints of this range's palette own the vertex, each relative
    /// to `SkinParams::joint_base`.
    ///
    /// Clamped against `SkinParams::joint_count` where they are used. A file
    /// naming a joint its skin has not got is malformed, and reading past a
    /// storage buffer's end is undefined on the backends that do not bound
    /// every access — so the clamp is what keeps a malformed asset producing a
    /// wrong vertex rather than an undefined read.
    uint4 joints;
    /// How much of the vertex each of those four joints owns, in the same
    /// order.
    ///
    /// **Used exactly as stored.** See `computeMain`.
    float4 weights;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SkinParams> skin;

/// Every animated instance's joint palette, end to end; this range's starts at
/// `SkinParams::joint_base`.
///
/// A matrix here is `crcbl_anim::Palette::matrices`' — the joint's global
/// transform times its inverse bind matrix, mesh space to posed space, with the
/// skinned mesh node's own transform deliberately absent (glTF §3.7.4.2). Where
/// the character *stands* is its `GpuInstance::transform`, applied by the vertex
/// stage afterwards like any other object's.
///
/// Stored the way `glam::Mat4::to_cols_array` produces it and read with no
/// transpose, for the reason `mesh.slang`'s header sets out in full: a
/// `float4x4` comes out of `slangc` decorated `RowMajor`, `mul(m, v)` comes out
/// as `OpVectorTimesMatrix`, and composing the two puts the HLSL matrix's column
/// 0 in the first sixteen bytes.
[[vk::binding(1, 0)]]
StructuredBuffer<float4x4> joints;

/// Which joints own each vertex, and by how much. One entry per **skinned**
/// vertex, indexed at `SkinParams::binding_base + i`, parallel to the bind-pose
/// run at `SkinParams::input_base + i`.
[[vk::binding(2, 0)]]
StructuredBuffer<SkinBinding> bindings;

/// The vertex pool: bind pose read out of it, skinned vertices written back
/// into it. The header says why it is one binding and not two, and what the two
/// ranges owe each other.
[[vk::binding(3, 0)]]
RWStructuredBuffer<MeshVertex> vertices;

/// The matrix that carries a normal through the linear part `basis` carries a
/// tangent through, matching `normal_basis` in `mesh.slang` and
/// `mesh_cluster.slang` character for character —
/// `crcbl_shaders::mesh`'s
/// `every_shader_that_transforms_a_normal_builds_it_with_one_function` is what
/// holds every spelling of it equal.
///
/// The cofactor matrix: row `i` is the cross product of the other two rows in
/// cyclic order, which is `det(basis)` times the inverse transpose. A normal is
/// *perpendicular* to a surface and only an angle-preserving transform carries a
/// perpendicular the way it carries a tangent, so the bare 3×3 is the wrong
/// matrix the moment the blend is anything but a rotation. The determinant is a
/// scalar and `computeMain` normalises, so a positive one never survives; a
/// negative one flips the normal onto the side the mirrored winding put the
/// surface on, which is `graphitemaster/normals_revisited`'s result and the
/// reason this is a cofactor rather than an inverse transpose.
float3x3 normal_basis(float3x3 basis)
{
    return float3x3(cross(basis[1], basis[2]),
                    cross(basis[2], basis[0]),
                    cross(basis[0], basis[1]));
}

/// Invocations per workgroup, matching
/// `crcbl_shaders::skinning::WORKGROUP_SIZE`.
///
/// One invocation is one vertex, so a caller dispatches
/// `vertex_count.div_ceil(WORKGROUP_SIZE)` groups.
///
/// # The tail of the last workgroup
///
/// That division rounds up, so the last group is launched with invocations for
/// vertices the range has not got — up to `WORKGROUP_SIZE - 1` of them. They are
/// stopped by the single test below and by nothing else: no clamp, no bound on
/// the buffer, no reliance on the backend's robustness. Without it the tail
/// would read one range's `SkinBinding`s and write over the *next* range's
/// skinned vertices, which is a neighbouring character's mesh dissolving and
/// not an error anything reports.
///
/// # Linear blend skinning, and what the four weights do
///
/// The four palette matrices are blended first and the vertex is transformed
/// once, rather than the vertex being transformed four times and the results
/// blended. For a position the two are identical — the transform is linear in
/// the matrix — and for a normal they are not: blending first is what gives
/// `normal_basis` a single linear part to take the cofactor of, where the other
/// order would need four cofactors and a blend of them, which is a different
/// (and no more correct) answer at four times the cost.
///
/// **The weights are used exactly as stored, and are not renormalised.** The
/// specification asks that a vertex's four weights sum to one;
/// `crcbl_scene::GltfPrimitive::weights` reports them as the file stored them
/// on the grounds that "a file whose weights do not sum is one whose author
/// should hear about it", and renormalising here would be this engine deciding
/// the opposite one layer further down. A set summing to 0.9 pulls its vertex a
/// tenth of the way to the palette's origin, which is a mesh that visibly
/// deflates — the loudest failure available, and free. Renormalising would cost
/// a divide per vertex on the frame's widest loop, would need an answer for a
/// set summing to zero that no per-vertex code can give correctly, and would
/// hide the defect at exactly the moment it is cheapest to see. A cooker that
/// wants normalised weights normalises them once, offline.
///
/// A set summing to zero is therefore defined rather than special-cased: the
/// blended matrix is all zeroes, the position is the palette origin, and the
/// only value that would not be finite is the normal — which is why the
/// normalise below is guarded rather than a bare `normalize`.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= skin.vertex_count)
    {
        return;
    }

    MeshVertex vertex = vertices[skin.input_base + index];
    SkinBinding binding = bindings[skin.binding_base + index];

    // `joint_count` is never zero — the block's writer refuses one — so the
    // subtraction cannot wrap.
    uint last = skin.joint_count - 1;
    uint4 joint = min(binding.joints, uint4(last, last, last, last));

    float4x4 blended = joints[skin.joint_base + joint.x] * binding.weights.x
                     + joints[skin.joint_base + joint.y] * binding.weights.y
                     + joints[skin.joint_base + joint.z] * binding.weights.z
                     + joints[skin.joint_base + joint.w] * binding.weights.w;

    float3 position = mul(blended, float4(vertex.position.xyz, 1.0)).xyz;
    // **Through the cofactor matrix, not through the bare 3×3**, for
    // `normal_basis`' reason: a joint carrying a non-uniform scale, and a blend
    // of joints whose rotations differ, both produce a linear part that moves a
    // normal off its surface's perpendicular. What this does *not* model is the
    // gradient of the weights across the surface: the true deformation is
    // `x ↦ M(x)·x` with `M` varying through the weights, so its Jacobian is this
    // blended linear part plus a term in `∂w/∂x`, and that term is dropped here
    // as it is in every engine that skins per vertex. It is exactly zero where
    // the weights are locally constant — the interior of a rigidly bound region
    // — and largest in the band around a bending joint, which is where skinning
    // artefacts have always lived. A vertex whose weight is entirely on one
    // joint is transformed exactly.
    float3 normal = mul(normal_basis((float3x3)blended), vertex.normal.xyz);

    MeshVertex skinned;
    skinned.position = float4(position, 1.0);
    // A guarded normalise rather than `normalize`, whose value for a zero
    // vector is undefined on all four targets. The blended cofactor is zero for
    // a vertex with no weight at all and for one whose blend collapsed the mesh
    // into a plane; a `NaN` written into the pool from either would not stay
    // local, because the geometry it poisons is read again by the shadow passes
    // and reaches the raster as triangles that simply vanish. The bind-pose
    // normal is the fallback: finite, already unit length, and obviously not the
    // posed answer.
    float square_length = dot(normal, normal);
    skinned.normal = float4(square_length > 0.0 ? normal * rsqrt(square_length)
                                                : vertex.normal.xyz,
                            0.0);
    skinned.color = vertex.color;
    skinned.uv = vertex.uv;

    vertices[skin.output_base + index] = skinned;
}
