//! **§3.5's mesh-shader path and its amplification stage** — the half of the
//! lit-mesh cluster that only a device reporting `VK_EXT_mesh_shader` can run.
//!
//! It is also the suite's shared scene, which is why it is one module rather
//! than several: the modules beside it import its extent, camera, spin constant
//! or frame helper instead of inventing their own.
//!
//! # What left, and what could not
//!
//! The backend-agnostic half is `crates/crcbl/tests/mesh_e2e/`, which runs on
//! Vulkan, Metal, D3D12 and wgpu from one binary — the four goldens, the
//! `Rgba16Float` scene target, the resize storm and the uniform level-of-detail
//! cut. **The four reference images went with it**, to
//! `crates/crcbl/tests/golden/`, and the two tests below that still compare
//! against one read *that* file rather than a second copy: the whole claim those
//! tests make is that the mesh path draws the *same* picture, so the reference
//! has to be the same picture.
//!
//! Every test left here opens its device demanding
//! [`Features::MESH_SHADER`] — most of them [`Features::TASK_SHADER`] as well —
//! and then asserts on what the adapter selected. wgpu, WARP and Metal have no
//! mesh stage at all, so each one is a claim about radv and lavapipe rather than
//! about the seam:
//!
//! * [`the_mesh_shader_path_matches_the_indirect_path_s_golden`] and
//!   [`a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths`]
//!   compare two *named* paths against each other, and one of the two does not
//!   exist elsewhere.
//! * [`per_cluster_culling_rejects_the_clusters_a_camera_hides`] and
//!   [`a_scaled_instance_keeps_the_clusters_its_own_size_puts_on_screen`] count
//!   the amplification stage's survivors out of the statistics ring. Nothing
//!   writes that word on a device with no amplification stage, so their
//!   agnostic half would be a counter nobody incremented.
//! * [`the_mesh_dispatch_extent_is_the_culled_instance_count`] reads
//!   `MeshTasksArgs`, which only the mesh path's tail emits. Its agnostic half —
//!   that each bucket's `DrawIndexedArgs::instance_count` follows the scene — is
//!   already `crates/crcbl/tests/draw_gen_e2e/`'s
//!   `a_bucket_fills_and_empties_as_its_instance_comes_and_goes`, so splitting
//!   it would have produced a second copy of a test that already runs
//!   everywhere.
//! * [`the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`] and
//!   [`the_shadow_cascades_select_coarser_than_the_camera`] read the per-cluster
//!   cut out of `ForwardRenderer::cluster_selection`, which only the
//!   amplification stage writes.
//! * [`the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is`] is what
//!   is left of a test that was split: the uniform cut's own claims — that it
//!   gets coarser with distance and that each level is the host rule's — are
//!   `mesh_e2e/lod.rs`'s and run everywhere, and what stayed is the comparison
//!   between the two granularities, which needs a mesh stage on one side.
//!
//! `a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths` was
//! split the same way: the picture the open box draws is the seam's and is
//! `mesh_e2e/goldens.rs`'s, and the byte-for-byte comparison of two paths is
//! what stayed.
//!
//! [`Features::MESH_SHADER`]: crcbl_hal::Features::MESH_SHADER
//! [`Features::TASK_SHADER`]: crcbl_hal::Features::TASK_SHADER

use crate::harness::{DeviceSlot, Headless, POISON, instance, poisoned};
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, CompositeAlpha,
    DeviceDesc, Extent3d, Features, Format, ImageAspect, ImageSubresourceLayers, Instance,
    MemoryLocation, PresentInfo, PresentMode, ResourceState, SubmitInfo, SwapchainDesc,
};

/// The size the mesh suite renders at.
///
/// The same 256×192 as the triangle, and for the same reason: the golden's
/// structural metric works on 8×8 blocks, and a smaller frame gives it too few
/// to average over.
pub(crate) const MESH_EXTENT: (u32, u32) = (256, 192);

/// Where the camera is for every mesh golden.
///
/// Far enough back that the cube does not touch the frame edge under either
/// projection, and off-axis on two of three axes so **three faces are visible at
/// once** — which is what makes a directional light legible and an orientation
/// mistake a different picture rather than a plausible one.
pub(crate) fn mesh_camera(projection: crcbl_render::Projection) -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(1.6, 1.2, 2.2),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection,
    }
}

/// The animation time every mesh golden renders at.
///
/// A constant, not a clock: a golden image of a spinning cube is only evidence
/// if the cube is in the same place every run.
///
/// Chosen so **three faces are visible at once**, which is not automatic — at
/// `0.7` the cube's `+X` face is edge-on to this camera to within a fifth of a
/// degree, and a two-face frame cannot show a lighting gradient however correct
/// the shader is. Three faces also means no symmetry is left for a transposed
/// matrix or a mirrored axis to hide behind.
pub(crate) const MESH_SECONDS: f32 = 0.35;

/// Puts one of the demo scene's meshes in the frame at `transform`, and answers
/// with the handle that names it.
///
/// **Insertion order is the caller's and it is load-bearing** — the slot an
/// object lands in is `docs/plan/25-lod.md`'s hysteresis key — so every scene in
/// this suite places its objects in the order the frame has always held them:
/// the cube first, then whatever stands beside it. That is what kept the goldens
/// still when the five demo setters that used to do this were retired.
pub(crate) fn place(
    renderer: &mut crcbl_render::ForwardRenderer,
    mesh: usize,
    material: usize,
    transform: glam::Mat4,
) -> crcbl_render::InstanceHandle {
    renderer
        .add_instance(&crcbl_render::InstanceDesc {
            mesh,
            material,
            transform,
        })
        .expect("an instance pool of thousands has room for a test scene")
}

/// Puts the demo scene's cube in the frame at `transform`, and **first**, so it
/// takes the pool slot it has always had.
///
/// The renderer used to insert this object itself and to take its transform as a
/// `begin_frame` argument. It is an ordinary instance now, so every renderer in
/// this suite places it the way an application does — and the scene the goldens
/// were blessed on is the same scene.
pub(crate) fn place_cube_at(renderer: &mut crcbl_render::ForwardRenderer, transform: glam::Mat4) {
    place(
        renderer,
        crcbl_render::scene::DEMO_CUBE,
        crcbl_render::scene::DEMO_UNTINTED,
        transform,
    );
}

/// The same, at the spin every frame in this suite is drawn at.
pub(crate) fn place_cube(renderer: &mut crcbl_render::ForwardRenderer) {
    place_cube_at(renderer, crcbl_render::ForwardRenderer::spin(MESH_SECONDS));
}

/// The demo scene's dunes patch, **at the origin and with no rotation**.
///
/// The identity is load-bearing rather than a default: the amplification stage
/// puts each group's sphere through the instance transform and leaves the camera
/// where it is, which is the same statement as putting the camera through the
/// inverse — and at the identity the two are the same *bits*, which is what lets
/// the host-versus-GPU comparisons in this suite be equalities rather than
/// tolerances.
///
/// Every caller asks [`ForwardRenderer::selects_levels`] first. A device with a
/// mesh stage and no amplification stage cannot choose a level of a DAG at all,
/// and `add_instance` has no vocabulary for refusing one.
///
/// [`ForwardRenderer::selects_levels`]: crcbl_render::ForwardRenderer::selects_levels
pub(crate) fn place_dunes(renderer: &mut crcbl_render::ForwardRenderer) {
    place(
        renderer,
        crcbl_render::scene::DEMO_DUNES,
        crcbl_render::scene::DEMO_UNTINTED,
        glam::Mat4::IDENTITY,
    );
}

impl Headless {
    /// Opens a ring at a pinned format for the mesh suite. See
    /// [`Headless::open_for_triangle`] on why the format is pinned rather than
    /// preferred.
    pub(crate) fn open_for_mesh() -> Self {
        Self::open_for_mesh_with(Features::GPU_DRIVEN)
    }

    /// The same ring, with a chosen optional feature set.
    ///
    /// **The set decides the forward pass's emit tail.** A device opened
    /// without [`Features::DRAW_INDIRECT_COUNT`] selects
    /// [`GeometryPath::IndirectPerBatch`](crcbl_hal::GeometryPath::IndirectPerBatch),
    /// which is the path Metal is on and which no adapter this suite can see
    /// would otherwise select — so asking for less is the only way to run that
    /// arm on real hardware at all.
    pub(crate) fn open_for_mesh_with(optional: Features) -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e mesh"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: optional | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e mesh ring"),
                surface,
                format,
                extent: MESH_EXTENT,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");
        Self {
            instance,
            device: DeviceSlot::new(device),
            surface,
            swapchain,
            queue,
            format,
        }
    }
}

