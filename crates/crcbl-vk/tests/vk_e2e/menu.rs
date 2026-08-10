//! Slice 14's shared menu, drawn.
//!
//! `crcbl_ui::menu` asserts the layout to the pixel and `crcbl_render::menu`
//! asserts the quads to the float, both with no device in the room. Neither can
//! show the two things a reviewer actually wants to see: **that the window
//! frame really is a nine-slice whose corners survive being stretched to a very
//! different panel, and that a menu is centred in framebuffers of different
//! shapes.** Both are pictures, and they are stacked into one reference here,
//! `tests/golden/menu_frame_two_sizes.png`.
//!
//! Because a swapchain has one extent and the point is that the two extents
//! differ, this is the one module that opens two `Headless` fixtures for a
//! single test and joins their frames afterwards.
//!
//! **It is the real art.** The panel, the button skin and the scrim are frames
//! of `crates/crcbl-render/assets/menu.crpix`, baked by that crate's own
//! `build.rs` and uploaded by `crcbl_render::MenuArt::register` — not a
//! lookalike assembled here. That is the whole reason the art lives in
//! `crcbl-render` rather than under `apps/`: this crate cannot see `apps/`, and
//! a golden image of a replica would be evidence about the replica.

use crate::harness::Headless;
use crate::sprite::{SPRITE_CLEAR, background_rgb, close, report_goldens, rgb, sprite_golden};
use crcbl_hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Format, ImageAspect,
    ImageSubresourceLayers, MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};

/// The two framebuffers the menu golden is taken in.
///
/// Different in **shape**, not only in size: 4:3 and then 8:3, which is the
/// canvas ratio `web/index.html` clamps to on a short viewport. A menu that
/// derived its position from one axis is off-centre in the other.
const MENU_TALL_EXTENT: (u32, u32) = (416, 352);
/// See [`MENU_TALL_EXTENT`].
const MENU_WIDE_EXTENT: (u32, u32) = (416, 224);

/// The scale both halves are pinned to.
///
/// Pinned rather than left to `Menu::layout`'s fit, because the point of the
/// picture is that **the corners are the same pixels in both halves**, and a fit
/// that chose a different scale for the short framebuffer would change them for
/// a legitimate reason and make the comparison say nothing.
const MENU_SCALE: u32 = 2;

/// The pause menu, as `apps/*/src/menu.rs` builds it.
fn golden_pause_menu() -> crcbl_render::Menu {
    crcbl_render::Menu::new(
        "PAUSED",
        vec![
            crcbl_render::MenuItem::new(1, "RESUME", "ESC"),
            crcbl_render::MenuItem::new(3, "FULLSCREEN", "F11"),
            crcbl_render::MenuItem::new(4, "DEBUG PANEL", "F3"),
        ],
    )
}

/// The smallest menu the type can express: one short item, no key hint.
fn golden_small_menu() -> crcbl_render::Menu {
    crcbl_render::Menu::new("GO", vec![crcbl_render::MenuItem::new(1, "OK", "")])
}

/// Renders one menu frame **through the real `MenuRenderer` and the real
/// `RenderGraph`** and reads the swapchain image back.
///
/// A clear pass first and the menu pass on top of it, because
/// `SpriteRenderer::add_pass` loads rather than clears — which is exactly how a
/// sample composites a menu over a game.
fn render_menu(
    headless: &Headless,
    renderer: &mut crcbl_render::MenuRenderer,
    pool: &mut crcbl_render::TransientPool,
    extent: (u32, u32),
) -> crcbl_golden::Image {
    use crcbl_render::RenderGraph;

    let device = headless.device.as_ref();
    let (width, height) = extent;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, extent);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("menu readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    renderer
        .begin_frame(device, extent)
        .expect("the instance and constant buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("menu frame"),
        queue: headless.queue,
    });

    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent,
                initial: ResourceState::Undefined,
                final_state: ResourceState::TransferSrc,
            },
        );
        graph
            .add_render_pass("menu background")
            .clear_color(target, SPRITE_CLEAR)
            .execute(|_| {});
        renderer.add_pass(&mut graph, target);
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });

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

    let mut color = vec![0u8; color_bytes as usize];
    headless.readback(staging, color_bytes, &mut color);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}

