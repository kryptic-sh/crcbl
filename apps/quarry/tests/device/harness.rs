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
    BufferDesc, BufferImageCopy, BufferUsage, Extent3d, Features, GeometryPath, ImageAspect,
    ImageSubresourceLayers, MemoryLocation, ReadbackDesc, ReadbackState, ResourceState,
};
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{ForwardRenderer, ImportedImage, InitialClaim, RenderGraph, TransientPool};
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

/// This suite's name, as it prefixes every line this fixture prints.
///
/// **`tests/run-quarry-e2e.sh` greps this**, so changing the string turns a
/// green suite into a failed harness run rather than into nothing.
pub(crate) const SUITE: &str = "crcbl quarry e2e";

/// The variable naming which adapter to open, echoed on the line above.
const ADAPTER_ENV_VAR: &str = "CRCBL_ADAPTER";

/// Which backend to open, `Null` unless `CRCBL_GPU` names another.
pub(crate) fn backend() -> GpuBackend {
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
    /// Which description was made resident, so a frame knows whether there is a
    /// per-cluster cut to read at all.
    levels: Levels,
    /// **One pool across every frame**, which is the point of holding it here:
    /// the graph compiles against the pool the last frame left, so barriers
    /// open where that frame closed them rather than from `Undefined` each
    /// time. A pool per frame would make every frame the first one.
    pool: TransientPool,
}

/// What one frame came out as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// Pixels that are not the frame's most common colour — the face against
    /// the clear.
    pub(crate) covered: usize,
    /// How many pixels there are, so `covered` reads as a share.
    pub(crate) pixels: usize,
    /// How many clusters each level contributed, finest first, or `None` where
    /// the device records no per-cluster cut.
    pub(crate) cut: Option<Vec<usize>>,
    /// The one level a non-mesh path's uniform cut drew, and what it cost, or
    /// `None` on the mesh path — where a mesh is one bucket and the level is
    /// chosen per cluster instead.
    pub(crate) uniform: Option<Uniform>,
    /// What the frame's culling kept: instances the camera's cull passed, and
    /// clusters the amplification stage passed. `None` until the ring has come
    /// round — it is a few frames behind, which is what makes it free of any
    /// wait.
    pub(crate) culled: Option<crcbl::render::CullStats>,
    /// The frame itself, as read back — empty where nothing was drawn.
    ///
    /// Held because coverage, the cut and the triangle counts are all *counts*,
    /// and a face lit from the wrong side leaves every one of them unchanged.
    pub(crate) pixels_rgba: Vec<u8>,
}

/// What a uniform cut drew, read out of the indirect arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Uniform {
    /// Which level, finest is zero — the bucket whose instance count came out
    /// non-zero.
    pub(crate) level: usize,
    /// Triangles the draw actually asked for, which is what
    /// `docs/plan/sample/14-quarry.md`'s "triangle count per path" means.
    pub(crate) triangles: u32,
}

