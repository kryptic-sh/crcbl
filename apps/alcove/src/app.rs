//! Alcove's start-up, and the methods the engine's loop calls.
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Alcove::tick       (the camera)
//!   draw_list.clear()
//!     ─────────────────────────────→ Alcove::draw        (hands the camera over)
//!     menu ───────────────────────→ Alcove::menu_kind
//!     debug overlay ──────────────→ Alcove::debug_sections
//!   gpu.frame()
//! ```
//!
//! There is no loop in this file and no simulation: what an occlusion fixture
//! has instead of a game is a camera somebody moves and a set of knobs somebody
//! turns.
//!
//! # The knobs are read, never kept
//!
//! [`crate::occlusion`] is the whole of the state, and it lives in the console's
//! own cells. [`Alcove`] holds a *reading* of them, refreshed once per frame in
//! [`HostedGame::draw`], so the pause panel's labels, the debug overlay's
//! `occlusion` section and the headless summary all report one instant — and so
//! that a line typed into the console moves the panel on the next frame rather
//! than being silently overwritten by it.
//!
//! # The selectors are copied out of the GPU at start-up
//!
//! [`HostedGame::debug_sections`] is handed a panel and `&self`, and no GPU, so
//! [`Alcove`] keeps the [`Paths`] the device resolved, copied once from
//! [`Gpu::paths`] in `assemble` — which is also what puts them in [`Summary`],
//! where a headless run can print them. The occlusion chain's per-pass cost is
//! the same shape and the same reason.

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
use crate::court;
use crate::gpu::{Gpu, OcclusionCost, Paths};
use crate::menu::{self, AlcoveAction, CameraMode, Menus};
use crate::occlusion::{self, Knobs};

/// How often [`Alcove::log_heartbeat`] logs, in ticks.
///
/// A second of simulated time at [`crate::DEFAULT_TICK_HZ`], which is what every
/// other sample's heartbeat is spaced at.
const HEARTBEAT_TICKS: u64 = 60;

/// What a completed run did.
#[derive(Clone, Debug, PartialEq)]
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
    /// Where every occlusion knob stood when the loop ended.
    pub knobs: Knobs,
    /// **What the occlusion chain cost, per pass.**
    ///
    /// `docs/plan/sample/19-alcove.md`'s "cost per technique, per frame", in the
    /// headless summary as well as on the panel: a run with `--split` up carries
    /// an `ssao` row and an `ssao-shipped` row, which is the comparison the
    /// charter asks the sample to make legible.
    pub occlusion_cost: OcclusionCost,
}

/// Anything that can stop alcove before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample, and an occlusion fixture has no simulation of its
/// own to fail.
pub type AlcoveError = crcbl::engine::LoopError;

/// Alcove, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Alcove {
    /// Which camera the next frame is drawn from.
    camera: CameraMode,
    /// The free camera, whether or not it is the one in use.
    flyer: Flyer,
    /// What the device resolved, copied once — see the module docs.
    paths: Paths,
    /// The three requested layers of topic 39's order, as the pause menu has
    /// them.
    effect_request: EffectRequest,
    /// The fourth layer, copied once: a row is built where there is no GPU to
    /// ask, and it cannot say "unavailable" without it.
    device_effects: RenderEffects,
    /// What every occlusion knob read on the last frame drawn.
    knobs: Knobs,
    /// What the occlusion chain cost on the last frame whose timestamps landed.
    occlusion_cost: OcclusionCost,
    /// The values the pause panel was last built for; `None` until the first
    /// pause, so the panel is always rebuilt once with the real ones.
    shown: Option<(CameraMode, EffectRequest, Knobs)>,
    /// Whether the loop has the simulation stopped, recorded in
    /// [`HostedGame::menu_kind`].
    paused: bool,
    /// Fixed steps run, for [`Alcove::log_heartbeat`]'s cadence.
    ticks: u64,
}

impl Alcove {
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
            flyer: Flyer::at(&court::fixed_camera()),
            paths,
            effect_request: effects,
            device_effects,
            knobs: Knobs::read(),
            occlusion_cost: OcclusionCost::default(),
            shown: None,
            paused: false,
            ticks: 0,
        }
    }

    /// The `[HUD]` line, on the cadence every other sample's heartbeat uses.
    ///
    /// The one thing this fixture logs from inside the tick, and it is there for
    /// the browser gate this sample does not have yet — `docs/backlog.md`'s
    /// "alcove's web demo is owed" — and for a person watching a headless run.
    /// It names the lighting path, so a page that opened some other device is
    /// legible, and the occlusion state, which is the whole of what this sample
    /// is for.
    fn log_heartbeat(&self) {
        if !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  lighting: {:?}  geometry: {:?}  binding: {:?}  camera: {}  \
             effects: {}  technique: {}  radius: {:.3}  seam: {}",
            self.ticks,
            self.paths.lighting,
            self.paths.geometry,
            self.paths.binding,
            self.camera.label(),
            self.paths.effects_row(),
            self.knobs.technique,
            self.knobs.radius,
            self.knobs.seam_row(),
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

    /// What every occlusion knob read on the last frame drawn.
    #[must_use]
    pub const fn knobs(&self) -> Knobs {
        self.knobs
    }

    /// The camera this frame is seen through.
    ///
    /// Both modes share the fixed camera's projection, which is what makes the
    /// pair comparable: a free camera with a lens of its own would produce a
    /// frame a reviewer cannot hold against the golden.
    #[must_use]
    pub fn camera(&self) -> crcbl::render::Camera {
        let fixed = court::fixed_camera();
        match self.camera {
            CameraMode::Fixed => fixed,
            CameraMode::Free => self.flyer.camera(fixed.projection),
        }
    }
}

