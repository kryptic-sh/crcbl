//! The play strip on towers' field, through the loop: a built tower picked
//! by a click, outlined and stepped up, every refusal of a frame told and a
//! burst of them bounded, the number keys, two games' rows at once, and a
//! choice that lasts one play.

use super::*;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crcbl::ecs::{ClientInputs, Entity, GameModule, World};
use crcbl::registry::{ParamKind, PlayAction, PlayArg, PlayControls, Registry};
use crcbl_towers::game::Refusal;
use crcbl_towers::tower::{ALL, Kind};
use crcbl_towers::{Tier, WAVES};

use super::play::{play_button, readout, select_first_plot, tower_cost, towers_editor};

/// The window pixel the middle of a spawned entity's box is drawn at.
fn spawned_pixel(editor: &mut Editor<HeadlessShell>, entity: Entity) -> PhysicalPoint {
    let (min, max) = editor
        .document_mut()
        .spawned_bounds(entity)
        .expect("a spawned entity in the world");
    let (corner, _) = editor.panels.viewport_pixels();
    let at = editor
        .camera
        .camera()
        .pixel_of((min + max) * 0.5, editor.panels.viewport_extent())
        .expect("the field's view has it in front of the eye");
    physical(corner + at)
}

/// Plays towers' field and builds a bolt tower on the first plot through the
/// strip, then selects nothing — so no handle stands over the tower — and
/// returns the tower's entity.
fn with_a_tower(editor: &mut Editor<HeadlessShell>) -> Entity {
    tap(editor, KeyCode::F5);
    select_first_plot(editor);
    let before = editor.document_mut().spawned();
    let at = centre(editor, play_button(editor, "Place tower (1)"));
    click(editor, at);
    editor.document_mut().select(None);
    editor.frame().expect("a frame");
    let built: Vec<Entity> = editor
        .document_mut()
        .spawned()
        .into_iter()
        .filter(|entity| !before.contains(entity))
        .collect();
    let [tower] = built[..] else {
        panic!("the build spawned {built:?}, not one tower");
    };
    tower
}

/// The system towers' controls are under, and the index of its action
/// `name`.
fn towers_action(editor: &Editor<HeadlessShell>, name: &str) -> (String, usize) {
    let controls = editor.document().play_controls();
    let (system, controls) = controls.first().expect("towers offers play controls");
    let index = controls
        .actions
        .iter()
        .position(|each| each.name == name)
        .expect("an action towers offers");
    ((*system).to_owned(), index)
}

