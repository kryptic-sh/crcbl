//! Rich presence: what the player's friends see them doing, and how a friend
//! joins them.
//!
//! Two keys matter to an invite flow. `connect` is what a friend's "Join
//! game" hands the game — a running game receives it as
//! [`SteamEvent::RichPresenceJoinRequested`](crate::SteamEvent::RichPresenceJoinRequested),
//! a launched one on its command line — and `steam_display` names a
//! localisation token for the friends list. Steam's limits are checked here,
//! before the call, so a value Steam would cut short or drop is an error the
//! game sees.

use std::collections::BTreeSet;

use crate::{
    Friends, SteamId,
    error::{SteamError, c_string},
    matchmaking::refused_unless,
};

/// The most rich-presence keys one player can have set
/// (`k_cchMaxRichPresenceKeys`, `isteamfriends.h`).
pub const MAX_RICH_PRESENCE_KEYS: usize = 30;

/// The longest rich-presence key, in bytes. `k_cchMaxRichPresenceKeyLength`
/// is 64 and counts the NUL, as every `cch` buffer size in the SDK does.
pub const MAX_RICH_PRESENCE_KEY_LENGTH: usize = 63;

/// The longest rich-presence value, in bytes.
/// `k_cchMaxRichPresenceValueLength` is 256 and counts the NUL: it is the size
/// of the `char` array a `connect` value is delivered in
/// (`GameRichPresenceJoinRequested_t::m_rgchConnect`), so a longer one could
/// not arrive intact.
pub const MAX_RICH_PRESENCE_VALUE_LENGTH: usize = 255;

impl Friends<'_> {
    /// Sets one of the player's rich-presence keys
    /// (`ISteamFriends::SetRichPresence`); an empty `value` removes it.
    ///
    /// # Errors
    ///
    /// Before the call: [`SteamError::InteriorNul`], [`SteamError::TooLong`]
    /// over [`MAX_RICH_PRESENCE_KEY_LENGTH`] or
    /// [`MAX_RICH_PRESENCE_VALUE_LENGTH`], or [`SteamError::TooMany`] for a
    /// new key past [`MAX_RICH_PRESENCE_KEYS`]. After: [`SteamError::Refused`].
    pub fn set_rich_presence(&self, key: &str, value: &str) -> Result<(), SteamError> {
        let c_key = c_string(key, "rich presence key", MAX_RICH_PRESENCE_KEY_LENGTH)?;
        let c_value = c_string(value, "rich presence value", MAX_RICH_PRESENCE_VALUE_LENGTH)?;
        let steam = self.steam();
        let mut keys = steam.presence_keys.borrow_mut();
        if !value.is_empty() && !keys.contains(key) && keys.len() >= MAX_RICH_PRESENCE_KEYS {
            return Err(SteamError::TooMany {
                what: "rich presence keys",
                max: MAX_RICH_PRESENCE_KEYS,
            });
        }
        let client = &steam.client;
        // SAFETY: `client.friends` is the non-null interface init resolved;
        // both strings are NUL-terminated and outlive the call.
        let done = unsafe {
            (client.lib.fns.friends.set_rich_presence)(
                client.friends,
                c_key.as_ptr(),
                c_value.as_ptr(),
            )
        };
        refused_unless(done, "SetRichPresence")?;
        if value.is_empty() {
            keys.remove(key);
        } else {
            keys.insert(key.to_owned());
        }
        Ok(())
    }

    /// Removes every rich-presence key (`ISteamFriends::ClearRichPresence`).
    pub fn clear_rich_presence(&self) {
        let steam = self.steam();
        let client = &steam.client;
        // SAFETY: see `set_rich_presence`.
        unsafe { (client.lib.fns.friends.clear_rich_presence)(client.friends) };
        steam.presence_keys.borrow_mut().clear();
    }

    /// Invites `friend` to the game with a rich-presence `connect` string
    /// (`ISteamFriends::InviteUserToGame`); accepting it delivers that string
    /// as [`SteamEvent::RichPresenceJoinRequested`](crate::SteamEvent::RichPresenceJoinRequested).
    ///
    /// # Errors
    ///
    /// As [`set_rich_presence`](Self::set_rich_presence) for `connect`, or
    /// [`SteamError::Refused`].
    pub fn invite_to_game(&self, friend: SteamId, connect: &str) -> Result<(), SteamError> {
        let connect = c_string(connect, "connect string", MAX_RICH_PRESENCE_VALUE_LENGTH)?;
        let client = &self.steam().client;
        // SAFETY: see `set_rich_presence`.
        let done = unsafe {
            (client.lib.fns.friends.invite_user_to_game)(client.friends, friend.0, connect.as_ptr())
        };
        refused_unless(done, "InviteUserToGame")
    }
}

/// The rich-presence keys this process has set, so the key limit is checked
/// before Steam would silently drop one.
pub(crate) type PresenceKeys = std::cell::RefCell<BTreeSet<String>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn a_set_key_reaches_steam_and_an_empty_value_removes_it() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        steam
            .friends()
            .set_rich_presence("connect", "+connect_lobby 7")
            .unwrap();
        steam.friends().set_rich_presence("connect", "").unwrap();
        assert_eq!(
            testing::script(|s| s.rich_presence.clone()),
            [
                ("connect".to_owned(), "+connect_lobby 7".to_owned()),
                ("connect".to_owned(), String::new()),
            ]
        );
        assert!(steam.presence_keys.borrow().is_empty());
    }

    #[test]
    fn keys_and_values_over_steams_limits_are_refused_before_the_call() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let friends = steam.friends();
        // The SDK's numbers, not the constants: 64 and 256, NUL included. The
        // drift gate ties the constants to the header; this ties the check to
        // the numbers.
        let key = "k".repeat(63);
        let value = "v".repeat(255);
        friends.set_rich_presence(&key, &value).unwrap();
        assert_eq!(
            friends.set_rich_presence(&format!("{key}k"), "v"),
            Err(SteamError::TooLong {
                argument: "rich presence key",
                len: 64,
                max: 63,
            })
        );
        assert_eq!(
            friends.set_rich_presence("k", &format!("{value}v")),
            Err(SteamError::TooLong {
                argument: "rich presence value",
                len: 256,
                max: 255,
            })
        );
        assert_eq!(
            friends.set_rich_presence("k\0", "v"),
            Err(SteamError::InteriorNul("rich presence key"))
        );
        assert_eq!(
            testing::script(|s| s.rich_presence.len()),
            1,
            "only the valid call reached Steam"
        );
    }

    #[test]
    fn a_thirty_first_key_is_refused_but_an_existing_one_can_change() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let friends = steam.friends();
        for n in 0..30 {
            friends.set_rich_presence(&format!("k{n}"), "v").unwrap();
        }
        assert_eq!(
            friends.set_rich_presence("one more", "v"),
            Err(SteamError::TooMany {
                what: "rich presence keys",
                max: 30,
            })
        );
        friends.set_rich_presence("k0", "changed").unwrap();
        friends.set_rich_presence("k0", "").unwrap();
        friends.set_rich_presence("one more", "v").unwrap();
        friends.clear_rich_presence();
        assert!(steam.presence_keys.borrow().is_empty());
        assert_eq!(testing::script(|s| s.calls.clear_rich_presence), 1);
    }

    #[test]
    fn a_refused_set_is_an_error_and_is_not_counted_as_a_key() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.refuse = true);
        assert_eq!(
            steam.friends().set_rich_presence("status", "hi"),
            Err(SteamError::Refused("SetRichPresence"))
        );
        assert!(steam.presence_keys.borrow().is_empty());
    }
}
