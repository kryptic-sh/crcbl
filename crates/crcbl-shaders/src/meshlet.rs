//! The cluster record a mesh shader indexes, in the byte layout
//! `shaders/mesh_cluster.slang` declares.
//!
//! Topic 03 §3.5 makes mesh shaders the primary
//! geometry path and a cluster its unit of work. The *builder* that partitions
//! a triangle list into clusters is `crcbl_scene::meshlet::build_meshlets` — a
//! bake-side producer over host arrays — and this module is the record it
//! produces and the GPU consumes.
//!
//! # Why the record lives here and the builder does not
//!
//! `crcbl-render` must not depend on `crcbl-scene`: that crate pulls in `gltf`,
//! and a renderer that reaches through the glTF importer to describe its own
//! geometry has the direction backwards. The two crates meet at exactly one
//! place — this one — which is already the home of [`GpuMaterial`] and
//! [`MeshVertex`] for the same reason. So the record both sides have to agree
//! on is here, beside them, and the builder stays where the importer is.
//!
//! [`GpuMaterial`]: crate::mesh::GpuMaterial
//! [`MeshVertex`]: crate::mesh::MeshVertex
//!
//! # The three arrays
//!
//! The meshoptimizer/NVIDIA layout, because it is what a mesh shader consumes
//! and because a per-cluster vertex bound is meaningless without it:
//!
//! * a **vertex array** of original vertex indices, one run per cluster;
//! * a **corner array** of `u8` indices *into that cluster's run* of the vertex
//!   array, three per triangle; and
//! * the **clusters** themselves — [`Meshlet`](crate::meshlet::Meshlet) — naming
//!   both runs and carrying
//!   the cluster's bounds.
//!
//! The corner being a `u8` is the whole reason
//! [`MAX_CLUSTER_VERTICES`](crate::meshlet::MAX_CLUSTER_VERTICES) exists.
//! A GPU has no byte-addressed storage buffer, so the corner array reaches the
//! device as `u32` **words**: word `w` holds corners `4w..4w+4`, least
//! significant byte first, and a cluster's run starts at whatever corner it
//! starts at. Nothing is padded per cluster and nothing needs to be — see
//! [`corner_words`](crate::meshlet::corner_words), which is the one place that
//! packing is written.
//!
//! # Counts are `u32` here and `usize` nowhere
//!
//! The builder used to hold its offsets as `usize`, which no GPU record can:
//! the shader reads four `uint`s. The narrowing therefore happens **in the
//! builder, as it closes each cluster**, and it is checked rather than cast —
//! [`Meshlet::new`](crate::meshlet::Meshlet::new) is the only constructor and it
//! refuses an offset or a count
//! that does not fit. A silent truncation would be a cluster pointing at
//! another cluster's corners, which draws a plausible picture of the wrong
//! geometry.

/// Bytes per [`Meshlet`], and the stride of the cluster storage buffer.
///
/// Four `uint` then eight `float`, no padding: a `std430` struct of scalars has
/// a stride that is exactly the sum of its members, which is the same reason
/// [`GpuMesh`](crate::mesh::GpuMesh) spells its bounds as six `float`s rather
/// than two `float3`s. Checked against the `ArrayStride` and the `Offset`
/// decorations `slangc` emits by this module's
/// `the_cluster_layout_matches_the_offsets_slangc_emits`.
pub const MESHLET_STRIDE: usize = 48;

/// The most vertices one cluster may reference.
///
/// A corner is a `u8` index into the cluster's own vertex run, so 256 is the
/// hard ceiling. This sits well below it at the figure the mesh-shader
/// ecosystem converged on, which keeps a cluster's vertex list inside a
/// wavefront's worth of work.
///
/// `shaders/mesh_cluster.slang` declares the same number as the size of its
/// mesh stage's vertex output array, and
/// `the_shader_declares_the_same_cluster_bounds` is what holds the two in step.
pub const MAX_CLUSTER_VERTICES: usize = 64;

/// The most triangles one cluster may hold.
///
/// The largest multiple of four at or below the 126 that D3D12's mesh-shader
/// output cap and Vulkan's `maxMeshOutputPrimitives` commonly report.
///
/// `shaders/mesh_cluster.slang` declares the same number as the size of its
/// mesh stage's primitive output array, and
/// `the_shader_declares_the_same_cluster_bounds` is what holds the two in step.
pub const MAX_CLUSTER_TRIANGLES: usize = 124;

// A corner is a `u8` index into a cluster's vertex run, so the bound above is
// what makes every `as u8` in the builder lossless. This fails the build if it
// ever stops being true.
const _: () = assert!(MAX_CLUSTER_VERTICES <= u8::MAX as usize + 1);

/// How many (cluster, instance slot) pairs one workgroup of
/// `mesh_cluster.slang`'s amplification stage decides: one lane each, the
/// kept ones compacted into one `DispatchMesh`.
///
/// What a renderer drawing behind that stage passes as
/// [`draw_gen::Params::task_lanes`](crate::draw_gen::Params::task_lanes), so the
/// dispatch `draw_gen.slang` sizes is the one the stage decodes. The shader
/// declares the same number as its `numthreads` and its payload's length, and
/// `the_shader_declares_the_same_task_chunk` holds the two in step.
pub const TASK_LANES: u32 = 32;

/// How many task workgroups one row of a bucket's chunked dispatch holds before
/// the next row starts — Vulkan's guaranteed minimum `maxTaskWorkGroupCount`
/// per dimension, which no HAL limit reports. Declared by both
/// `mesh_cluster.slang` and `draw_gen.slang`, and held in step by the same test.
pub const TASK_CHUNKS_PER_ROW: u32 = 65535;

/// The mesh-dispatch extents `draw_gen.slang`'s `write_task_extents` writes for
/// a bucket whose mesh has `clusters` clusters and `instances` surviving
/// instances, drawn behind a task stage of `lanes` lanes a workgroup: the
/// `ceil(clusters * instances / lanes)` chunks, in rows of
/// [`TASK_CHUNKS_PER_ROW`].
///
/// The host twin of the shader's arithmetic, including its split of the product
/// into `(c / L) * n + ceil((c % L) * n / L)`, so a test can hold the two to one
/// answer — see `crcbl`'s `mesh_e2e` task-chunk tests.
///
/// # Panics
///
/// If `lanes` is zero, which is the shader's "no task stage" value and has no
/// chunked extents.
#[must_use]
pub const fn task_extents(clusters: u32, instances: u32, lanes: u32) -> [u32; 3] {
    task_dispatch(task_chunks(clusters, instances, lanes))
}

/// How many task chunks a bucket of `clusters` clusters and `instances`
/// surviving instances takes behind a task stage of `lanes` lanes:
/// `ceil(clusters * instances / lanes)`, split as [`task_extents`] splits it.
/// `draw_gen.slang`'s `task_chunks`.
///
/// # Panics
///
/// If `lanes` is zero, on [`task_extents`]' terms.
#[must_use]
pub const fn task_chunks(clusters: u32, instances: u32, lanes: u32) -> u32 {
    assert!(lanes != 0, "a task stage has at least one lane");
    (clusters / lanes) * instances + ((clusters % lanes) * instances).div_ceil(lanes)
}

/// A task dispatch of `chunks` workgroups as the three words
/// `draw_gen.slang`'s `store_task_dispatch` writes: the chunks up to a row of
/// [`TASK_CHUNKS_PER_ROW`], the rows, and one.
#[must_use]
pub const fn task_dispatch(chunks: u32) -> [u32; 3] {
    let row = if chunks < TASK_CHUNKS_PER_ROW {
        chunks
    } else {
        TASK_CHUNKS_PER_ROW
    };
    [row, chunks.div_ceil(TASK_CHUNKS_PER_ROW), 1]
}

/// One draw region's chunk starts, from each bucket's
/// [`task_chunks`]: the running sum in front of each bucket, and one word more
/// — the region's end — as `draw_gen.slang` lays them down at
/// [`draw_gen::chunk_start_word`](crate::draw_gen::chunk_start_word), starting
/// from `base`.
///
/// `base` is where the scan that wrote the region had got to: zero for every
/// region but a point light's later faces, which one scan covers end to end.
/// Only differences within the region are counts, so a reader comparing the
/// GPU's words with these subtracts the first of each.
///
/// # Panics
///
/// If the sum passes `u32::MAX`, which the shader's `uint` sum would wrap at.
#[must_use]
pub fn chunk_starts(base: u32, chunks: &[u32]) -> Vec<u32> {
    let mut starts = Vec::with_capacity(chunks.len() + 1);
    let mut at = base;
    starts.push(at);
    for count in chunks {
        at = at.checked_add(*count).expect("a region's chunks fit a u32");
        starts.push(at);
    }
    starts
}

