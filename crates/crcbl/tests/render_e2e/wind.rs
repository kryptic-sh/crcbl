//! `docs/plan/56-wind.md`'s **CPU–GPU agreement**: "the GPU sample read back at
//! fixed points against the CPU sample, within a stated tolerance, on every
//! backend".
//!
//! The CPU copy is authoritative — decision 6 — so this is not a comparison of
//! two implementations. It is a measurement of how far the GPU's copy of the
//! one formula sits from the answer physics is already using, and the stated
//! bound is [`crcbl::shaders::wind::MAX_CPU_GPU_ERROR`], which that constant's
//! own docs price.
//!
//! # Why it lives in this suite
//!
//! `tests/run-render-e2e.sh` is the runner that goes round both a software
//! rasteriser and the hardware adapter, which is exactly what "on every
//! backend" needs of a claim about filtering hardware. Nothing here draws a
//! frame or compares a golden; it dispatches `shaders/wind.slang` over a list
//! of points and reads the velocities back.
//!
//! # Two layer pairs, for two different questions
//!
//! * **The committed pair** — `crates/crcbl-wind/assets/direction.png` and
//!   `intensity.png`, the layers the CPU suite samples — answers "does the
//!   shader agree about the field this engine ships".
//! * **A deliberately harsh pair**, built here, answers "how far can the two
//!   possibly be". A linear filter's guaranteed precision is a fraction of the
//!   *difference between neighbouring texels*, so the worst case is a layer
//!   whose neighbours are as far apart as an eight-bit texel can be. Measuring
//!   the tolerance on the smooth pair alone would be measuring the pair.

use std::path::Path;

use crcbl::hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindingFlags, BindingKind, BindingResource, BufferDesc, BufferImageCopy, BufferUsage,
    ComputePassDesc, ComputePipelineDesc, Extent3d, Features, FilterMode, ImageAspect,
    ImageBarrier, ImageDesc, ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage,
    ImageViewDesc, ImageViewType, MemoryLocation, Offset3d, PipelineLayoutDesc, ResourceState,
    SampleType, SamplerAddressMode, SamplerDesc, ShaderEntry, ShaderModuleDesc, ShaderStages,
    SubmitInfo,
};
use crcbl::shaders::wind::{
    MAX_CPU_GPU_ERROR, PARAMS_SIZE, PROBE_PARAMS_SIZE, ProbeParams, WORKGROUP_SIZE,
};
use crcbl_assets::MemorySource;
use crcbl_wind::{
    Beaufort, DirectionLayer, IntensityLayer, LayerGrid, Weather, WindField, load_direction_layer,
    load_intensity_layer,
};
use glam::{DVec2, DVec3};

use crate::harness::{Headless, poisoned};

/// The committed direction layer, and the scale its generator drew it at.
const DIRECTION_PNG: &[u8] = include_bytes!("../../../crcbl-wind/assets/direction.png");

/// Metres per texel of [`DIRECTION_PNG`], from
/// `crates/crcbl-wind/tools/authored.rs`.
const DIRECTION_METRES_PER_TEXEL: f64 = 8.0;

/// The committed intensity layer.
const INTENSITY_PNG: &[u8] = include_bytes!("../../../crcbl-wind/assets/intensity.png");

/// Metres per texel of [`INTENSITY_PNG`], from
/// `crates/crcbl-wind/tools/authored.rs`.
const INTENSITY_METRES_PER_TEXEL: f64 = 2.0;

/// The extent the fixture's unused image ring is created at. Nothing is drawn
/// into it; [`Headless`] wants one because every other suite that opens it
/// presents.
const EXTENT: (u32, u32) = (64, 48);

/// Where the camera sits for every run.
///
/// Deliberately not the origin and not a multiple of either layer's extent: the
/// shader reads positions *relative* to this, so an origin camera would leave
/// the whole camera-relative construction untested.
const CAMERA: DVec3 = DVec3::new(137.5, 4.0, -61.25);

