//! Steam Input over the fake: opening and closing, the pump's part, device
//! callbacks, the action reads, and the manifest the reads rely on.

use std::collections::HashMap;

use crcbl_input::{ActionDecl, ActionKind, ActionMap, Binding, PadButtons};

use super::*;
use crate::{
    AppId,
    client::init_on,
    testing::{self, FakeInput, FakeMsg, FakePad},
};

const MANIFEST: &CStr = c"/games/ew/crcbl_pad.vdf";
const PAD: InputHandle = 0x0001_0000_0000_0042;

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn input<R>(f: impl FnOnce(&mut FakeInput) -> R) -> R {
    testing::script(|s| f(&mut s.input))
}

fn open(steam: &mut Steam) -> SteamPads {
    SteamPads::open_at(steam, MANIFEST).unwrap()
}

/// Queues the device callback Steam sends when `handle` connects or
/// disconnects.
fn device(handle: InputHandle, connected: bool) {
    let id = if connected { 2801 } else { 2802 };
    testing::script(|s| {
        s.queue
            .push_back(FakeMsg::payload(id, handle.to_le_bytes().to_vec()));
    });
}

/// Pumps, then polls, collecting the events.
fn frame(steam: &mut Steam, pads: &mut SteamPads) -> Vec<GamepadEvent> {
    steam.pump();
    let mut events = Vec::new();
    pads.poll(|event| events.push(event));
    events
}

/// A connected DualSense-type pad holding `held`, with `analog` values.
fn script_pad(held: &[&'static str], analog: &[(&'static str, (f32, f32))]) {
    input(|input| {
        input.pads.insert(
            PAD,
            FakePad {
                input_type: input_type::PS5,
                held: held.to_vec(),
                analog: analog.iter().copied().collect::<HashMap<_, _>>(),
                inactive: Vec::new(),
            },
        );
    });
}

#[test]
fn open_owns_run_frame_registers_the_manifest_and_enables_device_callbacks() {
    let mut steam = steam();
    let pads = open(&mut steam);
    input(|input| {
        assert_eq!(
            input.log,
            [
                "Init",
                "SetInputActionManifestFilePath",
                "EnableDeviceCallbacks"
            ]
        );
        assert_eq!(
            input.explicit_run_frame,
            Some(true),
            "the pump owns RunFrame"
        );
        assert_eq!(input.manifest.as_deref(), Some("/games/ew/crcbl_pad.vdf"));
    });
    drop(pads);
    assert_eq!(input(|input| input.log.last().copied()), Some("Shutdown"));
}

#[test]
fn one_open_at_a_time_and_another_after_the_first_drops() {
    let mut steam = steam();
    let first = open(&mut steam);
    assert_eq!(
        SteamPads::open_at(&mut steam, MANIFEST).unwrap_err(),
        InputError::AlreadyOpen
    );
    assert_eq!(
        input(|input| input.log.len()),
        3,
        "the refusal called nothing"
    );
    drop(first);
    let _second = open(&mut steam);
}

#[test]
fn a_refused_init_calls_nothing_else_and_a_refused_manifest_shuts_down() {
    let mut steam = steam();
    input(|input| input.refuse_init = true);
    assert_eq!(
        SteamPads::open_at(&mut steam, MANIFEST).unwrap_err(),
        InputError::Steam(SteamError::Refused("ISteamInput::Init"))
    );
    assert_eq!(input(|input| input.log.clone()), ["Init"]);

    input(|input| {
        input.refuse_init = false;
        input.refuse_manifest = true;
        input.log.clear();
    });
    assert_eq!(
        SteamPads::open_at(&mut steam, MANIFEST).unwrap_err(),
        InputError::Steam(SteamError::Refused("SetInputActionManifestFilePath"))
    );
    assert_eq!(
        input(|input| input.log.clone()),
        ["Init", "SetInputActionManifestFilePath", "Shutdown"]
    );
    assert_eq!(steam.pads.strong_count(), 0, "and nothing is open");
}