/// Renders one frame of the forward pipeline **through the real render graph**
/// and reads the swapchain image back.
///
/// Deliberately `crcbl_render::ForwardRenderer` and `crcbl_render::RenderGraph`
/// rather than a hand-built copy: a golden image is only evidence about the code
/// the sandbox runs if it *is* the code the sandbox runs.
///
/// The `Rgba16Float` probe pass this used to record went to
/// `crates/crcbl/tests/gpu_scene/mesh_scene.rs` with the two lighting tests that
/// were its only readers; nothing left in this module measures the scene target.
pub(crate) fn render_mesh(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    camera: &crcbl_render::Camera,
) -> crcbl_golden::Image {
    use crcbl_render::RenderGraph;

    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let color_staging = device
        .create_buffer(&BufferDesc {
            label: Some("mesh readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    renderer
        .begin_frame(
            device,
            camera,
            &crcbl_render::DirectionalLight::default(),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
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
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it as a copy source and the
                // copy below needs no barrier of its own. Saying so in the
                // import is the whole point — there is still not one
                // hand-written barrier anywhere in this file's mesh path.
                final_state: ResourceState::TransferSrc,
            },
        );
        let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        // `&*pool`: the same pool the frame is about to be realised
        // against, so the barriers open where the last frame left off.
        graph.compile(&*pool).expect("a legal frame")
    };
    eprintln!("vk e2e: {}", compiled.dump());
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    // The copy is outside every pass and needs no barrier: the graph left the
    // swapchain image in `TransferSrc` because the import declared it that way.
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: color_staging,
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

    let mut color = poisoned(color_bytes as usize);
    headless.readback(color_staging, color_bytes, &mut color);

    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}

/// Where a golden that moved to `crates/crcbl/tests/mesh_e2e/` now lives.
///
/// **One reference file, reached from both sides.** The two tests below that
/// compare against a golden are each about the mesh path drawing the *same*
/// picture some other path already pins, so a copy under this crate would be a
/// second reference that could drift and turn both of them into checks that
/// cannot fail.
fn migrated_golden(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../crcbl/tests/golden/{name}.png"))
}

/// **The mesh-shader path draws the frame the indirect path's golden already
/// pins** — the same reference file, not one of its own.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes mesh shaders the primary
/// geometry path and its own design rule is that "the lesser path is a
/// constraint on data layout, not a separate renderer". A second golden for
/// this path would pass whatever the path happened to draw, which is the check
/// that cannot fail; the whole claim is that the picture is the *same* one, so
/// the reference has to be the same one.
///
/// # How this says the mesh stage actually ran
///
/// A path that silently degraded to an indirect tail would match this golden
/// too, so the golden is not the evidence — [`ForwardRenderer::geometry_path`]
/// is. It reports what the renderer **built**, and a renderer that built the
/// mesh path built a mesh pipeline out of `mesh_cluster.slang` and created no
/// raster pipeline at all. There is nothing for the pass to have fallen through
/// to, so a frame that matched came out of a mesh stage.
///
/// `crcbl-render`'s own `a_mesh_shader_device_draws_through_a_mesh_pipeline` is
/// the other half: it reads the recorded command stream and fails on an
/// indirect draw or an index-buffer bind.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_mesh_shader_path_matches_the_indirect_path_s_golden() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN | Features::MESH_SHADER);
    assert_eq!(
        headless.device.caps().geometry_path(),
        crcbl_hal::GeometryPath::MeshShader,
        "this test needs a device that selects the mesh path; radv reports \
         VK_EXT_mesh_shader and `docs/backlog.md` is where a driver that does not belongs"
    );
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds a mesh pipeline");
    place_cube(&mut renderer);
    assert_eq!(
        renderer.geometry_path(),
        crcbl_hal::GeometryPath::MeshShader,
        "the renderer must have built the mesh path — this is the assertion that \
         makes the golden below evidence about a mesh stage rather than about a \
         fall-through"
    );

    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    let golden = crcbl_golden::Golden::new(migrated_golden("mesh"));
    let outcome = golden.check(&frame).expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!(
        "vk e2e: golden mesh through the mesh-shader path — {}",
        comparison.summary()
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// [`mesh_camera`] pulled back far enough for two meshes to fit side by side.
///
/// The same eye direction, the same target, the same projection — only the
/// distance differs, because a frame that has to hold the cube *and* a mesh
/// beside it needs about twice the width the cube alone does. Keeping the
/// direction is what makes this frame comparable to the cube golden's: three
/// cube faces are visible in both.
fn two_mesh_camera() -> crcbl_render::Camera {
    let mut camera = mesh_camera(crcbl_render::Projection::default());
    camera.eye *= 1.7;
    camera
}

/// Where the multi-cluster frames put the open box, in world units.
///
/// Left of the cube at [`two_mesh_camera`]'s distance and clear of it: the two
/// meshes have to be separable by eye, because the failure these frames exist to
/// catch is one mesh drawing the *other's* clusters. The same place
/// `mesh_e2e/goldens.rs` puts it, because the golden the arms below compare
/// against is that suite's.
const OPEN_BOX_AT: glam::Vec3 = glam::Vec3::new(-2.0, 0.0, 0.0);

/// What one arm of the multi-cluster comparison drew, and how.
struct OpenBoxFrame {
    /// The path the renderer **built**, which is the claim that matters — see
    /// [`the_mesh_shader_path_matches_the_indirect_path_s_golden`].
    path: crcbl_hal::GeometryPath,
    /// Whether §3.5's amplification stage was in front of it.
    culls_clusters: bool,
    image: crcbl_golden::Image,
}

/// One frame of the cube and the open box, on whichever geometry path
/// `optional` selects, with the device opened and closed around it.
fn render_open_box(optional: Features) -> OpenBoxFrame {
    let headless = Headless::open_for_mesh_with(optional);
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let path = renderer.geometry_path();
    let culls_clusters = renderer.culls_clusters();
    place(
        &mut renderer,
        crcbl_render::scene::DEMO_OPEN_BOX,
        crcbl_render::scene::DEMO_UNTINTED,
        glam::Mat4::from_translation(OPEN_BOX_AT),
    );

    let image = render_mesh(&headless, &mut renderer, &mut pool, &two_mesh_camera());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    OpenBoxFrame {
        path,
        culls_clusters,
        image,
    }
}

/// **A mesh of several clusters draws the same frame through both geometry
/// paths**, and the per-cluster cull in front of the mesh stage does not move
/// it.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5's unit of work is a cluster, and
/// the open box is the only mesh in the demo scene with more than one of them:
/// the cube is 24 vertices and the pyramid 16, against a bound of 64. So a mesh
/// stage that ignored [`Meshlet::vertex_offset`] and
/// [`Meshlet::triangle_offset`] *within* a mesh drew a correct picture of both,
/// and `crcbl_shaders::meshlet::open_box_clusters` is the geometry that stops
/// that being true — five clusters, one per face, four of them at offsets that
/// are not zero.
///
/// [`Meshlet::vertex_offset`]: crcbl_shaders::meshlet::Meshlet::vertex_offset
/// [`Meshlet::triangle_offset`]: crcbl_shaders::meshlet::Meshlet::triangle_offset
///
/// # Why this could not move
///
/// The claim about the *picture* — that the box's five clusters each draw their
/// own face, in their own colours, matching the reviewed reference — is the
/// seam's, and it is
/// `crates/crcbl/tests/mesh_e2e/goldens.rs`'s
/// `a_multi_cluster_mesh_draws_one_face_per_cluster_and_matches_its_golden`,
/// which runs on all four backends. What is left here names two paths and can
/// only run where both exist.
///
/// # What each assertion rules out
///
/// * **The two paths agree byte for byte.** One adapter, one driver, one scene:
///   every differing pixel would be a difference the two submissions made. This
///   is the comparison that gives the mesh path something to be wrong against,
///   because the indirect path reads the index buffer and never touches a
///   cluster record at all.
/// * **The frame is not blank.** Two empty frames agree byte for byte too, so
///   the comparison above means nothing until something is in the picture. The
///   count is measured rather than assumed — every face of both meshes is
///   flat-shaded, and a cleared frame is one distinct colour.
/// * **The mesh arm matches the golden the agnostic suite blessed**, from that
///   suite's own file rather than a copy. A mesh stage that lost the
///   per-cluster offsets emits face zero five times over: the same triangle
///   count, the same buffer sizes, a different picture.
/// * **And the culled frame is the same picture.** A third arm asks for
///   `Features::TASK_SHADER` as well, which is what makes the renderer build
///   §3.5's per-cluster cull — and from this camera that cull rejects two of
///   the box's five clusters. Both are walls seen from behind, so both were
///   contributing no pixels and the frame must come out identical. That is the
///   whole of requirement one: **culling on does not move the picture**, and
///   [`per_cluster_culling_rejects_the_clusters_a_camera_hides`] is the other
///   half, because a cull that rejected nothing at all would pass this too.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_multi_cluster_mesh_draws_the_same_frame_through_both_geometry_paths() {
    let mesh = render_open_box(Features::GPU_DRIVEN | Features::MESH_SHADER);
    let indirect = render_open_box(Features::GPU_DRIVEN);
    let (mesh_path, through_mesh_stage) = (mesh.path, mesh.image);
    let (indirect_path, through_index_buffer) = (indirect.path, indirect.image);
    assert_eq!(
        mesh_path,
        crcbl_hal::GeometryPath::MeshShader,
        "this test needs a device that selects the mesh path; radv reports \
         VK_EXT_mesh_shader and `docs/backlog.md` is where a driver that does not belongs"
    );
    assert_ne!(
        indirect_path, mesh_path,
        "both arms built the same path, so this is a self-comparison rather than a \
         comparison of two paths"
    );
    assert!(
        !mesh.culls_clusters,
        "asking for MESH_SHADER without TASK_SHADER must build the un-amplified \
         mesh stage — this arm is the device with mesh shaders and no task shaders, \
         and it has to keep drawing"
    );

    // Anti-vacuity: two blank frames match perfectly, so the frame has to hold
    // something before the comparison means anything. Measured here — a cleared
    // frame is 1 distinct colour and the cube alone is 4 — so anything at or
    // below that is a frame the open box did not reach.
    let colours = through_mesh_stage.distinct_colors(64);
    assert!(
        colours > 4,
        "the frame is missing geometry ({colours} distinct colours), so comparing it \
         against another frame like it proves nothing"
    );

    assert_eq!(
        through_mesh_stage.pixels(),
        through_index_buffer.pixels(),
        "{mesh_path:?} and {indirect_path:?} draw the open box differently"
    );

    // The third arm: the same frame with §3.5's per-cluster cull in front of
    // it. Two of the box's five clusters are rejected from this camera and the
    // picture must not move by a single texel.
    let culled =
        render_open_box(Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER);
    if culled.culls_clusters {
        assert_eq!(
            culled.path,
            crcbl_hal::GeometryPath::MeshShader,
            "the amplification stage belongs in front of a mesh stage"
        );
        assert_eq!(
            culled.image.pixels(),
            through_mesh_stage.pixels(),
            "per-cluster culling moved the picture; the clusters it rejects from \
             this camera are walls seen from behind, which contribute no pixels"
        );
    } else {
        eprintln!(
            "vk e2e: no TASK_SHADER on this device; the per-cluster cull did not run \
             and this arm compared nothing"
        );
    }

    let comparison = crcbl_golden::Golden::new(migrated_golden("mesh_clusters"))
        .check(&through_mesh_stage)
        .expect("the reference is readable")
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    eprintln!(
        "vk e2e: golden multi-cluster mesh through the mesh-shader path — {}",
        comparison.summary()
    );
}

/// The camera every face of the open box is front-on from: above it, and
/// between its walls.
///
/// Its faces point **inward**, so this is the only kind of viewpoint from which
/// a correct cull rejects nothing — which is exactly the case
/// [`per_cluster_culling_rejects_the_clusters_a_camera_hides`] needs, and the
/// one an over-eager cone test fails. Far enough up that the cube two units
/// away is still inside the frame, so the baseline below is one rather than
/// zero.
fn inside_the_open_box_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: OPEN_BOX_AT + glam::Vec3::new(0.2, 3.2, 0.3),
        target: OPEN_BOX_AT,
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::default(),
    }
}