/// Camera-relative positions the field is sampled at, on both sides.
///
/// A prime-ish stride in each axis so the list never settles onto a texel
/// lattice of either layer, and a spread wider than both layers so the `repeat`
/// addressing is crossed in both directions. The count is a multiple of
/// [`WORKGROUP_SIZE`] so the dispatch is exact.
/// `pixels` with each row padded to `alignment` bytes, and the padded row's
/// width in texels for [`BufferImageCopy::buffer_row_length`].
///
/// A copy's row pitch is the backend's business and they disagree: D3D12
/// requires a multiple of 256 bytes and refuses anything else by name, where
/// Vulkan takes a tightly packed row. `crcbl_render`'s own upload path pads for
/// exactly this reason, and a fixture that hand-rolls an upload owes the same
/// arithmetic — a 4-texel-wide layer's row is 16 bytes.
fn padded_rows(pixels: &[u8], width: u32, alignment: u64) -> (Vec<u8>, u32) {
    const TEXEL: usize = 4;
    let row_bytes = width as usize * TEXEL;
    let pitch = (row_bytes as u64).next_multiple_of(alignment.max(TEXEL as u64)) as usize;
    let mut padded = Vec::with_capacity(pixels.len() / row_bytes.max(1) * pitch);
    for row in pixels.chunks(row_bytes) {
        padded.extend_from_slice(row);
        padded.resize(padded.len() + pitch - row.len(), 0);
    }
    (padded, (pitch / TEXEL) as u32)
}

fn probe_points() -> Vec<DVec3> {
    (0..256)
        .map(|index| {
            let t = f64::from(index);
            DVec3::new(t * 1.37 - 170.0, t * 0.11 - 12.0, t * -2.19 + 210.0)
        })
        .collect()
}

/// The committed pair, loaded the way a game loads it.
fn authored_layers() -> (DirectionLayer, IntensityLayer) {
    let mut source = MemorySource::new();
    source
        .insert(Path::new("wind/direction.png"), DIRECTION_PNG.to_vec())
        .expect("a legal asset key");
    source
        .insert(Path::new("wind/intensity.png"), INTENSITY_PNG.to_vec())
        .expect("a legal asset key");
    (
        load_direction_layer(
            &source,
            Path::new("wind/direction.png"),
            DIRECTION_METRES_PER_TEXEL,
        )
        .expect("a direction layer"),
        load_intensity_layer(
            &source,
            Path::new("wind/intensity.png"),
            INTENSITY_METRES_PER_TEXEL,
        )
        .expect("an intensity layer"),
    )
}

/// The worst case a bilinear filter can be asked for: a checkerboard of the two
/// extremes an eight-bit texel can hold, so every tap interpolates across the
/// whole range.
fn harsh_pixels(size: u32, low: [u8; 4], high: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for row in 0..size {
        for column in 0..size {
            let extreme = if (row + column) % 2 == 0 { low } else { high };
            pixels.extend_from_slice(&extreme);
        }
    }
    pixels
}

/// A layer pair, and the bytes the GPU copy is uploaded from.
struct Layers {
    direction: DirectionLayer,
    intensity: IntensityLayer,
    direction_pixels: Vec<u8>,
    intensity_pixels: Vec<u8>,
}

impl Layers {
    /// The committed pair.
    fn authored() -> Self {
        let (direction, intensity) = authored_layers();
        let direction_pixels = crcbl_sprite::load::decode_png(DIRECTION_PNG)
            .expect("the committed file is a PNG")
            .pixels;
        let intensity_pixels = crcbl_sprite::load::decode_png(INTENSITY_PNG)
            .expect("the committed file is a PNG")
            .pixels;
        Self {
            direction,
            intensity,
            direction_pixels,
            intensity_pixels,
        }
    }

