use crcbl_core::input::{KeyCode, PointerButton};

use super::*;
use crate::{
    ActionDecl, GamepadEvent, GamepadId, GamepadSnapshot, InputTickState, PadAxis, PadKind,
    PointerAxis,
};

const PAD: GamepadId = GamepadId(7);

/// One fixed tick at 60 Hz.
const TICK: f32 = 1.0 / 60.0;

/// One thing a test does to a map.
type Step = Box<dyn Fn(&mut ActionMap)>;

fn decl(name: &str, kind: ActionKind, bindings: Vec<Binding>) -> ActionDecl {
    ActionDecl {
        name: name.to_owned(),
        kind,
        bindings,
    }
}

fn button(name: &str, bindings: Vec<Binding>) -> ActionDecl {
    decl(name, ActionKind::Button, bindings)
}

/// `jump` on Space in gameplay, `accept` on Space and Enter in `menu`, and
/// `close` on Escape in a modal `inventory`.
fn stacked() -> ActionMap {
    let mut map = ActionMap::new();
    map.declare(button("jump", vec![Binding::Key(KeyCode::Space)]));
    map.declare(button("crouch", vec![Binding::Key(KeyCode::KeyC)]));
    map.declare_in(
        "menu",
        button(
            "accept",
            vec![Binding::Key(KeyCode::Space), Binding::Key(KeyCode::Enter)],
        ),
    );
    map.declare_in(
        "inventory",
        button("close", vec![Binding::Key(KeyCode::Escape)]),
    );
    map.set_tracing(true);
    map
}

/// The newest entry's outcome.
fn newest(map: &ActionMap) -> &Outcome {
    &map.trace().next_back().expect("an entry").outcome
}

fn read(action: &str, binding: Binding) -> TracedRead {
    TracedRead {
        action: action.to_owned(),
        binding,
    }
}

fn tap(map: &mut ActionMap, key: KeyCode) {
    map.key_event(key, true);
    map.key_event(key, false);
}

fn pad(map: &mut ActionMap, edit: impl FnOnce(&mut GamepadSnapshot)) {
    let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
    edit(&mut snapshot);
    map.gamepad_event(&GamepadEvent::State { id: PAD, snapshot });
}

/// **A press is traced to the context that consumed it and the binding that
/// read it**, and printed the way an inspector row shows it.
#[test]
fn the_trace_names_the_consuming_context_and_binding() {
    let mut map = stacked();
    map.push_context("menu").expect("declared");
    tap(&mut map, KeyCode::Enter);

    let entry = map.trace().next_back().expect("Enter was traced");
    assert_eq!(entry.input, TracedInput::Key(KeyCode::Enter));
    assert_eq!(
        entry.outcome,
        Outcome::Read {
            context: "menu".to_owned(),
            reads: vec![read("accept", Binding::Key(KeyCode::Enter))],
        }
    );
    assert_eq!(entry.count, 1);
    assert_eq!(entry.input.to_string(), "Enter");
    assert_eq!(entry.outcome.to_string(), "menu: accept (Enter)");
    assert_eq!(map.trace().count(), 1, "the release is not traced");
}

/// **An input a higher context binds is traced as eaten there, and the
/// context beneath that binds it too is not among its readers** — then,
/// with the higher context gone, it reaches the one beneath.
#[test]
fn an_input_a_higher_context_eats_does_not_reach_the_contexts_beneath() {
    let mut map = stacked();
    map.push_context("menu").expect("declared");
    tap(&mut map, KeyCode::Space);
    assert_eq!(
        *newest(&map),
        Outcome::Read {
            context: "menu".to_owned(),
            reads: vec![read("accept", Binding::Key(KeyCode::Space))],
        },
        "gameplay's jump is bound to Space too, and must not be listed",
    );

    map.pop_context("menu").expect("on top");
    tap(&mut map, KeyCode::Space);
    assert_eq!(
        *newest(&map),
        Outcome::Read {
            context: crate::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![read("jump", Binding::Key(KeyCode::Space))],
        }
    );
}