/// A camera **inside** the box looking level at its `-Z` wall, where the
/// rejections are the frustum's alone.
///
/// Every face still points at the eye, so the cone rejects nothing here. What
/// falls out is the floor — whose bounding sphere sits directly beneath a
/// camera looking horizontally, entirely below the bottom plane — and the `+Z`
/// wall, whose sphere is entirely behind the eye. The cube is ninety degrees
/// off the view direction and outside the frame altogether, which is what makes
/// this camera's baseline zero.
fn behind_the_open_box_wall_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: OPEN_BOX_AT + glam::Vec3::new(0.0, 0.4, -0.3),
        target: OPEN_BOX_AT + glam::Vec3::new(0.0, 0.4, -1.0),
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::default(),
    }
}

/// A camera above the box and just outside two of its walls, where the
/// **radius term** of the cone rule is what keeps those two.
///
/// The only place that term reaches a number this suite can read. On this mesh
/// every cluster is a flat face at `cone_cutoff == 1.0`, so the conservative
/// rule and the radius-free one differ exactly where the camera has crossed a
/// face's own plane — here the `+X` and `+Z` walls, whose centre-to-camera dot
/// products land between zero and the bounding radius.
/// `crcbl_render::cull`'s `the_radius_term_keeps_two_clusters_the_point_form_drops`
/// works out both counts on the same camera; a shader that dropped `+ radius`
/// reports three of five here instead of five.
fn across_the_open_box_wall_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: OPEN_BOX_AT + glam::Vec3::new(0.8, 3.0, 0.8),
        target: OPEN_BOX_AT,
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::default(),
    }
}

/// Renders one frame and reads back how many clusters §3.5's amplification
/// stage kept.
///
/// **The counter is `DrawGen::visible_count`'s cluster word**, which is topic 03
/// §3.6's one permitted readback — the same buffer the instance survivor count
/// lives in, so this adds no second copy to the frame. It is zeroed by the
/// clearing dispatch at the top of every frame, so the number is this frame's
/// and not an accumulation.
///
/// The copy is its own submission after [`render_mesh`]'s: the graph leaves the
/// buffer in [`ResourceState::ShaderRead`], which is where the next frame on
/// that slot expects it, so this moves it out and puts it straight back.
fn surviving_clusters(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    camera: &crcbl_render::Camera,
) -> u32 {
    let _ = render_mesh(headless, renderer, pool, camera);
    read_stats_word(
        headless,
        renderer,
        crcbl_shaders::cull::CLUSTER_SURVIVOR_WORD,
    )
}

