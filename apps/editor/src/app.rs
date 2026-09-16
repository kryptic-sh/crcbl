//! The window, the device, the camera and the loop that drives the
//! [`Document`].
//!
//! **A hand-written loop, like `apps/bare`'s**, rather than the engine's
//! [`GameLoop`](crcbl::engine::GameLoop): the editor has no simulation to tick,
//! no menu, no pause and no HUD, so what the hosted loop would bring is a
//! schedule for things this slice does not have. `docs/plan/08-editor.md`'s
//! edit-mode schedule is the missing piece that makes that a switch instead of
//! an omission, and until it lands the honest shape is the one that says in as
//! many words that nothing ticks.
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
//! One [`crcbl::greybox::GREYBOX_CUBE`] per entity, scaled to its own extents,
//! over [`ForwardRenderer::set_ground_grid`]'s screen-space floor. The
//! selection is a [`DebugDraw`](crcbl::render::debug_draw::DebugDraw) box, and
//! the layer is forced on at start-up: `r_debug_draw` is off by default, so an
//! editor that did not switch it on would draw no selection and report nothing
//! wrong.
//!
//! The panels are [`crate::panel`]'s, composited over that picture by
//! [`UiRenderer`] in the same graph. **The scene is drawn over the whole
//! window** and the viewport pane is the hole the panels leave in it — that
//! module's docs say why it cannot yet be anything else — so the ray a click
//! becomes is cast through the whole window, and a click outside the pane's own
//! rectangle picks nothing.
//!
//! # The keyboard is not read here
//!
//! [`crate::keys`] holds it, as an [`ActionMap`] whose reserved `ui` and `text`
//! contexts are pushed from what the panels say about themselves — so the
//! arrows nudge the selection until an outliner row takes focus, and nothing
//! here fires while a field is being typed into.

use std::time::Duration;

use crcbl::core::input::{Modifiers, PointerButton, ScrollDelta};
use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, FrameOutcome, GpuContext, GpuContextDesc, GpuError,
    Handled, LoopError, ModeRequest, Pending, PointerCapture, RunSummary, SettingsSource,
    WINDOWED_IDLE, accept_close, open_window, wait_for_configure,
};
use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY, scene3d};
use crcbl::hal::CommandEncoderDesc;
use crcbl::input::ActionMap;
use crcbl::math::{Mat4, Quat, Vec2, Vec3};
use crcbl::reflect::Value;
use crcbl::render::grid::GridStyle;
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{
    Aabb, DirectionalLight, ForwardRenderer, InstanceHandle, OrbitCamera, Projection, RenderGraph,
    TransientPool, UiRenderer,
};
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::{ButtonState, DisplayMode, Shell, ShellEvent, WindowDesc, WindowId, open};
use crcbl::store::settings::SettingsStack;
use crcbl::text_input::TextPump;
use crcbl::ui::tree::{DockLayout, SelectMode};

use crate::args::Options;
use crate::command::EditCommand;
use crate::document::{Document, EditError};
use crate::keys::Action;
use crate::layout;
use crate::panel::{PanelInput, Panels};

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
    /// One instance per entity, in [`Document::outline`]'s order, rewritten
    /// every frame — `apps/towers` does the same and for the same reason: a
    /// renderer told only about edges needs a second copy of the state to
    /// compare against.
    instances: Vec<(SceneEntityId, InstanceHandle)>,
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
    camera: OrbitCamera,
    /// Which drag, if any, the pointer is in the middle of.
    drag: Option<Drag>,
    clock_source: Clock,
    budget: FrameBudget,
    events: u64,
    windowed: bool,
    /// What the window title was last set to, so it is only written when it
    /// changes — a title set every frame is a round trip to the window system
    /// for nothing.
    title: String,
    mode: ModeRequest,
}

