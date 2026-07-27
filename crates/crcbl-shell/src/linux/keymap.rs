//! Linux evdev codes → engine vocabulary: [`KeyCode`], [`PointerButton`].
//!
//! # Why this is ours and not libxkbcommon's
//!
//! [`KeyCode`] is a *physical position*, and a position must not come from a
//! layout database. `wl_keyboard.key` carries the kernel's evdev code, which is
//! already the position — `KEY_A` is 30 whether the user types QWERTY, AZERTY
//! or Dvorak — so the mapping below is a fixed table with no state and no
//! dependency. That is what keeps WASD a square under the left hand on every
//! layout, and what keeps the gameplay binding path working on a machine with
//! no libxkbcommon at all (see [`xkb`](super::xkb)).
//!
//! XKB is asked only about things that genuinely depend on the layout: the
//! keysym, the committed text, the modifier bits and whether a key repeats.
//!
//! The numbers are `linux/input-event-codes.h`, which is kernel UAPI and
//! therefore frozen — a code that meant `KEY_A` in 1995 still does.

use crcbl_core::KeyCode;
use crcbl_core::input::PointerButton;

/// Every evdev code this engine has a [`KeyCode`] name for.
///
/// A `match` rather than a lookup array: the codes are sparse (`84`, `85`,
/// `101` and everything from `112` up are keys we do not name), the compiler
/// turns a dense-enough match into a jump table anyway, and a table with holes
/// invites an off-by-one that a `match` cannot express.
///
/// Returns `None` for a key the engine does not name. That is not data loss:
/// [`ShellEvent::Key`](crate::ShellEvent::Key) still carries the raw
/// [`Scancode`](crcbl_core::input::Scancode), so the key stays bindable,
/// loggable and round-trippable through a profile.
#[must_use]
pub const fn key_code(evdev: u32) -> Option<KeyCode> {
    // linux/input-event-codes.h, in numeric order. The comment on each row is
    // the kernel's own name, so a reader can diff this against the header.
    Some(match evdev {
        1 => KeyCode::Escape,          // KEY_ESC
        2 => KeyCode::Digit1,          // KEY_1
        3 => KeyCode::Digit2,          // KEY_2
        4 => KeyCode::Digit3,          // KEY_3
        5 => KeyCode::Digit4,          // KEY_4
        6 => KeyCode::Digit5,          // KEY_5
        7 => KeyCode::Digit6,          // KEY_6
        8 => KeyCode::Digit7,          // KEY_7
        9 => KeyCode::Digit8,          // KEY_8
        10 => KeyCode::Digit9,         // KEY_9
        11 => KeyCode::Digit0,         // KEY_0
        12 => KeyCode::Minus,          // KEY_MINUS
        13 => KeyCode::Equal,          // KEY_EQUAL
        14 => KeyCode::Backspace,      // KEY_BACKSPACE
        15 => KeyCode::Tab,            // KEY_TAB
        16 => KeyCode::KeyQ,           // KEY_Q
        17 => KeyCode::KeyW,           // KEY_W
        18 => KeyCode::KeyE,           // KEY_E
        19 => KeyCode::KeyR,           // KEY_R
        20 => KeyCode::KeyT,           // KEY_T
        21 => KeyCode::KeyY,           // KEY_Y
        22 => KeyCode::KeyU,           // KEY_U
        23 => KeyCode::KeyI,           // KEY_I
        24 => KeyCode::KeyO,           // KEY_O
        25 => KeyCode::KeyP,           // KEY_P
        26 => KeyCode::BracketLeft,    // KEY_LEFTBRACE
        27 => KeyCode::BracketRight,   // KEY_RIGHTBRACE
        28 => KeyCode::Enter,          // KEY_ENTER
        29 => KeyCode::ControlLeft,    // KEY_LEFTCTRL
        30 => KeyCode::KeyA,           // KEY_A
        31 => KeyCode::KeyS,           // KEY_S
        32 => KeyCode::KeyD,           // KEY_D
        33 => KeyCode::KeyF,           // KEY_F
        34 => KeyCode::KeyG,           // KEY_G
        35 => KeyCode::KeyH,           // KEY_H
        36 => KeyCode::KeyJ,           // KEY_J
        37 => KeyCode::KeyK,           // KEY_K
        38 => KeyCode::KeyL,           // KEY_L
        39 => KeyCode::Semicolon,      // KEY_SEMICOLON
        40 => KeyCode::Quote,          // KEY_APOSTROPHE
        41 => KeyCode::Backquote,      // KEY_GRAVE
        42 => KeyCode::ShiftLeft,      // KEY_LEFTSHIFT
        43 => KeyCode::Backslash,      // KEY_BACKSLASH
        44 => KeyCode::KeyZ,           // KEY_Z
        45 => KeyCode::KeyX,           // KEY_X
        46 => KeyCode::KeyC,           // KEY_C
        47 => KeyCode::KeyV,           // KEY_V
        48 => KeyCode::KeyB,           // KEY_B
        49 => KeyCode::KeyN,           // KEY_N
        50 => KeyCode::KeyM,           // KEY_M
        51 => KeyCode::Comma,          // KEY_COMMA
        52 => KeyCode::Period,         // KEY_DOT
        53 => KeyCode::Slash,          // KEY_SLASH
        54 => KeyCode::ShiftRight,     // KEY_RIGHTSHIFT
        55 => KeyCode::NumpadMultiply, // KEY_KPASTERISK
        56 => KeyCode::AltLeft,        // KEY_LEFTALT
        57 => KeyCode::Space,          // KEY_SPACE
        58 => KeyCode::CapsLock,       // KEY_CAPSLOCK
        59 => KeyCode::F1,             // KEY_F1
        60 => KeyCode::F2,             // KEY_F2
        61 => KeyCode::F3,             // KEY_F3
        62 => KeyCode::F4,             // KEY_F4
        63 => KeyCode::F5,             // KEY_F5
        64 => KeyCode::F6,             // KEY_F6
        65 => KeyCode::F7,             // KEY_F7
        66 => KeyCode::F8,             // KEY_F8
        67 => KeyCode::F9,             // KEY_F9
        68 => KeyCode::F10,            // KEY_F10
        69 => KeyCode::NumLock,        // KEY_NUMLOCK
        70 => KeyCode::ScrollLock,     // KEY_SCROLLLOCK
        71 => KeyCode::Numpad7,        // KEY_KP7
        72 => KeyCode::Numpad8,        // KEY_KP8
        73 => KeyCode::Numpad9,        // KEY_KP9
        74 => KeyCode::NumpadSubtract, // KEY_KPMINUS
        75 => KeyCode::Numpad4,        // KEY_KP4
        76 => KeyCode::Numpad5,        // KEY_KP5
        77 => KeyCode::Numpad6,        // KEY_KP6
        78 => KeyCode::NumpadAdd,      // KEY_KPPLUS
        79 => KeyCode::Numpad1,        // KEY_KP1
        80 => KeyCode::Numpad2,        // KEY_KP2
        81 => KeyCode::Numpad3,        // KEY_KP3
        82 => KeyCode::Numpad0,        // KEY_KP0
        83 => KeyCode::NumpadDecimal,  // KEY_KPDOT
        86 => KeyCode::IntlBackslash,  // KEY_102ND — the extra ISO key
        87 => KeyCode::F11,            // KEY_F11
        88 => KeyCode::F12,            // KEY_F12
        89 => KeyCode::IntlRo,         // KEY_RO — JIS
        96 => KeyCode::NumpadEnter,    // KEY_KPENTER
        97 => KeyCode::ControlRight,   // KEY_RIGHTCTRL
        98 => KeyCode::NumpadDivide,   // KEY_KPSLASH
        99 => KeyCode::PrintScreen,    // KEY_SYSRQ
        100 => KeyCode::AltRight,      // KEY_RIGHTALT — AltGr on most layouts
        102 => KeyCode::Home,          // KEY_HOME
        103 => KeyCode::ArrowUp,       // KEY_UP
        104 => KeyCode::PageUp,        // KEY_PAGEUP
        105 => KeyCode::ArrowLeft,     // KEY_LEFT
        106 => KeyCode::ArrowRight,    // KEY_RIGHT
        107 => KeyCode::End,           // KEY_END
        108 => KeyCode::ArrowDown,     // KEY_DOWN
        109 => KeyCode::PageDown,      // KEY_PAGEDOWN
        110 => KeyCode::Insert,        // KEY_INSERT
        111 => KeyCode::Delete,        // KEY_DELETE
        119 => KeyCode::Pause,         // KEY_PAUSE
        124 => KeyCode::IntlYen,       // KEY_YEN — JIS
        125 => KeyCode::SuperLeft,     // KEY_LEFTMETA
        126 => KeyCode::SuperRight,    // KEY_RIGHTMETA
        127 => KeyCode::ContextMenu,   // KEY_COMPOSE
        _ => return None,
    })
}

