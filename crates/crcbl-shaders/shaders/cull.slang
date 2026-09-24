// crcbl-targets: spirv, wgsl, msl, dxil

// Frustum culling on the GPU: one thread per instance, a compacted list of the
// survivors, and an atomic counter saying how many there are.
//
// `docs/plan/03-gpu-driven-rendering.md` §3.3's first half — "Compute pass:
// frustum cull against instance AABBs → compacted visible instance list". The
// second half, the `draw_indexed_indirect` records that list turns into, is a
// slice of its own and nothing here anticipates it beyond leaving the list in
// the form it wants: instance indices, ascending nowhere in particular, with a
// count beside them.
//
// **`draw_gen.slang` is what consumes the list**, scattering the survivors into
// per-bucket runs and writing the indirect arguments a draw reads. This pass is
// still checked on its own, by reading the list back and comparing it against a
// CPU reference — `crcbl_render::cull`, which is ordinary Rust and is the
// oracle — because a consumer that drew the right picture from the wrong list
// would say nothing about this one.
//
// # Every slot is tested, so every slot has to say whether it is one
//
// The dispatch walks `0..instance_count` and the array is a pool: an element
// between two live instances may be a slot whose instance was removed, still
// holding the transform and mesh id it had when it was. `GpuInstance::flags`
// bit 0 is what tells them apart, and the entry point asks it before it reads
// anything else in the record. Without that the pass culls — and keeps —
// removed instances on stale data, which is a contract "pass the right
// `instance_count`" cannot make a caller able to keep once slots are reused.
//
// # The structs are declared again here, and that is the rule
//
// `GpuInstance` and `GpuMesh` are also declared in `mesh.slang`. There is no
// shared header: `tools/compile-shaders.sh` hashes one source per artifact, so
// an `#include` would be a file whose edits nothing downstream notices.
// `mesh_shader.slang` re-declares its vertex struct for the same reason. What
// keeps the two copies honest is a test — `crcbl_shaders::cull`'s
// `the_shared_structs_are_declared_identically_in_both_shaders` — which reads
// both sources and compares the declarations character for character.
//
// # An AABB rotated by a transform is not an AABB
//
// The mesh's box is local-space, and the instance's transform rotates it. The
// tight bound of a rotated box is not a box, so this uses the standard
// conservative method: the transformed box's **centre** is the transform
// applied to the local centre, and its **half-extent** is the *absolute value*
// of the transform's 3x3 part applied to the local half-extent. The result
// contains the rotated box and is generally larger than it, which is the
// correct direction for a cull to err in — Ericson, *Real-Time Collision
// Detection* §4.2.6, and the reason a rotated instance is a test case rather
// than an assumption.
//
// # The planes are not normalized, and must not be
//
// The frustum arrives as six half-spaces where a point is inside when
// `dot(plane.xyz, p) + plane.w >= 0`. Both sides of the centre/extent test
// below scale linearly with the plane's normal, so an unnormalized plane gives
// the same answer as a normalized one — and under the engine's reversed-Z
// **infinite** projection one of Gribb-Hartmann's six planes comes out with a
// zero normal (the far plane, which is at infinity and rejects nothing).
// Normalizing would divide that one by zero and turn every test against it into
// a `NaN` comparison, which passes for "outside". `crcbl_render::cull::Frustum`
// therefore hands them over as extracted.
//
// # Three entry points over one set of declarations
//
// `computeMain` is the frustum cull every generator runs — and under
// `CULL_FACES`, a point light's, which also tags each survivor with the faces of
// its cube the box reaches. `occlusionMain` and `lateMain` are
// `docs/plan/03-gpu-driven-rendering.md` §3.3's two occlusion phases, which only
// the camera's generator runs: the first reprojects every frustum survivor into
// the previous frame's farthest-depth pyramid and marks the ones it hides, the
// second tests the marked ones against this frame's early depth and rescues what
// that does not hide. `crcbl_render::cull`'s `occlusion` module is their oracle.
//
// A survivor-list entry is the instance index below `ENTRY_INDEX_MASK` and the
// cull's tags above it; `draw_gen.slang` reads both.

