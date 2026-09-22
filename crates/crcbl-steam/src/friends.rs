//! `ISteamFriends`: the player's social identity.
//!
//! Only the local persona name so far; lobbies, invites, rich presence and the
//! friends list arrive with `docs/plan/42-steam.md`'s slices 3a and 3b.

use crate::Steam;

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

impl Friends<'_> {
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
