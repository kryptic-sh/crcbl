//! Start-up, and the [`HostedGame`] methods the engine's loop calls.
//!
//! # There is no loop in this file
//!
//! ```text
//! Loop::frame()                     ← the engine's
//!   pump, menu, pause, fullscreen, resize
//!     ──────────────────────────────→ Viewer::button_event   (pan press)
//!     ──────────────────────────────→ Viewer::wheel_event    (zoom)
//!     ──────────────────────────────→ Viewer::pointer_event  (orbit press, drag)
//!     ──────────────────────────────→ Viewer::key_event      (F, I, W)
//!   run_ticks  ─────────────────────→ Viewer::tick           (nothing at all)
//!   draw_list.clear()
//!     ──────────────────────────────→ Viewer::draw           (re-frame, camera,
//!                                                             wireframe,
//!                                                             listing panel)
//!     menu, debug overlay             ← the engine's
//!   gpu.frame()
//! ```
//!
//! **This file used to be a hand-written loop**, and the reason it was is gone.
//! [`crcbl::engine::Loop`] folded a pump down to a position and the primary
//! button's two edges: a scroll reached no hook at all and a middle-button drag
//! reached nothing, so a model viewer could not be hosted. It now delivers the
//! whole pointer — [`HostedGame::wheel_event`], [`HostedGame::button_event`] and
//! [`PointerUpdate::motion`] — and this sample is what that change was made for
//! and what proves it is enough.
//!
//! What came back with the frame is everything the loop gives every other
//! sample and a copied loop had dropped: the menu, `F11`, and **rule 4's debug
//! panel**, so `--debug-overlay` reaches something rather than parsing and doing
//! nothing. See [`crate::menu`] for what the panel is for in an application with
//! nothing to pause.
//!
//! # It simulates nothing, and that is the charter exception
//!
//! `docs/plan/sample/00-samples-overview.md` rule 2 makes every sample
//! client/server authoritative. `docs/plan/sample/05-viewer.md` names this
//! sample as the one sanctioned exception: rule 2 exists so a *game*'s state
//! lives on the server, and there is no state here — the file is on disk, the
//! camera is the user's, and nothing else changes. So [`Viewer::tick`] is empty
//! and there is no `GameModule` below, and their absence is the exception rather
//! than an oversight.

use crcbl::core::input::{KeyCode, PointerButton, ScrollDelta};
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, LoopError, PointerUpdate, RunSummary,
    open_shell, open_window, requested_window_size, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::render::OrbitCamera;
use crcbl::render::cull::Aabb;
use crcbl::scene::gltf_render::Skip;
use crcbl::shell::DisplayMode;
use crcbl::ui::{DebugModule, DebugSection, draw_list::DrawList};

use crate::args::Options;
use crate::controls::Controls;
use crate::gpu::Gpu;
use crate::listing::Listing;
use crate::menu::{MenuKind, Menus};
use crate::model::{self, LoadError, Model};

/// Frames the model again, fitting it in the view from wherever the camera is.
///
/// One of the three keys this application binds. None of them is one of the
/// loop's, so all three arrive through [`HostedGame::key_event`] like any other
/// game's.
pub const REFRAME_KEY: KeyCode = KeyCode::KeyF;

/// Shows or hides [`crate::listing`]'s panel. Off to begin with.
///
/// `I` for the information it puts on screen, and it is free: the loop reserves
/// [`PAUSE_KEY`](crcbl::engine::PAUSE_KEY),
/// [`DEBUG_OVERLAY_KEY`](crcbl::engine::DEBUG_OVERLAY_KEY) and
/// [`FULLSCREEN_KEY`](crcbl::engine::FULLSCREEN_KEY), arbitrates
/// [`MENU_UP_KEY`](crcbl::engine::MENU_UP_KEY),
/// [`MENU_DOWN_KEY`](crcbl::engine::MENU_DOWN_KEY) and
/// [`MENU_ACTIVATE_KEY`](crcbl::engine::MENU_ACTIVATE_KEY) while a menu is
/// showing, and this application already binds [`REFRAME_KEY`]. `I` is none of
/// those, and `no_two_bindings_claim_the_same_key` is what keeps that true.
pub const LISTING_KEY: KeyCode = KeyCode::KeyI;

