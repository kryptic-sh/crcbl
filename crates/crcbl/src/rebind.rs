//! The listen-for-input rebind flow: a page of actions a player rebinds by
//! pressing the input, asked on a clash, and kept in their profile between
//! runs.
//!
//! ```text
//!   a page ── an action row ──▶ listening
//!     ▲                            │ the next input
//!     │      ┌──── no clash ───────┤
//!     │      ▼                     ▼ a clash
//!     └── rebound ◀── SWAP ── Conflict ── CANCEL ──┘
//! ```
//!
//! [`Rebinder`] is the state machine and the profile, [`RebindIds`] the widget
//! ids its two panels use, and [`menus`] builds those panels. A game
//! hosts it: it declares its own actions with their defaults, lists the ones a
//! player may rebind as [`RebindRow`]s, hands the input hooks through, and
//! answers [`HostedGame::captures_input`] with [`Rebinder::listening`].
//! `apps/options`' `CONTROLS` page and `apps/puppet`'s controls overlay are
//! the two hosts; this module is the half they share.
//!
//! # Listening
//!
//! Choosing an action row sets the flow listening, and
//! [`HostedGame::captures_input`] hands it every input the loop would
//! otherwise keep for the menu: the next key press, mouse button or pad button
//! becomes the binding. **Escape cancels**, so it cannot itself be bound; F3,
//! F11 and the console key stay the loop's. A captured input replaces the
//! action's bindings **on the same device** and leaves the others — rebinding
//! jump to `J` keeps its pad button.
//!
//! **The input hooks here observe and never feed the map.** Where the map's
//! edges land is the host's to decide — a game replays its inputs after
//! [`ActionMap::begin_tick`], and a page with no simulation can feed them as
//! they come — so the host feeds [`Rebinder::actions_mut`] itself and hands
//! the same input to [`Rebinder::key`], [`Rebinder::button`] or
//! [`Rebinder::pad`] as well.
//!
//! # A clash
//!
//! An input already bound to another action in the same context opens the
//! clash panel, and **the player chooses**: `SWAP` gives the other action this
//! one's old input on that device, `CANCEL` changes nothing. An automatic swap
//! would move a binding the player never looked at, and refusing outright
//! would make every reshuffle two steps through an unbound action; asking
//! costs one press and surprises nobody. The other action need not be a row
//! of the page: a game that lists only some of its actions still asks before
//! taking an input from one it does not list, and names it by its action name.
//!
//! # Kept
//!
//! The player's rebinds are written to their profile
//! ([`crate::store::profile`]) **whenever the map's overrides change**, so a
//! rebind on the page, `RESET CONTROLS` and the console's `bind` all reach the
//! file through one rule rather than one writer each. A headless run writes
//! nowhere, as the settings file's own save does.
//!
//! [`HostedGame::captures_input`]: crate::engine::HostedGame::captures_input

use crate::core::input::{KeyCode, PointerButton};
use crate::input::{
    ActionMap, Binding, DefaultLabels, GamepadEvent, GamepadId, HintLabels, PadButton, PadButtons,
    PadKind,
};
use crate::store::profile::{Profile, ProfileStore};
use crcbl_ui::WidgetId;
use crcbl_ui::menu::{Caption, Menu, MenuItem};

/// One action a page offers: its name in the map and in the profile, and the
/// label its row wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebindRow {
    /// The action's name, which is also its key in the profile.
    pub name: &'static str,
    /// What its row says.
    pub label: &'static str,
}

/// The widget ids a page and its clash panel use: one block, starting where
/// [`RebindIds::starting_at`] says, so a host places the whole flow among its
/// own ids by choosing one number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebindIds {
    first: WidgetId,
}

impl RebindIds {
    /// The block starting at `first`. Every id from `first` up to
    /// [`RebindIds::action`] of the last row is the flow's.
    #[must_use]
    pub const fn starting_at(first: WidgetId) -> Self {
        Self { first }
    }

    /// The row that puts every action back on its defaults.
    #[must_use]
    pub const fn reset(self) -> WidgetId {
        self.first
    }

