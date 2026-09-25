//! `docs/plan/29-fp-rendering.md`'s second camera, on whichever backend
//! `CRCBL_GPU` names: a view of the scene the renderer already holds, drawn into
//! a target of its own in the same frame as the primary camera.
//!
//! Pixels are the observable. A view that recorded every pass and drew into the
//! wrong image, or drew nothing, or drew the primary camera's picture, compiles
//! and submits cleanly — and only a readback of both targets can tell those
//! apart from a view that works. The same goes for a transparent view's alpha,
//! which no picture composited onto an opaque swapchain shows at all.

use crcbl::hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, Features, Format,
    ImageAspect, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, MemoryLocation, PresentInfo,
    ResourceState, SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{
    AtlasDesc, Camera, DirectionalLight, EffectRequest, ForwardRenderer, FrameTargets,
    ImportedImage, InitialClaim, InstanceHandle, RenderEffects, RenderGraph, SampleMode, Sky,
    SlotCopy, Sprite, SpriteRenderer, TransientPool, ViewBackground, ViewDesc, ViewId,
    ViewLighting, ViewMask, ViewTarget,
};

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place};

/// The view's target: square, and a width whose four-byte rows meet the 256-byte
/// copy pitch wgpu and D3D12 enforce.
const VIEW_EXTENT: (u32, u32) = (128, 128);

/// An alpha byte a pixel no geometry covered holds in a transparent view's
/// target, and the one every other pixel holds.
const EMPTY: u8 = 0;
const COVERED: u8 = u8::MAX;

/// Reads `image`, `extent` in size, back out of a finished frame's encoder.
fn copy_back(
    headless: &Headless,
    encoder: &mut dyn crcbl::hal::CommandEncoder,
    image: ImageHandle,
    extent: (u32, u32),
) -> crcbl::hal::BufferHandle {
    let staging = headless
        .device
        .create_buffer(&BufferDesc {
            label: Some("view readback"),
            size: u64::from(extent.0) * u64::from(extent.1) * 4,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(extent.0, extent.1),
    });
    staging
}

/// A colour target of `extent` in the fixture's format that a frame can be
/// drawn into, copied from and read back.
fn target_image(
    headless: &Headless,
    label: &str,
    extent: (u32, u32),
) -> (ImageHandle, ImageViewHandle) {
    let device = headless.device.as_ref();
    let image = device
        .create_image(&ImageDesc {
            label: Some(label),
            image_type: ImageType::D2,
            extent: Extent3d::d2(extent.0, extent.1),
            format: headless.format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
        })
        .expect("a view target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some(label),
            image,
            view_type: ImageViewType::D2,
            format: headless.format,
            range: ImageSubresourceRange::all(headless.format),
        })
        .expect("a view target view");
    (image, view)
}

/// The byte order the fixture's format stores a pixel in.
fn channel_order(format: Format) -> crcbl_golden::ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    }
}

/// Waits for `staging` and decodes it as an `extent` image in `format`'s order.
fn read_back(
    headless: &Headless,
    staging: crcbl::hal::BufferHandle,
    extent: (u32, u32),
) -> crcbl_golden::Image {
    let bytes = u64::from(extent.0) * u64::from(extent.1) * 4;
    let mut pixels = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut pixels);
    headless.device.destroy_buffer(staging);
    crcbl_golden::Image::from_readback(extent.0, extent.1, &pixels, channel_order(headless.format))
        .expect("the readback is exactly one image")
}

/// One secondary view to draw in a frame: which, through what camera, and into
/// which image of what size.
struct Drawn {
    view: ViewId,
    camera: Camera,
    target: (ImageHandle, ImageViewHandle),
    extent: (u32, u32),
}

/// One frame of the primary camera into the swapchain and of every `views`
/// entry into its own target, all of them read back — the primary's first.
fn render_views(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    views: &[Drawn],
) -> (crcbl_golden::Image, Vec<crcbl_golden::Image>) {
    render_views_lit(
        headless,
        renderer,
        pool,
        &DirectionalLight::default(),
        views,
    )
}

