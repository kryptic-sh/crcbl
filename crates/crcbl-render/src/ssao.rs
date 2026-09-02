//! `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the
//! full-screen passes between the depth prepass and the forward pass.
//!
//! ```text
//!   pass    ssao ────────▶ ssao-blur ────────▶ ssao-upsample ────────▶ forward
//!   writes  ssao           ssao-blurred        ssao-upsampled          (× frame.ambient)
//!           └─ half extent, Rgba8Unorm ┘      └ scene extent, Rgba8Unorm ┘
//! ```
//!
//! The prepass's `scene-depth` is read by all three of the occlusion passes and
//! is the only input any of them has besides the one before it: the march
//! gathers its horizons from it, the blur weights its kernel by it, and the
//! reconstruction reconciles the two extents against it.
//!
//! A module of its own rather than more of [`crate::forward`], which is a file
//! this crate is trying to stop growing: what lives here is three pipelines,
//! their caches and the pass chain, and none of it is reachable from the
//! geometry passes except through [`Ssao::add_passes`].
//!
//! # The gather runs small and the reconstruction runs full
//!
//! `shaders/ssao.slang`'s `RESOLUTION_DIVISOR` is the factor and that file's
//! header is the argument: occlusion is a low-frequency field, so the march and
//! the blur run over an image [`half_extent`] of the scene's — a quarter of the
//! invocations — and `shaders/ssao_upsample.slang` reconstructs the channel the
//! forward pass binds. The reconstruction is depth-aware rather than bilinear,
//! because the field is low-frequency **within a surface** and discontinuous
//! across a silhouette; a distance weight alone draws the wall's occlusion as a
//! rim around whatever stands in front of it.
//!
//! **Only [`Ssao::add_passes`]' last image is the scene's size**, and only
//! [`half_extent`] decides the others'. A caller that sized them itself would be
//! a second opinion about a division the shaders also hold.
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
//! # The bent direction is the fourth switch, and it is the one that is on
//!
//! [`r_ssao_bent_normals`] is what `docs/plan/46-ambient-occlusion.md` calls
//! the second half of the rung: the same horizon sweep that measures how much
//! of the room is hidden also reports which way what is left of it lies, and
//! `mesh.slang` samples its ambient irradiance along that direction instead of
//! along the shading normal. `shaders/ssao.slang`'s header is where the
//! encoding and the sweep's by-product are argued.
//!
//! **It defaults on, which is the one switch here that does not default to what
//! shipped before it.** The other three exist to buy quality a tier may not
//! want to pay for; this one is the rung. What it costs is the three channels
//! the target widened by — `R8Unorm` to `Rgba8Unorm` — and the arithmetic in
//! the pass that already runs, and what turning it off restores is the zero
//! sentinel every consumer answers with the shading normal.
//!
//! # The intensity is a third switch, and it buys nothing
//!
//! [`r_ssao_intensity`] is not on that ladder: it costs one comparison in the
//! pass that already runs, and what it changes is how much of the occlusion the
//! horizons measured a frame actually shows.
//! `shaders/ssao_upsample.slang`'s `ao_intensity` is the whole of it — the
//! reconstructed visibility raised to the exponent this writes, at the end of
//! the chain and before `mesh.slang` tints it.
//!
//! **A power and not a blend towards one**, which is what lets it ask for
//! *more* occlusion than the integral found: `docs/backlog.md` records the
//! multi-bounce tint narrowing the contrast of every occluded surface, and a
//! blend could only weaken it further. The default is the exponent that changes
//! nothing, so this switch too leaves the frame every golden was blessed at.
//!
//! None of the four is a quality preset, and none is set by one.
//! `crcbl::settings::presets` writes the `[engine.video]` keys of a tier, and
//! `docs/plan/39-capabilities.md`'s tier table has no row for the occlusion
//! chain — so all four stay what they are: variables declared beside the pass
//! that reads them, the way `crate::debug_draw`'s switch is. What a tier should
//! spend on them is `docs/backlog.md`'s, and so is the open question a
//! `VIDEO_KEYS` row would answer: a preset clears an effect by writing that
//! effect's key, so a knob a preset selects needs a row of its own.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, Format, GraphicsPipelineHandle, HalError,
    ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, SampleType, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{SSAO, SSAO_BLUR, SSAO_UPSAMPLE, ssao};

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

