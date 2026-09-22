//! `ISteamUser`: who is playing.

use crate::Steam;

/// A Steam account id — `CSteamID` as its 64-bit value. Stable for an account
/// across sessions and machines, so a game can key a profile or a participant
/// on it. `0` is not a valid account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SteamId(pub u64);

/// `ISteamUser`, borrowed from a [`Steam`]; from [`Steam::user`].
#[derive(Debug, Clone, Copy)]
pub struct User<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// The local user's account.
    #[must_use]
    pub fn user(&self) -> User<'_> {
        User { steam: self }
    }
}

impl User<'_> {
    /// The local user's [`SteamId`] (`ISteamUser::GetSteamID`).
    #[must_use]
    pub fn steam_id(&self) -> SteamId {
        let client = &self.steam.client;
        // SAFETY: `client.user` is the non-null `ISteamUser` init resolved, and
        // Steam stays initialised while `client` lives.
        SteamId(unsafe { (client.lib.fns.user.get_steam_id)(client.user) })
    }

    /// Whether the client is logged on to Steam's servers
    /// (`ISteamUser::BLoggedOn`). Steam can be running and initialised while
    /// offline, in which case this is `false`.
    #[must_use]
    pub fn logged_on(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: as in `steam_id`.
        unsafe { (client.lib.fns.user.logged_on)(client.user) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn the_steam_id_and_log_on_state_are_the_clients() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert_eq!(steam.user().steam_id(), SteamId(testing::STEAM_ID));
        assert!(steam.user().logged_on());
        testing::script(|s| s.logged_on = false);
        assert!(!steam.user().logged_on());
    }
}