/// **A modal context stopping an input bound beneath it is traced as the
/// block, by name**, and a plain context lets the same input fall through.
#[test]
fn a_modal_context_is_named_as_blocking_what_it_does_not_bind() {
    let mut map = stacked();
    map.push_context_modal("inventory").expect("declared");
    tap(&mut map, KeyCode::KeyC);
    assert_eq!(
        *newest(&map),
        Outcome::Blocked {
            modal: "inventory".to_owned()
        }
    );
    assert_eq!(newest(&map).to_string(), "blocked by modal inventory");
    assert!(!map.button_held("crouch"));

    map.pop_context("inventory").expect("on top");
    map.push_context("inventory").expect("declared");
    tap(&mut map, KeyCode::KeyC);
    assert_eq!(
        *newest(&map),
        Outcome::Read {
            context: crate::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![read("crouch", Binding::Key(KeyCode::KeyC))],
        },
        "a plain context lets it fall through",
    );
}

/// **An input no active context binds is traced as unbound** — including
/// one bound only in a context that is off the stack.
#[test]
fn an_unbound_input_is_traced_as_unbound() {
    let mut map = stacked();
    tap(&mut map, KeyCode::KeyQ);
    assert_eq!(*newest(&map), Outcome::Unbound);
    tap(&mut map, KeyCode::Enter);
    assert_eq!(
        *newest(&map),
        Outcome::Unbound,
        "only the menu binds Enter, and it is not pushed",
    );
    assert_eq!(newest(&map).to_string(), "unbound");
}

/// **A context that owns an input no live binding read says so**: a chord's
/// key without its modifier, and a disabled action's key, are eaten by their
/// context and read by nothing.
#[test]
fn an_owned_input_nothing_read_is_traced_with_no_readers() {
    let mut map = ActionMap::new();
    map.declare(button(
        "reload",
        vec![Binding::Chord {
            modifier: crate::Modifier::Shift,
            key: KeyCode::KeyR,
        }],
    ));
    map.declare(button("jump", vec![Binding::Key(KeyCode::Space)]));
    map.set_enabled("jump", false);
    map.set_tracing(true);

    tap(&mut map, KeyCode::KeyR);
    let eaten = Outcome::Read {
        context: crate::GAMEPLAY_CONTEXT.to_owned(),
        reads: Vec::new(),
    };
    assert_eq!(*newest(&map), eaten);
    assert_eq!(newest(&map).to_string(), "gameplay: nothing read it");
    tap(&mut map, KeyCode::Space);
    assert_eq!(*newest(&map), eaten, "a disabled action reads nothing");

    map.key_event(KeyCode::ShiftLeft, true);
    tap(&mut map, KeyCode::KeyR);
    assert_eq!(
        *newest(&map),
        Outcome::Read {
            context: crate::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![read(
                "reload",
                Binding::Chord {
                    modifier: crate::Modifier::Shift,
                    key: KeyCode::KeyR,
                }
            )],
        }
    );
}

/// **The trace holds [`RESOLUTION_TRACE_CAP`] entries and drops the oldest**,
/// and an input repeating the newest entry counts on it rather than pushing
/// another.
#[test]
fn the_trace_is_bounded_at_its_cap() {
    let mut map = stacked();
    let keys = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
    ];
    assert!(
        keys.len() > RESOLUTION_TRACE_CAP,
        "the script overflows the cap"
    );
    for key in keys {
        tap(&mut map, key);
    }
    assert_eq!(map.trace().count(), RESOLUTION_TRACE_CAP);
    let traced: Vec<&TracedInput> = map.trace().map(|entry| &entry.input).collect();
    let kept = &keys[keys.len() - RESOLUTION_TRACE_CAP..];
    assert_eq!(
        traced,
        kept.iter()
            .map(|&key| TracedInput::Key(key))
            .collect::<Vec<_>>()
            .iter()
            .collect::<Vec<_>>(),
        "the newest kept, oldest first",
    );

    for _ in 0..3 {
        map.mouse_scroll(0.0, 1.0);
    }
    assert_eq!(map.trace().count(), RESOLUTION_TRACE_CAP);
    let wheel = map.trace().next_back().expect("an entry");
    assert_eq!(wheel.input, TracedInput::Wheel);
    assert_eq!(wheel.count, 3, "three turns, one entry");
}

