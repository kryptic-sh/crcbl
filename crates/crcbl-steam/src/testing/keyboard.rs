//! The fake on-screen keyboards (`ISteamUtils`): what each call was given,
//! and the text a dismissal hands back.

use std::ffi::{CStr, c_char};

use super::script;
use crate::ffi::ISteamUtils;

/// What the fake keyboards answer, and what they have seen.
#[derive(Debug, Default)]
pub(crate) struct FakeKeyboard {
    /// Every `(mode, lines, description, max chars, existing)`
    /// `ShowGamepadTextInput` was given.
    pub(crate) shown: Vec<(i32, i32, String, u32, String)>,
    /// Every `(mode, x, y, width, height)` `ShowFloatingGamepadTextInput` was
    /// given.
    pub(crate) floating: Vec<(i32, i32, i32, i32, i32)>,
    /// Every dismiss call, by name.
    pub(crate) dismissed: Vec<&'static str>,
    /// The accepted text, without a NUL.
    pub(crate) text: Vec<u8>,
    /// What `GetEnteredGamepadTextLength` answers instead of the text's
    /// length, when set.
    pub(crate) length: Option<u32>,
    /// `GetEnteredGamepadTextInput` answers `false`.
    pub(crate) refuse_read: bool,
    /// Every buffer size `GetEnteredGamepadTextInput` was offered.
    pub(crate) offered: Vec<u32>,
}

/// A C string argument, copied.
fn text(ptr: *const c_char) -> String {
    // SAFETY: every caller passes a NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

pub(super) unsafe extern "C" fn fake_show_gamepad_text_input(
    _: *mut ISteamUtils,
    mode: i32,
    lines: i32,
    description: *const c_char,
    max_chars: u32,
    existing: *const c_char,
) -> bool {
    let call = (mode, lines, text(description), max_chars, text(existing));
    script(|s| {
        s.keyboard.shown.push(call);
        !s.refuse
    })
}

pub(super) unsafe extern "C" fn fake_get_entered_gamepad_text_length(_: *mut ISteamUtils) -> u32 {
    script(|s| {
        s.keyboard
            .length
            .unwrap_or_else(|| u32::try_from(s.keyboard.text.len()).unwrap())
    })
}

/// Writes as much of the text as fits before a NUL in `capacity` bytes, as a
/// C string copy does.
pub(super) unsafe extern "C" fn fake_get_entered_gamepad_text_input(
    _: *mut ISteamUtils,
    out: *mut c_char,
    capacity: u32,
) -> bool {
    script(|s| {
        s.keyboard.offered.push(capacity);
        if s.keyboard.refuse_read {
            return false;
        }
        let Some(room) = usize::try_from(capacity).unwrap().checked_sub(1) else {
            return false;
        };
        let text = &s.keyboard.text;
        let n = text.len().min(room);
        // SAFETY: the caller passes `capacity` writable bytes, and `n + 1` is
        // at most that.
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), n);
            out.add(n).write(0);
        }
        true
    })
}

pub(super) unsafe extern "C" fn fake_dismiss_gamepad_text_input(_: *mut ISteamUtils) -> bool {
    script(|s| {
        s.keyboard.dismissed.push("DismissGamepadTextInput");
        !s.refuse
    })
}

pub(super) unsafe extern "C" fn fake_show_floating_gamepad_text_input(
    _: *mut ISteamUtils,
    mode: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> bool {
    script(|s| {
        s.keyboard.floating.push((mode, x, y, width, height));
        !s.refuse
    })
}

pub(super) unsafe extern "C" fn fake_dismiss_floating_gamepad_text_input(
    _: *mut ISteamUtils,
) -> bool {
    script(|s| {
        s.keyboard.dismissed.push("DismissFloatingGamepadTextInput");
        !s.refuse
    })
}
