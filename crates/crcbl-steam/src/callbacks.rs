//! The callback ids this crate claims, and what each payload decodes to.
//!
//! One row per bound callback: its id, written as Valve writes it — a base
//! plus an offset (`k_iSteamFriendsCallbacks + 31`) — its C struct name, the
//! size the payload must have, and the decode. The size is `size_of` the
//! declared struct, which `ffi::structs`'s layout tables pin per operating
//! system. The drift gate compares each row's base and offset with the
//! header's own `k_iCallback` expression, and each base with the header's
//! `enum`.
//!
//! Ids not in the table are skipped by the pump, by design: the pipe carries
//! dozens nobody bound, and every SDK adds more. Call results — payloads that
//! arrive through `SteamAPI_ManualDispatch_GetAPICallResult` rather than the
//! pipe — are `crate::call`'s rows, not these; one that Steam also broadcasts
//! on the pipe (`LobbyEnter_t`) is skipped there like any other unclaimed id.

use crate::{
    AppId, EResult, SteamId,
    ffi::structs::{
        AvatarImageLoaded, DlcInstalled, FloatingGamepadTextInputDismissed,
        FriendRichPresenceUpdate, GameLobbyJoinRequested, GameOverlayActivated,
        GameRichPresenceJoinRequested, GamepadTextInputDismissed, LobbyChatMsg, LobbyChatUpdate,
        LobbyDataUpdate, NewUrlLaunchParameters, PersonaStateChange, RemoteStorageLocalFileChange,
        ScreenshotReady, ScreenshotRequested, SteamApiCallCompleted, SteamInputDeviceConnected,
        SteamInputDeviceDisconnected, SteamNetConnectionInfo, SteamNetConnectionStatusChanged,
        SteamRelayNetworkStatus, SteamRemotePlaySessionConnected,
        SteamRemotePlaySessionDisconnected, UserAchievementStored, UserStatsReceived,
        UserStatsStored, steam_id,
    },
    friends::PersonaChange,
    matchmaking::{LobbyId, MemberChange},
};

/// The callback id bases, as `steam_api_internal.h` declares them:
/// `enum { k_iSteam…Callbacks = … };`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Base {
    /// `k_iSteamFriendsCallbacks`.
    Friends = 300,
    /// `k_iSteamMatchmakingCallbacks`.
    Matchmaking = 500,
    /// `k_iSteamUtilsCallbacks`.
    Utils = 700,
    /// `k_iSteamAppsCallbacks`.
    Apps = 1000,
    /// `k_iSteamUserStatsCallbacks`.
    UserStats = 1100,
    /// `k_iSteamNetworkingSocketsCallbacks`.
    NetworkingSockets = 1220,
    /// `k_iSteamNetworkingUtilsCallbacks`.
    NetworkingUtils = 1280,
    /// `k_iSteamRemoteStorageCallbacks`.
    RemoteStorage = 1300,
    /// `k_iSteamScreenshotsCallbacks`.
    Screenshots = 2300,
    /// `k_iSteamRemotePlayCallbacks`.
    RemotePlay = 5700,
    /// `k_iSteamControllerCallbacks` — Steam Input's callbacks, under the
    /// name of the interface it replaced.
    Controller = 2800,
    /// `k_iSteamTimelineCallbacks`.
    Timeline = 6000,
}

impl Base {
    /// Every base, for the drift gate.
    #[cfg(test)]
    pub(crate) const ALL: &[Self] = &[
        Self::Friends,
        Self::Matchmaking,
        Self::Utils,
        Self::Apps,
        Self::UserStats,
        Self::NetworkingSockets,
        Self::NetworkingUtils,
        Self::RemoteStorage,
        Self::Screenshots,
        Self::Controller,
        Self::RemotePlay,
        Self::Timeline,
    ];

    /// Valve's name for the base, as the headers spell it.
    #[cfg(test)]
    pub(crate) const fn valve_name(self) -> &'static str {
        match self {
            Self::Friends => "k_iSteamFriendsCallbacks",
            Self::Matchmaking => "k_iSteamMatchmakingCallbacks",
            Self::Utils => "k_iSteamUtilsCallbacks",
            Self::Apps => "k_iSteamAppsCallbacks",
            Self::UserStats => "k_iSteamUserStatsCallbacks",
            Self::RemoteStorage => "k_iSteamRemoteStorageCallbacks",
            Self::NetworkingSockets => "k_iSteamNetworkingSocketsCallbacks",
            Self::NetworkingUtils => "k_iSteamNetworkingUtilsCallbacks",
            Self::Screenshots => "k_iSteamScreenshotsCallbacks",
            Self::Controller => "k_iSteamControllerCallbacks",
            Self::RemotePlay => "k_iSteamRemotePlayCallbacks",
            Self::Timeline => "k_iSteamTimelineCallbacks",
        }
    }
}

