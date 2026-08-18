//! Start-up, the loop, and teardown.
//!
//! ```text
//! Viewer::frame()
//!   wait_events, pump ──┬─▶ Pending::observe   (close, resize, destroy)
//!                       └─▶ Controls::apply    (orbit, pan, zoom, F)
//!   gpu.set_camera(orbit.camera())
//!   gpu.frame()
//! ```
//!
//! # The loop is hand-written, and that is not a preference
//!
//! Every game in this tree hands its frame to [`crcbl::engine::Loop`], which is
//! the right default and the one `crcbl new` scaffolds. It cannot host this
//! application: the loop reduces a pump to [`crcbl::engine::PointerUpdate`] —
//! a position and the primary button's two edges — and a scroll wheel reaches
//! no hook at all. A model viewer without wheel zoom and without a second drag
//! button is not a model viewer, so the frame is taken back here, exactly as
//! `apps/bare`'s docs say a caller may: "this type is the default, not the toll
//! gate".
//!
//! What that costs is written down rather than hidden: there is no menu, no
//! pause and no debug overlay in this application, because those are the loop's
//! and this file does not reimplement them. `docs/backlog.md` carries the
//! engine-side alternative — widening the hosted loop's input so a viewer could
//! use it — as the thing that would delete this argument.
//!
//! # It simulates nothing, and that is the charter exception
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 makes every sample
//! client/server authoritative. `docs/plan/sample/05-viewer.md` names this
//! sample as the one sanctioned exception: rule 2 exists so a *game*'s state
//! lives on the server, and there is no state here — the file is on disk, the
//! camera is the user's, and nothing else changes. So there is no tick, no
//! `FrameClock` and no `GameModule` below, and their absence is the exception
//! rather than an oversight.

use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, LoopError, ModeRequest, Pending, WINDOWED_IDLE,
    accept_close, open_shell, open_window, requested_window_size,
};
use crcbl::prelude::*;
use crcbl::render::OrbitCamera;
use crcbl::render::cull::Aabb;
use crcbl::scene::gltf_render::Skip;
use crcbl::shell::DisplayMode;

use crate::args::Options;
use crate::controls::{Controls, Request};
use crate::gpu::Gpu;
use crate::model::{self, LoadError, Model};

/// What a finished run reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Which shell backend ran.
    pub backend: ShellBackend,
    /// Frames presented.
    pub frames: u64,
    /// Shell events observed, start-up included.
    pub events: u64,
    /// The swapchain's size when the loop stopped.
    pub extent: (u32, u32),
    /// The mode the window system actually had the window in, **not** the one
    /// `--fullscreen` asked for. It is free to refuse.
    pub mode: DisplayMode,
    /// Why it stopped.
    pub exit: ExitReason,
    /// How many instances the document placed.
    ///
    /// Zero would be a run that presented empty frames, which is the one
    /// failure a headless smoke test could otherwise report as a pass.
    pub instances: usize,
    /// How many glTF features the conversion could not honour.
    ///
    /// Every one of them was printed on stderr at start-up and logged at
    /// warning level by the conversion itself; the count is here so a run over
    /// a directory of models can be graded without reading the log.
    pub skipped: usize,
}

/// What can stop the viewer: the file, or everything below it.
///
/// **Its own type rather than [`LoopError<LoadError>`](LoopError)**, which is
/// what every other sample uses. That alias prefixes a game's own error with
/// `game error:` — right for a game, and here it would put the word "game" in
/// front of the one message this application exists to print well, in a tool
/// that is not a game and whose user is not a developer. `viewer: model.glb:
/// not a file` is the line; nothing is gained by decorating it.
#[derive(Debug)]
pub enum ViewerError {
    /// The document would not open — see [`LoadError`].
    Load(LoadError),
    /// The window system, the window, or the device refused.
    Engine(LoopError),
}