/// Draws the document's triangles as lines instead of filling them. Off to
/// begin with.
///
/// `W` for wireframe, and it is free on [`LISTING_KEY`]'s terms: it is none of
/// the three keys the loop reserves, none of the three it arbitrates while a
/// menu is showing, and neither of the two this application already binds. This
/// viewer has no keyboard movement — the turntable is a pointer gesture, see
/// [`crate::controls`] — so `W` is not the half of a `WASD` set the way it would
/// be in a game.
pub const WIREFRAME_KEY: KeyCode = KeyCode::KeyW;

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
///
/// The document is read **before** the loop is built, so [`Viewer`]'s own
/// `HostedGame::Error` is [`Infallible`](core::convert::Infallible): once there
/// is a window there is nothing left in this application that can fail.
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

/// The viewer, as the engine's loop hosts it.
#[derive(Debug)]
pub struct Viewer {
    /// The turntable. Not a [`Camera`]: an eye and a target integrated directly
    /// drift apart, and "orbit" would depend on how far away the target
    /// happened to be — see [`OrbitCamera`].
    orbit: OrbitCamera,
    controls: Controls,
    /// What the model occupies, kept so [`REFRAME_KEY`] can fit it again after
    /// the user has zoomed away.
    bounds: Aabb,
    /// The extent the last frame was drawn at, refreshed in [`Viewer::draw`].
    ///
    /// A drag is measured against the window's height, and the input for a
    /// frame is routed before that frame's swapchain is resized — so this is
    /// the surface the pointer actually moved across, which is the one the
    /// gesture means. A hosted game is handed the GPU only in `tick` and
    /// `draw`, which is why it is remembered rather than asked for.
    extent: (u32, u32),
    /// [`REFRAME_KEY`] was pressed since the last frame drew.
    ///
    /// Deferred rather than applied in `key_event` because framing needs the
    /// window's aspect, and the extent above is a frame behind while the input
    /// is being routed. Applied in [`Viewer::draw`], which is handed the GPU.
    reframe: bool,
    /// Whether [`REFRAME_KEY`] is currently held.
    ///
    /// The loop forwards a key's auto-repeats as further presses — a menu walks
    /// its list on them — so an edge is the caller's to find. Without it a held
    /// `F` would re-frame on every frame, which fights a drag made with the
    /// other hand.
    reframe_held: bool,
    /// Whether [`LISTING_KEY`] is currently held, for the reason `reframe_held`
    /// above exists: the loop forwards auto-repeats as further presses, and a
    /// panel that toggled on every one would strobe under a resting finger.
    listing_held: bool,
    /// What the frame is drawn with: lines, or filled triangles.
    ///
    /// **The state actually in force, not the one asked for.** [`Viewer::draw`]
    /// writes back whatever [`Gpu::set_wireframe`](crate::gpu::Gpu::set_wireframe)
    /// answered, so a device with no line fill mode leaves this `false` and the
    /// debug panel's row says so — rather than claiming a wireframe over a solid
    /// frame.
    wireframe: bool,
    /// [`WIREFRAME_KEY`] was pressed since the last frame drew.
    ///
    /// Deferred to [`Viewer::draw`] on `reframe`'s terms: switching the view on
    /// builds a pipeline, and a hosted game is handed the GPU only in `tick` and
    /// `draw`. Applied on the edge alone rather than once a frame, so a device
    /// that refuses logs a line per press instead of one per frame.
    wireframe_pending: bool,
    /// Whether [`WIREFRAME_KEY`] is currently held, for `listing_held`'s reason.
    wireframe_held: bool,
    /// `docs/plan/sample/05-viewer.md` milestone 2's listing — see
    /// [`crate::listing`]. Hidden until [`LISTING_KEY`] is pressed.
    listing: Listing,
    instances: usize,
    skipped: usize,
}

/// The loop the viewer runs in.
///
/// A type alias, because the loop is the engine's. `S` is the shell type: the
/// native path builds `Loop<dyn Shell>`, and the tests build
/// `Loop<HeadlessShell>` so they can inject the events a compositor would send.
pub type Loop<S = dyn Shell> = crcbl::engine::Loop<S, Viewer>;

/// Loads the model, opens a window and a device, and runs until something stops
/// it.
///
/// # Errors
///
/// [`ViewerError`] from the load, from start-up, from the frame that failed, or
/// from teardown.
pub fn run(options: &Options) -> Result<Summary, ViewerError> {
    Ok(crcbl::engine::drive(start(options)?)?)
}

