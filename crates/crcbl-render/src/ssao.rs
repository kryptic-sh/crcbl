//! `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the two
//! full-screen passes between the depth prepass and the forward pass.
//!
//! ```text
//! depth-prepass ──▶ scene-depth ──▶ ssao ──▶ ssao ──▶ ssao-blur ──▶ ssao-blurred
//!                   │              (R8Unorm)            ▲           (R8Unorm)
//!                   └───────────────────────────────────┘                │
//!                                                       forward ◀────────┘
//!                                                    (× frame.ambient)
//! ```
//!
//! A module of its own rather than more of [`crate::forward`], which is a file
//! this crate is trying to stop growing: what lives here is two pipelines, two
//! caches and the pass pair, and none of it is reachable from the geometry
//! passes except through [`Ssao::add_passes`].
//!
//! **The blur is not optional.** `shaders/ssao.slang` says why in full: its
//! rotation table makes the raw result carry a 4×4 tile as banding, and each of
//! its samples is a binary depth comparison that one driver may resolve
//! differently from another. The blur's footprint is exactly that tile, so it
//! removes the banding — and it divides an isolated flipped sample by sixteen
//! wherever its taps all count, which `shaders/ssao_blur.slang` is precise
//! about: the kernel is weighted on view-space depth, so the divisor is the full
//! sixteen on a flat surface and falls towards one at a silhouette, where the
//! taps it drops are the ones a box blur used to draw a halo with.
//!
//! The off-switch is data rather than a branch: a frame that does not add these
//! passes leaves `ForwardRenderer`'s white placeholder bound, and `mesh.slang`
//! multiplies its ambient by 1.0. There is no device fact to gate on — every
//! backend has a full-screen draw, a sampled `D32Float` and an `R8Unorm` target.
//!
//! # The higher rung, and why it is two switches
//!
//! `docs/backlog.md` records banding along the **tangential** axis, which is a
//! shortage of distinct slice planes rather than of steps along one: the raw
//! channel's neighbourhood carries eight plane orientations at the shipping
//! count, whatever the blur then does with it. [`r_ssao_slices`] is what buys
//! more of them — `shaders/ssao.slang`'s `SLICE_COUNT_MAX` has the arithmetic —
//! and [`r_ssao_blur_passes`] is what widens the footprint they are averaged
//! over.
//!
//! **Two switches and not one**, because they are two different purchases: the
//! slices are arithmetic and texture fetches inside one pass, and a second blur
//! is a whole extra full-screen read and write of an `R8Unorm` image. A
//! bandwidth-bound tier may want the first and refuse the second, and a knob
//! that bundled them could not say so. Both default to what ships, so a frame
//! nobody has touched the console on is the frame every golden was blessed at.
//!
//! Neither is a quality preset. `docs/plan/43-render-standards.md`'s tiers are
//! their own unbuilt row; these are the two variables such a preset would set,
//! declared beside the pass that reads them the way `crate::debug_draw`'s switch
//! is.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{SSAO, SSAO_BLUR, ssao};

use crate::graph::{ImageId, RenderGraph};

/// Vertices in the over-sized full-screen triangle `ssao.slang` and
/// `ssao_blur.slang` generate from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

crcbl_console::convar! {
    /// Planes each pixel sweeps for a horizon: 2 ships, 4 adds an eighth turn.
    pub static r_ssao_slices: i64 in 2 ..= 4 = 2;
}

crcbl_console::convar! {
    /// Times the occlusion blur runs over the raw channel: 1 ships.
    pub static r_ssao_blur_passes: i64 in 1 ..= 2 = 1;
}

/// [`r_ssao_slices`] as the shader's uniform wants it.
///
/// Clamped on the way through rather than trusted: the variable's own range is
/// the console's guard, and this is the one that holds if the constant the
/// shader declares ever moves below it. `ssao.slang`'s `slice_count` clamps
/// again on its side, which is where a value that never reached this function
/// at all is caught.
pub(crate) fn slice_count() -> u8 {
    let asked = r_ssao_slices.get_i64().clamp(
        i64::from(ssao::SLICE_COUNT_DEFAULT),
        i64::from(ssao::SLICE_COUNT_MAX),
    );
    u8::try_from(asked).unwrap_or(ssao::SLICE_COUNT_DEFAULT)
}

/// [`r_ssao_blur_passes`] as a pass count, clamped into what
/// [`Ssao::add_passes`] can record.
pub(crate) fn blur_passes() -> u32 {
    let asked = r_ssao_blur_passes
        .get_i64()
        .clamp(1, i64::from(Ssao::MAX_BLUR_PASSES));
    u32::try_from(asked).unwrap_or(1)
}

