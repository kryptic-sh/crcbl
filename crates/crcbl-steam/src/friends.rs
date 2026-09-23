//! `ISteamFriends`: the player's social identity and their friends.
//!
//! The local persona, the friends list and each friend's name, state and
//! avatar (`crate::avatar`), friends' rich presence, and the overlay's
//! dialogs; setting rich presence and sending invites is `crate::presence`.

use crate::{
    LobbyId, Steam, SteamId,
    error::{SteamError, c_string},
};

/// Which of the users Steam knows about to list (`EFriendFlags`); combine
/// with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FriendFlags(pub u32);

impl FriendFlags {
    /// `k_EFriendFlagBlocked`.
    pub const BLOCKED: Self = Self(0x01);
    /// `k_EFriendFlagFriendshipRequested`.
    pub const FRIENDSHIP_REQUESTED: Self = Self(0x02);
    /// `k_EFriendFlagImmediate` — the player's own friends list.
    pub const IMMEDIATE: Self = Self(0x04);
    /// `k_EFriendFlagClanMember`.
    pub const CLAN_MEMBER: Self = Self(0x08);
    /// `k_EFriendFlagOnGameServer`.
    pub const ON_GAME_SERVER: Self = Self(0x10);
    /// `k_EFriendFlagRequestingFriendship`.
    pub const REQUESTING_FRIENDSHIP: Self = Self(0x80);
    /// `k_EFriendFlagRequestingInfo`.
    pub const REQUESTING_INFO: Self = Self(0x100);
    /// `k_EFriendFlagIgnored`.
    pub const IGNORED: Self = Self(0x200);
    /// `k_EFriendFlagIgnoredFriend`.
    pub const IGNORED_FRIEND: Self = Self(0x400);
    /// `k_EFriendFlagChatMember`.
    pub const CHAT_MEMBER: Self = Self(0x1000);
    /// `k_EFriendFlagAll`.
    pub const ALL: Self = Self(0xFFFF);
}

impl core::ops::BitOr for FriendFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// What changed about a user (`EPersonaChange` bits, from
/// `PersonaStateChange_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersonaChange(pub u32);

impl PersonaChange {
    /// `k_EPersonaChangeName`.
    pub const NAME: Self = Self(0x0001);
    /// `k_EPersonaChangeStatus`.
    pub const STATUS: Self = Self(0x0002);
    /// `k_EPersonaChangeComeOnline`.
    pub const CAME_ONLINE: Self = Self(0x0004);
    /// `k_EPersonaChangeGoneOffline`.
    pub const WENT_OFFLINE: Self = Self(0x0008);
    /// `k_EPersonaChangeGamePlayed`.
    pub const GAME_PLAYED: Self = Self(0x0010);
    /// `k_EPersonaChangeAvatar`.
    pub const AVATAR: Self = Self(0x0040);
    /// `k_EPersonaChangeRelationshipChanged`.
    pub const RELATIONSHIP: Self = Self(0x0200);
    /// `k_EPersonaChangeNickname`.
    pub const NICKNAME: Self = Self(0x1000);
    /// `k_EPersonaChangeRichPresence`.
    pub const RICH_PRESENCE: Self = Self(0x4000);

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A user's online state (`EPersonaState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonaState {
    /// `k_EPersonaStateOffline`, or not a user this client knows.
    Offline,
    /// `k_EPersonaStateOnline`.
    Online,
    /// `k_EPersonaStateBusy`.
    Busy,
    /// `k_EPersonaStateAway`.
    Away,
    /// `k_EPersonaStateSnooze` — away for a long time.
    Snooze,
    /// `k_EPersonaStateLookingToTrade`.
    LookingToTrade,
    /// `k_EPersonaStateLookingToPlay`.
    LookingToPlay,
    /// `k_EPersonaStateInvisible` — only ever the local user's own.
    Invisible,
    /// A value this crate does not name.
    Unknown(i32),
}

impl PersonaState {
    /// Maps an `EPersonaState`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Offline,
            1 => Self::Online,
            2 => Self::Busy,
            3 => Self::Away,
            4 => Self::Snooze,
            5 => Self::LookingToTrade,
            6 => Self::LookingToPlay,
            7 => Self::Invisible,
            other => Self::Unknown(other),
        }
    }
}