/// Reads the model, then opens a shell, a window and a device for it.
///
/// **The file first.** A bad path is the most likely way this application fails
/// and it is the one a user can fix, so it is reported before a window flashes
/// up and disappears.
///
/// # Errors
///
/// [`ViewerError`] if the document would not load or the window system, window
/// or device refused.
pub fn start(options: &Options) -> Result<Loop, ViewerError> {
    let model = load_and_report(options)?;
    let shell = open_shell(options.common.headless)?;
    with_shell(shell, options, model)
}

/// Builds the loop on an already-open shell and an already-loaded model.
///
/// Separate from [`start`] so a test can play compositor on a concrete
/// [`HeadlessShell`](crcbl::shell::HeadlessShell) — the same split every sample
/// has, and here it is the only way to script a drag.
///
/// # Errors
///
/// [`ViewerError`] if the window never configured or the device would not open.
pub fn with_shell<S: Shell + ?Sized>(
    mut shell: Box<S>,
    options: &Options,
    model: Model,
) -> Result<Loop<S>, ViewerError> {
    let clock_source = Clock::new(options.common.headless);
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
    let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
    let gpu = Gpu::open(
        shell.as_ref(),
        window,
        extent,
        options.common.gpu(),
        &model.render.scene,
        &model.render.instances,
        // The grid is scaled to the document's own size — see `crate::gpu`.
        // The largest axis rather than the diagonal, so a long thin model does
        // not get a cell sized for a span it only has in one direction.
        model.bounds.half_extent().max_element() * 2.0,
    )?;

    // Frame on load, against the extent the window actually configured at: an
    // aspect guessed from the requested size would frame a model that hangs off
    // the sides of the window it is really in.
    let mut orbit = OrbitCamera::new(model.bounds.center(), 1.0, Projection::default());
    orbit.frame(model.bounds, aspect_of(extent));

    // Read off the document once, here, because nothing in this milestone can
    // change it afterwards — see `Listing::of`.
    let listing = Listing::of(&model);

    Ok(Loop::new(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        Viewer {
            orbit,
            controls: Controls::new(),
            bounds: model.bounds,
            extent,
            reframe: false,
            reframe_held: false,
            listing_held: false,
            wireframe: false,
            wireframe_pending: false,
            wireframe_held: false,
            listing,
            instances: model.render.instances.len(),
            skipped: model.render.skipped.len(),
        },
        options.common.loop_config(),
    ))
}

impl Viewer {
    /// Where the frame is drawn from.
    #[must_use]
    pub fn camera(&self) -> Camera {
        self.orbit.camera()
    }

    /// What the model occupies, in world space.
    #[must_use]
    pub const fn bounds(&self) -> Aabb {
        self.bounds
    }
}

/// The viewer's half of the frame, and nothing else.
impl HostedGame for Viewer {
    /// The document is read before the loop exists — see [`ViewerError`] — so
    /// there is nothing left here that can fail. Uninhabited rather than a
    /// placeholder, which is the type system agreeing.
    type Error = core::convert::Infallible;
    type Gpu = Gpu;
    type MenuKind = MenuKind;
    /// Every button on this sample's one panel is the loop's; see
    /// [`crate::menu`] for why there is no fourth.
    type MenuAction = core::convert::Infallible;
    type Summary = Summary;

    const NAME: &'static str = "viewer";

    fn menus() -> Menus {
        crate::menu::menus()
    }

    /// Nothing is simulated, so nothing steps. See the [module docs](self) for
    /// why that is a charter exception rather than an empty call site waiting to
    /// be filled in: there is no state here to advance.
    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {}

    /// `F` frames the model again, `I` shows the listing and `W` draws it as
    /// lines — each once per press.
    ///
    /// Framing and the wireframe are deferred to [`Viewer::draw`], which is
    /// where this application is handed the aspect and the GPU; the panel needs
    /// neither, so it flips here.
    fn key_event(&mut self, key: KeyCode, pressed: bool) {
        match key {
            REFRAME_KEY => {
                self.reframe |= pressed && !self.reframe_held;
                self.reframe_held = pressed;
            }
            LISTING_KEY => {
                if pressed && !self.listing_held {
                    self.listing.toggle();
                }
                self.listing_held = pressed;
            }
            WIREFRAME_KEY => {
                if pressed && !self.wireframe_held {
                    self.wireframe = !self.wireframe;
                    self.wireframe_pending = true;
                }
                self.wireframe_held = pressed;
            }
            _ => {}
        }
    }

