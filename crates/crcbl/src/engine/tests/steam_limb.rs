//! The loop's Steam limb against the engine's own fixture: a scripted Steam
//! lent by the fixture game — not a fake Steam client, and nothing that can
//! sign anyone in.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use super::*;
use crate::steam::SteamEvent;

/// A Steam a test scripts: each pump hands over the next frame's events, and
/// notes itself in a log the test may share with a pad source.
#[derive(Debug, Default)]
pub(super) struct ScriptedSteam {
    frames: VecDeque<Vec<SteamEvent>>,
    pumps: u32,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl SteamSource for ScriptedSteam {
    fn pump(&mut self, emit: &mut dyn FnMut(SteamEvent)) {
        self.pumps += 1;
        self.log.borrow_mut().push("steam");
        for event in self.frames.pop_front().unwrap_or_default() {
            emit(event);
        }
    }
}

/// A loop whose game lends it a scripted Steam.
fn with_steam() -> Hosted {
    let mut engine = hosted(None);
    engine.game_mut().steam = Some(ScriptedSteam::default());
    engine
}

/// Queues `events` for the next pump.
fn next_pump(engine: &mut Hosted, events: Vec<SteamEvent>) {
    let steam = engine.game_mut().steam.as_mut().expect("lent");
    steam.frames.push_back(events);
}

const OPENED: SteamEvent = SteamEvent::OverlayActivated { active: true };
const CLOSED: SteamEvent = SteamEvent::OverlayActivated { active: false };

/// **An opened overlay is the window's focus loss**: the held key comes up
/// through the game's key path, the loop pauses, and a second opening while
/// paused leaves it paused — the same check the game-reported loss passes.
#[test]
fn an_opened_overlay_releases_held_keys_and_pauses() {
    a_focus_loss_releases_held_keys_and_pauses(with_steam(), |engine| {
        next_pump(engine, vec![OPENED]);
    });
}

/// **Closing the overlay resumes nothing**: resuming stays the player's, as
/// after alt-tab. Nor is a closing a focus loss of its own.
#[test]
fn a_closed_overlay_leaves_the_game_paused() {
    let mut engine = with_steam();
    next_pump(&mut engine, vec![CLOSED]);
    step(&mut engine);
    assert!(!engine.is_paused(), "a closing paused a running game");
    next_pump(&mut engine, vec![OPENED]);
    step(&mut engine);
    assert!(engine.is_paused());
    next_pump(&mut engine, vec![CLOSED]);
    step(&mut engine);
    step(&mut engine);
    assert!(engine.is_paused(), "the overlay closing unpaused the game");
}

/// The pads' half of the focus loss: a pad button held on the game's map when
/// the overlay opens is released, and stays released while the pad keeps
/// reporting it held.
#[test]
fn an_opened_overlay_releases_a_held_pad_button() {
    use crate::input::{
        ActionDecl, ActionKind, Binding, GamepadEvent, GamepadId, GamepadSnapshot, PadButton,
        PadKind,
    };
    let mut engine = with_steam();
    let held = GamepadEvent::State {
        id: GamepadId(1),
        snapshot: GamepadSnapshot {
            buttons: [PadButton::South].into_iter().collect(),
            ..GamepadSnapshot::neutral(PadKind::Xbox)
        },
    };
    let actions = &mut engine.game_mut().actions.0;
    actions.declare(ActionDecl {
        name: "pad_jump".to_owned(),
        kind: ActionKind::Button,
        bindings: vec![Binding::PadButton(PadButton::South)],
    });
    actions.gamepad_event(&held);
    assert!(actions.button_held("pad_jump"));

    next_pump(&mut engine, vec![OPENED]);
    step(&mut engine);
    let actions = &mut engine.game_mut().actions.0;
    assert!(!actions.button_held("pad_jump"), "the overlay released it");
    actions.gamepad_event(&held);
    assert!(!actions.button_held("pad_jump"), "and it is withheld");
}

/// **Every event reaches the game, once, in order** — the overlay's too —
/// and Steam is pumped once a frame, paused or not.
#[test]
fn every_steam_event_reaches_the_game_once_in_order() {
    let mut engine = with_steam();
    let events = vec![
        SteamEvent::NewLaunchParameters,
        OPENED,
        SteamEvent::FloatingKeyboardDismissed,
    ];
    next_pump(&mut engine, events.clone());
    step(&mut engine);
    assert_eq!(engine.game().steam_events, events);
    step(&mut engine);
    step(&mut engine);
    assert_eq!(engine.game().steam_events, events, "nothing twice");
    assert!(engine.is_paused());
    let pumps = engine.game().steam.as_ref().map(|steam| steam.pumps);
    assert_eq!(pumps, Some(3), "pumped every frame, paused ones included");
}

/// **Steam before the pads**: a `SteamPads` reads the device callbacks the
/// same frame's pump drained, so the pump comes first, every frame.
#[test]
fn steam_is_pumped_before_the_pads_are_polled() {
    struct LoggedPads(Rc<RefCell<Vec<&'static str>>>);
    impl PadSource for LoggedPads {
        fn poll(&mut self, _emit: &mut dyn FnMut(crate::input::GamepadEvent)) {
            self.0.borrow_mut().push("pads");
        }
    }
    let mut engine = with_steam();
    let log = Rc::clone(&engine.game().steam.as_ref().expect("lent").log);
    engine.set_pad_source(Some(Box::new(LoggedPads(Rc::clone(&log)))));
    step(&mut engine);
    step(&mut engine);
    assert_eq!(*log.borrow(), ["steam", "pads", "steam", "pads"]);
}

/// A game that lends no Steam is never asked for events, and nothing pauses.
#[test]
fn without_steam_nothing_is_pumped_or_paused() {
    let mut engine = hosted(None);
    step(&mut engine);
    assert!(engine.game().steam_events.is_empty());
    assert!(!engine.is_paused());
}
