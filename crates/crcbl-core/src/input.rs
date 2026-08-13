//! Input vocabulary: the names every layer of the engine uses for keys,
//! buttons, axes and the devices they came from.
//!
//! # Why this is in `crcbl-core` and not in `crcbl-shell`
//!
//! `docs/plan/01-foundations.md` §1.4 puts "input event normalization into
//! engine types" in `crcbl-core::input`, and `docs/plan/19-input.md` explains
//! why: the action layer (`crcbl-input`, P2), the profile store (topic 14, where
//! user rebinds are serialized), the UI (glyph hints), and the editor all have
//! to *name* a key. If [`KeyCode`] lived in the windowing crate, the save-game
//! crate would depend on a windowing library to spell `Space`, and a headless
//! `crcbl sim` replay would link a shell it never opens.
//!
//! The split is therefore:
//!
//! ```text
//! crcbl-core::input   vocabulary — KeyCode, Keysym, PointerButton, Modifiers,
//!                                  ButtonState, ScrollDelta, DeviceId,
//!                                  ContactId, TouchPhase
//! crcbl-shell         events     — ShellEvent, which names windows and is only
//!                                  ever produced by a shell backend
//! ```
//!
//! Nothing here knows what a window is, and nothing here allocates.
//!
//! # Physical versus logical: three coordinates for one keystroke
//!
//! A key press carries three independent facts, and conflating any two of them
//! is the classic input bug:
//!
//! | Fact | Type | Depends on layout? | Used for |
//! | --- | --- | --- | --- |
//! | Which key was pressed | [`KeyCode`] | no | gameplay bindings ("W moves forward" stays under the W key on AZERTY) |
//! | What the OS calls that key | [`Scancode`] | no | round-tripping keys we have no name for |
//! | What symbol it produces | [`Keysym`] | **yes** | rebind menus, shortcut display, layout-sensitive shortcuts |
//!
//! And a fourth, which is not a keystroke at all: **text**. Composed text
//! arrives as a separate commit string (`ShellEvent::TextCommit`) because IME
//! input has no one-to-one relationship with key presses — one keystroke can
//! commit five characters, five keystrokes can commit one, and a dead key
//! commits nothing.
//!
//! # `Keysym` is the X11/XKB namespace
//!
//! [`Keysym`] holds an X11 keysym value, which is what `xkbcommon` produces.
//! This is a decision, not an accident: two of the five planned shell backends
//! (Wayland and X11 — the only two that exist at P0) produce these natively,
//! and the other three need a translation table whatever namespace we pick.
//! Inventing an engine-specific symbol enum would mean *three* tables instead of
//! two plus a well-documented, stable, externally-specified numbering that
//! already covers every keyboard symbol anyone has ever shipped.

use core::fmt;

bitflags::bitflags! {
    /// Modifier keys held when an input event was produced.
    ///
    /// Latched state (Caps Lock, Num Lock) is included alongside held state
    /// because a rebind UI and a shortcut matcher both need it, and asking the
    /// shell for it later — after the event has been queued — would race with
    /// the user letting go.
    ///
    /// Left and right modifiers are *not* distinguished here. The distinction
    /// survives in [`KeyCode`] ([`ShiftLeft`](KeyCode::ShiftLeft) versus
    /// [`ShiftRight`](KeyCode::ShiftRight)), which is where a game that wants to
    /// bind them separately should look; a shortcut matcher never wants it, and
    /// duplicating the axis here would mean every `contains(SHIFT)` check had to
    /// remember to test two bits.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Modifiers: u32 {
        /// Either Shift key is held.
        const SHIFT = 1 << 0;
        /// Either Control key is held.
        const CTRL = 1 << 1;
        /// Either Alt key is held (Option on macOS).
        const ALT = 1 << 2;
        /// Either Super key is held — Windows key, Command on macOS, Meta on
        /// X11.
        const SUPER = 1 << 3;
        /// Caps Lock is latched on.
        const CAPS_LOCK = 1 << 4;
        /// Num Lock is latched on.
        const NUM_LOCK = 1 << 5;
        /// AltGr / ISO Level-3 Shift is held.
        ///
        /// Separate from [`ALT`](Self::ALT) because on many European layouts
        /// AltGr produces characters, and a shortcut bound to `Alt+E` must not
        /// fire when the user types `€`.
        const ALT_GR = 1 << 6;
    }
}

