//! Bucket draw recording and material-mode partitioning for forward passes.

use super::ForwardRenderer;
use crate::draw_gen::GeneratedDraws;
use crcbl_hal::{
    BindGroupHandle, BufferHandle, DeviceCaps, DrawIndirect, DrawIndirectCount, Features,
    GeometryPath, GraphicsPipelineHandle, IndexFormat, PipelineLayoutHandle,
};

/// Which indirect call the forward pass records per bucket.
///
/// Derived from [`GeometryPath`] at build and stored, because the answer cannot
/// change while a device is open and re-deriving it inside the pass body would
/// be a capability query per draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EmitTail {
    /// [`GeometryPath::MeshShader`]: one `draw_mesh_tasks` per bucket, of a
    /// **mesh** pipeline — no vertex stage, no index buffer and no indirect
    /// arguments anywhere in the call. §3.5's primary geometry path; see
    /// `shaders/mesh_cluster.slang`.
    Mesh,
    /// [`GeometryPath::IndirectCount`]: the draw count comes from GPU memory
    /// too, so the CPU never learns whether a bucket drew anything.
    Count,
    /// [`GeometryPath::IndirectPerBatch`]: one `draw_indexed_indirect` per
    /// bucket with a count of one. An empty bucket's argument structure carries
    /// an instance count of zero and draws nothing, which is what makes this the
    /// same picture rather than an approximation of it.
    PerBatch,
}

impl EmitTail {
    /// What the device prefers within its supported geometry paths.
    ///
    /// One value per [`GeometryPath`] since 2026-08: the mesh-shader path used
    /// to degrade to an indirect tail and log that it had, because there was no
    /// mesh pipeline to select.
    pub(super) const fn from_path(path: GeometryPath) -> Self {
        match path {
            GeometryPath::MeshShader => Self::Mesh,
            GeometryPath::IndirectCount => Self::Count,
            GeometryPath::IndirectPerBatch => Self::PerBatch,
        }
    }

    /// Whether this tail draws through a mesh pipeline, which decides the bind
    /// group layout, the pipeline kind and the constant block — everything the
    /// two shapes of this pass differ in.
    pub(super) const fn is_mesh(self) -> bool {
        matches!(self, Self::Mesh)
    }

    /// The most buckets one call may draw on `caps`, or `None` where this tail
    /// records a call per bucket.
    ///
    /// **A range of buckets per call** needs every tail's one argument
    /// structure per bucket to be drawable `draw_count` at a time —
    /// [`Features::MULTI_DRAW_INDIRECT`], which `draw_mesh_tasks_indirect`
    /// needs past one draw exactly as the indexed form does — and each of
    /// those draws to know which bucket it is — [`Features::DRAW_INDEX`], which
    /// `mesh.slang`'s SPIR-V vertex stages and `mesh_cluster.slang`'s SPIR-V
    /// task and mesh stages add to the one bound block's words. Either
    /// missing, and the tail keeps its call per bucket, which draws the same
    /// picture.
    ///
    /// The same rule for all three tails, because what the mesh tail adds is
    /// met by the layout rather than by the device: its extents are one
    /// [`MESH_ARGS_SIZE`](crcbl_shaders::draw_gen::MESH_ARGS_SIZE) structure a
    /// bucket, which is the smallest stride a multi-draw of them may step by.
    ///
    /// **Behind an amplification stage the mesh tail draws no ranges at all**:
    /// each partition is one flat call — see [`BucketDraws::flat`] — which
    /// needs neither feature. [`ForwardRenderer::partitions`] is what chooses.
    pub(super) fn range_limit(self, caps: &DeviceCaps) -> Option<u32> {
        let needs = Features::MULTI_DRAW_INDIRECT | Features::DRAW_INDEX;
        (caps.features.contains(needs) && caps.limits.max_draw_indirect_count > 1)
            .then_some(caps.limits.max_draw_indirect_count)
    }
}

/// One multi-draw call: `count` consecutive buckets, starting at the bucket of
/// [`BucketDraws::calls`] element `first`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DrawRange {
    pub(super) first: usize,
    pub(super) count: u32,
}