/// The path check reads the filesystem, so it does not run under Miri.
#[cfg_attr(miri, ignore)]
#[test]
fn the_manifest_must_be_an_absolute_path_to_a_file() {
    let mut steam = steam();
    let relative = SteamPads::open(&mut steam, Path::new("crcbl_pad.vdf")).unwrap_err();
    assert!(
        matches!(
            relative,
            InputError::Manifest {
                reason: "not an absolute path",
                ..
            }
        ),
        "{relative:?}"
    );
    let dir = std::env::temp_dir();
    let missing = SteamPads::open(&mut steam, &dir.join("no-such-crcbl-pad.vdf")).unwrap_err();
    assert!(
        matches!(
            missing,
            InputError::Manifest {
                reason: "not a file",
                ..
            }
        ),
        "{missing:?}"
    );
    assert!(input(|input| input.log.is_empty()), "Steam was never asked");

    let file = dir.join(format!("crcbl-pad-{}.vdf", std::process::id()));
    std::fs::write(&file, PAD_MANIFEST).unwrap();
    let pads = SteamPads::open(&mut steam, &file);
    std::fs::remove_file(&file).unwrap();
    pads.unwrap();
    assert_eq!(
        input(|input| input.manifest.clone()),
        file.to_str().map(str::to_owned)
    );
}

#[test]
fn the_pump_runs_steam_inputs_frame_only_while_pads_are_open() {
    let mut steam = steam();
    steam.pump();
    assert!(input(|input| input.log.is_empty()));
    let pads = open(&mut steam);
    steam.pump();
    steam.pump();
    let frames = input(|input| input.log.iter().filter(|call| **call == "RunFrame").count());
    assert_eq!(frames, 2);
    drop(pads);
    steam.pump();
    let frames = input(|input| input.log.iter().filter(|call| **call == "RunFrame").count());
    assert_eq!(frames, 2, "none after Shutdown");
}