/// Which bucket of a flat call, and which chunk of that bucket, task workgroup
/// `workgroup` is — `mesh_cluster.slang`'s `taskMain` search, transcribed.
///
/// `starts` is the region's chunk starts from the segment's first bucket on,
/// through the region's end — [`chunk_starts`] sliced at that bucket — and the
/// answer is the bucket's offset from the first. The last bucket whose chunks
/// start at or before `starts[0] + workgroup`, so an empty bucket, which
/// starts where the next one does, is never the answer; a workgroup past the
/// region's chunks lands on its last bucket with a chunk past its pairs.
///
/// # Panics
///
/// If `starts` has fewer than two words — a region of no buckets, which no
/// flat call draws.
#[must_use]
pub fn chunk_bucket(starts: &[u32], workgroup: u32) -> (u32, u32) {
    assert!(starts.len() >= 2, "a flat call draws at least one bucket");
    let target = starts[0] + workgroup;
    let mut offset = 0usize;
    let mut limit = starts.len() - 1;
    while limit - offset > 1 {
        let middle = usize::midpoint(offset, limit);
        if starts[middle] <= target {
            offset = middle;
        } else {
            limit = middle;
        }
    }
    (
        u32::try_from(offset).expect("a u32-indexed table"),
        target - starts[offset],
    )
}

/// A cluster's bounding sphere and normal cone, matching
/// `struct ClusterBounds` in `shaders/mesh_cluster.slang`.
///
/// Read by §3.5's per-cluster cull — `mesh_cluster.slang`'s amplification stage
/// — and by nothing else. They are in the record rather than in a table of
/// their own because they share a cluster's lifetime exactly: decided when the
/// cluster is built, gone when it is, and a second table would be a second
/// thing to keep in step.
///
/// `PartialEq` but **not** `Eq`, like [`GpuMesh`](crate::mesh::GpuMesh) beside
/// it: these are floats, and a type claiming total equality over an `f32` is
/// claiming something `NaN` makes untrue.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClusterBounds {
    /// Centre of the bounding sphere, in the positions' own space.
    ///
    /// The midpoint of the cluster's vertex AABB. That makes the sphere below a
    /// *valid* bound and not the minimal one: Ritter's and Welzl's are both
    /// tighter, and neither was worth transcribing for a first cut.
    pub center: [f32; 3],

    /// Distance from [`center`](Self::center) to the cluster's furthest vertex.
    /// Never negative, and zero only for a cluster whose vertices coincide.
    pub radius: f32,

    /// The unit direction the cluster's triangles face on average.
    ///
    /// The normalised sum of the triangles' *un-normalised* cross products, so
    /// the average is weighted by area — a cross product's length is twice its
    /// triangle's area. Area weighting is the choice because it lets a large
    /// face set the axis instead of letting a cluster's tessellation slivers
    /// drag it, and because it is what makes the sum of a closed shape's
    /// normals cancel exactly.
    ///
    /// [`OMNIDIRECTIONAL_AXIS`](Self::OMNIDIRECTIONAL_AXIS) when that sum
    /// cancelled and named no direction.
    pub cone_axis: [f32; 3],

    /// The cosine of the cone's half angle: the smallest
    /// `dot(cone_axis, unit triangle normal)` over the cluster's triangles,
    /// clamped to `-1.0..=1.0` so a rounding overshoot cannot escape the range
    /// the cull rule below takes a square root inside.
    ///
    /// `1.0` is a cluster whose triangles all face exactly one way. A value at
    /// or below zero is a cone spanning a hemisphere or more, which no view
    /// direction can reject;
    /// [`OMNIDIRECTIONAL_CUTOFF`](Self::OMNIDIRECTIONAL_CUTOFF) is the extreme
    /// of that, a cone that cancelled into the whole sphere of directions.
    ///
    /// # The cull this is for
    ///
    /// `mesh_cluster.slang`'s amplification stage rejects a wholly back-facing
    /// cluster, and [`crcbl_render::cull::cluster_survives_cull`] is the same
    /// rule in Rust. With `d = center - camera` and `r` the
    /// [`radius`](Self::radius), both in one space:
    ///
    /// ```text
    /// cone_cutoff > 0.0
    ///     && dot(cone_axis, d) > sqrt(1.0 - cone_cutoff * cone_cutoff) * length(d) + r
    /// ```
    ///
    /// **The `+ r` is what makes it safe, and it is not decoration.** Take it
    /// away and the test is `dot(cone_axis, v) > sqrt(1 - cone_cutoff²)` with
    /// `v` the unit direction from the camera to the centre — which is the
    /// exact test for a cluster of *zero* radius, because it treats every
    /// triangle as sharing the centre's view direction. A cluster with a real
    /// radius close to the camera can hold a front-facing triangle and still be
    /// rejected by that form, which is geometry dropped out of the picture in
    /// the case nobody tries. Over uniformly sampled configurations the
    /// radius-free form drops geometry in a few per cent of them, and the form
    /// above in none.
    ///
    /// # Why it is right
    ///
    /// Write `a = acos(cone_cutoff)` for the cone's half angle, `t` for the
    /// angle between `cone_axis` and `d`, and let `p` be any point of the
    /// cluster and `n` any normal of the cone. The triangle at `p` faces away
    /// from the camera when `dot(n, p - camera) > 0`; every point of the
    /// cluster is inside the sphere, so `dot(n, p - camera) >= dot(n, d) - r`,
    /// and the whole cluster is back-facing as soon as
    /// `min over the cone of dot(n, d) > r`. That minimum is
    /// `length(d) * cos(t + a)`, and expanding the cosine gives
    /// `cone_cutoff * dot(cone_axis, d) - sqrt(1 - cone_cutoff²) * |d × cone_axis| > r`.
    /// The inequality above implies that one for every `cone_cutoff` in
    /// `(0, 1]`, and the two coincide at `r == 0` and at `cone_cutoff == 1` —
    /// so it is conservative everywhere and exact where the geometry is a point
    /// or a plane.
    ///
    /// The `cone_cutoff > 0.0` half survives the correction unchanged, and the
    /// radius term does not subsume it: `sqrt(1 - cone_cutoff²)` is even in
    /// `cone_cutoff`, so it cannot tell a narrow cone from one wider than a
    /// hemisphere — and at `a >= pi/2` the cone holds a front-facing normal for
    /// every possible view.
    ///
    /// [`crcbl_render::cull::cluster_survives_cull`]: https://docs.rs/crcbl-render
    pub cone_cutoff: f32,
}

impl ClusterBounds {
    /// The [`cone_axis`](Self::cone_axis) of a cluster whose normals cancelled.
    ///
    /// Any unit vector is a correct axis for a cone of
    /// [`OMNIDIRECTIONAL_CUTOFF`](Self::OMNIDIRECTIONAL_CUTOFF), since that
    /// cone is every direction whatever its axis. It is a unit vector rather
    /// than a zero one so that a consumer which normalises the axis anyway
    /// cannot produce a NaN from it.
    pub const OMNIDIRECTIONAL_AXIS: [f32; 3] = [0.0, 0.0, 1.0];

    /// The [`cone_cutoff`](Self::cone_cutoff) meaning "this cone is the whole
    /// sphere of directions": the cluster is never backface-cullable.
    ///
    /// This is what a cluster whose triangle normals cancel gets — a closed
    /// shape small enough to fit one cluster, or a fan of opposing faces — and
    /// what a cluster of nothing but zero-area triangles gets, since a zero
    /// cross product names no direction either. Both cases have to land on a
    /// defined value: a NaN here would make the cull rule's comparison false on
    /// some targets and true on others, silently dropping geometry.
    pub const OMNIDIRECTIONAL_CUTOFF: f32 = -1.0;

    /// How far an instance transform may be from carrying every angle across
    /// unchanged, relative to its own scale, before the per-cluster cone test is
    /// skipped for it.
    ///
    /// **The cone is a set of normals, and only a similarity carries one.** A
    /// transform that scales axes unequally moves a normal one way and the
    /// surface it belongs to the other, so neither the cone's axis nor its half
    /// angle survives — and the widened angle has no bound this engine can state
    /// and defend. A cluster of such an instance is kept and the frustum culls
    /// it; `crcbl_render::cull::preserves_angles` and `mesh_cluster.slang`'s
    /// `preserves_angles` are the two spellings of the test, and
    /// `the_shader_declares_the_same_similarity_tolerance` holds this number
    /// equal in both.
    ///
    /// Measured rather than picked. Over rotations, uniform scales from `0.001`
    /// to `1000` and the two composed, the largest relative off-diagonal of
    /// `BᵀB` was `2.8e-7` and the largest relative spread of its diagonal
    /// `5.0e-7` — `f32` noise and nothing else. The smallest real non-uniformity
    /// this workspace treats as one, `crcbl_scene`'s axis ratio of `1.001`,
    /// lands at `2.0e-3`. This sits between them with room on both sides.
    pub const SIMILARITY_TOLERANCE: f32 = 1e-4;
}

/// Why a [`Meshlet`] could not be built.
///
/// One variant, because there is one way for the conversion to fail: a number
/// the host counted in `usize` that a `uint` cannot hold. A hand-written
/// `Display` rather than a `thiserror` derive, because this crate has no
/// dependencies at all — see its manifest, and `crate::sha256`, which is the
/// same decision taken about something far more dangerous to hand-roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshletTooLarge {
    /// Which field overflowed, as it is spelled in [`Meshlet`].
    pub field: &'static str,
    /// The value the host counted, which a `u32` cannot hold.
    pub value: usize,
}