/// [`render_views`], with the frame lit by `sun` rather than the default one.
fn render_views_lit(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    sun: &DirectionalLight,
    views: &[Drawn],
) -> (crcbl_golden::Image, Vec<crcbl_golden::Image>) {
    let device = headless.device.as_ref();
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let camera = mesh_camera(crcbl::render::Projection::default());
    renderer
        .begin_frame(device, &camera, sun, acquired.extent)
        .expect("the uniform buffers are writable");
    for drawn in views {
        renderer
            .begin_view(device, drawn.view, &drawn.camera, drawn.extent)
            .expect("the view's uniform buffers are writable");
    }

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("view frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: acquired.extent,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        let targets: Vec<ViewTarget> = views
            .iter()
            .map(|drawn| ViewTarget {
                view: drawn.view,
                target: graph.import_image(
                    "view target",
                    ImportedImage {
                        image: drawn.target.0,
                        view: drawn.target.1,
                        format: headless.format,
                        extent: drawn.extent,
                        // Where the previous frame left it, which the pool
                        // records — the first frame finds nothing recorded and
                        // says `Undefined`.
                        initial: pool
                            .imported_image_use(drawn.target.0)
                            .unwrap_or(ResourceState::Undefined),
                        claim: InitialClaim::Tracked,
                        final_state: ResourceState::TransferSrc,
                    },
                ),
                extent: drawn.extent,
            })
            .collect();
        renderer.add_passes_with_views(
            &mut graph,
            &*pool,
            FrameTargets {
                target,
                extent: acquired.extent,
                skinning: None,
                views: &targets,
            },
            |_, _| {},
        );
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let primary_staging = copy_back(headless, encoder.as_mut(), acquired.image, acquired.extent);
    let view_staging: Vec<_> = views
        .iter()
        .map(|drawn| copy_back(headless, encoder.as_mut(), drawn.target.0, drawn.extent))
        .collect();
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

    let primary = read_back(headless, primary_staging, acquired.extent);
    let pictures = view_staging
        .into_iter()
        .zip(views)
        .map(|(staging, drawn)| read_back(headless, staging, drawn.extent))
        .collect();
    device.destroy_command_buffer(commands);
    (primary, pictures)
}

/// One frame of the primary camera into the swapchain and of `view` into
/// `view_image`, both read back.
fn render_both(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    view: ViewId,
    view_image: (ImageHandle, ImageViewHandle),
) -> (crcbl_golden::Image, crcbl_golden::Image) {
    let camera = mesh_camera(crcbl::render::Projection::default());
    // The view looks at the same cube from the other side, so its picture is a
    // camera of its own and not a copy of the primary camera's.
    let view_camera = Camera {
        eye: Vec3::new(-1.6, 1.2, -2.2),
        ..camera
    };
    let (primary, mut pictures) = render_views(
        headless,
        renderer,
        pool,
        &[Drawn {
            view,
            camera: view_camera,
            target: view_image,
            extent: VIEW_EXTENT,
        }],
    );
    (primary, pictures.remove(0))
}

/// **A view draws the scene into its own target, and an instance hidden from it
/// is gone from its picture and nobody else's.**
///
/// The cube at the origin fills the centre of both targets. Hiding it from the
/// view has to turn the view's centre into the background its corner shows,
/// while the primary camera's centre keeps the cube — which is what separates a
/// working mask from a view that stopped drawing, and both from a cull that
/// dropped the cube everywhere.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_view_draws_its_own_picture_and_skips_what_is_hidden_from_it() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(device, headless.queue, headless.format).expect("a forward renderer");
    let cube: InstanceHandle = place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::IDENTITY,
    );
    let view = renderer
        .create_view(device, headless.queue, &ViewDesc::default())
        .expect("a view");
    let (image, image_view) = target_image(&headless, "view target", VIEW_EXTENT);

    let (primary, shown) = render_both(
        &headless,
        &mut renderer,
        &mut pool,
        view,
        (image, image_view),
    );
    let centre = |picture: &crcbl_golden::Image, extent: (u32, u32)| {
        picture
            .pixel(extent.0 / 2, extent.1 / 2)
            .expect("inside the frame")
    };
    let corner = |picture: &crcbl_golden::Image| picture.pixel(1, 1).expect("inside the frame");
    eprintln!(
        "crcbl forward e2e: views — primary centre {:?}, view centre {:?}, view corner {:?}",
        centre(&primary, MESH_EXTENT),
        centre(&shown, VIEW_EXTENT),
        corner(&shown)
    );
    assert_ne!(
        centre(&shown, VIEW_EXTENT),
        corner(&shown),
        "the view's centre has to be the cube, or the comparison below is between two empty \
         pictures"
    );

    renderer.set_instance_views(cube, ViewMask::ALL.without(view));
    let (primary_after, hidden) = render_both(
        &headless,
        &mut renderer,
        &mut pool,
        view,
        (image, image_view),
    );
    assert_eq!(
        centre(&hidden, VIEW_EXTENT),
        corner(&hidden),
        "the cube is hidden from the view, so its centre is the background"
    );
    assert_ne!(
        centre(&primary_after, MESH_EXTENT),
        corner(&primary_after),
        "and the primary camera still draws the cube it was never hidden from"
    );
    assert_eq!(
        centre(&primary_after, MESH_EXTENT),
        centre(&primary, MESH_EXTENT),
        "whose picture the view's mask does not touch"
    );

    device.wait_idle().expect("idle");
    renderer.destroy(device);
    device.destroy_image_view(image_view);
    device.destroy_image(image);
    pool.destroy(device);
    headless.finish();
}

