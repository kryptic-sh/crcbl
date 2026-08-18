//! Umbrella crate for the Crucible engine: one dependency for a game.
//!
//! A game written against Crucible names `crcbl` and nothing else. This crate
//! re-exports the engine crates that exist at the current phase, so a project
//! scaffolded by `crcbl new` — and `apps/sandbox`, which is the same shape —
//! never spells out a workspace path per subsystem, and never gains a
//! dependency on a *backend*.
//!
//! ```text
//! crcbl::core    → crcbl-core    handles, WorldPos, FrameArena, FrameClock, input
//! crcbl::shell   → crcbl-shell   the windowing seam and its backends
//! crcbl::hal     → crcbl-hal     the GPU seam, plus the recording null backend
//! crcbl::render  → crcbl-render  the render graph, cameras, the forward frame
//! crcbl::scene   → crcbl-scene   glTF import and the mesh bakes (feature `scene`)
//! crcbl::shaders → crcbl-shaders the engine's shaders, as SPIR-V
//! crcbl::ui      → crcbl-ui      draw lists, the glyph atlas, HUD widgets
//! crcbl::ecs     → crcbl-ecs     the world, components, systems, the schedule
//! crcbl::phys    → crcbl-phys    rigid bodies, colliders, forces, queries
//! crcbl::net     → crcbl-net     the transport seam and the wire protocol
//! crcbl::server  → crcbl-server  the authoritative simulation and its hash
//! crcbl::client  → crcbl-client  prediction, reconciliation, interpolation
//! crcbl::input   → crcbl-input   action maps and bindings
//! crcbl::audio   → crcbl-audio   the mixer, the sound bank, the cue grammar
//! crcbl::store   → crcbl-store   platform storage and atomic writes
//! crcbl::sprite  → crcbl-sprite  sheets, clips and the baked-pair reader
//! crcbl::webgpu  → crcbl-webgpu  the wasm → JS command stream (wasm32 only)
//! crcbl::math    → glam          the maths the renderer's types are spelled in
//! crcbl::log     → log           the logging facade the engine records through
//! crcbl::backend → (this crate)  runtime GPU backend selection
//! crcbl::adapter → (this crate)  which adapter inside that backend
//! crcbl::engine  → (this crate)  the shell↔HAL join every sample repeats
//! ```
//!
//! # One dependency is the whole point, and it took until S3 to mean it
//!
//! The list above used to stop at `crcbl::math`, and the six entries it had
//! were the graphics stack. Everything a *game* does that is not drawing — the
//! ECS, physics, the server and client, input, audio, saves, sprites — had no
//! re-export, so each of `apps/{breakout,flappy,asteroids,horde}` named eleven
//! workspace paths beside this one and `apps/sandbox` was the only consumer
//! shaped like the thing this crate promises.
//!
//! Nine of those crates are here now. **None of them depends on `crcbl`**,
//! which is what makes it nine `pub use` lines rather than a restructuring:
//! the dependency arrows all already pointed this way.
//!
//! # What is deliberately not here
//!
//! * **No backend *type*.** `crcbl-vk` is a dependency as of P1.1 — something
//!   has to be, or `backend::open` could not exist — but it is **not**
//!   re-exported, and no `VkInstance` is reachable from this crate's public
//!   API. `docs/plan/11-cli-headless.md` names "a sample linking `crcbl-vk`
//!   directly" as an architecture regression; the registry is what stops a
//!   sample needing to. `apps/sandbox` asks for
//!   [`GpuBackend::Vulkan`](backend::GpuBackend::Vulkan) by value and holds a
//!   `Box<dyn Instance>`. See [`backend`] for the full argument.
//! * **No engine loop.** There is no `crcbl::run(game)`. The loop shape is
//!   `fn tick(dt)` driven by an *outer* loop the host owns —
//!   `docs/plan/10-wasm-webgpu.md` requires it, because on wasm that outer loop
//!   is `requestAnimationFrame`, which calls the engine and cannot be called by
//!   it. `crcbl-shell`'s crate docs spell out the consequence: a
//!   framework-shaped `run()` would compile on wasm and deadlock on the first
//!   frame. `apps/sandbox` and the `crcbl new` template each own their loop,
//!   and each is short because owning it is cheap.
//!
//! # Stability
//!
//! Provisional, like both seams below it. The re-export list grows one line per
//! phase (`crcbl-render` at P1, `crcbl-ecs` / `crcbl-net` at P2, …); nothing
//! here is expected to be removed.

