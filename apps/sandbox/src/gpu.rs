//! Where the two seams meet: a `crcbl-shell` window becomes a `crcbl-hal`
//! surface, a swapchain, and — since P1.3 — a **render graph**.
//!
//! This module was the whole point of P0.7: `crcbl-shell` had been complete
//! since P0.6 and `crcbl-hal` since P0.3, but nothing had ever *joined* them,
//! and the join is where a seam mismatch shows up. P0.7 drove it against
//! [`NullBackend`](crcbl::hal::null); P1.1 drove the same code against real
//! Vulkan; P1.2 drew a triangle through it. **P1.3 hands the frame to
//! `crcbl-render`.**
//!
//! # The join itself lives in `crcbl::engine`
//!
//! Everything that is not specific to the sandbox — opening a backend, choosing
//! an adapter that can present, the swapchain, the frames-in-flight ring,
//! resize and teardown — moved to [`crcbl::engine::GpuContext`], which
//! `apps/breakout/src/gpu.rs` uses too. It was the same code twice, and only
//! this copy carried the rationale below, so the pair could only drift. Finding
//! 3 in the list at the end of these docs asked for exactly that move.
//!
//! # There are no barriers in this file
//!
//! There used to be two, hand-written, around the render pass:
//!
//! ```text
//! barrier Undefined → ColorAttachment
//! render pass: clear + triangle
//! barrier ColorAttachment → Present
//! ```
//!
//! `docs/plan/02-vulkan-backend.md` §2.4 says "**no manual barriers outside the
//! graph, ever**", and both of those are gone. What replaced them is a
//! *declaration* — the swapchain image is imported into the graph saying "it
//! arrives [`Undefined`](crcbl::hal::ResourceState::Undefined) and must leave
//! [`Present`](crcbl::hal::ResourceState::Present)" — and the graph computes the
//! rest, including the transition of the HDR scene target from a colour
//! attachment into a sampled texture, which the hand-written version never had
//! to think about because there was no second pass.
//!
//! The frame is now:
//!
//! ```text
//! acquire → build the graph → compile → execute (barriers computed)
//!         → submit(wait acquire, signal present + timeline)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! and the graph's own dump explains it — `CRCBL_LOG=debug` prints it once, and
//! once per resize.
//!
//! # HDR from P1
//!
//! The mesh is drawn into a transient `Rgba16Float` target with a `D32Float`
//! reversed-Z depth buffer, and a second pass tonemaps that into the swapchain.
//! `docs/plan/ROADMAP.md`'s correction asks for exactly that from the first lit
//! mesh, "even with no HDR content", so P7's real stack does not re-bless every
//! golden image in the repository. Both targets are graph transients: this file
//! never names an image, a view or a size for either.
//!
//! # Frames in flight, not `wait_idle`
//!
//! [`Device::destroy_command_buffer`] may not be called until the submission
//! that used it has completed, and the seam offers exactly two ways to know
//! that: a timeline semaphore, or [`Device::wait_idle`] — which the seam itself
//! documents as "a shutdown and test primitive" that "destroys pipelining". So
//! this keeps a two-deep ring keyed on a timeline semaphore value, and falls
//! back to `wait_idle` only on a device that has no timeline semaphores.
//!
//! # What the join revealed
//!
//! P0.7 was the first time anything drove both seams at once, and
//! `docs/plan/01-foundations.md` freezes neither at P0. The findings are kept
//! here because this is where they were found.
//!
//! 1. **Two sources of truth for the swapchain extent, with no stated
//!    precedence** — *fixed in the seam.*
//!    [`WindowState::size`](crcbl::shell::WindowState::size) is one;
//!    [`SurfaceCaps::current_extent`](crcbl::hal::SurfaceCaps::current_extent)
//!    is the other, and on Vulkan it is a real size on X11 and deliberately
//!    `0xFFFFFFFF` ("you choose") on Wayland. `crcbl-hal`'s
//!    [`swapchain`](crcbl::hal::swapchain) module now states the rule as four
//!    numbered backend obligations, and [`Gpu::open`] is the reference
//!    implementation of the caller's half.
//! 2. **[`SurfaceTarget::Offscreen`](crcbl::core::SurfaceTarget) embedded a
//!    size, so a headless target went stale on resize** — *fixed by deleting
//!    the size.* [`Gpu::resize`] therefore reconfigures the swapchain and
//!    nothing else, on every backend.
//! 3. **`unsafe` at the join is unavoidable and lands in application code** —
//!    *fixed above the seam.* [`Instance::create_surface`] is `unsafe` because
//!    it dereferences platform handles, and the safety obligation ("these
//!    outlive the surface") is one only the code holding *both* the shell and
//!    the device can discharge. P1.3 predicted the home — "an engine-setup
//!    helper in the `crcbl` umbrella, where both seams already meet" — and
//!    that is [`GpuContext`], which this file now opens. No crate under
//!    `apps/` contains an `unsafe` block.
//! 4. **Teardown order is stated in three places and enforced in none.** The
//!    swapchain must die before the surface, the surface before the window, and
//!    the device may outlive its instance. Still a convention rather than a
//!    type, but it is now hand-written once — in `GpuContext::destroy` — rather
//!    than once per sample.
//! 5. **The swapchain's configured extent was unobservable** — *fixed in the
//!    seam*, `AcquiredFrame::extent`.
//! 6. **A render pass needed a view the seam would not give it** — *fixed in
//!    the seam*, `AcquiredFrame::view`.