/// **A view built from [`ViewDesc::default`] draws exactly the primary
/// camera's picture** when it is handed the primary camera and a target of the
/// primary's size — opaque alpha, sky, resolve and all.
///
/// What says a default view is the view it was before
/// [`ViewBackground`] existed: the primary camera is what every golden in the
/// tree is blessed from, and a default view that drifted from it by one byte —
/// an alpha, a skipped pass, a different extent — fails here.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_default_view_draws_the_primary_camera_s_picture_byte_for_byte() {
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
    // A sky, so the background the default view must keep is a pass it draws
    // and not the clear it would share with a view that drew none.
    renderer.set_sky(test_sky());
    let view = renderer
        .create_view(device, headless.queue, &ViewDesc::default())
        .expect("a view");
    let target = target_image(&headless, "default view target", MESH_EXTENT);
    let (primary, mut pictures) = render_views(
        &headless,
        &mut renderer,
        &mut pool,
        &[Drawn {
            view,
            camera: mesh_camera(crcbl::render::Projection::default()),
            target,
            extent: MESH_EXTENT,
        }],
    );
    let drawn = pictures.remove(0);
    let differing = primary
        .pixels()
        .chunks_exact(4)
        .zip(drawn.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    eprintln!(
        "crcbl forward e2e: views — a default view differs from the primary camera at \
         {differing} pixels"
    );
    assert_eq!(
        differing, 0,
        "a default view through the primary camera must draw the primary camera's picture"
    );
    assert!(
        drawn
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel[3] == COVERED),
        "and its alpha is opaque everywhere, as it always was"
    );

    device.wait_idle().expect("idle");
    renderer.destroy(device);
    device.destroy_image_view(target.1);
    device.destroy_image(target.0);
    pool.destroy(device);
    headless.finish();
}

/// The sky the transparent-view checks set, so the opaque background they
/// compare against is a drawn pass rather than a clear.
fn test_sky() -> Sky {
    Sky {
        zenith: Vec3::new(0.25, 0.35, 0.7),
        horizon: Vec3::new(0.6, 0.55, 0.5),
        ground: Vec3::new(0.15, 0.12, 0.1),
    }
}

