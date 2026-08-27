// crcbl-targets: spirv, msl, dxil

// `docs/plan/03-gpu-driven-rendering.md` §3.5's geometry path, in the forward
// pass: the same lit mesh `mesh.slang` draws, with a **mesh stage** in place of
// its vertex stage and a cluster in place of an index range.
//
// # Why this is a second file rather than a second entry point on `mesh.slang`
//
// **`mesh.slang` must keep emitting WGSL**, because `crcbl-webgpu` is the web
// backend and the forward pass is what it draws. WGSL has no mesh shaders at
// all — Slang refuses `SetMeshOutputCounts` for that target by name, which is
// what `mesh_shader.slang`'s header records — so a mesh entry point added to
// `mesh.slang` would make the whole file fail to compile for a target its own
// `// crcbl-targets:` line requires. A target list is per source, so the only
// way to have one source with WGSL and one without is two sources.
//
// That is the whole of the argument. Everything else about the two files is
// deliberately identical, and where it could drift it is not duplicated at all:
// **the fragment stage of the mesh pipeline is `mesh.slang`'s own
// `fragmentMain`**, named as a second module at pipeline creation. So the
// shading — Lambert, the GGX lobe, the material row, the page sample — exists
// once. A
// copy of it here would be the drift this arrangement exists to prevent, and it
// would be invisible: two shading models that agree today and differ after one
// of them is edited.
//
// What *is* restated here is the data layout: `struct MeshVertex`,
// `FrameUniforms`, `GpuInstance`, `GpuMesh` and `VertexOutput` are declared
// again, exactly as `mesh_shader.slang` restates `triangle.slang`'s `Vertex`.
// Slang has no shared header this build uses, and the layouts are pinned from
// the Rust side by `crcbl_shaders::mesh`'s offset tests — one source of bytes,
// two declarations of them.
//
// **`VertexOutput` is the one that has to match by *order*, not just by
// layout.** Its members take interface locations in declaration order, and the
// fragment stage reading them is the other file's, so a member inserted here
// and not there is a fragment stage reading the wrong varying. The pair is
// checked by rendering: `crcbl-vk`'s `the_mesh_shader_path_draws_the_indirect
// _path's_frame` compares this pipeline's frame against the raster pipeline's
// on one device, pixel for pixel.
//
// # A cluster is the unit of work, and the CPU never counts them per frame
//
// One workgroup draws one cluster of one instance:
//
//   * `SV_GroupID.x` is the cluster, within the mesh this bucket draws.
//   * `SV_GroupID.y` is the slot in the bucket's run of *surviving* instances,
//     which `cull.slang` filled and `draw_gen.slang` compacted.
//
// **Neither extent is the CPU's.** They are two of the three words
// `draw_gen.slang` writes for this bucket, and the seam's
// `draw_mesh_tasks_indirect` is what reads them: the x extent is the bucket's
// mesh's cluster count and the y extent is how many instances survived the
// instance cull. So a culled instance is not a workgroup that returns early —
// it is a workgroup that is never launched, which is the difference between
// culling that skips output and culling that skips work.
//
// The bounds check below survives that and is now exactly that: a bound. The
// extents make it true for every group the driver launches, and it stays
// because the extents arrive from a buffer rather than from a caller — a wrong
// argument structure would otherwise reach `visible_instances` past what
// `draw_gen.slang` wrote, and an out-of-range instance index is a hang rather
// than a wrong picture.
//
// # Per-cluster culling lives in the amplification stage, and only there
//
// §3.5's second bullet — frustum and normal-cone rejection per cluster — is
// `taskMain` below, which runs one workgroup per (cluster, instance slot) and
// dispatches the mesh stage for the pairs that survive. **The instance cull is
// unchanged and still runs first**: `cull.slang` decides which instances exist
// at all, `draw_gen.slang` compacts them, and this rejects clusters *within* a
// survivor.
//
// `Features::TASK_SHADER` is a capability separate from `MESH_SHADER`, so a
// device that has the mesh stage and not the amplification stage must keep
// drawing. That is what `meshMain` is: the un-amplified entry point, which
// culls nothing and emits every cluster of every surviving instance, exactly as
// it did before this stage existed. `amplifiedMeshMain` is the same body behind
// a payload — see `emit_cluster`, which is the whole of both.
//
// # The zero-base rule does not apply here, and that is worth saying
//
// `mesh.slang`'s header measures the four targets disagreeing about
// `SV_VertexID` and `SV_InstanceID`, and resolves it by passing zero for every
// draw's bases. A mesh shader has neither semantic: there is no vertex fetch to
// have a base vertex, and no instancing to have a base instance. Every index
// below comes from a buffer — the cluster's corner run, its vertex run, the
// mesh table's `base_vertex` — so there is no draw-call number in this file at
// all, and nothing for a target to disagree about.

/// The most vertices one cluster may reference, and the size of this stage's
/// vertex output array.
///
/// The same number as `crcbl_shaders::meshlet::MAX_CLUSTER_VERTICES`, which is
/// what the builder bounds a cluster by. That crate's
/// `the_shader_declares_the_same_cluster_bounds` reads this line and fails if
/// the two ever differ — a stage declaring fewer vertices than a cluster may
/// reference writes past its own array.
static const uint MAX_CLUSTER_VERTICES = 64;

/// The most triangles one cluster may hold, and the size of this stage's
/// primitive output array. Held in step with
/// `crcbl_shaders::meshlet::MAX_CLUSTER_TRIANGLES` by the same test.
static const uint MAX_CLUSTER_TRIANGLES = 124;

/// Threads per workgroup: one per vertex of a full cluster.
///
/// [`MAX_CLUSTER_VERTICES`] rather than a number of its own, so the vertex loop
/// below is one iteration for every cluster the builder can produce. The
/// triangle loop takes more than one, which is why both are loops rather than a
/// thread index used directly.
static const uint THREADS = MAX_CLUSTER_VERTICES;

/// One vertex, matching `struct MeshVertex` in `mesh.slang` and
/// `crcbl_shaders::mesh::MeshVertex`.
struct MeshVertex
{
    float4 position;
    float4 normal;
    float4 color;
    /// Base-colour texture coordinates in `xy`; `zw` unused.
    float4 uv;
};

/// Matching `static const uint SHADOW_CASCADES` in `mesh.slang`, because
/// `FrameUniforms` below sizes an array with it and a block this file thought
/// was shorter is one whose later members it reads at the wrong offsets.
static const uint SHADOW_CASCADES = 2;

/// Matching `static const uint SHADOW_LIGHT_TILES` in `mesh.slang`, for
/// `SHADOW_CASCADES`' reason exactly: `FrameUniforms` below sizes a second array
/// with it.
static const uint SHADOW_LIGHT_TILES = 14;

/// Matching `struct FrameUniforms` in `mesh.slang`.
///
/// The shadow members are declared and **not read**: nothing in this file
/// shades, and the shadow pass reaches its cascade matrix through `view_proj`
/// rather than through `shadow_view_proj`, exactly as `mesh.slang` says. They
/// are here because this block and that one must be the same bytes — `mesh.slang`'s
/// `fragmentMain` is this pipeline's fragment stage and reads them out of the
/// same buffer.
struct FrameUniforms
{
    float4x4 view_proj;
    float4 camera_position;
    float4 ambient;
    float4x4 shadow_view_proj[SHADOW_CASCADES];
    float4 cascade_far;
    float4 shadow_params;
    /// The froxel grid's extent: `x` across, `y` down, `z` depth slices, `w`
    /// padding.
    ///
    /// **Read by nothing here** — nothing in this file shades — and declared
    /// because this block is `mesh.slang`'s and the two declarations are held
    /// identical. The slot held `docs/plan/25-lod.md`'s selection numbers until
    /// the light list arrived, by which time no stage read those either.
    uint4 cluster_grid;
    /// World → light tile `i`'s shadow clip. **Read by nothing here**, on the
    /// cascades' terms exactly: this file does not shade, and a spot's or a point
    /// light's face's map is rendered through `view_proj` like every other pass.
    float4x4 light_view_proj[SHADOW_LIGHT_TILES];
    /// The irradiance grid's origin, reciprocal spacing and counts. **Read by
    /// nothing here**, on the cascades' terms exactly: this file does not shade,
    /// and the grid is `mesh.slang`'s `fragmentMain`'s alone.
    ///
    /// Declared all the same, because the two blocks are one buffer. The
    /// *binding* the rows arrive on needs no mirror — it is 23, past everything
    /// this file declares — but a member of the shared block is not a binding,
    /// and a block short of three rows here would be one whose earlier members
    /// this file still reads correctly and whose size the two stages disagree
    /// about.
    float4 probe_origin;
    float4 probe_inv_spacing;
    uint4 probe_counts;
    /// `docs/plan/25-lod.md`'s selection numbers, as `draw_gen.slang`'s
    /// `DrawGenParams::lod_params` carries them: `x` is how many pixels one unit
    /// of length subtends one unit from the eye, `y` is the budget an unexpanded
    /// group's projected error must exceed to expand, `z` is the budget an
    /// expanded one is held down to, and `w` is unread padding.
    ///
    /// **Read here and by nothing in `mesh.slang`**, which declares the member
    /// all the same because the two blocks are one buffer. The screen-error
    /// heatmap is what reads it: this stage projects a cluster's producing
    /// group's error itself, and the two budgets are what the ramp is anchored
    /// on. That is the same pair `draw_gen.slang` judged the cut against — the
    /// renderer writes one set of numbers into both blocks.
    ///
    /// **Last in the block**, on `probe_origin`'s terms exactly: no existing
    /// member's offset moves, so every golden blessed before this member existed
    /// still matches.
    float4 lod_params;
};