impl std::fmt::Display for MeshletTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "meshlet {} is {}, which does not fit the u32 the shader reads",
            self.field, self.value
        )
    }
}

impl std::error::Error for MeshletTooLarge {}

/// One cluster: where its two runs live, and its bounds. Matches
/// `struct Meshlet` in `shaders/mesh_cluster.slang`.
///
/// `PartialEq` but not `Eq`, for [`ClusterBounds`]' reason: it holds floats.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meshlet {
    /// First entry of this cluster's run in the vertex array.
    pub vertex_offset: u32,

    /// How many original vertex indices this cluster references. At most
    /// [`MAX_CLUSTER_VERTICES`].
    pub vertex_count: u32,

    /// First entry of this cluster's run in the corner array, **counted in
    /// corners** — not in the `u32` words the corners reach the device packed
    /// into, and not in triangles. See [`corner_words`], which is what the two
    /// spellings meet in.
    pub triangle_offset: u32,

    /// How many triangles this cluster holds; its corner run is three times
    /// this long. At most [`MAX_CLUSTER_TRIANGLES`].
    pub triangle_count: u32,

    /// Bounding sphere and normal cone over this cluster's own geometry.
    pub bounds: ClusterBounds,
}

impl Meshlet {
    /// A cluster at host offsets, or the field that would not fit a `uint`.
    ///
    /// **The only constructor that narrows**, which is the point: the builder
    /// counts in `usize` and the shader reads `uint`, and a cast between them
    /// that wrapped would produce a cluster addressing another cluster's
    /// corners — geometry that draws, and is the wrong geometry. Every caller
    /// therefore has to decide what to do about a mesh too large for the
    /// record, rather than discovering it as a picture.
    ///
    /// # Errors
    ///
    /// [`MeshletTooLarge`], naming the first field that exceeded [`u32::MAX`].
    pub fn new(
        vertex_offset: usize,
        vertex_count: usize,
        triangle_offset: usize,
        triangle_count: usize,
        bounds: ClusterBounds,
    ) -> Result<Self, MeshletTooLarge> {
        let narrow = |field, value: usize| {
            u32::try_from(value).map_err(|_| MeshletTooLarge { field, value })
        };
        Ok(Self {
            vertex_offset: narrow("vertex_offset", vertex_offset)?,
            vertex_count: narrow("vertex_count", vertex_count)?,
            triangle_offset: narrow("triangle_offset", triangle_offset)?,
            triangle_count: narrow("triangle_count", triangle_count)?,
            bounds,
        })
    }

    /// The bytes one cluster-buffer element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; MESHLET_STRIDE] {
        let mut bytes = [0u8; MESHLET_STRIDE];
        let mut at = 0usize;
        for value in [
            self.vertex_offset,
            self.vertex_count,
            self.triangle_offset,
            self.triangle_count,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self
            .bounds
            .center
            .iter()
            .chain(&[self.bounds.radius])
            .chain(&self.bounds.cone_axis)
            .chain(&[self.bounds.cone_cutoff])
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, MESHLET_STRIDE);
        bytes
    }

    /// The inverse of [`Meshlet::to_bytes`].
    ///
    /// So a test can decode what a cluster buffer actually holds rather than
    /// trusting a host-side copy of it, which is the same reason
    /// [`GpuMesh::from_bytes`](crate::mesh::GpuMesh::from_bytes) exists.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; MESHLET_STRIDE]) -> Self {
        let word = |offset: usize| {
            bytes[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array"))
        };
        let uint_at = |offset: usize| u32::from_le_bytes(word(offset));
        let float_at = |offset: usize| f32::from_le_bytes(word(offset));
        Self {
            vertex_offset: uint_at(0),
            vertex_count: uint_at(4),
            triangle_offset: uint_at(8),
            triangle_count: uint_at(12),
            bounds: ClusterBounds {
                center: [float_at(16), float_at(20), float_at(24)],
                radius: float_at(28),
                cone_axis: [float_at(32), float_at(36), float_at(40)],
                cone_cutoff: float_at(44),
            },
        }
    }
}

/// Bytes in one bucket's cluster-draw constant block.
///
/// Ten `uint`s, which `std140` rounds up to the next multiple of 16 — the two
/// words past the eighth are [`ClusterDrawConstants::chunk_starts_at`] and
/// [`ClusterDrawConstants::chunk_starts_end`], and the last two are padding.
pub const CLUSTER_DRAW_CONSTANTS_SIZE: usize = 48;

/// What one bucket tells the mesh stage about itself, matching
/// `struct ClusterDrawConstants` in `shaders/mesh_cluster.slang`.
///
/// A block of its own rather than [`DrawConstants`](crate::mesh::DrawConstants),
/// because all but its first field mean nothing to the raster path and a
/// record carrying them there would be unread words in every frame that does
/// not use them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterDrawConstants {
    /// The word holding where this bucket's run of surviving instances starts
    /// this frame — the same number
    /// [`DrawConstants::start_at`](crate::mesh::DrawConstants::start_at)
    /// carries, and the start read there is added to the dispatch's instance
    /// slot instead of to `SV_InstanceID`.
    pub start_at: u32,
    /// This bucket's mesh's first cluster in the cluster buffer.
    pub cluster_base: u32,
    /// How many clusters that mesh has. The dispatch is sized to it, and the
    /// stage checks it again so an over-sized dispatch cannot reach the next
    /// mesh's clusters.
    pub cluster_count: u32,
    /// Which bucket this is, which is the element of the indirect-argument
    /// buffer holding the instance count culling produced.
    pub bucket: u32,
    /// How many groups one instance's run of the LOD hysteresis state holds —
    /// the stride between two instances in it, which is every resident mesh's
    /// group count summed.
    ///
    /// **A frame-wide number in a per-bucket block**, and the same value in
    /// every one of them. It lives here because this is the only uniform block
    /// the amplification stage reads and the state index is
    /// `instance * group_stride + group`; nothing about it is per bucket. See
    /// [`crcbl_shaders::cluster_select`](crate::cluster_select), which is where
    /// the state and the two budgets over it are described.
    pub group_stride: u32,
    /// Where the [`LevelGroup`](crate::level_select::LevelGroup) records start
    /// in the shared table buffer, in **words** — the same offset
    /// [`draw_gen::Params::level_groups_at`](crate::draw_gen::Params::level_groups_at)
    /// carries into the draw-argument pass.
    ///
    /// **The screen-error heatmap is what reads it**, and only that: the mesh
    /// stage projects a cluster's producing group's error itself so the overlay
    /// can shade by the number the selection judged, and the record it projects
    /// lives in that region. Nothing about the *cut* needs it — that arrives
    /// through the hysteresis state.
    ///
    /// A frame-wide number in a per-bucket block for
    /// [`group_stride`](Self::group_stride)'s reason, and fixed for the life of
    /// the renderer besides: the host packs the table regions when a mesh
    /// becomes resident, never per frame.
    pub level_groups_at: u32,
    /// The word of the shared table buffer holding
    /// [`cluster_base`](Self::cluster_base) —
    /// [`TableOffsets::bucket_cluster_bases_at`](crate::draw_gen::TableOffsets::bucket_cluster_bases_at)
    /// plus this bucket.
    ///
    /// **What the SPIR-V task and mesh stages read the cluster base from**, so
    /// that one block can stand for a whole range of consecutive buckets drawn
    /// by one multi-draw `draw_mesh_tasks_indirect`: draw `d` of that call
    /// reads the word `d` past this one, as it reads the start `d` past
    /// [`start_at`](Self::start_at) and the argument structure `d` past
    /// [`bucket`](Self::bucket). See `mesh_cluster.slang`'s "One call for a
    /// range of buckets".
    pub cluster_base_at: u32,
    /// The word of the same buffer holding
    /// [`cluster_count`](Self::cluster_count) —
    /// [`TableOffsets::bucket_clusters_at`](crate::draw_gen::TableOffsets::bucket_clusters_at)
    /// plus this bucket — on [`cluster_base_at`](Self::cluster_base_at)'s
    /// terms.
    pub cluster_count_at: u32,
    /// The word of the visible-instances buffer holding where this bucket's
    /// task chunks start this frame in this block's region —
    /// [`draw_gen::chunk_start_word`](crate::draw_gen::chunk_start_word).
    ///
    /// **What a flat task call searches from.** Behind a task stage a pass
    /// records one `draw_mesh_tasks_indirect` per pipeline partition, binding
    /// the partition's first bucket's block, and `taskMain` finds which bucket
    /// each workgroup belongs to among the words from this one on — then reads
    /// that bucket's words as a ranged call's draw does. See
    /// `mesh_cluster.slang`'s `taskMain`.
    pub chunk_starts_at: u32,
    /// The word holding where the block's region's chunks end — the chunk start
    /// one past its last bucket — which bounds that search.
    pub chunk_starts_end: u32,
}

