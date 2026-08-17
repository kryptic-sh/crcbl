//! The graph and its transient pool under a resize storm.
//!
//! This is the path with the most moving parts and the least obvious failure
//! mode, and until it moved here it had only ever run on Vulkan — where getting
//! a transient's lifetime wrong is a validation error rather than a wrong
//! picture. Every other backend had no evidence at all that a window being
//! dragged does not leak one set of scene targets per size it passed through.

use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube};
use crcbl::hal::{
    CommandEncoderDesc, CompositeAlpha, PresentInfo, PresentMode, SubmitInfo, SwapchainDesc,
};
use crcbl::render::{
    DirectionalLight, ForwardRenderer, PassKind, Projection, RenderGraph, TransientPool,
};

/// A resize storm, driven through the **render graph** rather than around it.
///
/// Every size change invalidates both scene transients, so the pool must hand
/// out new ones and retire the old; and the tonemap's bind group names a
/// *graph-owned* view, so it must be rebuilt when that view changes and
/// destroyed when it does — while a previous frame may still be reading it.
///
/// Three things are asserted, and each one is something a picture does not show:
///
/// * **Every render pass renders at the size that was just configured**, which
///   is the graph deriving its render area from the attachments rather than from
///   anything remembered. The shadow pass is the one exception and is asserted
///   as one: topic 18's atlas is a quality setting, so a resize must leave its
///   extent alone — and an atlas re-created at the swapchain's size would still
///   render and still pass every golden.
/// * **The pool converges rather than accumulating** one set of targets per size
///   the window passed through: this frame's own transients, plus at most
///   `RETIRE_AFTER_FRAMES` generations of stale ones. The per-frame figure is
///   read off the compiled graph rather than written down, so a pass added to
///   the forward path moves the ceiling instead of failing this test about
///   something else.
/// * **And the device saw nothing out of band.** [`Headless::finish`] is what
///   stands in for the Vulkan original's `validation_report().assert_clean()`
///   here — a bind group destroyed while in flight, a transient freed too early
///   or a stale view sampled is the failure this exists to catch, and on Vulkan
///   it still arrives through the layer that runs under this suite too.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_graph_and_its_pool_survive_a_resize_storm() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());
    let ring = format!("{} ring", crate::SUITE);

    // Sizes chosen to be genuinely different rather than a nudge — including one
    // that is not a multiple of anything, because a row-pitch assumption hides
    // behind round numbers. Nothing here is read back, so the odd size is a
    // claim about the graph and the pool rather than about a copy.
    let sizes = [
        MESH_EXTENT,
        (64, 48),
        (300, 130),
        (17, 5),
        (256, 192),
        (64, 48),
        MESH_EXTENT,
    ];
    for extent in sizes {
        device
            .reconfigure_swapchain(
                headless.swapchain,
                &SwapchainDesc {
                    label: Some(&ring),
                    surface: headless.surface,
                    format: headless.format,
                    extent,
                    image_count: 2,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("reconfigure keeps the handle valid");

        // How many physical images one frame of this renderer needs, read off
        // the last compiled frame. The ceiling below is a multiple of it, and
        // taking it from the graph is what keeps that ceiling a statement about
        // *retirement*.
        let mut per_frame = 0;
        // Two frames per size, so the second one exercises the *reuse* path
        // rather than only the create path.
        for _ in 0..2 {
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("an image");
            assert_eq!(acquired.extent, extent);
            renderer
                .begin_frame(device, &camera, &DirectionalLight::default(), extent)
                .expect("uniforms");

            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("resize frame"),
                queue: headless.queue,
            });
            let compiled = {
                let mut graph = RenderGraph::new(headless.queue);
                let target = graph.import_image(
                    "swapchain",
                    ForwardRenderer::present_target(
                        acquired.image,
                        acquired.view,
                        headless.format,
                        extent,
                    ),
                );
                let _ = renderer.add_passes(&mut graph, target, extent);
                graph.compile(&pool).expect("a legal frame")
            };
            per_frame = compiled.physical_image_count();
            // Every *render* pass renders at the size that was just configured.
            // A compute pass has no attachments and so no area — asserting one
            // on the cull and draw-argument dispatches would be asserting on a
            // zero the graph fills in for them.
            let mut rendered = 0;
            let mut shadow_atlas = false;
            for pass in compiled.passes() {
                if pass.kind() != PassKind::Render {
                    continue;
                }
                rendered += 1;
                let expected = if pass.label() == "shadow" {
                    shadow_atlas = true;
                    crcbl::render::shadow::atlas_extent()
                } else {
                    extent
                };
                assert_eq!(
                    (pass.render_area().width, pass.render_area().height),
                    expected,
                    "pass {:?} rendered at the wrong size",
                    pass.label()
                );
            }
            assert!(
                rendered >= 2,
                "the forward and tonemap passes, or this loop checked nothing"
            );
            assert!(
                shadow_atlas,
                "no shadow pass in the frame, so the extent above checked nothing"
            );
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("executed");
            let commands = encoder.finish().expect("recorded");
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
            // Nothing is pipelined here, so an idle stands in for the frame
            // loop's timeline wait before the pool retires anything.
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);
            pool.retire_unused(device);
        }

        assert!(
            per_frame > 0,
            "the frame declared no transient images, so the ceiling below is zero \
             and the assertion after it could not fail"
        );
        let ceiling = per_frame * (crcbl::render::transient::RETIRE_AFTER_FRAMES as usize + 1);
        assert!(
            pool.image_count() <= ceiling,
            "after resizing to {extent:?} the pool holds {} images, over the {ceiling} \
             a bounded retirement allows",
            pool.image_count()
        );
    }

    renderer.destroy(device);
    pool.destroy(device);
    // The device's own verdict is the assertion, exactly as the validation
    // report was in the Vulkan original.
    headless.finish();
}