/// Matching `struct GpuLight` in `mesh.slang`.
///
/// Declared here and read by nothing, for `materials`' reason exactly: the
/// layout this pipeline is built with carries the light bindings because
/// `mesh.slang`'s fragment stage reads them, and Slang's Metal target numbers a
/// stage's resources by *declaration order* in the source. A file that skipped
/// them would put its own buffers at indices the backend binds something else
/// at.
struct GpuLight
{
    float4 position;
    float4 color;
    float4 direction;
    uint kind;
    float cos_inner;
    uint shadow_tile;
    uint pad1;
};

/// Matching `struct GpuProbe` in `mesh.slang`.
///
/// Declared here and read by nothing, on `GpuLight`'s terms exactly: the
/// binding below it is one this file *does* read, and Slang's Metal target
/// numbers a stage's buffers by declaration order — so a source that skipped the
/// probe table would put its own table one index below where the backend binds
/// it.
struct GpuProbe
{
    float4 sh_r;
    float4 sh_g;
    float4 sh_b;
};

/// Matching `struct GpuInstance` in `mesh.slang`.
struct GpuInstance
{
    float4x4 transform;
    uint mesh;
    uint material;
    uint sector;
    uint flags;
    /// The pool vertex this instance's indices are relative to, read **instead
    /// of** the mesh entry's when `INSTANCE_BASE_VERTEX_OVERRIDE` is set in
    /// `flags` and ignored when it is not. A deformed instance names its
    /// **source** mesh above and carries the base of the region a skinning
    /// dispatch wrote it into here, so the bucket, the level tables and the
    /// bounding box all still resolve through the mesh it was made from.
    uint base_vertex;
    /// Pads the record to a multiple of the transform's alignment, matching
    /// `mesh.slang` — where the four targets' disagreement about the tail is
    /// written out.
    uint pad0;
    uint pad1;
    uint pad2;
};

/// Matching `static const uint INSTANCE_BASE_VERTEX_OVERRIDE` in `mesh.slang`.
static const uint INSTANCE_BASE_VERTEX_OVERRIDE = 2;

/// Matching `struct GpuMesh` in `mesh.slang`.
struct GpuMesh
{
    uint base_vertex;
    uint base_index;
    uint index_count;
    float min_x;
    float min_y;
    float min_z;
    float max_x;
    float max_y;
    float max_z;
};

/// Matching `struct GpuMaterial` in `mesh.slang`. Declared and not read — see
/// the binding below.
struct GpuMaterial
{
    float4 base_color;
    uint base_color_texture;
    float metallic;
    float roughness;
    uint tiling;
    float tile_metres;
    float emissive_r;
    float emissive_g;
    float emissive_b;
};

/// One cluster, matching `crcbl_shaders::meshlet::Meshlet`.
///
/// Scalars rather than a `float3` for the bounds, for `GpuMesh`'s reason: under
/// `std430` a `float3` is 16-byte aligned, so a struct of scalars is the only
/// one whose stride is the sum of its members. `MESHLET_STRIDE` is 48 and this
/// is what makes it so.
struct Meshlet
{
    /// First entry of this cluster's run of `cluster_vertices`.
    uint vertex_offset;
    /// How many original vertex indices it references. At most
    /// [`MAX_CLUSTER_VERTICES`].
    uint vertex_count;
    /// First entry of its run of `cluster_corners`, **counted in corners** —
    /// not in the words they are packed into.
    uint triangle_offset;
    /// How many triangles it holds. At most [`MAX_CLUSTER_TRIANGLES`].
    uint triangle_count;
    /// Bounding sphere and normal cone, in the mesh's own space. Read by
    /// `cluster_survives`, which is the amplification stage's whole job.
    float center_x;
    float center_y;
    float center_z;
    float radius;
    float cone_axis_x;
    float cone_axis_y;
    float cone_axis_z;
    float cone_cutoff;
};

/// [`ClusterSelect::flags`] bit meaning the producing group is real. The same
/// number as `crcbl_shaders::cluster_select::ClusterSelect::HAS_PRODUCER`, held
/// in step with it by that module's
/// `the_shader_declares_the_same_selection_flags`.
static const uint HAS_PRODUCER = 1;

/// The same, for the containing group.
static const uint HAS_CONTAINER = 2;

/// The first `ClusterSelect::flags` bit carrying the DAG level this cluster was
/// decimated to. Bits 0 and 1 are the two flags above; everything over them is
/// the level, which the LOD tint reads so a cut spanning several levels of one
/// mesh can be seen rather than only asserted. The same number as
/// `crcbl_shaders::cluster_select::ClusterSelect::LEVEL_SHIFT`, held in step
/// with it by `the_shader_declares_the_same_selection_flags`.
static const uint LEVEL_SHIFT = 2;

/// What `FrameUniforms::ambient`'s `w` must reach for the LOD view.
///
/// The same lane the normals view rides in, one sentinel further out:
/// `crcbl_shaders::mesh::FrameUniforms::LOD_VIEW_ON` is the `2.0` the host
/// writes, and this threshold sits between it and the normals view's `1.0`. The
/// fragment stage tests this one **first**, or a `2.0` would be caught by the
/// normals threshold on its way past. `mesh.slang` declares the same constant
/// and `the_lod_view_threshold_lies_above_the_normals_view` holds the two files
/// in step.
static const float LOD_VIEW = 1.5;

/// What `FrameUniforms::ambient`'s `w` must reach for the **screen-error
/// heatmap**, the LOD tint's sibling overlay.
///
/// The same lane again, one sentinel further out:
/// `crcbl_shaders::mesh::FrameUniforms::HEATMAP_VIEW_ON` is the `3.0` the host
/// writes, and this threshold sits between it and `LOD_VIEW_ON`'s `2.0`. Tested
/// **before** `LOD_VIEW`, or a `3.0` would be caught by the tint's threshold on
/// its way past — the same ordering `LOD_VIEW` has against the normals view, and
/// `crcbl_shaders::mesh`'s `the_heatmap_view_threshold_lies_above_the_lod_view`
/// is what holds the three sentinels and the three thresholds interleaved.
///
/// `mesh.slang` needs no mirror of *this* constant and does mirror the sentinel:
/// its `fragmentMain` tests `>= LOD_VIEW` and passes the colour it was handed
/// through unshaded, which is what both overlays want, and its `vertexMain`
/// writes the same flat grey for both — the two indirect paths select a level
/// per instance, so they have neither a per-cluster level nor a per-cluster
/// error to draw.
static const float HEATMAP_VIEW = 2.5;