// D3D12 takes each resource's register from its binding. A declaration's
// `D3D12_REGISTER` suffix is `register(<class><binding>, space<set>)`, the
// numbers its `[[vk::binding(binding, set)]]` names, and
// `crcbl_shaders::declaration_order` checks that the two agree. HLSL only,
// because Slang's Metal target would take its argument-table indices from a
// `register` as well.
#if defined(CRCBL_TARGET_HLSL)
#define D3D12_REGISTER(...) : register(__VA_ARGS__)
#else
#define D3D12_REGISTER(...)
#endif

/// One drawable object, matching `crcbl_shaders::mesh::GpuInstance` and
/// `mesh.slang`'s copy.
struct GpuInstance
{
    /// Model → sector. Rotation, translation and scale, **non-uniform
    /// included**: the raster stages carry a normal through `normal_basis`, the
    /// upper-left 3×3's cofactor matrix, rather than through the 3×3 itself.
    float4x4 transform;
    /// Where this instance was **last frame**, matching `mesh.slang`'s copy —
    /// the same model → sector matrix, one frame behind. `InstancePool` writes
    /// it; nothing reads it yet, and this pass never will: where a pixel came
    /// from is a raster-stage question.
    float4x4 previous_transform;
    /// Which mesh to draw: an index into `meshes` below. The vertex stage
    /// resolves this instance's base vertex through it, which is what lets one
    /// draw cover instances of different meshes.
    uint mesh;
    /// Which material to shade with: an index into the material table
    /// `mesh.slang` shades through. Nothing here reads it — this pass decides
    /// what is on screen, not what colour it is.
    uint material;
    /// Which sector `transform` is relative to. **Reserved**: the f64
    /// sector→camera offset table and the addition that would make this mean
    /// anything are §3.3's, so today every instance is in sector 0 and
    /// `transform` is effectively model → world.
    uint sector;
    /// Per-instance bits. Bit 0 is `INSTANCE_LIVE` below: clear means this
    /// element holds a removed instance's leftovers, and the entry point
    /// rejects it before reading anything else. Bit 1 is the base-vertex
    /// override the raster stages read. A set bit retains this instance because
    /// the source mesh bounds do not describe its skinned vertices.
    uint flags;
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh entry's when `INSTANCE_BASE_VERTEX_OVERRIDE` is set in
    /// `flags` and ignored when it is not. A deformed instance names its
    /// **source** mesh above and carries the base of the region a skinning
    /// dispatch wrote it into here, so the bucket, the level tables and the
    /// bounding box all still resolve through the mesh it was made from.
    uint base_vertex;
    /// The pool vertex the **previous frame's** deformation of this instance
    /// was written into, on the same terms as the field above — read by the two
    /// geometry stages when the override bit is set, and by nothing here:
    /// this pass decides what is on screen, and neither question is where a fragment was.
    uint previous_base_vertex;
    /// Pads the record to a multiple of the transform's alignment, matching
    /// `mesh.slang` — where the four targets' disagreement about the tail is
    /// written out.
    uint pad1;
    uint pad2;
};

/// `GpuInstance::flags` bit 0: this element is a live instance, matching
/// `crcbl_shaders::mesh::GpuInstance::LIVE`.
///
/// A named constant rather than a literal `1` at the test below, for the reason
/// `PLANE_COUNT` is one: a bare bit in an expression is a number the next bit
/// this word gains has no way to be checked against.
static const uint INSTANCE_LIVE = 1;

/// `GpuInstance::flags` bit 1: raster stages fetch this instance's vertices
/// from a skinning region instead of the source mesh's bind-pose run.
///
/// The mesh table still holds the source bounds, so they cannot reject a
/// deforming instance: its skinned vertices may have moved into the frustum.
static const uint INSTANCE_BASE_VERTEX_OVERRIDE = 2;

/// Which element of `visible_count` this pass adds to.
///
/// The same number as `crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD`, held in
/// step with it by that module's
/// `the_shaders_index_the_culling_stats_at_the_words_this_crate_says` — which
/// is also what pins `mesh_cluster.slang` to the element beside this one.
static const uint INSTANCE_SURVIVOR_WORD = 0;

/// One resident mesh's range in the geometry pools **and its local-space
/// bounds**, matching `crcbl_shaders::mesh::GpuMesh` and `mesh.slang`'s copy.
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
    /// The mesh's UV range, matching `mesh.slang`. Declared and not read: this
    /// pass resolves ranges and boxes, and a row short of four floats here puts
    /// every entry after it at the wrong offset.
    float uv_scale_u;
    float uv_scale_v;
    float uv_offset_u;
    float uv_offset_v;
    /// The mesh's flags word, matching `mesh.slang`. Declared and not read, for
    /// the UV range's reason exactly: a row short of it puts every entry after
    /// this one at the wrong offset, and nothing anywhere reports that.
    uint flags;
};

