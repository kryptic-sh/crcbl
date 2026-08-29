//! `docs/plan/49-antialiasing.md`'s antialiasing ladder, second rung: the three
//! SMAA 1x passes that resolve the tonemapped frame into the target.
//!
//! # It takes the resolve slot, it does not stand beside FXAA
//!
//! [`crate::fxaa`]'s header describes that slot: the pass that reads what the
//! tonemap wrote and writes what the UI is composited onto, which is why
//! switching antialiasing on changes the *shape* of the frame rather than
//! adding a pass to it. This module is the same slot filled by a better and
//! more expensive filter. **The two are never both recorded** — a tier that is
//! off is a frame with fewer passes, which is what
//! [`RenderEffects`](crate::RenderEffects) means by a toggle — and
//! [`crate::forward`] owns that choice because it owns both ends.
//!
//! Three passes where FXAA is one, and the middle one is why:
//!
//! 1. **`smaa-edges`** reads the tonemapped frame and writes a two-channel mask
//!    saying which pixels have an edge on their west or north side.
//! 2. **`smaa-weights`** searches along each edge for the pattern it belongs to
//!    and reads that pattern's coverage out of two precomputed tables.
//! 3. **`smaa-blend`** mixes each pixel with the neighbour its weights point
//!    at, and writes the caller's target.
//!
//! # The tables are uploaded once, and the sampler is what makes them work
//!
//! `crcbl_shaders::smaa` holds both as committed bytes, so they arrive here as
//! two `upload_texture` calls at build time and are read by every frame after —
//! the `dfg` table's shape exactly, and for the same reason that module gives:
//! a table four rasterisers derived independently would sit underneath every
//! golden in the suite.
//!
//! **The area lookup relies on bilinear filtering.** A pattern's length reaches
//! the table as the square root of a pixel count, which lands *between* the
//! tabulated texels, and the value read out is meant to be the interpolation of
//! the two either side. A nearest sampler would compile, bind, draw an
//! antialiased frame, and quantise every blend weight to the nearest tabulated
//! distance — a quality bug with no symptom a test would name. So the sampler
//! these three passes share is **linear and clamp-to-edge**, and the edge pass
//! takes a **nearest** one of its own because every fetch it makes is a whole
//! texel away and wants that texel's own value.

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, Device, FilterMode, Format, GraphicsPipelineHandle,
    HalError, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation, PipelineLayoutDesc,
    PipelineLayoutHandle, QueueHandle, SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle,
    ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::{SMAA_BLEND, SMAA_EDGES, SMAA_WEIGHTS, smaa};

use crate::graph::{ImageId, RenderGraph};
use crate::ssao::cached_group;
use crate::texture::{UploadedTexture, upload_texture};

/// Vertices in the over-sized full-screen triangle each `smaa_*.slang` source
/// generates from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// The format the edge mask is written in: two channels of eight bits, each
/// holding zero or one.
///
/// A binary mask, so eight bits is seven more than it needs; there is no
/// two-channel one-bit colour format on any of the four backends, and
/// `Rg8Unorm` is the narrowest that reads back as exactly `0.0` or `1.0`, which
/// the blend-weight pass's comparisons depend on.
pub(crate) const EDGES_FORMAT: Format = Format::Rg8Unorm;

/// The format the blend weights are written in: one weight per side of the
/// pixel, each a coverage fraction in `0..=1`.
///
/// Eight bits per channel on [`crate::graph`]'s `ambient_occlusion` argument:
/// the value is a fraction of a pixel, and the quantisation is far below what
/// a filter mixing two neighbouring colours can resolve.
pub(crate) const WEIGHTS_FORMAT: Format = Format::Rgba8Unorm;

