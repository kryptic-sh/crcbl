//! `ISteamFriends`: the player's social identity.
//!
//! The local persona name, the overlay's invite dialog, and (in
//! `crate::presence`) rich presence and game invites; the friends list and
//! avatars arrive with `docs/plan/42-steam.md`'s slice 3b.

use crate::{LobbyId, Steam};

/// `ISteamFriends`, borrowed from a [`Steam`]; from [`Steam::friends`].
#[derive(Debug, Clone, Copy)]
pub struct Friends<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// The player's persona and, later, their friends.
    #[must_use]
    pub fn friends(&self) -> Friends<'_> {
        Friends { steam: self }
    }
}

impl<'a> Friends<'a> {
    /// The `Steam` this was borrowed from.
    pub(crate) const fn steam(&self) -> &'a Steam {
        self.steam
    }

    /// Opens the overlay's invite dialog for `lobby`
    /// (`ActivateGameOverlayInviteDialog`): the friends the player picks are
    /// invited, and accepting delivers
    /// [`SteamEvent::LobbyJoinRequested`](crate::SteamEvent::LobbyJoinRequested)
    /// to them. Does nothing visible when the overlay is not injected — see
    /// [`Utils::overlay_enabled`](crate::Utils::overlay_enabled).
    pub fn open_invite_dialog(&self, lobby: LobbyId) {
        let client = &self.steam.client;
        // SAFETY: `client.friends` is the non-null interface init resolved.
        unsafe {
            (client.lib.fns.friends.activate_game_overlay_invite_dialog)(client.friends, lobby.0)
        };
    }

    /// The local player's display name (`ISteamFriends::GetPersonaName`) —
    /// what friends see, not the account name, and free to change between
    /// sessions; key nothing on it. Use [`User::steam_id`](crate::User::steam_id)
    /// for identity.
    #[must_use]
    pub fn persona_name(&self) -> String {
        let client = &self.steam.client;
        // SAFETY: `client.friends` is the non-null `ISteamFriends` init
        // resolved, Steam stays initialised while `client` lives, and the
        // returned string is copied before any other Steam call.
        unsafe {
            let name = (client.lib.fns.friends.get_persona_name)(client.friends);
            self.steam.copy_string(name)
        }
    }
}