impl ClusterDrawConstants {
    /// The bytes one bucket's block holds, in `std140` order — every one of
    /// them a field, so a block written whole leaves nothing undefined.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CLUSTER_DRAW_CONSTANTS_SIZE] {
        let mut bytes = [0u8; CLUSTER_DRAW_CONSTANTS_SIZE];
        let mut at = 0usize;
        for value in [
            self.start_at,
            self.cluster_base,
            self.cluster_count,
            self.bucket,
            self.group_stride,
            self.level_groups_at,
            self.cluster_base_at,
            self.cluster_count_at,
            self.chunk_starts_at,
            self.chunk_starts_end,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes
    }
}

/// A corner array packed into the `u32` words a storage buffer hands a shader.
///
/// Word `w` holds corners `4w..4w+4`, least significant byte first — which is
/// what a little-endian `u32` load of the same bytes reads, so the shader's
/// `(word >> ((corner & 3) * 8)) & 0xff` and this function are the same
/// statement from opposite sides. The tail is zero-padded to a whole word,
/// which no cluster's run can reach past: a cluster names its own corners by
/// [`Meshlet::triangle_offset`] and [`Meshlet::triangle_count`], so the padding
/// is addressable by nothing.
///
/// **No per-cluster padding**, which is what the word-addressed read buys: a
/// cluster closed early by [`MAX_CLUSTER_VERTICES`] ends at any corner and the
/// next run starts there, exactly as the host array has it.
#[must_use]
pub fn corner_words(corners: &[u8]) -> Vec<u32> {
    corners
        .chunks(4)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u32, |word, (lane, &corner)| {
                    word | (u32::from(corner) << (lane * 8))
                })
        })
        .collect()
}

/// [`corner_words`] as the little-endian bytes a buffer upload takes.
#[must_use]
pub fn corner_bytes(corners: &[u8]) -> Vec<u8> {
    corner_words(corners)
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
}

/// A cluster array as the bytes a storage-buffer upload takes.
#[must_use]
pub fn cluster_bytes(clusters: &[Meshlet]) -> Vec<u8> {
    clusters
        .iter()
        .flat_map(|cluster| cluster.to_bytes())
        .collect()
}

/// One mesh's clusters and the two arrays they index — everything a mesh stage
/// needs about a mesh's geometry, ready to upload.
///
/// The same three arrays `crcbl_scene::meshlet::MeshletBuild` holds, in the
/// owned form a caller uploads from. A separate type rather than that one
/// because this crate cannot see it: `crcbl-scene` depends on this crate and
/// not the other way round.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshClusters {
    /// The clusters, in the order the builder emitted them.
    pub clusters: Vec<Meshlet>,
    /// Original vertex indices, one run per cluster. **Mesh-relative**, exactly
    /// like the index buffer: the base vertex is added by whoever reads them.
    pub vertices: Vec<u32>,
    /// Triangle corners, three per triangle, one run per cluster. Each is an
    /// index into its own cluster's run of [`vertices`](Self::vertices).
    pub corners: Vec<u8>,
}

/// What is wrong with one cluster, and the numbers that say so.
///
/// Every variant is a read the mesh stage would make outside an array. They are
/// separate because the numbers differ, not because a caller branches on them:
/// [`ClustersInvalid`] is a diagnostic, and the only thing anyone does with one
/// is print it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClusterFault {
    /// The cluster's run in [`MeshClusters::vertices`] runs off the end.
    VertexRun {
        /// [`Meshlet::vertex_offset`].
        offset: u32,
        /// [`Meshlet::vertex_count`].
        count: u32,
        /// How long the array actually is.
        len: usize,
    },
    /// The cluster's run in [`MeshClusters::corners`] runs off the end. Its
    /// length is three times [`Meshlet::triangle_count`].
    CornerRun {
        /// [`Meshlet::triangle_offset`], counted in corners.
        offset: u32,
        /// Corners, so three per triangle.
        count: u32,
        /// How long the array actually is.
        len: usize,
    },
    /// The cluster holds more vertices than [`MAX_CLUSTER_VERTICES`], which is
    /// what the mesh stage's per-cluster arrays are sized for.
    TooManyVertices {
        /// [`Meshlet::vertex_count`].
        count: u32,
    },
    /// The cluster holds more triangles than [`MAX_CLUSTER_TRIANGLES`], on
    /// [`TooManyVertices`](Self::TooManyVertices)' terms.
    TooManyTriangles {
        /// [`Meshlet::triangle_count`].
        count: u32,
    },
    /// An entry of the cluster's vertex run names a vertex the mesh has not
    /// got. **Mesh-relative**, so it is compared against the mesh's own vertex
    /// count and not against the pool's.
    VertexIndex {
        /// Which entry of the cluster's own run, from zero.
        at: u32,
        /// What it names.
        index: u32,
        /// How many vertices the mesh has.
        vertices: usize,
    },
    /// A corner names an entry past the end of its own cluster's vertex run.
    Corner {
        /// Which corner of the cluster's own run, from zero.
        at: u32,
        /// What it names.
        corner: u8,
        /// [`Meshlet::vertex_count`], which is what bounds it.
        count: u32,
    },
}

impl std::fmt::Display for ClusterFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::VertexRun { offset, count, len } => write!(
                f,
                "takes vertices {offset}..{} of an array of {len}",
                u64::from(offset) + u64::from(count)
            ),
            Self::CornerRun { offset, count, len } => write!(
                f,
                "takes corners {offset}..{} of an array of {len}",
                u64::from(offset) + u64::from(count)
            ),
            Self::TooManyVertices { count } => write!(
                f,
                "holds {count} vertices, and a cluster holds at most {MAX_CLUSTER_VERTICES}"
            ),
            Self::TooManyTriangles { count } => write!(
                f,
                "holds {count} triangles, and a cluster holds at most {MAX_CLUSTER_TRIANGLES}"
            ),
            Self::VertexIndex {
                at,
                index,
                vertices,
            } => write!(
                f,
                "names vertex {index} at entry {at} of its run, and the mesh has {vertices}"
            ),
            Self::Corner { at, corner, count } => write!(
                f,
                "has corner {at} naming entry {corner} of a run of {count}"
            ),
        }
    }
}

/// A cluster array that would read outside itself, and which cluster does it.
///
/// Returned by [`MeshClusters::check`]; see there for why this is worth a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClustersInvalid {
    /// Which cluster, indexing [`MeshClusters::clusters`].
    pub cluster: usize,
    /// What is wrong with it.
    pub fault: ClusterFault,
}

impl std::fmt::Display for ClustersInvalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cluster {} {}", self.cluster, self.fault)
    }
}

impl std::error::Error for ClustersInvalid {}

impl MeshClusters {
    /// **Every read a mesh stage makes from these arrays lands inside them.**
    ///
    /// The mesh shader indexes `vertices` by a cluster's run, `corners` by the
    /// same, and the mesh's vertex buffer by whatever `vertices` holds — none of
    /// it bounds-checked, because none of it can be. A cluster naming a vertex
    /// the mesh has not got is an out-of-bounds device read that no layer below
    /// reports: the draw succeeds, the frame is wrong or the device is lost, and
    /// nothing says which. So it is refused on the host, where the arrays are
    /// still `Vec`s and the answer is a sentence.
    ///
    /// `vertices` is the mesh's **own** vertex count, mesh-relative like the
    /// index buffer — the pool's base vertex is added by whoever reads it and is
    /// not this type's business.
    ///
    /// # Errors
    ///
    /// [`ClustersInvalid`] naming the first cluster that is wrong and what is
    /// wrong with it. First rather than all of them because the arrays are
    /// generated: one fault is a builder bug, and the second is the same bug.
    pub fn check(&self, vertices: usize) -> Result<(), ClustersInvalid> {
        for (cluster, meshlet) in self.clusters.iter().enumerate() {
            let fault = |fault| Err(ClustersInvalid { cluster, fault });
            if meshlet.vertex_count as usize > MAX_CLUSTER_VERTICES {
                return fault(ClusterFault::TooManyVertices {
                    count: meshlet.vertex_count,
                });
            }
            if meshlet.triangle_count as usize > MAX_CLUSTER_TRIANGLES {
                return fault(ClusterFault::TooManyTriangles {
                    count: meshlet.triangle_count,
                });
            }
            // Widened before adding: both are what the device reads as `uint`,
            // and a run that wraps `u32` would otherwise pass as a short one.
            let vertex_end = u64::from(meshlet.vertex_offset) + u64::from(meshlet.vertex_count);
            if vertex_end > self.vertices.len() as u64 {
                return fault(ClusterFault::VertexRun {
                    offset: meshlet.vertex_offset,
                    count: meshlet.vertex_count,
                    len: self.vertices.len(),
                });
            }
            let corners = meshlet.triangle_count * 3;
            let corner_end = u64::from(meshlet.triangle_offset) + u64::from(corners);
            if corner_end > self.corners.len() as u64 {
                return fault(ClusterFault::CornerRun {
                    offset: meshlet.triangle_offset,
                    count: corners,
                    len: self.corners.len(),
                });
            }

            let run = meshlet.vertex_offset as usize..vertex_end as usize;
            for (at, &index) in self.vertices[run].iter().enumerate() {
                if index as usize >= vertices {
                    return fault(ClusterFault::VertexIndex {
                        at: at as u32,
                        index,
                        vertices,
                    });
                }
            }
            let run = meshlet.triangle_offset as usize..corner_end as usize;
            for (at, &corner) in self.corners[run].iter().enumerate() {
                if u32::from(corner) >= meshlet.vertex_count {
                    return fault(ClusterFault::Corner {
                        at: at as u32,
                        corner,
                        count: meshlet.vertex_count,
                    });
                }
            }
        }
        Ok(())
    }
}

