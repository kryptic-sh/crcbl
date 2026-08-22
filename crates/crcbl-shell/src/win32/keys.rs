//! Win32 keyboard numbering → engine vocabulary, and the two things `WM_CHAR`
//! needs done to it.
//!
//! Everything here is a pure function over integers, for the reason
//! [`geometry`](super::geometry) gives at length: it is the part of the backend
//! that can be wrong in a way Windows would never complain about, so it is the
//! part that is unit-tested without Windows.
//!
//! # The scan code is in `lParam`, and the extended bit is part of it
//!
//! A `WM_KEYDOWN`'s `lParam` is a bit field, and this module reads three of its
//! fields:
//!
//! | Bits | Meaning |
//! | --- | --- |
//! | 16–23 | the **scan code**, as the keyboard sent it |
//! | 24 | the **extended** flag: the key's make code was prefixed with `E0` |
//! | 30 | the previous key state — `1` means the key was already down |
//!
//! Bit 24 is not decoration. PS/2 set 1 reuses one byte for two keys and tells
//! them apart with an `E0` prefix, so scan code `0x1C` is Enter *or* the
//! numeric keypad's Enter, `0x38` is left Alt *or* right Alt, and `0x47` is
//! Numpad 7 *or* Home. [`scancode`] folds the prefix back in as `0xE000`, which
//! is the numbering `MapVirtualKeyExW`'s `MAPVK_VK_TO_VSC_EX` also produces, and
//! [`key_code`] matches on the folded value. A backend that drops the bit binds
//! Home and Numpad 7 to the same action.
//!
//! # This is a different table from evdev, and the resemblance is the trap
//!
//! The two Linux backends share `linux::keymap`, because X11 keycodes really
//! are evdev codes plus eight. **Win32 scan codes are PS/2 set 1**, and the
//! tempting observation is that the main block *coincides*: evdev's numbering
//! was defined from set 1, so `KEY_A` is 30 and set 1's A is `0x1E`, which is
//! the same number.
//!
//! That coincidence is exactly what makes reusing the table a trap, because it
//! stops at `0x53`. Above it the two diverge completely — evdev continues flat
//! (`KEY_UP` is 103, `KEY_KPENTER` is 96), while set 1 leaves `0x60`–`0x6F`
//! unassigned and puts every one of those keys in an `E0`-prefixed second range
//! (`ArrowUp` is `0xE048`, `NumpadEnter` is `0xE01C`). A shared table would be
//! right for the letters and silently wrong for every arrow, navigation key,
//! right-hand modifier and keypad Enter. So this is a second table, written from
//! the set-1 assignment, and the tests below check the same two properties the
//! evdev one does: every [`KeyCode`] is reachable, and no two scan codes name
//! the same key.
//!
//! # Why the virtual key is carried as well
//!
//! [`KeyCode`] is a *position*, so it comes from the scan code and never from
//! the virtual key — `VK_A` is the key labelled A, which on AZERTY is a
//! different position. The virtual key is still needed for one thing:
//! [`crcbl_core::input::Keysym`], which is by definition layout-dependent. [`named_keysym`] answers
//! for the keys whose symbol does not depend on the layout, and everything it
//! returns `None` for is asked of `MapVirtualKeyW` — see
//! [`input`](super::input).

use crcbl_core::KeyCode;
use crcbl_core::input::Modifiers;

use super::ffi::value;

/// The bit in a key message's `lParam` that marks an `E0`-prefixed key.
const EXTENDED_BIT: isize = 1 << 24;
/// The bit that says the key was already down — an auto-repeat.
const PREVIOUS_STATE_BIT: isize = 1 << 30;
/// What [`scancode`] ORs into an extended key's code.
const EXTENDED_PREFIX: u32 = 0xE000;

/// The scan code a key message's `lParam` carries, with the `E0` prefix folded
/// back in.
///
/// See the [module docs](self): the extended bit is part of the key's identity,
/// not a flag beside it.
#[must_use]
pub const fn scancode(l_param: isize) -> u32 {
    let code = ((l_param >> 16) & 0xFF) as u32;
    if l_param & EXTENDED_BIT == 0 {
        code
    } else {
        EXTENDED_PREFIX | code
    }
}

