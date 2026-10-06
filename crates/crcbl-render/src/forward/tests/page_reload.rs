//! [`ForwardRenderer::replace_page`], counted on the null backend: a replaced
//! page is a new image, each slot moves onto it at its own frame, and the old
//! one lives until the last slot has left it.
//!
//! The ring is the frame counter these read. [`FRAMES_IN_FLIGHT`] calls to
//! `begin_frame` visit every slot once, so "retired only after the
//! frames-in-flight count passes" is a count of images the recorder holds,
//! taken after each of those calls — no device, no fence.

use super::*;

/// A base-colour page of one layer at `extent`, every texel `texel`.
fn one_layer_page(extent: u32, texel: [u8; 4]) -> scene::PageDesc<'static> {
    let mut page = scene::PageDesc::empty();
    page.set_extent(PageKind::BaseColor, extent);
    page.push_layer(
        PageKind::BaseColor,
        texel.repeat(extent as usize * extent as usize),
    );
    page
}

/// One `begin_frame` on a default camera, which is what moves the ring.
fn begin(renderer: &mut ForwardRenderer, device: &dyn Device) {
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            (64, 48),
        )
        .expect("write");
}

/// The level-0 copies `recorder` saw into `image`, as their extents.
fn level0_copies(recorder: &Recorder, image: crcbl_hal::ImageHandle) -> Vec<crcbl_hal::Extent3d> {
    use crcbl_hal::null::Command;
    recorder
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::CopyBufferToImage(copy)
                if copy.image == image && copy.image_subresource.mip == 0 =>
            {
                Some(copy.image_extent)
            }
            _ => None,
        })
        .collect()
}

/// **A replaced page is a new image, and the old one is retired only once
/// every slot has moved off it** — after the frames-in-flight count of
/// `begin_frame`s, not at the call and not a frame early.
///
/// The image count rises by one at the call and holds through every slot's
/// frame but the last; the bind-group count never moves, because every rebuilt
/// group replaced one. A retirement that ran at the call — destroying an image
/// a submitted frame still samples — and one that never ran each move the
/// image count off the expected line; a lap of the ring that rebuilt again
/// would be a slot that did not know it had moved.
#[test]
fn a_replaced_page_moves_each_slot_at_its_own_frame_and_retires_the_old() {
    let (recorder, device, queue) = open_with(Features::GPU_DRIVEN);
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let extent = scene::demo().page.extent(PageKind::BaseColor);
    let before = renderer.base_color_page_import();
    let images = recorder.live_objects(ObjectKind::Image);
    let groups = recorder.live_objects(ObjectKind::BindGroup);

    renderer
        .replace_page(
            device,
            queue,
            PageKind::BaseColor,
            &one_layer_page(extent, [0x10, 0x20, 0x30, 0xFF]),
        )
        .expect("the same count at the same extent");
    let after = renderer.base_color_page_import();
    assert_ne!(after.image, before.image, "a replacement is a new image");
    assert_ne!(after.view, before.view);
    assert_eq!(
        level0_copies(&recorder, after.image),
        vec![crcbl_hal::Extent3d::d2(extent, extent)],
        "the new image holds the replacement's one layer"
    );
    assert_eq!(
        recorder.live_objects(ObjectKind::Image),
        images + 1,
        "the new page stands beside the one every slot still names"
    );
    assert_eq!(
        recorder.live_objects(ObjectKind::BindGroup),
        groups,
        "no group moves until its slot's frame"
    );

    for round in 0..FRAMES_IN_FLIGHT {
        begin(&mut renderer, device);
        let expected = if round + 1 < FRAMES_IN_FLIGHT {
            images + 1
        } else {
            images
        };
        assert_eq!(
            recorder.live_objects(ObjectKind::Image),
            expected,
            "round {round}: the replaced page lives until the last slot has left it"
        );
        assert_eq!(
            recorder.live_objects(ObjectKind::BindGroup),
            groups,
            "round {round}: every rebuilt group replaced one"
        );
        let frame = renderer.frame;
        let bound = renderer.primary.mesh_group_entries[frame]
            .iter()
            .find(|entry| entry.binding == BASE_COLOR_PAGE_BINDING)
            .map(|entry| entry.resource);
        assert_eq!(
            bound,
            Some(BindingResource::ImageView(after.view)),
            "round {round}: the slot that began a frame binds the new page"
        );
    }
    let groups_created = |recorder: &Recorder| {
        recorder
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::Created {
                        kind: ObjectKind::BindGroup,
                        ..
                    }
                )
            })
            .count()
    };
    let created = groups_created(&recorder);
    for _ in 0..FRAMES_IN_FLIGHT {
        begin(&mut renderer, device);
    }
    assert_eq!(
        groups_created(&recorder),
        created,
        "a slot already on the page in force rebuilds nothing"
    );

    renderer.destroy(device);
    recorder.assert_valid();
}

