//! The outranking pad chord ([`ActionMap::set_pad_chords_outrank`]) on the
//! controls EW asked for it with: LB+Select and LB+Start, global chords over
//! the global map and inventory buttons, rebound by the player to a gameplay
//! reload.

use super::*;
use crate::{ActionDecl, ActionOverride, GamepadEvent, GamepadId, GamepadSnapshot, PadKind};

const TICK: f32 = 1.0 / 60.0;
const PAD: GamepadId = GamepadId(7);

/// EW's shape, as the binding asset it would ship: the map and the inventory
/// on Select and Start, and free look and the backpack drop on LB with each,
/// all global; reload in gameplay, marked outranking; and a modal inventory
/// screen. Written in [`ActionMap::to_ron`]'s canonical form.
const CONTROLS: &str = r#"[
    (
        action: "map",
        kind: Button,
        context: "global",
        keyboard: ["KeyM"],
        gamepad: ["Pad:Select"],
    ),
    (
        action: "inventory",
        kind: Button,
        context: "global",
        keyboard: ["Tab"],
        gamepad: ["Pad:Start"],
    ),
    (
        action: "free_look",
        kind: Button,
        context: "global",
        gamepad: ["Pad:LeftShoulder+Select"],
    ),
    (
        action: "drop_backpack",
        kind: Button,
        context: "global",
        gamepad: ["Pad:LeftShoulder+Start"],
    ),
    (
        action: "reload",
        kind: Button,
        keyboard: ["KeyR"],
        gamepad: ["Pad:West"],
        pad_chords_outrank: true,
    ),
    (
        action: "close",
        kind: Button,
        context: "backpack",
        keyboard: ["Escape"],
        gamepad: ["Pad:East"],
    ),
]
"#;

/// Each menu button, the global chord on it, and the menu it opens.
const MENU_CHORDS: [(PadButton, &str, &str); 2] = [
    (PadButton::Select, "free_look", "map"),
    (PadButton::Start, "drop_backpack", "inventory"),
];

fn controls() -> ActionMap {
    let mut map = ActionMap::from_ron(CONTROLS).unwrap_or_else(|error| panic!("{error}"));
    map.gamepad_event(&GamepadEvent::Connected {
        id: PAD,
        kind: PadKind::Xbox,
    });
    map
}

fn lb(button: PadButton) -> Binding {
    Binding::PadChord {
        modifier: PadButton::LeftShoulder,
        button,
    }
}

/// What EW's Controls page saves when the player moves LB+`button` from its
/// global action to reload: the old action cleared, reload on the chord alone.
fn reassigned(button: PadButton, former: &str) -> Vec<ActionOverride> {
    vec![
        ActionOverride {
            action: former.to_owned(),
            bindings: Vec::new(),
        },
        ActionOverride {
            action: "reload".to_owned(),
            bindings: vec![lb(button)],
        },
    ]
}

/// The controls with LB+`button` moved to reload.
fn rebound(button: PadButton, former: &str) -> ActionMap {
    let mut map = controls();
    assert_eq!(map.apply_overrides(&reassigned(button, former)), []);
    map
}

fn hold_on(map: &mut ActionMap, id: GamepadId, buttons: &[PadButton]) {
    map.gamepad_event(&GamepadEvent::State {
        id,
        snapshot: GamepadSnapshot {
            buttons: buttons.iter().copied().collect(),
            ..GamepadSnapshot::neutral(PadKind::Xbox)
        },
    });
}

fn hold(map: &mut ActionMap, buttons: &[PadButton]) {
    hold_on(map, PAD, buttons);
}

/// **The rebound chord presses and releases reload, and the menu stays
/// shut**; the menu button alone still opens it, and reload does not.
#[test]
fn a_rebound_outranking_chord_presses_and_releases_its_action_and_not_the_menu() {
    for (button, former, menu) in MENU_CHORDS {
        let mut map = rebound(button, former);
        map.begin_tick(TICK);
        hold(&mut map, &[PadButton::LeftShoulder, button]);
        assert!(map.just_pressed("reload"), "{button:?}: the chord reloads");
        assert!(!map.button_held(menu), "{button:?}: and opens no {menu}");

        map.begin_tick(TICK);
        hold(&mut map, &[]);
        assert!(map.just_released("reload"), "{button:?}: and lets go");
        assert!(!map.button_held(menu));

        map.begin_tick(TICK);
        hold(&mut map, &[button]);
        assert!(
            map.just_pressed(menu),
            "{button:?}: alone, it is the menu's"
        );
        assert!(!map.button_held("reload"));
    }
}

/// **The controls as shipped are untouched by the mark**: the global chord is
/// the global context's own, so LB+Select is free look and Select alone the
/// map, with reload marked all along.
#[test]
fn unmodified_menu_buttons_and_their_global_chords_work_as_declared() {
    for (button, former, menu) in MENU_CHORDS {
        let mut map = controls();
        map.begin_tick(TICK);
        hold(&mut map, &[button]);
        assert!(map.just_pressed(menu), "{button:?} opens {menu}");
        hold(&mut map, &[]);
        hold(&mut map, &[PadButton::LeftShoulder, button]);
        assert!(map.button_held(former), "LB+{button:?} is {former}");
        assert!(!map.button_held(menu), "and not {menu}");
        assert!(!map.button_held("reload"));
    }
}

