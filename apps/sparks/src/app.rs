//! Sparks' start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Sparks::tick   (one particle step)
//!   draw_list.clear()
//!     ─────────────────────────────→ Sparks::draw    (camera, instances, panel)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! What is left here is start-up, because a window's title is this sample's,
//! and the trait methods, because they are what a hosted game is.
//!
//! # The simulation is on the tick and the drawing is on the frame
//!
//! [`Sparks::tick`] steps [`Show`] by the *fixed* timestep and nothing else,
//! which is what makes the run replayable from its seed at all — a simulation
//! advanced by a variable frame time would produce a different spray on every
//! machine. [`Sparks::draw`] then reads whatever the last tick left and points
//! the instances at it. Between the two, a frame drawn twice against one tick
//! draws the same particles twice, which is a still frame rather than a wrong
//! one.

use crcbl::core::input::KeyCode;
use crcbl::engine::{Booted, Clock, FrameInfo, HostedGame, RunSummary, wait_for_configure};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, WindowId};
use crcbl::ui::{DebugModule, DebugSection};

use crate::gpu::Gpu;
use crate::menu::{MenuKind, Menus};
use crate::show::{Reading, Show};

pub use crate::args::Options;

// ---- defaults ----------------------------------------------------------------

/// How often [`Sparks::log_heartbeat`] logs, in ticks.
///
/// Half a second at [`crate::show::TICK_HZ`], rather than the second every
/// other sample uses. The browser gate reads the smoke puff's count on **both
/// sides of a stop**, and the puff's whole cycle is
/// [`crate::show::PUFF_ON_TICKS`] plus [`crate::show::PUFF_OFF_TICKS`]; at one
/// line a second the drained part of that window is a couple of lines, and a
/// gate that has two samples to work with is one that flakes.
const HEARTBEAT_TICKS: u64 = 30;

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// How many particles were alive on the last frame.
    pub live: u32,
    /// How many instances the last frame pointed at them. Zero would mean a run
    /// that simulated an effect nobody drew, which is the one failure a
    /// headless smoke test could otherwise report as a pass.
    pub drawn: usize,
    /// How many spawns the hostile effect's budget refused over the run. Zero
    /// would mean the clamp never fired.
    pub clamped: u64,
    /// How many commands the last page drew.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop sparks.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample, and a show built from a seed has nothing of its
/// own to fail at — [`Show::new`] cannot refuse — so it takes the default type
/// parameter and its `Game` variant is uninhabited.
pub type SparksError = crcbl::engine::LoopError;

// ---- the debug panel ---------------------------------------------------------

/// Sparks' section of the debug panel: `docs/plan/20-particles.md`'s "live
/// effects, particle counts vs budgets, pool occupancy".
#[derive(Debug)]
struct Stats<'a> {
    show: &'a Show,
    reading: Reading,
    drawn: usize,
    commands: usize,
}

impl DebugModule for Stats<'_> {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("vfx");
        out.row("tick", format_args!("{}", self.show.tick_count()));
        out.row(
            "live",
            format_args!("{}/{}", self.reading.live, self.reading.capacity),
        );
        out.row(
            "pool",
            format_args!("{}/{}", self.reading.reserved, self.reading.capacity),
        );
        out.row("effects", format_args!("{}", self.reading.effects));
        // One row per effect: what it holds against what it was allowed, which
        // is the panel reading the budget claim is made of.
        for effect in self.show.vfx().effects() {
            out.row(
                "effect",
                format_args!(
                    "{}/{} reserved {} clamped {}",
                    effect.live,
                    effect.budget,
                    effect.reserved,
                    effect.clamped()
                ),
            );
        }
        // The dogfood rows, and the ones that separate "the simulation ran" from
        // "the frame drew it".
        out.row("instances", format_args!("{}", self.drawn));
        out.row("commands", format_args!("{}", self.commands));
    }
}

// ---- the hosted game ---------------------------------------------------------

/// Sparks, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Sparks {
    show: Show,
    /// How many seconds the frame clock has run for — the camera's orbit, which
    /// is presentation and therefore not on the tick.
    elapsed: f64,
    /// How many instances the last [`Sparks::draw`] pointed at particles.
    drawn: usize,
    /// How many commands the last one emitted.
    commands: usize,
}

/// The loop sparks runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Sparks>;

/// Runs the full loop.
///
/// # Errors
///
/// [`SparksError`] if the shell or the GPU failed. Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, SparksError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the show.
///
/// # Errors
///
/// [`SparksError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, SparksError> {
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
/// [`SparksError`] if the window never configured or the GPU would not open.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, SparksError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut |_| events += 1)?;

    let gpu = Gpu::open(shell.as_ref(), window, extent, options.common.gpu())?;
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

/// The half of start-up that is the same however the GPU arrived.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    let booted = crcbl::engine::arm_screenshot(booted, &options.common);
    Loop::new(
        booted,
        Sparks {
            show: Show::new(options.seed),
            elapsed: 0.0,
            drawn: 0,
            commands: 0,
        },
        options.common.loop_config(),
    )
}

