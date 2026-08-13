//! Lumen's start-up, and the methods the engine's loop calls.
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Lumen::tick      (the camera, the lamp)
//!   draw_list.clear()
//!     ─────────────────────────────→ Lumen::draw      (hands the camera over)
//!     menu ───────────────────────→ Lumen::menu_kind
//!     debug overlay ──────────────→ Lumen::debug_sections
//!   gpu.frame()
//! ```
//!
//! There is no loop in this file, and no simulation: what a lighting fixture has
//! instead of a game is a camera somebody moves and a light that moves itself.
//! Both are stepped inside `run_ticks`'s `while`, not after it — anything
//! stepped once per frame has a speed proportional to the frame rate, and a
//! headless run pinned to 1/60 s cannot see that.
//!
//! # The selectors are copied out of the GPU at start-up
//!
//! [`HostedGame::debug_sections`] is handed a panel and `&self`, and no GPU:
//! the panel is gathered before the frame runs, and the bundle is the loop's to
//! hold. So [`Lumen`] keeps the [`Paths`] the device resolved, copied once from
//! [`Gpu::paths`] at [`with_shell`] — which is also what puts them in
//! [`Summary`], where a headless run can print them.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, RunSummary, open_window, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::shell::{
    DisplayMode, LogicalSize, ShellBackend as Backend, WindowDesc, open, open_backend,
};
use crcbl::ui::draw_list::DrawList;

use crate::args::Options;
use crate::camera::Flyer;
use crate::gpu::{Gpu, Paths, Unbuilt};
use crate::menu::{self, CameraMode, LumenAction, Menus};
use crate::room;

/// What a completed run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Which shell backend ran.
    pub backend: Backend,
    /// Frames presented.
    pub frames: u64,
    /// Fixed simulation steps executed.
    pub ticks: u64,
    /// Shell events observed, of every kind.
    pub events: u64,
    /// The swapchain's size when the loop stopped.
    pub extent: (u32, u32),
    /// Why it stopped.
    pub exit: ExitReason,
    /// Whether the simulation was stopped when the loop ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for.
    pub mode: DisplayMode,
    /// **Which of the three selectors the frames were drawn through**, and
    /// whether the run forced any of them.
    ///
    /// `docs/plan/sample/00-samples-overview.md` rule 12: the selected paths
    /// appear in the debug panel *and* in the headless summary line. This is the
    /// second of those — the panel is a windowed run's answer, and a CI job has
    /// no window.
    pub paths: Paths,
    /// Which camera the run ended on.
    pub camera: CameraMode,
}

/// Anything that can stop lumen before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample. A lighting fixture has no simulation of its own to
/// fail, so it takes the default type parameter and its `Game` variant is
/// uninhabited.
pub type LumenError = crcbl::engine::LoopError;

/// Lumen, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Lumen {
    /// Which camera the next frame is drawn from.
    camera: CameraMode,
    /// The free camera, whether or not it is the one in use — kept across a
    /// swap so a reviewer who looks at the golden pose and swaps back is where
    /// they left off.
    flyer: Flyer,
    /// What the device resolved, copied once — see the module docs.
    paths: Paths,
    /// The value the pause panel was last built for — `None` until the first
    /// pause, so the panel is always rebuilt once with the real value.
    shown: Option<CameraMode>,
}

impl Lumen {
    /// A fixture starting on `camera`, drawn through `paths`, with the free
    /// camera at the golden pose.
    #[must_use]
    pub fn new(camera: CameraMode, paths: Paths) -> Self {
        Self {
            camera,
            flyer: Flyer::at(&room::fixed_camera()),
            paths,
            shown: None,
        }
    }

    /// Which camera the next frame is drawn from.
    #[must_use]
    pub const fn camera_mode(&self) -> CameraMode {
        self.camera
    }

    /// The free camera, whether or not it is the one in use.
    #[must_use]
    pub const fn flyer(&self) -> &Flyer {
        &self.flyer
    }

    /// The camera this frame is seen through.
    ///
    /// Both modes share the fixed camera's projection, which is what makes the
    /// pair comparable: a free camera with a lens of its own would produce a
    /// frame a reviewer cannot hold against the golden.
    #[must_use]
    pub fn camera(&self) -> crcbl::render::Camera {
        let fixed = room::fixed_camera();
        match self.camera {
            CameraMode::Fixed => fixed,
            CameraMode::Free => self.flyer.camera(fixed.projection),
        }
    }
}

