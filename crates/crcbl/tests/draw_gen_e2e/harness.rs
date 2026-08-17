//! The fixture the rest of the suite opens with — not a test module.
//!
//! [`Headless`] is an offscreen surface, a device and a swapchain-shaped image
//! ring, opened on whatever [`crcbl::backend::open`] selects. It is the
//! backend-agnostic twin of `crcbl-vk`'s `vk_e2e/harness.rs` fixture, and the
//! two differ in exactly two places:
//!
//! * **The adapter is chosen through [`crcbl::adapter`]** rather than by taking
//!   the first one enumerated, so `CRCBL_ADAPTER` pins a device class here the
//!   way it does everywhere else in `crates/crcbl/tests/`.
//! * **[`Headless::finish`] asserts on [`Device::take_error`]**, not on a
//!   validation report. There is no cross-backend equivalent of a Vulkan
//!   validation layer; the seam's own out-of-band error channel is what every
//!   backend has. `tests/hal_seam_e2e.rs` records the same substitution and the
//!   same caveat: on wgpu and WebGPU this is where a failure the return value
//!   did not carry actually arrives, and on Vulkan, Metal and D3D12 the trait's
//!   default answers `None`.
//!
//! [`select_adapter`] prints a `crcbl draw gen e2e: device on adapter …` line,
//! and that line is load-bearing outside this file: `tests/run-draw-gen-e2e.sh`
//! greps the first one to report which device really ran and to check that a
//! `CRCBL_ADAPTER` it exported actually arrived. Rewording it turns a green
//! suite into a failed harness run.
//!
//! The scene helpers below the fixture — [`MESH_EXTENT`], [`mesh_camera`],
//! [`place`], [`render_mesh`], [`read_stats_word`] — are `vk_e2e/mesh.rs`'s,
//! moved here because `cull_stats` and `draw_gen` are the modules that import
//! them. `render_mesh` reads back only the swapchain image: the `Rgba16Float`
//! probe pass its Vulkan original also runs exists for lighting assertions that
//! live in `mesh.rs` and have not moved.

use core::ops::Deref;
use std::time::{Duration, Instant};

use crcbl::adapter::ADAPTER_ENV_VAR;
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::core::SurfaceTarget;
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, CompositeAlpha, Device,
    DeviceDesc, Extent3d, Features, Format, HalError, ImageAspect, ImageSubresourceLayers,
    Instance, MemoryLocation, PresentInfo, PresentMode, ReadbackDesc, ReadbackState, ResourceState,
    SubmitInfo, SwapchainDesc,
};
use crcbl::math::Mat4;
use crcbl::render::{Camera, ForwardRenderer, InstanceDesc, InstanceHandle, TransientPool};

/// The size the cull probe's frustums are built for.
///
/// `vk_e2e`'s fixture's own extent, kept rather than rounded: small enough that
/// a software rasteriser is fast, and neither dimension is a multiple of 256, so
/// a readback whose rows a backend padded to its own alignment cannot be
/// mistaken for a tightly packed one.
pub(crate) const EXTENT: (u32, u32) = (64, 48);

/// The byte every readback destination is filled with before it is polled.
///
/// Deliberately not `0`. A frame that legitimately rendered black reads back as
/// zeroes, and so does a destination no copy ever reached — an ambiguity that
/// makes the failing assertion unable to say whether the frame was empty or the
/// readback never landed. This value is neither a plausible pixel nor a
/// plausible counter, so a byte of it surviving into an assertion is evidence
/// that nothing was copied over it.
pub(crate) const POISON: u8 = 0xA5;

/// A readback destination of `len` bytes, filled with [`POISON`].
pub(crate) fn poisoned(len: usize) -> Vec<u8> {
    vec![POISON; len]
}

/// How long [`Headless::readback`] polls before it declares the copy lost.
const READBACK_DEADLINE: Duration = Duration::from_secs(30);

