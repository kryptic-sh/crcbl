//! A view's target drawn inside the UI: `UiRenderer::add_passes_with_textures`
//! sampling what a secondary view rendered in the same graph — the editor
//! viewport's shape, and a game's picture-in-picture.
//!
//! The observable is the view's own picture. The view target is read back
//! beside the frame the UI composited, and the rectangle the draw list put it
//! in must hold the same pixels, texel for pixel: a rectangle that sampled the
//! image atlas, the blank set 1 binds for everything else, a flipped or shifted
//! texture, or a view the graph had not finished drawing, each differs from it.

use crcbl::math::Vec2;
use crcbl::render::{UiRenderer, UiTexture};
use crcbl::ui::{DrawList, FontAtlas, TextureId};

use super::*;

/// The name the draw list gives the view's picture.
const PICTURE: TextureId = TextureId::new(3);

/// A name the draw list uses and the call never pairs with an image.
const UNNAMED: TextureId = TextureId::new(4);

/// What the frame is cleared to before the UI composites over it: a colour no
/// lit cube face or default background is.
const BACKDROP: [f32; 4] = [1.0, 0.0, 1.0, 1.0];

/// [`BACKDROP`] as the sRGB target stores it.
const BACKDROP_BYTES: [u8; 3] = [255, 0, 255];

/// How far a sampled texel may land from the texel it was: the sRGB decode on
/// the sample and the encode on the write, rounded once each.
const ROUND_TRIP: u8 = 1;

/// Whether two pixels differ in any colour channel.
fn differ(a: [u8; 4], b: [u8; 4]) -> bool {
    a[..3] != b[..3]
}

/// A colour target the view can draw into, the UI can sample and a readback can
/// copy.
fn sampled_target(headless: &Headless, extent: (u32, u32)) -> (ImageHandle, ImageViewHandle) {
    let device = headless.device.as_ref();
    let image = device
        .create_image(&ImageDesc {
            label: Some("sampled view target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(extent.0, extent.1),
            format: headless.format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED | ImageUsage::TRANSFER_SRC,
        })
        .expect("a view target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some("sampled view target"),
            image,
            view_type: ImageViewType::D2,
            format: headless.format,
            range: ImageSubresourceRange::all(headless.format),
        })
        .expect("a view target view");
    (image, view)
}