/// [`crcbl-assets`](crcbl_assets): the IO seam — [`AssetSource`], the
/// [`DirSource`] over a directory, and the registry above them.
///
/// Not optional and not behind `scene`, because it is what *any* content
/// reaches this engine through. It is also the argument `scene::import_gltf`
/// takes: without this line a caller outside the workspace could name that
/// function and not the `&dyn AssetSource` it wants, which is a public entry
/// point nothing could call.
///
/// [`AssetSource`]: crcbl_assets::AssetSource
/// [`DirSource`]: crcbl_assets::DirSource
pub use crcbl_assets as assets;
/// [`crcbl-audio`](crcbl_audio): the mixer, the sound bank, the output stream
/// and the spatial cue grammar.
pub use crcbl_audio as audio;
/// [`crcbl-client`](crcbl_client): the predicting half of the session —
/// reconciliation against the server's snapshots and interpolation between
/// them.
pub use crcbl_client as client;
/// [`crcbl-core`](crcbl_core): handles, `WorldPos`, the frame arena, the frame
/// clock, the input vocabulary and `SurfaceTarget`.
///
/// Named `core` at the cost of shadowing the `core` sysroot crate *inside this
/// crate only*, which is why this crate's own code uses `std::` paths
/// throughout. Consumers get the name they want: `crcbl::core::FrameClock`.
pub use crcbl_core as core;
/// [`crcbl_core::log`]: the engine's own logging — the five level macros, the
/// `CRCBL_LOG` filter, and the sink they reach.
///
/// `crcbl::log::info!(…)` is [`crcbl_core::info!`], which the module re-exports
/// beside the sink; [`core::log::init_logging`] installs that sink. This used to
/// re-export the `log` crate instead, and the call sites did not change when it
/// stopped, because the path is the same either way.
///
/// The `log` crate is still underneath, and deliberately: `wgpu`, `naga` and
/// `gpu-allocator` report through that facade, and the sink implements
/// `log::Log` so their diagnostics land in the same stream as the engine's
/// rather than nowhere.
pub use crcbl_core::log;
/// [`crcbl-ecs`](crcbl_ecs): the world, its components, its systems and the
/// schedule that runs them.
pub use crcbl_ecs as ecs;
/// [`crcbl-hal`](crcbl_hal): the GPU backend seam, and the recording
/// [`null`](crcbl_hal::null) backend standing in for one until P1.
pub use crcbl_hal as hal;
/// [`crcbl-input`](crcbl_input): action maps, bindings, and the button and axis
/// accessors a game polls once a tick.
pub use crcbl_input as input;
/// [`crcbl-jobs`](crcbl_jobs): the spawn seam, the two communication
/// primitives, and the work-stealing pool a game's data-parallel passes run on.
///
/// Re-exported for the same reason the rest are: a game that wants
/// [`Pool::par_for`](crcbl_jobs::pool::Pool::par_for) over its crowd should not
/// have to name a second workspace path to get it, and
/// [`default_spawner`](crcbl_jobs::default_spawner) is the one place the
/// browser-versus-native question is answered — a sample writing that `cfg`
/// itself is exactly what the seam exists to prevent.
pub use crcbl_jobs as jobs;
/// [`crcbl-net`](crcbl_net): the transport seam, the wire protocol and the
/// in-memory transport a single-player session is built on.
pub use crcbl_net as net;
/// [`crcbl-phys`](crcbl_phys): rigid bodies, colliders, forces, the broadphase
/// and the sweep queries.
pub use crcbl_phys as phys;
/// [`crcbl-render`](crcbl_render): the render graph, the transient pool, the
/// per-pass GPU timers, cameras, and the forward frame.
///
/// Everything above the seam. A game builds a
/// [`RenderGraph`](crcbl_render::RenderGraph) and never writes a barrier —
/// `docs/plan/02-vulkan-backend.md` §2.4's rule is "no manual barriers outside
/// the graph, ever", and this is the crate that makes it keepable.
pub use crcbl_render as render;
/// [`crcbl-scene`](crcbl_scene): glTF import, meshlet clustering, mesh
/// simplification and the cluster DAG.
///
/// **The bake side of [`render::scene`], and the only way
/// to reach it.** A [`Geometry::Flat`](crcbl_render::scene::Geometry::Flat)
/// carries a [`MeshClusters`](crcbl_shaders::meshlet::MeshClusters) and a
/// [`Geometry::Dag`](crcbl_render::scene::Geometry::Dag) a
/// [`ClusterDag`](crcbl_shaders::cluster_dag::ClusterDag); `crcbl-render` cannot
/// build either, because §3.5 makes the cluster build a bake step and the
/// renderer must not depend on this crate. So an application that describes its
/// own resident meshes calls
/// [`build_meshlets`](crcbl_scene::build_meshlets) and
/// [`MeshletBuild::into_clusters`](crcbl_scene::MeshletBuild::into_clusters), or
/// [`build_cluster_dag`](crcbl_scene::build_cluster_dag) and
/// [`ClusterDag::cook`](crcbl_scene::ClusterDag::cook), and hands the result to
/// [`ForwardRenderer::with_scene`](crcbl_render::ForwardRenderer::with_scene).
///
/// Behind the non-default `scene` feature, because this crate depends on `gltf`
/// and a game that ships cooked meshes links no parser for the source format —
/// the same split, for the same reason, as [`sprite`]'s `load` and `bake`.
#[cfg(feature = "scene")]
pub use crcbl_scene as scene;
/// [`crcbl-server`](crcbl_server): the authoritative simulation, its fixed
/// timestep and the state hash the determinism harness compares.
pub use crcbl_server as server;
/// [`crcbl-shaders`](crcbl_shaders): the engine's Slang sources and the SPIR-V
/// and WGSL compiled from them.
///
/// Re-exported so a game — and `apps/sandbox`, which is the same shape — reaches
/// the engine's own shaders without a second workspace path. It names no
/// backend and no seam type: it hands out `&[u32]`, `Option<&str>` and
/// entry-point names, which is exactly what [`hal::ShaderModuleDesc`] takes.
/// Pass **both** artifacts on every call — `crcbl-vk` reads the SPIR-V,
/// `crcbl-wgpu` reads the WGSL, and the caller does not choose.
pub use crcbl_shaders as shaders;
/// [`crcbl-shell`](crcbl_shell): the windowing seam, its Linux backends and
/// [`HeadlessShell`](crcbl_shell::HeadlessShell).
pub use crcbl_shell as shell;
/// [`crcbl-sprite`](crcbl_sprite): sheets, clips, playback, and the reader for
/// the PNG-and-sidecar pair a build script bakes.
///
/// The `load` half only. A build script that needs the *encoder* depends on
/// `crcbl-sprite` directly with its `bake` feature — cargo resolves
/// build-dependency features separately from these, so the encoder never
/// reaches a shipped binary.
pub use crcbl_sprite as sprite;
/// [`crcbl-store`](crcbl_store): platform-standard storage roots, atomic
/// writes, and the browser's `fetch` and OPFS backends.
pub use crcbl_store as store;
/// [`crcbl-webgpu`](crcbl_webgpu): the wasm → JS command stream — the encoding,
/// and the three `__crcbl_web_gpu_stream_*` exports a browser shim drains it
/// through.
///
/// **`wasm32` only**, like the dependency itself: the transport's whole purpose
/// is to hand a frame to JavaScript, and a native build has nothing to hand it
/// to. It is the one backend-adjacent crate this crate re-exports, and it is not
/// a backend — there is no `Instance` and no `Device` in it, only the encoding
/// and the seam, which is what keeps "no backend type is reachable from
/// `crcbl`" true above.
///
/// **The manifest entry is what carries the symbols; this line is what stops
/// that being luck.** The three exports in [`crcbl_webgpu::web::shim`] are
/// `#[unsafe(no_mangle)]`, and `web/tools/check-exports.mjs`'s own header states
/// the risk they run: a `no_mangle` symbol in a dependency rlib is not
/// *guaranteed* to survive into a `cdylib`. Measured on this toolchain it does
/// even with nothing referencing the crate — dropping this `pub use` and
/// rebuilding `crcbl_breakout.wasm` produces a byte-identical artifact, `cmp`
/// says so, and all three symbols are in both. So the re-export is not load
/// bearing today; it is here so the crate is genuinely named by something,
/// rather than the ABI resting on a linker behaviour nobody promised. Removing
/// the dependency line above *does* take all three symbols out of the artifact,
/// which is the state the gate found and this change fixes.
///
/// Nothing encodes into the stream yet — no HAL implementation writes through
/// [`StreamChannel::encode`](crcbl_webgpu::web::StreamChannel::encode) and
/// nothing calls [`install`](crcbl_webgpu::web::install) — so the exports answer
/// `0` on every frame today. That is the documented "a shim that started before
/// the engine did" case, not a failure, and it stays that way until the WebGPU
/// backend arrives to encode through it.
#[cfg(target_arch = "wasm32")]
pub use crcbl_webgpu as webgpu;
/// [`glam`]: the linear algebra `crcbl::render`'s cameras and transforms are
/// spelled in.
///
/// Re-exported rather than left to the caller because a game handing a
/// `Mat4` to [`render::ForwardRenderer::begin_frame`] has to be handing it *the
/// same* `Mat4` — two versions of glam in one binary is a type error whose
/// message names neither crate helpfully.
pub use glam as math;

