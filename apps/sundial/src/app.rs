//! Sundial's start-up, and the methods the engine's loop calls.
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Sundial::tick     (the camera and the sun)
//!   draw_list.clear()
//!     ─────────────────────────────→ Sundial::draw      (hands both over)
//!     menu ───────────────────────→ Sundial::menu_kind
//!     debug overlay ──────────────→ Sundial::debug_sections
//!   gpu.frame()
//! ```
//!
//! There is no simulation in this file beyond a clock: what a shadow fixture has
//! instead of a game is a camera somebody moves, a sun that moves on its own, and
//! two knobs somebody turns.
//!
//! # The knobs are read, never kept — and the clock is kept, never read
//!
//! [`crate::filter`] is console state and lives in the console's own cells;
//! [`Sundial`] holds a *reading* of it, refreshed once per frame in
//! [`HostedGame::draw`], so the pause panel's labels, the debug overlay's
//! `shadow filter` section and the headless summary all report one instant — and
//! so that a line typed into the console moves the panel on the next frame rather
//! than being silently overwritten by it.
//!
//! **The clock is the other way round.** [`crate::sun::Clock`] is a tick counter
//! and lives here, because a tick is where the simulation has got to rather than
//! a setting: a console variable holding it would be a second writer racing the
//! `tick` method, and a run whose sun jumped when somebody typed would not be
//! deterministic.
//!
//! # The selectors are copied out of the GPU at start-up
//!
//! [`HostedGame::debug_sections`] is handed a panel and `&self`, and no GPU, so
//! [`Sundial`] keeps the [`Paths`] the device resolved, copied once from
//! [`Gpu::paths`] in `assemble` — which is also what puts them in [`Summary`],
//! where a headless run can print them. The shadow work's per-pass cost is the
//! same shape and the same reason.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock as ClockSource, ExitReason, FrameInfo, HostedGame, PointerUpdate, RunSummary,
    open_window, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::render::{DebugView, EffectRequest, Flyer, RenderEffects};
use crcbl::shell::{DisplayMode, PointerMode, ShellBackend as Backend, WindowDesc, WindowId};
use crcbl::ui::draw_list::DrawList;

use crate::args::Options;
use crate::filter::{self, Knobs, Step};
use crate::gpu::{Gpu, Paths, ShadowCost};
use crate::menu::{self, CameraMode, Menus, SundialAction};
use crate::plaza;
use crate::sun::{Clock, Sky};

/// How often [`Sundial::log_heartbeat`] logs, in ticks.
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
    /// The mode the window system actually had the window in, **not** the one the
    /// run last asked for.
    pub mode: DisplayMode,
    /// **Which of the three selectors the frames were drawn through**, and
    /// whether the run forced any of them.
    ///
    /// `docs/plan/sample/00-samples-overview.md` rule 12: the selected paths
    /// appear in the debug panel *and* in the headless summary line. This is the
    /// second of those — the panel is a windowed run's answer, and a CI job has no
    /// window.
    pub paths: Paths,
    /// Which camera the run ended on.
    pub camera: CameraMode,
    /// Where both filter knobs stood when the loop ended.
    pub knobs: Knobs,
    /// Where the scripted clock stood when the loop ended.
    pub clock: Clock,
    /// **What the shadow work cost, per pass.**
    ///
    /// `docs/plan/sample/18-sundial.md`'s "cost per technique, per frame", in the
    /// headless summary as well as on the panel.
    pub shadow_cost: ShadowCost,
}

/// Anything that can stop sundial before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these variants
/// for every sample, and a shadow fixture has no simulation of its own to fail.
pub type SundialError = crcbl::engine::LoopError;

/// Sundial, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Sundial {
    /// Which camera the next frame is drawn from.
    camera: CameraMode,
    /// The free camera, whether or not it is the one in use.
    flyer: Flyer,
    /// Where the sun is, and whether it is moving.
    clock: Clock,
    /// What the device resolved, copied once — see the module docs.
    paths: Paths,
    /// The three requested layers of topic 39's order, as the pause menu has
    /// them.
    effect_request: EffectRequest,
    /// The fourth layer, copied once: a row is built where there is no GPU to
    /// ask, and it cannot say "unavailable" without it.
    device_effects: RenderEffects,
    /// What both filter knobs read on the last frame drawn.
    knobs: Knobs,
    /// What the shadow work cost on the last frame whose timestamps landed.
    shadow_cost: ShadowCost,
    /// The values the pause panel was last built for; `None` until the first
    /// pause, so the panel is always rebuilt once with the real ones.
    shown: Option<(CameraMode, EffectRequest, Knobs, Clock, DebugView)>,
    /// Whether the loop has the simulation stopped, recorded in
    /// [`HostedGame::menu_kind`].
    paused: bool,
    /// Fixed steps run, for [`Sundial::log_heartbeat`]'s cadence.
    ticks: u64,
}

impl Sundial {
    /// A fixture starting on `camera` with its clock at `clock`, drawn through
    /// `paths` with `effects` asked for on a device that permits
    /// `device_effects`, and with the free camera at the fixed pose.
    #[must_use]
    pub fn new(
        camera: CameraMode,
        clock: Clock,
        paths: Paths,
        effects: EffectRequest,
        device_effects: RenderEffects,
    ) -> Self {
        Self {
            camera,
            flyer: Flyer::at(&plaza::fixed_camera()),
            clock,
            paths,
            effect_request: effects,
            device_effects,
            knobs: Knobs::read(),
            shadow_cost: ShadowCost::default(),
            shown: None,
            paused: false,
            ticks: 0,
        }
    }