/// One word of the frame's culling statistics, copied back after the frame that
/// wrote it.
///
/// Topic 03 §3.6's ring is one buffer with a counter per producer in it — the
/// cull pass's survivors, the amplification stage's, and `light_cluster.slang`'s
/// refused assignments — so reading any of them is the same copy with a
/// different offset. One helper rather than one per counter, because the
/// barriers around it are the part worth getting right once.
pub(crate) fn read_stats_word(
    headless: &Headless,
    renderer: &crcbl_render::ForwardRenderer,
    word_index: u32,
) -> u32 {
    let device = headless.device.as_ref();
    let stats = renderer.draws().visible_count(renderer.frame());
    let word = u64::from(word_index) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("culling statistics readback"),
            size: 4,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("cluster survivor copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl_hal::BufferBarrier {
            buffer: stats,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: stats,
        src_offset: word,
        dst: staging,
        dst_offset: 0,
        size: 4,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut bytes = [POISON; 4];
    headless.readback(staging, 4, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    u32::from_le_bytes(bytes)
}

/// **§3.5's per-cluster cull, counted** — the check a golden image cannot make.
///
/// Culling that rejects nothing passes every golden, and so does culling that
/// rejects exactly the clusters contributing no pixels, which is what this
/// scene's rejections *are*. Pixels therefore cannot tell a working cull from a
/// no-op, and the surviving-cluster count is what can.
///
/// Every number below is asserted twice over: once with the open box in the
/// scene and once without it, so each is attributed to the box rather than to
/// the cube that shares the frame. The box is five clusters, one per face.
///
/// | camera | without the box | with it | what rejected the difference |
/// | --- | --- | --- | --- |
/// | above and inside the walls | 1 | 6 | **nothing** — all five survive |
/// | the golden's, out beyond `+X` and `+Z` | 1 | 4 | the **cone**, two walls |
/// | inside, level at the `-Z` wall | 0 | 3 | the **frustum**, two clusters |
/// | above and just outside two walls | 1 | 6 | nothing — the **radius term** |
///
/// The second row is the cull doing work; the first is what catches an
/// over-eager cone test, and it is the assertion most easily left out. The last
/// is the same shape one step finer: it is the only count in this suite that
/// moves if the cone rule loses its radius term, which is the correction this
/// slice made. Which clusters go in each case is worked out in
/// `crcbl_render::cull` — `the_open_box_loses_the_clusters_each_of_the_three_test_cameras_hides`
/// and `the_radius_term_keeps_two_clusters_the_point_form_drops` — on the same
/// geometry and the same aspect, because a total on its own does not say which.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn per_cluster_culling_rejects_the_clusters_a_camera_hides() {
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    if !headless.device.caps().supports(Features::TASK_SHADER) {
        eprintln!(
            "vk e2e: no TASK_SHADER on this device; per-cluster culling cannot run. radv and \
             lavapipe both report it, and `docs/backlog.md` is where a driver that does not belongs"
        );
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds an amplification stage");
    place_cube(&mut renderer);
    assert!(
        renderer.culls_clusters(),
        "the renderer must have *built* the amplification stage, or every count \
         below is the un-amplified path's and says nothing about culling"
    );

    let at = glam::Mat4::from_translation(OPEN_BOX_AT);
    let faces = crcbl_shaders::mesh::OPEN_BOX_FACES.len() as u32;

    // Each row: the camera, the count with the box out of the scene, and the
    // count with it in. The first is what the cube contributes and is measured
    // rather than assumed.
    let mut counts = Vec::new();
    // The box goes in and out of the scene per camera, so its handle is held
    // rather than a setter's field. Removing before placing again is what keeps
    // it in one slot: a second insert would leave the first copy live and drawn.
    let mut open_box: Option<crcbl_render::InstanceHandle> = None;
    for (label, camera) in [
        ("above and inside the walls", inside_the_open_box_camera()),
        ("the golden's", two_mesh_camera()),
        ("inside, level at a wall", behind_the_open_box_wall_camera()),
        (
            "above and across two walls",
            across_the_open_box_wall_camera(),
        ),
    ] {
        if let Some(handle) = open_box.take() {
            renderer.remove_instance(handle);
        }
        let without = surviving_clusters(&headless, &mut renderer, &mut pool, &camera);
        open_box = Some(place(
            &mut renderer,
            crcbl_render::scene::DEMO_OPEN_BOX,
            crcbl_render::scene::DEMO_UNTINTED,
            at,
        ));
        let with = surviving_clusters(&headless, &mut renderer, &mut pool, &camera);
        eprintln!("vk e2e: clusters from {label}: {without} without the box, {with} with it");
        counts.push((without, with));
    }

    let [
        (inside_without, inside_with),
        (golden_without, golden_with),
        (wall_without, wall_with),
        (across_without, across_with),
    ] = counts[..]
    else {
        unreachable!("four cameras, four rows")
    };

    // **Nothing is rejected that should not be.** Every face of the box is
    // front-on from this camera and every one is inside the frame, so all five
    // survive — the assertion an over-eager cone test fails and a golden image
    // cannot make.
    assert_eq!(
        inside_without, 1,
        "the cube is one cluster and it is in frame"
    );
    assert_eq!(
        inside_with,
        inside_without + faces,
        "from inside the walls every cluster of the box is front-facing, so a \
         correct cull rejects none of them"
    );

    // **Clusters really are rejected.** Two of the five walls are seen from
    // behind here, and the frame this camera draws is the golden one — which is
    // how the picture stays identical while the count drops.
    assert_eq!(golden_without, 1, "the same cube, the golden's own camera");
    assert_eq!(
        golden_with, 4,
        "the +X and +Z walls face away from the golden's camera, so three of the \
         box's five clusters survive beside the cube's one"
    );
    assert!(
        golden_with < inside_with,
        "the two cameras must disagree, or the cull rejected nothing anywhere: \
         {golden_with} against {inside_with}"
    );

    // **And the frustum half rejects on its own.** Every face still points at
    // this eye, so nothing here is the cone's doing: the floor is under the
    // frame and the `+Z` wall is behind it.
    assert_eq!(
        wall_without, 0,
        "the cube is ninety degrees off this view direction and out of frame"
    );
    assert_eq!(
        wall_with, 3,
        "the floor's sphere is entirely below the bottom plane and the +Z wall's \
         entirely behind the eye, leaving three of five"
    );

    // **And the cone rule's radius term is load-bearing here.** This camera has
    // crossed the planes of the `+X` and `+Z` walls, so their centre-to-camera
    // dot products are positive but smaller than the bounding radius — kept by
    // the rule as documented, dropped by the radius-free form the doc used to
    // carry. It is the only count in this suite that the term moves.
    assert_eq!(across_without, 1, "the cube is in frame from up here too");
    assert_eq!(
        across_with,
        across_without + faces,
        "the conservative cone rule keeps all five; a shader that lost the \
         `+ radius` term reports {} instead",
        across_without + faces - 2
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// `crcbl::screenshot`'s trough — the open box scaled into a long narrow room
/// and lifted so its floor is the plane `y = 0` — slid `offset` units along its
/// own run.
///
/// The engine's own **non-rigid** instance, and the arrangement that found the
/// bug the test below guards. Its numbers are copied rather than referred to
/// because `crcbl-vk` cannot depend on `crcbl`; a trough of some other size
/// would not be evidence about the scene this came from.
fn trough(offset: f32) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(offset, 1.0, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(6.0, 2.0, 1.6))
}

/// The camera that scene is drawn with: straight down into the trough from just
/// above its walls. `+Z` up, because the view direction is `Y`.
fn trough_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 2.2, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: crcbl_render::Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// **A cluster's bounding sphere is scaled by its instance, and a scene-sized
/// instance is where that stops being free.**
///
/// [`per_cluster_culling_rejects_the_clusters_a_camera_hides`] places the box by
/// translation alone, so every count there is the same whether or not the cull
/// scales a radius. This one is the trough: the same five clusters under a
/// `6 x 2 x 1.6` scale, which multiplies each face's real extent by up to six
/// while its mesh-space radius stays `0.71`.
///
/// | trough | clusters from the box | what a mesh-space radius reported |
/// | --- | --- | --- |
/// | centred under the camera | 5 | 3 — both long walls lost |
/// | three units along its run | 5 | 1 — everything but one wall lost |
/// | forty units along its run | 0 | 0 |
///
/// The middle row is the defect `docs/backlog.md` recorded as "a large open box
/// offset laterally from the camera stops drawing entirely": the trough spans
/// `x = 0..6` there and the near half of its floor fills a good part of the
/// frame, while a sphere of radius `0.71` about a centre three units off-axis is
/// entirely outside the left plane. The instance cull above it keeps the
/// instance — [`crcbl_render::cull::visible_instances`] works on the mesh's
/// *box*, which the transform scales correctly — so the frame comes back as
/// clear colour with the counters reporting a draw, and this counter is the only
/// thing in the frame that can say why.
///
/// The last row is what stops the first two being satisfied by a cull that
/// rejects nothing: forty units out the trough is gone, radius and all. Which
/// clusters go in each case is worked out in `crcbl_render::cull` —
/// `a_scaled_trough_slid_across_the_camera_keeps_the_floor_it_still_shows` and
/// `a_scaled_instances_cluster_sphere_still_contains_its_geometry` — where the
/// answer is arithmetic and the geometry's own vertices say what is on screen.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_scaled_instance_keeps_the_clusters_its_own_size_puts_on_screen() {
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    if !headless.device.caps().supports(Features::TASK_SHADER) {
        eprintln!(
            "vk e2e: no TASK_SHADER on this device; per-cluster culling cannot run. radv and \
             lavapipe both report it, and `docs/backlog.md` is where a driver that does not belongs"
        );
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds an amplification stage");
    place_cube(&mut renderer);
    assert!(
        renderer.culls_clusters(),
        "the renderer must have *built* the amplification stage, or every count \
         below is the un-amplified path's and says nothing about culling"
    );

    let camera = trough_camera();
    let faces = crcbl_shaders::mesh::OPEN_BOX_FACES.len() as u32;
    // The cube alone: nothing else has been placed yet.
    let without = surviving_clusters(&headless, &mut renderer, &mut pool, &camera);
    assert_eq!(
        without, 1,
        "the cube is one cluster and it is under this camera, which is what every \
         count below is measured against"
    );

    // One slot for the trough, **moved** between measurements rather than
    // inserted again: a second insert would leave the first copy live and drawn,
    // and every count below would be of two troughs.
    let trough_at = |offset: f32| crcbl_render::InstanceDesc {
        mesh: crcbl_render::scene::DEMO_OPEN_BOX,
        material: crcbl_render::scene::DEMO_UNTINTED,
        transform: trough(offset),
    };
    let open_box = renderer
        .add_instance(&trough_at(0.0))
        .expect("an instance pool of thousands has room for the trough");
    for offset in [0.0f32, 3.0] {
        renderer.set_instance(open_box, &trough_at(offset));
        let with = surviving_clusters(&headless, &mut renderer, &mut pool, &camera);
        eprintln!("vk e2e: clusters with the trough {offset} units along its run: {with}");
        assert_eq!(
            with,
            without + faces,
            "a trough this size keeps every cluster from straight above it; a cull \
             carrying the mesh-space radius across reports fewer, and the missing \
             ones are the floor and the walls the frame is made of"
        );
    }

    renderer.set_instance(open_box, &trough_at(40.0));
    let away = surviving_clusters(&headless, &mut renderer, &mut pool, &camera);
    assert_eq!(
        away, without,
        "forty units out the whole trough is outside the frustum, radius and all — \
         without this the counts above are satisfied by a cull that rejects nothing"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// What one frame's draw-argument pass produced, as far as the mesh path's
/// dispatch is concerned.
struct GeneratedDispatch {
    /// The per-bucket mesh-dispatch arguments — what
    /// `CommandEncoder::draw_mesh_tasks_indirect` reads.
    mesh_args: Vec<crcbl_shaders::draw_gen::MeshTasksArgs>,
    /// The per-bucket indexed-draw arguments, for
    /// `DrawIndexedArgs::instance_count`: the same survivor count reached
    /// through the other half of the pass.
    args: Vec<crcbl_shaders::draw_gen::DrawIndexedArgs>,
    /// Surviving instances across every bucket, out of the culling statistics.
    survivors: u32,
}

/// Renders one frame and reads back everything the draw-argument pass wrote
/// about the dispatch.
///
/// Three device-local buffers into one staging copy, on
/// [`surviving_clusters`]' terms exactly: its own submission after
/// [`render_mesh`]'s, each buffer taken out of the state the graph's imports
/// leave it in and put straight back.
fn generated_dispatch(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    camera: &crcbl_render::Camera,
) -> GeneratedDispatch {
    use crcbl_shaders::draw_gen::{DRAW_ARGS_SIZE, DrawIndexedArgs, MESH_ARGS_SIZE, MeshTasksArgs};

    let _ = render_mesh(headless, renderer, pool, camera);

    let device = headless.device.as_ref();
    let draws = renderer.draws();
    let frame = renderer.frame();
    let buckets = draws.bucket_count();
    let mesh_bytes = u64::from(buckets) * MESH_ARGS_SIZE as u64;
    let args_bytes = u64::from(buckets) * DRAW_ARGS_SIZE as u64;
    let total = mesh_bytes + args_bytes + 4;

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("mesh dispatch readback"),
            size: total,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    // Each source with the state this slot's imports declare it rests in.
    let sources = [
        (
            draws.mesh_args(frame),
            ResourceState::IndirectArgument,
            // **Not zero.** The extents share a buffer with the one draw count
            // per bucket that precedes them, so the accessor that names them is
            // what says where they start — reading at zero copies the counts
            // back and decodes them as workgroup extents.
            draws.mesh_args_offset(0),
            mesh_bytes,
            0u64,
        ),
        (
            draws.args(frame),
            ResourceState::IndirectArgument,
            0,
            args_bytes,
            mesh_bytes,
        ),
        (
            draws.visible_count(frame),
            ResourceState::ShaderRead,
            u64::from(crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD) * 4,
            4,
            mesh_bytes + args_bytes,
        ),
    ];

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh dispatch copy"),
        queue: headless.queue,
    });
    let around = |into_transfer: bool| -> Vec<crcbl_hal::BufferBarrier> {
        sources
            .iter()
            .map(|(buffer, state, ..)| {
                let (from, to) = if into_transfer {
                    (*state, ResourceState::TransferSrc)
                } else {
                    (ResourceState::TransferSrc, *state)
                };
                crcbl_hal::BufferBarrier {
                    buffer: *buffer,
                    from,
                    to,
                    queue_transfer: None,
                }
            })
            .collect()
    };
    let out = around(true);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    for (buffer, _, src_offset, size, dst_offset) in sources {
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: buffer,
            src_offset,
            dst: staging,
            dst_offset,
            size,
        });
    }
    let back = around(false);
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut bytes = poisoned(total as usize);
    headless.readback(staging, total, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let at = |offset: usize, len: usize| -> Vec<u8> { bytes[offset..offset + len].to_vec() };
    let mesh_args = (0..buckets as usize)
        .map(|bucket| {
            let block = at(bucket * MESH_ARGS_SIZE, MESH_ARGS_SIZE);
            MeshTasksArgs::from_bytes(&block.try_into().expect("MESH_ARGS_SIZE bytes"))
        })
        .collect();
    let args = (0..buckets as usize)
        .map(|bucket| {
            let block = at(
                mesh_bytes as usize + bucket * DRAW_ARGS_SIZE,
                DRAW_ARGS_SIZE,
            );
            DrawIndexedArgs::from_bytes(&block.try_into().expect("DRAW_ARGS_SIZE bytes"))
        })
        .collect();
    let survivors = u32::from_le_bytes(
        bytes[(mesh_bytes + args_bytes) as usize..]
            .try_into()
            .expect("four bytes"),
    );
    GeneratedDispatch {
        mesh_args,
        args,
        survivors,
    }
}

/// **The mesh path's dispatch extents are the count culling produced, not the
/// size of the instance pool** — the check a golden cannot make, and the whole
/// difference between culling that skips output and culling that skips work.
///
/// A dispatch sized on the CPU has to cover every slot the instance pool ever
/// handed out, because a removed instance leaves a hole and the live ones above
/// it stay in the array. The picture is the same either way: the stages read
/// the survivor count and the groups past it emit nothing. So pixels cannot
/// tell the two apart, and neither can the recorded command stream, which says
/// only which buffer the extents come from.
///
/// What can is the buffer's contents, and this is three claims about them:
///
/// * **Each bucket's y extent is that bucket's own survivor count**, equal to
///   the `instance_count` the same pass wrote into the indexed-draw arguments —
///   two words written by two atomic adds, which have to agree and are checked
///   rather than assumed.
/// * **They sum to the frame's surviving-instance count**, which is
///   `cull.slang`'s own counter and the one number here that neither half of
///   the draw-argument pass computed.
/// * **They move when the scene does, per bucket.** The box is taken out and
///   its bucket's extent goes to zero while the cube's stays one — a pool-sized
///   extent is one number for every bucket and cannot be both, and
///   `InstancePool::slot_count` never shrinks, so the pool is the same size in
///   both rounds.
///
/// The whole sequence runs twice, which is what pins the clearing pass over
/// these words too: an extent left accumulating across frames doubles on the
/// second lap through the ring.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_mesh_dispatch_extent_is_the_culled_instance_count() {
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    place_cube(&mut renderer);
    if renderer.geometry_path() != crcbl_hal::GeometryPath::MeshShader {
        eprintln!(
            "vk e2e: this device does not select the mesh path, so it records no indirect \
             mesh dispatch. radv and lavapipe both report VK_EXT_mesh_shader, and \
             `docs/backlog.md` is where a driver that does not belongs"
        );
        renderer.destroy(headless.device.as_ref());
        pool.destroy(headless.device.as_ref());
        headless.finish();
        return;
    }

    let at = glam::Mat4::from_translation(OPEN_BOX_AT);
    let camera = two_mesh_camera();
    let faces = crcbl_shaders::mesh::OPEN_BOX_FACES.len() as u32;

    for lap in 0..2 {
        // In and out of the scene per lap, through the slot the box has held
        // since the first one: `InstancePool::slot_count` never shrinks, so the
        // removal frees the slot and the next insert takes it back.
        let open_box = place(
            &mut renderer,
            crcbl_render::scene::DEMO_OPEN_BOX,
            crcbl_render::scene::DEMO_UNTINTED,
            at,
        );
        let with = generated_dispatch(&headless, &mut renderer, &mut pool, &camera);
        renderer.remove_instance(open_box);
        let without = generated_dispatch(&headless, &mut renderer, &mut pool, &camera);

        for (label, produced) in [("with the box", &with), ("without it", &without)] {
            let extents: Vec<u32> = produced
                .mesh_args
                .iter()
                .map(|args| args.group_count_y)
                .collect();
            eprintln!("vk e2e: lap {lap}, {label}: y extents {extents:?}");

            for (bucket, (mesh, args)) in produced.mesh_args.iter().zip(&produced.args).enumerate()
            {
                assert_eq!(
                    mesh.group_count_y, args.instance_count,
                    "lap {lap}, {label}: bucket {bucket}'s dispatch extent and its draw's \
                     instance count are the same survivor count counted twice, and they \
                     disagree"
                );
                assert_eq!(
                    mesh.group_count_z, 1,
                    "lap {lap}, {label}: bucket {bucket}'s z extent"
                );
            }
            assert_eq!(
                extents.iter().sum::<u32>(),
                produced.survivors,
                "lap {lap}, {label}: the extents must account for every instance the cull \
                 pass counted, and no others"
            );
        }

        // The x extents are each bucket's own mesh's cluster count, and two
        // residents' are not one — so a table that handed every bucket the same
        // number fails here. The dunes patch's is **every level of its DAG**,
        // because one bucket covers the whole hierarchy and the amplification
        // stage is what picks the cut out of it; that number is taken from the
        // committed artifact rather than written down, so it follows a re-cook.
        let dunes_clusters: u32 = crcbl_shaders::cluster_dag::dunes_dag()
            .levels
            .iter()
            .map(|level| u32::try_from(level.clusters.clusters.len()).expect("small"))
            .sum();
        let x: Vec<u32> = with
            .mesh_args
            .iter()
            .map(|args| args.group_count_x)
            .collect();
        assert_eq!(
            x,
            vec![1, 1, faces, dunes_clusters],
            "lap {lap}: the cube and the pyramid are one cluster each, the open box is \
             one per face, and the dunes patch is every cluster of every level"
        );

        // **The extent tracks the scene, per bucket.** The box's goes to zero
        // while the cube's stays one: a dispatch sized by the instance pool
        // would be the same number for both, in both rounds.
        // The open box's bucket, which the renderer builds third — no longer the
        // last one, now that the dunes patch is behind it. Named against its own
        // cluster count rather than against a position in the list, so a bucket
        // table reordered under this test fails here instead of measuring the
        // wrong mesh.
        let box_bucket = 2;
        assert_eq!(
            x[box_bucket], faces,
            "lap {lap}: bucket {box_bucket} is not the open box's"
        );
        assert_eq!(
            with.mesh_args[box_bucket].group_count_y, 1,
            "lap {lap}: the box is in the scene and in frame"
        );
        assert_eq!(
            without.mesh_args[box_bucket].group_count_y, 0,
            "lap {lap}: the box left the scene, so its bucket has nothing to dispatch"
        );
        assert_eq!(
            with.mesh_args[0].group_count_y, without.mesh_args[0].group_count_y,
            "lap {lap}: the cube did not move, so its extent must not have"
        );
        assert_eq!(
            with.mesh_args[0].group_count_y, 1,
            "lap {lap}: and the cube's extent is one instance, not the pool's slot count"
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Where the camera stands for the dunes patch: at its near edge, a little way
/// up, looking along the surface as it recedes.
///
/// `docs/plan/25-lod.md`'s shape for per-cluster selection, and the arrangement
/// `crcbl_shaders::cluster_dag`'s `a_receding_patch_draws_its_near_end_finer
/// _than_its_far_end` reads its histograms from: the patch is centred on the
/// origin in `x` and `z` with its height on `y`, so an eye at negative `z` and a
/// small `y` is a viewer standing at one end of a ground plane whose far edge is
/// tens of times further away than its near one. That ratio is the whole source
/// of the variation — the distance term is what makes one cut span levels.
fn dunes_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 4.0, -crcbl_shaders::dunes::DUNES_EXTENT - 2.0),
        target: glam::Vec3::new(0.0, 0.0, 0.0),
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::default(),
    }
}

/// The pixel budget the mixing assertion is read at.
///
/// Inside the band where the near end's groups and the far end's fall on
/// opposite sides of the descent's test from [`dunes_camera`]. Below it every
/// group expands and the patch is level 0 whole; far above it none does and the
/// patch is one coarse level. Both of those extremes are asserted below, which
/// is what shows the mixing assertion can fail.
const DUNES_MIXING_BUDGET: f32 = 32.0;

/// Reads back the cut this frame's amplification stage chose, as one entry per
/// cluster of the dunes DAG.
///
/// The buffer is `ForwardRenderer::cluster_selection`, which the stage writes
/// from instance slot zero and nothing in the frame reads. The copy is its own
/// submission after the frame's: the graph leaves the buffer in
/// `ResourceState::ShaderReadWrite`, which is where the next frame on that slot
/// expects it, so this moves it out and puts it straight back.
fn selected_clusters(
    headless: &Headless,
    renderer: &crcbl_render::ForwardRenderer,
) -> Vec<crcbl_shaders::cluster_dag::ClusterAt> {
    let selection = renderer
        .cluster_selection(renderer.frame())
        .expect("an amplification stage records its cut");
    read_cut(headless, renderer, selection)
}

/// The same, for the cut `cascade`'s shadow pass chose — the shadow LOD bias'
/// observable.
///
/// A buffer of its own rather than the camera's, because the colour pass is
/// recorded last and writes over that one; see
/// `ForwardRenderer::shadow_selection`.
fn shadow_selected_clusters(
    headless: &Headless,
    renderer: &crcbl_render::ForwardRenderer,
    cascade: usize,
) -> Vec<crcbl_shaders::cluster_dag::ClusterAt> {
    let selection = renderer
        .shadow_selection(renderer.frame(), cascade)
        .expect("an amplification stage records every cascade's cut");
    read_cut(headless, renderer, selection)
}

/// Copies one selection buffer out and decodes the dunes DAG's run of it.
fn read_cut(
    headless: &Headless,
    renderer: &crcbl_render::ForwardRenderer,
    selection: crcbl_hal::BufferHandle,
) -> Vec<crcbl_shaders::cluster_dag::ClusterAt> {
    let device = headless.device.as_ref();
    let range = renderer
        .cluster_range(crcbl_render::scene::DEMO_DUNES)
        .expect("the mesh path has a cluster pool");
    let bytes = u64::from(range.count) * 4;

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("cluster selection readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("cluster selection copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl_hal::BufferBarrier {
            buffer: selection,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderReadWrite, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderReadWrite);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: selection,
        src_offset: u64::from(range.base) * 4,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut words = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut words);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    // The pool holds the DAG's levels finest first and contiguously, so the
    // run splits back into levels by their own cluster counts.
    let dag = crcbl_shaders::cluster_dag::dunes_dag();
    let mut drawn = Vec::new();
    let mut at = 0usize;
    for (level, here) in dag.levels.iter().enumerate() {
        for cluster in 0..here.clusters.clusters.len() {
            let word = u32::from_le_bytes(words[at * 4..at * 4 + 4].try_into().expect("4"));
            assert!(
                word <= 1,
                "cluster {at} recorded {word}, which is not a flag"
            );
            if word == 1 {
                drawn.push(crcbl_shaders::cluster_dag::ClusterAt { level, cluster });
            }
            at += 1;
        }
    }
    assert_eq!(at * 4, words.len(), "the DAG and its pool run disagree");
    drawn
}

/// A camera high above the patch, far enough out that **every** group's sphere
/// is in front of the eye rather than around it.
///
/// The distinction matters and is not a detail: a group's sphere is grown to
/// contain every sphere below it, so the top levels' spheres bound essentially
/// the whole mesh — and an eye standing at the patch's own edge is *inside*
/// them. `group_is_expanded` answers infinity there, which is the conservative
/// and monotone answer, so from [`dunes_camera`] the deepest levels expand
/// whatever the budget is and no budget can select the root. From up here they
/// do not, which is what makes "draw the coarsest level" reachable at all.
fn dunes_distant_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 4.0 * crcbl_shaders::dunes::DUNES_EXTENT, 0.0),
        target: glam::Vec3::ZERO,
        // Looking straight down `-Y`, so `Y` cannot be the up axis.
        up: glam::Vec3::Z,
        projection: crcbl_render::Projection::default(),
    }
}