impl DrawRange {
    /// `buckets` — the bucket index of each call, in call order — packed into
    /// runs of consecutive buckets of at most `limit` each.
    ///
    /// **Consecutive buckets and nothing looser**, because that is what one
    /// call can stand for: its draws step through the argument structures —
    /// or the mesh tail's extents — one stride at a time and its geometry
    /// stages through their per-bucket words one word at a time, and both are
    /// laid out in bucket order. A bucket of
    /// another partition between two of this one's ends the range, and so does
    /// the device's own ceiling on draws per call.
    ///
    /// A range of one bucket records the call a list with no ranges records
    /// for that bucket.
    pub(super) fn pack(buckets: impl IntoIterator<Item = u32>, limit: u32) -> Vec<Self> {
        let mut ranges: Vec<Self> = Vec::new();
        let mut last = None;
        for (index, bucket) in buckets.into_iter().enumerate() {
            match ranges.last_mut() {
                Some(range) if last == bucket.checked_sub(1) && range.count < limit => {
                    range.count += 1;
                }
                _ => ranges.push(Self {
                    first: index,
                    count: 1,
                }),
            }
            last = Some(bucket);
        }
        ranges
    }
}

/// Everything a geometry pass needs to record one indirect call per bucket.
///
/// **One description for the three passes that record them**: the depth prepass,
/// the forward pass and each shadow view. They differ in the pipeline and in
/// which bind group and which cull's arguments they draw from, and in nothing
/// else — so the emit tail, the index-buffer bind and the per-bucket loop are one
/// piece of code rather than three that agree today. The shadow pass gained a
/// viewport per tile around this; the prepass gained nothing at all, which is the
/// point of it being the colour pass's twin.
#[derive(Clone)]
pub(super) struct BucketDraws {
    pub(super) pipeline: GraphicsPipelineHandle,
    pub(super) layout: PipelineLayoutHandle,
    /// The whole index pool, bound at offset zero — see [`BucketDraws::record`].
    pub(super) indices: BufferHandle,
    pub(super) emit: EmitTail,
    /// Per bucket: the dynamic offset of its constant block, and the offsets of
    /// its argument structure, its count word and its dispatch extents — in
    /// draw region 0.
    pub(super) calls: Vec<(u32, u64, u64, u64)>,
    /// How far one draw region moves each of those four: the constant blocks by
    /// a bucket count of strides, the arguments by a region of structures, and
    /// the counts and extents — which share a buffer, a region apart — by one
    /// region of both. See [`crcbl_shaders::draw_gen::DRAW_REGIONS`].
    pub(super) region_step: RegionStep,
    /// `Some` where a call draws a range of consecutive buckets rather than one
    /// — [`EmitTail::range_limit`] — with the ranges [`calls`](Self::calls)
    /// packs into. `None` records a call per bucket, which is also the correct
    /// answer for any list, so a list that was never packed draws right.
    pub(super) ranges: Option<Vec<DrawRange>>,
    /// `Some` where this list is one **flat task call** — the mesh tail behind
    /// an amplification stage — with the
    /// [flat segment](crcbl_shaders::draw_gen::flat_segment) whose dispatch it
    /// reads. Takes precedence over [`ranges`](Self::ranges).
    ///
    /// One `draw_mesh_tasks_indirect` of one structure, which
    /// `draw_gen.slang` sized to every task chunk of the segment's buckets,
    /// under the first bucket's block: `mesh_cluster.slang`'s `taskMain` finds
    /// each workgroup's bucket by searching the chunk starts that block names.
    /// The list's buckets must be exactly the segment's run, which
    /// [`ForwardRenderer::partitions`] checks.
    pub(super) flat: Option<u32>,
}

/// [`BucketDraws::region_step`]: what one draw region adds to a call's offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RegionStep {
    pub(super) constants: u32,
    pub(super) args: u64,
    pub(super) counts: u64,
}

