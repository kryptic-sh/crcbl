//! `docs/plan/43-render-standards.md` §6's auto-exposure: the luminance
//! histogram of the finished frame, the reduce that turns it into one number,
//! and the buffer the tonemap reads that number out of.
//!
//! ```text
//!  begin_frame ──▶ extent ──▶ params[frame]
//!
//!  add_passes ── compute "exposure-clear"     ──▶ histogram[frame]
//!             ── compute "exposure-histogram" scene-color ──▶ histogram[frame]
//!             ── compute "exposure-reduce"    histogram   ──▶ measured[frame]
//!                                                  tonemap reads measured[frame]
//! ```
//!
//! A module of its own on [`crate::volumetric`]'s terms exactly: three
//! pipelines, two buffer rings and the pass group, none of it reachable from
//! the passes that draw except through [`Exposure::add_passes`].
//!
//! # Nothing here is read back, and nothing here is a frame behind
//!
//! All three passes run inside the frame they measure — the histogram bins the
//! scene colour the passes before it produced, and the tonemap that follows
//! reads the exposure the reduce wrote. So the exposure a frame is drawn with
//! is measured from that frame, with no readback to stall on and no
//! ping-ponged buffer holding the previous frame's answer.
//!
//! What it does **not** do is adapt over time: there is no time constant, so a
//! cut between two differently-lit shots lands in a single frame. That is the
//! next rung in [`docs/plan/48-post-processing.md`], and it is the one that
//! needs a value to survive between frames — which is what makes it a change to
//! this ring rather than a constant somewhere.
//!
//! [`docs/plan/48-post-processing.md`]: crate
//!
//! # The switch is a lane of the tonemap's block
//!
//! `tonemap.slang` binds [`Exposure::measured`]'s buffer on every frame and
//! reads it only when its block says to, so a frame that adds no passes here
//! writes a zero into that lane and draws exactly the picture this engine drew
//! before the pass existed. [`crate::forward`] is what writes both.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ComputePipelineHandle, Device, HalError, ImageViewHandle, ImageViewType,
    MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, QueueHandle, ResourceState,
    SampleType, ShaderStages, check_portable_storage_buffers,
};
use crcbl_shaders::EXPOSURE;
use crcbl_shaders::exposure::{
    BIN_COUNT, BIN_STRIDE, ExposureParams, MEASURED_SIZE, PARAMS_SIZE, WORKGROUP_SIZE,
};

use std::cell::Cell;
use std::rc::Rc;

use crate::draw_gen::{bound, compute_pipeline_entry, fill_at_start_up, storage, uniform};
use crate::graph::{ImageId, ImportedBuffer, RenderGraph};
use crate::ssao::cached_group;

/// How far one frame's exposure may travel toward the one its histogram asks
/// for.
///
/// **Why a step and not the measurement itself.** A real eye takes seconds to
/// adapt; an exposure that lands on its target in a single frame turns a camera
/// cut, a muzzle flash or a door opening into a flicker. So a view hands this
/// in every frame with its own delta, and
/// [`ForwardRenderer::set_exposure_adaptation`](crate::ForwardRenderer::set_exposure_adaptation)
/// is where it goes.
///
/// **The two rates differ because the eye's do.** Adapting *down* to a scene
/// that just got bright is fast and adapting back up is slow, and a viewer
/// notices immediately when they are swapped. They are named for what the
/// exposure does, not for what the scene did: `brighten` is the exposure
/// climbing, which is the picture getting brighter as the eye opens up in the
/// dark.
///
/// Each is a fraction of the remaining distance per second, so `2.0` covers
/// half the distance in a quarter of a second and anything at or above
/// `1.0 / delta` arrives in one frame. It is the linear approximation of the
/// exponential approach every engine writes as `1 - exp(-rate * delta)`, and it
/// is linear on purpose: this workspace lets no transcendental reach a colour,
/// and an exposure multiplies every texel of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureAdaptation {
    /// Fraction of the remaining distance per second while the exposure climbs.
    pub brighten: f32,
    /// The same while it falls.
    pub darken: f32,
    /// How long this frame was, in seconds — the view's own delta.
    pub delta: f32,
}

impl ExposureAdaptation {
    /// The two blends the block carries: `rate * delta`, clamped into `[0, 1]`.
    ///
    /// Clamped here rather than only in the shader because a rate and a delta
    /// are both a caller's numbers, and the product of two of those is where a
    /// pause, a breakpoint or a first frame puts a blend of several hundred.
    fn blends(self) -> (f32, f32) {
        let blend = |rate: f32| (rate * self.delta).clamp(0.0, 1.0);
        (blend(self.brighten), blend(self.darken))
    }
}

