//! The compute pass that reads the reflective shadow map into every probe row.
//!
//! ```text
//!  crate::rsm's three targets ─┐
//!  the captured visibility maps ┼─▶ probe_gather.slang ─▶ crate::probe's rows
//!  ProbeVolume::position, as data ┘   one workgroup per probe
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster updater, second half.
//! [`crate::rsm`] is the pass that draws the map and
//! [`crcbl_shaders::probe_gather`] owns the parameter block and the two numbers
//! this side and the shader have to agree on.
//!
//! # A layout of its own, which is where the writable rows belong
//!
//! The mesh layout binds the probe table **read-only** and is at WebGPU's
//! guaranteed storage-buffer ceiling with no headroom — `crate::forward`'s
//! `PROBE_TABLE_BINDING` carries both facts and names this pass as where the
//! writable binding goes. So this is a bind group layout of its own, four
//! entries of which are images and two of which are buffers, and the table's
//! device-local memory is what makes the write legal at all — see [`crate::probe`].
//!
//! # The probe positions arrive as data
//!
//! `probe_octahedral.slang`'s rule exactly:
//! [`ProbeVolume::position`](crcbl_shaders::probe::ProbeVolume::position) owns
//! the clipmap arithmetic that turns a level and a cell into a place, so the
//! host evaluates it once for every row and the shader reads a table. A second
//! transcription of it in Slang would be a second thing that can drift from the
//! volume it has to agree with, and `a_position_table_walks_the_volume_in_row_order`
//! is what holds this one to the module.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ComputePipelineHandle, Device, HalError, ImageViewHandle, ImageViewType,
    MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, ResourceState, SampleType,
    ShaderStages, check_portable_storage_buffers,
};
use crcbl_shaders::probe::ProbeVolume;
use crcbl_shaders::probe_gather::{GATHER_PARAMS_SIZE, GATHER_WORKGROUP_SIZE, GatherParams};

use crate::draw_gen::{bound, compute_pipeline, storage, uniform};
use crate::graph::{BufferId, ImageId, RenderGraph};
use crate::ssao::cached_group;

/// Bytes one probe's position occupies in the table the shader reads: a
/// `float4`, because that is what a `StructuredBuffer<float4>` element is.
const POSITION_STRIDE: u64 = 16;

/// How large a [`ProbeGather`] is and what it writes into.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProbeGatherDesc<'a> {
    /// Debug name; every resource is named after it.
    pub(crate) label: Option<&'a str>,
    /// The volume whose rows this fills — read for its probe count and for
    /// where each of those probes stands.
    pub(crate) volume: ProbeVolume,
    /// The probe table's ring, one buffer per frame in flight, in the order
    /// [`ProbeTable::buffer`](crate::probe::ProbeTable::buffer) hands them out.
    /// Its length is the ring this builds over.
    pub(crate) probes: &'a [BufferHandle],
}

/// The gather's pipeline, its per-frame parameter blocks and the probe-position
/// table it reads.
#[derive(Debug)]
pub(crate) struct ProbeGather {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: ComputePipelineHandle,
    /// One parameter block per frame in flight, host-uploaded and rewritten
    /// every frame — the sun moves, and so does the cascade the map covers.
    params: Vec<BufferHandle>,
    /// This frame's rows, in the ring's own order.
    probes: Vec<BufferHandle>,
    /// Where each probe stands, evaluated once from the volume.
    positions: BufferHandle,
    /// One cached bind group per frame — see [`cached_group`] for why a group
    /// naming a graph transient cannot be built where the others are.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// Rows the volume holds, which is also the workgroup count.
    rows: u32,
}

/// The three targets [`crate::rsm`]'s pass wrote, as the graph knows them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RsmImages {
    /// `RsmOutput::albedo`.
    pub(crate) albedo: ImageId,
    /// `RsmOutput::normal`.
    pub(crate) normal: ImageId,
    /// `RsmOutput::world`.
    pub(crate) world: ImageId,
}

impl ProbeGather {
    /// One pass, which is what [`crate::forward::ForwardRenderer::MAX_PASSES`]
    /// counts it as.
    pub(crate) const PASSES: u32 = 1;