/// Whether a key-**down** message says the key was already held.
///
/// Bit 30 is the previous key state, and it is meaningful only on the way down:
/// on a `WM_KEYUP` the key was of course down a moment ago, so the bit is always
/// set and reading it there would report every release as a repeat. The caller
/// is what knows which message this is, so the check is left there and stated
/// here.
#[must_use]
pub const fn was_already_down(l_param: isize) -> bool {
    l_param & PREVIOUS_STATE_BIT != 0
}

/// The engine key a PS/2 set 1 scan code names, layout-independently.
///
/// `None` for a key this engine has no name for — a laptop's brightness key, a
/// keyboard's media block. That is not data loss:
/// [`ShellEvent::Key`](crate::ShellEvent::Key) still carries the raw
/// [`Scancode`](crcbl_core::input::Scancode), so the key stays bindable and
/// round-trippable through a profile.
///
/// A `match` rather than a lookup table, for the reason the evdev table gives:
/// the extended codes are a second, sparse range and a table with a hole in it
/// invites an off-by-one a `match` cannot express.
#[must_use]
pub const fn key_code(scancode: u32) -> Option<KeyCode> {
    // PS/2 scan code set 1, in numeric order, with the extended block after it.
    Some(match scancode {
        0x0001 => KeyCode::Escape,
        0x0002 => KeyCode::Digit1,
        0x0003 => KeyCode::Digit2,
        0x0004 => KeyCode::Digit3,
        0x0005 => KeyCode::Digit4,
        0x0006 => KeyCode::Digit5,
        0x0007 => KeyCode::Digit6,
        0x0008 => KeyCode::Digit7,
        0x0009 => KeyCode::Digit8,
        0x000A => KeyCode::Digit9,
        0x000B => KeyCode::Digit0,
        0x000C => KeyCode::Minus,
        0x000D => KeyCode::Equal,
        0x000E => KeyCode::Backspace,
        0x000F => KeyCode::Tab,
        0x0010 => KeyCode::KeyQ,
        0x0011 => KeyCode::KeyW,
        0x0012 => KeyCode::KeyE,
        0x0013 => KeyCode::KeyR,
        0x0014 => KeyCode::KeyT,
        0x0015 => KeyCode::KeyY,
        0x0016 => KeyCode::KeyU,
        0x0017 => KeyCode::KeyI,
        0x0018 => KeyCode::KeyO,
        0x0019 => KeyCode::KeyP,
        0x001A => KeyCode::BracketLeft,
        0x001B => KeyCode::BracketRight,
        0x001C => KeyCode::Enter,
        0x001D => KeyCode::ControlLeft,
        0x001E => KeyCode::KeyA,
        0x001F => KeyCode::KeyS,
        0x0020 => KeyCode::KeyD,
        0x0021 => KeyCode::KeyF,
        0x0022 => KeyCode::KeyG,
        0x0023 => KeyCode::KeyH,
        0x0024 => KeyCode::KeyJ,
        0x0025 => KeyCode::KeyK,
        0x0026 => KeyCode::KeyL,
        0x0027 => KeyCode::Semicolon,
        0x0028 => KeyCode::Quote,
        0x0029 => KeyCode::Backquote,
        0x002A => KeyCode::ShiftLeft,
        0x002B => KeyCode::Backslash,
        0x002C => KeyCode::KeyZ,
        0x002D => KeyCode::KeyX,
        0x002E => KeyCode::KeyC,
        0x002F => KeyCode::KeyV,
        0x0030 => KeyCode::KeyB,
        0x0031 => KeyCode::KeyN,
        0x0032 => KeyCode::KeyM,
        0x0033 => KeyCode::Comma,
        0x0034 => KeyCode::Period,
        0x0035 => KeyCode::Slash,
        0x0036 => KeyCode::ShiftRight,
        0x0037 => KeyCode::NumpadMultiply,
        0x0038 => KeyCode::AltLeft,
        0x0039 => KeyCode::Space,
        0x003A => KeyCode::CapsLock,
        0x003B => KeyCode::F1,
        0x003C => KeyCode::F2,
        0x003D => KeyCode::F3,
        0x003E => KeyCode::F4,
        0x003F => KeyCode::F5,
        0x0040 => KeyCode::F6,
        0x0041 => KeyCode::F7,
        0x0042 => KeyCode::F8,
        0x0043 => KeyCode::F9,
        0x0044 => KeyCode::F10,
        // **Not** Num Lock. Pause's make code is the `E1`-prefixed sequence
        // `E1 1D 45`, which the keyboard driver reports as a bare `0x45`;
        // Num Lock is the *extended* `0xE045`, because MSDN's list of extended
        // keys names it. Swapping the two is the classic Win32 mistake here.
        0x0045 => KeyCode::Pause,
        0x0046 => KeyCode::ScrollLock,
        0x0047 => KeyCode::Numpad7,
        0x0048 => KeyCode::Numpad8,
        0x0049 => KeyCode::Numpad9,
        0x004A => KeyCode::NumpadSubtract,
        0x004B => KeyCode::Numpad4,
        0x004C => KeyCode::Numpad5,
        0x004D => KeyCode::Numpad6,
        0x004E => KeyCode::NumpadAdd,
        0x004F => KeyCode::Numpad1,
        0x0050 => KeyCode::Numpad2,
        0x0051 => KeyCode::Numpad3,
        0x0052 => KeyCode::Numpad0,
        0x0053 => KeyCode::NumpadDecimal,
        // SysRq: Alt+Print Screen reports this instead of `0xE037`, because the
        // Alt makes the keyboard send the unprefixed code. Both are the Print
        // Screen key and both map to it — the one place this table is not
        // injective, and the test below names it.
        0x0054 => KeyCode::PrintScreen,
        0x0056 => KeyCode::IntlBackslash,
        0x0057 => KeyCode::F11,
        0x0058 => KeyCode::F12,
        // The JIS keys. `0x0070` (Kana), `0x0079` (Henkan) and `0x007B`
        // (Muhenkan) are the neighbours this engine has no name for.
        0x0073 => KeyCode::IntlRo,
        0x007D => KeyCode::IntlYen,

        // The `E0`-prefixed block.
        0xE01C => KeyCode::NumpadEnter,
        0xE01D => KeyCode::ControlRight,
        0xE035 => KeyCode::NumpadDivide,
        0xE037 => KeyCode::PrintScreen,
        0xE038 => KeyCode::AltRight,
        0xE045 => KeyCode::NumLock,
        0xE047 => KeyCode::Home,
        0xE048 => KeyCode::ArrowUp,
        0xE049 => KeyCode::PageUp,
        0xE04B => KeyCode::ArrowLeft,
        0xE04D => KeyCode::ArrowRight,
        0xE04F => KeyCode::End,
        0xE050 => KeyCode::ArrowDown,
        0xE051 => KeyCode::PageDown,
        0xE052 => KeyCode::Insert,
        0xE053 => KeyCode::Delete,
        0xE05B => KeyCode::SuperLeft,
        0xE05C => KeyCode::SuperRight,
        0xE05D => KeyCode::ContextMenu,
        _ => return None,
    })
}