crcbl_console::convar! {
    /// Exponent the measured occlusion is raised to: 1 ships, over it darkens.
    pub static r_ssao_intensity: f32 in 0.25 ..= 4.0 = 1.0;
}

crcbl_console::convar! {
    /// Gather a bent direction beside the scalar and steer the ambient by it.
    pub static r_ssao_bent_normals: bool = true;
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

/// [`r_ssao_intensity`] as the shader's uniform wants it.
///
/// Clamped on the way through for [`slice_count`]'s reason exactly: the
/// variable's own range is the console's guard and this is the one that holds
/// if `shaders/ssao_upsample.slang`'s bounds ever move inside it.
/// `ao_intensity` clamps again on its side, which is where a value that never
/// reached this function at all is caught — and where a zero is answered with
/// `crcbl_shaders::ssao::INTENSITY_DEFAULT` rather than with the floor, because
/// every visibility raised to zero is one and that frame has no occlusion in it.
pub(crate) fn intensity() -> f32 {
    r_ssao_intensity
        .get_f32()
        .clamp(ssao::INTENSITY_MIN, ssao::INTENSITY_MAX)
}

/// [`r_ssao_bent_normals`] as the shader's uniform wants it.
///
/// **On by default**, unlike the two switches above, which default to what
/// ships: the direction is what `docs/plan/46-ambient-occlusion.md` calls the
/// half of the AO rung worth having, so the frame a person gets without
/// touching anything is the one with it. Turning it off leaves the gather
/// writing the zero sentinel, and every consumer answers that with the shading
/// normal it already had — see `shaders/ssao.slang`'s `bent_normals`.
pub(crate) fn bent_normals() -> bool {
    r_ssao_bent_normals.get_bool()
}

/// The extent the march and the blur run at: `extent` divided by
/// `shaders/ssao.slang`'s `RESOLUTION_DIVISOR` on each axis, **rounded up**.
///
/// **Rounded up, which is what keeps a small frame drawable.** A floor takes
/// every extent under the divisor to zero, and a zero-sized transient is an
/// image no backend will create — so a one-pixel scene would be a frame that
/// fails to build rather than a frame with one occlusion sample in it. The
/// ceiling is also what keeps the two grids covering: every pixel of the scene
/// has a sample at or before it, which is where
/// `shaders/ssao_upsample.slang` reads its nearest tap. What it costs is at most
/// one sample of overhang on an odd axis, whose taps the upsample clamps back
/// into the image.
///
/// **The one place the division is spelled in this crate.** The shaders hold the
/// same constant — `crcbl_shaders::ssao::RESOLUTION_DIVISOR` is the mirror they
/// are checked against — and a second halving written somewhere else is an image
/// the passes would render a fraction of, with nothing to say so but the
/// picture.
pub(crate) fn half_extent(extent: (u32, u32)) -> (u32, u32) {
    let divisor = ssao::RESOLUTION_DIVISOR;
    (extent.0.div_ceil(divisor), extent.1.div_ceil(divisor))
}

/// [`r_ssao_blur_passes`] as a pass count, clamped into what
/// [`Ssao::add_passes`] can record.
pub(crate) fn blur_passes() -> u32 {
    let asked = r_ssao_blur_passes
        .get_i64()
        .clamp(1, i64::from(Ssao::MAX_BLUR_PASSES));
    u32::try_from(asked).unwrap_or(1)
}

/// The images one frame's occlusion chain works in.
///
/// A shape rather than four positional arguments, and not only for
/// [`Ssao::add_passes`]' argument count: three of these are the same thing — a
/// working image at [`half_extent`] — and the fourth is the one that is not,
/// which is exactly the distinction a positional list buries. `crate::forward`
/// is what creates them, and [`Ssao::add_passes`] is where each one's contents
/// and extent are stated.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OcclusionImages {
    /// Where the march writes, at [`half_extent`].
    pub(crate) raw: ImageId,
    /// Where the first blur writes, at [`half_extent`].
    pub(crate) blurred: ImageId,
    /// Where a second blur writes, at [`half_extent`]. [`Some`] exactly when
    /// [`r_ssao_blur_passes`] asked for one.
    pub(crate) again: Option<ImageId>,
    /// Where the reconstruction writes: the **frame's own extent**, and the
    /// image the forward pass binds.
    pub(crate) upsampled: ImageId,
}