/// Everything the occlusion pair owns.
///
/// Built once by [`Ssao::new`] and released by [`Ssao::destroy`], which is the
/// shape every other resource group in this crate has — see
/// [`crate::light_grid::LightGrid`].
#[derive(Debug)]
pub(crate) struct Ssao {
    /// `[frame]`: `ssao.slang`'s uniform block. One per frame in flight for the
    /// frame uniforms' reason exactly — the previous frame may still be reading
    /// last frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the occlusion group, cached against the depth view.
    ///
    /// **One per frame in flight**, because this group names [`Ssao::uniforms`]
    /// as well as the depth transient — and that is a ring. A single cache keyed
    /// on the view alone would hand the even frames' block to the odd frames.
    groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    blur_layout: BindGroupLayoutHandle,
    blur_pipeline_layout: PipelineLayoutHandle,
    blur_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the blur group, cached against the raw occlusion view and the
    /// depth view together.
    ///
    /// **A ring for [`Ssao::groups`]' reason exactly**: `ssao_blur.slang` weights
    /// its kernel on view-space depth, so it binds the same [`Ssao::uniforms`]
    /// block the occlusion pass does — and a single cache keyed on the views
    /// alone would hand the even frames' block to the odd frames.
    blur_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the second blur's group, when [`r_ssao_blur_passes`] asks for
    /// one.
    ///
    /// **Its own ring rather than [`Ssao::blur_groups`] reused**, and not for
    /// tidiness: the two passes bind different occlusion images, so one cache
    /// shared between them would be rebuilt twice a frame — a descriptor write
    /// per pass per frame, which is the cost `cached_group` exists to avoid.
    /// The pipeline and the layout *are* shared, because the second blur is the
    /// same shader reading the first one's output.
    ///
    /// Empty of groups on every frame the switch is at its default, and the
    /// slots themselves cost a pointer each.
    blur_again_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl Ssao {
    /// Blur passes [`r_ssao_blur_passes`] may ask for.
    ///
    /// Two: the shipping one and the second the tangential rung wants. There is
    /// no third because the kernel's footprint doubles with each — see
    /// `shaders/ssao_blur.slang`, whose whole argument is that its footprint is
    /// `ssao.slang`'s tile — and a third would be blurring past every contact
    /// in the frame.
    pub(crate) const MAX_BLUR_PASSES: u32 = 2;

    /// The most passes [`Ssao::add_passes`] can add to a frame: the march, and
    /// every blur behind it.
    ///
    /// A **ceiling**, which is what `crate::forward`'s `RENDER_PASSES` needs
    /// from every term it adds up — see [`Ssao::passes`] for what a given frame
    /// actually records.
    pub(crate) const MAX_PASSES: u32 = 1 + Self::MAX_BLUR_PASSES;

    /// Passes [`Ssao::add_passes`] adds to a frame that asked for `blurs` of
    /// them: the occlusion march, and one per blur.
    pub(crate) const fn passes(blurs: u32) -> u32 {
        1 + blurs
    }

    /// Builds both pipelines and the uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated: it is
    /// [`crate::forward`]'s, because the tonemap pass is the third caller and the
    /// shape it carries — a triangle out of `SV_VertexID`, no depth state, one
    /// colour target — is documented there.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the caller
    /// holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // **No sampler in either layout**, which is the whole of what reading by
        // `Load` buys — see the binding comments in `shaders/ssao.slang`.
        let entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // **`Depth`, and it is the seam's half of `DepthTexture2D`.**
                    // WebGPU will only bind a `D32Float` view through a depth
                    // sample type, and the WGSL artifact agrees — it declares
                    // `texture_depth_2d`, which `textureLoad` takes. There is no
                    // comparison sampler beside it because nothing here compares:
                    // this binding is fetched, not sampled.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("ssao depth"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("ssao"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssao"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })?;

        // **The same shape, one binding longer.** The blur unprojects the depth
        // it weights its kernel by, so it names the uniform block at 0 exactly
        // as the pass above does and its two images follow — see
        // `shaders/ssao_blur.slang`.
        let blur_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Float`, unlike the depth beside it: the raw occlusion is
                    // an ordinary `R8Unorm` colour image and the WGSL artifact
                    // declares `texture_2d<f32>` for it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Depth`, and the paragraph on the occlusion pass's own
                    // depth binding is this one's too: it is the same image,
                    // read through the same `texture_depth_2d`.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let blur_desc = BindGroupLayoutDesc {
            label: Some("ssao blur"),
            entries: &blur_entries,
        };
        check_portable_storage_buffers(Some("ssao blur"), &[&blur_desc])?;
        let blur_layout = device.create_bind_group_layout(&blur_desc)?;
        let blur_set_layouts = [blur_layout];
        let blur_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("ssao blur"),
            bind_group_layouts: &blur_set_layouts,
            push_constants: None,
        })?;

        let targets = [ColorTargetState::opaque(Format::R8Unorm)];
        let pipeline = build_fullscreen(device, "ssao", &SSAO, pipeline_layout, &targets)?;
        let blur_pipeline = build_fullscreen(
            device,
            "ssao blur",
            &SSAO_BLUR,
            blur_pipeline_layout,
            &targets,
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("ssao params"),
                size: ssao::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            uniforms,
            layout,
            pipeline_layout,
            pipeline,
            groups: vec![None; frames],
            blur_layout,
            blur_pipeline_layout,
            blur_pipeline,
            blur_groups: vec![None; frames],
            blur_again_groups: vec![None; frames],
        })
    }

    /// Writes `frame`'s uniform block.
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
        params: ssao::SsaoParams,
    ) -> Result<(), HalError> {
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the `ssao` pass and its blurs, in that order, and returns the image
    /// the forward pass should read.
    ///
    /// `depth` must be the prepass's stored depth. `blurred` is where the first
    /// blur writes, and `again` — [`Some`] exactly when [`r_ssao_blur_passes`]
    /// asked for a second — is where the second one does. Every occlusion image
    /// is the caller's so it can declare the read on the pass that consumes the
    /// last of them: a pass declares its own accesses and this one cannot
    /// declare the forward pass's.
    ///
    /// **The returned id is the one to bind**, rather than the caller assuming
    /// `blurred`, because which image holds the finished channel is this
    /// function's business and gets it wrong silently — a forward pass bound to
    /// the half-blurred image draws a frame nobody would question.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        depth: ImageId,
        raw: ImageId,
        blurred: ImageId,
        again: Option<ImageId>,
    ) -> ImageId {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        // Split so the closures below borrow different halves of `self`; one
        // `&mut self` shared between them is what the borrow checker refuses, and
        // it would be refusing something genuinely wrong — a pass body may run at
        // any point after it is declared.
        let (groups, blur_groups, again_groups) = (
            &mut self.groups,
            &mut self.blur_groups,
            &mut self.blur_again_groups,
        );
        let cached = &mut groups[frame];
        let blur_cached = &mut blur_groups[frame];
        let again_cached = &mut again_groups[frame];

        graph
            .add_render_pass("ssao")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(raw, LoadOp::DontCare, StoreOp::Store, ClearValue::default())
            // The prepass left this in `DepthStencilWrite`. Declaring the read is
            // what moves it to a shader-readable layout, and without it every
            // backend reads whatever the depth writes left behind.
            .read_image(depth)
            .execute(move |ctx| {
                let view = ctx.image_view(depth);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view;
                        // written here so the list is a complete description of
                        // the layout rather than a list with a hole in it.
                        resource: BindingResource::ImageView(view),
                    },
                ];
                let Some(group) =
                    cached_group(cached, device, &[(1, view)], "ssao depth", layout, entries)
                else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });

        let blur = Blur {
            uniforms,
            layout: self.blur_layout,
            pipeline_layout: self.blur_pipeline_layout,
            pipeline: self.blur_pipeline,
        };
        blur.add_pass(graph, "ssao-blur", blur_cached, raw, depth, blurred);
        let Some(again) = again else {
            return blurred;
        };
        // The second blur reads the first one's output and writes its own
        // image. **Not back into `raw`**, which would be a write-after-read on
        // an image this same frame's march wrote and this same frame's first
        // blur read: the graph would have to order it, the pool could not alias
        // it, and the saving is one `R8Unorm` image on the frames that asked for
        // this at all.
        blur.add_pass(graph, "ssao-blur-2", again_cached, blurred, depth, again);
        again
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self
            .groups
            .into_iter()
            .chain(self.blur_groups)
            .chain(self.blur_again_groups)
            .flatten()
        {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_graphics_pipeline(self.blur_pipeline);
        device.destroy_pipeline_layout(self.blur_pipeline_layout);
        device.destroy_bind_group_layout(self.blur_layout);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}