/// The cube's clusters — **one**, because 24 vertices and 12 triangles are
/// inside both bounds with room to spare.
///
/// # Why this is data here rather than a call to the builder
///
/// `crcbl_scene::meshlet::build_meshlets` is what partitions a triangle list,
/// and neither this crate nor `crcbl-render` can call it: `crcbl-scene` depends
/// on this one, and the renderer must not depend on `crcbl-scene` at all. §3.5
/// makes the meshlet build a **bake step** for exactly that reason — the
/// renderer receives clusters rather than computing them — and the cube is
/// this crate's own hardcoded geometry, so its clusters are cooked here.
///
/// The values are not asserted in prose: `crcbl-scene`'s
/// `the_hardcoded_meshes_cluster_the_way_the_shaders_crate_says` runs the real
/// builder over [`cube_vertices`](crate::mesh::cube_vertices) and
/// [`cube_indices`](crate::mesh::cube_indices) and compares all three arrays,
/// so this is a committed artifact with a check beside it — the same
/// arrangement as the compiled SPIR-V and `compile-shaders.sh --check`.
#[must_use]
pub fn cube_clusters() -> MeshClusters {
    whole_mesh_clusters(
        crate::mesh::CUBE_VERTEX_COUNT,
        &crate::mesh::cube_indices(),
        ClusterBounds {
            // The cube spans -0.5..0.5 on every axis, so its AABB midpoint is
            // the origin and its furthest corner is half the unit cube's
            // diagonal away.
            center: [0.0, 0.0, 0.0],
            radius: 0.866_025_4,
            // A closed shape's area-weighted normals cancel, so the builder
            // finds no axis and hands back the cone that rejects nothing.
            cone_axis: ClusterBounds::OMNIDIRECTIONAL_AXIS,
            cone_cutoff: ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
        },
    )
}

/// The pyramid's clusters — **one**, on [`cube_clusters`]' terms exactly: 16
/// vertices and 6 triangles, cooked here, checked against the real builder by
/// `crcbl-scene`.
#[must_use]
pub fn pyramid_clusters() -> MeshClusters {
    whole_mesh_clusters(
        crate::mesh::PYRAMID_VERTEX_COUNT,
        &crate::mesh::pyramid_indices(),
        ClusterBounds {
            // The apex is above the base, so the AABB midpoint sits slightly
            // up the Y axis rather than at the origin.
            center: [0.0, 0.049_999_997, 0.0],
            radius: 0.722_841_6,
            // Closed, like the cube.
            cone_axis: ClusterBounds::OMNIDIRECTIONAL_AXIS,
            cone_cutoff: ClusterBounds::OMNIDIRECTIONAL_CUTOFF,
        },
    )
}