/// The keysym for a key whose symbol does not depend on the keyboard layout.
///
/// The table itself lives in [`crate::keysym`], shared with the AppKit
/// backend: it is a pure function of the engine's own [`KeyCode`] with no
/// platform in it, and two copies of one fact were exactly the drift the
/// codebase's rules warn about. Re-exported here so `keys::named_keysym` keeps
/// reading the same at every call site.
pub use crate::keysym::named_keysym;

/// Whether a virtual key is down, in a `GetKeyboardState` snapshot.
///
/// The high bit is the held state and the low bit is the *toggle* state, which
/// is the distinction Caps Lock depends on: a Caps Lock that is on but not
/// physically held has bit 0 set and bit 7 clear.
const fn is_down(keys: &[u8; 256], key: usize) -> bool {
    keys[key] & 0x80 != 0
}

/// Whether a virtual key is latched on. See [`is_down`].
const fn is_toggled(keys: &[u8; 256], key: usize) -> bool {
    keys[key] & 0x01 != 0
}

/// The modifiers a `GetKeyboardState` snapshot describes.
///
/// # AltGr is a Control that was never pressed
///
/// On every layout that has an AltGr — which is most of Europe — Windows
/// synthesizes a **left Control down** alongside the right Alt, because that is
/// how the original IBM AltGr was encoded. Reported literally, a French user
/// typing `€` (AltGr+E) sends `Ctrl+Alt+E`, and every `Ctrl+E` shortcut in the
/// application fires while they are writing a price.
///
/// So the pair is recognized and unpicked: right Alt held **together with** left
/// Control is AltGr, which sets [`ALT_GR`](Modifiers::ALT_GR) and contributes
/// neither [`CTRL`](Modifiers::CTRL) nor [`ALT`](Modifiers::ALT). The test is
/// the pair rather than right-Alt alone because on a US layout right Alt really
/// is a plain Alt and sends no Control with it — and a US user's `RAlt+E` must
/// still be `Alt+E`.
///
/// Real `Ctrl+AltGr+E` — left Control genuinely held while AltGr is pressed — is
/// indistinguishable from AltGr alone at this layer, on Windows, by
/// construction. Every toolkit has this limitation and none of them can fix it;
/// it is recorded here rather than discovered.
#[must_use]
pub fn modifiers(keys: &[u8; 256]) -> Modifiers {
    let left_control = is_down(keys, value::VK_L_CONTROL);
    let right_control = is_down(keys, value::VK_R_CONTROL);
    let left_alt = is_down(keys, value::VK_L_MENU);
    let right_alt = is_down(keys, value::VK_R_MENU);
    let alt_gr = right_alt && left_control;

    let mut modifiers = Modifiers::empty();
    modifiers.set(
        Modifiers::SHIFT,
        is_down(keys, value::VK_L_SHIFT) || is_down(keys, value::VK_R_SHIFT),
    );
    modifiers.set(Modifiers::CTRL, right_control || (left_control && !alt_gr));
    modifiers.set(Modifiers::ALT, left_alt || (right_alt && !alt_gr));
    modifiers.set(
        Modifiers::SUPER,
        is_down(keys, value::VK_L_WIN) || is_down(keys, value::VK_R_WIN),
    );
    modifiers.set(Modifiers::ALT_GR, alt_gr);
    modifiers.set(Modifiers::CAPS_LOCK, is_toggled(keys, value::VK_CAPITAL));
    modifiers.set(Modifiers::NUM_LOCK, is_toggled(keys, value::VK_NUM_LOCK));
    modifiers
}

