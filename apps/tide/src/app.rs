//! Tide's start-up, and the methods the engine's loop calls.
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Tide::tick     (the cameras)
//!   draw_list.clear()
//!     ─────────────────────────────→ Tide::draw      (the knobs, the camera, a placard)
//!     menu ───────────────────────→ Tide::menu_kind
//!     debug overlay ──────────────→ Tide::debug_sections
//!   gpu.frame()                                       (stages the scene, draws)
//! ```
//!
//! There is no simulation in this file: milestone 1's water is still, so what a
//! water fixture has at this milestone is a camera somebody moves, one that
//! orbits on the fixed step, and three knobs somebody turns.
//!
//! # The knobs are read, never kept
//!
//! [`crate::knobs`] is the cell; [`Tide`] holds a reading of it, refreshed once
//! a frame in [`HostedGame::draw`], on `apps/sundial/src/app.rs`' terms — so the
//! pause panel, the debug overlay and the summary report one instant. **The
//! heartbeat reports what the GPU staged**, not the cell: a page that wrote a
//! knob the frame then refused to stage would otherwise read as a knob that
//! worked.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock as ClockSource, FrameInfo, HostedGame, PointerUpdate, RunSummary, open_window,
    wait_for_configure,
};
use crcbl::math::Vec2;
use crcbl::prelude::*;
use crcbl::render::Flyer;
use crcbl::shell::{PointerMode, WindowDesc, WindowId};
use crcbl::ui::draw_list::DrawList;

use crate::args::Options;
use crate::gpu::{Gpu, Paths, WaterCost};
use crate::knobs::{self, Knobs};
use crate::medium::Preset;
use crate::menu::{self, CameraMode, Menus, TideAction};
use crate::scene::{self, Scene};

/// How often [`Tide::log_heartbeat`] logs, in ticks — a second of simulated
/// time at [`crate::DEFAULT_TICK_HZ`], every other sample's spacing.
const HEARTBEAT_TICKS: u64 = 60;

/// Where a stub room's placard is drawn, in pixels from the frame's top left.
const PLACARD_AT: Vec2 = Vec2::new(16.0, 16.0);

/// The placard's text size, in pixels.
const PLACARD_SIZE: f32 = 18.0;

/// The placard's colour.
const PLACARD_COLOR: [f32; 4] = [0.95, 0.93, 0.85, 1.0];

/// What a completed run did.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The half of the report every sample shares.
    pub run: RunSummary,
    /// Which of the three selectors the frames were drawn through — rule 12's
    /// headless half.
    pub paths: Paths,
    /// Where the knobs stood when the loop ended.
    pub knobs: Knobs,
    /// Which scene and medium the last frame actually drew.
    pub staged: (Scene, Preset),
    /// What the water passes cost, off the last frame whose timestamps landed.
    pub water_cost: WaterCost,
}

/// Anything that can stop tide before it starts — the engine's own variants,
/// since a water fixture with no simulation has nothing of its own to fail.
pub type TideError = crcbl::engine::LoopError;

/// Tide, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Tide {
    /// What the knobs read on the last frame drawn.
    knobs: Knobs,
    /// Which scene and medium the GPU had staged on the last frame drawn.
    staged: (Scene, Preset),
    /// The free camera, whether or not it is the one in use.
    flyer: Flyer,
    /// Fixed steps the orbit has turned through.
    orbit_ticks: u64,
    /// What the device resolved, copied once, on sundial's terms.
    paths: Paths,
    /// What the water cost on the last frame whose timestamps landed.
    water_cost: WaterCost,
    /// The knobs the pause panel was last built for; `None` until the first
    /// pause, so the panel is always rebuilt once with the real ones.
    shown: Option<Knobs>,
    /// Whether the loop has the simulation stopped.
    paused: bool,
    /// Fixed steps run, for [`Tide::log_heartbeat`]'s cadence.
    ticks: u64,
}

impl Tide {
    /// A fixture reading `knobs`, drawn through `paths`, with the free camera at
    /// the fixed pose.
    #[must_use]
    pub fn new(knobs: Knobs, paths: Paths) -> Self {
        Self {
            knobs,
            staged: (knobs.scene, knobs.medium),
            flyer: Flyer::at(&scene::fixed_camera()),
            orbit_ticks: 0,
            paths,
            water_cost: WaterCost::default(),
            shown: None,
            paused: false,
            ticks: 0,
        }
    }