/// What a transparent view's picture says about itself, counted.
struct Coverage {
    /// Pixels at [`COVERED`].
    covered: usize,
    /// Pixels at [`EMPTY`].
    empty: usize,
    /// Pixels at neither — a blended edge or a stray value.
    partial: usize,
}

fn coverage(picture: &crcbl_golden::Image) -> Coverage {
    let mut counted = Coverage {
        covered: 0,
        empty: 0,
        partial: 0,
    };
    for pixel in picture.pixels().chunks_exact(4) {
        match pixel[3] {
            COVERED => counted.covered += 1,
            EMPTY => counted.empty += 1,
            _ => counted.partial += 1,
        }
    }
    counted
}

/// **A transparent view's alpha is the frame's coverage, and where it covers,
/// its colour is the opaque view's.**
///
/// Two views through one camera into two targets of one size, in one frame:
/// one [`ViewDesc::transparent`], the other the same effects over the frame's
/// own background, which has a sky in it. Then:
///
/// * the transparent picture's alpha is exactly `0` or `255` at every pixel —
///   no filter blended an edge — with the cube's pixels at `255` and the
///   corner at `0`;
/// * **every covered pixel is the opaque view's pixel byte for byte**: the
///   transparent background changes nothing geometry shows;
/// * every empty pixel is black, where the opaque view shows the sky: no sky
///   was drawn under the transparent one.
///
/// Then again with bloom on the frame, whose composite is the one pass between
/// the forward pass and the tonemap that rewrites every pixel: the coverage has
/// to come through it unchanged.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_transparent_view_writes_coverage_into_alpha_and_the_opaque_colour_where_covered() {
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
    renderer.set_sky(test_sky());
    let transparent = renderer
        .create_view(device, headless.queue, &ViewDesc::transparent())
        .expect("a transparent view");
    let opaque = renderer
        .create_view(
            device,
            headless.queue,
            &ViewDesc {
                background: ViewBackground::Scene,
                ..ViewDesc::transparent()
            },
        )
        .expect("its opaque twin");
    let clear_target = target_image(&headless, "transparent view", VIEW_EXTENT);
    let opaque_target = target_image(&headless, "opaque view", VIEW_EXTENT);
    let camera = mesh_camera(crcbl::render::Projection::default());
    let both = [
        Drawn {
            view: transparent,
            camera,
            target: clear_target,
            extent: VIEW_EXTENT,
        },
        Drawn {
            view: opaque,
            camera,
            target: opaque_target,
            extent: VIEW_EXTENT,
        },
    ];

    let (_, pictures) = render_views(&headless, &mut renderer, &mut pool, &both);
    let (clear, solid) = (&pictures[0], &pictures[1]);
    let counted = coverage(clear);
    let pixels = (VIEW_EXTENT.0 * VIEW_EXTENT.1) as usize;
    let mut differing = 0usize;
    let mut worst = 0u8;
    let mut lit_background = 0usize;
    for (ours, theirs) in clear
        .pixels()
        .chunks_exact(4)
        .zip(solid.pixels().chunks_exact(4))
    {
        if ours[3] == COVERED {
            let apart = (0..3)
                .map(|c| ours[c].abs_diff(theirs[c]))
                .max()
                .unwrap_or(0);
            differing += usize::from(apart != 0);
            worst = worst.max(apart);
        } else if ours[..3] != [0, 0, 0] {
            lit_background += 1;
        }
    }
    eprintln!(
        "crcbl forward e2e: views — transparent view: {} covered, {} empty, {} partial of \
         {pixels}; {differing} covered pixels differ from the opaque view's, by at most \
         {worst}; opaque corner {:?}",
        counted.covered,
        counted.empty,
        counted.partial,
        solid.pixel(1, 1).expect("inside the frame"),
    );
    assert_eq!(
        counted.partial, 0,
        "every alpha is coverage, 0 or 255 — anything between is a filter or a blend"
    );
    assert!(
        counted.covered > pixels / 20 && counted.empty > pixels / 4,
        "the cube covers part of the picture and not all of it"
    );
    let centre = clear
        .pixel(VIEW_EXTENT.0 / 2, VIEW_EXTENT.1 / 2)
        .expect("inside the frame");
    let corner = clear.pixel(1, 1).expect("inside the frame");
    assert_eq!(centre[3], COVERED, "the cube is at the centre");
    assert_eq!(corner, [0, 0, 0, EMPTY], "and nothing is in the corner");
    assert!(
        solid
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel[3] == COVERED),
        "the opaque twin is opaque everywhere"
    );
    assert_ne!(
        solid.pixel(1, 1).expect("inside the frame")[..3],
        [0, 0, 0],
        "the opaque twin's corner is the sky, so the black below is a sky not drawn"
    );
    assert_eq!(
        lit_background, 0,
        "an empty pixel is transparent black: no sky and no clear colour under it"
    );
    assert_eq!(
        differing, 0,
        "a covered pixel is the colour the opaque view draws there"
    );

    // Bloom on the frame: its composite rewrites every pixel of the image the
    // tonemap reads, so it is the pass that would lose the alpha.
    renderer.set_effect_request(EffectRequest {
        camera: RenderEffects::DEFAULT_STACK.union(RenderEffects::BLOOM),
        ..EffectRequest::default()
    });
    let (_, bloomed) = render_views(&headless, &mut renderer, &mut pool, &both);
    assert!(
        renderer.effects().contains(RenderEffects::BLOOM),
        "the frame drew bloom, or this half checks nothing"
    );
    let after = coverage(&bloomed[0]);
    eprintln!(
        "crcbl forward e2e: views — with bloom: {} covered, {} empty, {} partial",
        after.covered, after.empty, after.partial
    );
    assert_eq!(after.partial, 0, "bloom kept the coverage whole");
    assert_eq!(
        (after.covered, after.empty),
        (counted.covered, counted.empty),
        "and exactly where it was"
    );

    device.wait_idle().expect("idle");
    renderer.destroy(device);
    for (image, view) in [clear_target, opaque_target] {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    pool.destroy(device);
    headless.finish();
}

