//! Quarry's start-up, and the methods the engine's loop calls.
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, input, menu, pause, resize
//!   run_ticks  ─────────────────────→ Quarry::tick     (both moving cameras)
//!   draw_list.clear()
//!     ─────────────────────────────→ Quarry::draw      (hands the camera over)
//!     menu ───────────────────────→ Quarry::menu_kind
//!     debug overlay ──────────────→ Quarry::debug_sections
//!   gpu.frame()
//! ```
//!
//! There is no loop in this file, and no simulation: a geometry fixture's only
//! moving parts are its cameras — one a reviewer flies, and one that runs down
//! the face on its own. Both are stepped inside `run_ticks`'s `while`, not after
//! it — anything stepped once per frame has a speed proportional to the frame
//! rate, and a headless run pinned to 1/60 s cannot see that.
//!
//! The second of them is [`CameraMode::Dolly`], and it is the charter's
//! "**a slow dolly past the switch distance shows no boundary popping, on every
//! path**" made watchable: `tests/device/dolly.rs` asserts that claim frame by
//! frame on one renderer, and this is the same run with a window in front of it.
//! It is also what the browser page opens on — see `src/web.rs`, which is
//! compiled only for `wasm32`.
//!
//! # The numbers are copied out of the GPU each frame
//!
//! [`HostedGame::debug_sections`] is handed a panel and `&self`, and no GPU: the
//! panel is gathered before the frame runs, and the bundle is the loop's to
//! hold. So [`Quarry`] keeps what the panel and the summary report — the
//! [`Paths`] the device resolved, the triangle count, the budget in force, and
//! the frame's culling — copied in [`HostedGame::draw`], which is where a GPU is
//! in hand. The culling has to be re-read every frame rather than once at
//! start-up: `crcbl::render::CullStatsRing` answers a few frames behind, so a
//! value copied at `assemble` would be [`None`] for the whole run.

use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, PointerUpdate, RunSummary, open_window,
    wait_for_configure,
};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::render::{CullStats, DebugView, Flyer};
use crcbl::shell::{DisplayMode, PointerMode, ShellBackend as Backend, WindowDesc, WindowId};
use crcbl::ui::draw_list::DrawList;

use crate::args::Options;
use crate::camera;
use crate::gpu::{Gpu, Paths};
use crate::menu::{self, CameraMode, Menus, QuarryAction};

/// How often [`Quarry::log_heartbeat`] logs, in ticks.
///
/// A second of simulated time at [`crate::DEFAULT_TICK_HZ`], which is what every
/// other sample's heartbeat is spaced at.
const HEARTBEAT_TICKS: u64 = 60;

/// How long [`CameraMode::Dolly`] takes to run the face once, in seconds.
///
/// [`camera::dolly`] translates the eye by half of [`crate::face::DEPTH_METRES`]
/// between [`camera::DOLLY_START`] and [`camera::DOLLY_END`] — ninety metres —
/// so this is the input to [`DOLLY_SPEED`] rather than a number anybody reads on
/// its own, exactly as [`camera::FLY_SPEED`] is derived from a stated time.
///
/// Thirty, and it was chosen against a measurement rather than picked. The
/// charter's Proves section wants "a slow dolly past the switch distance" to
/// show "no boundary popping", and a boundary arriving has to be *watchable* for
/// that to be a claim anyone can check.
///
/// What was read, on `--headless --frames 2000 --backend vk --camera dolly`, is
/// the heartbeat's own `cull:` row across a full traversal: the cut ran 331
/// clusters at the start pose down to 151 at the far end over the thirty
/// heartbeats between them, moving a handful of clusters per second and never
/// more than fourteen. A traversal short enough to cross the face in a couple of
/// seconds would put that whole descent into a few frames, which is the jump cut
/// this sample exists to say does not happen. [`camera::FLY_SPEED`] is the other
/// end of the same trade and five times as fast, because a camera being flown is
/// aimed and one being watched is not.
const DOLLY_SECONDS: f32 = 30.0;

/// How fast the animated dolly travels, in metres a second.
///
/// Half of [`crate::face::DEPTH_METRES`] in [`DOLLY_SECONDS`] seconds, written
/// as that division so the speed follows the face if the face ever changes size
/// — the same rule [`camera::FLY_SPEED`] is written under.
/// `the_dolly_speed_is_the_distance_over_the_stated_time` is the check.
const DOLLY_SPEED: f32 = crate::face::DEPTH_METRES * 0.5 / DOLLY_SECONDS;

/// **A watched camera is slower than a driven one**, checked at compile time.
///
/// A `const` assertion rather than a test, for [`camera::FLY_SPEED`]'s own
/// reason: both sides are constants, so a [`DOLLY_SECONDS`] shortened until the
/// dolly kept pace with a held `W` is something that can fail to build rather
/// than fail to run. The charter asks for "a slow dolly", and this is the one
/// place that word is given a floor.
const _: () = assert!(
    DOLLY_SPEED < camera::FLY_SPEED,
    "a dolly nobody is steering must not run at the speed a reviewer flies",
);

/// How long one there-and-back cycle of [`CameraMode::Dolly`] takes, in seconds.
const DOLLY_PERIOD: f32 = 2.0 * DOLLY_SECONDS;

/// Where along [`camera::dolly`] the animated camera is after `elapsed` seconds.
///
/// **A triangle, not a sawtooth, and that is the whole of the design.** Running
/// to [`camera::DOLLY_END`] and restarting at [`camera::DOLLY_START`] would put
/// ninety metres of translation into one frame — a cut jump, in the sample whose
/// entire subject is that the cut does not jump. So the camera turns round and
/// walks back up the face instead: the position is continuous everywhere, only
/// its direction reverses, and the reversal happens at the two ends where the
/// cut is momentarily stationary. `the_dolly_turns_round_rather_than_jumping_back`
/// is what holds it to that.
#[must_use]
pub fn dolly_at(elapsed: f32) -> f32 {
    // 0 → 1 → 0 as the phase runs 0 → 1 → 2, and `rem_euclid` is what makes a
    // negative or a many-periods-large `elapsed` land in the same place a small
    // positive one would.
    let phase = (elapsed / DOLLY_SECONDS).rem_euclid(2.0);
    let along = 1.0 - (phase - 1.0).abs();
    camera::DOLLY_START + (camera::DOLLY_END - camera::DOLLY_START) * along
}

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
    /// Triangles in the face at level 0 — what a cut is a reduction *of*.
    pub triangles: usize,
    /// The screen-space error budget the cut was selected under, in pixels.
    pub lod_budget: f32,
    /// What the last frame whose readback landed kept, or [`None`] where the
    /// ring never came round — a run of a handful of frames.
    pub cull: Option<CullStats>,
}