impl BucketDraws {
    /// Binds the pipeline and, unless this is a mesh pipeline, the index pool.
    ///
    /// Recorded once per pass; [`BucketDraws::record`] is once per bind group.
    pub(super) fn open(&self, encoder: &mut dyn crcbl_hal::CommandEncoder) {
        encoder.bind_graphics_pipeline(self.pipeline);
        if !self.emit.is_mesh() {
            // The index pool is bound whole, at offset zero, for every mesh in
            // it: the mesh's place is the draw's first index and its table entry,
            // not a buffer offset. That is what makes one bind enough for the
            // scene P7 puts in here.
            //
            // A mesh pipeline has no index buffer at all — the corner triples
            // come out of the cluster records — so binding one would be a bind no
            // stage could read.
            encoder.bind_index_buffer(self.indices, 0, IndexFormat::Uint32);
        }
    }

    /// How many calls [`record`](Self::record) records for this list: one per
    /// range where it has them, one per bucket where it does not.
    pub(super) fn call_count(&self) -> u64 {
        if self.flat.is_some() {
            return 1;
        }
        self.ranges.as_ref().map_or(self.calls.len(), Vec::len) as u64
    }

    /// Records this list's calls, drawing `group`'s view of `draws`.
    ///
    /// One call per bucket, or one per range of buckets where
    /// [`ranges`](Self::ranges) is set — either way the number the CPU records
    /// does not depend on what is in the scene, which is the whole of what
    /// topic 03 §3.3 asks for. An empty bucket's arguments carry an instance
    /// count of zero, and a draw of a range with nothing in it draws nothing.
    pub(super) fn record(
        &self,
        encoder: &mut dyn crcbl_hal::CommandEncoder,
        group: BindGroupHandle,
        draws: &GeneratedDraws,
    ) {
        self.record_region(encoder, group, draws, 0);
    }

