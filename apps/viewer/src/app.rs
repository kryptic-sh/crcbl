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
//!     ──────────────────────────────→ Viewer::key_event      (F, I, W, N, B, -, =)
//!   run_ticks  ─────────────────────→ Viewer::tick           (nothing at all)
//!   draw_list.clear()
//!     ──────────────────────────────→ Viewer::draw           (re-export poll,
//!                                                             re-frame, camera,
//!                                                             wireframe, exposure,
//!                                                             clip, skeleton,
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

use std::rc::Rc;

use crcbl::core::input::{KeyCode, PointerButton, ScrollDelta};
use crcbl::engine::{
    Booted, Clock, ExitReason, FrameInfo, HostedGame, LoopError, PointerUpdate, RunSummary,
    open_shell, open_window, requested_window_size, wait_for_configure,
};
use crcbl::prelude::*;
use crcbl::render::cull::Aabb;
use crcbl::render::{OrbitCamera, RenderEffects};
use crcbl::scene::gltf_render::Skip;
use crcbl::shell::DisplayMode;
use crcbl::ui::{DebugModule, DebugSection, draw_list::DrawList};

use crate::args::Options;
use crate::controls::Controls;
use crate::gpu::Gpu;
use crate::listing::Listing;
use crate::menu::{MenuKind, Menus};
use crate::model::{self, LoadError, Model};
use crate::watch::Watch;

/// Frames the model again, fitting it in the view from wherever the camera is.
///
/// One of the keys this application binds. None of them is one of the loop's, so
/// every one arrives through [`HostedGame::key_event`] like any other game's,
/// and `no_two_bindings_claim_the_same_key` is what keeps that true.
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

/// Draws each surface's world-space normal as a colour instead of shading it.
/// Off to begin with.
///
/// `N` for normals, and it is free on [`LISTING_KEY`]'s terms: it is none of the
/// three keys the loop reserves, none of the three it arbitrates while a menu is
/// showing, and none of the four this application already binds.
pub const NORMALS_KEY: KeyCode = KeyCode::KeyN;

/// Draws the document's posed skeleton over the frame — see [`crate::anim`].
/// Off to begin with.
///
/// **Off, on [`LISTING_KEY`]'s argument and not merely by analogy with it.** A
/// viewer's job is to show the user's asset unadorned — see [`crate`] — and
/// bones drawn over a model nobody asked to see bones on are exactly the
/// decoration this sample is not allowed to add. The clip plays either way, so
/// what the key controls is the annotation and never the document.
///
/// `B` for bones, which is what every DCC tool calls this display, and it is
/// free on [`LISTING_KEY`]'s terms: it is none of the three keys the loop
/// reserves, none of the three it arbitrates while a menu is showing, and none
/// of the keys this application already binds — which
/// `no_two_bindings_claim_the_same_key` is what keeps true.
pub const SKELETON_KEY: KeyCode = KeyCode::KeyB;

/// Darkens the picture: one press divides the tonemap's exposure by
/// [`exposure_step`].
///
/// `-` and `=` in their US-QWERTY positions, which is where every browser, every
/// image viewer and every map puts "less" and "more" — so the pair needs no
/// learning, and it is free on [`LISTING_KEY`]'s terms: neither is one of the
/// loop's three reserved keys, one of the three it arbitrates for a menu, or one
/// of the three this application already binds. The physical key rather than the
/// shifted glyph, so `+` needs no modifier and a layout that puts `+` elsewhere
/// still works.
pub const EXPOSURE_DOWN_KEY: KeyCode = KeyCode::Minus;

/// Brightens the picture, by [`EXPOSURE_DOWN_KEY`]'s factor the other way.
pub const EXPOSURE_UP_KEY: KeyCode = KeyCode::Equal;

/// How much of a stop one press of [`EXPOSURE_UP_KEY`] moves the exposure.
///
/// A third, which is the increment every camera's exposure-compensation dial
/// has: fine enough that a press is an adjustment rather than a jump, coarse
/// enough that crossing the whole of
/// [`EXPOSURE_MIN`](crcbl::render::EXPOSURE_MIN) to
/// [`EXPOSURE_MAX`](crcbl::render::EXPOSURE_MAX) — ten stops — is thirty
/// presses, which a held key covers in a second or so.
pub const EXPOSURE_STOPS_PER_PRESS: f32 = 1.0 / 3.0;

/// What one press of [`EXPOSURE_UP_KEY`] multiplies the exposure by.
///
/// **A ratio, not a difference.** Exposure is a scale, so equal ratios are what
/// feel like equal steps: adding a tenth would be a doubling at the bottom of
/// the range and invisible at the top. Two to the power of
/// [`EXPOSURE_STOPS_PER_PRESS`], because a *stop* is a doubling — which is what
/// makes three presses exactly twice as bright wherever they start.
///
/// A function rather than a `const`, because `f32::powf` is not one.
#[must_use]
pub fn exposure_step() -> f32 {
    2.0f32.powf(EXPOSURE_STOPS_PER_PRESS)
}

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
    /// Which of topic 18's effects the frames were drawn through, **resolved**.
    ///
    /// Read back off the renderer rather than copied from the request the
    /// context handed it — see [`Gpu::effects`](crate::gpu::Gpu::effects). It is
    /// this sample's only observable for its own
    /// `renderer.set_effect_request(ctx.effect_request())`, and for
    /// [`Gpu::reload`](crate::gpu::Gpu::reload) carrying that request across a
    /// document change: without it either line could be deleted and every test
    /// here would stay green. `crate::gpu`'s
    /// `the_players_video_clamp_reaches_the_frame_and_survives_a_reload` is
    /// what reads it.
    pub effects: RenderEffects,
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
    /// Whether the frame is drawn as world-space normals rather than shaded.
    ///
    /// **No `_pending` twin beside it, unlike the wireframe**, because there is
    /// nothing to defer to: the normals view builds no pipeline and cannot be
    /// refused — see [`Gpu::set_normals_view`](crate::gpu::Gpu::set_normals_view)
    /// — so this is both what was asked for and what is drawn, and
    /// [`Viewer::draw`] pushes it at the renderer every frame beside the camera.
    normals: bool,
    /// Whether [`NORMALS_KEY`] is currently held, for `listing_held`'s reason.
    normals_held: bool,
    /// Whether the posed skeleton is drawn over the frame — see
    /// [`SKELETON_KEY`], which is what flips it.
    skeleton: bool,
    /// Whether [`SKELETON_KEY`] is currently held, for `listing_held`'s reason.
    skeleton_held: bool,
    /// Net presses of [`EXPOSURE_UP_KEY`] less [`EXPOSURE_DOWN_KEY`] since the
    /// last frame drew, applied in [`Viewer::draw`].
    ///
    /// Deferred on `wireframe_pending`'s terms — the exposure lives on the
    /// renderer, and a hosted game is handed the GPU only in `tick` and `draw`.
    ///
    /// **There is no `_held` guard beside it, and that is the point.** The loop
    /// forwards a key's auto-repeats as further presses — `MenuPump::observe`
    /// does not fold them away — which the three toggles above have to defend
    /// against because flipping a switch thirty times a second is a strobe. Here
    /// it is the feature: holding the key keeps changing the value, which is
    /// what makes ten stops reachable without thirty separate presses. Counted
    /// rather than flagged so that a frame which saw several repeats moves by
    /// several steps instead of one.
    exposure_steps: i32,
    /// The exposure the last [`Viewer::draw`] left in force, as the renderer
    /// answered it.
    ///
    /// [`Viewer::wireframe`]'s distinction: the renderer clamps, so this holds
    /// what the frame is drawn with rather than the product of every press —
    /// which is what lets the listing panel say that a key has reached the end
    /// of its range.
    exposure: f32,
    /// The exposure the menu's slider is to be set to on the next
    /// [`Viewer::draw`], when the player has dragged its handle.
    ///
    /// Deferred on `exposure_steps`' terms — the renderer is reachable only
    /// from `tick` and `draw`, and [`Viewer::menu_kind`] is where the handle is
    /// read.
    exposure_pending: Option<f32>,
    /// Where [`Viewer::menu_kind`] last saw the slider's handle.
    ///
    /// **Compared for exact equality, and that is deliberate.** This holds the
    /// number the widget itself last held, not a value derived from anything,
    /// so a difference is a drag and nothing else. A tolerance here would need
    /// to be wider than one pixel of groove, which is a drag the player can see
    /// and the viewer would ignore.
    exposure_handle: f32,
    /// `docs/plan/sample/05-viewer.md` milestone 2's listing — see
    /// [`crate::listing`]. Hidden until [`LISTING_KEY`] is pressed.
    listing: Listing,
    /// Milestone 3's re-export loop — see [`crate::watch`] and [`Viewer::tick`].
    watch: Watch,
    /// How many times the document has been read again since the window opened.
    ///
    /// The debug panel's observable for the reload path, and the only one there
    /// is: a re-export that changes nothing a person can see — the same
    /// geometry moved by a millimetre — leaves every other row where it was, so
    /// without this there is no way to tell a reload that ran from one that was
    /// never offered.
    reloads: u64,
    instances: usize,
    skipped: usize,
    /// How many joints the document's skins declare — `crate::model::Rig`'s
    /// count, and the whole of the rig that reaches a number on a panel.
    ///
    /// **A count and never a name.** The clip names are on the listing panel;
    /// this row and the `[HUD]` line take the number alone, because a clip name
    /// is arbitrary text out of someone else's file and the heartbeat is parsed
    /// as `name: value` pairs by `web/tools/browser-e2e.mjs` and by this
    /// module's own tests. One space or one colon in a name would take the
    /// parse apart.
    joints: usize,
    /// What the frames are being drawn with, re-read each [`Viewer::draw`] off
    /// the renderer.
    ///
    /// Every frame rather than once at start-up, because a reload replaces the
    /// renderer: a value copied before the first document was swapped would go
    /// on reporting the request the *previous* one held.
    effects: RenderEffects,
    /// Ticks this run has taken, for the heartbeat's cadence and its first
    /// column.
    ticks: u64,
    /// The document's rig, with a playhead — see [`crate::anim`].
    ///
    /// `None` for a document with no skin, which is nearly every document: then
    /// nothing is sampled, nothing is drawn by [`SKELETON_KEY`], and the
    /// heartbeat's `pose` reports the zero a skeleton standing at rest would.
    ///
    /// Built from the [`Model`]'s own [`Playable`](crate::anim::Playable),
    /// which is the document's half and is shared; this is the half that moves,
    /// so it lives here and is rebuilt whenever a document is adopted.
    player: Option<crate::anim::Player>,
    /// How far the turntable has carried the camera, in radians.
    ///
    /// Kept only to be reported: the camera holds the actual pose. It is on the
    /// debug panel because it is the one number here that moves on its own, and
    /// the browser gate reads it to prove the frame is advancing — see
    /// `web/tools/browser-e2e.mjs`.
    turned: f32,
    /// Whether the visitor has taken hold of the camera.
    ///
    /// Latched, and never cleared: once someone has aimed this at something,
    /// having it drift away again is the tool moving under their hands. See
    /// [`TURNTABLE_RATE`].
    handed_over: bool,
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
    // The whole document, because a rigged one is a scene, its instances and
    // the rig that pairs them, and the grid extent is derived from it too — see
    // `Gpu::open`.
    let gpu = Gpu::open(shell.as_ref(), window, extent, options.common.gpu(), &model)?;

    Ok(assemble(
        Booted {
            shell,
            window,
            gpu,
            clock_source,
            events,
        },
        options,
        &model,
    ))
}

