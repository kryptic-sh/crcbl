//! The one `KeyCode` → keysym mapping that has no platform in it.
//!
//! Compiled on every target: the AppKit and Win32 backends read it, and so
//! does the [headless](crate::headless) backend, which runs everywhere and
//! reports the same keysyms a real backend would.
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
    Some(match key {
        KeyCode::Backspace => Keysym::BACKSPACE,
        KeyCode::Tab => Keysym::TAB,
        KeyCode::Enter => Keysym::RETURN,
        KeyCode::Pause => Keysym::PAUSE,
        KeyCode::ScrollLock => Keysym::SCROLL_LOCK,
        KeyCode::Escape => Keysym::ESCAPE,
        KeyCode::Home => Keysym::HOME,
        KeyCode::ArrowLeft => Keysym::LEFT,
        KeyCode::ArrowUp => Keysym::UP,
        KeyCode::ArrowRight => Keysym::RIGHT,
        KeyCode::ArrowDown => Keysym::DOWN,
        KeyCode::PageUp => Keysym::PAGE_UP,
        KeyCode::PageDown => Keysym::PAGE_DOWN,
        KeyCode::End => Keysym::END,
        KeyCode::PrintScreen => Keysym::PRINT,
        KeyCode::Insert => Keysym::INSERT,
        KeyCode::ContextMenu => Keysym::MENU,
        KeyCode::NumLock => Keysym::NUM_LOCK,
        KeyCode::NumpadEnter => Keysym::KP_ENTER,
        KeyCode::NumpadMultiply => Keysym::KP_MULTIPLY,
        KeyCode::NumpadAdd => Keysym::KP_ADD,
        KeyCode::NumpadSubtract => Keysym::KP_SUBTRACT,
        KeyCode::NumpadDecimal => Keysym::KP_DECIMAL,
        KeyCode::NumpadDivide => Keysym::KP_DIVIDE,
        KeyCode::Numpad0 => Keysym::KP_0,
        KeyCode::Numpad1 => Keysym::KP_1,
        KeyCode::Numpad2 => Keysym::KP_2,
        KeyCode::Numpad3 => Keysym::KP_3,
        KeyCode::Numpad4 => Keysym::KP_4,
        KeyCode::Numpad5 => Keysym::KP_5,
        KeyCode::Numpad6 => Keysym::KP_6,
        KeyCode::Numpad7 => Keysym::KP_7,
        KeyCode::Numpad8 => Keysym::KP_8,
        KeyCode::Numpad9 => Keysym::KP_9,
        KeyCode::F1 => Keysym::F1,
        KeyCode::F2 => Keysym::F2,
        KeyCode::F3 => Keysym::F3,
        KeyCode::F4 => Keysym::F4,
        KeyCode::F5 => Keysym::F5,
        KeyCode::F6 => Keysym::F6,
        KeyCode::F7 => Keysym::F7,
        KeyCode::F8 => Keysym::F8,
        KeyCode::F9 => Keysym::F9,
        KeyCode::F10 => Keysym::F10,
        KeyCode::F11 => Keysym::F11,
        KeyCode::F12 => Keysym::F12,
        KeyCode::ShiftLeft => Keysym::SHIFT_L,
        KeyCode::ShiftRight => Keysym::SHIFT_R,
        KeyCode::ControlLeft => Keysym::CONTROL_L,
        KeyCode::ControlRight => Keysym::CONTROL_R,
        KeyCode::CapsLock => Keysym::CAPS_LOCK,
        KeyCode::AltLeft => Keysym::ALT_L,
        KeyCode::AltRight => Keysym::ALT_R,
        KeyCode::SuperLeft => Keysym::SUPER_L,
        KeyCode::SuperRight => Keysym::SUPER_R,
        KeyCode::Delete => Keysym::DELETE,
        // The layout's job: every key that produces a character.
        _ => return None,
    })
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