/// Two equally wide images, one above the other, as a single reference.
///
/// The alternative was two golden files, and one is better here for a reason
/// that is about review rather than about plumbing: the claim the picture makes
/// is a *comparison* — these corners are those corners — and a reviewer who has
/// to open two files and flick between them is not making it.
fn stack(top: &crcbl_golden::Image, bottom: &crcbl_golden::Image) -> crcbl_golden::Image {
    assert_eq!(top.width(), bottom.width(), "a stack needs one width");
    let mut pixels = top.pixels().to_vec();
    pixels.extend_from_slice(bottom.pixels());
    crcbl_golden::Image::from_rgba8(top.width(), top.height() + bottom.height(), pixels)
        .expect("two images of one width stack exactly")
}

/// The pixel rect a laid-out menu's window frame occupies.
///
/// `crcbl_ui` lays out in screen pixels already, so this is the layout's own
/// numbers — but taken as `u32`s, and asserted against the drawn image rather
/// than trusted. `assert_menu_pixels` is what makes that not circular.
fn panel_pixels(layout: &crcbl_render::MenuLayout) -> [u32; 4] {
    let (min, max) = layout.panel();
    [min.x as u32, min.y as u32, max.x as u32, max.y as u32]
}

/// **The feature, in one picture: one window skin, two panels, two
/// framebuffers.**
///
/// The top half is a three-item pause menu in a 4:3 framebuffer with its second
/// item selected, so the picture carries `Idle`, `Hovered` and `Idle`. The
/// bottom half is a one-item menu in an 8:3 framebuffer with its item held down,
/// so the third frame of the skin is there too — all three states of the shipped
/// button art, in one reference.
///
/// The two panels differ by more than a factor of two in both axes and are drawn
/// at the same scale, so the assertion that matters is that their corner blocks
/// come back **pixel-for-pixel identical**. That is what "the frame does not
/// smudge when the menu grows" means, said as bytes.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_shared_menu_keeps_its_frame_at_two_panel_sizes_and_two_shapes() {
    let style = crcbl_render::MenuStyle::pixel_art(MENU_SCALE);
    let atlas = crcbl_render::FontAtlas::built_in();

    let mut tall_menu = golden_pause_menu();
    tall_menu.select_next();
    let tall_layout = tall_menu.layout_with(MENU_TALL_EXTENT, &atlas, &style);

    let mut wide_menu = golden_small_menu();
    wide_menu.press(true);
    let wide_layout = wide_menu.layout_with(MENU_WIDE_EXTENT, &atlas, &style);

    // Both halves are rendered on their own ring, because a swapchain has one
    // extent and the point is that the two extents differ.
    let tall_headless = Headless::open_for_sprites_at(MENU_TALL_EXTENT);
    let mut tall_pool = crcbl_render::TransientPool::new();
    let mut tall_renderer = crcbl_render::MenuRenderer::new(
        tall_headless.device.as_ref(),
        tall_headless.queue,
        tall_headless.format,
    )
    .expect("the menu renderer builds");
    tall_renderer.set_menu(Some((&tall_menu, &tall_layout)));
    let tall_image = render_menu(
        &tall_headless,
        &mut tall_renderer,
        &mut tall_pool,
        MENU_TALL_EXTENT,
    );
    tall_renderer.destroy(tall_headless.device.as_ref());
    tall_pool.destroy(tall_headless.device.as_ref());
    tall_headless.finish();

    let wide_headless = Headless::open_for_sprites_at(MENU_WIDE_EXTENT);
    let mut wide_pool = crcbl_render::TransientPool::new();
    let mut wide_renderer = crcbl_render::MenuRenderer::new(
        wide_headless.device.as_ref(),
        wide_headless.queue,
        wide_headless.format,
    )
    .expect("the menu renderer builds");
    wide_renderer.set_menu(Some((&wide_menu, &wide_layout)));
    let wide_image = render_menu(
        &wide_headless,
        &mut wide_renderer,
        &mut wide_pool,
        MENU_WIDE_EXTENT,
    );
    wide_renderer.destroy(wide_headless.device.as_ref());
    wide_pool.destroy(wide_headless.device.as_ref());
    wide_headless.finish();

    let image = stack(&tall_image, &wide_image);
    let verdict = sprite_golden("menu_frame_two_sizes", &image);
    assert_menu_pixels(&tall_image, &tall_layout, MENU_TALL_EXTENT);
    assert_menu_pixels(&wide_image, &wide_layout, MENU_WIDE_EXTENT);
    assert_menu_corners_match(&tall_image, &tall_layout, &wide_image, &wide_layout);
    report_goldens(vec![verdict]);
}