    /// The checkerboard pair.
    fn harsh() -> Self {
        let direction_grid = LayerGrid::new(16, 16, 8.0).expect("a grid");
        let intensity_grid = LayerGrid::new(16, 16, 2.0).expect("a grid");
        // The direction extremes are a quarter turn either side of the
        // prevailing wind, which is as far as a deflection can swing without
        // two neighbours cancelling into the [`crcbl_wind::
        // MIN_DIRECTION_LENGTH_SQUARED`] fallback — a case the two copies
        // decide independently and neither is wrong about.
        let direction_pixels = harsh_pixels(16, [181, 74, 0, 255], [181, 181, 0, 255]);
        let intensity_pixels = harsh_pixels(16, [0, 0, 0, 255], [255, 0, 0, 255]);
        Self {
            direction: DirectionLayer::from_rgba8(direction_grid, &direction_pixels)
                .expect("a direction layer"),
            intensity: IntensityLayer::from_rgba8(intensity_grid, &intensity_pixels)
                .expect("an intensity layer"),
            direction_pixels,
            intensity_pixels,
        }
    }
}

/// Everything one run of the probe owns on the device.
struct WindProbe {
    params: crcbl::hal::BufferHandle,
    probe_params: crcbl::hal::BufferHandle,
    points: crcbl::hal::BufferHandle,
    velocities: crcbl::hal::BufferHandle,
    staging: crcbl::hal::BufferHandle,
    images: [crcbl::hal::ImageHandle; 2],
    views: [crcbl::hal::ImageViewHandle; 2],
    sampler: crcbl::hal::SamplerHandle,
    layouts: [crcbl::hal::BindGroupLayoutHandle; 2],
    groups: [crcbl::hal::BindGroupHandle; 2],
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    pipeline: crcbl::hal::ComputePipelineHandle,
    uploads: Vec<crcbl::hal::BufferHandle>,
}

/// Bytes one `float4` takes in a storage buffer.
const VECTOR_STRIDE: u32 = 16;