/// **The mark is opt-in**: an unmarked gameplay chord on Select reads nothing,
/// even held beside the marked one; and with reload unmarked, the rebound
/// chord beneath the global button reads nothing and the global map opens, as
/// before the mark existed.
#[test]
fn an_unmarked_chord_keeps_the_ordinary_precedence() {
    let mut map = rebound(PadButton::Select, "free_look");
    map.declare(ActionDecl {
        name: "melee".to_owned(),
        kind: crate::ActionKind::Button,
        bindings: vec![Binding::PadChord {
            modifier: PadButton::RightShoulder,
            button: PadButton::Select,
        }],
    });
    hold(
        &mut map,
        &[
            PadButton::LeftShoulder,
            PadButton::RightShoulder,
            PadButton::Select,
        ],
    );
    assert!(map.button_held("reload"));
    assert!(!map.button_held("melee"), "rode in on the marked chord");
    hold(&mut map, &[]);

    map.set_pad_chords_outrank("reload", false)
        .expect("declared");
    assert_eq!(map.pad_chords_outrank("reload"), Some(false));
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(!map.button_held("reload"), "an unmarked chord outranked");
    assert!(map.button_held("map"), "the global button is the map's");

    assert_eq!(
        map.set_pad_chords_outrank("nope", true),
        Err(ActionMapError::UnknownAction("nope".to_owned()))
    );
    assert_eq!(map.pad_chords_outrank("nope"), None);
}

/// **A context above that binds the same chord keeps it**: reload on
/// LB+Select with free look still on it leaves the chord free look's.
#[test]
fn a_context_above_binding_the_same_chord_keeps_it() {
    let mut map = controls();
    map.rebind("reload", vec![lb(PadButton::Select)])
        .expect("declared");
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.button_held("free_look"), "the global chord is global's");
    assert!(!map.button_held("reload"), "taken from the context above");
    assert!(!map.button_held("map"));
}

/// **A modal screen still blocks gameplay**: under the backpack the chord
/// reaches no reload, and a chord held as the screen opens lets go of reload
/// without opening the map, and does not reload again once it closes until
/// it is pressed again.
#[test]
fn a_modal_context_above_blocks_an_outranking_chord() {
    let mut map = rebound(PadButton::Select, "free_look");
    map.push_context_modal("backpack").expect("declared");
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(
        !map.button_held("reload"),
        "the chord reached past the modal"
    );
    assert!(map.button_held("map"), "what is left is the global button");
    hold(&mut map, &[]);
    map.pop_context("backpack").expect("on top");

    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.button_held("reload"));
    map.begin_tick(TICK);
    map.push_context_modal("backpack").expect("declared");
    assert!(
        map.just_released("reload"),
        "the screen takes the chord away"
    );
    assert!(!map.button_held("map"), "and hands Select to nobody held");
    map.pop_context("backpack").expect("on top");
    map.begin_tick(TICK);
    assert!(
        !map.button_held("reload"),
        "closed, still not pressed again"
    );
    hold(&mut map, &[PadButton::LeftShoulder]);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.just_pressed("reload"), "pressed again");
}

/// **Letting go of LB first does not open the map**, and LB pressed over a
/// held Select lets go of the map without reloading: the modifier hands the
/// held button between two contexts, and the new reader waits for a release.
#[test]
fn the_modifier_handing_a_held_button_over_leaks_neither_action() {
    let mut map = rebound(PadButton::Select, "free_look");
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.just_pressed("reload"));
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::Select]);
    assert!(map.just_released("reload"), "LB let go first");
    assert!(!map.button_held("map"), "the map opened on the way out");
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::Select]);
    assert!(!map.button_held("map"), "nor on the next snapshot");
    hold(&mut map, &[]);

    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::Select]);
    assert!(map.just_pressed("map"));
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::Select, PadButton::LeftShoulder]);
    assert!(map.just_released("map"), "the chord takes Select");
    assert!(!map.button_held("reload"), "without a press of it");
    hold(&mut map, &[PadButton::Select]);
    assert!(!map.button_held("map"), "and does not give it back held");
    hold(&mut map, &[]);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.button_held("reload"), "a fresh chord after the release");
}

/// **Focus loss lets go of the chord, and it waits for neutral**: the pad
/// still holding LB+Select when snapshots resume reloads nothing and opens
/// nothing until it has let go.
#[test]
fn focus_loss_releases_the_chord_until_the_pad_goes_neutral() {
    let mut map = rebound(PadButton::Select, "free_look");
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    map.begin_tick(TICK);
    map.release_gamepads();
    assert!(map.just_released("reload"));

    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(!map.button_held("reload"), "resumed held: not a press");
    assert!(!map.button_held("map"));
    hold(&mut map, &[PadButton::Select]);
    assert!(!map.button_held("map"), "LB let go of before Select");
    hold(&mut map, &[]);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(map.just_pressed("reload"), "neutral, then pressed");
}

