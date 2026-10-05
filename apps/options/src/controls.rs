//! The `CONTROLS` page: a player's key binds, rebound by pressing the input
//! and kept in their profile between runs.
//!
//! ```text
//!   SETTINGS ── CONTROLS ──▶ Controls ── an action row ──▶ listening
//!                               ▲                             │ the next input
//!                               │      ┌──── no clash ────────┤
//!                               │      ▼                      ▼ a clash
//!                               └── rebound ◀── SWAP ── Conflict ── CANCEL ──┘
//! ```
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
//!
//! # Listening
//!
//! Choosing an action row sets the page listening, and
//! [`HostedGame::captures_input`](crcbl::engine::HostedGame::captures_input)
//! hands it every input the loop would otherwise keep for the menu: the next
//! key press, mouse button or pad button becomes the binding. **Escape
//! cancels**, so it cannot itself be bound; F3, F11 and the console key stay
//! the loop's. A captured input replaces the action's bindings **on the same
//! device** and leaves the others — rebinding jump to `J` keeps its pad
//! button.
//!
//! # A clash
//!
//! An input already bound to another action in the same context opens the
//! `ALREADY BOUND` panel, and **the player chooses**: `SWAP` gives the other
//! action this one's old input on that device, `CANCEL` changes nothing. An
//! automatic swap would move a binding the player never looked at, and
//! refusing outright would make every reshuffle two steps through an unbound
//! action; asking costs one press and surprises nobody.
//!
//! # Kept
//!
//! The player's rebinds are written to their profile
//! ([`crcbl::store::profile`]) **whenever the map's overrides change**, so a
//! rebind on this page, `RESET CONTROLS` and the console's `bind` all reach the
//! file through one rule rather than one writer each. A headless run writes
//! nowhere, as the settings file's `SAVE` does.

use crcbl::core::input::{KeyCode, PointerButton};
use crcbl::input::{
    ActionDecl, ActionKind, ActionMap, Binding, DefaultLabels, GamepadEvent, GamepadId, HintLabels,
    PadButton, PadButtons, PadKind,
};
use crcbl::store::profile::{Profile, ProfileStore};
use crcbl::ui::WidgetId;
use crcbl::ui::menu::{Caption, Menu, MenuItem};

use crate::app::SaveState;
use crate::menu::PRESENT_MODE_ID;

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

/// The id of the row that puts every action back on its defaults.
pub const RESET_CONTROLS_ID: WidgetId = CONTROLS_ID + 1;

/// The id of the row that goes back to the settings page.
pub const BACK_ID: WidgetId = RESET_CONTROLS_ID + 1;

/// The id of the clash panel's row that takes the input from the other action.
pub const SWAP_ID: WidgetId = BACK_ID + 1;

/// The id of the clash panel's row that leaves both actions as they were.
pub const CANCEL_ID: WidgetId = SWAP_ID + 1;

/// The id of the row for the `index`th entry of [`ACTIONS`].
#[must_use]
pub const fn action_id(index: usize) -> WidgetId {
    CANCEL_ID + 1 + index as WidgetId
}

/// The entry of [`ACTIONS`] a row id names, or `None` for any other id.
#[must_use]
pub fn action_of(id: WidgetId) -> Option<usize> {
    (0..ACTIONS.len()).find(|index| action_id(*index) == id)
}

/// The page's heading.
pub const CONTROLS_TITLE: &str = "CONTROLS";

/// The clash panel's heading.
pub const CONFLICT_TITLE: &str = "ALREADY BOUND";

/// What a listening row says in place of its bindings.
pub const LISTENING_HINT: &str = "PRESS AN INPUT";

/// What a row wears after its bindings while the map reads the action down.
pub const HELD_MARK: &str = "(held)";

/// What a row with no binding at all says.
pub const UNBOUND_HINT: &str = "nothing";

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

/// The page and the clash panel, with every row's hint left for the first
/// frame to write.
#[must_use]
pub fn menus() -> (Menu, Menu) {
    let mut items: Vec<MenuItem> = ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| MenuItem::new(action_id(index), action.label, ""))
        .collect();
    items.push(MenuItem::new(RESET_CONTROLS_ID, "RESET CONTROLS", ""));
    items.push(MenuItem::new(BACK_ID, "BACK", ""));
    let conflict = Menu::new(
        CONFLICT_TITLE,
        vec![
            MenuItem::new(SWAP_ID, "SWAP", ""),
            MenuItem::new(CANCEL_ID, "CANCEL", ""),
        ],
    );
    (Menu::new(CONTROLS_TITLE, items), conflict)
}

