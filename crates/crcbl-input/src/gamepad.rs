//! The gamepad seam: the one vocabulary every pad backend speaks, and how an
//! [`ActionMap`] resolves it.
//!
//! **Every backend — XInput, evdev, GameController, the Web Gamepad API, Steam
//! Input — emits [`GamepadEvent`]s and nothing else**, by the conventions
//! below, so a game written against them cannot tell which backend spoke.
//!
//! # Two ways to consume them
//!
//! - **Directly.** The events are plain data: a game with its own input
//!   adapter matches on them and reads [`GamepadSnapshot::stick`] and
//!   [`GamepadSnapshot::trigger`], raw and finite, and never builds an
//!   [`ActionMap`] at all.
//! - **Through an [`ActionMap`]**, the optional layer on top:
//!   [`ActionMap::gamepad_event`] resolves them against
//!   [`Binding::PadButton`], [`Binding::PadDpad`], [`Binding::PadStick`] and
//!   [`Binding::PadTrigger`], with dead zones, contexts and `last_device`.
//!
//! A game can do both from one stream, since an event is `Copy`:
//!
//! ```
//! use crcbl_input::{GamepadEvent, Stick};
//!
//! /// A game's own adapter: the left stick of the pad that last moved.
//! #[derive(Default)]
//! struct Adapter {
//!     left: Option<(u64, (f32, f32))>,
//! }
//!
//! impl Adapter {
//!     fn feed(&mut self, event: &GamepadEvent) {
//!         match *event {
//!             GamepadEvent::State { id, snapshot } => {
//!                 self.left = Some((u64::from(id.0), snapshot.stick(Stick::Left)));
//!             }
//!             GamepadEvent::Disconnected { id } => {
//!                 self.left = self.left.filter(|(pad, _)| *pad != u64::from(id.0));
//!             }
//!             GamepadEvent::Connected { .. } => {}
//!         }
//!     }
//! }
//!
//! # let events: Vec<GamepadEvent> = Vec::new();
//! let mut adapter = Adapter::default();
//! let mut map = crcbl_input::ActionMap::new(); // optional
//! for event in &events {
//!     adapter.feed(event);
//!     map.gamepad_event(event);
//! }
//! ```
//!
//! # Conventions a backend must meet
//!
//! - **Buttons are positional.** [`PadButton::South`] is the bottom face button
//!   whatever is printed on it — A on an Xbox pad, Cross on a PlayStation one,
//!   B on a Nintendo one. What the button is *called* is a glyph's business, not
//!   a binding's (`docs/notes/simulation.md`).
//! - **Sticks are −1…1 with +X right and +Y up**, the convention
//!   [`ActionMap::virtual_stick`] and [`Binding::PointerPosition`] already use.
//!   A backend whose device reports +Y down flips it; nothing downstream does.
//! - **Triggers are 0…1**, 0 at rest.
//! - **Axes are raw.** No dead zone is applied by a backend: the dead zone is a
//!   binding's ([`Binding::PadStick`]'s `deadzone`, [`Binding::PadTrigger`]'s
//!   `threshold`), so two actions on one stick can disagree about it, and a game
//!   that filters for itself reads the same raw values a binding does.
//! - **Axes are finite.** A direct consumer can rely on it from a backend in
//!   this workspace, and [`GamepadSnapshot::is_finite`] checks it; the map
//!   drops a snapshot with a non-finite axis anyway and keeps the pad's
//!   previous state, as [`ActionMap::mouse_motion`] drops a `NaN` delta.
//!
//! # A snapshot is a level
//!
//! A [`GamepadEvent::State`] says what the pad is doing *now*, not what changed:
//! a backend may send one per poll or only when something moved, and the map
//! holds the last one until the next arrives — so a backend that sends on change
//! costs nothing while the pad is idle, and one that re-sends an identical
//! snapshot changes nothing. Edges are the map's to find, by comparing a
//! snapshot against the one before it.
//!
//! # Every pad drives every pad binding
//!
//! Until local-multiplayer device assignment lands, a [`Binding::PadButton`] is
//! down while *any* connected pad holds it, a trigger reads the furthest-pulled
//! pad, and a [`Binding::PadStick`] sums every pad's filtered deflection into
//! the same unit disc a [`Binding::Wasd`] and a [`Binding::PadDpad`] share
//! with it.
//!
//! # Focus loss and disconnection release
//!
//! [`ActionMap::release_gamepads`] drops every pad to neutral — the pad half of
//! what `crcbl::engine::lose_focus` does for keys — and a pad button held
//! through it is **withheld until it is released**, exactly as a key held across
//! a context push is (`context.rs`): the player coming back from an alt-tab with
//! A still down does not jump. Sticks and triggers are levels and are not
//! withheld by it; the pad's next snapshot restores them. Only
//! [`ActionMap::suppress_held`] withholds a stick or trigger, until it rests
//! (`context.rs`). A [`GamepadEvent::Disconnected`]
//! pad's held state is released on the spot.

use std::sync::atomic::{AtomicU32, Ordering};

use super::{ActionMap, Binding, Device, Pads};