impl Modifiers {
    /// The modifiers a shortcut matcher cares about: Shift, Ctrl, Alt, Super.
    ///
    /// Masks off latched state, which is never part of a chord.
    pub const CHORD: Self = Self::SHIFT
        .union(Self::CTRL)
        .union(Self::ALT)
        .union(Self::SUPER);

    /// Just the chord bits of `self`.
    #[inline]
    #[must_use]
    pub const fn chord(self) -> Self {
        self.intersection(Self::CHORD)
    }
}

/// Which physical device an event came from.
///
/// `docs/plan/19-input.md` requires "per-device id" on raw events from day one,
/// because local-multiplayer device-to-player assignment is post-MVP but is
/// impossible to retrofit into an event stream that never said which keyboard
/// pressed the key. Ids are unique for the lifetime of the process and are not
/// reused when a device is unplugged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u32);

impl DeviceId {
    /// The device that events with no attributable source carry.
    ///
    /// Backends that cannot tell two keyboards apart (X11 core protocol without
    /// XInput2, and the browser) use this for everything, and so do synthesized
    /// events from a replay or a test script.
    pub const UNKNOWN: Self = Self(0);
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::UNKNOWN {
            f.write_str("device ?")
        } else {
            write!(f, "device {}", self.0)
        }
    }
}

/// Whether a two-state input element went down or came up.
///
/// One type for keyboard keys, pointer buttons and gamepad buttons: they are
/// the same fact, and a separate `KeyState`/`MouseState` pair only creates
/// conversion noise at the action layer, which treats all three identically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonState {
    /// The element went down.
    Pressed,
    /// The element came up.
    Released,
}

impl ButtonState {
    /// Whether this is [`ButtonState::Pressed`].
    #[inline]
    #[must_use]
    pub const fn is_pressed(self) -> bool {
        matches!(self, Self::Pressed)
    }

    /// [`Pressed`](Self::Pressed) for `true`, [`Released`](Self::Released) for
    /// `false`.
    #[inline]
    #[must_use]
    pub const fn from_pressed(pressed: bool) -> Self {
        if pressed {
            Self::Pressed
        } else {
            Self::Released
        }
    }
}

/// The raw, platform-assigned code for a physical key.
///
/// Opaque on purpose: its numbering is whatever the window system uses (Linux
/// evdev codes offset by 8 on both Wayland and X11, `MapVirtualKey` scan codes
/// on Win32, `kVK_*` on macOS). It exists so a key the engine has no
/// [`KeyCode`] name for can still be bound, logged and round-tripped through a
/// profile rather than being silently dropped.
///
/// Never compare a `Scancode` against a literal in engine code — that is
/// platform sniffing with extra steps. Compare [`KeyCode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scancode(pub u32);

impl fmt::Display for Scancode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scancode {}", self.0)
    }
}

/// A layout-mapped key symbol, in the X11/XKB keysym numbering.
///
/// See the [module docs](self) for why that numbering. The values a game
/// actually wants are usually reachable through [`Keysym::to_char`]; the raw
/// number matters only for named function keys, which live above
/// `0xFF00`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Keysym(pub u32);

impl Keysym {
    /// No symbol — a key that produces nothing in the current layout, or a
    /// backend that could not map one. `XKB_KEY_NoSymbol`.
    pub const NONE: Self = Self(0);

    /// Start of the "Unicode keysym" block: `0x0100_0000 | codepoint`.
    const UNICODE_BASE: u32 = 0x0100_0000;

