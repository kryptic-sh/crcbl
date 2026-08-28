//! The base-colour page's mip chain, read back level by level.
//!
//! `docs/plan/43-render-standards.md`'s filtering rung: every layer of the page
//! goes up with the chain `crcbl::render::mip` builds on the host, one copy per
//! level. A picture cannot show it — the sampler still clamps to level 0 until
//! the rung's sampler slice re-blesses the goldens — so this copies each level
//! of the page's image into a buffer and compares the bytes with the host's own
//! chain. An upload that wrote every level into mip 0, skipped the lower ones,
//! or read a level from the wrong staging offset lands different bytes in one
//! of them and no golden would ever have said so.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Features,
    ImageAspect, ImageBarrier, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange,
    MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl::render::scene::{Geometry, MeshDesc, PageDesc, ProbeGrid};
use crcbl::render::{Capacities, ForwardRenderer, SceneDesc, mip};
use crcbl::shaders::mesh::GpuMaterial;
use std::borrow::Cow;

/// The page's side, in texels: three levels, so the chain has a middle.
const EXTENT: u32 = 4;

/// The layer this appends past [`PageDesc::UNTEXTURED_LAYER`].
const PATTERN_LAYER: u32 = 1;

/// A 4×4 layer whose quadrants differ, so every level of its chain differs
/// from every other and from the white layer beside it: a red/black checker,
/// solid green, a blue/white checker and solid yellow.
fn pattern() -> Vec<u8> {
    let mut texels = Vec::with_capacity((EXTENT * EXTENT * 4) as usize);
    for y in 0..EXTENT {
        for x in 0..EXTENT {
            let even = (x + y) % 2 == 0;
            let rgb: [u8; 3] = match (x < 2, y < 2) {
                (true, true) if even => [0xFF, 0x00, 0x00],
                (true, true) => [0x00, 0x00, 0x00],
                (false, true) => [0x00, 0xFF, 0x00],
                (true, false) if even => [0x00, 0x00, 0xFF],
                (true, false) => [0xFF, 0xFF, 0xFF],
                (false, false) => [0xFF, 0xFF, 0x00],
            };
            texels.extend_from_slice(&rgb);
            texels.push(0xFF);
        }
    }
    texels
}

/// Copies one level of one layer of `image` into a buffer and returns its
/// texels tightly packed — the padded rows the copy alignment wants are
/// stripped here.
fn read_level(headless: &Headless, image: ImageHandle, layer: u32, level: u32) -> Vec<u8> {
    let device = headless.device.as_ref();
    let side = mip::level_extent(EXTENT, level);
    let row_bytes = u64::from(side) * 4;
    // The pitch the device's copy alignment asks for, as `crcbl::render::texture`
    // pads it going the other way: WebGPU and D3D12 want 256 bytes per row of a
    // multi-row copy, Vulkan takes anything a texel divides.
    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(4);
    let pitch = row_bytes.next_multiple_of(alignment);
    let size = pitch * u64::from(side);
    let label = format!("page level {level} of layer {layer}");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some(&label),
            size,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some(&label),
        queue: headless.queue,
    });
    // The page belongs to the renderer, which left it in `ShaderRead` for the
    // frames it never drew; a reader outside a frame asks for it and puts it
    // back, so the renderer's own declaration of the page stays true.
    let range = ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: level,
        mip_count: 1,
        base_layer: layer,
        layer_count: 1,
    };
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::ShaderRead,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: u32::try_from(pitch / 4).expect("a small pitch"),
        buffer_image_height: side,
        image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: level,
            base_layer: layer,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(side, side),
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::TransferSrc,
            ResourceState::ShaderRead,
        )],
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut padded = poisoned(size as usize);
    headless.readback(staging, size, &mut padded);
    device.destroy_buffer(staging);

    padded
        .chunks_exact(pitch as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect()
}

/// **Every level of every layer is on the device, and it is the host's
/// chain.** Level 0 of the pattern layer is the pattern, its two lower levels
/// are what `crcbl::render::mip::chain` computed for it, and the white layer's
/// lower levels are white — so the copies landed in the right layer and the
/// right mip, from the right offset, and not one of the four backends did it
/// differently.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_page_carries_the_hosts_mip_chain_on_every_layer() {
    let headless = Headless::open_for_mesh_with(Features::empty());
    let device = headless.device.as_ref();

    let level0 = pattern();
    let expected = mip::chain(&level0, EXTENT);
    assert_eq!(
        expected.len(),
        2,
        "a 4×4 layer has two levels below its own"
    );
    // Anti-vacuity: the levels this compares differ from each other and from
    // the white layer beside them, so a copy that wrote the wrong level, or the
    // wrong layer, cannot compare equal by accident.
    let white = |side: u32| vec![0xFFu8; (side * side * 4) as usize];
    assert_ne!(expected[0], white(2));
    assert_ne!(expected[1], white(1));
    assert_ne!(&expected[0][..4], &expected[1][..]);

    let mut page = PageDesc::opaque_white(EXTENT);
    assert_eq!(page.push_layer(level0.clone()), PATTERN_LAYER);
    // One cube and one row, because a description with no mesh is not one the
    // renderer builds — `docs/backlog.md` has the entry. Nothing here draws it.
    let scene = SceneDesc {
        meshes: vec![MeshDesc {
            label: Cow::Borrowed("a cube nobody draws"),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(crcbl::shaders::mesh::cube_vertex_bytes()),
                indices: Cow::Owned(crcbl::shaders::mesh::cube_indices()),
                clusters: crcbl::shaders::meshlet::cube_clusters(),
            },
        }],
        materials: vec![GpuMaterial::UNTINTED],
        page,
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    };
    let renderer = ForwardRenderer::with_scene(device, headless.queue, headless.format, &scene)
        .expect("a forward renderer over a page with a chain");
    let image = renderer.base_color_page_import().image;

    assert_eq!(
        read_level(&headless, image, PATTERN_LAYER, 0),
        level0,
        "level 0 of the pattern layer is the pattern"
    );
    assert_eq!(
        read_level(&headless, image, PATTERN_LAYER, 1),
        expected[0],
        "level 1 of the pattern layer is the host's first level below it"
    );
    assert_eq!(
        read_level(&headless, image, PATTERN_LAYER, 2),
        expected[1],
        "level 2 of the pattern layer is the host's one texel"
    );
    assert_eq!(
        read_level(&headless, image, PageDesc::UNTEXTURED_LAYER, 1),
        white(2),
        "the white layer's chain is white"
    );

    eprintln!(
        "{}: the page's {EXTENT}² chain reads back level for level on layer {PATTERN_LAYER}",
        crate::SUITE
    );
    renderer.destroy(device);
    headless.finish();
}