    /// Builds the pipeline, the parameter ring and the probe-position table.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for a ring of no frames or a volume with
    /// no probes — neither is a gather, and both would reach the device as a
    /// zero-sized buffer or an empty dispatch. [`HalError`] from any seam call;
    /// a failure part-way through releases what it had already created.
    pub(crate) fn new(device: &dyn Device, desc: &ProbeGatherDesc<'_>) -> Result<Self, HalError> {
        let rows = desc.volume.total();
        if desc.probes.is_empty() || rows == 0 {
            return Err(HalError::InvalidDescriptor(
                "a probe gather needs at least one frame in flight and one probe".to_string(),
            ));
        }
        let label = desc.label.unwrap_or("probe gather");
        let mut rollback = Rollback::default();
        let result = Self::build(device, desc, label, rows, &mut rollback);
        if result.is_err() {
            rollback.run(device);
        }
        result
    }

    fn build(
        device: &dyn Device,
        desc: &ProbeGatherDesc<'_>,
        label: &str,
        rows: u32,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        // **`probe_gather.slang`'s declaration order**, which is the only order
        // Metal and D3D12 agree about — see `crcbl_shaders::declaration_order`.
        //
        // Every image is `UnfilterableFloat`: the shader `Load`s all four and
        // binds no sampler, WebGPU checks the layout against the *view's* format
        // rather than against how the shader reads it, and two of these formats
        // — `Rgba32Float` and the maps' `Rg32Float` — are unfilterable without a
        // device feature. Declaring the filterable pair as unfilterable is
        // allowed and is the honest description of what this pass does with
        // them; the other way round is what made every 3D demo draw black on
        // Chromium, and `docs/plan/50-irradiance-probes.md` records it.
        let sampled = |binding: u32, view_type: ImageViewType| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::SampledImage {
                view_type,
                sample_type: SampleType::UnfilterableFloat,
            },
            count: 1,
            flags: BindingFlags::empty(),
        };
        let entries = [
            uniform(0),
            storage(1, true),
            sampled(2, ImageViewType::D2Array),
            sampled(3, ImageViewType::D2),
            sampled(4, ImageViewType::D2),
            sampled(5, ImageViewType::D2),
            storage(6, false),
        ];
        let layout_desc = BindGroupLayoutDesc {
            label: Some(label),
            entries: &entries,
        };
        check_portable_storage_buffers(Some(label), &[&layout_desc])?;
        let layout = device.create_bind_group_layout(&layout_desc)?;
        rollback.layouts.push(layout);
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some(label),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);
        let pipeline = compute_pipeline(
            device,
            label,
            &crcbl_shaders::PROBE_GATHER,
            pipeline_layout,
            GATHER_WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(pipeline);

        let positions = device.create_buffer(&BufferDesc {
            label: Some(&format!("{label} positions")),
            size: u64::from(rows) * POSITION_STRIDE,
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })?;
        rollback.buffers.push(positions);
        device.write_buffer(positions, 0, &position_table(&desc.volume))?;

        let mut params = Vec::with_capacity(desc.probes.len());
        for frame in 0..desc.probes.len() {
            let block = device.create_buffer(&BufferDesc {
                label: Some(&format!("{label} params {frame}")),
                size: GATHER_PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(block);
            params.push(block);
        }

        Ok(Self {
            layout,
            pipeline_layout,
            pipeline,
            params,
            probes: desc.probes.to_vec(),
            positions,
            groups: vec![None; desc.probes.len()],
            rows,
        })
    }

    /// Writes `frame`'s parameter block.
    ///
    /// Called once per frame, before [`ProbeGather::add_pass`], against the same
    /// frame slot the probe table is bound from.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the write failed.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        sun_color: [f32; 3],
        texel_area: f32,
    ) -> Result<(), HalError> {
        let params = GatherParams {
            sun_color,
            texel_area,
            rsm_side: crcbl_shaders::probe_gather::RSM_SIDE,
            probes: self.rows,
        };
        device.write_buffer(self.params[frame], 0, &params.to_bytes())
    }

    /// Adds the gather to `graph`: one workgroup per probe, reading `images` and
    /// `visibility` and writing `table`.
    ///
    /// `table` is the graph's id for the same buffer `frame` names in the ring —
    /// declared [`ResourceState::ShaderReadWrite`] here, which is what makes the
    /// graph order this write against the reads the drawing passes declare on
    /// it.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_pass<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        images: RsmImages,
        visibility: ImageViewHandle,
        table: BufferId,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let params = self.params[frame];
        let positions = self.positions;
        let rows = self.rows;
        let probes = self.probes[frame];
        // Split off before the closure, so it borrows this slot of the cache
        // rather than the whole of `self` — `crate::ssao`'s add_passes does the
        // same and for the same reason.
        let cached = &mut self.groups[frame];
        graph
            .add_compute_pass("probe-gather")
            .read_image(images.albedo)
            .read_image(images.normal)
            .read_image(images.world)
            .use_buffer(table, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let albedo = ctx.image_view(images.albedo);
                let normal = ctx.image_view(images.normal);
                let world = ctx.image_view(images.world);
                let device = ctx.device();
                let entries = vec![
                    bound(0, params),
                    bound(1, positions),
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(visibility),
                    },
                    // Overwritten by `cached_group` with the realised views;
                    // written here so the list is a complete description of the
                    // layout rather than one with three holes in it.
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::ImageView(albedo),
                    },
                    BindGroupEntry {
                        binding: 4,
                        array_index: 0,
                        resource: BindingResource::ImageView(normal),
                    },
                    BindGroupEntry {
                        binding: 5,
                        array_index: 0,
                        resource: BindingResource::ImageView(world),
                    },
                    bound(6, probes),
                ];
                // **The visibility view is part of the key too.** It is the
                // placeholder until `capture_probe_visibility` has run and the
                // captured array afterwards, and a group keyed on the transients
                // alone would go on naming the placeholder for the rest of the
                // process.
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(2, visibility), (3, albedo), (4, normal), (5, world)],
                    "probe gather",
                    layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                // **One workgroup per probe**, not `div_ceil` of anything: a
                // group is a probe and its threads stride the whole map. See
                // `crcbl_shaders::probe_gather::GATHER_WORKGROUP_SIZE`.
                encoder.dispatch(rows, 1, 1);
            });
    }

    /// Releases everything. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        for (_, group) in self.groups.into_iter().flatten() {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.layout);
        device.destroy_buffer(self.positions);
        for buffer in self.params {
            device.destroy_buffer(buffer);
        }
    }
}