impl WindProbe {
    /// Uploads the layers, the block and the point list, and builds the two
    /// bind groups and the pipeline.
    ///
    /// **Two bind groups, because the wind is one of them.** Set 0 is decision
    /// 6's bind group — the uniform block, the two layers and a sampler — and
    /// is what a consumer at a later rung binds unchanged. Set 1 is this
    /// probe's own list of points and the answers it takes back.
    fn new(headless: &Headless, field: &WindField, layers: &Layers, points: &[DVec3]) -> Self {
        let device = headless.device.as_ref();
        let count = u32::try_from(points.len()).expect("a probe list this side of four billion");
        let velocity_bytes = u64::from(count) * u64::from(VECTOR_STRIDE);
        let mut uploads = Vec::new();

        let mut host = |label: &str, bytes: &[u8]| {
            let buffer = device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size: bytes.len() as u64,
                    usage: BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a host-upload buffer");
            device
                .write_buffer(buffer, 0, bytes)
                .expect("a host-upload buffer is what write_buffer is for");
            uploads.push(buffer);
            buffer
        };

        let params_bytes = field.gpu_params(CAMERA).to_bytes();
        let probe_bytes = ProbeParams { count }.to_bytes();
        let point_bytes: Vec<u8> = points
            .iter()
            .flat_map(|point| {
                [point.x as f32, point.y as f32, point.z as f32, 0.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<u8>>()
            })
            .collect();

        let params_upload = host("wind params upload", &params_bytes);
        let probe_upload = host("wind probe params upload", &probe_bytes);
        let points_upload = host("wind points upload", &point_bytes);
        // **Rows padded to the device's copy alignment, not tightly packed.**
        // D3D12 requires a copy's row pitch to be a multiple of 256 bytes, and
        // a 4-texel-wide layer's row is 16 — which is why the fixture's own
        // tiny layers failed there and nowhere else. `crcbl_render`'s upload
        // path pads for the same reason.
        let alignment = device
            .caps()
            .limits
            .optimal_buffer_copy_offset_alignment
            .max(1);
        let (direction_rows, direction_row_texels) = padded_rows(
            &layers.direction_pixels,
            layers.direction.grid().width,
            alignment,
        );
        let (intensity_rows, intensity_row_texels) = padded_rows(
            &layers.intensity_pixels,
            layers.intensity.grid().width,
            alignment,
        );
        let direction_upload = host("wind direction layer upload", &direction_rows);
        let intensity_upload = host("wind intensity layer upload", &intensity_rows);

        let uniform = |label: &str, size: u64| {
            device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a uniform buffer")
        };
        let params = uniform("wind params", PARAMS_SIZE as u64);
        let probe_params = uniform("wind probe params", PROBE_PARAMS_SIZE as u64);

        let points_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("wind points"),
                size: point_bytes.len() as u64,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a storage buffer");
        let velocities = device
            .create_buffer(&BufferDesc {
                label: Some("wind velocities"),
                size: velocity_bytes,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a storage buffer");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("wind readback"),
                size: velocity_bytes,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a host-readback buffer");

        // `Rgba8Unorm`, not `Rgba8UnormSrgb`: these are data, and an sRGB view
        // would put a decode curve between the authored byte and the speed it
        // stands for that the CPU copy has no counterpart for.
        let image = |label: &str, grid: LayerGrid| {
            device
                .create_image(&ImageDesc {
                    label: Some(label),
                    image_type: ImageType::D2,
                    extent: Extent3d::d2(grid.width, grid.height),
                    format: crcbl::hal::Format::Rgba8Unorm,
                    mip_levels: 1,
                    samples: 1,
                    usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
                })
                .expect("a sampled image")
        };
        let images = [
            image("wind direction layer", layers.direction.grid()),
            image("wind intensity layer", layers.intensity.grid()),
        ];
        let views = [
            device
                .create_image_view(&ImageViewDesc {
                    label: Some("wind direction layer"),
                    image: images[0],
                    view_type: ImageViewType::D2,
                    format: crcbl::hal::Format::Rgba8Unorm,
                    range: ImageSubresourceRange {
                        aspect: ImageAspect::COLOR,
                        base_mip: 0,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                })
                .expect("a view"),
            device
                .create_image_view(&ImageViewDesc {
                    label: Some("wind intensity layer"),
                    image: images[1],
                    view_type: ImageViewType::D2,
                    format: crcbl::hal::Format::Rgba8Unorm,
                    range: ImageSubresourceRange {
                        aspect: ImageAspect::COLOR,
                        base_mip: 0,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                })
                .expect("a view"),
        ];

        // Linear and repeating, which is what `crcbl_wind::layers`' wrapped
        // texel indices and half-texel shift are the CPU statement of. No mip
        // chain exists, so the mip filter cannot be reached.
        let sampler = device
            .create_sampler(&SamplerDesc {
                label: Some("wind layers"),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mip_filter: FilterMode::Nearest,
                address_mode: [SamplerAddressMode::Repeat; 3],
                lod_min: 0.0,
                lod_max: 0.0,
                anisotropy: 1.0,
                compare: None,
            })
            .expect("a sampler");

        let wind_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::Sampler { comparison: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let probe_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                    stride: VECTOR_STRIDE,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                    stride: VECTOR_STRIDE,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let layouts = [
            device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("wind"),
                    entries: &wind_entries,
                })
                .expect("the wind layout"),
            device
                .create_bind_group_layout(&BindGroupLayoutDesc {
                    label: Some("wind probe"),
                    entries: &probe_entries,
                })
                .expect("the probe layout"),
        ];

        let wind_group = [
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(params),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::ImageView(views[0]),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: BindingResource::ImageView(views[1]),
            },
            BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: BindingResource::Sampler(sampler),
            },
        ];
        let probe_group = [
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(probe_params),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::whole_buffer(points_buffer),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: BindingResource::whole_buffer(velocities),
            },
        ];
        let groups = [
            device
                .create_bind_group(&BindGroupDesc {
                    label: Some("wind"),
                    layout: layouts[0],
                    entries: &wind_group,
                    variable_count: None,
                })
                .expect("the wind bind group"),
            device
                .create_bind_group(&BindGroupDesc {
                    label: Some("wind probe"),
                    layout: layouts[1],
                    entries: &probe_group,
                    variable_count: None,
                })
                .expect("the probe bind group"),
        ];

        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("wind"),
                bind_group_layouts: &layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        // All four blobs, because all four backends run this file.
        let module = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("wind.slang"),
                spirv: crcbl::shaders::WIND.spirv(),
                wgsl: crcbl::shaders::WIND.wgsl(),
                msl: crcbl::shaders::WIND.msl(),
                dxil: &crcbl::shaders::WIND.dxil_containers(),
            })
            .expect("the committed shader is accepted");
        let entry_point = crcbl::shaders::WIND
            .entry_point(crcbl::shaders::Stage::Compute)
            .expect("wind.slang has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&ComputePipelineDesc {
                label: Some("wind"),
                layout: pipeline_layout,
                compute: ShaderEntry {
                    module,
                    entry_point,
                },
                workgroup_size: [WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        let probe = Self {
            params,
            probe_params,
            points: points_buffer,
            velocities,
            staging,
            images,
            views,
            sampler,
            layouts,
            groups,
            pipeline_layout,
            pipeline,
            uploads,
        };
        probe.upload(
            headless,
            layers,
            [
                (params_upload, params, PARAMS_SIZE as u64),
                (probe_upload, probe_params, PROBE_PARAMS_SIZE as u64),
                (points_upload, points_buffer, point_bytes.len() as u64),
            ],
            [
                (direction_upload, direction_row_texels),
                (intensity_upload, intensity_row_texels),
            ],
        );
        probe
    }

    /// Copies the three blocks and the two layers onto the device.
    fn upload(
        &self,
        headless: &Headless,
        layers: &Layers,
        buffers: [(crcbl::hal::BufferHandle, crcbl::hal::BufferHandle, u64); 3],
        images: [(crcbl::hal::BufferHandle, u32); 2],
    ) {
        let device = headless.device.as_ref();
        let mut encoder = device.create_command_encoder(&crcbl::hal::CommandEncoderDesc {
            label: Some("wind upload"),
            queue: headless.queue,
        });
        for (source, destination, size) in buffers {
            encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
                src: source,
                src_offset: 0,
                dst: destination,
                dst_offset: 0,
                size,
            });
        }
        let range = ImageSubresourceRange {
            aspect: ImageAspect::COLOR,
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };
        let grids = [layers.direction.grid(), layers.intensity.grid()];
        let to_transfer: Vec<ImageBarrier> = self
            .images
            .iter()
            .map(|image| {
                ImageBarrier::new(
                    *image,
                    range,
                    ResourceState::Undefined,
                    ResourceState::TransferDst,
                )
            })
            .collect();
        encoder.pipeline_barrier(&Barriers {
            images: &to_transfer,
            ..Barriers::default()
        });
        for (((upload, row_texels), image), grid) in images.into_iter().zip(self.images).zip(grids)
        {
            // Whole-subresource at offset zero, with the row length the padding
            // above produced: a tightly packed row is what D3D12 refuses when
            // it is not a multiple of 256 bytes.
            encoder.copy_buffer_to_image(&BufferImageCopy {
                buffer: upload,
                buffer_offset: 0,
                buffer_row_length: row_texels,
                buffer_image_height: 0,
                image,
                image_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 0,
                    base_layer: 0,
                    layer_count: 1,
                },
                image_offset: Offset3d { x: 0, y: 0, z: 0 },
                image_extent: Extent3d::d2(grid.width, grid.height),
            });
        }
        let to_read: Vec<ImageBarrier> = self
            .images
            .iter()
            .map(|image| {
                ImageBarrier::new(
                    *image,
                    range,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                )
            })
            .collect();
        encoder.pipeline_barrier(&Barriers {
            images: &to_read,
            buffers: &[
                crcbl::hal::BufferBarrier::new(
                    self.params,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                crcbl::hal::BufferBarrier::new(
                    self.probe_params,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                crcbl::hal::BufferBarrier::new(
                    self.points,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("the upload records");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("the upload submits");
        device.wait_idle().expect("idle");
    }

    /// Dispatches the shader and reads the velocities back.
    fn run(&self, headless: &Headless, count: u32) -> Vec<DVec3> {
        let device = headless.device.as_ref();
        let bytes = u64::from(count) * u64::from(VECTOR_STRIDE);
        let mut encoder = device.create_command_encoder(&crcbl::hal::CommandEncoderDesc {
            label: Some("wind probe"),
            queue: headless.queue,
        });
        encoder.begin_compute_pass(&ComputePassDesc {
            label: Some("wind probe"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(self.pipeline);
        encoder.bind_group(0, self.groups[0], &[], self.pipeline_layout);
        encoder.bind_group(1, self.groups[1], &[], self.pipeline_layout);
        encoder.dispatch(count.div_ceil(WORKGROUP_SIZE), 1, 1);
        encoder.end_compute_pass();
        encoder.pipeline_barrier(&Barriers {
            buffers: &[crcbl::hal::BufferBarrier::new(
                self.velocities,
                ResourceState::ShaderReadWrite,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.velocities,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: bytes,
        });
        let commands = encoder.finish().expect("the dispatch records");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("the dispatch submits");
        device.wait_idle().expect("idle");

        let mut read = poisoned(bytes as usize);
        headless.readback(self.staging, bytes, &mut read);
        read.chunks_exact(16)
            .map(|vector| {
                let lane = |at: usize| {
                    f64::from(f32::from_le_bytes([
                        vector[at],
                        vector[at + 1],
                        vector[at + 2],
                        vector[at + 3],
                    ]))
                };
                DVec3::new(lane(0), lane(4), lane(8))
            })
            .collect()
    }

    fn destroy(self, headless: &Headless) {
        let device = headless.device.as_ref();
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        for layout in self.layouts {
            device.destroy_bind_group_layout(layout);
        }
        device.destroy_sampler(self.sampler);
        for view in self.views {
            device.destroy_image_view(view);
        }
        for image in self.images {
            device.destroy_image(image);
        }
        for buffer in self.uploads.into_iter().chain([
            self.params,
            self.probe_params,
            self.points,
            self.velocities,
            self.staging,
        ]) {
            device.destroy_buffer(buffer);
        }
    }
}

/// The field, its GPU copy's answers, and how far apart they were.
struct Agreement {
    /// The largest absolute difference in metres per second.
    worst: f64,
    /// Where it was, as an index into the point list.
    worst_at: usize,
    /// The CPU's answer there.
    cpu: DVec3,
    /// The GPU's.
    gpu: DVec3,
    /// The strongest wind the CPU answered anywhere in the list.
    strongest: f64,
    /// [`Agreement::worst`] as a fraction of the weather's base speed, which is
    /// the shape [`MAX_CPU_GPU_ERROR`] is stated in and the reason it is: the
    /// filter's error is a weight error, and a weight error multiplies whatever
    /// the weights are carrying.
    relative: f64,
}

/// Runs one field over one layer pair and measures the disagreement.
fn measure(headless: &Headless, field: &WindField, layers: &Layers) -> Agreement {
    let points = probe_points();
    let probe = WindProbe::new(headless, field, layers, &points);
    let gpu = probe.run(headless, points.len() as u32);
    probe.destroy(headless);
    assert_eq!(gpu.len(), points.len());

    let mut measured = Agreement {
        worst: 0.0,
        worst_at: 0,
        cpu: DVec3::ZERO,
        gpu: DVec3::ZERO,
        strongest: 0.0,
        relative: 0.0,
    };
    for (index, (relative, answered)) in points.iter().zip(&gpu).enumerate() {
        let cpu = field.sample(CAMERA + *relative);
        let apart = (cpu - *answered).length();
        measured.strongest = measured.strongest.max(cpu.length());
        if apart > measured.worst {
            measured.worst = apart;
            measured.worst_at = index;
            measured.cpu = cpu;
            measured.gpu = *answered;
        }
    }
    measured.relative = measured.worst / field.weather().speed;
    measured
}

/// The fixture, with nothing optional asked for: this dispatches a compute
/// shader and samples two textures, which every backend can do.
fn open() -> Headless {
    Headless::open_at_format(EXTENT, None, Features::empty())
}

/// One sweep: a layer pair at every [`Beaufort`] preset, reported and bounded.
///
/// Both agreement tests are this function — the only thing that differs is
/// which layers go in, and the tolerance is stated once so neither can quietly
/// relax it.
fn sweep(name: &str, layers: &Layers) -> f64 {
    let headless = open();
    let mut worst_relative = 0.0f64;
    for preset in Beaufort::ALL {
        let mut weather =
            Weather::from_beaufort(DVec2::new(0.8, -0.6), preset).expect("a direction");
        weather.set_gust(0.4, 24.0).expect("a real gust");
        let mut field = WindField::new(
            weather,
            DirectionLayer::from_rgba8(layers.direction.grid(), &layers.direction_pixels)
                .expect("a direction layer"),
            IntensityLayer::from_rgba8(layers.intensity.grid(), &layers.intensity_pixels)
                .expect("an intensity layer"),
        );
        // Somewhere into the session, so the scroll offset is not zero and the
        // gust phase the block carries is a real reduction rather than nothing.
        for _ in 0..317 {
            field.advance(1.0 / 60.0);
        }
        let measured = measure(&headless, &field, layers);
        eprintln!(
            "{suite}: {name} / {preset:?} ({speed} m/s) — worst {:.6} m/s = {:.5} of base, \
             at point {} (cpu {:?}, gpu {:?}); strongest wind {:.3} m/s",
            measured.worst,
            measured.relative,
            measured.worst_at,
            measured.cpu,
            measured.gpu,
            measured.strongest,
            suite = crate::SUITE,
            speed = preset.speed(),
        );
        assert!(
            measured.strongest > preset.speed() * 0.2,
            "{name} / {preset:?} barely blew anywhere in the list ({:.3} m/s), \
             so the comparison is empty",
            measured.strongest
        );
        assert!(
            measured.relative <= MAX_CPU_GPU_ERROR,
            "{name} / {preset:?}: the GPU answered {:?} where the CPU answered {:?} at point \
             {}, {:.6} m/s apart — {:.5} of the {} m/s base speed, past the \
             {MAX_CPU_GPU_ERROR} `crcbl_shaders::wind::MAX_CPU_GPU_ERROR` states",
            measured.gpu,
            measured.cpu,
            measured.worst_at,
            measured.worst,
            measured.relative,
            preset.speed(),
        );
        worst_relative = worst_relative.max(measured.relative);
    }
    headless.finish();
    eprintln!(
        "{}: {name} — worst across every preset: {worst_relative:.5} of base speed, \
         against a {MAX_CPU_GPU_ERROR} bound",
        crate::SUITE
    );
    worst_relative
}

/// The committed layers, at every Beaufort preset, with a gust running.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_gpu_agrees_with_the_cpu_over_the_committed_layers() {
    sweep("committed", &Layers::authored());
}

/// The checkerboard pair, which is the hardest thing a linear filter can be
/// asked to interpolate — and therefore the measurement the tolerance is set
/// from.
///
/// It is also the test that says the bound is not slack: a checkerboard has to
/// disagree by *something*, and a run where it did not would mean the two sides
/// were not being compared at all.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_gpu_agrees_with_the_cpu_over_a_layer_pair_built_to_disagree() {
    let worst = sweep("checkerboard", &Layers::harsh());
    assert!(
        worst > 1e-4,
        "a checkerboard layer read through a hardware filter agreed with the exact CPU \
         interpolation to {worst:.8} of base speed. That is not the filter this bound is \
         about, so something is not being compared — the likeliest cause is a probe that \
         read its own poison or a field with no wind in it."
    );
}

/// Calm means calm on the GPU too: a zero-intensity texel moves nothing, and it
/// is exactly zero rather than nearly so.
///
/// Decision 6's formula makes the gust a **factor** on the intensity rather than
/// a term beside it, which is the whole reason this can be an equality. A
/// shader that added the gust instead would blow through a calm texel, and
/// would pass an agreement test with any tolerance wide enough to cover the
/// filter.
#[test]
#[ignore = "opens a GPU device"]
fn calm_means_calm_on_the_gpu() {
    let headless = open();
    let grid = LayerGrid::new(4, 4, 6.0).expect("a grid");
    let direction_pixels: Vec<u8> = (0..16).flat_map(|_| [255u8, 128, 0, 255]).collect();
    let intensity_pixels: Vec<u8> = (0..16).flat_map(|_| [0u8, 0, 0, 255]).collect();
    let layers = Layers {
        direction: DirectionLayer::from_rgba8(grid, &direction_pixels).expect("a direction layer"),
        intensity: IntensityLayer::from_rgba8(grid, &intensity_pixels).expect("an intensity layer"),
        direction_pixels,
        intensity_pixels,
    };
    let mut weather =
        Weather::from_beaufort(DVec2::new(0.6, 0.8), Beaufort::Violent).expect("a direction");
    weather.set_gust(1.0, 9.0).expect("a real gust");
    let mut field = WindField::new(weather, layers.direction.clone(), layers.intensity.clone());
    for _ in 0..91 {
        field.advance(1.0 / 60.0);
    }

    let points = probe_points();
    let probe = WindProbe::new(&headless, &field, &layers, &points);
    let gpu = probe.run(&headless, points.len() as u32);
    probe.destroy(&headless);
    for (index, answered) in gpu.iter().enumerate() {
        assert_eq!(
            *answered,
            DVec3::ZERO,
            "point {index} is over a calm layer and the GPU blew {answered:?}"
        );
    }
    headless.finish();
}

/// **A padded row is a whole number of texels wide and a multiple of the
/// device's alignment**, and the padding is at the end of each row rather than
/// at the end of the image.
///
/// The claim behind [`padded_rows`], which exists because D3D12 refuses a row
/// pitch that is not a multiple of 256 bytes and the fixture's layers are four
/// texels wide. It runs on every backend because it touches none: the
/// arithmetic is what went wrong, and only the WARP leg could see it.
#[test]
fn a_padded_row_is_a_multiple_of_the_alignment_and_keeps_its_texels() {
    // Two rows of four RGBA texels: 16 bytes each, the shape that failed.
    let pixels: Vec<u8> = (0..32).collect();
    let (padded, row_texels) = padded_rows(&pixels, 4, 256);

    assert_eq!(row_texels, 64, "256 bytes is 64 RGBA texels");
    assert_eq!(padded.len(), 512, "two rows at the padded pitch");
    assert_eq!(&padded[..16], &pixels[..16], "the first row's texels");
    assert!(
        padded[16..256].iter().all(|byte| *byte == 0),
        "the first row's padding is not zeroed",
    );
    assert_eq!(&padded[256..272], &pixels[16..], "the second row's texels");

    // An alignment a row already satisfies pads nothing.
    let (tight, tight_texels) = padded_rows(&pixels, 4, 4);
    assert_eq!((tight, tight_texels), (pixels, 4));
}