/// Opens whatever backend `CRCBL_GPU` names.
///
/// Also where the process gets a logger: `crcbl-core` already owns a `log::Log`
/// and its `CRCBL_LOG` filtering, so this is the engine's own sink rather than a
/// second one written for the tests. Idempotent, and it loses gracefully to a
/// logger already installed.
fn instance() -> Box<dyn Instance> {
    crcbl::core::log::init_logging();
    let instance = crcbl::backend::open().expect("a backend opens");
    assert_backend_matches_the_pin(instance.as_ref());
    instance
}

/// **The backend that opened is the backend that was asked for.**
///
/// The one failure this suite cannot survive silently: every test here is meant
/// to pass on every backend, so a fallback produces a green run that is evidence
/// about a backend nobody named. Both names go through [`GpuBackend::from_name`]
/// rather than a second table.
fn assert_backend_matches_the_pin(instance: &dyn Instance) {
    let Ok(requested) = std::env::var(BACKEND_ENV_VAR) else {
        return;
    };
    let opened = GpuBackend::from_name(&instance.backend().to_string())
        .expect("every backend the registry can open has a GpuBackend spelling");
    assert_eq!(
        Some(opened),
        GpuBackend::from_name(&requested),
        "{BACKEND_ENV_VAR}={requested} was asked for and {} answered",
        instance.backend()
    );
}

/// The adapter [`ADAPTER_ENV_VAR`] names, announced on stderr.
///
/// See this file's header for why the line matters outside it.
fn select_adapter(instance: &dyn Instance) -> crcbl::hal::AdapterInfo {
    let adapters = instance.adapters();
    let pin = crcbl::adapter::pin();
    let adapter = crcbl::adapter::select(pin.as_deref(), &adapters)
        .unwrap_or_else(|miss| panic!("{miss}"))
        .clone();
    eprintln!(
        "crcbl draw gen e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = pin.as_deref().unwrap_or("<unset>"),
    );
    adapter
}

/// The fixture's device, in a slot [`Headless::finish`] can empty.
///
/// `Headless` has a [`Drop`] impl — the failure path's diagnostics — and a type
/// with one cannot have a field moved out of it. The [`Deref`] is what keeps
/// every `headless.device.…` reading as it did in the suite this came from.
pub(crate) struct DeviceSlot(Option<Box<dyn Device>>);

impl DeviceSlot {
    fn new(device: Box<dyn Device>) -> Self {
        Self(Some(device))
    }

    /// Destroys the device. Emptying the slot is the point: a second call does
    /// nothing, and every later deref panics with the message below rather than
    /// reaching a dangling handle.
    fn destroy(&mut self) {
        drop(self.0.take());
    }
}

impl Deref for DeviceSlot {
    type Target = Box<dyn Device>;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("`Headless::finish` has already destroyed this fixture's device")
    }
}

/// An offscreen surface, a device, and a swapchain-shaped image ring.
pub(crate) struct Headless {
    pub(crate) device: DeviceSlot,
    surface: crcbl::hal::SurfaceHandle,
    pub(crate) swapchain: crcbl::hal::SwapchainHandle,
    pub(crate) queue: crcbl::hal::QueueHandle,
    pub(crate) format: Format,
    /// Destroyed last, and therefore declared last: a field is dropped in
    /// declaration order, and the surface handle above it names an object this
    /// owns.
    instance: Box<dyn Instance>,
}

impl Headless {
    /// The cull probe's ring: [`EXTENT`], and nothing required.
    pub(crate) fn open() -> Self {
        Self::open_at(EXTENT, Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY)
    }

    /// A ring the mesh scenes render into, on the best geometry path the
    /// adapter offers.
    pub(crate) fn open_for_mesh() -> Self {
        Self::open_for_mesh_with(Features::GPU_DRIVEN)
    }

    /// The same ring, with a chosen optional feature set.
    ///
    /// **The set decides the forward pass's emit tail.** A device opened without
    /// [`Features::DRAW_INDIRECT_COUNT`] selects
    /// [`GeometryPath::IndirectPerBatch`](crcbl::hal::GeometryPath::IndirectPerBatch),
    /// which is the path Metal is on and which an adapter reporting everything
    /// would otherwise never select — so asking for less is the only way to run
    /// that arm on some machines at all.
    pub(crate) fn open_for_mesh_with(optional: Features) -> Self {
        Self::open_at(
            MESH_EXTENT,
            optional | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
        )
    }