    /// The `[HUD]` line, on the cadence every other sample's heartbeat uses.
    ///
    /// The one thing this fixture logs from inside the tick, and it is what a
    /// browser gate will read as well as what a person watching a headless run
    /// has. It names the lighting path, so a page that opened some other device is
    /// legible, and the filter, the seam and the sun, which are the whole of what
    /// this sample is for.
    ///
    /// **`view` is here for a browser gate specifically.** The debug views are
    /// the one control on this page whose effect is a *picture* — a whole-canvas
    /// reading cannot see it, because the atlas viewer replaces a lit frame with
    /// a grey one and a stat over the canvas mixes the scene with the HUD drawn
    /// over it. `crcbl::debug_view::current` is the cell every route to it
    /// writes, and printing it here is what makes
    /// `crate::web::__crcbl_sundial_atlas_view`'s effect a reading rather
    /// than an inference. `apps/quarry`'s heartbeat carries the same field for
    /// the same reason.
    ///
    /// **The two bias counts are here for that reason too.** What either of them
    /// moves is acne on the open pavement or a gap under the plinth — a few
    /// pixels of a lit surface, which no whole-canvas statistic separates from
    /// the sun having moved. Both are printed off [`Knobs`], which reads the
    /// console's own cells, so `crate::web::__crcbl_sundial_bias`'s effect is a
    /// reading here rather than an inference from the picture.
    fn log_heartbeat(&self) {
        if !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  lighting: {:?}  geometry: {:?}  binding: {:?}  camera: {}  \
             effects: {}  filter: {}  seam: {}  bias: {}  normal offset: {}  view: {}  \
             sun: {}",
            self.ticks,
            self.paths.lighting,
            self.paths.geometry,
            self.paths.binding,
            self.camera.label(),
            self.paths.effects_row(),
            self.knobs.filter.label(),
            self.knobs.seam_row(),
            Knobs::bias_row(self.knobs.bias()),
            Knobs::bias_row(self.knobs.offset()),
            crcbl::debug_view::current().label(),
            self.sky().row(),
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

    /// What both filter knobs read on the last frame drawn.
    #[must_use]
    pub const fn knobs(&self) -> Knobs {
        self.knobs
    }

    /// Where the scripted clock stands.
    #[must_use]
    pub const fn clock(&self) -> Clock {
        self.clock
    }

    /// Where the sun stands this frame.
    #[must_use]
    pub fn sky(&self) -> Sky {
        self.clock.sky()
    }

    /// The camera this frame is seen through.
    ///
    /// The two fixed poses have lenses of their own —
    /// [`plaza::counter_camera`]'s is wider, for the reason its doc gives — and
    /// the free camera borrows the fixed pose's, which is what makes a walked
    /// frame comparable with the golden it started from.
    #[must_use]
    pub fn camera(&self) -> crcbl::render::Camera {
        let fixed = plaza::fixed_camera();
        match self.camera {
            CameraMode::Fixed => fixed,
            CameraMode::Counters => plaza::counter_camera(),
            CameraMode::Free => self.flyer.camera(fixed.projection),
        }
    }
}

/// Every key this sample binds, the thing it does, and what the usage text calls
/// it.
///
/// **One table rather than a `match` beside a help paragraph**, for
/// `crate::menu::PRESSED_ROWS`' reason: a key and its description are one fact
/// about a control, and the way the two drift is a key that stops the sun while
/// the help says it cycles the filter.
type KeyBinding = (KeyCode, fn(), &'static str);

/// The bindings that write a console cell.
///
/// The clock's keys are not here: they move [`Sundial::clock`], which is state on
/// this type rather than in the console, so they cannot be a `fn()` with nothing
/// to write to. [`CLOCK_KEYS`] is theirs.
pub(crate) const KEYS: [KeyBinding; 5] = [
    (KeyCode::KeyF, cycle_filter, "F"),
    (KeyCode::KeyX, filter::toggle_seam, "X"),
    (KeyCode::KeyC, toggle_cascade_view, "C"),
    // **`T` for tiles**, which is what the picture is of: the letters this
    // fixture would rather have are taken — `A` and `S` are the flyer's, and the
    // flyer is offered every key before this table is walked, so a binding on
    // one of them would be unreachable rather than merely confusing.
    (KeyCode::KeyT, toggle_atlas_view, "T"),
    (KeyCode::KeyR, reset_all, "R"),
];

/// The `,` and `.` pair, which is not in [`KEYS`] because it only does anything
/// while the seam is up — [`filter::nudge_seam`] is where that is decided.
pub(crate) const SEAM_KEYS: [(KeyCode, bool, &str); 2] =
    [(KeyCode::Comma, false, ","), (KeyCode::Period, true, ".")];

/// The pairs that walk the sun's bias counts: `[` and `]` for the constant bias,
/// `;` and `'` for the normal offset, and a coarse pair each beside them.
///
/// Not in [`KEYS`] for [`SEAM_KEYS`]' reason — an entry there is a bare `fn()`
/// and these carry which cell they write, which way and how far — and **a pair
/// each**, because the whole of `docs/plan/sample/18-sundial.md`'s milestone 2 is
/// the two counts moving against each other: a single key that cycled through
/// presets could not put one at zero while the other stands where it ships.
///
/// **Bracket for the light-ward one and quote for the sideways one**, which is
/// as much mnemonic as this fixture claims: the letters that would be better are
/// taken. `A` and `S` are the flyer's, and the flyer is offered every key before
/// this table is walked.
///
/// # Why a second pair each, and not `Shift` on the first
///
/// [`filter::Step::Fine`] cannot reach the far end of either count's range in a
/// walk anybody would make, and that end — the plinth with no shadow under it at
/// all — is half of what milestone 2 is about. A modifier is the usual way to
/// hang a coarser step off the same keys, and neither half of one is available
/// here:
///
/// * **Nothing delivers a modifier to a game.** `crcbl-shell` stamps
///   [`crcbl::core::input::Modifiers`] onto every key event it produces, and
///   [`crcbl::engine::HostedGame::key_event`] takes a key and an edge; the loop
///   drops the rest at that boundary. Widening the trait is engine work, and
///   every sample implements it.
/// * **`Shift` is the free camera's.** [`Flyer::key`] binds it to descend, and
///   the flyer is offered every key before this table is walked — so a chord
///   would sink the camera out of the pose the goldens are taken from while the
///   count moved, which is the one thing a comparison fixture cannot afford.
///
/// So a second pair each, on the digit row, which is where this fixture still
/// had adjacent pairs free — outer pair to the outer count, so the coarse pairs
/// sit in the same order as the fine ones above them.
pub(crate) const BIAS_KEYS: [(KeyCode, &str, bool, Step, &str); 8] = [
    (KeyCode::BracketLeft, filter::BIAS, false, Step::Fine, "["),
    (KeyCode::BracketRight, filter::BIAS, true, Step::Fine, "]"),
    (KeyCode::Semicolon, filter::OFFSET, false, Step::Fine, ";"),
    (KeyCode::Quote, filter::OFFSET, true, Step::Fine, "'"),
    (KeyCode::Digit9, filter::BIAS, false, Step::Coarse, "9"),
    (KeyCode::Digit0, filter::BIAS, true, Step::Coarse, "0"),
    (KeyCode::Digit7, filter::OFFSET, false, Step::Coarse, "7"),
    (KeyCode::Digit8, filter::OFFSET, true, Step::Coarse, "8"),
];

/// What one of [`CLOCK_KEYS`] does to the clock.
///
/// An enum rather than a `fn(&mut Clock)` because a function pointer in a
/// `const` cannot take a method: the three arms below are `Clock`'s own, and this
/// is the smallest thing that lets one table carry them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClockKey {
    /// Start the clock, or stop it.
    Toggle,
    /// Move it one [`crate::sun::SCRUB_STEP`] back, and stop it.
    Back,
    /// Move it one step forward, and stop it.
    Forward,
}

/// The clock's own keys: stop and start it, and scrub it either way.
///
/// A separate table because these reach `&mut self` rather than a console cell.
pub(crate) const CLOCK_KEYS: [(KeyCode, ClockKey, &str); 3] = [
    (KeyCode::KeyP, ClockKey::Toggle, "P"),
    (KeyCode::Minus, ClockKey::Back, "-"),
    (KeyCode::Equal, ClockKey::Forward, "="),
];

fn cycle_filter() {
    filter::cycle(filter::FILTER);
}

/// Swaps between the shaded picture and that picture tinted by cascade.
///
/// What `C` and the panel's `CASCADES` row both do. `docs/plan/45-shadows.md`'s
/// eighth decision made the cascade switch a band rather than a step, and this
/// is the picture that band is looked at in — which is the one thing a shadow
/// fixture could not show before the overlay existed.
pub fn toggle_cascade_view() {
    crcbl::debug_view::toggle(DebugView::Cascades);
}

/// Swaps between the frame and the shadow atlas drawn over it.
///
/// What `T` and the panel's `ATLAS` row both do —
/// `docs/plan/sample/18-sundial.md`'s milestone 1 atlas viewer. The plaza's sun
/// and its two lamps compete for the tiles `crcbl::render::shadow` budgets, and
/// which of them got one is the question this fixture had no way to ask: a light
/// that was refused a tile still lights, so the frame looks the same either way.
pub fn toggle_atlas_view() {
    crcbl::debug_view::toggle(DebugView::ShadowAtlas);
}

/// Both halves of `R`: the console's knobs and this run's clock.
///
/// The clock half cannot be done from a `fn()`, so [`KEYS`]' entry resets the
/// knobs and [`HostedGame::key_event`] resets the clock beside it — which is why
/// this is the one binding whose two halves are not in one place, and why
/// [`SundialAction::Reset`] does both.
fn reset_all() {
    filter::reset();
}

/// The loop sundial runs in. A type alias, because the loop is the engine's.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Sundial>;

/// Runs the full loop.
///
/// # Errors
///
/// [`SundialError`] if the shell or the GPU refused. Teardown runs on every path:
/// a failing frame must still release the swapchain, the surface and the window.
pub fn run(options: &Options) -> Result<Summary, SundialError> {
    let summary = crcbl::engine::drive(start(options)?);
    // **The knobs go back on the way out**, whatever happened. A run that moved
    // `r_shadow_filter` and exited would otherwise leave the cell moved for
    // anything sharing this process — which is every test in this crate, and the
    // golden suite's own second frame.
    filter::reset();
    summary
}

/// Opens a shell, a window, a GPU and the plaza.
///
/// # Errors
///
/// [`SundialError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, SundialError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits.
///
/// The browser cannot use this — a main thread may not sit in a blocking
/// [`wait_for_configure`] — and takes [`PendingLoop`] instead. What the two share
/// is everything after the waiting, which is `assemble`.
///
/// # Errors
///
/// [`SundialError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, SundialError> {
    let clock_source = ClockSource::new(options.common.headless);
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
/// [`SundialError`] if the shell refused it.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &ClockSource,
    options: &Options,
) -> Result<WindowId, SundialError> {
    Ok(open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Crucible — sundial",
            app_id: "sh.kryptic.crcbl.sundial",
            // 4:3, so a windowed frame and a golden are the same framing: the
            // fixed camera's field of view is vertical, and a different aspect
            // crops or reveals the ends of the colonnade.
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
    // The starting filter state, into the cells `crate::filter` reads — before
    // the first frame and before the pause panel is first built, so a run started
    // with `--filter box` shows that on its first pause.
    options.apply();

    let paths = booted.gpu.paths();
    let effects = booted.gpu.effect_request();
    let device_effects = booted.gpu.device_effects();

    Loop::new(
        booted,
        Sundial::new(
            options.camera,
            options.clock(),
            paths,
            effects,
            device_effects,
        ),
        options.common.loop_config(),
    )
}

impl HostedGame for Sundial {
    /// A shadow fixture has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the whole of its state machine.
    type MenuKind = bool;
    type MenuAction = SundialAction;
    type Summary = Summary;

    const NAME: &'static str = "sundial";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a tick period is a fraction of a second"
        )]
        let dt = tick_dt as f32;
        self.ticks += 1;
        // **The sun advances on the fixed step and on nothing else.** That is the
        // whole of the determinism claim: at any tick rate, on any machine, tick
        // `k` of the clock is the same sun.
        self.clock.advance();
        // The camera integrates whether or not it is the one being drawn from: a
        // reviewer who swaps to a fixed pose, walks, and swaps back should arrive
        // where the keys took them.
        self.flyer.advance(dt);
        self.log_heartbeat();
    }

    /// The camera's keys, and this sample's own.
    ///
    /// The flyer is offered every key first and reports whether it took one, so a
    /// binding below can never shadow `WASD` — see
    /// [`crcbl::render::Flyer::key`], whose return value is exactly that answer.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if self.flyer.key(key, pressed) || !pressed {
            return;
        }
        for (bound, act, _) in KEYS {
            if bound == key {
                act();
                // `R` puts the clock back as well as the knobs — see `reset_all`.
                if bound == KeyCode::KeyR {
                    self.clock.reset();
                }
                return;
            }
        }
        for (bound, right, _) in SEAM_KEYS {
            if bound == key {
                filter::nudge_seam(right);
                return;
            }
        }
        for (bound, cell, up, step, _) in BIAS_KEYS {
            if bound == key {
                filter::nudge_bias(cell, up, step);
                return;
            }
        }
        for (bound, act, _) in CLOCK_KEYS {
            if bound == key {
                match act {
                    ClockKey::Toggle => self.clock.toggle(),
                    ClockKey::Back => self.clock.scrub(false),
                    ClockKey::Forward => self.clock.scrub(true),
                }
                return;
            }
        }
    }

    /// The mouse look, and the one condition it is bound under.
    ///
    /// **`at.is_none()` is what says the pointer is really captured** — see
    /// [`PointerUpdate::motion`], and `apps/lantern/src/app.rs`, which carries the
    /// argument in full.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let Some(motion) = pointer.motion.filter(|_| pointer.at.is_none()) else {
            return;
        };
        self.flyer.look(motion);
    }

    /// [`PointerMode::Locked`] while the plaza is being flown, free while the
    /// pause panel is up.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<SundialAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: SundialAction) {
        match action {
            SundialAction::CycleCamera => {
                self.camera = self.camera.next();
                // Arriving back at a fixed pose is also how a reviewer gets back
                // to the golden framing, so the free camera is put back there.
                if self.camera != CameraMode::Free {
                    self.flyer = Flyer::at(&plaza::fixed_camera());
                }
                // And nothing is held down after a menu press: the press happened
                // while the menu owned the keyboard, so a key that was down when
                // the panel opened has no release coming.
                self.flyer.release_all();
            }
            // Read-modify-write on the layer a panel owns, leaving the camera
            // stack and `[engine.video]` as they were. It reaches the renderer in
            // `draw`.
            SundialAction::ToggleEffect(effect) => {
                self.effect_request =
                    menu::toggled_effect(self.effect_request, self.device_effects, effect);
            }
            SundialAction::CycleFilter => filter::cycle(filter::FILTER),
            SundialAction::ToggleCascades => toggle_cascade_view(),
            SundialAction::ToggleAtlas => toggle_atlas_view(),
            SundialAction::ToggleSeam => filter::toggle_seam(),
            SundialAction::ToggleSun => self.clock.toggle(),
            SundialAction::Reset => {
                filter::reset();
                self.clock.reset();
            }
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        // Recorded for `pointer_mode`, which the loop polls immediately after
        // this: a panel that went up on this frame must free the pointer on this
        // frame.
        self.paused = paused;
        // Read here as well as in `draw`, because `draw` does not run before the
        // first panel is laid out and a knob a key moved while paused has to reach
        // the label on the same frame.
        let knobs = Knobs::read();
        // The debug view is process-global console state like the knobs, so it
        // is read here for the same reason and joins the rebuild key beside
        // them: `C` or `T` pressed while paused has to move the `CASCADES` and
        // `ATLAS` rows on the frame it is pressed on.
        //
        // **The view itself and not a flag per row.** The engine holds exactly
        // one — `crcbl::debug_view::current` is the cell — so a pair of flags
        // here would be a key that can spell a state the engine cannot be in.
        let view = crcbl::debug_view::current();
        if paused && self.shown != Some((self.camera, self.effect_request, knobs, self.clock, view))
        {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a press
            // on a row does not throw the reviewer back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(
                true,
                menu::pause_menu(
                    self.camera,
                    self.effect_request,
                    self.device_effects,
                    knobs,
                    self.clock,
                    view,
                ),
            );
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some((self.camera, self.effect_request, knobs, self.clock, view));
        }
        self.knobs = knobs;
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, _frame: FrameInfo) {
        // The fixture draws no HUD of its own: everything it has to say about a
        // frame is a debug-panel row.
        gpu.set_camera(self.camera());
        gpu.set_sun(self.sky().light());
        // Here rather than in `tick`, which does not run while paused: the row
        // that was just pressed is on a panel over a frame that has to change
        // behind it.
        gpu.set_effect_request(self.effect_request);
        // Re-read rather than recomputed: the device clamps last, so what the
        // panel and the summary report comes back off the renderer.
        self.paths = gpu.paths();
        self.knobs = Knobs::read();
        self.shadow_cost = gpu.shadow_cost();
    }

    /// Four sections, and each is something the charter asks for.
    ///
    /// Rule 12's path reporting is the first. The second is this sample's whole
    /// subject — the filter and, while the seam is up, the one on each side of it.
    /// The third is where the sun stands, which is the other half of a frame
    /// nobody can reproduce without. The fourth is what the shadow work cost.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.paths);
        panel.add(&self.knobs);
        panel.add(&self.shadow_cost);
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
            clock: self.clock,
            shadow_cost: self.shadow_cost.clone(),
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "sundial: {} frames, {} ticks on the {} shell at {}x{} ({:?}), {:?} / {:?} / {:?}, \
             filter {}, sun {}, cost {}",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.exit,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.knobs.filter.label(),
            summary.clock.sky().row(),
            summary.shadow_cost.row(),
        );
    }
}

