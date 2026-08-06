//! Apple virtual key codes → engine vocabulary, the modifier flags, and what
//! `NSEvent` calls a character.
//!
//! Everything here is a pure function over integers and characters, for the
//! reason [`geometry`](super::geometry) gives at length: it is the part of the
//! backend that can be wrong in a way AppKit would never complain about, so it
//! is the part that is unit-tested without a Mac.
//!
//! # A third numbering, and it resembles neither of the other two
//!
//! The two Linux backends share [`linux::keymap`](crate::linux::keymap) because
//! X11 keycodes really are evdev codes plus eight, and
//! [`win32::keys`](crate::win32::keys) is a second table because Win32 scan
//! codes are PS/2 set 1 — which *coincides* with evdev for the letters and
//! diverges above `0x53`. **Apple's `kVK_*` numbering coincides with neither at
//! any point.** `A` is `0x00`, `S` is `0x01`, `D` is `0x02`: the codes are the
//! positions of the original Apple Extended Keyboard's key matrix, so the
//! alphabetic block is not even contiguous and the digits run
//! `1 2 3 4 6 5 = 9 7 - 8 0` in code order. There is nothing to share and
//! nothing to be tempted by; the risk here is transcription, which is what the
//! tests below are pointed at.
//!
//! # Four keys the seam names and macOS cannot produce
//!
//! `PrintScreen`, `ScrollLock`, `Pause` and `ContextMenu` have no `kVK_*` code.
//! On a Mac keyboard those positions are `F13`, `F14` and `F15`, which are their
//! own keys and not a Print Screen wearing a hat — mapping them across would
//! make a Mac user's `F13` fire a binding on `PrintScreen`. So they are
//! unreachable, the test below asserts that the unreachable set is **exactly**
//! those four, and a key nobody can press is better than a key that fires for
//! something else.
//!
//! `NumLock` is the one that goes the other way: `kVK_ANSI_KeypadClear` is the
//! key in that position, an Apple keyboard labels it `Clear`, and a PC keyboard
//! plugged into a Mac reports its Num Lock as exactly that code. It is mapped.
//!
//! # A modifier key produces no key event at all
//!
//! Pressing Shift on this platform delivers `flagsChanged:` and **never**
//! `keyDown:`. So the down/up edge has to be reconstructed, and the flags word's
//! documented half cannot do it: it says *a* Shift is held, not which one. The
//! device-dependent bits — [`ffi::modifier`](super::ffi::modifier) — carry one
//! bit per physical key, and [`modifier_is_down`] is the reconstruction.

use crcbl_core::KeyCode;
use crcbl_core::input::{Keysym, Modifiers};

use super::ffi::{NSUInteger, modifier};

