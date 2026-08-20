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
//! [`Gpu::paths`] in `assemble` — which is also what puts them in
//! [`Summary`], where a headless run can print them.
//!
//! The pause menu's effect rows are the same shape and the same reason:
//! [`HostedGame::apply`] is handed no GPU either, so a press edits the
//! [`EffectRequest`] [`Lumen`] holds and [`HostedGame::draw`] — which is handed
//! one — hands it over and re-reads what came back out of the four layers. The
//! device layer is copied once beside it, because it cannot change mid-run and
//! a row cannot be labelled without it.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, PointerUpdate, RunSummary, open_window,
    wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::render::{EffectRequest, Flyer, RenderEffects};
use crcbl::shell::{DisplayMode, PointerMode, ShellBackend as Backend, WindowDesc, WindowId};
use crcbl::ui::draw_list::DrawList;

use crate::args::Options;
use crate::gpu::{Gpu, Paths, Unbuilt};
use crate::menu::{self, CameraMode, LumenAction, Menus};
use crate::room;

/// How often [`Lumen::log_heartbeat`] logs, in ticks.
///
/// A second of simulated time at [`crate::DEFAULT_TICK_HZ`], which is what every
/// other sample's heartbeat is spaced at.
const HEARTBEAT_TICKS: u64 = 60;

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
    /// What the device resolved, copied once — see the module docs — and its
    /// `effects` re-read each [`HostedGame::draw`], because the pause menu can
    /// move that one mid-run.
    paths: Paths,
    /// The three requested layers of topic 39's order, as the pause menu has
    /// them: the command line wrote the first version at start-up and the
    /// effect rows edit the programmatic one — see [`menu::toggled_effect`].
    ///
    /// Held here rather than read off the renderer per press because
    /// [`HostedGame::apply`] is handed no GPU; [`HostedGame::draw`] is where it
    /// reaches one.
    effect_request: EffectRequest,
    /// The fourth layer, copied once for the reason [`Paths`] is: a row is built
    /// where there is no GPU to ask, and it cannot say "unavailable" without it.
    device_effects: RenderEffects,
    /// The values the pause panel was last built for — `None` until the first
    /// pause, so the panel is always rebuilt once with the real ones.
    shown: Option<(CameraMode, EffectRequest)>,
    /// Whether the loop has the simulation stopped, recorded in
    /// [`HostedGame::menu_kind`].
    ///
    /// The loop owns the pause and this is a *copy* of it, kept for one caller:
    /// [`HostedGame::pointer_mode`] is asked with no argument and has to answer
    /// "is a panel up", and `menu_kind` is the one place the loop says so.
    paused: bool,
    /// Fixed steps run, for [`Lumen::log_heartbeat`]'s cadence.
    ticks: u64,
}

impl Lumen {
    /// A fixture starting on `camera`, drawn through `paths` with `effects`
    /// asked for on a device that permits `device_effects`, and with the free
    /// camera at the golden pose.
    #[must_use]
    pub fn new(
        camera: CameraMode,
        paths: Paths,
        effects: EffectRequest,
        device_effects: RenderEffects,
    ) -> Self {
        Self {
            camera,
            flyer: Flyer::at(&room::fixed_camera()),
            paths,
            effect_request: effects,
            device_effects,
            shown: None,
            paused: false,
            ticks: 0,
        }
    }

