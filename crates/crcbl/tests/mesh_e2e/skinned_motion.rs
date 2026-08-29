//! A **skinned** instance's motion vector, read back through the same debug
//! view [`crate::motion`] reads.
//!
//! That file's scene moves an instance's transform, which the geometry stages
//! have always been able to subtract: `GpuInstance::previous_transform` times
//! the vertex is where the fragment was. A deformed instance's vertex is not a
//! constant — `crcbl_render::skinning`'s dispatch writes a new pose into the
//! vertex pool every frame — so the same subtraction over *this* frame's vertex
//! says a limb moved exactly as the body it hangs off did, whatever it actually
//! did. `GpuInstance::previous_base_vertex` is the fix and this file is what
//! observes it.
//!
//! # The claim, and what a transform-only reading does with it
//!
//! The cube here sits at [`crate::motion::MOVING_AT`] under
//! [`Mat4::IDENTITY`] and **never moves**: its instance record carries the same
//! transform for every frame, so the previous-transform half of the subtraction
//! is zero throughout. What moves is the palette — one joint, every vertex bound
//! to it — and the frame that translates it by [`crate::motion::MOVE_BY`] owes
//! exactly the displacement `crate::motion`'s transform move measures for the
//! same cube under the same camera. A stage reading `vertex.position` on the
//! override arm reads rest there instead, which is
//! [`a_deformed_cube_reads_the_motion_its_palette_moved_it_by`]'s failure.
//!
//! # The numbers here were measured
//!
//! On radv, and quoted in the test that uses them, on [`crate::motion`]'s terms
//! — every band and every population floor in this file is that file's, because
//! the frame being asserted about is that file's frame drawn a different way.

use crate::harness::{Headless, poisoned};
use crate::hdr::HdrTarget;
use crate::mesh_scene::MESH_EXTENT;
use crate::motion::{
    CAMERA_MOTION_FLOOR, COVERED_FLOOR, MOTION_BAND, MOVE_BY, MOVING_AT, arm_motion_view, at_rest,
    covered, decode, motion_camera, project, texel_at,
};
use crcbl::hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Extent3d, ImageAspect,
    ImageSubresourceLayers, MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};
use crcbl::math::Mat4;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, ImportedImage, InitialClaim, RenderGraph,
    SkinnedInstanceDesc, SkinnedMesh, Skinning, SkinningDesc, TransientPool,
    forward::FRAMES_IN_FLIGHT,
};
use crcbl_shaders::skinning::{JOINTS_PER_VERTEX, SkinBinding};

/// The one skinned cube and the fixture it is drawn on, so a test can render
/// several frames through **one** renderer and one ping-pong.
///
/// [`crate::motion`]'s `MotionScene` with the skinned entry points in place of
/// the rigid ones, and for the reason that type gives: the previous half of the
/// region, the previous transform and the previous view-projection are all
/// state carried from one frame to the next, so a helper that opened a device
/// per frame would be measuring a renderer that had never drawn anything
/// before.
struct SkinnedMotionScene {
    headless: Headless,
    renderer: ForwardRenderer,
    pool: TransientPool,
    skinning: Skinning,
    /// Kept so it can be handed back before the renderer is destroyed — the
    /// region is the renderer's pool space, not this type's.
    skinned: SkinnedMesh,
    bindings: Vec<SkinBinding>,
}