/// Planes in a frustum. Six, and the loop below is written against this rather
/// than against a literal.
static const uint PLANE_COUNT = 6;

/// Faces of a point light's cube, matching `crcbl_render::shadow::POINT_FACES`.
static const uint FACE_COUNT = 6;

/// Planes kept per face: the four sides of its 90° pyramid, which are the only
/// planes that separate one face from another — see `CULL_FACES`.
static const uint FACE_PLANES = 4;

/// `FACE_COUNT * FACE_PLANES`, spelled as a literal because it sizes an array
/// in a struct `mesh_cluster.slang` re-declares field for field.
static const uint FACE_PLANE_SLOTS = 24;

/// `CullParams::features` bit: tag each survivor with the point-light faces its
/// box reaches, in `ENTRY_FACE_SHIFT`'s six bits. Read by `computeMain`.
static const uint CULL_FACES = 1;

/// `CullParams::features` bit: the farthest-depth pyramid holds the previous
/// frame, so `occlusionMain` may test against it. Clear on the first frame, on
/// a resize and wherever the pyramid was not built — the second phase alone
/// then decides.
static const uint CULL_OCCLUSION_HISTORY = 2;

/// `CullParams::features` bit: `occlusionMain` drops an instance whose
/// projected box is narrower than `CullParams::small_feature_pixels`.
static const uint CULL_SMALL_FEATURE = 4;

/// The low bits of a survivor-list entry: the instance index. Everything above
/// is a tag, and `crcbl_render::DrawGen::new` refuses a capacity that would
/// reach it.
static const uint ENTRY_INDEX_MASK = 16777215;

/// Where a survivor's six face bits start — bit `ENTRY_FACE_SHIFT + f` is face
/// `f`, written by `computeMain` under `CULL_FACES`.
static const uint ENTRY_FACE_SHIFT = 24;

/// The survivor passed the second phase's test after failing the first:
/// `lateMain` sets it and `draw_gen.slang`'s `lateScatterMain` scatters it.
static const uint ENTRY_RESCUED = 1073741824;

/// The survivor failed the first phase's test against the previous frame's
/// pyramid. `occlusionMain` sets it; the early scatter skips it and `lateMain`
/// tests it again.
static const uint ENTRY_OCCLUDED = 2147483648;

/// Which element of `visible_count` counts the survivors the **first** phase
/// marked occluded, matching `crcbl_shaders::cull::OCCLUSION_EARLY_REJECT_WORD`.
static const uint OCCLUSION_EARLY_REJECT_WORD = 5;

/// Which element counts the ones the **second** phase still found occluded —
/// the instances the frame did not draw — matching
/// `crcbl_shaders::cull::OCCLUSION_LATE_REJECT_WORD`.
static const uint OCCLUSION_LATE_REJECT_WORD = 6;

/// Which element counts the instances the small-feature test dropped, matching
/// `crcbl_shaders::cull::SMALL_FEATURE_REJECT_WORD`.
static const uint SMALL_FEATURE_REJECT_WORD = 7;

/// Farthest-depth pyramid levels bound below, matching
/// `crcbl_shaders::cull::OCCLUSION_MAX_LEVELS`.
static const uint OCCLUSION_MAX_LEVELS = 8;

/// Texels the projected rectangle is widened by on every side before it is
/// looked up, matching `crcbl_shaders::cull::OCCLUSION_GUARD_TEXELS`. The
/// rectangle bounds every pixel centre a rasteriser could cover, and one texel
/// more absorbs the difference between this arithmetic and the vertex stage's.
static const int OCCLUSION_GUARD_TEXELS = 1;

/// How much nearer than the stored depth an instance's nearest point must be
/// before it counts as hidden, as a fraction of that depth — `2^-12`, matching
/// `crcbl_shaders::cull::OCCLUSION_DEPTH_SLACK`. A surface touching the
/// occluder is kept rather than decided by rounding.
static const float OCCLUSION_DEPTH_SLACK = 0.000244140625;

/// A projected coordinate at or past this many pixels from the origin is
/// treated as unprojectable, which keeps the integer conversion below in range.
static const float PROJECTION_LIMIT = 16777216.0;