impl std::fmt::Display for ViewerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(error) => write!(f, "{error}"),
            Self::Engine(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ViewerError {}

impl From<LoadError> for ViewerError {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

impl From<LoopError> for ViewerError {
    fn from(error: LoopError) -> Self {
        Self::Engine(error)
    }
}

impl From<crcbl::engine::GpuError> for ViewerError {
    fn from(error: crcbl::engine::GpuError) -> Self {
        Self::Engine(LoopError::Gpu(error))
    }
}

impl From<crcbl::engine::ConfigureError> for ViewerError {
    fn from(error: crcbl::engine::ConfigureError) -> Self {
        Self::Engine(LoopError::Configure(error))
    }
}

impl From<ShellError> for ViewerError {
    fn from(error: ShellError) -> Self {
        Self::Engine(LoopError::Shell(error))
    }
}

/// The viewer, and everything one turn of its loop needs.
#[derive(Debug)]
pub struct Viewer<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: Gpu,
    /// The turntable. Not a [`Camera`]: an eye and a target integrated directly
    /// drift apart, and "orbit" would depend on how far away the target
    /// happened to be — see [`OrbitCamera`].
    orbit: OrbitCamera,
    controls: Controls,
    /// What the model occupies, kept so `F` can frame it again after the user
    /// has zoomed away.
    bounds: Aabb,
    clock_source: Clock,
    budget: FrameBudget,
    events: u64,
    windowed: bool,
    /// What the window system was last seen doing with the display mode.
    ///
    /// A hand-rolled loop has to carry this: accepting a close request destroys
    /// the window, so a summary built afterwards has nothing left to read the
    /// mode off. See [`ModeRequest::mode_at_exit`].
    mode: ModeRequest,
    instances: usize,
    skipped: usize,
}

/// Loads the model, opens a window and a device, and runs until something stops
/// it.
///
/// # Errors
///
/// [`ViewerError`] from the load, from start-up, from the frame that failed, or
/// from teardown.
pub fn run(options: &Options) -> Result<Summary, ViewerError> {
    let mut viewer = Viewer::start(options)?;
    let outcome = loop {
        match viewer.frame() {
            Ok(Flow::Continue) => {}
            Ok(Flow::Stop(reason)) => break Ok(reason),
            Err(error) => break Err(error),
        }
    };
    match outcome {
        Ok(reason) => viewer.finish(reason),
        Err(error) => {
            if let Err(teardown) = viewer.finish(ExitReason::Failed) {
                crcbl::log::error!("teardown after a failed frame also failed: {teardown}");
            }
            Err(error)
        }
    }
}

impl Viewer<dyn Shell> {
    /// Reads the model, then opens a shell, a window and a device for it.
    ///
    /// **The file first.** A bad path is the most likely way this application
    /// fails and it is the one a user can fix, so it is reported before a
    /// window flashes up and disappears.
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if the document would not load or the window system,
    /// window or device refused.
    pub fn start(options: &Options) -> Result<Self, ViewerError> {
        let model = load_and_report(options)?;
        let shell = open_shell(options.common.headless)?;
        Self::with_shell(shell, options, model)
    }
}

impl<S: Shell + ?Sized> Viewer<S> {
    /// Builds the loop on an already-open shell and an already-loaded model.
    ///
    /// Separate from [`Viewer::start`] so a test can play compositor on a
    /// concrete [`HeadlessShell`](crcbl::shell::HeadlessShell) — the same split
    /// every sample has, and here it is the only way to script a drag.
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if the window never configured or the device would not
    /// open.
    pub fn with_shell(
        mut shell: Box<S>,
        options: &Options,
        model: Model,
    ) -> Result<Self, ViewerError> {
        let mut clock_source = Clock::new(options.common.headless);
        clock_source.set_limit(options.common.limit);
        let window = open_window(
            shell.as_mut(),
            &clock_source,
            &WindowDesc {
                title: "crcbl — viewer",
                app_id: "sh.kryptic.crcbl.viewer",
                size: requested_window_size(options.common.size),
                // Asked for at creation rather than switched to afterwards, so
                // `--fullscreen` does not show a decorated window first.
                mode: options.common.display_mode(),
                ..WindowDesc::default()
            },
        )?;

        let mut events = 0;
        let extent = crcbl::engine::wait_for_configure(shell.as_mut(), window, &mut events)?;
        let gpu = Gpu::open(
            shell.as_ref(),
            window,
            extent,
            options.common.gpu(),
            &model.render.scene,
            &model.render.instances,
        )?;

        // Frame on load, against the extent the window actually configured at:
        // an aspect guessed from the requested size would frame a model that
        // hangs off the sides of the window it is really in.
        let mut orbit = OrbitCamera::new(model.bounds.center(), 1.0, Projection::default());
        orbit.frame(model.bounds, aspect_of(extent));

        Ok(Self {
            windowed: !options.common.headless,
            shell,
            window,
            gpu,
            orbit,
            controls: Controls::new(),
            bounds: model.bounds,
            clock_source,
            budget: FrameBudget::new(options.common.frame_budget()),
            events,
            mode: ModeRequest::new(),
            instances: model.render.instances.len(),
            skipped: model.render.skipped.len(),
        })
    }

    /// The swapchain's current size.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// Where the frame is drawn from.
    #[must_use]
    pub fn camera(&self) -> Camera {
        self.orbit.camera()
    }

    /// The shell, for a test playing compositor.
    #[cfg(test)]
    pub fn shell_mut(&mut self) -> &mut S {
        self.shell.as_mut()
    }

    /// The one window.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.window
    }

    /// One turn: pump, route the input, draw, present.
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if the shell or the device refused something.
    pub fn frame(&mut self) -> Result<Flow, ViewerError> {
        if self.budget.is_spent() {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }
        if self.windowed {
            // A viewer is idle whenever nobody is dragging it, so the loop
            // waits for an event rather than spinning a GPU at the display's
            // rate to redraw a still picture.
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let mut pending = Pending::default();
        let extent = self.gpu.extent();
        let (controls, orbit) = (&mut self.controls, &mut self.orbit);
        let mut reframe = false;
        self.shell.pump(&mut |event| {
            // The window's half — close, resize, destroy — and this
            // application's half, which is everything the hosted loop drops.
            let _ = pending.observe(&event);
            if controls.apply(&event, extent, orbit) == Request::Reframe {
                reframe = true;
            }
        });
        self.events += pending.count;
        // Before the close below, which destroys the window: this is the only
        // place the mode can still be read.
        self.mode.check(&*self.shell, self.window);

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            accept_close(self.shell.as_mut(), self.window)?;
            return Ok(Flow::Stop(ExitReason::CloseRequested));
        }
        if let Some(size) = pending.resized {
            self.gpu.resize((size.width, size.height))?;
        }
        if reframe {
            // Against the extent *after* the resize above, so a re-frame on the
            // same frame as a window resize fits the window it is about to be
            // drawn in.
            self.orbit.frame(self.bounds, aspect_of(self.gpu.extent()));
        }

        // Sleeps if `--fps` says the frame is early, and is the only thing this
        // loop wants a clock for.
        let _ = self.clock_source.advance();

        self.gpu.set_camera(self.orbit.camera());
        let outcome = self.gpu.frame()?;
        self.budget.record(outcome)?;
        Ok(Flow::Continue)
    }

    /// Releases the device, then the window.
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if either refused. Both are attempted regardless,
    /// because a leaked device is worse than a lost error.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, ViewerError> {
        let summary = Summary {
            backend: self.shell.backend(),
            frames: self.budget.presented(),
            events: self.events,
            extent: self.gpu.extent(),
            mode: self.mode.mode_at_exit(&*self.shell, self.window),
            exit,
            instances: self.instances,
            skipped: self.skipped,
        };
        let gpu_result = self.gpu.destroy();
        let shell_result = if exit.window_survives() {
            self.shell.destroy_window(self.window)
        } else {
            Ok(())
        };
        gpu_result?;
        shell_result?;
        Ok(summary)
    }
}