pub use crcbl::engine::{FrameOutcome, GpuError};

use std::time::{Duration, Instant};

use crcbl::engine::{GpuContext, GpuContextDesc, GpuOptions, Pacing};
use crcbl::hal::CommandEncoderDesc;
use crcbl::prelude::*;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers,
    RenderGraph, TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// The sandbox's GPU side: the shared join plus the milestone 3–5 renderer.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the camera is and how it projects. Milestone 5 is a write to
    /// `camera.projection` and nothing else.
    pub camera: Camera,
    /// The single directional light of milestone 4.
    pub light: DirectionalLight,
    /// UI compositing, for the debug overlay.
    ///
    /// The sandbox has no game HUD and is not getting one — it is a milestone
    /// harness, not a game. It has a UI pass because
    /// `docs/plan/sample/00-samples-overview.md` rule 4 applies to it too, and
    /// a sample that cannot turn the panel on is a finding about the panel.
    ui: UiRenderer,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    /// Seconds of animation, advanced by the loop rather than read from a clock
    /// here — a headless run must produce the same picture on every machine.
    elapsed: f32,
    /// Whether the graph dump has been logged since the last shape change.
    dumped: bool,
    /// The last frame's graph dump, kept only for the loop's own tests: it is
    /// how a test sees whether the UI pass was in the frame at all. `add_pass`
    /// declares nothing when the draw list is empty, so the pass's presence in
    /// this string *is* "the overlay reached the GPU".
    #[cfg(test)]
    last_dump: String,
}