/// **A transparent view drawn into an icon, copied into a `BGRA` atlas slot and
/// drawn as a sprite, shows the model over whatever the sprite is drawn on** —
/// `EW`'s model-to-icon path end to end, in one graph.
///
/// The fixture's format is pinned to `Bgra8UnormSrgb`, the swapchain format the
/// view renders in and so the atlas format the copy needs. The sprite covers
/// the whole frame at one texel a pixel, so each frame pixel is one icon texel:
/// where the icon is covered the frame holds the icon's colour, and where it is
/// empty it holds the backdrop the sprite was drawn over — which a sprite
/// sampling an opaque icon, or a straight-alpha blend fed a premultiplied or
/// alpha-less texel, would not.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_transparent_icon_copied_into_a_bgra_atlas_draws_over_what_is_behind_it() {
    let headless = Headless::open_at_format(
        MESH_EXTENT,
        Some(Format::Bgra8UnormSrgb),
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(device, headless.queue, headless.format).expect("a forward renderer");
    let cube = place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::IDENTITY,
    );
    // An icon's model: in its own view alone, and casting on nothing.
    let view = renderer
        .create_view(device, headless.queue, &ViewDesc::transparent())
        .expect("a transparent view");
    renderer.set_instance_views(cube, ViewMask::only(view));
    renderer.set_instance_casts_shadow(cube, false);

    let mut sprites =
        SpriteRenderer::new(device, headless.queue, headless.format).expect("a sprite renderer");
    let atlas = sprites
        .create_atlas(
            device,
            &AtlasDesc {
                label: "icon atlas",
                cell: MESH_EXTENT,
                columns: 1,
                rows: 1,
                sample: SampleMode::Pixel,
                format: Format::Bgra8UnormSrgb,
            },
        )
        .expect("a BGRA atlas");
    let slot = sprites.allocate_slot(atlas).expect("a free cell");
    let icon = target_image(&headless, "icon", MESH_EXTENT);
    let primary = target_image(&headless, "unused primary", MESH_EXTENT);
    /// What the sprite is drawn over, in linear light.
    const BACKDROP: [f32; 4] = [0.2, 0.45, 0.1, 1.0];

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let camera = mesh_camera(crcbl::render::Projection::default());
    renderer
        .begin_frame(device, &camera, &DirectionalLight::default(), MESH_EXTENT)
        .expect("the uniform buffers are writable");
    renderer
        .begin_view(device, view, &camera, MESH_EXTENT)
        .expect("the view's uniform buffers are writable");
    // The whole frame, in clip space: an identity camera and a quad from corner
    // to corner put icon texel (x, y) on frame pixel (x, y).
    sprites
        .begin_frame(
            device,
            &[Sprite::new(atlas, [-1.0, -1.0, 2.0, 2.0], slot.uv())],
            Mat4::IDENTITY,
            MESH_EXTENT,
        )
        .expect("the sprite buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("icon frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let import =
            |graph: &mut RenderGraph<'_>, label, (image, view): (ImageHandle, ImageViewHandle)| {
                graph.import_image(
                    label,
                    ImportedImage {
                        image,
                        view,
                        format: headless.format,
                        extent: MESH_EXTENT,
                        initial: ResourceState::Undefined,
                        claim: InitialClaim::Tracked,
                        final_state: ResourceState::TransferSrc,
                    },
                )
            };
        let primary_target = import(&mut graph, "unused primary", primary);
        let icon_target = import(&mut graph, "icon", icon);
        let swapchain = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        renderer.add_passes_with_views(
            &mut graph,
            &pool,
            FrameTargets {
                target: primary_target,
                extent: MESH_EXTENT,
                skinning: None,
                views: &[ViewTarget {
                    view,
                    target: icon_target,
                    extent: MESH_EXTENT,
                }],
            },
            |_, _| {},
        );
        sprites
            .add_slot_copies(
                &mut graph,
                &[SlotCopy {
                    source: icon_target,
                    slot,
                }],
            )
            .expect("the view's target is a cell-sized BGRA copy source");
        graph
            .add_render_pass("backdrop")
            .clear_color(swapchain, BACKDROP)
            .execute(|_| {});
        sprites.add_pass(&mut graph, swapchain);
        graph.compile(&pool).expect("a legal frame")
    };
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let frame_staging = copy_back(&headless, encoder.as_mut(), acquired.image, MESH_EXTENT);
    let icon_staging = copy_back(&headless, encoder.as_mut(), icon.0, MESH_EXTENT);
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
    let frame = read_back(&headless, frame_staging, MESH_EXTENT);
    let icon_picture = read_back(&headless, icon_staging, MESH_EXTENT);
    device.destroy_command_buffer(commands);

    let backdrop = frame.pixel(1, 1).expect("inside the frame");
    let counted = coverage(&icon_picture);
    let mut off_icon = 0usize;
    let mut off_backdrop = 0usize;
    for (shown, texel) in frame
        .pixels()
        .chunks_exact(4)
        .zip(icon_picture.pixels().chunks_exact(4))
    {
        let near = |a: &[u8], b: &[u8]| (0..3).all(|c| a[c].abs_diff(b[c]) <= 1);
        match texel[3] {
            COVERED => off_icon += usize::from(!near(shown, texel)),
            _ => off_backdrop += usize::from(!near(shown, &backdrop)),
        }
    }
    eprintln!(
        "crcbl forward e2e: views — icon: {} covered, {} empty, {} partial; backdrop {backdrop:?}; \
         {off_icon} covered pixels not the icon's colour, {off_backdrop} empty ones not the \
         backdrop",
        counted.covered, counted.empty, counted.partial
    );
    assert_eq!(counted.partial, 0, "the icon's alpha is coverage");
    assert!(
        counted.covered > 0 && counted.empty > 0,
        "the icon has a model in it and room around it"
    );
    assert_eq!(
        icon_picture.pixel(1, 1).expect("inside the frame")[3],
        EMPTY,
        "the icon's corner is empty, so the frame's corner is the backdrop"
    );
    assert_ne!(
        backdrop[..3],
        [0, 0, 0],
        "the backdrop shows through: a sprite blending the icon as opaque would draw its \
         transparent black here"
    );
    assert_eq!(off_backdrop, 0, "every empty texel shows the backdrop");
    assert_eq!(
        off_icon, 0,
        "every covered texel shows the icon's colour, the right way up"
    );

    device.wait_idle().expect("idle");
    sprites.destroy(device);
    renderer.destroy(device);
    for (image, view) in [icon, primary] {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    pool.destroy(device);
    headless.finish();
}