/// The host's copy of the state `docs/plan/25-lod.md`'s hysteresis makes the GPU
/// carry between frames.
///
/// **The oracle stopped being a function of one camera when hysteresis landed**,
/// and this is what replaces it: a frame's cut depends on every frame before it,
/// so the host rule has to be walked frame for frame beside the device rather
/// than evaluated once at the end. One of these is built per renderer, because
/// the state is the renderer's buffer and a fresh renderer starts from the zeroes
/// `DrawGen` wrote into it.
struct DunesHistory {
    dag: crcbl_shaders::cluster_dag::ClusterDag,
    /// Which groups were expanded after the last frame this walked, in
    /// `ClusterDag::level_groups` order.
    expanded: Vec<bool>,
}

impl DunesHistory {
    fn new() -> Self {
        let dag = crcbl_shaders::cluster_dag::dunes_dag();
        let expanded = vec![false; dag.group_count()];
        Self { dag, expanded }
    }

    /// Renders one frame at `camera` and advances the host state by the same
    /// frame.
    ///
    /// The three numbers come back out of the renderer rather than being
    /// re-derived here, exactly as they did before: the viewport and the field
    /// of view are the harness's and the hold budget is the renderer's constant,
    /// and a test that recomputed either would be comparing two derivations
    /// instead of two implementations of one rule.
    fn step(
        &mut self,
        headless: &Headless,
        renderer: &mut crcbl_render::ForwardRenderer,
        pool: &mut crcbl_render::TransientPool,
        camera: &crcbl_render::Camera,
    ) -> [f32; 3] {
        let _ = render_mesh(headless, renderer, pool, camera);
        let [pixels_per_unit, expand, hold] = renderer.lod_params();
        self.expanded = self.dag.expand(
            camera.eye.to_array(),
            pixels_per_unit,
            crcbl_shaders::cluster_select::LodBudgets { expand, hold },
            &self.expanded,
        );
        [pixels_per_unit, expand, hold]
    }