/// The buffers one frame's measurement lives in.
///
/// **A window for a test, and this pass's only public surface**, on
/// [`FroxelBuffers`](crate::FroxelBuffers)' terms: the histogram is checked
/// against `crcbl_shaders::exposure` bin by bin, which is the one thing that
/// can tell a wrong bin from a wrong dispatch extent — a frame-level check sees
/// only the exposure the two of them compose to.
#[derive(Clone, Copy, Debug)]
pub struct ExposureBuffers {
    /// `[bin]`: how many texels of the frame landed in each bin, as
    /// `histogramMain` left it.
    pub histogram: BufferHandle,
    /// `[0]`: the exposure `reduceMain` measured, and the one the tonemap
    /// applied.
    pub measured: BufferHandle,
}

/// Everything the pass owns.
///
/// Built once by [`Exposure::new`] and released by [`Exposure::destroy`], which
/// is the shape every other resource group in this crate has.
#[derive(Debug)]
pub(crate) struct Exposure {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    /// Zeroes the bins. A pass of its own because the histogram accumulates
    /// with an atomic add — `exposure.slang` says why that is the only clear a
    /// storage buffer has here.
    clear: ComputePipelineHandle,
    histogram_pipeline: ComputePipelineHandle,
    reduce: ComputePipelineHandle,
    /// `[frame]`: the extent being binned, one block per frame in flight for
    /// the frame uniforms' reason — the previous frame may still be reading
    /// last frame's while this one is written.
    params: Vec<BufferHandle>,
    /// `[frame]`: the bins.
    histograms: Vec<BufferHandle>,
    /// `[frame]`: the one float the tonemap reads.
    measured: Vec<BufferHandle>,
    /// `[frame]`: the group all three dispatches bind, cached against the scene
    /// view — which is a graph transient, so it cannot be built until the graph
    /// has realised one.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl Exposure {
    /// Passes [`Exposure::add_passes`] adds to a frame.
    ///
    /// Exact rather than a ceiling: the three run or none of them do.
    pub(crate) const PASSES: u32 = 3;