/// The pipeline, layout and uniform block both blur passes share.
///
/// The second blur is the *same shader* reading the first one's output — see
/// this module's header on why there can be a second at all — so what differs
/// between the two passes is three image ids and a label, and everything else
/// is this. Gathered into a type rather than passed as six arguments, and
/// extracted the moment there were two callers rather than left as a copy of
/// the pass body with two ids changed.
#[derive(Clone, Copy)]
struct Blur {
    uniforms: BufferHandle,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
}

impl Blur {
    /// Adds one blur pass: `source` and `depth` in, `target` out.
    ///
    /// `cached` is this pass's own group ring slot — the two passes bind
    /// different source images, so sharing one slot between them would rebuild
    /// the group twice a frame.
    fn add_pass<'a>(
        self,
        graph: &mut RenderGraph<'a>,
        label: &'static str,
        cached: &'a mut Option<(Vec<ImageViewHandle>, BindGroupHandle)>,
        source: ImageId,
        depth: ImageId,
        target: ImageId,
    ) {
        let Self {
            uniforms,
            layout,
            pipeline_layout,
            pipeline,
        } = self;
        graph
            .add_render_pass(label)
            .color(
                target,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(source)
            // **The depth this pass weights its kernel by**, and the
            // declaration is what gives it a shader-readable layout here as
            // well: the march declared its own read, and a barrier the graph was
            // never told about is one it does not insert.
            .read_image(depth)
            .execute(move |ctx| {
                let view = ctx.image_view(source);
                let depth_view = ctx.image_view(depth);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view,
                        // as is the depth below; written here so the list is a
                        // complete description of the layout rather than a list
                        // with two holes in it.
                        resource: BindingResource::ImageView(view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(depth_view),
                    },
                ];
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(1, view), (2, depth_view)],
                    label,
                    layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }
}