impl SkinnedMotionScene {
    /// Opens the fixture and puts one skinned cube in the frame, at
    /// [`MOVING_AT`] and unspun, so its silhouette is the one
    /// [`crate::motion`]'s moving instance draws.
    fn open() -> Self {
        let headless = Headless::open_for_mesh();
        let device = headless.device.as_ref();
        let mut renderer = ForwardRenderer::new(device, headless.queue, headless.format)
            .expect("the forward renderer builds");
        arm_motion_view(&mut renderer);
        let skinned = renderer
            .reserve_skinned(crcbl::render::scene::DEMO_CUBE)
            .expect("the demo pool has room for two halves of a cube");
        renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &skinned,
                material: crcbl::render::scene::DEMO_UNTINTED,
                // **The transform never changes in this file**, which is what
                // makes every motion below the palette's. `MOVING_AT` is the
                // origin, so this is where `crate::motion`'s moving instance
                // stands before it moves.
                transform: Mat4::from_translation(MOVING_AT),
            })
            .expect("an instance pool of thousands has room for one object");
        let skinning = Skinning::new(
            device,
            &SkinningDesc {
                label: Some("skinned motion"),
                frames: FRAMES_IN_FLIGHT,
                ranges: 1,
                joints: 1,
                bindings: skinned.vertex_count(),
                // The renderer's own pool, which is what makes the dispatch's
                // output reachable by its draws at all.
                vertices: renderer.vertex_buffer(),
            },
        )
        .expect("a skinning pass");
        // **One joint and no split**, unlike `vk_e2e/skinning.rs`'s parted
        // cube: the claim here is that a whole surface's motion follows the
        // palette, so every vertex moving together is what lets the prediction
        // be a single displacement rather than a field.
        let bindings = vec![
            SkinBinding {
                joints: [0; JOINTS_PER_VERTEX],
                weights: [1.0, 0.0, 0.0, 0.0],
            };
            skinned.vertex_count() as usize
        ];
        Self {
            headless,
            renderer,
            pool: TransientPool::new(),
            skinning,
            skinned,
            bindings,
        }
    }

    /// Draws one skinned frame at `palette` and answers with the scene target
    /// it wrote.
    fn frame(&mut self, camera: &Camera, palette: &[Mat4]) -> HdrTarget {
        let hdr = self.render(camera, palette);
        assert_eq!(
            hdr.len(),
            (MESH_EXTENT.0 * MESH_EXTENT.1 * 8) as usize,
            "the scene target came back the wrong size, so every value read out of it is at \
             the wrong offset"
        );
        HdrTarget(hdr)
    }

    /// One frame of the forward pipeline through the **skinned** entry points,
    /// with the `Rgba16Float` scene target copied back.
    ///
    /// `tests/gpu_scene/mesh_scene.rs`'s `render_mesh_lit` with
    /// [`ForwardRenderer::begin_skinned_frame`] and
    /// [`ForwardRenderer::add_skinned_passes`] in place of the rigid pair — the
    /// shape `crates/crcbl-vk/tests/vk_e2e/skinning.rs` already reproduces for
    /// the same reason, since that file's frames also differ from the shared
    /// scene's in exactly those two calls. The swapchain image is acquired and
    /// presented but never read: this file's assertions are all about the HDR
    /// target, and the ring still has to be turned over for the next frame.
    fn render(&mut self, camera: &Camera, palette: &[Mat4]) -> Vec<u8> {
        let device = self.headless.device.as_ref();
        let (width, height) = MESH_EXTENT;
        let acquired = device
            .acquire_next_frame(self.headless.swapchain)
            .expect("the ring always has an image");
        assert_eq!(acquired.extent, MESH_EXTENT);

        let hdr_bytes = u64::from(width) * u64::from(height) * 8;
        let hdr_staging = device
            .create_buffer(&BufferDesc {
                label: Some("skinned motion hdr readback"),
                size: hdr_bytes,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");

        // **The skinned entry point, not `begin_frame` with a flag.** It moves
        // the ping-pong and then points the object at the half this frame's
        // dispatch fills *and* the half the frame before filled, in that order,
        // which is the whole of what this file is about.
        let range = self.skinned.skin_range(palette, &self.bindings);
        self.renderer
            .begin_skinned_frame(
                device,
                &mut self.skinning,
                core::slice::from_ref(&range),
                camera,
                &DirectionalLight::default(),
                MESH_EXTENT,
            )
            .expect("a legal skinning plan");

        // Where the graph's realised HDR handle lands, so the copy below can
        // name it — `mesh_scene.rs`'s argument for the `Cell` exactly: the pass
        // body runs synchronously inside `execute`, on this thread.
        let hdr_handle: std::cell::Cell<Option<crcbl::hal::ImageHandle>> =
            std::cell::Cell::new(None);
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("skinned motion frame"),
            queue: self.headless.queue,
        });
        let compiled = {
            let mut graph = RenderGraph::new(self.headless.queue);
            let target = graph.import_image(
                "swapchain",
                ImportedImage {
                    image: acquired.image,
                    view: acquired.view,
                    format: self.headless.format,
                    extent: MESH_EXTENT,
                    initial: ResourceState::Undefined,
                    claim: InitialClaim::Acquired,
                    final_state: ResourceState::TransferSrc,
                },
            );
            let scene = self.renderer.add_skinned_passes(
                &mut graph,
                &self.pool,
                target,
                MESH_EXTENT,
                &self.skinning,
            );
            let sink = &hdr_handle;
            graph
                .add_compute_pass("hdr probe")
                .use_image(scene, ResourceState::TransferSrc)
                .execute(move |ctx| sink.set(Some(ctx.image(scene))));
            graph.compile(&self.pool).expect("a legal frame")
        };
        eprintln!("{}: {}", crate::SUITE, compiled.dump());
        compiled
            .execute(device, &mut self.pool, encoder.as_mut(), None)
            .expect("the graph executed");

        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: hdr_staging,
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: hdr_handle.get().expect("the probe pass ran"),
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: crcbl::hal::Offset3d::default(),
            image_extent: Extent3d::d2(width, height),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(self.headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device
            .present(
                self.headless.queue,
                &PresentInfo {
                    swapchain: self.headless.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");

        let mut hdr = poisoned(hdr_bytes as usize);
        self.headless.readback(hdr_staging, hdr_bytes, &mut hdr);
        device.destroy_command_buffer(commands);
        device.destroy_buffer(hdr_staging);
        hdr
    }

    /// Releases everything in dependency order and asks the fixture what the
    /// device saw.
    fn finish(self) {
        let device = self.headless.device.as_ref();
        device.wait_idle().expect("the queue drains");
        self.skinning.destroy(device);
        let mut renderer = self.renderer;
        renderer.release_skinned(self.skinned);
        renderer.destroy(device);
        let mut pool = self.pool;
        pool.destroy(device);
        self.headless.finish();
    }
}

/// The palette that leaves the cube in its bind pose: one joint, no transform.
fn rest_palette() -> Vec<Mat4> {
    vec![Mat4::IDENTITY]
}

/// The palette that carries the whole cube along `+X` by [`MOVE_BY`].
///
/// **The same displacement [`crate::motion`] gives its moving instance**, so
/// the prediction and the band are that file's — see the module docs.
fn moved_palette() -> Vec<Mat4> {
    vec![Mat4::from_translation(MOVE_BY)]
}

/// Every covered texel of `frame`, asserted at rest, with the population it ran
/// over checked against [`COVERED_FLOOR`].
fn assert_at_rest(frame: &HdrTarget, what: &str) {
    let field = covered(frame);
    let mut drawn = 0usize;
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        drawn += 1;
        let motion = decode(*encoded);
        assert!(
            at_rest(motion),
            "{what}: texel {:?} reads {motion:?}, and the deformed surface did not move",
            texel_at(index)
        );
    }
    assert!(
        drawn > COVERED_FLOOR,
        "{what} covered only {drawn} texel(s), so the loop above checked almost nothing"
    );
}

/// **A skinned cube whose palette does not change reads rest**, on the frame it
/// is placed and on the frame after it.
///
/// The two frames are different claims. The first is the one with no previous
/// deformation at all: the other half of the region holds whatever its
/// reservation came with, and a renderer that pointed the record at it would
/// subtract against undefined vertices — which is a motion field of noise, not
/// a wrong number. The second is the one where the previous half is real and
/// holds the same pose, which is what a subtraction that *happens* has to come
/// out of at zero.
///
/// Measured on radv: 3553 covered texels on the first frame and 3553 on the
/// second, every one of them at rest.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_skinned_cube_at_a_still_palette_reads_rest() {
    let camera = motion_camera();
    let mut scene = SkinnedMotionScene::open();
    let first = scene.frame(&camera, &rest_palette());
    let second = scene.frame(&camera, &rest_palette());
    scene.finish();

    assert_at_rest(&first, "the first frame");
    assert_at_rest(&second, "the second frame");
}

