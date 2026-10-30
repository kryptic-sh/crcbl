//! The extents a frame is drawn at are the ones its `begin_frame` froze,
//! counted on the null backend.
//!
//! `ForwardRenderer::set_render_scale` takes effect at the next `begin_frame`:
//! that call writes every block for the frame's internal extent and builds the
//! upscale where it differs from the target's. What this pins is that a scale
//! moved between `begin_frame` and `add_passes` really waits, and that
//! `add_passes` handed a target of another extent is refused.

use super::*;

/// The extent the frame below draws its internal image at, and the labels of
/// the passes it records — with `between` run on the renderer after
/// `begin_frame` and before `add_passes`.
fn drawn(
    device: &dyn Device,
    queue: QueueHandle,
    renderer: &mut ForwardRenderer,
    add_extent: (u32, u32),
    between: impl FnOnce(&mut ForwardRenderer),
) -> ((u32, u32), Vec<String>) {
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            TEST_EXTENT,
        )
        .expect("write");
    between(renderer);
    let image = swapchain_image_at(device, add_extent);
    let pool = crate::TransientPool::new();
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", image);
    let mut internal = None;
    renderer.add_passes_with_views(
        &mut graph,
        &pool,
        FrameTargets {
            target,
            extent: add_extent,
            skinning: None,
            views: &[],
        },
        |_, targets| internal = Some(targets.internal_extent),
    );
    let labels = graph
        .compile(&pool)
        .expect("a legal frame")
        .passes()
        .iter()
        .map(|pass| pass.label().to_string())
        .collect();
    device.destroy_image_view(image.view);
    device.destroy_image(image.image);
    (internal.expect("the overlay ran"), labels)
}

/// **A render scale moved after `begin_frame` is the next frame's**, in both
/// directions: lowered, the frame is still drawn whole with no upscale;
/// raised back, the frame is still drawn at half and upscaled. The next frame
/// draws at the scale that was set.
#[test]
fn a_render_scale_moved_after_begin_frame_waits_for_the_next_frame() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let half = ForwardRenderer::scaled_extent(0.5, TEST_EXTENT);
    assert_ne!(
        half, TEST_EXTENT,
        "a half scale that rounds back draws no upscale"
    );
    let upscales = |labels: &[String]| labels.iter().any(|label| label == "upscale");

    let (internal, labels) = drawn(device, queue, &mut renderer, TEST_EXTENT, |renderer| {
        renderer.set_render_scale(0.5);
    });
    assert_eq!(
        internal, TEST_EXTENT,
        "the lowered scale reached this frame"
    );
    assert!(!upscales(&labels), "{labels:?}");

    let (internal, labels) = drawn(device, queue, &mut renderer, TEST_EXTENT, |renderer| {
        renderer.set_render_scale(1.0);
    });
    assert_eq!(
        internal, half,
        "the frame begun at half was not drawn at half"
    );
    assert!(upscales(&labels), "{labels:?}");

    let (internal, labels) = drawn(device, queue, &mut renderer, TEST_EXTENT, |_| {});
    assert_eq!(
        internal, TEST_EXTENT,
        "the raised scale never reached a frame"
    );
    assert!(!upscales(&labels), "{labels:?}");

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **`add_passes` handed a target `begin_frame` was not is refused**, rather
/// than drawn out of blocks written for another extent.
#[test]
#[should_panic(expected = "the `begin_frame` that opened this frame one of")]
fn add_passes_refuses_a_target_begin_frame_was_not_handed() {
    let (_recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    drawn(device, queue, &mut renderer, (32, 32), |_| {});
}