/// Builds the loop out of a bundle that has arrived, whichever path brought it.
///
/// Shared by the blocking [`with_shell`] and the polled [`PendingLoop`], so the
/// camera a browser opens on is framed by the same code as the one a terminal
/// does. The extent is read off the bundle rather than passed in: the polled
/// path may have resized the swapchain after the request went out, and the
/// aspect the camera is framed at has to be the one the frame is drawn at.
fn assemble<S: Shell + ?Sized>(
    booted: Booted<S, Gpu>,
    options: &Options,
    model: &Model,
) -> Loop<S> {
    let extent = booted.gpu.extent();

    // Frame on load, against the extent the window actually configured at: an
    // aspect guessed from the requested size would frame a model that hangs off
    // the sides of the window it is really in.
    let mut orbit = OrbitCamera::new(model.bounds.center(), 1.0, Projection::default());
    orbit.frame(model.bounds, aspect_of(extent));

    // Read off the document once, here, because nothing in this milestone can
    // change it afterwards — see `Listing::of`.
    let listing = Listing::of(model);

    // Read before the GPU is handed to the loop, and off the renderer rather
    // than from a constant: the default exposure is the renderer's to choose.
    let exposure = booted.gpu.exposure();
    // The same, for the effect set: the device clamps last, so what a run that
    // never reached a frame would have drawn comes back off the renderer.
    let effects = booted.gpu.effects();

    Loop::new(
        booted,
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
            normals: false,
            normals_held: false,
            skeleton: false,
            skeleton_held: false,
            player: model.playable.as_ref().map(crate::anim::Player::new),
            exposure_steps: 0,
            exposure,
            exposure_pending: None,
            exposure_handle: crate::menu::handle_at(exposure),
            watch: Watch::new(&options.model),
            reloads: 0,
            turned: 0.0,
            handed_over: false,
            ticks: 0,
            listing,
            instances: model.render.instances.len(),
            skipped: model.render.skipped.len(),
            joints: model.rig.joints,
            effects,
        },
        options.common.loop_config(),
    )
}

/// Start-up with the waits turned inside out, one poll per frame.
///
/// The state machine, the event pump and the resize-during-start-up race are
/// [`crcbl::engine::PolledBoot`]'s. What is left here is this sample's
/// `Options` and its document — and the document is why this exists at all: a
/// browser cannot block on a device request, and this sample's renderer is
/// built out of the glTF it was asked to show, so the document has to be
/// carried through the wait rather than loaded after it. See
/// [`crcbl::engine::PolledGpu::Context`].
#[derive(Debug)]
pub struct PendingLoop<S: Shell + ?Sized = dyn Shell> {
    boot: crcbl::engine::PolledBoot<S, Gpu>,
    options: Options,
    model: Rc<Model>,
}

impl<S: Shell + ?Sized> PendingLoop<S> {
    /// Creates the window and starts the wait, blocking on neither.
    ///
    /// `model` is the caller's rather than loaded here, because the two callers
    /// get it from different places: natively it is read off a path, and in a
    /// browser it is a document compiled into the module — there is no file to
    /// name. `clock_source` is the caller's for the reason every sample's is:
    /// `std::time::Instant::now` panics on `wasm32-unknown-unknown`, so a page
    /// drives the loop from `performance.now()` instead.
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if the shell refused the window.
    pub fn request(
        mut shell: Box<S>,
        options: &Options,
        clock_source: Clock,
        model: Rc<Model>,
    ) -> Result<Self, ViewerError> {
        let window = open_window(
            shell.as_mut(),
            &clock_source,
            &WindowDesc {
                title: "crcbl — viewer",
                app_id: "sh.kryptic.crcbl.viewer",
                size: requested_window_size(options.common.size),
                mode: options.common.display_mode(),
                ..WindowDesc::default()
            },
        )?;
        Ok(Self {
            boot: crcbl::engine::PolledBoot::request(
                shell,
                window,
                clock_source,
                options.common.gpu(),
                Rc::clone(&model),
            ),
            options: options.clone(),
            model,
        })
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`ViewerError`] if the window went away before it had a size, or if the
    /// device request failed.
    pub fn poll(&mut self) -> Result<Option<Loop<S>>, ViewerError> {
        let Some(booted) = self.boot.poll::<ViewerError>()? else {
            return Ok(None);
        };
        Ok(Some(assemble(booted, &self.options, &self.model)))
    }
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

    /// **How far the clip has carried the skeleton from its rest pose, in
    /// metres** — the one number here that moves because a *document* is
    /// playing rather than because the frame is.
    ///
    /// Zero for a document with no rig, which is honest rather than a
    /// placeholder: a skeleton that is not there is a skeleton standing still.
    /// See [`crate::anim::Player::deviation`] for what it measures and why it
    /// is a property of the pose rather than of the playhead.
    #[must_use]
    pub fn pose(&self) -> f32 {
        self.player
            .as_ref()
            .map_or(0.0, crate::anim::Player::deviation)
    }

    /// **The re-export loop**: notices that the document has been written
    /// again, converts it, and swaps it into the frame.
    ///
    /// `docs/plan/sample/05-viewer.md` V-F4. `dt` is wall-clock seconds — the
    /// frame's, not the tick's — and [`Watch::poll`] spends it against an
    /// interval of its own, so the rate a document is noticed at moves neither
    /// with `--tick-hz` nor with the frame rate. See [`crate::watch`] for what
    /// that interval is and why a poll is enough.
    ///
    /// **Called from [`Viewer::draw`], not from [`Viewer::tick`]**, and the
    /// difference is the whole feature: a paused frame runs no ticks, so an
    /// artist who re-exported with the pause panel up used to see nothing
    /// happen until they closed it.
    ///
    /// **Every failure keeps the frame that is already on screen** and says so
    /// once. A `.glb` caught mid-write, a document too large for the pools it
    /// asks for, an export that converted to nothing — all of them are things
    /// the next save fixes, and a viewer that went blank at the first bad one
    /// would be a worse tool than one that kept drawing. The skips are printed
    /// to stderr as they are at start-up, for `load_and_report`'s reason: the
    /// person who just re-exported is the one who needs to read them.
    fn poll_for_re_export(&mut self, gpu: &mut Gpu, dt: f64) {
        if !self.watch.poll(dt) {
            return;
        }
        let path = self.watch.path();
        let model = match model::load(path) {
            Ok(model) => model,
            Err(error) => {
                crcbl::log::warn!(
                    "viewer: {} was written again and could not be read, so the document already \
                     on screen was kept: {error}",
                    path.display(),
                );
                return;
            }
        };
        let key = model.key.display().to_string();
        // `drew_nothing` is false by construction: `model::load` computes the
        // bounds from the primitives the conversion would draw, so a document
        // with nothing to draw has no bounds and was refused above as a
        // `LoadError::NoGeometry`. Passed anyway rather than hard-coded, so
        // that a `world_bounds` which one day counts something the conversion
        // skips reports it here instead of printing a skip list with no verdict
        // under it.
        for line in skip_report(&key, model.skipped(), model.render.instances.is_empty()) {
            eprintln!("{line}");
        }
        if let Err(error) = self.adopt(gpu, &model) {
            crcbl::log::warn!(
                "viewer: {key} was re-read but the renderer refused it, so the document already \
                 on screen was kept: {error}",
            );
            return;
        }
        crcbl::log::info!(
            "viewer: {key} reloaded — {} instance(s), {} skipped",
            self.instances,
            self.skipped,
        );
    }

