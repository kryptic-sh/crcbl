//! Play mode through the loop: F5, F6 and the toolbar, the frames that tick
//! in between, and every edit path refused on the status line.
//!
//! The editor here plays the compiled-in scene through
//! `crate::document::play_tests::drifting`, a test vocabulary whose module
//! moves every block along X each tick; the shipped vocabulary's one module
//! is towers', which plays towers' field and not the greybox scene — the
//! tests at the end play it, and take part through its play strip.

use super::*;

use crate::document::play_tests::{DRIFT_M, TICK, drifting, drifting_document};

/// An editor on the compiled-in scene and the drifting vocabulary, whose
/// frames are each one of the module's ticks long — so a frame of play is a
/// tick, and a test counts them.
fn drifting_editor(frames: u64) -> Editor<HeadlessShell> {
    let mut editor = headless(frames);
    // The same scene the editor opened, so the instances, the panels and the
    // camera built from that one describe this one too.
    editor.document = drifting_document();
    editor.clock_source = Clock::manual(TICK);
    editor
}

/// The X of `id`'s position.
fn x_of(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> f64 {
    leaves(editor, id, gizmo::POSITION)[0]
}

/// Asserts the status line is a warning that names play mode.
fn assert_refused_for_play(editor: &Editor<HeadlessShell>, what: &str) {
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{what}: {text}");
    assert!(text.contains("play mode"), "{what}: {text}");
}

/// An editor that is playing and then paused, so nothing moves under a test
/// comparing the scene before and after a refused edit — edits are refused
/// paused as much as playing.
fn paused_editor(frames: u64) -> Editor<HeadlessShell> {
    let mut editor = drifting_editor(frames);
    editor.act(&Action::PlayStop);
    editor.act(&Action::Pause);
    assert_eq!(editor.document().play_state(), PlayState::Paused);
    editor
}

/// **F5 plays and the frames tick, F6 pauses and resumes, and F5 again puts
/// the scene back** — each on the status line and on the toolbar's state.
#[test]
fn f5_plays_f6_pauses_and_f5_stops_and_restores() {
    const STEP: SceneEntityId = SceneEntityId(1);
    let mut editor = drifting_editor(40);
    let before = editor.document_mut().files().expect("ids");
    let was = x_of(&mut editor, STEP);

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    let (text, _) = editor.panels.status();
    assert!(text.starts_with("Playing drift"), "{text}");
    let moved = x_of(&mut editor, STEP);
    assert!(
        moved > was,
        "the frames after F5 did not tick: {was} to {moved}"
    );

    tap(&mut editor, KeyCode::F6);
    assert_eq!(editor.document().play_state(), PlayState::Paused);
    assert!(editor.panels.status().0.starts_with("Paused"));
    let held = x_of(&mut editor, STEP);
    editor.frame().expect("a frame");
    assert_eq!(x_of(&mut editor, STEP), held, "a paused frame ticked");

    tap(&mut editor, KeyCode::F6);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    editor.frame().expect("a frame");
    assert!(
        (x_of(&mut editor, STEP) - held) >= DRIFT_M,
        "a resumed frame did not tick"
    );

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    assert!(editor.panels.status().0.starts_with("Stopped"));
    assert_eq!(editor.document_mut().files().expect("ids"), before);

    let summary = editor.finish(ExitReason::FrameBudget).expect("teardown");
    assert!(summary.run.ticks > 0, "the summary counted no ticks");
    assert!(
        summary.run.paused,
        "a run that ended editing was not paused"
    );
}

/// **The toolbar's buttons are F5's and F6's**: a click on each changes the
/// play state the way its key does, and the button that started play is the
/// one that stops it.
#[test]
fn the_toolbar_plays_pauses_and_stops() {
    let mut editor = drifting_editor(40);
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    let [play, pause] = editor.panels.toolbar_buttons();

    for (button, state) in [
        (play, PlayState::Playing),
        (pause, PlayState::Paused),
        (pause, PlayState::Playing),
        (play, PlayState::Editing),
    ] {
        let at = centre(&editor, button);
        click(&mut editor, at);
        assert_eq!(editor.document().play_state(), state);
    }
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// F6 while editing starts nothing and says how to.
#[test]
fn pause_while_editing_says_play_is_not_running() {
    let mut editor = drifting_editor(4);
    editor.act(&Action::Pause);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    assert!(editor.panels.status().0.starts_with("Not playing"));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A scene no registered game plays still plays**, and the status line says
/// only the world ticks — the compiled-in greybox scene, whose system no
/// module in the shipped vocabulary is registered under.
#[test]
fn a_scene_with_no_module_plays_and_says_only_the_world_ticks() {
    let mut editor = headless(8);
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Playing);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Info, "{text}");
    assert!(text.contains("only the"), "{text}");
    editor.frame().expect("a frame");
    editor.act(&Action::PlayStop);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Keyboard edits are refused in play mode**: a nudge, a delete and a
/// duplicate each leave the scene and the log as they were and say why.
#[test]
fn keyboard_edits_are_refused_in_play_mode() {
    let mut editor = paused_editor(8);
    editor.document_mut().select(Some(SceneEntityId(2)));
    let before = editor.document_mut().files().expect("ids");
    for action in [
        Action::Nudge { axis: 0, sign: 1.0 },
        Action::Delete,
        Action::Duplicate,
    ] {
        editor.panels.set_status("Ready", Tone::Info);
        editor.act(&action);
        assert_refused_for_play(&editor, &format!("{action:?}"));
        assert_eq!(editor.document_mut().files().expect("ids"), before);
        assert!(
            editor.document().log().is_empty(),
            "{action:?} was recorded"
        );
    }
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Undo and redo are refused in play mode**, leaving the log where it was —
/// one entry done and one undone, so both directions have something to walk.
#[test]
fn undo_and_redo_are_refused_in_play_mode() {
    let mut editor = drifting_editor(8);
    editor.document_mut().select(Some(SceneEntityId(2)));
    editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
    editor.act(&Action::Nudge { axis: 1, sign: 1.0 });
    editor.act(&Action::Undo);
    editor.act(&Action::PlayStop);
    for action in [Action::Undo, Action::Redo] {
        editor.panels.set_status("Ready", Tone::Info);
        editor.act(&action);
        assert_refused_for_play(&editor, &format!("{action:?}"));
        assert_eq!(
            editor.document().log().position(),
            1,
            "{action:?} walked the log"
        );
    }
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A save in play mode is refused and writes nothing**, from a document
/// opened from a directory — so it is play mode that refused it, not the
/// lack of anywhere to write — and the compiled-in scene says play mode too,
/// rather than that it has nowhere to go.
#[test]
fn a_save_in_play_mode_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    drifting_document()
        .save_to(dir.path())
        .expect("a writable directory");
    let chunk = dir.path().join("sys").join("blocks.ron");
    let on_disk = std::fs::read(&chunk).expect("the save wrote the chunk");

    let mut editor = drifting_editor(16);
    editor.document = Document::open_dir(dir.path(), drifting()).expect("what we wrote");
    editor.act(&Action::PlayStop);
    for _ in 0..4 {
        editor.frame().expect("a frame");
    }
    editor.act(&Action::Save);
    assert_refused_for_play(&editor, "save");
    assert_eq!(
        std::fs::read(&chunk).expect("still there"),
        on_disk,
        "a save in play mode wrote the played scene",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    let mut built_in = paused_editor(4);
    built_in.act(&Action::Save);
    assert_refused_for_play(&built_in, "save of the compiled-in scene");
    built_in.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A paste in play mode is refused** when the clipboard answers, on the
/// status line, and spawns nothing — a clipping the scene could otherwise
/// hold, so it is play mode that refused it.
#[test]
fn a_paste_in_play_mode_is_refused() {
    let mut editor = drifting_editor(8);
    let clipping = editor
        .document_mut()
        .copy(SceneEntityId(2))
        .expect("in the scene");
    let window = editor.window;
    editor
        .shell_mut()
        .clipboard_offer(window, &[crcbl::shell::ClipboardOffer::text(&clipping)])
        .expect("the headless clipboard takes an offer");
    editor.act(&Action::PlayStop);
    editor.act(&Action::Pause);
    let count = editor.document().entity_count();

    editor.act(&Action::Paste);
    for _ in 0..3 {
        editor.frame().expect("a frame");
    }
    assert_refused_for_play(&editor, "paste");
    assert_eq!(editor.document().entity_count(), count, "the paste spawned");
    assert!(editor.document().log().is_empty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A gizmo drag in play mode is refused**: the handle is grabbed, every
/// write is refused on the status line, and the selection stays where play
/// left it.
#[test]
fn a_gizmo_drag_in_play_mode_is_refused() {
    let id = SceneEntityId(2);
    let mut editor = paused_editor(16);
    editor.document_mut().select(Some(id));
    editor.frame().expect("a frame");
    let was = leaves(&mut editor, id, gizmo::POSITION);

    let (from, to) = handle_at(&mut editor, gizmo::Grip::Move(gizmo::Axis::X));
    drag(&mut editor, (from + to) * 0.5, to + (to - from) * 0.5);

    assert_refused_for_play(&editor, "gizmo drag");
    assert_eq!(
        leaves(&mut editor, id, gizmo::POSITION),
        was,
        "the drag moved it"
    );
    assert!(editor.document().log().is_empty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A ring drag in play mode is refused** like any other handle's: every
/// write is refused on the status line, and the rotation stays where play
/// left it.
#[test]
fn a_ring_drag_in_play_mode_is_refused() {
    let id = SceneEntityId(2);
    let mut editor = paused_editor(16);
    editor.document_mut().select(Some(id));
    editor.frame().expect("a frame");
    editor.act(&Action::Rotate);
    let was = editor.rotation_of(id).expect("a block has a rotation");

    let (grab, release) = super::ring_quarter(&mut editor, gizmo::Axis::Y);
    drag(&mut editor, grab, release);

    assert_refused_for_play(&editor, "ring drag");
    assert_eq!(editor.rotation_of(id), Some(was), "the drag turned it");
    assert!(editor.document().log().is_empty());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Towers' creeps are drawn while its field plays, and gone once it
/// stops** — counted off the renderer's own live records as well as the
/// editor's instances, through the field opened from its committed directory
/// the way `editor <SCENE_DIR>` opens it.
#[test]
fn towers_creeps_are_drawn_while_the_field_plays_and_gone_after_stop() {
    /// Ticks handed to each frame: under the clock's catch-up cap, so none is
    /// dropped, and enough that the build phase runs down in a few dozen
    /// frames.
    const TICKS_PER_FRAME: u32 = 4;

    let mut editor = towers_editor(400, TICKS_PER_FRAME);
    let spawned = spawned_drawn;
    let live = |editor: &Editor<HeadlessShell>| {
        editor
            .renderer
            .cull_records()
            .0
            .iter()
            .filter(|record| record.flags & crcbl::shaders::mesh::GpuInstance::LIVE != 0)
            .count()
    };
    editor.frame().expect("a frame");
    let scene = live(&editor);
    assert_eq!(spawned(&editor), 0, "an editing field drew a creep");

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().playing_modules(), ["towers"]);
    // The build phase at towers' rate, in frames, and two frames more: the
    // one whose ticks release the first creep, and the one that draws it.
    let build = crcbl_towers::wave::GAP_S * f64::from(crcbl_towers::DEFAULT_TICK_HZ);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = (build / f64::from(TICKS_PER_FRAME)).ceil() as u32 + 2;
    for _ in 0..frames {
        editor.frame().expect("a frame");
    }
    let creeps = editor.document_mut().spawned().len();
    assert!(creeps > 0, "no creep walked in {frames} frames of play");
    assert_eq!(spawned(&editor), creeps, "a walking creep is not drawn");
    assert_eq!(
        live(&editor),
        scene + creeps,
        "the renderer does not draw the creeps"
    );

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    assert_eq!(spawned(&editor), 0, "a creep outlived play");
    assert_eq!(live(&editor), scene, "the renderer still draws a creep");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// An editor on towers' committed field, opened from its directory the way
/// `editor <SCENE_DIR>` opens it, whose frames are each `ticks` of towers'
/// ticks long.
fn towers_editor(frames: u64, ticks: u32) -> Editor<HeadlessShell> {
    let field = Path::new(env!("CARGO_MANIFEST_DIR")).join("../towers/assets/scenes/field.scn");
    let mut options = options(frames);
    options.scene = Some(field);
    let mut editor = Editor::with_shell(Box::new(HeadlessShell::new()), &options)
        .expect("the null backend opens towers' field");
    let period = Duration::from_secs_f64(1.0 / f64::from(crcbl_towers::DEFAULT_TICK_HZ));
    editor.clock_source = Clock::manual(period * ticks);
    editor
}

/// How many entities the editor's instances draw that a playing module
/// spawned.
fn spawned_drawn(editor: &Editor<HeadlessShell>) -> usize {
    use crate::app::instances::Drawn;

    editor
        .instances
        .instances
        .iter()
        .filter(|placed| matches!(placed.drawn, Drawn::Spawned(_)))
        .count()
}

/// The play strip's button the last frame drew reading `label`.
fn play_button(editor: &Editor<HeadlessShell>, label: &str) -> NodeKey {
    editor
        .panels
        .play_buttons()
        .iter()
        .find(|(each, _)| each == label)
        .map(|(_, key)| *key)
        .unwrap_or_else(|| {
            panic!(
                "the strip has no `{label}` button: {:?}",
                editor.panels.play_buttons()
            )
        })
}

/// What the play strip last drew for `label`.
fn readout(editor: &Editor<HeadlessShell>, label: &str) -> String {
    editor
        .panels
        .play_readout()
        .iter()
        .find(|(each, _)| *each == label)
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| panic!("the strip shows no {label}"))
}

/// Selects the field's first plot, in file order.
fn select_first_plot(editor: &mut Editor<HeadlessShell>) {
    let (_, plots) = editor
        .document_mut()
        .outline()
        .into_iter()
        .find(|(system, _)| system == "plots")
        .expect("the field has plots");
    editor.document_mut().select(Some(plots[0]));
}

/// What a base tower of `kind` costs.
fn tower_cost(kind: crcbl_towers::tower::Kind) -> u32 {
    kind.spec(crcbl_towers::Tier::Base).cost
}

/// **The play strip places a tower on the selected plot**: the strip shows
/// towers' actions and the run's numbers while the field plays, a click on
/// `Place tower` is sent and builds a tower that is drawn, the gold the strip
/// shows drops by its price, and a second click on the taken plot is refused
/// on the status line with the game's reason.
#[test]
fn the_play_strip_places_a_tower_and_a_taken_plot_is_refused_on_the_status_line() {
    use crcbl_towers::tower::Kind;

    let mut editor = towers_editor(60, 1);
    editor.frame().expect("a frame");
    assert!(
        editor.panels.play_strip().is_none(),
        "an editing field shows a play strip"
    );

    tap(&mut editor, KeyCode::F5);
    assert!(
        editor.panels.play_strip().is_some(),
        "no strip while playing"
    );
    for label in ["Lives", "Gold", "Wave", "Outcome"] {
        readout(&editor, label);
    }
    let gold = crcbl_towers::wave::STARTING_GOLD;
    assert_eq!(readout(&editor, "Gold"), gold.to_string());
    select_first_plot(&mut editor);
    let before = spawned_drawn(&editor);

    let at = centre(&editor, play_button(&editor, "Place tower"));
    click(&mut editor, at);
    assert_eq!(editor.panels.status(), ("Sent Place tower", Tone::Info));
    assert_eq!(
        readout(&editor, "Gold"),
        (gold - tower_cost(Kind::Bolt)).to_string(),
        "the strip does not show the price paid",
    );
    assert_eq!(spawned_drawn(&editor), before + 1, "the tower is not drawn");

    click(&mut editor, at);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert_eq!(
        text,
        format!(
            "{REFUSED}{}",
            crcbl_towers::game::Refusal::PlotTaken.label()
        )
    );
    assert_eq!(
        readout(&editor, "Gold"),
        (gold - tower_cost(Kind::Bolt)).to_string()
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A choice on the strip steps on a click, and the build takes it**: the
/// kind button reads the first kind, then the next, and the tower placed
/// after costs that kind's price.
#[test]
fn the_strips_kind_choice_steps_and_the_build_takes_it() {
    use crcbl_towers::tower::{ALL, Kind};

    let mut editor = towers_editor(40, 1);
    tap(&mut editor, KeyCode::F5);
    select_first_plot(&mut editor);
    let [first, second] = [ALL[0], ALL[1]].map(Kind::label);
    let at = centre(&editor, play_button(&editor, first));
    click(&mut editor, at);
    play_button(&editor, second);

    let at = centre(&editor, play_button(&editor, "Place tower"));
    click(&mut editor, at);
    assert_eq!(
        readout(&editor, "Gold"),
        (crcbl_towers::wave::STARTING_GOLD - tower_cost(ALL[1])).to_string(),
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The strip's `Start wave` sends the first wave, and its numbers follow**,
/// and a build with nothing selected says what to select rather than sending
/// anything.
#[test]
fn the_strip_starts_a_wave_and_a_build_needs_a_plot() {
    let mut editor = towers_editor(40, 1);
    tap(&mut editor, KeyCode::F5);
    let waves = crcbl_towers::WAVES.len();
    assert_eq!(readout(&editor, "Wave"), format!("0/{waves}"));

    let at = centre(&editor, play_button(&editor, "Start wave"));
    click(&mut editor, at);
    assert_eq!(readout(&editor, "Wave"), format!("1/{waves}"));

    editor.document_mut().select(None);
    let at = centre(&editor, play_button(&editor, "Place tower"));
    click(&mut editor, at);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("select one of `plots`"), "{text}");
    assert_eq!(
        readout(&editor, "Gold"),
        crcbl_towers::wave::STARTING_GOLD.to_string()
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Stop takes the strip, the towers and the numbers away, and the files
/// are the ones play began with.**
#[test]
fn stop_takes_the_strip_and_the_towers_away() {
    let mut editor = towers_editor(40, 1);
    editor.frame().expect("a frame");
    let before = editor.document_mut().files().expect("ids");
    tap(&mut editor, KeyCode::F5);
    select_first_plot(&mut editor);
    let at = centre(&editor, play_button(&editor, "Place tower"));
    click(&mut editor, at);
    assert!(spawned_drawn(&editor) > 0, "nothing was built");

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().play_state(), PlayState::Editing);
    assert!(
        editor.panels.play_strip().is_none(),
        "the strip outlived play"
    );
    assert!(editor.panels.play_readout().is_empty());
    assert_eq!(spawned_drawn(&editor), 0, "a tower outlived play");
    assert_eq!(editor.document_mut().files().expect("ids"), before);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A scene whose game offers no play controls shows no strip**: the
/// drifting test module plays the greybox scene with none, and the viewport
/// keeps the height it had while editing.
#[test]
fn a_game_with_no_play_controls_shows_no_strip() {
    let mut editor = drifting_editor(8);
    editor.frame().expect("a frame");
    let viewport = editor.panels.viewport();
    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().playing_modules(), ["drift"]);
    assert!(editor.panels.play_strip().is_none());
    assert!(editor.panels.play_buttons().is_empty());
    assert_eq!(editor.panels.viewport(), viewport, "the viewport shrank");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
