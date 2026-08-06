//! The one `KeyCode` → keysym mapping that has no platform in it.
//!
//! Most of a backend's keysym work is layout-shaped and stays in the backend:
//! which scancode a physical key has, whether a character key answers for a
//! layout's symbol, whether AltGr is `ISO_Level3_Shift`. The table here is the
//! residue — the keys whose X11 keysym cannot move with the keyboard layout,
//! mapped straight from the engine's own [`KeyCode`]. It was written out twice
//! (in `appkit::keys` and `win32::keys`) until a third copy made the
//! extraction obvious; both backends re-export [`named_keysym`] now, so the two
//! cannot drift.

use crcbl_core::KeyCode;
use crcbl_core::input::Keysym;

/// The keysym for a key whose symbol does not depend on the keyboard layout.
///
/// `None` means "ask the layout" — the letters, digits, punctuation and space,
/// which is exactly the set each backend's own layout question answers for. The
/// numbering is X11's, which is what [`Keysym`] holds and what
/// `crcbl_core::input` explains the choice of.
///
/// Right Alt is `Alt_R`, not `ISO_Level3_Shift`: whether that key is AltGr is a
/// property of the *layout*, and a backend cannot know it from the key alone.
/// The AltGr fact is reported where it is actually observable —
/// [`crcbl_core::input::Modifiers::ALT_GR`], out of each backend's `modifiers`.
#[must_use]
pub const fn named_keysym(key: KeyCode) -> Option<Keysym> {
    Some(Keysym(match key {
        KeyCode::Backspace => 0xFF08,
        KeyCode::Tab => 0xFF09,
        KeyCode::Enter => 0xFF0D,
        KeyCode::Pause => 0xFF13,
        KeyCode::ScrollLock => 0xFF14,
        KeyCode::Escape => 0xFF1B,
        KeyCode::Home => 0xFF50,
        KeyCode::ArrowLeft => 0xFF51,
        KeyCode::ArrowUp => 0xFF52,
        KeyCode::ArrowRight => 0xFF53,
        KeyCode::ArrowDown => 0xFF54,
        KeyCode::PageUp => 0xFF55,
        KeyCode::PageDown => 0xFF56,
        KeyCode::End => 0xFF57,
        KeyCode::PrintScreen => 0xFF61,
        KeyCode::Insert => 0xFF63,
        KeyCode::ContextMenu => 0xFF67,
        KeyCode::NumLock => 0xFF7F,
        KeyCode::NumpadEnter => 0xFF8D,
        KeyCode::NumpadMultiply => 0xFFAA,
        KeyCode::NumpadAdd => 0xFFAB,
        KeyCode::NumpadSubtract => 0xFFAD,
        KeyCode::NumpadDecimal => 0xFFAE,
        KeyCode::NumpadDivide => 0xFFAF,
        KeyCode::Numpad0 => 0xFFB0,
        KeyCode::Numpad1 => 0xFFB1,
        KeyCode::Numpad2 => 0xFFB2,
        KeyCode::Numpad3 => 0xFFB3,
        KeyCode::Numpad4 => 0xFFB4,
        KeyCode::Numpad5 => 0xFFB5,
        KeyCode::Numpad6 => 0xFFB6,
        KeyCode::Numpad7 => 0xFFB7,
        KeyCode::Numpad8 => 0xFFB8,
        KeyCode::Numpad9 => 0xFFB9,
        KeyCode::F1 => 0xFFBE,
        KeyCode::F2 => 0xFFBF,
        KeyCode::F3 => 0xFFC0,
        KeyCode::F4 => 0xFFC1,
        KeyCode::F5 => 0xFFC2,
        KeyCode::F6 => 0xFFC3,
        KeyCode::F7 => 0xFFC4,
        KeyCode::F8 => 0xFFC5,
        KeyCode::F9 => 0xFFC6,
        KeyCode::F10 => 0xFFC7,
        KeyCode::F11 => 0xFFC8,
        KeyCode::F12 => 0xFFC9,
        KeyCode::ShiftLeft => 0xFFE1,
        KeyCode::ShiftRight => 0xFFE2,
        KeyCode::ControlLeft => 0xFFE3,
        KeyCode::ControlRight => 0xFFE4,
        KeyCode::CapsLock => 0xFFE5,
        KeyCode::AltLeft => 0xFFE9,
        KeyCode::AltRight => 0xFFEA,
        KeyCode::SuperLeft => 0xFFEB,
        KeyCode::SuperRight => 0xFFEC,
        KeyCode::Delete => 0xFFFF,
        // The layout's job: every key that produces a character.
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keysyms_cover_the_keys_no_layout_can_move_and_no_others() {
        assert_eq!(named_keysym(KeyCode::F1), Some(Keysym(0xFFBE)));
        assert_eq!(named_keysym(KeyCode::Escape), Some(Keysym(0xFF1B)));
        assert_eq!(named_keysym(KeyCode::SuperLeft), Some(Keysym(0xFFEB)));
        assert_eq!(named_keysym(KeyCode::ShiftLeft), Some(Keysym(0xFFE1)));
        assert_eq!(named_keysym(KeyCode::Numpad5), Some(Keysym(0xFFB5)));
        // Right Alt is Alt_R, not ISO_Level3_Shift: whether it is AltGr is a
        // layout fact, and it is reported through `Modifiers::ALT_GR`.
        assert_eq!(named_keysym(KeyCode::AltRight), Some(Keysym(0xFFEA)));

        // None of the named ones is a character, which is what makes the split
        // with the layout's own answer clean rather than overlapping.
        for key in KeyCode::ALL {
            if let Some(keysym) = named_keysym(*key) {
                assert_eq!(keysym.to_char(), None, "{key} is not a character key");
            }
        }
        // And the character keys are exactly the ones left for the layout.
        for key in [
            KeyCode::KeyA,
            KeyCode::Digit1,
            KeyCode::Space,
            KeyCode::Minus,
            KeyCode::IntlYen,
        ] {
            assert_eq!(named_keysym(key), None, "{key} depends on the layout");
        }
    }

    /// Both backends re-export this one table — which is the point of the
    /// extraction — and nothing has quietly replaced a re-export with a local
    /// copy. Comparing the re-export's answers against the shared table's for
    /// every key is the check that catches a copy that has drifted: identical
    /// by definition here, red the moment one side is a different function.
    #[test]
    fn both_backends_re_export_the_shared_table() {
        for key in KeyCode::ALL {
            assert_eq!(
                crate::appkit::keys::named_keysym(*key),
                named_keysym(*key),
                "{key}: the AppKit backend is not answering from the shared table",
            );
            assert_eq!(
                crate::win32::keys::named_keysym(*key),
                named_keysym(*key),
                "{key}: the Win32 backend is not answering from the shared table",
            );
        }
    }
}