/// The cut in one line:
/// `12 instance(s), 431 of 900 cluster(s) (312 frustum, 157 cone), from frame 57`.
///
/// `docs/plan/sample/14-quarry.md`'s exit criterion asks for "how much of the
/// reduction is instance culling and how much is cluster culling, because a
/// single total hides which one is working" — and then for "per-cluster frustum
/// and normal-cone rejection counts on the debug panel", which is the same
/// complaint one level down: `431 of 900` is equally consistent with the normal
/// cone rejecting all 469 and with it rejecting none, so the two rejection
/// counts are printed beside it.
///
/// The `of` number is [`ClusterCull::tested`](crcbl::render::ClusterCull::tested)
/// — the clusters the amplification
/// stage actually put to a test, which is the cut it was handed. **Not the
/// resident pool**: a cluster the DAG descent left to a coarser relative was
/// never offered to either cull, and counting it as rejected would report the
/// descent's work as the cull's.
///
/// A path with no amplification stage says so rather than printing three zeroes
/// the clearing pass left. The frame number is part of the line because
/// `crcbl::render::CullStatsRing` answers a few frames behind, and a number
/// printed as this frame's would be a stale one presented as current.
///
/// One function, called by the summary line and by the heartbeat, so the two
/// cannot describe the same cut two ways.
#[must_use]
pub fn cull_row(cull: Option<CullStats>) -> String {
    match cull {
        None => "cull not read back yet".to_string(),
        Some(stats) => {
            let clusters = match stats.clusters {
                Some(clusters) => format!(
                    "{} of {} cluster(s) ({} frustum, {} cone)",
                    clusters.survivors,
                    clusters.tested(),
                    clusters.frustum_rejects,
                    clusters.cone_rejects,
                ),
                None => "no cluster stage".to_string(),
            };
            format!(
                "{} instance(s), {clusters}, from frame {}",
                stats.instances, stats.frame
            )
        }
    }
}

/// Anything that can stop quarry before it starts.
///
/// An alias rather than an enum: [`crcbl::engine::LoopError`] owns these
/// variants for every sample. A geometry fixture has no simulation of its own to
/// fail, so it takes the default type parameter and its `Game` variant is
/// uninhabited.
pub type QuarryError = crcbl::engine::LoopError;

/// Quarry, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Quarry {
    /// Which camera the next frame is drawn from.
    camera: CameraMode,
    /// The free camera, whether or not it is the one in use — kept across a
    /// swap so a reviewer who looks at the dolly pose and swaps back is where
    /// they left off.
    flyer: Flyer,
    /// How far into its there-and-back cycle [`CameraMode::Dolly`] is, in
    /// seconds — see [`dolly_at`].
    ///
    /// **Advanced only while the dolly is the camera being drawn from**, which
    /// is the opposite of [`Self::flyer`]'s rule and for the opposite reason: a
    /// flown camera is somewhere the reviewer put it and should still be there
    /// when they come back, while an animated one that ran on unwatched would
    /// jump to wherever the clock had got to the moment it was selected. Frozen,
    /// it resumes from the pose it was last showing, and its first pose of all
    /// is [`camera::DOLLY_START`] — the one [`CameraMode::Fixed`] holds. Every
    /// entry into this mode is therefore continuous.
    dolly_elapsed: f32,
    /// What the device resolved, copied once — see the module docs.
    paths: Paths,
    /// Triangles in the face at level 0, copied once: it cannot change mid-run.
    triangles: usize,
    /// The budget the cut is selected under, in pixels.
    ///
    /// Starts as what the command line asked for and is replaced in
    /// [`HostedGame::draw`] by what a frame was *actually* selected under —
    /// `Gpu::frame_lod_budget` — so a setting that never reached `begin_frame`
    /// is reported as the renderer's own default rather than as the number that
    /// was asked for.
    lod_budget: f32,
    /// Which overlay is being drawn, if any. **Owned here rather than read off
    /// the renderer**, because the pause menu's rows are applied in
    /// [`HostedGame::apply`], which is handed no GPU; [`HostedGame::draw`] is
    /// where it reaches one.
    ///
    /// One value rather than a flag per overlay, which is what makes the panel's
    /// rows exclusive — see [`menu::toggled_to`].
    view: DebugView,
    /// Where the LOD selection is pinned, or [`None`] while it follows the
    /// camera. **Owned here** on [`Self::view`]'s terms, and written to the
    /// renderer every [`HostedGame::draw`].
    ///
    /// The *position* rather than a `bool`, because that is what the panel has
    /// to print: "frozen" on its own tells a reviewer the cut is not the one for
    /// where they are standing and not where it is the cut for, which is the
    /// only half they can act on.
    frozen: Option<Vec3>,
    /// What the last frame whose readback landed kept — re-read every
    /// [`HostedGame::draw`], because the ring answers a few frames behind.
    cull: Option<CullStats>,
    /// The values the pause panel was last built for — `None` until the first
    /// pause, so the panel is always rebuilt once with the real ones.
    shown: Option<(CameraMode, DebugView, bool)>,
    /// Whether the loop has the simulation stopped, recorded in
    /// [`HostedGame::menu_kind`].
    ///
    /// The loop owns the pause and this is a *copy* of it, kept for one caller:
    /// [`HostedGame::pointer_mode`] is asked with no argument and has to answer
    /// "is a panel up", and `menu_kind` is the one place the loop says so.
    paused: bool,
    /// Fixed steps run, for [`Quarry::log_heartbeat`]'s cadence.
    ticks: u64,
}

impl Quarry {
    /// A fixture starting on `camera`, drawn through `paths`, over a face of
    /// `triangles` selected at `lod_budget` pixels, with the free camera at the
    /// dolly's start pose.
    #[must_use]
    pub fn new(
        camera: CameraMode,
        paths: Paths,
        triangles: usize,
        lod_budget: f32,
        view: DebugView,
    ) -> Self {
        Self {
            camera,
            flyer: camera::flyer(),
            dolly_elapsed: 0.0,
            paths,
            triangles,
            lod_budget,
            view,
            // Following the camera. There is no flag for it and deliberately
            // none: freezing is a thing a reviewer does *at* a viewpoint they
            // flew to, so a run that started frozen would be frozen at the
            // dolly's start pose, which is the one place the cut is already
            // being looked at from.
            frozen: None,
            cull: None,
            shown: None,
            paused: false,
            ticks: 0,
        }
    }

