//! Steam's on-screen keyboards (`ISteamUtils`): what a Steam Deck, or Big
//! Picture on a desktop, types with when there is no physical keyboard.
//!
//! Two of them, which deliver text differently:
//!
//! - **The full-screen keyboard** ([`Utils::show_text_input`]) takes the
//!   whole screen, and hands the accepted text back once, when it closes, as
//!   [`SteamEvent::TextInputDismissed`] — committed text, the same thing a
//!   physical keyboard's `ShellEvent::TextCommit` carries, so a text field
//!   takes it the same way and cannot tell the two apart.
//! - **The floating keyboard** ([`Utils::show_floating_keyboard`]) sits
//!   beside the field it was opened for and types into the window as the
//!   operating system's own key events, so its text arrives through the shell
//!   like a physical keyboard's; [`SteamEvent::FloatingKeyboardDismissed`]
//!   says only that it closed.

use core::ffi::c_char;

use crate::{Steam, SteamError, SteamEvent, error::c_string, utils::Utils};

/// The most accepted text [`SteamEvent::TextInputDismissed`] reads. Far past
/// anything typed on an on-screen keyboard; a length past it is a library
/// that broke its own contract, and is counted rather than allocated.
const MAX_ENTERED_TEXT_BYTES: usize = 64 * 1024;

/// What the full-screen keyboard shows (`EGamepadTextInputMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextInputMode {
    /// Plain text (`k_EGamepadTextInputModeNormal`).
    Normal,
    /// Hidden as it is typed (`k_EGamepadTextInputModePassword`).
    Password,
}

/// How many lines the full-screen keyboard takes
/// (`EGamepadTextInputLineMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextInputLines {
    /// One line (`k_EGamepadTextInputLineModeSingleLine`).
    Single,
    /// Several (`k_EGamepadTextInputLineModeMultipleLines`).
    Multiple,
}

/// A request for the full-screen keyboard; see [`Utils::show_text_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextInputRequest<'a> {
    /// Plain or hidden.
    pub mode: TextInputMode,
    /// One line or several.
    pub lines: TextInputLines,
    /// What the field is for, shown above it.
    pub description: &'a str,
    /// The most characters the player may type.
    pub max_chars: u32,
    /// The text the field starts with.
    pub existing: &'a str,
}

/// The floating keyboard's layout, and what Enter does
/// (`EFloatingGamepadTextInputMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatingKeyboardMode {
    /// Enter closes it (`k_EFloatingGamepadTextInputModeModeSingleLine`).
    SingleLine,
    /// The player closes it themselves
    /// (`k_EFloatingGamepadTextInputModeModeMultipleLines`).
    MultipleLines,
    /// An email layout; Enter closes it
    /// (`k_EFloatingGamepadTextInputModeModeEmail`).
    Email,
    /// A numeric layout; Enter closes it
    /// (`k_EFloatingGamepadTextInputModeModeNumeric`).
    Numeric,
}

/// Where the field the floating keyboard types into is, in pixels from the
/// game window's top-left corner, so the keyboard can avoid covering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextField {
    /// The field's left edge.
    pub x: i32,
    /// The field's top edge.
    pub y: i32,
    /// Its width.
    pub width: i32,
    /// Its height.
    pub height: i32,
}