/// Loads the model and puts everything the conversion could not do in front of
/// the user.
///
/// **Skips are printed, not only logged.** `docs/plan/sample/05-viewer.md`'s
/// exit criterion is that a file nobody curated either loads or says why not,
/// naming the file, the feature and the reason. The conversion already logs
/// each one at warning level, which under the default `CRCBL_LOG` filter a user
/// never sees — so they are written to stderr as well, where the person who
/// opened the file is looking.
///
/// # Errors
///
/// [`ViewerError::Game`] wrapping the [`LoadError`], or a document nothing at
/// all could be made of — which is not a [`LoadError`] from the conversion,
/// because the conversion is infallible by design and reports a full list of
/// reasons instead.
fn load_and_report(options: &Options) -> Result<Model, ViewerError> {
    let model = model::load(&options.model)?;
    let drew_nothing = model.render.instances.is_empty();
    for line in skip_report(
        &model.key.display().to_string(),
        model.skipped(),
        drew_nothing,
    ) {
        eprintln!("{line}");
    }
    if drew_nothing {
        return Err(LoadError::NoGeometry(options.model.clone()).into());
    }
    Ok(model)
}

/// The lines [`load_and_report`] prints, built rather than printed.
///
/// **Separated so the loudness is testable.** The sample's exit criteria ask
/// that a skipped feature produce an actionable message naming the file, the
/// feature and the reason — and an `eprintln!` inline in the loader is a claim
/// no test can observe. Silencing the reporting left all of this crate's tests
/// green, which is the failure this split closes: a line per skip, then one
/// summary line saying whether what is on screen is the rest of the document or
/// none of it.
fn skip_report(key: &str, skipped: &[Skip], drew_nothing: bool) -> Vec<String> {
    let mut lines: Vec<String> = skipped
        .iter()
        .map(|skip| format!("viewer: {key}: {skip}"))
        .collect();
    if drew_nothing {
        lines.push(format!(
            "viewer: {key}: nothing in this document could be converted; \
             {} feature(s) were skipped, listed above",
            skipped.len(),
        ));
    } else if !skipped.is_empty() {
        lines.push(format!(
            "viewer: {key}: {} of this document's features were skipped; \
             what is on screen is the rest",
            skipped.len(),
        ));
    }
    lines
}