/// The heatmap ramp's coldest stop: a cluster whose producing group's error
/// projects to nothing this frame.
///
/// # Why these five stops, and why not a rainbow
///
/// The question a viewer brings to this overlay is "how close to the budget is
/// this, and where is the selection about to switch". A rainbow answers neither:
/// its hues have no order anyone can read off, so "hotter" becomes a legend
/// lookup per pixel. These five are **strictly increasing in luminance** —
/// `crcbl_shaders::mesh`'s `the_heatmap_ramp_climbs_in_luminance` reads them out
/// of this file and checks it — so the picture reads as a height field over the
/// error even in a screenshot, and even to a reader who cannot separate the
/// hues.
///
/// # Both budgets are landmarks on it, and that is what the breaks are for
///
/// `t` is the projected error over the **expand** budget, so `t = 1` is that
/// budget exactly — and it is the ceiling of the data on the amplified path,
/// because a cluster the descent kept is one whose producer did not expand,
/// which is `projected <= expand`. The ramp is anchored there rather than on the
/// hold budget for that reason: hold is where an *already* expanded group gives
/// up, and no cluster in the cut sits above it.
///
/// The ramp is discontinuous at both budgets on purpose. `t = hold/expand`
/// jumps from teal to amber and `t = 1` jumps from yellow to white, so each
/// budget draws itself as a contour line across the surface rather than being
/// somewhere in a gradient a viewer has to estimate. Luminance still climbs
/// across both jumps, so the discontinuities cost the ordering nothing.
///
/// Between the two is `docs/plan/25-lod.md`'s hysteresis band: a cluster shaded
/// amber-to-yellow is one whose producer would already have expanded had it
/// been expanded last frame, which is exactly "about to switch".
static const float3 HEAT_UNDER_LOW = float3(0.08, 0.10, 0.35);

/// The cool ramp's top, reached as the projected error approaches the hold
/// budget.
static const float3 HEAT_UNDER_HIGH = float3(0.10, 0.55, 0.60);

/// The hysteresis band's bottom — the **hold** budget, and the first of the two
/// contour lines.
static const float3 HEAT_BAND_LOW = float3(0.85, 0.45, 0.10);

/// The band's top, reached as the projected error approaches the expand budget.
static const float3 HEAT_BAND_HIGH = float3(0.97, 0.85, 0.20);

/// At or over the **expand** budget: the ramp's brightest stop, and the second
/// contour line.
///
/// Reachable in the cut only where the eye is inside a group's own sphere, which
/// is where `projected_error` answers [`INSIDE_THE_SPHERE`] — a cluster the
/// descent kept is one whose producer did not expand, and that is exactly
/// `projected <= expand`. It is the ordinary colour of the *coarse* half of a
/// frame drawn by `meshMain`, which culls nothing and selects nothing, so the
/// clusters whose producing groups are over the budget are drawn anyway.
static const float3 HEAT_OVER = float3(1.00, 1.00, 1.00);

/// The smallest divisor `heat_tint` will normalise by.
///
/// `crcbl_render::ForwardRenderer::set_lod_error_budget` takes any `float` — the
/// *flag* in front of it refuses a non-positive one, the setter does not — and an
/// orthographic camera's budget is negative infinity by design. So this is the
/// one line that keeps the division from producing an infinity or a NaN in a
/// colour, whatever a caller reaches it with.
static const float RAMP_FLOOR = 1.0e-6;

/// Where `projected` sits between the two budgets, as a colour.
///
/// See [`HEAT_UNDER_LOW`] for the ramp and for why it is this one. **Comparisons
/// and `lerp`, never an index into a table**: `lod_tint` below carries the
/// measurement that made that rule, and this function's input varies per cluster
/// inside one wave exactly as that one's does.
float3 heat_tint(float projected, float expand, float hold)
{
    float budget = max(expand, RAMP_FLOOR);
    float t = projected / budget;
    if (t >= 1.0)
    {
        return HEAT_OVER;
    }
    // Equal budgets put the break at the ramp's own top, so the band is empty
    // and the cool half covers the whole range — which is what a caller who
    // switched hysteresis off asked for.
    float band = clamp(hold / budget, 0.0, 1.0);
    if (t >= band)
    {
        return lerp(HEAT_BAND_LOW, HEAT_BAND_HIGH,
                    saturate((t - band) / max(1.0 - band, RAMP_FLOOR)));
    }
    return lerp(HEAT_UNDER_LOW, HEAT_UNDER_HIGH, saturate(t / max(band, RAMP_FLOOR)));
}

/// What `projected_error` answers for an eye inside the group's own sphere.
///
/// The largest finite `float`, standing in for the infinity
/// `crcbl_shaders::cluster_select::GroupCost::projected_error` returns there:
/// an infinity has no literal every target of this file spells the same way,
/// and every comparison the projection takes part in is against a budget a
/// caller set — a finite number, which this is above. So the two answer the
/// same decisions from the same inputs.
static const float INSIDE_THE_SPHERE = 3.4028235e38;

/// One group's simplification cost in **pixels**, from an eye at `eye`.
///
/// **`shaders/draw_gen.slang` declares this function verbatim**, and that file
/// is where the metric is documented in full. It is copied rather than shared
/// because Slang has no header this build uses, and it is copied *exactly* for
/// the reason two spellings of one metric are dangerous here: that pass judges
/// the cut with it and this one shades a cluster by it, so a drifted copy is an
/// overlay that disagrees with the picture it is drawn over while looking
/// entirely plausible. `crcbl_shaders::cluster_select`'s
/// `the_two_shaders_project_the_error_with_one_function` holds the two texts
/// equal.
float projected_error(float error, float3 center, float radius, float3 eye, float pixels_per_unit)
{
    float3 delta = eye - center;
    float separation = sqrt(delta.x * delta.x + delta.y * delta.y + delta.z * delta.z);
    float distance = separation - radius;
    if (distance <= 0.0)
    {
        return INSIDE_THE_SPHERE;
    }
    return error * pixels_per_unit / distance;
}

/// The largest factor by which `basis` can grow a vector's length, bounded from
/// above — what a length in the mesh's own space has to be multiplied by to be a
/// length in the space `basis` maps it into: a bounding *sphere*'s radius, so
/// that it still bounds the transformed cluster, and a simplification error, so
/// that it is still the distance the surface may have moved.
///
/// That factor is `basis`' largest singular value, and this bounds it rather
/// than computing it: `sigma_max^2 = lambda_max(B^T B)`, and a symmetric
/// matrix's spectral radius is at most any induced norm of it, so the largest
/// absolute row sum of `B^T B` is an upper bound on `sigma_max^2`. Bounding is
/// the safe direction for a cull, and the bound is **exact** whenever `basis`'
/// columns are orthogonal — every rotation-then-scale, which is every transform
/// this engine builds — because `B^T B` is then diagonal and its rows are the
/// columns' squared lengths.
///
/// The alternative, `max` over the columns' lengths, is exact on the same
/// matrices and *under*-estimates on a sheared one, which is the direction that
/// drops geometry. This form needs no contract about what a caller may pass.
///
/// `1.0` for a rotation or a translation, so nothing that was already correct
/// moves. `crcbl_render::cull`'s `max_stretch` is this function in Rust and
/// `cluster_survives_cull` is the oracle a test compares this whole rule to.
///
/// **Read by both halves of this file and by `shaders/draw_gen.slang`**: the
/// cull carries a cluster's sphere across a transform with it, `cluster_heat`
/// below carries a group's radius and error, and the selection that overlay is
/// drawn over does the same in the other file. `crcbl_shaders::cluster_select`'s
/// `the_two_shaders_bound_a_stretched_length_with_one_function` is what holds
/// the two shader texts equal.
float max_stretch(float3x3 basis)
{
    float3x3 gram = mul(transpose(basis), basis);
    float bound = 0.0;
    for (uint row = 0; row < 3; ++row)
    {
        bound = max(bound, abs(gram[row][0]) + abs(gram[row][1]) + abs(gram[row][2]));
    }
    return sqrt(bound);
}

/// A distinct colour per DAG level, for the LOD view.
///
/// **A `switch` rather than an array index**, and that is not style. `level`
/// varies per cluster inside one wave, and this repository has already paid for
/// a divergent index into constant storage once: RADV emitted
/// `v_readfirstlane_b32` on the offset and every lane read element zero, so a
/// tint that should have shown a mosaic would have shown one flat colour — the
/// exact failure this view exists to make visible. See
/// `shaders/push_constant_probe.slang`, which carries the measurement.
///
/// Eight hues, cycling: a DAG deeper than eight repeats rather than fading to
/// something unreadable. quarry's face builds twelve levels, so 8 to 11 repeat 0
/// to 3, which still reads as "these clusters are not the ones beside them".
float3 lod_tint(uint level)
{
    switch (level % 8u)
    {
        case 0u: return float3(0.90, 0.25, 0.25);
        case 1u: return float3(0.95, 0.60, 0.20);
        case 2u: return float3(0.90, 0.90, 0.25);
        case 3u: return float3(0.30, 0.85, 0.35);
        case 4u: return float3(0.25, 0.80, 0.85);
        case 5u: return float3(0.30, 0.45, 0.95);
        case 6u: return float3(0.65, 0.35, 0.90);
        default: return float3(0.95, 0.45, 0.80);
    }
}