/// The dolly: **the library's**, so the window and the goldens fly the same
/// path.
///
/// It used to be three items here. A windowed run whose fixed pose is not the
/// pose `tests/golden/` was blessed from is a picture nobody can hold against
/// the committed one, and two copies of a camera is exactly how that happens —
/// so `crcbl_quarry::camera` owns them and this re-exports them under the names
/// every module in this suite already reaches for.
pub(crate) use crcbl_quarry::camera::{DOLLY_END, DOLLY_START, dolly};

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
        Self::open_on(levels, budget, GeometryPath::MeshShader)
    }

    /// The same, on the geometry path `path` names — **by subtracting features
    /// from one capable adapter**, which is how the engine's own suites reach a
    /// path no device would select for itself.
    ///
    /// `GeometryPath::from_features` reads `MESH_SHADER` then
    /// `DRAW_INDIRECT_COUNT` and falls through to `IndirectPerBatch`, so
    /// withholding one feature at a time walks down the three. A device that
    /// never had the feature lands on the same path by the same rule, which is
    /// what makes the forced path evidence about the path rather than about this
    /// adapter.
    pub(crate) fn open_on(levels: Levels, budget: f32, path: GeometryPath) -> Self {
        let wanted = match path {
            GeometryPath::MeshShader => {
                Features::MESH_SHADER | Features::TASK_SHADER | Features::GPU_DRIVEN
            }
            GeometryPath::IndirectCount => Features::GPU_DRIVEN,
            GeometryPath::IndirectPerBatch => {
                Features::GPU_DRIVEN.difference(Features::DRAW_INDIRECT_COUNT)
            }
        };
        Self::open_with(levels, budget, wanted, path)
    }

    fn open_with(levels: Levels, budget: f32, wanted: Features, path: GeometryPath) -> Self {
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
                optional_features: wanted,
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
        let selected = ctx.device().caps().geometry_path();
        // **Which device drew, named once per fixture.** `tests/run-quarry-e2e.sh`
        // greps this prefix: a green run that never said what it ran on is
        // evidence about nothing, and on a machine with three adapters the
        // answer is not guessable from the backend alone.
        eprintln!(
            "{SUITE}: device on adapter {:?} ({ADAPTER_ENV_VAR}={})",
            ctx.adapter()
                .expect("the context's own adapter is enumerable")
                .name,
            crcbl::adapter::pin().as_deref().unwrap_or("<unset>"),
        );
        eprintln!(
            "{SUITE}: {levels:?} at a {budget}px budget, {} triangles, geometry path {selected:?}, \
             format {:?}",
            face.triangles(),
            ctx.format(),
        );
        // **The path asked for is the path that opened.** Subtracting a feature
        // is a request, and a device that never offered it lands one rung lower
        // than intended — so a run that silently drew through `IndirectPerBatch`
        // while claiming `IndirectCount` would be a green comparison between one
        // path and itself. `Null` selects its own path and is exempt: it draws
        // nothing to compare.
        assert!(
            selected == path || backend() == GpuBackend::Null,
            "{path:?} was asked for by withholding features and {selected:?} opened"
        );
        Self {
            ctx,
            renderer,
            triangles: face.triangles(),
            levels,
            pool: TransientPool::new(),
        }
    }

    /// Renders one frame from `at` along the dolly and measures what came out.
    ///
    /// **Frames accumulate.** `docs/plan/25-lod.md`'s hysteresis is device-local
    /// state a shader writes once a frame, so a cut depends on every frame
    /// before it on this renderer — which is what makes a dolly a different
    /// measurement from the same positions rendered by fresh contexts.
    pub(crate) fn frame(&mut self, at: f32) -> Frame {
        self.lit_frame(at, &crcbl::render::DirectionalLight::default())
    }

    /// The same, from a camera of the caller's own rather than a pose on the
    /// dolly — and **without the coverage assertion**.
    ///
    /// The two halves go together. A caller reaching for this is aiming the
    /// camera somewhere the dolly does not go, and `freeze.rs` turns it away
    /// from the face on purpose: a frame that draws nothing is the *observable*
    /// there, so demanding that a fifth of it be covered would refuse the
    /// measurement rather than make it. Everything else the fixture asserts —
    /// that the graph compiled a forward pass and that a draw reached it — still
    /// holds, and the cut and the cull statistics still come back.
    pub(crate) fn frame_from(&mut self, camera: &crcbl::render::Camera) -> Frame {
        frame_body(
            self,
            camera,
            &crcbl::render::DirectionalLight::default(),
            Coverage::Unchecked,
        )
    }

    /// The same, under a light of the caller's choosing.
    ///
    /// The one thing quarry measures that is not a count. See
    /// `shading::the_face_is_shaded_by_the_light_it_is_given`, which is the
    /// caller and the reason this exists.
    pub(crate) fn lit_frame(&mut self, at: f32, light: &crcbl::render::DirectionalLight) -> Frame {
        frame_body(self, &dolly(at), light, Coverage::Required)
    }

    /// Unwinds in reverse order of creation. Nothing here has a `Drop`, and a
    /// leaked pipeline is what `crcbl-vk`'s teardown warning reports.
    pub(crate) fn finish(mut self) {
        self.ctx.drain().expect("the queue drains");
        self.pool.destroy(self.ctx.device());
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy().expect("teardown is clean");
    }
}
/// Renders one frame of `levels` and asserts what came out.
pub(crate) fn draw_and_measure(levels: Levels, budget: f32) -> Option<Vec<usize>> {
    let mut quarry = Quarry::open(levels, budget);
    let cut = quarry.frame(DOLLY_START).cut;
    quarry.finish();
    cut
}