/// [`crcbl-ui`](crcbl_ui): immediate-mode UI toolkit — draw lists, glyph atlas,
/// HUD skeleton, and widgets.
pub use crcbl_ui as ui;

pub mod adapter;

pub mod args;

pub mod backend;

pub mod engine;

pub mod perf;

pub mod session;

// Deliberately **not** gated to `wasm32`, even though only a browser build calls
// it: the log queue is plain Rust with no web dependency, and gating it would
// put its tests on the one target the test suite never runs. Each sample's
// `web.rs` is gated, which is where the wasm-only parts live.
pub mod web;

// Builds everywhere: its open and its readback are poll-based state machines
// ([`screenshot::PendingOffscreen`], [`screenshot::PendingReadback`]) that block
// nowhere, so a wasm build compiles the module rather than a hang. The blocking
// conveniences over them (`OffscreenSetup::open`, `draw_and_readback`) stay
// native-only inside the module, where their docs say why.
pub mod screenshot;

/// The names a game touches on every frame.
///
/// `use crcbl::prelude::*;` is the first line of the `crcbl new` template. It is
/// deliberately small — vocabulary and seam entry points, never a whole
/// namespace — so a glob import cannot collide with a game's own types.
pub mod prelude {
    pub use crate::backend::{GpuBackend, GpuError};
    pub use crcbl_core::time::{ManualTime, MonotonicTime, TimeSource};
    pub use crcbl_core::{FrameClock, SurfaceTarget};
    pub use crcbl_hal::null::NullInstance;
    pub use crcbl_hal::{Device, HalError, Instance, SurfaceError};
    pub use crcbl_render::{
        Camera, DirectionalLight, ForwardRenderer, GraphError, Projection, RenderGraph,
    };
    pub use crcbl_shell::{
        CloseReply, PhysicalSize, Shell, ShellBackend, ShellError, ShellEvent, WindowDesc, WindowId,
    };
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;

