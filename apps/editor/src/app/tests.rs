use super::*;

use std::path::Path;

use crcbl::core::input::KeyCode;
use crcbl::engine::FrameLimit;
use crcbl::shell::{HeadlessShell, PhysicalPoint, PhysicalSize};
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
pub(super) fn headless(frames: u64) -> Editor<HeadlessShell> {
    Editor::with_shell(Box::new(HeadlessShell::new()), &options(frames))
        .expect("the null backend runs everywhere")
}

impl<S: Shell + ?Sized> Editor<S> {
    /// The shell, for a test to inject into.
    fn shell_mut(&mut self) -> &mut S {
        self.shell.as_mut()
    }
}

/// **A windowed frame idles only until the frame limiter's deadline.**
///
/// The defect this guards: [`Editor::frame`] used to hand a fixed
/// `WINDOWED_IDLE` to [`Shell::wait_events`] on every windowed frame, and
/// on Win32 and X11 only input ends that wait early — so an editor that
/// draws every frame paid all of it on top of each one. Observed through
/// the headless shell's count of waits, which that fixed idle raised once
/// a frame.
///
/// Built headless, then switched to windowed on a real clock, because the
/// limiter's deadline lives on a real clock alone — and because a
/// non-headless build would open the person's own settings. Two limited
/// frames: the first has no deadline behind it, the second has a whole
/// period ahead.
#[test]
fn a_windowed_frame_idles_only_until_the_frame_limiters_deadline() {
    for (limit, frames, waits) in [(FrameLimit::unlimited(), 5, 0), (FrameLimit::fps(50), 2, 1)] {
        let mut editor = headless(frames);
        editor.windowed = true;
        editor.clock_source = Clock::new(false);
        editor.clock_source.set_limit(limit);
        // Waiting for the window to configure already waited on the shell.
        let before = editor.shell_mut().wait_count();
        for _ in 0..frames {
            assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
        }
        assert_eq!(
            editor.shell_mut().wait_count() - before,
            waits,
            "{frames} windowed frames at {limit} waited on the shell a \
             different number of times than the limiter's deadlines allow",
        );
        editor.finish(ExitReason::FrameBudget).expect("teardown");
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

/// **Delete and duplicate reach the document through the loop, and a frame
/// draws whatever the scene now holds** — one entity fewer, then a copy
/// selected in its place, and back again on undo.
#[test]
fn delete_and_duplicate_through_the_loop_change_what_a_frame_draws() {
    let mut editor = Editor::start(&options(8)).expect("headless starts");
    let count = editor.document().entity_count();
    editor.document_mut().select(Some(SceneEntityId(2)));

    editor.act(&Action::Delete);
    assert_eq!(editor.document().entity_count(), count - 1);
    assert_eq!(editor.document().selected(), None);
    assert_eq!(editor.frame().expect("a frame"), Flow::Continue);

    editor.act(&Action::Undo);
    editor.document_mut().select(Some(SceneEntityId(2)));
    editor.act(&Action::Duplicate);
    assert_eq!(editor.document().entity_count(), count + 1);
    let copy = editor.document().selected().expect("the copy is selected");
    assert_ne!(copy, SceneEntityId(2));
    assert_eq!(editor.frame().expect("a frame"), Flow::Continue);

    editor.act(&Action::Undo);
    assert_eq!(editor.document().entity_count(), count);
    assert!(!editor.document().is_dirty());
    assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// A window pixel as the shell takes one.
fn physical(at: Vec2) -> PhysicalPoint {
    PhysicalPoint {
        x: f64::from(at.x),
        y: f64::from(at.y),
    }
}

/// **Dragging a gizmo handle moves the selection along that axis alone, as
/// one undo**, and a press on the handle does not re-pick whatever is
/// behind it.
#[test]
fn dragging_a_handle_moves_the_selection_along_its_axis_as_one_undo() {
    let mut editor = headless(16);
    let id = SceneEntityId(2);
    editor.document_mut().select(Some(id));
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    let read = |editor: &mut Editor<HeadlessShell>, axis: usize| {
        let Value::Float(value) = editor
            .document_mut()
            .read(id, &format!("position.{axis}"))
            .expect("a block")
        else {
            panic!("a position is a number");
        };
        value
    };
    let was = [
        read(&mut editor, 0),
        read(&mut editor, 1),
        read(&mut editor, 2),
    ];

    let handles = editor.handles();
    let x = handles
        .iter()
        .find(|handle| handle.axis == gizmo::Axis::X)
        .expect("the default view shows the X handle");
    let (corner, _) = editor.panels.viewport_pixels();
    let grab = corner + (x.from + x.to) * 0.5;
    let release = corner + x.to + (x.to - x.from) * 0.5;

    let window = editor.window;
    let shell = editor.shell_mut();
    shell
        .move_pointer(window, physical(grab), (0.0, 0.0))
        .expect("live");
    shell
        .button(
            window,
            PointerButton::Left,
            ButtonState::Pressed,
            Some(physical(grab)),
        )
        .expect("live");
    editor.frame().expect("a frame");
    for step in 1..=4 {
        let at = grab + (release - grab) * (step as f32 / 4.0);
        let delta = (release - grab) / 4.0;
        editor
            .shell_mut()
            .move_pointer(
                window,
                physical(at),
                (f64::from(delta.x), f64::from(delta.y)),
            )
            .expect("live");
        editor.frame().expect("a frame");
    }
    editor
        .shell_mut()
        .button(
            window,
            PointerButton::Left,
            ButtonState::Released,
            Some(physical(release)),
        )
        .expect("live");
    editor.frame().expect("a frame");

    assert_eq!(
        editor.document().selected(),
        Some(id),
        "the press re-picked"
    );
    let now = [
        read(&mut editor, 0),
        read(&mut editor, 1),
        read(&mut editor, 2),
    ];
    assert!(
        now[0] > was[0] + 0.1,
        "the X handle did not move it along +X: {now:?}"
    );
    assert_eq!([now[1], now[2]], [was[1], was[2]], "it moved off its axis");
    assert_eq!(editor.document().log().len(), 1, "a drag is one entry");

    editor.act(&Action::Undo);
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A copy goes out through the shell's clipboard and a paste comes back
/// through it** — the read answered on a later frame, as every backend's
/// is, and the pasted entity selected.
#[test]
fn a_copied_entity_pastes_back_through_the_clipboard() {
    let mut editor = Editor::start(&options(8)).expect("headless starts");
    let count = editor.document().entity_count();
    editor.document_mut().select(Some(SceneEntityId(2)));

    editor.act(&Action::Copy);
    editor.act(&Action::Paste);
    assert_eq!(
        editor.document().entity_count(),
        count,
        "the paste spawned before the clipboard answered",
    );
    for _ in 0..3 {
        assert_eq!(editor.frame().expect("a frame"), Flow::Continue);
    }
    assert_eq!(editor.document().entity_count(), count + 1);
    let pasted = editor.document().selected().expect("the paste is selected");
    assert_ne!(pasted, SceneEntityId(2));
    assert_eq!(
        editor
            .document_mut()
            .read(pasted, "position.1")
            .expect("pasted"),
        editor
            .document_mut()
            .read(SceneEntityId(2), "position.1")
            .expect("held"),
    );

    editor.act(&Action::Undo);
    assert_eq!(editor.document().entity_count(), count);
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
/// picks.** The claim the viewport pane's rectangle exists for: a click
/// outside it still unprojects to *some* ray through the pane's camera, so
/// without the gate a click on the outliner would select whatever that ray
/// happens to meet.
///
/// The two clicks are at the **same scene depth** — the pane's own
/// rectangle is the only difference — and the panel click is aimed at the
/// outliner, so a gate that quietly let it through would select something
/// rather than nothing.
#[test]
fn a_click_in_a_panel_picks_nothing_and_one_in_the_viewport_picks() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");

    // The pane's own middle: the scene was framed on the pane at start-up,
    // so that pixel looks at the middle of the scene.
    let (pane_min, pane_max) = editor.panels.viewport_pixels();
    let middle = ((pane_min + pane_max) * 0.5).floor();
    assert!(
        editor.panels.in_viewport(middle),
        "the pane's middle is not in the viewport pane: {:?}",
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
        let ray = editor.ray_at(at);
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

/// **A click in the pane picks through the pane's own camera**, measured
/// from the pane's corner and unprojected against the pane's extent —
/// which is the matrix the picture in it was drawn with.
///
/// The pane is offset from the window's corner by the side column, so the
/// whole-window unprojection the editor used when the scene was drawn under
/// a hole in the panels answers a different question. The click is aimed
/// at a pixel where the two answers **differ**, found by asking both, so a
/// pick that went back to the window's matrix selects the wrong entity
/// rather than passing by coincidence.
#[test]
fn a_click_in_the_offset_pane_picks_through_the_panes_own_camera() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");

    let (min, max) = editor.panels.viewport_pixels();
    assert!(
        min.x > 0.0,
        "the pane starts at the window's left edge, so an offset cannot be told from none"
    );
    let pane = editor.panels.viewport_extent();
    let window = editor.extent();
    let camera = editor.camera.camera();

    // A grid over the pane, and the first pixel whose pane-relative pick
    // hits something the whole-window pick does not.
    let mut aimed = None;
    'scan: for row in 1..16 {
        for column in 1..16 {
            let at = (min + (max - min) * Vec2::new(column as f32, row as f32) / 16.0).floor();
            let through_pane = camera.ray_through(at - min + Vec2::splat(0.5), pane);
            let through_window = camera.ray_through(at + Vec2::splat(0.5), window);
            let expected = editor.document_mut().pick_ray(&through_pane);
            let wrong = editor.document_mut().pick_ray(&through_window);
            if expected.is_some() && expected != wrong {
                aimed = Some((at, expected));
                break 'scan;
            }
        }
    }
    let (at, expected) =
        aimed.expect("no pixel of the pane tells the pane's camera from the window's");

    click(
        &mut editor,
        PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        },
    );
    assert_eq!(
        editor.document().selected(),
        expected,
        "a click at {at:?} in a pane at {min:?} picked through some other camera",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The scene is drawn at the pane's size, and a pane that changes size
/// gets a target of its new size.**
///
/// Read off what the frame did: the extent the scene was drawn at, the
/// rectangle the draw list samples it into, and the transient pool, which
/// allocates a target for the new size — and nothing on a steady frame.
#[test]
fn resizing_the_pane_reallocates_the_viewport_target() {
    let mut editor = headless(200);
    editor.frame().expect("a frame");
    editor.frame().expect("a frame");

    let drawn = |editor: &Editor<HeadlessShell>| {
        let (min, max) = editor.panels.viewport_pixels();
        let sampled = editor
            .panels
            .draw_list()
            .commands()
            .iter()
            .find_map(|command| match command {
                crcbl::ui::DrawCommand::Texture {
                    texture, min, max, ..
                } if *texture == VIEWPORT_TEXTURE => Some((*min, *max)),
                _ => None,
            })
            .expect("the panels sample the viewport's picture");
        assert_eq!(
            sampled,
            (min, max),
            "the picture is drawn over the pane exactly"
        );
        let size = (max - min).round();
        assert_eq!(
            editor.drawn_viewport,
            Some((size.x as u32, size.y as u32)),
            "the scene is drawn at the pane's size in window pixels",
        );
        editor.drawn_viewport.expect("a frame was drawn")
    };

    let before = drawn(&editor);
    let pooled = editor.pool.image_count();
    editor.frame().expect("a frame");
    assert_eq!(
        editor.pool.image_count(),
        pooled,
        "a steady frame allocates nothing"
    );

    let window = editor.window;
    let (width, height) = editor.extent();
    editor
        .shell_mut()
        .resize(window, PhysicalSize::new(width + 160, height + 96))
        .expect("live");
    // Two frames: the swapchain may be reconfigured on the first, which
    // presents nothing, and the panels are laid out at the new size by
    // the time the second is drawn.
    editor.frame().expect("a frame");
    editor.frame().expect("a frame");
    let after = drawn(&editor);
    assert_ne!(after, before, "the resize did not change the pane");
    assert!(
        editor.pool.image_count() > pooled,
        "the pane's new size drew into the old target: nothing was allocated",
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