/// Something that happened in Steam, drained from [`Steam::events`](crate::Steam::events).
///
/// One variant per bound callback, added by the slice that binds it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SteamEvent {
    /// The Steam overlay opened (`active`) or closed. An open overlay should
    /// pause the game and release held input exactly as focus loss does.
    OverlayActivated {
        /// Whether it has just opened.
        active: bool,
    },
    /// The player accepted an invite to `lobby`, or chose "Join game" on a
    /// friend in it, while the game was running
    /// (`GameLobbyJoinRequested_t`). Join it with
    /// [`Matchmaking::join_lobby`](crate::Matchmaking::join_lobby).
    LobbyJoinRequested {
        /// The lobby to join.
        lobby: LobbyId,
        /// The friend it was joined through, when it was.
        friend: Option<SteamId>,
    },
    /// The player accepted a rich-presence invite while the game was running
    /// (`GameRichPresenceJoinRequested_t`); `connect` is the inviter's
    /// `connect` value, e.g. `+connect_lobby 109775241234567890`.
    RichPresenceJoinRequested {
        /// The friend it was joined through, when it was.
        friend: Option<SteamId>,
        /// The connect string.
        connect: String,
    },
    /// Something about a user changed (`PersonaStateChange_t`): read the
    /// new name, state or avatar through [`Friends`](crate::Friends). Steam
    /// sends one per friend at start-up, as it learns about each.
    PersonaStateChanged {
        /// Who.
        user: SteamId,
        /// What changed.
        change: PersonaChange,
    },
    /// An avatar that was still downloading has arrived
    /// (`AvatarImageLoaded_t`): ask [`Friends::avatar`](crate::Friends::avatar)
    /// again.
    AvatarLoaded {
        /// Whose avatar.
        user: SteamId,
        /// Its width in pixels.
        width: u32,
        /// Its height in pixels.
        height: u32,
    },
    /// A friend's rich presence changed (`FriendRichPresenceUpdate_t`): read
    /// it with [`Friends::rich_presence`](crate::Friends::rich_presence).
    FriendRichPresenceChanged {
        /// The friend.
        friend: SteamId,
        /// The app the presence belongs to.
        app: AppId,
    },
    /// The relay network's readiness changed (`SteamRelayNetworkStatus_t`);
    /// see [`Networking::relay_status`](crate::Networking::relay_status).
    RelayStatusChanged(crate::RelayStatus),
    /// The game was launched again through a Steam URL while running
    /// (`NewUrlLaunchParameters_t`): re-read
    /// [`Apps::launch_command_line`](crate::Apps::launch_command_line).
    NewLaunchParameters,
    /// A member of a lobby this client is in entered, left, dropped or was
    /// removed (`LobbyChatUpdate_t`).
    LobbyMemberChanged {
        /// The lobby.
        lobby: LobbyId,
        /// Whose membership changed.
        member: SteamId,
        /// How.
        change: MemberChange,
        /// Who made the change — `member` itself unless it was a kick or ban.
        by: SteamId,
    },
    /// A lobby this client holds a [`Lobby`](crate::Lobby) for has a new
    /// owner. Steam passes ownership on by itself when the owner leaves; this
    /// event is derived by re-reading the owner after every member and data
    /// change, not a Steam callback of its own.
    LobbyOwnerChanged {
        /// The lobby.
        lobby: LobbyId,
        /// Its owner now.
        owner: SteamId,
    },
    /// A lobby's data, or one member's, changed (`LobbyDataUpdate_t`).
    LobbyDataChanged {
        /// The lobby.
        lobby: LobbyId,
        /// The member whose data changed; `None` for the lobby's own data.
        member: Option<SteamId>,
        /// `false` only when the lobby no longer exists.
        success: bool,
    },
    /// A Steam Cloud file changed while the game was running
    /// (`RemoteStorageLocalFileChange_t`, read with `GetLocalFileChange`) —
    /// another device's write synced down, as when a Steam Deck resumes.
    /// Load the file again; `crcbl_store::synced::SyncedFile::load`
    /// classifies the change as it would at start-up.
    CloudFileChanged {
        /// The cloud file name, for a file written through
        /// [`SteamCloudStorage`](crate::SteamCloudStorage); an absolute path
        /// for an Auto-Cloud file.
        path: String,
    },
    /// A user's stats and achievements arrived for this game
    /// (`UserStatsReceived_t`). For the local user with `EResult::OK`, this is
    /// what makes [`Stats`](crate::Stats) usable; Steam sends it on its own
    /// soon after init.
    StatsReceived {
        /// Whose.
        user: SteamId,
        /// Whether they could be fetched.
        result: EResult,
    },
    /// [`Stats::store`](crate::Stats::store) finished (`UserStatsStored_t`).
    StatsStored {
        /// `EResult::OK`, or why not.
        result: EResult,
    },
    /// An achievement was stored, or its progress shown
    /// (`UserAchievementStored_t`).
    AchievementStored {
        /// Its API name.
        name: String,
        /// `(current, max)` for a progress notice; `None` when it unlocked.
        progress: Option<(u32, u32)>,
    },
    /// The full-screen on-screen keyboard
    /// ([`Utils::show_text_input`](crate::Utils::show_text_input)) closed
    /// (`GamepadTextInputDismissed_t`). `text` is what the player accepted —
    /// committed text, like a physical keyboard's — or `None` when they
    /// cancelled, or when Steam would not hand over the text it reported
    /// (counted in
    /// [`PumpDiagnostics::decode_mismatches`](crate::PumpDiagnostics::decode_mismatches)).
    TextInputDismissed {
        /// The accepted text.
        text: Option<String>,
    },
    /// The floating keyboard
    /// ([`Utils::show_floating_keyboard`](crate::Utils::show_floating_keyboard))
    /// closed (`FloatingGamepadTextInputDismissed_t`). What it typed arrived
    /// as ordinary key and text events through the window.
    FloatingKeyboardDismissed,
    /// The player asked for a screenshot while the game hooks them
    /// ([`Screenshots::hook`](crate::Screenshots::hook);
    /// `ScreenshotRequested_t`): capture the frame and hand it to
    /// [`Screenshots::write`](crate::Screenshots::write). Steam takes none of
    /// its own.
    ScreenshotRequested,
    /// A screenshot reached the library and can be tagged
    /// (`ScreenshotReady_t`).
    ScreenshotReady {
        /// Which.
        screenshot: crate::ScreenshotId,
        /// `EResult::OK`, or why not.
        result: EResult,
    },
    /// A DLC the player owns was installed (`DlcInstalled_t`).
    DlcInstalled {
        /// The DLC's app id.
        app: AppId,
    },
    /// A Remote Play session connected (`SteamRemotePlaySessionConnected_t`);
    /// read it through [`RemotePlay`](crate::RemotePlay).
    RemotePlayConnected {
        /// The session.
        session: crate::RemotePlaySession,
    },
    /// A Remote Play session disconnected
    /// (`SteamRemotePlaySessionDisconnected_t`).
    RemotePlayDisconnected {
        /// The session.
        session: crate::RemotePlaySession,
    },
    /// A lobby chat message arrived (`LobbyChatMsg_t`, read with
    /// `GetLobbyChatEntry`). Every member receives its own too.
    LobbyChatMessage {
        /// The lobby.
        lobby: LobbyId,
        /// Who sent it.
        sender: SteamId,
        /// Its `EChatEntryType`: `1` (`k_EChatEntryTypeChatMsg`) for an
        /// ordinary message.
        kind: i32,
        /// The message bytes, exactly as sent.
        body: Vec<u8>,
    },
}