/// Whether a frame is required to have the face in it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// A fifth of the frame must differ from its most common colour — see
    /// [`frame_body`]. Every pose on the dolly is aimed at the face, so a frame
    /// that came out nearly empty there is the draw failing.
    Required,
    /// The camera may be aimed anywhere, including away from the face. Only
    /// [`Quarry::frame_from`] passes this, and its header says why.
    Unchecked,
}

/// The body of one frame: renders from `camera` and measures it.
fn frame_body(
    quarry: &mut Quarry,
    camera: &crcbl::render::Camera,
    light: &crcbl::render::DirectionalLight,
    coverage: Coverage,
) -> Frame {
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
    let acquired = quarry
        .ctx
        .acquire()
        .expect("the offscreen ring always has an image")
        .expect("an offscreen ring never goes out of date");
    assert_eq!(acquired.extent, EXTENT);
    let device = quarry.ctx.device();

    quarry
        .renderer
        .begin_frame(device, camera, light, EXTENT)
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
                // Acquired from the ring, so the acquire's semaphore is what
                // orders this frame against the last one that used the image.
                claim: InitialClaim::Acquired,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it a copy source and the copy
                // below needs no barrier of its own.
                final_state: ResourceState::TransferSrc,
            },
        );
        let _hdr = quarry.renderer.add_passes(&mut graph, target, EXTENT);
        graph.compile(&quarry.pool).expect("a legal frame")
    };
    let dump = compiled.dump();
    compiled
        .execute(device, &mut quarry.pool, encoder.as_mut(), None)
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
    let mut covered = 0usize;
    let mut frame = Vec::new();
    let pixels = (width * height) as usize;
    if backend() == GpuBackend::Null {
        eprintln!(
            "{SUITE}: the Null backend records and draws nothing, so this frame was asserted as a \
             recording only — run with CRCBL_GPU=vk (or dx12, metal) for the pixels"
        );
    } else {
        frame = vec![POISON; bytes as usize];
        readback(quarry.ctx.device(), staging, bytes, &mut frame);
        let mut histogram: std::collections::HashMap<[u8; 4], usize> =
            std::collections::HashMap::new();
        for pixel in frame.chunks_exact(4) {
            *histogram
                .entry([pixel[0], pixel[1], pixel[2], pixel[3]])
                .or_default() += 1;
        }
        let (background, count) = histogram
            .iter()
            .max_by_key(|(_, count)| **count)
            .expect("a frame has pixels in it");
        covered = pixels - count;
        eprintln!(
            "{SUITE}: {} distinct colour(s), {covered} of {pixels} pixels ({:.1}%) are not the \
             most common one {background:?}",
            histogram.len(),
            covered as f32 / pixels as f32 * 100.0,
        );
        // **A fraction, not "more than one colour".** A single lit triangle
        // clears that bar and is not a quarry face; this is a wall filling most
        // of the frame from a camera aimed at it, so anything under a fifth is
        // the geometry failing rather than the camera being generous.
        assert!(
            coverage == Coverage::Unchecked || covered * 5 > pixels,
            "only {covered} of {pixels} pixels differ from {background:?}, so what drew is not \
             the face — the graph recorded the frame, so this is the draw producing almost \
             nothing"
        );
    }

    let cut = (quarry.levels == Levels::Dag)
        .then(|| read_the_cut(quarry))
        .flatten();
    let uniform = (quarry.levels == Levels::Dag)
        .then(|| read_the_uniform_cut(quarry))
        .flatten();

    quarry.ctx.device().destroy_command_buffer(commands);
    quarry.ctx.device().destroy_buffer(staging);
    Frame {
        covered,
        pixels,
        cut,
        uniform,
        culled: quarry.renderer.cull_stats(),
        pixels_rgba: frame,
    }
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
                "{SUITE}: no amplification stage on this device, so there is no per-cluster cut to \
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
    eprintln!("{SUITE}: cut drew {total} cluster(s), per level (finest first) {drawn:?}");
    assert!(
        total > 0,
        "the amplification stage ran and selected no cluster at all, yet the frame drew — so \
         this is reading the wrong buffer rather than an empty cut"
    );
    Some(drawn)
}