    /// The `[HUD]` line, on the cadence breakout, flappy, asteroids, horde and
    /// hud use: every [`HEARTBEAT_TICKS`] steps, which is a second of simulated
    /// time at the default rate.
    ///
    /// The one thing this fixture logs from inside the tick, and it is there for
    /// the browser gate — `web/tools/browser-e2e.mjs` reads two claims out of
    /// it, neither of which anything else in the page can answer.
    ///
    /// The first is which lighting path the frame is being drawn through. On the
    /// web that is the whole point of publishing this sample: a browser has no
    /// ray query, so [`crcbl::hal::LightingPath::Rasterised`] is what a page
    /// takes **by construction** — and a line that names it is what tells a
    /// rasterised room from a page that opened some other device.
    ///
    /// The second is that the fixture is advancing on its own. `lamp x` is the
    /// value for it: the orbiting light is the only thing in the room that
    /// moves, [`crate::room::lamp`] is a pure function of the seconds
    /// [`Gpu::advance`] has accumulated, and those seconds accumulate here — in
    /// the tick — so a frame loop that was presenting without ticking would
    /// leave it standing still.
    fn log_heartbeat(&self, gpu: &Gpu) {
        if !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        let crcbl::render::Light::Point(lamp) = room::lamp(gpu.elapsed()) else {
            // `room::lamp` returns a point light and this sample's own
            // `the_lamp_orbits_inside_the_room` pins that; there is no position
            // to report if it ever stops being one.
            return;
        };
        crcbl::log::info!(
            "[HUD] tick: {}  lighting: {:?}  geometry: {:?}  binding: {:?}  camera: {}  \
             effects: {}  lamp x: {:.2}",
            self.ticks,
            self.paths.lighting,
            self.paths.geometry,
            self.paths.binding,
            self.camera.label(),
            self.paths.effects_row(),
            lamp.position.x,
        );
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
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two
/// share is everything after the waiting, which is `assemble`.
///
/// # Errors
///
/// [`LumenError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, LumenError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(shell.as_mut(), &clock_source, options)?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.common.gpu(),
        options.forced,
        options.effects,
    )?;

    Ok(assemble(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        options,
    ))
}

/// Creates the one window this sample has: its title, its app id, its size.
///
/// # Errors
///
/// [`LumenError`] if the shell refused it.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    options: &Options,
) -> Result<WindowId, LumenError> {
    Ok(open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Crucible — lumen",
            app_id: "sh.kryptic.crcbl.lumen",
            // 4:3, so a windowed frame and a golden are the same framing: the
            // fixed camera's field of view is vertical, and a different aspect
            // crops or reveals the room's side walls.
            size: crcbl::engine::requested_window_size(options.common.size),
            mode: options.common.display_mode(),
            ..WindowDesc::default()
        },
    )?)
}

