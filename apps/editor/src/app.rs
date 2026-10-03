//! The window, the device, the camera and the loop that drives the
//! [`Document`].
//!
//! **A hand-written loop, like `apps/bare`'s**, rather than the engine's
//! [`GameLoop`](crcbl::engine::GameLoop): while editing nothing ticks, and the
//! editor has no menu and no HUD, so what the hosted loop would bring is a
//! schedule for things this tool does not have. Play mode is the one thing
//! that ticks, and it is the document's: each frame hands
//! [`Document::advance`] the frame's time, which runs the scene's modules on a
//! fixed step at the world's own rate and does nothing while editing or
//! paused. See [`crate::document`]'s `play` module.
//!
//! # Everything here is a call into [`Document`]
//!
//! A click becomes a ray, the ray becomes [`Document::pick_ray`]; a key becomes
//! an [`EditCommand`], the command becomes [`Document::apply`]; a frame of the
//! panels is [`Panels::frame`]. Nothing in this file writes a component field
//! or lays out a widget, which is the plan's "nothing editor-side may be
//! implemented GUI-only" kept by construction — the headless tests in
//! [`crate::document`] and [`crate::panel`] exercise the same calls with no
//! device at all.
//!
//! # The picture
//!
//! A scene's meshes as their glTF assets, and one
//! [`crcbl::greybox::GREYBOX_CUBE`] per other entity, scaled to its own
//! extents, over [`ForwardRenderer::set_ground_grid`]'s screen-space floor. A
//! renderer holds the geometry it was built with, so a mesh naming an asset it
//! lacks rebuilds it — `meshes`' module docs say why and how. Each selected
//! entity is a [`DebugDraw`](crcbl::render::debug_draw::DebugDraw) box, the
//! primary's in a colour of its own, and the layer is forced on at start-up:
//! `r_debug_draw` is off by default, so an editor that did not switch it on
//! would draw no selection and report nothing wrong.
//!
//! **The picture is drawn into the viewport pane, not the window.** The
//! renderer's camera draws into a graph transient sized to the pane
//! ([`Panels::viewport_extent`]), and the panels' draw list carries a
//! rectangle naming that picture ([`crate::panel::VIEWPORT_TEXTURE`]), which
//! [`UiRenderer::add_passes_with_textures`] samples in the same graph, added
//! after the scene's passes — so the graph draws the scene first and puts the
//! barrier between the two. The window itself is cleared and then covered by
//! the panels and the pane.
//!
//! **The primary camera, not a second view.** `docs/plan/08-editor.md` decided
//! on "a secondary view rendered to a texture a UI rect samples", and
//! [`ForwardRenderer::create_view`] is the renderer half it named. A renderer
//! always draws its primary camera, though, so a view of its own would leave
//! the primary drawing a picture nobody sees; the editor has one camera, and
//! drawing that camera into a target of the pane's size is the same picture
//! without the wasted frame — a default view draws the primary camera's
//! picture byte for byte, which `forward_e2e`'s `views` suite holds. A second
//! pane would be a `create_view` target sampled the same way.
//!
//! A click is picked through that same camera: the ray goes through the
//! click's position **inside the pane**, against the pane's extent, which is
//! the matrix the picture was drawn with. A click outside the pane's rectangle
//! picks nothing.
//!
//! # Dragging an asset into the scene
//!
//! A press on a mesh asset's row in the asset browser
//! ([`Panels::asset_at`]) starts a drag, and a release over the viewport drops
//! it: the ray through the release pixel — the pick's ray — lands on the first
//! surface it strikes, or else on the ground plane, and
//! [`Document::spawn_mesh`] stands the mesh there as one undoable entry and
//! the editor selects it. A release anywhere else drops nothing. Enter on a
//! focused row ([`crate::panel::PanelFrame::spawn`]) places the asset where the
//! ray through the view's centre meets the ground, which is the same command
//! from the keyboard. Both are refused in play mode, on the status line.
//!
//! # The keyboard is not read here
//!
//! [`crate::keys`] holds it, as an [`ActionMap`] whose reserved `ui` and `text`
//! contexts are pushed from what the panels say about themselves — so the
//! arrows nudge the selection until an outliner row takes focus, and nothing
//! here fires while a field is being typed into.
//!
//! # Unsaved edits are asked about
//!
//! A new scene, an open and the window closing put the unsaved bar up for a
//! dirty document, and nothing else is done until it is answered; a window
//! that goes without asking leaves a recovery copy. `unsaved`'s module docs
//! say which backends hold a close request open and which recover.
//!
//! # Recovery copies are offered back
//!
//! At start-up the copies an earlier run left are pruned and the newest
//! offered back on a bar, and a dirty scene is autosaved into the same
//! directory on a timer; `recovery`'s module docs say how.

use std::path::PathBuf;
use std::time::Duration;

use crcbl::core::input::{Modifiers, PointerButton, ScrollDelta};
use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, FrameOutcome, GpuContext, GpuContextDesc, GpuError,
    Handled, LoopError, ModeRequest, PAUSE_KEY, Pending, PointerCapture, RunSummary,
    SettingsSource, open_window, wait_for_configure,
};
use crcbl::hal::{CommandEncoderDesc, ImageUsage};
use crcbl::input::ActionMap;
use crcbl::math::{DQuat, DVec3, Vec2, Vec3};
use crcbl::reflect::Value;
use crcbl::registry::Rotation;
use crcbl::render::grid::GridStyle;
use crcbl::render::{
    Aabb, DirectionalLight, ForwardRenderer, OrbitCamera, Projection, RenderGraph,
    TransientImageDesc, TransientPool, UiRenderer, UiTexture, ViewRay,
};
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::{
    ButtonState, ClipboardContent, ClipboardOffer, DisplayMode, Shell, ShellEvent, WindowDesc,
    WindowId, open,
};
use crcbl::store::settings::SettingsStack;
use crcbl::text_input::TextPump;
use crcbl::ui::tree::{DockLayout, SelectMode};

use crate::args::Options;
use crate::clipboard::{Paste, PasteTarget};
use crate::command::EditCommand;
use crate::document::{Document, EditError, Hit, PlayState, RecoveryCopy};
use crate::gizmo;
use crate::keys::Action;
use crate::layout;
use crate::panel::{PanelInput, Panels, Tone, VIEWPORT_TEXTURE};

mod files;
mod instances;
mod meshes;
mod recovery;
mod unsaved;

use instances::Placed;
use meshes::Shelf;

/// How far one arrow key moves the selection, in metres.
///
/// A centimetre, which is the `#[reflect(step)]` [`crate::scene::Block`] carries
/// on its half extents — the step a component says a drag should take. Holding
/// the key repeats, so a coarse move is a held key rather than a second
/// constant.
pub const NUDGE_M: f64 = 0.01;

/// How far one pixel of a right-drag turns the camera, in radians.
///
/// A full turn across about 900 pixels, which is the sensitivity `apps/viewer`
/// settled on for the same gesture.
const ORBIT_RADIANS_PER_PIXEL: f32 = 0.007;

/// How far one wheel detent zooms, in [`OrbitCamera::zoom`]'s units.
const ZOOM_PER_DETENT: f32 = 0.12;

/// How far one pixel of a high-resolution scroll zooms.
const ZOOM_PER_PIXEL: f32 = 0.004;

/// How much air the ground grid is drawn around the scene, as a multiple of its
/// own extent.
const GRID_MARGIN: f32 = 4.0;

/// How far one wheel detent scrolls a panel, in pixels.
///
/// Three rows of `crcbl_ui`'s outliner, which is what a detent moves a list
/// everywhere else and what `crcbl::debug_console::WHEEL_LINES` moves the
/// console's log — the conversion `crcbl_core::input::ScrollDelta` says is the
/// application's, made once here rather than at each reader.
const WHEEL_LINE_PIXELS: f32 = 3.0 * crcbl::ui::tree::OUTLINER_ROW_HEIGHT;

/// What stops a run: the loop's own failures, and — as
/// [`LoopError::Game`] — the one document failure that can end one.
///
/// Only *opening* a document is fatal. An edit the document refuses while the
/// editor is running is logged and the loop continues, because a command
/// naming a field that is not there is a mistake in the editing rather than a
/// failure of the tool.
pub type EditorError = LoopError<EditError>;

/// What a completed run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// The half every binary in this workspace shares.
    pub run: RunSummary,
    /// How many entities the document held.
    pub entities: usize,
    /// How many commands were applied and still stood at exit — the log's
    /// position, not its length, so a run that undid everything reports none.
    pub commands: usize,
}