/// Creates the one window this sample has: its title, its app id, its size.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    mode: DisplayMode,
    size: Option<crcbl::shell::PhysicalSize>,
) -> Result<WindowId, SparksError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Sparks",
            app_id: "sh.kryptic.crcbl.sparks",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Sparks {
    /// The show, for scripted tests and for an embedder that drives it.
    pub const fn show(&self) -> &Show {
        &self.show
    }

    /// How many instances the last frame pointed at particles.
    pub const fn drawn(&self) -> usize {
        self.drawn
    }

    /// How many commands the last frame's page drew.
    pub const fn commands(&self) -> usize {
        self.commands
    }

    /// The `[HUD]` line, every [`HEARTBEAT_TICKS`] steps.
    ///
    /// It is the only thing this sample logs from inside the tick, and
    /// `web/tools/browser-e2e.mjs` reads three claims out of it.
    ///
    /// One is the heartbeat itself — it exists while the demo runs and stops
    /// while it is paused, which is how a browser tells a paused loop from a
    /// running one.
    ///
    /// The second is that an emitter's count **rises while it emits and comes
    /// back to nothing after it stops**, which is `puff` beside
    /// `puff-emitting`. Sparks takes no input, so nothing external can be shown
    /// to have reached it; the pair has to be read off numbers the simulation
    /// moves on its own, and the schedule in [`crate::show`] is what moves them.
    ///
    /// The third is the budget: `spam` against `spam-share`, and `clamped`
    /// beside them. A count sitting at its share could be an emitter that
    /// happens to ask for exactly that many; a refusal counter climbing while
    /// the count holds still cannot be anything but a clamp.
    fn log_heartbeat(&self) {
        if !crcbl::engine::heartbeat_due(self.show.tick_count(), HEARTBEAT_TICKS) {
            return;
        }
        let reading = self.show.reading();
        crcbl::log::info!(
            "[HUD] tick: {}  live: {}  sparks: {}  puff: {}  puff-emitting: {}  \
             spam: {}  spam-share: {}  clamped: {}  pool: {}  effects: {}",
            self.show.tick_count(),
            reading.live,
            reading.sparks,
            reading.puff,
            if reading.puff_emitting { "yes" } else { "no" },
            reading.spam,
            reading.spam_share,
            reading.spam_clamped,
            reading.reserved,
            reading.effects,
        );
    }
}

/// Sparks' half of the frame, and nothing else.
impl HostedGame for Sparks {
    /// A show built from a seed has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Sparks declares no menu action of its own — see [`crate::menu`].
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "sparks";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        // The **fixed** step, from the loop's own clock, cast once. A run with
        // a different `--tick-hz` is a different run and says so in its
        // summary; a run with the same one replays.
        #[allow(clippy::cast_possible_truncation)]
        self.show.step(tick_dt as f32);
        self.log_heartbeat();
    }

    /// Sparks reads no key of its own.
    ///
    /// Every key this sample answers to is the loop's — `ESC` pauses, `F3`
    /// toggles the panel, `F11` goes fullscreen — and the effects run
    /// themselves from the seed. There is nothing here to forward a key to, and
    /// a binding that did nothing would be worse than none.
    fn key_event(&mut self, _key: KeyCode, _pressed: bool) {}

    fn menu_action(_id: crcbl::ui::WidgetId) -> Option<core::convert::Infallible> {
        None
    }

    fn apply(&mut self, action: core::convert::Infallible) {
        match action {}
    }

    fn menu_kind(&mut self, _menus: &mut Menus, paused: bool) -> MenuKind {
        MenuKind::of(paused)
    }

    fn draw(
        &mut self,
        gpu: &mut Gpu,
        draw_list: &mut crcbl::ui::draw_list::DrawList,
        frame: FrameInfo,
    ) {
        self.elapsed += frame.render_dt.as_secs_f64();
        gpu.set_camera(crate::stage::camera(self.elapsed));
        self.drawn = gpu.place_particles(&self.show);
        self.commands =
            crate::page::draw(draw_list, gpu.atlas(), gpu.extent(), &self.show.reading()).commands;
    }

    /// **One section, and no second one.** No network section: this sample has
    /// no connection to report on, because `docs/plan/20-particles.md` puts
    /// visual-only effects entirely on the client. No audio section either — it
    /// plays nothing, and a section that said so would be a module with no
    /// system behind it.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&Stats {
            show: &self.show,
            reading: self.show.reading(),
            drawn: self.drawn,
            commands: self.commands,
        });
    }

    fn summary(&self, run: RunSummary) -> Summary {
        let reading = self.show.reading();
        Summary {
            run,
            live: reading.live,
            drawn: self.drawn,
            clamped: reading.spam_clamped,
            commands: self.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "sparks: {} frames, {} ticks, {} live, {} instances, {} clamped, \
             {} page commands ({:?})",
            summary.run.frames,
            summary.run.ticks,
            summary.live,
            summary.drawn,
            summary.clamped,
            summary.commands,
            summary.run.exit,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The browser gate's `beatMs` for this demo is this heartbeat**:
    /// [`HEARTBEAT_TICKS`] at [`crate::show::TICK_HZ`], in milliseconds.
    ///
    /// Not an assertion in the gate but its denominator. Group B reads the
    /// observed beat against `beatMs` to work out how far behind real time the
    /// machine is running the demo, and scales every later budget by that, so a
    /// period changed here and not there stretches or shrinks every timeout in
    /// the run and nothing reddens.
    #[test]
    fn the_browser_gates_beat_is_this_samples_heartbeat() {
        crcbl_sample_test::assert_browser_gate_beat(
            "sparks",
            HEARTBEAT_TICKS,
            crate::show::TICK_HZ,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

crcbl::impl_pending_loop!(
    running: Loop,
    gpu: Gpu,
    options: Options,
    error: SparksError,
    window: |shell, clock, options| open_the_window(
        shell,
        clock,
        options.common.display_mode(),
        options.common.size,
    ),
    context: |_options| (),
    assemble: |booted, options| Ok(assemble(booted, options)),
);