/// The engine key an Apple virtual key code names, layout-independently.
///
/// `None` for a key this engine has no name for — the volume block, `F17`–`F20`,
/// the JIS `Eisu` and `Kana` keys. That is not data loss:
/// [`ShellEvent::Key`](crate::ShellEvent::Key) still carries the raw
/// [`Scancode`](crcbl_core::input::Scancode), so the key stays bindable and
/// round-trippable through a profile.
///
/// A `match` rather than a lookup table, for the reason the other two backends
/// give: the assignment is sparse — `0x42`, `0x44`, `0x46`, `0x4D`, `0x6C` and
/// `0x6E` are holes — and a table with holes in it invites an off-by-one a
/// `match` cannot express.
#[must_use]
pub const fn key_code(keycode: u16) -> Option<KeyCode> {
    // `kVK_*` from `Carbon/HIToolbox/Events.h`, in numeric order.
    Some(match keycode {
        0x00 => KeyCode::KeyA,
        0x01 => KeyCode::KeyS,
        0x02 => KeyCode::KeyD,
        0x03 => KeyCode::KeyF,
        0x04 => KeyCode::KeyH,
        0x05 => KeyCode::KeyG,
        0x06 => KeyCode::KeyZ,
        0x07 => KeyCode::KeyX,
        0x08 => KeyCode::KeyC,
        0x09 => KeyCode::KeyV,
        // `kVK_ISO_Section`: the extra key an ISO keyboard has beside the left
        // Shift, which the W3C numbering calls `IntlBackslash`.
        0x0A => KeyCode::IntlBackslash,
        0x0B => KeyCode::KeyB,
        0x0C => KeyCode::KeyQ,
        0x0D => KeyCode::KeyW,
        0x0E => KeyCode::KeyE,
        0x0F => KeyCode::KeyR,
        0x10 => KeyCode::KeyY,
        0x11 => KeyCode::KeyT,
        0x12 => KeyCode::Digit1,
        0x13 => KeyCode::Digit2,
        0x14 => KeyCode::Digit3,
        0x15 => KeyCode::Digit4,
        // Six before five, and seven, nine and zero out of order below. The
        // matrix is not sorted and neither is this.
        0x16 => KeyCode::Digit6,
        0x17 => KeyCode::Digit5,
        0x18 => KeyCode::Equal,
        0x19 => KeyCode::Digit9,
        0x1A => KeyCode::Digit7,
        0x1B => KeyCode::Minus,
        0x1C => KeyCode::Digit8,
        0x1D => KeyCode::Digit0,
        0x1E => KeyCode::BracketRight,
        0x1F => KeyCode::KeyO,
        0x20 => KeyCode::KeyU,
        0x21 => KeyCode::BracketLeft,
        0x22 => KeyCode::KeyI,
        0x23 => KeyCode::KeyP,
        0x24 => KeyCode::Enter,
        0x25 => KeyCode::KeyL,
        0x26 => KeyCode::KeyJ,
        0x27 => KeyCode::Quote,
        0x28 => KeyCode::KeyK,
        0x29 => KeyCode::Semicolon,
        0x2A => KeyCode::Backslash,
        0x2B => KeyCode::Comma,
        0x2C => KeyCode::Slash,
        0x2D => KeyCode::KeyN,
        0x2E => KeyCode::KeyM,
        0x2F => KeyCode::Period,
        0x30 => KeyCode::Tab,
        0x31 => KeyCode::Space,
        0x32 => KeyCode::Backquote,
        // `kVK_Delete` is the key a PC calls Backspace, and `kVK_ForwardDelete`
        // (`0x75`) is the one a PC calls Delete. Apple's names are the trap.
        0x33 => KeyCode::Backspace,
        0x35 => KeyCode::Escape,
        0x36 => KeyCode::SuperRight,
        0x37 => KeyCode::SuperLeft,
        0x38 => KeyCode::ShiftLeft,
        0x39 => KeyCode::CapsLock,
        0x3A => KeyCode::AltLeft,
        0x3B => KeyCode::ControlLeft,
        0x3C => KeyCode::ShiftRight,
        0x3D => KeyCode::AltRight,
        0x3E => KeyCode::ControlRight,
        0x41 => KeyCode::NumpadDecimal,
        0x43 => KeyCode::NumpadMultiply,
        0x45 => KeyCode::NumpadAdd,
        // `kVK_ANSI_KeypadClear`: labelled Clear on an Apple keyboard and
        // reported by a PC keyboard's Num Lock. See the module docs.
        0x47 => KeyCode::NumLock,
        0x4B => KeyCode::NumpadDivide,
        0x4C => KeyCode::NumpadEnter,
        0x4E => KeyCode::NumpadSubtract,
        0x52 => KeyCode::Numpad0,
        0x53 => KeyCode::Numpad1,
        0x54 => KeyCode::Numpad2,
        0x55 => KeyCode::Numpad3,
        0x56 => KeyCode::Numpad4,
        0x57 => KeyCode::Numpad5,
        0x58 => KeyCode::Numpad6,
        0x59 => KeyCode::Numpad7,
        0x5B => KeyCode::Numpad8,
        0x5C => KeyCode::Numpad9,
        0x5D => KeyCode::IntlYen,
        0x5E => KeyCode::IntlRo,
        // The function row, which is where the numbering is at its least
        // ordered: F5 through F9 are contiguous and F1 through F4 are not.
        0x60 => KeyCode::F5,
        0x61 => KeyCode::F6,
        0x62 => KeyCode::F7,
        0x63 => KeyCode::F3,
        0x64 => KeyCode::F8,
        0x65 => KeyCode::F9,
        0x67 => KeyCode::F11,
        0x6D => KeyCode::F10,
        0x6F => KeyCode::F12,
        0x72 => KeyCode::Insert,
        0x73 => KeyCode::Home,
        0x74 => KeyCode::PageUp,
        0x75 => KeyCode::Delete,
        0x76 => KeyCode::F4,
        0x77 => KeyCode::End,
        0x78 => KeyCode::F2,
        0x79 => KeyCode::PageDown,
        0x7A => KeyCode::F1,
        0x7B => KeyCode::ArrowLeft,
        0x7C => KeyCode::ArrowRight,
        0x7D => KeyCode::ArrowDown,
        0x7E => KeyCode::ArrowUp,
        _ => return None,
    })
}