    /// The `[HUD]` line, on the cadence every other sample's heartbeat uses:
    /// every [`HEARTBEAT_TICKS`] steps, which is a second of simulated time at
    /// the default rate.
    ///
    /// What it names is what a run of this sample is *for*: the geometry path
    /// the frames are taking, and how much of the reduction each stage did. A
    /// browser gate has no debug panel to read, and neither has a CI log.
    ///
    /// **`eye z` is here for that gate specifically.** `web/tools/browser-e2e.mjs`
    /// proves a page is simulating rather than merely presenting by watching one
    /// number advance, and it has to be a number nothing on the JS side and no
    /// frame counter can move. The camera's position down the face is that: it
    /// changes only because [`HostedGame::tick`] moved it, and on the page —
    /// which opens on [`CameraMode::Dolly`] — it is the thing a visitor is
    /// actually watching.
    fn log_heartbeat(&self) {
        if !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  geometry: {:?}  binding: {:?}  camera: {}  eye z: {:.2}  \
             budget: {}px  view: {}  cull: {}",
            self.ticks,
            self.paths.geometry,
            self.paths.binding,
            self.camera.label(),
            self.camera().eye.z,
            self.lod_budget,
            self.view.label(),
            cull_row(self.cull),
        );
    }

    /// Which camera the next frame is drawn from.
    #[must_use]
    pub const fn camera_mode(&self) -> CameraMode {
        self.camera
    }

    /// Which overlay the frame is drawn with, if any.
    #[must_use]
    pub const fn debug_view(&self) -> DebugView {
        self.view
    }

    /// Where the LOD selection is pinned, or [`None`] while it follows the
    /// camera.
    #[must_use]
    pub const fn frozen_selection_eye(&self) -> Option<Vec3> {
        self.frozen
    }

    /// The free camera, whether or not it is the one in use.
    #[must_use]
    pub const fn flyer(&self) -> &Flyer {
        &self.flyer
    }

    /// The camera this frame is seen through.
    ///
    /// All three modes share the fixed camera's projection, which is what makes
    /// them comparable: a free camera with a lens of its own would produce a
    /// frame a reviewer cannot hold against the goldens.
    #[must_use]
    pub fn camera(&self) -> crcbl::render::Camera {
        let fixed = camera::dolly(camera::DOLLY_START);
        match self.camera {
            CameraMode::Fixed => fixed,
            CameraMode::Dolly => camera::dolly(dolly_at(self.dolly_elapsed)),
            CameraMode::Free => self.flyer.camera(fixed.projection),
        }
    }

    /// Pins the LOD selection where the camera is standing, or unpins it.
    ///
    /// **[`Self::camera`]'s eye, not the flyer's**: the pose to pin is the one
    /// the reviewer is looking through, and while [`CameraMode::Dolly`] is
    /// running that is not where the free camera was left. Pinning a viewpoint
    /// that is not on screen would give a cut nobody can hold against a picture.
    ///
    /// Unfreezing throws the pinned position away rather than keeping it for a
    /// second press. Somewhere to fly *back* to is what [`CameraMode::Fixed`]
    /// already is, and a remembered eye that the next freeze silently did not
    /// use would be worse than none.
    fn toggle_freeze(&mut self) {
        self.frozen = match self.frozen {
            Some(_) => None,
            None => Some(self.camera().eye),
        };
    }
}

/// The loop quarry runs in. A type alias, because the loop is the engine's.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Quarry>;

/// Runs the full loop.
///
/// # Errors
///
/// [`QuarryError`] if the shell or the GPU refused. Teardown runs on every path:
/// a failing frame must still release the swapchain, the surface and the window,
/// or `crcbl-vk`'s device teardown logs objects still alive.
pub fn run(options: &Options) -> Result<Summary, QuarryError> {
    crcbl::engine::drive(start(options)?)
}