    /// The `[HUD]` line, on the cadence every other sample's heartbeat uses.
    ///
    /// What a browser gate reads: the lighting path, so a page that opened some
    /// other device is legible; the scene and medium **the GPU staged** and the
    /// camera in use, which are the page's three knobs; and the water's cost,
    /// which is where the browser price of the two water passes is read.
    fn log_heartbeat(&self) {
        if !crcbl::engine::heartbeat_due(self.ticks, HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  lighting: {:?}  geometry: {:?}  binding: {:?}  effects: {}  \
             scene: {}  medium: {}  camera: {}  cost: {}",
            self.ticks,
            self.paths.lighting,
            self.paths.geometry,
            self.paths.binding,
            self.paths.effects.row(),
            self.staged.0.label(),
            self.staged.1.label(),
            self.knobs.camera.label(),
            self.water_cost.row(),
        );
    }

    /// What the knobs read on the last frame drawn.
    #[must_use]
    pub const fn knobs(&self) -> Knobs {
        self.knobs
    }

    /// The camera this frame is seen through.
    #[must_use]
    pub fn camera(&self) -> crcbl::render::Camera {
        match self.knobs.camera {
            CameraMode::Fixed => scene::fixed_camera(),
            CameraMode::Orbit => scene::orbit_camera(self.orbit_ticks),
            CameraMode::Free => self.flyer.camera(scene::fixed_camera().projection),
        }
    }

    /// Takes a reading of the cell, and puts the free camera back at the fixed
    /// pose whenever the camera moves off it — the way back to the golden framing
    /// is to cycle round again.
    fn read_knobs(&mut self) {
        let knobs = knobs::read();
        if knobs.camera != self.knobs.camera {
            if knobs.camera != CameraMode::Free {
                self.flyer = Flyer::at(&scene::fixed_camera());
            }
            // Nothing is held down after a switch: a key that was down when a
            // menu or a page took the press has no release coming.
            self.flyer.release_all();
        }
        self.knobs = knobs;
    }
}

/// Every key this sample binds, what it does, and what the usage text calls it
/// — one table, on sundial's `KEYS`' argument.
type KeyBinding = (KeyCode, fn(), &'static str);

/// The bindings, each a write to [`crate::knobs`]' cell.
///
/// `N`, `M` and `C` rather than the letters a reader might guess: `A`, `S`, `D`
/// and `W` are the flyer's, and the flyer is offered every key before this table
/// is walked.
pub(crate) const KEYS: [KeyBinding; 4] = [
    (KeyCode::KeyN, cycle_scene, "N"),
    (KeyCode::KeyM, cycle_medium, "M"),
    (KeyCode::KeyC, cycle_camera, "C"),
    (KeyCode::KeyR, reset, "R"),
];

fn cycle_scene() {
    knobs::cycle_scene();
}

fn cycle_medium() {
    knobs::cycle_medium();
}

fn cycle_camera() {
    knobs::cycle_camera();
}

fn reset() {
    knobs::reset();
}

/// The loop tide runs in. A type alias, because the loop is the engine's.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Tide>;

/// Runs the full loop.
///
/// # Errors
///
/// [`TideError`] if the shell or the GPU refused. Teardown runs on every path.
pub fn run(options: &Options) -> Result<Summary, TideError> {
    let summary = crcbl::engine::drive(start(options)?);
    // The knobs go back on the way out, so nothing sharing this process inherits
    // a run's scene.
    knobs::reset();
    summary
}

/// Opens a shell, a window, a GPU and the gallery.
///
/// # Errors
///
/// [`TideError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, TideError> {
    let shell = crcbl::engine::open_shell(options.common.headless)?;
    with_shell(shell, options)
}

/// Builds the loop on an already-open shell, blocking on both waits — the
/// native half; the browser takes [`PendingLoop`].
///
/// # Errors
///
/// [`TideError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, TideError> {
    let clock_source = ClockSource::new(options.common.headless);
    let window = open_the_window(shell.as_mut(), &clock_source, options)?;

    let mut events = 0;
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;

    // Before the GPU opens, because the GPU stages the scene the cell names.
    options.apply();
    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.common.gpu(),
        options.forced,
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

/// Creates the one window this sample has.
///
/// # Errors
///
/// [`TideError`] if the shell refused it.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &ClockSource,
    options: &Options,
) -> Result<WindowId, TideError> {
    Ok(open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Crucible — tide",
            app_id: "sh.kryptic.crcbl.tide",
            // 4:3, so a windowed frame and a golden are the same framing.
            size: crcbl::engine::requested_window_size(options.common.size),
            mode: options.common.display_mode(),
            ..WindowDesc::default()
        },
    )?)
}

