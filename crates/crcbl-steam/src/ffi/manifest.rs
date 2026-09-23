//! Every bound function, written once.
//!
//! Each entry names the Rust field, the prototype alias, the exported symbol,
//! **the C declaration it was copied from, verbatim**, and the Rust signature.
//! The `bindings!` macro turns that one list into:
//!
//! - the `prototype` aliases, each documented with its C declaration;
//! - one function-pointer struct per group, and [`Fns`] holding them all;
//! - [`Fns::resolve`], which looks every symbol (and every group's interface
//!   accessor) up through a resolver and fails with `InitError::NoSymbol`
//!   naming the first one missing;
//! - [`INTERFACES`], the accessor rows the handshake is built from;
//! - `BINDINGS` (test builds), the symbol/declaration pairs the drift gate
//!   looks for in the SDK headers.
//!
//! So a function cannot be loaded without being in the table the drift gate
//! reads, and its declaration exists in one place.
//!
//! **Provenance.** Every declaration was written from the SDK 1.65 header
//! mirror (see `docs/plan/42-steam.md`, "Conventions"). The drift gate has
//! run against that mirror's headers, and **not** yet against an SDK zip
//! downloaded from Valve, which no machine this was written on had; until it
//! has, a declaration here is a claim about the mirror's fidelity as much as
//! about the SDK. Some choices are worth naming. Every enum crossing here
//! (`ESteamHardwareType`, `ESteamHardwareDefaultConfig`,
//! `ENotificationPosition`, `ELobbyType`, `EChatEntryType`,
//! `EPersonaState`, `EActivateGameOverlayToWebPageMode`) is taken to be
//! `int`-sized, as every Steamworks enum without an explicit base is. `bool` is
//! C's one-byte `_Bool`, which Rust's `bool` matches across `extern "C"`. A
//! `CSteamID *` out-parameter is declared `*mut u64`: `CSteamID` is exactly
//! one 64-bit value, and Steam writes it into storage this crate owns and
//! aligns. A `const char *` return is Steam's buffer, copied before anything
//! else runs (`crate::strings`).

use core::ffi::{c_char, c_void};

use super::{
    HSteamPipe, ISteamApps, ISteamFriends, ISteamMatchmaking, ISteamUser, ISteamUtils,
    SteamApiCall, SteamErrMsg,
    structs::CallbackMsg,
    versions::{self, Interface},
};
use crate::error::InitError;

/// One bound function, as the drift gate looks for it.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoundFn {
    /// The exported symbol.
    pub(crate) symbol: &'static str,
    /// The C declaration, as the header spells it.
    pub(crate) declaration: &'static str,
}

/// Every interface accessor — `ISteamXxx *SteamAPI_SteamXxx_vNNN();` — typed
/// as returning `void *`; each caller casts to its opaque interface type.
pub(crate) type Accessor = unsafe extern "C" fn() -> *mut c_void;

/// Looks a symbol up by name, answering its address or null.
pub(crate) type Resolver<'a> = dyn FnMut(&'static str) -> *mut c_void + 'a;

/// The type of a group's `accessor` field; a macro so `bindings!` can emit
/// the field only for groups that name an interface.
macro_rules! accessor_field {
    ($iface:path) => {
        Accessor
    };
}