/// **Which single level a non-mesh path drew, and what it cost.**
///
/// The indirect paths' observable, and the counterpart to [`read_the_cut`].
/// There is no second buffer recording an intention: `draw_gen.slang` scatters
/// the instance into the bucket for the level it chose, so the bucket whose
/// `instance_count` came out non-zero **is** the level, and its `index_count` is
/// what the draw asked the device for.
///
/// `None` on the mesh path, where a mesh is one bucket and the level is chosen
/// per cluster — see [`read_the_cut`], which is that path's observable.
fn read_the_uniform_cut(quarry: &Quarry) -> Option<Uniform> {
    use crcbl::shaders::draw_gen::{DRAW_ARGS_SIZE, DrawIndexedArgs};

    let buckets = quarry.renderer.level_buckets(0);
    if buckets.is_empty() {
        return None;
    }
    let device = quarry.ctx.device();
    let args = quarry.renderer.draw_args(quarry.renderer.frame());
    let bytes = DRAW_ARGS_SIZE as u64 * u64::from(buckets[buckets.len() - 1] + 1);

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("quarry draw args readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&crcbl::hal::CommandEncoderDesc {
        label: Some("quarry draw args copy"),
        queue: quarry.ctx.queue(),
    });
    // The graph leaves the argument buffer in `IndirectArgument`, which is where
    // the next frame on this slot expects it — so this moves it out and puts it
    // straight back.
    let barrier = |from, to| {
        [crcbl::hal::BufferBarrier {
            buffer: args,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::IndirectArgument, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::IndirectArgument);
    encoder.pipeline_barrier(&crcbl::hal::Barriers {
        buffers: &out,
        ..crcbl::hal::Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: args,
        src_offset: 0,
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

    let mut drew = None;
    for (level, bucket) in buckets.iter().enumerate() {
        let at = *bucket as usize * DRAW_ARGS_SIZE;
        let args = DrawIndexedArgs::from_bytes(
            words[at..at + DRAW_ARGS_SIZE]
                .try_into()
                .expect("one argument structure"),
        );
        if args.instance_count == 0 {
            continue;
        }
        assert!(
            drew.is_none(),
            "two of this mesh's level buckets drew, and a uniform cut is one level"
        );
        drew = Some(Uniform {
            level,
            triangles: args.index_count / 3 * args.instance_count,
        });
    }
    // **The recording backend routes nothing, because it draws nothing**, so an
    // empty argument buffer is the honest answer there rather than a failure.
    // Everywhere else it is one: the frame drew, so a cut that picked a level
    // and failed to route it would leave every bucket at zero and be reported
    // here rather than shown as an empty frame.
    if backend() == GpuBackend::Null {
        return drew;
    }
    let drew = drew.expect(
        "every level bucket came out with no instances, so the cut picked a level and failed to \
         route it — the frame drew, so this is not an empty scene",
    );
    eprintln!(
        "{SUITE}: uniform cut drew level {} — {} triangle(s)",
        drew.level, drew.triangles
    );
    Some(drew)
}