/// One physical pad, for as long as it stays connected.
///
/// Allocated by the input layer with [`GamepadId::allocate`], so ids from two
/// backends never collide. A pad that is unplugged and plugged back in is a
/// new id wherever the backend cannot tell it is the same device — XInput, for
/// one, identifies a slot and not a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GamepadId(pub u32);

impl GamepadId {
    /// A fresh id, never returned before in this process.
    ///
    /// # Panics
    /// If the process has allocated every `u32`, which at one pad a second is
    /// over a century of plugging.
    #[must_use]
    pub fn allocate() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(id, 0, "gamepad ids wrapped");
        Self(id)
    }
}

/// A pad button, by **position** — see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    /// The bottom face button: A (Xbox), Cross (PlayStation), B (Nintendo).
    South,
    /// The right face button: B (Xbox), Circle (PlayStation), A (Nintendo).
    East,
    /// The left face button: X (Xbox), Square (PlayStation), Y (Nintendo).
    West,
    /// The top face button: Y (Xbox), Triangle (PlayStation), X (Nintendo).
    North,
    /// The left bumper: LB, L1, L.
    LeftShoulder,
    /// The right bumper: RB, R1, R.
    RightShoulder,
    /// The left stick pressed in: LS, L3.
    LeftStick,
    /// The right stick pressed in: RS, R3.
    RightStick,
    /// The right centre button: Menu, Options, +.
    Start,
    /// The left centre button: View, Share/Create, −.
    Select,
    /// The d-pad's up.
    DpadUp,
    /// The d-pad's down.
    DpadDown,
    /// The d-pad's left.
    DpadLeft,
    /// The d-pad's right.
    DpadRight,
    /// The logo button: Xbox, PS, Home. Many platforms keep it for themselves,
    /// so a backend that never sees it never reports it.
    Guide,
}

impl PadButton {
    /// Every button, in declaration order.
    pub const ALL: [Self; 15] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftStick,
        Self::RightStick,
        Self::Start,
        Self::Select,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::Guide,
    ];

    /// The four buttons a [`Binding::PadDpad`] reads.
    pub(crate) const DPAD: [Self; 4] = [
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// A set of [`PadButton`]s: the ones a pad holds down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PadButtons(u16);

impl PadButtons {
    /// No button held.
    pub const EMPTY: Self = Self(0);

    /// Whether `button` is in the set.
    #[must_use]
    pub const fn contains(self, button: PadButton) -> bool {
        self.0 & button.bit() != 0
    }

    /// Adds `button` to the set.
    pub const fn insert(&mut self, button: PadButton) {
        self.0 |= button.bit();
    }

    /// Removes `button` from the set.
    pub const fn remove(&mut self, button: PadButton) {
        self.0 &= !button.bit();
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl FromIterator<PadButton> for PadButtons {
    fn from_iter<I: IntoIterator<Item = PadButton>>(buttons: I) -> Self {
        let mut set = Self::EMPTY;
        for button in buttons {
            set.insert(button);
        }
        set
    }
}

/// One of a pad's analog axes, and its index into [`GamepadSnapshot::axes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadAxis {
    /// The left stick's X, −1…1, +X right.
    LeftX,
    /// The left stick's Y, −1…1, **+Y up**.
    LeftY,
    /// The right stick's X, −1…1, +X right.
    RightX,
    /// The right stick's Y, −1…1, **+Y up**.
    RightY,
    /// The left trigger, 0…1.
    LeftTrigger,
    /// The right trigger, 0…1.
    RightTrigger,
}

/// Which stick a [`Binding::PadStick`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    /// The left stick.
    Left,
    /// The right stick.
    Right,
}

/// Which trigger a [`Binding::PadTrigger`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// The left trigger: LT, L2, ZL.
    Left,
    /// The right trigger: RT, R2, ZR.
    Right,
}

/// What family a pad is, for the glyphs a hint shows. Bindings never read it:
/// buttons are positional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PadKind {
    /// An Xbox-layout pad, and anything XInput reports.
    Xbox,
    /// A DualShock or DualSense.
    PlayStation,
    /// A Switch Pro controller or Joy-Con pair.
    Switch,
    /// The Steam Deck's built-in controls.
    SteamDeck,
    /// Anything the backend could not place.
    #[default]
    Generic,
}

/// What one pad is doing now — a level, not a delta, like
/// [`ActionMap::virtual_stick`]'s deflection. See the module docs for the
/// conventions every field follows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadSnapshot {
    /// The buttons held down.
    pub buttons: PadButtons,
    /// Every axis, indexed by [`PadAxis`] (`axes[PadAxis::LeftY as usize]`):
    /// sticks −1…1 with +Y up, triggers 0…1, all raw.
    pub axes: [f32; 6],
    /// The pad's family.
    pub kind: PadKind,
}

impl GamepadSnapshot {
    /// A pad of `kind` at rest: nothing held, sticks centred, triggers out.
    #[must_use]
    pub const fn neutral(kind: PadKind) -> Self {
        Self {
            buttons: PadButtons::EMPTY,
            axes: [0.0; 6],
            kind,
        }
    }

    /// One axis's value.
    #[must_use]
    pub const fn axis(&self, axis: PadAxis) -> f32 {
        self.axes[axis as usize]
    }