/// Where every probe of `volume` stands, one `float4` a row in the table's own
/// order: `x`-fastest within a level, and the levels one after another finest
/// first.
///
/// [`ProbeVolume::position`](crcbl_shaders::probe::ProbeVolume::position) is the
/// one place that arithmetic is written — see the [module docs](self).
fn position_table(volume: &ProbeVolume) -> Vec<u8> {
    let rows = volume.total() as usize;
    let mut bytes = Vec::with_capacity(rows * POSITION_STRIDE as usize);
    for level in 0..volume.level_count() {
        for z in 0..volume.counts[2] {
            for y in 0..volume.counts[1] {
                for x in 0..volume.counts[0] {
                    for value in volume.position(level, [x, y, z]) {
                        bytes.extend_from_slice(&value.to_le_bytes());
                    }
                    // The `w` lane a `float4` element carries; unread.
                    bytes.extend_from_slice(&0f32.to_le_bytes());
                }
            }
        }
    }
    debug_assert_eq!(bytes.len(), rows * POSITION_STRIDE as usize);
    bytes
}

/// What a failed [`ProbeGather::new`] has to give back, in the order it must.
#[derive(Default)]
struct Rollback {
    pipelines: Vec<ComputePipelineHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    layouts: Vec<BindGroupLayoutHandle>,
    buffers: Vec<BufferHandle>,
}

impl Rollback {
    fn run(self, device: &dyn Device) {
        for pipeline in self.pipelines {
            device.destroy_compute_pipeline(pipeline);
        }
        for layout in self.pipeline_layouts {
            device.destroy_pipeline_layout(layout);
        }
        for layout in self.layouts {
            device.destroy_bind_group_layout(layout);
        }
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};