/// **Unplugging lets go of the chord**, and a second pad still holding Select
/// once the one holding LB is unplugged does not open the map.
#[test]
fn unplugging_releases_the_chord_and_hands_nothing_on() {
    let mut map = rebound(PadButton::Select, "free_look");
    map.begin_tick(TICK);
    hold(&mut map, &[PadButton::LeftShoulder, PadButton::Select]);
    map.begin_tick(TICK);
    map.gamepad_event(&GamepadEvent::Disconnected { id: PAD });
    assert!(map.just_released("reload"));
    assert!(!map.button_held("map"));

    let other = GamepadId(8);
    map.begin_tick(TICK);
    hold_on(&mut map, PAD, &[PadButton::LeftShoulder]);
    hold_on(&mut map, other, &[PadButton::Select]);
    assert!(map.just_pressed("reload"), "every pad drives every binding");
    map.begin_tick(TICK);
    map.gamepad_event(&GamepadEvent::Disconnected { id: PAD });
    assert!(map.just_released("reload"));
    assert!(
        !map.button_held("map"),
        "the other pad's Select opened the map"
    );
}

/// **The keyboard's menu keys stay the menus'**: M and Tab open the map and
/// the inventory while the chord on their pad buttons is held.
#[test]
fn keyboard_menu_bindings_work_while_the_chord_is_held() {
    for (button, former, menu) in MENU_CHORDS {
        let key = if menu == "map" {
            KeyCode::KeyM
        } else {
            KeyCode::Tab
        };
        let mut map = rebound(button, former);
        hold(&mut map, &[PadButton::LeftShoulder, button]);
        map.begin_tick(TICK);
        map.key_event(key, true);
        assert!(map.just_pressed(menu), "{key:?} opens {menu}");
        assert!(map.button_held("reload"), "beside the held chord");
    }
}

/// **The rebind round-trips through its text and survives restoring the
/// defaults**: saved and applied to a fresh map, the chord still reloads;
/// the defaults restored, LB+Select is free look again and reload stays
/// marked for the next rebind.
#[test]
fn the_rebind_round_trips_and_restoring_the_defaults_keeps_the_mark() {
    let played = rebound(PadButton::Select, "free_look");
    let saved: Vec<(String, Vec<String>)> = played
        .overrides()
        .into_iter()
        .map(|entry| {
            let texts = entry.bindings.iter().map(ToString::to_string).collect();
            (entry.action, texts)
        })
        .collect();
    assert_eq!(
        saved,
        [
            ("free_look".to_owned(), Vec::new()),
            (
                "reload".to_owned(),
                vec!["Pad:LeftShoulder+Select".to_owned()]
            ),
        ]
    );
    let read: Vec<ActionOverride> = saved
        .iter()
        .map(|(action, texts)| ActionOverride {
            action: action.clone(),
            bindings: texts.iter().map(|text| text.parse().unwrap()).collect(),
        })
        .collect();

    let mut fresh = controls();
    assert_eq!(fresh.apply_overrides(&read), []);
    assert_eq!(fresh.overrides(), played.overrides());
    hold(&mut fresh, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(fresh.button_held("reload"), "the loaded rebind reloads");
    assert!(!fresh.button_held("map"));
    hold(&mut fresh, &[]);

    assert_eq!(fresh.apply_overrides(&[]), []);
    assert_eq!(fresh.overrides(), []);
    assert_eq!(fresh.pad_chords_outrank("reload"), Some(true));
    hold(&mut fresh, &[PadButton::LeftShoulder, PadButton::Select]);
    assert!(fresh.button_held("free_look"), "the defaults are back");
    assert!(!fresh.button_held("reload") && !fresh.button_held("map"));
    hold(&mut fresh, &[]);

    assert_eq!(
        fresh.apply_overrides(&reassigned(PadButton::Start, "drop_backpack")),
        []
    );
    hold(&mut fresh, &[PadButton::LeftShoulder, PadButton::Start]);
    assert!(fresh.button_held("reload"), "rebound again, still marked");
    assert!(!fresh.button_held("inventory"));
}

/// **The mark round-trips through a binding asset**: the controls write back
/// byte for byte with it, read back marked, and an unmarked action writes no
/// field for it.
#[test]
fn the_mark_round_trips_through_a_binding_asset() {
    let map = controls();
    assert_eq!(map.to_ron().as_deref(), Ok(CONTROLS));
    assert_eq!(map.pad_chords_outrank("reload"), Some(true));
    assert_eq!(map.pad_chords_outrank("map"), Some(false));

    let mut unmarked = controls();
    unmarked
        .set_pad_chords_outrank("reload", false)
        .expect("declared");
    let written = unmarked.to_ron().expect("writable");
    assert!(!written.contains("pad_chords_outrank"), "{written}");
    let read = ActionMap::from_ron(&written).expect("its own output");
    assert_eq!(read.pad_chords_outrank("reload"), Some(false));
}
