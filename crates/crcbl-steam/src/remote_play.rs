//! `ISteamRemotePlay`: the Remote Play sessions streaming this game, and
//! Remote Play Together invites.
//!
//! A session is a device streaming the game from this machine — the player's
//! own phone or TV, or a friend joined through Remote Play Together.
//! [`SteamEvent::RemotePlayConnected`](crate::SteamEvent::RemotePlayConnected)
//! and `…Disconnected` say when one comes and goes; [`RemotePlay`] reads who
//! and what it is. The session avatars, the direct-input calls and the mouse
//! cursor calls are not bound: nothing asks for them yet.

use crate::{Steam, SteamError, SteamId};

/// A Remote Play session (`RemotePlaySessionID_t`); zero is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemotePlaySession(pub u32);

/// What a session's device is (`ESteamDeviceFormFactor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormFactor {
    /// Unknown (`k_ESteamDeviceFormFactorUnknown`).
    Unknown,
    /// A phone (`k_ESteamDeviceFormFactorPhone`).
    Phone,
    /// A tablet (`k_ESteamDeviceFormFactorTablet`).
    Tablet,
    /// A computer (`k_ESteamDeviceFormFactorComputer`).
    Computer,
    /// A TV (`k_ESteamDeviceFormFactorTV`).
    Tv,
    /// A VR headset (`k_ESteamDeviceFormFactorVRHeadset`).
    VrHeadset,
    /// A value this crate does not name.
    Other(i32),
}

impl FormFactor {
    /// Maps an `ESteamDeviceFormFactor`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Unknown,
            1 => Self::Phone,
            2 => Self::Tablet,
            3 => Self::Computer,
            4 => Self::Tv,
            5 => Self::VrHeadset,
            other => Self::Other(other),
        }
    }
}

/// `ISteamRemotePlay`, borrowed from a [`Steam`]; from [`Steam::remote_play`].
#[derive(Debug, Clone, Copy)]
pub struct RemotePlay<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// Remote Play sessions.
    #[must_use]
    pub fn remote_play(&self) -> RemotePlay<'_> {
        RemotePlay { steam: self }
    }
}

impl RemotePlay<'_> {
    /// Every session connected now (`GetSessionCount`, `GetSessionID`).
    #[must_use]
    pub fn sessions(&self) -> Vec<RemotePlaySession> {
        let client = &self.steam.client;
        let fns = &client.lib.fns.remote_play;
        // SAFETY: `client.remote_play` is the non-null interface init
        // resolved; `RemotePlay` borrows the `!Send` `Steam`, so this is the
        // pump thread.
        let count = unsafe { (fns.get_session_count)(client.remote_play) };
        (0..i32::try_from(count).unwrap_or(i32::MAX))
            // SAFETY: as above; an index past the end answers zero.
            .map(|index| unsafe { (fns.get_session_id)(client.remote_play, index) })
            .filter(|&id| id != 0)
            .map(RemotePlaySession)
            .collect()
    }

    /// Whether `session` joined through a Remote Play Together invite
    /// (`BSessionRemotePlayTogether`).
    #[must_use]
    pub fn together(&self, session: RemotePlaySession) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        unsafe {
            (client.lib.fns.remote_play.session_remote_play_together)(client.remote_play, session.0)
        }
    }

    /// Who is at the other end of `session` (`GetSessionSteamID`).
    #[must_use]
    pub fn user(&self, session: RemotePlaySession) -> SteamId {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        SteamId(unsafe {
            (client.lib.fns.remote_play.get_session_steam_id)(client.remote_play, session.0)
        })
    }

    /// The guest id of a Remote Play Together guest, or `None` for a session
    /// that is not one (`GetSessionGuestID`).
    #[must_use]
    pub fn guest(&self, session: RemotePlaySession) -> Option<u32> {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        let guest = unsafe {
            (client.lib.fns.remote_play.get_session_guest_id)(client.remote_play, session.0)
        };
        (guest != 0).then_some(guest)
    }

    /// The name of `session`'s device, or `None` for a session that is not
    /// connected (`GetSessionClientName`, which answers null then).
    #[must_use]
    pub fn client_name(&self, session: RemotePlaySession) -> Option<String> {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        let name = unsafe {
            (client.lib.fns.remote_play.get_session_client_name)(client.remote_play, session.0)
        };
        // Null is this call's documented "no such session", not a lost
        // string, so it is not counted lossy.
        (!name.is_null()).then(|| {
            // SAFETY: straight out of the call, before any other Steam call.
            unsafe { self.steam.copy_string(name) }
        })
    }

    /// What `session`'s device is (`GetSessionClientFormFactor`).
    #[must_use]
    pub fn form_factor(&self, session: RemotePlaySession) -> FormFactor {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        FormFactor::from_raw(unsafe {
            (client.lib.fns.remote_play.get_session_client_form_factor)(
                client.remote_play,
                session.0,
            )
        })
    }

    /// `session`'s screen in pixels, or `None` when Steam does not know it
    /// (`BGetSessionClientResolution`: false, or zero by zero).
    #[must_use]
    pub fn resolution(&self, session: RemotePlaySession) -> Option<(u32, u32)> {
        let client = &self.steam.client;
        let (mut width, mut height) = (0_i32, 0_i32);
        // SAFETY: as in `sessions`; both out-parameters are writable.
        let known = unsafe {
            (client.lib.fns.remote_play.get_session_client_resolution)(
                client.remote_play,
                session.0,
                &raw mut width,
                &raw mut height,
            )
        };
        let (width, height) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
        (known && width > 0 && height > 0).then_some((width, height))
    }

    /// Invites `friend` to Remote Play Together
    /// (`BSendRemotePlayTogetherInvite`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam cannot send it — the game not
    /// configured for Remote Play Together, for one.
    pub fn invite(&self, friend: SteamId) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        let sent = unsafe {
            (client.lib.fns.remote_play.send_remote_play_together_invite)(
                client.remote_play,
                friend.0,
            )
        };
        sent.then_some(())
            .ok_or(SteamError::Refused("BSendRemotePlayTogetherInvite"))
    }

    /// Opens the overlay's Remote Play Together panel
    /// (`ShowRemotePlayTogetherUI`).
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when the game is not configured for Remote
    /// Play Together.
    pub fn show_together_panel(&self) -> Result<(), SteamError> {
        let client = &self.steam.client;
        // SAFETY: as in `sessions`.
        let shown = unsafe {
            (client.lib.fns.remote_play.show_remote_play_together_ui)(client.remote_play)
        };
        shown
            .then_some(())
            .ok_or(SteamError::Refused("ShowRemotePlayTogetherUI"))
    }
}

#[cfg(test)]
mod tests;
