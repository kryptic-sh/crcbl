//! `ISteamApps`: the running game as Steam sees it.
//!
//! Ownership, the game's language, and how the game was launched — including
//! the `+connect_lobby <id>` argument Steam launches a game with when the
//! player accepts a lobby invite while it is not running; and, in
//! [`content`]'s half, ownership, DLC, betas and the install directory.

mod content;

pub(crate) use content::grow;
pub use content::{Beta, BetaCount, BetaFlags, Dlc, FileDetails, MAX_TEXT_BYTES};

use core::ffi::c_char;

use crate::{LobbyId, Steam, callbacks::fixed_string, error::SteamError};

/// The argument Steam passes a game launched to join a lobby, followed by the
/// lobby id.
pub const CONNECT_LOBBY: &str = "+connect_lobby";

/// The lobby a launch asked to join: the id after the first
/// [`CONNECT_LOBBY`] among `args` — `std::env::args()` for a launch Steam
/// made to accept an invite, or the words of
/// [`Apps::launch_command_line`]. `None` when there is no
/// `+connect_lobby`, or the word after it is not a lobby id.
pub fn connect_lobby<I, S>(args: I) -> Option<LobbyId>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    args.by_ref().find(|arg| arg.as_ref() == CONNECT_LOBBY)?;
    let id: u64 = args.next()?.as_ref().parse().ok()?;
    (id != 0).then_some(LobbyId(id))
}

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

    /// The command line a `steam://run/<appid>//<command line>/` launch passed
    /// (`ISteamApps::GetLaunchCommandLine`); empty when there was none. Read it
    /// at start-up and again on
    /// [`SteamEvent::NewLaunchParameters`](crate::SteamEvent::NewLaunchParameters),
    /// and pass its words to [`connect_lobby`].
    ///
    /// Read through [`grow`], as every string this crate reads is: the header
    /// states no maximum, and Steam's copy stops a byte short to leave its
    /// NUL, so a line that reaches the last byte but one is read again into
    /// a larger buffer rather than returned cut.
    ///
    /// # Errors
    ///
    /// [`SteamError::Truncated`] for a line past [`MAX_TEXT_BYTES`].
    pub fn launch_command_line(&self) -> Result<String, SteamError> {
        let client = &self.steam.client;
        let ((), [buffer]) = grow("GetLaunchCommandLine", |[buffer], capacity| {
            // SAFETY: see `subscribed`; `buffer` is `capacity` writable bytes.
            unsafe {
                (client.lib.fns.apps.get_launch_command_line)(
                    client.apps,
                    buffer.as_mut_ptr().cast::<c_char>(),
                    capacity,
                );
            }
            Some(())
        })?;
        let (line, lossy) = fixed_string(&buffer);
        if lossy {
            self.steam
                .lossy_strings
                .set(self.steam.lossy_strings.get() + 1);
        }
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn connect_lobby_finds_the_id_after_the_flag() {
        assert_eq!(
            connect_lobby(["game.exe", "+connect_lobby", "109775241"]),
            Some(LobbyId(109_775_241))
        );
        // Trailing arguments after it do not matter.
        assert_eq!(
            connect_lobby("-windowed +connect_lobby 42 -novid".split_whitespace()),
            Some(LobbyId(42))
        );
    }

    #[test]
    fn connect_lobby_is_none_when_absent_or_malformed() {
        assert_eq!(connect_lobby(["game.exe", "-windowed"]), None);
        assert_eq!(connect_lobby(Vec::<String>::new()), None);
        assert_eq!(connect_lobby(["+connect_lobby"]), None, "no id after it");
        assert_eq!(connect_lobby(["+connect_lobby", "lobby"]), None);
        assert_eq!(connect_lobby(["+connect_lobby", "-1"]), None);
        assert_eq!(connect_lobby(["+connect_lobby", "0"]), None, "the nil id");
        assert_eq!(
            connect_lobby(["+connect_lobby", "18446744073709551616"]),
            None,
            "over u64"
        );
        assert_eq!(
            connect_lobby(["connect_lobby", "42"]),
            None,
            "the plus is part of it"
        );
    }

    #[test]
    fn the_launch_command_line_is_copied_up_to_its_nul() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert_eq!(steam.apps().launch_command_line(), Ok(String::new()));
        testing::script(|s| s.launch_line = b"+connect_lobby 42".to_vec());
        assert_eq!(
            steam.apps().launch_command_line(),
            Ok("+connect_lobby 42".into())
        );
    }

    /// A long line is read whole, however Steam's copy cut it the first
    /// time; one past the largest buffer is truncated, never cut.
    #[test]
    fn a_long_launch_command_line_grows_and_one_past_the_cap_is_truncated() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        for len in [1023, 1024, 5000] {
            testing::script(|s| s.launch_line = vec![b'x'; len]);
            assert_eq!(
                steam.apps().launch_command_line().map(|line| line.len()),
                Ok(len),
                "{len} bytes"
            );
        }
        testing::script(|s| s.launch_line = vec![b'x'; MAX_TEXT_BYTES]);
        assert_eq!(
            steam.apps().launch_command_line(),
            Err(SteamError::Truncated("GetLaunchCommandLine"))
        );
    }

    #[test]
    fn subscribed_is_the_clients_answer() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert!(!steam.apps().subscribed());
        testing::script(|s| s.subscribed = true);
        assert!(steam.apps().subscribed());
    }
}