    /// The character this symbol produces, if it produces one.
    ///
    /// Implements the two rules XKB uses: keysyms `0x20..=0x7E` and
    /// `0xA0..=0xFF` are Latin-1 directly, and everything else that maps to a
    /// character is `0x0100_0000 | codepoint`. Function keys, modifiers and
    /// dead keys return `None`.
    ///
    /// This deliberately does **not** cover the several hundred legacy
    /// non-Latin-1 keysyms that predate the Unicode block (Greek, Cyrillic and
    /// friends in `0x05xx`–`0x07xx`). Text input is delivered as committed
    /// strings, not reconstructed from keysyms, so the table would exist only
    /// to be wrong in a way nobody notices; a shortcut label falls back to the
    /// [`KeyCode`] name.
    #[must_use]
    pub const fn to_char(self) -> Option<char> {
        let codepoint = match self.0 {
            0x20..=0x7E | 0xA0..=0xFF => self.0,
            value if value >= Self::UNICODE_BASE => value - Self::UNICODE_BASE,
            _ => return None,
        };
        char::from_u32(codepoint)
    }

    /// The keysym for a character, in the canonical form XKB produces.
    #[must_use]
    pub const fn from_char(character: char) -> Self {
        let codepoint = character as u32;
        match codepoint {
            0x20..=0x7E | 0xA0..=0xFF => Self(codepoint),
            _ => Self(Self::UNICODE_BASE | codepoint),
        }
    }

    /// Whether this is [`Keysym::NONE`].
    #[inline]
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == Self::NONE.0
    }
}

impl fmt::Display for Keysym {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_char() {
            Some(character) if !character.is_control() => write!(f, "'{character}'"),
            _ => write!(f, "keysym {:#06x}", self.0),
        }
    }
}

/// A button on a pointing device.
///
/// Named by role, not by physical position: `Left` is the primary button even
/// on a left-handed mouse, because that is the swap the OS already applied and
/// the game must not apply again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointerButton {
    /// Primary button.
    Left,
    /// Secondary (context-menu) button.
    Right,
    /// Wheel click.
    Middle,
    /// Thumb button that means "back" — `BTN_SIDE`, `XBUTTON1`.
    Back,
    /// Thumb button that means "forward" — `BTN_EXTRA`, `XBUTTON2`.
    Forward,
    /// Anything else, by platform-assigned index.
    ///
    /// Bindable and loggable, never interpreted by the engine.
    Other(u16),
}

/// One finger on a touchscreen, for as long as it stays down.
///
/// **Unique among the contacts that are down at the same moment, stable from the
/// [`TouchPhase::Began`] that starts a gesture to the
/// [`Ended`](TouchPhase::Ended) or [`Cancelled`](TouchPhase::Cancelled) that
/// finishes it, and free to be reused after that.** Those three sentences are
/// the whole contract, and the third is the one that bites: a consumer keying
/// per-gesture state off this — which finger is holding the movement stick —
/// must drop that state when the contact ends, or the next finger to land
/// inherits it.
///
/// A contact is not a device. Two fingers on one screen are one [`DeviceId`] and
/// two `ContactId`s, which is exactly why the id could not be folded into the
/// device: a per-device registry cannot represent a second finger.
///
/// The numbering is the platform's (a browser `pointerId`, a `wl_touch` id, a
/// Win32 pointer id) and is meaningless to compare against a literal or across
/// windows — like [`Scancode`], it exists to be matched against *other* ids from
/// the same stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContactId(pub u32);

impl fmt::Display for ContactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contact {}", self.0)
    }
}

/// Where a [`ContactId`] is in its gesture.
///
/// A touchscreen has no "hover": a contact exists from the moment it lands until
/// the moment it goes away, so this is the lifecycle rather than a button state
/// — which is why touch does not reuse [`ButtonState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TouchPhase {
    /// The finger landed. The first event any [`ContactId`] gets.
    Began,
    /// It moved while down.
    Moved,
    /// The user lifted it. **The gesture completed**, and a consumer commits
    /// whatever it meant — a tap fires, a stick centres, a drag lands where it
    /// was let go.
    Ended,
    /// The system took the gesture away: the OS claimed it for an edge swipe,
    /// palm rejection changed its mind, the window lost focus, the browser
    /// decided the page was scrolling after all.
    ///
    /// **Not an [`Ended`](Self::Ended), and a consumer that treats it as one
    /// fires actions the user did not ask for.** The position it carries is the
    /// last one the platform knew, not a place anyone chose, so the answer is to
    /// undo the interaction rather than complete it — the drag goes back, the
    /// tap does not fire, the stick centres without the character having lunged.
    /// What both share is that the contact is over: the id is gone either way,
    /// and a consumer that ignores this one is left holding a finger that will
    /// never move again.
    Cancelled,
}

