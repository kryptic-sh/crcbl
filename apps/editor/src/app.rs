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
//! an [`EditCommand`], the command becomes [`Document::apply`]. Nothing in this
//! file writes a component field, which is the plan's "nothing editor-side may
//! be implemented GUI-only" kept by construction — the headless tests in
//! [`crate::document`] exercise the same calls with no device at all.
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
//! There is **no panel**. `docs/plan/07-ui-debug.md`'s rungs are what a viewport
//! pane, an outliner and a property inspector are made of, and another slice is
//! building them; the outline is logged at start-up and counted in the summary
//! instead, which is the honest amount of "lists the entities per system" a
//! slice with no UI can deliver.

use crcbl::core::input::{KeyCode, Modifiers, PointerButton, ScrollDelta};
use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, FrameOutcome, GpuContext, GpuContextDesc, GpuError,
    Handled, LoopError, ModeRequest, Pending, RunSummary, WINDOWED_IDLE, accept_close, open_window,
    wait_for_configure,
};
use crcbl::greybox::{GREYBOX_CUBE, GREYBOX_GREY, scene3d};
use crcbl::hal::CommandEncoderDesc;
use crcbl::math::{Mat4, Quat, Vec2, Vec3};
use crcbl::reflect::Value;
use crcbl::render::grid::GridStyle;
use crcbl::render::scene::InstanceDesc;
use crcbl::render::{
    Aabb, DirectionalLight, ForwardRenderer, InstanceHandle, OrbitCamera, Projection, RenderGraph,
    TransientPool,
};
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::{ButtonState, DisplayMode, Shell, ShellEvent, WindowDesc, WindowId, open};

use crate::args::Options;
use crate::command::EditCommand;
use crate::document::{Document, EditError};

/// How far one arrow key moves the selection, in metres.
///
/// A centimetre, which is `apps/breakout`'s `Brick` own `#[reflect(step)]` on
/// the half extents — the step that component says a drag should take. Holding
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

/// The window, the device, the document and the camera.
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
    camera: OrbitCamera,
    /// Which drag, if any, the pointer is in the middle of.
    drag: Option<Drag>,
    /// Where the pointer was left, in framebuffer pixels, or [`None`] while it
    /// is outside the window.
    ///
    /// Carried into the next batch through [`Pending::carrying`], which is what
    /// that constructor is for: a batch that starts from [`Default`] has nothing
    /// to difference a motion event against, so the first pixel of every drag
    /// would be dropped on a backend with no unaccelerated delta.
    pointer: Option<Vec2>,
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