/// Every key this sample binds, the thing it does, and what the usage text calls
/// it.
///
/// **One table rather than a `match` beside a help paragraph**, for
/// `crate::menu::PRESSED_ROWS`' reason: a key and its description are one fact
/// about a control, and the way the two drift is a key that moves the intensity
/// while the help says it moves the radius.
type KeyBinding = (KeyCode, fn(), &'static str);

/// The bindings themselves.
pub(crate) const KEYS: [KeyBinding; 9] = [
    (KeyCode::KeyT, cycle_technique, "T"),
    (KeyCode::KeyV, occlusion::toggle_occlusion_view, "V"),
    (KeyCode::KeyB, occlusion::toggle_bent_normals, "B"),
    (KeyCode::KeyX, occlusion::toggle_seam, "X"),
    (KeyCode::KeyR, occlusion::reset, "R"),
    (KeyCode::BracketLeft, radius_down, "["),
    (KeyCode::BracketRight, radius_up, "]"),
    (KeyCode::Minus, intensity_down, "-"),
    (KeyCode::Equal, intensity_up, "="),
];

/// The `,` and `.` pair, which is not in [`KEYS`] because it only does anything
/// while the seam is up — [`occlusion::nudge_seam`] is where that is decided.
pub(crate) const SEAM_KEYS: [(KeyCode, bool, &str); 2] =
    [(KeyCode::Comma, false, ","), (KeyCode::Period, true, ".")];

fn cycle_technique() {
    occlusion::cycle(occlusion::TECHNIQUE);
}

fn radius_up() {
    occlusion::nudge_radius(true);
}

fn radius_down() {
    occlusion::nudge_radius(false);
}

fn intensity_up() {
    occlusion::nudge_intensity(true);
}

fn intensity_down() {
    occlusion::nudge_intensity(false);
}

/// The loop alcove runs in. A type alias, because the loop is the engine's.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Alcove>;

/// Runs the full loop.
///
/// # Errors
///
/// [`AlcoveError`] if the shell or the GPU refused. Teardown runs on every path:
/// a failing frame must still release the swapchain, the surface and the window.
pub fn run(options: &Options) -> Result<Summary, AlcoveError> {
    let summary = crcbl::engine::drive(start(options)?);
    // **The knobs go back on the way out**, whatever happened. A run that moved
    // `r_ssao_technique` and exited would otherwise leave the cell moved for
    // anything sharing this process — which is every test in this crate, and the
    // golden suite's own second frame.
    occlusion::reset();
    summary
}

/// Opens a shell, a window, a GPU and the court.
///
/// # Errors
///
/// [`AlcoveError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, AlcoveError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in a blocking
/// `wait_for_configure` — and would take a pending loop instead. That is the web
/// front end this sample owes; `docs/backlog.md` says what it needs.
///
/// # Errors
///
/// [`AlcoveError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, AlcoveError> {
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
/// [`AlcoveError`] if the shell refused it.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    options: &Options,
) -> Result<WindowId, AlcoveError> {
    Ok(open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Crucible — alcove",
            app_id: "sh.kryptic.crcbl.alcove",
            // 4:3, so a windowed frame and a golden are the same framing: the
            // fixed camera's field of view is vertical, and a different aspect
            // crops or reveals the court's side walls.
            size: crcbl::engine::requested_window_size(options.common.size),
            mode: options.common.display_mode(),
            ..WindowDesc::default()
        },
    )?)
}

/// The half of start-up that is the same however the GPU arrived.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    // `--screenshot`, armed before the first frame because the frame it names is
    // counted from this point.
    #[cfg(not(target_arch = "wasm32"))]
    let booted = {
        let mut booted = booted;
        if let Some(request) = options.common.screenshot_request() {
            booted.gpu.context_mut().set_screenshot(request);
        }
        booted
    };
    // The starting occlusion state, into the cells `crate::occlusion` reads —
    // before the first frame and before the pause panel is first built, so a run
    // started with `--technique hemisphere` shows that on its first pause.
    options.apply();

    let paths = booted.gpu.paths();
    let effects = booted.gpu.effect_request();
    let device_effects = booted.gpu.device_effects();

    Loop::new(
        booted,
        Alcove::new(options.camera, paths, effects, device_effects),
        options.common.loop_config(),
    )
}