    /// The cut the host rule says that frame drew, at the per-cluster
    /// granularity.
    ///
    /// The uniform granularity's twin, `ClusterDag::uniform_level_from`, is
    /// `crates/crcbl/tests/mesh_e2e/lod.rs`'s: the uniform cut runs on every
    /// backend, so the test that holds it to the host rule went with it.
    fn cut(&self) -> Vec<crcbl_shaders::cluster_dag::ClusterAt> {
        self.dag.cut_from(&self.expanded)
    }
}

/// Renders the dunes patch once and returns the cut the GPU chose, the cut the
/// host rule says it should have chosen, and the numbers the frame selected
/// under.
fn dunes_cut(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    history: &mut DunesHistory,
    camera: &crcbl_render::Camera,
    budget: f32,
) -> (
    Vec<crcbl_shaders::cluster_dag::ClusterAt>,
    Vec<crcbl_shaders::cluster_dag::ClusterAt>,
    [f32; 3],
) {
    renderer.set_lod_error_budget(budget);
    let params = history.step(headless, renderer, pool, camera);
    (selected_clusters(headless, renderer), history.cut(), params)
}

/// **The GPU descends the DAG to the cut the host rule says**, and one cut of
/// one mesh draws its near end finer than its far end.
///
/// `docs/plan/25-lod.md`'s runtime selection, and the check a golden cannot
/// make: a frame whose every cluster came from one level is an entirely
/// plausible picture and matches any golden blessed from it. So this reads back
/// what the amplification stage actually chose and asserts three things about
/// it.
///
/// * **It is the host rule's own answer**, cluster for cluster.
///   `ClusterDag::cut` is the reference implementation the crack-free tests in
///   `crcbl_shaders::cluster_dag` run over the committed artifact, and it is
///   handed the very numbers the renderer put in the frame block rather than a
///   second derivation of them. That is what holds the shader's
///   `group_is_expanded` to `GroupCost::projected_error`: not a claim about
///   floating point, but the decision itself compared over every group of the
///   DAG.
/// * **The near third and the far third report different levels.** This is the
///   observable the whole slice exists to produce, and the one thing that
///   distinguishes per-cluster selection from a whole-mesh LOD.
/// * **And both extremes collapse it**, which is what shows the mixing
///   assertion can fail: under a budget nothing satisfies the descent expands
///   every group and draws level 0 alone, and under one everything satisfies it
///   expands none and draws the top level alone. Neither is a mixed cut, and the
///   histogram check below reds on both — so a shader whose descent ignored the
///   distance term would be caught rather than passing by drawing a uniform cut
///   nobody looked at.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_gpu_descends_the_dag_to_the_cut_the_host_rule_says() {
    use std::collections::BTreeMap;

    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    if !headless.device.caps().supports(Features::TASK_SHADER) {
        eprintln!(
            "vk e2e: no TASK_SHADER on this device; the DAG descent cannot run. radv and \
             lavapipe both report it, and `docs/backlog.md` is where a driver that does not belongs"
        );
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    place_cube(&mut renderer);
    assert!(
        renderer.culls_clusters(),
        "the descent runs in the amplification stage, and this device reports none"
    );
    // **At the origin, with no rotation.** The shader puts each group's sphere
    // through the instance transform and leaves the camera where it is, which is
    // the same statement as putting the camera through the inverse — and at the
    // identity the two are the same *bits*, which is what lets the comparison
    // below be an equality rather than a tolerance.
    assert!(
        renderer.selects_levels(),
        "a device with an amplification stage can choose a level of the patch"
    );
    place_dunes(&mut renderer);

    let dag = crcbl_shaders::cluster_dag::dunes_dag();
    let mut history = DunesHistory::new();

    // --- the cut, against the host rule ---
    let (drawn, want, [pixels_per_unit, budget, hold]) = dunes_cut(
        &headless,
        &mut renderer,
        &mut pool,
        &mut history,
        &dunes_camera(),
        DUNES_MIXING_BUDGET,
    );
    assert_eq!(budget, DUNES_MIXING_BUDGET, "the frame took the budget set");
    assert!(
        hold < budget,
        "the renderer selected under one threshold ({budget} and {hold}), so this frame \
         says nothing about hysteresis"
    );
    assert!(
        pixels_per_unit > 0.0,
        "a viewport of no height selects nothing"
    );
    eprintln!(
        "vk e2e: dunes cut — {} cluster(s) at {pixels_per_unit} px/unit, budgets \
         {budget}/{hold}",
        drawn.len()
    );
    assert_eq!(
        drawn, want,
        "the amplification stage chose a different cut from the host rule"
    );

    // --- the observable: near finer than far ---
    let mut near: BTreeMap<usize, usize> = BTreeMap::new();
    let mut far: BTreeMap<usize, usize> = BTreeMap::new();
    for &crcbl_shaders::cluster_dag::ClusterAt { level, cluster } in &drawn {
        let depth = dag.levels[level].clusters.clusters[cluster].bounds.center[2];
        if depth < -crcbl_shaders::dunes::DUNES_EXTENT / 3.0 {
            *near.entry(level).or_default() += 1;
        } else if depth > crcbl_shaders::dunes::DUNES_EXTENT / 3.0 {
            *far.entry(level).or_default() += 1;
        }
    }
    eprintln!("vk e2e: dunes near {near:?}, far {far:?}");
    assert_mixes_levels(&near, &far);

    // --- and the cut the GPU chose is a crack-free cover of the surface ---
    //
    // **Asserted on this cut, not inferred from two facts about it.** That the
    // read-back cut equals the host rule's, and that the host rule's cuts are
    // crack-free over a sweep, would together imply this — but the sweep runs at
    // its own pixels-per-unit and does not contain the pair a real frame selects
    // under, and crack-freedom is the property the whole DAG exists for. So it
    // is checked here, at the camera and budget the engine actually shipped.
    let interfaces = dag
        .check_cover(&drawn)
        .unwrap_or_else(|fault| panic!("the GPU's cut is not a cover of the surface: {fault}"));
    eprintln!("vk e2e: dunes cut — {interfaces} interface edge(s)");
    assert!(
        interfaces > 20,
        "only {interfaces} edge(s) have one level on one side and another on the other,          which is too few to be the seam between the two ends"
    );

    // The same check, shown to refuse — because one that passed whatever it was
    // handed would be worse than the inference it replaces. Both tears start
    // from the cut the GPU actually produced: drop one of its clusters and the
    // surface has a hole where that cluster was, draw every one of them twice
    // and each interior edge has four faces on it.
    let mut holed = drawn.clone();
    let dropped = holed.remove(drawn.len() / 2);
    let fault = dag
        .check_cover(&holed)
        .expect_err("a GPU cut missing a cluster has a hole in it");
    eprintln!("vk e2e: dunes cut without {dropped:?} — {fault}");
    assert!(
        fault.what.contains("hole"),
        "dropping {dropped:?} from the GPU's cut was reported as {fault}"
    );
    let doubled: Vec<crcbl_shaders::cluster_dag::ClusterAt> =
        drawn.iter().chain(&drawn).copied().collect();
    let fault = dag
        .check_cover(&doubled)
        .expect_err("a GPU cut drawn twice covers the surface twice");
    eprintln!("vk e2e: dunes cut drawn twice — {fault}");
    assert!(
        fault.what.contains("overlapping"),
        "drawing the GPU's cut twice was reported as {fault}"
    );

    // --- and both extremes collapse it, so the assertion above can fail ---
    //
    // The leaves are forced by the budget alone and the root needs the distant
    // camera as well, for the reason `dunes_distant_camera` carries.
    for (camera, budget, name) in [
        (
            dunes_camera(),
            f32::NEG_INFINITY,
            "a budget every group exceeds",
        ),
        (
            dunes_distant_camera(),
            f32::INFINITY,
            "a budget no group exceeds, from far enough out",
        ),
    ] {
        let (collapsed, collapsed_want, _) = dunes_cut(
            &headless,
            &mut renderer,
            &mut pool,
            &mut history,
            &camera,
            budget,
        );
        assert_eq!(
            collapsed, collapsed_want,
            "{name}: the GPU and the host rule disagree about the collapsed cut"
        );
        let levels: std::collections::BTreeSet<usize> =
            collapsed.iter().map(|at| at.level).collect();
        let count = collapsed.len();
        eprintln!("vk e2e: dunes under {name} — levels {levels:?}, {count} cluster(s)");
        assert_eq!(
            levels.len(),
            1,
            "{name} must draw one level and drew {levels:?}"
        );
        assert_ne!(
            drawn, collapsed,
            "{name} chose the same cut as the mixing budget, so the budget decides nothing"
        );
        // The same split the mixing assertion reads, which must now refuse —
        // and it is run rather than reasoned about, because a check nobody has
        // seen fail is a check nobody has tested.
        let mut collapsed_near: BTreeMap<usize, usize> = BTreeMap::new();
        let mut collapsed_far: BTreeMap<usize, usize> = BTreeMap::new();
        for &crcbl_shaders::cluster_dag::ClusterAt { level, cluster } in &collapsed {
            let depth = dag.levels[level].clusters.clusters[cluster].bounds.center[2];
            if depth < -crcbl_shaders::dunes::DUNES_EXTENT / 3.0 {
                *collapsed_near.entry(level).or_default() += 1;
            } else if depth > crcbl_shaders::dunes::DUNES_EXTENT / 3.0 {
                *collapsed_far.entry(level).or_default() += 1;
            }
        }
        // The default hook would print this deliberate failure as if something
        // had gone wrong, in the middle of a passing run.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let refused =
            std::panic::catch_unwind(|| assert_mixes_levels(&collapsed_near, &collapsed_far))
                .is_err();
        std::panic::set_hook(hook);
        assert!(
            refused,
            "{name} draws one level, so the mixing assertion has to reject it — \
             a check that cannot fail is not a check"
        );
    }

    renderer.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The shadow cascades descend to a coarser cut than the colour pass**, from
/// one frame, read out of both passes' own buffers.
///
/// `docs/plan/25-lod.md`'s "**Shadow LOD bias**: shadow-pass culling (topic 18,
/// same shader) selects +1/+2 coarser levels — casters are cheap where it never
/// shows." The bias is `crcbl_render::SHADOW_LOD_BIAS`, applied to both budgets
/// and to nothing else: the cascades select from the **camera's** eye at the
/// camera's pixels per unit, so the only difference between the two cuts is the
/// factor, and that is what makes "coarser by this much" a statement rather than
/// "different".
///
/// Four things are asserted, and the fourth is what makes the first three mean
/// anything:
///
/// * **The parameters are the camera's, scaled.** Bit-exact, because they are
///   one multiplication apart and a tolerance would hide the eye changing too.
/// * **Both passes' cuts are the host rule's own**, cluster for cluster, from
///   histories walked frame by frame — the same comparison
///   [`the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`] makes for the
///   camera, extended to a pass that had no observable at all before this.
///
///   **This is also what pins the eye**, and it is the only thing that can: the
///   shadow parameters carry the two budgets and the scale but not the point
///   they are projected from. The oracle is evaluated at the *camera's* eye, so
///   a shadow pass still selecting from the sun — a point hundreds of units away
///   along the light — would project every group differently and fail here. And
///   every cascade is compared against the one oracle, which the old rule could
///   not have satisfied either: it put the eye at `camera.eye + direction *
///   cascade_far`, so two cascades asked two different questions of one caster.
/// * **The shadow cut is a crack-free cover**, by `ClusterDag::check_cover` on
///   the read-back cut. The bias is a uniform scaling of both budgets, so the
///   monotonicity that makes a cut a cover survives it — and this is that
///   argument run rather than asserted.
/// * **And the shadow cut is strictly smaller and strictly coarser**, with the
///   levels printed. Setting `crcbl_render::SHADOW_LOD_BIAS` to one makes the two
///   passes one rule and reds both of those, which is how they were shown to be
///   checks rather than decoration.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_shadow_cascades_select_coarser_than_the_camera() {
    use crcbl_shaders::cluster_select::LodBudgets;

    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    if !headless.device.caps().supports(Features::TASK_SHADER) {
        eprintln!(
            "vk e2e: no TASK_SHADER on this device; the DAG descent cannot run, so neither \
             pass selects anything to be biased"
        );
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    place_cube(&mut renderer);
    assert!(
        renderer.selects_levels(),
        "a device with an amplification stage can choose a level of the patch"
    );
    place_dunes(&mut renderer);
    renderer.set_lod_error_budget(DUNES_MIXING_BUDGET);

    let dag = crcbl_shaders::cluster_dag::dunes_dag();
    // Two histories, because there are two: each pass carries its own hysteresis
    // state, so each has to be walked frame for frame beside the device rather
    // than evaluated from this frame's camera alone.
    let mut camera_history = DunesHistory::new();
    let mut shadow_history = DunesHistory::new();

    let camera = dunes_camera();
    let [pixels_per_unit, expand, hold] =
        camera_history.step(&headless, &mut renderer, &mut pool, &camera);
    let [shadow_scale, shadow_expand, shadow_hold] = renderer.shadow_lod_params();

    // --- the parameters: the camera's numbers, both budgets scaled ---
    assert_eq!(
        shadow_scale.to_bits(),
        pixels_per_unit.to_bits(),
        "the cascades selected at {shadow_scale} pixels per unit and the camera at \
         {pixels_per_unit}, so they are not projecting through one lens and the cuts \
         below differ for a second reason"
    );
    assert_eq!(
        shadow_expand.to_bits(),
        (expand * crcbl_render::SHADOW_LOD_BIAS).to_bits(),
        "the cascades' expand budget is {shadow_expand}, not the camera's {expand} times \
         {}",
        crcbl_render::SHADOW_LOD_BIAS
    );
    assert_eq!(
        shadow_hold.to_bits(),
        (hold * crcbl_render::SHADOW_LOD_BIAS).to_bits(),
        "the cascades' hold budget is {shadow_hold}, not the camera's {hold} times {}",
        crcbl_render::SHADOW_LOD_BIAS
    );
    // And the scaling actually raised them, at the budget this frame selected
    // under. A bias of one satisfies every equality above and leaves the two
    // passes as one rule, which is what the rest of this test would then be
    // measuring.
    assert!(
        shadow_expand > expand && shadow_hold > hold,
        "the cascades selected under {shadow_expand}/{shadow_hold} against the camera's \
         {expand}/{hold}, so the bias of {} bought no headroom",
        crcbl_render::SHADOW_LOD_BIAS
    );
    // The shadow history's own frame, under the budgets the renderer just
    // reported rather than under this test's own multiplication.
    shadow_history.expanded = dag.expand(
        camera.eye.to_array(),
        shadow_scale,
        LodBudgets {
            expand: shadow_expand,
            hold: shadow_hold,
        },
        &shadow_history.expanded,
    );

    // --- both cuts, each out of its own pass's buffer ---
    let drawn = selected_clusters(&headless, &renderer);
    assert_eq!(
        drawn,
        camera_history.cut(),
        "the colour pass chose a different cut from the host rule"
    );
    let want_shadow = shadow_history.cut();
    for cascade in 0..crcbl_render::shadow::CASCADES {
        let shadow = shadow_selected_clusters(&headless, &renderer, cascade);
        assert_eq!(
            shadow, want_shadow,
            "cascade {cascade} chose a different cut from the host rule under the biased \
             budgets"
        );
    }
    let shadow = want_shadow;

    // --- coarser, with the numbers ---
    //
    // **Not "the finest level moved"**, and that is the point of a per-cluster
    // cut: both passes still reach level 0 at the patch's near edge, because a
    // group four times over the budget is a group four times over it under
    // either. What the bias moves is *how much* of the surface descends, and the
    // three statements below are that, from the coarsest measure to the finest.
    let histogram = |cut: &[crcbl_shaders::cluster_dag::ClusterAt]| {
        let mut levels = vec![0usize; dag.levels.len()];
        for at in cut {
            levels[at.level] += 1;
        }
        levels
    };
    let (camera_levels, shadow_levels) = (histogram(&drawn), histogram(&shadow));
    eprintln!(
        "vk e2e: shadow bias — {pixels_per_unit} px/unit; camera budgets {expand}/{hold} \
         drew {} cluster(s) per level {camera_levels:?}; shadow budgets \
         {shadow_expand}/{shadow_hold} drew {} cluster(s) per level {shadow_levels:?}",
        drawn.len(),
        shadow.len(),
    );

    // **The exact statement: the shadow expands a subset of what the camera
    // does.** One eye, one scale and budgets `k > 1` times as large, from
    // histories that both started at all-zero — so a group over `k * expand` is
    // over `expand`, a group held above `k * hold` was held above `hold`, and the
    // induction carries frame to frame. That makes the shadow cut at or above
    // the camera's *everywhere*, which is what "coarser" has to mean for a cut
    // that spans levels. Asserted on the oracles, which the equalities above
    // just pinned to the device's own two buffers.
    let mut only_camera = 0usize;
    for (group, (&shadow_expanded, &camera_expanded)) in shadow_history
        .expanded
        .iter()
        .zip(&camera_history.expanded)
        .enumerate()
    {
        assert!(
            !shadow_expanded || camera_expanded,
            "group {group} expanded for the shadow pass and not for the camera, so the \
             biased cut descends somewhere the camera's does not"
        );
        only_camera += usize::from(camera_expanded && !shadow_expanded);
    }
    assert!(
        only_camera > 0,
        "every group the camera expanded the shadow pass expanded too, so the two cuts \
         are one cut and the subset above is satisfied by equality"
    );
    eprintln!(
        "vk e2e: shadow bias — {only_camera} of the {} group(s) the camera expanded stayed \
         collapsed for the shadow pass",
        camera_history.expanded.iter().filter(|at| **at).count()
    );

    // The same fact counted two more ways, because a subset of expansions is
    // what produces both and neither is readable off the other.
    assert!(
        shadow.len() < drawn.len(),
        "the shadow pass drew {} cluster(s) against the camera's {}, so a coarser cut is \
         not a cheaper one",
        shadow.len(),
        drawn.len()
    );
    let mut camera_below = 0usize;
    let mut shadow_below = 0usize;
    for level in 0..dag.levels.len() {
        camera_below += camera_levels[level];
        shadow_below += shadow_levels[level];
        assert!(
            shadow_below <= camera_below,
            "through level {level} the shadow pass drew {shadow_below} cluster(s) against \
             the camera's {camera_below}, so its cut is finer somewhere rather than \
             coarser everywhere"
        );
    }

    // --- and it is still a cover ---
    //
    // The property the whole DAG exists for, at the budgets the shadow pass
    // actually shipped rather than at a sweep's. A bias applied per group or per
    // cascade would let a child expand under a parent that had not, and this is
    // where that would land.
    let interfaces = dag.check_cover(&shadow).unwrap_or_else(|fault| {
        panic!("the shadow pass's cut is not a cover of the surface: {fault}")
    });
    eprintln!("vk e2e: shadow bias — shadow cut has {interfaces} interface edge(s)");

    renderer.destroy(headless.device.as_ref());
    headless.finish();
}

/// Where the camera stands to make a uniform cut pick each of three levels.
///
/// **Distance along `-z` and nothing else**: the same eye height and the same
/// target as [`dunes_camera`], so the only thing that changes between them is
/// the term the metric divides by. Positions rather than levels, because which
/// level each produces is what the test reads back — writing the levels here and
/// the eyes there would be two halves of one claim with nothing holding them
/// together.
///
/// The three are far apart on purpose: a group's sphere is grown to contain
/// everything below it, so the levels are separated by hundreds of units rather
/// than by tens, and an eye a little further back than another selects the same
/// level. `crcbl_shaders::cluster_dag`'s
/// `the_uniform_level_is_the_finest_level_the_per_cluster_cut_draws` sweeps the
/// same rule host-side without needing a camera at all.
const DUNES_RECEDING_CAMERAS: [f32; 3] = [2.0, 200.0, 1000.0];

/// A camera `back` units past the dunes patch's near edge, looking at it.
fn dunes_camera_back(back: f32) -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 4.0, -crcbl_shaders::dunes::DUNES_EXTENT - back),
        ..dunes_camera()
    }
}