/// Reassembles the UTF-16 code units `WM_CHAR` delivers one at a time.
///
/// # Why this is a state machine and not a cast
///
/// `WM_CHAR`'s `wParam` is a UTF-16 **code unit**, not a character. A codepoint
/// above the BMP — an emoji, a rare CJK ideograph, anything a modern IME can
/// commit — arrives as *two* messages carrying a surrogate pair, and casting
/// either half on its own produces nothing valid: `char::from_u32` rejects the
/// surrogate range outright, so a backend that ignores this silently drops every
/// astral character rather than mangling it.
///
/// The pair is held across exactly one message. A high surrogate followed by
/// something that is not a low surrogate is a malformed stream — dropped, and
/// the new unit is then interpreted on its own, so one bad message costs one
/// character rather than desynchronising everything after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Utf16 {
    /// The high half of a surrogate pair, waiting for its low half.
    pending: Option<u16>,
}

/// The first UTF-16 high surrogate.
const HIGH_SURROGATE_START: u16 = 0xD800;
/// The first UTF-16 low surrogate, which is also one past the last high one.
const LOW_SURROGATE_START: u16 = 0xDC00;
/// One past the last UTF-16 surrogate.
const SURROGATE_END: u16 = 0xE000;
/// The first codepoint outside the BMP, which is what a pair encodes.
const ASTRAL_START: u32 = 0x1_0000;

impl Utf16 {
    /// Feeds one code unit, and yields a character when one is complete.
    ///
    /// Control characters are **not** filtered here — see [`is_text`], which is
    /// the separate question of whether a complete character is worth
    /// committing.
    pub fn push(&mut self, unit: u16) -> Option<char> {
        let high = self.pending.take();
        if let Some(high) = high
            && (LOW_SURROGATE_START..SURROGATE_END).contains(&unit)
        {
            let codepoint = ASTRAL_START
                + (((high - HIGH_SURROGATE_START) as u32) << 10)
                + (unit - LOW_SURROGATE_START) as u32;
            return char::from_u32(codepoint);
        }
        if (HIGH_SURROGATE_START..LOW_SURROGATE_START).contains(&unit) {
            self.pending = Some(unit);
            return None;
        }
        char::from_u32(unit as u32)
    }
}