    /// The orbit drag: the primary button's edges, and the movement of the
    /// pointer while any drag is running.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        self.controls.pointer(pointer, self.extent, &mut self.orbit);
    }

    /// The pan drag. Both non-primary buttons start one — see
    /// [`crate::controls`].
    fn button_event(&mut self, button: PointerButton, pressed: bool) {
        self.controls.button(button, pressed);
    }

    fn wheel_event(&mut self, delta: ScrollDelta) {
        Controls::wheel(delta, &mut self.orbit);
    }

    /// The viewer's panel carries no id of its own, so the loop never asks.
    fn menu_action(_id: crcbl::ui::WidgetId) -> Option<core::convert::Infallible> {
        None
    }

    fn apply(&mut self, action: core::convert::Infallible) {
        match action {}
    }

    fn menu_kind(&mut self, _menus: &mut Menus, paused: bool) -> MenuKind {
        MenuKind::of(paused)
    }

    /// Hands the camera to the GPU, re-frames first if `F` asked, and appends
    /// the listing panel if `I` did.
    ///
    /// The panel is the only thing this sample puts in `draw_list` — there is
    /// still no HUD. It goes in first, so the debug overlay the loop appends
    /// after this returns is the one on top where the two overlap. They are
    /// pinned to opposite corners, so today they do not.
    fn draw(&mut self, gpu: &mut Gpu, draw_list: &mut DrawList, _frame: FrameInfo) {
        // Read here rather than on resize, so it is the extent this frame is
        // actually drawn at — the loop has already applied any resize by now.
        self.extent = gpu.extent();
        if std::mem::take(&mut self.reframe) {
            self.orbit.frame(self.bounds, aspect_of(self.extent));
        }
        // The answer, not the request — see [`Viewer::wireframe`]. A device that
        // has no line fill mode leaves this `false`, and the debug row below
        // reports the frame that is actually drawn.
        if std::mem::take(&mut self.wireframe_pending) {
            self.wireframe = gpu.set_wireframe(self.wireframe);
        }
        gpu.set_camera(self.orbit.camera());
        // Laid out against the same extent, and with the atlas the UI pass will
        // actually draw with: a panel measured with a second atlas is a
        // background rect the wrong size for the text inside it.
        self.listing.render(
            draw_list,
            crcbl::math::Vec2::new(self.extent.0 as f32, self.extent.1 as f32),
            gpu.atlas(),
        );
    }

    fn debug_sections(&self, panel: &mut crcbl::ui::DebugPanel) {
        panel.add(self);
    }

    fn summary(&self, run: RunSummary) -> Summary {
        Summary {
            backend: run.backend,
            frames: run.frames,
            events: run.events,
            extent: run.extent,
            mode: run.mode,
            exit: run.exit,
            instances: self.instances,
            skipped: self.skipped,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "viewer: {} frames, {} events on the {} shell at {}x{} ({} instances, {} skipped, {:?})",
            summary.frames,
            summary.events,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.instances,
            summary.skipped,
            summary.exit,
        );
    }
}

/// The viewer's own rows on the debug panel.
///
/// The three numbers a person looking at an unfamiliar document wants and
/// cannot get any other way: how much of it was placed, how much of it was
/// refused, and how far away the camera has ended up — which is what says
/// "nothing is on screen because you zoomed past it" rather than "nothing is on
/// screen because the file is empty".
///
/// The fourth row is the wireframe, and it reports the frame that was **drawn**:
/// the field behind it holds what the device answered rather than what
/// [`WIREFRAME_KEY`] asked for, so a press that could not be honoured leaves
/// this saying `off` instead of quietly disagreeing with the picture.
impl DebugModule for Viewer {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("viewer");
        out.row("instances", format_args!("{}", self.instances));
        out.row("skipped", format_args!("{}", self.skipped));
        out.row("dist", format_args!("{:.2}", self.orbit.distance()));
        out.row(
            "wireframe",
            format_args!("{}", if self.wireframe { "on" } else { "off" }),
        );
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
/// [`ViewerError::Load`] wrapping the [`LoadError`], or a document nothing at
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

    use crcbl::engine::{
        DEBUG_OVERLAY_KEY, FULLSCREEN_KEY, Flow, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, MENU_UP_KEY,
        PAUSE_KEY,
    };
    use crcbl::math::Vec3;
    use crcbl::shell::{HeadlessShell, PhysicalPoint};

    use crate::fixture;