/// The keysym for a key whose symbol does not depend on the keyboard layout.
///
/// The table itself lives in [`crate::keysym`], shared with the Win32 backend:
/// it is a pure function of the engine's own [`KeyCode`] with no platform in
/// it, and two copies of one fact were exactly the drift the codebase's rules
/// warn about. Re-exported here so `keys::named_keysym` keeps reading the same
/// at every call site.
pub use crate::keysym::named_keysym;

/// The first codepoint AppKit uses for a function key it has no character for.
///
/// `NSUpArrowFunctionKey` and its neighbours live in the Unicode private use
/// area from `0xF700` to `0xF8FF`, and `-[NSEvent charactersIgnoringModifiers]`
/// answers with them for every arrow, function and navigation key. They are
/// **not characters**: turning one into a [`Keysym`] would put a private-use
/// codepoint in a rebind menu, so [`keysym_from_character`] refuses the range and
/// the key answers from [`named_keysym`] instead.
const FUNCTION_KEY_START: u32 = 0xF700;
/// One past the last of them. See [`FUNCTION_KEY_START`].
const FUNCTION_KEY_END: u32 = 0xF900;

/// A character with its case removed, in the form XKB names a key by.
///
/// XKB — and therefore [`Keysym`] and both Linux backends — names the
/// *lowercase* symbol of a letter key, and
/// `-[NSEvent charactersIgnoringModifiers]` applies Shift, so a rebind menu
/// would read `W` on macOS and `w` everywhere else for the same physical key.
/// The Win32 backend lowercases for the same reason and from the opposite
/// direction, where `MAPVK_VK_TO_CHAR` answers uppercase unconditionally.
///
/// A character whose lowercase form is more than one codepoint (there are a
/// handful in Unicode, none of them on a keyboard) keeps its original form
/// rather than being truncated into a different letter.
fn unshifted(character: char) -> char {
    let mut lowered = character.to_lowercase();
    match (lowered.next(), lowered.next()) {
        (Some(single), None) => single,
        _ => character,
    }
}

/// The keysym a character from `charactersIgnoringModifiers` names.
///
/// [`Keysym::NONE`] for the private-use function-key encodings and for control
/// characters, both of which that method really does return — Escape arrives as
/// `\u{1b}`, Enter as `\r`, and the up arrow as `\u{f700}`. Every one of them is
/// a key [`named_keysym`] already answers for, so refusing here is what keeps
/// the two sources from disagreeing.
#[must_use]
pub fn keysym_from_character(character: char) -> Keysym {
    let codepoint = character as u32;
    if (FUNCTION_KEY_START..FUNCTION_KEY_END).contains(&codepoint) || character.is_control() {
        return Keysym::NONE;
    }
    Keysym::from_char(unshifted(character))
}

/// Whether a committed character is text rather than a control code.
///
/// The input method commits `\r` for Enter and `\t` for Tab through
/// `insertText:` like any other string, and a text field that trusted the stream
/// would insert them verbatim.
/// [`ShellEvent::TextCommit`](crate::ShellEvent::TextCommit) means *text*, and
/// this is the same filter the other three backends apply, so all four agree
/// about what typing Enter commits: nothing.
#[must_use]
pub fn is_text(character: char) -> bool {
    !character.is_control()
}