/// The half of start-up that is the same however the GPU arrived.
fn assemble<S: Shell + ?Sized>(booted: Booted<S, Gpu>, options: &Options) -> Loop<S> {
    let booted = crcbl::engine::arm_screenshot(booted, &options.common);
    // Again, for the polled path, whose GPU opened before any of this ran — and
    // harmless on the blocking one, which wrote the same value a moment ago.
    options.apply();
    let paths = booted.gpu.paths();
    Loop::new(
        booted,
        Tide::new(options.knobs, paths),
        options.common.loop_config(),
    )
}

impl HostedGame for Tide {
    /// A water fixture has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the whole of its state machine.
    type MenuKind = bool;
    type MenuAction = TideAction;
    type Summary = Summary;

    const NAME: &'static str = "tide";

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
        // The orbit turns on the fixed step and on nothing else, and only while
        // it is the camera in use, so switching to it starts where it was left.
        if self.knobs.camera == CameraMode::Orbit {
            self.orbit_ticks += 1;
        }
        self.flyer.advance(dt);
        self.log_heartbeat();
    }

    /// The camera's keys, and this sample's own. The flyer is offered every key
    /// first, so a binding below can never shadow `WASD`.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if self.flyer.key(key, pressed) || !pressed {
            return;
        }
        if let Some((_, act, _)) = KEYS.iter().find(|(bound, _, _)| *bound == key) {
            act();
        }
    }

    /// The mouse look, bound only while the pointer is really captured — see
    /// `apps/lantern/src/app.rs`, which carries the argument in full.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let Some(motion) = pointer.motion.filter(|_| pointer.at.is_none()) else {
            return;
        };
        self.flyer.look(motion);
    }

    /// [`PointerMode::Locked`] while the gallery is being flown, free while the
    /// pause panel is up.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<TideAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: TideAction) {
        match action {
            TideAction::CycleCamera => cycle_camera(),
            TideAction::CycleScene => cycle_scene(),
            TideAction::CycleMedium => cycle_medium(),
            TideAction::Reset => reset(),
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        // Recorded for `pointer_mode`, which the loop polls immediately after.
        self.paused = paused;
        // Read here as well as in `draw`, so a row pressed while paused moves its
        // label on the frame it is pressed on.
        self.read_knobs();
        if paused && self.shown != Some(self.knobs) {
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(true, menu::pause_menu(self.knobs));
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some(self.knobs);
        }
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, draw_list: &mut DrawList, _frame: FrameInfo) {
        self.read_knobs();
        gpu.set_camera(self.camera());
        gpu.show(self.knobs.scene, self.knobs.medium);
        // Read back rather than assumed: the stage moves inside `Gpu::frame`, so
        // this is what the last frame drew, which is what the heartbeat is
        // supposed to say.
        self.staged = gpu.staged();
        self.paths = gpu.paths();
        self.water_cost = gpu.water_cost();
        if let Some(placard) = self.knobs.scene.placard() {
            draw_list.text(PLACARD_AT, placard, PLACARD_COLOR, PLACARD_SIZE);
        }
    }

    /// Three sections: rule 12's paths, the water this frame drew, and what it
    /// cost.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.paths);
        panel.add(self);
        panel.add(&self.water_cost);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            run,
            paths: self.paths,
            knobs: self.knobs,
            staged: self.staged,
            water_cost: self.water_cost.clone(),
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "tide: {} frames, {} ticks on the {} shell at {}x{} ({:?}), {:?} / {:?} / {:?}, \
             scene {}, medium {}, cost {}",
            summary.run.frames,
            summary.run.ticks,
            summary.run.backend,
            summary.run.extent.0,
            summary.run.extent.1,
            summary.run.exit,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.staged.0.label(),
            summary.staged.1.label(),
            summary.water_cost.row(),
        );
    }
}

/// The scene, its water and the camera, as a panel section.
impl crcbl::ui::DebugModule for Tide {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        let (scene, medium) = self.staged;
        let eye = self.camera().eye;
        section.set_title("water");
        section.row_str("scene", scene.label());
        section.row_str("medium", medium.label());
        let bodies = scene.bodies(medium);
        section.row("bodies", format_args!("{}", bodies.len()));
        if let Some(body) = bodies.first() {
            let [red, green, blue] = body.medium.absorption;
            section.row(
                "absorption",
                format_args!("{red:.3} {green:.3} {blue:.3} /m"),
            );
            let [red, green, blue] = body.medium.scattering;
            section.row(
                "scattering",
                format_args!("{red:.3} {green:.3} {blue:.3} /m"),
            );
        }
        section.row_str("camera", self.knobs.camera.label());
        section.row(
            "eye",
            format_args!("{:.2} {:.2} {:.2}", eye.x, eye.y, eye.z),
        );
    }
}