/// Everything the three passes own.
///
/// Built once by [`Smaa::new`] and released by [`Smaa::destroy`], which is the
/// shape every other resource group in this crate has — see [`crate::ssao`].
#[derive(Debug)]
pub(crate) struct Smaa {
    /// `[frame]`: the block all three passes read. One buffer per frame in
    /// flight, and one *block* for the three passes — they run at one extent,
    /// and `crcbl_shaders::smaa::SmaaParams` says why that is one block rather
    /// than three that agree.
    uniforms: Vec<BufferHandle>,
    edges_layout: BindGroupLayoutHandle,
    edges_pipeline_layout: PipelineLayoutHandle,
    edges_pipeline: GraphicsPipelineHandle,
    weights_layout: BindGroupLayoutHandle,
    weights_pipeline_layout: PipelineLayoutHandle,
    weights_pipeline: GraphicsPipelineHandle,
    blend_layout: BindGroupLayoutHandle,
    blend_pipeline_layout: PipelineLayoutHandle,
    blend_pipeline: GraphicsPipelineHandle,
    /// **Nearest**, for the edge pass alone — see the module docs.
    point_sampler: SamplerHandle,
    /// **Linear** and clamp-to-edge, for the two passes that read between
    /// texels — see the module docs, which is where the area table's dependence
    /// on it is written down.
    linear_sampler: SamplerHandle,
    /// `crcbl_shaders::smaa::area_bytes`, uploaded once as an `Rg8Unorm` image.
    area: UploadedTexture,
    /// `crcbl_shaders::smaa::search_bytes`, uploaded once as an `R8Unorm` image.
    search: UploadedTexture,
    /// `[frame]`: the edge pass's group, cached against the source view.
    ///
    /// **One per frame in flight** for [`crate::fxaa`]'s reason: the group names
    /// [`Smaa::uniforms`] as well as the transient, and that is a ring.
    edges_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the weight pass's group, cached against the edge mask's view.
    weights_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    /// `[frame]`: the blend pass's group, cached against the source and the
    /// weights views together.
    blend_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

/// One sampled-image entry of a bind group layout.
fn sampled(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// One sampler entry.
fn sampler_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler { comparison: false },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// The uniform-block entry.
///
/// A uniform buffer rather than a push constant, for the reason every post pass
/// in this crate gives: WebGPU has no push constants, and one Slang entry point
/// cannot read both a push-constant block and a bound one, so a range here
/// would fork three shaders.
fn uniform_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::UniformBuffer { dynamic: false },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

impl Smaa {
    /// Passes [`Smaa::add_passes`] adds to a frame.
    pub(crate) const PASSES: u32 = 3;

    /// Builds the three pipelines, the two samplers, the two tables and the
    /// uniform ring.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`crate::fxaa::Fxaa::new`]'s terms exactly.
    ///
    /// `target_format` is what the third pass writes, which is the caller's
    /// target — and, because the tonemap writes an intermediate of the same
    /// description, also the format the first and third passes read.
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
        target_format: Format,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        // Declaration order is binding order in every one of these, because the
        // sources declare their resources in the order they number them and
        // Slang's Metal target follows the declarations — see
        // `crcbl_shaders::declaration_order`.
        let edges_entries = [sampled(0), sampler_entry(1), uniform_entry(2)];
        let edges_desc = BindGroupLayoutDesc {
            label: Some("smaa edges"),
            entries: &edges_entries,
        };
        let weights_entries = [
            sampled(0),
            sampled(1),
            sampled(2),
            sampler_entry(3),
            uniform_entry(4),
        ];
        let weights_desc = BindGroupLayoutDesc {
            label: Some("smaa weights"),
            entries: &weights_entries,
        };
        let blend_entries = [sampled(0), sampled(1), sampler_entry(2), uniform_entry(3)];
        let blend_desc = BindGroupLayoutDesc {
            label: Some("smaa blend"),
            entries: &blend_entries,
        };
        check_portable_storage_buffers(Some("smaa"), &[&edges_desc, &weights_desc, &blend_desc])?;

        let edges_layout = device.create_bind_group_layout(&edges_desc)?;
        let edges_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("smaa edges"),
            bind_group_layouts: &[edges_layout],
            push_constants: None,
        })?;
        let edges_pipeline = build_fullscreen(
            device,
            "smaa-edges",
            &SMAA_EDGES,
            edges_pipeline_layout,
            &[ColorTargetState::opaque(EDGES_FORMAT)],
        )?;

