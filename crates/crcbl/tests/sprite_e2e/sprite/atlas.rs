//! A rendered image registered as a sprite: an offscreen target a render pass
//! filled, copied into an atlas slot, and drawn by the sprite pass — on a real
//! device, read back. And the same for host pixels written straight into a
//! slot.
//!
//! `crcbl_render::sprite_pass::atlas`'s unit tests pin the copy's destination
//! and the barrier order against the recorder; these are the claims only a
//! driver can settle: that the texels a render pass wrote — or the bytes the
//! host staged — are the texels the sprite samples, and that refilling a slot
//! while the frame that drew it is still unread leaves that frame its old
//! picture.
//!
//! The "render" is a clear. That is the one render-pass write whose output is a
//! known colour at every texel, so the sampled value is a direct read-out of
//! which image landed in which cell with no rasterisation in between.

use crate::harness::Headless;
use crate::sprite::{
    FrameStaging, SPRITE_CLEAR, SPRITE_EXTENT, assert_background,
    assert_the_camera_maps_a_world_unit_to_a_pixel, background_rgb, close, rgb, sprite_camera,
    srgb_byte, world_to_pixel,
};
use crcbl::hal::{CommandEncoderDesc, Format, ImageUsage, PresentInfo, ResourceState, SubmitInfo};
use crcbl::render::{
    ATLAS_FORMAT, AtlasDesc, AtlasSlot, RenderGraph, SlotCopy, Sprite, TransientImageDesc,
};

/// One cell, in texels. Drawn at twice that in world units, which is two
/// device pixels a texel under the suite's camera.
const CELL: (u32, u32) = (16, 16);

/// The two colours rendered into cells, in **linear** light — a clear value
/// on an sRGB target means a linear colour. Different in every channel, so a
/// cell showing the other's texels fails on all three.
const RED: [f32; 4] = [0.80, 0.05, 0.10, 1.0];
const GREEN: [f32; 4] = [0.05, 0.60, 0.25, 1.0];

/// Four sRGB-encoded colours written into a cell's quadrants, in
/// `[top-left, top-right, bottom-left, bottom-right]` order. Every channel
/// differs between every pair, so a flipped, transposed or mis-pitched copy
/// swaps colours rather than repeating one. The atlas decodes them and the
/// sRGB frame encodes them again, so each lands on its own bytes.
const WRITTEN: [[u8; 3]; 4] = [[200, 40, 60], [30, 180, 90], [70, 110, 220], [240, 210, 20]];

/// How a slot is filled this frame.
#[derive(Clone, Copy)]
enum Fill<'a> {
    /// A linear colour cleared into a cell-sized transient and copied in.
    Rendered([f32; 4]),
    /// Host pixels, exactly one cell, written in.
    Written(&'a [u8]),
}

/// One cell of opaque pixels whose four quadrants are `colours`, in
/// [`WRITTEN`]'s order.
fn quadrant_cell(colours: [[u8; 3]; 4]) -> Vec<u8> {
    let (width, height) = CELL;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let quadrant = usize::from(y >= height / 2) * 2 + usize::from(x >= width / 2);
            pixels.extend(colours[quadrant]);
            pixels.push(255);
        }
    }
    pixels
}

/// The device pixel at the centre of each quadrant of `rect`, in
/// [`WRITTEN`]'s order.
fn quadrant_centres(rect: [f32; 4]) -> [(u32, u32); 4] {
    let low = world_to_pixel([rect[0], rect[1]]);
    let quarter = rect[2] / 4.0;
    let (left, right) = (low[0] + quarter, low[0] + 3.0 * quarter);
    // `low` is the world *minimum* corner, which is the **bottom** of the
    // quad, so it is the larger screen row.
    let (top, bottom) = (low[1] - 3.0 * quarter, low[1] - quarter);
    [
        (left as u32, top as u32),
        (right as u32, top as u32),
        (left as u32, bottom as u32),
        (right as u32, bottom as u32),
    ]
}

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