/// Opens a shell, a window, a GPU and the face.
///
/// # Errors
///
/// [`QuarryError`] if any of them refused.
pub fn start(options: &Options) -> Result<Loop, QuarryError> {
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
/// [`QuarryError`] if the window never configured or the HAL seam failed.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
) -> Result<Loop<S>, QuarryError> {
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
        options.lod_budget,
        options.debug_view(),
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
/// [`QuarryError`] if the shell refused it.
fn open_the_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    options: &Options,
) -> Result<WindowId, QuarryError> {
    Ok(open_window(
        shell,
        clock_source,
        &WindowDesc {
            title: "Crucible — quarry",
            app_id: "sh.kryptic.crcbl.quarry",
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
    // Read before the bundle moves into the loop: what the device resolved, how
    // big the face is, and the two LOD settings as the renderer has them.
    let paths = booted.gpu.paths();
    let triangles = booted.gpu.triangles();
    let lod_budget = booted.gpu.lod_budget();
    let view = booted.gpu.debug_view();

    Loop::new(
        booted,
        Quarry::new(options.camera, paths, triangles, lod_budget, view),
        options.common.loop_config(),
    )
}

impl HostedGame for Quarry {
    /// A geometry fixture has nothing of its own to fail at.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    /// Paused or not, which is the whole of its state machine.
    type MenuKind = bool;
    type MenuAction = QuarryAction;
    type Summary = Summary;

    const NAME: &'static str = "quarry";

    fn menus() -> Menus {
        menu::menus()
    }

    fn tick(&mut self, _gpu: &mut Gpu, tick_dt: f64) {
        #[allow(clippy::cast_possible_truncation)]
        let dt = tick_dt as f32;
        self.ticks += 1;
        // The camera integrates whether or not it is the one being drawn from:
        // a reviewer who swaps to the dolly pose, flies, and swaps back should
        // arrive where the keys took them.
        self.flyer.advance(dt);
        // The animated dolly does not, and the field says why. Here rather than
        // in `draw` for the reason the module docs give: a camera stepped once a
        // frame runs at a speed proportional to how fast the machine is, and a
        // headless run pinned to 1/60 s could not see that.
        if self.camera == CameraMode::Dolly {
            self.dolly_elapsed = (self.dolly_elapsed + dt).rem_euclid(DOLLY_PERIOD);
        }
        self.log_heartbeat();
    }

    /// [`menu::FREEZE_KEY`] pins the selection; every other key the loop's own
    /// three did not claim goes to the camera.
    ///
    /// **On the press and not the release**, and it is not passed on: the flyer
    /// does not bind this key, so forwarding it would be forwarding a key
    /// nothing reads, and acting on the release as well would freeze and
    /// immediately unfreeze.
    ///
    /// The panel does not have to be up. A reviewer flying the face pins the cut
    /// where they are standing and keeps flying, which is the gesture the whole
    /// feature is — see [`menu::FREEZE_KEY`] on why `F` was free.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if key == menu::FREEZE_KEY {
            if pressed {
                self.toggle_freeze();
            }
            return;
        }
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
    /// request is not a grant, and a camera that turned anyway would swing the
    /// view while a visible cursor walked out of the window.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let Some(motion) = pointer.motion.filter(|_| pointer.at.is_none()) else {
            return;
        };
        self.flyer.look(motion);
    }

    /// [`PointerMode::Locked`] while the face is being flown, free while the
    /// pause panel is up.
    ///
    /// Answered from the pause alone — not from [`CameraMode`] — because the
    /// free camera integrates whether or not it is the one being drawn from,
    /// exactly as the keyboard's walk does.
    fn pointer_mode(&self) -> PointerMode {
        if self.paused {
            PointerMode::Free
        } else {
            PointerMode::Locked
        }
    }

    fn menu_action(id: crcbl::ui::WidgetId) -> Option<QuarryAction> {
        menu::action_for(id)
    }

    fn apply(&mut self, action: QuarryAction) {
        match action {
            QuarryAction::ToggleCamera => {
                let left = self.camera;
                self.camera = self.camera.toggled();
                // Leaving the free camera is also how a reviewer gets back to
                // the goldens' framing, so it is put back there — otherwise
                // "look at the reference pose" would be a flight rather than a
                // press. Keyed on the mode being *left*, not the one arrived at:
                // the cycle is three long now, and a reviewer who flew away and
                // pressed once has left the free camera whatever they landed on.
                if left == CameraMode::Free {
                    self.flyer = camera::flyer();
                }
                // And nothing is held down after a menu press: the press
                // happened while the menu owned the keyboard, so a key that was
                // down when the panel opened has no release coming.
                self.flyer.release_all();
            }
            // Recorded here and handed to the renderer in `draw`, which is the
            // method with a GPU in it.
            QuarryAction::ToggleLodView => {
                self.view = menu::toggled_to(self.view, DebugView::LodTint);
            }
            QuarryAction::ToggleHeatmap => {
                self.view = menu::toggled_to(self.view, DebugView::Heatmap);
            }
            QuarryAction::ToggleFreeze => self.toggle_freeze(),
        }
    }

    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> bool {
        // Recorded for `pointer_mode`, which the loop polls immediately after
        // this: a panel that went up on this frame must free the pointer on this
        // frame, or the cursor comes back one frame into a menu the reviewer is
        // already trying to click.
        self.paused = paused;
        let showing = (self.camera, self.view, self.frozen.is_some());
        if paused && self.shown != Some(showing) {
            // A row's label changed (or this is the first pause): rebuild the
            // panel with the values in force, restoring the selection so a press
            // on a row does not throw the reviewer back to the top.
            let selected = menus
                .current()
                .and_then(crcbl::ui::menu::Menu::selected_item)
                .map(|item| item.id);
            menus.replace(
                true,
                menu::pause_menu(self.camera, self.view, self.frozen.is_some()),
            );
            if let Some(id) = selected {
                menus
                    .current_mut()
                    .expect("the pause menu is in the set")
                    .select_id(id);
            }
            self.shown = Some(showing);
        }
        paused
    }

    fn draw(&mut self, gpu: &mut Gpu, _draw_list: &mut DrawList, _frame: FrameInfo) {
        // The fixture draws no HUD of its own: everything it has to say about a
        // frame is a debug-panel row. What `draw` does is hand over the camera
        // the ticks moved and the tint the menu set, and read this frame's
        // numbers back.
        gpu.set_camera(self.camera());
        // Here rather than in `tick`, which does not run while paused: the row
        // that was just pressed is on a panel over a frame that has to change
        // behind it.
        // Infallible here — this bundle has a renderer — and the `Result` is
        // `GameGpu::set_debug_view`'s, which exists so a bundle without one can
        // say so.
        gpu.set_debug_view(self.view)
            .expect("quarry's bundle holds a renderer");
        // Here for the same reason, and unconditionally rather than on the
        // frames it changed: a `None` written every frame is the renderer's own
        // default written every frame, which is the state every golden was
        // blessed in.
        gpu.set_frozen_selection_eye(self.frozen);
        self.paths = gpu.paths();
        // The budget a frame was really selected under, not the one the flag
        // asked for — see the field. Zero is what the renderer holds before its
        // first `begin_frame`, and reporting that would be a row about nothing,
        // so the requested value stands until a frame has run.
        let selected_under = gpu.frame_lod_budget();
        if selected_under > 0.0 {
            self.lod_budget = selected_under;
        }
        // Every frame, not once: the ring answers a few frames behind, so this
        // is `None` for the first handful of frames of every run and then stops
        // being.
        self.cull = gpu.cull_stats();
    }

    /// Two sections, and each is something the charter asks for.
    ///
    /// Rule 12's path reporting is the first, and this is the sample it matters
    /// most in: three paths is the widest selector in the engine, and the mesh
    /// path's per-cluster cut and the indirect paths' per-instance one are not
    /// the same picture. The second is this sample's own subject —
    /// `docs/plan/sample/14-quarry.md`'s "amplification-stage culling is doing
    /// work", on the screen, beside the budget that decided the cut and the
    /// camera position it was decided from.
    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(&self.paths);
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
            triangles: self.triangles,
            lod_budget: self.lod_budget,
            cull: self.cull,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "quarry: {} frames, {} ticks on the {} shell at {}x{} ({:?}), {:?} / {:?} / {:?}, \
             effects {}, {} triangles at a {}px budget, {}",
            summary.frames,
            summary.ticks,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.exit,
            summary.paths.geometry,
            summary.paths.binding,
            summary.paths.lighting,
            summary.paths.effects.row(),
            summary.triangles,
            summary.lod_budget,
            cull_row(summary.cull),
        );
    }
}