#[test]
fn a_device_callback_with_no_pads_open_is_counted_and_dropped() {
    let mut steam = steam();
    device(PAD, true);
    steam.pump();
    assert_eq!(steam.diagnostics().unknown, 1);
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

/// **A re-plugged controller keeps its id**: Steam's handle names the
/// controller, so a disconnect and reconnect is the same `GamepadId`, and a
/// second controller gets another.
#[test]
fn device_callbacks_connect_and_disconnect_and_a_returning_handle_keeps_its_id() {
    let mut steam = steam();
    let mut pads = open(&mut steam);
    script_pad(&[], &[]);
    device(PAD, true);
    let events = frame(&mut steam, &mut pads);
    let [GamepadEvent::Connected { id, kind }] = events[..] else {
        panic!("a connection at rest is one event: {events:?}");
    };
    assert_eq!(kind, PadKind::PlayStation);

    device(PAD, true);
    assert_eq!(frame(&mut steam, &mut pads), [], "a repeat changes nothing");

    device(PAD, false);
    assert_eq!(
        frame(&mut steam, &mut pads),
        [GamepadEvent::Disconnected { id }]
    );
    device(PAD, false);
    assert_eq!(frame(&mut steam, &mut pads), [], "nor does a repeat here");

    device(PAD, true);
    assert_eq!(
        frame(&mut steam, &mut pads),
        [GamepadEvent::Connected { id, kind }],
        "the same id"
    );

    input(|input| {
        input.pads.insert(7, FakePad::default());
    });
    device(7, true);
    let events = frame(&mut steam, &mut pads);
    let [GamepadEvent::Connected { id: other, kind }] = events[..] else {
        panic!("{events:?}");
    };
    assert_ne!(other, id);
    assert_eq!(kind, PadKind::Generic);
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

/// Held buttons and stick deflections arrive as a snapshot, positionally,
/// with the pad action set activated on the controller first.
#[test]
fn actions_read_through_the_by_value_returns_arrive_as_a_snapshot() {
    let mut steam = steam();
    let mut pads = open(&mut steam);
    script_pad(
        &["south", "dpad_left", "left_stick_click"],
        &[("left_stick", (0.25, 1.0)), ("right_trigger", (0.5, 0.0))],
    );
    device(PAD, true);
    let events = frame(&mut steam, &mut pads);
    let [
        GamepadEvent::Connected { id, .. },
        GamepadEvent::State { id: same, snapshot },
    ] = events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(same, id);
    assert_eq!(
        snapshot.buttons,
        [PadButton::South, PadButton::DpadLeft, PadButton::LeftStick]
            .into_iter()
            .collect::<PadButtons>()
    );
    assert_eq!(snapshot.stick(Stick::Left), (0.25, 1.0), "+Y up, unflipped");
    assert_eq!(snapshot.trigger(Trigger::Right), 0.5);
    let set = input(|input| {
        let set = input.names.iter().position(|name| name == "pad").unwrap() + 1;
        assert!(
            input
                .activated
                .contains(&(PAD, u64::try_from(set).unwrap()))
        );
        set
    });
    assert_ne!(set, 0);

    assert_eq!(
        frame(&mut steam, &mut pads),
        [],
        "an unchanged pad is quiet"
    );
    script_pad(&[], &[]);
    assert_eq!(
        frame(&mut steam, &mut pads),
        [GamepadEvent::State {
            id,
            snapshot: GamepadSnapshot::neutral(PadKind::PlayStation),
        }]
    );
    assert_eq!(pads.rejected_axes(), 0);
}

/// Each digital action lands on its own button, and nothing else.
#[test]
fn every_digital_action_is_its_own_button() {
    for (index, &(_, button)) in BUTTONS.iter().enumerate() {
        let mut reading = Reading::default();
        reading.buttons[index] = InputDigitalActionData {
            state: 1,
            active: 1,
        };
        let (snapshot, _) = snapshot_of(PadKind::Generic, &reading);
        assert_eq!(
            snapshot.buttons,
            [button].into_iter().collect::<PadButtons>(),
            "{button:?}"
        );
    }
}

/// Sticks clamp to −1…1 and triggers to 0…1; a value that is not finite reads
/// as centred and is counted; an inactive action reads released or centred.
#[test]
fn axes_are_clamped_non_finite_values_rejected_and_inactive_actions_neutral() {
    let analog = |x, y| InputAnalogActionData {
        mode: 0,
        x,
        y,
        active: 1,
    };
    let mut reading = Reading::default();
    reading.sticks[0] = analog(1.5, -3.0);
    reading.sticks[1] = analog(f32::NAN, 0.5);
    reading.triggers[0] = analog(-0.2, 0.0);
    reading.triggers[1] = analog(f32::INFINITY, 0.0);
    let (snapshot, rejected) = snapshot_of(PadKind::Xbox, &reading);
    assert_eq!(snapshot.stick(Stick::Left), (1.0, -1.0));
    assert_eq!(snapshot.stick(Stick::Right), (0.0, 0.5));
    assert_eq!(snapshot.trigger(Trigger::Left), 0.0);
    assert_eq!(snapshot.trigger(Trigger::Right), 0.0);
    assert_eq!(rejected, 2);
    assert!(snapshot.is_finite());

    let mut reading = Reading::default();
    reading.sticks[0] = InputAnalogActionData {
        active: 0,
        ..analog(1.0, 1.0)
    };
    reading.buttons[0] = InputDigitalActionData {
        state: 1,
        active: 0,
    };
    let (snapshot, _) = snapshot_of(PadKind::Xbox, &reading);
    assert_eq!(snapshot, GamepadSnapshot::neutral(PadKind::Xbox));
}

#[test]
fn input_types_name_their_family() {
    for (raw, kind) in [
        (0, PadKind::Generic),
        (1, PadKind::Generic),
        (input_type::XBOX_360, PadKind::Xbox),
        (input_type::XBOX_ONE, PadKind::Xbox),
        (4, PadKind::Generic),
        (input_type::PS4, PadKind::PlayStation),
        (input_type::SWITCH_JOY_CON_PAIR, PadKind::Switch),
        (input_type::SWITCH_JOY_CON_SINGLE, PadKind::Switch),
        (input_type::SWITCH_PRO, PadKind::Switch),
        (input_type::PS3, PadKind::PlayStation),
        (input_type::PS5, PadKind::PlayStation),
        (input_type::STEAM_DECK, PadKind::SteamDeck),
        (input_type::SWITCH_2_PRO, PadKind::Switch),
        (255, PadKind::Generic),
    ] {
        assert_eq!(kind_of(raw), kind, "{raw}");
    }
}

/// While Steam answers `0` for the handles, nothing is read; the lookups are
/// made again each poll, and the reads start once Steam answers.
#[test]
fn handles_are_looked_up_again_until_steam_answers_them() {
    let mut steam = steam();
    let mut pads = open(&mut steam);
    input(|input| input.not_ready = true);
    script_pad(&["north"], &[]);
    device(PAD, true);
    let events = frame(&mut steam, &mut pads);
    assert!(
        matches!(events[..], [GamepadEvent::Connected { .. }]),
        "{events:?}"
    );
    let lookups = input(|input| input.lookups.clone());
    assert_eq!(lookups, ["pad"], "no action lookups before the set's");
    assert!(input(|input| input.activated.is_empty()));

    input(|input| input.not_ready = false);
    let events = frame(&mut steam, &mut pads);
    let [GamepadEvent::State { snapshot, .. }] = events[..] else {
        panic!("{events:?}");
    };
    assert!(snapshot.buttons.contains(PadButton::North));
    let resolved = input(|input| input.lookups.len());
    frame(&mut steam, &mut pads);
    assert_eq!(
        input(|input| input.lookups.len()),
        resolved,
        "no lookups once every handle is answered"
    );
}

/// An action the manifest Steam loaded lacks is never read — it reads
/// released — and does not stop the others.
#[test]
fn an_action_steam_never_answers_reads_released() {
    let mut steam = steam();
    let mut pads = open(&mut steam);
    input(|input| input.missing = vec!["east"]);
    script_pad(&["east", "west"], &[]);
    device(PAD, true);
    let events = frame(&mut steam, &mut pads);
    let [_, GamepadEvent::State { snapshot, .. }] = events[..] else {
        panic!("{events:?}");
    };
    assert_eq!(
        snapshot.buttons,
        [PadButton::West].into_iter().collect::<PadButtons>()
    );
}

/// **EW requirement 4, as a test**: an `ActionMap` resolves the Steam
/// backend's events exactly as it resolves a hand-built event carrying the
/// same snapshot — the seam cannot tell who spoke.
#[test]
fn an_action_map_cannot_tell_steam_input_from_a_hand_built_event() {
    fn map() -> ActionMap {
        let mut map = ActionMap::new();
        for (name, kind, binding) in [
            (
                "jump",
                ActionKind::Button,
                Binding::PadButton(PadButton::South),
            ),
            (
                "move",
                ActionKind::Axis2,
                Binding::PadStick {
                    stick: Stick::Left,
                    deadzone: 0.2,
                },
            ),
            (
                "fire",
                ActionKind::Axis1,
                Binding::PadTrigger {
                    trigger: Trigger::Right,
                    threshold: 0.1,
                },
            ),
        ] {
            map.declare(ActionDecl {
                name: name.to_owned(),
                kind,
                bindings: vec![binding],
            });
        }
        map
    }

    let mut steam = steam();
    let mut pads = open(&mut steam);
    script_pad(
        &["south"],
        &[("left_stick", (0.6, -0.3)), ("right_trigger", (0.7, 0.0))],
    );
    device(PAD, true);
    let events = frame(&mut steam, &mut pads);

    let mut from_steam = map();
    for event in &events {
        from_steam.gamepad_event(event);
    }
    let mut by_hand = map();
    let id = GamepadId(9_999);
    let mut snapshot = GamepadSnapshot::neutral(PadKind::PlayStation);
    snapshot.buttons.insert(PadButton::South);
    snapshot.axes[PadAxis::LeftX as usize] = 0.6;
    snapshot.axes[PadAxis::LeftY as usize] = -0.3;
    snapshot.axes[PadAxis::RightTrigger as usize] = 0.7;
    by_hand.gamepad_event(&GamepadEvent::Connected {
        id,
        kind: PadKind::PlayStation,
    });
    by_hand.gamepad_event(&GamepadEvent::State { id, snapshot });

    assert!(from_steam.button_held("jump"));
    assert_eq!(from_steam.button_held("jump"), by_hand.button_held("jump"));
    assert_eq!(
        from_steam.just_pressed("jump"),
        by_hand.just_pressed("jump")
    );
    // Equal, to within the noise Miri deliberately adds to `hypot` on each
    // call; a natively built run computes the two bit for bit alike.
    let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
    let (steam_move, hand_move) = (from_steam.axis2("move"), by_hand.axis2("move"));
    assert!(
        close(steam_move.0, hand_move.0) && close(steam_move.1, hand_move.1),
        "{steam_move:?} {hand_move:?}"
    );
    assert!(close(from_steam.axis1("fire"), by_hand.axis1("fire")));
    assert_ne!(steam_move, (0.0, 0.0));
}

/// A small reader for Valve's KeyValues text — quoted strings, braces and
/// `//` comments, all [`PAD_MANIFEST`] uses — as a tree of `(key, value)`.
#[derive(Debug, Clone, PartialEq)]
enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    fn parse(text: &str) -> Vec<(String, Self)> {
        let mut tokens = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => tokens.push(chars.by_ref().take_while(|&c| c != '"').collect()),
                '{' | '}' => tokens.push(c.to_string()),
                '/' if chars.peek() == Some(&'/') => {
                    chars.by_ref().take_while(|&c| c != '\n').for_each(drop);
                }
                c if c.is_whitespace() => {}
                other => panic!("unexpected {other:?} in the manifest"),
            }
        }
        let mut tokens = tokens.into_iter();
        let block = Self::block(&mut tokens);
        assert!(tokens.next().is_none(), "text after the root block");
        block
    }

    fn block(tokens: &mut impl Iterator<Item = String>) -> Vec<(String, Self)> {
        let mut entries = Vec::new();
        while let Some(key) = tokens.next() {
            if key == "}" {
                break;
            }
            let value = tokens.next().expect("a key without a value");
            let value = if value == "{" {
                Self::Block(Self::block(tokens))
            } else {
                Self::Text(value)
            };
            entries.push((key, value));
        }
        entries
    }

    fn get(&self, key: &str) -> &Self {
        let Self::Block(entries) = self else {
            panic!("{key} looked up in a text value");
        };
        let found: Vec<_> = entries.iter().filter(|(k, _)| k == key).collect();
        assert_eq!(found.len(), 1, "{key} appears {} times", found.len());
        &found[0].1
    }

    fn text(&self) -> &str {
        let Self::Text(text) = self else {
            panic!("a block where text was expected");
        };
        text
    }

    fn keys(&self) -> Vec<&str> {
        let Self::Block(entries) = self else {
            panic!("keys of a text value");
        };
        entries.iter().map(|(k, _)| k.as_str()).collect()
    }
}