/// **Off, nothing is recorded** — a claim included — and turning it off
/// drops what was.
#[test]
fn tracing_off_records_nothing() {
    let mut map = stacked();
    map.set_tracing(false);
    assert!(!map.is_tracing());
    tap(&mut map, KeyCode::Space);
    map.trace_claimed(TracedInput::Key(KeyCode::Escape), "the loop: pause");
    assert_eq!(map.trace().count(), 0);

    map.set_tracing(true);
    map.trace_claimed(TracedInput::Key(KeyCode::Escape), "the loop: pause");
    assert_eq!(
        *newest(&map),
        Outcome::Claimed {
            by: "the loop: pause".to_owned()
        }
    );
    assert_eq!(newest(&map).to_string(), "claimed by the loop: pause");
    map.set_tracing(false);
    map.set_tracing(true);
    assert_eq!(map.trace().count(), 0, "off dropped the entry");
}

/// **A pad button, and a stick pushed past the activity threshold, are
/// traced; a stick drifting inside it is not**, and a stick held where it is
/// is not traced again.
#[test]
fn a_pad_press_and_a_stick_push_are_traced_and_drift_is_not() {
    let mut map = ActionMap::new();
    map.declare(button("jump", vec![Binding::PadButton(PadButton::South)]));
    map.declare(decl(
        "walk",
        ActionKind::Axis2,
        vec![Binding::PadStick {
            stick: Stick::Left,
            deadzone: 0.2,
        }],
    ));
    map.set_tracing(true);

    pad(&mut map, |pad| pad.axes[PadAxis::LeftX as usize] = 0.3);
    assert_eq!(map.trace().count(), 0, "drift inside the threshold");

    pad(&mut map, |pad| pad.buttons.insert(PadButton::South));
    assert_eq!(
        map.trace().next_back().map(|e| &e.input),
        Some(&TracedInput::PadButton(PadButton::South))
    );
    assert_eq!(
        *newest(&map),
        Outcome::Read {
            context: crate::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![read("jump", Binding::PadButton(PadButton::South))],
        }
    );

    pad(&mut map, |pad| pad.axes[PadAxis::LeftX as usize] = 0.9);
    pad(&mut map, |pad| pad.axes[PadAxis::LeftX as usize] = 0.95);
    assert_eq!(map.trace().count(), 2, "pushed once, then held over");
    let push = map.trace().next_back().expect("the push");
    assert_eq!(push.input, TracedInput::PadStick(Stick::Left));
    assert_eq!(push.input.to_string(), "PadStick:Left");
    assert_eq!(
        push.outcome,
        Outcome::Read {
            context: crate::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![read(
                "walk",
                Binding::PadStick {
                    stick: Stick::Left,
                    deadzone: 0.2,
                }
            )],
        }
    );
}

/// **An input its owner withholds is traced as withheld**: a trigger held
/// through a suppress, pulled further, still reads as released.
#[test]
fn a_withheld_input_is_traced_as_withheld() {
    let mut map = ActionMap::new();
    map.declare(button(
        "fire",
        vec![Binding::PadTrigger {
            trigger: Trigger::Right,
            threshold: 0.1,
        }],
    ));
    map.set_tracing(true);
    pad(&mut map, |pad| {
        pad.axes[PadAxis::RightTrigger as usize] = 0.3
    });
    map.suppress_held();
    pad(&mut map, |pad| {
        pad.axes[PadAxis::RightTrigger as usize] = 0.9
    });
    assert!(!map.button_held("fire"));
    assert_eq!(
        *newest(&map),
        Outcome::Withheld {
            context: crate::GAMEPLAY_CONTEXT.to_owned()
        }
    );
    assert_eq!(
        newest(&map).to_string(),
        "withheld by gameplay until released"
    );
}

/// Drives one action of `kind` bound to `binding` with `drive`, and returns
/// whether its value moved and whether the trace listed it as a reader.
fn moved_and_traced(
    kind: ActionKind,
    binding: &Binding,
    prepare: impl Fn(&mut ActionMap),
    drive: impl Fn(&mut ActionMap),
) -> (bool, bool) {
    let mut map = ActionMap::new();
    map.declare(decl("act", kind, vec![binding.clone()]));
    prepare(&mut map);
    map.set_tracing(true);
    let before = format!("{:?}", map.action("act"));
    drive(&mut map);
    let moved = format!("{:?}", map.action("act")) != before;
    let traced = map.trace().any(|entry| {
        matches!(&entry.outcome, Outcome::Read { reads, .. }
            if reads.iter().any(|read| read.action == "act"))
    });
    assert_eq!(
        map.trace().count(),
        1,
        "{kind:?} {binding}: one press, one entry"
    );
    (moved, traced)
}