impl Utils<'_> {
    /// Opens the full-screen on-screen keyboard (`ShowGamepadTextInput`).
    /// The accepted text arrives as [`SteamEvent::TextInputDismissed`] after
    /// a later pump.
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] for a description or starting text holding
    /// a NUL; [`SteamError::Refused`] when Steam will not show it — outside
    /// Big Picture or a Steam Deck, for one.
    pub fn show_text_input(&self, request: &TextInputRequest<'_>) -> Result<(), SteamError> {
        let description = c_string(request.description, "description", usize::MAX)?;
        let existing = c_string(request.existing, "existing", usize::MAX)?;
        let mode = match request.mode {
            TextInputMode::Normal => 0,
            TextInputMode::Password => 1,
        };
        let lines = match request.lines {
            TextInputLines::Single => 0,
            TextInputLines::Multiple => 1,
        };
        let client = &self.steam.client;
        // SAFETY: `client.utils` is the non-null interface init resolved;
        // `Utils` borrows the `!Send` `Steam`, so this is the pump thread; both
        // strings are NUL-terminated and outlive the call.
        let shown = unsafe {
            (client.lib.fns.utils.show_gamepad_text_input)(
                client.utils,
                mode,
                lines,
                description.as_ptr(),
                request.max_chars,
                existing.as_ptr(),
            )
        };
        shown
            .then_some(())
            .ok_or(SteamError::Refused("ShowGamepadTextInput"))
    }

    /// Closes the full-screen keyboard if it is open
    /// (`DismissGamepadTextInput`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam answers `false`.
    pub fn dismiss_text_input(&self) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `show_text_input`.
        let dismissed = unsafe { (client.lib.fns.utils.dismiss_gamepad_text_input)(client.utils) };
        dismissed
            .then_some(())
            .ok_or(SteamError::Refused("DismissGamepadTextInput"))
    }

    /// Opens the floating keyboard beside `field`
    /// (`ShowFloatingGamepadTextInput`). It types into the window as key
    /// events; [`SteamEvent::FloatingKeyboardDismissed`] says when it closes.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not show it.
    pub fn show_floating_keyboard(
        &self,
        mode: FloatingKeyboardMode,
        field: TextField,
    ) -> Result<(), SteamError> {
        let mode = match mode {
            FloatingKeyboardMode::SingleLine => 0,
            FloatingKeyboardMode::MultipleLines => 1,
            FloatingKeyboardMode::Email => 2,
            FloatingKeyboardMode::Numeric => 3,
        };
        let client = &self.steam.client;
        // SAFETY: as in `show_text_input`; every argument is by value.
        let shown = unsafe {
            (client.lib.fns.utils.show_floating_gamepad_text_input)(
                client.utils,
                mode,
                field.x,
                field.y,
                field.width,
                field.height,
            )
        };
        shown
            .then_some(())
            .ok_or(SteamError::Refused("ShowFloatingGamepadTextInput"))
    }

    /// Closes the floating keyboard if it is open
    /// (`DismissFloatingGamepadTextInput`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam answers `false`.
    pub fn dismiss_floating_keyboard(&self) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `show_text_input`.
        let dismissed =
            unsafe { (client.lib.fns.utils.dismiss_floating_gamepad_text_input)(client.utils) };
        dismissed
            .then_some(())
            .ok_or(SteamError::Refused("DismissFloatingGamepadTextInput"))
    }
}

impl Steam {
    /// Handles `GamepadTextInputDismissed_t` for this game: reads the
    /// accepted text, sized by `GetEnteredGamepadTextLength`, and queues
    /// [`SteamEvent::TextInputDismissed`]. Another app's is counted unknown.
    pub(crate) fn text_input_dismissed(&mut self, submitted: bool, app: u32) {
        if app != self.app.0 {
            self.diagnostics.unknown += 1;
            return;
        }
        let text = if submitted { self.entered_text() } else { None };
        self.queue
            .push_back(SteamEvent::TextInputDismissed { text });
    }

    /// The text the player accepted, or `None` — counted in
    /// `decode_mismatches` — when Steam reports a length it cannot have or
    /// will not hand the text over.
    fn entered_text(&mut self) -> Option<String> {
        let client = &self.client;
        let utils = &client.lib.fns.utils;
        // SAFETY: `client.utils` is live, and this is the pump thread.
        let length = unsafe { (utils.get_entered_gamepad_text_length)(client.utils) };
        let Some(length_bytes) = usize::try_from(length)
            .ok()
            .filter(|&bytes| bytes <= MAX_ENTERED_TEXT_BYTES)
        else {
            self.diagnostics.decode_mismatches += 1;
            return None;
        };
        // One more than the length, so the text fits with its NUL whether or
        // not Steam's length counts one; no overflow, being at most
        // `MAX_ENTERED_TEXT_BYTES + 1`.
        let size = length + 1;
        let mut buffer = vec![0_u8; length_bytes + 1];
        // SAFETY: as above; `buffer` is `size` writable bytes for the call.
        let read = unsafe {
            (utils.get_entered_gamepad_text_input)(
                client.utils,
                buffer.as_mut_ptr().cast::<c_char>(),
                size,
            )
        };
        if !read {
            self.diagnostics.decode_mismatches += 1;
            return None;
        }
        let (text, lossy) = crate::callbacks::fixed_string(&buffer);
        if lossy {
            self.lossy_strings.set(self.lossy_strings.get() + 1);
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests;