    /// The row that leaves the page.
    #[must_use]
    pub const fn back(self) -> WidgetId {
        self.first + 1
    }

    /// The clash panel's row that takes the input from the other action.
    #[must_use]
    pub const fn swap(self) -> WidgetId {
        self.first + 2
    }

    /// The clash panel's row that leaves both actions as they were.
    #[must_use]
    pub const fn cancel(self) -> WidgetId {
        self.first + 3
    }

    /// The row for the `index`th [`RebindRow`].
    #[must_use]
    pub const fn action(self, index: usize) -> WidgetId {
        self.cancel() + 1 + index as WidgetId
    }

    /// The row index an id names among `rows` rows, or `None` for any other
    /// id.
    #[must_use]
    pub fn action_of(self, id: WidgetId, rows: usize) -> Option<usize> {
        (0..rows).find(|index| self.action(*index) == id)
    }
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

/// What the page is doing with the player's next input.
#[derive(Clone, Debug, PartialEq)]
pub enum Capture {
    /// Nothing: the menu has the input.
    Idle,
    /// Waiting for the input to bind to the `action`th row.
    Listening {
        /// The row being rebound.
        action: usize,
    },
    /// The input `binding` is already `other`'s, and the player is choosing.
    Conflict {
        /// The row being rebound.
        action: usize,
        /// The input the player pressed.
        binding: Binding,
        /// The action that already has it, by name — a row of the page or
        /// not.
        other: String,
    },
}

/// What the last write of the profile did.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ProfileWrite {
    /// Nothing has been written: the binds have not moved since the profile
    /// was opened.
    #[default]
    Untouched,
    /// Written to the player's own profile.
    Saved,
    /// This run has nowhere to write — a headless run, which must not touch
    /// whichever home directory it is executing in.
    Nowhere,
    /// The write was refused, with what the storage said. Shown rather than
    /// logged and dropped: a player told nothing would go on believing their
    /// binds are kept.
    Failed(String),
}

impl ProfileWrite {
    /// What this state says on the page, empty while nothing was written.
    #[must_use]
    pub fn hint(&self) -> String {
        match self {
            Self::Untouched => String::new(),
            Self::Saved => "SAVED".to_owned(),
            Self::Nowhere => "NOWHERE TO SAVE".to_owned(),
            Self::Failed(error) => format!("FAILED: {error}"),
        }
    }
}

/// The flow's state: the map, the rows a player may rebind, the profile they
/// are kept in, and the capture — see the [module docs](self).
#[derive(Debug)]
pub struct Rebinder {
    actions: ActionMap,
    rows: Vec<RebindRow>,
    store: ProfileStore,
    /// The profile as it was loaded, carrying any binds for actions this
    /// build does not declare, which each save keeps.
    profile: Profile,
    /// The overrides the profile last had written, or was loaded with — what
    /// a change is measured against.
    written: Vec<(String, Vec<String>)>,
    capture: Capture,
    /// What the last write did.
    saved: ProfileWrite,
    /// How many of the loaded profile's entries were refused.
    refused: usize,
    /// Every key down, so an auto-repeat of a key held when listening began
    /// is not read as the player's choice.
    keys: Vec<KeyCode>,
    /// The buttons each pad last held, so only a button going down is read.
    pads: Vec<(GamepadId, PadButtons)>,
}