    /// Puts `model` on screen in place of the document already there: the
    /// renderer is rebuilt around it, and the bounds, the counts and the
    /// listing follow.
    ///
    /// The tail of [`Viewer::poll_for_re_export`], and the whole of what a
    /// dropped document does with the frame — see
    /// `Viewer::poll_for_dropped_document`. One function because it is one
    /// event: a different document arriving at a viewer that is already
    /// running. Two copies is where the browser would quietly stop rebuilding
    /// the listing.
    ///
    /// **The camera is not touched here**, and the two callers differ on
    /// exactly that. A re-export is the same document again, so the pose an
    /// artist has aimed has to survive it; a document a visitor just chose is
    /// one nobody has aimed at, so the drop path asks for a re-frame itself.
    ///
    /// # Errors
    ///
    /// Whatever [`Gpu::reload`](crate::gpu::Gpu::reload) refused the document
    /// with. Nothing here has run at that point and the frame already on screen
    /// is untouched — `reload` unwinds its own half-built renderer — so a
    /// caller's whole obligation is to say so.
    fn adopt(&mut self, gpu: &mut Gpu, model: &Model) -> Result<(), crcbl::hal::HalError> {
        gpu.reload(
            &model.render.scene,
            &model.render.instances,
            // The new document's rig, not the old one's: the regions the frame
            // deforms are reserved out of the renderer this call builds.
            &model.skinned,
            // The same extent `crate::app::with_shell` opened with, and for the
            // same reason — see `crate::gpu`.
            crate::gpu::grid_extent_for(model),
        )?;
        self.bounds = model.bounds;
        self.instances = model.render.instances.len();
        self.skipped = model.skipped().len();
        self.joints = model.rig.joints;
        // A different document is a different rig, so the playhead starts
        // again; the *overlay's* visibility is the visitor's rather than the
        // document's and is carried across, exactly as the listing's is below.
        self.player = model.playable.as_ref().map(crate::anim::Player::new);
        // Rebuilt rather than edited row by row, because every row is the
        // document's; the panel's own visibility is the one thing that is not,
        // so it is the one thing carried across.
        let showing = self.listing.is_visible();
        self.listing = Listing::of(model);
        self.listing.set_visible(showing);
        self.reloads += 1;
        Ok(())
    }

    /// **The drop target**: opens the document a visitor dropped on the canvas,
    /// if one has landed since the last frame.
    ///
    /// `docs/plan/sample/05-viewer.md` V-F5's browser half, and the counterpart
    /// of [`Viewer::poll_for_re_export`] — the native viewer is pointed at a
    /// path and this one is handed bytes, and past that they are the same
    /// event. [`crate::web`] owns the buffer the page writes into and the
    /// sentence the page reads back; this is the frame that turns one into the
    /// other, called from [`Viewer::draw`] beside the re-export poll for the
    /// same reason: a paused page still draws, and a visitor who dropped a file
    /// with the panel up must not have to close it first.
    ///
    /// **A document that will not parse keeps the frame that is already on
    /// screen**, exactly as a bad re-export does, and says why. It is the whole
    /// point of this application that a file either loads or explains itself,
    /// and a page has no exit code to say it with — so the sentence goes back
    /// to the shim, which puts it on the status bar under the canvas.
    #[cfg(target_arch = "wasm32")]
    fn poll_for_dropped_document(&mut self, gpu: &mut Gpu) {
        let Some((name, bytes)) = crate::web::take_dropped_document() else {
            return;
        };
        let verdict = self.open_dropped(gpu, &name, bytes);
        crate::web::report_dropped_document(verdict);
    }

    /// [`Viewer::poll_for_dropped_document`]'s half that can fail, as the
    /// sentence a visitor reads.
    ///
    /// A `String` rather than a `Result` because both arms end in the same
    /// place — one line of text on the status bar — and the caller has no
    /// decision left to make with a typed error. Every arm is also logged where
    /// it is produced, at the level the outcome deserves, so the browser
    /// console carries the same account the native terminal would.
    #[cfg(target_arch = "wasm32")]
    fn open_dropped(&mut self, gpu: &mut Gpu, name: &std::path::Path, bytes: Vec<u8>) -> String {
        let model = match model::load_bytes(name, bytes) {
            Ok(model) => model,
            Err(error) => {
                // `LoadError` already names the file and what to do about it,
                // which is the sentence the native viewer prints and exits on.
                let verdict = error.to_string();
                crcbl::log::warn!(
                    "viewer: a dropped document could not be read, so the document already on \
                     screen was kept: {verdict}",
                );
                return verdict;
            }
        };
        let key = model.key.display().to_string();
        // Logged rather than printed: `skip_report` writes to stderr at
        // start-up and in the re-export loop, and this target has none. The
        // same lines reach the browser console through the shim's log drain,
        // and the panel behind `I` holds them for whoever wants to look.
        for line in skip_report(&key, model.skipped(), model.render.instances.is_empty()) {
            crcbl::log::info!("{line}");
        }
        if let Err(error) = self.adopt(gpu, &model) {
            let verdict = format!(
                "{key}: the renderer refused it, so the document already on screen was kept: \
                 {error}"
            );
            crcbl::log::warn!("viewer: {verdict}");
            return verdict;
        }
        // Only now, and only here: `adopt` deliberately leaves the camera
        // alone. Nobody has aimed at this document yet, so it is framed the way
        // start-up frames the one the page opened with. Taken by
        // [`Viewer::draw`] a few lines below the call that set it, against this
        // frame's extent.
        self.reframe = true;
        // **And the turntable starts again, for the same reason and only for
        // the same caller.** A visitor who has taken hold of one document has
        // aimed at *that* one; the line above already throws that aim away
        // because this is a different document, and leaving the turntable
        // latched off would hand them a new file sitting still at an angle they
        // chose for the old one. A re-export is the opposite case — the same
        // document again, which is why `adopt` does neither of these and the
        // native path sets neither.
        self.handed_over = false;
        let verdict = format!(
            "{key} opened — {} instance(s), {} skipped",
            self.instances, self.skipped,
        );
        crcbl::log::info!("viewer: {verdict}");
        verdict
    }
}

/// The viewer's half of the frame, and nothing else.
/// How fast the idle turntable carries the camera, in radians a second.
///
/// **Why a viewer turns by itself at all.** A document that sits still is a
/// picture: nothing about it says the thing on screen is being rendered right
/// now, from geometry, by a device that had to be asked for — which is the
/// whole claim a demo makes. `apps/quarry` answers this with a dolly that walks
/// its face, and this is the same answer for a sample whose subject does not
/// move.
///
/// A full turn takes about eighteen seconds at this rate. Slow enough to read a
/// silhouette against, fast enough that a visitor sees it move before deciding
/// the page is broken.
///
/// It stops for good the moment anyone drags, pans or zooms — see
/// [`Viewer::handed_over`].
const TURNTABLE_RATE: f32 = 0.35;