/// An overlay page `ActivateGameOverlay` opens, named as the header lists
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverlayDialog {
    /// `"Friends"`.
    Friends,
    /// `"Community"`.
    Community,
    /// `"Players"` — the recently-played-with list.
    Players,
    /// `"Settings"`.
    Settings,
    /// `"OfficialGameGroup"`.
    OfficialGameGroup,
    /// `"Stats"`.
    Stats,
    /// `"Achievements"`.
    Achievements,
}

impl OverlayDialog {
    /// The string Steam takes.
    const fn name(self) -> &'static core::ffi::CStr {
        match self {
            Self::Friends => c"Friends",
            Self::Community => c"Community",
            Self::Players => c"Players",
            Self::Settings => c"Settings",
            Self::OfficialGameGroup => c"OfficialGameGroup",
            Self::Stats => c"Stats",
            Self::Achievements => c"Achievements",
        }
    }
}

/// An overlay page about one user, for `ActivateGameOverlayToUser`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserDialog {
    /// `"steamid"` — their profile.
    Profile,
    /// `"chat"` — a chat window with them.
    Chat,
    /// `"stats"`.
    Stats,
    /// `"achievements"`.
    Achievements,
    /// `"friendadd"` — prompt to add them as a friend.
    FriendAdd,
    /// `"friendremove"` — prompt to remove them.
    FriendRemove,
    /// `"friendrequestaccept"`.
    FriendRequestAccept,
    /// `"friendrequestignore"`.
    FriendRequestIgnore,
}

impl UserDialog {
    /// The string Steam takes.
    const fn name(self) -> &'static core::ffi::CStr {
        match self {
            Self::Profile => c"steamid",
            Self::Chat => c"chat",
            Self::Stats => c"stats",
            Self::Achievements => c"achievements",
            Self::FriendAdd => c"friendadd",
            Self::FriendRemove => c"friendremove",
            Self::FriendRequestAccept => c"friendrequestaccept",
            Self::FriendRequestIgnore => c"friendrequestignore",
        }
    }
}