/// Where the camera is and where the sun stands, as a panel section.
impl crcbl::ui::DebugModule for Sundial {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        let eye = self.camera().eye;
        let sky = self.sky();
        section.set_title("sun");
        section.row_str(
            "clock",
            if self.clock.running() {
                "RUNNING"
            } else {
                "STOPPED"
            },
        );
        section.row("tick", format_args!("{}", sky.tick));
        section.row(
            "elevation",
            format_args!("{:.1} deg", sky.elevation.to_degrees()),
        );
        section.row(
            "azimuth",
            format_args!("{:+.1} deg", sky.azimuth.to_degrees()),
        );
        section.row_str("camera", self.camera.label());
        section.row(
            "eye",
            format_args!("{:.2} {:.2} {:.2}", eye.x, eye.y, eye.z),
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

/// A [`Loop`] being started one poll at a time, for a caller that may not block —
/// which on a browser main thread is every caller.
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
    /// [`ClockSource::new`]'s: `std::time::Instant::now` panics on
    /// `wasm32-unknown-unknown`, so a page drives the loop from
    /// `performance.now()` instead.
    ///
    /// # Errors
    ///
    /// [`SundialError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: ClockSource,
    ) -> Result<Self, SundialError> {
        let window = open_the_window(shell.as_mut(), &clock_source, options)?;
        Ok(Self {
            boot: crcbl::engine::PolledBoot::request(
                shell,
                window,
                clock_source,
                options.common.gpu(),
                (),
            ),
            options: options.clone(),
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`SundialError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, SundialError> {
        let Some(booted) = self.boot.poll::<SundialError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun::{FIXTURE_TICK, SCRUB_STEP};

    /// **Every key this sample binds is spelled in the usage text**, and no two
    /// bindings take the same key.
    ///
    /// The failure this catches is a key added to one of the three tables and
    /// left out of `--help`, which is a control nobody can find, and a key bound
    /// twice, which is a control that does whichever of two things the tables
    /// happen to be walked in.
    #[test]
    fn every_key_is_bound_once_and_named_in_the_help() {
        let mut bound: Vec<KeyCode> = Vec::new();
        let mut named: Vec<&str> = Vec::new();
        for (key, _, name) in KEYS {
            bound.push(key);
            named.push(name);
        }
        for (key, _, name) in SEAM_KEYS {
            bound.push(key);
            named.push(name);
        }
        for (key, _, _, _, name) in BIAS_KEYS {
            bound.push(key);
            named.push(name);
        }
        for (key, _, name) in CLOCK_KEYS {
            bound.push(key);
            named.push(name);
        }
        assert!(bound.len() > 5, "the fixture binds {} keys", bound.len());
        for (at, key) in bound.iter().enumerate() {
            assert!(
                !bound[..at].contains(key),
                "{key:?} is bound twice, so one of the two controls is unreachable"
            );
        }
        // The `KEYS:` block alone, and a key is named only where a row of it
        // starts with that key: `USAGE.contains("C")` is true of "CI" and of
        // "OPTIONS", which is a check that cannot fail.
        let keys = crate::USAGE
            .split("KEYS:")
            .nth(1)
            .expect("the usage text has a KEYS: block");
        let names_a_row = |name: &str| {
            keys.lines().any(|line| {
                line.strip_prefix("    ")
                    .and_then(|row| row.split("  ").next())
                    .is_some_and(|spelt| spelt.split(' ').any(|token| token == name))
            })
        };
        for name in named {
            assert!(
                names_a_row(name),
                "the usage text's KEYS: block has no row for the {name} key"
            );
        }
        assert!(
            !names_a_row("Q"),
            "a key nothing binds is named in the KEYS: block, so the check above is not \
             reading rows"
        );
    }

    /// **Each bias key walks the count the pause panel says it does**, the way
    /// that panel's own row says it walks it, and leaves the other count alone.
    ///
    /// **Checked against `crate::menu`'s hint strings rather than against
    /// [`BIAS_KEYS`] itself.** A check that read which cell a row names and then
    /// asserted that row moved it would agree with any swap of the two: the
    /// table would be its own answer key, which is a green light wired to
    /// nothing. The panel is the independent statement — it is what a reviewer
    /// reads before pressing anything — so a row filed under the wrong count,
    /// walking the wrong way, or spending the wrong step is a disagreement
    /// between two files rather than one file agreeing with itself. The hint
    /// prints each count's keys in order: down then up, fine pair then coarse.
    ///
    /// **Driven from the middle of each range**, so a press is the step rather
    /// than the step clamped; `crate::filter`'s own
    /// `the_coarse_pair_reaches_the_far_end_of_a_range_and_lands_on_the_fine_step`
    /// is where the ends are walked.
    #[test]
    fn each_bias_key_walks_the_count_the_panel_says_it_does() {
        /// A pair is two keys, one down and one up, and the panel prints them in
        /// that order.
        const PAIR: usize = 2;

        let _held = filter::held();
        let mut fixture = Sundial::new(
            CameraMode::Fixed,
            Clock::default(),
            Paths {
                geometry: crcbl::hal::GeometryPath::MeshShader,
                binding: crcbl::hal::BindingModel::Bindless,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: RenderEffects::DEFAULT_STACK,
            },
            EffectRequest::default(),
            RenderEffects::all(),
        );

        // What the pause panel's row for one count prints, as the keys it names
        // in the order it names them.
        let printed = |cell: &str| -> Vec<&'static str> {
            let hint = if cell == filter::BIAS {
                menu::BIAS_KEYS
            } else {
                menu::OFFSET_KEYS
            };
            hint.split_whitespace()
                .filter(|token| *token != "/")
                .collect()
        };

        // The cell each key wrote, where the panel spells that key, and how far
        // one press took the count.
        let mut walked: Vec<(&str, usize, f32)> = Vec::new();
        for (key, cell, _, _, name) in BIAS_KEYS {
            let other = if cell == filter::BIAS {
                filter::OFFSET
            } else {
                filter::BIAS
            };
            let keys = printed(cell);
            let at = keys
                .iter()
                .position(|spelt| *spelt == name)
                .unwrap_or_else(|| {
                    panic!("{name} writes {cell}, whose panel row names {keys:?} instead")
                });

            filter::reset();
            let start = filter::set_bias(cell, filter::ceiling(cell) / 2.0);
            let untouched = filter::var(other).get_f32();

            fixture.key_event(key, true);
            let moved = filter::var(cell).get_f32() - start;
            assert!(
                (filter::var(other).get_f32() - untouched).abs() < 1e-5,
                "{name} moved {other} as well as {cell}"
            );
            assert_eq!(
                moved < 0.0,
                at % PAIR == 0,
                "{name} took {cell} from {start} to {}, and its panel row prints it as the \
                 {} of its pair",
                filter::var(cell).get_f32(),
                if at % PAIR == 0 { "down key" } else { "up key" }
            );

            // A release is not a press, on the clock keys' check's terms: a
            // fixture that acted on both edges would walk every count twice as
            // far as the finger asked for.
            fixture.key_event(key, false);
            assert!(
                (filter::var(cell).get_f32() - (start + moved)).abs() < 1e-5,
                "{name} acted on its release as well as on its press"
            );
            walked.push((cell, at, moved.abs()));
        }

        for cell in [filter::BIAS, filter::OFFSET] {
            let keys = printed(cell);
            let mut pairs: Vec<f32> = Vec::new();
            for (at, spelt) in keys.iter().enumerate() {
                let (_, _, far) = walked
                    .iter()
                    .find(|(wrote, spot, _)| *wrote == cell && *spot == at)
                    .unwrap_or_else(|| {
                        panic!("{cell}'s panel row names {spelt}, which no key is bound to")
                    });
                if at % PAIR == 0 {
                    pairs.push(*far);
                } else {
                    assert!(
                        (*far - pairs[at / PAIR]).abs() < 1e-5,
                        "{cell}'s {spelt} walks {far} texels and its own pair walks {}",
                        pairs[at / PAIR]
                    );
                }
            }
            assert!(
                pairs.len() > 1,
                "{cell} has one pair, so no press reaches the end of its range"
            );
            for (at, far) in pairs.iter().enumerate().skip(1) {
                assert!(
                    *far > pairs[at - 1],
                    "{cell}'s pairs walk {:?} texels a press, so a later pair is no coarser \
                     than the one the panel prints before it",
                    pairs
                );
            }
        }
    }

    /// **`R` puts the clock back as well as the knobs**, and so does the panel's
    /// RESET row.
    ///
    /// The two halves live in different places — `reset_all` writes the console
    /// and `key_event` writes the clock — which is exactly the shape that drifts,
    /// so both routes are driven here rather than one.
    ///
    /// [`crate::filter::held`] because the console half of `R` writes every cell
    /// the other checks of them are reading: a reset landing in the middle of one
    /// of those is a knob it did not ask for.
    #[test]
    fn reset_puts_the_clock_back_by_key_and_by_row() {
        let _held = filter::held();
        let mut fixture = Sundial::new(
            CameraMode::Fixed,
            Clock::at(FIXTURE_TICK + 3 * SCRUB_STEP, false),
            Paths {
                geometry: crcbl::hal::GeometryPath::IndirectPerBatch,
                binding: crcbl::hal::BindingModel::ArrayPages,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: RenderEffects::DEFAULT_STACK,
            },
            EffectRequest::default(),
            RenderEffects::all(),
        );
        assert_ne!(fixture.clock(), Clock::default());

        fixture.key_event(KeyCode::KeyR, true);
        assert_eq!(
            fixture.clock(),
            Clock::default(),
            "the R key left the clock where it was"
        );

        fixture.clock = Clock::at(7, false);
        fixture.apply(SundialAction::Reset);
        assert_eq!(
            fixture.clock(),
            Clock::default(),
            "the RESET row left the clock where it was"
        );
    }

    /// **The clock's keys stop it, start it and scrub it**, and a key nothing
    /// binds moves nothing.
    #[test]
    fn the_clock_keys_stop_start_and_scrub_the_sun() {
        let mut fixture = Sundial::new(
            CameraMode::Fixed,
            Clock::default(),
            Paths {
                geometry: crcbl::hal::GeometryPath::MeshShader,
                binding: crcbl::hal::BindingModel::Bindless,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: RenderEffects::DEFAULT_STACK,
            },
            EffectRequest::default(),
            RenderEffects::all(),
        );
        assert!(fixture.clock().running());

        fixture.key_event(KeyCode::KeyP, true);
        assert!(!fixture.clock().running(), "P did not stop the sun");
        fixture.key_event(KeyCode::KeyP, true);
        assert!(fixture.clock().running(), "P did not start it again");

        fixture.key_event(KeyCode::Equal, true);
        assert_eq!(fixture.clock().tick(), FIXTURE_TICK + SCRUB_STEP);
        assert!(!fixture.clock().running(), "a scrub stops the clock");
        fixture.key_event(KeyCode::Minus, true);
        assert_eq!(fixture.clock().tick(), FIXTURE_TICK);

        let before = fixture.clock();
        fixture.key_event(KeyCode::KeyJ, true);
        assert_eq!(fixture.clock(), before, "an unbound key moved the clock");
        // A release is not a press: every binding above is on the down edge, and
        // a fixture that acted on both would do everything twice.
        fixture.key_event(KeyCode::KeyP, false);
        assert_eq!(fixture.clock(), before, "a key release acted like a press");
    }

    /// **Each debug-view key shows its picture, a second press takes it away,
    /// the pause panel names which of the two the frame is drawing, and a
    /// second key *replaces* the first.**
    ///
    /// The two halves live in different files — this one's key table writes the
    /// engine's `debug_view` cell and [`crate::menu::pause_menu`] reads it back
    /// — which is exactly the shape that drifts into a key toggling a picture no
    /// panel names. So each row is built from the value its key moved rather
    /// than from a literal, and the panel's own press is driven beside the key,
    /// because a [`SundialAction`] is a second route to the same cell.
    ///
    /// **One table over both keys rather than a check each**, and the exclusivity
    /// at the end is why it is worth being one: the engine holds exactly one
    /// debug view, so `T` pressed while `C`'s overlay is up must leave the panel
    /// reading `ON` once. Two separate checks, each starting from the shaded
    /// frame, could not see that at all — and a panel that read `ON` twice about
    /// one picture is a panel nobody can act on.
    ///
    /// [`crcbl::debug_view::for_test`] is held for the module docs' reason: the
    /// view is a process-global console variable and `cargo test` runs a crate's
    /// checks as threads of one process.
    #[test]
    fn each_debug_view_key_shows_its_picture_and_the_panel_names_it() {
        let _view = crcbl::debug_view::for_test();
        let mut fixture = Sundial::new(
            CameraMode::Fixed,
            Clock::default(),
            Paths {
                geometry: crcbl::hal::GeometryPath::MeshShader,
                binding: crcbl::hal::BindingModel::Bindless,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: RenderEffects::DEFAULT_STACK,
            },
            EffectRequest::default(),
            RenderEffects::all(),
        );
        // The panel as it stands right now, row by row — built from the cell the
        // key moved, exactly as `menu_kind` builds it.
        let rows = || {
            menu::pause_menu(
                CameraMode::Fixed,
                EffectRequest::default(),
                RenderEffects::all(),
                Knobs::read(),
                Clock::default(),
                crcbl::debug_view::current(),
            )
            .items()
            .iter()
            .map(|item| (item.id, item.label.clone()))
            .collect::<Vec<_>>()
        };
        let row = |id| {
            rows()
                .into_iter()
                .find(|(found, _)| *found == id)
                .unwrap_or_else(|| panic!("the panel has no row {id:?}"))
                .1
        };
        // Every debug-view row the panel carries, so the count below is about
        // the panel rather than about the two this check drives.
        let views: [(KeyCode, DebugView, crcbl::ui::WidgetId, &str, SundialAction); 2] = [
            (
                KeyCode::KeyC,
                DebugView::Cascades,
                menu::CASCADES_ID,
                "CASCADES",
                SundialAction::ToggleCascades,
            ),
            (
                KeyCode::KeyT,
                DebugView::ShadowAtlas,
                menu::ATLAS_ID,
                "ATLAS",
                SundialAction::ToggleAtlas,
            ),
        ];
        // How many rows read `ON` — the count that says the panel is describing
        // one picture.
        let showing = || {
            views
                .iter()
                .filter(|(_, _, id, _, _)| row(*id).ends_with(": ON"))
                .count()
        };

        for (key, view, id, name, action) in views {
            assert_eq!(
                crcbl::debug_view::current(),
                DebugView::Shaded,
                "{name} starts from a frame that is already showing something"
            );
            assert_eq!(row(id), format!("{name}: OFF"));

            fixture.key_event(key, true);
            assert_eq!(
                crcbl::debug_view::current(),
                view,
                "{key:?} did not put {view:?} in force"
            );
            assert_eq!(
                row(id),
                format!("{name}: ON"),
                "the panel does not follow {key:?}"
            );

            fixture.key_event(key, true);
            assert_eq!(
                crcbl::debug_view::current(),
                DebugView::Shaded,
                "a second press left {view:?} up, so {key:?} cannot take it back"
            );
            assert_eq!(row(id), format!("{name}: OFF"));

            // The panel's own press, which is the other route to the same cell.
            fixture.apply(action);
            assert_eq!(
                crcbl::debug_view::current(),
                view,
                "the {name} row did not show {view:?}"
            );
            assert_eq!(row(id), format!("{name}: ON"));

            // And a key release is not a press, on the clock keys' terms.
            fixture.key_event(key, false);
            assert_eq!(
                crcbl::debug_view::current(),
                view,
                "a key release acted like a press"
            );

            assert_eq!(showing(), 1, "{name} is up and the panel says otherwise");
            fixture.apply(action);
        }

        // **A second view replaces the first**, which is what makes the panel's
        // two rows one reading. Driven from the keys rather than from the cell,
        // because the key is what a reviewer presses.
        fixture.key_event(KeyCode::KeyC, true);
        fixture.key_event(KeyCode::KeyT, true);
        assert_eq!(crcbl::debug_view::current(), DebugView::ShadowAtlas);
        assert_eq!(
            showing(),
            1,
            "the panel reads ON for two rows about one picture: {:?}",
            rows()
        );
        assert_eq!(row(menu::CASCADES_ID), "CASCADES: OFF");
    }

    /// **The camera row cycles all three poses and the sky follows the clock**,
    /// which is the pair `debug_sections` prints and the summary carries.
    #[test]
    fn the_camera_cycles_and_the_sky_follows_the_clock() {
        let mut fixture = Sundial::new(
            CameraMode::Fixed,
            Clock::default(),
            Paths {
                geometry: crcbl::hal::GeometryPath::MeshShader,
                binding: crcbl::hal::BindingModel::Bindless,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: RenderEffects::DEFAULT_STACK,
            },
            EffectRequest::default(),
            RenderEffects::all(),
        );
        assert_eq!(fixture.camera().eye, plaza::fixed_camera().eye);
        fixture.apply(SundialAction::CycleCamera);
        assert_eq!(fixture.camera_mode(), CameraMode::Counters);
        assert_eq!(fixture.camera().eye, plaza::counter_camera().eye);
        fixture.apply(SundialAction::CycleCamera);
        assert_eq!(fixture.camera_mode(), CameraMode::Free);
        fixture.apply(SundialAction::CycleCamera);
        assert_eq!(fixture.camera_mode(), CameraMode::Fixed);

        assert_eq!(fixture.sky(), Sky::at(FIXTURE_TICK));
        fixture.clock = Clock::at(FIXTURE_TICK + 40, true);
        assert_eq!(fixture.sky(), Sky::at(FIXTURE_TICK + 40));
        assert_ne!(
            fixture.sky().towards(),
            Sky::at(FIXTURE_TICK).towards(),
            "the sky did not move with the clock"
        );
    }
}
