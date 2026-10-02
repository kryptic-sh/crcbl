//! How an action's binding is shown to the player right now:
//! [`ActionMap::hint`] answers with a [`Hint`] — `Space` while the player is
//! on the keyboard, `A` once they pick up an Xbox pad, `Cross` on a
//! PlayStation one.
//!
//! # Which binding
//!
//! **The binding for the device the player last used.** [`ActionMap::hint`]
//! takes the first of the action's bindings whose [`Binding::device`] is
//! [`ActionMap::last_device`]. An action with nothing on that device falls
//! back to the device used before it, and so on back through every device
//! that has spoken — a player who nudged the mouse while playing on the pad is
//! still shown the pad's button for an action the mouse cannot press — and
//! then, before any device has spoken or when none it heard from is bound, to
//! the action's first binding.
//!
//! # Labels, not artwork
//!
//! A hint's [`label`](Hint::label) is text. The engine ships **no glyph
//! images**: what a button looks like on screen is a game's art direction,
//! and the families' own button art is theirs to license, so that is the
//! game's call and not one an engine can make for every game built on it. A
//! game with an icon atlas maps the rest of the hint — the
//! [`binding`](Hint::binding) and the [`pad`](Hint::pad) family — to its own
//! sprites.
//!
//! The default labels are plain ASCII words (`A`, `Cross`, `LB`, `Space`),
//! not symbols such as `Ⓐ`, because a font that lacks a symbol draws nothing
//! or a box where the hint should be. A game whose font has them supplies its
//! own [`HintLabels`] to [`ActionMap::hint_with`], overriding only the entries
//! it prints differently.
//!
//! # Which pad family
//!
//! A pad button is named for the pad that last spoke
//! ([`ActionMap::last_pad_kind`]); before any pad spoke, for the first
//! connected one; with none connected, as [`PadKind::Generic`]'s positional
//! names. Bindings never read the family — buttons are positional
//! (`gamepad.rs`) — so this is the one place it matters.

mod labels;

pub use labels::{DefaultLabels, HintLabels};

use crate::{ActionMap, Binding, Device, PadKind};

/// How one action's binding is shown to the player — see the
/// [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    /// The kind of device the binding listens to — [`Binding::device`] of
    /// [`Hint::binding`].
    pub device: Device,
    /// The binding shown.
    pub binding: Binding,
    /// The pad family the label names the binding for, when the binding is a
    /// pad's; `None` for every other device.
    pub pad: Option<PadKind>,
    /// What to print: the binding as the player's device names it.
    pub label: String,
}

impl ActionMap {
    /// How the named action's binding is shown to the player now, in the
    /// engine's own [`DefaultLabels`] — see the [module docs](self) for which
    /// binding is picked. `None` if no such action is declared or it has no
    /// bindings.
    ///
    /// ```
    /// use crcbl_core::input::KeyCode;
    /// use crcbl_input::{ActionMap, GamepadEvent, GamepadId, GamepadSnapshot, PadButton, PadKind};
    ///
    /// let mut map = ActionMap::from_ron(
    ///     r#"[(action: "jump", kind: Button, keyboard: ["Space"], gamepad: ["Pad:South"])]"#,
    /// )?;
    /// map.key_event(KeyCode::KeyQ, true);
    /// assert_eq!(map.hint("jump").map(|hint| hint.label).as_deref(), Some("Space"));
    ///
    /// let mut snapshot = GamepadSnapshot::neutral(PadKind::PlayStation);
    /// snapshot.buttons.insert(PadButton::South);
    /// map.gamepad_event(&GamepadEvent::State { id: GamepadId(1), snapshot });
    /// assert_eq!(map.hint("jump").map(|hint| hint.label).as_deref(), Some("Cross"));
    /// # Ok::<(), crcbl_input::BindingAssetError>(())
    /// ```
    #[must_use]
    pub fn hint(&self, action: &str) -> Option<Hint> {
        self.hint_with(action, &DefaultLabels)
    }

    /// [`ActionMap::hint`], with every label taken from `labels` — a game's
    /// own table, which overrides the engine's entry by entry.
    #[must_use]
    pub fn hint_with(&self, action: &str, labels: &(impl HintLabels + ?Sized)) -> Option<Hint> {
        let bindings = self.bindings(action)?;
        let binding = self
            .devices
            .iter()
            .find_map(|&device| bindings.iter().find(|binding| binding.device() == device))
            .or_else(|| bindings.first())?;
        let device = binding.device();
        let pad = (device == Device::Gamepad).then(|| self.hint_pad_kind());
        Some(Hint {
            device,
            binding: binding.clone(),
            pad,
            label: labels.label(binding, pad.unwrap_or_default()),
        })
    }

    /// The family a pad hint is named for — see the [module docs](self).
    fn hint_pad_kind(&self) -> PadKind {
        self.last_pad_kind
            .or_else(|| self.pads.values().next().map(|pad| pad.kind))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