    /// A stick's deflection as `(x, y)`, +Y up, with no dead zone applied.
    #[must_use]
    pub const fn stick(&self, stick: Stick) -> (f32, f32) {
        match stick {
            Stick::Left => (self.axis(PadAxis::LeftX), self.axis(PadAxis::LeftY)),
            Stick::Right => (self.axis(PadAxis::RightX), self.axis(PadAxis::RightY)),
        }
    }

    /// A trigger's pull, 0…1, with no threshold applied.
    #[must_use]
    pub const fn trigger(&self, trigger: Trigger) -> f32 {
        match trigger {
            Trigger::Left => self.axis(PadAxis::LeftTrigger),
            Trigger::Right => self.axis(PadAxis::RightTrigger),
        }
    }

    /// Whether every axis is finite — what the map requires of a snapshot
    /// before it takes one.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.axes.iter().all(|value| value.is_finite())
    }
}

/// What a backend reports about a pad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GamepadEvent {
    /// A pad appeared, at rest.
    Connected {
        /// The pad.
        id: GamepadId,
        /// Its family.
        kind: PadKind,
    },
    /// A pad went away; whatever it held is released.
    Disconnected {
        /// The pad.
        id: GamepadId,
    },
    /// What a pad is doing now.
    State {
        /// The pad.
        id: GamepadId,
        /// Its buttons and axes.
        snapshot: GamepadSnapshot,
    },
}

/// How far a stick or trigger must travel before the pad counts as the device
/// that last spoke — see [`ActionMap::last_device`].
///
/// Past any stick's rest noise: XInput's own suggested dead zones put a worn
/// stick's drift at about a quarter of the throw (`XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE`
/// is 7849 of 32767), and a pad that took the glyphs back from the keyboard
/// every time its stick settled would flicker the whole UI.
pub const PAD_ACTIVITY_THRESHOLD: f32 = 0.5;

/// A stick's `(x, y)` with a **scaled radial** dead zone applied: zero within
/// `deadzone` of centre, then rescaled so the edge of the dead zone reads 0 and
/// full throw reads 1, clamped to the unit disc, with the direction kept.
///
/// Radial rather than per-axis, because a per-axis dead zone snaps a slightly
/// diagonal push onto the nearest axis; rescaled rather than cut, because a cut
/// jumps from 0 straight to `deadzone` the moment the stick leaves it.
pub(crate) fn radial_deadzone((x, y): (f32, f32), deadzone: f32) -> (f32, f32) {
    let magnitude = x.hypot(y);
    if !magnitude.is_finite() || magnitude <= deadzone {
        return (0.0, 0.0);
    }
    let scaled = (magnitude.min(1.0) - deadzone) / (1.0 - deadzone);
    (x / magnitude * scaled, y / magnitude * scaled)
}

/// A trigger's pull with `threshold` applied: zero at or below it, then
/// rescaled so the threshold reads 0 and full pull reads 1.
pub(crate) fn trigger_past(value: f32, threshold: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= threshold {
        0.0
    } else {
        (value - threshold) / (1.0 - threshold)
    }
}

/// Whether a dead zone or threshold is one a binding can use: finite, and in
/// `0.0..1.0` — at 1.0 nothing could ever get past it.
pub(crate) fn valid_deadzone(value: f32) -> bool {
    (0.0..1.0).contains(&value)
}

/// Whether any stick or trigger is past [`PAD_ACTIVITY_THRESHOLD`].
fn deflected(snapshot: &GamepadSnapshot) -> [bool; 4] {
    let stick = |stick| {
        let (x, y) = snapshot.stick(stick);
        x.hypot(y) > PAD_ACTIVITY_THRESHOLD
    };
    [
        stick(Stick::Left),
        stick(Stick::Right),
        snapshot.trigger(Trigger::Left) > PAD_ACTIVITY_THRESHOLD,
        snapshot.trigger(Trigger::Right) > PAD_ACTIVITY_THRESHOLD,
    ]
}

impl ActionMap {
    /// Feed a pad event from any backend — the one way pads reach a map.
    ///
    /// Optional: the events mean the same to a game that reads them directly
    /// and never calls this (see the module docs).
    ///
    /// **Activity** makes [`Device::Gamepad`] the last device, by the rule
    /// `device.rs` states for the others: a button going down, or a stick or
    /// trigger moving out past [`PAD_ACTIVITY_THRESHOLD`]. A release does not,
    /// a stick returning to centre does not, and neither does a stick held
    /// where it already was — holding it over is like holding a key down, and
    /// the pad spoke when it went over. A connection or disconnection is not
    /// the player speaking either.
    ///
    /// A [`GamepadEvent::State`] with a non-finite axis is dropped, and the
    /// pad keeps its previous state. One for a pad never announced as
    /// [`GamepadEvent::Connected`] connects it.
    pub fn gamepad_event(&mut self, event: &GamepadEvent) {
        match *event {
            GamepadEvent::Connected { id, kind } => {
                self.pads.insert(id, GamepadSnapshot::neutral(kind));
            }
            GamepadEvent::Disconnected { id } => {
                self.pads.remove(&id);
            }
            GamepadEvent::State { id, snapshot } => {
                if !snapshot.is_finite() {
                    return;
                }
                let before = self
                    .pads
                    .insert(id, snapshot)
                    .unwrap_or(GamepadSnapshot::neutral(snapshot.kind));
                let pressed = !snapshot.buttons.difference(before.buttons).is_empty();
                let went_over = deflected(&before)
                    .into_iter()
                    .zip(deflected(&snapshot))
                    .any(|(was, is)| is && !was);
                if pressed || went_over {
                    self.last_device = Some(Device::Gamepad);
                }
            }
        }
        self.repad();
    }

