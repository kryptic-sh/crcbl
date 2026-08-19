//! The fixture every quarry device test opens with, and the two readbacks
//! they measure a frame by.
//!
//! `docs/plan/sample/14-quarry.md`'s milestone 1 is "meshlet bake +
//! `MeshShader` path rendering the scene". Both halves are asserted here
//! without a window, through [`GpuContext::open_offscreen`]:
//!
//! * **resident** — the scene `crcbl_quarry::scene` describes is one a
//!   [`ForwardRenderer`] accepts: its pools fit, its meshlet clusters are
//!   well-formed, and its vertex bytes are the layout the shader reads;
//! * **drawn** — a frame recorded through the real render graph and read back,
//!   which is the only thing that can tell a scene that was accepted from one
//!   that produces a picture.
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
//! **`Null` draws nothing, and the frame test says so rather than skipping.**
//! It records the whole frame — every pass, every barrier, the draw itself —
//! and asserts that; the pixels are asserted wherever there are pixels. Which
//! of the two ran is printed, because "not supported here" reported as "passed"
//! is the shape this repo keeps removing.
//!
//! [`GpuContext::open_offscreen`]: crcbl::engine::GpuContext::open_offscreen
//! [`ForwardRenderer`]: crcbl::render::ForwardRenderer

use crcbl::backend::GpuBackend;
use crcbl::engine::{GpuContext, GpuContextDesc};
use crcbl::hal::{
    BufferDesc, BufferImageCopy, BufferUsage, Extent3d, Features, ImageAspect,
    ImageSubresourceLayers, MemoryLocation, ReadbackDesc, ReadbackState, ResourceState,
};
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{Camera, ForwardRenderer, ImportedImage, RenderGraph, TransientPool};
use crcbl_quarry::{dag, face, scene};

/// Quads per side. Smaller than the binary's 256: these assert that the scene
/// is *acceptable* and that it draws, neither of which a face four times the
/// area exercises more of, while it would cost every job the generation time.
pub(crate) const CELLS: u32 = 64;

/// The offscreen ring's size, and `crates/crcbl/tests/gpu_scene`'s for its
/// stated reason — a smaller frame gives a structural comparison too few 8×8
/// blocks to average over, and 256 pixels of RGBA8 satisfy the 256-byte copy
/// pitch D3D12 and WebGPU enforce without padding.
pub(crate) const EXTENT: (u32, u32) = (256, 192);

/// The pixel budget every frame here renders at unless it is the subject.
///
/// `ForwardRenderer`'s own default, restated so the frames that are *not* about
/// selection say which budget they took rather than inheriting one silently.
pub(crate) const DEFAULT_BUDGET: f32 = 1.0;

/// The budget the mixing assertion is read at.
///
/// Measured, not chosen: swept over 1, 4, 16, 64 and 256 pixels from this
/// camera, the cut goes `[100, 2, …]`, `[28, 43, …]`, `[0, 30, 18, …]`,
/// `[0, 5, 22, 5, …]`, `[0, 0, 21, 7, …]`. Sixteen is where two levels both
/// carry a substantial share and the base is gone entirely, which is the point
/// furthest from either extreme — and the extremes are asserted below, which is
/// what shows this assertion can fail.
pub(crate) const MIXING_BUDGET: f32 = 16.0;

/// The share of a cut one level holds before it counts as dominating it.
///
/// Four fifths. The two cuts this separates are `[100, 2, …]` at one pixel —
/// 98% — and `[0, 30, 18, …]` at [`MIXING_BUDGET`], which is 63%.
pub(crate) const DOMINATED: f32 = 0.8;

/// A budget no group's projected error can exceed from this camera, so the
/// descent stops at the top of the DAG.
pub(crate) const COARSE_BUDGET: f32 = 4096.0;

/// How long the frame readback polls before it declares the copy lost. The
/// engine's own suites use the same bound.
const READBACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// The byte the readback buffer is filled with before the copy. Nothing a
/// renderer writes is this on every channel, so a destination still holding it
/// is a copy that never happened rather than a frame that came out dark.
const POISON: u8 = 0xA5;

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