/// One cluster's half of `docs/plan/25-lod.md`'s DAG descent, matching
/// `crcbl_shaders::cluster_select::ClusterSelect`.
///
/// **Both halves name a group, never this cluster**, and that is the whole
/// correctness argument: every cluster a group touches carries the *same index*,
/// so every one of them reads the same word of `group_state` and the group's
/// parents are drawn all together or not at all. A per-cluster distance term
/// gives two clusters of one group two answers to one question and tears the
/// mesh along a boundary that group locked — invisible until a camera sits where
/// the two answers differ. `crcbl_scene::cluster_dag`'s module docs carry the
/// argument in full.
///
/// The judging itself is `draw_gen.slang`'s, once per (instance, group); this
/// stage reads the answer. That is what hysteresis needed — an answer carried
/// between frames has to be stored per group, and a group has to have a name to
/// be stored under.
struct ClusterSelect
{
    /// Which of the two groups this cluster has: `HAS_PRODUCER`, `HAS_CONTAINER`.
    uint flags;
    /// Added to `GpuMesh::base_vertex` when this cluster's vertex run is
    /// resolved — which level of the DAG its geometry lives in. Zero for level 0
    /// and for every mesh with no DAG.
    uint vertex_base;
    /// The group whose simplification produced this cluster, as an index into
    /// the shared group array — the numbering `group_state` is keyed by. Read
    /// only under `HAS_PRODUCER`.
    uint producer_group;
    /// The group this cluster is a child of — the one that would simplify it
    /// away — in the same numbering. Read only under `HAS_CONTAINER`.
    uint container_group;
};

/// One indexed indirect draw's arguments, matching `draw_gen.slang`'s.
///
/// Read here as **data** rather than executed as a draw: `instance_count` is
/// how many instances survived culling into this bucket. The same number sizes
/// this dispatch's y extent, out of `draw_gen.slang`'s separate mesh-argument
/// structure — so what `group_is_live` does with it below is a range check
/// rather than the filter it once was.
struct DrawIndexedArgs
{
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};

/// Planes in a frustum, as `cull.slang` declares it.
static const uint PLANE_COUNT = 6;

/// The frustum this frame culls against, matching `struct CullParams` in
/// `cull.slang` and `crcbl_shaders::cull::Params`.
///
/// **The same buffer the instance cull reads**, bound a second time rather than
/// copied: the two rejections are one camera's, and a frustum that reached the
/// two stages differently would keep an instance and drop every cluster of it.
/// `crcbl_shaders::cull`'s `the_shared_structs_are_declared_identically_in_
/// every_shader` is what holds this declaration to that one.
struct CullParams
{
    /// The camera frustum's six half-spaces, `xyz` the (unnormalized) outward
    /// plane normal and `w` its offset. A point is inside the plane when
    /// `dot(xyz, p) + w >= 0`.
    float4 planes[PLANE_COUNT];
    /// Instances the cull pass tested. Read by nothing here — the mesh path's
    /// instance bound is its own dispatch extent.
    uint instance_count;
    /// Elements the visible list can hold. Read by nothing here either.
    uint capacity;
};

/// Which element of `cull_stats` counts surviving clusters.
///
/// Element 0 is `cull.slang`'s surviving *instances*, so this is the one word
/// this shader owns. The same number as
/// `crcbl_shaders::cull::CLUSTER_SURVIVOR_WORD`, held in step with it by that
/// module's `the_shaders_index_the_culling_stats_at_the_words_this_crate_says`.
static const uint CLUSTER_SURVIVOR_WORD = 1;

/// Which element of `cull_stats` counts the clusters the **frustum** rejected.
///
/// Element 2 is `light_cluster.slang`'s dropped light assignments, so the two
/// rejection words start after it. The same numbers as
/// `crcbl_shaders::cull::CLUSTER_FRUSTUM_REJECT_WORD` and
/// `CLUSTER_CONE_REJECT_WORD`, held in step with them by that module's
/// `the_shaders_index_the_culling_stats_at_the_words_this_crate_says`.
static const uint CLUSTER_FRUSTUM_REJECT_WORD = 3;

/// Which element of `cull_stats` counts the clusters the **normal cone**
/// rejected — the ones the frustum kept and that face away.
static const uint CLUSTER_CONE_REJECT_WORD = 4;

/// What `taskMain` hands `amplifiedMeshMain`: which pair of the dispatch
/// survived.
///
/// The two group indices rather than a resolved cluster record, because a
/// payload is per surviving group and the mesh stage can re-read what it needs
/// from the same buffers — where a copied record would be a second layout to
/// keep in step with `struct Meshlet` above.
struct ClusterPayload
{
    /// The cluster's index within this bucket's mesh's run, which is
    /// `SV_GroupID.x` of the task group that kept it.
    uint cluster;
    /// The slot in this bucket's run of surviving instances, which is that
    /// group's `SV_GroupID.y`.
    uint instance;
};

/// Where this bucket's run of visible instances starts, and which clusters its
/// mesh owns. Matching `crcbl_shaders::meshlet::ClusterDrawConstants`.
///
/// A block of its own rather than `mesh.slang`'s `DrawConstants`, because it
/// says three more things and the raster path would carry them unread. One
/// block per bucket, reached through a dynamic offset — the same mechanism the
/// raster path uses for its own.
struct ClusterDrawConstants
{
    /// This bucket's first slot in `visible_instances`, added to the group's
    /// instance slot.
    uint base;
    /// This bucket's mesh's first cluster in `clusters`.
    uint cluster_base;
    /// How many clusters that mesh has. The dispatch's x extent is this number
    /// too, reached through `draw_gen.slang`'s mesh-argument structure, and the
    /// bound is checked again here so an argument structure naming a larger one
    /// cannot draw another mesh's clusters.
    uint cluster_count;
    /// Which bucket this is, which is the element of `draw_args` holding the
    /// instance count culling produced.
    uint bucket;
    /// How many groups one instance's run of `group_state` holds, which is the
    /// stride between two instances in it.
    ///
    /// A frame-wide number in a per-bucket block, and the same value in every
    /// one of them: this is the only uniform block the amplification stage
    /// reads, and `draw_gen.slang` — which writes the state — carries the same
    /// number in its own params block.
    uint group_stride;
    /// Where the `LevelGroup` records start in `tables`, in words — the same
    /// offset `draw_gen.slang`'s `DrawGenParams::level_groups_at` carries,
    /// because `level_group_at` below reads the region that pass wrote its
    /// decisions from.
    ///
    /// A frame-wide number in a per-bucket block for `group_stride`'s reason
    /// exactly, and fixed for the life of the renderer besides: the host packs
    /// the regions when a mesh becomes resident, not per frame.
    uint level_groups_at;
};

/// One group of a mesh's cluster DAG, matching `struct LevelGroup` in
/// `draw_gen.slang` and `crcbl_shaders::level_select::LevelGroup`.
///
/// Six scalars and not a `float3` in sight, for `struct Meshlet`'s reason above:
/// under `std430` a `float3` is 16-byte aligned and would put four bytes of
/// padding in front of itself.
///
/// **Read here for the screen-error heatmap alone.** The *cut* is
/// `draw_gen.slang`'s decision and this stage reads its answer out of
/// `group_state`; what the overlay wants is the number that decision was made
/// on, which is this record projected from the eye.
struct LevelGroup
{
    /// The level whose clusters this group grouped. Read by nothing here — the
    /// level a cluster was decimated to reaches the LOD tint through
    /// `ClusterSelect::flags` instead — and declared because the record is six
    /// words and a reader that skipped one would read every field after it at
    /// the wrong offset.
    uint level;
    /// How far this group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    float error;
    /// Centre of the sphere the error is projected from, in the mesh's own
    /// space.
    float center_x;
    float center_y;
    float center_z;
    /// Distance from that centre to the furthest point the sphere contains.
    float radius;
};

/// Words one `LevelGroup` occupies in `tables`, matching `draw_gen.slang`'s
/// constant of the same name and
/// `crcbl_shaders::level_select::LEVEL_GROUP_STRIDE` divided by four.
static const uint LEVEL_GROUP_WORDS = 6;