/// **A UI rectangle naming a view's target draws the view's own pixels**, at
/// one texel per pixel where it was put, and a rectangle naming a texture the
/// call was not handed draws nothing over the backdrop.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_ui_rectangle_draws_the_view_it_names_texel_for_pixel() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(device, headless.queue, headless.format).expect("a forward renderer");
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::IDENTITY,
    );
    let view = renderer
        .create_view(device, headless.queue, &ViewDesc::default())
        .expect("a view");
    let target = sampled_target(&headless, VIEW_EXTENT);
    let mut ui = UiRenderer::new(device, headless.queue, headless.format).expect("a ui renderer");

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let frame = acquired.extent;
    assert!(
        frame.0 > VIEW_EXTENT.0 && frame.1 > VIEW_EXTENT.1,
        "the frame {frame:?} has no room around a {VIEW_EXTENT:?} picture"
    );
    // Off every edge, and not at the frame's centre: a rectangle drawn at the
    // wrong offset lands on backdrop, not on a mirror of itself.
    let offset = (frame.0 - VIEW_EXTENT.0 - 8, (frame.1 - VIEW_EXTENT.1) / 2);
    let at = Vec2::new(offset.0 as f32, offset.1 as f32);
    let size = Vec2::new(VIEW_EXTENT.0 as f32, VIEW_EXTENT.1 as f32);
    let whole = (Vec2::ZERO, Vec2::ONE);
    let mut list = DrawList::new();
    list.texture(at, at + size, PICTURE, whole, [1.0; 4]);
    list.texture(
        Vec2::splat(4.0),
        Vec2::splat(36.0),
        UNNAMED,
        whole,
        [1.0; 4],
    );
    ui.begin_frame(device, &list, &FontAtlas::built_in(), 1.0)
        .expect("the ui upload");

    let camera = mesh_camera(crcbl::render::Projection::default());
    renderer
        .begin_frame(device, &camera, &DirectionalLight::default(), frame)
        .expect("the uniform buffers are writable");
    let view_camera = Camera {
        eye: Vec3::new(-1.6, 1.2, -2.2),
        ..camera
    };
    renderer
        .begin_view(device, view, &view_camera, VIEW_EXTENT)
        .expect("the view's uniform buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ui view frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let swapchain = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: frame,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        let picture = graph.import_image(
            "view target",
            ImportedImage {
                image: target.0,
                view: target.1,
                format: headless.format,
                extent: VIEW_EXTENT,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::TransferSrc,
            },
        );
        // The primary camera draws somewhere nobody looks: this test is about
        // the view and the UI, and a renderer always draws its primary.
        let unseen = graph.create_image(
            "primary",
            crcbl::render::TransientImageDesc::new(
                frame,
                headless.format,
                ImageUsage::COLOR_ATTACHMENT,
            ),
        );
        renderer.add_passes_with_views(
            &mut graph,
            &pool,
            FrameTargets {
                target: unseen,
                extent: frame,
                skinning: None,
                views: &[ViewTarget {
                    view,
                    target: picture,
                    extent: VIEW_EXTENT,
                }],
            },
            |_, _| {},
        );
        graph
            .add_render_pass("backdrop")
            .clear_color(swapchain, BACKDROP)
            .execute(|_| {});
        ui.add_passes_with_textures(
            &mut graph,
            swapchain,
            frame,
            &[UiTexture {
                id: PICTURE,
                image: picture,
            }],
        );
        graph.compile(&pool).expect("a legal frame")
    };
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let frame_staging = copy_back(&headless, encoder.as_mut(), acquired.image, frame);
    let view_staging = copy_back(&headless, encoder.as_mut(), target.0, VIEW_EXTENT);
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
    let composited = read_back(&headless, frame_staging, frame);
    let shown = read_back(&headless, view_staging, VIEW_EXTENT);
    device.destroy_command_buffer(commands);

    // The view drew a picture worth comparing: the cube in its middle and the
    // background in its corner, neither of them the backdrop.
    let texel = |x: u32, y: u32| shown.pixel(x, y).expect("inside the view");
    let (centre, corner) = (texel(VIEW_EXTENT.0 / 2, VIEW_EXTENT.1 / 2), texel(1, 1));
    assert!(
        differ(centre, corner),
        "the view's centre {centre:?} is its corner {corner:?}: it drew no cube to compare"
    );
    for pixel in [centre, corner] {
        assert_ne!(
            pixel[..3],
            BACKDROP_BYTES,
            "the view drew the backdrop colour"
        );
    }

    let mut worst = (0u8, (0, 0));
    for y in 0..VIEW_EXTENT.1 {
        for x in 0..VIEW_EXTENT.0 {
            let drawn = composited
                .pixel(x + offset.0, y + offset.1)
                .expect("inside the frame");
            let expected = texel(x, y);
            let gap = (0..3)
                .map(|channel| drawn[channel].abs_diff(expected[channel]))
                .max()
                .unwrap_or(0);
            if gap > worst.0 {
                worst = (gap, (x, y));
            }
        }
    }
    eprintln!(
        "crcbl forward e2e: ui view — centre {centre:?}, corner {corner:?}, worst gap {} at {:?}",
        worst.0, worst.1
    );
    assert!(
        worst.0 <= ROUND_TRIP,
        "the rectangle at {offset:?} is not the view's picture: texel {:?} is off by {}",
        worst.1,
        worst.0
    );

    let backdrop = |x: u32, y: u32| {
        let pixel = composited.pixel(x, y).expect("inside the frame");
        pixel[..3]
            .iter()
            .zip(BACKDROP_BYTES)
            .all(|(have, want)| have.abs_diff(want) <= ROUND_TRIP)
    };
    assert!(
        backdrop(offset.0 - 1, offset.1 + VIEW_EXTENT.1 / 2),
        "the picture spilled left of its rectangle"
    );
    assert!(
        backdrop(20, 20),
        "a texture the call was not handed drew something over the backdrop"
    );

    device.wait_idle().expect("idle");
    ui.destroy(device);
    renderer.destroy(device);
    device.destroy_image_view(target.1);
    device.destroy_image(target.0);
    pool.destroy(device);
    headless.finish();
}