    /// Drop every pad to neutral — what focus loss owes the pads, called beside
    /// `crcbl::engine::lose_focus`.
    ///
    /// Each pad stays connected at rest until its next snapshot. **A button
    /// held through this is withheld until it is released**, so a pad still
    /// holding A when the window comes back does not press A again; sticks and
    /// triggers are levels and come back with the next snapshot. The engine
    /// loop calls this on the map a game hands it through `Game::actions`; a
    /// game that feeds pads into a map it keeps to itself calls it on focus
    /// loss too.
    pub fn release_gamepads(&mut self) {
        self.suppressed.pad_buttons = self.suppressed.pad_buttons.union(self.held_pad_buttons);
        for snapshot in self.pads.values_mut() {
            *snapshot = GamepadSnapshot::neutral(snapshot.kind);
        }
        // Not `repad`, which would lift the withholding just added: nothing is
        // held now, and the lift waits for a snapshot that says so.
        self.held_pad_buttons = PadButtons::EMPTY;
        self.resolve_matching(Binding::reads_gamepad);
    }

    /// The pads currently connected, and what each last reported.
    pub fn gamepads(&self) -> impl Iterator<Item = (GamepadId, &GamepadSnapshot)> {
        self.pads.iter().map(|(&id, snapshot)| (id, snapshot))
    }

    /// Recompute what the pads hold between them, lift the withholding of any
    /// button no pad holds any more and of any stick or trigger back at rest,
    /// and re-resolve every pad binding.
    fn repad(&mut self) {
        self.held_pad_buttons = self
            .pads
            .values()
            .fold(PadButtons::EMPTY, |held, pad| held.union(pad.buttons));
        self.suppressed.pad_buttons = self
            .suppressed
            .pad_buttons
            .intersection(self.held_pad_buttons);
        for stick in [Stick::Left, Stick::Right] {
            if self.stick_rests(stick) {
                self.suppressed.pad_sticks.remove(&stick);
            }
        }
        for trigger in [Trigger::Left, Trigger::Right] {
            if self.trigger_rests(trigger) {
                self.suppressed.pad_triggers.remove(&trigger);
            }
        }
        self.resolve_matching(Binding::reads_gamepad);
    }

    /// Whether `stick` is at rest on every pad: inside the dead zone of every
    /// [`Binding::PadStick`] on it — reading zero through each of them — or
    /// exactly centred if none reads it. What lifts a suppressed stick.
    pub(crate) fn stick_rests(&self, stick: Stick) -> bool {
        let deadzone = self
            .bindings_all()
            .filter_map(|binding| match *binding {
                Binding::PadStick {
                    stick: bound,
                    deadzone,
                } if bound == stick => Some(deadzone),
                _ => None,
            })
            .reduce(f32::min)
            .unwrap_or(0.0);
        self.pads.values().all(|pad| {
            let (x, y) = pad.stick(stick);
            x.hypot(y) <= deadzone
        })
    }

    /// Whether `trigger` is at rest on every pad: at or below the threshold of
    /// every [`Binding::PadTrigger`] on it, or fully out if none reads it.
    /// What lifts a suppressed trigger.
    pub(crate) fn trigger_rests(&self, trigger: Trigger) -> bool {
        let threshold = self
            .bindings_all()
            .filter_map(|binding| match *binding {
                Binding::PadTrigger {
                    trigger: bound,
                    threshold,
                } if bound == trigger => Some(threshold),
                _ => None,
            })
            .reduce(f32::min)
            .unwrap_or(0.0);
        self.pads
            .values()
            .all(|pad| pad.trigger(trigger) <= threshold)
    }

    /// Every binding of every declared action.
    fn bindings_all(&self) -> impl Iterator<Item = &Binding> {
        self.slots.iter().flat_map(|slot| slot.decl.bindings.iter())
    }
}