/// **The manifest and the reads agree**: every action this backend looks up
/// is declared in the category its read needs — digital in `Button`, sticks
/// as `joystick_move` in `StickPadGyro`, triggers in `AnalogTrigger` — the
/// set is the one looked up, nothing else is declared, and every title has
/// an English string.
#[test]
fn the_manifest_declares_exactly_the_actions_the_backend_reads() {
    let root = Kv::parse(PAD_MANIFEST);
    assert_eq!(root.len(), 1);
    let (name, manifest) = &root[0];
    assert_eq!(name, "Action Manifest");
    assert_eq!(
        manifest.keys(),
        ["configurations", "actions", "localization"]
    );

    let actions = manifest.get("actions");
    assert_eq!(actions.keys(), [ACTION_SET.to_str().unwrap()]);
    let set = actions.get(ACTION_SET.to_str().unwrap());
    let english = manifest.get("localization").get("english");
    let localized = |token: &str| {
        let key = token.strip_prefix('#').expect("a title is a # token");
        assert!(!english.get(key).text().is_empty(), "{key}");
    };
    localized(set.get("title").text());

    let buttons = set.get("Button");
    let names: Vec<&str> = BUTTONS.iter().map(|(n, _)| n.to_str().unwrap()).collect();
    assert_eq!(buttons.keys(), names);
    for name in names {
        localized(buttons.get(name).text());
    }

    let sticks = set.get("StickPadGyro");
    let names: Vec<&str> = STICKS.iter().map(|(n, _)| n.to_str().unwrap()).collect();
    assert_eq!(sticks.keys(), names);
    for name in names {
        let stick = sticks.get(name);
        assert_eq!(stick.get("input_mode").text(), "joystick_move", "{name}");
        localized(stick.get("title").text());
    }

    let triggers = set.get("AnalogTrigger");
    let names: Vec<&str> = TRIGGERS.iter().map(|(n, _)| n.to_str().unwrap()).collect();
    assert_eq!(triggers.keys(), names);
    for name in names {
        localized(triggers.get(name).text());
    }
    assert_eq!(
        set.keys(),
        ["title", "StickPadGyro", "AnalogTrigger", "Button"]
    );
}