impl TouchPhase {
    /// Whether this phase ends the contact — [`Ended`](Self::Ended) or
    /// [`Cancelled`](Self::Cancelled).
    ///
    /// The test anything tracking live contacts writes; it is the *reason* they
    /// differ that the two variants exist for, not the fact that they both stop.
    #[inline]
    #[must_use]
    pub const fn ends_contact(self) -> bool {
        matches!(self, Self::Ended | Self::Cancelled)
    }

    /// A short, stable name for logs and test assertions.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Began => "Began",
            Self::Moved => "Moved",
            Self::Ended => "Ended",
            Self::Cancelled => "Cancelled",
        }
    }
}

impl fmt::Display for TouchPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How far a scroll event moved, in the units the device reported.
///
/// The two variants are not interchangeable and must not be collapsed by the
/// shell: a notched wheel genuinely reports discrete clicks, a touchpad
/// genuinely reports pixels, and the conversion factor between them is a
/// *policy* decision (how many pixels is one line?) that belongs to the action
/// layer and the UI, where it can be configured, not to a backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    /// Discrete detents — `y` positive scrolls the content up (away from the
    /// user), matching every platform's convention for a wheel.
    Lines {
        /// Horizontal detents.
        x: f32,
        /// Vertical detents.
        y: f32,
    },
    /// Continuous motion in physical pixels, from a touchpad or high-resolution
    /// wheel. Same sign convention as [`ScrollDelta::Lines`].
    Pixels {
        /// Horizontal pixels.
        x: f64,
        /// Vertical pixels.
        y: f64,
    },
}

impl ScrollDelta {
    /// Whether this delta is exactly zero in both axes.
    ///
    /// Wayland's `wl_pointer.axis_stop` and X11's button-4/5 emulation both
    /// produce zero-length scrolls; consumers usually want to drop them.
    #[must_use]
    pub fn is_zero(self) -> bool {
        match self {
            Self::Lines { x, y } => x == 0.0 && y == 0.0,
            Self::Pixels { x, y } => x == 0.0 && y == 0.0,
        }
    }
}

