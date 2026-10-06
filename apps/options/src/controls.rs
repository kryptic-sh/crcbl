//! The `CONTROLS` page: a player's key binds, rebound by pressing the input
//! and kept in their profile between runs.
//!
//! The flow itself — listening, the clash panel and its `SWAP`, the profile
//! written whenever the map's overrides change — is the engine's
//! [`crcbl::rebind`], which `apps/puppet`'s controls overlay hosts too. What
//! is this sample's is below: the actions, their defaults, and where the
//! page's ids sit among the settings page's.
//!
//! # What it rebinds
//!
//! [`ACTIONS`], a small gameplay set declared the way a game declares its own.
//! This sample has no game, so nothing *does* anything on these actions; what
//! shows that a binding is live is the `(held)` mark its row wears while the
//! map reads its input down — the same map, fed the same events, a game would
//! read. An input the loop's menu claims while a panel has input (the arrows,
//! ENTER, the pad's face buttons and d-pad) never reaches that map here,
//! because this sample always has a panel up.

use crcbl::core::input::KeyCode;
use crcbl::input::{ActionDecl, ActionKind, ActionMap, Binding, PadButton};
use crcbl::rebind::{RebindIds, RebindRow, Rebinder};
use crcbl::store::profile::ProfileStore;
use crcbl::ui::WidgetId;
use crcbl::ui::menu::Menu;

use crate::menu::PRESENT_MODE_ID;

pub use crcbl::rebind::{Capture, LISTENING_HINT};

/// The page's state: [`crcbl::rebind`]'s flow over [`ACTIONS`].
pub type Controls = Rebinder;

/// One action this page offers: its name in the map and in the profile, the
/// label its row wears, and what it is bound to before the player says
/// otherwise.
#[derive(Debug)]
pub struct ControlAction {
    /// The action's name, which is also its key in the profile.
    pub name: &'static str,
    /// What its row says.
    pub label: &'static str,
    /// Its default key.
    pub key: KeyCode,
    /// Its default pad button.
    pub pad: PadButton,
}

/// The actions the page lists, in the order it lists them.
pub const ACTIONS: [ControlAction; 5] = [
    ControlAction {
        name: "jump",
        label: "JUMP",
        key: KeyCode::Space,
        pad: PadButton::South,
    },
    ControlAction {
        name: "interact",
        label: "INTERACT",
        key: KeyCode::KeyE,
        pad: PadButton::West,
    },
    ControlAction {
        name: "reload",
        label: "RELOAD",
        key: KeyCode::KeyR,
        pad: PadButton::North,
    },
    ControlAction {
        name: "crouch",
        label: "CROUCH",
        key: KeyCode::ControlLeft,
        pad: PadButton::East,
    },
    ControlAction {
        name: "sprint",
        label: "SPRINT",
        key: KeyCode::ShiftLeft,
        pad: PadButton::LeftStick,
    },
];

/// The id of the settings page's row that opens this one.
pub const CONTROLS_ID: WidgetId = PRESENT_MODE_ID + 1;

/// Where the page's own ids start: just after [`CONTROLS_ID`].
pub const IDS: RebindIds = RebindIds::starting_at(CONTROLS_ID + 1);

/// The id of the row that puts every action back on its defaults.
pub const RESET_CONTROLS_ID: WidgetId = IDS.reset();

/// The id of the row that goes back to the settings page.
pub const BACK_ID: WidgetId = IDS.back();

/// The id of the clash panel's row that takes the input from the other action.
pub const SWAP_ID: WidgetId = IDS.swap();

/// The id of the clash panel's row that leaves both actions as they were.
pub const CANCEL_ID: WidgetId = IDS.cancel();

/// The id of the row for the `index`th entry of [`ACTIONS`].
#[must_use]
pub const fn action_id(index: usize) -> WidgetId {
    IDS.action(index)
}

/// The entry of [`ACTIONS`] a row id names, or `None` for any other id.
#[must_use]
pub fn action_of(id: WidgetId) -> Option<usize> {
    IDS.action_of(id, ACTIONS.len())
}

/// The map this page rebinds: every [`ACTIONS`] entry on its key and its pad
/// button, in the gameplay context.
#[must_use]
pub fn default_actions() -> ActionMap {
    let mut actions = ActionMap::new();
    for action in &ACTIONS {
        actions.declare(ActionDecl {
            name: action.name.to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(action.key), Binding::PadButton(action.pad)],
        });
    }
    actions
}

/// [`ACTIONS`] as the page's rows.
fn rows() -> Vec<RebindRow> {
    ACTIONS
        .iter()
        .map(|action| RebindRow {
            name: action.name,
            label: action.label,
        })
        .collect()
}

/// The page's state: [`default_actions`] with the profile in `store` on top.
#[must_use]
pub fn open(store: ProfileStore) -> Controls {
    Rebinder::open(default_actions(), rows(), store)
}

/// The page and the clash panel, with every row's hint left for the first
/// frame to write.
#[must_use]
pub fn menus() -> (Menu, Menu) {
    crcbl::rebind::menus(IDS, &rows())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row id names its action back, and the page's ids stay clear of
    /// the settings row that opens it.
    #[test]
    fn every_action_row_names_its_action_back() {
        for index in 0..ACTIONS.len() {
            assert_eq!(action_of(action_id(index)), Some(index));
        }
        assert_eq!(action_of(CANCEL_ID), None);
        assert_eq!(action_of(CONTROLS_ID), None);
        let ids = [RESET_CONTROLS_ID, BACK_ID, SWAP_ID, CANCEL_ID];
        assert!(
            !ids.contains(&CONTROLS_ID),
            "a page row shares the opener's id"
        );
    }
}