impl Rebinder {
    /// `defaults` with the profile in `store` on top, offering `rows`.
    ///
    /// A profile that cannot be read is logged and stands as the defaults; an
    /// entry naming an action this build does not declare, or a binding it
    /// cannot read, is logged and skipped — see
    /// [`ActionMap::apply_override_text`]. Nothing is written back at open:
    /// opening is a read.
    #[must_use]
    pub fn open(defaults: ActionMap, rows: Vec<RebindRow>, store: ProfileStore) -> Self {
        let profile = store.load_or_default();
        let mut actions = defaults;
        let refusals = actions.apply_override_text(profile.binds());
        for refusal in &refusals {
            crate::log::warn!("profile: {}: {refusal}", store.file());
        }
        let written = actions.override_text();
        Self {
            actions,
            rows,
            store,
            profile,
            written,
            capture: Capture::Idle,
            saved: ProfileWrite::default(),
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

    /// The map, for the host to feed and for the console's `bind` to rebind.
    pub const fn actions_mut(&mut self) -> &mut ActionMap {
        &mut self.actions
    }

    /// The rows the page lists, in order.
    #[must_use]
    pub fn rows(&self) -> &[RebindRow] {
        &self.rows
    }

    /// What the page is doing with the next input.
    #[must_use]
    pub const fn capture(&self) -> &Capture {
        &self.capture
    }

    /// Whether the page is waiting for an input to bind — what a host
    /// answers [`HostedGame::captures_input`](crate::engine::HostedGame::captures_input)
    /// with.
    #[must_use]
    pub const fn listening(&self) -> bool {
        matches!(self.capture, Capture::Listening { .. })
    }

    /// What the last write of the profile did.
    #[must_use]
    pub const fn saved(&self) -> &ProfileWrite {
        &self.saved
    }

    /// Writes every row's hint and the caption under the page's title onto
    /// `page`, as [`menus`] built it.
    pub fn refresh_page(&self, ids: RebindIds, page: &mut Menu) {
        for index in 0..self.rows.len() {
            page.set_item_hint(ids.action(index), self.hint(index));
        }
        page.subtitle = self.subtitle();
    }

    /// Writes the caption naming the clash onto the clash panel — empty when
    /// there is none.
    pub fn refresh_conflict(&self, conflict: &mut Menu) {
        conflict.subtitle = self.conflict_subtitle();
    }

    /// Starts listening for the `action`th row.
    pub fn listen(&mut self, action: usize) {
        if action < self.rows.len() {
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
        let name = self.rows[action].name;
        let device = binding.device();
        let mine = self.bindings_of(name);
        let given: Vec<Binding> = mine
            .iter()
            .filter(|old| old.device() == device)
            .cloned()
            .collect();
        let theirs = self.bindings_of(&other);
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
        self.rebind(&other, swapped);
        self.rebind(name, replaced_on_device(&mine, &binding));
    }

    /// Takes the clash panel's `CANCEL`, or Escape while listening, or a host
    /// leaving the page: nothing changes.
    pub fn cancel(&mut self) {
        self.capture = Capture::Idle;
    }

    /// Puts every action back on its defaults.
    pub fn reset(&mut self) {
        self.capture = Capture::Idle;
        let refusals = self.actions.apply_override_text(std::iter::empty());
        for refusal in refusals {
            crate::log::warn!("controls: {refusal}");
        }
    }

    /// A key went down or came up. While listening, a press of a key not
    /// already down is the player's choice — or, for Escape, their cancel.
    ///
    /// Observes only: the host feeds the map — see the [module docs](self).
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
        if fresh && self.listening() {
            if key == crate::engine::PAUSE_KEY {
                self.cancel();
            } else {
                self.captured(Binding::Key(key));
            }
        }
    }

    /// A mouse button went down or came up — the primary one included, which
    /// the loop reports as a pointer press. While listening, a press is the
    /// player's choice.
    ///
    /// Observes only, as [`Rebinder::key`] does.
    pub fn button(&mut self, button: PointerButton, pressed: bool) {
        if pressed && self.listening() {
            self.captured(Binding::MouseButton(button));
        }
    }

    /// A pad event. While listening, the first button it puts down that was
    /// up is the player's choice.
    ///
    /// Observes only, as [`Rebinder::key`] does — but every event, listening
    /// or not, so a button already held when listening began is known to be
    /// held.
    pub fn pad(&mut self, event: &GamepadEvent) {
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
        let name = self.rows[action].name;
        match self
            .actions
            .bound_elsewhere(name, &binding)
            .map(str::to_owned)
        {
            Some(other) => {
                self.capture = Capture::Conflict {
                    action,
                    binding,
                    other,
                };
            }
            None => {
                self.capture = Capture::Idle;
                let mine = self.bindings_of(name);
                self.rebind(name, replaced_on_device(&mine, &binding));
            }
        }
    }

    /// The named action's current bindings.
    fn bindings_of(&self, name: &str) -> Vec<Binding> {
        self.actions.bindings(name).unwrap_or_default().to_vec()
    }

    /// Rebinds the named action, logging the one refusal the map can give —
    /// a pad threshold out of range, which nothing captured here carries.
    fn rebind(&mut self, name: &str, bindings: Vec<Binding>) {
        if let Err(error) = self.actions.rebind(name, bindings) {
            crate::log::warn!("controls: {error}");
        }
    }

    /// Writes the profile if the map's overrides moved since the last write,
    /// whoever moved them, and answers whether they had.
    ///
    /// A failure is shown on the page and not retried every frame: the next
    /// change tries again.
    pub fn persist(&mut self) -> bool {
        let overrides = self.actions.override_text();
        if overrides == self.written {
            return false;
        }
        let actions = &self.actions;
        self.profile
            .set_binds(|name| actions.bindings(name).is_some(), overrides.clone());
        self.saved = match self.store.save(&self.profile) {
            Ok(true) => ProfileWrite::Saved,
            Ok(false) => ProfileWrite::Nowhere,
            Err(error) => {
                crate::log::warn!("controls: {error}");
                ProfileWrite::Failed(error.to_string())
            }
        };
        self.written = overrides;
        true
    }

    /// How `binding` is named on the page, for the pad that last spoke.
    fn label(&self, binding: &Binding) -> String {
        let kind = self.actions.last_pad_kind().unwrap_or(PadKind::Generic);
        DefaultLabels.label(binding, kind)
    }

    /// What the named action is called on the page: its row's label, or for
    /// an action the page does not list, its name.
    fn label_of<'a>(&'a self, name: &'a str) -> std::borrow::Cow<'a, str> {
        self.rows
            .iter()
            .find(|row| row.name == name)
            .map_or_else(|| name.to_uppercase().into(), |row| row.label.into())
    }

    /// What the `index`th row says: its bindings, each label once — two keys
    /// that print the same, such as the two Shifts, are one entry to a
    /// reader.
    #[must_use]
    pub fn hint(&self, index: usize) -> String {
        if self.capture == (Capture::Listening { action: index }) {
            return LISTENING_HINT.to_owned();
        }
        let name = self.rows[index].name;
        let mut labels: Vec<String> = Vec::new();
        for binding in self.bindings_of(name) {
            let label = self.label(&binding);
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        let mut hint = if labels.is_empty() {
            UNBOUND_HINT.to_owned()
        } else {
            labels.join(" / ")
        };
        if self.actions.button_held(name) {
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
                    self.rows[*action].label
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
            ProfileWrite::Untouched => {}
            ProfileWrite::Failed(_) => lines.push(Caption::warning(self.saved.hint())),
            ProfileWrite::Saved | ProfileWrite::Nowhere => {
                lines.push(Caption::hint(self.saved.hint()));
            }
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
        let other = self.label_of(other);
        vec![
            Caption::warning(format!("{} IS ON {other}", self.label(binding))),
            Caption::hint(format!(
                "SWAP GIVES {other} THE OLD INPUT OF {}",
                self.rows[*action].label
            )),
        ]
    }
}

/// The page offering `rows` and the clash panel, with every row's hint left
/// for [`Rebinder::refresh_page`] to write.
///
/// A free function over the rows rather than a method, because a host builds
/// its menus before it has opened a profile —
/// [`HostedGame::menus`](crate::engine::HostedGame::menus) takes no `self`.
#[must_use]
pub fn menus(ids: RebindIds, rows: &[RebindRow]) -> (Menu, Menu) {
    let mut items: Vec<MenuItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| MenuItem::new(ids.action(index), row.label, ""))
        .collect();
    items.push(MenuItem::new(ids.reset(), "RESET CONTROLS", ""));
    items.push(MenuItem::new(ids.back(), "BACK", ""));
    let conflict = Menu::new(
        CONFLICT_TITLE,
        vec![
            MenuItem::new(ids.swap(), "SWAP", ""),
            MenuItem::new(ids.cancel(), "CANCEL", ""),
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

#[cfg(test)]
mod tests;