/// `bindings` with `captured` in place of every binding on its device, at the
/// place the first of them held — or at the end, for a device the list did
/// not have.
fn replaced_on_device(bindings: &[Binding], captured: &Binding) -> Vec<Binding> {
    let device = captured.device();
    // Every binding before the device's first is on another device, so this
    // count is that first binding's place once the device's are taken out.
    let at = bindings
        .iter()
        .take_while(|binding| binding.device() != device)
        .count();
    let mut replaced: Vec<Binding> = bindings
        .iter()
        .filter(|binding| binding.device() != device)
        .cloned()
        .collect();
    replaced.insert(at, captured.clone());
    replaced
}

/// What the page is doing with the player's next input.
#[derive(Clone, Debug, PartialEq)]
pub enum Capture {
    /// Nothing: the menu has the input.
    Idle,
    /// Waiting for the input to bind to the `action`th entry of [`ACTIONS`].
    Listening {
        /// The entry being rebound.
        action: usize,
    },
    /// The input `binding` is already `other`'s, and the player is choosing.
    Conflict {
        /// The entry being rebound.
        action: usize,
        /// The input the player pressed.
        binding: Binding,
        /// The entry that already has it.
        other: usize,
    },
}

/// The page's state: the map, the profile it is kept in, and the capture.
#[derive(Debug)]
pub struct Controls {
    actions: ActionMap,
    store: ProfileStore,
    /// The profile as it was loaded, carrying any binds for actions this
    /// build does not declare, which each save keeps.
    profile: Profile,
    /// The overrides the profile last had written, or was loaded with — what
    /// a change is measured against.
    written: Vec<(String, Vec<String>)>,
    capture: Capture,
    /// What the last write did.
    saved: SaveState,
    /// How many of the loaded profile's entries were refused.
    refused: usize,
    /// Every key down, so an auto-repeat of a key held when listening began
    /// is not read as the player's choice.
    keys: Vec<KeyCode>,
    /// The buttons each pad last held, so only a button going down is read.
    pads: Vec<(GamepadId, PadButtons)>,
}

impl Controls {
    /// [`default_actions`] with the profile in `store` on top.
    ///
    /// A profile that cannot be read is logged and stands as the defaults; an
    /// entry naming an action this build does not declare, or a binding it
    /// cannot read, is logged and skipped — see
    /// [`ActionMap::apply_override_text`]. Nothing is written back at open:
    /// opening is a read.
    #[must_use]
    pub fn open(store: ProfileStore) -> Self {
        let profile = store.load_or_default();
        let mut actions = default_actions();
        let refusals = actions.apply_override_text(profile.binds());
        for refusal in &refusals {
            crcbl::log::warn!("profile: {}: {refusal}", store.file());
        }
        let written = actions.override_text();
        Self {
            actions,
            store,
            profile,
            written,
            capture: Capture::Idle,
            saved: SaveState::default(),
            refused: refusals.len(),
            keys: Vec::new(),
            pads: Vec::new(),
        }
    }

    /// The map, as rebound so far.
    #[must_use]
    pub const fn actions(&self) -> &ActionMap {
        &self.actions
    }

    /// The map, for the console's `bind` to rebind.
    pub fn actions_mut(&mut self) -> &mut ActionMap {
        &mut self.actions
    }

    /// What the page is doing with the next input.
    #[must_use]
    pub const fn capture(&self) -> &Capture {
        &self.capture
    }

    /// Whether the page is waiting for an input to bind.
    #[must_use]
    pub const fn listening(&self) -> bool {
        matches!(self.capture, Capture::Listening { .. })
    }

    /// What the last write of the profile did.
    #[must_use]
    pub const fn saved(&self) -> &SaveState {
        &self.saved
    }

    /// Starts listening for the `action`th entry of [`ACTIONS`].
    pub fn listen(&mut self, action: usize) {
        if action < ACTIONS.len() {
            self.capture = Capture::Listening { action };
        }
    }

    /// Takes the clash panel's `SWAP`: the input moves to the action being
    /// rebound, and the other action gets that action's old bindings on the
    /// same device in its place.
    pub fn swap(&mut self) {
        let Capture::Conflict {
            action,
            binding,
            other,
        } = std::mem::replace(&mut self.capture, Capture::Idle)
        else {
            return;
        };
        let device = binding.device();
        let mine = self.bindings_of(action);
        let given: Vec<Binding> = mine
            .iter()
            .filter(|old| old.device() == device)
            .cloned()
            .collect();
        let theirs = self.bindings_of(other);
        let at = theirs
            .iter()
            .position(|held| *held == binding)
            .unwrap_or(theirs.len());
        let mut swapped: Vec<Binding> = theirs
            .iter()
            .filter(|held| **held != binding)
            .cloned()
            .collect();
        for (offset, old) in given.into_iter().enumerate() {
            swapped.insert((at + offset).min(swapped.len()), old);
        }
        self.rebind(other, swapped);
        self.rebind(action, replaced_on_device(&mine, &binding));
    }