/// What a held pointer button is doing to the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    /// Right button: turn the camera around the pivot.
    Orbit,
    /// Middle button: slide the pivot across the view plane.
    Pan,
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
        let mut renderer =
            ForwardRenderer::with_scene(gpu.device(), gpu.queue(), gpu.format(), &scene3d())
                .map_err(GpuError::Hal)?;

        // Rolled back by hand from here on: this type has no `Drop`, so a `?`
        // past this point would leak the renderer's pipelines rather than
        // release them — `apps/towers` carries the same note.
        let bounds = scene_bounds(&mut document);
        let extent = bounds.half_extent().length().max(1.0) * GRID_MARGIN;
        if let Err(error) =
            renderer.set_ground_grid(gpu.device(), Some(GridStyle::for_extent(extent)))
        {
            renderer.destroy(gpu.device());
            return Err(GpuError::Hal(error).into());
        }
        let instances = match place(&mut renderer, &mut document) {
            Ok(instances) => instances,
            Err(error) => {
                renderer.destroy(gpu.device());
                return Err(GpuError::pools("the editor's entities", &error).into());
            }
        };
        let ui = match UiRenderer::new(gpu.device(), gpu.queue(), gpu.format()) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(gpu.device());
                return Err(GpuError::Hal(error).into());
            }
        };

        let panels = Panels::new(&mut document, dock.clone(), gpu.extent());
        let title = document.title();
        Ok(Self {
            windowed: !options.common.headless,
            shell,
            window,
            gpu,
            renderer,
            pool: TransientPool::new(),
            instances,
            document,
            panels,
            ui,
            actions: crate::keys::map(),
            text_pump: TextPump::new(),
            pointer_state: PointerCapture::new(),
            modifiers: Modifiers::empty(),
            settings,
            settings_source,
            opened_layout: dock,
            elapsed: Duration::ZERO,
            camera: OrbitCamera::new(bounds.center(), 1.0, Projection::default()),
            drag: None,
            clock_source,
            budget: FrameBudget::new(options.common.frame_budget()),
            events,
            title,
            mode: ModeRequest::new(),
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
        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
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
            modifiers,
            ..
        } = self;
        shell.pump(&mut |event| {
            if pending.observe(&event) == Handled::Game {
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
            }
        });
        self.events += pending.count;
        self.mode.check(&*self.shell, self.window);

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            accept_close(self.shell.as_mut(), self.window)?;
            return Ok(Flow::Stop(ExitReason::CloseRequested));
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
        // a press.
        let in_viewport = pending
            .pointer
            .is_some_and(|at| self.panels.in_viewport(at));
        self.drive_camera(&pending, in_viewport);
        if pending.pointer_pressed && in_viewport {
            self.panels.release_keyboard();
            self.pick(&pending);
        }

        let asked = crate::keys::actions(&self.actions, self.modifiers, editing);
        let pointer = self.pointer_state.resolve(&pending);
        let input = PanelInput {
            pointer,
            nav: crcbl::nav::nav_input(&self.actions),
            text: self.text_pump.frame(dt),
            extent: self.extent(),
            select: select_mode(self.modifiers),
            scroll: if in_viewport {
                0.0
            } else {
                wheel_pixels(&pending)
            },
        };
        self.panels.frame(&mut self.document, input);

        let requests = self.panels.take_clipboard_requests();
        self.text_pump
            .serve(requests, self.shell.as_mut(), self.window);
        self.sync_contexts();

        for action in asked {
            self.act(&action);
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

        if let (Some(drag), Some(motion)) = (self.drag, pending.motion) {
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
                    let height = self.extent().1.max(1) as f32;
                    self.camera.pan(-motion.x / height, motion.y / height);
                }
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

    /// Selects whatever the left button landed on.
    ///
    /// Called only for a press inside the viewport pane — see
    /// [`Editor::frame`]. The ray is cast through the **whole window** because
    /// that is what the scene was drawn through: the pane is a hole in the
    /// panels rather than a view of its own, so an unprojection against the
    /// pane's own extent would use a different matrix from the picture.
    fn pick(&mut self, pending: &Pending) {
        let Some(at) = pending.pointer else {
            return;
        };
        let extent = self.extent();
        if extent.0 == 0 || extent.1 == 0 {
            return;
        }
        // Half a pixel, because `Camera::ray_through` takes a pixel's top-left
        // corner and a click is about the pixel's middle.
        let ray = self
            .camera
            .camera()
            .ray_through(at + Vec2::splat(0.5), extent);
        let hit = self.document.pick_ray(&ray);
        self.document.select(hit);
    }

    /// Carries out one keyboard action.
    ///
    /// A refusal is logged rather than propagated: nudging with nothing
    /// selected, or saving a document with no directory, are things a person
    /// does and then does differently — not conditions that should end the run.
    fn act(&mut self, action: &Action) {
        let outcome = match action {
            Action::Nudge { axis, sign } => self.nudge(*axis, sign * NUDGE_M),
            Action::Undo => self.document.undo().map(|_| ()),
            Action::Redo => self.document.redo().map(|_| ()),
            Action::Save => self.document.save(),
            Action::Frame => {
                self.frame_scene();
                Ok(())
            }
        };
        if let Err(error) = outcome {
            crcbl::log::warn!("editor: {error}");
        }
    }

    /// Builds and applies the [`EditCommand`] one arrow key means.
    ///
    /// **The command is built from what the field currently holds**, read back
    /// through the same dotted path it will be written through — so a nudge is
    /// relative without the command being relative, which is what keeps an
    /// inverse exact.
    fn nudge(&mut self, axis: usize, delta: f64) -> Result<(), EditError> {
        let Some(entity) = self.document.selected() else {
            crcbl::log::info!("editor: nothing is selected");
            return Ok(());
        };
        let path = format!("position.{axis}");
        let Value::Float(was) = self.document.read(entity, &path)? else {
            crcbl::log::warn!("editor: {path} is not a number");
            return Ok(());
        };
        self.document.apply(EditCommand::SetProperty {
            entity,
            path,
            value: Value::Float(was + delta),
        })
    }

    /// Puts the whole scene back in view, at the angle the camera is already
    /// looking from.
    fn frame_scene(&mut self) {
        let extent = self.extent();
        if extent.0 == 0 || extent.1 == 0 {
            return;
        }
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

        for index in 0..self.instances.len() {
            let (id, handle) = self.instances[index];
            if let Some(desc) = instance_of(&mut self.document, id) {
                self.renderer.set_instance(handle, &desc);
            }
        }
        if let Some(id) = self.document.selected()
            && let Some((min, max)) = self.document.bounds(id)
        {
            self.renderer.debug_draw().aabb(min, max, SELECTION_COLOR);
        }

        let camera = self.camera.camera();
        self.renderer
            .begin_frame(self.gpu.device(), &camera, &sun(), extent)
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
            let _hdr = self
                .renderer
                .add_passes(&mut graph, &self.pool, target, extent);
            // The panels, over the scene the pass above just tonemapped onto
            // the swapchain.
            self.ui.add_passes(&mut graph, target, extent);
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
        Ok(outcome)
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
                ticks: 0,
                events: self.events,
                extent: self.gpu.extent(),
                exit,
                // Nothing here ticks, so the simulation is never running; see
                // the module docs.
                paused: true,
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

/// The app id the window system matches this tool to its `.desktop` file by.
const APP_ID: &str = "sh.kryptic.crcbl.editor";

/// The colour the selection's bounds are drawn in: a warm amber, which is
/// legible against the greybox grey in both the lit and the shadowed half.
const SELECTION_COLOR: [f32; 4] = [1.0, 0.72, 0.2, 1.0];

/// The mode this tool asks for. It never asks for anything else.
pub const DISPLAY_MODE: DisplayMode = DisplayMode::Windowed;

/// The light the scene is shaded by. The engine's own default: an editor is not
/// a place to art-direct, and a scene lit from somewhere surprising reads as a
/// bug in the scene.
fn sun() -> DirectionalLight {
    DirectionalLight::default()
}

/// Opens what the command line named, or the compiled-in scene.
///
/// Both through [`crate::scene::vocabulary`], which is the components **this**
/// build knows: a directory whose manifest names a system it does not is refused
/// by that system's name rather than opened with the chunk missing.
fn open_document(options: &Options) -> Result<Document, EditorError> {
    let document = match &options.scene {
        Some(path) => Document::open_dir(path.clone(), crate::scene::vocabulary()),
        None => Document::built_in(),
    };
    document.map_err(LoopError::Game)
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

/// The box around everything in the document, or a unit box for an empty one.
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
        min: Vec3::splat(-0.5),
        max: Vec3::splat(0.5),
    })
}

/// How one entity is drawn: the unit cube, scaled to its own extents.
fn instance_of(document: &mut Document, id: SceneEntityId) -> Option<InstanceDesc> {
    let (min, max) = document.bounds(id)?;
    Some(InstanceDesc {
        mesh: GREYBOX_CUBE,
        material: GREYBOX_GREY,
        transform: Mat4::from_scale_rotation_translation(
            max - min,
            Quat::IDENTITY,
            (min + max) * 0.5,
        ),
    })
}

/// Places one instance per entity, in the document's own order.
fn place(
    renderer: &mut ForwardRenderer,
    document: &mut Document,
) -> Result<Vec<(SceneEntityId, InstanceHandle)>, crcbl::render::instance_pool::InstancePoolError> {
    let ids: Vec<SceneEntityId> = document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect();
    let mut placed = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(desc) = instance_of(document, id) else {
            continue;
        };
        placed.push((id, renderer.add_instance(&desc)?));
    }
    Ok(placed)
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
mod tests {
    use super::*;

    use std::path::Path;

    use crcbl::core::input::KeyCode;
    use crcbl::shell::{HeadlessShell, PhysicalPoint};
    use crcbl::ui::tree::NodeKey;

    fn options(frames: u64) -> Options {
        let mut common = crcbl::args::Common::new(crate::args::DEFAULT_TICK_HZ);
        common.headless = true;
        common.frames = Some(frames);
        common.backend = Some(crcbl::backend::GpuBackend::Null);
        Options {
            common,
            scene: None,
        }
    }

    /// An editor on a shell a test can inject into.
    ///
    /// `Editor::start` opens its own shell and hands back no handle to it;
    /// [`Editor::with_shell`] takes one, and the concrete type is what a
    /// scripted click needs — so the whole loop runs against the events a
    /// window system would have delivered rather than against a stand-in.
    fn headless(frames: u64) -> Editor<HeadlessShell> {
        Editor::with_shell(Box::new(HeadlessShell::new()), &options(frames))
            .expect("the null backend runs everywhere")
    }

    impl<S: Shell + ?Sized> Editor<S> {
        /// The shell, for a test to inject into.
        fn shell_mut(&mut self) -> &mut S {
            self.shell.as_mut()
        }
    }

    /// The middle of a laid-out node, as a whole pixel.
    fn centre(editor: &Editor<HeadlessShell>, key: NodeKey) -> PhysicalPoint {
        let (min, max) = editor
            .panels
            .ui()
            .rect(key)
            .expect("the node was laid out last frame");
        let at = (min + max) * 0.5;
        PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        }
    }

    /// Clicks at `at` — a press frame and a release frame — through the shell.
    fn click(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint) {
        let window = editor.window;
        let shell = editor.shell_mut();
        shell.move_pointer(window, at, (0.0, 0.0)).expect("live");
        shell
            .button(window, PointerButton::Left, ButtonState::Pressed, Some(at))
            .expect("live");
        editor.frame().expect("a frame");
        editor
            .shell_mut()
            .button(window, PointerButton::Left, ButtonState::Released, Some(at))
            .expect("live");
        editor.frame().expect("a frame");
        // And one more: the tree resolves the click when the *next* frame
        // begins, so what a click changed is drawn a frame after it.
        editor.frame().expect("a frame");
    }

    /// Presses and releases `key`, a frame each.
    fn tap(editor: &mut Editor<HeadlessShell>, key: KeyCode) {
        let window = editor.window;
        editor.shell_mut().key_press(window, key).expect("live");
        editor.frame().expect("a frame");
        editor.shell_mut().key_release(window, key).expect("live");
        editor.frame().expect("a frame");
    }

    /// **The whole loop runs against the null backend, presents its budget and
    /// tears down.** The end-to-end claim: a document opens, a device opens,
    /// frames are recorded, and nothing is left alive.
    #[test]
    fn a_headless_run_presents_its_budget_and_stops() {
        let summary = run(&options(6)).expect("the null backend runs everywhere");
        assert_eq!(summary.run.frames, 6);
        assert_eq!(summary.run.exit, ExitReason::FrameBudget);
        assert_eq!(
            summary.entities,
            Document::built_in()
                .expect("the compiled-in scene is a scene")
                .entity_count(),
            "the run opened a different document from the compiled-in one",
        );
        assert_eq!(summary.commands, 0, "nothing was edited");
    }

    /// **An edit made through the loop's own path reaches the document**, which
    /// is what says the keyboard is wired to the command enum rather than to a
    /// field.
    #[test]
    fn an_action_applied_through_the_loop_records_a_command() {
        let mut editor = Editor::start(&options(2)).expect("headless starts");
        editor.document_mut().select(Some(SceneEntityId(0)));
        let before = editor
            .document_mut()
            .read(SceneEntityId(0), "position.0")
            .expect("a brick has an x");

        editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
        assert_eq!(editor.document().log().position(), 1);
        assert!(editor.document().is_dirty());
        let Value::Float(was) = before else {
            panic!("a brick's x is a number, got {before:?}");
        };
        assert_eq!(
            editor
                .document_mut()
                .read(SceneEntityId(0), "position.0")
                .expect("a brick has an x"),
            Value::Float(was + NUDGE_M),
        );

        editor.act(&Action::Undo);
        assert!(!editor.document().is_dirty());
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Nudging with nothing selected changes nothing and records nothing — it
    /// is a thing a person does, not a failure of the run.
    #[test]
    fn a_nudge_with_nothing_selected_records_nothing() {
        let mut editor = Editor::start(&options(2)).expect("headless starts");
        assert_eq!(editor.document().selected(), None);
        editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
        assert!(editor.document().log().is_empty());
        assert!(!editor.document().is_dirty());
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Saving the compiled-in scene says there is nowhere to write rather than
    /// guessing one, and leaves the document dirty.
    #[test]
    fn saving_the_built_in_scene_is_refused_and_leaves_it_dirty() {
        let mut editor = Editor::start(&options(2)).expect("headless starts");
        editor.document_mut().select(Some(SceneEntityId(0)));
        editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
        editor.act(&Action::Save);
        assert!(
            editor.document().is_dirty(),
            "a refused save must not clear the marker",
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// The loop can be stepped by hand, which is what lets a test drive it a
    /// frame at a time.
    #[test]
    fn the_loop_can_be_stepped_one_frame_at_a_time() {
        let mut editor = Editor::start(&options(3)).expect("headless starts");
        for _ in 0..3 {
            assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
        }
        assert_eq!(
            editor.frame().expect("a frame"),
            Flow::Stop(ExitReason::FrameBudget),
        );
        let summary = editor.finish(ExitReason::FrameBudget).expect("teardown");
        assert_eq!(summary.run.frames, 3);
    }

    /// **A click in a panel picks nothing, and the same click in the viewport
    /// picks.** The claim the viewport pane's rectangle exists for: the ray is
    /// cast through the whole window, so without the gate a click on the
    /// outliner would select whatever the scene happens to have behind it.
    ///
    /// The two clicks are at the **same scene depth** — the pane's own
    /// rectangle is the only difference — and the panel click is aimed at the
    /// outliner, so a gate that quietly let it through would select something
    /// rather than nothing.
    #[test]
    fn a_click_in_a_panel_picks_nothing_and_one_in_the_viewport_picks() {
        let mut editor = headless(200);
        editor.frame().expect("a frame");

        // The window's own middle: the scene was framed on the whole window at
        // start-up, so that pixel looks at the middle of the scene — and it is
        // inside the viewport pane, which the panels leave to the right of the
        // side column.
        let extent = editor.extent();
        let middle = Vec2::new(extent.0 as f32, extent.1 as f32) * 0.5;
        assert!(
            editor.panels.in_viewport(middle),
            "the window's middle is not in the viewport pane: {:?}",
            editor.panels.viewport(),
        );
        click(
            &mut editor,
            PhysicalPoint {
                x: f64::from(middle.x),
                y: f64::from(middle.y),
            },
        );
        assert!(
            editor.document().selected().is_some(),
            "a click at the middle of the framed scene hit nothing, so the other \
             half of this test would pass vacuously",
        );

        // Inside the outliner, below its last row: a click on the panel that is
        // not a click on a row, which is the case a pick gate has to refuse.
        let outliner = editor
            .panels
            .outliner_key()
            .expect("the outliner was built");
        let (_, panel_max) = editor.panels.ui().rect(outliner).expect("laid out");
        let rows = editor.panels.row_keys();
        let last = editor
            .panels
            .ui()
            .rect(*rows.last().expect("the outliner has rows"))
            .expect("laid out");
        let below = Vec2::new(panel_max.x - 8.0, (last.1.y + panel_max.y) * 0.5);
        assert!(
            below.y > last.1.y && below.y < panel_max.y,
            "there is no empty room under the rows: {below:?} in {panel_max:?}",
        );
        assert!(
            !editor.panels.in_viewport(below),
            "the outliner is inside the viewport pane, so this proves nothing",
        );

        // **Close the camera until the scene reaches under that point.** A gate
        // that is never asked a real question is not a gate: with the scene
        // framed, a ray through the panel misses everything and a test here
        // would pass with the gate deleted.
        let probe = |editor: &mut Editor<HeadlessShell>, at: Vec2| {
            let extent = editor.extent();
            let ray = editor
                .camera
                .camera()
                .ray_through(at + Vec2::splat(0.5), extent);
            editor.document_mut().pick_ray(&ray)
        };
        // 21 steps of 0.1 leave the camera about four metres out, which is
        // still outside the block it is looking at.
        for _ in 0..21 {
            if probe(&mut editor, below).is_some() {
                break;
            }
            editor.camera.zoom(0.1);
        }
        assert!(
            probe(&mut editor, below).is_some(),
            "the scene never reached under the outliner, so the gate below is \
             not what stops the click",
        );

        editor.document_mut().select(None);
        click(
            &mut editor,
            PhysicalPoint {
                x: f64::from(below.x),
                y: f64::from(below.y),
            },
        );
        assert_eq!(
            editor.document().selected(),
            None,
            "a click in the outliner's own area picked an entity out of the scene",
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A click on an outliner row selects the entity the row names**, through
    /// the whole loop: a real pointer event, resolved by the tree against the
    /// rectangles it laid out.
    #[test]
    fn a_click_on_an_outliner_row_selects_the_entity_it_names() {
        let mut editor = headless(200);
        editor.frame().expect("a frame");
        let rows = editor.panels.row_keys();
        // Row 0 is the system's own header; row 1 is the first entity.
        assert!(rows.len() > 2, "the outliner built {} rows", rows.len());
        let at = centre(&editor, rows[1]);

        assert_eq!(editor.document().selected(), None);
        click(&mut editor, at);
        assert_eq!(
            editor.document().selected(),
            Some(SceneEntityId(0)),
            "the row for the first entity selected something else",
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **Typing into an inspector field does not nudge the selection**, and it
    /// does not save or undo either — the whole point of the reserved contexts.
    ///
    /// The document is `apps/puppet`'s blockout, because its `Surface` carries
    /// the only `String` in either sample's vocabulary and a text field is what
    /// a text context is about.
    #[test]
    fn typing_in_a_field_does_not_nudge_the_selection() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut source = Document::open(
            &crcbl_puppet::map::built_in_source(),
            Path::new(crcbl_puppet::map::BLOCKOUT),
            crate::scene::vocabulary(),
        )
        .expect("the committed blockout is a scene");
        source.save_to(dir.path()).expect("a writable directory");

        let mut options = options(400);
        options.scene = Some(dir.path().to_path_buf());
        let mut editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
            .expect("what we just wrote is a scene");
        editor.frame().expect("a frame");

        // The one component in either sample's vocabulary with a `String` in
        // it, found by asking rather than by counting rows: a system that
        // reordered, or a field that moved, must fail loudly here rather than
        // leave the test typing into a drag-value.
        let ids: Vec<SceneEntityId> = editor
            .document_mut()
            .outline()
            .into_iter()
            .flat_map(|(_, ids)| ids)
            .collect();
        let selected = ids
            .into_iter()
            .find(|id| {
                editor
                    .document_mut()
                    .component(*id)
                    .is_some_and(|component| component.type_name().ends_with("Surface"))
            })
            .expect("puppet's blockout holds a surface");
        editor.document_mut().select(Some(selected));
        editor.frame().expect("a frame");

        // Click into the `Label` field: the first inspector row, whose widget
        // is the node built straight after its label span.
        let props = editor.panels.props_key().expect("the inspector was built");
        let label_row = editor.panels.ui().child_keys(props)[0];
        let field = editor.panels.ui().child_keys(label_row)[1];
        let on_field = centre(&editor, field);
        click(&mut editor, on_field);
        assert!(
            editor.panels.text_editing(),
            "the first inspector row is not a text field, so this proves nothing",
        );

        let before = editor.document_mut().read(selected, "position.0").unwrap();
        let commands = editor.document().log().position();
        for key in [
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::PageUp,
            KeyCode::KeyF,
        ] {
            tap(&mut editor, key);
        }
        assert_eq!(
            editor.document_mut().read(selected, "position.0").unwrap(),
            before,
            "an arrow typed into a field moved the entity",
        );
        assert_eq!(
            editor.document().log().position(),
            commands,
            "typing into a field recorded a command of its own",
        );
        assert_eq!(
            editor.document().selected(),
            Some(selected),
            "typing moved the selection",
        );
        assert!(
            editor.panels.text_editing(),
            "the field stopped editing part-way, so the keys were not all typed",
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **The mode the tool documents is the one it asks the window system
    /// for.** Read off the options rather than compared with itself: a constant
    /// asserted against a constant says nothing about what
    /// [`Editor::with_shell`] passes to [`open_window`].
    #[test]
    fn the_tool_asks_for_the_mode_it_says_it_does() {
        assert_eq!(options(1).common.display_mode(), DISPLAY_MODE);
        let mut fullscreen = options(1);
        fullscreen.common.fullscreen = true;
        assert_ne!(
            fullscreen.common.display_mode(),
            DISPLAY_MODE,
            "--fullscreen must ask for something else, or the flag does nothing",
        );
    }
}