/// The modifiers an `NSEvent`'s `modifierFlags` describes.
///
/// # Two flags that look like an answer and are not
///
/// * **`NSEventModifierFlagNumericPad` is not Num Lock.** It means "the key in
///   this event is on the keypad or is an arrow key", so it is set for the
///   length of an arrow keystroke and clear otherwise. Reading it as
///   [`NUM_LOCK`](Modifiers::NUM_LOCK) would report the modifier as latching on
///   and off every time the player pressed left. macOS has no Num Lock state at
///   all, so that bit is **never** set by this backend.
/// * **There is no AltGr, and Option is not it.** On this platform the Option
///   key is both the Alt of a shortcut (`⌘⌥I`) and the level-3 shift that
///   produces characters (`⌥e` is a dead acute), and there is no third key to
///   tell the two roles apart. It is reported as [`ALT`](Modifiers::ALT),
///   because a game's `Alt+E` binding must fire, and the characters it produces
///   reach the consumer as [`TextCommit`](crate::ShellEvent::TextCommit) rather
///   than being reconstructed from the modifier. The Win32 backend reaches the
///   opposite conclusion from the same starting point — there, AltGr is a real
///   right-Alt-plus-synthetic-Control pair that can be recognized and unpicked.
///
/// The `fn` key has no seam modifier and is dropped too.
#[must_use]
pub fn modifiers(flags: NSUInteger) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    modifiers.set(Modifiers::SHIFT, flags & modifier::SHIFT != 0);
    modifiers.set(Modifiers::CTRL, flags & modifier::CONTROL != 0);
    modifiers.set(Modifiers::ALT, flags & modifier::OPTION != 0);
    modifiers.set(Modifiers::SUPER, flags & modifier::COMMAND != 0);
    modifiers.set(Modifiers::CAPS_LOCK, flags & modifier::CAPS_LOCK != 0);
    modifiers
}