    /// A directory holding one `.glb`, and the options that open it.
    ///
    /// The directory has to outlive the run, so it is returned beside the
    /// options rather than dropped at the end of this function — a `TempDir`
    /// Which backend this crate's tests drive, `Null` unless `CRCBL_GPU` names
    /// another.
    ///
    /// **So a viewer frame can reach a real driver.** Every other sample gets a
    /// lavapipe run in CI; this one has none, because the samples' CI steps run
    /// the *binary* headless and the viewer's binary needs a `.glb` on disk that
    /// the repo does not carry — fixtures here are built in code rather than
    /// vendored, deliberately, so a diff shows what changed. These tests already
    /// build one and drive the whole path, so pointing them at a driver is the
    /// cheaper route to the same coverage.
    ///
    /// **An unparseable name is a panic, not a fallback.** `hal_seam_e2e`'s
    /// `assert_backend_matches_the_pin` exists for this: a run that quietly
    /// substitutes `Null` is a green result that is evidence about a backend
    /// nobody named, which is worse than no run.
    fn test_backend() -> GpuBackend {
        match std::env::var("CRCBL_GPU") {
            Err(_) => GpuBackend::Null,
            Ok(name) => GpuBackend::from_name(&name).unwrap_or_else(|| {
                panic!(
                    "CRCBL_GPU names {name:?}, which is not a backend — refusing to fall back to \
                     Null and report a pass about a backend nobody asked for"
                )
            }),
        }
    }