/// The half of start-up that is the same however the GPU arrived.
///
/// [`Booted`] is what both bring-up paths hand over, so the fixture is built and
/// the loop assembled in one place rather than one per path — a second copy is
/// how the browser build would come to run a subtly different sample.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    // All three read before the bundle moves into the loop: what the flags asked
    // for, resolved into a request, and what this device permits.
    let paths = booted.gpu.paths();
    let effects = booted.gpu.effect_request();
    let device_effects = booted.gpu.device_effects();

    Loop::new(
        booted,
        Lumen::new(options.camera, paths, effects, device_effects),
        options.common.loop_config(),
    )
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
        self.ticks += 1;
        // The camera integrates whether or not it is the one being drawn from:
        // a reviewer who swaps to the golden pose, walks, and swaps back should
        // arrive where the keys took them.
        self.flyer.advance(dt);
        // The lamp's orbit, on the fixed timestep, so `--headless --frames N`
        // renders a bit-reproducible room on every machine — which is what makes
        // a golden image of it evidence rather than a coincidence.
        gpu.advance(dt);
        self.log_heartbeat(gpu);
    }

    /// Every key the loop's own three did not claim goes to the camera.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.flyer.key(key, pressed);
    }

    /// The mouse look, and the one condition it is bound under.
    ///
    /// **`at.is_none()` is what says the pointer is really captured.**
    /// [`PointerUpdate::motion`] states that shape: under
    /// [`PointerMode::Locked`] there is no absolute position at all, so a locked
    /// frame carries a motion and no `at`, and an unlocked one that moved
    /// carries both. Binding the look to that rather than to the request
    /// [`pointer_mode`](HostedGame::pointer_mode) makes is the whole point — a
    /// request is not a grant, the loop declines the lock on a shell without
    /// `ShellCaps::has_mouselook`, and a camera that turned anyway would swing
    /// the view while a visible cursor walked out of the window and clicked on
    /// whatever is behind it.
    ///
    /// It is also what makes the paused case correct on the frame the panel
    /// opens rather than the one after: the pointer comes back the moment the
    /// loop releases it, and the look stops on the same event.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let Some(motion) = pointer.motion.filter(|_| pointer.at.is_none()) else {
            return;
        };
        self.flyer.look(motion);
    }

    /// [`PointerMode::Locked`] while the room is being flown, free while the
    /// pause panel is up.
    ///
    /// Answered from the pause alone — not from
    /// [`CameraMode`] — because the free camera integrates whether or not it is
    /// the one being drawn from, exactly as the keyboard's walk does: a reviewer
    /// who looks around at the golden pose and then swaps to the free camera
    /// arrives facing where they looked.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
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
            // Read-modify-write on the layer a panel owns, leaving the camera
            // stack and `[engine.video]` as they were — `menu::toggled_effect`
            // is where that is argued. It reaches the renderer in `draw`.
            LumenAction::ToggleEffect(effect) => {
                self.effect_request =
                    menu::toggled_effect(self.effect_request, self.device_effects, effect);
            }
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        // Recorded for `pointer_mode`, which the loop polls immediately after
        // this: a panel that went up on this frame must free the pointer on this
        // frame, or the cursor comes back one frame into a menu the reviewer is
        // already trying to click.
        self.paused = paused;
        if paused && self.shown != Some((self.camera, self.effect_request)) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a press
            // on a row does not throw the reviewer back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(
                true,
                menu::pause_menu(self.camera, self.effect_request, self.device_effects),
            );
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some((self.camera, self.effect_request));
        }
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, _frame: FrameInfo) {
        // The fixture draws no HUD of its own: everything it has to say about a
        // frame is a debug-panel row. What `draw` does is hand over the camera
        // the ticks moved, and the effect request the pause menu edited.
        gpu.set_camera(self.camera());
        // Here rather than in `tick`, which does not run while paused: the row
        // that was just pressed is on a panel over a frame that has to change
        // behind it. `set_effect_request` lands on this frame — `begin_frame`
        // has not run yet — and the frame in flight never moves.
        gpu.set_effect_request(self.effect_request);
        // Re-read rather than recomputed: the device clamps last, so what the
        // panel and the summary report comes back off the renderer.
        self.paths = gpu.paths();
    }

    /// Three sections, and each is something the charter asks for.
    ///
    /// Rule 12's path reporting is the first — a fixture that did not say which
    /// arm it drew through would be one whose picture nobody could attribute.
    /// The second is the sample's own honesty: the mirror panel and the metal
    /// block are lit by reflection alone, and what stands in for the reflection
    /// this path cannot trace is a baked probe volume — a reviewer has to be
    /// told that on the screen rather than in a document nobody has open. The
    /// third
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

// ---- polled start-up ---------------------------------------------------------

