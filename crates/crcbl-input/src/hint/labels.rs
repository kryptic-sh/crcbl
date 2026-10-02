//! The words a [`Hint`](super::Hint) prints: [`HintLabels`], whose provided
//! methods are the engine's tables, and [`DefaultLabels`], which uses them
//! unchanged.

use std::borrow::Cow;

use crcbl_core::input::{KeyCode, PointerButton};

use crate::{Binding, PadButton, PadKind, PointerAxis, Stick, Trigger};

/// What a hint calls each input — a label table a game can override entry by
/// entry.
///
/// **Every method has the engine's answer as its default**, so a game
/// implements this on its own type and overrides only what it would print
/// differently: a pad family's buttons as the symbols its font carries, the
/// keys in the player's language. [`HintLabels::label`] composes the entries
/// into a whole binding's label — `Shift+Tab`, `WASD`, `LB+A` — and is
/// itself overridable for a game that spells composites its own way.
///
/// Keys are named by **position**, after the US-QWERTY legend, because that is
/// all a [`KeyCode`] knows: on AZERTY, [`KeyCode::KeyW`] prints `W` over the
/// key labelled Z. The layout's own legend is the shell's to know, and a game
/// that has it overrides [`HintLabels::key`].
///
/// ```
/// use std::borrow::Cow;
/// use crcbl_input::{HintLabels, PadButton, PadKind};
///
/// /// A game whose font has the circled letters.
/// struct Circled;
///
/// impl HintLabels for Circled {
///     fn pad_button(&self, kind: PadKind, button: PadButton) -> Cow<'static, str> {
///         match (kind, button) {
///             (PadKind::Xbox, PadButton::South) => "Ⓐ".into(),
///             _ => crcbl_input::DefaultLabels.pad_button(kind, button),
///         }
///     }
/// }
///
/// assert_eq!(Circled.pad_button(PadKind::Xbox, PadButton::South), "Ⓐ");
/// assert_eq!(Circled.pad_button(PadKind::Xbox, PadButton::East), "B");
/// ```
pub trait HintLabels {
    /// A key's label — `Space`, `W`, `Shift`, `Up`.
    fn key(&self, key: KeyCode) -> Cow<'static, str> {
        key_label(key).into()
    }

    /// A mouse button's label — `LMB`, `RMB`, `MMB`, `Mouse 4`.
    fn mouse_button(&self, button: PointerButton) -> Cow<'static, str> {
        match button {
            PointerButton::Left => "LMB".into(),
            PointerButton::Right => "RMB".into(),
            PointerButton::Middle => "MMB".into(),
            PointerButton::Back => "Mouse 4".into(),
            PointerButton::Forward => "Mouse 5".into(),
            PointerButton::Other(index) => format!("Mouse {index}").into(),
        }
    }

    /// [`Binding::MouseMotion`]'s label.
    fn mouse_motion(&self) -> Cow<'static, str> {
        "Mouse".into()
    }

    /// The wheel's label, for [`Binding::MouseScroll`] and after a
    /// [`Binding::ScrollChord`]'s key.
    fn wheel(&self) -> Cow<'static, str> {
        "Wheel".into()
    }

    /// [`Binding::PointerPosition`]'s label, whichever axis it reads.
    fn pointer_position(&self, axis: PointerAxis) -> Cow<'static, str> {
        let _ = axis;
        "Pointer".into()
    }

    /// An on-screen control's label: by default its id, verbatim, since the
    /// id is the only name the engine has for it.
    fn control(&self, id: &str) -> Cow<'static, str> {
        id.to_owned().into()
    }

    /// A pad button's label on a pad of `kind`: what that family prints on
    /// the button in that position.
    ///
    /// | Button          | Xbox | PlayStation | Switch | Steam Deck | Generic            |
    /// | --------------- | ---- | ----------- | ------ | ---------- | ------------------ |
    /// | `South`         | A    | Cross       | B      | A          | South              |
    /// | `East`          | B    | Circle      | A      | B          | East               |
    /// | `West`          | X    | Square      | Y      | X          | West               |
    /// | `North`         | Y    | Triangle    | X      | Y          | North              |
    /// | `LeftShoulder`  | LB   | L1          | L      | L1         | Left bumper        |
    /// | `RightShoulder` | RB   | R1          | R      | R1         | Right bumper       |
    /// | `LeftStick`     | LS   | L3          | LS     | L3         | Left stick button  |
    /// | `RightStick`    | RS   | R3          | RS     | R3         | Right stick button |
    /// | `Start`         | Menu | Options     | +      | Menu       | Start              |
    /// | `Select`        | View | Share       | -      | View       | Select             |
    /// | `Guide`         | Xbox | PS          | Home   | Steam      | Guide              |
    ///
    /// The d-pad is `D-pad up`, `D-pad down`, `D-pad left` and `D-pad right`
    /// on every family. The face buttons are where the families disagree, and
    /// why bindings are positional: a Switch's `B` is where an Xbox pad's `A`
    /// is. `Select` is `Share` on a DualShock 4 and `Create` on a DualSense,
    /// which a [`PadKind`] does not tell apart, so the older word is printed;
    /// the Switch's `−` is printed as an ASCII hyphen, which every font has.
    fn pad_button(&self, kind: PadKind, button: PadButton) -> Cow<'static, str> {
        pad_button_label(kind, button).into()
    }

    /// [`Binding::PadDpad`]'s label on a pad of `kind`.
    fn dpad(&self, kind: PadKind) -> Cow<'static, str> {
        let _ = kind;
        "D-pad".into()
    }

    /// A stick's label on a pad of `kind`. Every family prints the same
    /// words; `kind` is there for a game whose art differs by family.
    fn stick(&self, kind: PadKind, stick: Stick) -> Cow<'static, str> {
        let _ = kind;
        match stick {
            Stick::Left => "Left stick".into(),
            Stick::Right => "Right stick".into(),
        }
    }

    /// A trigger's label on a pad of `kind`: `LT`/`RT` on Xbox, `L2`/`R2` on
    /// PlayStation and the Steam Deck, `ZL`/`ZR` on Switch, and
    /// `Left trigger`/`Right trigger` on a pad the backend could not place.
    fn trigger(&self, kind: PadKind, trigger: Trigger) -> Cow<'static, str> {
        trigger_label(kind, trigger).into()
    }

    /// A whole binding's label, composed from the entries above: a chord as
    /// its two halves joined by `+`, a [`Binding::KeyAxis`] as its two keys
    /// joined by `/`, and a [`Binding::Wasd`] as its four keys in up, left,
    /// down, right order — run together when each is one character (`WASD`),
    /// joined by `/` when not (`Up/Left/Down/Right`). A pad binding is named
    /// for a pad of `kind`; nothing else reads it.
    fn label(&self, binding: &Binding, kind: PadKind) -> String {
        match binding {
            Binding::Key(key) => self.key(*key).into_owned(),
            Binding::Chord { modifier, key } => {
                format!("{}+{}", self.key(modifier.keys()[0]), self.key(*key))
            }
            Binding::KeyAxis { negative, positive } => {
                format!("{}/{}", self.key(*negative), self.key(*positive))
            }
            Binding::Wasd {
                up,
                down,
                left,
                right,
            } => {
                let keys = [*up, *left, *down, *right].map(|key| self.key(key));
                let separator = if keys.iter().all(|key| key.chars().count() == 1) {
                    ""
                } else {
                    "/"
                };
                keys.join(separator)
            }
            Binding::ScrollChord { held } => format!("{}+{}", self.key(*held), self.wheel()),
            Binding::MouseButton(button) => self.mouse_button(*button).into_owned(),
            Binding::ButtonChord { modifier, button } => format!(
                "{}+{}",
                self.key(modifier.keys()[0]),
                self.mouse_button(*button)
            ),
            Binding::MouseMotion => self.mouse_motion().into_owned(),
            Binding::MouseScroll => self.wheel().into_owned(),
            Binding::PointerPosition { axis } => self.pointer_position(*axis).into_owned(),
            Binding::Virtual(id) => self.control(id).into_owned(),
            Binding::PadButton(button) => self.pad_button(kind, *button).into_owned(),
            Binding::PadChord { modifier, button } => format!(
                "{}+{}",
                self.pad_button(kind, *modifier),
                self.pad_button(kind, *button)
            ),
            Binding::PadDpad => self.dpad(kind).into_owned(),
            Binding::PadStick { stick, .. } => self.stick(kind, *stick).into_owned(),
            Binding::PadTrigger { trigger, .. } => self.trigger(kind, *trigger).into_owned(),
        }
    }
}