/// Which word of a group's record holds which field.
///
/// Named rather than written as literals in `level_group_at` below, for
/// `CLUSTER_SURVIVOR_WORD`'s reason: six scalars in a row permute silently — a
/// radius read as an error shades a cluster the wrong colour rather than
/// crashing — and `crcbl_shaders::level_select`'s
/// `the_shader_reads_the_selection_records_at_the_words_this_module_encodes` is
/// what holds these against the bytes the host writes.
static const uint LEVEL_GROUP_LEVEL = 0;
static const uint LEVEL_GROUP_ERROR = 1;
static const uint LEVEL_GROUP_CENTER_X = 2;
static const uint LEVEL_GROUP_CENTER_Y = 3;
static const uint LEVEL_GROUP_CENTER_Z = 4;
static const uint LEVEL_GROUP_RADIUS = 5;

[[vk::binding(0, 0)]]
ConstantBuffer<FrameUniforms> frame;

[[vk::binding(1, 0)]]
StructuredBuffer<MeshVertex> vertices;

[[vk::binding(2, 0)]]
StructuredBuffer<GpuInstance> instances;

[[vk::binding(3, 0)]]
ConstantBuffer<ClusterDrawConstants> draw;

[[vk::binding(4, 0)]]
StructuredBuffer<GpuMesh> meshes;

[[vk::binding(5, 0)]]
StructuredBuffer<uint> visible_instances;

/// The material table. **Declared here and read by nothing**, and the reason is
/// Metal: Slang's Metal target ignores `[[vk::binding]]` and hands each
/// resource the next index in its stage's flat table, in declaration order,
/// while `crcbl-mtl` computes an index by counting same-table descriptors in
/// the *layout*. The layout this pipeline is built with carries bindings 6, 7
/// and 8 because `mesh.slang`'s fragment stage reads them — so a source that
/// skipped them would put `clusters` at buffer index 6 in the MSL and have the
/// backend bind it at 9. `crcbl_shaders::declaration_order` is the lint that
/// checks the ordering half of this rule; the completeness half is this
/// comment.
[[vk::binding(6, 0)]]
StructuredBuffer<GpuMaterial> materials;

/// The base-colour page, on `materials`' terms exactly: the fragment stage
/// samples it, this stage does not, and the declaration is what keeps the
/// Metal tables aligned with the layout.
[[vk::binding(7, 0)]]
Texture2DArray<float4> base_color_textures;

/// The page's sampler, for the same reason.
[[vk::binding(8, 0)]]
SamplerState base_color_sampler;

/// Every resident mesh's clusters, back to back, in mesh order. A bucket's
/// mesh names its own run with `draw.cluster_base` and `draw.cluster_count`.
[[vk::binding(9, 0)]]
StructuredBuffer<Meshlet> clusters;

/// Original vertex indices, one run per cluster. **Mesh-relative**, like the
/// index buffer the raster path reads: `GpuMesh::base_vertex` is added here.
[[vk::binding(10, 0)]]
StructuredBuffer<uint> cluster_vertices;

/// Triangle corners, packed four to a word, least significant byte first.
///
/// A storage buffer has no byte addressing, so corner `n` is byte `n & 3` of
/// word `n >> 2` — which is exactly what a little-endian load of the host's
/// `u8` array reads. `crcbl_shaders::meshlet::corner_words` is the other half
/// of that statement. Packing by word rather than padding each cluster's run
/// is what lets a cluster closed early by the vertex bound end on any corner.
[[vk::binding(11, 0)]]
StructuredBuffer<uint> cluster_corners;

/// The per-bucket indirect arguments `draw_gen.slang` wrote. Read for
/// `instance_count` and nothing else.
[[vk::binding(12, 0)]]
StructuredBuffer<DrawIndexedArgs> draw_args;

/// This frame's frustum — the amplification stage's alone.
///
/// **Bound only when there is an amplification stage**: a device without
/// `Features::TASK_SHADER` builds a pipeline whose layout stops at binding 12,
/// and `meshMain` reads neither this nor `cull_stats` below.
[[vk::binding(13, 0)]]
ConstantBuffer<CullParams> cull;

/// The frame's culling statistics, which this stage adds each cluster it tested
/// into: `CLUSTER_SURVIVOR_WORD`, `CLUSTER_FRUSTUM_REJECT_WORD` or
/// `CLUSTER_CONE_REJECT_WORD`, one of the three and never two.
///
/// The buffer `cull.slang` counts surviving instances into, one element along —
/// topic 03 §3.6's delayed readback ring is this one buffer, and a second
/// counter of its own would be a second readback. `Atomic<uint>` rather than
/// `InterlockedAdd` for `cull.slang`'s reason: spelled as a type it lowers on
/// every target.
[[vk::binding(14, 0)]]
RWStructuredBuffer<Atomic<uint> > cull_stats;

/// Every resident cluster's half of the DAG descent, parallel to `clusters`.
///
/// **Bound whenever `clusters` is**, because `vertex_base` is geometry rather
/// than selection: a DAG's levels are separate vertex ranges and both mesh entry
/// points have to resolve the right one. Bindings 15 and 16 are `mesh.slang`'s
/// shadow atlas and its sampler — declared by the fragment stage of this very
/// pipeline, so the numbers are taken and this is 17.
[[vk::binding(17, 0)]]
StructuredBuffer<ClusterSelect> cluster_select;

/// The cut this frame chose, one word per resident cluster: `1` where the
/// descent selected the cluster and `0` where it did not.
///
/// `docs/plan/25-lod.md`'s observable, and the only thing that can show
/// per-cluster selection happening — a frame whose every cluster came from one
/// level looks entirely plausible and matches any golden blessed from it. Read
/// back by `crcbl-vk`'s
/// `the_gpu_descends_the_dag_to_the_cut_the_host_rule_says` and by nothing in
/// the frame.
///
/// **Instance slot zero writes it and no other slot does**, so several instances
/// of one mesh do not race for one entry and no atomic is needed: what it holds
/// is the cut chosen for a bucket's first surviving instance. A bucket whose
/// instances were all culled launches no task group and leaves the entry at
/// whatever the last frame that drew it wrote, which is why a reader compares
/// two cameras rather than trusting one.
[[vk::binding(18, 0)]]
RWStructuredBuffer<uint> cluster_selection;

/// `docs/plan/25-lod.md`'s hysteresis state, as `draw_gen.slang` left it this
/// frame: one word per (instance, group), `1` where that group is expanded.
///
/// **Read here and written nowhere**, which is what makes a stage running
/// thousands of task groups per instance able to use a per-group decision at
/// all: one invocation of the draw-argument pass owns an instance's whole run
/// and every task group of that instance reads it. The graph orders the two —
/// that pass writes, this one reads, and the barrier between them is the same
/// one `visible_instances` already needs.
///
/// Bound whenever `cluster_select` is, because the two are one mechanism: the
/// record names a group and this says what that group decided.
[[vk::binding(19, 0)]]
StructuredBuffer<uint> group_state;

/// The frame's light list, on `materials`' terms exactly: the fragment stage
/// reads it, this file does not, and the declaration is what keeps the Metal
/// argument table aligned with the layout.
[[vk::binding(20, 0)]]
StructuredBuffer<GpuLight> lights;

/// The froxel grid, for the same reason.
[[vk::binding(21, 0)]]
StructuredBuffer<uint> cluster_lights;

/// The irradiance grid's rows, for the same reason and one more of its own.
///
/// **Binding 22 is `mesh.slang`'s occlusion channel and 23 is its probe table**,
/// both declared by the fragment stage of this very pipeline. 22 is a *texture*
/// and this file declares one texture whatever happens after it, so it needs no
/// mirror; 23 is a buffer, and the buffer below it is this file's. Skipping it
/// would put `tables` one index below where `crcbl-mtl` binds it — that backend
/// counts the same-table entries of the layout, which carries 23 on every path.
[[vk::binding(23, 0)]]
StructuredBuffer<GpuProbe> probes;

/// Every host-written table `draw_gen.slang` reads, as one flat array of words —
/// the same buffer and the same regions, described there in full.
///
/// **Read here for one region only**: the `LevelGroup` records at
/// `draw.level_groups_at`, through `level_group_at` below. The screen-error
/// heatmap is the only reader, and what it wants is the group cost the cut was
/// judged on rather than the cut itself, which arrives through `group_state`.
///
/// **Bound wherever `clusters` is**, so both mesh entry points can read it: a
/// device with no amplification stage still draws through `emit_cluster`, and a
/// binding present for one entry point and absent for the other is a descriptor
/// read out of an empty slot.
[[vk::binding(24, 0)]]
StructuredBuffer<uint> tables;