    /// deletes its tree when it goes.
    fn model_at(document: &[u8], frames: u64) -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("panel.glb");
        std::fs::write(&path, document).expect("the document");
        let mut common = Common::new(crate::args::DEFAULT_TICK_HZ);
        common.headless = true;
        common.frames = Some(frames);
        common.backend = Some(test_backend());
        (
            dir,
            Options {
                common,
                model: path,
            },
        )
    }

    fn scripted(options: &Options) -> Loop<HeadlessShell> {
        let model = model::load(&options.model).expect("the fixture loads");
        with_shell(Box::new(HeadlessShell::new()), options, model).expect("headless always starts")
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

    /// One whole press of `key`: the press and the release the platform sends
    /// after it, then the frame that routes both.
    ///
    /// `HeadlessShell::key_press` injects the press alone, and a binding that
    /// finds the edge of a press stays latched without the release — see
    /// `a_held_listing_key_toggles_the_panel_once_not_once_a_frame`.
    fn tap(engine: &mut Loop<HeadlessShell>, window: crcbl::shell::WindowId, key: KeyCode) {
        engine
            .shell_mut()
            .key_press(window, key)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, key)
            .expect("the window is live");
        engine.frame().expect("a frame");
    }

    /// The value drawn immediately after the row labelled `label`.
    fn row_value(drawn: &[String], label: &str) -> String {
        let at = drawn
            .iter()
            .position(|text| text == label)
            .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
        drawn
            .get(at + 1)
            .unwrap_or_else(|| panic!("no value after {label} in {drawn:?}"))
            .clone()
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
        let engine = scripted(&options);
        let camera = engine.game().camera();

        assert!(
            (camera.target - fixture::QUAD_CENTRE).length() < 1e-4,
            "the camera looks at {:?}, and the model is at {:?}",
            camera.target,
            fixture::QUAD_CENTRE,
        );

        // The quad's bounding sphere has radius sqrt(0.5² + 0.5²); the eye must
        // be outside it, and inside the half-angle the projection has at this
        // window's aspect.
        let radius = engine.game().bounds().half_extent().length();
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

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Every gesture this application has, played through the engine's loop.**
    ///
    /// The claim the migration rests on: an orbit drag, a pan drag on the
    /// non-primary button, a wheel zoom and `F` all reach the camera through
    /// [`crcbl::engine::Loop`], with no loop of this crate's own in between.
    /// `crate::controls` proves each mapping is the right way round; this proves
    /// the events arrive at all — and it is what goes red if the engine stops
    /// dispatching `button_event`, `wheel_event` or `PointerUpdate::motion`.
    #[test]
    fn every_gesture_reaches_the_camera_through_the_hosted_loop() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 32);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();

        // Orbit: the primary button, which the loop delivers as
        // `PointerUpdate`'s two edges.
        let before = engine.game().camera();
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Pressed,
                Some(PhysicalPoint::ORIGIN),
            )
            .expect("the window is live");
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint::new(120.0, 0.0), (120.0, 0.0))
            .expect("the window is live");
        engine.frame().expect("a frame");
        let orbited = engine.game().camera();
        assert_ne!(orbited.eye, before.eye, "the drag never reached the camera");
        assert_eq!(orbited.target, before.target, "an orbit does not pan");
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Released,
                Some(PhysicalPoint::new(120.0, 0.0)),
            )
            .expect("the window is live");
        engine.frame().expect("a frame");

        // Pan: the middle button, which used to reach nothing at all — the loop
        // matched the primary button and dropped every other one.
        for (state, at) in [
            (
                crcbl::core::input::ButtonState::Pressed,
                PhysicalPoint::new(120.0, 0.0),
            ),
            (
                crcbl::core::input::ButtonState::Released,
                PhysicalPoint::new(180.0, 0.0),
            ),
        ] {
            if matches!(state, crcbl::core::input::ButtonState::Released) {
                engine
                    .shell_mut()
                    .move_pointer(window, at, (60.0, 0.0))
                    .expect("the window is live");
                engine.frame().expect("a frame");
            }
            engine
                .shell_mut()
                .button(window, PointerButton::Middle, state, Some(at))
                .expect("the window is live");
            engine.frame().expect("a frame");
        }
        let panned = engine.game().camera();
        assert_ne!(
            panned.target, orbited.target,
            "the middle-button drag never reached the pivot",
        );

        // Zoom: the wheel, which used to fall into the loop's `_` arm.
        let out = (panned.eye - panned.target).length();
        engine
            .shell_mut()
            .scroll(window, ScrollDelta::Lines { x: 0.0, y: 8.0 }, None)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let zoomed = {
            let camera = engine.game().camera();
            (camera.eye - camera.target).length()
        };
        assert!(
            zoomed < out,
            "the wheel did nothing: {zoomed} is not < {out}"
        );

        // And `F` puts it back: the distance the wheel left is not the framed
        // one, so a re-frame that did nothing would fail here.
        engine
            .shell_mut()
            .key_press(window, REFRAME_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let framed = {
            let camera = engine.game().camera();
            (camera.eye - camera.target).length()
        };
        assert!(
            framed > zoomed,
            "F left the eye at {framed}, which is no further out than the {zoomed} \
             the wheel had brought it to",
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A pan drag does not survive the window going away.**
    ///
    /// The release for a button held when focus is lost is one no platform
    /// sends; the loop owes it, and this sample is the one that would show the
    /// debt — a model that leaps the next time the window is clicked on.
    #[test]
    fn a_pan_drag_is_released_when_the_window_loses_focus() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();

        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Right,
                crcbl::core::input::ButtonState::Pressed,
                Some(PhysicalPoint::ORIGIN),
            )
            .expect("the window is live");
        engine.frame().expect("a frame");

        // **The drag is live first**, or the assertion below passes on a press
        // that never arrived — which is exactly what it went green on while the
        // loop's `button_event` dispatch was broken on purpose.
        let pressed = engine.game().camera();
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint::new(40.0, 0.0), (40.0, 0.0))
            .expect("the window is live");
        engine.frame().expect("a frame");
        let dragging = engine.game().camera();
        assert_ne!(
            dragging.target, pressed.target,
            "the pan drag never started, so the focus loss below proves nothing",
        );

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint::new(200.0, 100.0), (160.0, 100.0))
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            engine.game().camera(),
            dragging,
            "the pan drag survived the focus loss",
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **`--debug-overlay` reaches a panel now**, which is rule 4 and the whole
    /// point of giving the frame back to the engine.
    ///
    /// It parsed and did nothing while this sample owned its loop: there was no
    /// UI pass here at all, so the flag was a promise the binary could not keep.
    /// Both halves are checked — the panel's rows are in the frame's draw list,
    /// and the UI pass is in the frame's graph, since
    /// `UiRenderer::add_pass` declares nothing for an empty list and "drawn" and
    /// "composited" are different claims.
    #[test]
    fn f3_toggles_a_debug_panel_with_this_samples_own_rows_in_it() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).is_empty(),
            "with both panels off the viewer draws no UI at all: {:?}",
            ui_text(&engine),
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let drawn = ui_text(&engine);
        for row in ["frame", "fps", "avg", "worst", "window", "viewer"] {
            assert!(drawn.iter().any(|t| t == row), "missing {row}: {drawn:?}");
        }
        assert_eq!(
            row_value(&drawn, "instances"),
            "1",
            "the panel's numbers are this document's: {drawn:?}",
        );
        assert_eq!(row_value(&drawn, "skipped"), "0");
        assert!(
            engine.gpu().last_dump().contains("ui-composite"),
            "the panel must be composited, not merely drawn:\n{}",
            engine.gpu().last_dump(),
        );

        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            !ui_text(&engine).iter().any(|t| t == "frame"),
            "F3 hides it again",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **`I` reaches the listing panel through the engine's loop, and the panel
    /// reaches the GPU.**
    ///
    /// `crate::listing`'s own tests say the lines are right; this says they
    /// arrive. Both halves are checked, for the reason the debug-panel test
    /// above gives: the rows are in the frame's draw list *and* the UI pass is
    /// in the frame's graph, since `UiRenderer::add_pass` declares nothing for
    /// an empty list and "drawn" and "composited" are different claims.
    ///
    /// The engine's overlay is switched off, so every line asserted on here is
    /// the viewer's own — `instances` is a row on both panels.
    #[test]
    fn i_toggles_the_listing_panel_and_the_frame_composites_it() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).is_empty(),
            "the listing is off until it is asked for: {:?}",
            ui_text(&engine),
        );

        tap(&mut engine, window, LISTING_KEY);
        let drawn = ui_text(&engine);
        assert!(
            drawn.iter().any(|text| text == "panel.glb"),
            "the panel names the document it is describing: {drawn:?}",
        );
        assert_eq!(
            row_value(&drawn, "instances"),
            "1",
            "the panel's numbers are this document's: {drawn:?}",
        );
        assert!(
            drawn.iter().any(|text| text == "nothing was skipped"),
            "and it says the fixture arrived intact: {drawn:?}",
        );
        assert!(
            engine.gpu().last_dump().contains("ui-composite"),
            "the panel must be composited, not merely drawn:\n{}",
            engine.gpu().last_dump(),
        );

        tap(&mut engine, window, LISTING_KEY);
        assert!(
            ui_text(&engine).is_empty(),
            "I hides it again: {:?}",
            ui_text(&engine),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A held `I` toggles the panel once, not once a frame.**
    ///
    /// The loop hands the game a key's auto-repeats as further presses — its
    /// key fold reads the button state and never the `repeat` flag — so
    /// finding the edge is this application's job, and without it a resting
    /// finger strobes the panel at the frame rate. Two repeats rather than one,
    /// so a guard that only ignored the *first* of them would still fail.
    #[test]
    fn a_held_listing_key_toggles_the_panel_once_not_once_a_frame() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");

        engine
            .shell_mut()
            .key_press(window, LISTING_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            !ui_text(&engine).is_empty(),
            "the press never opened the panel, so the repeats below prove nothing",
        );

        for _ in 0..2 {
            engine
                .shell_mut()
                .key_repeat(window, LISTING_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
            assert!(
                ui_text(&engine).iter().any(|text| text == "panel.glb"),
                "a repeat closed the panel: {:?}",
                ui_text(&engine),
            );
        }

        engine
            .shell_mut()
            .key_release(window, LISTING_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert!(
            ui_text(&engine).iter().any(|text| text == "panel.glb"),
            "letting go closed it: {:?}",
            ui_text(&engine),
        );

        // And the next real press still works, which is what says the guard
        // released rather than latched.
        tap(&mut engine, window, LISTING_KEY);
        assert!(ui_text(&engine).is_empty(), "{:?}", ui_text(&engine));
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A held `W` toggles the wireframe once, not once a frame** — and the
    /// panel reports what the device answered.
    ///
    /// `a_held_listing_key_toggles_the_panel_once_not_once_a_frame`'s claim for
    /// the second binding that needs an edge, and this one needs it more: every
    /// press reaches the seam, so a strobing toggle is a pipeline swap a frame.
    /// Two repeats rather than one, so a guard that only ignored the *first*
    /// would still fail.
    ///
    /// The row is the observable rather than a field, because the row is what a
    /// user reads: [`Viewer::wireframe`] holds the state the GPU answered with,
    /// so a device that refused would leave this saying `off` and the assertion
    /// would name that rather than a flag agreeing with itself.
    #[test]
    fn a_held_wireframe_key_toggles_the_view_once_not_once_a_frame() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 24);
        options.common.debug_overlay = Some(true);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert!(
            engine.gpu().wireframe_supported(),
            "the null device is asked for the line fill mode and has it, so a refusal here is \
             this sample's request going missing rather than the device",
        );
        assert_eq!(
            row_value(&ui_text(&engine), "wireframe"),
            "off",
            "the view is off until it is asked for",
        );

        engine
            .shell_mut()
            .key_press(window, WIREFRAME_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "wireframe"),
            "on",
            "the press never reached the renderer, so the repeats below prove nothing",
        );

        for _ in 0..2 {
            engine
                .shell_mut()
                .key_repeat(window, WIREFRAME_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
            assert_eq!(
                row_value(&ui_text(&engine), "wireframe"),
                "on",
                "a repeat switched the view back off",
            );
        }

        engine
            .shell_mut()
            .key_release(window, WIREFRAME_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "wireframe"),
            "on",
            "letting go switched it off",
        );

        // And the next real press still works, which is what says the guard
        // released rather than latched.
        tap(&mut engine, window, WIREFRAME_KEY);
        assert_eq!(row_value(&ui_text(&engine), "wireframe"), "off");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **No two bindings claim the same key.**
    ///
    /// Three of these are the loop's reserved keys and three more are the ones
    /// it arbitrates for a menu that is showing; the other three are this
    /// application's. A collision is silent — the loop would swallow the key
    /// and the viewer's half would simply never run — so it is checked rather
    /// than argued about in a comment.
    #[test]
    fn no_two_bindings_claim_the_same_key() {
        let bound = [
            ("reframe", REFRAME_KEY),
            ("listing", LISTING_KEY),
            ("wireframe", WIREFRAME_KEY),
            ("pause", PAUSE_KEY),
            ("debug overlay", DEBUG_OVERLAY_KEY),
            ("fullscreen", FULLSCREEN_KEY),
            ("menu up", MENU_UP_KEY),
            ("menu down", MENU_DOWN_KEY),
            ("menu activate", MENU_ACTIVATE_KEY),
        ];
        for (at, (name, key)) in bound.iter().enumerate() {
            for (other_name, other) in &bound[at + 1..] {
                assert_ne!(key, other, "{name} and {other_name} are both {key:?}");
            }
        }
    }

    /// **The grid floor is in the frame this application records, and after the
    /// tonemap.**
    ///
    /// `docs/plan/sample/05-viewer.md` milestone 1's third item. The graph dump
    /// is the observable rather than a field on the renderer, for the reason the
    /// UI pass's assertion above gives: "switched on" and "in the frame" are
    /// different claims, and only the second is the one milestone 1 makes.
    ///
    /// The order matters as much as the presence — see [`crate::gpu`]. A grid
    /// recorded before the tonemap would be tonemapped like scene content, and
    /// the dump is where that is visible without a pixel to look at.
    #[test]
    fn the_frame_carries_a_grid_floor_after_the_tonemap() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 4);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");

        let dump = engine.gpu().last_dump().to_string();
        let grid = dump
            .find(r#"pass "grid""#)
            .unwrap_or_else(|| panic!("no grid pass in the viewer's frame:\n{dump}"));
        let tonemap = dump
            .find(r#"pass "tonemap""#)
            .expect("a frame has to reach the swapchain");
        assert!(
            tonemap < grid,
            "the grid must be recorded after the tonemap, not before it:\n{dump}"
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Escape opens the one panel this sample has**, which is the only way to
    /// reach fullscreen and the debug panel with a pointer.
    #[test]
    fn escape_opens_the_menu_and_escape_closes_it() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 24);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::None);

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::Menu);
        assert!(
            ui_text(&engine).iter().any(|t| t == "FULLSCREEN"),
            "the panel's buttons are on screen: {:?}",
            ui_text(&engine),
        );

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert_eq!(engine.menu_kind(), MenuKind::None);
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A window resize reaches the swapchain**, and the drag scale with it:
    /// the same movement of the hand turns the model by the same amount
    /// whatever size the window is, so the extent the gesture is measured
    /// against has to follow the window.
    #[test]
    fn a_resize_reaches_the_swapchain() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 8);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();

        engine
            .shell_mut()
            .resize(window, crcbl::shell::PhysicalSize::new(640, 400))
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(engine.gpu().extent(), (640, 400));

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// A close request stops the loop and is answered, rather than the window
    /// being left waiting for a reply that never comes.
    #[test]
    fn a_close_request_stops_the_loop() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 64);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();

        engine
            .shell_mut()
            .request_close(window)
            .expect("the window is live");
        assert_eq!(
            engine.frame().expect("a frame"),
            Flow::Stop(ExitReason::CloseRequested),
        );
        let summary = engine
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