    /// A sample's whole dependency list, exercised through this crate.
    ///
    /// The nine simulation re-exports are what let `apps/*/Cargo.toml` name
    /// `crcbl` alone, and a `pub use` is not the kind of thing anyone notices
    /// going missing — nothing in this crate calls them. So each one is *used*
    /// here rather than merely mentioned: drop a line from the list above and
    /// this stops compiling.
    ///
    /// It builds a server and a client over one in-memory transport — the
    /// shape a single-player sample runs — because that is the one place
    /// several of the re-exports have to agree on a type. A
    /// [`net::InMemoryTransport`](crcbl_net::InMemoryTransport) handed to a
    /// server and a client that each named their own `crcbl-net` would not
    /// compile, which is the failure this is really guarding.
    #[test]
    fn the_umbrella_re_exports_a_whole_game_and_not_only_the_graphics_stack() {
        let (to_server, to_client) = crate::net::InMemoryTransport::pair();
        let tick_hz = 60;
        // Non-zero on both counts, because `assert_explicit` panics otherwise:
        // `ProtocolCompatibility::DEFAULT` is the placeholder and a session is
        // required to say who it is.
        let compatibility = crate::net::ProtocolCompatibility {
            engine_build_id: 1,
            schema_hash: 1,
            ..crate::net::ProtocolCompatibility::DEFAULT
        };

        let server = crate::server::Server::try_new_with_compatibility(
            crate::ecs::World::new(),
            to_server,
            tick_hz,
            compatibility,
        )
        .expect("the only fallible part is reading OS entropy for the resume token");
        let client = crate::client::Client::new_with_compatibility(
            crate::ecs::World::new(),
            to_client,
            tick_hz,
            compatibility,
        );
        assert_eq!(
            server.tick_id().get(),
            0,
            "a server that has not been updated has run no ticks"
        );
        assert_eq!(
            client.last_applied_tick().get(),
            0,
            "and its client has applied none"
        );

        // The rest of the list, one observable apiece.
        let mixer = crate::audio::mixer::Mixer::new();
        assert_eq!(mixer.voice_count(), 0, "a fresh mixer plays nothing");

        let mut actions = crate::input::ActionMap::new();
        actions.begin_tick(0.0);

        let body = crate::phys::RigidBody::default();
        assert_eq!(
            body.velocity,
            crate::math::DVec3::ZERO,
            "a default body is at rest"
        );

        let cell = crate::sprite::Rect::new(0, 0, 8, 8);
        assert_eq!(cell.w, 8, "a sheet cell keeps the width it was cut at");

        // `crcbl::log` is the facade itself, so the check is that its macros
        // expand through the re-export — a wrapper macro would be needed if
        // they did not, and this is what says one is not.
        crate::log::debug!("the umbrella's log re-export expands at {tick_hz} Hz");

        // `crcbl::store` names a root rather than writing to one: a unit test
        // that touched the user's real config directory would be a test with a
        // side effect.
        let _ = crate::store::NativeStorage::config("crcbl-umbrella-test");
    }