/// `BTN_LEFT`, the first of the mouse-button block.
const BTN_LEFT: u32 = 0x110;
/// `BTN_RIGHT`.
const BTN_RIGHT: u32 = 0x111;
/// `BTN_MIDDLE`.
const BTN_MIDDLE: u32 = 0x112;
/// `BTN_SIDE`.
const BTN_SIDE: u32 = 0x113;
/// `BTN_EXTRA`.
const BTN_EXTRA: u32 = 0x114;

/// `wl_pointer.button`'s evdev button code as a [`PointerButton`].
///
/// The five named buttons are the ones every platform agrees on;
/// anything else keeps its raw evdev code inside
/// [`PointerButton::Other`], which is what makes a gaming mouse's sixth thumb
/// button bindable rather than invisible. It is *not* renumbered from zero:
/// the raw code is what a profile has to round-trip, and inventing an index
/// would make two backends disagree about what `Other(2)` means.
#[must_use]
pub const fn pointer_button(evdev: u32) -> PointerButton {
    match evdev {
        BTN_LEFT => PointerButton::Left,
        BTN_RIGHT => PointerButton::Right,
        BTN_MIDDLE => PointerButton::Middle,
        BTN_SIDE => PointerButton::Back,
        BTN_EXTRA => PointerButton::Forward,
        #[expect(
            clippy::cast_possible_truncation,
            reason = "evdev button codes live in 0x100..0x200; the truncation \
                      cannot be reached from a real device"
        )]
        other => PointerButton::Other(other as u16),
    }
}

