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
//! crcbl::shaders → crcbl-shaders the engine's shaders, as SPIR-V
//! crcbl::math    → glam          the maths the renderer's types are spelled in
//! crcbl::backend → (this crate)  runtime GPU backend selection
//! ```
//!
//! # What is deliberately not here
//!
//! * **No backend *type*.** `crcbl-vk` is a dependency as of P1.1 — something
//!   has to be, or [`backend::open`] could not exist — but it is **not**
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

/// [`crcbl-core`](crcbl_core): handles, `WorldPos`, the frame arena, the frame
/// clock, the input vocabulary and `SurfaceTarget`.
///
/// Named `core` at the cost of shadowing the `core` sysroot crate *inside this
/// crate only*, which is why this crate's own code uses `std::` paths
/// throughout. Consumers get the name they want: `crcbl::core::FrameClock`.
pub use crcbl_core as core;
/// [`crcbl-hal`](crcbl_hal): the GPU backend seam, and the recording
/// [`null`](crcbl_hal::null) backend standing in for one until P1.
pub use crcbl_hal as hal;
/// [`crcbl-render`](crcbl_render): the render graph, the transient pool, the
/// per-pass GPU timers, cameras, and the forward frame.
///
/// Everything above the seam. A game builds a
/// [`RenderGraph`](crcbl_render::RenderGraph) and never writes a barrier —
/// `docs/plan/02-vulkan-backend.md` §2.4's rule is "no manual barriers outside
/// the graph, ever", and this is the crate that makes it keepable.
pub use crcbl_render as render;
/// [`crcbl-shaders`](crcbl_shaders): the engine's Slang sources and the SPIR-V
/// compiled from them.
///
/// Re-exported so a game — and `apps/sandbox`, which is the same shape — reaches
/// the engine's own shaders without a second workspace path. It names no
/// backend and no seam type: it hands out `&[u32]` and entry-point names, which
/// is exactly what [`hal::ShaderModuleDesc`] takes.
pub use crcbl_shaders as shaders;
/// [`crcbl-shell`](crcbl_shell): the windowing seam, its Linux backends and
/// [`HeadlessShell`](crcbl_shell::HeadlessShell).
pub use crcbl_shell as shell;
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

pub mod backend;

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

        let instance: Box<dyn Instance> = Box::new(NullInstance::tier_a());
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
}