/// An offscreen context with the quarry face resident in a renderer.
pub(crate) struct Quarry {
    pub(crate) ctx: GpuContext,
    pub(crate) renderer: ForwardRenderer,
    pub(crate) triangles: usize,
}

/// Which description of the same face to make resident.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Levels {
    /// One flat mesh, drawn at full detail from every camera —
    /// [`crcbl_quarry::scene`].
    Flat,
    /// The cluster hierarchy — [`crcbl_quarry::dag`].
    Dag,
}

impl Quarry {
    /// Opens the ring, generates the face and makes it resident.
    pub(crate) fn open(levels: Levels, budget: f32) -> Self {
        crcbl::core::log::init_logging();
        let ctx = GpuContext::open_offscreen(
            EXTENT,
            &GpuContextDesc {
                label: "quarry",
                backend: Some(backend()),
                // The mesh path is this sample's subject so it is asked for, and
                // not *required*, because neither assertion here is about which
                // path draws: `GeometryPath::from_features` resolves downward on
                // a device without it.
                optional_features: Features::MESH_SHADER
                    | Features::TASK_SHADER
                    | Features::GPU_DRIVEN,
                ..GpuContextDesc::default()
            },
        )
        .expect("an offscreen context opens");

        let face = face::quarry_face(CELLS);
        let desc = match levels {
            Levels::Flat => scene::quarry_scene(&face).expect("the face partitions into meshlets"),
            Levels::Dag => dag::dag_scene(&face).expect("the face coarsens"),
        };
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), ctx.format(), &desc)
                .expect("the renderer makes the quarry scene resident");
        renderer
            .add_instance(&InstanceDesc {
                mesh: 0,
                material: 0,
                transform: crcbl::math::Mat4::IDENTITY,
            })
            .expect("one instance fits the reservation this scene asked for");
        renderer.set_lod_error_budget(budget);
        eprintln!(
            "quarry: {levels:?} at a {budget}px budget, {} triangles, geometry path {:?}, \
             format {:?}",
            face.triangles(),
            ctx.device().caps().geometry_path(),
            ctx.format(),
        );
        Self {
            ctx,
            renderer,
            triangles: face.triangles(),
        }
    }

    /// Unwinds in reverse order of creation. Nothing here has a `Drop`, and a
    /// leaked pipeline is what `crcbl-vk`'s teardown warning reports.
    pub(crate) fn finish(mut self) {
        self.ctx.drain().expect("the queue drains");
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy().expect("teardown is clean");
    }
}
/// Renders one frame of `levels` and asserts what came out.
pub(crate) fn draw_and_measure(levels: Levels, budget: f32) -> Option<Vec<usize>> {
    let mut quarry = Quarry::open(levels, budget);
    let (width, height) = EXTENT;
    let bytes = u64::from(width) * u64::from(height) * 4;

    let staging = quarry
        .ctx
        .device()
        .create_buffer(&BufferDesc {
            label: Some("quarry readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut pool = TransientPool::new();

    let acquired = quarry
        .ctx
        .acquire()
        .expect("the offscreen ring always has an image")
        .expect("an offscreen ring never goes out of date");
    assert_eq!(acquired.extent, EXTENT);
    let device = quarry.ctx.device();

    // **Above the near edge, looking down the length of the face.** The face
    // occupies X ∈ ±`WIDTH_METRES`/2, Y ∈ 0..`HEIGHT_METRES` and Z ∈
    // 0..`DEPTH_METRES`, and its winding is counter-clockwise seen from +Y — so
    // a camera has to be above it and looking along +Z or it sees the back of
    // every triangle, which is the difference between an empty frame and a
    // picture. Standing outside the near edge on −Z puts the whole run of it in
    // shot, which is the shape the sample is for.
    let camera = Camera {
        eye: crcbl::math::Vec3::new(0.0, face::HEIGHT_METRES, -30.0),
        target: crcbl::math::Vec3::new(0.0, 0.0, face::DEPTH_METRES * 0.5),
        up: crcbl::math::Vec3::Y,
        projection: crcbl::render::Projection::default(),
    };
    quarry
        .renderer
        .begin_frame(
            device,
            &camera,
            &crcbl::render::DirectionalLight::default(),
            EXTENT,
        )
        .expect("the uniform buffer is writable");

    let mut encoder = device.create_command_encoder(&crcbl::hal::CommandEncoderDesc {
        label: Some("quarry frame"),
        queue: quarry.ctx.queue(),
    });
    let compiled = {
        let mut graph = RenderGraph::new(quarry.ctx.queue());
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: quarry.ctx.format(),
                extent: EXTENT,
                initial: ResourceState::Undefined,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it a copy source and the copy
                // below needs no barrier of its own.
                final_state: ResourceState::TransferSrc,
            },
        );
        let _hdr = quarry.renderer.add_passes(&mut graph, target, EXTENT);
        graph.compile(&pool).expect("a legal frame")
    };
    let dump = compiled.dump();
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
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
    quarry
        .ctx
        .submit_and_present(&acquired, commands)
        .expect("the frame submits and presents");

    // **Recorded, on every backend including the one that draws nothing.** The
    // graph named the forward pass and a draw reached it; a scene that produced
    // no draw would compile to a frame with neither.
    assert!(
        dump.contains("forward"),
        "the compiled frame has no forward pass in it:\n{dump}"
    );

    // **Drawn, wherever there are pixels to read.**
    if backend() == GpuBackend::Null {
        eprintln!(
            "quarry: the Null backend records and draws nothing, so this frame was asserted as a \
             recording only — run with CRCBL_GPU=vk (or dx12, metal) for the pixels"
        );
    } else {
        let mut frame = vec![POISON; bytes as usize];
        readback(quarry.ctx.device(), staging, bytes, &mut frame);
        let mut histogram: std::collections::HashMap<[u8; 4], usize> =
            std::collections::HashMap::new();
        for pixel in frame.chunks_exact(4) {
            *histogram
                .entry([pixel[0], pixel[1], pixel[2], pixel[3]])
                .or_default() += 1;
        }
        let pixels = (width * height) as usize;
        let (background, count) = histogram
            .iter()
            .max_by_key(|(_, count)| **count)
            .expect("a frame has pixels in it");
        let covered = pixels - count;
        eprintln!(
            "quarry: {} distinct colour(s), {covered} of {pixels} pixels ({:.1}%) are not the \
             most common one {background:?}",
            histogram.len(),
            covered as f32 / pixels as f32 * 100.0,
        );
        // **A fraction, not "more than one colour".** A single lit triangle
        // clears that bar and is not a quarry face; this is a wall filling most
        // of the frame from a camera aimed at it, so anything under a fifth is
        // the geometry failing rather than the camera being generous.
        assert!(
            covered * 5 > pixels,
            "only {covered} of {pixels} pixels differ from {background:?}, so what drew is not \
             the face — the graph recorded the frame, so this is the draw producing almost \
             nothing"
        );
    }

    let cut = (levels == Levels::Dag)
        .then(|| read_the_cut(&quarry))
        .flatten();

    quarry.ctx.device().destroy_command_buffer(commands);
    quarry.ctx.device().destroy_buffer(staging);
    pool.destroy(quarry.ctx.device());
    quarry.finish();
    cut
}

/// Reads `size` bytes of `staging` into `out`, polling with a deadline rather
/// than sleeping — `docs/plan/12-testing.md`.
fn readback(
    device: &dyn crcbl::hal::Device,
    staging: crcbl::hal::BufferHandle,
    size: u64,
    out: &mut [u8],
) {
    let request = device
        .request_readback(&ReadbackDesc {
            label: Some("quarry frame readback"),
            buffer: staging,
            offset: 0,
            size,
            after: None,
        })
        .expect("a readback request");
    let started = std::time::Instant::now();
    loop {
        match device
            .poll_readback(request, out)
            .expect("the readback did not fail")
        {
            ReadbackState::Ready => break,
            ReadbackState::Pending => assert!(
                started.elapsed() < READBACK_DEADLINE,
                "the {size}-byte readback was still Pending after {:?}. Nothing was copied, so \
                 the destination still holds {POISON:#04x} and every byte read out of it is this \
                 fill rather than a frame.",
                started.elapsed(),
            ),
        }
        std::thread::yield_now();
    }
    device.destroy_readback(request);
}

/// **Which level each cluster of the face was drawn from.**
///
/// `docs/plan/25-lod.md`'s observable, and the only thing that can show
/// per-cluster selection happening: a frame whose every cluster came from one
/// level is a plausible picture and matches any golden blessed from it.
/// `ForwardRenderer::cluster_selection` is the buffer the amplification stage
/// records its cut into, one `u32` per resident cluster, and nothing in the
/// frame reads it.
///
/// Answers with how many clusters each level contributed, finest first, or
/// `None` where the device has no amplification stage to record a cut.
pub(crate) fn read_the_cut(quarry: &Quarry) -> Option<Vec<usize>> {
    let selection = match quarry.renderer.cluster_selection(quarry.renderer.frame()) {
        Some(selection) => selection,
        None => {
            eprintln!(
                "quarry: no amplification stage on this device, so there is no per-cluster cut to \
                 read — that is every device without TASK_SHADER and every non-mesh path"
            );
            return None;
        }
    };
    let device = quarry.ctx.device();
    let range = quarry
        .renderer
        .cluster_range(0)
        .expect("the mesh path has a cluster pool");
    let bytes = u64::from(range.count) * 4;

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("quarry cut readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&crcbl::hal::CommandEncoderDesc {
        label: Some("quarry cut copy"),
        queue: quarry.ctx.queue(),
    });
    // Its own submission after the frame's, so the buffer is moved out of the
    // state the next frame on this slot expects it in and put straight back.
    let barrier = |from, to| {
        [crcbl::hal::BufferBarrier {
            buffer: selection,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderReadWrite, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderReadWrite);
    encoder.pipeline_barrier(&crcbl::hal::Barriers {
        buffers: &out,
        ..crcbl::hal::Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: selection,
        src_offset: u64::from(range.base) * 4,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&crcbl::hal::Barriers {
        buffers: &back,
        ..crcbl::hal::Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(
            quarry.ctx.queue(),
            &crcbl::hal::SubmitInfo::new(&[commands]),
        )
        .expect("submit");

    let mut words = vec![POISON; bytes as usize];
    readback(device, staging, bytes, &mut words);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    // The pool holds the DAG's levels finest first and contiguously, so the run
    // splits back into levels by their own cluster counts.
    let dag = dag::quarry_dag(&face::quarry_face(CELLS)).expect("the face coarsens");
    let mut at = 0usize;
    let mut drawn = Vec::with_capacity(dag.levels.len());
    for level in &dag.levels {
        let mut here = 0usize;
        for _ in 0..level.clusters.clusters.len() {
            let word =
                u32::from_le_bytes(words[at * 4..at * 4 + 4].try_into().expect("four bytes"));
            assert!(
                word <= 1,
                "cluster {at} of the run recorded {word}, which is not a flag — the copy read \
                 something other than the cut"
            );
            here += usize::try_from(word).expect("a flag");
            at += 1;
        }
        drawn.push(here);
    }
    assert_eq!(
        at * 4,
        words.len(),
        "the DAG's levels and the pool run it was uploaded into disagree about how many clusters \
         there are"
    );
    let total: usize = drawn.iter().sum();
    eprintln!("quarry: cut drew {total} cluster(s), per level (finest first) {drawn:?}");
    assert!(
        total > 0,
        "the amplification stage ran and selected no cluster at all, yet the frame drew — so \
         this is reading the wrong buffer rather than an empty cut"
    );
    Some(drawn)
}
