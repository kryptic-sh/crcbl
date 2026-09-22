//! `ISteamApps`: the running game as Steam sees it.
//!
//! The basics so far — ownership and the game's language; the launch command
//! line arrives with `docs/plan/42-steam.md`'s slice 3a, and DLC, betas and
//! the rest of ownership with slice 11.

use crate::Steam;

/// `ISteamApps`, borrowed from a [`Steam`]; from [`Steam::apps`].
#[derive(Debug, Clone, Copy)]
pub struct Apps<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// The running game: ownership and language.
    #[must_use]
    pub fn apps(&self) -> Apps<'_> {
        Apps { steam: self }
    }
}

impl Apps<'_> {
    /// Whether the player has a licence for the running app
    /// (`ISteamApps::BIsSubscribed`). Under the shared test app 480 this says
    /// nothing about a real licence.
    #[must_use]
    pub fn subscribed(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: `client.apps` is the non-null `ISteamApps` init resolved,
        // and Steam stays initialised while `client` lives.
        unsafe { (client.lib.fns.apps.is_subscribed)(client.apps) }
    }

    /// The language the player chose for this game in Steam, as Steam's API
    /// language name — `"english"`, `"german"`, `"schinese"`
    /// (`ISteamApps::GetCurrentGameLanguage`). This, not
    /// [`Utils::ui_language`](crate::Utils::ui_language), is the one a game
    /// should localise into.
    #[must_use]
    pub fn game_language(&self) -> String {
        let client = &self.steam.client;
        // SAFETY: as in `subscribed`; the string is copied before any other
        // Steam call.
        unsafe {
            let language = (client.lib.fns.apps.get_current_game_language)(client.apps);
            self.steam.copy_string(language)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn subscribed_is_the_clients_answer() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert!(!steam.apps().subscribed());
        testing::script(|s| s.subscribed = true);
        assert!(steam.apps().subscribed());
    }
}
