//! `ISteamScreenshots`: the player's Steam screenshot library.
//!
//! By default the Steam overlay takes a screenshot when the player presses its
//! key. A game that would rather supply its own image — the frame without the
//! debug overlay, or at a resolution of its choosing — [`Screenshots::hook`]s
//! them: Steam then sends
//! [`SteamEvent::ScreenshotRequested`](crate::SteamEvent::ScreenshotRequested)
//! instead, and the game [`write`](Screenshots::write)s the pixels it
//! captured.
//! [`SteamEvent::ScreenshotReady`](crate::SteamEvent::ScreenshotReady) follows
//! either way, after which the screenshot can be tagged.
//!
//! The engine has no capture of a running game's frame yet (the offscreen
//! readback in `crcbl::screenshot` renders a scene of its own), so where the
//! pixels come from is the game's; `write` takes packed 8-bit RGB, which is
//! what Steam takes.

use crate::{Steam, SteamError, SteamId, error::c_string};

/// A screenshot in the library, good for this game process (`ScreenshotHandle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenshotId(pub u32);

/// `ISteamScreenshots`, borrowed from a [`Steam`]; from [`Steam::screenshots`].
#[derive(Debug, Clone, Copy)]
pub struct Screenshots<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// The screenshot library.
    #[must_use]
    pub fn screenshots(&self) -> Screenshots<'_> {
        Screenshots { steam: self }
    }
}

impl Screenshots<'_> {
    /// Takes a screenshot now (`TriggerScreenshot`): the overlay's own, or —
    /// while hooked — a
    /// [`SteamEvent::ScreenshotRequested`](crate::SteamEvent::ScreenshotRequested)
    /// for the game.
    pub fn trigger(&self) {
        let client = &self.steam.client;
        // SAFETY: `client.screenshots` is the non-null interface init
        // resolved; `Screenshots` borrows the `!Send` `Steam`, so this is the
        // pump thread.
        unsafe { (client.lib.fns.screenshots.trigger_screenshot)(client.screenshots) };
    }

    /// Hooks screenshots (`hook`) or hands them back to the overlay
    /// (`HookScreenshots`) — see the module docs.
    pub fn hook(&self, hook: bool) {
        let client = &self.steam.client;
        // SAFETY: as in `trigger`.
        unsafe { (client.lib.fns.screenshots.hook_screenshots)(client.screenshots, hook) };
    }

    /// Whether the game hooks screenshots (`IsScreenshotsHooked`).
    #[must_use]
    pub fn hooked(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `trigger`.
        unsafe { (client.lib.fns.screenshots.is_screenshots_hooked)(client.screenshots) }
    }

    /// Adds `rgb` — `width × height` pixels of packed 8-bit red, green and
    /// blue, top row first — to the library (`WriteScreenshot`).
    ///
    /// The pixels are copied before the call: Steam takes them through a
    /// mutable pointer, and a borrowed slice may not be written to.
    ///
    /// # Errors
    ///
    /// [`SteamError::OutOfRange`] for a zero or oversized dimension, or a
    /// buffer whose length is not `3 × width × height` — checked before the
    /// call, so Steam never reads past the pixels; [`SteamError::Refused`]
    /// when Steam answers `INVALID_SCREENSHOT_HANDLE`.
    pub fn write(&self, rgb: &[u8], width: u32, height: u32) -> Result<ScreenshotId, SteamError> {
        let (Ok(w), Ok(h)) = (i32::try_from(width), i32::try_from(height)) else {
            return Err(SteamError::OutOfRange("screenshot size"));
        };
        let expected = u32::try_from(rgb.len())
            .ok()
            .filter(|_| width > 0 && height > 0)
            .filter(|&len| {
                width
                    .checked_mul(height)
                    .and_then(|pixels| pixels.checked_mul(3))
                    == Some(len)
            });
        let Some(size) = expected else {
            return Err(SteamError::OutOfRange("screenshot pixels"));
        };
        let mut pixels = rgb.to_vec();
        let client = &self.steam.client;
        // SAFETY: as in `trigger`; `pixels` is `size` bytes, exactly
        // `3 × width × height`, writable and alive for the call.
        let handle = unsafe {
            (client.lib.fns.screenshots.write_screenshot)(
                client.screenshots,
                pixels.as_mut_ptr().cast(),
                size,
                w,
                h,
            )
        };
        if handle == 0 {
            return Err(SteamError::Refused("WriteScreenshot"));
        }
        Ok(ScreenshotId(handle))
    }

    /// Names where a screenshot was taken, such as the map
    /// (`SetLocation`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] for a location holding a NUL;
    /// [`SteamError::Refused`] when Steam answers `false`.
    pub fn set_location(&self, screenshot: ScreenshotId, location: &str) -> Result<(), SteamError> {
        let location = c_string(location, "location", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: as in `trigger`; `location` is NUL-terminated for the call.
        let set = unsafe {
            (client.lib.fns.screenshots.set_location)(
                client.screenshots,
                screenshot.0,
                location.as_ptr(),
            )
        };
        set.then_some(()).ok_or(SteamError::Refused("SetLocation"))
    }

    /// Tags `user` as in a screenshot (`TagUser`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam answers `false` — past
    /// `k_nScreenshotMaxTaggedUsers`, for one.
    pub fn tag_user(&self, screenshot: ScreenshotId, user: SteamId) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `trigger`.
        let tagged = unsafe {
            (client.lib.fns.screenshots.tag_user)(client.screenshots, screenshot.0, user.0)
        };
        tagged.then_some(()).ok_or(SteamError::Refused("TagUser"))
    }
}

#[cfg(test)]
mod tests;