/// The engine's labels, every entry of [`HintLabels`] left as it is — what
/// [`ActionMap::hint`](crate::ActionMap::hint) uses.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultLabels;

impl HintLabels for DefaultLabels {}

/// A key's label: its US-QWERTY legend where it has a short one, and its
/// stable name ([`KeyCode::as_str`]) where that already reads as one — `Tab`,
/// `Enter`, `F1`, `PageUp`.
fn key_label(key: KeyCode) -> &'static str {
    match key {
        KeyCode::Backquote => "`",
        KeyCode::Minus => "-",
        KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        KeyCode::Backslash => "\\",
        KeyCode::Semicolon => ";",
        KeyCode::Quote => "'",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Slash => "/",
        KeyCode::Escape => "Esc",
        KeyCode::ShiftLeft | KeyCode::ShiftRight => "Shift",
        KeyCode::ControlLeft | KeyCode::ControlRight => "Ctrl",
        KeyCode::AltLeft | KeyCode::AltRight => "Alt",
        KeyCode::SuperLeft | KeyCode::SuperRight => "Super",
        KeyCode::ContextMenu => "Menu",
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::Numpad0 => "Num 0",
        KeyCode::Numpad1 => "Num 1",
        KeyCode::Numpad2 => "Num 2",
        KeyCode::Numpad3 => "Num 3",
        KeyCode::Numpad4 => "Num 4",
        KeyCode::Numpad5 => "Num 5",
        KeyCode::Numpad6 => "Num 6",
        KeyCode::Numpad7 => "Num 7",
        KeyCode::Numpad8 => "Num 8",
        KeyCode::Numpad9 => "Num 9",
        KeyCode::NumpadDivide => "Num /",
        KeyCode::NumpadMultiply => "Num *",
        KeyCode::NumpadSubtract => "Num -",
        KeyCode::NumpadAdd => "Num +",
        KeyCode::NumpadEnter => "Num Enter",
        KeyCode::NumpadDecimal => "Num .",
        // `KeyA` → `A`, `Digit1` → `1`: the stable names put the legend after
        // a prefix, and only the letters and the digit row carry these two.
        other => {
            let name = other.as_str();
            name.strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .unwrap_or(name)
        }
    }
}