/// What a claimed payload decodes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// A game-visible event.
    Event(SteamEvent),
    /// A game-visible event one of whose strings was not valid UTF-8 and was
    /// read lossily; the pump counts it.
    LossyEvent(SteamEvent),
    /// `SteamAPICallCompleted_t`: an asynchronous call's answer is ready; the
    /// pump hands it to the call registry.
    CallCompleted(SteamApiCallCompleted),
    /// `SteamNetConnectionStatusChangedCallback_t`: a connection changed
    /// state; the pump hands one arriving on a listen socket to its listener.
    ConnectionStatus {
        /// The connection.
        connection: u32,
        /// The listen socket it arrived on, or zero for one this end opened.
        listen_socket: u32,
        /// Its `ESteamNetworkingConnectionState` now.
        state: i32,
        /// The certified identity at the other end, if it is a Steam id.
        remote: Option<SteamId>,
    },
    /// `RemoteStorageLocalFileChange_t`: the pump reads the changes with
    /// `GetLocalFileChange` and queues [`SteamEvent::CloudFileChanged`] for
    /// each.
    LocalFileChange,
    /// `UserStatsReceived_t`, `UserStatsStored_t` or
    /// `UserAchievementStored_t`: the pump keeps it only for the running
    /// game, and marks the stats ready on the local user's arrival.
    Stats {
        /// `m_nGameID`: the app for a Steam game.
        game_id: u64,
        /// What happened.
        event: SteamEvent,
        /// Whether the event's text was read lossily.
        lossy: bool,
    },
    /// `GamepadTextInputDismissed_t`: the pump reads the accepted text with
    /// `GetEnteredGamepadTextInput` and queues
    /// [`SteamEvent::TextInputDismissed`].
    TextInput {
        /// Whether the player accepted the text.
        submitted: bool,
        /// `m_unAppID`.
        app: u32,
    },
    /// `SteamInputDeviceConnected_t` or `SteamInputDeviceDisconnected_t`: the
    /// pump hands it to the open `SteamPads`, if any.
    InputDevice {
        /// The controller's `InputHandle_t`.
        handle: u64,
        /// Whether it connected, rather than disconnected.
        connected: bool,
    },
    /// `LobbyChatMsg_t`: the pump reads the entry with `GetLobbyChatEntry`
    /// and queues [`SteamEvent::LobbyChatMessage`].
    ChatMessage {
        /// The lobby.
        lobby: LobbyId,
        /// `m_iChatID`, the entry to read.
        chat_id: u32,
    },
}

impl PartialEq for SteamApiCallCompleted {
    fn eq(&self, other: &Self) -> bool {
        // Copies, never references: the struct is packed.
        let (a, b) = (*self, *other);
        (a.async_call, a.callback, a.param_size) == (b.async_call, b.callback, b.param_size)
    }
}

impl Eq for SteamApiCallCompleted {}