/// The dunes level this frame's uniform cut selected, read out of the bucket
/// that actually drew.
///
/// **The indirect arguments are the observable**, and there is no second buffer
/// recording an intention: `draw_gen.slang` scatters the instance into the
/// bucket for the level it chose, so the bucket whose `instance_count` came out
/// non-zero *is* the level. A selection that picked a level and then failed to
/// route it would leave every bucket at zero and be reported here rather than
/// drawn as an empty frame.
fn selected_dunes_level(
    headless: &Headless,
    renderer: &crcbl_render::ForwardRenderer,
) -> Option<usize> {
    let device = headless.device.as_ref();
    let buckets = renderer.level_buckets(crcbl_render::scene::DEMO_DUNES);
    assert!(
        !buckets.is_empty(),
        "this device selects per cluster, so no bucket is a level"
    );
    let args = renderer.draw_args(renderer.frame());
    let stride = crcbl_shaders::draw_gen::DRAW_ARGS_SIZE as u64;
    let bytes = stride * u64::from(buckets[buckets.len() - 1] + 1);

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("draw args readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("draw args copy"),
        queue: headless.queue,
    });
    // The graph leaves the argument buffer in `IndirectArgument`, which is where
    // the next frame on this slot expects it — so this moves it out and puts it
    // straight back, exactly as `selected_clusters` does for the cut buffer.
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl_hal::BufferBarrier {
            buffer: args,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::IndirectArgument, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::IndirectArgument);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: args,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut words = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut words);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let mut drew: Vec<usize> = Vec::new();
    for (level, &bucket) in buckets.iter().enumerate() {
        let at = bucket as usize * crcbl_shaders::draw_gen::DRAW_ARGS_SIZE;
        let args = crcbl_shaders::draw_gen::DrawIndexedArgs::from_bytes(
            words[at..at + crcbl_shaders::draw_gen::DRAW_ARGS_SIZE]
                .try_into()
                .expect("one argument structure"),
        );
        if args.instance_count > 0 {
            assert!(
                args.index_count > 0,
                "level {level}'s bucket drew {} instance(s) of no indices at all",
                args.instance_count
            );
            drew.push(level);
        }
    }
    assert!(
        drew.len() <= 1,
        "one instance was scattered into levels {drew:?}, which is not a uniform cut"
    );
    drew.first().copied()
}