        let weights_layout = device.create_bind_group_layout(&weights_desc)?;
        let weights_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("smaa weights"),
            bind_group_layouts: &[weights_layout],
            push_constants: None,
        })?;
        let weights_pipeline = build_fullscreen(
            device,
            "smaa-weights",
            &SMAA_WEIGHTS,
            weights_pipeline_layout,
            &[ColorTargetState::opaque(WEIGHTS_FORMAT)],
        )?;

        let blend_layout = device.create_bind_group_layout(&blend_desc)?;
        let blend_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("smaa blend"),
            bind_group_layouts: &[blend_layout],
            push_constants: None,
        })?;
        let blend_pipeline = build_fullscreen(
            device,
            "smaa-blend",
            &SMAA_BLEND,
            blend_pipeline_layout,
            &[ColorTargetState::opaque(target_format)],
        )?;

        // **Nearest**, and only the edge pass takes it — see the module docs.
        let point_sampler = device.create_sampler(&SamplerDesc {
            label: Some("smaa source"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        // **Linear**, and the area table's interpolation is what needs it.
        let linear_sampler = device.create_sampler(&SamplerDesc {
            label: Some("smaa tables"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;

        let area = upload_texture(
            device,
            queue,
            "smaa area table",
            // Two bytes a texel: the coverage below the pattern's line in red
            // and above it in green — `crcbl_shaders::smaa::AREA_TEXEL_BYTES`.
            Format::Rg8Unorm,
            area_extent().0,
            area_extent().1,
            smaa::area_bytes(),
        )?;
        let search = upload_texture(
            device,
            queue,
            "smaa search table",
            Format::R8Unorm,
            search_extent().0,
            search_extent().1,
            smaa::search_bytes(),
        )?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("smaa params"),
                size: smaa::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            uniforms,
            edges_layout,
            edges_pipeline_layout,
            edges_pipeline,
            weights_layout,
            weights_pipeline_layout,
            weights_pipeline,
            blend_layout,
            blend_pipeline_layout,
            blend_pipeline,
            point_sampler,
            linear_sampler,
            area,
            search,
            edges_groups: (0..frames).map(|_| None).collect(),
            weights_groups: (0..frames).map(|_| None).collect(),
            blend_groups: (0..frames).map(|_| None).collect(),
        })
    }

    /// Writes this frame's block.
    ///
    /// The extent is the frame's, and it is the only thing the three shaders
    /// cannot derive — see [`crcbl_shaders::smaa::SmaaParams::for_extent`],
    /// whose default leaves it zero precisely so an unwritten block draws a tell
    /// rather than something that nearly works.
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
    ) -> Result<(), HalError> {
        let params = smaa::SmaaParams::for_extent(extent.0, extent.1);
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds the three passes: `source` through `edges` and `weights` into
    /// `target`.
    ///
    /// `edges` and `weights` are the caller's because every other multi-pass
    /// effect in this crate takes its transients that way — see
    /// [`crate::ssao::Ssao::add_passes`] — and because the graph is where a
    /// frame's images are declared.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        source: ImageId,
        edges: ImageId,
        weights: ImageId,
        target: ImageId,
    ) {
        let point_sampler = self.point_sampler;
        let linear_sampler = self.linear_sampler;
        let (area_view, search_view) = (self.area.view, self.search.view);
        let uniforms = self.uniforms[frame];
        let (edges_pipeline, edges_pipeline_layout, edges_layout) = (
            self.edges_pipeline,
            self.edges_pipeline_layout,
            self.edges_layout,
        );
        let (weights_pipeline, weights_pipeline_layout, weights_layout) = (
            self.weights_pipeline,
            self.weights_pipeline_layout,
            self.weights_layout,
        );
        let (blend_pipeline, blend_pipeline_layout, blend_layout) = (
            self.blend_pipeline,
            self.blend_pipeline_layout,
            self.blend_layout,
        );
        // Split so the three closures below borrow different halves of `self`;
        // one `&mut self` shared between them is what the borrow checker
        // refuses, and it would be refusing something genuinely wrong — a pass
        // body may run at any point after it is declared. `crate::ssao` splits
        // its two the same way.
        let Self {
            edges_groups,
            weights_groups,
            blend_groups,
            ..
        } = self;
        let edges_cached = &mut edges_groups[frame];
        let weights_cached = &mut weights_groups[frame];
        let blend_cached = &mut blend_groups[frame];

        graph
            .add_render_pass("smaa-edges")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the mask — a pixel with no edge is written as zero rather
            // than discarded, which `smaa_edges.slang`'s header records — so
            // loading or clearing it is pure bandwidth.
            .color(
                edges,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(source)
            .execute(move |ctx| {
                let view = ctx.image_view(source);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        // Overwritten by `cached_group` with the realised view;
                        // written here so the list is a complete description of
                        // the layout rather than a list with a hole in it.
                        resource: BindingResource::ImageView(view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::Sampler(point_sampler),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                ];
                let Some(group) = cached_group(
                    edges_cached,
                    device,
                    &[(0, view)],
                    "smaa edges source",
                    edges_layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(edges_pipeline);
                encoder.bind_group(0, group, &[], edges_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });

        graph
            .add_render_pass("smaa-weights")
            // `DontCare` on the edge pass's terms: a pixel the mask says has no
            // edge is written as a zero weight rather than skipped, so the
            // triangle covers the target and a load is pure bandwidth.
            .color(
                weights,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(edges)
            .execute(move |ctx| {
                let view = ctx.image_view(edges);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(area_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(search_view),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::Sampler(linear_sampler),
                    },
                    BindGroupEntry {
                        binding: 4,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                ];
                // Only the mask is keyed on: the two tables are this struct's
                // own images and outlive every frame, so a cache keyed on them
                // as well would compare two handles that never move.
                let Some(group) = cached_group(
                    weights_cached,
                    device,
                    &[(0, view)],
                    "smaa weights edges",
                    weights_layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(weights_pipeline);
                encoder.bind_group(0, group, &[], weights_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });

        graph
            .add_render_pass("smaa-blend")
            .color(
                target,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(source)
            .read_image(weights)
            .execute(move |ctx| {
                let source_view = ctx.image_view(source);
                let weights_view = ctx.image_view(weights);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(source_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(weights_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::Sampler(linear_sampler),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(uniforms),
                    },
                ];
                let Some(group) = cached_group(
                    blend_cached,
                    device,
                    &[(0, source_view), (1, weights_view)],
                    "smaa blend source",
                    blend_layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(blend_pipeline);
                encoder.bind_group(0, group, &[], blend_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self
            .edges_groups
            .into_iter()
            .chain(self.weights_groups)
            .chain(self.blend_groups)
            .flatten()
        {
            device.destroy_bind_group(cached.1);
        }
        self.search.destroy(device);
        self.area.destroy(device);
        device.destroy_sampler(self.linear_sampler);
        device.destroy_sampler(self.point_sampler);
        device.destroy_graphics_pipeline(self.blend_pipeline);
        device.destroy_graphics_pipeline(self.weights_pipeline);
        device.destroy_graphics_pipeline(self.edges_pipeline);
        device.destroy_pipeline_layout(self.blend_pipeline_layout);
        device.destroy_pipeline_layout(self.weights_pipeline_layout);
        device.destroy_pipeline_layout(self.edges_pipeline_layout);
        device.destroy_bind_group_layout(self.blend_layout);
        device.destroy_bind_group_layout(self.weights_layout);
        device.destroy_bind_group_layout(self.edges_layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}

/// The area table's extent, as the seam wants it.
///
/// A function rather than a pair of constants because the conversion is the
/// whole content: `crcbl_shaders::smaa` counts texels in `usize` and an image
/// extent is `u32`, and a table too large for one is a table that could not be
/// uploaded at all.
fn area_extent() -> (u32, u32) {
    (
        u32::try_from(smaa::AREA_WIDTH).unwrap_or_else(|_| unreachable!("AREA_WIDTH fits a u32")),
        u32::try_from(smaa::AREA_HEIGHT).unwrap_or_else(|_| unreachable!("AREA_HEIGHT fits a u32")),
    )
}

/// The search table's extent, likewise.
fn search_extent() -> (u32, u32) {
    (
        u32::try_from(smaa::SEARCH_WIDTH)
            .unwrap_or_else(|_| unreachable!("SEARCH_WIDTH fits a u32")),
        u32::try_from(smaa::SEARCH_HEIGHT)
            .unwrap_or_else(|_| unreachable!("SEARCH_HEIGHT fits a u32")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The extents handed to the two uploads are the tables' own**, and the
    /// bytes are exactly what an image of that extent and format holds.
    ///
    /// The upload refuses a length that does not match, so this would fail at
    /// build time on a real device — but the null backend that every
    /// `crcbl-render` test opens accepts any bytes, and the one thing a wrong
    /// extent produces on a real one is a table read at a stride nothing here
    /// would name.
    #[test]
    fn the_table_extents_match_the_bytes_they_upload() {
        let (width, height) = area_extent();
        assert_eq!(
            (width as usize) * (height as usize) * smaa::AREA_TEXEL_BYTES,
            smaa::area_bytes().len()
        );

        let (width, height) = search_extent();
        assert_eq!(
            (width as usize) * (height as usize),
            smaa::search_bytes().len()
        );
    }
}