/// Where the camera is and what the cut cost, as a panel section.
///
/// On [`Quarry`] itself because that is what owns the numbers — the rule
/// [`crcbl::ui::DebugModule`] states.
///
/// **The cull rows are the sample's own claim.**
/// `docs/plan/sample/14-quarry.md` asks for "per-cluster frustum and normal-cone
/// rejection counts on the debug panel". They are read out of
/// `crcbl::render::CullStatsRing`, which is deliberately a few frames behind —
/// topic 03 §3.6 permits exactly one readback and this is it — so the rows name
/// the frame they came from rather than printing an old number as this frame's,
/// and say `pending` until the ring has come round rather than showing a zero.
/// A path with no amplification stage has no cluster count at all, and that is
/// said in words: reporting its zero would claim every cluster was rejected in a
/// frame that drew all of them.
impl crcbl::ui::DebugModule for Quarry {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        let eye = self.camera().eye;
        section.set_title("quarry");
        section.row_str("camera", self.camera.label());
        section.row(
            "eye",
            format_args!("{:.1} {:.1} {:.1}", eye.x, eye.y, eye.z),
        );
        match self.camera {
            CameraMode::Dolly => section.row(
                "pose",
                format_args!("{:.2} along the dolly", dolly_at(self.dolly_elapsed)),
            ),
            CameraMode::Free if self.flyer.has_moved() => section.row_str("pose", "flown"),
            CameraMode::Fixed | CameraMode::Free => section.row_str("pose", "dolly start"),
        }
        section.row("triangles", format_args!("{} at level 0", self.triangles));
        section.row("lod budget", format_args!("{} px", self.lod_budget));
        // **Which view, not two on/off rows.** The three overlays share one
        // lane and resolve to one picture, so a panel with a row per switch
        // could say `on` twice about a frame that drew one of them.
        section.row_str("view", self.view.label());
        // **Where it is frozen, not that it is.** The cut under a frozen
        // selection belongs to a viewpoint that is deliberately not this one, so
        // a row saying only `frozen` leaves a reviewer unable to tell a cut
        // chosen two metres back from one chosen at the other end of the face —
        // which is the difference they froze it to look at.
        match self.frozen {
            None => section.row_str("selection", "follows the camera"),
            Some(at) => section.row(
                "selection",
                format_args!("frozen at {:.1} {:.1} {:.1}", at.x, at.y, at.z),
            ),
        }
        match self.cull {
            None => {
                section.row_str("instances kept", "pending — the ring is frames behind");
                section.row_str("clusters kept", "pending — the ring is frames behind");
                section.row_str("clusters rejected", "pending — the ring is frames behind");
            }
            Some(stats) => {
                section.row(
                    "instances kept",
                    format_args!("{} (frame {})", stats.instances, stats.frame),
                );
                match stats.clusters {
                    Some(clusters) => {
                        // **`of` the clusters *tested*, not the pool.** The
                        // second number is the cut the DAG descent chose, which
                        // is the set the amplification stage put to a cull; a
                        // cluster left to a coarser relative was never offered
                        // to one, and printing the pool here would blame the
                        // cull for the descent's work.
                        section.row(
                            "clusters kept",
                            format_args!(
                                "{} of {} (frame {})",
                                clusters.survivors,
                                clusters.tested(),
                                stats.frame,
                            ),
                        );
                        // The row `docs/plan/sample/14-quarry.md` asks for by
                        // name. Two numbers rather than their sum: a total
                        // rejected is the survivor count again with the sign
                        // flipped, and says nothing about which test earned it.
                        section.row(
                            "clusters rejected",
                            format_args!(
                                "{} frustum, {} cone",
                                clusters.frustum_rejects, clusters.cone_rejects,
                            ),
                        );
                    }
                    None => {
                        section.row_str("clusters kept", "no amplification stage on this path");
                        section.row_str("clusters rejected", "no amplification stage on this path");
                    }
                }
            }
        }
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
    /// [`QuarryError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
    ) -> Result<Self, QuarryError> {
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
    /// [`QuarryError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, QuarryError> {
        let Some(booted) = self.boot.poll::<QuarryError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options)))
    }
}

#[cfg(test)]
mod tests {
    use crcbl::engine::{Flow, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, PAUSE_KEY};
    use crcbl::render::ClusterCull;
    use crcbl::shell::HeadlessShell;

    use super::*;