/// What the mesh stage emits and `mesh.slang`'s `fragmentMain` consumes.
///
/// **Member order is the interface**, not just the layout: locations are
/// assigned in declaration order and the consumer is the other file's fragment
/// stage. See the header.
struct VertexOutput
{
    float4 position : SV_Position;
    float3 world_position : POSITION0;
    float3 world_normal : NORMAL0;
    float4 color : COLOR0;
    /// This primitive's material row. `nointerpolation` for the reason
    /// `mesh.slang` gives at length: an index is not a quantity, and
    /// interpolating one produces a fragment reading a row no vertex named.
    nointerpolation uint material : TEXCOORD0;
    float2 uv : TEXCOORD1;
};

/// Corner `corner` of the packed corner array.
uint corner_at(uint corner)
{
    uint word = cluster_corners[corner >> 2];
    return (word >> ((corner & 3) * 8)) & 0xffu;
}

/// Group `group` of the concatenated DAG groups, its five floats read back
/// through `asfloat`.
///
/// `draw_gen.slang`'s accessor of the same name, over the same region of the
/// same buffer — the only difference is where the region's offset is carried,
/// which is that pass's params block there and this pass's per-bucket block
/// here. The bit pattern the host wrote is the bit pattern a float load would
/// have produced, so this is a reinterpretation and not a conversion.
LevelGroup level_group_at(uint group)
{
    uint at = draw.level_groups_at + group * LEVEL_GROUP_WORDS;
    LevelGroup record;
    record.level = tables[at + LEVEL_GROUP_LEVEL];
    record.error = asfloat(tables[at + LEVEL_GROUP_ERROR]);
    record.center_x = asfloat(tables[at + LEVEL_GROUP_CENTER_X]);
    record.center_y = asfloat(tables[at + LEVEL_GROUP_CENTER_Y]);
    record.center_z = asfloat(tables[at + LEVEL_GROUP_CENTER_Z]);
    record.radius = asfloat(tables[at + LEVEL_GROUP_RADIUS]);
    return record;
}

/// The screen-error heatmap's colour for one cluster of an instance at
/// `transform`.
///
/// **The producing group's projected error, not the containing group's**, and
/// that is the choice the overlay is about. A cluster is in the cut while
/// `!E(producer) && E(container)`, so the producer is the group that would
/// *refine* this patch next — the one whose error the selection is holding just
/// under the budget, and the one whose crossing replaces this cluster with finer
/// ones. The container's error is already over the budget by construction and
/// says how far past the switch the coarser half is, which is a different
/// question and one the tint answers better.
///
/// **A cluster with no producer costs exactly zero**, and that is a fact rather
/// than a fallback: nothing simplified it, so it is the original surface and its
/// simplification cannot have moved anything. The ramp's floor is the honest
/// colour for it. The group is not read at all in that case — an absent
/// producer's index is zero, which is in range for a mesh that has a hierarchy
/// and is not for one that has none.
///
/// **In world space, exactly as `draw_gen.slang`'s `select_level` is**: the
/// sphere's centre goes through the instance's transform, its radius and the
/// group's error through [`max_stretch`] of that transform's 3x3, and the eye
/// stays where it is. The scaling is not an embellishment here — it is what
/// keeps this tint a picture *of* the cut. The selector judges a scaled
/// instance's groups at the stretched size, so an overlay that projected the
/// mesh-space lengths would shade the frame by a number nothing in the selection
/// uses, which is the one way a heatmap can be wrong while looking entirely
/// plausible.
float3 cluster_heat(uint cluster_index, float4x4 transform)
{
    ClusterSelect select = cluster_select[cluster_index];
    float projected = 0.0;
    if ((select.flags & HAS_PRODUCER) != 0)
    {
        LevelGroup group = level_group_at(select.producer_group);
        float stretch = max_stretch((float3x3)transform);
        float3 center = mul(transform,
                            float4(group.center_x, group.center_y, group.center_z, 1.0)).xyz;
        projected = projected_error(group.error * stretch, center, group.radius * stretch,
                                    frame.camera_position.xyz, frame.lod_params.x);
    }
    return heat_tint(projected, frame.lod_params.y, frame.lod_params.z);
}

/// How many workgroups a group of the dispatch is worth: `1` while both its
/// indices are inside what the frame actually holds, and `0` past either.
///
/// **A bound rather than a filter**, since the dispatch extents became the same
/// two numbers: every group the driver launches is inside them. It is kept
/// because those extents are read out of a buffer — see the header — so this is
/// the range check on the only untrusted input either stage has, and what it
/// guards is `visible_instances` being indexed past the run `draw_gen.slang`
/// filled.
///
/// **Arithmetic rather than a comparison**, because both entry points below put
/// the result in front of a call the DXIL validator wants unbranched:
/// `SetMeshOutputCounts` refuses to be reached through a branch on dxc 1.9, and
/// Slang lowers every `?:` into an `if`/`else` pair. `min(1, max(a, b) - b)` is
/// `1` exactly when `a > b`, and the `max` is what stops the unsigned
/// subtraction wrapping.
uint group_is_live(uint3 group)
{
    uint drawn = draw_args[draw.bucket].instance_count;
    return min(1u, max(drawn, group.y) - group.y)
         * min(1u, max(draw.cluster_count, group.x) - group.x);
}

/// How far a basis may be from carrying every angle across unchanged, relative
/// to its own scale, before the cone test below stops trusting it.
///
/// The same number as `crcbl_shaders::meshlet::ClusterBounds::SIMILARITY_TOLERANCE`,
/// held in step with it by that module's
/// `the_shader_declares_the_same_similarity_tolerance`, and measured there.
static const float SIMILARITY_TOLERANCE = 1e-4;

/// Whether `basis` carries every angle across unchanged — a rotation, a uniform
/// scale, a reflection, or those composed.
///
/// **What the cone test needs and the sphere does not.** A cluster's cone is a
/// set of *normals*, and a transform that stretches one axis moves a normal one
/// way and the surface it belongs to the other: the axis this function's caller
/// builds with the 3x3 is then not the cone's axis, and the cone's half-angle is
/// not its own either. Both would have to move together, and the widened angle
/// has no bound this shader could state and defend — so a cluster of an instance
/// that scales unequally is **kept**, and the frustum is what culls it. Fewer
/// clusters rejected, never a visible one dropped.
///
/// `transpose(B) * B` is `s * s * I` exactly when `B` is `s` times an orthogonal
/// matrix, which is the definition being tested: every column the same length,
/// no two of them oblique. Read relative to the largest diagonal, so it holds at
/// any scale. `crcbl_render::cull::preserves_angles` is the same rule in Rust
/// and the oracle a test compares this whole cull to.
///
/// A degenerate basis — a zero scale, which draws nothing — answers `false`, so
/// its clusters are kept rather than culled through a matrix with no angles left
/// in it.
bool preserves_angles(float3x3 basis)
{
    float3x3 gram = mul(transpose(basis), basis);
    float largest = max(gram[0][0], max(gram[1][1], gram[2][2]));
    if (largest <= 0.0)
    {
        return false;
    }
    float slack = SIMILARITY_TOLERANCE * largest;
    return abs(gram[0][1]) <= slack && abs(gram[0][2]) <= slack && abs(gram[1][2]) <= slack
           && largest - gram[0][0] <= slack && largest - gram[1][1] <= slack
           && largest - gram[2][2] <= slack;
}

/// What `cluster_survives` answered: kept, or which of its two tests refused.
///
/// Three named values rather than a bool, because the counters downstream are
/// the whole point — a survivor count on its own cannot tell a frustum doing all
/// of the work from a normal cone doing all of it. The caller turns one of these
/// into one word of `cull_stats`.
///
/// `CLUSTER_KEPT` is zero so the arithmetic at the call site stays what it was:
/// the survival flag is `verdict == CLUSTER_KEPT`, and nothing branches in front
/// of `DispatchMesh`.
static const uint CLUSTER_KEPT = 0;
/// The bounding sphere is entirely outside one of the frustum's six planes.
static const uint CLUSTER_REJECTED_BY_FRUSTUM = 1;
/// Every triangle of the cluster faces away from the camera. Reached only by a
/// cluster the frustum kept — the two tests are asked in that order.
static const uint CLUSTER_REJECTED_BY_CONE = 2;