/// A bind group naming each `(binding, view)` of `views`, built once and kept
/// until any of those views changes.
///
/// **The shape every group in this engine that names a graph transient has.** A
/// transient's view is realised by the graph and is therefore not known until a
/// pass body runs, so such a group cannot be built where the others are — and
/// rebuilding it every frame would be a descriptor write per frame for handles
/// that only ever change on a resize. `entries` is the group's complete
/// description with any value at each of those bindings; this replaces those
/// entries.
///
/// A group naming one transient passes a one-element slice. The cache key is
/// **every** view, because a group naming two of them is stale as soon as either
/// moves — which is what the blur pass needs and what a key on the first alone
/// would silently get wrong on a resize.
///
/// [`None`] means the group could not be created and the caller should record
/// nothing. Recording a pass that draws nothing is better than aborting a frame:
/// the window loses that pass's contribution, the log says why, and the next
/// frame retries.
///
/// **On a backend whose creation cannot fail synchronously this returns [`Some`]
/// regardless**, and that is deliberate rather than a gap. A command-stream
/// backend hands back a handle it allocated itself and learns the browser's
/// verdict later, so the pass records its draw, the invalid group makes the
/// submission invalid, and the failure arrives through
/// [`Device::take_error`] — where
/// `crcbl::engine`'s frame acquire turns it into an error that stops the frame.
/// That is louder than skipping a pass, and it is the right way round: a bind
/// group this code built wrongly is a bug, not a device that ran out of room.
/// The branch below stays for the backends that can still answer immediately.
///
/// It lives in this module because most of its callers do; the other is the
/// tonemap pass in [`crate::forward`], which is the file this split exists to
/// stop growing.
///
/// # Panics
///
/// If `entries` has no entry at one of the bindings — which is a caller that
/// built its list against a different layout than the one it passed.
pub(crate) fn cached_group(
    cache: &mut Option<(Vec<ImageViewHandle>, BindGroupHandle)>,
    device: &dyn Device,
    views: &[(u32, ImageViewHandle)],
    label: &str,
    layout: BindGroupLayoutHandle,
    mut entries: Vec<BindGroupEntry>,
) -> Option<BindGroupHandle> {
    if let Some((cached, group)) = cache
        && cached.iter().eq(views.iter().map(|(_, view)| view))
    {
        return Some(*group);
    }
    if let Some((_, stale)) = cache.take() {
        device.destroy_bind_group(stale);
    }
    for &(binding, view) in views {
        let slot = entries
            .iter_mut()
            .find(|entry| entry.binding == binding)
            .unwrap_or_else(|| panic!("{label}: the entry list has no binding {binding} to fill"));
        slot.resource = BindingResource::ImageView(view);
    }
    match device.create_bind_group(&BindGroupDesc {
        label: Some(label),
        layout,
        entries: &entries,
        variable_count: None,
    }) {
        Ok(group) => {
            *cache = Some((views.iter().map(|(_, view)| *view).collect(), group));
            Some(group)
        }
        Err(error) => {
            crcbl_core::log::error!("graph: {label} bind group failed: {error}");
            None
        }
    }
}