/// **The level the uniform cut picks is the finest level the per-cluster cut
/// draws** — the two granularities of `docs/plan/25-lod.md`'s runtime selection
/// agreeing modulo detail, measured rather than argued.
///
/// The plan gives the two indirect tails a uniform cut — "every cluster at one
/// depth, which is exactly a whole-mesh level — drawn as ordinary index ranges.
/// Same hierarchy, same error metric, one decision per instance instead of per
/// cluster." The coarse decision has to be the fine one's floor, and this is
/// that, on two real devices:
///
/// * The **mesh-shader arm** runs first, on a device opened with an
///   amplification stage, and reads back the per-cluster cut its descent chose —
///   checked against the host rule camera for camera, on
///   [`the_gpu_descends_the_dag_to_the_cut_the_host_rule_says`]' terms.
/// * The **uniform arm** opens a second device *without* the mesh-stage
///   features, which is the only way one machine reaches both paths, and reads
///   the level out of the bucket that drew.
/// * The two are compared per camera, and the levels are asserted not to be one
///   number three times over, or the comparison is vacuous.
///
/// # What moved
///
/// The uniform cut's own claims — that it gets coarser the further the patch is,
/// and that each level it picks is the host rule's — are
/// `crates/crcbl/tests/mesh_e2e/lod.rs`'s
/// `the_uniform_cut_gets_coarser_as_the_dunes_patch_recedes`, and they run on
/// every backend. They are not repeated here: what is left is the half that
/// needs a mesh stage on one side of the comparison.
///
/// The arms run one after the other rather than side by side so only one
/// instance is live at a time.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_two_geometry_paths_agree_about_how_fine_the_dunes_patch_is() {
    // --- the mesh arm: the finest level its per-cluster cut reaches ---
    let mut finest: Vec<Option<usize>> = Vec::new();
    {
        let headless = Headless::open_for_mesh_with(
            Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
        );
        if headless.device.caps().supports(Features::TASK_SHADER) {
            let mut pool = crcbl_render::TransientPool::new();
            let mut renderer = crcbl_render::ForwardRenderer::new(
                headless.device.as_ref(),
                headless.queue,
                headless.format,
            )
            .expect("the forward renderer builds");
            place_cube(&mut renderer);
            assert!(renderer.selects_levels());
            place_dunes(&mut renderer);
            // Its own history, because the state is this renderer's buffer and
            // the uniform arm below opens a second device with a second one.
            let mut history = DunesHistory::new();
            for back in DUNES_RECEDING_CAMERAS {
                let camera = dunes_camera_back(back);
                history.step(&headless, &mut renderer, &mut pool, &camera);
                let cut = selected_clusters(&headless, &renderer);
                assert_eq!(
                    cut,
                    history.cut(),
                    "from {back} units back the per-cluster cut is not the host rule's"
                );
                finest.push(cut.iter().map(|at| at.level).min());
            }
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
        } else {
            eprintln!(
                "vk e2e: no TASK_SHADER on this device, so there is no per-cluster cut to \
                 compare the uniform one against; the uniform arm below still runs"
            );
        }
        headless.finish();
    }

    // --- the uniform arm: a device with no mesh stage at all ---
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    assert_ne!(
        headless.device.caps().geometry_path(),
        crcbl_hal::GeometryPath::MeshShader,
        "the mesh-stage features were withheld and this device selected the mesh path \
         anyway, so both arms are the same arm"
    );
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    place_cube(&mut renderer);
    assert!(
        renderer.selects_levels(),
        "a device with no mesh stage takes the patch through a uniform cut"
    );
    place_dunes(&mut renderer);

    let mut history = DunesHistory::new();
    let mut compared = 0usize;
    let mut chosen: Vec<usize> = Vec::new();
    for (index, back) in DUNES_RECEDING_CAMERAS.into_iter().enumerate() {
        let camera = dunes_camera_back(back);
        let [pixels_per_unit, budget, hold] =
            history.step(&headless, &mut renderer, &mut pool, &camera);
        let level = selected_dunes_level(&headless, &renderer)
            .unwrap_or_else(|| panic!("no bucket drew the patch from {back} units back"));
        eprintln!(
            "vk e2e: dunes from {back} units back — level {level} at {pixels_per_unit} \
             px/unit, budgets {budget}/{hold}"
        );
        if let Some(Some(mesh_finest)) = finest.get(index) {
            assert_eq!(
                level, *mesh_finest,
                "from {back} units back the uniform cut takes level {level} and the \
                 per-cluster cut's finest level is {mesh_finest} — the two paths are \
                 selecting against different metrics"
            );
            compared += 1;
        }
        chosen.push(level);
    }

    // **The three cameras must not all reach the same level.** The comparison
    // above is satisfied by both granularities answering zero everywhere, which
    // is what a selection wired to a constant does; that the uniform cut gets
    // *coarser* with distance is
    // `crates/crcbl/tests/mesh_e2e/lod.rs`'s
    // `the_uniform_cut_gets_coarser_as_the_dunes_patch_recedes`, and this is
    // only the part of it this comparison needs to not be vacuous.
    assert!(
        chosen
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1,
        "the levels selected at {DUNES_RECEDING_CAMERAS:?} units back are {chosen:?}, \
         so the comparison above is one number against itself"
    );
    // The mesh arm either ran for every camera or, on a device with no
    // amplification stage, for none — and it says which above. A partial run is
    // neither, and would be a comparison quietly made for some cameras and not
    // others.
    assert!(
        compared == 0 || compared == DUNES_RECEDING_CAMERAS.len(),
        "the mesh arm produced a per-cluster cut for {compared} of the {} cameras",
        DUNES_RECEDING_CAMERAS.len()
    );
    if compared == 0 {
        eprintln!(
            "vk e2e: no per-cluster cut on this device, so the two granularities were \
             not compared; only the uniform arm ran"
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// The near third of the patch is drawn at a finer level than the far third,
/// and both ends drew something.
///
/// Its own function so the test above can run it in a `catch_unwind` and show
/// that it refuses a cut that does not mix — which is the difference between an
/// assertion and a decoration.
fn assert_mixes_levels(
    near: &std::collections::BTreeMap<usize, usize>,
    far: &std::collections::BTreeMap<usize, usize>,
) {
    assert!(
        !near.is_empty() && !far.is_empty(),
        "one end of the patch drew nothing at all: near {near:?}, far {far:?}"
    );
    assert!(
        near.keys().min() < far.keys().min(),
        "the near end {near:?} is not drawn finer than the far end {far:?}"
    );
}