    /// [`record`](Self::record) for draw region `region` of `draws`: the same
    /// buckets, each call's constant block, arguments, count and extents moved
    /// `region` regions along — which is what makes the call read that region's
    /// run start and instance count.
    pub(super) fn record_region(
        &self,
        encoder: &mut dyn crcbl_hal::CommandEncoder,
        group: BindGroupHandle,
        draws: &GeneratedDraws,
        region: u32,
    ) {
        let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u32;
        let mesh_stride = crcbl_shaders::draw_gen::MESH_ARGS_SIZE as u32;
        let step = self.region_step;
        if let (Some(segment), Some((constant_offset, ..))) = (self.flat, self.calls.first()) {
            // The segment's first bucket's block in this region: its chunk
            // starts are where `taskMain`'s search begins, and every other
            // bucket's words are the ones after its own.
            encoder.bind_group(
                0,
                group,
                &[constant_offset + region * step.constants],
                self.layout,
            );
            // One structure, sized on the GPU to every chunk of the segment,
            // which an empty segment leaves as a dispatch of no workgroups.
            let flat = u64::from(region * crcbl_shaders::draw_gen::FLAT_SEGMENTS + segment);
            encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                args: draws.counts,
                offset: draws.flat_args_offset + flat * u64::from(mesh_stride),
                draw_count: 1,
                stride: mesh_stride,
            });
            return;
        }
        if let Some(ranges) = &self.ranges {
            for range in ranges {
                let (constant_offset, args_offset, _, mesh_args_offset) = self.calls[range.first];
                // The range's first bucket's block, and nothing else bound for
                // the rest of it: draw `d` of the call reads the words `d` past
                // the ones this block names — `mesh.slang`'s and
                // `mesh_cluster.slang`'s "One call for a range of buckets".
                encoder.bind_group(
                    0,
                    group,
                    &[constant_offset + region * step.constants],
                    self.layout,
                );
                if self.emit.is_mesh() {
                    // Every bucket's extents of the range, one structure a
                    // bucket: an empty bucket's are a legal dispatch of no
                    // workgroups, exactly as in the call per bucket below.
                    encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                        args: draws.counts,
                        offset: mesh_args_offset + u64::from(region) * step.counts,
                        draw_count: range.count,
                        stride: mesh_stride,
                    });
                } else {
                    // Every structure of the range, read unconditionally, on
                    // `PerBatch`'s terms below: an instance count of zero draws
                    // nothing, so no count word is needed to skip one.
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args: draws.args,
                        offset: args_offset + u64::from(region) * step.args,
                        draw_count: range.count,
                        stride,
                    });
                }
            }
            return;
        }
        for &(constant_offset, args_offset, count_offset, mesh_args_offset) in &self.calls {
            let constant_offset = &(constant_offset + region * step.constants);
            let args_offset = &(args_offset + u64::from(region) * step.args);
            let count_offset = &(count_offset + u64::from(region) * step.counts);
            let mesh_args_offset = &(mesh_args_offset + u64::from(region) * step.counts);
            // The block written at build for this bucket: which word holds where
            // its run of surviving instances starts this frame. `SV_InstanceID`
            // walks the run from that start, each entry names an instance, the
            // instance names its mesh, and the mesh table says where that mesh's
            // vertices start — none of which the draw call carries. The mesh
            // path's block says the same and three things more; see
            // `meshlet::ClusterDrawConstants`.
            encoder.bind_group(0, group, &[*constant_offset], self.layout);
            match self.emit {
                EmitTail::Mesh => {
                    // One workgroup per (cluster, **surviving** instance), or
                    // behind the task stage one per chunk of those pairs, and
                    // neither extent is the CPU's: they are the three words the
                    // draw-argument pass wrote for this bucket.
                    //
                    // That is the whole difference between culling that skips
                    // output and culling that skips work. A dispatch sized here
                    // would have to cover every slot the instance pool ever handed
                    // out — a removed instance leaves a hole and the live ones
                    // above it stay in the array — and launch a workgroup for
                    // each, which then reads the survivor count and returns.
                    //
                    // Recorded unconditionally, unlike a CPU-sized dispatch: an
                    // extent of zero is a legal indirect dispatch of no
                    // workgroups, so an empty scene needs no branch here and the
                    // recorded stream stays the same whatever the scene holds.
                    encoder.draw_mesh_tasks_indirect(&DrawIndirect {
                        // The buffer the per-bucket draw counts are also in —
                        // the extents follow them, which is what
                        // `DrawGen::mesh_args_offset` accounts for. See
                        // [`GeneratedDraws::counts`].
                        args: draws.counts,
                        offset: *mesh_args_offset,
                        draw_count: 1,
                        stride: mesh_stride,
                    });
                }
                EmitTail::Count => {
                    encoder.draw_indexed_indirect_count(&DrawIndirectCount {
                        args: draws.args,
                        args_offset: *args_offset,
                        count_buffer: draws.counts,
                        count_offset: *count_offset,
                        // One argument structure per bucket, so this is the
                        // ceiling rather than a guess: the count in the buffer is
                        // zero or one and the GPU decides which.
                        max_draw_count: 1,
                        stride,
                    });
                }
                EmitTail::PerBatch => {
                    encoder.draw_indexed_indirect(&DrawIndirect {
                        args: draws.args,
                        offset: *args_offset,
                        // Read the bucket's one structure unconditionally — a
                        // device without a GPU-side count cannot ask whether there
                        // is anything in it, and an instance count of zero draws
                        // nothing anyway. That is why the two paths are the same
                        // picture and not an approximation of each other.
                        draw_count: 1,
                        stride,
                    });
                }
            }
        }
    }
}