#[test]
fn the_keyvalues_reader_reads_nesting_and_skips_comments() {
    let parsed = Kv::parse("// c\n\"a\" { \"b\" \"c\" // d\n \"e\" { } }");
    assert_eq!(
        parsed,
        [(
            "a".to_owned(),
            Kv::Block(vec![
                ("b".to_owned(), Kv::Text("c".to_owned())),
                ("e".to_owned(), Kv::Block(Vec::new())),
            ])
        )]
    );
}

/// Opens pads with [`PAD`] connected and its handles resolved.
fn connected(steam: &mut Steam) -> (SteamPads, GamepadId) {
    let mut pads = open(steam);
    script_pad(&[], &[]);
    device(PAD, true);
    let events = frame(steam, &mut pads);
    let [GamepadEvent::Connected { id, .. }] = events[..] else {
        panic!("{events:?}");
    };
    (pads, id)
}

/// **A glyph is the first origin's PNG, copied before it returns**: the
/// buffer Steam answered from is overwritten after the call, and the path
/// survives.
#[test]
fn a_buttons_glyph_is_its_first_origins_png_copied_out() {
    let mut steam = steam();
    let (pads, id) = connected(&mut steam);
    input(|input| {
        input.origins.insert("south", vec![5, 9]);
    });
    testing::script(|s| s.set_string(b"/glyphs/ps5_cross.png"));
    let path = pads.glyph(
        &steam,
        id,
        PadControl::Button(PadButton::South),
        GlyphSize::Medium,
    );
    testing::script(|s| s.set_string(b"/glyphs/overwritten"));
    assert_eq!(path, Some(PathBuf::from("/glyphs/ps5_cross.png")));
    assert_eq!(input(|input| input.glyphs.clone()), [(5, 1, 0)]);
}

