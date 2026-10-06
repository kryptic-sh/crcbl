//! The game's resolution trace through the loop: kept while the debug panel
//! shows and dropped when it hides, with the presses the loop takes before
//! the game hears them recorded as claims — on the engine's own fixture and a
//! headless shell.

use super::*;

use crate::input::{Outcome, TraceEntry, TracedInput, TracedRead};

/// The game map's trace, oldest first.
fn trace(engine: &Hosted) -> Vec<TraceEntry> {
    engine.game().actions.0.trace().cloned().collect()
}

/// The newest entry of the game map's trace.
fn newest(engine: &Hosted) -> TraceEntry {
    trace(engine).pop().expect("an entry")
}

/// **The trace runs exactly while the panel shows, and says where every
/// press went**: the game's own key read by its binding, the pause key and
/// the panel's accept claimed by the loop before the game heard them.
#[test]
fn the_games_trace_follows_the_panel_and_names_the_loops_claims() {
    let mut engine = playing();
    assert!(!engine.debug().is_visible(), "the fixture starts hidden");
    tap_and_frame(&mut engine, SERVE_KEY);
    assert!(
        !engine.game().actions.0.is_tracing(),
        "hidden, nothing traced"
    );

    tap_and_frame(&mut engine, DEBUG_OVERLAY_KEY);
    engine.frame().expect("the fake never fails");
    assert!(engine.game().actions.0.is_tracing(), "showing, traced");

    tap_and_frame(&mut engine, SERVE_KEY);
    let served = newest(&engine);
    assert_eq!(served.input, TracedInput::Key(SERVE_KEY));
    assert_eq!(
        served.outcome,
        Outcome::Read {
            context: crate::input::GAMEPLAY_CONTEXT.to_owned(),
            reads: vec![TracedRead {
                action: SERVE_ACTION.to_owned(),
                binding: crate::input::Binding::Key(SERVE_KEY),
            }],
        }
    );

    tap_and_frame(&mut engine, PAUSE_KEY);
    assert!(engine.is_paused());
    let paused = newest(&engine);
    assert_eq!(paused.input, TracedInput::Key(PAUSE_KEY));
    assert_eq!(paused.outcome.to_string(), "claimed by the loop: pause");

    tap_and_frame(&mut engine, MENU_ACTIVATE_KEY);
    assert!(!engine.is_paused(), "RESUME took the key");
    let resumed = newest(&engine);
    assert_eq!(resumed.input, TracedInput::Key(MENU_ACTIVATE_KEY));
    assert_eq!(resumed.outcome.to_string(), "claimed by the loop's menu");

    tap_and_frame(&mut engine, DEBUG_OVERLAY_KEY);
    engine.frame().expect("the fake never fails");
    assert!(!engine.game().actions.0.is_tracing(), "hidden again");
    assert!(trace(&engine).is_empty(), "and what it held is dropped");
}