/// **A page of a different extent is a fresh allocation at that extent**, and
/// the graph declaration follows it, so a caller importing the page to write a
/// layer is told the size the image really is.
#[test]
fn a_page_of_a_new_extent_is_a_fresh_image_at_that_extent() {
    let (recorder, device, queue) = open_with(Features::GPU_DRIVEN);
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let extent = scene::demo().page.extent(PageKind::BaseColor);
    let before = renderer.base_color_page_import();
    let larger = extent * 2;

    renderer
        .replace_page(
            device,
            queue,
            PageKind::BaseColor,
            &one_layer_page(larger, [0xFF; 4]),
        )
        .expect("an extent change is a new image, which is always allowed");
    let after = renderer.base_color_page_import();
    assert_ne!(after.image, before.image);
    assert_eq!(after.extent, (larger, larger));
    assert_eq!(
        level0_copies(&recorder, after.image),
        vec![crcbl_hal::Extent3d::d2(larger, larger)],
        "the replacement went into an image of its own extent, not the old one's"
    );
    assert!(
        level0_copies(&recorder, before.image).len() == 1,
        "nothing was written into the image a frame in flight samples"
    );

    for _ in 0..FRAMES_IN_FLIGHT {
        begin(&mut renderer, device);
    }
    renderer.destroy(device);
    recorder.assert_valid();
}

/// **A replacement that would move a material row's layer is refused, and
/// changes nothing**: a different layer count, and a layer of the wrong length.
#[test]
fn a_replacement_that_would_move_a_layer_is_refused_and_changes_nothing() {
    let (recorder, device, queue) = open_with(Features::GPU_DRIVEN);
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let extent = scene::demo().page.extent(PageKind::BaseColor);
    let before = renderer.base_color_page_import();
    let images = recorder.live_objects(ObjectKind::Image);

    let mut two = one_layer_page(extent, [0xFF; 4]);
    two.push_layer(
        PageKind::BaseColor,
        [0u8; 4].repeat(extent as usize * extent as usize),
    );
    let refused = renderer.replace_page(device, queue, PageKind::BaseColor, &two);
    assert!(
        matches!(&refused, Err(HalError::InvalidDescriptor(why)) if why.contains("1 layer(s)")),
        "a second layer changes what the rows' numbers mean: {refused:?}"
    );

    let mut short = scene::PageDesc::empty();
    short.set_extent(PageKind::BaseColor, extent);
    short.push_layer(PageKind::BaseColor, vec![0xFF; 4]);
    assert!(
        matches!(
            renderer.replace_page(device, queue, PageKind::BaseColor, &short),
            Err(HalError::InvalidDescriptor(_))
        ),
        "a layer of the wrong length is refused before anything is created"
    );

    assert_eq!(renderer.base_color_page_import(), before);
    assert_eq!(
        recorder.live_objects(ObjectKind::Image),
        images,
        "a refusal creates nothing"
    );

    // A kind the renderer holds no layers of has nothing to replace.
    renderer
        .replace_page(device, queue, PageKind::Normal, &scene::PageDesc::empty())
        .expect("none for none is accepted");
    assert_eq!(recorder.live_objects(ObjectKind::Image), images);

    renderer.destroy(device);
    recorder.assert_valid();
}