impl Gpu {
    /// Opens the join and builds the forward renderer on top of it.
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, or if any HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        projection: crcbl::render::Projection,
    ) -> Result<Self, GpuError> {
        let ctx = GpuContext::open(
            shell,
            window,
            extent,
            &GpuContextDesc {
                label: "sandbox",
                ..GpuContextDesc::from(gpu)
            },
        )?;

        // Milestones 3–5. Built after the swapchain because the tonemap
        // pipeline has to name the colour format the pass will render to.
        let renderer = ForwardRenderer::new(ctx.device(), ctx.queue(), ctx.format())?;
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        // Rolled back by hand: `Gpu` has no `Drop`, so a `?` here would leak the
        // forward renderer's pipelines rather than release them.
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(menu) => menu,
            Err(error) => {
                ui.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            timers,
            camera: Camera::default().with_projection(projection),
            light: DirectionalLight::default(),
            ui,
            menu,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            elapsed: 0.0,
            dumped: false,
            #[cfg(test)]
            last_dump: String::new(),
        })
    }

    /// The swapchain's current size — the one it was **configured** at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// The format the swapchain was created with. Test-only.
    #[cfg(test)]
    pub const fn format(&self) -> crcbl::hal::Format {
        self.ctx.format()
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    ///
    /// Empty on a device with no timestamp queries, and empty for the first few
    /// frames — the report is deliberately frames latent; see
    /// [`crcbl::render::PassTimers`].
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded: draws, instances and triangles,
    /// summed over the three passes this bundle adds.
    ///
    /// Two of the four are `indirect` here and stay that way on every geometry
    /// path — the forward renderer draws through arguments the GPU wrote. See
    /// [`crcbl::render::ForwardRenderer::counters`].
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
            .plus(self.menu.counters())
            .plus(self.ui.counters())
    }

    /// Takes this frame's UI geometry, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// Takes this frame's menu, or `None` on a frame that shows none.
    ///
    /// CPU only — the upload happens inside [`Gpu::frame`], at the extent the
    /// swapchain was actually acquired at.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// The sprites the menu pass will draw this frame, for the loop's own tests.
    #[cfg(test)]
    pub fn menu_sprites(&self) -> &[crcbl::render::Sprite] {
        self.menu.frame_sprites()
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The debug overlay measures its own panel with it, and must measure with
    /// the *same* atlas the pass draws with or the background rect is the wrong
    /// size for the text inside it.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The UI geometry this frame handed over, for the loop's own tests.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The last frame's render-graph dump, for the loop's own tests.
    #[cfg(test)]
    pub fn last_dump(&self) -> &str {
        &self.last_dump
    }

    /// Seconds of animation this frame will draw.
    ///
    /// Test-only, and the strongest thing the sandbox has to say "the
    /// simulation did not advance": the cube's angle is a pure function of it,
    /// so a single tick that slipped through a pause changes it.
    #[cfg(test)]
    pub const fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Advances the animation by `dt` seconds.
    ///
    /// Driven by the loop's clock rather than read from one here, so a headless
    /// run renders the same cube on every machine — which is what makes a
    /// golden image of it worth anything.
    pub const fn advance(&mut self, dt: f32) {
        self.elapsed += dt;
    }

    /// Builds this frame's graph, compiles it, executes it, submits and
    /// presents.
    ///
    /// **The whole frame, and not one barrier in it.**
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is reported as [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let Some(acquired) = self.ctx.acquire()? else {
            // The graph's shape changed, so the dump is worth printing again.
            self.dumped = false;
            return Ok(FrameOutcome::Reconfigured);
        };
        let extent = acquired.extent;

        self.renderer.begin_frame(
            self.ctx.device(),
            &self.camera,
            &self.light,
            ForwardRenderer::spin(self.elapsed),
            extent,
        )?;
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
        // Upload this frame's UI geometry: the debug overlay, and only that.
        self.ui
            .begin_frame(self.ctx.device(), &self.draw_list, &self.atlas, 1.0)
            .map_err(GpuError::Hal)?;

        let format = self.ctx.format();
        let compiled = {
            let mut graph = RenderGraph::new(self.ctx.queue());
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, format, extent),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            // **Between the scene and the text, and that order is the whole
            // join.** The menu's scrim dims what is already in the target, so it
            // has to come after the tonemap; the panel is opaque and the labels
            // are UI-pass text, so it has to come before the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
            // Composited on top of the tonemapped scene, so the overlay is
            // readable over whatever the frame drew.
            self.ui.add_pass(&mut graph, target, extent);
            // The pool is what remembers the previous frame, so the barriers
            // that open this one are ordered against it rather than against
            // nothing.
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle. Once per shape rather than once per frame, because a dump
        // every frame is a log nobody reads.
        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the sandbox frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("sandbox frame"),
                queue: self.ctx.queue(),
            });
        compiled.execute(
            self.ctx.device(),
            &mut self.pool,
            encoder.as_mut(),
            self.timers.as_mut(),
        )?;
        let command_buffer = encoder.finish()?;

        let outcome = self.ctx.submit_and_present(&acquired, command_buffer)?;
        // Only after the submit's retire, so nothing the pool destroys can
        // still be referenced by a submission that has not completed.
        self.pool.retire_unused(self.ctx.device());
        if outcome == FrameOutcome::Reconfigured {
            self.dumped = false;
        }
        Ok(outcome)
    }

    /// Resizes the swapchain to `extent`.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error
    /// — a minimized window reports one, and the swapchain is simply left
    /// alone.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        self.ctx.resize(extent)?;
        self.dumped = false;
        Ok(())
    }

    /// Changes how presented frames are paced, mid-run — the settings
    /// screen's half of the pair [`GpuContext::set_pacing`] documents.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the swapchain could not be rebuilt; the old one stays.
    pub fn set_pacing(&mut self, pacing: Pacing) -> Result<(), GpuError> {
        self.ctx.set_pacing(pacing)
    }

    /// How long a wait for a present id that was never given may block before
    /// the run is judged to have lost the id guard. Far above the instant an
    /// intact guard answers; a lost guard runs the whole thing out, which is
    /// what makes the wayland e2e fail loudly rather than pass slowly.
    const UNPRESENTED_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

    /// Waits for a present id this swapchain was never given, and returns how
    /// long the device took to say there was nothing to wait for.
    ///
    /// The `--wait-unpresented` probe. On a device with present feedback the
    /// only thing that makes this answer at once is the "id the swapchain was
    /// never given" guard in `crcbl-vk`, so the elapsed time is the observable
    /// that says the guard is still there. A device without the capability
    /// answers immediately without consulting it, which is why the e2e run
    /// prints which case it was in.
    pub fn wait_unpresented(&mut self) -> Result<Duration, crcbl::hal::SurfaceError> {
        let started = Instant::now();
        self.ctx.device().wait_until_presented(
            self.ctx.swapchain(),
            u64::MAX,
            Self::UNPRESENTED_WAIT_TIMEOUT,
        )?;
        Ok(started.elapsed())
    }

    /// The pacing asked for — what [`set_pacing`](Self::set_pacing) last
    /// received, before resolution against the display. Test-only.
    #[cfg(test)]
    #[must_use]
    pub const fn pacing(&self) -> Pacing {
        self.ctx.pacing()
    }

    /// Tears everything down in the order the seam requires.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        // Nothing may be destroyed while the device might still be using it.
        self.ctx.drain()?;
        self.ui.destroy(self.ctx.device());
        self.menu.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

/// The frame's half of this bundle, for [`crcbl::engine::Loop`].
///
/// Six one-line forwards. Every one already existed for the loop that used to
/// call them from `app.rs`; the trait is what lets the engine call them instead.
impl crcbl::engine::GameGpu for Gpu {
    fn atlas(&self) -> &FontAtlas {
        Self::atlas(self)
    }

    fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        Self::set_menu(self, menu);
    }

    fn take_draw_list(&mut self, list: &mut DrawList) {
        Self::take_draw_list(self, list);
    }

    fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        Self::timings(self)
    }

    fn counters(&self) -> crcbl::render::FrameCounters {
        Self::counters(self)
    }

    fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        Self::frame(self)
    }

    fn destroy(self) -> Result<(), GpuError> {
        Self::destroy(self)
    }
}

/// The two questions both halves of the engine ask a swapchain's owner.
///
/// The sandbox has no browser build and so no `PolledGpu` impl; these two are
/// the half a running loop needs and are declared once for both.
impl crcbl::engine::GpuSurface for Gpu {
    fn extent(&self) -> (u32, u32) {
        Self::extent(self)
    }

    fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        Self::resize(self, extent)
    }
}