/// **The trace lists a binding as a reader exactly when its action's value
/// moved** — for every binding a press drives, on every kind of action, so
/// the trace's questions and resolution's cannot drift apart.
#[test]
fn a_binding_is_traced_as_reading_exactly_when_its_action_moves() {
    let none = |_: &mut ActionMap| {};
    let key = |key| move |map: &mut ActionMap| map.key_event(key, true);
    let south = |map: &mut ActionMap| pad(map, |pad| pad.buttons.insert(PadButton::South));
    let dpad = |map: &mut ActionMap| pad(map, |pad| pad.buttons.insert(PadButton::DpadUp));
    let stick = |map: &mut ActionMap| pad(map, |pad| pad.axes[PadAxis::LeftX as usize] = 0.8);
    let trigger = |map: &mut ActionMap| {
        pad(map, |pad| pad.axes[PadAxis::RightTrigger as usize] = 0.7);
    };
    let wasd = Binding::Wasd {
        up: KeyCode::KeyW,
        down: KeyCode::KeyS,
        left: KeyCode::KeyA,
        right: KeyCode::KeyD,
    };
    let key_axis = Binding::KeyAxis {
        negative: KeyCode::KeyS,
        positive: KeyCode::KeyW,
    };
    let cases: Vec<(Binding, Step)> = vec![
        (Binding::Key(KeyCode::KeyW), Box::new(key(KeyCode::KeyW))),
        (wasd, Box::new(key(KeyCode::KeyW))),
        (key_axis, Box::new(key(KeyCode::KeyW))),
        (
            Binding::MouseButton(PointerButton::Left),
            Box::new(|map: &mut ActionMap| map.mouse_button(PointerButton::Left, true)),
        ),
        (
            Binding::MouseScroll,
            Box::new(|map: &mut ActionMap| map.mouse_scroll(0.0, 1.0)),
        ),
        (
            Binding::Virtual("pad".to_owned()),
            Box::new(|map: &mut ActionMap| map.virtual_button("pad", true)),
        ),
        (
            Binding::Virtual("pad".to_owned()),
            Box::new(|map: &mut ActionMap| map.virtual_stick("pad", 0.5, 0.0)),
        ),
        (Binding::PadButton(PadButton::South), Box::new(south)),
        (Binding::PadDpad, Box::new(dpad)),
        (
            Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.2,
            },
            Box::new(stick),
        ),
        (
            Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.6,
            },
            Box::new(trigger),
        ),
        (
            Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.9,
            },
            Box::new(trigger),
        ),
    ];
    let mut readers = 0;
    for (binding, drive) in &cases {
        for kind in [ActionKind::Button, ActionKind::Axis1, ActionKind::Axis2] {
            let (moved, traced) = moved_and_traced(kind, binding, none, drive);
            assert_eq!(
                traced, moved,
                "{kind:?} on {binding}: moved {moved}, traced {traced}"
            );
            readers += usize::from(traced);
        }
    }
    assert!(
        readers > cases.len(),
        "the table exercised readers, not only refusals"
    );

    // A 1-D axis with a pointer position: the position replaces every other
    // binding on it, so a key bound beside it reads nothing.
    let axis = |map: &mut ActionMap| {
        map.rebind(
            "act",
            vec![
                Binding::PointerPosition {
                    axis: PointerAxis::X,
                },
                Binding::Key(KeyCode::KeyW),
            ],
        )
        .expect("declared");
        map.pointer_position(0.25, 0.0);
    };
    let (moved, traced) = moved_and_traced(
        ActionKind::Axis1,
        &Binding::Key(KeyCode::KeyW),
        axis,
        key(KeyCode::KeyW),
    );
    assert_eq!((moved, traced), (false, false));
}