    fn open_at(extent: (u32, u32), optional_features: Features) -> Self {
        let instance = instance();
        let adapter = select_adapter(instance.as_ref());

        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. `finish` tears the swapchain down
        // before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("the offscreen ring reports its own caps");
        // The ring's preferred format rather than a pinned `Rgba8UnormSrgb`.
        // `vk_e2e`'s fixture pins one because it compares against a golden
        // blessed in it; nothing in this suite commits an image, and the two
        // frames `every_geometry_path…` compares are drawn through the same
        // choice.
        let format = caps.preferred_format().expect("some format is offered");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl draw gen e2e"),
                adapter: adapter.id,
                // Nothing is required, so the same fixture opens on a discrete
                // GPU and on a software rasteriser and the tests branch on what
                // actually came back.
                required_features: Features::empty(),
                optional_features,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl::hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("crcbl draw gen e2e ring"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");

        Self {
            device: DeviceSlot::new(device),
            surface,
            swapchain,
            queue,
            format,
            instance,
        }
    }

    /// Reads `size` bytes of `staging` back into `out`, polling with a deadline
    /// rather than sleeping — `docs/plan/12-testing.md`.
    pub(crate) fn readback(&self, staging: crcbl::hal::BufferHandle, size: u64, out: &mut [u8]) {
        let device = self.device.as_ref();
        let readback = device
            .request_readback(&ReadbackDesc {
                label: Some("crcbl draw gen e2e readback"),
                buffer: staging,
                offset: 0,
                size,
                after: None,
            })
            .expect("a readback request");
        let started = Instant::now();
        let deadline = started + READBACK_DEADLINE;
        loop {
            match device
                .poll_readback(readback, out)
                .expect("the readback did not fail")
            {
                ReadbackState::Ready => break,
                ReadbackState::Pending => assert!(
                    Instant::now() < deadline,
                    "the {size}-byte readback was still Pending after {:?}, past the \
                     {READBACK_DEADLINE:?} this polls for. Nothing was copied into the \
                     destination, so it still holds {POISON:#04x} and every byte a caller \
                     reads out of it is this harness's fill and not a frame.",
                    started.elapsed(),
                ),
            }
            std::thread::yield_now();
        }
        device.destroy_readback(readback);
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires, then asserts
    /// the device reported nothing out of band.
    ///
    /// **Callers must end with this rather than dropping the fixture.** It is
    /// what the Vulkan original's `validation_report().assert_clean()` stands in
    /// for here; a test that drops the fixture instead never asks the device
    /// what it saw.
    pub(crate) fn finish(mut self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        if let Some(error) = self.device.take_error() {
            panic!(
                "the device reported out of band: {error}. Every call in this test returned \
                 success, so this is a failure the return values did not carry."
            );
        }
        self.device.destroy();
    }
}

impl Drop for Headless {
    /// The failure path's copy of what [`Headless::finish`] reports.
    ///
    /// `finish` is the last line of a test, so a test that panics never reaches
    /// it and the device's verdict is discarded on exactly the runs that go red.
    /// This prints it instead, and prints nothing at all otherwise.
    ///
    /// **Nothing here may panic.** It runs while the thread is already
    /// unwinding, and a second panic aborts the process, destroying the output
    /// this exists to produce.
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        eprintln!(
            "crcbl draw gen e2e: the fixture was dropped by a panicking test, so `finish` \
             never ran. What it can still see:"
        );
        match self.device.0.as_ref() {
            // The distinction a flake needs: a lost device is a driver-side
            // failure that makes every other symptom downstream noise, and it is
            // otherwise invisible because nothing in a failing test asks.
            Some(device) => {
                match device.wait_idle() {
                    Ok(()) => eprintln!(
                        "crcbl draw gen e2e:   wait_idle: Ok — the device is alive and idle, \
                         so the submission completed and the failure is in what it produced"
                    ),
                    Err(HalError::DeviceLost(detail)) => eprintln!(
                        "crcbl draw gen e2e:   wait_idle: device lost ({detail}) — nothing \
                         this test read back means anything"
                    ),
                    Err(error) => eprintln!("crcbl draw gen e2e:   wait_idle: {error}"),
                }
                match device.take_error() {
                    Some(error) => eprintln!("crcbl draw gen e2e:   out-of-band error: {error}"),
                    None => eprintln!("crcbl draw gen e2e:   out-of-band error: none"),
                }
            }
            None => eprintln!(
                "crcbl draw gen e2e:   wait_idle: not asked, `finish` already destroyed the \
                 device"
            ),
        }
    }
}

// --- the mesh scene ---------------------------------------------------------

/// The size the mesh scenes render at.
///
/// The same 256×192 `vk_e2e/mesh.rs` and `tests/render_e2e.rs` use, and for the
/// reason both state: a smaller frame gives a structural comparison too few 8×8
/// blocks to average over.
pub(crate) const MESH_EXTENT: (u32, u32) = (256, 192);

/// The animation time every mesh frame here renders at.
///
/// A constant, not a clock. Chosen so **three faces of the cube are visible at
/// once**, which is not automatic — at `0.7` its `+X` face is edge-on to this
/// camera to within a fifth of a degree — and three faces means no symmetry is
/// left for a transposed matrix or a mirrored axis to hide behind.
pub(crate) const MESH_SECONDS: f32 = 0.35;

/// Where the camera is for every mesh frame.
///
/// Far enough back that the cube does not touch the frame edge under either
/// projection, and off-axis on two of three axes so three faces are visible at
/// once.
pub(crate) fn mesh_camera(projection: crcbl::render::Projection) -> Camera {
    Camera {
        eye: crcbl::math::Vec3::new(1.6, 1.2, 2.2),
        target: crcbl::math::Vec3::ZERO,
        up: crcbl::math::Vec3::Y,
        projection,
    }
}

/// Puts one of the demo scene's meshes in the frame at `transform`, and answers
/// with the handle that names it.
///
/// **Insertion order is the caller's and it is load-bearing** — the slot an
/// object lands in is `docs/plan/25-lod.md`'s hysteresis key, and it is also the
/// instance index every CPU reference in this suite is written against — so
/// every scene here places the cube first and whatever stands beside it after.
pub(crate) fn place(
    renderer: &mut ForwardRenderer,
    mesh: usize,
    material: usize,
    transform: Mat4,
) -> InstanceHandle {
    renderer
        .add_instance(&InstanceDesc {
            mesh,
            material,
            transform,
        })
        .expect("an instance pool of thousands has room for a test scene")
}

/// Puts the demo scene's cube in the frame at `transform`, and **first**, so it
/// takes the pool slot every reference here expects it in.
pub(crate) fn place_cube_at(renderer: &mut ForwardRenderer, transform: Mat4) {
    place(
        renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        transform,
    );
}

/// The same, at the spin every frame in this suite is drawn at.
pub(crate) fn place_cube(renderer: &mut ForwardRenderer) {
    place_cube_at(renderer, ForwardRenderer::spin(MESH_SECONDS));
}

/// Renders one frame of the forward pipeline **through the real render graph**
/// and reads the swapchain image back.
///
/// Deliberately [`ForwardRenderer`] and [`crcbl::render::RenderGraph`] rather
/// than a hand-built copy: a frame is only evidence about the code an
/// application runs if it *is* the code an application runs.
pub(crate) fn render_mesh(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &Camera,
) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
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
            &crcbl::render::DirectionalLight::default(),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
        queue: headless.queue,
    });

    let compiled = {
        let mut graph = crcbl::render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it as a copy source and the
                // copy below needs no barrier of its own. Saying so in the
                // import is the point — there is not one hand-written barrier
                // anywhere in this path.
                final_state: ResourceState::TransferSrc,
            },
        );
        let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        // `&*pool`: the same pool the frame is about to be realised against, so
        // the barriers open where the last frame left off.
        graph.compile(&*pool).expect("a legal frame")
    };
    eprintln!("crcbl draw gen e2e: {}", compiled.dump());
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
        image_offset: crcbl::hal::Offset3d::default(),
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
    renderer: &ForwardRenderer,
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
        label: Some("culling statistics copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl::hal::BufferBarrier {
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
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
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