/// One claimed callback id.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row {
    /// Valve's base for the id.
    pub(crate) base: Base,
    /// The offset from [`base`](Self::base).
    pub(crate) offset: i32,
    /// The C struct name, for the drift gate.
    #[cfg(test)]
    pub(crate) name: &'static str,
    /// The size the payload must be: `size_of` the declared struct.
    pub(crate) size: usize,
    /// Decodes a payload of exactly [`size`](Self::size) bytes; `None` for any
    /// other length.
    pub(crate) decode: fn(&[u8]) -> Option<Decoded>,
}

impl Row {
    /// The callback id: `base + offset`.
    pub(crate) const fn id(&self) -> i32 {
        self.base as i32 + self.offset
    }
}

/// A `CSteamID` that may be `k_steamIDNil` (zero), which Valve's comments
/// call "invalid".
const fn optional_id(raw: u64) -> Option<SteamId> {
    if raw == 0 { None } else { Some(SteamId(raw)) }
}

/// Every claimed callback.
pub(crate) const ROWS: &[Row] = &[
    Row {
        base: Base::Utils,
        offset: 3,
        #[cfg(test)]
        name: "SteamAPICallCompleted_t",
        size: size_of::<SteamApiCallCompleted>(),
        decode: |bytes| read::<SteamApiCallCompleted>(bytes).map(Decoded::CallCompleted),
    },
    Row {
        base: Base::Friends,
        offset: 31,
        #[cfg(test)]
        name: "GameOverlayActivated_t",
        size: size_of::<GameOverlayActivated>(),
        decode: |bytes| {
            read::<GameOverlayActivated>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::OverlayActivated {
                    active: payload.active != 0,
                })
            })
        },
    },
    Row {
        base: Base::Friends,
        offset: 4,
        #[cfg(test)]
        name: "PersonaStateChange_t",
        size: size_of::<PersonaStateChange>(),
        decode: |bytes| {
            read::<PersonaStateChange>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::PersonaStateChanged {
                    user: SteamId(payload.user),
                    change: PersonaChange(payload.change.cast_unsigned()),
                })
            })
        },
    },
    Row {
        base: Base::Friends,
        offset: 34,
        #[cfg(test)]
        name: "AvatarImageLoaded_t",
        size: size_of::<AvatarImageLoaded>(),
        // A negative size is not an image; it decodes to nothing and is counted.
        decode: |bytes| {
            let payload = read::<AvatarImageLoaded>(bytes)?;
            Some(Decoded::Event(SteamEvent::AvatarLoaded {
                user: SteamId(steam_id(payload.user)),
                width: u32::try_from(payload.width).ok()?,
                height: u32::try_from(payload.height).ok()?,
            }))
        },
    },
    Row {
        base: Base::Friends,
        offset: 36,
        #[cfg(test)]
        name: "FriendRichPresenceUpdate_t",
        size: size_of::<FriendRichPresenceUpdate>(),
        decode: |bytes| {
            read::<FriendRichPresenceUpdate>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::FriendRichPresenceChanged {
                    friend: SteamId(steam_id(payload.friend)),
                    app: AppId(payload.app),
                })
            })
        },
    },
    Row {
        base: Base::Friends,
        offset: 33,
        #[cfg(test)]
        name: "GameLobbyJoinRequested_t",
        size: size_of::<GameLobbyJoinRequested>(),
        decode: |bytes| {
            read::<GameLobbyJoinRequested>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::LobbyJoinRequested {
                    lobby: LobbyId(steam_id(payload.lobby)),
                    friend: optional_id(steam_id(payload.friend)),
                })
            })
        },
    },
    Row {
        base: Base::Friends,
        offset: 37,
        #[cfg(test)]
        name: "GameRichPresenceJoinRequested_t",
        size: size_of::<GameRichPresenceJoinRequested>(),
        decode: |bytes| {
            read::<GameRichPresenceJoinRequested>(bytes).map(|payload| {
                let (connect, lossy) = fixed_string(&payload.connect);
                let event = SteamEvent::RichPresenceJoinRequested {
                    friend: optional_id(steam_id(payload.friend)),
                    connect,
                };
                if lossy {
                    Decoded::LossyEvent(event)
                } else {
                    Decoded::Event(event)
                }
            })
        },
    },
    Row {
        base: Base::Apps,
        offset: 14,
        #[cfg(test)]
        name: "NewUrlLaunchParameters_t",
        size: size_of::<NewUrlLaunchParameters>(),
        decode: |bytes| {
            read::<NewUrlLaunchParameters>(bytes)
                .map(|_| Decoded::Event(SteamEvent::NewLaunchParameters))
        },
    },
    Row {
        base: Base::UserStats,
        offset: 1,
        #[cfg(test)]
        name: "UserStatsReceived_t",
        size: size_of::<UserStatsReceived>(),
        decode: |bytes| {
            read::<UserStatsReceived>(bytes).map(|payload| Decoded::Stats {
                game_id: payload.game_id,
                event: SteamEvent::StatsReceived {
                    user: SteamId(steam_id(payload.user)),
                    result: EResult(payload.result),
                },
                lossy: false,
            })
        },
    },
    Row {
        base: Base::UserStats,
        offset: 2,
        #[cfg(test)]
        name: "UserStatsStored_t",
        size: size_of::<UserStatsStored>(),
        decode: |bytes| {
            read::<UserStatsStored>(bytes).map(|payload| Decoded::Stats {
                game_id: payload.game_id,
                event: SteamEvent::StatsStored {
                    result: EResult(payload.result),
                },
                lossy: false,
            })
        },
    },
    Row {
        base: Base::UserStats,
        offset: 3,
        #[cfg(test)]
        name: "UserAchievementStored_t",
        size: size_of::<UserAchievementStored>(),
        decode: |bytes| {
            read::<UserAchievementStored>(bytes).map(|payload| {
                let (name, lossy) = fixed_string(&payload.name);
                let (current, max) = (payload.current, payload.max);
                Decoded::Stats {
                    game_id: payload.game_id,
                    event: SteamEvent::AchievementStored {
                        name,
                        progress: (current, max).ne(&(0, 0)).then_some((current, max)),
                    },
                    lossy,
                }
            })
        },
    },
    Row {
        base: Base::RemoteStorage,
        offset: 33,
        #[cfg(test)]
        name: "RemoteStorageLocalFileChange_t",
        size: size_of::<RemoteStorageLocalFileChange>(),
        decode: |bytes| {
            read::<RemoteStorageLocalFileChange>(bytes).map(|_| Decoded::LocalFileChange)
        },
    },
    Row {
        base: Base::Matchmaking,
        offset: 5,
        #[cfg(test)]
        name: "LobbyDataUpdate_t",
        size: size_of::<LobbyDataUpdate>(),
        decode: |bytes| {
            read::<LobbyDataUpdate>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::LobbyDataChanged {
                    lobby: LobbyId(payload.lobby),
                    // The lobby's own id in the member field means lobby data.
                    member: (payload.member != payload.lobby).then_some(SteamId(payload.member)),
                    success: payload.success != 0,
                })
            })
        },
    },
    Row {
        base: Base::Matchmaking,
        offset: 6,
        #[cfg(test)]
        name: "LobbyChatUpdate_t",
        size: size_of::<LobbyChatUpdate>(),
        decode: |bytes| {
            read::<LobbyChatUpdate>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::LobbyMemberChanged {
                    lobby: LobbyId(payload.lobby),
                    member: SteamId(payload.changed),
                    change: MemberChange::from_flags(payload.state_change),
                    by: SteamId(payload.making_change),
                })
            })
        },
    },
    Row {
        base: Base::Matchmaking,
        offset: 7,
        #[cfg(test)]
        name: "LobbyChatMsg_t",
        size: size_of::<LobbyChatMsg>(),
        decode: |bytes| {
            read::<LobbyChatMsg>(bytes).map(|payload| Decoded::ChatMessage {
                lobby: LobbyId(payload.lobby),
                chat_id: payload.chat_id,
            })
        },
    },
    Row {
        base: Base::NetworkingSockets,
        offset: 1,
        #[cfg(test)]
        name: "SteamNetConnectionStatusChangedCallback_t",
        size: size_of::<SteamNetConnectionStatusChanged>(),
        decode: |bytes| {
            let payload = read::<SteamNetConnectionStatusChanged>(bytes)?;
            let info = payload.info;
            Some(Decoded::ConnectionStatus {
                connection: payload.connection,
                listen_socket: info.listen_socket,
                state: info.state,
                remote: crate::net::remote_of(&info.identity),
            })
        },
    },
    Row {
        base: Base::NetworkingUtils,
        offset: 1,
        #[cfg(test)]
        name: "SteamRelayNetworkStatus_t",
        size: size_of::<SteamRelayNetworkStatus>(),
        decode: |bytes| {
            let payload = read::<SteamRelayNetworkStatus>(bytes)?;
            let (status, lossy) = crate::RelayStatus::from_raw(&payload);
            let event = SteamEvent::RelayStatusChanged(status);
            Some(if lossy {
                Decoded::LossyEvent(event)
            } else {
                Decoded::Event(event)
            })
        },
    },
    Row {
        base: Base::Utils,
        offset: 14,
        #[cfg(test)]
        name: "GamepadTextInputDismissed_t",
        size: size_of::<GamepadTextInputDismissed>(),
        decode: |bytes| {
            read::<GamepadTextInputDismissed>(bytes).map(|payload| Decoded::TextInput {
                submitted: payload.submitted != 0,
                app: payload.app,
            })
        },
    },
    Row {
        base: Base::Utils,
        offset: 38,
        #[cfg(test)]
        name: "FloatingGamepadTextInputDismissed_t",
        size: size_of::<FloatingGamepadTextInputDismissed>(),
        decode: |bytes| {
            read::<FloatingGamepadTextInputDismissed>(bytes)
                .map(|_| Decoded::Event(SteamEvent::FloatingKeyboardDismissed))
        },
    },
    Row {
        base: Base::Apps,
        offset: 5,
        #[cfg(test)]
        name: "DlcInstalled_t",
        size: size_of::<DlcInstalled>(),
        decode: |bytes| {
            read::<DlcInstalled>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::DlcInstalled {
                    app: AppId(payload.app),
                })
            })
        },
    },
    Row {
        base: Base::RemotePlay,
        offset: 1,
        #[cfg(test)]
        name: "SteamRemotePlaySessionConnected_t",
        size: size_of::<SteamRemotePlaySessionConnected>(),
        decode: |bytes| {
            read::<SteamRemotePlaySessionConnected>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::RemotePlayConnected {
                    session: crate::RemotePlaySession(payload.session),
                })
            })
        },
    },
    Row {
        base: Base::RemotePlay,
        offset: 2,
        #[cfg(test)]
        name: "SteamRemotePlaySessionDisconnected_t",
        size: size_of::<SteamRemotePlaySessionDisconnected>(),
        decode: |bytes| {
            read::<SteamRemotePlaySessionDisconnected>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::RemotePlayDisconnected {
                    session: crate::RemotePlaySession(payload.session),
                })
            })
        },
    },
    Row {
        base: Base::Screenshots,
        offset: 1,
        #[cfg(test)]
        name: "ScreenshotReady_t",
        size: size_of::<ScreenshotReady>(),
        decode: |bytes| {
            read::<ScreenshotReady>(bytes).map(|payload| {
                Decoded::Event(SteamEvent::ScreenshotReady {
                    screenshot: crate::ScreenshotId(payload.screenshot),
                    result: EResult(payload.result),
                })
            })
        },
    },
    Row {
        base: Base::Screenshots,
        offset: 2,
        #[cfg(test)]
        name: "ScreenshotRequested_t",
        size: size_of::<ScreenshotRequested>(),
        decode: |bytes| {
            read::<ScreenshotRequested>(bytes)
                .map(|_| Decoded::Event(SteamEvent::ScreenshotRequested))
        },
    },
    Row {
        base: Base::Controller,
        offset: 1,
        #[cfg(test)]
        name: "SteamInputDeviceConnected_t",
        size: size_of::<SteamInputDeviceConnected>(),
        decode: |bytes| {
            read::<SteamInputDeviceConnected>(bytes).map(|payload| Decoded::InputDevice {
                handle: payload.handle,
                connected: true,
            })
        },
    },
    Row {
        base: Base::Controller,
        offset: 2,
        #[cfg(test)]
        name: "SteamInputDeviceDisconnected_t",
        size: size_of::<SteamInputDeviceDisconnected>(),
        decode: |bytes| {
            read::<SteamInputDeviceDisconnected>(bytes).map(|payload| Decoded::InputDevice {
                handle: payload.handle,
                connected: false,
            })
        },
    },
];