/// A `wl_fixed` (signed 24.8) as an `f64`.
///
/// Pointer positions and scroll amounts both arrive in this form. Truncating to
/// an integer here would quantize aim on a 240 Hz mouse over a scaled output,
/// which is exactly what [`PhysicalPoint`](crate::PhysicalPoint) is `f64` to
/// avoid.
#[inline]
#[must_use]
pub fn fixed_to_f64(value: i32) -> f64 {
    f64::from(value) / 256.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasd_and_the_gameplay_keys_map_to_their_evdev_codes() {
        // The table's whole purpose, spelled out: these are kernel UAPI numbers
        // and they do not move.
        assert_eq!(key_code(17), Some(KeyCode::KeyW));
        assert_eq!(key_code(30), Some(KeyCode::KeyA));
        assert_eq!(key_code(31), Some(KeyCode::KeyS));
        assert_eq!(key_code(32), Some(KeyCode::KeyD));
        assert_eq!(key_code(57), Some(KeyCode::Space));
        assert_eq!(key_code(1), Some(KeyCode::Escape));
        assert_eq!(key_code(42), Some(KeyCode::ShiftLeft));
        assert_eq!(key_code(54), Some(KeyCode::ShiftRight));
        assert_eq!(key_code(125), Some(KeyCode::SuperLeft));
    }

    #[test]
    fn every_named_key_is_reachable_from_some_evdev_code() {
        // A key in the vocabulary that no scancode produces is a key nobody can
        // ever bind on Linux — the failure this test exists to catch, because
        // it is invisible in every other way.
        let mut missing: Vec<&'static str> = Vec::new();
        for key in KeyCode::ALL {
            if !(0..=255).any(|code| key_code(code) == Some(*key)) {
                missing.push(key.as_str());
            }
        }
        assert!(missing.is_empty(), "unreachable KeyCodes: {missing:?}");
    }

    #[test]
    fn no_evdev_code_maps_to_two_different_keys() {
        // The other direction: a duplicated arm would silently shadow, and the
        // shadowed key would simply never fire.
        let mut seen: Vec<KeyCode> = (0..=255).filter_map(key_code).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "two evdev codes map to one KeyCode");
        assert_eq!(
            count,
            KeyCode::ALL.len(),
            "the table is exactly the vocabulary"
        );
    }

    #[test]
    fn unnamed_keys_degrade_to_none_rather_than_to_a_wrong_key() {
        // Holes in the kernel's numbering, and the multimedia block above 112.
        for code in [0, 84, 85, 101, 113, 128, 190, 240, u32::MAX] {
            assert_eq!(key_code(code), None, "evdev {code}");
        }
    }

    #[test]
    fn pointer_buttons_name_the_five_everyone_agrees_on() {
        assert_eq!(pointer_button(0x110), PointerButton::Left);
        assert_eq!(pointer_button(0x111), PointerButton::Right);
        assert_eq!(pointer_button(0x112), PointerButton::Middle);
        assert_eq!(pointer_button(0x113), PointerButton::Back);
        assert_eq!(pointer_button(0x114), PointerButton::Forward);
        // A gaming mouse's extra thumb button keeps its raw code.
        assert_eq!(pointer_button(0x115), PointerButton::Other(0x115));
        assert_eq!(pointer_button(0x120), PointerButton::Other(0x120));
    }

    #[test]
    fn wl_fixed_keeps_the_sub_pixel_part() {
        // 24.8 signed. Truncating would quantize aim, which is the bug the type
        // choice in `geom` exists to prevent.
        assert!((fixed_to_f64(256) - 1.0).abs() < f64::EPSILON);
        assert!((fixed_to_f64(-256) + 1.0).abs() < f64::EPSILON);
        assert!((fixed_to_f64(128) - 0.5).abs() < f64::EPSILON);
        assert!((fixed_to_f64(-1) + 0.003_906_25).abs() < f64::EPSILON);
        assert!((fixed_to_f64(0)).abs() < f64::EPSILON);
    }
}
