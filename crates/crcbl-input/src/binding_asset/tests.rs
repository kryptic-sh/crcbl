use super::*;
use crate::{ActionOverride, ActionValue, Repeat};
use crcbl_core::input::{KeyCode, PointerButton};

/// An asset in the form [`ActionMap::to_ron`] writes: one field a line, empty
/// lists left out, the device lists in [`DEVICE_LISTS`] order.
const CANONICAL: &str = r#"[
    (
        action: "move",
        kind: Axis2,
        keyboard: ["Wasd:KeyW,KeyS,KeyA,KeyD", "Wasd:ArrowUp,ArrowDown,ArrowLeft,ArrowRight"],
        gamepad: ["PadStick:Left>0.2", "Pad:Dpad"],
        touch: ["Virtual:stick_move"],
    ),
    (
        action: "jump",
        kind: Button,
        keyboard: ["Space"],
        mouse: ["Mouse:Left"],
        gamepad: ["Pad:South"],
        patterns: [Tap(200, "hop"), Hold(250, "jump_charge"), DoubleTap(250, 250, "flip")],
    ),
    (
        action: "jump_charge",
        kind: Button,
    ),
    (
        action: "hop",
        kind: Button,
    ),
    (
        action: "flip",
        kind: Button,
    ),
    (
        action: "zoom",
        kind: Axis1,
        mouse: ["ControlLeft+Scroll"],
        gamepad: ["PadTrigger:Right>0.25"],
    ),
    (
        action: "confirm",
        kind: Button,
        context: "menu",
        keyboard: ["Enter"],
        gamepad: ["Pad:East"],
        patterns: [DoubleTapOnRelease(150, 300, "jump_charge")],
    ),
]
"#;

/// Sixteen ticks a second, exact in binary, as `patterns.rs`'s tests use:
/// the asset's 250 ms is four of them exactly.
const TICK: f32 = 0.0625;

fn load(text: &str) -> ActionMap {
    ActionMap::from_ron(text).unwrap_or_else(|error| panic!("{error}"))
}

fn refusal(text: &str) -> BindingAssetError {
    match ActionMap::from_ron(text) {
        Ok(_) => panic!("accepted:\n{text}"),
        Err(error) => error,
    }
}