/// Whether a committed character is text rather than a control code.
///
/// `WM_CHAR` is the system's translation of a key press, so it delivers `\u{8}`
/// for Backspace, `\r` for Enter and `\u{1b}` for Escape — every one of which
/// would be inserted verbatim by a text field that trusted the stream.
/// [`ShellEvent::TextCommit`](crate::ShellEvent::TextCommit) means *text*, and
/// this is the same filter the two Linux backends apply to what XKB hands them,
/// so the three agree about what typing Enter commits: nothing.
#[must_use]
pub fn is_text(character: char) -> bool {
    !character.is_control()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key message's `lParam` for a scan code, with the flags spelled out.
    const fn l_param(code: isize, extended: bool, previous: bool) -> isize {
        let mut packed = code << 16;
        if extended {
            packed |= EXTENDED_BIT;
        }
        if previous {
            packed |= PREVIOUS_STATE_BIT;
        }
        // Bits 0-15 are the repeat count, which this backend does not read;
        // a non-zero one here proves that.
        packed | 1
    }

    /// A `GetKeyboardState` snapshot with these virtual keys held.
    fn held(keys: &[usize]) -> [u8; 256] {
        let mut state = [0u8; 256];
        for key in keys {
            state[*key] = 0x80;
        }
        state
    }

    #[test]
    fn the_extended_bit_is_folded_into_the_scan_code_rather_than_dropped() {
        // The whole reason this module reads bit 24: set 1 gives Enter and the
        // keypad's Enter the same byte, and the prefix is the only difference.
        assert_eq!(scancode(l_param(0x1C, false, false)), 0x001C);
        assert_eq!(scancode(l_param(0x1C, true, false)), 0xE01C);
        assert_eq!(key_code(0x001C), Some(KeyCode::Enter));
        assert_eq!(key_code(0xE01C), Some(KeyCode::NumpadEnter));

        // And the three other pairs a dropped bit would collapse.
        assert_eq!(key_code(0x001D), Some(KeyCode::ControlLeft));
        assert_eq!(key_code(0xE01D), Some(KeyCode::ControlRight));
        assert_eq!(key_code(0x0038), Some(KeyCode::AltLeft));
        assert_eq!(key_code(0xE038), Some(KeyCode::AltRight));
        assert_eq!(key_code(0x0047), Some(KeyCode::Numpad7));
        assert_eq!(key_code(0xE047), Some(KeyCode::Home));

        // The high byte of the scan-code field is masked off: `lParam` carries
        // a repeat count below it and context bits above it.
        assert_eq!(scancode(l_param(0x39, false, true)), 0x0039);
        assert_eq!(scancode(-1), 0xE0FF, "every bit set is still one byte");
    }

    #[test]
    fn pause_is_the_bare_forty_five_and_num_lock_is_the_extended_one() {
        // Backwards in a lot of published tables, and the failure is silent:
        // a game binds Pause and the player's Num Lock triggers it.
        assert_eq!(key_code(0x0045), Some(KeyCode::Pause));
        assert_eq!(key_code(0xE045), Some(KeyCode::NumLock));
    }

    #[test]
    fn the_previous_state_bit_is_what_marks_an_auto_repeat() {
        // Held keys repeat at the system rate; a jump button that forwards them
        // fires thirty times a second.
        assert!(!was_already_down(l_param(0x11, false, false)));
        assert!(was_already_down(l_param(0x11, false, true)));
    }

    #[test]
    fn wasd_and_the_gameplay_keys_map_to_their_set_one_codes() {
        // These are the numbers a `WM_KEYDOWN` actually carries, and they are
        // *not* the evdev ones — which is the whole reason this table is not
        // `linux::keymap`.
        assert_eq!(key_code(0x0011), Some(KeyCode::KeyW));
        assert_eq!(key_code(0x001E), Some(KeyCode::KeyA));
        assert_eq!(key_code(0x001F), Some(KeyCode::KeyS));
        assert_eq!(key_code(0x0020), Some(KeyCode::KeyD));
        assert_eq!(key_code(0x0039), Some(KeyCode::Space));
        assert_eq!(key_code(0x0001), Some(KeyCode::Escape));
        assert_eq!(key_code(0xE05B), Some(KeyCode::SuperLeft));
    }

    /// Where this table and the evdev one agree, and where they stop.
    ///
    /// Linux-only because [`linux::keymap`](crate::linux::keymap) is compiled
    /// there and only there — which is the whole point: the resemblance below
    /// `0x54` is what makes sharing one table tempting, and the divergence above
    /// it is what makes sharing wrong. Neither half is visible from one side.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_two_numberings_coincide_for_the_letters_and_for_nothing_above_them() {
        assert_eq!(crate::linux::keymap::key_code(17), Some(KeyCode::KeyW));
        assert_eq!(key_code(17), Some(KeyCode::KeyW), "0x11 is W in both");
        assert_eq!(crate::linux::keymap::key_code(30), Some(KeyCode::KeyA));
        assert_eq!(key_code(30), Some(KeyCode::KeyA), "0x1E is A in both");

        assert_eq!(
            crate::linux::keymap::key_code(103),
            Some(KeyCode::ArrowUp),
            "evdev continues flat"
        );
        assert_eq!(key_code(103), None, "set 1 assigns nothing at 0x67");
        assert_eq!(key_code(0xE048), Some(KeyCode::ArrowUp), "it is prefixed");
        assert_eq!(
            crate::linux::keymap::key_code(96),
            Some(KeyCode::NumpadEnter)
        );
        assert_eq!(key_code(96), None);
        assert_eq!(key_code(0xE01C), Some(KeyCode::NumpadEnter));
    }

    /// Every scan code a `WM_KEYDOWN` can carry: one byte, and one byte with
    /// the `E0` prefix folded in.
    fn all_scancodes() -> impl Iterator<Item = u32> {
        (0..=0xFFu32).chain((0..=0xFFu32).map(|code| EXTENDED_PREFIX | code))
    }

    #[test]
    fn every_named_key_is_reachable_from_some_scan_code() {
        // A key in the vocabulary that no scan code produces is a key nobody
        // can bind on Windows — invisible in every other way.
        let reachable: Vec<KeyCode> = all_scancodes().filter_map(key_code).collect();
        let mut missing: Vec<&'static str> = Vec::new();
        for key in KeyCode::ALL {
            if !reachable.contains(key) {
                missing.push(key.as_str());
            }
        }
        assert!(missing.is_empty(), "unreachable KeyCodes: {missing:?}");
    }

    #[test]
    fn print_screen_is_the_only_key_two_scan_codes_name() {
        // The other direction: a duplicated arm silently shadows, and the
        // shadowed key simply never fires. Print Screen is the one real
        // duplicate — Alt+Print Screen sends the unprefixed `0x54` — so it is
        // named here rather than allowed by a loose assertion.
        let mut seen: Vec<KeyCode> = all_scancodes().filter_map(key_code).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            count - seen.len(),
            1,
            "exactly one scan code pair may share a KeyCode"
        );
        assert_eq!(
            seen.len(),
            KeyCode::ALL.len(),
            "the table is the vocabulary"
        );
        assert_eq!(key_code(0x0054), Some(KeyCode::PrintScreen));
        assert_eq!(key_code(0xE037), Some(KeyCode::PrintScreen));
    }

    #[test]
    fn unnamed_keys_degrade_to_none_rather_than_to_a_wrong_key() {
        // Holes in set 1, the JIS neighbours this engine does not name, and the
        // extended codes the media block uses.
        for code in [
            0x0000, 0x0055, 0x0059, 0x0070, 0x0079, 0xE010, 0xE022, 0xE06F,
        ] {
            assert_eq!(key_code(code), None, "scan code {code:#06x}");
        }
    }

    #[test]
    fn alt_gr_is_reported_as_itself_and_not_as_control_plus_alt() {
        // The bug this exists to not have: a French user typing € would
        // otherwise fire every Ctrl+E shortcut in the application.
        let alt_gr = modifiers(&held(&[value::VK_R_MENU, value::VK_L_CONTROL]));
        assert!(alt_gr.contains(Modifiers::ALT_GR));
        assert!(!alt_gr.contains(Modifiers::CTRL), "{alt_gr:?}");
        assert!(!alt_gr.contains(Modifiers::ALT), "{alt_gr:?}");
        assert_eq!(alt_gr.chord(), Modifiers::empty());

        // A US keyboard's right Alt sends no Control with it, so it stays Alt.
        let plain = modifiers(&held(&[value::VK_R_MENU]));
        assert_eq!(plain, Modifiers::ALT);
        // And a real Ctrl+Alt — left Alt, not AltGr — is untouched.
        let real = modifiers(&held(&[value::VK_L_CONTROL, value::VK_L_MENU]));
        assert_eq!(real, Modifiers::CTRL | Modifiers::ALT);
        // Right Control with AltGr is still a Control: it is not the key
        // Windows synthesizes.
        let both = modifiers(&held(&[
            value::VK_R_MENU,
            value::VK_L_CONTROL,
            value::VK_R_CONTROL,
        ]));
        assert!(both.contains(Modifiers::ALT_GR | Modifiers::CTRL));
    }

    #[test]
    fn shift_and_super_come_from_either_side_and_the_locks_from_the_toggle_bit() {
        assert_eq!(modifiers(&held(&[value::VK_L_SHIFT])), Modifiers::SHIFT);
        assert_eq!(modifiers(&held(&[value::VK_R_SHIFT])), Modifiers::SHIFT);
        assert_eq!(modifiers(&held(&[value::VK_L_WIN])), Modifiers::SUPER);
        assert_eq!(modifiers(&held(&[value::VK_R_WIN])), Modifiers::SUPER);
        assert_eq!(modifiers(&[0u8; 256]), Modifiers::empty());

        // Caps Lock latched on is bit 0, not bit 7 — a Caps Lock that is *on*
        // is not a Caps Lock that is held, and reading the wrong bit reports
        // the modifier only during the keystroke that toggles it.
        let mut state = [0u8; 256];
        state[value::VK_CAPITAL] = 0x01;
        state[value::VK_NUM_LOCK] = 0x01;
        assert_eq!(
            modifiers(&state),
            Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK
        );
        // Held but not latched: the key is on its way down and the lock is off.
        let mut pressing = [0u8; 256];
        pressing[value::VK_CAPITAL] = 0x80;
        assert_eq!(modifiers(&pressing), Modifiers::empty());
    }

    #[test]
    fn a_surrogate_pair_becomes_one_character_and_a_lone_half_becomes_none() {
        // The astral case: `WM_CHAR` delivers 🎮 as two messages, and casting
        // either half alone yields nothing valid at all.
        let mut text = Utf16::default();
        assert_eq!(text.push(0xD83C), None, "the high half commits nothing");
        assert_eq!(text.push(0xDFAE), Some('🎮'));
        assert_eq!(text, Utf16::default(), "and the pair is not held on to");

        // A BMP character is one message.
        assert_eq!(text.push(0x3042), Some('あ'));
        assert_eq!(text.push(u16::from(b'q')), Some('q'));

        // A high surrogate followed by something that is not a low one costs
        // one character, not the rest of the stream.
        assert_eq!(text.push(0xD83C), None);
        assert_eq!(text.push(u16::from(b'a')), Some('a'));
        // A lone low surrogate is not a character and is not held.
        assert_eq!(text.push(0xDFAE), None);
        assert_eq!(text.push(u16::from(b'b')), Some('b'));
    }

    #[test]
    fn control_characters_are_not_text() {
        // `WM_CHAR` really does deliver these, and a text field that trusted
        // the stream would insert a literal escape when the user pressed Esc.
        for control in ['\u{8}', '\r', '\n', '\t', '\u{1b}', '\u{7f}'] {
            assert!(!is_text(control), "{control:?}");
        }
        for text in ['a', ' ', 'é', '🎮', '日'] {
            assert!(is_text(text), "{text:?}");
        }
    }
}
