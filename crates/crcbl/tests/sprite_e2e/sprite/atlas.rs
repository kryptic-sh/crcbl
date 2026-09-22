//! A rendered image registered as a sprite: an offscreen target a render pass
//! filled, copied into an atlas slot, and drawn by the sprite pass — on a real
//! device, read back.
//!
//! `crcbl_render::sprite_pass::atlas`'s unit tests pin the copy's destination
//! and the barrier order against the recorder; these are the claims only a
//! driver can settle: that the texels a render pass wrote are the texels the
//! sprite samples, and that freeing and refilling a slot while the frame that
//! drew it is still unread leaves that frame its old picture.
//!
//! The "render" is a clear. That is the one render-pass write whose output is a
//! known colour at every texel, so the sampled value is a direct read-out of
//! which image landed in which cell with no rasterisation in between.

use crate::harness::Headless;
use crate::sprite::{
    FrameStaging, SPRITE_CLEAR, SPRITE_EXTENT, assert_background,
    assert_the_camera_maps_a_world_unit_to_a_pixel, close, rgb, sprite_camera, srgb_byte,
    world_to_pixel,
};
use crcbl::hal::{CommandEncoderDesc, ImageUsage, PresentInfo, ResourceState, SubmitInfo};
use crcbl::render::{AtlasDesc, AtlasSlot, RenderGraph, SlotCopy, Sprite, TransientImageDesc};

/// One cell, in texels. Drawn at twice that in world units, which is two
/// device pixels a texel under the suite's camera.
const CELL: (u32, u32) = (16, 16);

/// The two colours rendered into cells, in **linear** light — a clear value
/// on an sRGB target means a linear colour. Different in every channel, so a
/// cell showing the other's texels fails on all three.
const RED: [f32; 4] = [0.80, 0.05, 0.10, 1.0];
const GREEN: [f32; 4] = [0.05, 0.60, 0.25, 1.0];

/// The byte triple a linear colour lands on in the sRGB frame.
fn stored(linear: [f32; 4]) -> [u8; 3] {
    [
        srgb_byte(linear[0]),
        srgb_byte(linear[1]),
        srgb_byte(linear[2]),
    ]
}

/// A 32-unit square with its minimum corner at `at`.
fn rect(at: [f32; 2]) -> [f32; 4] {
    [at[0], at[1], 32.0, 32.0]
}

/// The device pixel at the centre of `rect`.
fn centre(rect: [f32; 4]) -> (u32, u32) {
    let pixel = world_to_pixel([rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0]);
    (pixel[0] as u32, pixel[1] as u32)
}