/// Whether a modifier key is down, from the flags a `flagsChanged:` carried.
///
/// `None` for a key that is not a modifier, which is what makes the caller's
/// arm total without a second list to keep in step.
///
/// # Why the device-dependent bits and not the documented ones
///
/// `flagsChanged:` says *what the flags are now* and its `keyCode` says *which
/// key moved*; neither says which way. The documented
/// [`SHIFT`](modifier::SHIFT) bit cannot answer it either, because it stays set
/// while the other Shift is still held — so releasing the left Shift of a
/// two-Shift chord would read as still down. One bit per physical key is the
/// only thing that gets that right, and
/// [`ffi::modifier`](super::ffi::modifier) is where they are written down.
///
/// [`CapsLock`](KeyCode::CapsLock) is the exception and has no device bit: it is
/// a **latch**, and the flag says whether it is on. So macOS reports a press when
/// Caps Lock turns on and a release when it turns off, rather than a press and a
/// release per physical tap. That is a fact about the platform rather than a gap
/// here, and it is the same shape X11's `LockMask` has.
#[must_use]
pub const fn modifier_is_down(key: KeyCode, flags: NSUInteger) -> Option<bool> {
    let bit = match key {
        KeyCode::ShiftLeft => modifier::DEVICE_LEFT_SHIFT,
        KeyCode::ShiftRight => modifier::DEVICE_RIGHT_SHIFT,
        KeyCode::ControlLeft => modifier::DEVICE_LEFT_CONTROL,
        KeyCode::ControlRight => modifier::DEVICE_RIGHT_CONTROL,
        KeyCode::AltLeft => modifier::DEVICE_LEFT_OPTION,
        KeyCode::AltRight => modifier::DEVICE_RIGHT_OPTION,
        KeyCode::SuperLeft => modifier::DEVICE_LEFT_COMMAND,
        KeyCode::SuperRight => modifier::DEVICE_RIGHT_COMMAND,
        KeyCode::CapsLock => modifier::CAPS_LOCK,
        _ => return None,
    };
    Some(flags & bit != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NSEventModifierFlagNumericPad`.
    ///
    /// Written here rather than in [`ffi::modifier`](super::super::ffi::modifier)
    /// because **nothing reads it**, which is the whole assertion: it is a
    /// key-class marker — "the key in this event is on the keypad or is an arrow
    /// key" — and reading it as the Num Lock its name suggests would report the
    /// latch switching on and off as the player pressed left.
    const NUMERIC_PAD: NSUInteger = 1 << 21;
    /// `NSEventModifierFlagFunction` — the `fn` key, which the seam has no name
    /// for. Also read by nothing, for the same reason it is written here.
    const FUNCTION: NSUInteger = 1 << 23;

    /// Every virtual key code an `NSEvent` can carry: `keyCode` is an
    /// `unsigned short`, and Apple assigns nothing above `0x7E`.
    fn all_keycodes() -> impl Iterator<Item = u16> {
        0..=0xFFu16
    }

    #[test]
    fn the_alphabetic_block_is_a_key_matrix_and_not_an_alphabet() {
        // The whole reason this table is transcribed rather than derived: `A` is
        // zero and `S`, `D`, `F` follow it, because the numbering is the order
        // of a 1987 keyboard's key matrix.
        assert_eq!(key_code(0x00), Some(KeyCode::KeyA));
        assert_eq!(key_code(0x01), Some(KeyCode::KeyS));
        assert_eq!(key_code(0x02), Some(KeyCode::KeyD));
        assert_eq!(key_code(0x03), Some(KeyCode::KeyF));
        // And the digits, where six comes before five and the punctuation is
        // interleaved with them.
        assert_eq!(key_code(0x16), Some(KeyCode::Digit6));
        assert_eq!(key_code(0x17), Some(KeyCode::Digit5));
        assert_eq!(key_code(0x18), Some(KeyCode::Equal));
        assert_eq!(key_code(0x19), Some(KeyCode::Digit9));
    }

    #[test]
    fn wasd_and_the_gameplay_keys_map_to_their_apple_codes() {
        assert_eq!(key_code(0x0D), Some(KeyCode::KeyW));
        assert_eq!(key_code(0x00), Some(KeyCode::KeyA));
        assert_eq!(key_code(0x01), Some(KeyCode::KeyS));
        assert_eq!(key_code(0x02), Some(KeyCode::KeyD));
        assert_eq!(key_code(0x31), Some(KeyCode::Space));
        assert_eq!(key_code(0x35), Some(KeyCode::Escape));
        assert_eq!(key_code(0x67), Some(KeyCode::F11), "the sample's mode key");
    }

    #[test]
    fn apples_delete_is_backspace_and_its_forward_delete_is_delete() {
        // `kVK_Delete` is the key above the return key — Backspace on a PC — and
        // `kVK_ForwardDelete` is the one in the navigation block. Taking Apple's
        // name at face value binds Backspace to Delete for every Mac player.
        assert_eq!(key_code(0x33), Some(KeyCode::Backspace));
        assert_eq!(key_code(0x75), Some(KeyCode::Delete));
        // And `kVK_Help`, which is the key in the Insert position.
        assert_eq!(key_code(0x72), Some(KeyCode::Insert));
    }

    #[test]
    fn the_function_row_is_not_in_order_and_neither_is_this_table() {
        // F1 through F4 are scattered above the navigation block while F5
        // through F9 are contiguous below it. A table written as `0x60 + n`
        // would be right for five keys and wrong for seven.
        for (code, key) in [
            (0x7Au16, KeyCode::F1),
            (0x78, KeyCode::F2),
            (0x63, KeyCode::F3),
            (0x76, KeyCode::F4),
            (0x60, KeyCode::F5),
            (0x61, KeyCode::F6),
            (0x62, KeyCode::F7),
            (0x64, KeyCode::F8),
            (0x65, KeyCode::F9),
            (0x6D, KeyCode::F10),
            (0x67, KeyCode::F11),
            (0x6F, KeyCode::F12),
        ] {
            assert_eq!(key_code(code), Some(key), "{key} is {code:#04x}");
        }
    }

    #[test]
    fn every_named_key_is_reachable_except_the_four_macos_has_no_code_for() {
        // The set is asserted by name rather than by count: a table edit that
        // made one more key unreachable would otherwise read as a smaller
        // number and nothing else.
        let reachable: Vec<KeyCode> = all_keycodes().filter_map(key_code).collect();
        let missing: Vec<&'static str> = KeyCode::ALL
            .iter()
            .filter(|key| !reachable.contains(key))
            .map(|key| key.as_str())
            .collect();
        assert_eq!(
            missing,
            // In `KeyCode::ALL`'s own order, which is where the list comes from.
            ["ContextMenu", "PrintScreen", "ScrollLock", "Pause"],
            "macOS has a kVK_* code for every other key the seam names"
        );
    }

    #[test]
    fn no_two_virtual_key_codes_name_the_same_key() {
        // A duplicated arm silently shadows and the shadowed key never fires.
        // Win32 has exactly one legitimate duplicate (Print Screen); Apple's
        // numbering has none, so this is an equality rather than an allowance.
        let mut seen: Vec<KeyCode> = all_keycodes().filter_map(key_code).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a kVK_* code is named twice");
        assert_eq!(seen.len(), KeyCode::ALL.len() - 4, "the four above");
    }

    #[test]
    fn the_holes_in_the_numbering_are_none_rather_than_a_wrong_key() {
        // Unassigned codes, the volume block, the function keys the seam has no
        // name for, and the JIS input-mode keys.
        for code in [
            0x34u16, 0x3F, 0x40, 0x42, 0x44, 0x46, 0x48, 0x49, 0x4A, 0x4F, 0x50, 0x51, 0x5A, 0x5F,
            0x66, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6E, 0x70, 0x71, 0x7F, 0x80, 0xFF,
        ] {
            assert_eq!(key_code(code), None, "virtual key {code:#04x}");
        }
    }

    #[test]
    fn a_layout_character_becomes_its_unshifted_keysym() {
        // The rebind-menu case: the same physical key must read the same on
        // every platform, and `charactersIgnoringModifiers` still applies Shift.
        assert_eq!(keysym_from_character('W'), Keysym::from_char('w'));
        assert_eq!(keysym_from_character('w'), Keysym::from_char('w'));
        assert_eq!(keysym_from_character('/'), Keysym(0x2F));
        assert_eq!(keysym_from_character('é'), Keysym(0xE9));
        assert_eq!(keysym_from_character('€'), Keysym(0x0100_20AC));
    }

    #[test]
    fn appkits_private_function_key_codepoints_are_not_symbols() {
        // `charactersIgnoringModifiers` answers `\u{f700}` for the up arrow and
        // `\r` for Return. Both are keys `named_keysym` already answers for, and
        // letting either through would put a private-use codepoint or a control
        // character in a rebind menu.
        for private in ['\u{f700}', '\u{f704}', '\u{f8ff}'] {
            assert_eq!(keysym_from_character(private), Keysym::NONE, "{private:?}");
        }
        for control in ['\r', '\t', '\u{1b}', '\u{8}', '\u{7f}'] {
            assert_eq!(keysym_from_character(control), Keysym::NONE, "{control:?}");
        }
        // And the codepoint just outside the private block is an ordinary
        // character again, so the range is a range and not "anything high".
        assert!(!keysym_from_character('\u{f6ff}').is_none());
    }

    #[test]
    fn control_characters_are_not_text() {
        for control in ['\u{8}', '\r', '\n', '\t', '\u{1b}', '\u{7f}'] {
            assert!(!is_text(control), "{control:?}");
        }
        for text in ['a', ' ', 'é', '🎮', '日'] {
            assert!(is_text(text), "{text:?}");
        }
    }

    #[test]
    fn the_numeric_pad_flag_is_never_read_as_num_lock() {
        // The trap this platform sets: `NSEventModifierFlagNumericPad` is set
        // for the length of an arrow keystroke, so reading it as a latch makes
        // Num Lock appear to switch on every time the player presses left.
        let arrow = modifiers(NUMERIC_PAD);
        assert_eq!(arrow, Modifiers::empty(), "{arrow:?}");
        assert!(!modifiers(NUMERIC_PAD | modifier::SHIFT).contains(Modifiers::NUM_LOCK));
    }

    #[test]
    fn option_is_alt_and_never_alt_gr() {
        // A game's `Alt+E` has to fire on a Mac. The characters Option produces
        // arrive as `TextCommit`, which is where they belong.
        let option = modifiers(modifier::OPTION);
        assert!(option.contains(Modifiers::ALT));
        assert!(!option.contains(Modifiers::ALT_GR), "{option:?}");
        assert_eq!(option.chord(), Modifiers::ALT);
    }

    #[test]
    fn command_is_super_and_caps_lock_is_latched() {
        assert_eq!(modifiers(modifier::COMMAND), Modifiers::SUPER);
        assert_eq!(modifiers(modifier::CONTROL), Modifiers::CTRL);
        assert_eq!(modifiers(modifier::SHIFT), Modifiers::SHIFT);
        assert_eq!(modifiers(modifier::CAPS_LOCK), Modifiers::CAPS_LOCK);
        assert_eq!(modifiers(0), Modifiers::empty());
        assert_eq!(modifiers(FUNCTION), Modifiers::empty(), "no fn");

        // A shortcut chord, with the latch masked off where a matcher wants it.
        let chord = modifiers(modifier::COMMAND | modifier::SHIFT | modifier::CAPS_LOCK);
        assert_eq!(chord.chord(), Modifiers::SUPER | Modifiers::SHIFT);
    }

    #[test]
    fn releasing_one_shift_of_a_two_shift_chord_reads_as_released() {
        // The reason the device-dependent bits exist. With both Shifts held the
        // documented `SHIFT` flag is set; letting go of the left one leaves it
        // set, so a backend reading it would report the left Shift as still
        // down for as long as the right one was held.
        let both = modifier::SHIFT | modifier::DEVICE_LEFT_SHIFT | modifier::DEVICE_RIGHT_SHIFT;
        assert_eq!(modifier_is_down(KeyCode::ShiftLeft, both), Some(true));
        assert_eq!(modifier_is_down(KeyCode::ShiftRight, both), Some(true));

        let right_only = modifier::SHIFT | modifier::DEVICE_RIGHT_SHIFT;
        assert_eq!(
            modifier_is_down(KeyCode::ShiftLeft, right_only),
            Some(false)
        );
        assert_eq!(
            modifier_is_down(KeyCode::ShiftRight, right_only),
            Some(true)
        );
        assert!(
            modifiers(right_only).contains(Modifiers::SHIFT),
            "and Shift is still held, which is the whole difficulty"
        );
    }

    #[test]
    fn every_modifier_key_has_its_own_device_bit_and_nothing_else_has_one() {
        // A shared bit would report the wrong key going down, and the right
        // Control's bit is the one that is not beside its left twin.
        let keys = [
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ];
        for held in keys {
            for other in keys {
                let down = modifier_is_down(other, device_bit_of(held))
                    .expect("every entry here is a modifier");
                assert_eq!(
                    down,
                    other == held,
                    "{held} down reported {other} as {down}"
                );
            }
        }
        // Caps Lock rides the documented latch instead, and reads as pressed
        // exactly while it is on.
        assert_eq!(modifier_is_down(KeyCode::CapsLock, 0), Some(false));
        assert_eq!(
            modifier_is_down(KeyCode::CapsLock, modifier::CAPS_LOCK),
            Some(true)
        );
        // And a key that is not a modifier has no answer at all.
        assert_eq!(modifier_is_down(KeyCode::KeyW, NSUInteger::MAX), None);
        assert_eq!(modifier_is_down(KeyCode::Space, 0), None);
    }

    /// The device bit a modifier key owns, for the exhaustive check above.
    fn device_bit_of(key: KeyCode) -> NSUInteger {
        match key {
            KeyCode::ShiftLeft => modifier::DEVICE_LEFT_SHIFT,
            KeyCode::ShiftRight => modifier::DEVICE_RIGHT_SHIFT,
            KeyCode::ControlLeft => modifier::DEVICE_LEFT_CONTROL,
            KeyCode::ControlRight => modifier::DEVICE_RIGHT_CONTROL,
            KeyCode::AltLeft => modifier::DEVICE_LEFT_OPTION,
            KeyCode::AltRight => modifier::DEVICE_RIGHT_OPTION,
            KeyCode::SuperLeft => modifier::DEVICE_LEFT_COMMAND,
            KeyCode::SuperRight => modifier::DEVICE_RIGHT_COMMAND,
            other => panic!("{other} is not a modifier with a device bit"),
        }
    }
}