/// What is printed on `button` of a pad of `kind` — the table
/// [`HintLabels::pad_button`] documents.
const fn pad_button_label(kind: PadKind, button: PadButton) -> &'static str {
    use PadKind::{Generic, PlayStation, SteamDeck, Switch, Xbox};
    match button {
        PadButton::South => match kind {
            Xbox | SteamDeck => "A",
            PlayStation => "Cross",
            Switch => "B",
            Generic => "South",
        },
        PadButton::East => match kind {
            Xbox | SteamDeck => "B",
            PlayStation => "Circle",
            Switch => "A",
            Generic => "East",
        },
        PadButton::West => match kind {
            Xbox | SteamDeck => "X",
            PlayStation => "Square",
            Switch => "Y",
            Generic => "West",
        },
        PadButton::North => match kind {
            Xbox | SteamDeck => "Y",
            PlayStation => "Triangle",
            Switch => "X",
            Generic => "North",
        },
        PadButton::LeftShoulder => match kind {
            Xbox => "LB",
            PlayStation | SteamDeck => "L1",
            Switch => "L",
            Generic => "Left bumper",
        },
        PadButton::RightShoulder => match kind {
            Xbox => "RB",
            PlayStation | SteamDeck => "R1",
            Switch => "R",
            Generic => "Right bumper",
        },
        PadButton::LeftStick => match kind {
            Xbox | Switch => "LS",
            PlayStation | SteamDeck => "L3",
            Generic => "Left stick button",
        },
        PadButton::RightStick => match kind {
            Xbox | Switch => "RS",
            PlayStation | SteamDeck => "R3",
            Generic => "Right stick button",
        },
        PadButton::Start => match kind {
            Xbox | SteamDeck => "Menu",
            PlayStation => "Options",
            Switch => "+",
            Generic => "Start",
        },
        PadButton::Select => match kind {
            Xbox | SteamDeck => "View",
            PlayStation => "Share",
            Switch => "-",
            Generic => "Select",
        },
        PadButton::Guide => match kind {
            Xbox => "Xbox",
            PlayStation => "PS",
            Switch => "Home",
            SteamDeck => "Steam",
            Generic => "Guide",
        },
        PadButton::DpadUp => "D-pad up",
        PadButton::DpadDown => "D-pad down",
        PadButton::DpadLeft => "D-pad left",
        PadButton::DpadRight => "D-pad right",
    }
}

/// What is printed on `trigger` of a pad of `kind` — the list
/// [`HintLabels::trigger`] documents.
const fn trigger_label(kind: PadKind, trigger: Trigger) -> &'static str {
    match (kind, trigger) {
        (PadKind::Xbox, Trigger::Left) => "LT",
        (PadKind::Xbox, Trigger::Right) => "RT",
        (PadKind::PlayStation | PadKind::SteamDeck, Trigger::Left) => "L2",
        (PadKind::PlayStation | PadKind::SteamDeck, Trigger::Right) => "R2",
        (PadKind::Switch, Trigger::Left) => "ZL",
        (PadKind::Switch, Trigger::Right) => "ZR",
        (PadKind::Generic, Trigger::Left) => "Left trigger",
        (PadKind::Generic, Trigger::Right) => "Right trigger",
    }
}