/// One scripted session, with every kind of input, context changes, chords, a
/// suppress, patterns and pads.
fn script(map: &mut ActionMap) -> Vec<String> {
    let mut states = Vec::new();
    let step = |map: &mut ActionMap, states: &mut Vec<String>| {
        states.push(format!(
            "{:?} {:?} {:?} {:?}",
            InputTickState::capture(map).actions,
            map.last_device(),
            map.active_contexts().collect::<Vec<_>>(),
            map.held_keys(),
        ));
    };
    let events: Vec<Step> = vec![
        Box::new(|map| map.key_event(KeyCode::KeyW, true)),
        Box::new(|map| map.key_event(KeyCode::Space, true)),
        Box::new(|map| map.push_context("menu").expect("declared")),
        Box::new(|map| map.key_event(KeyCode::Space, false)),
        Box::new(|map| map.key_event(KeyCode::Space, true)),
        Box::new(|map| map.key_event(KeyCode::Enter, true)),
        Box::new(|map| map.begin_tick(TICK)),
        Box::new(|map| map.key_event(KeyCode::Enter, false)),
        Box::new(|map| map.pop_context("menu").expect("on top")),
        Box::new(|map| map.push_context_modal("inventory").expect("declared")),
        Box::new(|map| map.key_event(KeyCode::KeyC, true)),
        Box::new(|map| map.mouse_button(PointerButton::Left, true)),
        Box::new(|map| map.mouse_scroll(0.0, 2.0)),
        Box::new(|map| map.pop_context("inventory").expect("on top")),
        Box::new(|map| map.key_event(KeyCode::ShiftLeft, true)),
        Box::new(|map| map.key_event(KeyCode::KeyR, true)),
        Box::new(|map| map.begin_tick(TICK)),
        Box::new(|map| pad(map, |pad| pad.buttons.insert(PadButton::South))),
        Box::new(|map| pad(map, |pad| pad.axes[PadAxis::LeftX as usize] = 0.8)),
        Box::new(|map| map.suppress_held()),
        Box::new(|map| pad(map, |pad| pad.axes[PadAxis::RightTrigger as usize] = 0.9)),
        Box::new(|map| map.virtual_button("fire", true)),
        Box::new(|map| map.virtual_stick("move", 0.4, 0.3)),
        Box::new(|map| map.begin_tick(0.5)),
        Box::new(|map| map.key_event(KeyCode::KeyW, false)),
        Box::new(|map| map.mouse_motion(3.0, -2.0)),
        Box::new(|map| map.pointer_position(0.1, 0.2)),
        Box::new(|map| map.begin_tick(TICK)),
    ];
    step(map, &mut states);
    for event in &events {
        event(map);
        step(map, &mut states);
    }
    states
}

/// The map [`script`] drives: the [`stacked`] contexts, plus a chord, a held
/// pattern, a pad on every binding kind and on-screen controls.
fn scripted_map(tracing: bool) -> ActionMap {
    let mut map = stacked();
    map.set_tracing(tracing);
    map.declare(button(
        "reload",
        vec![Binding::Chord {
            modifier: crate::Modifier::Shift,
            key: KeyCode::KeyR,
        }],
    ));
    map.declare(decl(
        "walk",
        ActionKind::Axis2,
        vec![
            Binding::Wasd {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            },
            Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.2,
            },
            Binding::Virtual("move".to_owned()),
            Binding::MouseMotion,
        ],
    ));
    map.declare(button(
        "shoot",
        vec![
            Binding::MouseButton(PointerButton::Left),
            Binding::PadButton(PadButton::South),
            Binding::Virtual("fire".to_owned()),
            Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.5,
            },
        ],
    ));
    map.declare(decl("zoom", ActionKind::Axis1, vec![Binding::MouseScroll]));
    map.declare(decl(
        "aim",
        ActionKind::Axis1,
        vec![Binding::PointerPosition {
            axis: PointerAxis::X,
        }],
    ));
    map.set_hold("jump", Some(crate::Hold::default()))
        .expect("declared");
    map
}

/// **Tracing never changes resolution**: one script through a tracing map and
/// a silent one leaves every action, the last device, the stack and the held
/// keys the same after every step.
#[test]
fn resolution_is_identical_with_tracing_on_and_off() {
    let mut traced = scripted_map(true);
    let mut silent = scripted_map(false);
    let with = script(&mut traced);
    let without = script(&mut silent);
    assert!(
        traced.trace().count() > 4,
        "the tracing map recorded the script"
    );
    assert_eq!(silent.trace().count(), 0);
    for (index, (with, without)) in with.iter().zip(&without).enumerate() {
        assert_eq!(with, without, "step {index}");
    }
    assert_eq!(with.len(), without.len());
}