/// Records and submits one frame — each `fills` colour cleared into its own
/// cell-sized transient and copied into its slot, then `sprites` drawn over the
/// suite's clear — and returns the readback **without waiting for it**, so a
/// caller can put a second frame in flight behind it.
fn submit_frame(
    headless: &Headless,
    renderer: &mut crcbl::render::SpriteRenderer,
    pool: &mut crcbl::render::TransientPool,
    fills: &[(AtlasSlot, [f32; 4])],
    sprites: &[Sprite],
) -> (FrameStaging, crcbl::hal::CommandBufferHandle) {
    let device = headless.device.as_ref();
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let staging = FrameStaging::new(device, SPRITE_EXTENT);
    let aspect = SPRITE_EXTENT.0 as f32 / SPRITE_EXTENT.1 as f32;
    renderer
        .begin_frame(
            device,
            sprites,
            sprite_camera().view_projection(aspect),
            SPRITE_EXTENT,
        )
        .expect("the instance and constant buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("atlas frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: SPRITE_EXTENT,
                initial: ResourceState::Undefined,
                claim: crcbl::render::InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        let mut copies = Vec::with_capacity(fills.len());
        for &(slot, colour) in fills {
            let rendered = graph.create_image(
                "rendered icon",
                TransientImageDesc::new(
                    CELL,
                    crcbl::render::ATLAS_FORMAT,
                    ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                ),
            );
            graph
                .add_render_pass("render icon")
                .clear_color(rendered, colour)
                .execute(|_| {});
            copies.push(SlotCopy {
                source: rendered,
                slot,
            });
        }
        renderer
            .add_slot_copies(&mut graph, &copies)
            .expect("every source is a cell-sized copy source");
        graph
            .add_render_pass("sprite background")
            .clear_color(target, SPRITE_CLEAR)
            .execute(|_| {});
        renderer.add_pass(&mut graph, target);
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");
    staging.copy_from(encoder.as_mut(), acquired.image);
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    (staging, commands)
}

/// A renderer with one two-cell atlas.
fn atlas_renderer(headless: &Headless) -> (crcbl::render::SpriteRenderer, crcbl::render::SheetId) {
    let mut renderer = crcbl::render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let atlas = renderer
        .create_atlas(
            headless.device.as_ref(),
            &AtlasDesc {
                label: "icon atlas",
                cell: CELL,
                columns: 2,
                rows: 1,
                sample: crcbl::render::SampleMode::Pixel,
            },
        )
        .expect("the atlas is created");
    (renderer, atlas)
}

/// **A rendered target, copied into a slot, draws as a sprite** — and each of
/// two slots draws its own image, so the copy landed in the cell the slot's UVs
/// name and not in its neighbour.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_rendered_target_copied_into_a_slot_draws_as_a_sprite() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let (mut renderer, atlas) = atlas_renderer(&headless);
    let red = renderer.allocate_slot(atlas).expect("cell 0");
    let green = renderer.allocate_slot(atlas).expect("cell 1");

    let left = rect([-80.0, -16.0]);
    let right = rect([40.0, -16.0]);
    let (staging, commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        &[(red, RED), (green, GREEN)],
        &[
            Sprite::new(red.sheet(), left, red.uv()),
            Sprite::new(green.sheet(), right, green.uv()),
        ],
    );
    let image = staging.read(&headless);
    headless.device.destroy_command_buffer(commands);

    for (rect, colour, name) in [(left, RED, "red"), (right, GREEN, "green")] {
        let (x, y) = centre(rect);
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, stored(colour), 2),
            "the {name} slot's sprite at ({x}, {y}) should be {:?}, got {actual:?} — the \
             other slot's colour is a copy into the wrong cell, and the clear colour is a \
             sprite that sampled a cell nothing was copied into",
            stored(colour)
        );
        // And it ends where the sprite does: the gutter kept the neighbour's
        // texels out, and nothing drew outside the quad.
        let low = world_to_pixel([rect[0], rect[1]]);
        assert_background(&image, low[0] as u32 - 2, y);
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A slot freed and refilled while the frame that drew it is still in
/// flight leaves that frame its old texels.**
///
/// Frame one copies red into the slot and draws it, and is submitted with its
/// readback unread. The slot is freed, the same cell is allocated again, and
/// frame two copies green into it and draws it. Only then are both read: the
/// first must be red and the second green. Freeing destroyed nothing, and the
/// refill is a copy in the later submission, which the graph's barrier out of
/// `ShaderRead` orders after the first frame's sampling.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_slot_refilled_while_its_frame_is_in_flight_keeps_that_frame_its_texels() {
    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let (mut renderer, atlas) = atlas_renderer(&headless);
    let square = rect([-16.0, -16.0]);

    let first_slot = renderer.allocate_slot(atlas).expect("a free cell");
    let (first, first_commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        &[(first_slot, RED)],
        &[Sprite::new(atlas, square, first_slot.uv())],
    );

    renderer
        .free_slot(first_slot)
        .expect("the slot is live while its frame is in flight");
    let second_slot = renderer.allocate_slot(atlas).expect("the freed cell");
    assert_eq!(
        second_slot.index(),
        first_slot.index(),
        "the refill must reuse the cell the in-flight frame sampled, or this tests nothing"
    );
    let (second, second_commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        &[(second_slot, GREEN)],
        &[Sprite::new(atlas, square, second_slot.uv())],
    );

    let first = first.read(&headless);
    let second = second.read(&headless);
    headless.device.destroy_command_buffer(first_commands);
    headless.device.destroy_command_buffer(second_commands);

    let (x, y) = centre(square);
    for (image, colour, which) in [(&first, RED, "first"), (&second, GREEN, "second")] {
        let actual = rgb(image, x, y);
        assert!(
            close(actual, stored(colour), 2),
            "the {which} frame's sprite at ({x}, {y}) should be {:?}, got {actual:?}",
            stored(colour)
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
