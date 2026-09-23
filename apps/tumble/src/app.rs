//! Tumble's start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Tumble::tick   (SUBSTEPS physics steps)
//!   draw_list.clear()
//!     ─────────────────────────────→ Tumble::draw    (body instances, panel)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! # The simulation is on the tick and the drawing is on the frame
//!
//! [`Tumble::tick`] steps [`Scenes`] by the fixed timestep and nothing else,
//! which is what makes the hash at [`crate::scene::CHECK_TICK`] a constant.
//! [`Tumble::draw`] reads whatever the last tick left. The one key this sample
//! reads picks the room on screen, which the simulation never sees.

use crcbl::core::input::KeyCode;
use crcbl::engine::{Booted, Clock, FrameInfo, HostedGame, RunSummary, wait_for_configure};
use crcbl::prelude::*;
use crcbl::shell::{DisplayMode, WindowId};
use crcbl::ui::{DebugModule, DebugSection};

use crate::gpu::Gpu;
use crate::menu::{MenuKind, Menus};
use crate::scene::{CHECK_TICK, PINNED_HASH, Reading, Scenes, View};

pub use crate::args::Options;

// ---- defaults ----------------------------------------------------------------

/// How often [`Tumble::log_heartbeat`] logs, in ticks: once a simulated
/// second, and a divisor of [`CHECK_TICK`], so the heartbeat the browser gate
/// reads the pinned hash off is one the page actually logs.
const HEARTBEAT_TICKS: u64 = 60;

const _: () = assert!(
    CHECK_TICK.is_multiple_of(HEARTBEAT_TICKS),
    "the check tick must fall on a heartbeat"
);

// ---- summary -----------------------------------------------------------------

/// What a finished run reports.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// Every counter, as the last tick left them.
    pub reading: Reading,
    /// How many commands the last page drew. Zero would mean a run that
    /// simulated scenes nobody drew.
    pub commands: usize,
}

// ---- errors ------------------------------------------------------------------

/// What can stop tumble.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample, and scenes built from constants have nothing of
/// their own to fail at.
pub type TumbleError = crcbl::engine::LoopError;

// ---- the debug panel ---------------------------------------------------------

/// Tumble's section of the debug panel: a line of counters per room.
#[derive(Debug)]
struct Stats<'a> {
    scenes: &'a Scenes,
    commands: usize,
}

impl DebugModule for Stats<'_> {
    fn debug_section(&self, out: &mut DebugSection) {
        let r = self.scenes.reading();
        out.set_title("physics");
        out.row("tick", format_args!("{}", r.tick));
        out.row(
            "spin",
            format_args!(
                "{} flips, drift {:.1e}, box {:.3} m",
                r.spin.flips, r.spin.momentum_drift, r.spin.box_height
            ),
        );
        for (name, tally) in [
            ("wall", r.wall.contacts),
            ("pit", r.pit.contacts),
            ("pyramid", r.tower.pyramid),
            ("column", r.tower.column),
        ] {
            out.row(
                name,
                format_args!(
                    "{} awake, {} asleep, {} pairs, {} contacts, {}+ {}-",
                    tally.bodies,
                    tally.sleeping,
                    tally.pairs,
                    tally.touching,
                    tally.begun,
                    tally.ended
                ),
            );
        }
        out.row(
            "tower",
            format_args!(
                "pyramid top {:.2} mm, column top {:.2} mm, {} dominoes down",
                r.tower.pyramid_drift * 1.0e3,
                r.tower.column_drift * 1.0e3,
                r.tower.dominoes_down
            ),
        );
        out.row("hash", format_args!("{:016x}", r.hash));
        out.row("commands", format_args!("{}", self.commands));
    }
}

// ---- the hosted game ---------------------------------------------------------

/// Tumble, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Tumble {
    scenes: Scenes,
    /// How many commands the last [`Tumble::draw`] emitted.
    commands: usize,
}

/// The loop tumble runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Tumble>;

/// Runs the full loop.
///
/// # Errors
///
/// [`TumbleError`] if the shell or the GPU failed. Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, TumbleError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the scenes.
///
/// # Errors
///
/// [`TumbleError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, TumbleError> {
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
/// [`TumbleError`] if the window never configured or the GPU would not open.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, TumbleError> {
    let clock_source = Clock::new(options.common.headless);
    let window = open_the_window(
        shell.as_mut(),
        &clock_source,
        options.common.display_mode(),
        options.common.size,
    )?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

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
        Tumble {
            scenes: Scenes::new(),
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
) -> Result<WindowId, TumbleError> {
    Ok(crcbl::engine::open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Tumble",
            app_id: "sh.kryptic.crcbl.tumble",
            size: crcbl::engine::requested_window_size(size),
            mode,
            ..WindowDesc::default()
        },
    )?)
}

impl Tumble {
    /// The scenes, for scripted tests and for an embedder that drives them.
    pub const fn scenes(&self) -> &Scenes {
        &self.scenes
    }