/// A [`Loop`] being started one poll at a time, for a caller that may not
/// block — which on a browser main thread is every caller.
///
/// The state machine, the pump and the resize-during-start-up race are
/// [`crcbl::engine::PolledBoot`]'s; all that is left here is this sample's
/// `Options` and the `assemble` call the engine deliberately stops short of.
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    boot: crcbl::engine::PolledBoot<S, Gpu>,
    options: Options,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, without blocking on either half.
    ///
    /// `clock_source` is the caller's because the browser's cannot be
    /// [`Clock::new`]'s: `std::time::Instant::now` panics on
    /// `wasm32-unknown-unknown`, so a page drives the loop from
    /// `performance.now()` instead.
    ///
    /// # Errors
    ///
    /// [`LumenError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, LumenError> {
        let window = open_the_window(shell.as_mut(), &clock_source, options)?;
        Ok(Self {
            boot: crcbl::engine::PolledBoot::request(
                shell,
                window,
                clock_source,
                options.common.gpu(),
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`LumenError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, LumenError> {
        let Some(booted) = self.boot.poll::<LumenError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options)))
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

    /// Walks `downs` rows down the open panel and presses ENTER on the row it
    /// lands on.
    ///
    /// **The selection persists across a pause** — `menu_kind` rebuilds the
    /// panel and restores the selected id — so a second visit to the same row is
    /// `downs` of zero. The label assertions each caller makes afterwards are
    /// what say the right row fired.
    fn press_row(engine: &mut Loop<HeadlessShell>, window: crcbl::shell::WindowId, downs: usize) {
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

    /// **An effect flag reaches the renderer's resolved set and the summary.**
    ///
    /// The charter's "every effect toggles independently" as far as a headless
    /// run can see it: what a real device does to the picture is
    /// `tests/golden.rs`, and what this catches is the flag that stopped at
    /// [`Options`] — which would leave every frame drawing everything while the
    /// summary line claimed otherwise.
    ///
    /// One arm per effect, because a run that switched all three off reports the
    /// empty set whichever bit each flag was wired to.
    #[test]
    fn an_effect_flag_reaches_the_frame_and_the_summary() {
        use crcbl::render::RenderEffects;

        assert_eq!(
            run(&headless(4))
                .expect("headless runs everywhere")
                .paths
                .effects,
            RenderEffects::all(),
            "a run that asked for nothing must draw every effect",
        );

        for (off, row) in [
            (RenderEffects::SHADOWS, "ao ssr"),
            (RenderEffects::AMBIENT_OCCLUSION, "shadows ssr"),
            (RenderEffects::REFLECTIONS, "shadows ao"),
        ] {
            let mut options = headless(4);
            options.effects.remove(off);
            let summary = run(&options).expect("a frame with an effect off still runs");
            assert_eq!(
                summary.paths.effects,
                RenderEffects::all().difference(off),
                "{off:?} did not reach the renderer",
            );
            assert_eq!(summary.paths.effects_row(), row, "{off:?}");
        }
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

        // Three rows down from RESUME is CAMERA; the second visit is already on
        // it, which is what `press_row`'s docs are about.
        press_row(&mut engine, window, 3);
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
        press_row(&mut engine, window, 0);
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

    /// **An effect row reaches the renderer, and every report follows it.**
    ///
    /// The observable is the **resolved** set read back off the renderer, not a
    /// field on [`Lumen`]: a press that edited the request and never reached
    /// `set_effect_request` would leave the frame drawing every effect while the
    /// row and the panel both said `OFF`. What the removed passes do to the
    /// picture is `tests/golden.rs`'s
    /// `every_effect_toggles_and_the_frame_says_so`; this is the wiring in
    /// between, which no golden can see.
    #[test]
    fn an_effect_row_reaches_the_renderer_and_the_reports() {
        use crcbl::render::RenderEffects;

        let mut engine = scripted(&headless(400));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().paths().effects,
            RenderEffects::all(),
            "a run that asked for nothing draws every effect",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // Four rows down from RESUME is SHADOWS: the panel is the loop's three,
        // then CAMERA, then one row per effect.
        press_row(&mut engine, window, 4);
        assert_eq!(
            engine.gpu().paths().effects,
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "the row did not reach the renderer, or took more than shadows",
        );
        assert_eq!(engine.gpu().paths().effects_row(), "ao ssr");
        assert!(
            ui_text(&engine).iter().any(|text| text == "SHADOWS: OFF"),
            "the row's label must show what the frame now draws: {:?}",
            ui_text(&engine),
        );

        // And the same row puts it back, which is the comparison the charter's
        // matrix is for: one keypress between a shadowed room and an unshadowed
        // one, with no restart in between.
        press_row(&mut engine, window, 0);
        assert_eq!(engine.gpu().paths().effects, RenderEffects::all());
        assert!(
            ui_text(&engine).iter().any(|text| text == "SHADOWS: ON"),
            "the row's label must show the value it went back to: {:?}",
            ui_text(&engine),
        );

        // Off again, so the summary is asked about a run the menu changed.
        press_row(&mut engine, window, 0);
        let summary = engine.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(
            summary.paths.effects,
            RenderEffects::all().difference(RenderEffects::SHADOWS),
            "the summary reports the set the run ended on",
        );
    }

    /// Where the free camera is looking, whichever camera the frame is drawn
    /// from.
    fn flyer_forward(engine: &Loop<HeadlessShell>) -> crcbl::math::Vec3 {
        let fixed = room::fixed_camera();
        let camera = engine.game().flyer().camera(fixed.projection);
        (camera.target - camera.eye).normalize()
    }

    /// The mode the shell actually has the window in — not what the fixture
    /// asked for.
    fn shell_pointer_mode(engine: &mut Loop<HeadlessShell>) -> PointerMode {
        let window = engine.window();
        engine
            .shell_mut()
            .window_state(window)
            .expect("the loop's window is live")
            .pointer_mode
    }

    /// **The pointer is grabbed while the room is being flown and given back
    /// while the pause panel is up.**
    ///
    /// Read off the window rather than off [`Lumen::pointer_mode`], so what this
    /// asserts is the whole path: the fixture's answer, the loop's poll, and the
    /// shell call. A hook that returned the right value and was never polled
    /// leaves this window free for the whole run.
    ///
    /// The pause half is the one that matters for the bug being avoided — a
    /// captured cursor cannot press `RESUME` — and it is checked on the frame
    /// the panel appears, not a frame later.
    #[test]
    fn the_pointer_is_locked_while_flying_and_free_while_the_panel_is_up() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            shell_pointer_mode(&mut engine),
            PointerMode::Locked,
            "a fixture being flown must own the pointer",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert_eq!(
            shell_pointer_mode(&mut engine),
            PointerMode::Free,
            "the panel is up and the cursor is still captured",
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(!engine.is_paused());
        assert_eq!(
            shell_pointer_mode(&mut engine),
            PointerMode::Locked,
            "resuming did not take the pointer back",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A captured pointer turns the camera, and a free one does not.**
    ///
    /// End to end, because every piece of this is a seam: `HeadlessShell` drops
    /// the absolute position while the pointer is locked, exactly as a
    /// compositor does; the loop delivers that as a `PointerUpdate` with a
    /// `motion` and no `at`; and [`Lumen::pointer_event`] looks only on that
    /// shape. The second half is what the gate is for — the same movement with a
    /// visible cursor under it must move nothing, or a shell that refused the
    /// lock would have the view swinging while the cursor walks off the window.
    #[test]
    fn a_locked_pointer_looks_and_a_free_one_does_not() {
        use crcbl::shell::PhysicalPoint;

        let mut engine = scripted(&headless(400));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(shell_pointer_mode(&mut engine), PointerMode::Locked);

        let start = flyer_forward(&engine);
        let right = start.cross(crcbl::math::Vec3::Y).normalize();
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint { x: 90.0, y: 40.0 }, (50.0, 0.0))
            .expect("the window is live");
        engine.frame().expect("a frame");

        let turned = flyer_forward(&engine);
        assert!(
            turned.dot(right) > 0.01,
            "the mouse went right and the view leaned {} toward its own right {right:?}",
            turned.dot(right),
        );
        assert!(engine.game().flyer().has_moved());

        // Now with the panel up, which frees the pointer: the shell reports an
        // absolute position again and the same delta must turn nothing.
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(shell_pointer_mode(&mut engine), PointerMode::Free);

        let paused_at = flyer_forward(&engine);
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint { x: 140.0, y: 40.0 }, (50.0, 0.0))
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            flyer_forward(&engine),
            paused_at,
            "a visible cursor turned the camera",
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