/// The open box's clusters — **one per face**, and the first mesh here that is
/// more than one cluster at all.
///
/// [`OPEN_BOX_SUBDIVISIONS`](crate::mesh::OPEN_BOX_SUBDIVISIONS) is what makes
/// that true: a face is four by four quads of four unshared vertices, which is
/// [`MAX_CLUSTER_VERTICES`] exactly, so the builder's greedy walk closes a
/// cluster on every face boundary. That gives every cluster after the first a
/// non-zero [`Meshlet::vertex_offset`] *within one mesh* — which the cube and
/// the pyramid, being one cluster each, cannot produce however many of them are
/// resident.
///
/// # Why this is a loop and the other two are literals
///
/// The cube's and the pyramid's bounds are written out because there is one set
/// of them each. Five faces is where that stops being readable, and a face's
/// bounds are arithmetic rather than a measurement — see `face_bounds`. Every
/// value it produces is exact in `f32` for this mesh's coordinates, so
/// `crcbl-scene`'s `the_hardcoded_meshes_cluster_the_way_the_shaders_crate_says`
/// still compares this against the real builder for equality, exactly as it
/// does the two above.
#[must_use]
pub fn open_box_clusters() -> MeshClusters {
    let quads = crate::mesh::OPEN_BOX_QUADS_PER_FACE;
    let corners_per_face = quads * 6;
    let mut clusters = Vec::with_capacity(crate::mesh::OPEN_BOX_FACES.len());
    let mut corners = Vec::with_capacity(crate::mesh::OPEN_BOX_INDEX_COUNT);
    for (face, geometry) in crate::mesh::OPEN_BOX_FACES.iter().enumerate() {
        clusters.push(
            Meshlet::new(
                face * MAX_CLUSTER_VERTICES,
                MAX_CLUSTER_VERTICES,
                face * corners_per_face,
                quads * 2,
                face_bounds(geometry),
            )
            .unwrap_or_else(|error| unreachable!("a few hundred vertices of demo mesh: {error}")),
        );
        // A corner indexes its own cluster's vertex run, so every face
        // triangulates from zero again — which is the whole difference between
        // a corner and an index, and the thing a cluster at a non-zero
        // `vertex_offset` is what makes observable.
        for quad in 0..quads {
            let base = u8::try_from(quad * 4)
                .unwrap_or_else(|_| unreachable!("a cluster holds at most 256 vertices"));
            corners.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    MeshClusters {
        clusters,
        vertices: (0..crate::mesh::OPEN_BOX_VERTEX_COUNT as u32).collect(),
        corners,
    }
}

/// One flat rectangular face's bounding sphere and normal cone, which for the
/// open box is one cluster's.
///
/// Arithmetic a reader can check rather than a summary of a loop, because a
/// planar rectangle makes all three answers closed forms:
///
/// * the AABB's midpoint is the midpoint of either diagonal, and
///   `corners[0]`/`corners[2]` are one;
/// * the vertex furthest from it is a corner, at half the diagonal — a
///   subdivision of a rectangle puts no interior point outside its corners; and
/// * every triangle of a planar face faces the face's own way, so the cone is
///   that single direction and its cutoff is exactly `1.0`.
///
/// Each is exact in `f32` for this mesh's coordinates — every one of them is a
/// multiple of a sixteenth, and the radius is one correctly-rounded `sqrt` of a
/// sum that is itself exact — so the equality the pin test asserts is a
/// property of the numbers rather than luck.
fn face_bounds(face: &crate::mesh::Face) -> ClusterBounds {
    let center = [0, 1, 2].map(|axis| (face.corners[0][axis] + face.corners[2][axis]) * 0.5);
    let radius = [0, 1, 2]
        .iter()
        .map(|&axis| {
            let delta = face.corners[0][axis] - center[axis];
            delta * delta
        })
        .sum::<f32>()
        .sqrt();
    ClusterBounds {
        center,
        radius,
        cone_axis: face.normal,
        cone_cutoff: 1.0,
    }
}

/// The single-cluster decomposition of a mesh that fits one cluster whole.
///
/// A mesh inside both bounds is clustered by the builder into exactly one
/// cluster whose vertex run is every vertex in first-seen order — and for a
/// mesh whose indices are dense and ascending, as both of this crate's are,
/// that run is the identity and every corner is its own index. **Neither
/// property is assumed**: the caller is one of the two functions above, and
/// `crcbl-scene`'s pin test compares the result against the real builder array
/// for array.
///
/// # Panics
///
/// If the mesh does not fit one cluster, or an index does not fit a `u8`.
/// Both are compile-time facts about this crate's two meshes rather than
/// runtime conditions, and getting either wrong silently would be a cluster
/// describing geometry that is not there.
fn whole_mesh_clusters(
    vertex_count: usize,
    indices: &[u32],
    bounds: ClusterBounds,
) -> MeshClusters {
    let triangle_count = indices.len() / 3;
    assert!(
        vertex_count <= MAX_CLUSTER_VERTICES && triangle_count <= MAX_CLUSTER_TRIANGLES,
        "a {vertex_count}-vertex, {triangle_count}-triangle mesh is more than one \
         cluster, so its clusters are the builder's to produce"
    );
    let vertices: Vec<u32> = (0..vertex_count as u32).collect();
    let corners: Vec<u8> = indices
        .iter()
        .map(|&index| {
            u8::try_from(index)
                .unwrap_or_else(|_| unreachable!("a cluster holds at most 256 vertices"))
        })
        .collect();
    MeshClusters {
        clusters: vec![
            Meshlet::new(0, vertex_count, 0, triangle_count, bounds)
                .unwrap_or_else(|error| unreachable!("a single cluster of a demo mesh: {error}")),
        ],
        vertices,
        corners,
    }
}

#[cfg(test)]
mod tests {

    /// **The shader's similarity tolerance is this crate's.**
    ///
    /// Two spellings of one rule, one in Slang and one in
    /// `crcbl_render::cull::preserves_angles`, both reading this number. A
    /// shader that drifted to a looser one would trust the cone through a
    /// transform the oracle does not, which is a cluster dropped on the GPU and
    /// kept in every test.
    ///
    /// The literal is compared as text rather than parsed: the point is that the
    /// two files say the same thing, and a shader constant is not something this
    /// crate can evaluate.
    #[test]
    fn the_shader_declares_the_same_similarity_tolerance() {
        let source = include_str!("../shaders/mesh_cluster.slang");
        let declaration = "static const float SIMILARITY_TOLERANCE = 1e-4;";
        assert!(
            source.contains(declaration),
            "shaders/mesh_cluster.slang must declare `{declaration}`, matching \
             ClusterBounds::SIMILARITY_TOLERANCE"
        );
        assert_eq!(
            ClusterBounds::SIMILARITY_TOLERANCE,
            1e-4,
            "and this crate's constant is the number that declaration spells"
        );
        assert!(
            source.contains("preserves_angles(basis) && cluster.cone_cutoff > 0.0"),
            "the shader declares the tolerance and does not gate the cone test with it"
        );
    }
    use super::*;

    /// The offsets `slangc` actually emitted for `Meshlet`, read out of the
    /// disassembly of `spirv/mesh_cluster.spv`. Four `uint`s in a row would
    /// permute silently — a vertex offset read as a triangle offset draws
    /// geometry rather than crashing — so the byte each lands on is pinned
    /// rather than assumed.
    #[test]
    fn the_cluster_layout_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_runtimearr_Meshlet_std430 ArrayStride 48`, and
        // `OpMemberDecorate %Meshlet_std430 n Offset …`.
        assert_eq!(MESHLET_STRIDE, 48);

        let cluster = Meshlet {
            vertex_offset: 1,
            vertex_count: 2,
            triangle_offset: 3,
            triangle_count: 4,
            bounds: ClusterBounds {
                center: [5.0, 6.0, 7.0],
                radius: 8.0,
                cone_axis: [9.0, 10.0, 11.0],
                cone_cutoff: 12.0,
            },
        };
        let bytes = cluster.to_bytes();
        assert_eq!(bytes.len(), MESHLET_STRIDE);
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "vertex_offset at offset 0");
        assert_eq!(uint_at(4), 2, "vertex_count at offset 4");
        assert_eq!(uint_at(8), 3, "triangle_offset at offset 8");
        assert_eq!(uint_at(12), 4, "triangle_count at offset 12");
        assert_eq!(float_at(16), 5.0, "bounds.center at offset 16");
        assert_eq!(float_at(24), 7.0, "and it is three floats wide");
        assert_eq!(float_at(28), 8.0, "bounds.radius at offset 28");
        assert_eq!(float_at(32), 9.0, "bounds.cone_axis at offset 32");
        assert_eq!(float_at(40), 11.0, "and it is three floats wide");
        assert_eq!(float_at(44), 12.0, "bounds.cone_cutoff at offset 44");

        // And the decode agrees with the encode, field for field.
        assert_eq!(Meshlet::from_bytes(&bytes), cluster);
    }

    /// The offsets `slangc` emitted for `ClusterDrawConstants`, read out of the
    /// disassembly. Ten `uint`s in a row permute silently — a bucket index
    /// read as a cluster base draws another mesh's clusters — so each is pinned
    /// to its byte.
    #[test]
    fn the_cluster_constants_match_the_offsets_slangc_emits() {
        // `OpMemberDecorate %ClusterDrawConstants_std140 n Offset …`: 0, 4, 8,
        // 12, 16, 20, 24, 28, 32, 36, and a block of 40 bytes, which `std140`
        // rounds up to its multiple of 16.
        assert_eq!(CLUSTER_DRAW_CONSTANTS_SIZE, 48);
        assert_eq!(CLUSTER_DRAW_CONSTANTS_SIZE % 16, 0);

        let bytes = ClusterDrawConstants {
            start_at: 1,
            cluster_base: 2,
            cluster_count: 3,
            bucket: 4,
            group_stride: 5,
            level_groups_at: 6,
            cluster_base_at: 7,
            cluster_count_at: 8,
            chunk_starts_at: 9,
            chunk_starts_end: 10,
        }
        .to_bytes();
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1, "start_at at offset 0");
        assert_eq!(uint_at(4), 2, "cluster_base at offset 4");
        assert_eq!(uint_at(8), 3, "cluster_count at offset 8");
        assert_eq!(uint_at(12), 4, "bucket at offset 12");
        assert_eq!(uint_at(16), 5, "group_stride at offset 16");
        assert_eq!(uint_at(20), 6, "level_groups_at at offset 20");
        assert_eq!(uint_at(24), 7, "cluster_base_at at offset 24");
        assert_eq!(uint_at(28), 8, "cluster_count_at at offset 28");
        assert_eq!(uint_at(32), 9, "chunk_starts_at at offset 32");
        assert_eq!(uint_at(36), 10, "chunk_starts_end at offset 36");
        assert!(
            bytes[40..].iter().all(|byte| *byte == 0),
            "the std140 tail is padding, and zero"
        );
    }

    /// The narrowing refuses rather than wraps, and names the field it refused.
    ///
    /// The failure a cast would produce is not a crash: a wrapped offset points
    /// at another cluster's corners and draws them.
    #[test]
    fn an_offset_a_uint_cannot_hold_is_refused_by_name() {
        let huge = u32::MAX as usize + 1;
        let bounds = ClusterBounds::default();
        assert_eq!(
            Meshlet::new(huge, 3, 0, 1, bounds),
            Err(MeshletTooLarge {
                field: "vertex_offset",
                value: huge,
            })
        );
        assert_eq!(
            Meshlet::new(0, 3, huge, 1, bounds),
            Err(MeshletTooLarge {
                field: "triangle_offset",
                value: huge,
            })
        );
        // And a cluster whose numbers all fit is built, so the check above is
        // about the size rather than about the constructor always failing.
        assert_eq!(
            Meshlet::new(7, 3, 9, 1, bounds).expect("these fit"),
            Meshlet {
                vertex_offset: 7,
                vertex_count: 3,
                triangle_offset: 9,
                triangle_count: 1,
                bounds,
            }
        );
    }

    /// The message names the field and the value, because a mesh that will not
    /// cluster is a bake failure someone has to act on.
    #[test]
    fn the_overflow_message_names_the_field_and_the_value() {
        let message = MeshletTooLarge {
            field: "triangle_count",
            value: 5_000_000_000,
        }
        .to_string();
        assert!(message.contains("triangle_count"), "{message}");
        assert!(message.contains("5000000000"), "{message}");
    }

    /// Corner `n` is byte `n % 4` of word `n / 4`, which is what the shader's
    /// shift-and-mask reads. A packer that filled the word the other way round
    /// produces the same *length* and every triangle's corners transposed.
    #[test]
    fn corners_pack_four_to_a_word_least_significant_first() {
        let corners: Vec<u8> = (0..9).collect();
        let words = corner_words(&corners);
        assert_eq!(
            words.len(),
            3,
            "nine corners is three words, the last short"
        );
        assert_eq!(words[0], 0x0302_0100);
        assert_eq!(words[1], 0x0706_0504);
        assert_eq!(words[2], 0x0000_0008, "the tail pads with zeroes");

        // Read back exactly as the shader reads it.
        for (index, &corner) in corners.iter().enumerate() {
            let word = words[index / 4];
            let unpacked = (word >> ((index % 4) * 8)) & 0xff;
            assert_eq!(unpacked, u32::from(corner), "corner {index}");
        }
        assert_eq!(
            corner_bytes(&corners),
            corners
                .iter()
                .copied()
                .chain([0, 0, 0])
                .collect::<Vec<u8>>(),
            "the packed bytes are the corner bytes, which is what makes the \
             word view and the byte view the same array"
        );
    }

    /// An empty corner array is no words rather than one zero word, or every
    /// mesh with no triangles would upload a buffer with a corner in it.
    #[test]
    fn no_corners_is_no_words() {
        assert!(corner_words(&[]).is_empty());
        assert!(corner_bytes(&[]).is_empty());
        assert!(cluster_bytes(&[]).is_empty());
    }

    /// A cluster array uploads as its elements back to back at
    /// [`MESHLET_STRIDE`], with no gap a shader's `ArrayStride` would read
    /// across.
    #[test]
    fn a_cluster_array_uploads_at_the_stride_the_shader_reads() {
        let clusters = [
            Meshlet::new(0, 3, 0, 1, ClusterBounds::default()).expect("fits"),
            Meshlet::new(3, 4, 3, 2, ClusterBounds::default()).expect("fits"),
        ];
        let bytes = cluster_bytes(&clusters);
        assert_eq!(bytes.len(), clusters.len() * MESHLET_STRIDE);
        for (index, cluster) in clusters.iter().enumerate() {
            let at = index * MESHLET_STRIDE;
            let element: [u8; MESHLET_STRIDE] = bytes[at..at + MESHLET_STRIDE]
                .try_into()
                .expect("one whole element");
            assert_eq!(Meshlet::from_bytes(&element), *cluster, "element {index}");
        }
    }

    /// **The open box's clusters decode back to its own index buffer**, in
    /// order and with nothing dropped or repeated.
    ///
    /// `crcbl-scene`'s pin test compares these clusters against the real
    /// builder, which is the check that the *partition* is the one the engine
    /// would produce. This is the other half and it needs no other crate: that
    /// the partition describes **this mesh's triangles**. A `vertex_offset`
    /// short by one cluster, a corner run that restarted at the wrong place, or
    /// a triangulation emitted the other way round each survive a comparison
    /// against a builder that made the same mistake, and none of them survive
    /// this.
    #[test]
    fn the_open_box_clusters_decode_back_to_its_index_buffer() {
        let cooked = open_box_clusters();
        assert!(
            cooked.clusters.len() > 1,
            "the mesh exists to be several clusters, and is {}",
            cooked.clusters.len()
        );

        let mut decoded = Vec::with_capacity(crate::mesh::OPEN_BOX_INDEX_COUNT);
        for cluster in &cooked.clusters {
            let run =
                &cooked.vertices[cluster.vertex_offset as usize..][..cluster.vertex_count as usize];
            let corners = &cooked.corners[cluster.triangle_offset as usize..]
                [..cluster.triangle_count as usize * 3];
            for &corner in corners {
                decoded.push(run[usize::from(corner)]);
            }
        }
        assert_eq!(
            decoded,
            crate::mesh::open_box_indices(),
            "the clusters describe geometry the mesh does not have"
        );

        // Anti-vacuity for the offsets themselves: every cluster after the
        // first starts part way into both runs, which is the case the two
        // single-cluster meshes above cannot produce.
        for (index, cluster) in cooked.clusters.iter().enumerate().skip(1) {
            assert!(
                cluster.vertex_offset > 0 && cluster.triangle_offset > 0,
                "cluster {index} starts at ({}, {})",
                cluster.vertex_offset,
                cluster.triangle_offset
            );
        }
    }

    /// **The shader's output-array sizes are these constants**, and the two
    /// live in different languages with nothing but this to hold them
    /// together. A mesh stage declaring fewer vertices than a cluster may
    /// reference writes past its own array.
    #[test]
    fn the_shader_declares_the_same_cluster_bounds() {
        let source = include_str!("../shaders/mesh_cluster.slang");
        for (name, value) in [
            ("MAX_CLUSTER_VERTICES", MAX_CLUSTER_VERTICES),
            ("MAX_CLUSTER_TRIANGLES", MAX_CLUSTER_TRIANGLES),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&declaration),
                "shaders/mesh_cluster.slang must declare `{declaration}`, or its \
                 output arrays and this crate's bound disagree"
            );
        }
        // The two hardware ceilings the constants sit under, in a const block
        // because they are compile-time facts and clippy is right that a
        // runtime assertion over two constants is not a test.
        const {
            assert!(MAX_CLUSTER_VERTICES <= u8::MAX as usize + 1);
            // D3D12's mesh-shader output cap, and the `maxMeshOutputPrimitives`
            // every driver here reports.
            assert!(MAX_CLUSTER_TRIANGLES <= 126);
        }
    }

    /// Both shaders declare the chunk this crate sizes dispatches by, and the
    /// task stage is as wide as the chunk.
    ///
    /// A `draw_gen.slang` sizing chunks of one width and a task stage decoding
    /// another skips pairs or double-draws them, and nothing but a frame
    /// comparison would notice.
    #[test]
    fn the_shader_declares_the_same_task_chunk() {
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        let draw_gen = include_str!("../shaders/draw_gen.slang");
        let lanes = format!("static const uint TASK_LANES = {TASK_LANES};");
        let row = format!("static const uint TASK_CHUNKS_PER_ROW = {TASK_CHUNKS_PER_ROW};");
        assert!(
            cluster.contains(&lanes),
            "mesh_cluster.slang must declare `{lanes}`"
        );
        assert!(
            cluster.contains(&row),
            "mesh_cluster.slang must declare `{row}`"
        );
        assert!(
            draw_gen.contains(&row),
            "draw_gen.slang must declare `{row}`"
        );
        for needle in [
            "[numthreads(TASK_LANES, 1, 1)]\nvoid taskMain(",
            "uint pairs[TASK_LANES];",
            "uint workgroup = chunk_group.y * TASK_CHUNKS_PER_ROW + chunk_group.x;",
            "uint pair = chunk * TASK_LANES + lane;",
        ] {
            assert!(
                cluster.contains(needle),
                "mesh_cluster.slang's task stage must read `{needle}`"
            );
        }
    }

    /// [`task_extents`] is `ceil(clusters * instances / lanes)` chunks, exactly,
    /// laid out in rows of [`TASK_CHUNKS_PER_ROW`] — checked against the
    /// quotient computed wide, so the split that keeps the shader's product
    /// from overflowing is held to the answer it stands for.
    #[test]
    fn the_task_extents_cover_every_pair_in_whole_chunks() {
        // By hand: nothing survived, one instance of one cluster, a chunk
        // exactly full, one pair over it, and a single-cluster bucket of the
        // price scene's size.
        assert_eq!(task_extents(1, 0, TASK_LANES), [0, 0, 1]);
        assert_eq!(task_extents(1, 1, TASK_LANES), [1, 1, 1]);
        assert_eq!(task_extents(1, 32, TASK_LANES), [1, 1, 1]);
        assert_eq!(task_extents(1, 33, TASK_LANES), [2, 1, 1]);
        assert_eq!(task_extents(3, 11, TASK_LANES), [2, 1, 1]);
        assert_eq!(task_extents(1, 17_219, TASK_LANES), [539, 1, 1]);
        // Past one row: 65,536 chunks is one full row and one chunk of the next.
        assert_eq!(
            task_extents(TASK_LANES, TASK_CHUNKS_PER_ROW + 1, TASK_LANES),
            [TASK_CHUNKS_PER_ROW, 2, 1]
        );
        for clusters in [1u32, 2, 5, 31, 32, 33, 64, 100, 1_000, 4_095] {
            for instances in [0u32, 1, 2, 7, 31, 32, 33, 1_000, 17_219, 100_000] {
                let pairs = u64::from(clusters) * u64::from(instances);
                let chunks = pairs.div_ceil(u64::from(TASK_LANES));
                let [x, y, z] = task_extents(clusters, instances, TASK_LANES);
                let launched = u64::from(x) * u64::from(y);
                assert!(
                    launched >= chunks,
                    "{clusters} x {instances}: {launched} chunks cannot cover {chunks}"
                );
                assert!(
                    launched - chunks < u64::from(TASK_CHUNKS_PER_ROW),
                    "{clusters} x {instances}: more than a row's slack past {chunks} chunks"
                );
                assert_eq!(
                    u64::from(x),
                    chunks.min(u64::from(TASK_CHUNKS_PER_ROW)),
                    "{clusters} x {instances}: x is the chunks up to a row"
                );
                assert_eq!(
                    u64::from(y),
                    chunks.div_ceil(u64::from(TASK_CHUNKS_PER_ROW)),
                    "{clusters} x {instances}: y is the rows"
                );
                assert_eq!(z, 1);
            }
        }
    }

    /// **The flat call's work list names every chunk of a segment exactly once,
    /// and nothing else**: [`chunk_starts`] is the running sum a region holds,
    /// and [`chunk_bucket`], `taskMain`'s search transcribed, answers each of
    /// a segment's workgroups with the bucket and chunk a linear walk of the
    /// counts finds — from every first bucket, over empty buckets at either
    /// end and in runs, with a scan base that is not zero as a point light's
    /// later faces have.
    ///
    /// A workgroup past the segment's chunks — the last row's padding — lands
    /// on the region's last bucket with a chunk at or past its count, which
    /// the task stage's bound refuses.
    #[test]
    fn the_flat_work_list_finds_every_chunk_of_a_segment_once() {
        assert_eq!(chunk_starts(0, &[]), [0]);
        assert_eq!(chunk_starts(0, &[2, 0, 3]), [0, 2, 2, 5]);
        assert_eq!(chunk_starts(7, &[1, 1]), [7, 8, 9]);
        assert_eq!(
            task_chunks(5, 13, TASK_LANES),
            3,
            "65 pairs, one lane into a third chunk"
        );
        assert_eq!(task_chunks(1, 70, TASK_LANES), 3);
        assert_eq!(task_dispatch(0), [0, 0, 1]);
        assert_eq!(
            task_dispatch(TASK_CHUNKS_PER_ROW + 1),
            [TASK_CHUNKS_PER_ROW, 2, 1]
        );

        let tables: [&[u32]; 5] = [
            &[3],
            &[0, 0, 4, 0, 1, 0],
            &[1, 1, 1, 1, 1, 1, 1],
            &[5, 0, 0, 0, 2, 9, 0, 0],
            &[0, 0, 0],
        ];
        for chunks in tables {
            for base in [0, 1000] {
                let starts = chunk_starts(base, chunks);
                for first in 0..chunks.len() {
                    let slice = &starts[first..];
                    let total: u32 = chunks[first..].iter().sum();
                    // The linear walk: which bucket each chunk belongs to.
                    let mut walked = Vec::new();
                    for (offset, count) in chunks[first..].iter().enumerate() {
                        for chunk in 0..*count {
                            walked.push((u32::try_from(offset).expect("small"), chunk));
                        }
                    }
                    let searched: Vec<(u32, u32)> = (0..total)
                        .map(|workgroup| chunk_bucket(slice, workgroup))
                        .collect();
                    assert_eq!(
                        searched, walked,
                        "{chunks:?} from bucket {first}, base {base}"
                    );
                    let last = u32::try_from(chunks.len() - 1 - first).expect("small");
                    for past in total..total + 3 {
                        let (offset, chunk) = chunk_bucket(slice, past);
                        assert_eq!(
                            offset, last,
                            "{chunks:?} from {first}: padding workgroup {past}"
                        );
                        assert!(
                            chunk >= chunks[chunks.len() - 1],
                            "{chunks:?} from {first}: padding workgroup {past} names a real chunk"
                        );
                    }
                }
            }
        }
    }

    /// **The amplification stage skips the normal cone for a double-sided
    /// instance**, and reads the bit out of the bits the host writes it into.
    ///
    /// Four claims, each a different silence. The constants have to be the
    /// host's, or the stage unpacks the mode out of bits nothing writes and
    /// every instance answers mode zero — which is the tree as it was, with the
    /// back-facing clusters of a double-sided surface dropped from the mesh path
    /// and drawn on every other one. The predicate has to be *called* from the
    /// cone test rather than merely declared, which a constant check cannot see.
    /// It has to be called from exactly one place, because a second call in
    /// front of the frustum loop would keep every cluster of a double-sided
    /// instance whether or not it is on screen. And the amplification stage has
    /// to hand it the instance's own mode rather than a literal.
    ///
    /// `crcbl_render::cull::cone_may_reject` is the Rust twin, and its
    /// `a_double_sided_instance_keeps_the_cluster_that_faces_away` is where the
    /// answers themselves are stated.
    #[test]
    fn the_amplification_stage_skips_the_cone_for_a_double_sided_instance() {
        use crate::mesh::{GpuInstance, GpuMaterial};

        let source = include_str!("../shaders/mesh_cluster.slang");
        for (name, value) in [
            (
                "INSTANCE_MATERIAL_MODE_SHIFT",
                GpuInstance::MATERIAL_MODE_SHIFT,
            ),
            (
                "INSTANCE_MATERIAL_MODE_MASK",
                GpuInstance::MATERIAL_MODE_MASK,
            ),
            ("DOUBLE_SIDED", GpuMaterial::DOUBLE_SIDED),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&declaration),
                "shaders/mesh_cluster.slang must declare `{declaration}`, or the \
                 amplification stage reads the material mode out of bits the host does \
                 not write it into"
            );
        }

        let predicate = concat!(
            "bool cone_may_reject(uint material_mode)\n",
            "{\n",
            "    return (material_mode & DOUBLE_SIDED) == 0u;\n",
            "}\n",
        );
        assert!(
            source.contains(predicate),
            "shaders/mesh_cluster.slang does not carry this exact predicate:\n{predicate}"
        );

        let guard = concat!(
            "    if (cone_may_reject(material_mode) && preserves_angles(basis)",
            " && cluster.cone_cutoff > 0.0\n",
        );
        assert!(
            source.contains(guard),
            "shaders/mesh_cluster.slang does not open the cone test with this line, so a \
             double-sided instance loses its back-facing clusters:\n{guard}"
        );
        assert_eq!(
            source.matches("cone_may_reject(material_mode)").count(),
            1,
            "the predicate is called somewhere other than the cone test — in front of the \
             frustum loop it would keep every cluster of a double-sided instance, on \
             screen or not"
        );
        assert!(
            source.contains(
                "cluster_survives(cluster, instance.transform, instance_material_mode(instance))"
            ),
            "the amplification stage does not hand `cluster_survives` the instance's own \
             material mode, so the predicate answers for whatever it was handed instead"
        );
    }

    mod clusters_check {
        use super::super::*;

        /// One legal cluster over `vertices` mesh vertices and `triangles`
        /// triangles, with both runs starting at zero. Every test below breaks
        /// exactly one thing about this.
        fn sound(vertices: u32, triangles: u32) -> MeshClusters {
            MeshClusters {
                clusters: vec![Meshlet {
                    vertex_offset: 0,
                    vertex_count: vertices,
                    triangle_offset: 0,
                    triangle_count: triangles,
                    bounds: ClusterBounds::default(),
                }],
                vertices: (0..vertices).collect(),
                corners: (0..triangles * 3).map(|c| (c % vertices) as u8).collect(),
            }
        }

        /// **The builder's own output passes.** Without this the whole check
        /// could be rejecting everything and the tests below would not notice —
        /// each of them only asserts that something is refused.
        #[test]
        fn the_cooked_clusters_are_sound() {
            for (what, clusters, vertices) in [
                ("cube", cube_clusters(), crate::mesh::CUBE_VERTEX_COUNT),
                (
                    "pyramid",
                    pyramid_clusters(),
                    crate::mesh::PYRAMID_VERTEX_COUNT,
                ),
            ] {
                assert_eq!(clusters.check(vertices), Ok(()), "{what}");
            }
        }

        #[test]
        fn a_sound_cluster_passes() {
            assert_eq!(sound(8, 4).check(8), Ok(()));
        }

        /// A mesh-relative index past the mesh's own vertices — the fault that
        /// prompted this check, found by a red-check that came back green.
        #[test]
        fn a_run_naming_a_vertex_the_mesh_has_not_got_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.vertices[3] = 8;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::VertexIndex {
                        at: 3,
                        index: 8,
                        vertices: 8
                    }
                })
            );
        }

        #[test]
        fn a_corner_past_its_own_run_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.corners[5] = 8;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::Corner {
                        at: 5,
                        corner: 8,
                        count: 8
                    }
                })
            );
        }

        #[test]
        fn a_vertex_run_off_the_end_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.clusters[0].vertex_count = 9;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::VertexRun {
                        offset: 0,
                        count: 9,
                        len: 8
                    }
                })
            );
        }

        #[test]
        fn a_corner_run_off_the_end_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.clusters[0].triangle_offset = 1;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::CornerRun {
                        offset: 1,
                        count: 12,
                        len: 12
                    }
                })
            );
        }

        /// A run that wraps `u32` rather than running off the end. The device
        /// reads both as `uint`, so an addition done in `u32` would wrap to a
        /// small number and pass.
        #[test]
        fn a_run_that_wraps_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.clusters[0].vertex_offset = u32::MAX - 3;
            clusters.clusters[0].vertex_count = 8;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::VertexRun {
                        offset: u32::MAX - 3,
                        count: 8,
                        len: 8
                    }
                })
            );
        }

        /// The bound the mesh stage's per-cluster arrays are sized by, so
        /// exceeding it is an out-of-bounds write inside the shader rather than
        /// a bad read out of these arrays.
        #[test]
        fn a_cluster_over_the_shader_bounds_is_refused() {
            let mut clusters = sound(8, 4);
            clusters.clusters[0].vertex_count = MAX_CLUSTER_VERTICES as u32 + 1;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::TooManyVertices {
                        count: MAX_CLUSTER_VERTICES as u32 + 1
                    }
                })
            );
            let mut clusters = sound(8, 4);
            clusters.clusters[0].triangle_count = MAX_CLUSTER_TRIANGLES as u32 + 1;
            assert_eq!(
                clusters.check(8),
                Err(ClustersInvalid {
                    cluster: 0,
                    fault: ClusterFault::TooManyTriangles {
                        count: MAX_CLUSTER_TRIANGLES as u32 + 1
                    }
                })
            );
        }

        /// The cluster index is the caller's only way to find the bad one, so a
        /// fault in the second cluster must not be reported as the first's.
        #[test]
        fn the_reported_cluster_is_the_one_that_is_wrong() {
            let mut clusters = sound(8, 4);
            let second = clusters.clusters[0];
            clusters.clusters.push(Meshlet {
                vertex_offset: 8,
                ..second
            });
            clusters.vertices.extend(0..8);
            clusters.vertices[11] = 8;
            assert_eq!(
                clusters.check(8).unwrap_err().cluster,
                1,
                "entry 3 of cluster 1's run is entry 11 of the array"
            );
        }
    }
}