/// How often the `[HUD]` heartbeat is logged, in ticks — a second at the
/// default rate, which is the cadence every other sample's uses.
const HEARTBEAT_TICKS: u64 = 60;

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
    ///
    /// **The re-export loop is deliberately not here**, and it used to be. The
    /// tick was the only clock this application had, so `Watch::poll` was
    /// stepped on `tick_dt` — and that made `ESC` switch the viewer's headline
    /// feature off: [`crcbl::engine::run_ticks`] throws a paused frame's ticks
    /// away, so a document re-exported while the pause panel was up went
    /// unnoticed until the panel was closed. It runs from [`Viewer::draw`] now,
    /// on [`FrameInfo::render_dt`], which a paused frame still advances.
    fn tick(&mut self, _gpu: &mut Gpu, _tick_dt: f64) {
        // Still nothing simulated. What the tick is used for is the heartbeat,
        // and it has to be *here* rather than in `draw`: a paused frame still
        // draws, and a line that went on being logged through a pause would say
        // a stopped demo was running. `crcbl::engine::run_ticks` throws a
        // paused frame's ticks away, so this stops with the simulation and
        // starts again with it — which is exactly what the browser gate reads
        // it for.
        self.ticks += 1;
        self.log_heartbeat();
    }

    /// `F` frames the model again, `I` shows the listing, `W` draws it as lines
    /// and `N` draws its normals — each once per press — and `-`/`=` step the
    /// exposure, once per press *and once per auto-repeat*.
    ///
    /// Framing, the wireframe and the exposure are deferred to [`Viewer::draw`],
    /// which is where this application is handed the aspect and the GPU; the
    /// panel and the normals view need neither at the moment of the press, so
    /// they flip here.
    ///
    /// **The exposure pair deliberately has no held guard.** The loop forwards
    /// auto-repeats as further presses, which the four toggles fold away
    /// because a switch flipped every frame is a strobe — and which the exposure
    /// *wants*, because holding a key to sweep a range is how a continuous value
    /// is driven from a keyboard. The presses are counted rather than flagged,
    /// so a frame that saw several repeats moves by several steps.
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
            NORMALS_KEY => {
                if pressed && !self.normals_held {
                    self.normals = !self.normals;
                }
                self.normals_held = pressed;
            }
            SKELETON_KEY => {
                if pressed && !self.skeleton_held {
                    self.skeleton = !self.skeleton;
                }
                self.skeleton_held = pressed;
            }
            EXPOSURE_UP_KEY => self.exposure_steps += i32::from(pressed),
            EXPOSURE_DOWN_KEY => self.exposure_steps -= i32::from(pressed),
            _ => {}
        }
    }

    /// The orbit drag: the primary button's edges, and the movement of the
    /// pointer while any drag is running.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        self.controls.pointer(pointer, self.extent, &mut self.orbit);
        // A drag, not a hover: a pointer crossing the canvas has not taken hold
        // of anything, and stopping the turntable on it would stop it the moment
        // the mouse arrived.
        //
        // **A drag that moved**, and the motion is load-bearing rather than
        // decoration. A press with no movement is a click, and the commonest
        // click on this canvas is the one that hands it the keyboard — a
        // gesture about focus, not about the camera. Latching on the press
        // stopped the turntable for a visitor who had only clicked to type.
        //
        // It also produced a failure that took two occurrences to read.
        // `PointerUpdate`'s `pressed` and `released` are per-frame edges, so a
        // click landing inside one frame arrives as both at once and
        // `Controls::pointer` sets the drag and clears it again — while the
        // same click split across two frames arrives as a press alone, and
        // latched. The browser gate's focus click is that click: it coalesces
        // here and on the Linux and Windows runners, and on macOS it split
        // about one run in three, freezing the turntable a second into the run
        // at 20.6° and 20.3° in the two failures on record. See
        // `docs/backlog.md`. Nothing had stalled; the rule was wrong.
        //
        // A held button reaches here on the frames it is held for, so a press
        // that becomes a drag latches on the first frame the hand moves.
        self.handed_over |= self.controls.is_dragging() && pointer.motion.is_some();
    }

    /// The pan drag. Both non-primary buttons start one — see
    /// [`crate::controls`].
    fn button_event(&mut self, button: PointerButton, pressed: bool) {
        self.controls.button(button, pressed);
    }

    fn wheel_event(&mut self, delta: ScrollDelta) {
        Controls::wheel(delta, &mut self.orbit);
        self.handed_over = true;
    }

    /// The viewer's panel carries no id of its own, so the loop never asks.
    fn menu_action(_id: crcbl::ui::WidgetId) -> Option<core::convert::Infallible> {
        None
    }

    fn apply(&mut self, action: core::convert::Infallible) {
        match action {}
    }

    /// **Which panel this frame shows, and the exposure slider reconciled.**
    ///
    /// The slider is two-way: `-` and `=` move the exposure and the handle has
    /// to follow, and a drag moves the handle and the exposure has to follow.
    /// Both directions run here, in that order, and which one wins is decided
    /// by a comparison that cannot be fooled — `exposure_handle` is the number
    /// the widget itself last held, so a difference is the pointer and nothing
    /// else.
    ///
    /// The drag is applied one frame later, in [`Viewer::draw`], because a
    /// hosted game is handed the GPU only there. `set_slider` refuses a write
    /// while the handle is held, so the mirror below cannot pin a drag in
    /// progress under the cursor.
    ///
    /// [`MenuSet::get_mut`](crcbl::ui::MenuSet::get_mut) rather than
    /// `current_mut`, because the loop has not been told which menu this frame
    /// shows yet — that is what this call returns.
    fn menu_kind(&mut self, menus: &mut Menus, paused: bool) -> MenuKind {
        if let Some(menu) = menus.get_mut(MenuKind::Menu) {
            let handle = menu.slider(crate::menu::EXPOSURE_ID);
            match handle {
                Some(position) if position != self.exposure_handle => {
                    self.exposure_pending = Some(crate::menu::exposure_at(position));
                    self.exposure_handle = position;
                }
                _ => {
                    menu.set_slider(
                        crate::menu::EXPOSURE_ID,
                        crate::menu::handle_at(self.exposure),
                    );
                    if let Some(position) = menu.slider(crate::menu::EXPOSURE_ID) {
                        self.exposure_handle = position;
                    }
                }
            }
            // Every frame and unconditionally, so the number beside the groove
            // is the exposure the frame behind the panel was drawn with — the
            // renderer's answer, clamp and all, exactly as the listing row is.
            menu.set_item_hint(
                crate::menu::EXPOSURE_ID,
                crate::listing::exposure_value(self.exposure),
            );
        }
        MenuKind::of(paused)
    }

    /// Hands the camera to the GPU, re-frames first if `F` asked, and appends
    /// the listing panel if `I` did.
    ///
    /// The panel is the only thing this sample puts in `draw_list` — there is
    /// still no HUD. It goes in first, so the debug overlay the loop appends
    /// after this returns is the one on top where the two overlap. They are
    /// pinned to opposite corners, so today they do not.
    ///
    /// **This is also where the re-export loop runs** — see
    /// `Viewer::poll_for_re_export` for why it is not in the tick — and, on
    /// `wasm32`, where a document dropped on the canvas is opened. Both put a
    /// new document on screen through `Viewer::adopt`, and both are here rather
    /// than in the tick because a paused frame still draws.
    fn draw(&mut self, gpu: &mut Gpu, draw_list: &mut DrawList, frame: FrameInfo) {
        // Before anything else in the frame, which is the position the tick
        // used to give it: the tick ran immediately before this, so a document
        // that lands is framed and listed by the rest of this call rather than
        // by the next one.
        self.poll_for_re_export(gpu, frame.render_dt.as_secs_f64());
        // The browser's half of the same idea, and in the same place for the
        // same reason — see `Viewer::poll_for_dropped_document`. Native builds
        // have no page to be dropped on, so there is nothing here for them
        // rather than a poll that can never fire.
        #[cfg(target_arch = "wasm32")]
        self.poll_for_dropped_document(gpu);
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
        // The answer again, not the request — see [`Viewer::exposure`]. One
        // multiply per net step, so a frame that saw three repeats moves three
        // times as far, and the renderer's clamp decides where it stops.
        let steps = std::mem::take(&mut self.exposure_steps);
        if steps != 0 {
            self.exposure = gpu.scale_exposure(exposure_step().powi(steps));
        }
        // After the keys, so a frame that saw both ends with the value the
        // player was last **looking at** — the handle is on screen and the keys
        // are not.
        if let Some(exposure) = std::mem::take(&mut self.exposure_pending) {
            self.exposure = gpu.set_exposure(exposure);
        }
        // Every frame rather than on the key's edge, because there is no answer
        // to keep: the normals view builds nothing and cannot be refused, so the
        // field is the state and pushing it is idempotent.
        gpu.set_normals_view(self.normals);
        // The turntable, after every key and before the camera is handed over,
        // so a frame that re-framed still leaves on the new pose rather than one
        // step behind it. `render_dt` rather than the tick's: this is a property
        // of the picture, and a paused frame still draws one.
        if !self.handed_over {
            let step = TURNTABLE_RATE * frame.render_dt.as_secs_f32();
            self.orbit.orbit(step, 0.0);
            self.turned += step;
        }
        // The clip, stepped on `render_dt` for the turntable's reason above:
        // nothing in this sample is simulated, so playback is a property of the
        // picture and a paused frame still draws one. The loop is
        // `crate::anim::Player::advance`'s modulo, which is where the crate
        // leaves it.
        if let Some(player) = &mut self.player {
            player.advance(frame.render_dt.as_secs_f32());
            // **The same composition the overlay and the `pose` row read**, so
            // the geometry the GPU deforms and the skeleton drawn over it cannot
            // be a frame apart. The frame this feeds is recorded after this
            // call returns; a document with nothing skinned takes it and ignores
            // it, which costs one copy of a palette nobody uploads.
            gpu.set_palette(player.palette());
        }
        gpu.set_camera(self.orbit.camera());
        // Re-read rather than kept: the device clamps last, and a reload builds
        // a second renderer — so what the summary reports comes back off
        // whichever one this frame is being drawn with.
        self.effects = gpu.effects();
        // Every frame rather than only on a step, so the row cannot disagree
        // with the frame after anything else moves the exposure.
        self.listing.set_exposure(self.exposure);
        // **The skeleton first of the three things this frame draws in screen
        // space**: it belongs over the model it annotates, and the listing and
        // the engine's overlay belong over both. Nothing is drawn for a
        // document with no rig, and nothing at all until `B` asks — see
        // [`SKELETON_KEY`].
        if self.skeleton
            && let Some(player) = &self.player
        {
            player.draw(
                draw_list,
                &self.orbit.camera(),
                self.extent,
                // The same figure the grid is sized from, for the same reason:
                // the overlay has to be legible on a chair and on a cathedral,
                // so its one length is a fraction of the document's own size.
                self.bounds.half_extent().max_element(),
            );
        }
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
            effects: self.effects,
        }
    }

    fn log_summary(summary: &Summary) {
        crcbl::log::info!(
            "viewer: {} frames, {} events on the {} shell at {}x{} ({} instances, {} skipped, \
             effects {}, {:?})",
            summary.frames,
            summary.events,
            summary.backend,
            summary.extent.0,
            summary.extent.1,
            summary.instances,
            summary.skipped,
            summary.effects.row(),
            summary.exit,
        );
    }
}