/// The furthest any pad pulls `trigger`.
pub(crate) fn pad_trigger(pads: &Pads, trigger: Trigger) -> f32 {
    pads.values()
        .map(|pad| pad.trigger(trigger))
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionMapError};
    use crcbl_core::input::KeyCode;

    const TICK: f32 = 1.0 / 60.0;
    const PAD: GamepadId = GamepadId(7);

    fn action(name: &str, kind: ActionKind, bindings: Vec<Binding>) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind,
            bindings,
        }
    }

    fn state(id: GamepadId, snapshot: GamepadSnapshot) -> GamepadEvent {
        GamepadEvent::State { id, snapshot }
    }

    fn holding(buttons: &[PadButton]) -> GamepadSnapshot {
        GamepadSnapshot {
            buttons: buttons.iter().copied().collect(),
            ..GamepadSnapshot::neutral(PadKind::Xbox)
        }
    }

    fn with_axis(mut snapshot: GamepadSnapshot, axis: PadAxis, value: f32) -> GamepadSnapshot {
        snapshot.axes[axis as usize] = value;
        snapshot
    }

    /// A map with `jump` on South and `connected` pad [`PAD`].
    fn jump_map() -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(action(
            "jump",
            ActionKind::Button,
            vec![Binding::PadButton(PadButton::South)],
        ));
        map.gamepad_event(&GamepadEvent::Connected {
            id: PAD,
            kind: PadKind::Xbox,
        });
        map
    }

    fn close(actual: (f32, f32), expected: (f32, f32)) -> bool {
        (actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6
    }

    /// **A pad button has the edges a key has**: pressed on the snapshot it
    /// went down in, held while snapshots keep it down, released on the one
    /// that lets go — and an unrelated button changes nothing.
    #[test]
    fn a_pad_button_presses_holds_and_releases_through_its_binding() {
        let mut map = jump_map();
        map.begin_tick(TICK);
        map.gamepad_event(&state(PAD, holding(&[PadButton::East])));
        assert!(!map.button_held("jump"), "East is not South");

        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(map.just_pressed("jump"));
        assert!(map.button_held("jump"));

        map.begin_tick(TICK);
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(!map.just_pressed("jump"), "a re-sent level is not a press");
        assert!(map.button_held("jump"));

        map.begin_tick(TICK);
        map.gamepad_event(&state(PAD, holding(&[])));
        assert!(map.just_released("jump"));
        assert!(!map.button_held("jump"));
    }

    /// Two pads both drive the binding, and it stays down until neither holds
    /// it — the multiple-keys rule, for pads.
    #[test]
    fn a_button_held_on_two_pads_is_released_by_the_last_to_let_go() {
        let mut map = jump_map();
        let other = GamepadId(8);
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        map.gamepad_event(&state(other, holding(&[PadButton::South])));
        map.gamepad_event(&state(PAD, holding(&[])));
        assert!(map.button_held("jump"), "the other pad still holds it");
        map.gamepad_event(&state(other, holding(&[])));
        assert!(!map.button_held("jump"));

        // And in the other order, so neither pad's id is what decides.
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        map.gamepad_event(&state(other, holding(&[PadButton::South])));
        map.gamepad_event(&state(other, holding(&[])));
        assert!(map.button_held("jump"), "the first pad still holds it");
        map.gamepad_event(&state(PAD, holding(&[])));
        assert!(!map.button_held("jump"));
    }

    /// The scaled radial dead zone against hand-worked values: zero inside it,
    /// rescaled from its edge, direction kept, and clamped to the disc.
    #[test]
    fn the_radial_deadzone_zeroes_inside_rescales_outside_and_clamps() {
        assert_eq!(radial_deadzone((0.1, 0.0), 0.2), (0.0, 0.0));
        assert_eq!(radial_deadzone((0.0, -0.2), 0.2), (0.0, 0.0), "on the edge");
        // (0.6 − 0.2) / (1 − 0.2) = 0.5
        assert!(close(radial_deadzone((0.6, 0.0), 0.2), (0.5, 0.0)));
        assert!(close(radial_deadzone((0.0, -1.0), 0.2), (0.0, -1.0)));
        // Magnitude 1 exactly: kept as is.
        assert!(close(radial_deadzone((0.6, 0.8), 0.2), (0.6, 0.8)));
        // Magnitude 0.5 along (3, 4)/5: (0.5 − 0.2) / 0.8 = 0.375.
        assert!(close(radial_deadzone((0.3, 0.4), 0.2), (0.225, 0.3)));
        // Past the disc: clamped to it, direction kept.
        assert!(close(radial_deadzone((3.0, 4.0), 0.2), (0.6, 0.8)));
        assert!(
            close(radial_deadzone((0.3, 0.4), 0.0), (0.3, 0.4)),
            "no zone"
        );
    }

    /// The trigger threshold against hand-worked values.
    #[test]
    fn the_trigger_threshold_zeroes_below_and_rescales_above() {
        assert_eq!(trigger_past(0.0, 0.25), 0.0);
        assert_eq!(trigger_past(0.25, 0.25), 0.0, "at the threshold");
        // (0.5 − 0.25) / 0.75 = 1/3
        assert!((trigger_past(0.5, 0.25) - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(trigger_past(1.0, 0.25), 1.0);
        assert_eq!(trigger_past(2.0, 0.25), 1.0, "clamped to full pull");
    }

    /// **Stick and trigger bindings resolve through their dead zones**, on the
    /// kinds that read them.
    #[test]
    fn stick_and_trigger_bindings_resolve_through_their_deadzones() {
        let mut map = ActionMap::new();
        map.declare(action(
            "move",
            ActionKind::Axis2,
            vec![Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.2,
            }],
        ));
        map.declare(action(
            "throttle",
            ActionKind::Axis1,
            vec![Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.25,
            }],
        ));
        map.declare(action(
            "fire",
            ActionKind::Button,
            vec![Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.25,
            }],
        ));

        let rest = GamepadSnapshot::neutral(PadKind::Xbox);
        let drift = with_axis(rest, PadAxis::LeftX, 0.15);
        map.gamepad_event(&state(PAD, drift));
        assert_eq!(map.axis2("move"), (0.0, 0.0), "drift inside the zone");

        let pushed = with_axis(rest, PadAxis::LeftX, 0.6);
        let pulled = with_axis(pushed, PadAxis::RightTrigger, 0.5);
        map.gamepad_event(&state(PAD, pulled));
        assert!(
            close(map.axis2("move"), (0.5, 0.0)),
            "{:?}",
            map.axis2("move")
        );
        assert!((map.axis1("throttle") - 1.0 / 3.0).abs() < 1e-6);
        assert!(map.just_pressed("fire"), "past the threshold is down");

        map.gamepad_event(&state(PAD, with_axis(rest, PadAxis::RightTrigger, 0.25)));
        assert!(map.just_released("fire"), "at the threshold is up");
        assert_eq!(map.axis1("throttle"), 0.0);
    }

    /// **+Y is up**: a stick pushed up drives the same direction as the key
    /// bound to `up` on the same action, and a pushed-down one the other way.
    #[test]
    fn a_stick_pushed_up_reads_positive_y_like_the_up_key() {
        let mut map = ActionMap::new();
        map.declare(action(
            "move",
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
                    deadzone: 0.0,
                },
            ],
        ));
        let rest = GamepadSnapshot::neutral(PadKind::Xbox);

        map.key_event(KeyCode::KeyW, true);
        let by_key = map.axis2("move");
        map.key_event(KeyCode::KeyW, false);
        map.gamepad_event(&state(PAD, with_axis(rest, PadAxis::LeftY, 1.0)));
        assert_eq!(map.axis2("move"), by_key, "stick up is W");
        assert_eq!(map.axis2("move"), (0.0, 1.0));

        map.gamepad_event(&state(PAD, with_axis(rest, PadAxis::LeftY, -0.5)));
        assert_eq!(map.axis2("move"), (0.0, -0.5));
    }

    /// **The d-pad resolves exactly as the WASD composite does**: +Y up, a
    /// diagonal a unit vector, opposite directions cancelling — checked
    /// against hand-worked values and against a [`Binding::Wasd`] fed the same
    /// directions.
    #[test]
    fn the_dpad_resolves_to_an_axis2_like_wasd() {
        use PadButton::{DpadDown, DpadLeft, DpadRight, DpadUp};
        let mut map = ActionMap::new();
        map.declare(action("move", ActionKind::Axis2, vec![Binding::PadDpad]));
        map.declare(action(
            "walk",
            ActionKind::Axis2,
            vec![Binding::Wasd {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            }],
        ));
        let key = |button| match button {
            DpadUp => KeyCode::KeyW,
            DpadDown => KeyCode::KeyS,
            DpadLeft => KeyCode::KeyA,
            _ => KeyCode::KeyD,
        };
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let cases: [(&[PadButton], (f32, f32)); 9] = [
            (&[DpadUp], (0.0, 1.0)),
            (&[DpadDown], (0.0, -1.0)),
            (&[DpadLeft], (-1.0, 0.0)),
            (&[DpadRight], (1.0, 0.0)),
            (&[DpadUp, DpadRight], (half, half)),
            (&[DpadUp, DpadLeft], (-half, half)),
            (&[DpadDown, DpadRight], (half, -half)),
            (&[DpadDown, DpadLeft], (-half, -half)),
            (&[DpadUp, DpadDown], (0.0, 0.0)),
        ];
        for (buttons, expected) in cases {
            map.gamepad_event(&state(PAD, holding(buttons)));
            for &button in buttons {
                map.key_event(key(button), true);
            }
            assert!(
                close(map.axis2("move"), expected),
                "{buttons:?}: {:?}",
                map.axis2("move"),
            );
            assert_eq!(map.axis2("move"), map.axis2("walk"), "{buttons:?}");
            for &button in buttons {
                map.key_event(key(button), false);
            }
        }
        map.gamepad_event(&state(PAD, holding(&[])));
        assert_eq!(map.axis2("move"), (0.0, 0.0), "let go");
    }

    /// On a button the d-pad is down while any of its four is, as `Wasd` is
    /// while any of its keys is; a face button is not one of them.
    #[test]
    fn the_dpad_on_a_button_is_down_while_any_direction_is() {
        let mut map = ActionMap::new();
        map.declare(action("any", ActionKind::Button, vec![Binding::PadDpad]));
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(!map.button_held("any"));
        for button in PadButton::DPAD {
            map.gamepad_event(&state(PAD, holding(&[button])));
            assert!(map.button_held("any"), "{button:?}");
            map.gamepad_event(&state(PAD, holding(&[])));
            assert!(!map.button_held("any"));
        }
    }

    /// **Keys, the d-pad and the stick share one unit disc**: they sum, then
    /// clamp, so pushing two of them the same way is not faster and pushing
    /// them apart is a diagonal or a cancel.
    #[test]
    fn keys_the_dpad_and_the_stick_sum_into_one_unit_disc() {
        let mut map = ActionMap::new();
        map.declare(action(
            "move",
            ActionKind::Axis2,
            vec![
                Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                },
                Binding::PadDpad,
                Binding::PadStick {
                    stick: Stick::Left,
                    deadzone: 0.0,
                },
            ],
        ));
        let dpad = |button| holding(&[button]);
        let half = std::f32::consts::FRAC_1_SQRT_2;

        // The same way: clamped, not doubled.
        map.gamepad_event(&state(
            PAD,
            with_axis(dpad(PadButton::DpadRight), PadAxis::LeftX, 0.5),
        ));
        assert!(
            close(map.axis2("move"), (1.0, 0.0)),
            "{:?}",
            map.axis2("move")
        );

        // Crossed: a unit diagonal.
        map.gamepad_event(&state(
            PAD,
            with_axis(dpad(PadButton::DpadRight), PadAxis::LeftY, 1.0),
        ));
        assert!(
            close(map.axis2("move"), (half, half)),
            "{:?}",
            map.axis2("move")
        );

        // Opposed, inside the disc: the difference.
        map.gamepad_event(&state(
            PAD,
            with_axis(dpad(PadButton::DpadLeft), PadAxis::LeftX, 0.5),
        ));
        assert!(
            close(map.axis2("move"), (-0.5, 0.0)),
            "{:?}",
            map.axis2("move")
        );

        // A key against the d-pad cancels it; the three together up are up.
        map.gamepad_event(&state(PAD, dpad(PadButton::DpadRight)));
        map.key_event(KeyCode::KeyA, true);
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        map.key_event(KeyCode::KeyA, false);
        map.key_event(KeyCode::KeyW, true);
        map.gamepad_event(&state(
            PAD,
            with_axis(dpad(PadButton::DpadUp), PadAxis::LeftY, 1.0),
        ));
        assert!(
            close(map.axis2("move"), (0.0, 1.0)),
            "{:?}",
            map.axis2("move")
        );
    }

    /// A d-pad press makes the pad the last device, as any pad button does,
    /// and letting go of it does not.
    #[test]
    fn a_dpad_press_makes_the_pad_the_last_device() {
        let mut map = ActionMap::new();
        map.declare(action("move", ActionKind::Axis2, vec![Binding::PadDpad]));
        map.key_event(KeyCode::KeyQ, true);
        assert_eq!(map.last_device(), Some(Device::Keyboard));
        map.gamepad_event(&state(PAD, holding(&[PadButton::DpadLeft])));
        assert_eq!(map.last_device(), Some(Device::Gamepad));

        map.key_event(KeyCode::KeyQ, false);
        map.key_event(KeyCode::KeyQ, true);
        map.gamepad_event(&state(PAD, holding(&[])));
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "letting go of the d-pad is not the pad speaking",
        );
    }

    /// A d-pad composite in a pushed context takes the four d-pad buttons from
    /// a plain [`Binding::PadButton`] beneath it, and leaves the rest.
    #[test]
    fn a_pushed_dpad_takes_its_four_buttons_from_the_context_beneath() {
        let mut map = jump_map();
        map.declare(action(
            "up",
            ActionKind::Button,
            vec![Binding::PadButton(PadButton::DpadUp)],
        ));
        map.declare_in(
            "menu",
            action("move", ActionKind::Axis2, vec![Binding::PadDpad]),
        );
        map.push_context("menu").expect("declared");
        map.gamepad_event(&state(PAD, holding(&[PadButton::DpadUp, PadButton::South])));
        assert_eq!(map.axis2("move"), (0.0, 1.0));
        assert!(!map.button_held("up"), "the menu owns DpadUp");
        assert!(map.button_held("jump"), "and not South");
    }

    /// **The pad becomes the last device on activity and not on release**, as
    /// `device.rs` states for the others — and resting drift is not activity.
    #[test]
    fn the_last_device_becomes_the_pad_on_activity_and_not_on_release() {
        let mut map = ActionMap::new();
        map.gamepad_event(&GamepadEvent::Connected {
            id: PAD,
            kind: PadKind::Xbox,
        });
        assert_eq!(map.last_device(), None, "plugging in is not speaking");

        map.gamepad_event(&state(PAD, holding(&[PadButton::North])));
        assert_eq!(
            map.last_device(),
            Some(Device::Gamepad),
            "nothing binds North"
        );

        map.key_event(KeyCode::KeyQ, true);
        map.gamepad_event(&state(PAD, holding(&[])));
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "letting go of a pad button is not the pad speaking",
        );

        let rest = GamepadSnapshot::neutral(PadKind::Xbox);
        let drift = with_axis(rest, PadAxis::RightX, 0.3);
        map.gamepad_event(&state(PAD, drift));
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "drift is not a push"
        );

        let over = with_axis(rest, PadAxis::RightX, 0.9);
        map.gamepad_event(&state(PAD, over));
        assert_eq!(map.last_device(), Some(Device::Gamepad), "a push is");

        map.key_event(KeyCode::KeyQ, true);
        map.gamepad_event(&state(PAD, with_axis(rest, PadAxis::RightX, 0.95)));
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "a stick held over already spoke",
        );
        map.gamepad_event(&state(PAD, rest));
        assert_eq!(
            map.last_device(),
            Some(Device::Keyboard),
            "nor does centring"
        );

        map.gamepad_event(&state(PAD, with_axis(rest, PadAxis::LeftTrigger, 0.8)));
        assert_eq!(
            map.last_device(),
            Some(Device::Gamepad),
            "a trigger pull is"
        );
    }

    /// **Focus loss releases a held pad button, and it stays released until
    /// the player lets go** — the pad still holding it when snapshots resume is
    /// not a new press.
    #[test]
    fn focus_loss_releases_a_held_pad_button_until_it_is_let_go() {
        let mut map = jump_map();
        map.begin_tick(TICK);
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(map.button_held("jump"));

        map.begin_tick(TICK);
        map.release_gamepads();
        assert!(map.just_released("jump"));
        assert!(!map.button_held("jump"));
        assert_eq!(
            map.gamepads().collect::<Vec<_>>(),
            [(PAD, &GamepadSnapshot::neutral(PadKind::Xbox))],
            "still connected, at rest",
        );

        map.begin_tick(TICK);
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(!map.button_held("jump"), "withheld until released");

        map.gamepad_event(&state(PAD, holding(&[])));
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(map.just_pressed("jump"), "a fresh press after the lift");
    }

    /// Sticks are levels: focus loss centres them, and the next snapshot puts
    /// them back without waiting for a lift.
    #[test]
    fn focus_loss_centres_a_stick_until_the_next_snapshot() {
        let mut map = ActionMap::new();
        map.declare(action(
            "move",
            ActionKind::Axis2,
            vec![Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.0,
            }],
        ));
        let up = with_axis(GamepadSnapshot::neutral(PadKind::Xbox), PadAxis::LeftY, 1.0);
        map.gamepad_event(&state(PAD, up));
        map.release_gamepads();
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        map.gamepad_event(&state(PAD, up));
        assert_eq!(map.axis2("move"), (0.0, 1.0));
    }

    /// **A disconnected pad's held state is released** — its button and its
    /// stick — and it is gone from the map.
    #[test]
    fn a_disconnected_pad_releases_what_it_held() {
        let mut map = jump_map();
        map.declare(action(
            "move",
            ActionKind::Axis2,
            vec![Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.1,
            }],
        ));
        map.begin_tick(TICK);
        let held = with_axis(holding(&[PadButton::South]), PadAxis::LeftX, 1.0);
        map.gamepad_event(&state(PAD, held));
        assert!(map.button_held("jump"));
        assert_eq!(map.axis2("move"), (1.0, 0.0));

        map.begin_tick(TICK);
        map.gamepad_event(&GamepadEvent::Disconnected { id: PAD });
        assert!(map.just_released("jump"));
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        assert_eq!(map.gamepads().count(), 0);

        map.begin_tick(TICK);
        assert!(
            !map.button_held("jump"),
            "and not re-pressed by the next tick"
        );
    }

    /// A non-finite snapshot is dropped and the pad keeps the one before it.
    #[test]
    fn a_non_finite_snapshot_is_dropped() {
        let mut map = jump_map();
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        let poisoned = with_axis(holding(&[]), PadAxis::LeftX, f32::NAN);
        map.gamepad_event(&state(PAD, poisoned));
        assert!(map.button_held("jump"));
    }

    /// A pad button is consumed by the topmost context binding it, and one
    /// held across a push is withheld from the new owner, like a key.
    #[test]
    fn a_pad_button_is_routed_and_withheld_like_a_key() {
        let mut map = jump_map();
        map.declare_in(
            "menu",
            action(
                "accept",
                ActionKind::Button,
                vec![Binding::PadButton(PadButton::South)],
            ),
        );
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        map.push_context("menu").expect("declared");
        assert!(map.just_released("jump"), "the menu took South");
        assert!(!map.button_held("accept"), "withheld until released");

        map.gamepad_event(&state(PAD, holding(&[])));
        map.gamepad_event(&state(PAD, holding(&[PadButton::South])));
        assert!(map.just_pressed("accept"));
        assert!(!map.button_held("jump"));
    }

    /// A dead zone or threshold nothing could get past, or a `NaN` one, is
    /// refused when it is declared or rebound rather than silently inert.
    #[test]
    fn an_unusable_deadzone_is_refused() {
        let mut map = ActionMap::new();
        for deadzone in [1.0, -0.1, f32::NAN] {
            assert_eq!(
                map.try_declare(action(
                    "move",
                    ActionKind::Axis2,
                    vec![Binding::PadStick {
                        stick: Stick::Left,
                        deadzone,
                    }],
                )),
                Err(ActionMapError::InvalidDeadzone("move".to_owned())),
            );
        }
        map.declare(action("fire", ActionKind::Button, Vec::new()));
        assert_eq!(
            map.rebind(
                "fire",
                vec![Binding::PadTrigger {
                    trigger: Trigger::Left,
                    threshold: f32::INFINITY,
                }],
            ),
            Err(ActionMapError::InvalidDeadzone("fire".to_owned())),
        );
    }

    /// Ids from the allocator are never repeated.
    #[test]
    fn allocated_ids_are_distinct() {
        let a = GamepadId::allocate();
        let b = GamepadId::allocate();
        assert_ne!(a, b);
    }
}