    /// The re-exports are this crate's entire contract, so the test that
    /// matters is that all three resolve, and that the type crossing the two
    /// seams is *one* type rather than two that happen to match.
    #[test]
    fn the_umbrella_re_exports_every_seam_a_game_needs() {
        let mut shell: Box<dyn Shell> = Box::new(crate::shell::HeadlessShell::new());
        assert_eq!(shell.backend(), ShellBackend::Headless);
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");

        let mut clock = FrameClock::new(60);
        // The first update only establishes the baseline — it covers no time,
        // so a loop's first frame legitimately runs zero ticks.
        clock.update(std::time::Duration::ZERO);
        clock.update(std::time::Duration::from_millis(100));
        assert!(clock.consume_tick());

        let instance: Box<dyn Instance> = Box::new(NullInstance::gpu_driven());
        let target: SurfaceTarget = shell
            .surface_target(window)
            .expect("the handle is live, and the target exists before configure");
        // The join: `crcbl::shell` produced it, `crcbl::hal` consumes it, and
        // neither crate names the other.
        //
        // SAFETY: the target came from a live headless window in this process,
        // it names no platform object at all (`Offscreen`), and it outlives the
        // surface — which is destroyed two lines down.
        let surface = unsafe { instance.create_surface(&target) }
            .expect("the null backend accepts every target");
        instance.destroy_surface(surface);
    }