/// Records and submits one frame — each `fills` entry rendered in `format` and
/// copied, or written, into its slot, then `sprites` drawn over the suite's
/// clear — and returns the readback **without waiting for it**, so a caller can
/// put a second frame in flight behind it.
fn submit_frame(
    headless: &Headless,
    renderer: &mut crcbl::render::SpriteRenderer,
    pool: &mut crcbl::render::TransientPool,
    format: Format,
    fills: &[(AtlasSlot, Fill<'_>)],
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
        for &(slot, fill) in fills {
            let colour = match fill {
                Fill::Rendered(colour) => colour,
                Fill::Written(pixels) => {
                    renderer
                        .write_slot(device, &mut graph, slot, pixels)
                        .expect("one cell of pixels writes");
                    continue;
                }
            };
            let rendered = graph.create_image(
                "rendered icon",
                TransientImageDesc::new(
                    CELL,
                    format,
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
    atlas_renderer_in(headless, ATLAS_FORMAT)
}

/// A renderer with one two-cell atlas in `format`.
fn atlas_renderer_in(
    headless: &Headless,
    format: Format,
) -> (crcbl::render::SpriteRenderer, crcbl::render::SheetId) {
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
                format,
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
        ATLAS_FORMAT,
        &[(red, Fill::Rendered(RED)), (green, Fill::Rendered(GREEN))],
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
        ATLAS_FORMAT,
        &[(first_slot, Fill::Rendered(RED))],
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
        ATLAS_FORMAT,
        &[(second_slot, Fill::Rendered(GREEN))],
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

/// **Host pixels written into a slot draw as a sprite**, the right way up and
/// in the cell the slot names.
///
/// Four quadrant colours go into the atlas's *second* cell, and a sprite of
/// each cell is drawn. The written one must show every quadrant where the
/// pixels put it — a flipped, transposed or mis-pitched copy swaps them — and
/// the first cell, which nothing wrote, must stay transparent, so a write that
/// landed at the atlas's origin rather than the slot's shows up there. A
/// [`CELL`] row is 64 bytes, short of D3D12's 256-byte copy pitch, so on dx12
/// the staged rows are padded and the pitch is exercised.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn host_pixels_written_into_a_slot_draw_as_a_sprite() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let (mut renderer, atlas) = atlas_renderer(&headless);
    let empty = renderer.allocate_slot(atlas).expect("cell 0");
    let written = renderer.allocate_slot(atlas).expect("cell 1");
    let pixels = quadrant_cell(WRITTEN);

    let left = rect([-80.0, -16.0]);
    let right = rect([40.0, -16.0]);
    let (staging, commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        ATLAS_FORMAT,
        &[(written, Fill::Written(&pixels))],
        &[
            Sprite::new(atlas, left, empty.uv()),
            Sprite::new(atlas, right, written.uv()),
        ],
    );
    let image = staging.read(&headless);
    headless.device.destroy_command_buffer(commands);

    for ((x, y), expected) in quadrant_centres(right).into_iter().zip(WRITTEN) {
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, expected, 2),
            "the written slot at ({x}, {y}) should be {expected:?}, got {actual:?} — another \
             quadrant's colour is a flipped or mis-pitched copy, and the clear colour is a \
             write that never reached the cell"
        );
    }
    let (x, y) = centre(left);
    let actual = rgb(&image, x, y);
    assert!(
        close(actual, background_rgb(), 2),
        "the unwritten slot at ({x}, {y}) should be transparent over the clear {:?}, got \
         {actual:?} — the write landed in the wrong cell",
        background_rgb()
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A slot written while the frame that drew it is still in flight leaves
/// that frame its old texels.**
///
/// Frame one writes the quadrant pixels into the slot and draws it, and is
/// submitted with its readback unread. Frame two writes a single colour into
/// the same live slot — no free, no reallocation — and draws it. Only then are
/// both read: the first must still show its quadrants, the second the new
/// colour everywhere. The second write's staging copy is a pass in the later
/// submission, which the graph's barrier out of `ShaderRead` orders after the
/// first frame's sampling.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_slot_written_while_its_frame_is_in_flight_keeps_that_frame_its_texels() {
    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let (mut renderer, atlas) = atlas_renderer(&headless);
    let square = rect([-16.0, -16.0]);
    let slot = renderer.allocate_slot(atlas).expect("a free cell");
    let sprites = [Sprite::new(atlas, square, slot.uv())];

    let quadrants = quadrant_cell(WRITTEN);
    let (first, first_commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        ATLAS_FORMAT,
        &[(slot, Fill::Written(&quadrants))],
        &sprites,
    );
    let solid_colour = [90, 20, 160];
    let solid = quadrant_cell([solid_colour; 4]);
    let (second, second_commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        ATLAS_FORMAT,
        &[(slot, Fill::Written(&solid))],
        &sprites,
    );

    let first = first.read(&headless);
    let second = second.read(&headless);
    headless.device.destroy_command_buffer(first_commands);
    headless.device.destroy_command_buffer(second_commands);

    for ((x, y), expected) in quadrant_centres(square).into_iter().zip(WRITTEN) {
        let actual = rgb(&first, x, y);
        assert!(
            close(actual, expected, 2),
            "the first frame at ({x}, {y}) should keep {expected:?}, got {actual:?} — \
             {solid_colour:?} is the second write reaching a frame already submitted, and \
             the clear colour a first write that never landed"
        );
        let actual = rgb(&second, x, y);
        assert!(
            close(actual, solid_colour, 2),
            "the second frame at ({x}, {y}) should be {solid_colour:?}, got {actual:?}"
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A `BGRA` atlas draws both kinds of fill in their own colours**: a
/// `Bgra8UnormSrgb` render copied into one cell, and `RGBA` host pixels written
/// into the other.
///
/// The render is what [`ForwardRenderer`](crcbl::render::ForwardRenderer)'s
/// swapchain-format views produce, and the copy only moves bytes — so a `BGRA`
/// source into an `RGBA` atlas would arrive with red and blue exchanged, which
/// is why the atlas takes the format instead. The written quadrants are `RGBA`
/// as every write is, and land in their own colours only because the write
/// swaps them into the atlas's order; a write that staged them as they came
/// exchanges red and blue in every quadrant, and [`WRITTEN`] differs in both.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_bgra_atlas_draws_a_bgra_render_and_written_pixels_in_their_own_colours() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let (mut renderer, atlas) = atlas_renderer_in(&headless, Format::Bgra8UnormSrgb);
    let rendered = renderer.allocate_slot(atlas).expect("cell 0");
    let written = renderer.allocate_slot(atlas).expect("cell 1");
    let pixels = quadrant_cell(WRITTEN);

    let left = rect([-80.0, -16.0]);
    let right = rect([40.0, -16.0]);
    let (staging, commands) = submit_frame(
        &headless,
        &mut renderer,
        &mut pool,
        Format::Bgra8UnormSrgb,
        &[
            (rendered, Fill::Rendered(RED)),
            (written, Fill::Written(&pixels)),
        ],
        &[
            Sprite::new(atlas, left, rendered.uv()),
            Sprite::new(atlas, right, written.uv()),
        ],
    );
    let image = staging.read(&headless);
    headless.device.destroy_command_buffer(commands);

    let (x, y) = centre(left);
    let actual = rgb(&image, x, y);
    assert!(
        close(actual, stored(RED), 2),
        "the rendered slot at ({x}, {y}) should be {:?}, got {actual:?} — red and blue \
         exchanged is a BGRA render read as RGBA",
        stored(RED)
    );
    for ((x, y), expected) in quadrant_centres(right).into_iter().zip(WRITTEN) {
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, expected, 2),
            "the written slot at ({x}, {y}) should be {expected:?}, got {actual:?} — red and \
             blue exchanged is RGBA pixels staged into a BGRA atlas unswapped"
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