/// The window, the device, the document, the panels and the camera.
#[derive(Debug)]
pub struct Editor<S: Shell + ?Sized = dyn Shell> {
    shell: Box<S>,
    window: WindowId,
    gpu: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// One instance per entity, retaining its last published description so
    /// unchanged draws let the renderer settle motion history and reuse shadows.
    instances: Placed,
    /// The assets the renderer was built with — see `meshes`.
    shelf: Shelf,
    /// The document's [`Document::membership`] and [`Document::measures`] the
    /// shelf was last checked against: a mesh can only want a new asset when
    /// one of them moves.
    shelved: (u64, u64),
    /// How far the ground grid reaches, which a rebuilt renderer is given
    /// again.
    grid_extent: f32,
    document: Document,
    /// The docked outliner and inspector, and the join between what they show
    /// and what the document holds.
    panels: Panels,
    /// The compositor that draws [`Panels::draw_list`] over the scene.
    ui: UiRenderer,
    /// The editor's keyboard; see [`crate::keys`].
    actions: ActionMap,
    /// The typing and the clipboard, for the inspector's text fields.
    text_pump: TextPump,
    /// A paste waiting on the clipboard's answer, and what it is for.
    paste: Paste,
    /// Where the pointer is and whether its button is held.
    ///
    /// **Both halves of what the loop needs**: it carries the position into the
    /// next batch, so a frame whose pump delivers no motion does not forget the
    /// cursor, and it turns the batch's press and release *edges* into the
    /// `down` level a tree wants.
    pointer_state: PointerCapture,
    /// What the shell last stamped on a key event: the chord half
    /// [`crate::keys::actions`] reads, and the modifier an outliner click
    /// selects with.
    modifiers: Modifiers,
    /// The player's settings, and where they came from: what the dock layout is
    /// written to on the way out.
    settings: SettingsStack,
    settings_source: SettingsSource<'static>,
    /// The layout the run opened with, so a run that moved nothing writes
    /// nothing.
    opened_layout: DockLayout,
    /// The clock's total elapsed at the last frame, for this frame's `dt`.
    elapsed: Duration,
    /// How many ticks play mode has run this run, across every play.
    ticks: u64,
    camera: OrbitCamera,
    /// Which drag, if any, the pointer is in the middle of.
    drag: Option<Drag>,
    /// The mesh asset a press on the asset browser took hold of, dropped
    /// where the button comes up — see the module docs.
    dragged: Option<String>,
    /// Which handles the selection shows: W and R choose.
    gizmo_mode: gizmo::Mode,
    /// The absolute grid a drag lands on while Ctrl is held, from the player's
    /// settings.
    snap: gizmo::Snap,
    clock_source: Clock,
    budget: FrameBudget,
    events: u64,
    windowed: bool,
    /// What the window title was last set to, so it is only written when it
    /// changes — a title set every frame is a round trip to the window system
    /// for nothing.
    title: String,
    /// The extent the last recorded frame drew the scene at: the viewport
    /// pane's, in window pixels, and the size of the target the pane samples.
    drawn_viewport: Option<(u32, u32)>,
    mode: ModeRequest,
    /// What the unsaved bar is asking about, while it is up — see `unsaved`.
    unsaved: Option<unsaved::Guarded>,
    /// What a Save on the bar goes on with once the save-as it asked for
    /// lands.
    after_save: Option<unsaved::Guarded>,
    /// The asset root `--assets` named, which a scene opened in the run reads
    /// its meshes from too.
    assets: Option<PathBuf>,
    /// Where recovery copies are written, offered back from and pruned in, or
    /// [`None`] for a run that keeps none — see `unsaved` and `recovery`.
    recovery: Option<PathBuf>,
    /// The copies the recovery bar offers, as it lists them — see `recovery`.
    offered: Vec<RecoveryCopy>,
    /// What was offered when Open copy took the bar down to ask about
    /// unsaved edits, until that question ends — see `recovery`.
    held_offer: Vec<RecoveryCopy>,
    /// The autosave into [`recovery`](Self::recovery) — see `recovery`.
    autosave: recovery::Autosave,
    /// Whether the window is to close: set by an answer that lets it, and
    /// carried out before the frame draws.
    closing: bool,
    /// Whether the renderer is to be rebuilt from the document's assets on
    /// the next draw: a document put in place by an open reads them from
    /// another root.
    rebuild_due: bool,
}

/// What a held pointer button is doing: to the camera, or to a gizmo handle.
#[derive(Clone, Debug, PartialEq)]
enum Drag {
    /// Right button: turn the camera around the pivot.
    Orbit,
    /// Middle button: slide the pivot across the view plane.
    Pan,
    /// Left button on a gizmo handle: move, resize or turn the selection
    /// through it — every entity of the group for a translate, which an
    /// empty group never is, and the drag's own entity otherwise.
    Gizmo(gizmo::Drag, gizmo::Group),
}

impl Editor<dyn Shell> {
    /// Opens the document, a shell, a window and a device.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if any of them refused. A document that will not open is
    /// [`LoopError::Game`] carrying the key it is about, because a scene
    /// directory naming a file that is not there is an ordinary mistake and
    /// wants the message rather than a backtrace.
    pub fn start(options: &Options) -> Result<Self, EditorError> {
        let shell = if options.common.headless {
            open_backend_headless()?
        } else {
            open().map_err(LoopError::NoWindowSystem)?
        };
        Self::with_shell(shell, options)
    }
}

/// The headless shell, opened the way `apps/bare` opens it.
fn open_backend_headless() -> Result<Box<dyn Shell>, EditorError> {
    crcbl::shell::open_backend(crcbl::shell::ShellBackend::Headless).map_err(LoopError::Shell)
}

impl<S: Shell + ?Sized> Editor<S> {
    /// Builds the editor on an already-open shell.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the document would not open, the window never
    /// configured, or the device would not open.
    pub fn with_shell(mut shell: Box<S>, options: &Options) -> Result<Self, EditorError> {
        let mut document = open_document(options)?;
        log_outline(&mut document);

        let mut clock_source = Clock::new(options.common.headless);
        clock_source.set_limit(options.common.limit);
        let window = open_window(
            shell.as_mut(),
            &clock_source,
            &WindowDesc {
                title: &document.title(),
                app_id: APP_ID,
                size: crcbl::engine::requested_window_size(options.common.size),
                mode: options.common.display_mode(),
                ..WindowDesc::default()
            },
        )?;

        let mut events = 0;
        let extent = wait_for_configure(shell.as_mut(), window, &mut events)?;
        let gpu = GpuContext::open(
            shell.as_ref(),
            window,
            extent,
            &GpuContextDesc {
                label: "editor",
                ..GpuContextDesc::from(options.common.gpu())
            },
        )?;

        // **The selection is drawn through the debug-draw layer, which is off
        // by default.** `r_debug_draw` is a console variable a person can turn
        // back off; forcing it on here is what makes an editor that has just
        // started show the box around what was clicked.
        crcbl::render::debug_draw::r_debug_draw
            .set(&crcbl::console::Value::Bool(true))
            .expect("`r_debug_draw` is a writable bool");

        let settings_source = SettingsSource::for_run(options.common.headless);
        let settings = settings_source.open_editable(layout::APP_NAME);
        let dock = layout::load(&settings).unwrap_or_else(layout::default_layout);

        let mut editor = Self::build(
            shell,
            window,
            gpu,
            document,
            options,
            events,
            clock_source,
            dock,
            settings,
            settings_source,
        )?;
        editor.frame_scene();
        editor.offer_recovery(options.scene.as_deref());
        Ok(editor)
    }