/// A viewport's aspect ratio, guarding the one extent a window system reports
/// that has no ratio.
///
/// A minimised window is zero in either dimension, and
/// [`OrbitCamera::frame`] asserts a finite positive aspect — so framing while
/// minimised would take the process down. One is the same fallback
/// [`ForwardRenderer::begin_frame`] uses for the same extent.
fn aspect_of(extent: (u32, u32)) -> f32 {
    if extent.0 == 0 || extent.1 == 0 {
        return 1.0;
    }
    extent.0 as f32 / extent.1 as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::args::Common;

    /// A skip whose three fields are distinguishable in a message.
    fn skip(feature: &'static str, at: &str, why: &str) -> Skip {
        Skip {
            feature,
            at: at.to_string(),
            why: why.to_string(),
        }
    }

    /// **Every skipped feature reaches the person who opened the file.**
    ///
    /// `docs/plan/sample/05-viewer.md`'s exit criteria ask for an actionable
    /// message naming the file, the feature and the reason, and the conversion
    /// logs each one at a level the default `CRCBL_LOG` filter hides — so
    /// stderr is where a user actually sees them.
    ///
    /// This test exists because silencing the reporting left every other test
    /// in this crate green: the printing was an `eprintln!` inline in the
    /// loader, which nothing could observe. It asserts a line *per* skip rather
    /// than that some line appeared, so dropping one of several still fails.
    #[test]
    fn every_skip_is_named_on_its_own_line() {
        let skips = [
            skip("scale", "node 2 \"plate\"", "scales axes unequally"),
            skip("mode", "mesh 0 primitive 1", "TRIANGLE_FAN is not drawn"),
        ];
        let lines = skip_report("blocks.glb", &skips, false);
        assert_eq!(
            lines.len(),
            skips.len() + 1,
            "a line per skip and one summary: {lines:#?}"
        );
        for skip in &skips {
            assert!(
                lines.iter().any(|line| line.contains(skip.feature)
                    && line.contains(&skip.at)
                    && line.contains(&skip.why)
                    && line.contains("blocks.glb")),
                "no line named {} at {} — a message missing the file, the feature or the \
                 reason is not actionable: {lines:#?}",
                skip.feature,
                skip.at,
            );
        }
        assert!(
            lines.last().is_some_and(|line| line.contains("the rest")),
            "the summary must say what is on screen is the rest of the document: {lines:#?}"
        );
    }

    /// A document nothing could be converted from says so, rather than opening
    /// an empty window with a warning nobody reads.
    #[test]
    fn a_document_that_converted_to_nothing_says_so() {
        let skips = [skip("mode", "mesh 0 primitive 0", "POINTS is not drawn")];
        let lines = skip_report("points.glb", &skips, true);
        assert!(
            lines
                .last()
                .is_some_and(|line| line.contains("nothing in this document could be converted")),
            "{lines:#?}"
        );
    }

    /// A clean document prints nothing at all, so the loudness above cannot be
    /// satisfied by a message that always appears.
    #[test]
    fn a_document_with_nothing_skipped_is_silent() {
        assert!(skip_report("clean.glb", &[], false).is_empty());
    }

    use crcbl::core::input::PointerButton;
    use crcbl::math::Vec3;
    use crcbl::shell::{HeadlessShell, PhysicalPoint};

    use crate::fixture;

    /// A directory holding one `.glb`, and the options that open it.
    ///
    /// The directory has to outlive the run, so it is returned beside the
    /// options rather than dropped at the end of this function — a `TempDir`
    /// deletes its tree when it goes.
    fn model_at(document: &[u8], frames: u64) -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("panel.glb");
        std::fs::write(&path, document).expect("the document");
        let mut common = Common::new(crate::args::DEFAULT_TICK_HZ);
        common.headless = true;
        common.frames = Some(frames);
        common.backend = Some(GpuBackend::Null);
        (
            dir,
            Options {
                common,
                model: path,
            },
        )
    }

    fn scripted(options: &Options) -> Viewer<HeadlessShell> {
        let model = model::load(&options.model).expect("the fixture loads");
        Viewer::with_shell(Box::new(HeadlessShell::new()), options, model)
            .expect("headless always starts")
    }

    /// **The whole path runs**: a `.glb` on disk, through the asset seam, the
    /// converter and the renderer, presenting frames and tearing down.
    ///
    /// The end-to-end claim this application exists to make, on the null
    /// backend so it runs on a machine with no GPU. `instances` is what
    /// separates it from a run that presented blank frames.
    #[test]
    fn a_headless_run_draws_the_document_and_stops() {
        let (_dir, options) = model_at(&fixture::quad_glb(fixture::QUAD_CENTRE), 8);
        let summary = run(&options).expect("the null backend runs everywhere");
        assert_eq!(summary.frames, 8);
        assert_eq!(summary.exit, ExitReason::FrameBudget);
        assert_eq!(summary.backend, ShellBackend::Headless);
        assert_eq!(summary.instances, 1, "the document's one node was placed");
        assert_eq!(summary.skipped, 0, "nothing about the fixture is skipped");
        assert!(summary.extent.0 > 0 && summary.extent.1 > 0);
    }

    /// **Frame-on-load frames the model**, wherever the document put it.
    ///
    /// Two claims, and both are needed: the camera looks at the model's centre,
    /// and it stands far enough back that the whole of it is inside the view
    /// cone. A camera that merely pointed at the right place would pass an
    /// assertion on the target alone while sitting inside the model.
    #[test]
    fn the_camera_frames_the_model_on_load() {
        let (_dir, options) = model_at(&fixture::quad_glb(fixture::QUAD_CENTRE), 4);
        let viewer = scripted(&options);
        let camera = viewer.camera();

        assert!(
            (camera.target - fixture::QUAD_CENTRE).length() < 1e-4,
            "the camera looks at {:?}, and the model is at {:?}",
            camera.target,
            fixture::QUAD_CENTRE,
        );

        // The quad's bounding sphere has radius sqrt(0.5² + 0.5²); the eye must
        // be outside it, and inside the half-angle the projection has at this
        // window's aspect.
        let radius = viewer.bounds.half_extent().length();
        let distance = (camera.eye - camera.target).length();
        assert!(
            distance > radius,
            "the eye is {distance} from a model of radius {radius}, so it is inside it",
        );
        let Projection::Perspective { fov_y, .. } = camera.projection else {
            panic!("an orbit camera is perspective-only");
        };
        let half_fov = (0.5 * fov_y).min((0.5 * fov_y).tan().atan());
        assert!(
            (radius / distance).asin() < half_fov,
            "a sphere of radius {radius} at {distance} does not fit in a {fov_y} rad cone",
        );

        viewer.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A drag through the real loop turns the camera**, which is what says
    /// the mapping is wired into the frame rather than merely written.
    ///
    /// `crate::controls` proves the gesture is the right way round; this proves
    /// the events reach it at all. A loop that pumped the shell and never
    /// called `Controls::apply` passes every test in that module.
    #[test]
    fn a_drag_played_into_the_loop_moves_the_camera() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        let mut viewer = scripted(&options);
        viewer.frame().expect("a frame");
        let before = viewer.camera();
        let window = viewer.window();

        viewer
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Pressed,
                Some(PhysicalPoint::ORIGIN),
            )
            .expect("the window is live");
        viewer
            .shell_mut()
            .move_pointer(window, PhysicalPoint::new(120.0, 0.0), (120.0, 0.0))
            .expect("the window is live");
        viewer.frame().expect("a frame");

        let after = viewer.camera();
        assert_ne!(after.eye, before.eye, "the drag never reached the camera");
        assert_eq!(after.target, before.target, "an orbit does not pan");

        // And `F` puts it back: the distance the drag left is not the framed
        // one, so a re-frame that did nothing would fail here.
        viewer
            .shell_mut()
            .scroll(
                window,
                crcbl::core::input::ScrollDelta::Lines { x: 0.0, y: 8.0 },
                None,
            )
            .expect("the window is live");
        viewer.frame().expect("a frame");
        let zoomed = (viewer.camera().eye - viewer.camera().target).length();
        assert!(
            zoomed < (after.eye - after.target).length(),
            "the wheel did nothing"
        );

        viewer
            .shell_mut()
            .key_press(window, crcbl::core::input::KeyCode::KeyF)
            .expect("the window is live");
        viewer.frame().expect("a frame");
        let framed = (viewer.camera().eye - viewer.camera().target).length();
        assert!(
            framed > zoomed,
            "F left the eye at {framed}, which is no further out than the {zoomed} \
             the wheel had brought it to",
        );

        viewer.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A window resize reaches the swapchain**, which is the one thing a
    /// hand-written loop has to do that the engine's would have done.
    #[test]
    fn a_resize_reaches_the_swapchain() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 8);
        let mut viewer = scripted(&options);
        viewer.frame().expect("a frame");
        let window = viewer.window();

        viewer
            .shell_mut()
            .resize(window, crcbl::shell::PhysicalSize::new(640, 400))
            .expect("the window is live");
        viewer.frame().expect("a frame");
        assert_eq!(viewer.extent(), (640, 400));

        viewer.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// A close request stops the loop and is answered, rather than the window
    /// being left waiting for a reply that never comes.
    #[test]
    fn a_close_request_stops_the_loop() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 64);
        let mut viewer = scripted(&options);
        viewer.frame().expect("a frame");
        let window = viewer.window();

        viewer
            .shell_mut()
            .request_close(window)
            .expect("the window is live");
        assert_eq!(
            viewer.frame().expect("a frame"),
            Flow::Stop(ExitReason::CloseRequested),
        );
        let summary = viewer
            .finish(ExitReason::CloseRequested)
            .expect("teardown after a close");
        assert_eq!(summary.exit, ExitReason::CloseRequested);
    }

    /// **A document nothing could be made of is refused, not presented.**
    ///
    /// A viewer that opened a window on an empty scene would look exactly like
    /// one whose model failed to convert, which is the failure
    /// `docs/plan/sample/05-viewer.md` exists to catch.
    #[test]
    fn a_document_with_nothing_in_it_never_reaches_a_window() {
        let (_dir, options) = model_at(&fixture::empty_glb(), 4);
        let error = run(&options).expect_err("an empty document is not a run");
        assert!(
            matches!(error, ViewerError::Load(LoadError::NoGeometry(_))),
            "{error}",
        );
    }
}