/// Everything an invocation needs that is not buffer contents, matching
/// `crcbl_shaders::cull::Params`.
///
/// A uniform buffer rather than push constants, for the reason
/// `compute_probe.slang` gives: WGSL has no push constants, and this shader
/// declares that target.
struct CullParams
{
    /// The camera frustum's six half-spaces, `xyz` the (unnormalized) outward
    /// plane normal and `w` its offset. A point is inside the plane when
    /// `dot(xyz, p) + w >= 0`.
    float4 planes[PLANE_COUNT];
    /// Instances to test: the dispatch covers `0..instance_count`. A dispatch
    /// is a whole number of workgroups, so the last one runs past the end
    /// whenever this is not a multiple of the workgroup size.
    uint instance_count;
    /// Elements `visible` can hold. A survivor whose slot is at or past this
    /// still counts — see `visible_count` — but is not written.
    uint capacity;
    /// The `GpuInstance::flags` bit that removes an instance from this cull:
    /// the camera's own bit of the hidden-views mask, or
    /// `GpuInstance::CASTS_NO_SHADOW` for a cull that is no view's — every
    /// shadow cull.
    uint hidden_view;
    /// `CULL_*` bits: what this frame's dispatches do beyond the frustum.
    uint features;
    /// This frame's world → clip, column-major — what the second occlusion
    /// phase and the small-feature test project through.
    float4x4 view_proj;
    /// The previous frame's, which the first phase reprojects through because
    /// the pyramid it reads was drawn with it.
    float4x4 previous_view_proj;
    /// The depth prepass's extent in texels, which level 0 of the pyramid is.
    uint target_width;
    uint target_height;
    /// Levels of the farthest-depth pyramid bound in set 1, level 1 first. Zero
    /// where there is none, which hides nothing.
    uint pyramid_levels;
    uint occlusion_pad;
    /// The small-feature threshold, in pixels of the projected box's longer side.
    float small_feature_pixels;
    float small_feature_pad0;
    float small_feature_pad1;
    float small_feature_pad2;
    /// Under `CULL_FACES`: face `f`'s side planes at `f * FACE_PLANES`, on
    /// `planes`' convention.
    float4 face_planes[FACE_PLANE_SLOTS];
};

[[vk::binding(0, 0)]]
ConstantBuffer<CullParams> cull D3D12_REGISTER(b0, space0);

/// The instance array. `StructuredBuffer`, so this is a read-only storage
/// buffer: the cull pass decides what is visible and never edits an instance.
[[vk::binding(1, 0)]]
StructuredBuffer<GpuInstance> instances D3D12_REGISTER(t1, space0);

/// The mesh table, indexed by `GpuInstance::mesh`. Read-only for the same
/// reason.
[[vk::binding(2, 0)]]
StructuredBuffer<GpuMesh> meshes D3D12_REGISTER(t2, space0);

/// The compacted survivors, as indices into `instances`. **Written in whatever
/// order the atomic below hands slots out**, which is not ascending and is not
/// reproducible between runs — a consumer reads `visible_count` of them and
/// must not assume anything else.
[[vk::binding(3, 0)]]
RWStructuredBuffer<uint> visible D3D12_REGISTER(u3, space0);

/// How many survivors there were, in element `INSTANCE_SURVIVOR_WORD`.
/// `clear_counters.slang` zeroes it before the dispatch.
///
/// The buffer is the frame's culling statistics rather than this pass's own
/// counter: `mesh_cluster.slang`'s amplification stage counts surviving
/// *clusters* into the element beside this one, so the frame has one readback
/// rather than two.
///
/// This is the *true* count, which can exceed `CullParams::capacity`: the
/// atomic increments unconditionally and only the write into `visible` is
/// bounded. So an overflow is a number a caller can see and act on rather than
/// a list that quietly stops growing — the "overflow counter" topic 03's
/// 2026-07-27 correction asks per-bucket capacity to carry.
///
/// **`Atomic<uint>` rather than a plain `uint` with `InterlockedAdd`**, which is
/// what this was first written as. WGSL has no such intrinsic — its atomics are
/// a distinct `atomic<u32>` *type* — and Slang refuses the entry point outright
/// for that target: "entrypoint 'computeMain' uses features that are not
/// available in 'compute' stage for 'wgsl' compilation target", pointing at
/// `InterlockedAdd`. Spelled as a type it lowers on all four: `atomicAdd` in
/// WGSL, `atomic_fetch_add_explicit` in MSL, and `InterlockedAdd` again in the
/// HLSL the DXIL is built from.
[[vk::binding(4, 0)]]
RWStructuredBuffer<Atomic<uint> > visible_count D3D12_REGISTER(u4, space0);