/// One thing the keyboard asked for this batch.
///
/// Collected out of the pump and applied afterwards, because the pump's closure
/// already borrows the shell and applying a command borrows the document.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    /// Move the selection along an axis, in metres.
    Nudge {
        /// 0, 1 or 2 — the index the command's `position.N` path names.
        axis: usize,
        /// How far, signed.
        delta: f64,
    },
    /// Walk the undo log back one entry.
    Undo,
    /// Walk it forward one entry.
    Redo,
    /// Write the scene back over the directory it came from.
    Save,
    /// Put the whole scene back in view.
    Frame,
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

        let mut editor = Self::build(shell, window, gpu, document, options, events, clock_source)?;
        editor.frame_scene();
        Ok(editor)
    }

    /// The device-side half of [`with_shell`](Self::with_shell), split out so
    /// the rollback below is written once.
    fn build(
        shell: Box<S>,
        window: WindowId,
        gpu: GpuContext,
        mut document: Document,
        options: &Options,
        events: u64,
        clock_source: Clock,
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
            camera: OrbitCamera::new(bounds.center(), 1.0, Projection::default()),
            drag: None,
            pointer: None,
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

        let mut pending = Pending::carrying(self.pointer);
        let mut actions = Vec::new();
        self.shell.pump(&mut |event| {
            if pending.observe(&event) == Handled::Game {
                collect(&event, &mut actions);
            }
        });
        self.events += pending.count;
        self.pointer = pending.pointer;
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

        self.drive_camera(&pending);
        self.pick(&pending);
        for action in actions {
            self.act(&action);
        }
        self.update_title();

        // The clock is advanced although nothing ticks, because it is what
        // paces the loop — see `crate::args::DEFAULT_TICK_HZ`.
        let _ = self.clock_source.advance();

        let outcome = self.draw()?;
        self.budget.record(outcome)?;
        Ok(Flow::Continue)
    }

    /// Turns, slides and zooms the camera from this batch's pointer input.
    fn drive_camera(&mut self, pending: &Pending) {
        for (button, pressed) in &pending.buttons {
            let drag = match button {
                PointerButton::Right => Some(Drag::Orbit),
                PointerButton::Middle => Some(Drag::Pan),
                _ => None,
            };
            if let Some(drag) = drag {
                self.drag = pressed.then_some(drag);
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

        for scroll in &pending.scrolls {
            self.camera.zoom(match *scroll {
                ScrollDelta::Lines { y, .. } => y * ZOOM_PER_DETENT,
                #[allow(clippy::cast_possible_truncation)]
                ScrollDelta::Pixels { y, .. } => y as f32 * ZOOM_PER_PIXEL,
            });
        }
    }

    /// Selects whatever the left button landed on, if it landed this batch.
    fn pick(&mut self, pending: &Pending) {
        if !pending.pointer_pressed {
            return;
        }
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
            Action::Nudge { axis, delta } => self.nudge(*axis, *delta),
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
        let gpu_result = self.gpu.drain().and_then(|()| {
            self.pool.destroy(self.gpu.device());
            self.renderer.destroy(self.gpu.device());
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

/// Opens what the command line named, or the compiled-in board.
fn open_document(options: &Options) -> Result<Document, EditorError> {
    let document = match &options.scene {
        Some(path) => Document::open_dir(path.clone()),
        None => Document::built_in(),
    };
    document.map_err(LoopError::Game)
}

/// Writes the outline into the log: one line per system, with the ids it holds.
///
/// The log rather than a panel, because there is no panel — see the module
/// docs. It runs once, at start-up, so a run's log says what was opened.
fn log_outline(document: &mut Document) {
    crcbl::log::info!(
        "editor: {} — {} entities",
        document.name(),
        document.entity_count(),
    );
    for (system, ids) in document.outline() {
        crcbl::log::info!("editor:   {system}: {} entities", ids.len());
    }
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

/// Turns one shell event the loop did not claim into the action it means.
///
/// A free function rather than a method so that the pump's closure, which
/// already holds the shell, can call it.
///
/// **Presses only, and repeats included**: holding an arrow down is how a
/// coarse move is made, and a release that also nudged would double every tap.
fn collect(event: &ShellEvent, actions: &mut Vec<Action>) {
    let ShellEvent::Key {
        key_code: Some(code),
        state: ButtonState::Pressed,
        modifiers,
        ..
    } = event
    else {
        return;
    };
    if let Some(action) = action_for(*code, *modifiers) {
        actions.push(action);
    }
}

/// What one key press means, with the modifiers that were held.
///
/// Split out of [`collect`] because it is the whole of the binding table, and a
/// table is worth being able to read — and assert — on its own.
fn action_for(code: KeyCode, modifiers: Modifiers) -> Option<Action> {
    let ctrl = modifiers.contains(Modifiers::CTRL);
    let shift = modifiers.contains(Modifiers::SHIFT);
    Some(match code {
        // Ctrl is matched first, so a chord is never also a nudge.
        KeyCode::KeyZ if ctrl && shift => Action::Redo,
        KeyCode::KeyZ if ctrl => Action::Undo,
        KeyCode::KeyY if ctrl => Action::Redo,
        KeyCode::KeyS if ctrl => Action::Save,
        // Everything below is unmodified. Ctrl+arrow is a word jump everywhere
        // else and this editor claims no meaning for it.
        _ if ctrl => return None,
        KeyCode::ArrowLeft => Action::Nudge {
            axis: 0,
            delta: -NUDGE_M,
        },
        KeyCode::ArrowRight => Action::Nudge {
            axis: 0,
            delta: NUDGE_M,
        },
        KeyCode::ArrowUp => Action::Nudge {
            axis: 1,
            delta: NUDGE_M,
        },
        KeyCode::ArrowDown => Action::Nudge {
            axis: 1,
            delta: -NUDGE_M,
        },
        KeyCode::PageUp => Action::Nudge {
            axis: 2,
            delta: NUDGE_M,
        },
        KeyCode::PageDown => Action::Nudge {
            axis: 2,
            delta: -NUDGE_M,
        },
        KeyCode::KeyF => Action::Frame,
        _ => return None,
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

    use crcbl::shell::{HeadlessShell, ShellError};

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

    /// The actions a sequence of keystrokes produces, through a **real**
    /// backend: the headless shell builds the events, and [`collect`] reads
    /// exactly what a window system would have delivered.
    ///
    /// Hand-built `ShellEvent`s would test this file against itself; a key
    /// injected here carries the scancode, the keysym and the modifier state
    /// the seam actually stamps onto one.
    fn actions_of(keys: &[(KeyCode, Modifiers, ButtonState)]) -> Vec<Action> {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc {
                title: "editor test",
                app_id: APP_ID,
                ..WindowDesc::default()
            })
            .expect("the headless shell opens a window");
        for (code, modifiers, state) in keys {
            shell.set_modifiers(*modifiers);
            let pushed: Result<(), ShellError> = match state {
                ButtonState::Pressed => shell.key_press(window, *code),
                ButtonState::Released => shell.key_release(window, *code),
            };
            pushed.expect("the window is live");
        }
        let mut actions = Vec::new();
        shell.pump(&mut |event| collect(&event, &mut actions));
        actions
    }

    /// Every key in the table, pressed with no modifier held.
    fn pressed(keys: &[KeyCode]) -> Vec<Action> {
        let keys: Vec<_> = keys
            .iter()
            .map(|code| (*code, Modifiers::empty(), ButtonState::Pressed))
            .collect();
        actions_of(&keys)
    }

    /// **A modifier changes what a key means**, which is the whole of why
    /// `docs/plan/08-editor.md`'s smallest slice asked for modifier-carrying
    /// key events. Z alone is not undo; Ctrl+Z is; Ctrl+Shift+Z is redo.
    #[test]
    fn a_modifier_decides_what_a_key_means() {
        assert_eq!(pressed(&[KeyCode::KeyZ]), []);
        assert_eq!(
            actions_of(&[(KeyCode::KeyZ, Modifiers::CTRL, ButtonState::Pressed)]),
            [Action::Undo],
        );
        assert_eq!(
            actions_of(&[(
                KeyCode::KeyZ,
                Modifiers::CTRL | Modifiers::SHIFT,
                ButtonState::Pressed,
            )]),
            [Action::Redo],
        );
        assert_eq!(
            actions_of(&[(KeyCode::KeyY, Modifiers::CTRL, ButtonState::Pressed)]),
            [Action::Redo],
        );
        assert_eq!(
            actions_of(&[(KeyCode::KeyS, Modifiers::CTRL, ButtonState::Pressed)]),
            [Action::Save],
        );
        // A held Ctrl must not turn an arrow into a nudge.
        assert_eq!(
            actions_of(&[(KeyCode::ArrowLeft, Modifiers::CTRL, ButtonState::Pressed)]),
            [],
        );
    }

    /// Each arrow names the axis and the sign the help text promises.
    #[test]
    fn the_arrows_nudge_the_axes_the_help_text_names() {
        assert_eq!(
            pressed(&[
                KeyCode::ArrowRight,
                KeyCode::ArrowLeft,
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
                KeyCode::PageUp,
                KeyCode::PageDown,
            ]),
            [
                Action::Nudge {
                    axis: 0,
                    delta: NUDGE_M
                },
                Action::Nudge {
                    axis: 0,
                    delta: -NUDGE_M
                },
                Action::Nudge {
                    axis: 1,
                    delta: NUDGE_M
                },
                Action::Nudge {
                    axis: 1,
                    delta: -NUDGE_M
                },
                Action::Nudge {
                    axis: 2,
                    delta: NUDGE_M
                },
                Action::Nudge {
                    axis: 2,
                    delta: -NUDGE_M
                },
            ],
        );
    }

    /// A key release is not a key press: an editor that acted on both would
    /// nudge twice per tap.
    #[test]
    fn a_key_release_asks_for_nothing() {
        assert_eq!(
            actions_of(&[
                (KeyCode::ArrowLeft, Modifiers::empty(), ButtonState::Pressed),
                (
                    KeyCode::ArrowLeft,
                    Modifiers::empty(),
                    ButtonState::Released
                ),
            ]),
            [Action::Nudge {
                axis: 0,
                delta: -NUDGE_M
            }],
            "the release nudged as well",
        );
    }

    /// A key this editor has no meaning for asks for nothing, rather than
    /// falling through to whatever the last arm happened to be.
    #[test]
    fn an_unbound_key_asks_for_nothing() {
        assert_eq!(pressed(&[KeyCode::KeyQ, KeyCode::Space, KeyCode::Tab]), []);
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
            crcbl_breakout::Board::built_in().bricks().len(),
            "the run opened a different board from the one the game reads",
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

        editor.act(&Action::Nudge {
            axis: 0,
            delta: NUDGE_M,
        });
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
        editor.act(&Action::Nudge {
            axis: 0,
            delta: NUDGE_M,
        });
        assert!(editor.document().log().is_empty());
        assert!(!editor.document().is_dirty());
        editor.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// Saving the compiled-in board says there is nowhere to write rather than
    /// guessing one, and leaves the document dirty.
    #[test]
    fn saving_the_built_in_board_is_refused_and_leaves_it_dirty() {
        let mut editor = Editor::start(&options(2)).expect("headless starts");
        editor.document_mut().select(Some(SceneEntityId(0)));
        editor.act(&Action::Nudge {
            axis: 1,
            delta: NUDGE_M,
        });
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