/// Generates [`KeyCode`] plus its name table from one list.
///
/// Two derived tables (`as_str`, `from_name`) and a hundred-odd variants are
/// exactly the kind of thing that rots when maintained by hand — one added key
/// with a forgotten name arm is a binding that serializes and never loads back.
macro_rules! key_codes {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// A key identified by physical position, independent of keyboard
        /// layout.
        ///
        /// Variant names follow the W3C UI Events `code` values, which name
        /// positions after the US-QWERTY legends: [`KeyCode::KeyW`] is the key
        /// *where W is on a US keyboard*, which on AZERTY is the key labelled
        /// Z. That is precisely the property gameplay bindings want — WASD
        /// stays a square under the left hand everywhere — and precisely the
        /// property a rebind menu must *not* display, which is what
        /// [`Keysym`](crate::input::Keysym) is for.
        ///
        /// # Non-exhaustive on purpose
        ///
        /// Unlike [`SurfaceTarget`](crate::SurfaceTarget), which is exhaustive
        /// so that a new platform breaks every backend loudly, this enum is
        /// `#[non_exhaustive]`: consumers match a handful of keys and fall
        /// through on the rest, so adding a key must not be a breaking change.
        /// A key nobody named is a key nobody bound, which degrades to "the
        /// binding does not fire" — not to a wrong action.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum KeyCode {
            $(
                #[doc = concat!("The `", $name, "` key.")]
                $variant,
            )+
        }

        impl KeyCode {
            /// Every key this engine has a name for, in declaration order.
            ///
            /// Used by the rebind UI to enumerate choices and by tests to prove
            /// the name table round-trips.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable name of this key, as written in binding assets and
            /// profiles.
            ///
            /// Stable across releases: it is a serialization format, not a
            /// display string. A rebind menu shows the *keysym*, not this.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// Parses a name produced by [`KeyCode::as_str`].
            ///
            /// Not a [`FromStr`](core::str::FromStr) impl: the only possible
            /// failure is "no such key", and an error type carrying that one
            /// fact would be ceremony. Bindings for unknown names are kept as
            /// [`Scancode`](crate::input::Scancode) by the profile loader
            /// rather than dropped.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

key_codes! {
    // Alphanumeric block.
    KeyA => "KeyA", KeyB => "KeyB", KeyC => "KeyC", KeyD => "KeyD", KeyE => "KeyE",
    KeyF => "KeyF", KeyG => "KeyG", KeyH => "KeyH", KeyI => "KeyI", KeyJ => "KeyJ",
    KeyK => "KeyK", KeyL => "KeyL", KeyM => "KeyM", KeyN => "KeyN", KeyO => "KeyO",
    KeyP => "KeyP", KeyQ => "KeyQ", KeyR => "KeyR", KeyS => "KeyS", KeyT => "KeyT",
    KeyU => "KeyU", KeyV => "KeyV", KeyW => "KeyW", KeyX => "KeyX", KeyY => "KeyY",
    KeyZ => "KeyZ",
    Digit0 => "Digit0", Digit1 => "Digit1", Digit2 => "Digit2", Digit3 => "Digit3",
    Digit4 => "Digit4", Digit5 => "Digit5", Digit6 => "Digit6", Digit7 => "Digit7",
    Digit8 => "Digit8", Digit9 => "Digit9",

    // Punctuation, in US-QWERTY positions.
    Backquote => "Backquote", Minus => "Minus", Equal => "Equal",
    BracketLeft => "BracketLeft", BracketRight => "BracketRight", Backslash => "Backslash",
    Semicolon => "Semicolon", Quote => "Quote", Comma => "Comma", Period => "Period",
    Slash => "Slash",

    // Keys that only exist on some physical layouts. Present because a binding
    // made on an ISO or JIS keyboard has to survive a round trip through a
    // profile written on an ANSI one.
    IntlBackslash => "IntlBackslash", IntlRo => "IntlRo", IntlYen => "IntlYen",

    // Editing and whitespace.
    Escape => "Escape", Tab => "Tab", CapsLock => "CapsLock", Space => "Space",
    Backspace => "Backspace", Enter => "Enter", Delete => "Delete", Insert => "Insert",

    // Modifiers, left and right distinguished.
    ShiftLeft => "ShiftLeft", ShiftRight => "ShiftRight",
    ControlLeft => "ControlLeft", ControlRight => "ControlRight",
    AltLeft => "AltLeft", AltRight => "AltRight",
    SuperLeft => "SuperLeft", SuperRight => "SuperRight",
    ContextMenu => "ContextMenu",

    // Navigation.
    Home => "Home", End => "End", PageUp => "PageUp", PageDown => "PageDown",
    ArrowUp => "ArrowUp", ArrowDown => "ArrowDown",
    ArrowLeft => "ArrowLeft", ArrowRight => "ArrowRight",

    // Function row.
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
    F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",

    // System keys.
    PrintScreen => "PrintScreen", ScrollLock => "ScrollLock", Pause => "Pause",

    // Numpad.
    NumLock => "NumLock",
    Numpad0 => "Numpad0", Numpad1 => "Numpad1", Numpad2 => "Numpad2",
    Numpad3 => "Numpad3", Numpad4 => "Numpad4", Numpad5 => "Numpad5",
    Numpad6 => "Numpad6", Numpad7 => "Numpad7", Numpad8 => "Numpad8",
    Numpad9 => "Numpad9",
    NumpadDivide => "NumpadDivide", NumpadMultiply => "NumpadMultiply",
    NumpadSubtract => "NumpadSubtract", NumpadAdd => "NumpadAdd",
    NumpadEnter => "NumpadEnter", NumpadDecimal => "NumpadDecimal",
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip_and_are_unique() {
        let mut names: Vec<&'static str> = KeyCode::ALL.iter().map(|key| key.as_str()).collect();
        let count = names.len();
        assert!(count > 100, "the table covers a full keyboard, got {count}");
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "key names must be unique");

        for key in KeyCode::ALL {
            assert_eq!(KeyCode::from_name(key.as_str()), Some(*key));
            assert_eq!(key.to_string(), key.as_str());
        }
        assert_eq!(KeyCode::from_name("KeyÆ"), None);
    }

    #[test]
    fn wasd_is_a_physical_square_not_a_layout_one() {
        // The whole point of the type: these are positions, so a binding stored
        // as a name resolves to the same position on any layout.
        for name in ["KeyW", "KeyA", "KeyS", "KeyD"] {
            assert!(KeyCode::from_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn keysyms_map_both_ways_over_the_xkb_rules() {
        // Latin-1 range: the keysym *is* the codepoint.
        assert_eq!(Keysym::from_char('a'), Keysym(0x61));
        assert_eq!(Keysym(0x61).to_char(), Some('a'));
        assert_eq!(Keysym::from_char('ä'), Keysym(0xE4));
        assert_eq!(Keysym(0xE4).to_char(), Some('ä'));

        // Everything else lives in the Unicode block.
        let euro = Keysym::from_char('€');
        assert_eq!(euro, Keysym(0x0100_20AC));
        assert_eq!(euro.to_char(), Some('€'));

        // Function keys and modifiers are not characters.
        const XKB_KEY_F1: Keysym = Keysym(0xFFBE);
        assert_eq!(XKB_KEY_F1.to_char(), None);
        assert!(Keysym::NONE.is_none());
        assert_eq!(Keysym::NONE.to_char(), None);

        assert_eq!(Keysym::from_char('q').to_string(), "'q'");
        assert_eq!(XKB_KEY_F1.to_string(), "keysym 0xffbe");
    }

    #[test]
    fn every_printable_ascii_round_trips_through_a_keysym() {
        for codepoint in 0x20u32..=0x7E {
            let character = char::from_u32(codepoint).expect("ascii");
            assert_eq!(Keysym::from_char(character).to_char(), Some(character));
        }
    }

    #[test]
    fn chord_modifiers_exclude_latched_state() {
        let held = Modifiers::CTRL | Modifiers::SHIFT | Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK;
        assert_eq!(held.chord(), Modifiers::CTRL | Modifiers::SHIFT);
        assert!(!Modifiers::CHORD.contains(Modifiers::ALT_GR));
        assert_eq!(Modifiers::default(), Modifiers::empty());
    }

    #[test]
    fn button_state_is_one_type_for_every_kind_of_button() {
        assert!(ButtonState::from_pressed(true).is_pressed());
        assert_eq!(ButtonState::from_pressed(false), ButtonState::Released);
    }

    #[test]
    fn scroll_units_stay_distinct() {
        let lines = ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let pixels = ScrollDelta::Pixels { x: 0.0, y: -13.0 };
        assert_ne!(
            format!("{lines:?}"),
            format!("{pixels:?}"),
            "a wheel detent and 13 px must not be the same value"
        );
        assert!(ScrollDelta::Lines { x: 0.0, y: 0.0 }.is_zero());
        assert!(ScrollDelta::Pixels { x: 0.0, y: 0.0 }.is_zero());
        assert!(!lines.is_zero());
        assert!(!pixels.is_zero());
    }

    #[test]
    fn device_ids_are_displayable_and_default_to_unknown() {
        assert_eq!(DeviceId::default(), DeviceId::UNKNOWN);
        assert_eq!(DeviceId::UNKNOWN.to_string(), "device ?");
        assert_eq!(DeviceId(4).to_string(), "device 4");
        assert_eq!(Scancode(30).to_string(), "scancode 30");
    }
}