fn texts(map: &ActionMap, action: &str) -> Vec<String> {
    map.bindings(action)
        .expect("declared")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_canonical_asset_writes_back_byte_for_byte() {
    assert_eq!(load(CANONICAL).to_ron().as_deref(), Ok(CANONICAL));
}

/// What the canonical asset declares, read back through the map's own
/// accessors rather than through `to_ron`.
#[test]
fn the_asset_declares_its_records_in_file_order() {
    let map = load(CANONICAL);
    assert_eq!(
        map.action_names().collect::<Vec<_>>(),
        [
            "move",
            "jump",
            "jump_charge",
            "hop",
            "flip",
            "zoom",
            "confirm"
        ]
    );
    assert!(matches!(map.action("move"), Some(ActionValue::Axis2(_))));
    assert!(matches!(map.action("zoom"), Some(ActionValue::Axis1(_))));
    assert!(matches!(map.action("hop"), Some(ActionValue::Button(_))));
    assert_eq!(map.context_of("move"), Some(GAMEPLAY_CONTEXT));
    assert_eq!(map.context_of("confirm"), Some("menu"));
    assert_eq!(map.bindings("hop"), Some(&[][..]));
    assert_eq!(
        texts(&map, "move"),
        [
            "Wasd:KeyW,KeyS,KeyA,KeyD",
            "Wasd:ArrowUp,ArrowDown,ArrowLeft,ArrowRight",
            "PadStick:Left>0.2",
            "Pad:Dpad",
            "Virtual:stick_move"
        ]
    );
    assert_eq!(map.tap("jump"), Tap::new(0.2));
    assert_eq!(map.hold("jump"), Hold::new(0.25));
    assert_eq!(map.double_tap("jump"), DoubleTap::new(0.25, 0.25));
    assert_eq!(
        map.double_tap("confirm"),
        DoubleTap::new(0.15, 0.3).map(DoubleTap::on_release)
    );
    assert_eq!(map.emits("jump", Pattern::Tap), Some("hop"));
    assert_eq!(map.emits("jump", Pattern::Hold), Some("jump_charge"));
    assert_eq!(map.emits("jump", Pattern::DoubleTap), Some("flip"));
    assert_eq!(
        map.emits("confirm", Pattern::DoubleTap),
        Some("jump_charge")
    );
    assert_eq!(map.tap("move"), None);
}

/// **The device lists are presentation.** Written in any field order, they
/// flatten keyboard, mouse, gamepad, touch — and the writer puts them back
/// in that order.
#[test]
fn device_lists_flatten_in_the_documented_order() {
    let map = load(
        r#"[(action: "jump", kind: Button, touch: ["Virtual:jump"], gamepad: ["Pad:South"], mouse: ["Mouse:Left"], keyboard: ["Space", "KeyJ"])]"#,
    );
    assert_eq!(
        texts(&map, "jump"),
        ["Space", "KeyJ", "Mouse:Left", "Pad:South", "Virtual:jump"]
    );
    let written = map.to_ron().expect("one device after another");
    let at: Vec<usize> = DEVICE_LISTS
        .iter()
        .map(|device| {
            let field = list_name(*device);
            written.find(&format!("{field}: ")).expect("written")
        })
        .collect();
    assert!(at.is_sorted(), "{written}");
}

/// One binding of every variant, each in its device's list, comes back as
/// itself — so every variant has a list it is accepted in.
#[test]
fn every_binding_variant_has_a_list_and_reads_back() {
    let all = [
        Binding::Key(KeyCode::KeyR),
        Binding::Chord {
            modifier: crate::Modifier::Alt,
            key: KeyCode::KeyR,
        },
        Binding::KeyAxis {
            negative: KeyCode::KeyS,
            positive: KeyCode::KeyW,
        },
        Binding::Wasd {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        },
        Binding::MouseButton(PointerButton::Right),
        Binding::ButtonChord {
            modifier: crate::Modifier::Alt,
            button: PointerButton::Right,
        },
        Binding::MouseMotion,
        Binding::MouseScroll,
        Binding::ScrollChord {
            held: KeyCode::ControlLeft,
        },
        Binding::PointerPosition {
            axis: crate::PointerAxis::X,
        },
        Binding::PadButton(crate::PadButton::South),
        Binding::PadChord {
            modifier: crate::PadButton::LeftShoulder,
            button: crate::PadButton::South,
        },
        Binding::PadDpad,
        Binding::PadStick {
            stick: crate::Stick::Left,
            deadzone: 0.2,
        },
        Binding::PadTrigger {
            trigger: crate::Trigger::Right,
            threshold: 0.25,
        },
        Binding::Virtual("stick".to_owned()),
    ];
    let kinds: std::collections::HashSet<_> = all.iter().map(std::mem::discriminant).collect();
    assert_eq!(kinds.len(), 16, "one of each Binding variant");
    let mut map = ActionMap::new();
    map.declare(ActionDecl {
        name: "all".to_owned(),
        kind: ActionKind::Axis2,
        bindings: all.to_vec(),
    });
    let written = map.to_ron().expect("listed device by device");
    assert_eq!(load(&written).bindings("all"), Some(&all[..]));
}

/// **A pattern emits its named action**: the asset's hold presses
/// `jump_charge` on the tick it fires, and its tap presses `hop`.
#[test]
fn the_assets_patterns_emit_their_named_actions() {
    let mut map = load(CANONICAL);
    let mut charged = Vec::new();
    for tick in 0..8 {
        map.begin_tick(TICK);
        if tick == 0 {
            map.key_event(KeyCode::Space, true);
        }
        if map.just_pressed("jump_charge") {
            charged.push(tick);
        }
    }
    assert_eq!(charged, [4], "the 250 ms hold is four ticks");

    map.key_event(KeyCode::Space, false);
    map.begin_tick(TICK);
    map.key_event(KeyCode::Space, true);
    map.begin_tick(TICK);
    map.key_event(KeyCode::Space, false);
    // A double tap is attached, so the tap waits out its window first.
    let mut hopped = Vec::new();
    for tick in 0..8 {
        map.begin_tick(TICK);
        if map.just_pressed("hop") {
            hopped.push(tick);
        }
    }
    assert_eq!(hopped, [4], "one tick past the 250 ms window");
}

/// The bad record is always on line 3, after a good one on line 2.
fn on_line_three(record: &str) -> String {
    let lines = [
        "[",
        r#"    (action: "ok", kind: Button, keyboard: ["KeyO"]),"#,
        &format!("    {record},"),
        r#"    (action: "charge", kind: Button),"#,
        r#"    (action: "aim", kind: Axis1),"#,
        "]",
    ];
    lines.join("\n")
}

/// **Every refusal names what was wrong and where.**
#[test]
fn each_refusal_names_the_record_and_its_line() {
    let named = |action: &str| action.to_owned();
    for (record, expected) in [
        (
            r#"(action: "jump", kind: Button, patterns: [Hold(400, "jump_chrge")])"#,
            AssetRefusal::UnknownEmit {
                action: named("jump"),
                pattern: Pattern::Hold,
                emits: named("jump_chrge"),
            },
        ),
        (
            r#"(action: "jump", kind: Button, gamepad: ["Space"])"#,
            AssetRefusal::WrongList {
                action: named("jump"),
                binding: named("Space"),
                list: Device::Gamepad,
            },
        ),
        (
            r#"(action: "jump", kind: Button, keyboard: ["Pad:South"])"#,
            AssetRefusal::WrongList {
                action: named("jump"),
                binding: named("Pad:South"),
                list: Device::Keyboard,
            },
        ),
        (
            r#"(action: "jump", kind: Button, mouse: ["Virtual:jump"])"#,
            AssetRefusal::WrongList {
                action: named("jump"),
                binding: named("Virtual:jump"),
                list: Device::Pointer,
            },
        ),
        (
            r#"(action: "jump", kind: Button, touch: ["Mouse:Left"])"#,
            AssetRefusal::WrongList {
                action: named("jump"),
                binding: named("Mouse:Left"),
                list: Device::Touch,
            },
        ),
        (
            r#"(action: "jump", kind: Button, keyboard: ["Spacebar"])"#,
            AssetRefusal::BadBinding {
                action: named("jump"),
                error: BindingParseError {
                    text: named("Spacebar"),
                    reason: "no such key",
                },
            },
        ),
        (
            r#"(action: "ok", kind: Button)"#,
            AssetRefusal::Declare(ActionMapError::DuplicateName(named("ok"))),
        ),
        (
            r#"(action: "stick", kind: Axis2, gamepad: ["PadStick:Left>1.5"])"#,
            AssetRefusal::Declare(ActionMapError::InvalidDeadzone(named("stick"))),
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [Tap(100, "charge"), Tap(200, "charge")])"#,
            AssetRefusal::DuplicatePattern {
                action: named("jump"),
                pattern: Pattern::Tap,
            },
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [DoubleTap(100, 100, "charge"), DoubleTapOnRelease(100, 100, "charge")])"#,
            AssetRefusal::DuplicatePattern {
                action: named("jump"),
                pattern: Pattern::DoubleTap,
            },
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [Hold(400, "jump")])"#,
            AssetRefusal::EmitsItself {
                action: named("jump"),
                pattern: Pattern::Hold,
            },
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [Hold(400, "aim")])"#,
            AssetRefusal::EmitNotAButton {
                action: named("jump"),
                pattern: Pattern::Hold,
                emits: named("aim"),
            },
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [Hold(0, "charge")])"#,
            AssetRefusal::BadTime {
                action: named("jump"),
                pattern: Pattern::Hold,
            },
        ),
        (
            r#"(action: "jump", kind: Button, patterns: [DoubleTap(100, 0, "charge")])"#,
            AssetRefusal::BadTime {
                action: named("jump"),
                pattern: Pattern::DoubleTap,
            },
        ),
    ] {
        let error = refusal(&on_line_three(record));
        assert_eq!(error.refusal(), &expected, "{record}");
        assert_eq!(error.line(), 3, "{record}: {error}");
        assert!(error.column() > 1, "{record}: {error}");
        assert!(error.to_string().starts_with("line 3, column "), "{error}");
    }
}