/// How the overlay browser opens a web page
/// (`EActivateGameOverlayToWebPageMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WebPageMode {
    /// Beside the overlay's other windows, and stays open
    /// (`k_EActivateGameOverlayToWebPageMode_Default`).
    Default,
    /// Alone, and closing it closes the overlay
    /// (`k_EActivateGameOverlayToWebPageMode_Modal`).
    Modal,
}

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

    /// The local player's own online state (`GetPersonaState`).
    #[must_use]
    pub fn persona_state(&self) -> PersonaState {
        let client = &self.steam.client;
        // SAFETY: `client.friends` is the non-null interface init resolved;
        // every call below relies on the same.
        PersonaState::from_raw(unsafe {
            (client.lib.fns.friends.get_persona_state)(client.friends)
        })
    }

    /// Every user matching `flags` — [`FriendFlags::IMMEDIATE`] for the
    /// friends list (`GetFriendCount`, `GetFriendByIndex`).
    #[must_use]
    pub fn list(&self, flags: FriendFlags) -> Vec<SteamId> {
        let client = &self.steam.client;
        let fns = &client.lib.fns.friends;
        // Every named flag fits in the `int` Steam takes; `ALL` is 0xFFFF.
        let Ok(flags) = i32::try_from(flags.0) else {
            return Vec::new();
        };
        // SAFETY: see `persona_state`; each index is below the count Steam
        // gave for the same flags, as the header requires.
        unsafe {
            let count = (fns.get_friend_count)(client.friends, flags);
            (0..count.max(0))
                .map(|index| SteamId((fns.get_friend_by_index)(client.friends, index, flags)))
                .collect()
        }
    }

    /// A user's display name (`GetFriendPersonaName`) — for a user this
    /// client has not learned about yet, empty or `"[unknown]"` until
    /// [`request_user_information`](Self::request_user_information) is
    /// answered with [`SteamEvent::PersonaStateChanged`](crate::SteamEvent::PersonaStateChanged).
    #[must_use]
    pub fn name(&self, user: SteamId) -> String {
        let client = &self.steam.client;
        // SAFETY: see `persona_state`; the string is copied before any other
        // Steam call.
        unsafe {
            let name = (client.lib.fns.friends.get_friend_persona_name)(client.friends, user.0);
            self.steam.copy_string(name)
        }
    }

    /// A user's online state (`GetFriendPersonaState`).
    #[must_use]
    pub fn state(&self, user: SteamId) -> PersonaState {
        let client = &self.steam.client;
        // SAFETY: see `persona_state`.
        PersonaState::from_raw(unsafe {
            (client.lib.fns.friends.get_friend_persona_state)(client.friends, user.0)
        })
    }

    /// Asks Steam to fetch a user's name and, unless `name_only`, avatar
    /// (`RequestUserInformation`). `true`: it is being fetched, and
    /// [`SteamEvent::PersonaStateChanged`](crate::SteamEvent::PersonaStateChanged)
    /// follows; `false`: it is already known.
    pub fn request_user_information(&self, user: SteamId, name_only: bool) -> bool {
        let client = &self.steam.client;
        // SAFETY: see `persona_state`.
        unsafe {
            (client.lib.fns.friends.request_user_information)(client.friends, user.0, name_only)
        }
    }

    /// One of a friend's rich-presence values (`GetFriendRichPresence`);
    /// empty when unset or not yet known — see
    /// [`request_rich_presence`](Self::request_rich_presence).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] or [`SteamError::TooLong`] for a key Steam
    /// could not hold.
    pub fn rich_presence(&self, friend: SteamId, key: &str) -> Result<String, SteamError> {
        let key = c_string(
            key,
            "rich presence key",
            crate::MAX_RICH_PRESENCE_KEY_LENGTH,
        )?;
        let client = &self.steam.client;
        // SAFETY: see `persona_state`; `key` is NUL-terminated and outlives the
        // call, and the string is copied before any other Steam call.
        Ok(unsafe {
            let value = (client.lib.fns.friends.get_friend_rich_presence)(
                client.friends,
                friend.0,
                key.as_ptr(),
            );
            self.steam.copy_string(value)
        })
    }

    /// Asks Steam for a friend's rich presence (`RequestFriendRichPresence`);
    /// [`SteamEvent::FriendRichPresenceChanged`](crate::SteamEvent::FriendRichPresenceChanged)
    /// follows.
    pub fn request_rich_presence(&self, friend: SteamId) {
        let client = &self.steam.client;
        // SAFETY: see `persona_state`.
        unsafe { (client.lib.fns.friends.request_friend_rich_presence)(client.friends, friend.0) };
    }

    /// Opens the overlay to one of its pages (`ActivateGameOverlay`).
    pub fn open_overlay(&self, dialog: OverlayDialog) {
        let client = &self.steam.client;
        // SAFETY: see `persona_state`; the name is a static NUL-terminated
        // string.
        unsafe {
            (client.lib.fns.friends.activate_game_overlay)(client.friends, dialog.name().as_ptr())
        };
    }

    /// Opens the overlay to a page about `user`
    /// (`ActivateGameOverlayToUser`).
    pub fn open_overlay_to_user(&self, dialog: UserDialog, user: SteamId) {
        let client = &self.steam.client;
        // SAFETY: as in `open_overlay`.
        unsafe {
            (client.lib.fns.friends.activate_game_overlay_to_user)(
                client.friends,
                dialog.name().as_ptr(),
                user.0,
            );
        }
    }

    /// Opens the overlay's browser at `url`, which must carry its scheme,
    /// e.g. `https://` (`ActivateGameOverlayToWebPage`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`] for a URL Steam could not be handed whole.
    pub fn open_overlay_to_web_page(&self, url: &str, mode: WebPageMode) -> Result<(), SteamError> {
        let url = c_string(url, "overlay URL", usize::MAX)?;
        let mode = match mode {
            WebPageMode::Default => 0,
            WebPageMode::Modal => 1,
        };
        let client = &self.steam.client;
        // SAFETY: see `persona_state`; `url` is NUL-terminated and outlives
        // the call, and `mode` is a named `EActivateGameOverlayToWebPageMode`.
        unsafe {
            (client.lib.fns.friends.activate_game_overlay_to_web_page)(
                client.friends,
                url.as_ptr(),
                mode,
            );
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn the_list_walks_every_index_with_the_flags_it_was_counted_with() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.friends = vec![5, 6, 7]);
        assert_eq!(
            steam
                .friends()
                .list(FriendFlags::IMMEDIATE | FriendFlags::BLOCKED),
            [SteamId(5), SteamId(6), SteamId(7)]
        );
        assert_eq!(testing::script(|s| s.friend_flags.clone()), [5, 5, 5, 5]);
    }

    #[test]
    fn states_map_and_keep_unknown_values() {
        let named = [
            PersonaState::Offline,
            PersonaState::Online,
            PersonaState::Busy,
            PersonaState::Away,
            PersonaState::Snooze,
            PersonaState::LookingToTrade,
            PersonaState::LookingToPlay,
            PersonaState::Invisible,
        ];
        for (raw, expected) in (0..).zip(named) {
            assert_eq!(PersonaState::from_raw(raw), expected);
        }
        assert_eq!(PersonaState::from_raw(8), PersonaState::Unknown(8));
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.persona_state = 6);
        assert_eq!(steam.friends().persona_state(), PersonaState::LookingToPlay);
        assert_eq!(
            steam.friends().state(SteamId(5)),
            PersonaState::LookingToPlay
        );
    }

    #[test]
    fn persona_changes_test_every_bit_of_the_flag() {
        let change = PersonaChange(0x4041);
        assert!(change.contains(PersonaChange::NAME));
        assert!(change.contains(PersonaChange::RICH_PRESENCE));
        assert!(!change.contains(PersonaChange::STATUS));
        assert!(!change.contains(PersonaChange(0x0041 | 0x0002)));
    }

    #[test]
    fn names_presence_and_information_requests_reach_steam() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| {
            s.set_string(b"Barney");
            s.info_pending = true;
        });
        let friends = steam.friends();
        assert_eq!(friends.name(SteamId(5)), "Barney");
        assert_eq!(
            friends.rich_presence(SteamId(5), "status").unwrap(),
            "Barney"
        );
        assert!(friends.request_user_information(SteamId(5), true));
        friends.request_rich_presence(SteamId(6));
        assert_eq!(testing::script(|s| s.info_requests.clone()), [(5, true)]);
        assert_eq!(testing::script(|s| s.presence_requests.clone()), [6]);
        assert_eq!(
            friends.rich_presence(SteamId(5), &"k".repeat(64)),
            Err(SteamError::TooLong {
                argument: "rich presence key",
                len: 64,
                max: 63,
            })
        );
    }

    #[test]
    fn overlay_dialogs_pass_valves_names() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let friends = steam.friends();
        friends.open_overlay(OverlayDialog::Friends);
        friends.open_overlay(OverlayDialog::OfficialGameGroup);
        friends.open_overlay_to_user(UserDialog::Profile, SteamId(5));
        friends.open_overlay_to_user(UserDialog::FriendRequestAccept, SteamId(6));
        friends
            .open_overlay_to_web_page("https://example.com/", WebPageMode::Modal)
            .unwrap();
        assert_eq!(
            friends.open_overlay_to_web_page("https://a\0b", WebPageMode::Default),
            Err(SteamError::InteriorNul("overlay URL"))
        );
        let calls = testing::script(|s| s.overlays.clone());
        let calls: Vec<(&str, &str, u64)> =
            calls.iter().map(|(f, a, n)| (*f, a.as_str(), *n)).collect();
        assert_eq!(
            calls,
            [
                ("ActivateGameOverlay", "Friends", 0),
                ("ActivateGameOverlay", "OfficialGameGroup", 0),
                ("ActivateGameOverlayToUser", "steamid", 5),
                ("ActivateGameOverlayToUser", "friendrequestaccept", 6),
                ("ActivateGameOverlayToWebPage", "https://example.com/", 1),
            ]
        );
    }
}