impl ForwardRenderer {
    /// `draws` split by the `key` bits of each bucket's material mode, each
    /// partition under the pipeline `pipelines` pairs that value with.
    ///
    /// **The point of the whole arrangement.** A bucket's mode is fixed when the
    /// table is built and an instance is scattered into the bucket matching its
    /// mesh *and* its own mode, so a pass can bind a pipeline per mode instead
    /// of choosing one for the whole frame — a fragment stage for the mesh that
    /// masks, back-face culling off for the mesh that is double-sided, and
    /// neither for anything else.
    ///
    /// **An empty partition is dropped**, which is the zero-cost half: a scene
    /// whose materials are all one mode records exactly one partition under
    /// exactly the pipeline it always bound — same binds, same draws, same
    /// order, byte for byte the stream every golden in this tree was blessed
    /// against.
    ///
    /// `pipelines` must cover every value of `mode & key` a bucket can carry, or
    /// that bucket's draws are dropped from the pass entirely — the two callers
    /// below are what make that total, and both are exhaustive over
    /// [`super::DEPTH_MODES`] masked by their own `key`.
    ///
    /// `draws` is the whole table, a call per bucket in bucket order. Where the
    /// renderer draws a range of buckets per call — [`EmitTail::range_limit`] —
    /// each partition's buckets are packed into [`DrawRange`]s here, which is
    /// where a bucket of another mode between two of this partition's splits a
    /// range.
    ///
    /// **Behind an amplification stage each partition is one flat call
    /// instead** — [`BucketDraws::flat`] — whatever the device's draw index,
    /// since the call names its buckets through the chunk starts rather than
    /// through a draw index. That needs the partition's buckets to be one
    /// contiguous run — then it is the run
    /// [`flat_segments`](crcbl_shaders::draw_gen::flat_segments) laid down for
    /// its segment from the same modes — which the mode-major bucket table
    /// makes true; a partition that is not is a table the flat call cannot
    /// draw, and a panic rather than a frame missing its buckets.
    pub(super) fn partitions(
        &self,
        draws: &BucketDraws,
        key: u32,
        pipelines: &[(u32, GraphicsPipelineHandle)],
    ) -> Vec<BucketDraws> {
        pipelines
            .iter()
            .filter_map(|(mode, pipeline)| {
                let (buckets, calls): (Vec<u32>, Vec<_>) = draws
                    .calls
                    .iter()
                    .zip(&self.bucket_modes)
                    .enumerate()
                    .filter(|(_, (_, bucket))| (**bucket & key) == *mode)
                    .map(|(bucket, (call, _))| {
                        let bucket = u32::try_from(bucket)
                            .unwrap_or_else(|_| unreachable!("a table of u32-indexed buckets"));
                        (bucket, *call)
                    })
                    .unzip();
                if calls.is_empty() {
                    return None;
                }
                let flat = (draws.emit.is_mesh() && self.culls_clusters).then(|| {
                    let segment =
                        crcbl_shaders::draw_gen::flat_segment(key, *mode).unwrap_or_else(|| {
                            unreachable!("a pass splits by every mode bit or by side")
                        });
                    // Every bucket of this key and value, so one run of them
                    // is exactly the run `flat_segments` laid down for the
                    // segment from the same modes.
                    assert!(
                        buckets.windows(2).all(|pair| pair[1] == pair[0] + 1),
                        "the flat task call draws one run of buckets, and this partition's are \
                         {buckets:?}: the bucket table is not numbered mode-major"
                    );
                    segment
                });
                Some(BucketDraws {
                    pipeline: *pipeline,
                    layout: draws.layout,
                    indices: draws.indices,
                    emit: draws.emit,
                    calls,
                    region_step: draws.region_step,
                    ranges: if flat.is_some() {
                        None
                    } else {
                        self.range_limit
                            .map(|limit| DrawRange::pack(buckets, limit))
                    },
                    flat,
                })
            })
            .collect()
    }
}

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::scene;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{AdapterId, DeviceDesc, Format, Instance, QueueKind};
    use crcbl_shaders::mesh::GpuMaterial;

    #[test]
    fn partitions_preserve_all_metadata_and_order_for_every_tail() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .unwrap();
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut renderer = ForwardRenderer::with_scene(
            device.as_ref(),
            queue,
            Format::Rgba8UnormSrgb,
            &scene::demo(),
        )
        .unwrap();
        let masked = GpuMaterial::ALPHA_MODE_MASK;
        let double = GpuMaterial::DOUBLE_SIDED;
        let both = GpuMaterial::MODE_MASK;
        let unused = both + 1;
        renderer.bucket_modes = vec![0, masked, double, both, masked, 0, both];
        let calls = [
            (17, 29, 41, 53),
            (71, 83, 97, 109),
            (127, 139, 151, 163),
            (181, 193, 211, 223),
            (241, 257, 269, 281),
            (307, 331, 347, 359),
            (373, 389, 401, 419),
        ];
        let opaque_pipeline = renderer.shadow_pipeline.single;
        let masked_pipeline = renderer.depth_masked_pipeline.single;
        let double_pipeline = renderer.shadow_pipeline.double;
        let both_pipeline = renderer.depth_masked_pipeline.double;
        let unused_pipeline = renderer.tonemap_pipeline;
        let cases = [
            (
                both,
                vec![
                    (both, both_pipeline),
                    (unused, unused_pipeline),
                    (0, opaque_pipeline),
                    (double, double_pipeline),
                    (masked, masked_pipeline),
                ],
                vec![
                    (both_pipeline, vec![3, 6], vec![(0, 1), (1, 1)]),
                    (opaque_pipeline, vec![0, 5], vec![(0, 1), (1, 1)]),
                    (double_pipeline, vec![2], vec![(0, 1)]),
                    (masked_pipeline, vec![1, 4], vec![(0, 1), (1, 1)]),
                ],
            ),
            (
                double,
                vec![
                    (double, double_pipeline),
                    (unused, unused_pipeline),
                    (0, opaque_pipeline),
                ],
                vec![
                    (double_pipeline, vec![2, 3, 6], vec![(0, 2), (2, 1)]),
                    (opaque_pipeline, vec![0, 1, 4, 5], vec![(0, 2), (2, 2)]),
                ],
            ),
            (
                0,
                vec![(unused, unused_pipeline), (0, opaque_pipeline)],
                vec![(
                    opaque_pipeline,
                    vec![0, 1, 2, 3, 4, 5, 6],
                    // Seven consecutive buckets, three to a call.
                    vec![(0, 3), (3, 3), (6, 1)],
                )],
            ),
        ];
        // The default request grants no draw index, so nothing is packed until
        // the limit is set below.
        assert_eq!(renderer.range_limit, None);
        for (limit, emit) in [None, Some(3)].into_iter().flat_map(|limit| {
            [EmitTail::Count, EmitTail::PerBatch, EmitTail::Mesh].map(|emit| (limit, emit))
        }) {
            renderer.range_limit = limit;
            let source = BucketDraws {
                pipeline: renderer.mesh_pipeline.single,
                layout: renderer.tonemap_pipeline_layout,
                indices: renderer.pool.index_buffer(),
                emit,
                calls: calls.to_vec(),
                region_step: RegionStep {
                    constants: 256,
                    args: 1024,
                    counts: 64,
                },
                ranges: None,
                flat: None,
            };
            assert_ne!(source.layout, renderer.mesh_pipeline_layout);
            for (key, pipelines, expected) in &cases {
                let partitions = renderer.partitions(&source, *key, pipelines);
                assert_eq!(partitions.len(), expected.len());
                for (partition, (pipeline, selected, ranges)) in partitions.iter().zip(expected) {
                    assert_eq!(partition.pipeline, *pipeline);
                    assert_eq!(partition.layout, source.layout);
                    assert_eq!(partition.indices, source.indices);
                    assert_eq!(partition.emit, emit);
                    assert_eq!(partition.region_step, source.region_step);
                    assert_eq!(
                        partition.calls,
                        selected
                            .iter()
                            .map(|&index| calls[index])
                            .collect::<Vec<_>>()
                    );
                    // Packed wherever a limit is set, on every tail.
                    let packed = limit.map(|_| {
                        ranges
                            .iter()
                            .map(|&(first, count)| DrawRange { first, count })
                            .collect::<Vec<_>>()
                    });
                    assert_eq!(partition.ranges, packed, "{emit:?} at {limit:?}");
                    assert_eq!(
                        partition.call_count(),
                        packed.as_ref().map_or(selected.len(), Vec::len) as u64
                    );
                }
                assert_eq!(source.calls, calls);
            }
        }
        renderer.bucket_modes.clear();
        let empty = BucketDraws {
            pipeline: opaque_pipeline,
            layout: renderer.mesh_pipeline_layout,
            indices: renderer.pool.index_buffer(),
            emit: EmitTail::Count,
            calls: Vec::new(),
            region_step: RegionStep::default(),
            ranges: None,
            flat: None,
        };
        assert!(renderer.partitions(&empty, both, &cases[0].1).is_empty());
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// **Consecutive buckets pack into one call, and anything else splits
    /// it**: a gap where another partition's bucket stands, and the device's
    /// ceiling on draws per call. Each range starts at the call index of its
    /// first bucket, which is what [`BucketDraws::record`] binds the block of.
    #[test]
    fn ranges_pack_consecutive_buckets_and_split_at_gaps_and_the_limit() {
        let ranges = |buckets: &[u32], limit| {
            DrawRange::pack(buckets.iter().copied(), limit)
                .into_iter()
                .map(|range| (range.first, range.count))
                .collect::<Vec<_>>()
        };
        assert_eq!(ranges(&[], 8), []);
        assert_eq!(ranges(&[0, 1, 2, 3], u32::MAX), [(0, 4)]);
        assert_eq!(
            ranges(&[0, 1, 2, 4, 5, 7], 8),
            [(0, 3), (3, 2), (5, 1)],
            "a bucket missing from the list ends the range before it"
        );
        assert_eq!(
            ranges(&[0, 1, 2, 3, 4], 2),
            [(0, 2), (2, 2), (4, 1)],
            "and the limit ends a range of consecutive buckets"
        );
        assert_eq!(
            ranges(&[5, 6, 1, 2], 8),
            [(0, 2), (2, 2)],
            "a bucket before the last one starts a range of its own"
        );
        assert_eq!(ranges(&[3, 3], 8), [(0, 1), (1, 1)], "and so does a repeat");
        assert_eq!(ranges(&[0, 1, 2], 1), [(0, 1), (1, 1), (2, 1)]);
    }

    /// **Behind an amplification stage every partition of the mesh tail is one
    /// flat call of its segment**, whatever the device's draw index: the
    /// depth split's four modes are segments 0 to 3, the colour split's two
    /// sides segments 4 and 5, each list keeps its buckets' calls in order for
    /// the first bucket's block, and no ranges are packed. The indirect tails
    /// and a mesh tail without the stage keep their calls per bucket or per
    /// range.
    #[test]
    fn a_partition_behind_a_task_stage_is_one_flat_call_of_its_segment() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .unwrap();
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut renderer = ForwardRenderer::with_scene(
            device.as_ref(),
            queue,
            Format::Rgba8UnormSrgb,
            &scene::demo(),
        )
        .unwrap();
        let masked = GpuMaterial::ALPHA_MODE_MASK;
        let double = GpuMaterial::DOUBLE_SIDED;
        let both = GpuMaterial::MODE_MASK;
        // Mode-major, as the table builder numbers them.
        renderer.bucket_modes = vec![0, 0, masked, masked, double, both, both];
        let calls: Vec<(u32, u64, u64, u64)> = (0..7u32)
            .map(|bucket| (bucket * 256, u64::from(bucket) * 20, 0, 0))
            .collect();
        let depth = [
            (0, renderer.shadow_pipeline.single),
            (masked, renderer.depth_masked_pipeline.single),
            (double, renderer.shadow_pipeline.double),
            (both, renderer.depth_masked_pipeline.double),
        ];
        let sided = [
            (0, renderer.mesh_pipeline.single),
            (double, renderer.mesh_pipeline.double),
        ];
        for (emit, culls_clusters, limit) in [
            (EmitTail::Mesh, true, None),
            (EmitTail::Mesh, true, Some(4096)),
            (EmitTail::Mesh, false, Some(4096)),
            (EmitTail::Count, true, Some(4096)),
        ] {
            renderer.culls_clusters = culls_clusters;
            renderer.range_limit = limit;
            let case = format!("{emit:?}, task stage {culls_clusters}, limit {limit:?}");
            let source = BucketDraws {
                pipeline: renderer.mesh_pipeline.single,
                layout: renderer.mesh_pipeline_layout,
                indices: renderer.pool.index_buffer(),
                emit,
                calls: calls.clone(),
                region_step: RegionStep::default(),
                ranges: None,
                flat: None,
            };
            let flat = emit.is_mesh() && culls_clusters;
            for (key, pipelines, expected) in [
                (
                    both,
                    &depth[..],
                    vec![
                        (0, vec![0, 1]),
                        (1, vec![2, 3]),
                        (2, vec![4]),
                        (3, vec![5, 6]),
                    ],
                ),
                (
                    double,
                    &sided[..],
                    vec![(4, vec![0, 1, 2, 3]), (5, vec![4, 5, 6])],
                ),
            ] {
                let partitions = renderer.partitions(&source, key, pipelines);
                assert_eq!(partitions.len(), expected.len(), "{case}");
                for (partition, (segment, buckets)) in partitions.iter().zip(expected) {
                    assert_eq!(
                        partition.calls,
                        buckets
                            .iter()
                            .map(|&bucket| calls[bucket])
                            .collect::<Vec<_>>(),
                        "{case}: the partition's calls, first bucket first"
                    );
                    if flat {
                        assert_eq!(partition.flat, Some(segment), "{case}");
                        assert_eq!(partition.ranges, None, "{case}: a flat call packs no range");
                        assert_eq!(partition.call_count(), 1, "{case}");
                    } else {
                        assert_eq!(partition.flat, None, "{case}");
                        assert_eq!(
                            partition.call_count(),
                            1,
                            "{case}: one consecutive run is one ranged call"
                        );
                        assert!(partition.ranges.is_some(), "{case}");
                    }
                }
            }
        }
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A flat call cannot stand for a partition whose buckets are not one
    /// run**, so a table that is not numbered mode-major is a panic behind a
    /// task stage rather than a frame drawing another mode's buckets under
    /// this pipeline.
    #[test]
    #[should_panic(expected = "the bucket table is not numbered mode-major")]
    fn a_partition_that_is_not_one_run_is_refused_behind_a_task_stage() {
        let instance = NullInstance::gpu_driven();
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .unwrap();
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut renderer = ForwardRenderer::with_scene(
            device.as_ref(),
            queue,
            Format::Rgba8UnormSrgb,
            &scene::demo(),
        )
        .unwrap();
        renderer.bucket_modes = vec![0, GpuMaterial::ALPHA_MODE_MASK, 0];
        renderer.culls_clusters = true;
        let source = BucketDraws {
            pipeline: renderer.mesh_pipeline.single,
            layout: renderer.mesh_pipeline_layout,
            indices: renderer.pool.index_buffer(),
            emit: EmitTail::Mesh,
            calls: vec![(0, 0, 0, 0); 3],
            region_step: RegionStep::default(),
            ranges: None,
            flat: None,
        };
        let opaque = renderer.shadow_pipeline.single;
        let _ = renderer.partitions(&source, GpuMaterial::MODE_MASK, &[(0, opaque)]);
    }

    /// **Which tail draws a range of buckets per call, from the capabilities
    /// alone**: every tail, the mesh one included, where the device has a draw
    /// index and multi-draw and lets a call draw more than one; none without.
    #[test]
    fn a_tail_draws_ranges_only_with_a_draw_index_and_multi_draw() {
        let caps = |features: Features, max_draw_indirect_count: u32| DeviceCaps {
            features,
            limits: crcbl_hal::Limits {
                max_draw_indirect_count,
                ..crcbl_hal::Limits::desktop()
            },
        };
        let both = Features::MULTI_DRAW_INDIRECT | Features::DRAW_INDEX;
        for emit in [EmitTail::Count, EmitTail::PerBatch, EmitTail::Mesh] {
            assert_eq!(emit.range_limit(&caps(both, 4096)), Some(4096), "{emit:?}");
            assert_eq!(
                emit.range_limit(&caps(Features::GPU_DRIVEN | Features::DRAW_INDEX, 9)),
                Some(9),
                "{emit:?}"
            );
            for lacking in [
                Features::MULTI_DRAW_INDIRECT,
                Features::DRAW_INDEX,
                Features::GPU_DRIVEN,
                Features::empty(),
            ] {
                assert_eq!(
                    emit.range_limit(&caps(lacking, 4096)),
                    None,
                    "{emit:?} {lacking:?}"
                );
            }
            assert_eq!(
                emit.range_limit(&caps(both, 1)),
                None,
                "one draw a call is no range"
            );
        }
        // A mesh device with everything else still needs both halves.
        let mesh = Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER;
        assert_eq!(EmitTail::Mesh.range_limit(&caps(mesh, 4096)), None);
        assert_eq!(
            EmitTail::Mesh.range_limit(&caps(mesh | Features::DRAW_INDEX, 4096)),
            Some(4096)
        );
    }
}
