//! Which pad buttons a menu has taken from the game, pad by pad.
//!
//! A key the menu claims never reaches the game (`MenuPump::observe`), and a
//! pad button bound in the `ui` context is claimed the same way: pressed while
//! a panel has input, it is the panel's, and the game's copy of the pad shows
//! it up. A pad reports a snapshot of the whole pad, so claiming a button means
//! clearing it from the snapshot the game is handed, and doing so until it is
//! released:
//!
//! - **A button already down for the game stays down** until it is let go,
//!   as a key held into a panel does, so opening a menu never reads as a
//!   release the player did not make.
//! - **A claimed button stays hidden until it is let go**, even after the
//!   panel closes. South on `RESUME` closes the panel while South is still
//!   down, and the next snapshot would otherwise hand the game a South press
//!   nobody made at it.
//!
//! Only buttons are claimed. The sticks and triggers reach the game as they
//! are, as the WASD keys do under a panel: the menus' keyboard column was
//! narrowed to leave the games' movement alone, and a stick is movement.

use std::collections::HashMap;

use crate::input::{GamepadEvent, GamepadId, GamepadSnapshot, PadButton, PadButtons};

/// Per pad, what the game has been shown down and what the menu holds.
#[derive(Debug, Default)]
pub(super) struct PadClaims {
    pads: HashMap<GamepadId, Seen>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    /// Down as far as the game knows.
    game: PadButtons,
    /// Down, and the menu's until released.
    claimed: PadButtons,
}

impl PadClaims {
    /// The event the game is handed for `event`: the same, with the
    /// buttons the menu holds cleared from a `State`'s snapshot.
    ///
    /// `panel` is whether a panel had input for this event, and `ui` the
    /// buttons its `ui` context binds.
    pub(super) fn for_game(
        &mut self,
        event: &GamepadEvent,
        panel: bool,
        ui: PadButtons,
    ) -> GamepadEvent {
        match *event {
            GamepadEvent::Connected { id, .. } | GamepadEvent::Disconnected { id } => {
                self.pads.remove(&id);
                *event
            }
            GamepadEvent::State { id, snapshot } => {
                let seen = self.pads.entry(id).or_default();
                let mut next = Seen::default();
                for button in PadButton::ALL {
                    if !snapshot.buttons.contains(button) {
                        continue;
                    }
                    let fresh = !seen.game.contains(button) && !seen.claimed.contains(button);
                    if seen.claimed.contains(button) || (fresh && panel && ui.contains(button)) {
                        next.claimed.insert(button);
                    } else {
                        next.game.insert(button);
                    }
                }
                *seen = next;
                GamepadEvent::State {
                    id,
                    snapshot: GamepadSnapshot {
                        buttons: next.game,
                        ..snapshot
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PadKind;

    const PAD: GamepadId = GamepadId(7);

    fn ui() -> PadButtons {
        [PadButton::South, PadButton::East, PadButton::DpadUp]
            .into_iter()
            .collect()
    }

    fn state(buttons: &[PadButton]) -> GamepadEvent {
        GamepadEvent::State {
            id: PAD,
            snapshot: GamepadSnapshot {
                buttons: buttons.iter().copied().collect(),
                axes: [0.25, 0.0, 0.0, 0.0, 0.0, 0.5],
                ..GamepadSnapshot::neutral(PadKind::Xbox)
            },
        }
    }

    fn game_sees(claims: &mut PadClaims, buttons: &[PadButton], panel: bool) -> PadButtons {
        match claims.for_game(&state(buttons), panel, ui()) {
            GamepadEvent::State { snapshot, .. } => snapshot.buttons,
            other => panic!("a state stays a state: {other:?}"),
        }
    }

    fn set(buttons: &[PadButton]) -> PadButtons {
        buttons.iter().copied().collect()
    }

    #[test]
    fn with_no_panel_the_game_sees_the_whole_pad() {
        let mut claims = PadClaims::default();
        let all = [PadButton::South, PadButton::West];
        assert_eq!(game_sees(&mut claims, &all, false), set(&all));
    }

    #[test]
    fn a_ui_button_pressed_at_a_panel_is_the_panels_and_the_rest_are_the_games() {
        let mut claims = PadClaims::default();
        assert_eq!(
            game_sees(&mut claims, &[PadButton::South, PadButton::West], true),
            set(&[PadButton::West])
        );
    }

    #[test]
    fn a_claimed_button_stays_hidden_after_the_panel_closes_until_it_is_let_go() {
        let mut claims = PadClaims::default();
        assert_eq!(game_sees(&mut claims, &[PadButton::South], true), set(&[]));
        // South on RESUME closed the panel; South is still down.
        assert_eq!(game_sees(&mut claims, &[PadButton::South], false), set(&[]));
        assert_eq!(game_sees(&mut claims, &[], false), set(&[]));
        // Pressed again with no panel, it is the game's.
        assert_eq!(
            game_sees(&mut claims, &[PadButton::South], false),
            set(&[PadButton::South])
        );
    }

    #[test]
    fn a_button_held_into_a_panel_stays_down_for_the_game_until_released() {
        let mut claims = PadClaims::default();
        assert_eq!(
            game_sees(&mut claims, &[PadButton::DpadUp], false),
            set(&[PadButton::DpadUp])
        );
        assert_eq!(
            game_sees(&mut claims, &[PadButton::DpadUp], true),
            set(&[PadButton::DpadUp]),
            "opening the panel is not a release"
        );
        assert_eq!(game_sees(&mut claims, &[], true), set(&[]));
        assert_eq!(
            game_sees(&mut claims, &[PadButton::DpadUp], true),
            set(&[]),
            "pressed again at the panel, it is the panel's"
        );
    }

    #[test]
    fn the_axes_pass_untouched_and_other_events_pass_as_they_are() {
        let mut claims = PadClaims::default();
        let GamepadEvent::State { snapshot, .. } =
            claims.for_game(&state(&[PadButton::South]), true, ui())
        else {
            panic!("a state");
        };
        assert_eq!(snapshot.axes, [0.25, 0.0, 0.0, 0.0, 0.0, 0.5]);
        let gone = GamepadEvent::Disconnected { id: PAD };
        assert_eq!(claims.for_game(&gone, true, ui()), gone);
        // A pad that comes back starts with nothing claimed.
        assert_eq!(
            game_sees(&mut claims, &[PadButton::South], false),
            set(&[PadButton::South])
        );
    }

    #[test]
    fn two_pads_are_claimed_apart() {
        let mut claims = PadClaims::default();
        assert_eq!(game_sees(&mut claims, &[PadButton::South], true), set(&[]));
        let other = GamepadEvent::State {
            id: GamepadId(8),
            snapshot: GamepadSnapshot {
                buttons: set(&[PadButton::South]),
                ..GamepadSnapshot::neutral(PadKind::Xbox)
            },
        };
        let GamepadEvent::State { snapshot, .. } = claims.for_game(&other, false, ui()) else {
            panic!("a state");
        };
        assert_eq!(snapshot.buttons, set(&[PadButton::South]));
    }
}