/// The row claiming `id`, if any.
pub(crate) fn find(id: i32) -> Option<&'static Row> {
    ROWS.iter().find(|row| row.id() == id)
}

/// A `char[N]` field's text: up to its first NUL, or all of it if Steam left
/// none; and whether it had to be read lossily.
pub(crate) fn fixed_string(bytes: &[u8]) -> (String, bool) {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    match core::str::from_utf8(&bytes[..end]) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (String::from_utf8_lossy(&bytes[..end]).into_owned(), true),
    }
}

/// Types every bit pattern of which is a valid value: `ffi::structs`'s
/// payload structs, whose fields are all integers.
///
/// # Safety
///
/// Implement only for `Copy` types with no padding-sensitive invariants, no
/// references, no `bool`, no enums — any `size_of::<Self>()` bytes must be a
/// valid `Self`.
pub(crate) unsafe trait Pod: Copy {}

// SAFETY: for each, every field is an integer or an integer array, C's `bool`
// included as `u8` (see `ffi::structs`).
unsafe impl Pod for SteamApiCallCompleted {}
// SAFETY: as above.
unsafe impl Pod for GameOverlayActivated {}
// SAFETY: as above.
unsafe impl Pod for GameLobbyJoinRequested {}
// SAFETY: as above.
unsafe impl Pod for GameRichPresenceJoinRequested {}
// SAFETY: as above.
unsafe impl Pod for NewUrlLaunchParameters {}
// SAFETY: as above.
unsafe impl Pod for PersonaStateChange {}
// SAFETY: as above.
unsafe impl Pod for RemoteStorageLocalFileChange {}
// SAFETY: as above.
unsafe impl Pod for UserStatsReceived {}
// SAFETY: as above.
unsafe impl Pod for UserStatsStored {}
// SAFETY: as above.
unsafe impl Pod for UserAchievementStored {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::LeaderboardFindResult {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::LeaderboardScoresDownloaded {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::LeaderboardScoreUploaded {}
// SAFETY: as above.
unsafe impl Pod for SteamNetConnectionInfo {}
// SAFETY: as above.
unsafe impl Pod for SteamNetConnectionStatusChanged {}
// SAFETY: as above.
unsafe impl Pod for SteamRelayNetworkStatus {}
// SAFETY: as above.
unsafe impl Pod for AvatarImageLoaded {}
// SAFETY: as above.
unsafe impl Pod for FriendRichPresenceUpdate {}
// SAFETY: as above.
unsafe impl Pod for LobbyDataUpdate {}
// SAFETY: as above.
unsafe impl Pod for LobbyChatUpdate {}
// SAFETY: as above.
unsafe impl Pod for LobbyChatMsg {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::LobbyCreated {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::LobbyEnter {}
// SAFETY: as above.
unsafe impl Pod for DlcInstalled {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::FileDetailsResult {}
// SAFETY: as above.
unsafe impl Pod for SteamRemotePlaySessionConnected {}
// SAFETY: as above.
unsafe impl Pod for SteamRemotePlaySessionDisconnected {}
// SAFETY: as above.
unsafe impl Pod for ScreenshotReady {}
// SAFETY: as above.
unsafe impl Pod for ScreenshotRequested {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::SteamTimelineGamePhaseRecordingExists {}
// SAFETY: as above.
unsafe impl Pod for crate::ffi::structs::SteamTimelineEventRecordingExists {}
// SAFETY: as above.
unsafe impl Pod for GamepadTextInputDismissed {}
// SAFETY: as above.
unsafe impl Pod for FloatingGamepadTextInputDismissed {}
// SAFETY: as above.
unsafe impl Pod for SteamInputDeviceConnected {}
// SAFETY: as above.
unsafe impl Pod for SteamInputDeviceDisconnected {}

/// Copies a `T` out of exactly `size_of::<T>()` bytes; `None` for any other
/// length.
pub(crate) fn read<T: Pod>(bytes: &[u8]) -> Option<T> {
    if bytes.len() != size_of::<T>() {
        return None;
    }
    // SAFETY: `bytes` holds exactly `size_of::<T>()` readable bytes, the read
    // is unaligned, and `T: Pod` makes any bytes a valid value.
    Some(unsafe { bytes.as_ptr().cast::<T>().read_unaligned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `GameOverlayActivated_t` payload, byte by byte in little-endian
    /// (every supported target is): active, user-initiated, two bytes of
    /// padding, the app id, the overlay's pid.
    fn overlay_bytes(active: u8) -> Vec<u8> {
        let mut bytes = vec![active, 1, 0, 0];
        bytes.extend_from_slice(&480u32.to_le_bytes());
        bytes.extend_from_slice(&1234u32.to_le_bytes());
        bytes
    }

    #[test]
    fn the_ids_are_valves() {
        assert_eq!(
            find(331).map(|row| row.name),
            Some("GameOverlayActivated_t")
        );
        assert_eq!(
            find(703).map(|row| row.name),
            Some("SteamAPICallCompleted_t")
        );
        assert!(find(332).is_none());
    }

    #[test]
    fn no_two_rows_claim_one_id() {
        for (i, a) in ROWS.iter().enumerate() {
            for b in &ROWS[i + 1..] {
                assert_ne!(a.id(), b.id(), "{} and {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn overlay_activated_decodes_m_b_active() {
        let row = find(331).unwrap();
        assert_eq!(
            (row.decode)(&overlay_bytes(1)),
            Some(Decoded::Event(SteamEvent::OverlayActivated {
                active: true
            }))
        );
        assert_eq!(
            (row.decode)(&overlay_bytes(0)),
            Some(Decoded::Event(SteamEvent::OverlayActivated {
                active: false
            }))
        );
    }

    #[test]
    fn call_completed_decodes_every_field() {
        let mut bytes = 0x1122_3344_5566_7788u64.to_le_bytes().to_vec();
        bytes.extend_from_slice(&513i32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        let Some(Decoded::CallCompleted(done)) = (find(703).unwrap().decode)(&bytes) else {
            panic!("did not decode");
        };
        assert_eq!({ done.async_call }, 0x1122_3344_5566_7788);
        assert_eq!({ done.callback }, 513);
        assert_eq!({ done.param_size }, 24);
    }

    #[test]
    fn a_payload_of_the_wrong_length_does_not_decode() {
        let row = find(331).unwrap();
        let mut long = overlay_bytes(1);
        long.push(0);
        assert_eq!((row.decode)(&long), None);
        assert_eq!((row.decode)(&overlay_bytes(1)[..11]), None);
        assert_eq!((row.decode)(&[]), None);
    }

    /// A payload of `row`'s size, zeroed, with each `(offset, bytes)` written
    /// in: the layout tables pin the offsets, so a fixture only needs them.
    fn fixture(id: i32, fields: &[(usize, &[u8])]) -> (&'static Row, Vec<u8>) {
        let row = find(id).unwrap_or_else(|| panic!("no row claims {id}"));
        let mut bytes = vec![0; row.size];
        for &(at, field) in fields {
            bytes[at..at + field.len()].copy_from_slice(field);
        }
        (row, bytes)
    }

    #[test]
    fn the_new_ids_are_valves() {
        for (id, name) in [
            (333, "GameLobbyJoinRequested_t"),
            (337, "GameRichPresenceJoinRequested_t"),
            (1014, "NewUrlLaunchParameters_t"),
            (505, "LobbyDataUpdate_t"),
            (506, "LobbyChatUpdate_t"),
            (507, "LobbyChatMsg_t"),
        ] {
            assert_eq!(find(id).map(|row| row.name), Some(name), "{id}");
        }
        // Call results are not pipe rows: a broadcast `LobbyEnter_t` is skipped.
        assert!(find(504).is_none());
        assert!(find(513).is_none());
    }

    #[test]
    fn a_lobby_join_request_decodes_and_a_nil_friend_is_none() {
        let (row, bytes) = fixture(333, &[(0, &7u64.to_le_bytes()), (8, &9u64.to_le_bytes())]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::LobbyJoinRequested {
                lobby: LobbyId(7),
                friend: Some(SteamId(9)),
            }))
        );
        let (row, bytes) = fixture(333, &[(0, &7u64.to_le_bytes())]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::LobbyJoinRequested {
                lobby: LobbyId(7),
                friend: None,
            }))
        );
    }

    #[test]
    fn a_rich_presence_join_request_reads_the_connect_string_to_its_nul() {
        let (row, bytes) = fixture(
            337,
            &[(0, &5u64.to_le_bytes()), (8, b"+connect_lobby 42\0junk")],
        );
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::RichPresenceJoinRequested {
                friend: Some(SteamId(5)),
                connect: "+connect_lobby 42".into(),
            }))
        );
        // No NUL at all: the whole array, and never past it.
        let (row, bytes) = fixture(337, &[(8, &[b'x'; 256])]);
        let Some(Decoded::Event(SteamEvent::RichPresenceJoinRequested { connect, .. })) =
            (row.decode)(&bytes)
        else {
            panic!("did not decode");
        };
        assert_eq!(connect, "x".repeat(256));
        // Invalid UTF-8 is read lossily and marked so the pump counts it.
        let (row, bytes) = fixture(337, &[(8, b"caf\xE9\0")]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::LossyEvent(SteamEvent::RichPresenceJoinRequested {
                friend: None,
                connect: "caf\u{FFFD}".into(),
            }))
        );
    }