// # The farthest-depth pyramid, for the two occlusion phases
//
// Set 1 is `crcbl_render::occlusion`'s pyramid: level `n` holds, per texel, the
// **farthest** depth of the `2^n`-texel block of the prepass it covers — under
// reversed-Z the `min`, which `hiz.slang`'s `farthestMain` reduces. That is the
// conservative bound an occlusion test needs: every pixel of the block is at
// least that near, so a box whose nearest point is farther still cannot pass
// the forward pass's `GreaterOrEqual` test anywhere in it. `ssr.slang`'s
// pyramid keeps the nearest depth instead, which hides boxes that are really
// visible, and is not what is bound here.
//
// One image per level for `hiz.slang`'s reasons, so one binding per level: a
// WebGPU stage has sixteen sampled textures to spend and this uses eight. Every
// slot is filled — a pyramid shorter than `OCCLUSION_MAX_LEVELS` repeats its
// deepest level — and `CullParams::pyramid_levels` bounds what is read.

[[vk::binding(0, 1)]]
DepthTexture2D pyramid_1 D3D12_REGISTER(t0, space1);
[[vk::binding(1, 1)]]
DepthTexture2D pyramid_2 D3D12_REGISTER(t1, space1);
[[vk::binding(2, 1)]]
DepthTexture2D pyramid_3 D3D12_REGISTER(t2, space1);
[[vk::binding(3, 1)]]
DepthTexture2D pyramid_4 D3D12_REGISTER(t3, space1);
[[vk::binding(4, 1)]]
DepthTexture2D pyramid_5 D3D12_REGISTER(t4, space1);
[[vk::binding(5, 1)]]
DepthTexture2D pyramid_6 D3D12_REGISTER(t5, space1);
[[vk::binding(6, 1)]]
DepthTexture2D pyramid_7 D3D12_REGISTER(t6, space1);
[[vk::binding(7, 1)]]
DepthTexture2D pyramid_8 D3D12_REGISTER(t7, space1);

/// Whether `instance` is a record this view's cull may read at all.
///
/// The first two of the refusals every entry point asks, in the order
/// `computeMain` always asked them. A slot whose instance has been removed keeps
/// whatever transform and mesh id it last had — `crcbl_render::InstancePool::
/// remove` rewrites the flags word and nothing else — so the live bit is asked
/// before anything else in the record is interpreted, including the mesh id
/// that indexes the table.
bool admits(GpuInstance instance)
{
    if ((instance.flags & INSTANCE_LIVE) == 0)
    {
        return false;
    }
    // The view this cull draws for has been told not to draw the instance —
    // `crcbl_shaders::mesh::GpuInstance::HIDDEN_VIEWS_SHIFT`. A shadow cull
    // passes the no-shadow bit instead, so an instance hidden from a camera
    // still casts its shadow and one told not to cast does not.
    return (instance.flags & cull.hidden_view) == 0;
}

/// The world-space box of `instance`'s mesh bounds: its centre and half-extent.
///
/// The conservative transform the header names. `mul(m, v)` is row-wise —
/// `r[i] = dot(m[i], v)` — so `mul(abs(basis), e)` is `r[i] = sum_j |m[i][j]| *
/// e[j]`, which is the world-axis half-extent of the rotated box.
void world_box(GpuInstance instance, GpuMesh mesh, out float3 center, out float3 extent)
{
    float3 bounds_min = float3(mesh.min_x, mesh.min_y, mesh.min_z);
    float3 bounds_max = float3(mesh.max_x, mesh.max_y, mesh.max_z);
    float3 local_center = 0.5 * (bounds_max + bounds_min);
    float3 local_extent = 0.5 * (bounds_max - bounds_min);
    float3x3 basis = (float3x3)instance.transform;
    center = mul(instance.transform, float4(local_center, 1.0)).xyz;
    extent = mul(abs(basis), local_extent);
}