    /// The `[HUD]` line, every [`HEARTBEAT_TICKS`] ticks.
    ///
    /// `web/tools/browser-e2e.mjs` reads it. `tick` is the heartbeat itself;
    /// `hash` beside `pinned-tick` and `pinned` is the determinism check, the
    /// wasm build's hash at the tick the native test pins; the rest are every
    /// room's counters, whichever room is on screen.
    fn log_heartbeat(&self) {
        if !crcbl::engine::heartbeat_due(self.scenes.tick_count(), HEARTBEAT_TICKS) {
            return;
        }
        let r = self.scenes.reading();
        let (wall, pit, tower) = (r.wall.contacts, r.pit.contacts, r.tower);
        let bounce = wall.bounce_ratio().unwrap_or(0.0);
        crcbl::log::info!(
            "[HUD] tick: {}  view: {}  flips: {}  momentum-drift: {:.1e}  box-y: {:.3}  \
             drops: {}  wall-bodies: {}  wall-pairs: {}  wall-begun: {}  wall-ended: {}  \
             wall-pen-mm: {:.2}  wall-bounce: {:.2}  wall-persisted: {:.3}  pit-balls: {}  \
             pit-pairs: {}  pit-contacts: {}  pit-begun: {}  pit-ended: {}  \
             pit-pen-mm: {:.2}  pit-awake: {}  pit-sleeping: {}  pit-islands: {}  \
             pyramid-awake: {}  pyramid-sleeping: {}  \
             pyramid-points: {:.2}  pyramid-persisted: {:.3}  \
             pyramid-top-mm: {:.2}  column-top-mm: {:.2}  dominoes-down: {}  \
             hash: {:016x}  pinned-tick: {}  pinned: {:016x}",
            r.tick,
            r.view.name(),
            r.spin.flips,
            r.spin.momentum_drift,
            r.spin.box_height,
            r.spin.drops,
            wall.bodies,
            wall.pairs,
            wall.begun,
            wall.ended,
            wall.worst_penetration * 1.0e3,
            bounce,
            wall.persisted_ratio().unwrap_or(0.0),
            r.pit.balls,
            pit.pairs,
            pit.touching,
            pit.begun,
            pit.ended,
            pit.worst_penetration * 1.0e3,
            pit.bodies,
            pit.sleeping,
            pit.islands + pit.sleeping_islands,
            tower.pyramid.bodies,
            tower.pyramid.sleeping,
            tower.pyramid.points_per_manifold().unwrap_or(0.0),
            tower.pyramid.persisted_ratio().unwrap_or(0.0),
            tower.pyramid_drift * 1.0e3,
            tower.column_drift * 1.0e3,
            tower.dominoes_down,
            r.hash,
            CHECK_TICK,
            PINNED_HASH,
        );
    }
}

/// Tumble's half of the frame, and nothing else.
impl HostedGame for Tumble {
    /// Scenes built from constants have nothing of their own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Tumble declares no menu action of its own — see [`crate::menu`].
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "tumble";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        self.scenes.step(tick_dt);
        self.log_heartbeat();
    }

    /// `1` to `4` pick the room on screen. That is the only key, and it
    /// reaches the camera and the panel and not the simulation, so the hash
    /// the gate pins is the same whatever is pressed.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if pressed && let Some(view) = View::for_key(key) {
            self.scenes.set_view(view);
        }
    }

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
        _frame: FrameInfo,
    ) {
        gpu.place_bodies(&self.scenes);
        self.commands =
            crate::page::draw(draw_list, gpu.atlas(), gpu.extent(), &self.scenes.reading())
                .commands;
    }

    /// One section: the counters. No network or audio section, because this
    /// sample has neither.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&Stats {
            scenes: &self.scenes,
            commands: self.commands,
        });
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            run,
            reading: self.scenes.reading(),
            commands: self.commands,
        }
    }

    fn log_summary(summary: &Summary) {
        let r = &summary.reading;
        crcbl::log::info!(
            "tumble: {} frames, {} ticks, {} flips, momentum drift {:.1e}, box at {:.3} m, \
             wall {} bodies {}+ {}- contacts, pit {} balls {} pairs {} asleep, \
             pyramid top {:.2} mm, column top {:.2} mm, \
             hash {:016x}, {} page commands ({:?})",
            summary.run.frames,
            summary.run.ticks,
            r.spin.flips,
            r.spin.momentum_drift,
            r.spin.box_height,
            r.wall.contacts.bodies,
            r.wall.contacts.begun,
            r.wall.contacts.ended,
            r.pit.balls,
            r.pit.contacts.pairs,
            r.pit.contacts.sleeping,
            r.tower.pyramid_drift * 1.0e3,
            r.tower.column_drift * 1.0e3,
            r.hash,
            summary.commands,
            summary.run.exit,
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

crcbl::impl_pending_loop!(
    running: Loop,
    gpu: Gpu,
    options: Options,
    error: TumbleError,
    window: |shell, clock, options| open_the_window(
        shell,
        clock,
        options.common.display_mode(),
        options.common.size,
    ),
    context: |_options| (),
    assemble: |booted, options| Ok(assemble(booted, options)),
);

#[cfg(test)]
mod tests;