impl HostedGame for Alcove {
    /// An occlusion fixture has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the whole of its state machine.
    type MenuKind = bool;
    type MenuAction = AlcoveAction;
    type Summary = Summary;

    const NAME: &'static str = "alcove";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        #[allow(clippy::cast_possible_truncation)]
        let dt = tick_dt as f32;
        self.ticks += 1;
        // The camera integrates whether or not it is the one being drawn from: a
        // reviewer who swaps to the golden pose, walks, and swaps back should
        // arrive where the keys took them.
        self.flyer.advance(dt);
        self.log_heartbeat();
    }

    /// The camera's keys, and this sample's own.
    ///
    /// The flyer is offered every key first and reports whether it took one, so
    /// a binding below can never shadow `WASD` — see
    /// [`crcbl::render::Flyer::key`], whose return value is exactly that answer.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if self.flyer.key(key, pressed) || !pressed {
            return;
        }
        for (bound, act, _) in KEYS {
            if bound == key {
                act();
                return;
            }
        }
        for (bound, right, _) in SEAM_KEYS {
            if bound == key {
                occlusion::nudge_seam(right);
                return;
            }
        }
    }

    /// The mouse look, and the one condition it is bound under.
    ///
    /// **`at.is_none()` is what says the pointer is really captured** — see
    /// [`PointerUpdate::motion`], and `apps/lantern/src/app.rs`, which carries
    /// the argument in full.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let Some(motion) = pointer.motion.filter(|_| pointer.at.is_none()) else {
            return;
        };
        self.flyer.look(motion);
    }

    /// [`PointerMode::Locked`] while the court is being flown, free while the
    /// pause panel is up.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<AlcoveAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: AlcoveAction) {
        match action {
            AlcoveAction::ToggleCamera => {
                self.camera = self.camera.toggled();
                // Leaving the free camera is also how a reviewer gets back to
                // the golden pose, so it is put back there.
                if self.camera == CameraMode::Fixed {
                    self.flyer = Flyer::at(&court::fixed_camera());
                }
                // And nothing is held down after a menu press: the press
                // happened while the menu owned the keyboard, so a key that was
                // down when the panel opened has no release coming.
                self.flyer.release_all();
            }
            // Read-modify-write on the layer a panel owns, leaving the camera
            // stack and `[engine.video]` as they were. It reaches the renderer
            // in `draw`.
            AlcoveAction::ToggleEffect(effect) => {
                self.effect_request =
                    menu::toggled_effect(self.effect_request, self.device_effects, effect);
            }
            // The rest all write a console cell, which is the only place any of
            // this sample's occlusion state lives.
            AlcoveAction::ToggleOcclusionView => occlusion::toggle_occlusion_view(),
            AlcoveAction::CycleTechnique => occlusion::cycle(occlusion::TECHNIQUE),
            AlcoveAction::ToggleBentNormals => occlusion::toggle_bent_normals(),
            AlcoveAction::ToggleSeam => occlusion::toggle_seam(),
            AlcoveAction::ResetKnobs => occlusion::reset(),
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        // Recorded for `pointer_mode`, which the loop polls immediately after
        // this: a panel that went up on this frame must free the pointer on this
        // frame.
        self.paused = paused;
        // Read here as well as in `draw`, because `draw` does not run before the
        // first panel is laid out and a knob a key moved while paused has to
        // reach the label on the same frame.
        let knobs = Knobs::read();
        if paused && self.shown != Some((self.camera, self.effect_request, knobs)) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a press
            // on a row does not throw the reviewer back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(
                true,
                menu::pause_menu(self.camera, self.effect_request, self.device_effects, knobs),
            );
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some((self.camera, self.effect_request, knobs));
        }
        self.knobs = knobs;
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, _frame: FrameInfo) {
        // The fixture draws no HUD of its own: everything it has to say about a
        // frame is a debug-panel row.
        gpu.set_camera(self.camera());
        // Here rather than in `tick`, which does not run while paused: the row
        // that was just pressed is on a panel over a frame that has to change
        // behind it.
        gpu.set_effect_request(self.effect_request);
        // Re-read rather than recomputed: the device clamps last, so what the
        // panel and the summary report comes back off the renderer.
        self.paths = gpu.paths();
        self.knobs = Knobs::read();
        self.occlusion_cost = gpu.occlusion_cost();
    }

    /// Three sections, and each is something a charter asks for.
    ///
    /// Rule 12's path reporting is the first. The second is this sample's whole
    /// subject — every occlusion knob and, while the seam is up, the technique
    /// on each side of it. The third is what the chain cost, which is the
    /// charter's "cost per technique, per frame".
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.paths);
        panel.add(&self.knobs);
        panel.add(&self.occlusion_cost);
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
            knobs: self.knobs,
            occlusion_cost: self.occlusion_cost.clone(),
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "alcove: {} frames, {} ticks on the {} shell at {}x{} ({:?}), {:?} / {:?} / {:?}, \
             occlusion {}",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.exit,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.occlusion_cost.row(),
        );
    }
}

/// Where the camera is, as a panel section.
impl crcbl::ui::DebugModule for Alcove {
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