#[test]
fn sticks_and_triggers_take_their_analog_origins() {
    let mut steam = steam();
    let (pads, id) = connected(&mut steam);
    input(|input| {
        input.origins.insert("right_stick", vec![40]);
        input.origins.insert("left_trigger", vec![41]);
    });
    testing::script(|s| s.set_string(b"/glyphs/any.png"));
    for (control, origin, size, raw) in [
        (PadControl::Stick(Stick::Right), 40, GlyphSize::Small, 0),
        (PadControl::Trigger(Trigger::Left), 41, GlyphSize::Large, 2),
    ] {
        assert!(
            pads.glyph(&steam, id, control, size).is_some(),
            "{control:?}"
        );
        assert_eq!(
            input(|input| input.glyphs.last().copied()),
            Some((origin, raw, 0))
        );
    }
}

/// Nothing bound, bound to `k_EInputActionOrigin_None`, no image from
/// Steam, an empty path, or a pad that is not connected here: `None`, and no
/// glyph asked for when there is no origin to ask about.
#[test]
fn no_binding_no_image_or_no_such_pad_is_none() {
    let mut steam = steam();
    let (pads, id) = connected(&mut steam);
    let south = PadControl::Button(PadButton::South);
    assert_eq!(pads.glyph(&steam, id, south, GlyphSize::Small), None);
    input(|input| {
        input.origins.insert("south", vec![0]);
    });
    assert_eq!(pads.glyph(&steam, id, south, GlyphSize::Small), None);
    assert!(input(|input| input.glyphs.is_empty()));

    input(|input| {
        input.origins.insert("south", vec![5]);
        input.no_glyph = true;
    });
    assert_eq!(pads.glyph(&steam, id, south, GlyphSize::Small), None);
    input(|input| input.no_glyph = false);
    testing::script(|s| s.set_string(b""));
    assert_eq!(pads.glyph(&steam, id, south, GlyphSize::Small), None);
    assert_eq!(
        steam.diagnostics().lossy_strings,
        0,
        "a null is not lossy here"
    );

    testing::script(|s| s.set_string(b"/glyphs/a.png"));
    assert_eq!(
        pads.glyph(&steam, GamepadId(id.0 + 1), south, GlyphSize::Small),
        None
    );
}