// ---- polled start-up ---------------------------------------------------------

crcbl::impl_pending_loop!(
    running: Loop,
    gpu: Gpu,
    options: Options,
    error: TideError,
    window: |shell, clock, options| open_the_window(shell, clock, options),
    context: |options| options.apply(),
    assemble: |booted, options| Ok(assemble(booted, options)),
);

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture on the device paths a test does not need to open.
    fn fixture() -> Tide {
        Tide::new(
            Knobs::default(),
            Paths {
                geometry: crcbl::hal::GeometryPath::IndirectPerBatch,
                binding: crcbl::hal::BindingModel::ArrayPages,
                lighting: crcbl::hal::LightingPath::Rasterised,
                forced: crcbl::engine::ForcedPaths::default(),
                effects: crcbl::render::RenderEffects::DEFAULT_STACK,
            },
        )
    }

    /// **Every key this sample binds is spelled in the usage text's `KEYS:`
    /// block**, and no two bindings take the same key — sundial's check, on its
    /// row-start reading so `"C"` inside `"CI"` cannot satisfy it.
    #[test]
    fn every_key_is_bound_once_and_named_in_the_help() {
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
        for (at, (key, _, name)) in KEYS.iter().enumerate() {
            assert!(
                !KEYS[..at].iter().any(|(bound, _, _)| bound == key),
                "{key:?} is bound twice"
            );
            assert!(names_a_row(name), "the KEYS: block has no row for {name}");
        }
        assert!(!names_a_row("Q"), "the check above is not reading rows");
    }

    /// **Each key and each panel row moves the knob it names**, the reset puts
    /// all three back, and a release is not a press.
    #[test]
    fn each_key_and_row_moves_its_knob() {
        let _held = knobs::held();
        let mut fixture = fixture();

        fixture.key_event(KeyCode::KeyN, true);
        assert_eq!(knobs::read().scene, Scene::Courtyard.next());
        fixture.key_event(KeyCode::KeyN, false);
        assert_eq!(
            knobs::read().scene,
            Scene::Courtyard.next(),
            "a release acted"
        );

        fixture.key_event(KeyCode::KeyM, true);
        assert_eq!(knobs::read().medium, Preset::ClearPool.next());
        fixture.key_event(KeyCode::KeyC, true);
        assert_eq!(knobs::read().camera, CameraMode::Fixed.next());

        fixture.key_event(KeyCode::KeyR, true);
        assert_eq!(knobs::read(), Knobs::default(), "R left a knob moved");

        for (action, moved) in [
            (
                TideAction::CycleScene,
                Knobs {
                    scene: Scene::Courtyard.next(),
                    ..Knobs::default()
                },
            ),
            (
                TideAction::CycleMedium,
                Knobs {
                    medium: Preset::ClearPool.next(),
                    ..Knobs::default()
                },
            ),
            (
                TideAction::CycleCamera,
                Knobs {
                    camera: CameraMode::Fixed.next(),
                    ..Knobs::default()
                },
            ),
        ] {
            knobs::reset();
            fixture.apply(action);
            assert_eq!(knobs::read(), moved, "{action:?}");
        }
        fixture.apply(TideAction::Reset);
        assert_eq!(
            knobs::read(),
            Knobs::default(),
            "the RESET row left a knob moved"
        );
    }

    /// **The camera follows its knob**, and leaving the free camera puts the
    /// flyer back at the fixed pose.
    #[test]
    fn the_camera_follows_its_knob_and_the_flyer_goes_home() {
        let _held = knobs::held();
        let mut fixture = fixture();
        fixture.orbit_ticks = 7;
        fixture.read_knobs();
        assert_eq!(fixture.camera().eye, scene::fixed_camera().eye);

        knobs::set(Knobs {
            camera: CameraMode::Orbit,
            ..Knobs::default()
        });
        fixture.read_knobs();
        assert_eq!(fixture.camera().eye, scene::orbit_camera(7).eye);

        knobs::set(Knobs {
            camera: CameraMode::Free,
            ..Knobs::default()
        });
        fixture.read_knobs();
        fixture.flyer = Flyer::at(&scene::orbit_camera(300));
        knobs::reset();
        fixture.read_knobs();
        assert_eq!(
            fixture.flyer.camera(scene::fixed_camera().projection).eye,
            scene::fixed_camera().eye,
            "the flyer did not go back to the fixed pose"
        );
    }
}