    /// Takes the clash panel's `CANCEL`, or Escape while listening: nothing
    /// changes.
    pub fn cancel(&mut self) {
        self.capture = Capture::Idle;
    }

    /// Puts every action back on its defaults.
    pub fn reset(&mut self) {
        self.capture = Capture::Idle;
        let refusals = self.actions.apply_override_text(std::iter::empty());
        for refusal in refusals {
            crcbl::log::warn!("controls: {refusal}");
        }
    }

    /// A key went down or came up.
    ///
    /// Fed to the map either way. While listening, a press of a key not
    /// already down is the player's choice — or, for Escape, their cancel.
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        let fresh = if pressed {
            let fresh = !self.keys.contains(&key);
            if fresh {
                self.keys.push(key);
            }
            fresh
        } else {
            self.keys.retain(|held| *held != key);
            false
        };
        self.actions.key_event(key, pressed);
        if fresh && self.listening() {
            if key == crcbl::engine::PAUSE_KEY {
                self.cancel();
            } else {
                self.captured(Binding::Key(key));
            }
        }
    }

    /// A mouse button went down or came up — the primary one included, which
    /// the loop reports as a pointer press.
    pub fn button(&mut self, button: PointerButton, pressed: bool) {
        self.actions.mouse_button(button, pressed);
        if pressed && self.listening() {
            self.captured(Binding::MouseButton(button));
        }
    }

    /// A pad event: fed to the map, and while listening, the first button it
    /// puts down that was up is the player's choice.
    pub fn pad(&mut self, event: &GamepadEvent) {
        self.actions.gamepad_event(event);
        match *event {
            GamepadEvent::State { id, snapshot } => {
                let before = self
                    .pads
                    .iter()
                    .find(|(pad, _)| *pad == id)
                    .map_or(PadButtons::EMPTY, |(_, held)| *held);
                self.pads.retain(|(pad, _)| *pad != id);
                self.pads.push((id, snapshot.buttons));
                let pressed = PadButton::ALL
                    .into_iter()
                    .find(|button| snapshot.buttons.contains(*button) && !before.contains(*button));
                if let Some(button) = pressed
                    && self.listening()
                {
                    self.captured(Binding::PadButton(button));
                }
            }
            GamepadEvent::Disconnected { id } => self.pads.retain(|(pad, _)| *pad != id),
            GamepadEvent::Connected { .. } => {}
        }
    }

    /// The listening action takes `binding`, unless another action in its
    /// context already has it, which opens the clash panel instead.
    fn captured(&mut self, binding: Binding) {
        let Capture::Listening { action } = self.capture else {
            return;
        };
        let clash = self
            .actions
            .bound_elsewhere(ACTIONS[action].name, &binding)
            .and_then(|name| ACTIONS.iter().position(|entry| entry.name == name));
        match clash {
            Some(other) => {
                self.capture = Capture::Conflict {
                    action,
                    binding,
                    other,
                };
            }
            None => {
                self.capture = Capture::Idle;
                let mine = self.bindings_of(action);
                self.rebind(action, replaced_on_device(&mine, &binding));
            }
        }
    }

    /// The `index`th entry's current bindings.
    fn bindings_of(&self, index: usize) -> Vec<Binding> {
        self.actions
            .bindings(ACTIONS[index].name)
            .unwrap_or_default()
            .to_vec()
    }

    /// Rebinds the `index`th entry, logging the one refusal the map can give —
    /// a pad threshold out of range, which nothing captured here carries.
    fn rebind(&mut self, index: usize, bindings: Vec<Binding>) {
        if let Err(error) = self.actions.rebind(ACTIONS[index].name, bindings) {
            crcbl::log::warn!("controls: {error}");
        }
    }

    /// Writes the profile if the map's overrides moved since the last write,
    /// whoever moved them.
    ///
    /// A failure is shown on the page and not retried every frame: the next
    /// change tries again.
    pub fn persist(&mut self) {
        let overrides = self.actions.override_text();
        if overrides == self.written {
            return;
        }
        let actions = &self.actions;
        self.profile
            .set_binds(|name| actions.bindings(name).is_some(), overrides.clone());
        self.saved = match self.store.save(&self.profile) {
            Ok(true) => SaveState::Saved,
            Ok(false) => SaveState::Nowhere,
            Err(error) => {
                crcbl::log::warn!("controls: {error}");
                SaveState::Failed(error.to_string())
            }
        };
        self.written = overrides;
    }

    /// How `binding` is named on the page, for the pad that last spoke.
    fn label(&self, binding: &Binding) -> String {
        let kind = self.actions.last_pad_kind().unwrap_or(PadKind::Generic);
        DefaultLabels.label(binding, kind)
    }

    /// What the `index`th entry's row says.
    #[must_use]
    pub fn hint(&self, index: usize) -> String {
        if self.capture == (Capture::Listening { action: index }) {
            return LISTENING_HINT.to_owned();
        }
        let bindings = self.bindings_of(index);
        let mut hint = if bindings.is_empty() {
            UNBOUND_HINT.to_owned()
        } else {
            bindings
                .iter()
                .map(|binding| self.label(binding))
                .collect::<Vec<_>>()
                .join(" / ")
        };
        if self.actions.button_held(ACTIONS[index].name) {
            hint = format!("{hint} {HELD_MARK}");
        }
        hint
    }

    /// The lines under the page's title: what the page is waiting for, and
    /// what the last write did.
    #[must_use]
    pub fn subtitle(&self) -> Vec<Caption> {
        let mut lines = Vec::new();
        match &self.capture {
            Capture::Listening { action } => {
                lines.push(Caption::hint(format!(
                    "PRESS AN INPUT FOR {}",
                    ACTIONS[*action].label
                )));
                lines.push(Caption::hint("ESC CANCELS"));
            }
            Capture::Idle | Capture::Conflict { .. } => {
                lines.push(Caption::hint("ENTER A ROW, THEN PRESS THE NEW INPUT"));
            }
        }
        if self.refused > 0 {
            lines.push(Caption::warning(format!(
                "{} SAVED BIND(S) SKIPPED: SEE THE LOG",
                self.refused
            )));
        }
        match &self.saved {
            SaveState::Untouched | SaveState::Unsaved => {}
            SaveState::Failed(_) => lines.push(Caption::warning(self.saved.hint())),
            SaveState::Saved | SaveState::Nowhere => lines.push(Caption::hint(self.saved.hint())),
        }
        lines
    }

    /// The lines under the clash panel's title, naming the input and both
    /// actions — empty when there is no clash.
    #[must_use]
    pub fn conflict_subtitle(&self) -> Vec<Caption> {
        let Capture::Conflict {
            action,
            binding,
            other,
        } = &self.capture
        else {
            return Vec::new();
        };
        vec![
            Caption::warning(format!(
                "{} IS ON {}",
                self.label(binding),
                ACTIONS[*other].label
            )),
            Caption::hint(format!(
                "SWAP GIVES {} THE OLD INPUT OF {}",
                ACTIONS[*other].label, ACTIONS[*action].label
            )),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replacing on a device keeps every other device's binding where it was.
    #[test]
    fn a_capture_replaces_only_its_own_device() {
        let bindings = [
            Binding::Key(KeyCode::Space),
            Binding::PadButton(PadButton::South),
            Binding::Key(KeyCode::KeyK),
        ];
        assert_eq!(
            replaced_on_device(&bindings, &Binding::Key(KeyCode::KeyJ)),
            [
                Binding::Key(KeyCode::KeyJ),
                Binding::PadButton(PadButton::South),
            ]
        );
        assert_eq!(
            replaced_on_device(&bindings, &Binding::MouseButton(PointerButton::Right)),
            [
                Binding::Key(KeyCode::Space),
                Binding::PadButton(PadButton::South),
                Binding::Key(KeyCode::KeyK),
                Binding::MouseButton(PointerButton::Right),
            ]
        );
    }

    /// Every row id names its action back, and no id is shared.
    #[test]
    fn every_action_row_names_its_action_back() {
        for index in 0..ACTIONS.len() {
            assert_eq!(action_of(action_id(index)), Some(index));
        }
        assert_eq!(action_of(CANCEL_ID), None);
        let mut ids: Vec<WidgetId> = (0..ACTIONS.len()).map(action_id).collect();
        ids.extend([CONTROLS_ID, RESET_CONTROLS_ID, BACK_ID, SWAP_ID, CANCEL_ID]);
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two rows share an id");
    }
}