/// The loop lumen runs in. A type alias, because the loop is the engine's.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Lumen>;

/// Runs the full loop.
///
/// # Errors
///
/// [`LumenError`] if the shell or the GPU refused. Teardown runs on every path:
/// a failing frame must still release the swapchain, the surface and the window,
/// or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, LumenError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the room.
///
/// # Errors
///
/// [`LumenError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, LumenError> {
    let shell = if options.common.headless {
        // By name, never by fallback: the registry deliberately refuses to
        // auto-select headless, because a run that silently had no window would
        // look like a hang.
        open_backend(Backend::Headless).map_err(LumenError::Shell)?
    } else {
        open().map_err(LumenError::NoWindowSystem)?
    };
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell.
///
/// # Errors
///
/// [`LumenError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, LumenError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_window(
        shell.as_mut(),
        &clock_source,
        &WindowDesc {
            title: "Crucible — lumen",
            app_id: "sh.kryptic.crcbl.lumen",
            // 4:3, so a windowed frame and a golden are the same framing: the
            // fixed camera's field of view is vertical, and a different aspect
            // crops or reveals the room's side walls.
            size: options
                .common
                .size
                .map_or(LogicalSize::new(960.0, 720.0), |size| size.to_logical(1.0)),
            mode: options.common.display_mode(),
            ..WindowDesc::default()
        },
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
    crcbl::log::info!("shell: first configure at {}x{}", extent.0, extent.1);

    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.common.gpu(),
        options.forced,
    )?;
    let paths = gpu.paths();

    Ok(Loop::new(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        Lumen::new(options.camera, paths),
        options.common.loop_config(),
    ))
}

impl HostedGame for Lumen {
    /// A lighting fixture has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the whole of its state machine.
    type MenuKind = bool;
    type MenuAction = LumenAction;
    type Summary = Summary;

    const NAME: &'static str = "lumen";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, gpu: &mut Gpu, tick_dt: f64) {
        #[allow(clippy::cast_possible_truncation)]
        let dt = tick_dt as f32;
        // The camera integrates whether or not it is the one being drawn from:
        // a reviewer who swaps to the golden pose, walks, and swaps back should
        // arrive where the keys took them.
        self.flyer.advance(dt);
        // The lamp's orbit, on the fixed timestep, so `--headless --frames N`
        // renders a bit-reproducible room on every machine — which is what makes
        // a golden image of it evidence rather than a coincidence.
        gpu.advance(dt);
    }

    /// Every key the loop's own three did not claim goes to the camera.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.flyer.key(key, pressed);
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<LumenAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: LumenAction) {
        match action {
            LumenAction::ToggleCamera => {
                self.camera = self.camera.toggled();
                // Leaving the free camera is also how a reviewer gets back to
                // the golden pose, so it is put back there — otherwise "look at
                // the reference framing" would be a walk rather than a press.
                if self.camera == CameraMode::Fixed {
                    self.flyer = Flyer::at(&room::fixed_camera());
                }
                // And nothing is held down after a menu press: the press
                // happened while the menu owned the keyboard, so a key that was
                // down when the panel opened has no release coming.
                self.flyer.release_all();
            }
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        if paused && self.shown != Some(self.camera) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the value in force, restoring the selection so a press
            // on a row does not throw the reviewer back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(true, menu::pause_menu(self.camera));
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some(self.camera);
        }
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, _frame: FrameInfo) {
        // The fixture draws no HUD of its own: everything it has to say about a
        // frame is a debug-panel row. What `draw` does is hand over the camera
        // the ticks moved.
        gpu.set_camera(self.camera());
    }

    /// Three sections, and each is something the charter asks for.
    ///
    /// Rule 12's path reporting is the first — a fixture that did not say which
    /// arm it drew through would be one whose picture nobody could attribute.
    /// The second is the sample's own honesty: the mirror panel and the metal
    /// block are near-black, and a reviewer has to be told that on the screen
    /// where the black is rather than in a document nobody has open. The third
    /// is where the camera is, which is what turns "this looks wrong" into a
    /// position somebody else can stand at.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.paths);
        panel.add(&Unbuilt);
        panel.add(self);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            ticks: run.ticks,
            events: run.events,
            extent: run.extent,
            exit: run.exit,
            paused: run.paused,
            mode: run.mode,
            paths: self.paths,
            camera: self.camera,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "lumen: {} frames, {} ticks on the {} shell at {}x{} ({:?}), {:?} / {:?} / {:?}",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.exit,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
        );
    }
}