/// Everything the occlusion chain owns.
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
    /// The reconstruction's pipeline.
    ///
    /// **Its own pipeline on the blur's layout**, and that pairing is the whole
    /// of what the two passes share: `shaders/ssao_upsample.slang` declares the
    /// same three bindings in the same order — the block, an `R8Unorm` occlusion
    /// image, the scene's depth — so one layout describes both, while the shader
    /// behind it answers a different question. Two layouts here would be two
    /// descriptions of one shape to keep in step, and the seam would allocate a
    /// second identical descriptor layout to hold them.
    upsample_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the reconstruction's group, cached against the blurred
    /// occlusion view and the depth view together.
    ///
    /// **Its own ring rather than [`Ssao::blur_groups`] reused**, for
    /// [`Ssao::blur_again_groups`]' reason exactly: this pass binds a different
    /// occlusion image from the blur that precedes it, so one cache shared
    /// between them would be rebuilt twice a frame.
    upsample_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
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

    /// The most passes [`Ssao::add_passes`] can add to a frame: the march, every
    /// blur behind it, and the reconstruction that ends the chain.
    ///
    /// A **ceiling**, which is what `crate::forward`'s `RENDER_PASSES` needs
    /// from every term it adds up — see [`Ssao::passes`] for what a given frame
    /// actually records.
    pub(crate) const MAX_PASSES: u32 = Self::passes(Self::MAX_BLUR_PASSES);

    /// Passes [`Ssao::add_passes`] adds to a frame that asked for `blurs` of
    /// them: the occlusion march, one per blur, and the reconstruction.
    ///
    /// **The reconstruction is not conditional on anything.** It is what makes
    /// the half-resolution chain a full-resolution channel — see this module's
    /// header — so a frame that records the march records this as well.
    pub(crate) const fn passes(blurs: u32) -> u32 {
        1 + blurs + 1
    }

    /// Builds every pipeline and the uniform ring.
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

        let targets = [ColorTargetState::opaque(Format::Rgba8Unorm)];
        let pipeline = build_fullscreen(device, "ssao", &SSAO, pipeline_layout, &targets)?;
        let blur_pipeline = build_fullscreen(
            device,
            "ssao blur",
            &SSAO_BLUR,
            blur_pipeline_layout,
            &targets,
        )?;
        // **The blur's layout, and the same colour target.** The reconstruction
        // writes the same `R8Unorm` channel at a different extent, and an extent
        // is not part of a pipeline — the graph takes the render area from the
        // attachment it was given. See [`Ssao::upsample_pipeline`] for why the
        // layout is shared and the pipeline is not.
        let upsample_pipeline = build_fullscreen(
            device,
            "ssao upsample",
            &SSAO_UPSAMPLE,
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
            upsample_pipeline,
            upsample_groups: vec![None; frames],
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

    /// Adds the `ssao` pass, its blurs and the reconstruction, in that order,
    /// and returns the image the forward pass should read.
    ///
    /// `depth` must be the prepass's stored depth, at the scene's own extent.
    /// [`OcclusionImages`] is where the chain works: its three working images
    /// must all be [`half_extent`] of that depth, and its `upsampled` is the
    /// full-extent channel the last pass writes. Every occlusion image is the
    /// caller's so it can declare the read on the pass that consumes the last of
    /// them: a pass declares its own accesses and this one cannot declare the
    /// forward pass's.
    ///
    /// **The two extents are the caller's to get right and the graph is what
    /// checks them.** A render pass takes its area from its colour attachment —
    /// see `RenderGraph::compile` — so a working image handed in at the scene's
    /// size is not an error anywhere, it is a march that runs four times and a
    /// reconstruction that reads the corner of its input.
    ///
    /// **The returned id is the one to bind**, rather than the caller assuming
    /// any particular image, because which one holds the finished channel is
    /// this function's business and gets it wrong silently — a forward pass
    /// bound to the half-resolution image draws a frame nobody would question.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        depth: ImageId,
        images: OcclusionImages,
    ) -> ImageId {
        let OcclusionImages {
            raw,
            blurred,
            again,
            upsampled,
        } = images;
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let layout = self.layout;
        let uniforms = self.uniforms[frame];
        // Split so the closures below borrow different halves of `self`; one
        // `&mut self` shared between them is what the borrow checker refuses, and
        // it would be refusing something genuinely wrong — a pass body may run at
        // any point after it is declared.
        let (groups, blur_groups, again_groups, upsample_groups) = (
            &mut self.groups,
            &mut self.blur_groups,
            &mut self.blur_again_groups,
            &mut self.upsample_groups,
        );
        let cached = &mut groups[frame];
        let blur_cached = &mut blur_groups[frame];
        let again_cached = &mut again_groups[frame];
        let upsample_cached = &mut upsample_groups[frame];

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

        let blur = Filter {
            uniforms,
            layout: self.blur_layout,
            pipeline_layout: self.blur_pipeline_layout,
            pipeline: self.blur_pipeline,
        };
        blur.add_pass(graph, "ssao-blur", blur_cached, raw, depth, blurred);
        let filtered = match again {
            // The second blur reads the first one's output and writes its own
            // image. **Not back into `raw`**, which would be a write-after-read
            // on an image this same frame's march wrote and this same frame's
            // first blur read: the graph would have to order it, the pool could
            // not alias it, and the saving is one `R8Unorm` image on the frames
            // that asked for this at all.
            Some(again) => {
                blur.add_pass(graph, "ssao-blur-2", again_cached, blurred, depth, again);
                again
            }
            None => blurred,
        };
        // The reconstruction, reading whichever blur ran last and the scene's
        // depth, and writing the full-extent channel. **The same pass body as a
        // blur**, because it is the same three bindings and one triangle — what
        // differs is the pipeline and the extent of the image it was handed, and
        // neither of those is something this function does differently.
        let upsample = Filter {
            pipeline: self.upsample_pipeline,
            ..blur
        };
        upsample.add_pass(
            graph,
            "ssao-upsample",
            upsample_cached,
            filtered,
            depth,
            upsampled,
        );
        upsampled
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self
            .groups
            .into_iter()
            .chain(self.blur_groups)
            .chain(self.blur_again_groups)
            .chain(self.upsample_groups)
            .flatten()
        {
            device.destroy_bind_group(cached.1);
        }
        // Before the layout it was built against, which is the blur's — see
        // [`Ssao::upsample_pipeline`].
        device.destroy_graphics_pipeline(self.upsample_pipeline);
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

/// The pipeline, layout and uniform block every pass behind the march shares.
///
/// All of them take one occlusion image, the scene's depth and the block, and
/// draw one triangle into an `R8Unorm` target: the second blur is the *same
/// shader* reading the first one's output — see this module's header on why
/// there can be a second at all — and the reconstruction is a different shader
/// on the same layout, which is [`Ssao::upsample_pipeline`]'s paragraph. So what
/// differs between them is a pipeline, three image ids and a label, and
/// everything else is this. Gathered into a type rather than passed as seven
/// arguments, and extracted the moment there were two callers rather than left
/// as a copy of the pass body with two ids changed.
#[derive(Clone, Copy)]
struct Filter {
    uniforms: BufferHandle,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
}

impl Filter {
    /// Adds one pass: `source` and `depth` in, `target` out.
    ///
    /// `cached` is this pass's own group ring slot — each of these passes binds
    /// a different source image, so sharing one slot between them would rebuild
    /// the group once per pass per frame.
    ///
    /// **`target` is what decides the extent the pass runs at.** The graph takes
    /// a render pass's area from its colour attachment, so the same body records
    /// a half-extent blur and a full-extent reconstruction without either being
    /// told which it is.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The gather extent **rounds up**, so no axis of a drawable frame reaches
    /// zero.
    ///
    /// A floor is the spelling this would otherwise have, and it is wrong at
    /// both ends: it takes a frame narrower than the divisor to a zero-width
    /// transient, which is an image no backend creates and therefore a frame
    /// that fails rather than a frame with one occlusion sample in it, and it
    /// leaves an odd axis one sample short of its own last pixel — see
    /// [`tests::every_pixel_of_a_frame_has_a_sample_at_or_before_it`].
    ///
    /// [`tests::every_pixel_of_a_frame_has_a_sample_at_or_before_it`]: fn@every_pixel_of_a_frame_has_a_sample_at_or_before_it
    #[test]
    fn the_gather_extent_rounds_up() {
        assert_eq!(
            half_extent((1920, 1080)),
            (960, 540),
            "an even frame halves exactly"
        );
        assert_eq!(
            half_extent((1921, 1081)),
            (961, 541),
            "an odd axis keeps the sample its last pixel needs"
        );
        assert_eq!(
            half_extent((1, 1)),
            (1, 1),
            "and the smallest frame there is still has one sample in it"
        );
    }

    /// Every pixel of the frame has an occlusion sample at or before it.
    ///
    /// `shaders/ssao_upsample.slang` divides its pixel by the same constant to
    /// find the first of the samples it reads, and clamps the result into the
    /// image. That clamp is there for the *second* tap, which genuinely runs off
    /// the edge; a gather extent that floored would put the last row and column
    /// of an odd frame past the last sample as well, and the clamp would answer
    /// silently for the first tap too — reconstructing the frame's far edge from
    /// the sample before the one it wanted.
    ///
    /// Swept rather than spot-checked, because the property is about the odd
    /// extents and a fixture is only ever one of them.
    #[test]
    fn every_pixel_of_a_frame_has_a_sample_at_or_before_it() {
        for width in 1..=64u32 {
            let (samples, _) = half_extent((width, width));
            let nearest = (width - 1) / ssao::RESOLUTION_DIVISOR;
            assert!(
                nearest < samples,
                "the last pixel of a {width}-wide frame reads sample {nearest} of {samples}, so \
                 the upsample's clamp is standing in for a sample that is not there"
            );
        }
    }

    /// The console's range is the shader's range, and its default is the
    /// exponent that changes nothing.
    ///
    /// `convar!` takes its bounds as literals — a macro cannot read a constant
    /// — so the two numbers in the declaration above are a copy of
    /// `crcbl_shaders::ssao`'s, and this is what holds them to it. A console
    /// range wider than the shader's is a value a person can set and the frame
    /// silently ignores; narrower, and part of the shader's range is
    /// unreachable. Neither shows up as anything but a knob that does less than
    /// it says.
    #[test]
    fn the_console_range_is_the_range_the_reconstruction_honours() {
        assert_eq!(
            r_ssao_intensity.kind(),
            crcbl_console::Kind::Float {
                min: ssao::INTENSITY_MIN,
                max: ssao::INTENSITY_MAX,
            },
            "`r_ssao_intensity` accepts a range `shaders/ssao_upsample.slang` does not honour"
        );
        assert_eq!(
            intensity(),
            ssao::INTENSITY_DEFAULT,
            "a frame nobody has touched the console on must reach the shader at the exponent \
             every golden in this workspace was blessed under"
        );
    }
}