/// Whether the box is inside or straddles `half_space`.
///
/// How far the box can reach towards the plane from its centre, along that
/// plane's normal: the same `abs`-and-dot as the extent above, and the reason
/// neither the plane nor the normal needs to be unit length.
bool reaches(float4 half_space, float3 center, float3 extent)
{
    float radius = dot(abs(half_space.xyz), extent);
    return dot(half_space.xyz, center) + half_space.w >= -radius;
}

/// Whether the box is inside or straddles every plane of the frustum.
bool in_frustum(float3 center, float3 extent)
{
    for (uint plane = 0; plane < PLANE_COUNT; ++plane)
    {
        if (!reaches(cull.planes[plane], center, extent))
        {
            return false;
        }
    }
    return true;
}

/// A box projected to the screen: the pixel rectangle its eight corners span
/// and the nearest reversed-Z depth any of them has.
///
/// `crcbl_render::cull::ScreenBounds` is this in Rust.
struct ScreenBounds
{
    /// False when a corner is on or behind the eye plane, in front of the near
    /// plane, or too far off screen to convert — every caller reads that as
    /// "cannot be shown hidden or small".
    bool valid;
    float min_x;
    float min_y;
    float max_x;
    float max_y;
    /// The largest depth of the eight corners, which under reversed-Z is the
    /// nearest. Depth is monotone along a view ray, so no point of the box is
    /// nearer than its nearest corner.
    float nearest;
};

/// Projects the box `center ± extent` through `view_proj` onto a target of
/// `width` × `height` pixels, with pixel `(0, 0)` at the top left — the
/// convention `ssr.slang`'s `pixel_of` uses.
ScreenBounds project_box(float4x4 view_proj, float3 center, float3 extent, float width, float height)
{
    ScreenBounds bounds;
    bounds.valid = true;
    bounds.min_x = PROJECTION_LIMIT;
    bounds.min_y = PROJECTION_LIMIT;
    bounds.max_x = -PROJECTION_LIMIT;
    bounds.max_y = -PROJECTION_LIMIT;
    bounds.nearest = 0.0;
    for (uint corner = 0; corner < 8; ++corner)
    {
        float3 side = float3(
            (corner & 1) != 0 ? 1.0 : -1.0,
            (corner & 2) != 0 ? 1.0 : -1.0,
            (corner & 4) != 0 ? 1.0 : -1.0);
        float4 clip = mul(view_proj, float4(center + side * extent, 1.0));
        // Written so a `NaN` fails: every comparison with one is false, and a
        // corner nothing can be said about makes the box invalid.
        if (!(clip.w > 0.0))
        {
            bounds.valid = false;
            return bounds;
        }
        float depth = clip.z / clip.w;
        float x = (clip.x / clip.w * 0.5 + 0.5) * width;
        float y = (0.5 - clip.y / clip.w * 0.5) * height;
        if (!(depth <= 1.0) || !(abs(x) < PROJECTION_LIMIT) || !(abs(y) < PROJECTION_LIMIT))
        {
            bounds.valid = false;
            return bounds;
        }
        bounds.min_x = min(bounds.min_x, x);
        bounds.min_y = min(bounds.min_y, y);
        bounds.max_x = max(bounds.max_x, x);
        bounds.max_y = max(bounds.max_y, y);
        bounds.nearest = max(bounds.nearest, depth);
    }
    return bounds;
}

/// Level `level` of the pyramid at an integer texel. No sampler and no filter,
/// for `hiz.slang`'s reason: `min` of integer fetches is the same number on
/// every rasteriser.
float pyramid_load(uint level, int2 texel)
{
    int3 at = int3(texel, 0);
    switch (level)
    {
    case 1:
        return pyramid_1.Load(at);
    case 2:
        return pyramid_2.Load(at);
    case 3:
        return pyramid_3.Load(at);
    case 4:
        return pyramid_4.Load(at);
    case 5:
        return pyramid_5.Load(at);
    case 6:
        return pyramid_6.Load(at);
    case 7:
        return pyramid_7.Load(at);
    default:
        return pyramid_8.Load(at);
    }
}

