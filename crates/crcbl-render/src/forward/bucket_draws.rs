//! Bucket draw recording and material-mode partitioning for forward passes.

use super::ForwardRenderer;
use crate::draw_gen::GeneratedDraws;
use crcbl_hal::{
    BindGroupHandle, BufferHandle, DrawIndirect, DrawIndirectCount, GeometryPath,
    GraphicsPipelineHandle, IndexFormat, PipelineLayoutHandle,
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

    /// Records one call per bucket, drawing `group`'s view of `draws`.
    ///
    /// One call per bucket **always** — the number the CPU records does not depend
    /// on what is in the scene, which is the whole of what topic 03 §3.3 asks for.
    /// An empty bucket's arguments carry an instance count of zero.
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
                    // One workgroup per (cluster, **surviving** instance), and
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
    pub(super) fn partitions(
        &self,
        draws: &BucketDraws,
        key: u32,
        pipelines: &[(u32, GraphicsPipelineHandle)],
    ) -> Vec<BucketDraws> {
        pipelines
            .iter()
            .filter_map(|(mode, pipeline)| {
                let calls: Vec<(u32, u64, u64, u64)> = draws
                    .calls
                    .iter()
                    .zip(&self.bucket_modes)
                    .filter(|(_, bucket)| (**bucket & key) == *mode)
                    .map(|(call, _)| *call)
                    .collect();
                (!calls.is_empty()).then_some(BucketDraws {
                    pipeline: *pipeline,
                    layout: draws.layout,
                    indices: draws.indices,
                    emit: draws.emit,
                    calls,
                    region_step: draws.region_step,
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
                    (both_pipeline, vec![3, 6]),
                    (opaque_pipeline, vec![0, 5]),
                    (double_pipeline, vec![2]),
                    (masked_pipeline, vec![1, 4]),
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
                    (double_pipeline, vec![2, 3, 6]),
                    (opaque_pipeline, vec![0, 1, 4, 5]),
                ],
            ),
            (
                0,
                vec![(unused, unused_pipeline), (0, opaque_pipeline)],
                vec![(opaque_pipeline, vec![0, 1, 2, 3, 4, 5, 6])],
            ),
        ];
        for emit in [EmitTail::Count, EmitTail::PerBatch, EmitTail::Mesh] {
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
            };
            assert_ne!(source.layout, renderer.mesh_pipeline_layout);
            for (key, pipelines, expected) in &cases {
                let partitions = renderer.partitions(&source, *key, pipelines);
                assert_eq!(partitions.len(), expected.len());
                for (partition, (pipeline, selected)) in partitions.iter().zip(expected) {
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
        };
        assert!(renderer.partitions(&empty, both, &cases[0].1).is_empty());
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }
}