/// **A click on a built tower picks it, and the strip's `Upgrade` steps that
/// tower up**: with nothing picked the strip says what to click, a click on
/// the tower makes it the runtime pick and selects no scene entity, and the
/// upgrade is sent and paid for.
#[test]
fn a_click_on_a_built_tower_picks_it_and_upgrade_steps_it_up() {
    let mut editor = towers_editor(80, 1);
    let tower = with_a_tower(&mut editor);
    let paid = crcbl_towers::wave::STARTING_GOLD - tower_cost(Kind::Bolt);
    assert_eq!(readout(&editor, "Gold"), paid.to_string());

    let at = centre(&editor, play_button(&editor, "Upgrade (3)"));
    click(&mut editor, at);
    let (text, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning, "{text}");
    assert!(text.contains("click one of `turrets`"), "{text}");

    let on_the_tower = spawned_pixel(&mut editor, tower);
    click(&mut editor, on_the_tower);
    assert_eq!(
        editor.document_mut().picked_runtime("turrets"),
        Some(tower),
        "the click did not pick the tower"
    );
    assert!(
        editor.document().selection().is_empty(),
        "the click on the tower selected a scene entity"
    );

    let at = centre(&editor, play_button(&editor, "Upgrade (3)"));
    click(&mut editor, at);
    assert_eq!(editor.panels.status(), ("Sent Upgrade", Tone::Info));
    let upgraded = Kind::Bolt.spec(Tier::Upgraded).cost;
    assert_eq!(readout(&editor, "Gold"), (paid - upgraded).to_string());
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The boxes the viewport outlines in [`PICKED_COLOR`] — the runtime
/// pick's — as the renderer is handed them.
fn picked_boxes(editor: &mut Editor<HeadlessShell>) -> Vec<[Vec3; 8]> {
    selection_boxes(editor.document_mut())
        .into_iter()
        .filter(|(_, color)| *color == PICKED_COLOR)
        .map(|(corners, _)| corners)
        .collect()
}

/// The corners of where a spawned entity stands now.
fn spawned_corners(editor: &mut Editor<HeadlessShell>, entity: Entity) -> [Vec3; 8] {
    editor
        .document_mut()
        .spawned_placement(entity)
        .expect("a spawned entity in the world")
        .corners()
        .map(|corner| corner.as_vec3())
}

/// **The runtime pick is outlined, and the outline follows its tower until
/// it goes**: nothing before a click, the tower's own box once clicked, the
/// grown box after an upgrade, and nothing once the pick is cleared, play
/// stops, or a restart despawns the tower.
#[test]
fn the_runtime_pick_is_outlined_and_follows_its_tower_until_it_goes() {
    let mut editor = towers_editor(120, 1);
    let tower = with_a_tower(&mut editor);
    assert!(
        picked_boxes(&mut editor).is_empty(),
        "outlined before a click"
    );
    let on_the_tower = spawned_pixel(&mut editor, tower);
    click(&mut editor, on_the_tower);
    let base = spawned_corners(&mut editor, tower);
    assert_eq!(
        picked_boxes(&mut editor),
        [base],
        "the pick is not outlined"
    );

    let (system, upgrade) = towers_action(&editor, "Upgrade");
    editor
        .document_mut()
        .send_play(&system, upgrade, &[PlayArg::PickedRuntime(tower)])
        .expect("towers encodes an upgrade");
    editor.frame().expect("a frame");
    let grown = spawned_corners(&mut editor, tower);
    assert_ne!(grown, base, "the upgrade did not grow the tower");
    assert_eq!(
        picked_boxes(&mut editor),
        [grown],
        "the outline did not follow the tier"
    );

    editor.document_mut().set_runtime_pick(None);
    assert!(
        picked_boxes(&mut editor).is_empty(),
        "outlined once unpicked"
    );

    click(&mut editor, on_the_tower);
    assert_eq!(picked_boxes(&mut editor).len(), 1, "the click did not pick");
    tap(&mut editor, KeyCode::F5);
    assert!(picked_boxes(&mut editor).is_empty(), "outlined after stop");

    let tower = with_a_tower(&mut editor);
    let on_the_tower = spawned_pixel(&mut editor, tower);
    click(&mut editor, on_the_tower);
    assert_eq!(picked_boxes(&mut editor).len(), 1, "the click did not pick");
    let (_, restart) = towers_action(&editor, "Restart");
    editor
        .document_mut()
        .send_play(&system, restart, &[])
        .expect("towers encodes a restart");
    editor.frame().expect("a frame");
    assert!(
        !editor.document_mut().spawned().contains(&tower),
        "the restart left the tower standing"
    );
    assert_eq!(
        editor.document().runtime_pick(),
        Some(tower),
        "the pick itself was cleared, so the despawn is not what is held"
    );
    assert!(
        picked_boxes(&mut editor).is_empty(),
        "a despawned pick is outlined"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Every refusal of one frame reaches the status line**, in the order the
/// game made them: a build on the taken plot and an upgrade of a tower
/// already at the top tier, sent together, are both told.
#[test]
fn every_refusal_of_a_frame_reaches_the_status_line() {
    let mut editor = towers_editor(80, 1);
    let tower = with_a_tower(&mut editor);
    let (system, upgrade) = towers_action(&editor, "Upgrade");
    let (_, place) = towers_action(&editor, "Place tower");
    let upgrade_args = [PlayArg::PickedRuntime(tower)];
    editor
        .document_mut()
        .send_play(&system, upgrade, &upgrade_args)
        .expect("towers encodes an upgrade");
    editor.frame().expect("a frame");
    assert_eq!(
        editor.panels.status().1,
        Tone::Info,
        "the first upgrade was refused"
    );

    let document = editor.document_mut();
    document
        .send_play(&system, place, &[PlayArg::Picked(0), PlayArg::Choice(0)])
        .expect("towers encodes a build");
    document
        .send_play(&system, upgrade, &upgrade_args)
        .expect("towers encodes an upgrade");
    editor.frame().expect("a frame");
    let both = format!(
        "{REFUSED}{}{REFUSAL_SEPARATOR}{}",
        Refusal::PlotTaken.label(),
        Refusal::TopTier.label()
    );
    assert_eq!(editor.panels.status(), (both.as_str(), Tone::Warning));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The number keys send the strip's actions while a game plays, and
/// nothing while editing**: `2` is towers' `Start wave`, labelled so, and the
/// wave it sends is counted.
#[test]
fn the_number_keys_send_the_strips_actions() {
    let mut editor = towers_editor(40, 1);
    editor.frame().expect("a frame");
    let before = editor.panels.status().0.to_owned();
    tap(&mut editor, KeyCode::Digit2);
    assert_eq!(
        editor.panels.status().0,
        before,
        "a digit did something while editing"
    );

    tap(&mut editor, KeyCode::F5);
    play_button(&editor, "Start wave (2)");
    tap(&mut editor, KeyCode::Digit2);
    assert_eq!(editor.panels.status(), ("Sent Start wave", Tone::Info));
    // The tick that reads it runs after the strip is built, so the count is
    // drawn a frame later.
    editor.frame().expect("a frame");
    assert_eq!(readout(&editor, "Wave"), format!("1/{}", WAVES.len()));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// How many frames [`Pinger`] has been handed.
static PINGED: AtomicUsize = AtomicUsize::new(0);

/// A second game's module, under towers' `plots`: counts the frames it is
/// handed.
struct Pinger;

impl GameModule for Pinger {
    fn name(&self) -> &str {
        "pinger"
    }

    fn register(&self, _world: &mut World) {}

    fn tick(&mut self, _world: &mut World, inputs: ClientInputs<'_>) {
        PINGED.fetch_add(inputs.len(), Ordering::Relaxed);
    }
}

/// A one-byte command, whatever the action.
fn one_byte(_: &mut World, _: usize, _: &[PlayArg]) -> Result<Vec<u8>, String> {
    Ok(vec![1])
}

fn no_status(_: &mut World) -> Vec<(&'static str, String)> {
    Vec::new()
}

fn no_refusals(_: &mut World) -> Vec<String> {
    Vec::new()
}

/// The second game's controls: one action.
const PING: PlayControls = PlayControls {
    actions: &[PlayAction {
        name: "Ping",
        description: "Ping the game",
        params: &[],
    }],
    encode: one_byte,
    status: no_status,
    refusals: no_refusals,
};

/// Whether [`burst`] has told its refusals.
static BURST_TOLD: AtomicBool = AtomicBool::new(false);

/// How many refusals [`burst`] tells at once: more than the status line
/// names.
const BURST: usize = REFUSALS_SHOWN + 4;

/// The reason [`burst`] gives for its refusal number `n`.
fn burst_reason(n: usize) -> String {
    format!("REFUSAL {n}")
}

/// [`BURST`] refusals the first time it is read, and none after.
fn burst(_: &mut World) -> Vec<String> {
    if BURST_TOLD.swap(true, Ordering::Relaxed) {
        Vec::new()
    } else {
        (1..=BURST).map(burst_reason).collect()
    }
}

/// A game whose controls turn down a burst of commands in one frame.
const BURSTING: PlayControls = PlayControls {
    actions: &[PlayAction {
        name: "Ping",
        description: "Ping the game",
        params: &[],
    }],
    encode: one_byte,
    status: no_status,
    refusals: burst,
};

/// **A burst of refusals in one frame is one bounded line, and every one is
/// logged**: the first [`REFUSALS_SHOWN`] named in order, the rest counted,
/// and a warning in the log for each.
#[test]
fn a_burst_of_refusals_is_one_bounded_line_and_every_one_logged() {
    let mut editor = towers_editor(40, 1);
    let mut registry = Registry::new();
    registry.module("plots", |_, _, _| Ok(Box::new(Pinger)));
    registry.play_controls("plots", BURSTING);
    crcbl_towers::register_components(&mut registry);
    editor.document = Document::open(
        &crcbl_towers::built_in_source(),
        Path::new(crcbl_towers::FIELD),
        registry,
    )
    .expect("towers' vocabulary opens its field");

    let logs = crcbl::log::capture();
    tap(&mut editor, KeyCode::F5);
    let shown: Vec<String> = (1..=REFUSALS_SHOWN).map(burst_reason).collect();
    let line = format!(
        "{REFUSED}{} (and {} more in the log)",
        shown.join(REFUSAL_SEPARATOR),
        BURST - REFUSALS_SHOWN
    );
    assert_eq!(editor.panels.status(), (line.as_str(), Tone::Warning));
    let logged: Vec<String> = logs
        .records()
        .into_iter()
        .filter(|record| record.level == crcbl::log::Level::Warn)
        .map(|record| record.message)
        .filter(|message| message.starts_with("editor: the game refused a command"))
        .collect();
    let every: Vec<String> = (1..=BURST)
        .map(|n| format!("editor: the game refused a command — {}", burst_reason(n)))
        .collect();
    assert_eq!(logged, every, "a refusal went unlogged");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// A third game's controls, under a system towers' field does not list.
const BELL: PlayControls = PlayControls {
    actions: &[PlayAction {
        name: "Ring",
        description: "Ring the bell",
        params: &[ParamKind::Choice(&["loud"])],
    }],
    encode: one_byte,
    status: no_status,
    refusals: no_refusals,
};

/// **Two games playing one scene each get a row, in the order they tick, and
/// a game the scene does not start gets none**: the strip lists the second
/// game's `Ping` before towers' actions, numbered across both rows, and `1`
/// reaches the second game alone while `3` is towers' `Start wave`.
#[test]
fn two_games_controls_share_the_strip_and_the_scenes_games_alone_show() {
    let mut editor = towers_editor(40, 1);
    let mut registry = Registry::new();
    registry.module("plots", |_, _, _| Ok(Box::new(Pinger)));
    registry.play_controls("plots", PING);
    registry.module("bells", |_, _, _| Ok(Box::new(Pinger)));
    registry.play_controls("bells", BELL);
    crcbl_towers::register_components(&mut registry);
    editor.document = Document::open(
        &crcbl_towers::built_in_source(),
        Path::new(crcbl_towers::FIELD),
        registry,
    )
    .expect("towers' vocabulary opens its field");

    tap(&mut editor, KeyCode::F5);
    assert_eq!(editor.document().playing_modules(), ["pinger", "towers"]);
    let labels: Vec<&str> = editor
        .panels
        .play_buttons()
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Ping (1)",
            "Place tower (2)",
            ALL[0].label(),
            "Start wave (3)",
            "Upgrade (4)",
            "Restart (5)"
        ],
        "the strip is not the scene's two games in tick order",
    );

    tap(&mut editor, KeyCode::Digit1);
    editor.frame().expect("a frame");
    assert_eq!(
        PINGED.load(Ordering::Relaxed),
        1,
        "`1` did not reach the second game"
    );
    assert_eq!(readout(&editor, "Wave"), format!("0/{}", WAVES.len()));
    tap(&mut editor, KeyCode::Digit3);
    editor.frame().expect("a frame");
    assert_eq!(readout(&editor, "Wave"), format!("1/{}", WAVES.len()));
    assert_eq!(
        PINGED.load(Ordering::Relaxed),
        1,
        "towers' command reached the second game"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A choice lasts one play**: the kind stepped to the second during a
/// play reads the first again once play is stopped and started.
#[test]
fn a_choice_is_reset_when_play_stops() {
    let mut editor = towers_editor(40, 1);
    let [first, second] = [ALL[0], ALL[1]].map(Kind::label);
    tap(&mut editor, KeyCode::F5);
    let at = centre(&editor, play_button(&editor, first));
    click(&mut editor, at);
    play_button(&editor, second);

    tap(&mut editor, KeyCode::F5);
    tap(&mut editor, KeyCode::F5);
    play_button(&editor, first);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