    #[test]
    fn new_launch_parameters_is_one_byte() {
        let (row, bytes) = fixture(1014, &[]);
        assert_eq!(bytes.len(), 1);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::NewLaunchParameters))
        );
    }

    #[test]
    fn lobby_data_names_the_member_or_none_for_the_lobby_itself() {
        let lobby = 0x0186_0000_0000_0001_u64.to_le_bytes();
        let (row, bytes) = fixture(505, &[(0, &lobby), (8, &lobby), (16, &[1])]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::LobbyDataChanged {
                lobby: LobbyId(0x0186_0000_0000_0001),
                member: None,
                success: true,
            }))
        );
        let (row, bytes) = fixture(505, &[(0, &lobby), (8, &3u64.to_le_bytes())]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::LobbyDataChanged {
                lobby: LobbyId(0x0186_0000_0000_0001),
                member: Some(SteamId(3)),
                success: false,
            }))
        );
    }

    #[test]
    fn a_member_change_decodes_who_how_and_by_whom() {
        let (row, bytes) = fixture(
            506,
            &[
                (0, &1u64.to_le_bytes()),
                (8, &2u64.to_le_bytes()),
                (16, &3u64.to_le_bytes()),
                (24, &0x8u32.to_le_bytes()),
            ],
        );
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::LobbyMemberChanged {
                lobby: LobbyId(1),
                member: SteamId(2),
                change: MemberChange::Kicked,
                by: SteamId(3),
            }))
        );
    }

    #[test]
    fn a_chat_message_decodes_to_the_entry_to_read() {
        let (row, bytes) = fixture(
            507,
            &[
                (0, &1u64.to_le_bytes()),
                (8, &2u64.to_le_bytes()),
                (16, &[1]),
                (20, &77u32.to_le_bytes()),
            ],
        );
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::ChatMessage {
                lobby: LobbyId(1),
                chat_id: 77,
            })
        );
    }

    #[test]
    fn persona_avatar_and_presence_changes_decode() {
        let (row, bytes) = fixture(
            304,
            &[(0, &7u64.to_le_bytes()), (8, &0x4040_i32.to_le_bytes())],
        );
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::PersonaStateChanged {
                user: SteamId(7),
                change: PersonaChange(0x4040),
            }))
        );
        let (row, bytes) = fixture(
            334,
            &[
                (0, &7u64.to_le_bytes()),
                (8, &3i32.to_le_bytes()),
                (12, &64i32.to_le_bytes()),
                (16, &32i32.to_le_bytes()),
            ],
        );
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::AvatarLoaded {
                user: SteamId(7),
                width: 64,
                height: 32,
            }))
        );
        let (row, bytes) = fixture(334, &[(12, &(-1i32).to_le_bytes())]);
        assert_eq!(
            (row.decode)(&bytes),
            None,
            "a negative width is not an image"
        );
        let (row, bytes) = fixture(336, &[(0, &7u64.to_le_bytes()), (8, &480u32.to_le_bytes())]);
        assert_eq!(
            (row.decode)(&bytes),
            Some(Decoded::Event(SteamEvent::FriendRichPresenceChanged {
                friend: SteamId(7),
                app: AppId(480),
            }))
        );
    }
}