/// Expands the one list into prototypes, groups, the resolver and the tables;
/// see the module docs.
macro_rules! bindings {
    ($(
        $(#[$group_doc:meta])*
        $group:ident: $Group:ident $(for $iface:path)? {
            $(
                $field:ident: $Proto:ident = $symbol:literal,
                    $decl:literal,
                    fn($($arg:ty),* $(,)?) $(-> $ret:ty)?;
            )+
        }
    )+) => {
        /// One alias per bound function, each documented with the C
        /// declaration it asserts.
        pub(crate) mod prototype {
            use super::*;
            $($(
                #[doc = concat!("`", $decl, "`")]
                pub(crate) type $Proto = unsafe extern "C" fn($($arg),*) $(-> $ret)?;
            )+)+
        }

        $(
            $(#[$group_doc])*
            #[derive(Debug, Clone, Copy)]
            pub(crate) struct $Group {
                $(
                    /// The interface accessor.
                    pub(crate) accessor: accessor_field!($iface),
                )?
                $(
                    #[doc = concat!("`", $symbol, "`")]
                    pub(crate) $field: prototype::$Proto,
                )+
            }
        )+

        /// Every bound function, by group.
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct Fns {
            $(
                $(#[$group_doc])*
                pub(crate) $group: $Group,
            )+
        }

        impl Fns {
            /// Resolves every symbol and accessor through `resolve`.
            ///
            /// # Errors
            ///
            /// `InitError::NoSymbol` naming the first symbol `resolve`
            /// answered null for — an SDK older than these declarations.
            pub(crate) fn resolve(resolve: &mut Resolver<'_>) -> Result<Self, InitError> {
                Ok(Self {
                    $(
                        $group: $Group {
                            $(accessor: resolve_accessor(resolve, &$iface)?,)?
                            $(
                                $field: {
                                    let raw = resolve($symbol);
                                    if raw.is_null() {
                                        return Err(InitError::NoSymbol($symbol));
                                    }
                                    // SAFETY: the alias is this manifest's
                                    // declaration of exactly this symbol, and a
                                    // function pointer and a data pointer are
                                    // the same size on every supported target.
                                    // The drift gate checks the declaration
                                    // against the SDK's.
                                    unsafe { core::mem::transmute::<*mut c_void, prototype::$Proto>(raw) }
                                },
                            )+
                        },
                    )+
                })
            }
        }

        /// The interface rows the bound groups name, in manifest order — what
        /// the init handshake is built from.
        pub(crate) const INTERFACES: &[Interface] = &[$($($iface,)?)+];

        /// Every bound function's symbol and declaration, for the drift gate.
        #[cfg(test)]
        pub(crate) const BINDINGS: &[BoundFn] = &[
            $($(BoundFn { symbol: $symbol, declaration: $decl },)+)+
        ];
    };
}

/// Resolves one interface accessor.
fn resolve_accessor(resolve: &mut Resolver<'_>, iface: &Interface) -> Result<Accessor, InitError> {
    let raw = resolve(iface.accessor);
    if raw.is_null() {
        return Err(InitError::NoSymbol(iface.accessor));
    }
    // SAFETY: every accessor has the one shape `Accessor` declares (no
    // arguments, an interface pointer back), and function and data pointers
    // are the same size on every supported target.
    Ok(unsafe { core::mem::transmute::<*mut c_void, Accessor>(raw) })
}

bindings! {
    /// Init, shutdown and the pipe (`steam_api.h`, `steam_api_common.h`).
    lifecycle: LifecycleFns {
        init: SteamApiInit = "SteamInternal_SteamAPI_Init",
            "S_API ESteamAPIInitResult S_CALLTYPE SteamInternal_SteamAPI_Init( const char *pszInternalCheckInterfaceVersions, SteamErrMsg *pOutErrMsg );",
            fn(*const c_char, *mut SteamErrMsg) -> i32;
        shutdown: Shutdown = "SteamAPI_Shutdown",
            "S_API void S_CALLTYPE SteamAPI_Shutdown();",
            fn();
        get_pipe: GetHSteamPipe = "SteamAPI_GetHSteamPipe",
            "S_API HSteamPipe S_CALLTYPE SteamAPI_GetHSteamPipe();",
            fn() -> HSteamPipe;
        release_thread_memory: ReleaseCurrentThreadMemory = "SteamAPI_ReleaseCurrentThreadMemory",
            "S_API void S_CALLTYPE SteamAPI_ReleaseCurrentThreadMemory();",
            fn();
        restart_app_if_necessary: RestartAppIfNecessary = "SteamAPI_RestartAppIfNecessary",
            "S_API bool S_CALLTYPE SteamAPI_RestartAppIfNecessary( uint32 unOwnAppID );",
            fn(u32) -> bool;
    }

    /// Manual callback dispatch (`steam_api.h`). Never mixed with
    /// `SteamAPI_RunCallbacks`, which is deliberately not bound.
    dispatch: DispatchFns {
        init: ManualDispatchInit = "SteamAPI_ManualDispatch_Init",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_Init();",
            fn();
        run_frame: ManualDispatchRunFrame = "SteamAPI_ManualDispatch_RunFrame",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_RunFrame( HSteamPipe hSteamPipe );",
            fn(HSteamPipe);
        get_next_callback: ManualDispatchGetNextCallback = "SteamAPI_ManualDispatch_GetNextCallback",
            "S_API bool S_CALLTYPE SteamAPI_ManualDispatch_GetNextCallback( HSteamPipe hSteamPipe, CallbackMsg_t *pCallbackMsg );",
            fn(HSteamPipe, *mut CallbackMsg) -> bool;
        free_last_callback: ManualDispatchFreeLastCallback = "SteamAPI_ManualDispatch_FreeLastCallback",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_FreeLastCallback( HSteamPipe hSteamPipe );",
            fn(HSteamPipe);
        get_api_call_result: ManualDispatchGetApiCallResult = "SteamAPI_ManualDispatch_GetAPICallResult",
            "S_API bool S_CALLTYPE SteamAPI_ManualDispatch_GetAPICallResult( HSteamPipe hSteamPipe, SteamAPICall_t hSteamAPICall, void *pCallback, int cubCallback, int iCallbackExpected, bool *pbFailed );",
            fn(HSteamPipe, SteamApiCall, *mut c_void, i32, i32, *mut bool) -> bool;
    }

    /// `ISteamUser` (`steam_api_flat.h`).
    user: UserFns for versions::USER {
        get_steam_id: UserGetSteamId = "SteamAPI_ISteamUser_GetSteamID",
            "S_API uint64_steamid SteamAPI_ISteamUser_GetSteamID( ISteamUser* self );",
            fn(*mut ISteamUser) -> u64;
        logged_on: UserBLoggedOn = "SteamAPI_ISteamUser_BLoggedOn",
            "S_API bool SteamAPI_ISteamUser_BLoggedOn( ISteamUser* self );",
            fn(*mut ISteamUser) -> bool;
        get_player_steam_level: UserGetPlayerSteamLevel = "SteamAPI_ISteamUser_GetPlayerSteamLevel",
            "S_API int SteamAPI_ISteamUser_GetPlayerSteamLevel( ISteamUser* self );",
            fn(*mut ISteamUser) -> i32;
    }

    /// `ISteamFriends` (`steam_api_flat.h`).
    friends: FriendsFns for versions::FRIENDS {
        get_persona_name: FriendsGetPersonaName = "SteamAPI_ISteamFriends_GetPersonaName",
            "S_API const char * SteamAPI_ISteamFriends_GetPersonaName( ISteamFriends* self );",
            fn(*mut ISteamFriends) -> *const c_char;
        activate_game_overlay_invite_dialog: FriendsActivateGameOverlayInviteDialog = "SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog",
            "S_API void SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog( ISteamFriends* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamFriends, u64);
        set_rich_presence: FriendsSetRichPresence = "SteamAPI_ISteamFriends_SetRichPresence",
            "S_API bool SteamAPI_ISteamFriends_SetRichPresence( ISteamFriends* self, const char * pchKey, const char * pchValue );",
            fn(*mut ISteamFriends, *const c_char, *const c_char) -> bool;
        clear_rich_presence: FriendsClearRichPresence = "SteamAPI_ISteamFriends_ClearRichPresence",
            "S_API void SteamAPI_ISteamFriends_ClearRichPresence( ISteamFriends* self );",
            fn(*mut ISteamFriends);
        invite_user_to_game: FriendsInviteUserToGame = "SteamAPI_ISteamFriends_InviteUserToGame",
            "S_API bool SteamAPI_ISteamFriends_InviteUserToGame( ISteamFriends* self, uint64_steamid steamIDFriend, const char * pchConnectString );",
            fn(*mut ISteamFriends, u64, *const c_char) -> bool;
        get_persona_state: FriendsGetPersonaState = "SteamAPI_ISteamFriends_GetPersonaState",
            "S_API EPersonaState SteamAPI_ISteamFriends_GetPersonaState( ISteamFriends* self );",
            fn(*mut ISteamFriends) -> i32;
        get_friend_count: FriendsGetFriendCount = "SteamAPI_ISteamFriends_GetFriendCount",
            "S_API int SteamAPI_ISteamFriends_GetFriendCount( ISteamFriends* self, int iFriendFlags );",
            fn(*mut ISteamFriends, i32) -> i32;
        get_friend_by_index: FriendsGetFriendByIndex = "SteamAPI_ISteamFriends_GetFriendByIndex",
            "S_API uint64_steamid SteamAPI_ISteamFriends_GetFriendByIndex( ISteamFriends* self, int iFriend, int iFriendFlags );",
            fn(*mut ISteamFriends, i32, i32) -> u64;
        get_friend_persona_state: FriendsGetFriendPersonaState = "SteamAPI_ISteamFriends_GetFriendPersonaState",
            "S_API EPersonaState SteamAPI_ISteamFriends_GetFriendPersonaState( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64) -> i32;
        get_friend_persona_name: FriendsGetFriendPersonaName = "SteamAPI_ISteamFriends_GetFriendPersonaName",
            "S_API const char * SteamAPI_ISteamFriends_GetFriendPersonaName( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64) -> *const c_char;
        activate_game_overlay: FriendsActivateGameOverlay = "SteamAPI_ISteamFriends_ActivateGameOverlay",
            "S_API void SteamAPI_ISteamFriends_ActivateGameOverlay( ISteamFriends* self, const char * pchDialog );",
            fn(*mut ISteamFriends, *const c_char);
        activate_game_overlay_to_user: FriendsActivateGameOverlayToUser = "SteamAPI_ISteamFriends_ActivateGameOverlayToUser",
            "S_API void SteamAPI_ISteamFriends_ActivateGameOverlayToUser( ISteamFriends* self, const char * pchDialog, uint64_steamid steamID );",
            fn(*mut ISteamFriends, *const c_char, u64);
        activate_game_overlay_to_web_page: FriendsActivateGameOverlayToWebPage = "SteamAPI_ISteamFriends_ActivateGameOverlayToWebPage",
            "S_API void SteamAPI_ISteamFriends_ActivateGameOverlayToWebPage( ISteamFriends* self, const char * pchURL, EActivateGameOverlayToWebPageMode eMode );",
            fn(*mut ISteamFriends, *const c_char, i32);
        get_small_friend_avatar: FriendsGetSmallFriendAvatar = "SteamAPI_ISteamFriends_GetSmallFriendAvatar",
            "S_API int SteamAPI_ISteamFriends_GetSmallFriendAvatar( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64) -> i32;
        get_medium_friend_avatar: FriendsGetMediumFriendAvatar = "SteamAPI_ISteamFriends_GetMediumFriendAvatar",
            "S_API int SteamAPI_ISteamFriends_GetMediumFriendAvatar( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64) -> i32;
        get_large_friend_avatar: FriendsGetLargeFriendAvatar = "SteamAPI_ISteamFriends_GetLargeFriendAvatar",
            "S_API int SteamAPI_ISteamFriends_GetLargeFriendAvatar( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64) -> i32;
        request_user_information: FriendsRequestUserInformation = "SteamAPI_ISteamFriends_RequestUserInformation",
            "S_API bool SteamAPI_ISteamFriends_RequestUserInformation( ISteamFriends* self, uint64_steamid steamIDUser, bool bRequireNameOnly );",
            fn(*mut ISteamFriends, u64, bool) -> bool;
        get_friend_rich_presence: FriendsGetFriendRichPresence = "SteamAPI_ISteamFriends_GetFriendRichPresence",
            "S_API const char * SteamAPI_ISteamFriends_GetFriendRichPresence( ISteamFriends* self, uint64_steamid steamIDFriend, const char * pchKey );",
            fn(*mut ISteamFriends, u64, *const c_char) -> *const c_char;
        request_friend_rich_presence: FriendsRequestFriendRichPresence = "SteamAPI_ISteamFriends_RequestFriendRichPresence",
            "S_API void SteamAPI_ISteamFriends_RequestFriendRichPresence( ISteamFriends* self, uint64_steamid steamIDFriend );",
            fn(*mut ISteamFriends, u64);
    }

    /// `ISteamMatchmaking`, lobbies only (`steam_api_flat.h`).
    matchmaking: MatchmakingFns for versions::MATCHMAKING {
        create_lobby: MatchmakingCreateLobby = "SteamAPI_ISteamMatchmaking_CreateLobby",
            "S_API SteamAPICall_t SteamAPI_ISteamMatchmaking_CreateLobby( ISteamMatchmaking* self, ELobbyType eLobbyType, int cMaxMembers );",
            fn(*mut ISteamMatchmaking, i32, i32) -> SteamApiCall;
        join_lobby: MatchmakingJoinLobby = "SteamAPI_ISteamMatchmaking_JoinLobby",
            "S_API SteamAPICall_t SteamAPI_ISteamMatchmaking_JoinLobby( ISteamMatchmaking* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamMatchmaking, u64) -> SteamApiCall;
        leave_lobby: MatchmakingLeaveLobby = "SteamAPI_ISteamMatchmaking_LeaveLobby",
            "S_API void SteamAPI_ISteamMatchmaking_LeaveLobby( ISteamMatchmaking* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamMatchmaking, u64);
        invite_user_to_lobby: MatchmakingInviteUserToLobby = "SteamAPI_ISteamMatchmaking_InviteUserToLobby",
            "S_API bool SteamAPI_ISteamMatchmaking_InviteUserToLobby( ISteamMatchmaking* self, uint64_steamid steamIDLobby, uint64_steamid steamIDInvitee );",
            fn(*mut ISteamMatchmaking, u64, u64) -> bool;
        get_num_lobby_members: MatchmakingGetNumLobbyMembers = "SteamAPI_ISteamMatchmaking_GetNumLobbyMembers",
            "S_API int SteamAPI_ISteamMatchmaking_GetNumLobbyMembers( ISteamMatchmaking* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamMatchmaking, u64) -> i32;
        get_lobby_member_by_index: MatchmakingGetLobbyMemberByIndex = "SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex",
            "S_API uint64_steamid SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex( ISteamMatchmaking* self, uint64_steamid steamIDLobby, int iMember );",
            fn(*mut ISteamMatchmaking, u64, i32) -> u64;
        get_lobby_data: MatchmakingGetLobbyData = "SteamAPI_ISteamMatchmaking_GetLobbyData",
            "S_API const char * SteamAPI_ISteamMatchmaking_GetLobbyData( ISteamMatchmaking* self, uint64_steamid steamIDLobby, const char * pchKey );",
            fn(*mut ISteamMatchmaking, u64, *const c_char) -> *const c_char;
        set_lobby_data: MatchmakingSetLobbyData = "SteamAPI_ISteamMatchmaking_SetLobbyData",
            "S_API bool SteamAPI_ISteamMatchmaking_SetLobbyData( ISteamMatchmaking* self, uint64_steamid steamIDLobby, const char * pchKey, const char * pchValue );",
            fn(*mut ISteamMatchmaking, u64, *const c_char, *const c_char) -> bool;
        get_lobby_member_data: MatchmakingGetLobbyMemberData = "SteamAPI_ISteamMatchmaking_GetLobbyMemberData",
            "S_API const char * SteamAPI_ISteamMatchmaking_GetLobbyMemberData( ISteamMatchmaking* self, uint64_steamid steamIDLobby, uint64_steamid steamIDUser, const char * pchKey );",
            fn(*mut ISteamMatchmaking, u64, u64, *const c_char) -> *const c_char;
        set_lobby_member_data: MatchmakingSetLobbyMemberData = "SteamAPI_ISteamMatchmaking_SetLobbyMemberData",
            "S_API void SteamAPI_ISteamMatchmaking_SetLobbyMemberData( ISteamMatchmaking* self, uint64_steamid steamIDLobby, const char * pchKey, const char * pchValue );",
            fn(*mut ISteamMatchmaking, u64, *const c_char, *const c_char);
        send_lobby_chat_msg: MatchmakingSendLobbyChatMsg = "SteamAPI_ISteamMatchmaking_SendLobbyChatMsg",
            "S_API bool SteamAPI_ISteamMatchmaking_SendLobbyChatMsg( ISteamMatchmaking* self, uint64_steamid steamIDLobby, const void * pvMsgBody, int cubMsgBody );",
            fn(*mut ISteamMatchmaking, u64, *const c_void, i32) -> bool;
        get_lobby_chat_entry: MatchmakingGetLobbyChatEntry = "SteamAPI_ISteamMatchmaking_GetLobbyChatEntry",
            "S_API int SteamAPI_ISteamMatchmaking_GetLobbyChatEntry( ISteamMatchmaking* self, uint64_steamid steamIDLobby, int iChatID, CSteamID * pSteamIDUser, void * pvData, int cubData, EChatEntryType * peChatEntryType );",
            fn(*mut ISteamMatchmaking, u64, i32, *mut u64, *mut c_void, i32, *mut i32) -> i32;
        set_lobby_member_limit: MatchmakingSetLobbyMemberLimit = "SteamAPI_ISteamMatchmaking_SetLobbyMemberLimit",
            "S_API bool SteamAPI_ISteamMatchmaking_SetLobbyMemberLimit( ISteamMatchmaking* self, uint64_steamid steamIDLobby, int cMaxMembers );",
            fn(*mut ISteamMatchmaking, u64, i32) -> bool;
        get_lobby_member_limit: MatchmakingGetLobbyMemberLimit = "SteamAPI_ISteamMatchmaking_GetLobbyMemberLimit",
            "S_API int SteamAPI_ISteamMatchmaking_GetLobbyMemberLimit( ISteamMatchmaking* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamMatchmaking, u64) -> i32;
        set_lobby_type: MatchmakingSetLobbyType = "SteamAPI_ISteamMatchmaking_SetLobbyType",
            "S_API bool SteamAPI_ISteamMatchmaking_SetLobbyType( ISteamMatchmaking* self, uint64_steamid steamIDLobby, ELobbyType eLobbyType );",
            fn(*mut ISteamMatchmaking, u64, i32) -> bool;
        set_lobby_joinable: MatchmakingSetLobbyJoinable = "SteamAPI_ISteamMatchmaking_SetLobbyJoinable",
            "S_API bool SteamAPI_ISteamMatchmaking_SetLobbyJoinable( ISteamMatchmaking* self, uint64_steamid steamIDLobby, bool bLobbyJoinable );",
            fn(*mut ISteamMatchmaking, u64, bool) -> bool;
        get_lobby_owner: MatchmakingGetLobbyOwner = "SteamAPI_ISteamMatchmaking_GetLobbyOwner",
            "S_API uint64_steamid SteamAPI_ISteamMatchmaking_GetLobbyOwner( ISteamMatchmaking* self, uint64_steamid steamIDLobby );",
            fn(*mut ISteamMatchmaking, u64) -> u64;
    }

    /// `ISteamApps` (`steam_api_flat.h`).
    apps: AppsFns for versions::APPS {
        is_subscribed: AppsBIsSubscribed = "SteamAPI_ISteamApps_BIsSubscribed",
            "S_API bool SteamAPI_ISteamApps_BIsSubscribed( ISteamApps* self );",
            fn(*mut ISteamApps) -> bool;
        get_current_game_language: AppsGetCurrentGameLanguage = "SteamAPI_ISteamApps_GetCurrentGameLanguage",
            "S_API const char * SteamAPI_ISteamApps_GetCurrentGameLanguage( ISteamApps* self );",
            fn(*mut ISteamApps) -> *const c_char;
        get_launch_command_line: AppsGetLaunchCommandLine = "SteamAPI_ISteamApps_GetLaunchCommandLine",
            "S_API int SteamAPI_ISteamApps_GetLaunchCommandLine( ISteamApps* self, char * pszCommandLine, int cubCommandLine );",
            fn(*mut ISteamApps, *mut c_char, i32) -> i32;
    }

    /// `ISteamUtils` (`steam_api_flat.h`).
    utils: UtilsFns for versions::UTILS {
        get_app_id: UtilsGetAppId = "SteamAPI_ISteamUtils_GetAppID",
            "S_API uint32 SteamAPI_ISteamUtils_GetAppID( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> u32;
        is_running_on_steam_hardware: UtilsIsRunningOnSteamHardware = "SteamAPI_ISteamUtils_IsRunningOnSteamHardware",
            "S_API ESteamHardwareType SteamAPI_ISteamUtils_IsRunningOnSteamHardware( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> i32;
        get_steam_hardware_default_config: UtilsGetSteamHardwareDefaultConfig = "SteamAPI_ISteamUtils_GetSteamHardwareDefaultConfig",
            "S_API ESteamHardwareDefaultConfig SteamAPI_ISteamUtils_GetSteamHardwareDefaultConfig( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> i32;
        is_running_under_proton: UtilsIsRunningUnderProton = "SteamAPI_ISteamUtils_IsRunningUnderProton",
            "S_API bool SteamAPI_ISteamUtils_IsRunningUnderProton( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> bool;
        get_steam_ui_language: UtilsGetSteamUiLanguage = "SteamAPI_ISteamUtils_GetSteamUILanguage",
            "S_API const char * SteamAPI_ISteamUtils_GetSteamUILanguage( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> *const c_char;
        is_overlay_enabled: UtilsIsOverlayEnabled = "SteamAPI_ISteamUtils_IsOverlayEnabled",
            "S_API bool SteamAPI_ISteamUtils_IsOverlayEnabled( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> bool;
        is_steam_in_big_picture_mode: UtilsIsSteamInBigPictureMode = "SteamAPI_ISteamUtils_IsSteamInBigPictureMode",
            "S_API bool SteamAPI_ISteamUtils_IsSteamInBigPictureMode( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> bool;
        set_overlay_notification_position: UtilsSetOverlayNotificationPosition = "SteamAPI_ISteamUtils_SetOverlayNotificationPosition",
            "S_API void SteamAPI_ISteamUtils_SetOverlayNotificationPosition( ISteamUtils* self, ENotificationPosition eNotificationPosition );",
            fn(*mut ISteamUtils, i32);
        set_overlay_notification_inset: UtilsSetOverlayNotificationInset = "SteamAPI_ISteamUtils_SetOverlayNotificationInset",
            "S_API void SteamAPI_ISteamUtils_SetOverlayNotificationInset( ISteamUtils* self, int nHorizontalInset, int nVerticalInset );",
            fn(*mut ISteamUtils, i32, i32);
        get_server_real_time: UtilsGetServerRealTime = "SteamAPI_ISteamUtils_GetServerRealTime",
            "S_API uint32 SteamAPI_ISteamUtils_GetServerRealTime( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> u32;
        get_ip_country: UtilsGetIpCountry = "SteamAPI_ISteamUtils_GetIPCountry",
            "S_API const char * SteamAPI_ISteamUtils_GetIPCountry( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> *const c_char;
        get_image_size: UtilsGetImageSize = "SteamAPI_ISteamUtils_GetImageSize",
            "S_API bool SteamAPI_ISteamUtils_GetImageSize( ISteamUtils* self, int iImage, uint32 * pnWidth, uint32 * pnHeight );",
            fn(*mut ISteamUtils, i32, *mut u32, *mut u32) -> bool;
        get_image_rgba: UtilsGetImageRgba = "SteamAPI_ISteamUtils_GetImageRGBA",
            "S_API bool SteamAPI_ISteamUtils_GetImageRGBA( ISteamUtils* self, int iImage, uint8 * pubDest, int nDestBufferSize );",
            fn(*mut ISteamUtils, i32, *mut u8, i32) -> bool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address of a real function, for symbols a test resolves but never
    /// calls.
    extern "C" fn never_called() {}

    fn resolve_all_but(missing: &'static str) -> Result<Fns, InitError> {
        Fns::resolve(&mut |name| {
            if name == missing {
                core::ptr::null_mut()
            } else {
                never_called as *mut c_void
            }
        })
    }

    #[test]
    fn a_missing_function_is_no_symbol_naming_it() {
        let err = resolve_all_but("SteamAPI_ManualDispatch_GetNextCallback").unwrap_err();
        assert_eq!(
            err,
            InitError::NoSymbol("SteamAPI_ManualDispatch_GetNextCallback")
        );
    }

    #[test]
    fn a_missing_accessor_is_no_symbol_naming_it() {
        let err = resolve_all_but(versions::UTILS.accessor).unwrap_err();
        assert_eq!(err, InitError::NoSymbol("SteamAPI_SteamUtils_v011"));
    }

    #[test]
    fn every_symbol_is_resolved_and_every_declaration_names_its_symbol() {
        let mut asked = Vec::new();
        Fns::resolve(&mut |name| {
            asked.push(name);
            never_called as *mut c_void
        })
        .unwrap();
        for bound in BINDINGS {
            assert!(
                asked.contains(&bound.symbol),
                "{} never resolved",
                bound.symbol
            );
            // A declaration pasted under the wrong symbol would pass the drift
            // gate for the other function and leave this one unchecked.
            assert!(
                bound.declaration.contains(&format!(" {}(", bound.symbol)),
                "{bound:?}"
            );
        }
        for interface in INTERFACES {
            assert!(asked.contains(&interface.accessor), "{interface:?}");
        }
    }
}