/// What one half of the menu golden claims on its own: the panel is where the
/// layout says, it is centred in its framebuffer, it is opaque, and the scrim
/// dimmed everything outside it.
fn assert_menu_pixels(
    image: &crcbl_golden::Image,
    layout: &crcbl_render::MenuLayout,
    extent: (u32, u32),
) {
    let [x0, y0, x1, y1] = panel_pixels(layout);
    assert!(
        x1 > x0 + 8 && y1 > y0 + 8,
        "the panel is {x0}..{x1}, {y0}..{y1}"
    );

    // --- centred, measured on the drawn frame -----------------------------
    //
    // The margins either side of the panel are equal to within a pixel, on both
    // axes. Not "the panel is inside the screen": a panel pushed into a corner
    // is also inside the screen.
    let left_margin = x0;
    let right_margin = extent.0 - x1;
    let top_margin = y0;
    let bottom_margin = extent.1 - y1;
    assert!(
        left_margin.abs_diff(right_margin) <= 1,
        "{extent:?}: {left_margin} px of margin on the left and {right_margin} on \
         the right — the panel is not centred"
    );
    assert!(
        top_margin.abs_diff(bottom_margin) <= 1,
        "{extent:?}: {top_margin} px above and {bottom_margin} below"
    );

    // --- the frame is really drawn there ----------------------------------
    //
    // The panel's outermost texel is the art's near-black outline, `#0b0b12`, on
    // every side. A panel that never reached the GPU leaves the scrim here, and
    // the scrim is a *blend* of the clear colour — a different value entirely.
    const PANEL_OUTLINE: [u8; 3] = [0x0b, 0x0b, 0x12];
    let mid_x = (x0 + x1) / 2;
    let mid_y = (y0 + y1) / 2;
    for (x, y, side) in [
        (x0, mid_y, "left"),
        (x1 - 1, mid_y, "right"),
        (mid_x, y0, "top"),
        (mid_x, y1 - 1, "bottom"),
    ] {
        let actual = rgb(image, x, y);
        assert!(
            close(actual, PANEL_OUTLINE, 3),
            "{extent:?}: the panel's {side} edge at ({x}, {y}) is {actual:?}, not \
             the art's outline {PANEL_OUTLINE:?}"
        );
    }

    // --- the light comes from the top left --------------------------------
    //
    // One texel in from the outline: the highlight on the top and left edges,
    // the shadow on the bottom and right. A mirrored or transposed bevel is
    // still a frame, still centred, and still has fixed corners — this is the
    // only thing here that would notice.
    //
    // It is also what carries this check when the *reference* cannot. The
    // golden's tolerance is calibrated for a driver difference and is a fraction
    // of the whole image; the shadow band is a one-texel line on a 416x576
    // canvas, so recolouring it moves 0.9% of the pixels and compares equal.
    // Measured, not assumed — swapping `d` in `menu.crpix` for its own channels
    // reversed passed the reference and failed here.
    const PANEL_HIGHLIGHT: [u8; 3] = [0x9a, 0xa0, 0xcc];
    const PANEL_SHADOW: [u8; 3] = [0x2a, 0x2c, 0x40];
    for (x, y, expected, side) in [
        (x0 + MENU_SCALE, mid_y, PANEL_HIGHLIGHT, "left highlight"),
        (mid_x, y0 + MENU_SCALE, PANEL_HIGHLIGHT, "top highlight"),
        (x1 - 1 - MENU_SCALE, mid_y, PANEL_SHADOW, "right shadow"),
        (mid_x, y1 - 1 - MENU_SCALE, PANEL_SHADOW, "bottom shadow"),
    ] {
        let actual = rgb(image, x, y);
        assert!(
            close(actual, expected, 3),
            "{extent:?}: the panel's {side} at ({x}, {y}) is {actual:?}, not \
             {expected:?} — the bevel is mirrored, transposed or recoloured"
        );
    }

    // --- the scrim dimmed the frame, and did not dim the panel ------------
    //
    // Outside the panel the clear colour must have been darkened, and it must
    // still be *visible* — a scrim that replaced the frame rather than blending
    // over it would read as black, and one that never drew would read as the
    // clear colour untouched.
    let outside = rgb(image, 2, 2);
    let clear = background_rgb();
    assert!(
        outside.iter().zip(&clear).all(|(a, c)| a < c),
        "the corner is {outside:?} against a clear of {clear:?} — the scrim did \
         not dim the frame"
    );
    assert!(
        outside.iter().any(|c| *c > 8),
        "the corner is {outside:?} — the scrim replaced the frame instead of \
         dimming it"
    );
    // And the panel's own interior is the art's fill, not that fill dimmed: the
    // menu pass draws the scrim *before* the panel, and a pass order that put it
    // after would show up here and nowhere else.
    const PANEL_FILL: [u8; 3] = [0x14, 0x16, 0x1f];
    // One pixel inside the fill band, which starts at texel 4 on both axes — so
    // past the outline, the highlight and the two body texels, and above the
    // title rather than over a button.
    let (fill_x, fill_y) = (
        x0 + crcbl_ui_panel_inset() * MENU_SCALE + 1,
        y0 + crcbl_ui_panel_inset() * MENU_SCALE + 1,
    );
    let inside = rgb(image, fill_x, fill_y);
    assert!(
        close(inside, PANEL_FILL, 3),
        "the panel's interior at ({fill_x}, {fill_y}) is {inside:?}, not the \
         art's fill {PANEL_FILL:?} — the scrim was drawn over the panel"
    );
}