    /// The device-side half of [`with_shell`](Self::with_shell), split out so
    /// the rollback below is written once.
    #[allow(clippy::too_many_arguments)]
    fn build(
        shell: Box<S>,
        window: WindowId,
        gpu: GpuContext,
        mut document: Document,
        options: &Options,
        events: u64,
        clock_source: Clock,
        dock: DockLayout,
        settings: SettingsStack,
        settings_source: SettingsSource<'static>,
    ) -> Result<Self, EditorError> {
        let bounds = scene_bounds(&mut document);
        let grid_extent = grid_extent(&bounds);
        let wanted = document.mesh_assets();
        let (shelf, scene) = Shelf::build(document.assets(), &wanted);
        let (renderer, instances) = renderer_for(&gpu, &scene, grid_extent, &mut document, &shelf)?;
        let shelved = (document.membership(), document.measures());
        // Rolled back by hand: this type has no `Drop`, so a `?` would leak
        // the renderer's pipelines rather than release them — `apps/towers`
        // carries the same note.
        let ui = match UiRenderer::new(gpu.device(), gpu.queue(), gpu.format()) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(gpu.device());
                return Err(GpuError::Hal(error).into());
            }
        };

        let panels = Panels::new(&mut document, dock.clone(), gpu.extent());
        let title = document.title();
        let snap = gizmo::Snap::load(&settings);
        let autosave = recovery::Autosave::load(&settings);
        Ok(Self {
            windowed: !options.common.headless,
            shell,
            window,
            gpu,
            renderer,
            pool: TransientPool::new(),
            instances,
            shelf,
            shelved,
            grid_extent,
            document,
            panels,
            ui,
            actions: crate::keys::map(),
            text_pump: TextPump::new(),
            paste: Paste::default(),
            pointer_state: PointerCapture::new(),
            modifiers: Modifiers::empty(),
            settings,
            settings_source,
            opened_layout: dock,
            elapsed: Duration::ZERO,
            ticks: 0,
            camera: OrbitCamera::new(bounds.center(), 1.0, Projection::default()),
            drag: None,
            dragged: None,
            gizmo_mode: gizmo::Mode::default(),
            snap,
            clock_source,
            budget: FrameBudget::new(options.common.frame_budget()),
            events,
            title,
            drawn_viewport: None,
            mode: ModeRequest::new(),
            unsaved: None,
            after_save: None,
            assets: options.assets.clone(),
            recovery: unsaved::recovery_base(options),
            offered: Vec::new(),
            held_offer: Vec::new(),
            autosave,
            closing: false,
            rebuild_due: false,
        })
    }

    /// The swapchain's current size.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// The document being edited.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// The document being edited, to drive from a test the way the loop drives
    /// it from events.
    pub const fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// One turn: pump, read the input, draw, present.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the shell or the device refused something. An edit
    /// that the document refused is **not** one of them: a command naming a
    /// field that is not there is a mistake in the editing, not a failure of
    /// the run, so it is logged and the loop continues.
    pub fn frame(&mut self) -> Result<Flow, EditorError> {
        if self.budget.is_spent() {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }
        // Only up to the frame limiter's next deadline, and not at all without
        // one, as `crcbl::engine::Loop::frame` does: this loop draws every
        // frame, so a fixed idle was paid in full on each of them, because
        // nothing but input ends a Win32 or X11 wait early.
        if self.windowed
            && let Some(idle) = self.clock_source.idle()
        {
            self.shell.wait_events(Some(idle));
        }

        // **Read at the top of the frame, which is last frame's answer**, as
        // the engine loop reads the console's: the panel a person is typing at
        // is the one that was on screen when they typed, and the map's stack
        // was synced from the same answer at the end of the frame before.
        let editing = self.panels.text_editing();
        // The clock is advanced although nothing ticks, because it is what
        // paces the loop — see `crate::args::DEFAULT_TICK_HZ`. Its difference
        // is the frame the map and the caret are driven by, so the tick begins
        // once a frame exactly as `crcbl::nav`'s docs ask.
        let elapsed = self.clock_source.advance();
        let dt = elapsed.saturating_sub(self.elapsed);
        self.elapsed = elapsed;
        self.actions.begin_tick(dt.as_secs_f32());

        let mut pending = self.pointer_state.pending();
        let Self {
            shell,
            actions,
            text_pump,
            paste,
            modifiers,
            ..
        } = self;
        shell.pump(&mut |event| {
            // Escape is the engine loop's pause key, which `observe` claims
            // for the hosted loop's pause panel. This loop has none, so the
            // key is the editor's: what backs out of a text field and
            // cancels the unsaved bar.
            let escape = matches!(
                event,
                ShellEvent::Key {
                    key_code: Some(PAUSE_KEY),
                    ..
                }
            );
            if pending.observe(&event) == Handled::Game || escape {
                if let ShellEvent::Key {
                    key_code: Some(key),
                    state,
                    repeat: false,
                    modifiers: held,
                    ..
                } = event
                {
                    *modifiers = held;
                    actions.key_event(key, state == ButtonState::Pressed);
                }
                text_pump.observe(&event, editing);
                paste.observe(&event);
            }
        });
        self.events += pending.count;
        self.mode.check(&*self.shell, self.window);

        if pending.destroyed {
            // Gone without a request, so nothing could be asked.
            self.recover_unsaved();
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested
            && let Some(flow) = self.close_requested()?
        {
            return Ok(flow);
        }
        if let Some(size) = pending.resized {
            self.gpu.resize((size.width, size.height))?;
        }
        if pending.focus_lost {
            // No platform sends the releases for what was held when focus left,
            // and a map that was never told would nudge for ever.
            crate::keys::release_keys(&mut self.actions);
        }

        // The pointer and the camera are decided against the rectangles the
        // panels were **last** laid out with, which is the same layout the tree
        // resolves this frame's click against — so exactly one of them claims
        // a press. Nothing in the viewport is the pointer's while the unsaved
        // bar asks.
        let in_viewport = self.unsaved.is_none()
            && pending
                .pointer
                .is_some_and(|at| self.panels.in_viewport(at));
        self.drive_camera(&pending, in_viewport);
        if pending.pointer_pressed && in_viewport {
            self.panels.release_keyboard();
            if !self.grab_handle(&pending) {
                self.pick(&pending);
            }
        }
        self.drag_asset(&pending, in_viewport);
        // Taken out while it writes, which borrows the editor whole, and put
        // back unless the button came up.
        match self.drag.take() {
            Some(Drag::Gizmo(drag, group)) => {
                if pending.motion.is_some()
                    && let Some(at) = pending.pointer
                {
                    self.move_handle(&drag, &group, at);
                }
                if !pending.pointer_released {
                    self.drag = Some(Drag::Gizmo(drag, group));
                }
            }
            other => self.drag = other,
        }

        let mut asked = if self.unsaved.is_some() {
            crate::keys::unsaved(&self.actions)
                .map(Action::Unsaved)
                .into_iter()
                .collect()
        } else {
            crate::keys::actions(&self.actions, self.modifiers, editing)
        };
        // The recovery bar's keys only while it is up and nothing asks over
        // it: under the unsaved bar, Escape is that bar's Cancel.
        let keyed_recovery = if self.unsaved.is_none() && self.panels.recovery().is_some() {
            crate::keys::recovery(&self.actions, editing)
        } else {
            None
        };
        let pointer = self.pointer_state.resolve(&pending);
        let input = PanelInput {
            pointer,
            nav: crcbl::ui_nav::nav_input(&self.actions),
            text: self.text_pump.frame(dt),
            extent: self.extent(),
            select: select_mode(self.modifiers),
            scroll: if in_viewport {
                0.0
            } else {
                wheel_pixels(&pending)
            },
        };
        let panels = self.panels.frame(&mut self.document, input);
        asked.extend(panels.unsaved.map(Action::Unsaved));
        asked.extend(panels.toolbar);
        let accepted = panels.spawn;
        self.draw_gizmo(pointer.pos);

        let requests = self.panels.take_clipboard_requests();
        self.text_pump
            .serve(requests, self.shell.as_mut(), self.window);
        self.sync_contexts();

        // Before this frame's actions: the time the frame covers passed before
        // any key in it was read, so the frame that starts play does not tick
        // for time spent editing, and the one that pauses ticks what it played.
        self.ticks += u64::from(self.document.advance(dt));
        // What the game turned down in those ticks: a command sent from the
        // play strip is told here, a frame after it was sent.
        let refusals = self.document.take_play_refusals();
        if !refusals.is_empty() {
            for refusal in &refusals {
                crcbl::log::warn!("editor: the game refused a command — {refusal}");
            }
            self.panels
                .set_status(refused_status(&refusals), Tone::Warning);
        }
        // Before this frame's actions: a click on a toolbar button is what
        // committed a save-as being typed, and the scene it was typed for is
        // the one to save — not the new scene or the played one the button
        // asks for.
        if let Some(text) = panels.save_as {
            self.save_as(&text)?;
        }
        if let Some(text) = panels.open {
            self.open(&text);
        }
        if let Some(answer) = panels.recovery.or(keyed_recovery)
            && let Err(error) = self.answer_recovery(answer)
        {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
        for action in asked {
            self.act(&action);
        }
        self.follow_after_save();
        if let Some(flow) = self.close_if_asked()? {
            return Ok(flow);
        }
        self.tick_autosave();
        if let Some(asset) = accepted {
            self.place_at_centre(&asset);
        }
        if let Some((target, content)) = self.paste.take() {
            self.paste_content(&target, &content);
        }
        self.update_title();

        let outcome = self.draw()?;
        self.budget.record(outcome)?;
        Ok(Flow::Continue)
    }

    /// Puts the reserved contexts where this frame's panels say they belong.
    ///
    /// `ui` first and `text` over it, which is the order the stack wants, and
    /// `ui` comes off only once `text` has — see [`crate::keys::push_ui`].
    fn sync_contexts(&mut self) {
        let holds = self.panels.holds_keyboard();
        if holds {
            crate::keys::push_ui(&mut self.actions);
        }
        crcbl::input::text::sync(&mut self.actions, self.panels.text_editing())
            .expect("the editor's map declares the text context and nothing pushes over it");
        if !holds {
            crate::keys::pop_ui(&mut self.actions);
        }
    }

    /// Turns, slides and zooms the camera from this batch's pointer input.
    ///
    /// **A drag only begins inside the viewport**, and the wheel only zooms
    /// there: a right-drag over the inspector is not an orbit, and a wheel over
    /// the outliner scrolls it instead. A drag already under way keeps going
    /// wherever the pointer goes, which is what every turntable does and what
    /// stops an orbit dying at the panel's edge.
    fn drive_camera(&mut self, pending: &Pending, in_viewport: bool) {
        for (button, pressed) in &pending.buttons {
            let drag = match button {
                PointerButton::Right => Some(Drag::Orbit),
                PointerButton::Middle => Some(Drag::Pan),
                _ => None,
            };
            if let Some(drag) = drag {
                self.drag = (*pressed && in_viewport).then_some(drag);
            }
        }

        if let (Some(drag), Some(motion)) = (&self.drag, pending.motion) {
            match drag {
                // Negated on both axes: dragging right turns the scene right,
                // which means swinging the eye left — the grab-and-drag
                // direction `OrbitCamera::pan` documents and the one every
                // turntable in every tool uses.
                Drag::Orbit => self.camera.orbit(
                    -motion.x * ORBIT_RADIANS_PER_PIXEL,
                    -motion.y * ORBIT_RADIANS_PER_PIXEL,
                ),
                Drag::Pan => {
                    // The pane's height, which is what the picture spans.
                    let height = self.panels.viewport_extent().1 as f32;
                    self.camera.pan(-motion.x / height, motion.y / height);
                }
                // Moved by `move_handle`, which reads where the pointer is
                // rather than how far it went.
                Drag::Gizmo(..) => {}
            }
        }

        if !in_viewport {
            return;
        }
        for scroll in &pending.scrolls {
            self.camera.zoom(match *scroll {
                ScrollDelta::Lines { y, .. } => y * ZOOM_PER_DETENT,
                #[allow(clippy::cast_possible_truncation)]
                ScrollDelta::Pixels { y, .. } => y as f32 * ZOOM_PER_PIXEL,
            });
        }
    }

    /// The selection's gizmo handles in the current mode, in the pane's
    /// pixels — none for nothing selected, and none for an entity without the
    /// field the mode writes: no `position` to move, no `half_extents` to
    /// resize, or no `rotation` to turn. With several selected, translate's
    /// alone, while one of them has a `position` — see the gizmo's module
    /// docs.
    fn handles(&mut self) -> Vec<gizmo::Handle> {
        let Some((centre, frame)) = self.gizmo_centre() else {
            return Vec::new();
        };
        gizmo::handles(
            &self.camera.camera(),
            self.panels.viewport_extent(),
            centre,
            frame,
            self.panels.scale(),
            self.gizmo_mode,
        )
    }

    /// Where the handles stand, in render space, and the frame a scale
    /// handle's axes are turned by — or [`None`] where the current mode shows
    /// none (see [`handles`](Self::handles)).
    ///
    /// A lone entity's handles stand at the centre of its box, turned as it
    /// is; several selected share theirs at the selection's pivot
    /// ([`Document::selection_pivot`]), on the world's axes.
    fn gizmo_centre(&mut self) -> Option<(Vec3, DQuat)> {
        let id = self.document.primary()?;
        if self.document.selection().len() > 1 {
            if self.gizmo_mode != gizmo::Mode::Translate || self.group().members.is_empty() {
                return None;
            }
            let pivot = self.document.selection_pivot()?;
            return Some((pivot.as_vec3(), DQuat::IDENTITY));
        }
        if !self.has_field(id, self.gizmo_mode) {
            return None;
        }
        let (min, max) = self.document.bounds(id)?;
        let placement = self.document.placement(id)?;
        Some(((min + max) * 0.5, placement.rotation))
    }

    /// Every selected entity a translate moves — each one whose placing
    /// component has a `position` — and where each stands, in selection
    /// order.
    fn group(&mut self) -> gizmo::Group {
        let mut members = Vec::new();
        for id in self.document.selection().to_vec() {
            let (Some(system), Some(start)) = (
                self.document.placing_system(id),
                self.field_values(id, gizmo::POSITION),
            ) else {
                continue;
            };
            members.push(gizmo::Member {
                entity: id,
                system,
                start,
            });
        }
        gizmo::Group { members }
    }

    /// Whether the component placing `id` has the field `mode`'s handles
    /// write.
    fn has_field(&mut self, id: SceneEntityId, mode: gizmo::Mode) -> bool {
        match mode {
            gizmo::Mode::Rotate => self.rotation_of(id).is_some(),
            gizmo::Mode::Translate | gizmo::Mode::Scale => {
                self.field_values(id, mode.field()).is_some()
            }
        }
    }

    /// The three numbers `field.0` to `field.2` of the component placing `id`
    /// hold ([`Document::placing_system`]), or [`None`] where nothing places it
    /// or it has no such field.
    ///
    /// **By name, through the component's reflected paths** — the way an arrow
    /// key finds `position` — so any registered component with the field has
    /// handles for it and no component type is named here. The placing
    /// component's, so the handles move what the picture is drawn from.
    fn field_values(&mut self, id: SceneEntityId, field: &str) -> Option<[f64; 3]> {
        let system = self.document.placing_system(id)?;
        let mut values = [0.0; 3];
        for (index, value) in values.iter_mut().enumerate() {
            let Ok(Value::Float(read)) =
                self.document.read(id, &system, &format!("{field}.{index}"))
            else {
                return None;
            };
            *value = read;
        }
        Some(values)
    }

    /// The orientation of the component placing `id`, from its
    /// [`gizmo::ROTATION`] field — or [`None`] where nothing places it or it
    /// has no such field.
    ///
    /// By name, as [`field_values`](Self::field_values) is, and of the
    /// `crcbl::registry::Rotation` type the scene format reads: a field
    /// called `rotation` of any other type is not one the handles can write.
    fn rotation_of(&mut self, id: SceneEntityId) -> Option<DQuat> {
        let system = self.document.placing_system(id)?;
        let component = self.document.component(id, &system)?;
        let index = component
            .fields()
            .iter()
            .position(|field| field.name == gizmo::ROTATION)?;
        component
            .field(index)?
            .as_any()
            .downcast_ref::<Rotation>()
            .map(Rotation::quat)
    }

    /// Starts a gizmo drag if the press landed on a handle, and says whether it
    /// did — a press that missed every handle is a pick.
    fn grab_handle(&mut self, pending: &Pending) -> bool {
        let (Some(at), Some(id)) = (pending.pointer, self.document.primary()) else {
            return false;
        };
        let handles = self.handles();
        let (corner, _) = self.panels.viewport_pixels();
        let Some(grip) = gizmo::hit(&handles, at - corner, self.panels.scale()) else {
            return false;
        };
        let Some((centre, frame)) = self.gizmo_centre() else {
            return false;
        };
        let gesture = self.document.begin_gesture();
        let grabbed = match grip {
            gizmo::Grip::Rotate(axis) => self
                .begin_turn(id, axis, gesture, centre, at - corner)
                .map(|drag| (drag, gizmo::Group::default())),
            gizmo::Grip::Move(_) | gizmo::Grip::MovePlane(_) => {
                let group = self.group();
                // A group of one is its own pivot, as `gizmo::Drag::spread`
                // says: its position is what the drag moves and snaps.
                let start = match group.members.as_slice() {
                    [] => None,
                    [only] => Some(only.start),
                    _ => self
                        .document
                        .selection_pivot()
                        .map(|pivot| pivot.to_array()),
                };
                start
                    .and_then(|start| {
                        self.begin_drag(id, grip, gesture, start, (centre, frame), at)
                    })
                    .map(|drag| (drag, group))
            }
            gizmo::Grip::Scale(_) | gizmo::Grip::ScaleAll => self
                .field_values(id, gizmo::HALF_EXTENTS)
                .and_then(|start| self.begin_drag(id, grip, gesture, start, (centre, frame), at))
                .map(|drag| (drag, gizmo::Group::default())),
        };
        let Some((drag, group)) = grabbed else {
            return false;
        };
        self.drag = Some(Drag::Gizmo(drag, group));
        true
    }

    /// A drag of an arrow, a plane or a scale handle of `id`, whose field —
    /// or, for a translate, whose pivot — holds `start`, its handles standing
    /// at `shown`'s centre and turned by its frame, pressed at `at` in window
    /// pixels.
    fn begin_drag(
        &self,
        id: SceneEntityId,
        grip: gizmo::Grip,
        gesture: crate::command::Gesture,
        start: [f64; 3],
        shown: (Vec3, DQuat),
        at: Vec2,
    ) -> Option<gizmo::Drag> {
        let (centre, frame) = shown;
        gizmo::Drag::begin(
            id,
            grip,
            gesture,
            start,
            centre.as_dvec3(),
            frame,
            &self.pointer_at(at),
            self.panels.scale(),
        )
    }

    /// A drag of `id`'s ring about `axis`, pressed at `at` in the pane's
    /// pixels, its handles drawn about `shown` — or [`None`] for an entity with
    /// no rotation, or a centre behind the eye.
    ///
    /// The pivot is the placement's own centre in `f64`, not the drawn one, so
    /// a block whose position is its centre keeps that position to the bit.
    fn begin_turn(
        &mut self,
        id: SceneEntityId,
        axis: gizmo::Axis,
        gesture: crate::command::Gesture,
        shown: Vec3,
        at: Vec2,
    ) -> Option<gizmo::Drag> {
        let rotation = self.rotation_of(id)?;
        let pivot = self.document.placement(id)?.centre;
        let position = self
            .field_values(id, gizmo::POSITION)
            .map(DVec3::from_array);
        let camera = self.camera.camera();
        let centre = camera.pixel_of(shown, self.panels.viewport_extent())?;
        let toward_eye = axis.unit().dot(camera.eye.as_dvec3() - pivot);
        let facing = if toward_eye < 0.0 { -1.0 } else { 1.0 };
        Some(gizmo::Drag::turn(
            id,
            axis,
            gesture,
            pivot,
            at,
            gizmo::Turn {
                rotation,
                position,
                centre,
                facing,
            },
        ))
    }

    /// Moves, resizes or turns the dragged entity — or moves every entity of
    /// `group`, for a translate — to where the pointer at `at` puts it,
    /// snapped while Ctrl is held: one write of the drag's gesture, so the
    /// whole drag undoes at once.
    ///
    /// A handle that sets several leaves at once — a plane, the centre, a
    /// ring, any translate of several entities — sets them as one
    /// [`EditCommand::Batch`], which the log folds like a leaf
    /// (`crate::command::UndoLog::record_in`).
    fn move_handle(&mut self, drag: &gizmo::Drag, group: &gizmo::Group, at: Vec2) {
        let snap = self
            .modifiers
            .contains(Modifiers::CTRL)
            .then_some(self.snap);
        let pointer = self.pointer_at(at);
        let set = |entity, system: String, write: gizmo::Write| EditCommand::SetProperty {
            entity,
            system,
            path: write.path,
            value: Value::Float(write.value),
        };
        let commands: Vec<EditCommand> = if group.members.is_empty() {
            let Some(writes) = drag.writes(&pointer, snap) else {
                return;
            };
            // Read each move rather than held by the drag: nothing but the
            // drag edits the scene while it runs, so the answer cannot move
            // under it.
            let Some(system) = self.document.placing_system(drag.entity) else {
                return;
            };
            writes
                .into_iter()
                .map(|write| set(drag.entity, system.clone(), write))
                .collect()
        } else {
            let Some(writes) = drag.spread(group, &pointer, snap) else {
                return;
            };
            writes
                .into_iter()
                .map(|(member, write)| set(member.entity, member.system.clone(), write))
                .collect()
        };
        let command = EditCommand::one_or_batch(commands);
        if let Err(error) = self.document.apply_in(command, drag.gesture) {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Draws the selection's handles over the pane, the one being dragged or
    /// under `pointer` brightened.
    fn draw_gizmo(&mut self, pointer: Vec2) {
        let handles = self.handles();
        if handles.is_empty() {
            return;
        }
        let (corner, _) = self.panels.viewport_pixels();
        let hot = match &self.drag {
            Some(Drag::Gizmo(drag, _)) => Some(drag.grip()),
            _ => gizmo::hit(&handles, pointer - corner, self.panels.scale()),
        };
        self.panels
            .overlay_viewport(|list| gizmo::draw(list, &handles, corner, hot));
    }

    /// Shows `mode`'s handles from now on, and says on the status line what
    /// they do — or, for scale and rotate, why the selection has none.
    fn choose_mode(&mut self, mode: gizmo::Mode) {
        self.gizmo_mode = mode;
        let (text, tone) = match mode {
            gizmo::Mode::Translate => (
                format!(
                    "Translate: drag an arrow along its axis or a square across its plane; \
                     hold Ctrl to snap to the {} m grid",
                    self.snap.grid_step()
                ),
                Tone::Info,
            ),
            gizmo::Mode::Scale => self.field_status(
                mode,
                "Scale: select an entity with half extents to resize it",
                format!(
                    "Scale: drag a box to resize along its axis or the centre to resize \
                     evenly; hold Ctrl to snap half extents to {} m",
                    self.snap.scale_step()
                ),
                "resize",
            ),
            gizmo::Mode::Rotate => self.field_status(
                mode,
                "Rotate: select an entity with a rotation to turn it",
                format!(
                    "Rotate: drag a ring to turn about its axis; hold Ctrl to snap to {}°",
                    self.snap.angle_step()
                ),
                "turn",
            ),
        };
        self.panels.set_status(text, tone);
    }

    /// What the status line says when a mode whose handles need a field is
    /// chosen: `none` with nothing selected, why it shows no handles with
    /// several selected — it acts on one entity — `usage` when the selection
    /// has the field, and why it shows no handles when it does not — it has
    /// nothing to `verb`.
    fn field_status(
        &mut self,
        mode: gizmo::Mode,
        none: &str,
        usage: String,
        verb: &str,
    ) -> (String, Tone) {
        let Some(id) = self.document.primary() else {
            return (none.to_owned(), Tone::Info);
        };
        let label = match mode {
            gizmo::Mode::Translate => "Translate",
            gizmo::Mode::Scale => "Scale",
            gizmo::Mode::Rotate => "Rotate",
        };
        let count = self.document.selection().len();
        if count > 1 {
            return (
                format!(
                    "{label}: {count} entities are selected, and it acts on one at a time; \
                     select one to {verb} it"
                ),
                Tone::Warning,
            );
        }
        if self.has_field(id, mode) {
            return (usage, Tone::Info);
        }
        let placing = self.document.placing_system(id);
        let kind = placing
            .and_then(|system| self.document.component(id, &system))
            .map_or("entity", |component| {
                component
                    .type_name()
                    .rsplit("::")
                    .next()
                    .unwrap_or("entity")
            });
        (
            format!(
                "{label}: this {kind} has no `{}` field, so it has nothing to {verb}",
                mode.field()
            ),
            Tone::Warning,
        )
    }

    /// Takes hold of the asset a press on the browser landed on, and drops
    /// the one held where the button comes up over the viewport — see the
    /// module docs.
    fn drag_asset(&mut self, pending: &Pending, in_viewport: bool) {
        if pending.pointer_pressed && !in_viewport {
            self.dragged = pending.pointer.and_then(|at| self.panels.asset_at(at));
        }
        if !pending.pointer_released {
            return;
        }
        let Some(asset) = self.dragged.take() else {
            return;
        };
        if let (true, Some(at)) = (in_viewport, pending.pointer) {
            let ray = self.ray_at(at);
            let point = self.document.drop_point(&ray);
            self.place(&asset, point);
        }
    }

    /// Places `asset` where the ray through the middle of the viewport meets
    /// the ground: Enter on the asset browser.
    fn place_at_centre(&mut self, asset: &str) {
        let (min, max) = self.panels.viewport_pixels();
        let ray = self.ray_at((min + max) * 0.5);
        self.place(asset, Document::ground_point(&ray));
    }

    /// Spawns a mesh of `asset` standing on `point` and selects it, saying on
    /// the status line what was placed — and that it is a placeholder, and
    /// why, when its asset will not load — or why nothing was.
    fn place(&mut self, asset: &str, point: Result<DVec3, EditError>) {
        let placed = point.and_then(|point| self.document.spawn_mesh(asset, point));
        let id = match placed {
            Ok(id) => id,
            Err(error) => {
                crcbl::log::warn!("editor: {error}");
                self.panels.set_status(error.to_string(), Tone::Warning);
                return;
            }
        };
        self.document.select(Some(id));
        let prefix = format!("entity #{id}: ");
        let problem = self
            .document
            .mesh_problems()
            .into_iter()
            .find(|problem| problem.starts_with(&prefix));
        match problem {
            Some(problem) => self.panels.set_status(
                format!("Placed #{id} as a placeholder: {problem}"),
                Tone::Warning,
            ),
            None => self
                .panels
                .set_status(format!("Placed `{asset}` as #{id}"), Tone::Info),
        }
    }

    /// Selects whatever the left button landed on, alone — or, with Ctrl
    /// held, adds it to the selection or takes it out.
    ///
    /// A Ctrl click on nothing keeps the selection, so a slip between two
    /// entities does not throw away the ones gathered so far. Called only for
    /// a press inside the viewport pane — see [`Editor::frame`]. The ray is
    /// [`ray_at`](Self::ray_at)'s.
    ///
    /// While a scene plays, a click on a spawned entity a play action picks
    /// from — towers' built towers — makes it the runtime pick
    /// ([`Document::set_runtime_pick`]) and selects nothing, and a plain
    /// click on anything else clears that pick: one click, one thing picked.
    fn pick(&mut self, pending: &Pending) {
        let Some(at) = pending.pointer else {
            return;
        };
        let ray = self.ray_at(at);
        let hit = self.document.hit_ray(&ray);
        let spawned = match hit {
            Some(Hit::Spawned(entity)) => Some(entity),
            _ => None,
        };
        let scene = hit.and_then(Hit::scene);
        if self.modifiers.contains(Modifiers::CTRL) {
            if let Some(id) = scene {
                self.document.toggle_selected(id);
            }
            if spawned.is_some() {
                self.document.set_runtime_pick(spawned);
            }
        } else {
            self.document.select(scene);
            self.document.set_runtime_pick(spawned);
        }
    }

    /// The ray through the scene under `at`, a point in window pixels.
    ///
    /// **Through the pane, not the window**: the scene is drawn into a target
    /// of the pane's extent, so the pixel under the cursor is `at` less the
    /// pane's top-left, unprojected against that extent — the matrix the
    /// picture was drawn with. Half a pixel is added, because
    /// [`Camera::ray_through`](crcbl::render::Camera::ray_through) takes a
    /// pixel's top-left corner and a click is about the pixel's middle.
    fn ray_at(&self, at: Vec2) -> ViewRay {
        let (min, _) = self.panels.viewport_pixels();
        self.camera
            .camera()
            .ray_through(at - min + Vec2::splat(0.5), self.panels.viewport_extent())
    }

    /// The pointer at `at`, a point in window pixels, as a gizmo drag reads it:
    /// [`ray_at`](Self::ray_at)'s ray, and the point from the pane's corner.
    fn pointer_at(&self, at: Vec2) -> gizmo::Pointer {
        let (min, _) = self.panels.viewport_pixels();
        gizmo::Pointer {
            ray: self.ray_at(at),
            at: at - min,
        }
    }

    /// Carries out one keyboard action.
    ///
    /// A refusal is logged rather than propagated: nudging with nothing
    /// selected, or saving a document with no directory, are things a person
    /// does and then does differently — not conditions that should end the run.
    ///
    /// **While the unsaved bar asks, only its answer is carried out** — see
    /// `unsaved`. The keyboard asks for nothing else then, and the panels take
    /// no click but the bar's; this is the rule both stand on.
    fn act(&mut self, action: &Action) {
        if self.unsaved.is_some() && !matches!(action, Action::Unsaved(_)) {
            crcbl::log::info!("editor: {action:?} waits for the unsaved bar's answer");
            return;
        }
        let outcome = match action {
            Action::Nudge { axis, sign } => self.nudge(*axis, sign * NUDGE_M),
            Action::Undo => self.document.undo().map(|_| ()),
            Action::Redo => self.document.redo().map(|_| ()),
            Action::Save => self.save(),
            Action::SaveAs => self.begin_save_as(),
            Action::NewScene => self.ask_new_scene(),
            Action::Open => self.begin_open(),
            Action::Unsaved(answer) => self.answer(*answer),
            Action::Frame => {
                self.frame_scene();
                Ok(())
            }
            Action::Delete => self.on_selection(|document, ids| document.delete(ids)),
            Action::Duplicate => self.on_selection(|document, ids| {
                let copies = document.duplicate(ids)?;
                document.set_selection(copies);
                Ok(())
            }),
            Action::Copy => self.copy(),
            Action::Paste => {
                // Decided now, from where the keyboard is as the key goes
                // down: the answer arrives frames later.
                let target = self
                    .panels
                    .field_target()
                    .map_or(PasteTarget::Entities, |field| PasteTarget::Field {
                        entity: field.entity,
                        system: field.system.clone(),
                        path: field.path.clone(),
                    });
                if let Err(error) = self.paste.ask(self.shell.as_mut(), self.window, target) {
                    crcbl::log::warn!("editor: the clipboard refused the paste — {error}");
                }
                Ok(())
            }
            Action::Rename => match self.document.primary() {
                Some(id) => self.panels.begin_rename(&self.document, id),
                None => {
                    self.panels.set_status(RENAME_NOTHING, Tone::Info);
                    Ok(())
                }
            },
            Action::Translate => {
                self.choose_mode(gizmo::Mode::Translate);
                Ok(())
            }
            Action::Scale => {
                self.choose_mode(gizmo::Mode::Scale);
                Ok(())
            }
            Action::Rotate => {
                self.choose_mode(gizmo::Mode::Rotate);
                Ok(())
            }
            Action::PlayStop => self.play_or_stop(),
            Action::Pause => {
                self.pause_or_resume();
                Ok(())
            }
            Action::PlayAction(index) => {
                self.panels.send_numbered_play(&mut self.document, *index);
                Ok(())
            }
        };
        if let Err(error) = outcome {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Starts play mode from editing, or stops it and puts the scene back, and
    /// says on the status line which it did.
    fn play_or_stop(&mut self) -> Result<(), EditError> {
        if self.document.play_state() == PlayState::Editing {
            self.document.play()?;
            let status = self.playing_status();
            self.panels.set_status(status, Tone::Info);
        } else {
            self.document.stop()?;
            self.panels.set_status(STOPPED, Tone::Info);
        }
        Ok(())
    }

    /// Pauses a playing scene or resumes a paused one, and says so — or says
    /// there is nothing to pause while editing.
    fn pause_or_resume(&mut self) {
        match self.document.play_state() {
            PlayState::Editing => self.panels.set_status(NOT_PLAYING, Tone::Info),
            PlayState::Playing => {
                self.document.pause();
                self.panels.set_status(PAUSED, Tone::Info);
            }
            PlayState::Paused => {
                // Resuming takes no snapshot and builds no module, so there is
                // nothing for it to refuse.
                if let Err(error) = self.document.play() {
                    crcbl::log::warn!("editor: {error}");
                    self.panels.set_status(error.to_string(), Tone::Warning);
                    return;
                }
                let status = self.playing_status();
                self.panels.set_status(status, Tone::Info);
            }
        }
    }

    /// What the status line says while the scene plays: which modules run it,
    /// or that none does and only the world's own schedule ticks.
    fn playing_status(&self) -> String {
        let modules = self.document.playing_modules();
        if modules.is_empty() {
            "Playing: no game registers a module for this scene's systems, so only the world \
             ticks. Edits are refused until F5 stops play mode"
                .to_owned()
        } else {
            format!(
                "Playing {}: edits are refused until F5 stops play mode (F6 pauses)",
                modules.join(", ")
            )
        }
    }

    /// Saves the document, then says what the games it is made for would
    /// refuse in it — reported, not refused: see [`Document::problems`]. A
    /// document with no directory to save back to asks for one instead, as
    /// save-as does.
    fn save(&mut self) -> Result<(), EditError> {
        match self.document.save() {
            Err(EditError::NoOrigin) => self.begin_save_as(),
            Err(error) => Err(error),
            Ok(()) => self.report_saved("Saved"),
        }
    }

    /// Says on the status line that the document was saved, opening with
    /// `saved` — and what the games it is made for would refuse in it,
    /// reported rather than refused: see [`Document::problems`].
    fn report_saved(&mut self, saved: &str) -> Result<(), EditError> {
        let problems = self.document.problems()?;
        for problem in &problems {
            crcbl::log::warn!("editor: saved, but its game will refuse it: {problem}");
        }
        match problems.as_slice() {
            [] => self.panels.set_status(saved, Tone::Info),
            [only] => self.panels.set_status(
                format!("{saved}, but its game will refuse it: {only}"),
                Tone::Warning,
            ),
            [first, rest @ ..] => self.panels.set_status(
                format!(
                    "{saved}, but its game will refuse it: {first} (and {} more in the log)",
                    rest.len()
                ),
                Tone::Warning,
            ),
        }
        Ok(())
    }

    /// Offers the inspector field the keyboard means to the clipboard, as
    /// plain text — or, with no such field, every selected entity, as the
    /// engine's RON and as text. See [`Panels::field_target`].
    ///
    /// A clipboard that refuses is logged: a backend with none, or a window
    /// system that wants a recent input event first.
    fn copy(&mut self) -> Result<(), EditError> {
        if let Some(field) = self.panels.field_target().cloned() {
            let text = self
                .document
                .copy_field(field.entity, &field.system, &field.path)?;
            self.offer(&[ClipboardOffer::text(&text)]);
            self.panels
                .set_status(format!("Copied `{}`: {text}", field.path), Tone::Info);
            return Ok(());
        }
        let ids = self.document.selection().to_vec();
        if ids.is_empty() {
            crcbl::log::info!("editor: nothing is selected");
            return Ok(());
        }
        let text = self.document.copy(&ids)?;
        self.offer(&[ClipboardOffer::ron(&text), ClipboardOffer::text(&text)]);
        Ok(())
    }

    /// Hands `offers` to the shell's clipboard, logging a refusal.
    fn offer(&mut self, offers: &[ClipboardOffer<'_>]) {
        if let Err(error) = self.shell.clipboard_offer(self.window, offers) {
            crcbl::log::warn!("editor: the clipboard refused the copy — {error}");
        }
    }

    /// Carries out a paste whose answer arrived: spawns the entities it names
    /// and selects them — the last pasted the primary — or writes its value
    /// into the field it was asked for. A refusal is on the status line, and
    /// changes nothing.
    fn paste_content(&mut self, target: &PasteTarget, content: &ClipboardContent) {
        let Some(text) = content.text() else {
            crcbl::log::info!("editor: the clipboard holds no text to paste");
            return;
        };
        let outcome = match target {
            PasteTarget::Entities => self.document.paste(text).map(|pasted| {
                // A paste of nothing leaves the selection where it was.
                if !pasted.is_empty() {
                    self.document.set_selection(pasted);
                }
            }),
            PasteTarget::Field {
                entity,
                system,
                path,
            } => self.document.paste_field(*entity, system, path, text),
        };
        if let Err(error) = outcome {
            crcbl::log::warn!("editor: {error}");
            self.panels.set_status(error.to_string(), Tone::Warning);
        }
    }

    /// Runs `edit` on every selected entity, or says nothing is selected.
    fn on_selection(
        &mut self,
        edit: impl FnOnce(&mut Document, &[SceneEntityId]) -> Result<(), EditError>,
    ) -> Result<(), EditError> {
        let ids = self.document.selection().to_vec();
        if ids.is_empty() {
            crcbl::log::info!("editor: nothing is selected");
            return Ok(());
        }
        edit(&mut self.document, &ids)
    }

    /// Builds and applies the [`EditCommand`] one arrow key means: every
    /// selected entity moved by `delta` along `axis`, as one entry.
    ///
    /// **The command is built from what each field currently holds**, read
    /// back through the same dotted path it will be written through — so a
    /// nudge is relative without the command being relative, which is what
    /// keeps an inverse exact. It moves the component placing each entity
    /// ([`Document::placing_system`]), as the gizmo does, and passes over one
    /// nothing places.
    fn nudge(&mut self, axis: usize, delta: f64) -> Result<(), EditError> {
        let selection = self.document.selection().to_vec();
        let Some(&primary) = selection.last() else {
            crcbl::log::info!("editor: nothing is selected");
            return Ok(());
        };
        let path = format!("position.{axis}");
        let mut commands = Vec::with_capacity(selection.len());
        for entity in selection {
            let Some(system) = self.document.placing_system(entity) else {
                continue;
            };
            let Value::Float(was) = self.document.read(entity, &system, &path)? else {
                crcbl::log::warn!("editor: {path} of #{entity} is not a number");
                continue;
            };
            commands.push(EditCommand::SetProperty {
                entity,
                system,
                path: path.clone(),
                value: Value::Float(was + delta),
            });
        }
        if commands.is_empty() {
            self.panels.set_status(
                format!("#{primary} is not a thing in space, so nothing moves it"),
                Tone::Info,
            );
            return Ok(());
        }
        self.document.apply(EditCommand::one_or_batch(commands))
    }

    /// Puts the whole scene back in view of the pane, at the angle the camera
    /// is already looking from.
    fn frame_scene(&mut self) {
        let extent = self.panels.viewport_extent();
        let bounds = scene_bounds(&mut self.document);
        self.camera.frame(bounds, extent.0 as f32 / extent.1 as f32);
    }

    /// Writes the document's title into the window, if it has changed.
    fn update_title(&mut self) {
        let title = self.document.title();
        if title == self.title {
            return;
        }
        if let Err(error) = self.shell.set_title(self.window, &title) {
            crcbl::log::warn!("editor: the window would not take a title: {error}");
        }
        self.title = title;
    }

    /// Records and submits one frame.
    fn draw(&mut self) -> Result<FrameOutcome, EditorError> {
        let Some(acquired) = self.gpu.acquire()? else {
            return Ok(FrameOutcome::Reconfigured);
        };
        let extent = acquired.extent;

        self.shelve()?;
        self.instances
            .update(&mut self.renderer, &mut self.document, &self.shelf)
            .map_err(|error| GpuError::pools("the editor's entities", &error))?;
        for (corners, color) in selection_boxes(&mut self.document) {
            self.renderer.debug_draw().box_edges(&corners, color);
        }

        // The scene is drawn at the pane's extent — as the panels last laid it
        // out, which is the rectangle this frame's draw list samples it into.
        let viewport = self.panels.viewport_extent();
        let camera = self.camera.camera();
        self.renderer
            .begin_frame(self.gpu.device(), &camera, &sun(), viewport)
            .map_err(GpuError::Hal)?;
        // 1.0, because every size in the draw list is already this frame's
        // pixels: the tree was laid out against `extent` and a second
        // multiplier is a second thing that can disagree with it.
        self.ui
            .begin_frame(
                self.gpu.device(),
                self.panels.draw_list(),
                self.panels.atlas(),
                1.0,
            )
            .map_err(GpuError::Hal)?;

        let format = self.gpu.format();
        let compiled = {
            let mut graph = RenderGraph::new(self.gpu.queue());
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, format, extent),
            );
            // A transient, so a pane that changes size is a target of the new
            // size on the next frame and the old one is retired by the pool
            // once no frame asks for it.
            let scene = graph.create_image(
                "editor viewport",
                TransientImageDesc::new(
                    viewport,
                    format,
                    ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
                ),
            );
            let _hdr = self
                .renderer
                .add_passes(&mut graph, &self.pool, scene, viewport);
            // Nothing else covers the whole window any more: the panels and
            // the pane do, and whatever the dock leaves between them is this.
            graph
                .add_render_pass("editor background")
                .clear_color(target, BACKGROUND)
                .execute(|_| {});
            // The panels, and the pane sampling the scene the passes above
            // drew — declared as a read, so the graph puts the barrier
            // between the two.
            self.ui.add_passes_with_textures(
                &mut graph,
                target,
                extent,
                &[UiTexture {
                    id: VIEWPORT_TEXTURE,
                    image: scene,
                }],
            );
            graph.compile(&self.pool).map_err(GpuError::Graph)?
        };

        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("editor frame"),
                queue: self.gpu.queue(),
            });
        compiled
            .execute(self.gpu.device(), &mut self.pool, encoder.as_mut(), None)
            .map_err(GpuError::Graph)?;
        let command_buffer = encoder.finish().map_err(GpuError::Hal)?;
        let outcome = self.gpu.submit_and_present(&acquired, command_buffer)?;
        self.pool.retire_unused(self.gpu.device());
        self.drawn_viewport = Some(viewport);
        Ok(outcome)
    }

    /// Rebuilds the renderer around the shelf it has and every asset the
    /// document's meshes now name, when they name one it lacks — see `meshes`
    /// — or around those assets alone when a rebuild is due because the
    /// document was replaced.
    ///
    /// The rebuild is [`rebuild`](Self::rebuild)'s.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if the device would not drain, or refused the new
    /// renderer, its grid or an instance.
    fn shelve(&mut self) -> Result<(), EditorError> {
        let seen = (self.document.membership(), self.document.measures());
        if std::mem::take(&mut self.rebuild_due) {
            self.shelved = seen;
            let wanted = self.document.mesh_assets();
            return self.rebuild(&wanted);
        }
        if seen == self.shelved {
            return Ok(());
        }
        self.shelved = seen;
        let wanted = self.document.mesh_assets();
        if self.shelf.holds(&wanted) {
            return Ok(());
        }
        let assets = self.shelf.assets().union(&wanted).cloned().collect();
        self.rebuild(&assets)
    }

    /// Puts a renderer holding `assets`, read through the document's asset
    /// source, in place of the one drawing now, with every entity of the
    /// document placed in it.
    ///
    /// The device is drained first, because a renderer is destroyed idle; the
    /// old renderer stays in place if the new one is refused.
    ///
    /// # Errors
    ///
    /// As [`shelve`](Self::shelve).
    fn rebuild(&mut self, assets: &std::collections::BTreeSet<String>) -> Result<(), EditorError> {
        let (shelf, scene) = Shelf::build(self.document.assets(), assets);
        let (renderer, instances) = renderer_for(
            &self.gpu,
            &scene,
            self.grid_extent,
            &mut self.document,
            &shelf,
        )?;
        self.gpu.drain()?;
        let previous = std::mem::replace(&mut self.renderer, renderer);
        previous.destroy(self.gpu.device());
        self.instances = instances;
        self.shelf = shelf;
        Ok(())
    }

    /// Writes the dock layout back, if this run moved it.
    ///
    /// **A run that moved nothing writes nothing**, so a person who only looked
    /// at a scene does not get their settings file rewritten; and a headless
    /// run writes nothing whatever it moved, because
    /// [`SettingsSource::for_run`] hands one no file to write to. A refusal is
    /// a warning rather than a failed exit: nobody pressed Save, and an editor
    /// that would not close because `~/.config` is read-only would be worse
    /// than one that forgets where its panels were.
    fn save_layout(&mut self) {
        if self.panels.layout() == &self.opened_layout {
            return;
        }
        if let Err(error) = layout::store(&mut self.settings, self.panels.layout()) {
            crcbl::log::warn!("editor: the layout could not be recorded: {error}");
            return;
        }
        match self.settings_source.save(layout::APP_NAME, &self.settings) {
            Ok(true) => crcbl::log::info!("editor: the panel layout was saved"),
            Ok(false) => {}
            Err(error) => crcbl::log::warn!("editor: the layout could not be saved: {error}"),
        }
    }

    /// Releases everything, in dependency order.
    ///
    /// # Errors
    ///
    /// [`EditorError`] if any of it refused. All of it is attempted regardless,
    /// because a leaked device is worse than a lost error — and the windowed
    /// harness fails a run that destroys a device with objects still alive.
    pub fn finish(mut self, exit: ExitReason) -> Result<Summary, EditorError> {
        let summary = Summary {
            run: RunSummary {
                backend: self.shell.backend(),
                frames: self.budget.presented(),
                ticks: self.ticks,
                events: self.events,
                extent: self.gpu.extent(),
                exit,
                // Running only while play mode plays; see the module docs.
                paused: self.document.play_state() != PlayState::Playing,
                mode: self.mode.mode_at_exit(&*self.shell, self.window),
            },
            entities: self.document.entity_count(),
            commands: self.document.log().position(),
        };
        self.save_layout();
        let gpu_result = self.gpu.drain().and_then(|()| {
            self.pool.destroy(self.gpu.device());
            self.renderer.destroy(self.gpu.device());
            self.ui.destroy(self.gpu.device());
            self.gpu.destroy()
        });
        let shell_result = if exit.window_survives() {
            self.shell.destroy_window(self.window)
        } else {
            Ok(())
        };
        gpu_result?;
        shell_result?;
        Ok(summary)
    }
}

/// What the status line puts before the reason a playing game turned a
/// command down.
const REFUSED: &str = "Refused: ";

/// Between two refusals of one frame on the status line.
const REFUSAL_SEPARATOR: &str = "; ";

/// What the status line says for the refusals one frame's ticks brought, in
/// the order the game made them: each reason after [`REFUSED`], so two
/// commands turned down in one frame are both told rather than the older
/// dropped.
fn refused_status(refusals: &[String]) -> String {
    format!("{REFUSED}{}", refusals.join(REFUSAL_SEPARATOR))
}

/// What the status line says once play mode has stopped.
const STOPPED: &str = "Stopped: the scene is back as it was when play began";

/// What the status line says when a rename is asked for with nothing selected.
const RENAME_NOTHING: &str = "Rename: select an entity to name it";

/// What the status line says once play mode is paused.
const PAUSED: &str = "Paused: F6 resumes, F5 stops and puts the scene back";

/// What the status line says when pause is asked for while editing.
const NOT_PLAYING: &str = "Not playing: F5 starts play mode";

/// The app id the window system matches this tool to its `.desktop` file by.
const APP_ID: &str = "sh.kryptic.crcbl.editor";

/// What the window is cleared to under the panels: the panels' own
/// background, so a gap the dock leaves reads as part of them.
const BACKGROUND: [f32; 4] = [0.078, 0.09, 0.114, 1.0];

/// The colour the primary selected entity's box is drawn in: a warm amber,
/// which is legible against the greybox grey in both the lit and the shadowed
/// half.
const PRIMARY_COLOR: [f32; 4] = [1.0, 0.72, 0.2, 1.0];

/// The colour every other selected entity's box is drawn in: a pale blue,
/// legible against the same grey and never mistaken for the primary's amber.
const SELECTED_COLOR: [f32; 4] = [0.55, 0.78, 1.0, 1.0];

/// The mode this tool asks for. It never asks for anything else.
pub const DISPLAY_MODE: DisplayMode = DisplayMode::Windowed;

/// A renderer of `scene` with the ground grid reaching `grid_extent` and every
/// entity of `document` placed in it, drawn from `shelf`.
///
/// # Errors
///
/// [`EditorError`] if the device refused the renderer, the grid or an
/// instance — the renderer released first, since it has no `Drop` to do it.
fn renderer_for(
    gpu: &GpuContext,
    scene: &crcbl::render::scene::SceneDesc<'_>,
    grid_extent: f32,
    document: &mut Document,
    shelf: &Shelf,
) -> Result<(ForwardRenderer, Placed), EditorError> {
    let mut renderer = ForwardRenderer::with_scene(gpu.device(), gpu.queue(), gpu.format(), scene)
        .map_err(GpuError::Hal)?;
    if let Err(error) =
        renderer.set_ground_grid(gpu.device(), Some(GridStyle::for_extent(grid_extent)))
    {
        renderer.destroy(gpu.device());
        return Err(GpuError::Hal(error).into());
    }
    match Placed::place(&mut renderer, document, shelf) {
        Ok(instances) => Ok((renderer, instances)),
        Err(error) => {
            renderer.destroy(gpu.device());
            Err(GpuError::pools("the editor's entities", &error).into())
        }
    }
}

/// How far the ground grid reaches around a scene whose box is `bounds`.
fn grid_extent(bounds: &Aabb) -> f32 {
    bounds.half_extent().length().max(1.0) * GRID_MARGIN
}

/// The light the scene is shaded by. The engine's own default: an editor is not
/// a place to art-direct, and a scene lit from somewhere surprising reads as a
/// bug in the scene.
fn sun() -> DirectionalLight {
    DirectionalLight::default()
}

/// Opens what the command line named, or the compiled-in scene, reading its
/// meshes from the asset root the command line named, if it named one.
///
/// Both through [`crate::scene::vocabulary`], which is the components **this**
/// build knows: a directory whose manifest names a system it does not is refused
/// by that system's name rather than opened with the chunk missing.
fn open_document(options: &Options) -> Result<Document, EditorError> {
    let document = match &options.scene {
        Some(path) => Document::open_dir(path.clone(), crate::scene::vocabulary()),
        None => Document::built_in(),
    };
    let mut document = document.map_err(LoopError::Game)?;
    if let Some(root) = &options.assets {
        document.set_assets(Box::new(crcbl::assets::DirSource::at(root.clone())));
    }
    Ok(document)
}

/// Says in the log what was opened: the scene's name and how many entities it
/// holds.
///
/// The outliner draws the rest, so this is what a *run's log* needs rather than
/// what a person needs — a headless run has no panel to read, and its log is
/// the only place the scene is named.
fn log_outline(document: &mut Document) {
    let systems = document.outline().len();
    crcbl::log::info!(
        "editor: {} — {} entities in {systems} systems",
        document.name(),
        document.entity_count(),
    );
}

/// How a click on an outliner row changes the selection, from the modifiers
/// held: the convention every file manager shares, which is what
/// [`SelectMode`]'s own docs ask a caller to map.
fn select_mode(modifiers: Modifiers) -> SelectMode {
    if modifiers.contains(Modifiers::SHIFT) {
        SelectMode::Range
    } else if modifiers.contains(Modifiers::CTRL) {
        SelectMode::Toggle
    } else {
        SelectMode::Replace
    }
}

/// This batch's wheel as pixels of panel scrolling, positive downwards.
///
/// A detent is [`WHEEL_LINE_PIXELS`]; a high-resolution scroll is already
/// pixels. **Negated**, because a wheel's `+Y` scrolls back towards the start
/// of what is being scrolled — which is what `crcbl::debug_console`'s own wheel
/// does with the same sign, handing it to a `scroll_by` whose positive
/// direction is "back through the log" — while a scroll offset grows the other
/// way.
fn wheel_pixels(pending: &Pending) -> f32 {
    pending
        .scrolls
        .iter()
        .map(|scroll| match *scroll {
            ScrollDelta::Lines { y, .. } => -y * WHEEL_LINE_PIXELS,
            #[allow(clippy::cast_possible_truncation)]
            ScrollDelta::Pixels { y, .. } => -(y as f32),
        })
        .sum()
}

/// Every selected entity's box, as its eight corners in render space, and the
/// colour it is outlined in: [`PRIMARY_COLOR`] for the primary and
/// [`SELECTED_COLOR`] for the rest. An entity nothing places has no box.
///
/// The box itself, turned as it is drawn — not the world-axis box around it,
/// which would outline a turned block loosely.
fn selection_boxes(document: &mut Document) -> Vec<([Vec3; 8], [f32; 4])> {
    let primary = document.primary();
    let mut boxes = Vec::with_capacity(document.selection().len());
    for id in document.selection().to_vec() {
        let Some(placement) = document.placement(id) else {
            continue;
        };
        let color = if Some(id) == primary {
            PRIMARY_COLOR
        } else {
            SELECTED_COLOR
        };
        boxes.push((placement.corners().map(|corner| corner.as_vec3()), color));
    }
    boxes
}

/// The box around everything in the document, or for an empty one a unit
/// box standing on the ground at the origin.
///
/// Standing on the ground rather than about the origin: the view is framed
/// level with the box's centre, so a box about the origin would put the eye
/// on the ground plane itself, where no ray meets the ground in front of it
/// and a mesh dragged into a new scene would have nowhere to land.
fn scene_bounds(document: &mut Document) -> Aabb {
    let ids: Vec<SceneEntityId> = document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect();
    let corners = ids.into_iter().filter_map(|id| document.bounds(id));
    let mut bounds: Option<Aabb> = None;
    for (min, max) in corners {
        bounds = Some(match bounds {
            Some(box_) => Aabb {
                min: box_.min.min(min),
                max: box_.max.max(max),
            },
            None => Aabb { min, max },
        });
    }
    bounds.unwrap_or(Aabb {
        min: Vec3::new(-0.5, 0.0, -0.5),
        max: Vec3::new(0.5, 1.0, 0.5),
    })
}

/// Runs until something stops it.
///
/// # Errors
///
/// [`EditorError`] from the frame that failed, or from teardown.
pub fn run(options: &Options) -> Result<Summary, EditorError> {
    let mut editor = Editor::start(options)?;
    let outcome = loop {
        match editor.frame() {
            Ok(Flow::Continue) => {}
            Ok(Flow::Stop(reason)) => break Ok(reason),
            Err(error) => break Err(error),
        }
    };
    match outcome {
        Ok(reason) => editor.finish(reason),
        Err(error) => {
            // A failed frame ends the run with nothing asked.
            editor.recover_unsaved();
            if let Err(teardown) = editor.finish(ExitReason::Failed) {
                crcbl::log::error!("teardown after a failed frame also failed: {teardown}");
            }
            Err(error)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