    /// Builds the three pipelines and the buffer rings.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        queue: QueueHandle,
        frames: usize,
    ) -> Result<Self, HalError> {
        // **Two, not one.** A frame reads the slot behind it for the exposure
        // to adapt away from, and with a single slot that is the one this
        // frame's reduce also writes — which the graph refuses outright, since
        // a pass cannot claim one resource as both written and read. Enforced
        // here rather than branched around: [`crate::forward::FRAMES_IN_FLIGHT`]
        // is two, so the one-slot path would be a branch nothing ever takes.
        if frames < 2 {
            return Err(HalError::InvalidDescriptor(format!(
                "auto-exposure needs at least two frames in flight, so a frame can read \
                 the exposure the frame before it measured; got {frames}"
            )));
        }
        // `exposure.slang`'s declaration order, which is the only order Metal
        // and D3D12 agree about — see `crcbl_shaders::declaration_order`.
        let entries = [
            uniform(0),
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The `Rgba16Float` scene target: an ordinary colour image,
                    // and the WGSL artifact declares `texture_2d<f32>` for it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            storage(2, false),
            storage(3, false),
            // Read-only: the reduce reads the frame before's exposure and never
            // writes through this binding, which is what lets it be a different
            // slot of the same ring the entry above writes.
            storage(4, true),
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("exposure"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("exposure"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("exposure"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;
        // Three entry points of one module, which is why this is
        // `compute_pipeline_entry` rather than `compute_pipeline`: the three
        // share the binning arithmetic and shaders here have no `#include`.
        let clear = compute_pipeline_entry(
            device,
            "exposure clear",
            &EXPOSURE,
            "clearMain",
            pipeline_layout,
            WORKGROUP_SIZE,
        )?;
        let histogram_pipeline = compute_pipeline_entry(
            device,
            "exposure histogram",
            &EXPOSURE,
            "histogramMain",
            pipeline_layout,
            WORKGROUP_SIZE,
        )?;
        let reduce = compute_pipeline_entry(
            device,
            "exposure reduce",
            &EXPOSURE,
            "reduceMain",
            pipeline_layout,
            // One invocation for the whole reduce, on the shader's own terms:
            // float addition is not associative, so a tree would sum the bins in
            // an order a device schedules and the exposure would differ in its
            // last place between backends.
            1,
        )?;

        let mut params = Vec::with_capacity(frames);
        let mut histograms = Vec::with_capacity(frames);
        let mut measured = Vec::with_capacity(frames);
        for frame in 0..frames {
            params.push(device.create_buffer(&BufferDesc {
                label: Some(&format!("exposure params {frame}")),
                size: PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
            histograms.push(device.create_buffer(&BufferDesc {
                label: Some(&format!("exposure histogram {frame}")),
                size: u64::from(BIN_COUNT) * BIN_STRIDE as u64,
                // `TRANSFER_SRC` because a test reads the bins back and compares
                // them against the host's own binning of the same frame — the
                // pass has no other observable, since the exposure it produces
                // is a single number two different mistakes can agree on.
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })?);
            let slot = device.create_buffer(&BufferDesc {
                label: Some(&format!("exposure measured {frame}")),
                size: MEASURED_SIZE as u64,
                // `STORAGE` in three stages: written by the reduce, read by the
                // tonemap, and read again by the *next* frame's reduce as the
                // exposure to adapt away from. `TRANSFER_DST` for the start-up
                // fill below.
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })?;
            // **Every slot starts at the default exposure**, because the first
            // frame's reduce reads one of them as the value to adapt from and a
            // device-local allocation carries whatever was last in that memory.
            // A garbage prior inside the legal range would be indistinguishable
            // from a measurement, and would differ between runs.
            fill_at_start_up(
                device,
                queue,
                &format!("exposure measured {frame}"),
                slot,
                &crcbl_shaders::tonemap::DEFAULT_EXPOSURE.to_le_bytes(),
                // What `add_passes` imports it in, and what the tonemap left it
                // in on every frame after the first.
                ResourceState::ShaderRead,
            )?;
            measured.push(slot);
        }

        Ok(Self {
            layout,
            pipeline_layout,
            clear,
            histogram_pipeline,
            reduce,
            params,
            histograms,
            measured,
            groups: vec![None; frames],
        })
    }

    /// Writes `frame`'s parameter block: the extent of the image to bin.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the mapped write.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        extent: (u32, u32),
        adaptation: Option<ExposureAdaptation>,
    ) -> Result<(), HalError> {
        // `None` is both blends at one, which is the whole distance in one
        // frame — the picture this pass drew before adaptation existed, and the
        // one `ExposureParams::default` carries for the same reason.
        let (brighten_blend, darken_blend) =
            adaptation.map_or((1.0, 1.0), ExposureAdaptation::blends);
        device.write_buffer(
            self.params[frame],
            0,
            &ExposureParams {
                viewport_x: extent.0.max(1),
                viewport_y: extent.1.max(1),
                brighten_blend,
                darken_blend,
            }
            .to_bytes(),
        )
    }

    /// The slot holding what the frame before `frame` was exposed by.
    ///
    /// One step back around the same ring, which is the previous frame because
    /// the frame index advances by one — and never `frame`'s own slot, which is
    /// what [`Exposure::new`]'s two-slot floor is for.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    fn previous(&self, frame: usize) -> BufferHandle {
        assert!(frame < self.measured.len(), "no such frame slot");
        self.measured[(frame + self.measured.len() - 1) % self.measured.len()]
    }

    /// The buffer the tonemap binds: `frame`'s measured exposure.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn measured(&self, frame: usize) -> BufferHandle {
        self.measured[frame]
    }

    /// `frame`'s bins and the exposure they were reduced to.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn buffers(&self, frame: usize) -> ExposureBuffers {
        ExposureBuffers {
            histogram: self.histograms[frame],
            measured: self.measured[frame],
        }
    }

    /// Adds the clear, the histogram and the reduce, in that order.
    ///
    /// `scene` is the image the tonemap is about to read, and binning anything
    /// else would expose the frame for a picture nobody sees.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        extent: (u32, u32),
        scene: ImageId,
    ) {
        // Both arrive in the state the previous frame that used this slot left
        // them in, which is the state declared as final: the tonemap read the
        // measurement and nothing read the bins. Vacuous on the first frame, and
        // the real prior use on every later one.
        let bins = graph.import_buffer(
            "exposure-histogram",
            ImportedBuffer {
                buffer: self.histograms[frame],
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        let measured = graph.import_buffer(
            "exposure-measured",
            ImportedBuffer {
                buffer: self.measured[frame],
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        // The frame before's slot, always a different buffer from the one this
        // frame writes — `Exposure::new` refuses a ring too short for that.
        let previous_buffer = self.previous(frame);
        let previous = graph.import_buffer(
            "exposure-previous",
            ImportedBuffer {
                buffer: previous_buffer,
                // What the frame that wrote it left it in, and what this frame
                // leaves it in: nothing here writes through it.
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        let params = self.params[frame];
        let histogram_buffer = self.histograms[frame];
        let measured_buffer = self.measured[frame];
        let layout = self.layout;
        let pipeline_layout = self.pipeline_layout;
        let clear = self.clear;
        let histogram = self.histogram_pipeline;
        let reduce = self.reduce;
        let cached = &mut self.groups[frame];

        // One group for all three dispatches, built inside the first of them:
        // it names the scene colour, which is a graph transient, so there is no
        // view to name until the graph has realised one. Cached against that
        // view, and therefore rebuilt only on a resize.
        //
        // A shared `Cell` carries it to the two passes after this one: the pass
        // bodies run synchronously and in order on this thread, so the pass that
        // builds the group has always run before the two that bind it. `Rc`
        // rather than a borrow because each body is an owned closure the graph
        // keeps, and a body that finds no group records no dispatch rather than
        // binding a stale one.
        let group: Rc<Cell<Option<BindGroupHandle>>> = Rc::new(Cell::new(None));
        let build = Rc::clone(&group);
        graph
            .add_compute_pass("exposure-clear")
            // `ShaderReadWrite` rather than a write-only state, on the light
            // grid's terms: a storage-buffer descriptor permits reads whatever
            // the shader does with it.
            .use_buffer(bins, ResourceState::ShaderReadWrite)
            .use_buffer(measured, ResourceState::ShaderReadWrite)
            // The image the pass after this one bins. Declared here as well
            // because the group naming it is built in this pass, and a view is
            // only realised for a pass that declared the image.
            .read_image(scene)
            .execute(move |ctx| {
                let view = ctx.image_view(scene);
                let device = ctx.device();
                let entries = vec![
                    bound(0, params),
                    // Overwritten by `cached_group` with the realised view;
                    // written here so the list is a complete description of the
                    // layout rather than one with a hole in it.
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    },
                    bound(2, histogram_buffer),
                    bound(3, measured_buffer),
                    bound(4, previous_buffer),
                ];
                let Some(built) =
                    cached_group(cached, device, &[(1, view)], "exposure", layout, entries)
                else {
                    return;
                };
                build.set(Some(built));
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(clear);
                encoder.bind_group(0, built, &[], pipeline_layout);
                encoder.dispatch(BIN_COUNT.div_ceil(WORKGROUP_SIZE), 1, 1);
            });

        // One invocation per texel of the scene target, and never zero: the
        // extent is floored at one texel, and Metal rejects an empty dispatch
        // outright rather than treating it as a no-op.
        let texels = extent.0.max(1).saturating_mul(extent.1.max(1));
        let texel_groups = texels.div_ceil(WORKGROUP_SIZE);
        let bind = Rc::clone(&group);
        graph
            .add_compute_pass("exposure-histogram")
            .use_buffer(bins, ResourceState::ShaderReadWrite)
            .read_image(scene)
            .execute(move |ctx| {
                let Some(group) = bind.get() else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(histogram);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.dispatch(texel_groups, 1, 1);
            });

        let bind = group;
        graph
            .add_compute_pass("exposure-reduce")
            // The histogram's own output, read here — the graph's barrier
            // between the two comes from both declaring this one id.
            .use_buffer(bins, ResourceState::ShaderReadWrite)
            .use_buffer(measured, ResourceState::ShaderReadWrite)
            // The frame before's exposure, read to adapt away from.
            .use_buffer(previous, ResourceState::ShaderRead)
            .execute(move |ctx| {
                let Some(group) = bind.get() else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(reduce);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.dispatch(1, 1, 1);
            });
    }

    /// Releases every object this owns.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self.groups.into_iter().flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_compute_pipeline(self.reduce);
        device.destroy_compute_pipeline(self.histogram_pipeline);
        device.destroy_compute_pipeline(self.clear);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self
            .params
            .into_iter()
            .chain(self.histograms)
            .chain(self.measured)
        {
            device.destroy_buffer(buffer);
        }
    }
}