/// The viewer's own rows on the debug panel.
///
/// The numbers a person looking at an unfamiliar document wants and cannot get
/// any other way: how much of it was placed, how much of it was refused, how
/// big a skeleton it brought, and how far away the camera has ended up — which
/// is what says "nothing is on screen because you zoomed past it" rather than
/// "nothing is on screen because the file is empty".
///
/// `joints` is the rig, reduced to the one thing that fits a row, and `pose` is
/// what the clip is doing to it — see [`Viewer::pose`]. The clip *names* are on
/// the listing behind [`LISTING_KEY`], because a name out of someone else's
/// file is not something a `name: value` row can carry safely.
///
/// The last two rows are the debug views, and both report the frame that was
/// **drawn**: the wireframe's field holds what the device answered rather than
/// what [`WIREFRAME_KEY`] asked for, so a press that could not be honoured leaves
/// it saying `off` instead of quietly disagreeing with the picture, and the
/// normals view cannot be refused at all so its field is the same thing by
/// construction.
///
/// The normals row also names the **space**, because `n * 0.5 + 0.5` is a
/// convention two engines can hold in world or in view space and a picture does
/// not say which: a face keeping its colour as the camera orbits is what makes
/// `world` the answer to "is this face inverted", and a reader who does not know
/// which they are looking at cannot use either.
impl Viewer {
    /// The `[HUD]` line, on the cadence every other sample's heartbeat uses.
    ///
    /// **This sample has no simulation to report**, so what it names is the
    /// document: how many instances of it reached the renderer, how much of it
    /// the conversion could not bring in, how many joints its skins declare,
    /// how far its clip has bent them, and where the camera is. A CI log has no debug panel to read and neither has
    /// a browser gate.
    ///
    /// **Counts only, and `joints` is why that is written down.** Every value
    /// on this line is a number or a fixed word, because
    /// `web/tools/browser-e2e.mjs` and this module's tests read the line as
    /// `name: value` pairs separated by runs of spaces. A clip name would be
    /// arbitrary text out of a document nobody here wrote — one with a space or
    /// a colon in it takes that parse apart — so the names stay on the listing
    /// panel and only the joint count comes here.
    ///
    /// **`pose` is the document playing rather than the frame drawing.**
    /// `web/tools/browser-e2e.mjs` reads it to know that the clip in the
    /// document is being sampled and the skeleton composed from it: it is
    /// derived from the palette and from nothing else, so a playhead that
    /// advanced over a pose nobody ever wrote leaves it at zero. It is a
    /// distance in metres, which is a bare number like every other value on
    /// this line — the clip's *name* is on the listing panel, where arbitrary
    /// text out of someone else's file cannot take this parse apart.
    ///
    /// **`turn` is here for that gate specifically.**
    /// `web/tools/browser-e2e.mjs` proves a page is running rather than merely
    /// presenting by watching one number advance, and it has to be a number
    /// nothing on the JS side can move. Every other row here is a property of
    /// the document and never changes; the turntable's angle is the only thing
    /// this sample has that moves on its own. See [`TURNTABLE_RATE`], and note
    /// that it stops for good once a visitor takes hold — which is correct for
    /// the tool and is why the gate reads it before it touches the canvas.
    fn log_heartbeat(&self) {
        if !self.ticks.is_multiple_of(HEARTBEAT_TICKS) {
            return;
        }
        crcbl::log::info!(
            "[HUD] tick: {}  instances: {}  skipped: {}  joints: {}  pose: {:.2}  dist: {:.2}  \
             turn: {:.1}  held: {}  wireframe: {}  normals: {}",
            self.ticks,
            self.instances,
            self.skipped,
            self.joints,
            self.pose(),
            self.orbit.distance(),
            self.turned.to_degrees() % 360.0,
            if self.handed_over { "on" } else { "off" },
            if self.wireframe { "on" } else { "off" },
            if self.normals { "world" } else { "off" },
        );
    }
}

impl DebugModule for Viewer {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("viewer");
        out.row("instances", format_args!("{}", self.instances));
        out.row("skipped", format_args!("{}", self.skipped));
        out.row("joints", format_args!("{}", self.joints));
        out.row("pose", format_args!("{:.2}", self.pose()));
        out.row("dist", format_args!("{:.2}", self.orbit.distance()));
        // Wrapped to one turn so the row stays readable; it is the value the
        // browser gate watches for a frame advancing under its own steam.
        out.row(
            "turn",
            format_args!("{:.1}", self.turned.to_degrees() % 360.0),
        );
        // **The only thing that stops the turntable**, and the reading that tells
        // a camera someone took hold of from a page whose loop died — see the
        // viewer's intermittent macOS stall in `docs/backlog.md`, where the
        // angle sat frozen while the picture went on changing and nothing said
        // which of the two it was.
        out.row(
            "held",
            format_args!("{}", if self.handed_over { "on" } else { "off" }),
        );
        out.row("reloads", format_args!("{}", self.reloads));
        out.row(
            "wireframe",
            format_args!("{}", if self.wireframe { "on" } else { "off" }),
        );
        out.row(
            "normals",
            format_args!("{}", if self.normals { "world" } else { "off" }),
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
    use crcbl::math::Vec2;
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

    /// **The idle turntable turns, and stops for good when someone takes hold.**
    ///
    /// Both halves matter and the second one matters most. The camera moving on
    /// its own is what the browser gate watches to know a frame is advancing —
    /// see [`TURNTABLE_RATE`] — but it is also a camera that moves without
    /// being asked, and
    /// `every_gesture_reaches_the_camera_through_the_hosted_loop` asserts a drag
    /// moved the eye. If the turntable kept running through a drag, that
    /// assertion would pass on a build where the drag reached nothing at all:
    /// the turntable would have moved the eye for it.
    ///
    /// **Each way of taking hold is asserted on its own engine**, because they
    /// latch at different call sites and one of them covering for another is
    /// exactly how a line here stops being checked.
    ///
    /// **And a click is not one of them**, which is the third case here and the
    /// one with a failure behind it: a press that never moves is a gesture
    /// about focus, and stopping the turntable on it stopped it for a visitor
    /// who had clicked to type. See `pointer_event`, and the browser gate's
    /// macOS failures in `docs/backlog.md`.
    #[test]
    fn the_turntable_turns_until_someone_takes_hold_and_then_never_again() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 64);

        // It turns on its own, with nothing driving it.
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let idle = engine.game().camera();
        engine.frame().expect("a frame");
        let later = engine.game().camera();
        assert_ne!(
            later.eye, idle.eye,
            "the turntable never moved the camera, so nothing in this demo advances \
             under its own steam"
        );
        assert_eq!(
            later.target, idle.target,
            "a turntable orbits the document; it does not wander off it"
        );