    /// A volume with an unequal count on every axis and two levels, so a table
    /// walked in the wrong order is a different set of numbers rather than the
    /// same one.
    fn volume() -> ProbeVolume {
        ProbeVolume {
            origin: [-1.0, 0.5, 2.0],
            inv_spacing: [0.5, 0.25, 1.0],
            counts: [2, 3, 4],
            levels: 2,
        }
    }

    fn read(bytes: &[u8], row: usize) -> [f32; 3] {
        let at = row * POSITION_STRIDE as usize;
        let mut out = [0.0f32; 3];
        for (axis, value) in out.iter_mut().enumerate() {
            let start = at + axis * 4;
            *value = f32::from_le_bytes(
                bytes[start..start + 4]
                    .try_into()
                    .expect("four bytes of a float"),
            );
        }
        out
    }

    /// **The table walks the volume in the table's own order**, and every entry
    /// is [`ProbeVolume::position`]'s answer rather than a second copy of the
    /// clipmap arithmetic.
    ///
    /// Compared against the module the volume is defined by, not against
    /// written-out numbers: what could go wrong here is the *order* — a level
    /// stride, a `z`-fastest walk — and a hand-written expectation would have to
    /// re-derive the same positions to catch it.
    #[test]
    fn a_position_table_walks_the_volume_in_row_order() {
        let volume = volume();
        let bytes = position_table(&volume);
        assert_eq!(
            bytes.len(),
            volume.total() as usize * POSITION_STRIDE as usize
        );
        for level in 0..volume.level_count() {
            for z in 0..volume.counts[2] {
                for y in 0..volume.counts[1] {
                    for x in 0..volume.counts[0] {
                        let row = (volume.level_row(level)
                            + (z * volume.counts[1] + y) * volume.counts[0]
                            + x) as usize;
                        assert_eq!(
                            read(&bytes, row),
                            volume.position(level, [x, y, z]),
                            "row {row} is not probe ({x}, {y}, {z}) of level {level}"
                        );
                    }
                }
            }
        }
        // And the padding lane really is a lane rather than a fourth component
        // of the position — a table written as `float3`s would be shorter and
        // every row past the first would be wrong.
        assert_ne!(read(&bytes, 1), read(&bytes, 0));
    }

    /// A gather with nothing to gather into is refused by name rather than
    /// served as a zero-sized buffer no backend creates.
    #[test]
    fn a_gather_with_no_probes_is_refused() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let table = crate::probe::ProbeTable::new(
            device.as_ref(),
            queue,
            &crate::probe::ProbeTableDesc {
                label: Some("test"),
                capacity: 1,
                frames: 1,
            },
        )
        .expect("the null backend accepts every descriptor");
        let before = recorder.total_live_objects();
        let error = ProbeGather::new(
            device.as_ref(),
            &ProbeGatherDesc {
                label: Some("test"),
                volume: ProbeVolume::default(),
                probes: &[table.buffer(0)],
            },
        )
        .expect_err("a volume of no probes is a dispatch of no workgroups");
        assert!(
            matches!(&error, HalError::InvalidDescriptor(message)
                if message.contains("one probe")),
            "got: {error:?}"
        );
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "a refusal creates nothing"
        );

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Everything created is destroyed.
    #[test]
    fn a_gather_leaks_nothing() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let volume = volume();
        let table = crate::probe::ProbeTable::new(
            device.as_ref(),
            queue,
            &crate::probe::ProbeTableDesc {
                label: Some("test"),
                capacity: volume.total(),
                frames: 2,
            },
        )
        .expect("the null backend accepts every descriptor");
        let before = recorder.total_live_objects();
        let gather = ProbeGather::new(
            device.as_ref(),
            &ProbeGatherDesc {
                label: Some("test"),
                volume,
                probes: &[table.buffer(0), table.buffer(1)],
            },
        )
        .expect("the null backend accepts every descriptor");
        assert!(recorder.total_live_objects() > before);
        gather
            .begin_frame(device.as_ref(), 0, [1.0, 1.0, 1.0], 0.25)
            .expect("a host-upload write always lands");
        gather.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }
}