/// Whether a cluster of an instance may contribute a pixel, and which test said
/// so: `CLUSTER_KEPT`, `CLUSTER_REJECTED_BY_FRUSTUM` or
/// `CLUSTER_REJECTED_BY_CONE`.
///
/// **The cull is in world space**, which is where the frustum already is: the
/// planes are `cull.slang`'s own for this frame, and `frame.camera_position` is
/// world-space too. The sphere is carried there by the **whole** transform: the
/// centre by the matrix, the radius by `max_stretch` above, and the cone axis by
/// the 3x3 and then made a unit vector again.
///
/// **The cone half is skipped entirely where the transform does not preserve
/// angles**, which is the one thing the whole transform cannot carry a cone
/// across: see `preserves_angles`. The sphere still culls there.
///
/// The radius used to be carried across unchanged, on the grounds that
/// `GpuInstance::transform` is rigid. The engine's own scenes are not —
/// `crcbl::screenshot`'s trough is the open box scaled by `6 x 2 x 1.6` — and an
/// instance-sized sphere standing in for a scene-sized one rejects clusters that
/// fill half the frame, which is geometry silently missing from the picture.
///
/// # Frustum
///
/// The ordinary sphere-against-half-space test. The planes are **not**
/// normalized and must not be — `cull.slang`'s header says why, and the far
/// plane of an infinite reversed-Z projection has a zero normal — so the
/// radius is scaled by the plane normal's length rather than the plane by its
/// reciprocal. A zero normal makes the term zero and leaves `w >= 0`, which is
/// the degenerate plane rejecting nothing, exactly as it should.
///
/// # Normal cone
///
/// With `a = acos(cone_cutoff)` the cone's half angle, `d = center - camera`
/// and `r` the radius, every triangle of the cluster faces away when
///
/// ```text
/// cone_cutoff > 0 && dot(cone_axis, d) > sqrt(1 - cone_cutoff^2) * length(d) + r
/// ```
///
/// **The `+ r` is the whole of what makes this safe.** Without it the test
/// treats every triangle as sharing the centre's view direction, which is right
/// only for a cluster of zero radius: a cluster with a real radius close to the
/// camera can then hold a front-facing triangle and still be rejected, which is
/// dropped geometry. `crcbl_shaders::meshlet::ClusterBounds::cone_cutoff`
/// carries the derivation, and `crcbl_render::cull::cluster_survives_cull` is
/// the same rule in Rust with the cases that pin it.
///
/// The `cone_cutoff > 0` half is not redundant beside the radius term:
/// `sqrt(1 - cutoff^2)` is even in `cutoff`, so it cannot tell a narrow cone
/// from a cone wider than a hemisphere, and the latter holds a front-facing
/// normal for every possible view.
uint cluster_survives(Meshlet cluster, float4x4 transform)
{
    float3x3 basis = (float3x3)transform;
    float3 center = mul(transform, float4(cluster.center_x, cluster.center_y, cluster.center_z, 1.0)).xyz;
    float radius = cluster.radius * max_stretch(basis);

    for (uint plane = 0; plane < PLANE_COUNT; ++plane)
    {
        float4 half_space = cull.planes[plane];
        if (dot(half_space.xyz, center) + half_space.w < -radius * length(half_space.xyz))
        {
            return CLUSTER_REJECTED_BY_FRUSTUM;
        }
    }

    // A **unit** axis, so the two sides of the inequality below are in the same
    // units: the left is a length along the axis and the right is a length in
    // world space. Left as the raw product, a transform that scales by `s`
    // multiplies one side and not the other, which is a cone test answering
    // differently for the same shape at two sizes. `normalize` of a zero vector
    // is a NaN, so the length is taken first and a degenerate basis — a zero
    // scale, which draws nothing anyway — leaves the axis at zero and the
    // cluster kept.
    float3 axis = mul(basis,
                      float3(cluster.cone_axis_x, cluster.cone_axis_y, cluster.cone_axis_z));
    float axis_length = length(axis);
    axis = axis_length > 0.0 ? axis / axis_length : float3(0.0, 0.0, 0.0);
    float3 to_center = center - frame.camera_position.xyz;
    float sine = sqrt(max(0.0, 1.0 - cluster.cone_cutoff * cluster.cone_cutoff));
    if (preserves_angles(basis) && cluster.cone_cutoff > 0.0
        && dot(axis, to_center) > sine * length(to_center) + radius)
    {
        return CLUSTER_REJECTED_BY_CONE;
    }

    return CLUSTER_KEPT;
}

/// Whether this cluster is in the cut: `1` to draw it, `0` to leave it to a
/// coarser or finer relative.
///
/// The descent, and the whole of it —
/// `!E(the producer) && E(the container)`, with an absent producer reading as
/// never expanded and an absent container as always expanded. One task group
/// answers for one cluster out of that cluster's own record and two words of
/// `group_state`, with **no communication of any kind**; the reason that is
/// sound, and the reason both halves name a group rather than this cluster, is
/// on `struct ClusterSelect` above.
///
/// `E` itself is not computed here. `draw_gen.slang`'s `select_level` decided it
/// once for this (instance, group) pair and wrote it down, because
/// `docs/plan/25-lod.md`'s hysteresis makes the answer depend on the previous
/// frame's — and a stage with one workgroup per cluster has nowhere to keep one.
///
/// An absent group's index is zero rather than a sentinel, and zero is in range
/// for every instance's run, so the read is safe whether or not the compiler
/// short-circuits the flag test.
uint cluster_is_selected(ClusterSelect select, uint instance_index)
{
    uint base = instance_index * draw.group_stride;

    bool producer_expanded = (select.flags & HAS_PRODUCER) != 0
        && group_state[base + select.producer_group] != 0u;
    bool container_expanded = (select.flags & HAS_CONTAINER) == 0
        || group_state[base + select.container_group] != 0u;

    return (!producer_expanded && container_expanded) ? 1u : 0u;
}

/// The matrix that carries a mesh-space normal into `GpuInstance::transform`'s
/// space, built out of that transform's own upper-left 3x3.
///
/// `mesh.slang`'s copy, character for character, and its docs carry the whole
/// account: this is the cofactor matrix, `det(basis)` times the inverse
/// transpose, which is the transform a *perpendicular* needs where the bare 3x3
/// is the one a tangent needs. `crcbl_shaders::mesh`'s
/// `every_shader_that_transforms_a_normal_builds_it_with_one_function` holds
/// every copy equal, because a normal that differed between the raster path and
/// the mesh path would be one scene lit two ways.
///
/// Hoisted out of the vertex loop below rather than called per vertex: the
/// instance is fixed for the whole cluster, so this is three cross products per
/// meshlet.
float3x3 normal_basis(float3x3 basis)
{
    return float3x3(cross(basis[1], basis[2]),
                    cross(basis[2], basis[0]),
                    cross(basis[0], basis[1]));
}