/// The light a fixed icon view is lit by in
/// [`a_fixed_view_draws_the_same_icon_under_any_frame_light`]: from over the
/// camera's shoulder, so the cube's visible faces are lit unevenly and a
/// picture of it has more than one colour in it.
const ICON_KEY: DirectionalLight = DirectionalLight {
    direction: Vec3::new(0.5, 0.8, 0.3),
    color: Vec3::new(1.2, 1.1, 1.0),
    ambient: Vec3::new(0.15, 0.15, 0.18),
};

/// What one world's frame drew through its two icon views: the fixed one and
/// its scene-lit twin.
struct Icons {
    fixed: crcbl_golden::Image,
    scene_lit: crcbl_golden::Image,
}

/// One frame of a renderer built over `scene`, lit by `sun` and whatever
/// `setup` gives it, with the cube at the origin drawn through a fixed icon view
/// and a scene-lit one — both transparent, through the same camera, into
/// targets of one size.
fn draw_icons(
    headless: &Headless,
    scene: &crcbl::render::scene::SceneDesc<'_>,
    sun: &DirectionalLight,
    setup: impl FnOnce(&mut ForwardRenderer),
) -> Icons {
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::with_scene(device, headless.queue, headless.format, scene)
        .expect("a forward renderer");
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::IDENTITY,
    );
    setup(&mut renderer);
    let fixed = renderer
        .create_view(
            device,
            headless.queue,
            &ViewDesc {
                lighting: ViewLighting::Fixed(ICON_KEY),
                ..ViewDesc::transparent()
            },
        )
        .expect("a fixed view");
    let scene_lit = renderer
        .create_view(device, headless.queue, &ViewDesc::transparent())
        .expect("its scene-lit twin");
    let fixed_target = target_image(headless, "fixed icon", VIEW_EXTENT);
    let scene_target = target_image(headless, "scene-lit icon", VIEW_EXTENT);
    let camera = mesh_camera(crcbl::render::Projection::default());
    let (_, mut pictures) = render_views_lit(
        headless,
        &mut renderer,
        &mut pool,
        sun,
        &[
            Drawn {
                view: fixed,
                camera,
                target: fixed_target,
                extent: VIEW_EXTENT,
            },
            Drawn {
                view: scene_lit,
                camera,
                target: scene_target,
                extent: VIEW_EXTENT,
            },
        ],
    );
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    for (image, view) in [fixed_target, scene_target] {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    pool.destroy(device);
    let scene_lit = pictures.remove(1);
    Icons {
        fixed: pictures.remove(0),
        scene_lit,
    }
}