/// Where the camera is, as a panel section.
///
/// On [`Lumen`] itself because that is what owns the numbers — the rule
/// [`crcbl::ui::DebugModule`] states — and because the answer is the *mode* as
/// much as the position: a reviewer looking at an unexpected frame needs to know
/// whether they are standing at the golden pose or somewhere they walked to.
impl crcbl::ui::DebugModule for Lumen {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        let eye = self.camera().eye;
        section.set_title("camera");
        section.row_str("mode", self.camera.label());
        section.row(
            "eye",
            format_args!("{:.2} {:.2} {:.2}", eye.x, eye.y, eye.z),
        );
        section.row_str(
            "pose",
            if self.flyer.has_moved() && self.camera == CameraMode::Free {
                "walked"
            } else {
                "golden"
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crcbl::engine::{Flow, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, PAUSE_KEY};
    use crcbl::shell::HeadlessShell;

    use super::*;

    /// A loop over a *concrete* `HeadlessShell`, so a test can play compositor.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Always `--backend null`. These run on every CI leg, including ones with
    /// no Vulkan loader at all, and they are about the *loop* — the camera, the
    /// menu, determinism — not about a driver. The picture is
    /// `tests/golden.rs`'s.
    fn headless(frames: u64) -> Options {
        let mut options = Options::default();
        options.common.headless = true;
        options.common.frames = Some(frames);
        options.common.backend = Some(crcbl::backend::GpuBackend::Null);
        options
    }

    /// Every `Text` command the frame handed to the UI pass.
    fn ui_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
        use crcbl::ui::draw_list::DrawCommand;
        engine
            .gpu()
            .draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The CI-visible promise: a headless run terminates, and terminates with
    /// the same numbers every time.
    #[test]
    fn a_headless_run_is_deterministic() {
        let first = run(&headless(24)).expect("headless runs everywhere");
        let second = run(&headless(24)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.frames, 24);
        assert_eq!(first.exit, ExitReason::FrameBudget);
        assert_eq!(first.camera, CameraMode::Fixed);
    }

    /// **The summary names the paths the frames were drawn through**, which is
    /// rule 12's headless half.
    ///
    /// The null backend registers `NullInstance::gpu_driven`, whose bundle
    /// carries `DRAW_INDIRECT_COUNT` and `DESCRIPTOR_INDEXING` and **not**
    /// `MESH_SHADER` — so the answer here is the middle geometry path and the
    /// better binding model. Two different values rather than two defaults,
    /// which is what makes this an assertion about a device rather than about a
    /// struct literal: a summary that carried `Paths::of(&DeviceCaps::default())`
    /// would report the floor on both axes.
    #[test]
    fn the_headless_summary_names_the_selected_paths() {
        let summary = run(&headless(4)).expect("headless runs everywhere");
        assert_eq!(
            summary.paths.geometry,
            crcbl::hal::GeometryPath::IndirectCount,
            "the null device has a GPU-side draw count and no mesh stage",
        );
        assert_eq!(summary.paths.binding, crcbl::hal::BindingModel::Bindless);
        assert_eq!(
            summary.paths.lighting,
            crcbl::hal::LightingPath::Rasterised,
            "no device in this engine can trace anything yet",
        );
        assert_eq!(summary.paths.forced, crate::gpu::Forced::default());
    }

    /// **Forcing a lesser path really opens a lesser device**, and the summary
    /// says the run asked for it.
    ///
    /// The observable is the path the *device* selected, not the flag: a flag
    /// that reached `Options` and never reached `DeviceDesc` would leave this
    /// reporting `MeshShader` while claiming to have forced something.
    #[test]
    fn forcing_a_path_reaches_the_device_and_the_summary() {
        let mut options = headless(4);
        options.forced.geometry = Some(crcbl::hal::GeometryPath::IndirectPerBatch);
        options.forced.binding = Some(crcbl::hal::BindingModel::ArrayPages);
        let summary = run(&options).expect("a lesser device still runs");
        assert_eq!(
            summary.paths.geometry,
            crcbl::hal::GeometryPath::IndirectPerBatch
        );
        assert_eq!(summary.paths.binding, crcbl::hal::BindingModel::ArrayPages);
        assert_eq!(summary.paths.forced, options.forced);
    }

    /// **F3 turns the panel on, and the sections it shows are lumen's own.**
    ///
    /// Rule 4 in one test: the panel opens, and the two rows that make this a
    /// *fixture* rather than a picture — which path drew it, and what is not
    /// built yet — are on it.
    #[test]
    fn f3_shows_the_path_report_and_the_unbuilt_notice() {
        use crcbl::engine::DEBUG_OVERLAY_KEY;

        let mut options = headless(16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).is_empty(),
            "the fixture draws no UI at all while the panel is off",
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        let titles: Vec<&str> = engine
            .debug()
            .panel
            .sections()
            .iter()
            .map(crcbl::ui::DebugSection::title)
            .collect();
        for section in ["paths", "unbuilt", "camera"] {
            assert!(
                titles.contains(&section),
                "no {section} section on the panel: {titles:?}"
            );
        }
        // And the rows really reached the draw list, not just the panel.
        let drawn = ui_text(&engine);
        for row in ["geometry", "lighting", "metal", "mode"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The camera row swaps the camera, and swapping back restores the golden
    /// pose.**
    ///
    /// The observable is the camera's eye, not the mode: a toggle that flipped
    /// the enum and left `camera()` returning the same pose would pass every
    /// assertion about the mode alone.
    #[test]
    fn the_camera_row_swaps_the_camera_and_returns_it_to_the_golden_pose() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        for _ in 0..2 {
            assert_eq!(engine.frame().expect("a frame"), Flow::Continue);
        }
        assert_eq!(engine.game().camera().eye, room::fixed_camera().eye);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // `downs` arrow presses, then Enter. **The selection persists across a
        // pause** — `menu_kind` rebuilds the panel and restores the selected id
        // — so the first visit walks down to the CAMERA row and the second one
        // is already on it. The label assertions after each call are what say
        // the right row fired.
        let press_camera_row = |engine: &mut Loop<HeadlessShell>, downs: usize| {
            for _ in 0..downs {
                engine
                    .shell_mut()
                    .key_press(window, MENU_DOWN_KEY)
                    .expect("the window is live");
            }
            engine.frame().expect("a frame");
            engine
                .shell_mut()
                .key_press(window, MENU_ACTIVATE_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
            engine
                .shell_mut()
                .key_release(window, MENU_ACTIVATE_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
        };
        press_camera_row(&mut engine, 3);
        assert_eq!(engine.game().camera_mode(), CameraMode::Free);
        assert!(
            ui_text(&engine).iter().any(|text| text == "CAMERA: FREE"),
            "the row's label must show the new value: {:?}",
            ui_text(&engine),
        );

        // Resume, walk, and confirm the free camera actually moved.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        for _ in 0..30 {
            engine.frame().expect("a frame");
        }
        engine
            .shell_mut()
            .key_release(window, KeyCode::KeyW)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let walked = engine.game().camera().eye;
        assert_ne!(walked, room::fixed_camera().eye, "W did not walk");
        assert!(engine.game().flyer().has_moved());

        // And the row puts it back where the golden was taken from.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        press_camera_row(&mut engine, 0);
        assert_eq!(engine.game().camera_mode(), CameraMode::Fixed);
        assert!(
            ui_text(&engine).iter().any(|text| text == "CAMERA: FIXED"),
            "the row's label must show the value it went back to: {:?}",
            ui_text(&engine),
        );
        assert_eq!(
            engine.game().camera().eye,
            room::fixed_camera().eye,
            "swapping back did not return to the golden pose",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The lamp moves, and it moves on the **clock** rather than on the frame
    /// count.
    ///
    /// The whole reason the orbit is stepped inside `run_ticks`: a lamp advanced
    /// once per frame would be in a different place on every machine, and the
    /// golden would be a coincidence.
    #[test]
    fn the_lamp_orbits_on_the_simulation_clock() {
        let mut engine = scripted(&headless(200));
        for _ in 0..2 {
            engine.frame().expect("a frame");
        }
        let after_two = engine.gpu().elapsed();
        assert!(after_two > 0.0, "the lamp has to be moving first");

        let ticks = engine.ticks();
        for _ in 0..30 {
            engine.frame().expect("a frame");
        }
        let ran = engine.ticks() - ticks;
        let moved = engine.gpu().elapsed() - after_two;
        #[allow(clippy::cast_precision_loss)]
        let expected = ran as f32 / crate::args::DEFAULT_TICK_HZ as f32;
        assert!(
            (moved - expected).abs() < 1e-4,
            "{ran} ticks moved the lamp by {moved}s, and a tick is 1/{}s",
            crate::args::DEFAULT_TICK_HZ,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