    /// **An application reaches the cluster bake and makes its own mesh
    /// resident, naming this crate and nothing else.**
    ///
    /// The whole of what the `scene` feature is for, as one path. A
    /// [`Geometry::Flat`](crcbl_render::scene::Geometry::Flat) carries
    /// `MeshClusters`, and until `crcbl-scene` was reachable from here there was
    /// no way for an application to produce one: `crcbl-render` may not depend on
    /// the builder, and the clusters of every mesh the engine draws are either
    /// hardcoded in `crcbl-shaders` or cooked into `clusters/dunes.dag`. So this
    /// bakes its own — `build_meshlets` over positions and indices, through
    /// [`MeshletBuild::into_clusters`](crcbl_scene::MeshletBuild::into_clusters)
    /// — and hands it to the renderer.
    ///
    /// A **one-mesh, one-row** description on purpose: it is also what says the
    /// floor is gone. `with_scene` used to refuse anything shorter than the demo
    /// scene, because the five `set_*` wrappers named its meshes and its rows by
    /// position, so this description was unbuildable whatever the bake produced.
    #[cfg(feature = "scene")]
    #[test]
    fn an_application_bakes_its_own_mesh_and_the_renderer_makes_it_resident() {
        use crcbl_hal::{DeviceDesc, Format, QueueKind};
        use crcbl_render::scene::{Capacities, Geometry, MeshDesc, PageDesc, ProbeGrid, SceneDesc};
        use crcbl_shaders::mesh;
        use std::borrow::Cow;

        // An ordinary triangle list, the shape a glTF primitive arrives in.
        let vertices = mesh::pyramid_vertices();
        let indices = mesh::pyramid_indices();
        let positions: Vec<[f32; 3]> = vertices
            .iter()
            .map(|vertex| [vertex.position[0], vertex.position[1], vertex.position[2]])
            .collect();

        let build = crate::scene::build_meshlets(&positions, &indices)
            .expect("a whole number of triangles, every index inside the mesh");
        let cluster_count = build.clusters().len();
        assert!(
            cluster_count > 0,
            "a mesh of triangles must bake into at least one cluster, or the description \
             below would make a mesh nothing can draw resident"
        );
        let clusters = build.into_clusters();
        assert_eq!(
            clusters.clusters.len(),
            cluster_count,
            "into_clusters is a rename, so it must hand over every cluster the build grew"
        );

        let scene = SceneDesc {
            meshes: vec![MeshDesc {
                label: Cow::Borrowed("an application's own mesh"),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::pyramid_vertex_bytes()),
                    indices: Cow::Owned(indices),
                    clusters,
                },
            }],
            materials: vec![mesh::GpuMaterial::UNTINTED],
            page: PageDesc::opaque_white(1),
            probes: ProbeGrid::default(),
            capacities: Capacities::default(),
        };

        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let renderer =
            ForwardRenderer::with_scene(device.as_ref(), queue, Format::Rgba8UnormSrgb, &scene)
                .expect("a description of one baked mesh and one row is one the renderer can hold");
        renderer.destroy(device.as_ref());
    }
}