/// How many pixels two pictures of one extent differ at, and by how much at
/// most in any channel, alpha included.
fn difference(a: &crcbl_golden::Image, b: &crcbl_golden::Image) -> (usize, u8) {
    let mut differing = 0usize;
    let mut worst = 0u8;
    for (ours, theirs) in a.pixels().chunks_exact(4).zip(b.pixels().chunks_exact(4)) {
        let apart = (0..4)
            .map(|c| ours[c].abs_diff(theirs[c]))
            .max()
            .unwrap_or(0);
        differing += usize::from(apart != 0);
        worst = worst.max(apart);
    }
    (differing, worst)
}

/// **A [`ViewLighting::Fixed`] icon is the same picture, byte for byte,
/// whatever lights the world it is drawn in** — and a scene-lit icon of the
/// same model is not, which is what says the two worlds differ where it
/// matters.
///
/// The first world is the demo scene under the default sun, with no sky, no
/// probes and no other light. The second has an irradiance grid over the
/// origin, a sky, a point light beside the cube and a sun of another colour
/// from another direction. Each draws the cube at the origin through two
/// transparent views — one fixed, one lit by the scene — and the check is
/// between the worlds:
///
/// * the two fixed icons are **identical**, alpha included. Every term that
///   differs between the worlds is one the fixed view does not read, so there
///   is no arithmetic by which a byte may move, and the comparison has no
///   tolerance;
/// * the two scene-lit icons differ across most of the cube — the control,
///   without which identical fixed icons could be two views that ignored the
///   frame's light for some other reason, or two worlds that happened to light
///   the cube alike;
/// * the fixed icon has the cube in it, lit, so the equality is between two
///   pictures of something.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_fixed_view_draws_the_same_icon_under_any_frame_light() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let plain = draw_icons(
        &headless,
        &crcbl::render::scene::demo(),
        &DirectionalLight::default(),
        |_| {},
    );
    let mut probed = crcbl::render::scene::demo();
    probed.probes = crcbl::screenshot::probe_grid();
    probed.capacities.probes = probed.probes.volume.total();
    let lit = draw_icons(
        &headless,
        &probed,
        &DirectionalLight {
            direction: Vec3::new(-0.7, 0.5, -0.2),
            color: Vec3::new(2.4, 0.9, 0.4),
            ambient: Vec3::new(0.02, 0.05, 0.12),
        },
        |renderer| {
            renderer.set_sky(test_sky());
            renderer.set_lights(&[crcbl::render::Light::Point(crcbl::render::PointLight {
                position: Vec3::new(0.9, 0.8, 0.9),
                radius: 4.0,
                color: Vec3::new(0.2, 1.5, 0.3),
                fill: false,
            })]);
        },
    );

    let (fixed_differing, fixed_worst) = difference(&plain.fixed, &lit.fixed);
    let (scene_differing, scene_worst) = difference(&plain.scene_lit, &lit.scene_lit);
    let counted = coverage(&plain.fixed);
    let centre = plain
        .fixed
        .pixel(VIEW_EXTENT.0 / 2, VIEW_EXTENT.1 / 2)
        .expect("inside the frame");
    eprintln!(
        "crcbl forward e2e: views — fixed icons differ between the worlds at {fixed_differing} \
         pixels (by at most {fixed_worst}); scene-lit icons at {scene_differing} (by at most \
         {scene_worst}); the fixed icon covers {} pixels, centre {centre:?}",
        counted.covered
    );
    assert!(
        counted.covered > 0 && centre[3] == COVERED && centre[..3] != [0, 0, 0],
        "the fixed icon has the cube in it, lit"
    );
    assert!(
        scene_differing > counted.covered / 2,
        "the two worlds light a scene-lit icon differently across most of the cube, or the \
         equality below says nothing about the frame's light"
    );
    assert_eq!(
        fixed_differing, 0,
        "a fixed icon is the same picture whatever lights the world it is drawn in"
    );
    headless.finish();
}
