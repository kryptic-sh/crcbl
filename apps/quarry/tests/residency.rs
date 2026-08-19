//! The generated face, made resident on a device.
//!
//! `docs/plan/sample/14-quarry.md`'s milestone 1 is "meshlet bake +
//! `MeshShader` path rendering the scene". This asserts the half that can be
//! asserted before there is an application to look at: the scene
//! `crcbl_quarry::scene` describes is one a [`ForwardRenderer`] accepts — its
//! pools fit, its meshlet clusters are well-formed, and its vertex bytes are
//! the layout the shader reads.
//!
//! **It is not a picture and does not claim to be.** A frame drawn and read
//! back is the next slice. What this rules out is the class of failure only a
//! device notices — a pool a byte too small, a cluster naming a vertex past the
//! end, a stride that disagrees with `crcbl_shaders::mesh` — which is most of
//! the ways generated content goes wrong.
//!
//! # Which backend
//!
//! `Null` unless `CRCBL_GPU` names another, which is `apps/viewer`'s rule and
//! for its reason: the recording backend runs everywhere, so this is covered in
//! every job on every machine, and pinning a real one turns it into evidence
//! about a driver. An unparseable name panics rather than falling back — a run
//! that quietly substituted `Null` would be a green result about a backend
//! nobody asked for.
//!
//! [`ForwardRenderer`]: crcbl::render::ForwardRenderer

use crcbl::backend::GpuBackend;
use crcbl::hal::{
    CompositeAlpha, DeviceDesc, Features, PresentMode, QueueKind, SurfaceTarget, SwapchainDesc,
};
use crcbl::render::ForwardRenderer;
use crcbl::render::scene::InstanceDesc;
use crcbl_quarry::{face, scene};

/// Quads per side. Smaller than the binary's 256: this asserts the scene is
/// *acceptable*, which a face four times the area exercises no more of while
/// costing every job the generation time.
const CELLS: u32 = 64;

/// The offscreen ring's size. Nothing is read back, so it only has to be legal.
const EXTENT: (u32, u32) = (256, 192);

/// Which backend to open, `Null` unless `CRCBL_GPU` names another.
fn backend() -> GpuBackend {
    match std::env::var("CRCBL_GPU") {
        Err(_) => GpuBackend::Null,
        Ok(name) => GpuBackend::from_name(&name).unwrap_or_else(|| {
            panic!(
                "CRCBL_GPU names {name:?}, which is not a backend — refusing to fall back to Null \
                 and report a pass about a backend nobody asked for"
            )
        }),
    }
}

/// **The quarry face is a scene a `ForwardRenderer` accepts.**
///
/// The premise every later milestone stands on. `with_scene` is where a pool
/// too small, a malformed cluster run or a vertex stride disagreeing with the
/// shader is refused — so this failing means the *content* is wrong rather than
/// the drawing, which is why it is worth asserting apart from a picture.
#[test]
fn the_face_is_a_scene_the_renderer_makes_resident() {
    crcbl::core::log::init_logging();
    let instance = crcbl::backend::open_backend(backend()).expect("a backend opens");
    let adapters = instance.adapters();
    let adapter = crcbl::adapter::select(crcbl::adapter::pin().as_deref(), &adapters)
        .unwrap_or_else(|miss| panic!("{miss}"))
        .clone();

    // SAFETY: `Offscreen` names no window, so there is no surface whose
    // lifetime this could outlive — it is the one target always legal here.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface always opens");
    let caps = instance
        .surface_caps(surface, adapter.id)
        .expect("the offscreen ring reports its caps");
    let format = caps.preferred_format().expect("some format is offered");

    let device = instance
        .create_device(&DeviceDesc {
            label: Some("quarry residency"),
            adapter: adapter.id,
            required_features: Features::empty(),
            // The mesh path is this sample's subject so it is asked for, and
            // not *required*, because this assertion is about the scene rather
            // than which path draws it: `GeometryPath::from_features` resolves
            // downward on a device without it.
            optional_features: Features::MESH_SHADER | Features::GPU_DRIVEN,
            compatible_surface: Some(surface),
        })
        .expect("a device opens");
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("a graphics queue always exists");
    let swapchain = device
        .create_swapchain(&SwapchainDesc {
            label: Some("quarry ring"),
            surface,
            format,
            extent: EXTENT,
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect("the offscreen ring is created");

    let face = face::quarry_face(CELLS);
    let desc = scene::quarry_scene(&face).expect("the face partitions into meshlets");
    eprintln!(
        "quarry residency: {} triangles on {:?}, geometry path {:?}",
        face.triangles(),
        adapter.name,
        device.caps().geometry_path(),
    );

    let mut renderer = ForwardRenderer::with_scene(device.as_ref(), queue, format, &desc)
        .expect("the renderer makes the quarry scene resident");
    renderer
        .add_instance(&InstanceDesc {
            mesh: 0,
            material: 0,
            transform: crcbl::math::Mat4::IDENTITY,
        })
        .expect("one instance fits the reservation this scene asked for");

    // Unwound by hand, in reverse order of creation: nothing here has a `Drop`,
    // and a leaked pipeline is exactly what `crcbl-vk`'s teardown warning
    // reports.
    renderer.destroy(device.as_ref());
    device.destroy_swapchain(swapchain);
    instance.destroy_surface(surface);
}