/// Whether the box is hidden behind what the pyramid holds, seen through
/// `view_proj` — **the occlusion test both phases run**, and
/// `crcbl_render::cull::occluded` is it in Rust.
///
/// Conservative in the only direction a cull may err: anything unprojectable,
/// off screen or touching a covering texel's depth is **not** hidden.
///
/// # Which texels
///
/// The rectangle is widened by `OCCLUSION_GUARD_TEXELS` and clamped to the
/// target, and the level is the finest from 1 up at which it spans at most two
/// texels on each axis — so at most four `Load`s — or the deepest level, whose
/// whole covered range is read. A texel of level `n` covers the `2^n` prepass
/// texels its index names, and the last texel of an odd axis covers the rest
/// (`hiz.slang`'s third tap), which is why an index is clamped rather than
/// dropped.
bool occluded(float4x4 view_proj, float3 center, float3 extent)
{
    if (cull.pyramid_levels == 0)
    {
        return false;
    }
    ScreenBounds bounds = project_box(
        view_proj, center, extent, float(cull.target_width), float(cull.target_height));
    if (!bounds.valid)
    {
        return false;
    }
    int width = int(cull.target_width);
    int height = int(cull.target_height);
    int x0 = int(floor(bounds.min_x)) - OCCLUSION_GUARD_TEXELS;
    int y0 = int(floor(bounds.min_y)) - OCCLUSION_GUARD_TEXELS;
    int x1 = int(floor(bounds.max_x)) + OCCLUSION_GUARD_TEXELS;
    int y1 = int(floor(bounds.max_y)) + OCCLUSION_GUARD_TEXELS;
    // Entirely off the target: nothing the pyramid holds says anything about
    // it, so it is not hidden.
    if (x1 < 0 || y1 < 0 || x0 >= width || y0 >= height)
    {
        return false;
    }
    uint left = uint(max(x0, 0));
    uint top = uint(max(y0, 0));
    uint right = uint(min(x1, width - 1));
    uint bottom = uint(min(y1, height - 1));

    uint level = 1;
    while (level < cull.pyramid_levels
        && ((right >> level) - (left >> level) > 1 || (bottom >> level) - (top >> level) > 1))
    {
        ++level;
    }
    uint level_width = max(cull.target_width >> level, 1u);
    uint level_height = max(cull.target_height >> level, 1u);
    uint first_x = min(left >> level, level_width - 1);
    uint first_y = min(top >> level, level_height - 1);
    uint last_x = min(right >> level, level_width - 1);
    uint last_y = min(bottom >> level, level_height - 1);

    // The nearest a depth can be, so the first texel read lowers it.
    float farthest = 1.0;
    for (uint y = first_y; y <= last_y; ++y)
    {
        for (uint x = first_x; x <= last_x; ++x)
        {
            farthest = min(farthest, pyramid_load(level, int2(int(x), int(y))));
        }
    }
    return bounds.nearest * (1.0 + OCCLUSION_DEPTH_SLACK) < farthest;
}

/// Invocations per workgroup, matching
/// `crcbl_shaders::cull::WORKGROUP_SIZE`. One instance each.
///
/// The frustum cull, and under `CULL_FACES` the point-light face tags.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= cull.instance_count)
    {
        return;
    }

    GpuInstance instance = instances[index];
    if (!admits(instance))
    {
        return;
    }
    GpuMesh mesh = meshes[instance.mesh];
    // A mesh id whose entry has been cleared — `MeshPool::free` writes all
    // zeroes — names no geometry, and the all-zero bounds are a degenerate box
    // at the origin that would otherwise be "visible" whenever the origin is on
    // screen. The index count is the field that says an entry is empty, so it
    // is the field this asks.
    if (mesh.index_count == 0)
    {
        return;
    }

    uint entry = index;
    if ((instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) == 0)
    {
        float3 center;
        float3 extent;
        world_box(instance, mesh, center, extent);
        if (!in_frustum(center, extent))
        {
            return;
        }
        // **Which faces of a point light's cube the box reaches.** The frustum
        // above is the light's whole box, so the survivors — and the LOD
        // hysteresis `draw_gen.slang` updates for them — are exactly what one
        // cull against the sphere kept. What this adds is a tag per face: its
        // four side planes are the pyramid a 90° face projects, and the six
        // pyramids tile every direction, so a box inside the light's box that
        // reaches no face's sides cannot put a texel in that face's tile.
        if ((cull.features & CULL_FACES) != 0)
        {
            for (uint face = 0; face < FACE_COUNT; ++face)
            {
                bool inside = true;
                for (uint plane = 0; plane < FACE_PLANES; ++plane)
                {
                    if (!reaches(cull.face_planes[face * FACE_PLANES + plane], center, extent))
                    {
                        inside = false;
                        break;
                    }
                }
                if (inside)
                {
                    entry |= 1u << (ENTRY_FACE_SHIFT + face);
                }
            }
        }
    }
    else if ((cull.features & CULL_FACES) != 0)
    {
        // A deforming instance's source bounds do not describe its vertices, so
        // it is kept whole — in every face, on the same terms.
        entry |= ((1u << FACE_COUNT) - 1) << ENTRY_FACE_SHIFT;
    }

    uint slot = visible_count[INSTANCE_SURVIVOR_WORD].add(1);
    if (slot < cull.capacity)
    {
        visible[slot] = entry;
    }
}