/// One cluster of one instance, emitted — the body both mesh entry points are.
///
/// `[ForceInline]`, and it is load-bearing on two of the three targets for the
/// reason `mesh_shader.slang`'s `emit` records: mesh output arrays crossing a
/// function boundary abort Slang's Metal backend and produce HLSL dxc rejects
/// with "Not all elements of output SV_Position were written". Inlined, all
/// three take it — and the alternative is two copies of this body, which is the
/// drift the arrangement exists to prevent.
///
/// `active` is `0` for a group with nothing to draw, which emits no vertices
/// and no primitives. The amplification stage passes `1` unconditionally: a
/// group it dispatched is one it kept.
[ForceInline]
void emit_cluster(
    uint lane,
    uint cluster_index,
    uint instance_slot,
    uint active,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    // An inactive group reads cluster `cluster_base`, which is in bounds
    // because a bucket names a mesh and a mesh has at least one cluster. It
    // reads *something* rather than nothing so the load stays in front of the
    // call too.
    //
    // **One `SetMeshOutputCounts`, with no control flow in front of it.** DXIL
    // validation refuses more than one call — "SetMeshOutputCounts cannot be
    // called multiple times" — and dxc 1.9 reports that for a *single* call
    // reached through a branch as well. Both bounds are workgroup-uniform, so
    // every thread agrees, which is what the call requires.
    Meshlet cluster = clusters[cluster_index];
    SetMeshOutputCounts(cluster.vertex_count * active, cluster.triangle_count * active);
    if (active == 0u)
    {
        return;
    }

    // The instance names its mesh and the mesh names its vertices, exactly as
    // in `mesh.slang`: nothing on this path came from the dispatch, which is
    // what lets one bucket cover whatever culling left it.
    GpuInstance instance = instances[visible_instances[draw.base + instance_slot]];
    GpuMesh mesh = meshes[instance.mesh];

    // The overlay colour, chosen once for the whole cluster rather than per
    // vertex: both overlays are per-cluster facts and the heatmap's is a buffer
    // read and a square root.
    //
    // **A uniform branch**: every lane of every cluster of every instance reads
    // the same `frame.ambient.w`, so a frame drawing the shaded picture reaches
    // neither arm and pays for neither. The heatmap is tested first because its
    // sentinel clears both thresholds — see `HEATMAP_VIEW`.
    float4 overlay = float4(0.0, 0.0, 0.0, 1.0);
    if (frame.ambient.w >= HEATMAP_VIEW)
    {
        overlay = float4(cluster_heat(cluster_index, instance.transform), 1.0);
    }
    else if (frame.ambient.w >= LOD_VIEW)
    {
        overlay = float4(lod_tint(cluster_select[cluster_index].flags >> LEVEL_SHIFT), 1.0);
    }

    // **The level's base, not only the mesh's.** A DAG's levels are decimated
    // separately, so a coarser level's vertices are its own array and belong to
    // no vertex of the level below — where the instance names one mesh and so
    // one base. `vertex_base` is the difference, and it is zero for a mesh that
    // is a single level.
    uint level_base = cluster_select[cluster_index].vertex_base;

    // **Or the instance's own base, when it carries one.** `mesh.slang`'s
    // vertex stage reads the same bit for the same reason: a skinned instance's
    // vertices are a run a compute dispatch deformed them into rather than a
    // mesh entry's, and it goes on naming its source mesh so that everything
    // else — this bucket, the level tables, the bounding box — still resolves.
    // A skinned mesh has one level, so `level_base` is zero on that arm.
    uint base_vertex = (instance.flags & INSTANCE_BASE_VERTEX_OVERRIDE) != 0u
        ? instance.base_vertex
        : mesh.base_vertex;

    // Once for the cluster, not once per vertex: the instance is fixed for the
    // whole meshlet, so the three cross products are amortised over every
    // vertex the loop below writes.
    float3x3 normals = normal_basis((float3x3)instance.transform);

    for (uint v = lane; v < cluster.vertex_count; v += THREADS)
    {
        uint index = cluster_vertices[cluster.vertex_offset + v];
        MeshVertex vertex = vertices[index + base_vertex + level_base];

        float4 world = mul(instance.transform, float4(vertex.position.xyz, 1.0));

        VertexOutput output;
        output.position = mul(frame.view_proj, world);
        output.world_position = world.xyz;
        // Through the cofactor matrix hoisted above, which is the matrix
        // `mesh.slang`'s vertex stage uses on the same normal — a scaled
        // instance would otherwise light differently on the two paths.
        output.world_normal = mul(normals, vertex.normal.xyz);
        // Under either overlay the cluster's colour replaces the vertex's,
        // which is what makes a per-cluster fact — the level a cut spans, or
        // how close to the budget each patch is — visible at all. `LOD_VIEW` is
        // the lower of the two thresholds, so this one comparison covers both;
        // which colour it is was decided above.
        output.color = frame.ambient.w >= LOD_VIEW ? overlay : vertex.color;
        output.material = instance.material;
        output.uv = vertex.uv.xy;
        verts[v] = output;
    }

    for (uint t = lane; t < cluster.triangle_count; t += THREADS)
    {
        uint corner = cluster.triangle_offset + t * 3;
        // Corner indices are cluster-local, which is what the vertex array
        // above is indexed by too — so the triple needs no base added to it.
        tris[t] = uint3(corner_at(corner), corner_at(corner + 1), corner_at(corner + 2));
    }
}

/// The un-amplified mesh stage: **every** cluster of every surviving instance,
/// culled by nothing.
///
/// What a device with `Features::MESH_SHADER` and no `Features::TASK_SHADER`
/// draws through, and it is unchanged by the amplification stage existing. A
/// mesh entry point that declared `in payload` would require a task stage
/// upstream, which is why this and `amplifiedMeshMain` are two entry points
/// rather than one with a parameter.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(THREADS, 1, 1)]
void meshMain(
    uint lane: SV_GroupThreadID,
    uint3 group: SV_GroupID,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    uint active = group_is_live(group);
    emit_cluster(lane, draw.cluster_base + group.x * active, group.y, active, verts, tris);
}

/// §3.5's per-cluster cull: one workgroup per (cluster, instance slot), and a
/// mesh workgroup only for the pairs that survive.
///
/// One thread, because the decision is one cluster's and there is nothing to
/// parallelise over. `DispatchMesh` is called **exactly once and unbranched**,
/// with a group count of `1` or `0` — the DXIL rule `SetMeshOutputCounts` has,
/// and the reason nothing above it is a branch either.
///
/// A group past either bound — which the dispatch extents make unreachable, and
/// a wrong argument structure would not — reads the bucket's first cluster and
/// instance slot zero, which are in bounds and whose values it then multiplies
/// away. That is what keeps `visible_instances` — whose slots past the survivor
/// count hold whatever the buffer last had — from ever reaching `instances` as
/// an index.
///
/// The counters are one atomic per invocation and no more, as they were when
/// there was a single one: the verdict picks the word, and the `if` around it
/// replaces the add of zero this used to do for a group outside the cut. It is
/// in front of nothing the DXIL validator minds — `DispatchMesh` is below it and
/// reached unbranched.
[shader("amplification")]
[numthreads(1, 1, 1)]
void taskMain(uint3 group: SV_GroupID)
{
    uint active = group_is_live(group);
    Meshlet cluster = clusters[draw.cluster_base + group.x * active];
    uint instance_index = visible_instances[draw.base + group.y * active] * active;
    GpuInstance instance = instances[instance_index];

    uint index = draw.cluster_base + group.x * active;
    uint selected = cluster_is_selected(cluster_select[index], instance_index);
    uint verdict = cluster_survives(cluster, instance.transform);
    uint keep = active * selected * (verdict == CLUSTER_KEPT ? 1u : 0u);

    // **Only a cluster that was actually tested lands in a bucket, and it lands
    // in exactly one.** `tested` is the same `active * selected` the draw turns
    // on: a group past the dispatch extents read the bucket's first cluster
    // rather than its own, and a cluster the DAG descent did not select is a
    // relative of the one that was — neither was ever put to the frustum or the
    // cone, so counting either as rejected would report the descent's work as
    // the cull's. That is what makes the three words sum to the size of the cut
    // rather than to the whole resident pool, and it is the arithmetic
    // `apps/quarry`'s device suite asserts.
    uint tested = active * selected;
    uint word = verdict == CLUSTER_KEPT
        ? CLUSTER_SURVIVOR_WORD
        : (verdict == CLUSTER_REJECTED_BY_FRUSTUM ? CLUSTER_FRUSTUM_REJECT_WORD
                                                  : CLUSTER_CONE_REJECT_WORD);
    if (tested == 1u)
    {
        cull_stats[word].add(1u);
    }

    // **The cut, not what survived it.** `selected` alone and not `keep`: this
    // records which clusters the DAG descent chose, and a camera that leaves
    // half the mesh outside its frustum still chose them — the cull is a second
    // rejection on top, and folding it in here would make the record say a
    // level was not selected when what happened is that it was not visible.
    //
    // Slot zero alone writes, so the entry has one writer however many instances
    // of this mesh survived; `active` is the same bound the rest of this
    // function carries, so a group past either extent records nothing rather
    // than recording under the bucket's first cluster.
    if (group.y == 0u && active == 1u)
    {
        cluster_selection[index] = selected;
    }

    ClusterPayload payload;
    payload.cluster = group.x;
    payload.instance = group.y;
    DispatchMesh(keep, 1, 1, payload);
}

/// The mesh stage behind `taskMain`: `meshMain`'s body, for a pair the
/// amplification stage already decided to keep.
///
/// `SV_GroupID` here counts within the one workgroup `DispatchMesh` launched
/// and says nothing about which cluster this is, which is what the payload is
/// for.
[shader("mesh")]
[outputtopology("triangle")]
[numthreads(THREADS, 1, 1)]
void amplifiedMeshMain(
    uint lane: SV_GroupThreadID,
    in payload ClusterPayload amplification,
    out vertices VertexOutput verts[MAX_CLUSTER_VERTICES],
    out indices uint3 tris[MAX_CLUSTER_TRIANGLES])
{
    emit_cluster(
        lane,
        draw.cluster_base + amplification.cluster,
        amplification.instance,
        1u,
        verts,
        tris);
}