        // A drag: the press, and then the frame the hand moves on, which is the
        // one that latches.
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Pressed,
                None,
            )
            .expect("the window is live");
        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .move_pointer(window, PhysicalPoint::new(80.0, 0.0), (80.0, 0.0))
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_still(&mut engine, "a drag");

        // **And the click that is not a drag leaves it running.** The control
        // for the case above: a build that latched on the press would pass that
        // one and fail this, and it is the build the macOS runner was failing
        // on. The press and the release land in the same frame here, which is
        // what a click that lands inside one frame does — and the split click
        // below is the same gesture arriving over two.
        for gesture in ["a click inside one frame", "a click across two"] {
            let mut engine = scripted(&options);
            engine.frame().expect("a frame");
            let window = engine.window();
            engine
                .shell_mut()
                .button(
                    window,
                    PointerButton::Left,
                    crcbl::core::input::ButtonState::Pressed,
                    None,
                )
                .expect("the window is live");
            if gesture == "a click across two" {
                engine.frame().expect("a frame");
            }
            engine
                .shell_mut()
                .button(
                    window,
                    PointerButton::Left,
                    crcbl::core::input::ButtonState::Released,
                    None,
                )
                .expect("the window is live");
            engine.frame().expect("a frame");
            let before = engine.game().camera();
            engine.frame().expect("a frame");
            assert_ne!(
                engine.game().camera().eye,
                before.eye,
                "{gesture} stopped the turntable — a visitor who clicked the canvas to \
                 type has not taken hold of the camera",
            );
        }

        // The wheel, which latches at its own call site and nowhere else.
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        let window = engine.window();
        engine
            .shell_mut()
            .scroll(window, ScrollDelta::Lines { x: 0.0, y: -1.0 }, None)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_still(&mut engine, "a wheel");
    }

    /// Fails unless the camera is now still across two further frames.
    ///
    /// Two frames rather than one: the eye is compared against a pose taken
    /// *after* a frame the input was handled in, so a turntable that ran one
    /// last time before latching is not mistaken for one that stopped.
    fn assert_still(engine: &mut Loop<HeadlessShell>, took_hold: &str) {
        let held = engine.game().camera();
        engine.frame().expect("a frame");
        assert_eq!(
            engine.game().camera().eye,
            held.eye,
            "the turntable kept running after {took_hold} — a tool that drifts out from \
             under the pose someone aimed it at"
        );
    }

    /// **The panel says whether someone took hold of the camera.**
    ///
    /// The row exists for a failure that has already happened and could not be
    /// read: the browser gate's macOS run reported the turntable's angle frozen
    /// while the picture went on changing, and nothing in the run said whether
    /// the camera had been handed over or the loop had died. Those want
    /// opposite investigations — see `docs/backlog.md`.
    ///
    /// So the row is asserted against the thing it claims to report, in one
    /// run: it reads `off` while the turntable is carrying the camera, and `on`
    /// once a gesture has stopped it for good.
    #[test]
    fn the_panel_says_whether_the_camera_was_taken_hold_of() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 24);
        options.common.debug_overlay = Some(true);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        let idle = engine.game().camera();
        engine.frame().expect("a frame");
        assert_ne!(
            engine.game().camera().eye,
            idle.eye,
            "the turntable was not running, so `off` below would be the right \
             reading for the wrong reason",
        );
        assert_eq!(
            row_value(&ui_text(&engine), "held"),
            "off",
            "the panel said the camera was held while the turntable was moving it",
        );

        engine
            .shell_mut()
            .scroll(window, ScrollDelta::Lines { x: 0.0, y: -1.0 }, None)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "held"),
            "on",
            "a gesture stopped the turntable and the panel went on saying nobody \
             had touched it",
        );
        assert_still(&mut engine, "a wheel");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
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

    /// Every `Line` command the frame handed to the UI pass, as its two ends.
    ///
    /// The skeleton overlay is the only thing this application strokes lines
    /// with, so with the engine's debug overlay switched off this is the
    /// overlay and nothing else.
    fn ui_lines(engine: &Loop<HeadlessShell>) -> Vec<(Vec2, Vec2)> {
        use crcbl::ui::draw_list::DrawCommand;
        engine
            .gpu()
            .draw_list()
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Line { from, to, .. } => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }

    /// The text the **listing panel** drew, with the debug overlay's own rows
    /// left out.
    ///
    /// The two panels share a draw list and a label namespace, and both carry an
    /// `instances` row: the listing's is the document's count and the overlay's
    /// is this viewer's. A test with both panels up and no scope reads whichever
    /// the panel happened to draw first — which is what
    /// [`row_value`]'s duplicate check refuses.
    ///
    /// The overlay's first heading is the split, and it is the loop's own
    /// section rather than this sample's, so it is there whenever the overlay
    /// is. With the overlay off the whole list is the listing's, which is what
    /// the fallback says.
    fn listing_text(engine: &Loop<HeadlessShell>) -> Vec<String> {
        let drawn = ui_text(engine);
        let end = drawn
            .iter()
            .position(|text| text == "frame")
            .unwrap_or(drawn.len());
        drawn[..end].to_vec()
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
        let mut matches = drawn
            .iter()
            .enumerate()
            .filter(|(_, text)| *text == label)
            .map(|(at, _)| at);
        let at = matches
            .next()
            .unwrap_or_else(|| panic!("no {label} row in {drawn:?}"));
        // Row labels share one namespace across every section of the panel, and
        // two have collided already — `crcbl-render`'s frame timings draw a
        // `pending` row, and this sample's first draft named one of its own the
        // same. A reader tells them apart by the heading above them; a search
        // through the flat draw list cannot, and would read whichever came
        // first for ever after.
        assert!(
            matches.next().is_none(),
            "more than one {label} row in {drawn:?}, so this reads whichever the panel \
             happened to draw first"
        );
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

    /// **The `joints` row is the document's rig, and it is a count and nothing
    /// else.**
    ///
    /// Over the browser demo's own document, which is the one document this
    /// application ships with a rig in it — see [`crate::demo_model`] — and the
    /// one `web/tools/browser-e2e.mjs` waits on. The gate reads the same number
    /// off the `[HUD]` line that this reads off the panel, so a summary that
    /// stopped reaching [`Viewer`] fails here rather than as a browser run that
    /// times out.
    ///
    /// The listing panel is left closed. It draws a `joints` row of its own and
    /// [`row_value`] refuses a label that appears twice, which is the same
    /// collision `instances` already has.
    #[test]
    fn the_debug_panel_reports_the_documents_joint_count() {
        let (_dir, mut options) = model_at(&crate::demo_model::demo_glb(), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        let drawn = ui_text(&engine);
        assert_eq!(
            row_value(&drawn, "joints"),
            "2",
            "the demo document's skin binds two joints: {drawn:?}",
        );
        assert_eq!(
            row_value(&drawn, "instances"),
            "3",
            "and the joints placed nothing, so the instance count is unmoved: {drawn:?}",
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

    /// **A held `N` toggles the normals view once, not once a frame** — and the
    /// panel names the space it is drawn in.
    ///
    /// `a_held_wireframe_key_toggles_the_view_once_not_once_a_frame`'s claim for
    /// the third binding that needs an edge. It cannot be refused, so unlike the
    /// wireframe there is nothing here about what the device answered — what this
    /// says instead is that the row reads `world` and not merely `on`, because
    /// `n * 0.5 + 0.5` is a convention two engines can hold in either space and a
    /// reader who does not know which is looking at a picture they cannot use.
    ///
    /// `gpu::tests::the_normals_view_paints_each_face_the_encoding_of_its_world_normal`
    /// is what says the frame really is that; this is the routing under it.
    #[test]
    fn a_held_normals_key_toggles_the_view_once_not_once_a_frame() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 24);
        options.common.debug_overlay = Some(true);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "normals"),
            "off",
            "the view is off until it is asked for",
        );

        engine
            .shell_mut()
            .key_press(window, NORMALS_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "normals"),
            "world",
            "the press never reached the renderer, so the repeats below prove nothing",
        );

        for _ in 0..2 {
            engine
                .shell_mut()
                .key_repeat(window, NORMALS_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
            assert_eq!(
                row_value(&ui_text(&engine), "normals"),
                "world",
                "a repeat switched the view back off",
            );
        }

        engine
            .shell_mut()
            .key_release(window, NORMALS_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "normals"),
            "world",
            "letting go switched it off",
        );

        // And the next real press still works, which is what says the guard
        // released rather than latched.
        tap(&mut engine, window, NORMALS_KEY);
        assert_eq!(row_value(&ui_text(&engine), "normals"), "off");
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **`-` and `=` step the exposure through the loop, and the listing panel
    /// reports the value the renderer answered with.**
    ///
    /// Three claims a unit test of the mapping could not make: that the keys are
    /// routed at all, that a *held* key keeps stepping where the three toggles
    /// deliberately stop, and that the panel's row follows.
    ///
    /// The row is the observable rather than the field, for the wireframe
    /// toggle's reason: the row is what a user reads, and the field behind it
    /// holds what the renderer clamped to rather than the product of the presses.
    ///
    /// Three presses is exactly one stop, because the step is a third of one —
    /// so `1.00x` becoming `2.00x` also says the step is the size it claims.
    #[test]
    fn the_exposure_keys_step_the_renderer_and_the_panel_reports_it() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 128);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");

        // The row lives on the listing panel, which is off until it is asked
        // for.
        tap(&mut engine, window, LISTING_KEY);
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "1.00x",
            "the default is the renderer's, and it is what the panel shows",
        );

        for _ in 0..3 {
            tap(&mut engine, window, EXPOSURE_UP_KEY);
        }
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "2.00x",
            "three presses of {EXPOSURE_UP_KEY:?} is a stop, and a stop is a doubling",
        );

        // **The auto-repeats are further steps, not swallowed edges.** A press
        // and two repeats, all while the key is down, is three steps down — back
        // where it started.
        engine
            .shell_mut()
            .key_press(window, EXPOSURE_DOWN_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        for _ in 0..2 {
            engine
                .shell_mut()
                .key_repeat(window, EXPOSURE_DOWN_KEY)
                .expect("the window is live");
            engine.frame().expect("a frame");
        }
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "1.00x",
            "the repeats were folded away, so a held key does not sweep the range",
        );
        engine
            .shell_mut()
            .key_release(window, EXPOSURE_DOWN_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "1.00x",
            "letting go stepped once more",
        );

        // **The range has an end, and it is one a user can come back from.**
        // Five stops up is fifteen presses; twenty of them therefore have to
        // stop at the top rather than run past it.
        for _ in 0..20 {
            tap(&mut engine, window, EXPOSURE_UP_KEY);
        }
        assert!(
            (engine.gpu().exposure() - crcbl::render::EXPOSURE_MAX).abs() < 1e-4,
            "twenty presses reached {} rather than the clamp at {}",
            engine.gpu().exposure(),
            crcbl::render::EXPOSURE_MAX,
        );
        // And fifteen back down is the default again — which it could not be if
        // the presses past the top had wound a value up behind the clamp.
        for _ in 0..15 {
            tap(&mut engine, window, EXPOSURE_DOWN_KEY);
        }
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "1.00x",
            "the picture could not be brought back from the top of the range",
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The exposure slider's groove, read off the panel the loop is showing.
    ///
    /// Off the live menu rather than a rebuilt one: the value beside the groove
    /// changes width as the exposure moves, and the panel is sized by its widest
    /// row — so a rebuilt copy with a stale caption would hand back a rectangle
    /// the frame never drew.
    fn slider_groove(engine: &Loop<HeadlessShell>) -> (Vec2, Vec2) {
        let menu = engine.menus().current().expect("the panel is showing");
        let layout = menu.layout(engine.gpu().extent(), engine.gpu().atlas());
        let at = menu
            .items()
            .iter()
            .position(|item| item.id == crate::menu::EXPOSURE_ID)
            .expect("the panel has an exposure row");
        layout.items()[at].track.expect("the row is a slider")
    }

    /// Presses the primary button at `at`, in framebuffer pixels.
    fn press_at(engine: &mut Loop<HeadlessShell>, window: crcbl::shell::WindowId, at: Vec2) {
        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Pressed,
                Some(PhysicalPoint::new(f64::from(at.x), f64::from(at.y))),
            )
            .expect("the window is live");
    }

    /// **Dragging the panel's handle moves the exposure, and the keys move the
    /// handle back.**
    ///
    /// The claim a unit test of the mapping cannot make. `crcbl-ui` proves the
    /// handle follows the pointer and `crate::menu` proves the two conversions
    /// are inverses; what is left — and what nothing else covers — is that the
    /// loop routes a press on that row to the slider at all, that
    /// `Viewer::menu_kind` turns the handle into a renderer call, and that the
    /// mirror runs the other way rather than the panel and the frame drifting
    /// apart.
    ///
    /// The listing row is the observable for the same reason the key test uses
    /// it: it reports what the renderer answered, after the clamp.
    #[test]
    fn dragging_the_panel_handle_sets_the_exposure_and_the_keys_move_it_back() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 128);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        tap(&mut engine, window, LISTING_KEY);
        assert_eq!(row_value(&ui_text(&engine), "exposure"), "1.00x");

        // The panel has to be on screen *before* the press: a press that began
        // before the panel did is not the panel's, which is the loop's rule.
        tap(&mut engine, window, PAUSE_KEY);
        assert!(engine.menus().is_showing(), "ESC did not open the panel");

        // The right-hand end of the groove is the top of the range.
        let groove = slider_groove(&engine);
        let middle_y = (groove.0.y + groove.1.y) * 0.5;
        press_at(&mut engine, window, Vec2::new(groove.1.x, middle_y));
        // One frame routes the press and reads the handle; the next hands the
        // value to the renderer — `menu_kind` runs after `draw`.
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert!(
            (engine.gpu().exposure() - crcbl::render::EXPOSURE_MAX).abs() < 1e-3,
            "a drag to the end of the groove reached {} rather than {}",
            engine.gpu().exposure(),
            crcbl::render::EXPOSURE_MAX,
        );
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "32.00x",
            "the renderer moved and the listing panel did not follow",
        );

        // The other end, from a fresh press, so this is a drag and not a
        // one-way latch.
        let groove = slider_groove(&engine);
        press_at(&mut engine, window, Vec2::new(groove.0.x, middle_y));
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "0.03x",
            "a drag to the start of the groove did not reach the bottom of the range",
        );

        // **And the middle of the groove is one, which is what says the handle
        // is even in stops.** Both ends agree whatever the curve is, so a
        // linear mapping would pass everything above and land here at sixteen.
        let groove = slider_groove(&engine);
        press_at(
            &mut engine,
            window,
            Vec2::new((groove.0.x + groove.1.x) * 0.5, middle_y),
        );
        engine.frame().expect("a frame");
        engine.frame().expect("a frame");
        assert_eq!(
            row_value(&ui_text(&engine), "exposure"),
            "1.00x",
            "the middle of the groove is not the middle of the range in stops",
        );

        engine
            .shell_mut()
            .button(
                window,
                PointerButton::Left,
                crcbl::core::input::ButtonState::Released,
                None,
            )
            .expect("the window is live");
        engine.frame().expect("a frame");

        // **And the mirror, which is the direction a drag cannot prove.** Three
        // presses of the key is one stop, and one stop is a tenth of a groove
        // ten stops wide.
        let before = engine
            .menus()
            .current()
            .expect("the panel is showing")
            .slider(crate::menu::EXPOSURE_ID)
            .expect("the row is a slider");
        for _ in 0..3 {
            tap(&mut engine, window, EXPOSURE_UP_KEY);
        }
        let after = engine
            .menus()
            .current()
            .expect("the panel is showing")
            .slider(crate::menu::EXPOSURE_ID)
            .expect("the row is a slider");
        assert!(
            (after - before - 0.1).abs() <= 1e-3,
            "a stop moved the handle from {before} to {after}",
        );

        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A re-export is picked up, and the frame becomes the new document.**
    ///
    /// V-F4 end to end: the file on disk is written again, the watch settles it,
    /// the conversion runs, the renderer swaps its scene, and the viewer's own
    /// state — the bounds the camera frames against, the listing panel's rows —
    /// is the new file's.
    ///
    /// The bounds are the observable rather than the reload counter alone: a
    /// counter says the path ran and nothing about what it produced, and this
    /// fixture's two documents differ only in where the node puts the quad, so
    /// a reload that swapped the renderer and forgot everything else would move
    /// the counter and leave the camera framing the old corner of the world.
    #[test]
    fn a_re_export_is_picked_up_and_replaces_the_document() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 4096);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");
        assert_eq!(engine.game().reloads, 0, "nothing was written yet");
        assert_eq!(engine.game().bounds.center(), Vec3::ZERO);
        assert_eq!(engine.game().instances, 1);
        let window = engine.window();
        tap(&mut engine, window, LISTING_KEY);
        assert_eq!(row_value(&listing_text(&engine), "instances"), "1");

        // A second quad three metres along: a different instance count, a
        // different centre, and a different length on disk — so every number
        // below can move, and the watch can tell the two files apart on a
        // filesystem with a coarse modification time.
        std::fs::write(&options.model, fixture::two_quads_glb()).expect("the re-export");

        // `Watch` looks at the file four times a second and needs two agreeing
        // looks, so half a second of ticks. Bounded rather than open, so a
        // reload that never happens fails here instead of hanging.
        let ticks_per_second = f64::from(crate::args::DEFAULT_TICK_HZ);
        let frames = (ticks_per_second * 2.0).ceil() as usize;
        for _ in 0..frames {
            engine.frame().expect("a frame");
            if engine.game().reloads > 0 {
                break;
            }
        }
        assert_eq!(
            engine.game().reloads,
            1,
            "two seconds of ticks did not pick the re-export up",
        );
        assert_eq!(
            engine.game().instances,
            2,
            "the scene was swapped and the instance count was not",
        );
        assert_eq!(
            engine.game().bounds.center(),
            Vec3::new(1.5, 0.0, 0.0),
            "the scene was swapped but the bounds are still the old document's",
        );
        assert_eq!(
            row_value(&listing_text(&engine), "instances"),
            "2",
            "the listing panel is still describing the document that was replaced",
        );
        assert!(
            engine.game().listing.is_visible(),
            "the reload closed a panel the user had opened",
        );

        // And it settles: the same file, unchanged, is not read again.
        for _ in 0..frames {
            engine.frame().expect("a frame");
        }
        assert_eq!(
            engine.game().reloads,
            1,
            "a document nobody touched was reloaded again",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A re-export lands while the pause panel is up**, which is the whole of
    /// what V-F4 is worth to an artist: they press `ESC`, tab to Blender, save,
    /// tab back, and the new document is already on screen.
    ///
    /// This used not to work. `Watch::poll` was stepped from `Viewer::tick` on
    /// the simulation clock, and `crcbl::engine::run_ticks` throws a paused
    /// frame's ticks away — so `FrameInfo::ticks` was zero, the watch's timer
    /// never advanced, and the file was not so much as `stat`ed until the panel
    /// closed. The poll runs from `Viewer::draw` on `FrameInfo::render_dt` now,
    /// which the loop updates above its pause check.
    ///
    /// **The assertion that matters is that the run is still paused when the
    /// document has changed.** A test that unpaused first would pass against
    /// the broken version, which is why the pause is re-checked after the
    /// frames rather than only before them.
    #[test]
    fn a_re_export_while_paused_is_picked_up_without_unpausing() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 4096);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        assert_eq!(engine.game().reloads, 0, "nothing was written yet");
        assert_eq!(engine.game().instances, 1);

        tap(&mut engine, window, PAUSE_KEY);
        assert!(engine.is_paused(), "ESC did not stop the simulation");
        assert!(engine.menus().is_showing(), "ESC did not open the panel");

        // The same pair of documents the unpaused test uses, and for the same
        // reason: a different instance count, a different centre and a
        // different length on disk.
        std::fs::write(&options.model, fixture::two_quads_glb()).expect("the re-export");

        // `Watch` looks at the file four times a second and needs two agreeing
        // looks, so half a second of frames. A headless frame is
        // `crcbl::engine::HEADLESS_FRAME_STEP` of wall clock, and this allows
        // two seconds of them — bounded rather than open, so a reload that
        // never happens fails here instead of hanging.
        let frames = (2.0 / crcbl::engine::HEADLESS_FRAME_STEP.as_secs_f64()).ceil() as usize;
        let ticks_before = engine.ticks();
        for _ in 0..frames {
            engine.frame().expect("a frame");
            if engine.game().reloads > 0 {
                break;
            }
        }
        assert_eq!(
            engine.ticks(),
            ticks_before,
            "a tick ran while the panel was up, so this proves nothing about the wall clock",
        );
        assert!(
            engine.is_paused(),
            "the re-export unpaused the run, so the reload says nothing about a paused frame",
        );
        assert_eq!(
            engine.game().reloads,
            1,
            "two seconds of paused frames did not pick the re-export up",
        );
        assert_eq!(
            engine.game().instances,
            2,
            "the scene was swapped and the instance count was not",
        );
        assert_eq!(
            engine.game().bounds.center(),
            Vec3::new(1.5, 0.0, 0.0),
            "the scene was swapped but the bounds are still the old document's",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A document that cannot be read leaves the frame alone.**
    ///
    /// The case the artist loop actually produces: a `.glb` caught mid-write, or
    /// an export that failed. A viewer that blanked at the first bad read would
    /// be a worse tool than one that kept drawing the last good document, so the
    /// reload is refused and everything stays where it was.
    #[test]
    fn a_broken_re_export_keeps_the_document_already_on_screen() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 4096);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");

        std::fs::write(
            &options.model,
            b"not a glb at all, and not the right length either",
        )
        .expect("the bad write");
        let frames = (f64::from(crate::args::DEFAULT_TICK_HZ) * 2.0).ceil() as usize;
        for _ in 0..frames {
            engine.frame().expect("a frame");
        }
        assert_eq!(
            engine.game().reloads,
            0,
            "a document that will not parse was loaded"
        );
        assert_eq!(
            engine.game().bounds.center(),
            Vec3::ZERO,
            "the frame lost the document it had",
        );
        assert_eq!(engine.game().instances, 1, "the instance count moved");

        // **A document that parses but has nothing to draw is refused too.** A
        // point list is legal glTF that this renderer does not draw, so the
        // conversion skips it and `model::load` reports `NoGeometry` — a
        // different refusal from the parse failure above, and the one an
        // artist actually produces by exporting the wrong collection.
        std::fs::write(&options.model, fixture::points_glb()).expect("the points write");
        for _ in 0..frames {
            engine.frame().expect("a frame");
        }
        assert_eq!(
            engine.game().reloads,
            0,
            "a document nothing could be made of was loaded",
        );
        assert_eq!(engine.game().instances, 1, "the frame lost its geometry");

        // And the next good write still lands, so the refusal is not a latch.
        let moved = Vec3::new(12.5, 0.0, 0.0);
        std::fs::write(&options.model, fixture::quad_glb(moved)).expect("the re-export");
        for _ in 0..frames {
            engine.frame().expect("a frame");
            if engine.game().reloads > 0 {
                break;
            }
        }
        assert_eq!(
            engine.game().bounds.center(),
            moved,
            "the recovery never happened"
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A reload keeps the exposure.** It is the renderer's state and the
    /// renderer is replaced, so without `Gpu::reload` carrying it across, every
    /// save would reset the artist's brightness to the default.
    #[test]
    fn a_reload_keeps_the_exposure() {
        let (_dir, options) = model_at(&fixture::quad_glb(Vec3::ZERO), 4096);
        let mut engine = scripted(&options);
        let window = engine.window();
        engine.frame().expect("a frame");
        for _ in 0..3 {
            tap(&mut engine, window, EXPOSURE_UP_KEY);
        }
        let raised = engine.gpu().exposure();
        assert!(
            (raised - 2.0).abs() < 1e-4,
            "the key did not move it: {raised}"
        );

        std::fs::write(&options.model, fixture::quad_glb(Vec3::new(12.5, 0.0, 0.0)))
            .expect("the re-export");
        let frames = (f64::from(crate::args::DEFAULT_TICK_HZ) * 2.0).ceil() as usize;
        for _ in 0..frames {
            engine.frame().expect("a frame");
            if engine.game().reloads > 0 {
                break;
            }
        }
        assert_eq!(engine.game().reloads, 1, "the re-export was not picked up");
        assert!(
            (engine.gpu().exposure() - raised).abs() < 1e-4,
            "the reload reset the exposure to {}",
            engine.gpu().exposure(),
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **No two bindings claim the same key.**
    ///
    /// Three of these are the loop's reserved keys and three more are the ones
    /// it arbitrates for a menu that is showing; the rest are this
    /// application's. A collision is silent — the loop would swallow the key
    /// and the viewer's half would simply never run — so it is checked rather
    /// than argued about in a comment.
    #[test]
    fn no_two_bindings_claim_the_same_key() {
        let bound = [
            ("reframe", REFRAME_KEY),
            ("listing", LISTING_KEY),
            ("wireframe", WIREFRAME_KEY),
            ("normals", NORMALS_KEY),
            ("skeleton", SKELETON_KEY),
            ("exposure down", EXPOSURE_DOWN_KEY),
            ("exposure up", EXPOSURE_UP_KEY),
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

    /// **The skinning dispatch is in the frame, and in front of everything that
    /// reads a vertex out of the pool.**
    ///
    /// The frame this application records is the observable, for
    /// `the_frame_carries_a_grid_floor_after_the_tonemap`'s reason: a
    /// reservation made and a pass built are not "the document is deformed",
    /// and only the recorded graph says which. Ordering is half the claim —
    /// the dispatch writes the run the depth prepass and the forward pass read,
    /// and a graph that recorded it afterwards would draw the pose of the frame
    /// before on hardware and look perfect under a validation layer.
    ///
    /// The demo document rather than the quad fixture, because it is the one
    /// this crate has that wears a skin — `crate::demo_model`'s `crate-rig`.
    #[test]
    fn a_rigged_document_records_its_skinning_dispatch_before_it_draws() {
        let (_dir, options) = model_at(&crate::demo_model::demo_glb(), 4);
        let mut engine = scripted(&options);
        engine.frame().expect("a frame");

        let dump = engine.gpu().last_dump().to_string();
        let skinning = dump
            .find(r#"pass "skinning""#)
            .unwrap_or_else(|| panic!("the rigged document records no skinning pass:\n{dump}"));
        for drawn in [r#"pass "depth-prepass""#, r#"pass "forward""#] {
            let at = dump
                .find(drawn)
                .unwrap_or_else(|| panic!("no {drawn} in the viewer's frame:\n{dump}"));
            assert!(
                skinning < at,
                "the skinning dispatch is recorded after {drawn}, so the draw reads the \
                 region before the compute write:\n{dump}",
            );
        }

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

    /// **The clip in the document plays, through the loop, with nobody
    /// touching anything.**
    ///
    /// `crate::anim` asserts the conversion, the sampling and the wrap on their
    /// own; this is the claim that the frame steps them — that
    /// [`Viewer::draw`] carries the playhead and that `pose` on the heartbeat
    /// is a number the document moved rather than one this application prints.
    /// Over the demo document, which is the one this application ships a rig
    /// in and the one the browser gate reads.
    #[test]
    fn the_documents_clip_plays_without_anyone_touching_anything() {
        let (_dir, options) = model_at(&crate::demo_model::demo_glb(), 64);
        let mut engine = scripted(&options);

        engine.frame().expect("a frame");
        let opened = engine.game().pose();
        assert!(
            opened < 1e-6,
            "the demo's clip starts at the rest pose, so the frame it opens on does too: \
             {opened}",
        );

        // Frames until the pose has left rest. A budget rather than a fixed
        // count, because how far a headless frame carries the playhead is
        // wall-clock and not this test's to decide; what is asserted is that it
        // moves at all, which a viewer that never sampled could not do.
        let mut moved = 0.0;
        for _ in 0..options.common.frames.expect("a frame budget") {
            engine.frame().expect("a frame");
            moved = engine.game().pose();
            if moved > 1e-6 {
                break;
            }
        }
        assert!(
            moved > 1e-6,
            "the pose never left rest over a document whose clip turns a joint",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A document with no rig plays nothing and reports the zero honestly**,
    /// which is nearly every document anyone opens.
    ///
    /// Both halves: `pose` stays at zero, and `B` — which is a key a visitor
    /// can press over any file — draws nothing rather than reaching into a rig
    /// that is not there.
    #[test]
    fn a_document_with_no_rig_poses_nothing_and_draws_nothing() {
        let (_dir, mut options) = model_at(&fixture::quad_glb(Vec3::ZERO), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        tap(&mut engine, window, SKELETON_KEY);
        engine.frame().expect("a frame");

        assert_eq!(engine.game().pose(), 0.0);
        assert_eq!(
            ui_lines(&engine),
            Vec::new(),
            "a document with no skin has no skeleton to draw",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **`B` draws the posed skeleton, and what it draws moves because the
    /// clip does — not because the turntable does.**
    ///
    /// The overlay is off first, because a viewer shows the user's asset
    /// unadorned — see [`crate`] — and it is the listing panel's argument: a
    /// panel is asked for rather than dismissed.
    ///
    /// **The camera is taken hold of before the motion is asserted**, and that
    /// is the whole of why this test is worth having. The turntable moves the
    /// eye every frame, so every projected joint moves every frame whatever the
    /// pose is doing: an overlay wired to a skeleton that never moved would
    /// draw a different picture each frame and pass. A wheel event latches the
    /// turntable off for good — see [`TURNTABLE_RATE`] — so what moves after it
    /// is the clip and nothing else.
    #[test]
    fn b_draws_the_posed_skeleton_and_it_moves_with_the_clip() {
        let (_dir, mut options) = model_at(&crate::demo_model::demo_glb(), 64);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        assert_eq!(
            ui_lines(&engine),
            Vec::new(),
            "the overlay is something to ask for, not something to dismiss",
        );

        tap(&mut engine, window, SKELETON_KEY);
        let drawn = ui_lines(&engine);
        assert!(
            !drawn.is_empty(),
            "B drew no skeleton over a document that has one",
        );

        engine
            .shell_mut()
            .scroll(window, ScrollDelta::Lines { x: 0.0, y: -1.0 }, None)
            .expect("the window is live");
        engine.frame().expect("a frame");
        let held = engine.game().camera();
        let before = ui_lines(&engine);
        let posed = engine.game().pose();

        engine.frame().expect("a frame");
        assert_eq!(
            engine.game().camera().eye,
            held.eye,
            "the camera moved after the wheel, so nothing below is about the pose",
        );
        assert_ne!(
            engine.game().pose(),
            posed,
            "the playhead stood still across two frames",
        );
        assert_ne!(
            ui_lines(&engine),
            before,
            "the overlay drew the same picture over a clip that had moved",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The `pose` row and the `[HUD]` line's `pose` are the same number**, so
    /// the browser gate and the panel cannot come to disagree.
    ///
    /// The panel is where a person reads it and the heartbeat is where CI and
    /// `web/tools/browser-e2e.mjs` do; both call [`Viewer::pose`], and this is
    /// what says so rather than leaving two format strings to drift.
    #[test]
    fn the_debug_panel_reports_the_pose_the_heartbeat_does() {
        let (_dir, mut options) = model_at(&crate::demo_model::demo_glb(), 16);
        options.common.debug_overlay = Some(false);
        let mut engine = scripted(&options);
        let window = engine.window();

        engine.frame().expect("a frame");
        engine
            .shell_mut()
            .key_press(window, DEBUG_OVERLAY_KEY)
            .expect("the window is live");
        engine.frame().expect("a frame");

        let drawn = ui_text(&engine);
        assert_eq!(
            row_value(&drawn, "pose"),
            format!("{:.2}", engine.game().pose()),
            "the panel's pose row is not the number the heartbeat prints: {drawn:?}",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }
}