/// The first occlusion phase, and the camera's cull while
/// `crcbl_render::ForwardRenderer` has occlusion or small-feature culling on.
///
/// `computeMain`'s frustum cull, then the small-feature test against this
/// frame's projection, then the occlusion test against the **previous** frame's
/// pyramid reprojected through the previous frame's view-projection. A survivor
/// that fails the last is still a survivor — written with `ENTRY_OCCLUDED` so
/// `draw_gen.slang` selects its level, routes it and leaves it out of the early
/// scatter, and `lateMain` tests it again against this frame's depth. So this
/// phase decides only what is drawn *first*; nothing it gets wrong reaches a
/// pixel.
[shader("compute")]
[numthreads(64, 1, 1)]
void occlusionMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= cull.instance_count)
    {
        return;
    }

    GpuInstance instance = instances[index];
    if (!admits(instance))
    {
        return;
    }
    GpuMesh mesh = meshes[instance.mesh];
    // A mesh id whose entry has been cleared — `MeshPool::free` writes all
    // zeroes — names no geometry, and the all-zero bounds are a degenerate box
    // at the origin that would otherwise be "visible" whenever the origin is on
    // screen. The index count is the field that says an entry is empty, so it
    // is the field this asks.
    if (mesh.index_count == 0)
    {
        return;
    }

    uint entry = index;
    if ((instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) == 0)
    {
        float3 center;
        float3 extent;
        world_box(instance, mesh, center, extent);
        if (!in_frustum(center, extent))
        {
            return;
        }
        if ((cull.features & CULL_SMALL_FEATURE) != 0)
        {
            ScreenBounds bounds = project_box(cull.view_proj, center, extent,
                float(cull.target_width), float(cull.target_height));
            if (bounds.valid
                && max(bounds.max_x - bounds.min_x, bounds.max_y - bounds.min_y)
                    < cull.small_feature_pixels)
            {
                visible_count[SMALL_FEATURE_REJECT_WORD].add(1);
                return;
            }
        }
        if ((cull.features & CULL_OCCLUSION_HISTORY) != 0
            && occluded(cull.previous_view_proj, center, extent))
        {
            entry |= ENTRY_OCCLUDED;
            visible_count[OCCLUSION_EARLY_REJECT_WORD].add(1);
        }
    }

    uint slot = visible_count[INSTANCE_SURVIVOR_WORD].add(1);
    if (slot < cull.capacity)
    {
        visible[slot] = entry;
    }
}

/// The second occlusion phase: every survivor the first phase marked is tested
/// again, against the pyramid reduced from **this** frame's early depth through
/// this frame's view-projection.
///
/// That depth holds only what the first phase passed, which is less than the
/// frame will hold — so a box it hides is hidden by the finished frame too, and
/// this is the test the pixel-identity contract rests on. A box that passes is
/// tagged `ENTRY_RESCUED` in place for `draw_gen.slang`'s late scatter; one that
/// fails keeps its mark and is counted. Invocation `i` reads and writes entry
/// `i` alone.
[shader("compute")]
[numthreads(64, 1, 1)]
void lateMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= min(visible_count[INSTANCE_SURVIVOR_WORD].load(), cull.capacity))
    {
        return;
    }
    uint entry = visible[index];
    if ((entry & ENTRY_OCCLUDED) == 0)
    {
        return;
    }
    GpuInstance instance = instances[entry & ENTRY_INDEX_MASK];
    GpuMesh mesh = meshes[instance.mesh];
    float3 center;
    float3 extent;
    world_box(instance, mesh, center, extent);
    if (occluded(cull.view_proj, center, extent))
    {
        visible_count[OCCLUSION_LATE_REJECT_WORD].add(1);
        return;
    }
    visible[index] = entry | ENTRY_RESCUED;
}