/// **The claim the two halves make together**: the same skin, stretched to two
/// panels of very different sizes, with corner blocks that are byte-identical.
fn assert_menu_corners_match(
    tall: &crcbl_golden::Image,
    tall_layout: &crcbl_render::MenuLayout,
    wide: &crcbl_golden::Image,
    wide_layout: &crcbl_render::MenuLayout,
) {
    let a = panel_pixels(tall_layout);
    let b = panel_pixels(wide_layout);
    let size = |r: [u32; 4]| (r[2] - r[0], r[3] - r[1]);
    let (aw, ah) = size(a);
    let (bw, bh) = size(b);
    // Twice as wide and half again as tall. Written as integer ratios rather
    // than as pixel counts so that redrawing the art or relabelling a button
    // cannot quietly shrink the difference the comparison below depends on.
    assert!(
        aw >= bw * 2 && ah * 2 >= bh * 3,
        "the two panels are {aw}x{ah} and {bw}x{bh}, which is not different \
         enough for the comparison below to mean anything"
    );

    // The corner is `PANEL_INSETS` texels at `MENU_SCALE` pixels each, plus one
    // pixel past its own edge — that extra row and column is what pins where the
    // corner *ends* rather than only what it contains, so a cap that grew by a
    // pixel fails here.
    let corner = crcbl_ui_panel_inset() * MENU_SCALE + 1;
    for (name, from_right, from_bottom) in [
        ("top-left", false, false),
        ("top-right", true, false),
        ("bottom-left", false, true),
        ("bottom-right", true, true),
    ] {
        for dy in 0..corner {
            for dx in 0..corner {
                let at = |r: [u32; 4]| {
                    (
                        if from_right { r[2] - 1 - dx } else { r[0] + dx },
                        if from_bottom {
                            r[3] - 1 - dy
                        } else {
                            r[1] + dy
                        },
                    )
                };
                let (ax, ay) = at(a);
                let (bx, by) = at(b);
                let pa = rgb(tall, ax, ay);
                let pb = rgb(wide, bx, by);
                assert!(
                    close(pa, pb, 2),
                    "the {name} corner differs between a {aw}x{ah} panel and a \
                     {bw}x{bh} one, {dx} across and {dy} down from its outer \
                     corner: {pa:?} against {pb:?} — this is the smudge the whole \
                     feature exists to prevent"
                );
            }
        }
    }

    // --- the edges DID stretch --------------------------------------------
    //
    // Without this, a renderer that drew nothing but four corners at every size
    // would satisfy every assertion above. Halfway along the big panel's top
    // edge is past the small panel's right-hand corner entirely, and it must
    // still be the art's top band rather than background.
    const PANEL_TOP_BAND: [u8; 3] = [0x9a, 0xa0, 0xcc];
    let edge_x = (a[0] + a[2]) / 2;
    let edge_y = a[1] + MENU_SCALE; // the highlight row, one texel in
    let actual = rgb(tall, edge_x, edge_y);
    assert!(
        close(actual, PANEL_TOP_BAND, 3),
        "at ({edge_x}, {edge_y}) the big panel should be stretching its top \
         highlight, got {actual:?}"
    );
    assert!(
        aw > (crcbl_ui_panel_inset() * 2 * MENU_SCALE) * 3,
        "the big panel must be much wider than its own two corners for the \
         stretch to be visible"
    );
}

/// `crcbl_ui::menu::PANEL_INSETS`, as whole texels.
///
/// Read off the constant rather than written down, so a redrawn frame with a
/// deeper border moves this test with it instead of silently checking the wrong
/// block.
fn crcbl_ui_panel_inset() -> u32 {
    crcbl_render::MenuStyle::pixel_art(1).panel.left as u32
}