    /// A loop over a *concrete* `HeadlessShell`, so a test can play compositor.
    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        with_shell(Box::new(HeadlessShell::new()), options).expect("headless always starts")
    }

    /// Always `--backend null`. These run on every CI leg, including ones with
    /// no Vulkan loader at all, and they are about the *loop* — the camera, the
    /// menu, determinism — not about a driver. The picture is
    /// `tests/device/goldens.rs`'s.
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
    /// `downs` of zero.
    fn press_row(engine: &mut Loop<HeadlessShell>, window: WindowId, downs: usize) {
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

    /// The `quarry` section's rows as `(label, value)`, for a fixture whose
    /// readback landed with `cull` in it.
    ///
    /// The field is written by `HostedGame::draw` off a live renderer, which no
    /// headless run reaches: the Null backend builds no amplification stage, so
    /// a loop-driven panel can only ever show the "no stage" wording. Setting it
    /// here is what lets the numbers themselves be asserted.
    fn cull_rows(cull: Option<CullStats>) -> Vec<(String, String)> {
        use crcbl::hal::{BindingModel, GeometryPath, LightingPath};

        let mut quarry = Quarry::new(
            CameraMode::Dolly,
            Paths {
                geometry: GeometryPath::MeshShader,
                binding: BindingModel::Bindless,
                lighting: LightingPath::Rasterised,
                forced: crate::gpu::Forced::default(),
                effects: crcbl::render::RenderEffects::DEFAULT_STACK,
            },
            1000,
            1.0,
            DebugView::Shaded,
        );
        quarry.cull = cull;
        let mut section = crcbl::ui::DebugSection::default();
        crcbl::ui::DebugModule::debug_section(&quarry, &mut section);
        section
            .rows()
            .iter()
            .map(|row| (row.label.clone(), row.value.clone()))
            .collect()
    }

    /// **The panel attributes each rejection to the test that made it.**
    ///
    /// `docs/plan/sample/14-quarry.md` asks for "per-cluster frustum and
    /// normal-cone rejection counts on the debug panel", and the row is only
    /// worth having if the two numbers are told apart: printed the other way
    /// round it reads as the normal cone doing the work the frustum did, which
    /// is the exact confusion the split exists to end. So the two are given
    /// different values and each is required beside the word for its own test.
    #[test]
    fn the_panel_attributes_each_rejection_to_the_test_that_made_it() {
        let rows = cull_rows(Some(CullStats {
            instances: 12,
            clusters: Some(ClusterCull {
                survivors: 431,
                frustum_rejects: 312,
                cone_rejects: 157,
            }),
            frame: 57,
        }));
        let value = |label: &str| {
            rows.iter()
                .find(|(row, _)| row == label)
                .unwrap_or_else(|| panic!("no {label} row on the panel: {rows:?}"))
                .1
                .clone()
        };
        // 431 + 312 + 157 is 900, so the row reads as a partition of the cut
        // rather than as three unrelated counts.
        assert_eq!(value("clusters kept"), "431 of 900 (frame 57)");
        assert_eq!(value("clusters rejected"), "312 frustum, 157 cone");
    }

    /// **A path with no amplification stage says so on both rows.**
    ///
    /// Zeroes there are a claim — "every cluster was tested and none was
    /// rejected" — about a frame that has no per-cluster cull in it at all. The
    /// rejection row is the new half and the one that would be easiest to leave
    /// printing `0 frustum, 0 cone`.
    #[test]
    fn a_path_with_no_amplification_stage_prints_no_rejection_counts() {
        let rows = cull_rows(Some(CullStats {
            instances: 12,
            clusters: None,
            frame: 57,
        }));
        for label in ["clusters kept", "clusters rejected"] {
            let (_, value) = rows
                .iter()
                .find(|(row, _)| row == label)
                .unwrap_or_else(|| panic!("no {label} row on the panel: {rows:?}"));
            assert_eq!(value, "no amplification stage on this path");
        }
    }

    /// **The row names each rejection count, and names it by its own cause.**
    ///
    /// The whole point of the split is that a reader can tell a frustum doing
    /// all of the work from a normal cone doing all of it, so the two numbers
    /// are made different and each is looked for beside the word for its test.
    /// A row that printed them the other way round, or that printed one of them
    /// twice, satisfies "the numbers appear" and says the opposite thing.
    #[test]
    fn the_cull_row_attributes_each_rejection_to_the_test_that_made_it() {
        let row = cull_row(Some(CullStats {
            instances: 12,
            clusters: Some(ClusterCull {
                survivors: 431,
                frustum_rejects: 312,
                cone_rejects: 157,
            }),
            frame: 57,
        }));
        assert_eq!(
            row,
            "12 instance(s), 431 of 900 cluster(s) (312 frustum, 157 cone), from frame 57",
        );
        // The `of` number is the three added up, which is what makes the line
        // readable as a partition rather than as four unrelated counts.
        assert!(row.contains("of 900"), "431 + 312 + 157 is 900: {row}");
    }

    /// **A path with no amplification stage says so, and never prints zeroes.**
    ///
    /// Three zeroes are a claim — "every cluster was tested and every one of
    /// them survived nothing" — about a frame that has no per-cluster cull at
    /// all. `docs/plan/sample/00-samples-overview.md` rule 12 is what this row
    /// answers, and answering it with a fabricated number is worse than not
    /// answering it.
    #[test]
    fn the_cull_row_says_there_is_no_cluster_stage_rather_than_printing_zeroes() {
        let row = cull_row(Some(CullStats {
            instances: 12,
            clusters: None,
            frame: 57,
        }));
        assert_eq!(row, "12 instance(s), no cluster stage, from frame 57");
        assert!(
            !row.contains('0'),
            "an absent cluster stage must not reach the panel as a count: {row}"
        );
        assert_eq!(cull_row(None), "cull not read back yet");
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
        let first = run(&headless(16)).expect("headless runs everywhere");
        let second = run(&headless(16)).expect("headless runs everywhere");
        assert_eq!(first, second, "two identical runs must agree exactly");
        assert_eq!(first.frames, 16);
        assert_eq!(first.exit, ExitReason::FrameBudget);
        assert_eq!(first.camera, CameraMode::Fixed);
    }

    /// **The summary names the paths the frames were drawn through**, and the
    /// two numbers the charter asks be recorded beside them.
    ///
    /// The null backend registers `NullInstance::gpu_driven`, whose bundle
    /// carries `DRAW_INDIRECT_COUNT` and `DESCRIPTOR_INDEXING` and **not**
    /// `MESH_SHADER` — so the answer here is the middle geometry path and the
    /// better binding model. Two different values rather than two defaults,
    /// which is what makes this an assertion about a device rather than about a
    /// struct literal.
    #[test]
    fn the_headless_summary_names_the_selected_paths_and_the_counts() {
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
        assert_eq!(
            summary.triangles,
            crate::face::quarry_face(crate::gpu::CELLS).triangles(),
            "the summary must report the face the window actually made resident",
        );
        assert_eq!(summary.lod_budget, Options::default().lod_budget);
    }

    /// **Forcing a lesser path really opens a lesser device**, and the summary
    /// says the run asked for it.
    ///
    /// The observable is the path the *device* selected, not the flag: a flag
    /// that reached `Options` and never reached `DeviceDesc` would leave this
    /// reporting the device's own path while claiming to have forced something.
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

    /// **`--lod-budget` reaches the frame the renderer drew**, not just
    /// `Options`.
    ///
    /// The observable is `ForwardRenderer::lod_params`, which is what
    /// `begin_frame` wrote into the frame block — **not** a field this crate
    /// stored. A flag that reached [`Options`] and never reached
    /// `set_lod_error_budget` would leave every frame selecting under the
    /// renderer's own default while the panel and the summary reported the
    /// number that was asked for, and a test reading back the request would
    /// pass either way.
    #[test]
    fn the_lod_budget_flag_reaches_the_frame_and_the_summary() {
        const COARSE: f32 = 64.0;
        assert_ne!(
            COARSE,
            Options::default().lod_budget,
            "this test compares against the default and they are equal",
        );

        let mut options = headless(4);
        options.lod_budget = COARSE;
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().frame_lod_budget(),
            COARSE,
            "the frame selected under {} px, so the flag never reached begin_frame",
            engine.gpu().frame_lod_budget(),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");

        let summary = run(&options).expect("a coarser budget still runs");
        assert_eq!(summary.lod_budget, COARSE);
    }

    /// **`--lod-view` reaches the renderer**, and the pause row moves it back.
    #[test]
    fn the_lod_view_flag_and_its_row_reach_the_renderer() {
        let mut options = headless(400);
        options.lod_view = true;
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::LodTint,
            "the flag never reached the frame"
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // Four rows down from RESUME is LOD VIEW: the loop's three, then CAMERA.
        press_row(&mut engine, window, 4);
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::Shaded,
            "the row did not reach the renderer",
        );
        assert!(
            ui_text(&engine).iter().any(|text| text == "LOD VIEW: OFF"),
            "the row's label must show what the frame now draws: {:?}",
            ui_text(&engine),
        );

        press_row(&mut engine, window, 0);
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::LodTint,
            "the row did not put it back"
        );
        assert!(
            ui_text(&engine).iter().any(|text| text == "LOD VIEW: ON"),
            "{:?}",
            ui_text(&engine),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **`--heatmap` reaches the renderer, and its row replaces the tint rather
    /// than joining it.**
    ///
    /// The exclusivity is checked *through the renderer* rather than on
    /// [`menu::toggled_to`] alone — that function has its own unit test — because
    /// what a reviewer sees is the frame, and the two settings are separate
    /// booleans on the far side of this crate. A press that left the tint set
    /// would still draw the heatmap, by the renderer's precedence, and would
    /// then draw the tint the moment the heatmap row was pressed again.
    #[test]
    fn the_heatmap_flag_and_its_row_replace_the_tint_at_the_renderer() {
        let mut options = headless(400);
        options.heatmap = true;
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::Heatmap,
            "the flag never reached the frame"
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert!(
            ui_text(&engine).iter().any(|text| text == "HEATMAP: ON")
                && ui_text(&engine).iter().any(|text| text == "LOD VIEW: OFF"),
            "one overlay is drawn, so exactly one row says ON: {:?}",
            ui_text(&engine),
        );

        // Four rows down from RESUME is LOD VIEW, five is HEATMAP. Pressing the
        // tint's row from here must *swap* the overlay.
        press_row(&mut engine, window, 4);
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::LodTint,
            "the tint's row did not replace the heatmap",
        );
        assert!(
            ui_text(&engine).iter().any(|text| text == "HEATMAP: OFF")
                && ui_text(&engine).iter().any(|text| text == "LOD VIEW: ON"),
            "{:?}",
            ui_text(&engine),
        );

        // And back the other way, off the heatmap's own row.
        press_row(&mut engine, window, 1);
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::Heatmap,
            "the heatmap's row did not replace the tint",
        );
        // Pressing it again is the way back to the shaded picture, which is what
        // makes each row its own off switch.
        press_row(&mut engine, window, 0);
        assert_eq!(engine.gpu().debug_view(), DebugView::Shaded);
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The freeze key and its row pin the selection at the camera, and the
    /// renderer is what says so.**
    ///
    /// The observable is `ForwardRenderer::frozen_selection_eye` — what the next
    /// `begin_frame` will actually select from — rather than this fixture's own
    /// field, on `--lod-budget`'s terms: a key that reached [`Quarry::frozen`]
    /// and never reached `set_frozen_selection_eye` would leave every frame
    /// selecting from the live camera while the panel said `frozen`, and a test
    /// reading back the request would pass either way.
    ///
    /// **And it composes.** The last two assertions are the whole reason
    /// freezing is not a fourth [`DebugView`]: a reviewer pins the cut *in order
    /// to* look at it through the LOD tint, so switching the tint on must not
    /// unpin the selection and pinning must not clear the tint.
    #[test]
    fn the_freeze_key_and_row_pin_the_selection_at_the_renderer() {
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().frozen_selection_eye(),
            None,
            "a run nobody pressed the key in must select from the live camera",
        );

        let standing_at = engine.game().camera().eye;
        engine
            .shell_mut()
            .key_press(window, menu::FREEZE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            engine.gpu().frozen_selection_eye(),
            Some(standing_at),
            "the key never reached the renderer, or pinned somewhere the camera is not",
        );

        // The release must not undo it — a freeze that lasted as long as the key
        // was held would be unusable and would pass every assertion above.
        engine
            .shell_mut()
            .key_release(window, menu::FREEZE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(engine.gpu().frozen_selection_eye(), Some(standing_at));

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());
        assert!(
            ui_text(&engine)
                .iter()
                .any(|text| text == "FREEZE SELECTION: ON"),
            "{:?}",
            ui_text(&engine),
        );

        // Six rows down from RESUME is FREEZE SELECTION. Pressing it releases
        // the pin.
        press_row(&mut engine, window, 6);
        assert_eq!(
            engine.gpu().frozen_selection_eye(),
            None,
            "the row did not reach the renderer",
        );
        assert!(
            ui_text(&engine)
                .iter()
                .any(|text| text == "FREEZE SELECTION: OFF"),
            "{:?}",
            ui_text(&engine),
        );

        // Back on, then the tint on top of it: two rows that must both hold.
        press_row(&mut engine, window, 0);
        assert_eq!(engine.gpu().frozen_selection_eye(), Some(standing_at));
        // FREEZE SELECTION is the last of seven rows and the selection wraps, so
        // five more downs from it lands on LOD VIEW.
        press_row(&mut engine, window, 5);
        assert_eq!(
            engine.gpu().debug_view(),
            DebugView::LodTint,
            "five rows down from FREEZE SELECTION is LOD VIEW",
        );
        assert_eq!(
            engine.gpu().frozen_selection_eye(),
            Some(standing_at),
            "switching an overlay on unpinned the selection — freezing is not one of the views",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **F3 shows the path report and this sample's own numbers.**
    ///
    /// Rule 4 and rule 12 in one test: the panel opens, and the rows that make
    /// this a *fixture* rather than a picture — which path drew it, and what the
    /// cut cost — are on it.
    #[test]
    fn f3_shows_the_path_report_and_the_cut() {
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
        for section in ["paths", "quarry"] {
            assert!(
                titles.contains(&section),
                "no {section} section on the panel: {titles:?}"
            );
        }
        // And the rows really reached the draw list, not just the panel.
        let drawn = ui_text(&engine);
        for row in [
            "geometry",
            "lod budget",
            "clusters kept",
            "clusters rejected",
            "triangles",
        ] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The camera row swaps the camera, and swapping back restores the dolly
    /// pose.**
    ///
    /// The observable is the camera's eye, not the mode: a toggle that flipped
    /// the enum and left `camera()` returning the same pose would pass every
    /// assertion about the mode alone.
    #[test]
    fn the_camera_row_swaps_the_camera_and_returns_it_to_the_dolly_pose() {
        let fixed = camera::dolly(camera::DOLLY_START);
        let mut engine = scripted(&headless(400));
        let window = engine.window();
        for _ in 0..2 {
            assert_eq!(engine.frame().expect("a frame"), Flow::Continue);
        }
        assert_eq!(engine.game().camera().eye, fixed.eye);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(engine.is_paused());

        // Three rows down from RESUME is CAMERA; the second visit is already on
        // it, which is what `press_row`'s docs are about. The cycle is three
        // long, so the free camera is two presses away from the goldens' pose.
        press_row(&mut engine, window, 3);
        assert_eq!(engine.game().camera_mode(), CameraMode::Dolly);
        assert!(
            ui_text(&engine).iter().any(|text| text == "CAMERA: DOLLY"),
            "the row's label must show the new value: {:?}",
            ui_text(&engine),
        );
        press_row(&mut engine, window, 0);
        assert_eq!(engine.game().camera_mode(), CameraMode::Free);
        assert!(
            ui_text(&engine).iter().any(|text| text == "CAMERA: FREE"),
            "the row's label must show the new value: {:?}",
            ui_text(&engine),
        );

        // Resume, fly, and confirm the free camera actually moved.
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
        let flown = engine.game().camera().eye;
        assert_ne!(flown, fixed.eye, "W did not fly");
        assert!(engine.game().flyer().has_moved());

        // And the row puts it back where the goldens were taken from.
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
            fixed.eye,
            "swapping back did not return to the dolly's start pose",
        );
        // **The flyer, not `camera()`.** In `Fixed` the frame is drawn from the
        // dolly whatever the free camera is doing, so the assertion above passes
        // for a row that swapped the mode and left the flown position where it
        // was — and the next swap to `Free` would then land the reviewer back
        // out in the rock instead of at the goldens' framing.
        assert_eq!(
            engine.game().flyer().eye(),
            fixed.eye,
            "the row swapped the mode and left the free camera where it was flown to",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The speed is the distance over the stated time.**
    ///
    /// Arithmetic over constants, so this is the cheapest place the doc comment
    /// can be held to what it says. The other half of that claim — that the
    /// dolly is slower than a reviewer flies — is the `const` assertion above,
    /// which fails the build rather than a test.
    #[test]
    fn the_dolly_speed_is_the_distance_over_the_stated_time() {
        let travelled =
            camera::dolly(camera::DOLLY_END).eye - camera::dolly(camera::DOLLY_START).eye;
        assert!(
            (DOLLY_SPEED * DOLLY_SECONDS - travelled.length()).abs() < 1e-3,
            "{DOLLY_SPEED} m/s for {DOLLY_SECONDS}s does not cover the {} m the dolly moves",
            travelled.length(),
        );
    }

    /// **The dolly turns round at the ends rather than jumping back to the
    /// start**, so the loop has no discontinuity in it.
    ///
    /// The artefact this sample exists to disprove is a cut that jumps, and a
    /// sawtooth camera would produce one every [`DOLLY_SECONDS`] — ninety metres
    /// of translation in a single tick — while satisfying every assertion about
    /// the range being covered. So the observable is the *step between
    /// consecutive ticks*, held to what one tick of travel can be, across a
    /// period and a half: long enough to contain both turns and the wrap.
    ///
    /// The walk also has to reach both ends, or a function that never left the
    /// start pose would pass the smoothness half on its own.
    #[test]
    fn the_dolly_turns_round_rather_than_jumping_back() {
        let step = 1.0 / f64::from(crate::args::DEFAULT_TICK_HZ) as f32;
        // One tick of travel, plus the slack a float sum accumulates over the
        // thousands of steps below.
        let most = step / DOLLY_SECONDS * 1.01;
        let mut previous = dolly_at(0.0);
        let (mut lowest, mut highest) = (previous, previous);
        let mut elapsed = step;
        while elapsed <= DOLLY_PERIOD * 1.5 {
            let at = dolly_at(elapsed);
            assert!(
                (at - previous).abs() <= most,
                "the dolly jumped from {previous} to {at} at {elapsed}s, and one tick is {most}",
            );
            lowest = lowest.min(at);
            highest = highest.max(at);
            previous = at;
            elapsed += step;
        }
        assert!(
            (lowest - camera::DOLLY_START).abs() < most,
            "the dolly never came back to the start pose: it stopped at {lowest}",
        );
        assert!(
            (highest - camera::DOLLY_END).abs() < most,
            "the dolly never reached the far end: it stopped at {highest}",
        );
    }

    /// **The animated dolly runs on the simulation clock**, and only while it is
    /// the camera being drawn from.
    ///
    /// The observable is the camera's own position, not the accumulator: a mode
    /// that advanced a field `camera()` never read would leave the frame frozen
    /// while every number about it moved. The second half is what the field's
    /// docs promise — an animated camera that ran on unwatched would jump to
    /// wherever the clock had reached the moment it was selected, which is the
    /// same discontinuity the ping-pong exists to avoid.
    #[test]
    fn the_dolly_runs_on_the_simulation_clock_only_while_it_is_selected() {
        let mut options = headless(400);
        options.camera = CameraMode::Dolly;
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let from = engine.game().camera().eye;
        let ticks = engine.ticks();

        for _ in 0..40 {
            engine.frame().expect("a frame");
        }
        let ran = engine.ticks() - ticks;
        let moved = (engine.game().camera().eye - from).length();
        #[allow(clippy::cast_precision_loss)]
        let expected = DOLLY_SPEED * ran as f32 / crate::args::DEFAULT_TICK_HZ as f32;
        assert!(
            (moved - expected).abs() < expected * 0.1,
            "{ran} ticks moved {moved} m, and {DOLLY_SPEED} m/s for {ran}/{} s is {expected} m",
            crate::args::DEFAULT_TICK_HZ,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");

        // And the same run on the fixed camera does not move at all, which is
        // what says the tick is stepping the mode rather than a clock.
        let mut engine = scripted(&headless(400));
        engine.frame().expect("a frame");
        let held = engine.game().camera().eye;
        for _ in 0..40 {
            engine.frame().expect("a frame");
        }
        assert_eq!(
            engine.game().camera().eye,
            held,
            "the fixed camera moved, so the dolly is advancing whatever the mode is",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The free camera flies on the **clock** rather than on the frame count,
    /// and at this sample's own speed.
    ///
    /// The whole reason the camera is stepped inside `run_ticks`: one advanced
    /// once per frame would cross the face at a rate that depends on how fast
    /// the machine is.
    #[test]
    fn the_free_camera_flies_on_the_simulation_clock() {
        let mut options = headless(400);
        options.camera = CameraMode::Free;
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        let from = engine.game().camera().eye;
        let ticks = engine.ticks();

        engine
            .shell_mut()
            .key_press(window, KeyCode::KeyW)
            .expect("the window is live");
        for _ in 0..40 {
            engine.frame().expect("a frame");
        }
        let ran = engine.ticks() - ticks;
        let flew = (engine.game().camera().eye - from).length();
        #[allow(clippy::cast_precision_loss)]
        let expected = camera::FLY_SPEED * ran as f32 / crate::args::DEFAULT_TICK_HZ as f32;
        assert!(
            (flew - expected).abs() < expected * 0.1,
            "{ran} ticks flew {flew} m, and {} m/s for {ran}/{} s is {expected} m",
            camera::FLY_SPEED,
            crate::args::DEFAULT_TICK_HZ,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