/// **A cube the palette moved reads that motion, with an instance transform
/// that never changed** — and the frame after it, at the same palette, is back
/// at rest.
///
/// This is the claim a transform-only reading fails. The instance's transform
/// and previous transform are equal on every frame here, so
/// `GpuInstance::previous_transform` contributes nothing at all; a geometry
/// stage that put it through *this* frame's deformed vertex reads rest on the
/// moved frame, and the band below is what says it did not.
///
/// The prediction is [`project`]'s, over the same two world points
/// `crate::motion`'s transform move projects: the palette translates every
/// vertex by [`MOVE_BY`], so the deformed cube is the rigid cube at
/// `MOVING_AT + MOVE_BY` and the displacement its texels owe is the same one.
/// [`MOTION_BAND`] is that file's band for the same reason — a cube has depth
/// and a perspective camera carries its near corners further than its far ones,
/// so the population is a band and not a value.
///
/// The fourth frame is the control that separates "the palette moved" from "the
/// halves differ": the palette is unchanged from the third, so both halves hold
/// the same deformed pose and every texel owes rest again. A renderer that
/// pointed the previous base at the bind pose for ever — or at a half nothing
/// filled — would satisfy the third frame and fail this.
///
/// Measured on radv: the palette moves the projected centre 0.0372 of the
/// frame's width, all 3737 of the moved frame's covered texels read between
/// 0.0333 and 0.0455 in `u` — 0.894 to 1.224 times the centre's, the same band
/// [`crate::motion`] measured for the transform move — and all 3737 of the
/// fourth frame's read rest.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_deformed_cube_reads_the_motion_its_palette_moved_it_by() {
    let camera = motion_camera();
    let mut scene = SkinnedMotionScene::open();
    // Two frames at rest, so the history the third subtracts against is a real
    // deformation rather than the first frame's own.
    let _ = scene.frame(&camera, &rest_palette());
    let _ = scene.frame(&camera, &rest_palette());
    let moved = scene.frame(&camera, &moved_palette());
    let settled = scene.frame(&camera, &moved_palette());
    scene.finish();

    let before = project(&camera, MOVING_AT);
    let after = project(&camera, MOVING_AT + MOVE_BY);
    let predicted = (after[0] - before[0]) / MESH_EXTENT.0 as f32;
    assert!(
        predicted > CAMERA_MOTION_FLOOR,
        "this camera moves the deformed centre by {predicted} in u, so the positive sign and \
         the band below are not what its motion works out to"
    );

    let field = covered(&moved);
    let mut drawn = 0usize;
    for (index, texel) in field.iter().enumerate() {
        let Some(encoded) = texel else {
            continue;
        };
        drawn += 1;
        let motion = decode(*encoded);
        assert!(
            motion[0] > predicted * MOTION_BAND.0 && motion[0] < predicted * MOTION_BAND.1,
            "texel {:?} belongs to a cube whose palette carried it {predicted} across the \
             frame and reads {motion:?} in u — a value at rest is the previous position \
             being read out of this frame's deformed vertices",
            texel_at(index)
        );
    }
    assert!(
        drawn > COVERED_FLOOR,
        "the deformed cube covered only {drawn} texel(s), so the loop above checked almost \
         nothing"
    );

    assert_at_rest(&settled, "the frame after the palette settled");
}