/// What ron refuses — an unknown kind, an unknown field, a missing field, an
/// unknown pattern — it refuses by name, at the line it is on.
#[test]
fn ron_refusals_name_the_word_and_its_line() {
    for (record, word) in [
        (r#"(action: "jump", kind: Buton)"#, "Buton"),
        (
            r#"(action: "jump", kind: Button, keybaord: ["Space"])"#,
            "keybaord",
        ),
        (r#"(action: "jump", keyboard: ["Space"])"#, "kind"),
        (
            r#"(action: "jump", kind: Button, patterns: [Press(1, "charge")])"#,
            "Press",
        ),
    ] {
        let error = refusal(&on_line_three(record));
        let AssetRefusal::Parse(message) = error.refusal() else {
            panic!("{record}: {error}");
        };
        assert!(message.contains(word), "{record}: {message}");
        assert_eq!(error.line(), 3, "{record}: {error}");
    }
    let error = refusal("[\n    (action: \"jump\", kind: Button,\n");
    assert!(matches!(error.refusal(), AssetRefusal::Parse(_)), "{error}");
}

/// **Rebinds are diffs over the asset's defaults**: the overrides name only
/// what the player changed, apply to a freshly loaded map, and leave what
/// `to_ron` writes alone.
#[test]
fn rebind_overrides_are_diffs_over_the_asset_defaults() {
    let mut played = load(CANONICAL);
    let rebound: Vec<Binding> = ["KeyK", "Pad:North"]
        .iter()
        .map(|text| text.parse().expect("a binding"))
        .collect();
    played.rebind("jump", rebound.clone()).expect("declared");
    let saved = played.overrides();
    assert_eq!(
        saved,
        [ActionOverride {
            action: "jump".to_owned(),
            bindings: rebound,
        }]
    );
    assert_eq!(
        played.to_ron().as_deref(),
        Ok(CANONICAL),
        "the asset is the defaults"
    );

    let mut fresh = load(CANONICAL);
    assert_eq!(fresh.apply_overrides(&saved), []);
    assert_eq!(texts(&fresh, "jump"), ["KeyK", "Pad:North"]);
    assert_eq!(fresh.overrides(), saved);
    assert_eq!(fresh.apply_overrides(&[]), []);
    assert_eq!(
        texts(&fresh, "jump"),
        ["Space", "Mouse:Left", "Pad:South"],
        "back to the asset's defaults"
    );
}

/// A map the schema cannot say is refused by name, never written as a file
/// that would read back as a different map.
#[test]
fn to_ron_refuses_what_would_read_back_differently() {
    let button = |name: &str, bindings: Vec<Binding>| ActionDecl {
        name: name.to_owned(),
        kind: ActionKind::Button,
        bindings,
    };
    let mut mixed = ActionMap::new();
    mixed.declare(button(
        "jump",
        vec![
            Binding::Key(KeyCode::Space),
            Binding::PadButton(crate::PadButton::South),
            Binding::Key(KeyCode::KeyJ),
        ],
    ));
    assert_eq!(
        mixed.to_ron(),
        Err(AssetWriteError::MixedLists("jump".to_owned()))
    );

    let mut repeat = ActionMap::new();
    repeat.declare(button("next", Vec::new()));
    repeat
        .set_repeat("next", Some(Repeat::UI))
        .expect("declared");
    assert_eq!(
        repeat.to_ron(),
        Err(AssetWriteError::Repeat("next".to_owned()))
    );

    let mut unnamed = ActionMap::new();
    unnamed.declare(button("jump", Vec::new()));
    unnamed
        .set_hold("jump", Some(Hold::default()))
        .expect("declared");
    assert_eq!(
        unnamed.to_ron(),
        Err(AssetWriteError::NoEmit {
            action: "jump".to_owned(),
            pattern: Pattern::Hold
        })
    );

    let mut fraction = ActionMap::new();
    fraction.declare(button("jump", Vec::new()));
    fraction.declare(button("charge", Vec::new()));
    fraction
        .set_tap("jump", Tap::new(0.000_25))
        .expect("declared");
    fraction
        .set_emits("jump", Pattern::Tap, Some("charge"))
        .expect("a button");
    assert_eq!(
        fraction.to_ron(),
        Err(AssetWriteError::NotWholeMilliseconds {
            action: "jump".to_owned(),
            pattern: Pattern::Tap
        })
    );
}

/// The engine's own defaults are whole milliseconds, so a pattern built from
/// them writes and reads back exactly.
#[test]
fn the_default_pattern_times_are_whole_milliseconds() {
    for seconds in [crate::TAP_TIME, crate::HOLD_TIME, crate::DOUBLE_TAP_WINDOW] {
        let ms = whole_ms(seconds).expect("whole");
        assert_eq!(f32::from(ms) / MS_PER_SECOND, seconds);
    }
    assert_eq!(whole_ms(f32::NAN), None);
    assert_eq!(whole_ms(100_000.0), None, "past a u16 of milliseconds");
}
