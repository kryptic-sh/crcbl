//! The fake library the unit tests run against. Test builds only.
//!
//! A [`Lib`] whose function pointers are Rust `extern "C"` functions reading
//! and writing a thread-local [`Script`]: what init answers, whether each
//! accessor returns an interface or null, what the callback pipe yields, and
//! counters for every call that matters. The pointers have the exact types
//! `ffi::manifest` declares, so the code under test calls through the same
//! signatures it calls the real library through.
//!
//! **What this cannot prove** is that those signatures match C: a Rust callee
//! compiled from the same declaration agrees with its caller by construction.
//! That is the drift gate's job for types, and a real client's for the calling
//! convention.
//!
//! A function pointer captures nothing, hence the thread-local; each unit test
//! runs on its own thread (or, under nextest, its own process), so each starts
//! from [`Script::default`]. Each test also leaks its own [`Lib`], so the
//! one-live-`Steam` guard is per test.

mod input;
mod inventory;
mod keyboard;
mod ownership;
mod recording;
mod tickets;
mod workshop;

pub(crate) use input::{FakeInput, FakePad};
pub(crate) use inventory::FakeInventory;
pub(crate) use keyboard::FakeKeyboard;
pub(crate) use ownership::{FakeApps, FakeBeta, FakeRemotePlay, FakeSession};
pub(crate) use recording::{FakeScreenshots, FakeTimeline};
pub(crate) use tickets::FakeTickets;
pub(crate) use workshop::FakeWorkshop;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::{c_char, c_void},
    ptr::NonNull,
    sync::Mutex,
};

use crate::ffi::{
    HSteamListenSocket, HSteamNetConnection, HSteamPipe, ISteamApps, ISteamFriends,
    ISteamMatchmaking, ISteamNetworkingSockets, ISteamNetworkingUtils, ISteamRemoteStorage,
    ISteamUser, ISteamUserStats, ISteamUtils, Lib, SteamApiCall, SteamErrMsg,
    manifest::{
        AppsFns, DispatchFns, Fns, FriendsFns, LifecycleFns, MatchmakingFns, NetFns, NetUtilsFns,
        RemoteStorageFns, UserFns, UserStatsFns, UtilsFns,
    },
    structs::CallbackMsg,
    structs::{
        SteamNetConnectionInfo, SteamNetworkingIdentity, SteamNetworkingMessage,
        SteamRelayNetworkStatus,
    },
};

/// The pipe the fake hands out.
pub(crate) const PIPE: HSteamPipe = 7;
/// The local user the fake reports.
pub(crate) const STEAM_ID: u64 = 76_561_197_960_287_930;

/// One message the fake pipe will yield.
#[derive(Debug, Clone)]
pub(crate) struct FakeMsg {
    id: i32,
    /// `None` hands over a null `m_pubParam`.
    payload: Option<Vec<u8>>,
    size: i32,
}

impl FakeMsg {
    /// A message whose size is its payload's length.
    pub(crate) fn payload(id: i32, bytes: Vec<u8>) -> Self {
        let size = i32::try_from(bytes.len()).unwrap();
        Self {
            id,
            payload: Some(bytes),
            size,
        }
    }

    /// A message with a null payload pointer and the given size.
    pub(crate) fn null(id: i32, size: i32) -> Self {
        Self {
            id,
            payload: None,
            size,
        }
    }

    /// Appends a byte to the payload, growing the reported size with it.
    pub(crate) fn push_byte(&mut self, byte: u8) {
        if let Some(bytes) = &mut self.payload {
            bytes.push(byte);
            self.size += 1;
        }
    }
}

/// How often each fake function ran, and each protocol violation seen.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Calls {
    pub(crate) init: u32,
    pub(crate) shutdown: u32,
    pub(crate) dispatch_init: u32,
    pub(crate) run_frame: u32,
    pub(crate) release_thread_memory: u32,
    /// `GetNextCallback` answering true.
    pub(crate) next_true: u32,
    pub(crate) free: u32,
    /// `FreeLastCallback` with no message outstanding.
    pub(crate) free_without_next: u32,
    /// `GetNextCallback` while the previous message was never freed.
    pub(crate) next_while_unfreed: u32,
    /// A dispatch call on a pipe the fake never handed out.
    pub(crate) wrong_pipe: u32,
    pub(crate) clear_rich_presence: u32,
}

/// What the fake answers, and what it has seen.
#[derive(Debug)]
pub(crate) struct Script {
    pub(crate) init_result: i32,
    pub(crate) init_message: Vec<u8>,
    /// The handshake `SteamInternal_SteamAPI_Init` received, through its
    /// double NUL.
    pub(crate) handshake: Option<Vec<u8>>,
    /// The one interface accessor that answers null, by accessor name.
    pub(crate) null_accessor: Option<&'static str>,
    /// What `SteamAPI_RestartAppIfNecessary` answers.
    pub(crate) restart: bool,
    /// Every app id `SteamAPI_RestartAppIfNecessary` was asked about.
    pub(crate) restart_asked: Vec<u32>,
    pub(crate) logged_on: bool,
    pub(crate) steam_level: i32,
    pub(crate) app_id: u32,
    pub(crate) hardware: i32,
    pub(crate) default_config: i32,
    pub(crate) proton: bool,
    pub(crate) overlay_enabled: bool,
    pub(crate) big_picture: bool,
    pub(crate) server_time: u32,
    pub(crate) subscribed: bool,
    /// Every `SetOverlayNotificationPosition` argument, in order.
    pub(crate) notification_positions: Vec<i32>,
    /// Every `SetOverlayNotificationInset` pair, in order.
    pub(crate) notification_insets: Vec<(i32, i32)>,
    /// Every string call answers null instead of [`STRING`]'s buffer.
    pub(crate) null_string: bool,
    /// Every bool-returning call the fake makes answers `false`.
    pub(crate) refuse: bool,
    /// What `GetLaunchCommandLine` copies out, before its NUL.
    pub(crate) launch_line: Vec<u8>,
    /// Every `(key, value)` `SetRichPresence` received.
    pub(crate) rich_presence: Vec<(String, String)>,
    /// Every lobby `ActivateGameOverlayInviteDialog` opened for.
    pub(crate) invite_dialogs: Vec<u64>,
    /// Every `(friend, connect)` `InviteUserToGame` sent.
    pub(crate) game_invites: Vec<(u64, String)>,
    /// The handle the next `CreateLobby` or `JoinLobby` answers; `0` is
    /// `k_uAPICallInvalid`.
    pub(crate) next_call: SteamApiCall,
    /// Every `(type, max members)` `CreateLobby` received.
    pub(crate) created: Vec<(i32, i32)>,
    /// Every lobby `JoinLobby` was asked for.
    pub(crate) joined: Vec<u64>,
    /// Every lobby `LeaveLobby` left, in order.
    pub(crate) left: Vec<u64>,
    /// What `GetAPICallResult` answers per call: the bytes it writes, and
    /// whether it reports an IO failure. A call with no entry answers
    /// `false`.
    pub(crate) results: Vec<(SteamApiCall, Vec<u8>, bool)>,
    /// Every `(call, size, id)` `GetAPICallResult` was asked for.
    pub(crate) results_asked: Vec<(SteamApiCall, i32, i32)>,
    /// What `GetLobbyOwner` answers.
    pub(crate) lobby_owner: u64,
    /// What `GetLobbyMemberByIndex` walks.
    pub(crate) members: Vec<u64>,
    /// What `GetLobbyMemberLimit` answers.
    pub(crate) member_limit: i32,
    /// Every lobby-side write: `(call, lobby, key or friend, value)`.
    pub(crate) lobby_writes: Vec<(&'static str, u64, String, String)>,
    /// Every body `SendLobbyChatMsg` sent.
    pub(crate) chat_sent: Vec<Vec<u8>>,
    /// What `GetLobbyChatEntry` answers: sender, entry type, body, and the
    /// count it returns (normally the body's length).
    pub(crate) chat_entry: (u64, i32, Vec<u8>, i32),
    /// What `GetPersonaState` and `GetFriendPersonaState` answer.
    pub(crate) persona_state: i32,
    /// The users `GetFriendByIndex` walks, and the flags each call passed.
    pub(crate) friends: Vec<u64>,
    pub(crate) friend_flags: Vec<i32>,
    /// What `RequestUserInformation` answers, and each `(user, name only)`.
    pub(crate) info_pending: bool,
    pub(crate) info_requests: Vec<(u64, bool)>,
    /// Every friend `RequestFriendRichPresence` asked about.
    pub(crate) presence_requests: Vec<u64>,
    /// Every overlay call: `(function, argument, user or mode)`.
    pub(crate) overlays: Vec<(&'static str, String, u64)>,
    /// The small, medium and large avatar handles.
    pub(crate) avatar_handles: [i32; 3],
    /// The one image every handle names: width, height, pixels. `None`
    /// makes `GetImageSize` answer false.
    pub(crate) image: Option<(u32, u32, Vec<u8>)>,
    /// Every `(function, handle, buffer size)` the image calls received.
    pub(crate) image_calls: Vec<(&'static str, i32, i32)>,
    /// The networking loop.
    pub(crate) net: FakeNet,
    pub(crate) cloud: FakeCloud,
    pub(crate) voice: FakeVoice,
    pub(crate) stats: FakeStats,
    pub(crate) input: FakeInput,
    pub(crate) keyboard: FakeKeyboard,
    pub(crate) screenshots: FakeScreenshots,
    pub(crate) timeline: FakeTimeline,
    /// Slice 11's half of `ISteamApps`.
    pub(crate) apps_extra: FakeApps,
    pub(crate) remote_play: FakeRemotePlay,
    pub(crate) tickets: FakeTickets,
    pub(crate) workshop: FakeWorkshop,
    pub(crate) inventory: FakeInventory,
    /// What the pipe yields, in order.
    pub(crate) queue: VecDeque<FakeMsg>,
    /// The outstanding message's payload, alive until `FreeLastCallback` —
    /// so a pump that read it after the free would read freed memory, which
    /// Miri reports.
    current: Option<Option<Vec<u8>>>,
    pub(crate) calls: Calls,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            init_result: crate::ffi::init_result::OK,
            init_message: Vec::new(),
            handshake: None,
            null_accessor: None,
            restart: false,
            restart_asked: Vec::new(),
            logged_on: true,
            steam_level: 0,
            app_id: 480,
            hardware: 0,
            default_config: 0,
            proton: false,
            overlay_enabled: false,
            big_picture: false,
            server_time: 0,
            subscribed: false,
            notification_positions: Vec::new(),
            notification_insets: Vec::new(),
            null_string: false,
            refuse: false,
            launch_line: Vec::new(),
            rich_presence: Vec::new(),
            invite_dialogs: Vec::new(),
            game_invites: Vec::new(),
            next_call: 0,
            created: Vec::new(),
            joined: Vec::new(),
            left: Vec::new(),
            results: Vec::new(),
            results_asked: Vec::new(),
            lobby_owner: 0,
            members: Vec::new(),
            member_limit: 0,
            lobby_writes: Vec::new(),
            chat_sent: Vec::new(),
            chat_entry: (0, 0, Vec::new(), 0),
            persona_state: 0,
            friends: Vec::new(),
            friend_flags: Vec::new(),
            info_pending: false,
            info_requests: Vec::new(),
            presence_requests: Vec::new(),
            overlays: Vec::new(),
            avatar_handles: [0; 3],
            image: None,
            image_calls: Vec::new(),
            net: FakeNet::default(),
            cloud: FakeCloud::default(),
            voice: FakeVoice::default(),
            stats: FakeStats::default(),
            input: FakeInput::default(),
            keyboard: FakeKeyboard::default(),
            screenshots: FakeScreenshots::default(),
            timeline: FakeTimeline::default(),
            apps_extra: FakeApps::default(),
            remote_play: FakeRemotePlay::default(),
            tickets: FakeTickets::default(),
            workshop: FakeWorkshop::default(),
            inventory: FakeInventory::default(),
            queue: VecDeque::new(),
            current: None,
            calls: Calls::default(),
        }
    }
}

impl Script {
    /// Overwrites the one string buffer every string call answers from, in
    /// place — as Steam reuses its buffer — with `text` and a NUL.
    pub(crate) fn set_string(&mut self, text: &[u8]) {
        assert!(
            text.len() < STRING_CAPACITY && !text.contains(&0),
            "{text:?}"
        );
        let mut buffer = [0; STRING_CAPACITY];
        buffer[..text.len()].copy_from_slice(text);
        STRING.with(|cell| cell.set(buffer));
    }
}

/// The string buffer's size, NUL included.
const STRING_CAPACITY: usize = 64;

thread_local! {
    static SCRIPT: RefCell<Script> = RefCell::new(Script::default());
    /// The one buffer every string call answers from. A `Cell`, apart from the
    /// script, so a pointer into it stays valid while the script is borrowed
    /// and sees every later overwrite.
    static STRING: Cell<[u8; STRING_CAPACITY]> = const { Cell::new([0; STRING_CAPACITY]) };
}

/// A pointer into [`STRING`], or null when the script says so.
fn fake_string() -> *const c_char {
    if script(|s| s.null_string) {
        core::ptr::null()
    } else {
        STRING.with(|cell| cell.as_ptr().cast::<c_char>().cast_const())
    }
}

/// Reads or edits this thread's script.
pub(crate) fn script<R>(f: impl FnOnce(&mut Script) -> R) -> R {
    SCRIPT.with(|cell| f(&mut cell.borrow_mut()))
}

/// Every fake library this process has made. The real library is leaked and
/// kept reachable from `load::real`'s static; this does the same for the
/// fakes, so Miri's leak check stays on for everything else the tests do.
static FAKES: Mutex<Vec<&'static Lib>> = Mutex::new(Vec::new());

/// A fresh fake library, leaked like the real one.
pub(crate) fn fake_lib() -> &'static Lib {
    let lib: &'static Lib = Box::leak(Box::new(Lib::new(Fns {
        lifecycle: LifecycleFns {
            init: fake_init,
            shutdown: fake_shutdown,
            get_pipe: fake_get_pipe,
            release_thread_memory: fake_release_thread_memory,
            restart_app_if_necessary: fake_restart_app_if_necessary,
        },
        dispatch: DispatchFns {
            init: fake_dispatch_init,
            run_frame: fake_run_frame,
            get_next_callback: fake_get_next_callback,
            free_last_callback: fake_free_last_callback,
            get_api_call_result: fake_get_api_call_result,
        },
        user: UserFns {
            accessor: fake_user_accessor,
            get_steam_id: fake_get_steam_id,
            logged_on: fake_logged_on,
            get_player_steam_level: fake_get_player_steam_level,
            start_voice_recording: fake_start_voice_recording,
            stop_voice_recording: fake_stop_voice_recording,
            get_available_voice: fake_get_available_voice,
            get_voice: fake_get_voice,
            decompress_voice: fake_decompress_voice,
            get_voice_optimal_sample_rate: fake_get_voice_optimal_sample_rate,
            get_auth_session_ticket: tickets::fake_get_auth_session_ticket,
            get_auth_ticket_for_web_api: tickets::fake_get_auth_ticket_for_web_api,
            begin_auth_session: tickets::fake_begin_auth_session,
            end_auth_session: tickets::fake_end_auth_session,
            cancel_auth_ticket: tickets::fake_cancel_auth_ticket,
            user_has_license_for_app: tickets::fake_user_has_license_for_app,
            request_encrypted_app_ticket: tickets::fake_request_encrypted_app_ticket,
            get_encrypted_app_ticket: tickets::fake_get_encrypted_app_ticket,
        },
        net: NetFns {
            accessor: fake_net_accessor,
            create_listen_socket_p2p: fake_create_listen_socket_p2p,
            connect_p2p: fake_connect_p2p,
            accept_connection: fake_accept_connection,
            close_connection: fake_close_connection,
            close_listen_socket: fake_close_listen_socket,
            send_message_to_connection: fake_send_message_to_connection,
            receive_messages_on_connection: fake_receive_messages_on_connection,
            get_connection_info: fake_get_connection_info,
            release_message: fake_release_message,
        },
        net_utils: NetUtilsFns {
            accessor: fake_net_utils_accessor,
            init_relay_network_access: fake_init_relay_network_access,
            get_relay_network_status: fake_get_relay_network_status,
        },
        friends: FriendsFns {
            accessor: fake_friends_accessor,
            get_persona_name: fake_get_persona_name,
            activate_game_overlay_invite_dialog: fake_activate_game_overlay_invite_dialog,
            set_rich_presence: fake_set_rich_presence,
            clear_rich_presence: fake_clear_rich_presence,
            invite_user_to_game: fake_invite_user_to_game,
            get_persona_state: fake_get_persona_state,
            get_friend_count: fake_get_friend_count,
            get_friend_by_index: fake_get_friend_by_index,
            get_friend_persona_state: fake_get_friend_persona_state,
            get_friend_persona_name: fake_get_friend_persona_name,
            activate_game_overlay: fake_activate_game_overlay,
            activate_game_overlay_to_user: fake_activate_game_overlay_to_user,
            activate_game_overlay_to_web_page: fake_activate_game_overlay_to_web_page,
            get_small_friend_avatar: fake_get_small_friend_avatar,
            get_medium_friend_avatar: fake_get_medium_friend_avatar,
            get_large_friend_avatar: fake_get_large_friend_avatar,
            request_user_information: fake_request_user_information,
            get_friend_rich_presence: fake_get_friend_rich_presence,
            request_friend_rich_presence: fake_request_friend_rich_presence,
        },
        matchmaking: MatchmakingFns {
            accessor: fake_matchmaking_accessor,
            create_lobby: fake_create_lobby,
            join_lobby: fake_join_lobby,
            leave_lobby: fake_leave_lobby,
            invite_user_to_lobby: fake_invite_user_to_lobby,
            get_num_lobby_members: fake_get_num_lobby_members,
            get_lobby_member_by_index: fake_get_lobby_member_by_index,
            get_lobby_data: fake_get_lobby_data,
            set_lobby_data: fake_set_lobby_data,
            get_lobby_member_data: fake_get_lobby_member_data,
            set_lobby_member_data: fake_set_lobby_member_data,
            send_lobby_chat_msg: fake_send_lobby_chat_msg,
            get_lobby_chat_entry: fake_get_lobby_chat_entry,
            set_lobby_member_limit: fake_set_lobby_member_limit,
            get_lobby_member_limit: fake_get_lobby_member_limit,
            set_lobby_type: fake_set_lobby_type,
            set_lobby_joinable: fake_set_lobby_joinable,
            get_lobby_owner: fake_get_lobby_owner,
        },
        remote_storage: RemoteStorageFns {
            accessor: fake_remote_storage_accessor,
            file_write: fake_file_write,
            file_read: fake_file_read,
            file_delete: fake_file_delete,
            file_exists: fake_file_exists,
            get_file_size: fake_get_file_size,
            get_file_count: fake_get_file_count,
            get_file_name_and_size: fake_get_file_name_and_size,
            get_quota: fake_get_quota,
            is_cloud_enabled_for_account: fake_is_cloud_enabled_for_account,
            is_cloud_enabled_for_app: fake_is_cloud_enabled_for_app,
            get_local_file_change_count: fake_get_local_file_change_count,
            get_local_file_change: fake_get_local_file_change,
            begin_file_write_batch: fake_begin_file_write_batch,
            end_file_write_batch: fake_end_file_write_batch,
        },
        user_stats: UserStatsFns {
            accessor: fake_user_stats_accessor,
            get_stat_i32: fake_get_stat_i32,
            get_stat_f32: fake_get_stat_f32,
            set_stat_i32: fake_set_stat_i32,
            set_stat_f32: fake_set_stat_f32,
            set_achievement: fake_set_achievement,
            clear_achievement: fake_clear_achievement,
            get_achievement_and_unlock_time: fake_get_achievement_and_unlock_time,
            store_stats: fake_store_stats,
            find_or_create_leaderboard: fake_find_or_create_leaderboard,
            find_leaderboard: fake_find_leaderboard,
            download_leaderboard_entries: fake_download_leaderboard_entries,
            get_downloaded_leaderboard_entry: fake_get_downloaded_leaderboard_entry,
            upload_leaderboard_score: fake_upload_leaderboard_score,
        },
        input: input::FNS,
        screenshots: recording::SCREENSHOTS,
        timeline: recording::TIMELINE,
        ugc: workshop::UGC,
        inventory: inventory::INVENTORY,
        apps: AppsFns {
            accessor: fake_apps_accessor,
            is_subscribed: fake_is_subscribed,
            get_current_game_language: fake_get_current_game_language,
            get_launch_command_line: fake_get_launch_command_line,
            is_low_violence: ownership::fake_is_low_violence,
            is_vac_banned: ownership::fake_is_vac_banned,
            is_subscribed_app: ownership::fake_is_subscribed_app,
            is_dlc_installed: ownership::fake_is_dlc_installed,
            get_earliest_purchase_unix_time: ownership::fake_get_earliest_purchase_unix_time,
            is_subscribed_from_free_weekend: ownership::fake_is_subscribed_from_free_weekend,
            get_dlc_count: ownership::fake_get_dlc_count,
            get_dlc_data_by_index: ownership::fake_get_dlc_data_by_index,
            install_dlc: ownership::fake_install_dlc,
            uninstall_dlc: ownership::fake_uninstall_dlc,
            get_current_beta_name: ownership::fake_get_current_beta_name,
            mark_content_corrupt: ownership::fake_mark_content_corrupt,
            get_app_install_dir: ownership::fake_get_app_install_dir,
            is_app_installed: ownership::fake_is_app_installed,
            get_app_owner: ownership::fake_get_app_owner,
            get_app_build_id: ownership::fake_get_app_build_id,
            get_file_details: ownership::fake_get_file_details,
            is_subscribed_from_family_sharing: ownership::fake_is_subscribed_from_family_sharing,
            get_num_betas: ownership::fake_get_num_betas,
            get_beta_info: ownership::fake_get_beta_info,
            set_active_beta: ownership::fake_set_active_beta,
        },
        remote_play: ownership::REMOTE_PLAY,
        utils: UtilsFns {
            accessor: fake_utils_accessor,
            get_app_id: fake_get_app_id,
            is_running_on_steam_hardware: fake_is_running_on_steam_hardware,
            get_steam_hardware_default_config: fake_get_steam_hardware_default_config,
            is_running_under_proton: fake_is_running_under_proton,
            get_steam_ui_language: fake_get_steam_ui_language,
            is_overlay_enabled: fake_is_overlay_enabled,
            is_steam_in_big_picture_mode: fake_is_steam_in_big_picture_mode,
            set_overlay_notification_position: fake_set_overlay_notification_position,
            set_overlay_notification_inset: fake_set_overlay_notification_inset,
            get_server_real_time: fake_get_server_real_time,
            get_ip_country: fake_get_ip_country,
            get_image_size: fake_get_image_size,
            get_image_rgba: fake_get_image_rgba,
            show_gamepad_text_input: keyboard::fake_show_gamepad_text_input,
            get_entered_gamepad_text_length: keyboard::fake_get_entered_gamepad_text_length,
            get_entered_gamepad_text_input: keyboard::fake_get_entered_gamepad_text_input,
            dismiss_gamepad_text_input: keyboard::fake_dismiss_gamepad_text_input,
            show_floating_gamepad_text_input: keyboard::fake_show_floating_gamepad_text_input,
            dismiss_floating_gamepad_text_input: keyboard::fake_dismiss_floating_gamepad_text_input,
        },
    })));
    FAKES.lock().unwrap().push(lib);
    lib
}

/// A non-null interface pointer the fake never dereferences.
fn sentinel() -> *mut c_void {
    NonNull::<u64>::dangling().as_ptr().cast()
}

/// What an accessor answers: the sentinel, or null for the one the script
/// names.
fn accessor(name: &str) -> *mut c_void {
    if script(|s| s.null_accessor == Some(name)) {
        core::ptr::null_mut()
    } else {
        sentinel()
    }
}

unsafe extern "C" fn fake_init(
    versions: *const core::ffi::c_char,
    message: *mut SteamErrMsg,
) -> i32 {
    // SAFETY: the caller passes a double-NUL-terminated list; read up to and
    // including the second NUL of a pair.
    let handshake = unsafe {
        let mut bytes = Vec::new();
        let mut at = versions.cast::<u8>();
        loop {
            let byte = *at;
            bytes.push(byte);
            if byte == 0 && (bytes.len() == 1 || bytes[bytes.len() - 2] == 0) {
                break;
            }
            at = at.add(1);
        }
        bytes
    };
    script(|s| {
        s.calls.init += 1;
        s.handshake = Some(handshake);
        // SAFETY: the caller passes a writable `SteamErrMsg`.
        let out = unsafe { &mut *message };
        for (slot, &byte) in out.iter_mut().zip(&s.init_message) {
            *slot = core::ffi::c_char::from_ne_bytes([byte]);
        }
        s.init_result
    })
}

unsafe extern "C" fn fake_shutdown() {
    script(|s| s.calls.shutdown += 1);
}

unsafe extern "C" fn fake_get_pipe() -> HSteamPipe {
    PIPE
}

unsafe extern "C" fn fake_release_thread_memory() {
    script(|s| s.calls.release_thread_memory += 1);
}

unsafe extern "C" fn fake_dispatch_init() {
    script(|s| s.calls.dispatch_init += 1);
}

unsafe extern "C" fn fake_run_frame(pipe: HSteamPipe) {
    script(|s| {
        s.calls.run_frame += 1;
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
    });
}

unsafe extern "C" fn fake_get_next_callback(pipe: HSteamPipe, msg: *mut CallbackMsg) -> bool {
    script(|s| {
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
        if s.current.is_some() {
            s.calls.next_while_unfreed += 1;
        }
        let Some(next) = s.queue.pop_front() else {
            return false;
        };
        s.calls.next_true += 1;
        let current = s.current.insert(next.payload);
        let param = current
            .as_mut()
            .map_or(core::ptr::null_mut(), |bytes| bytes.as_mut_ptr());
        // SAFETY: the caller passes a writable `CallbackMsg_t`.
        unsafe {
            msg.write(CallbackMsg {
                steam_user: 1,
                callback: next.id,
                param,
                param_size: next.size,
            });
        }
        true
    })
}

unsafe extern "C" fn fake_free_last_callback(pipe: HSteamPipe) {
    script(|s| {
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
        s.calls.free += 1;
        if s.current.take().is_none() {
            s.calls.free_without_next += 1;
        }
    });
}

unsafe extern "C" fn fake_restart_app_if_necessary(app: u32) -> bool {
    script(|s| {
        s.restart_asked.push(app);
        s.restart
    })
}

unsafe extern "C" fn fake_user_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::USER.accessor)
}

unsafe extern "C" fn fake_get_steam_id(_: *mut ISteamUser) -> u64 {
    STEAM_ID
}

unsafe extern "C" fn fake_logged_on(_: *mut ISteamUser) -> bool {
    script(|s| s.logged_on)
}

unsafe extern "C" fn fake_get_player_steam_level(_: *mut ISteamUser) -> i32 {
    script(|s| s.steam_level)
}

/// The fake microphone and decoder.
#[derive(Debug, Default)]
pub(crate) struct FakeVoice {
    /// `start` and `stop`, in the order they were called.
    pub(crate) log: Vec<&'static str>,
    /// Between a start and the end of the tail after a stop.
    pub(crate) recording: bool,
    /// A stop was called: once `packets` is empty, recording ends.
    pub(crate) stopping: bool,
    /// Compressed packets the microphone has, oldest first.
    pub(crate) packets: VecDeque<Vec<u8>>,
    /// What `GetAvailableVoice` answers instead, when set.
    pub(crate) available_result: Option<i32>,
    /// `GetVoice` answers `BufferTooSmall` this many more times.
    pub(crate) too_small: u32,
    /// How many times `GetAvailableVoice` ran.
    pub(crate) available_calls: u32,
    /// Every buffer size `GetVoice` was offered.
    pub(crate) offered: Vec<u32>,
    /// Calls that passed a deprecated uncompressed argument that was not
    /// false, null or zero, or asked for uncompressed data.
    pub(crate) deprecated_misuse: u32,
    /// The PCM bytes `DecompressVoice` produces.
    pub(crate) pcm: Vec<u8>,
    /// What `DecompressVoice` answers instead, when set.
    pub(crate) decompress_result: Option<i32>,
    /// `DecompressVoice` reports a size one byte larger than any buffer.
    pub(crate) decompress_never_fits: bool,
    /// Every `(buffer size, sample rate)` `DecompressVoice` was given.
    pub(crate) decompressions: Vec<(u32, u32)>,
    pub(crate) optimal_rate: u32,
}

unsafe extern "C" fn fake_start_voice_recording(_: *mut ISteamUser) {
    script(|s| {
        s.voice.log.push("start");
        s.voice.recording = true;
        s.voice.stopping = false;
    });
}

unsafe extern "C" fn fake_stop_voice_recording(_: *mut ISteamUser) {
    script(|s| {
        s.voice.log.push("stop");
        s.voice.stopping = true;
    });
}

/// `k_EVoiceResultNotRecording`, `NoData`, `BufferTooSmall`.
const VOICE_NOT_RECORDING: i32 = 2;
const VOICE_NO_DATA: i32 = 3;
const VOICE_BUFFER_TOO_SMALL: i32 = 4;

unsafe extern "C" fn fake_get_available_voice(
    _: *mut ISteamUser,
    compressed: *mut u32,
    uncompressed: *mut u32,
    rate: u32,
) -> i32 {
    script(|s| {
        s.voice.available_calls += 1;
        if !uncompressed.is_null() || rate != 0 {
            s.voice.deprecated_misuse += 1;
        }
        if let Some(result) = s.voice.available_result {
            return result;
        }
        if !s.voice.recording {
            return VOICE_NOT_RECORDING;
        }
        let Some(front) = s.voice.packets.front() else {
            if s.voice.stopping {
                s.voice.recording = false;
                return VOICE_NOT_RECORDING;
            }
            return VOICE_NO_DATA;
        };
        // SAFETY: the caller passes a writable `uint32`.
        unsafe { compressed.write(u32::try_from(front.len()).unwrap()) };
        0
    })
}

// The signature is `GetVoice`'s, ten parameters as the header has them.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn fake_get_voice(
    _: *mut ISteamUser,
    want_compressed: bool,
    out: *mut c_void,
    capacity: u32,
    written: *mut u32,
    want_uncompressed: bool,
    uncompressed: *mut c_void,
    uncompressed_capacity: u32,
    uncompressed_written: *mut u32,
    rate: u32,
) -> i32 {
    script(|s| {
        s.voice.offered.push(capacity);
        if !want_compressed
            || want_uncompressed
            || !uncompressed.is_null()
            || uncompressed_capacity != 0
            || !uncompressed_written.is_null()
            || rate != 0
        {
            s.voice.deprecated_misuse += 1;
        }
        if s.voice.too_small > 0 {
            s.voice.too_small -= 1;
            return VOICE_BUFFER_TOO_SMALL;
        }
        let Some(front) = s.voice.packets.front() else {
            return VOICE_NO_DATA;
        };
        if front.len() > usize::try_from(capacity).unwrap() {
            return VOICE_BUFFER_TOO_SMALL;
        }
        let packet = s.voice.packets.pop_front().unwrap();
        // SAFETY: the caller passes `capacity` writable bytes, at least the
        // packet's length, and a writable `uint32`.
        unsafe {
            core::ptr::copy_nonoverlapping(packet.as_ptr(), out.cast::<u8>(), packet.len());
            written.write(u32::try_from(packet.len()).unwrap());
        }
        0
    })
}

unsafe extern "C" fn fake_decompress_voice(
    _: *mut ISteamUser,
    _: *const c_void,
    _: u32,
    out: *mut c_void,
    capacity: u32,
    written: *mut u32,
    rate: u32,
) -> i32 {
    script(|s| {
        s.voice.decompressions.push((capacity, rate));
        if let Some(result) = s.voice.decompress_result {
            return result;
        }
        let needed = if s.voice.decompress_never_fits {
            capacity + 1
        } else {
            u32::try_from(s.voice.pcm.len()).unwrap()
        };
        // SAFETY: the caller passes a writable `uint32`.
        unsafe { written.write(needed) };
        if needed > capacity {
            return VOICE_BUFFER_TOO_SMALL;
        }
        // SAFETY: the caller passes `capacity` writable bytes, at least
        // `needed`.
        unsafe {
            core::ptr::copy_nonoverlapping(
                s.voice.pcm.as_ptr(),
                out.cast::<u8>(),
                s.voice.pcm.len(),
            );
        }
        0
    })
}

unsafe extern "C" fn fake_get_voice_optimal_sample_rate(_: *mut ISteamUser) -> u32 {
    script(|s| s.voice.optimal_rate)
}

unsafe extern "C" fn fake_friends_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::FRIENDS.accessor)
}

unsafe extern "C" fn fake_get_persona_name(_: *mut ISteamFriends) -> *const c_char {
    fake_string()
}

/// The fake Steam Cloud: files by name, and a log of the calls that matter.
#[derive(Debug)]
pub(crate) struct FakeCloud {
    pub(crate) account_enabled: bool,
    pub(crate) app_enabled: bool,
    pub(crate) files: std::collections::BTreeMap<String, Vec<u8>>,
    /// `FileWrite` answers false and stores nothing.
    pub(crate) refuse_writes: bool,
    /// `FileRead` hands back one byte fewer than asked.
    pub(crate) short_reads: bool,
    /// `(total, available)` for `GetQuota`, or `None` to answer false.
    pub(crate) quota: Option<(u64, u64)>,
    /// The batch and write calls, in order: `begin`, `write <name>`, `end`.
    pub(crate) log: Vec<String>,
    /// What `GetLocalFileChange` reports: the path, the change, the path type.
    pub(crate) changes: Vec<(String, i32, i32)>,
    /// How many times any remote-storage function ran — for "no Steam call".
    pub(crate) calls: u32,
}

impl Default for FakeCloud {
    fn default() -> Self {
        Self {
            account_enabled: true,
            app_enabled: true,
            files: std::collections::BTreeMap::new(),
            refuse_writes: false,
            short_reads: false,
            quota: None,
            log: Vec::new(),
            changes: Vec::new(),
            calls: 0,
        }
    }
}

/// Counts a remote-storage call, whichever thread made it, and copies its
/// file-name argument.
///
/// # Safety
///
/// `name` is a NUL-terminated string live for the call.
unsafe fn cloud_call(name: *const c_char) -> String {
    script(|s| s.cloud.calls += 1);
    // SAFETY: the caller's promise.
    unsafe { arg(name) }
}

unsafe extern "C" fn fake_remote_storage_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::REMOTE_STORAGE.accessor)
}

unsafe extern "C" fn fake_file_write(
    _: *mut ISteamRemoteStorage,
    name: *const c_char,
    data: *const c_void,
    size: i32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name and `size` readable
    // bytes.
    let (name, bytes) = unsafe {
        (
            cloud_call(name),
            core::slice::from_raw_parts(data.cast::<u8>(), usize::try_from(size).unwrap_or(0))
                .to_vec(),
        )
    };
    script(|s| {
        s.cloud.log.push(format!("write {name}"));
        if s.cloud.refuse_writes {
            return false;
        }
        s.cloud.files.insert(name, bytes);
        true
    })
}

unsafe extern "C" fn fake_file_read(
    _: *mut ISteamRemoteStorage,
    name: *const c_char,
    out: *mut c_void,
    size: i32,
) -> i32 {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { cloud_call(name) };
    script(|s| {
        let Some(bytes) = s.cloud.files.get(&name) else {
            return 0;
        };
        let mut len = bytes.len().min(usize::try_from(size).unwrap_or(0));
        if s.cloud.short_reads {
            len = len.saturating_sub(1);
        }
        // SAFETY: the caller passes `size` writable bytes, and `len` is no
        // more than that.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), len) };
        i32::try_from(len).unwrap_or(i32::MAX)
    })
}

unsafe extern "C" fn fake_file_delete(_: *mut ISteamRemoteStorage, name: *const c_char) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { cloud_call(name) };
    script(|s| s.cloud.files.remove(&name).is_some())
}

unsafe extern "C" fn fake_file_exists(_: *mut ISteamRemoteStorage, name: *const c_char) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { cloud_call(name) };
    script(|s| s.cloud.files.contains_key(&name))
}

unsafe extern "C" fn fake_get_file_size(_: *mut ISteamRemoteStorage, name: *const c_char) -> i32 {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { cloud_call(name) };
    script(|s| {
        s.cloud
            .files
            .get(&name)
            .map_or(0, |bytes| i32::try_from(bytes.len()).unwrap_or(i32::MAX))
    })
}

unsafe extern "C" fn fake_get_file_count(_: *mut ISteamRemoteStorage) -> i32 {
    script(|s| {
        s.cloud.calls += 1;
        i32::try_from(s.cloud.files.len()).unwrap_or(i32::MAX)
    })
}

unsafe extern "C" fn fake_get_file_name_and_size(
    _: *mut ISteamRemoteStorage,
    index: i32,
    size: *mut i32,
) -> *const c_char {
    let (name, len) = script(|s| {
        s.cloud.calls += 1;
        let (name, bytes) = s
            .cloud
            .files
            .iter()
            .nth(usize::try_from(index).expect("a valid index"))
            .expect("an index below the count");
        (name.clone(), bytes.len())
    });
    script(|s| s.set_string(name.as_bytes()));
    // SAFETY: the caller passes a writable `int32`.
    unsafe { size.write(i32::try_from(len).unwrap_or(i32::MAX)) };
    fake_string()
}

unsafe extern "C" fn fake_get_quota(
    _: *mut ISteamRemoteStorage,
    total: *mut u64,
    available: *mut u64,
) -> bool {
    let quota = script(|s| {
        s.cloud.calls += 1;
        s.cloud.quota
    });
    let Some((all, free)) = quota else {
        return false;
    };
    // SAFETY: the caller passes two writable `uint64`s.
    unsafe {
        total.write(all);
        available.write(free);
    }
    true
}

unsafe extern "C" fn fake_is_cloud_enabled_for_account(_: *mut ISteamRemoteStorage) -> bool {
    script(|s| {
        s.cloud.calls += 1;
        s.cloud.account_enabled
    })
}

unsafe extern "C" fn fake_is_cloud_enabled_for_app(_: *mut ISteamRemoteStorage) -> bool {
    script(|s| {
        s.cloud.calls += 1;
        s.cloud.app_enabled
    })
}

unsafe extern "C" fn fake_get_local_file_change_count(_: *mut ISteamRemoteStorage) -> i32 {
    script(|s| {
        s.cloud.calls += 1;
        i32::try_from(s.cloud.changes.len()).unwrap_or(i32::MAX)
    })
}

unsafe extern "C" fn fake_get_local_file_change(
    _: *mut ISteamRemoteStorage,
    index: i32,
    change: *mut i32,
    path_type: *mut i32,
) -> *const c_char {
    let (path, kind, path_kind) = script(|s| {
        s.cloud.calls += 1;
        s.cloud.changes[usize::try_from(index).expect("a valid index")].clone()
    });
    script(|s| s.set_string(path.as_bytes()));
    // SAFETY: the caller passes two writable enums, `int`-sized.
    unsafe {
        change.write(kind);
        path_type.write(path_kind);
    }
    fake_string()
}

unsafe extern "C" fn fake_begin_file_write_batch(_: *mut ISteamRemoteStorage) -> bool {
    script(|s| {
        s.cloud.calls += 1;
        s.cloud.log.push("begin".into());
    });
    true
}

unsafe extern "C" fn fake_end_file_write_batch(_: *mut ISteamRemoteStorage) -> bool {
    script(|s| {
        s.cloud.calls += 1;
        s.cloud.log.push("end".into());
    });
    true
}

/// One downloaded leaderboard entry the fake holds: user, rank, score and
/// details.
pub(crate) type FakeEntry = (u64, i32, i32, Vec<i32>);

/// The fake stats, achievements and leaderboards.
#[derive(Debug, Default)]
pub(crate) struct FakeStats {
    pub(crate) ints: std::collections::BTreeMap<String, i32>,
    pub(crate) floats: std::collections::BTreeMap<String, f32>,
    /// Name to (unlocked, unlock time).
    pub(crate) achievements: std::collections::BTreeMap<String, (bool, u32)>,
    pub(crate) stores: u32,
    /// Every `(name, sort, display)` `FindOrCreateLeaderboard` got; `FindLeaderboard`
    /// records `-1` for both.
    pub(crate) finds: Vec<(String, i32, i32)>,
    /// Every `(leaderboard, method, score, details)` uploaded.
    pub(crate) uploads: Vec<(u64, i32, i32, Vec<i32>)>,
    /// Every `(leaderboard, request, start, end)` downloaded.
    pub(crate) downloads: Vec<(u64, i32, i32, i32)>,
    /// What `GetDownloadedLeaderboardEntry` hands out, by index.
    pub(crate) entries: Vec<FakeEntry>,
    /// The entries handle it answers for; any other is refused.
    pub(crate) entries_handle: u64,
    /// An index it refuses.
    pub(crate) refuse_entry: Option<i32>,
    /// Every details capacity it was offered.
    pub(crate) details_offered: Vec<i32>,
    /// How many times any user-stats function ran — for "no Steam call".
    pub(crate) calls: u32,
}

/// Counts a user-stats call and copies its name argument.
///
/// # Safety
///
/// `name` is a NUL-terminated string live for the call.
unsafe fn stats_call(name: *const c_char) -> String {
    script(|s| s.stats.calls += 1);
    // SAFETY: the caller's promise.
    unsafe { arg(name) }
}

unsafe extern "C" fn fake_user_stats_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::USER_STATS.accessor)
}

unsafe extern "C" fn fake_get_stat_i32(
    _: *mut ISteamUserStats,
    name: *const c_char,
    out: *mut i32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    let Some(value) = script(|s| s.stats.ints.get(&name).copied()) else {
        return false;
    };
    // SAFETY: the caller passes a writable `int32`.
    unsafe { out.write(value) };
    true
}

unsafe extern "C" fn fake_get_stat_f32(
    _: *mut ISteamUserStats,
    name: *const c_char,
    out: *mut f32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    let Some(value) = script(|s| s.stats.floats.get(&name).copied()) else {
        return false;
    };
    // SAFETY: the caller passes a writable `float`.
    unsafe { out.write(value) };
    true
}

unsafe extern "C" fn fake_set_stat_i32(
    _: *mut ISteamUserStats,
    name: *const c_char,
    value: i32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats
            .ints
            .get_mut(&name)
            .map(|slot| *slot = value)
            .is_some()
    })
}

unsafe extern "C" fn fake_set_stat_f32(
    _: *mut ISteamUserStats,
    name: *const c_char,
    value: f32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats
            .floats
            .get_mut(&name)
            .map(|slot| *slot = value)
            .is_some()
    })
}

unsafe extern "C" fn fake_set_achievement(_: *mut ISteamUserStats, name: *const c_char) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats
            .achievements
            .get_mut(&name)
            .map(|state| *state = (true, 1_700_000_000))
            .is_some()
    })
}

unsafe extern "C" fn fake_clear_achievement(_: *mut ISteamUserStats, name: *const c_char) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats
            .achievements
            .get_mut(&name)
            .map(|state| *state = (false, 0))
            .is_some()
    })
}

unsafe extern "C" fn fake_get_achievement_and_unlock_time(
    _: *mut ISteamUserStats,
    name: *const c_char,
    unlocked: *mut bool,
    time: *mut u32,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    let Some((done, when)) = script(|s| s.stats.achievements.get(&name).copied()) else {
        return false;
    };
    // SAFETY: the caller passes a writable `bool` and `uint32`.
    unsafe {
        unlocked.write(done);
        time.write(when);
    }
    true
}

unsafe extern "C" fn fake_store_stats(_: *mut ISteamUserStats) -> bool {
    script(|s| {
        s.stats.calls += 1;
        s.stats.stores += 1;
    });
    allowed()
}

unsafe extern "C" fn fake_find_or_create_leaderboard(
    _: *mut ISteamUserStats,
    name: *const c_char,
    sort: i32,
    display: i32,
) -> SteamApiCall {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats.finds.push((name, sort, display));
        s.next_call
    })
}

unsafe extern "C" fn fake_find_leaderboard(
    _: *mut ISteamUserStats,
    name: *const c_char,
) -> SteamApiCall {
    // SAFETY: the caller passes a NUL-terminated name.
    let name = unsafe { stats_call(name) };
    script(|s| {
        s.stats.finds.push((name, -1, -1));
        s.next_call
    })
}

unsafe extern "C" fn fake_download_leaderboard_entries(
    _: *mut ISteamUserStats,
    leaderboard: u64,
    request: i32,
    start: i32,
    end: i32,
) -> SteamApiCall {
    script(|s| {
        s.stats.calls += 1;
        s.stats.downloads.push((leaderboard, request, start, end));
        s.next_call
    })
}

unsafe extern "C" fn fake_get_downloaded_leaderboard_entry(
    _: *mut ISteamUserStats,
    entries: u64,
    index: i32,
    out: *mut crate::ffi::structs::LeaderboardEntry,
    details: *mut i32,
    capacity: i32,
) -> bool {
    let found = script(|s| {
        s.stats.calls += 1;
        s.stats.details_offered.push(capacity);
        if entries != s.stats.entries_handle || s.stats.refuse_entry == Some(index) {
            return None;
        }
        usize::try_from(index)
            .ok()
            .and_then(|index| s.stats.entries.get(index).cloned())
    });
    let Some((user, rank, score, held)) = found else {
        return false;
    };
    let written = held.len().min(usize::try_from(capacity).unwrap_or(0));
    // SAFETY: the caller passes a writable `LeaderboardEntry_t` (written
    // unaligned: the struct is packed) and `capacity` writable `int32`s, at
    // least `written`.
    unsafe {
        out.write_unaligned(crate::ffi::structs::LeaderboardEntry {
            user: user.to_le_bytes(),
            rank,
            score,
            details: i32::try_from(held.len()).unwrap(),
            ugc: 0,
        });
        core::ptr::copy_nonoverlapping(held.as_ptr(), details, written);
    }
    true
}

unsafe extern "C" fn fake_upload_leaderboard_score(
    _: *mut ISteamUserStats,
    leaderboard: u64,
    method: i32,
    score: i32,
    details: *const i32,
    count: i32,
) -> SteamApiCall {
    // SAFETY: the caller passes `count` readable `int32`s.
    let held =
        unsafe { core::slice::from_raw_parts(details, usize::try_from(count).unwrap()) }.to_vec();
    script(|s| {
        s.stats.calls += 1;
        s.stats.uploads.push((leaderboard, method, score, held));
        s.next_call
    })
}

unsafe extern "C" fn fake_apps_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::APPS.accessor)
}

unsafe extern "C" fn fake_is_subscribed(_: *mut ISteamApps) -> bool {
    script(|s| s.subscribed)
}

unsafe extern "C" fn fake_get_current_game_language(_: *mut ISteamApps) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_utils_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::UTILS.accessor)
}

unsafe extern "C" fn fake_get_app_id(_: *mut ISteamUtils) -> u32 {
    script(|s| s.app_id)
}

unsafe extern "C" fn fake_is_running_on_steam_hardware(_: *mut ISteamUtils) -> i32 {
    script(|s| s.hardware)
}

unsafe extern "C" fn fake_get_steam_hardware_default_config(_: *mut ISteamUtils) -> i32 {
    script(|s| s.default_config)
}

unsafe extern "C" fn fake_is_running_under_proton(_: *mut ISteamUtils) -> bool {
    script(|s| s.proton)
}

unsafe extern "C" fn fake_get_steam_ui_language(_: *mut ISteamUtils) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_is_overlay_enabled(_: *mut ISteamUtils) -> bool {
    script(|s| s.overlay_enabled)
}

unsafe extern "C" fn fake_is_steam_in_big_picture_mode(_: *mut ISteamUtils) -> bool {
    script(|s| s.big_picture)
}

unsafe extern "C" fn fake_set_overlay_notification_position(_: *mut ISteamUtils, position: i32) {
    script(|s| s.notification_positions.push(position));
}

unsafe extern "C" fn fake_set_overlay_notification_inset(
    _: *mut ISteamUtils,
    horizontal: i32,
    vertical: i32,
) {
    script(|s| s.notification_insets.push((horizontal, vertical)));
}

unsafe extern "C" fn fake_get_server_real_time(_: *mut ISteamUtils) -> u32 {
    script(|s| s.server_time)
}

unsafe extern "C" fn fake_get_ip_country(_: *mut ISteamUtils) -> *const c_char {
    fake_string()
}

/// A C string argument, copied.
///
/// # Safety
///
/// `text` is a NUL-terminated string live for the call.
unsafe fn arg(text: *const c_char) -> String {
    // SAFETY: the caller's promise.
    unsafe { std::ffi::CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

/// What every bool-returning call answers.
fn allowed() -> bool {
    !script(|s| s.refuse)
}

unsafe extern "C" fn fake_get_api_call_result(
    pipe: HSteamPipe,
    call: SteamApiCall,
    out: *mut c_void,
    size: i32,
    id: i32,
    failed: *mut bool,
) -> bool {
    script(|s| {
        if pipe != PIPE {
            s.calls.wrong_pipe += 1;
        }
        s.results_asked.push((call, size, id));
        let Some((_, bytes, io_failure)) = s.results.iter().find(|(c, ..)| *c == call) else {
            return false;
        };
        // SAFETY: the caller passes `size` writable bytes and a writable bool.
        unsafe {
            failed.write(*io_failure);
            let len = bytes.len().min(usize::try_from(size).unwrap_or(0));
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), len);
        }
        true
    })
}

unsafe extern "C" fn fake_activate_game_overlay_invite_dialog(_: *mut ISteamFriends, lobby: u64) {
    script(|s| s.invite_dialogs.push(lobby));
}

unsafe extern "C" fn fake_set_rich_presence(
    _: *mut ISteamFriends,
    key: *const c_char,
    value: *const c_char,
) -> bool {
    // SAFETY: the caller passes two NUL-terminated strings.
    let pair = unsafe { (arg(key), arg(value)) };
    let allowed = allowed();
    if allowed {
        script(|s| s.rich_presence.push(pair));
    }
    allowed
}

unsafe extern "C" fn fake_clear_rich_presence(_: *mut ISteamFriends) {
    script(|s| s.calls.clear_rich_presence += 1);
}

unsafe extern "C" fn fake_invite_user_to_game(
    _: *mut ISteamFriends,
    friend: u64,
    connect: *const c_char,
) -> bool {
    // SAFETY: the caller passes a NUL-terminated string.
    let connect = unsafe { arg(connect) };
    script(|s| s.game_invites.push((friend, connect)));
    allowed()
}

unsafe extern "C" fn fake_matchmaking_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::MATCHMAKING.accessor)
}

unsafe extern "C" fn fake_create_lobby(
    _: *mut ISteamMatchmaking,
    kind: i32,
    max: i32,
) -> SteamApiCall {
    script(|s| {
        s.created.push((kind, max));
        s.next_call
    })
}

unsafe extern "C" fn fake_join_lobby(_: *mut ISteamMatchmaking, lobby: u64) -> SteamApiCall {
    script(|s| {
        s.joined.push(lobby);
        s.next_call
    })
}

unsafe extern "C" fn fake_leave_lobby(_: *mut ISteamMatchmaking, lobby: u64) {
    script(|s| s.left.push(lobby));
}

unsafe extern "C" fn fake_invite_user_to_lobby(
    _: *mut ISteamMatchmaking,
    lobby: u64,
    friend: u64,
) -> bool {
    script(|s| {
        s.lobby_writes.push((
            "InviteUserToLobby",
            lobby,
            friend.to_string(),
            String::new(),
        ));
    });
    allowed()
}

unsafe extern "C" fn fake_get_num_lobby_members(_: *mut ISteamMatchmaking, _: u64) -> i32 {
    script(|s| i32::try_from(s.members.len()).unwrap())
}

unsafe extern "C" fn fake_get_lobby_member_by_index(
    _: *mut ISteamMatchmaking,
    _: u64,
    index: i32,
) -> u64 {
    script(|s| s.members[usize::try_from(index).unwrap()])
}

unsafe extern "C" fn fake_get_lobby_data(
    _: *mut ISteamMatchmaking,
    _: u64,
    _: *const c_char,
) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_set_lobby_data(
    _: *mut ISteamMatchmaking,
    lobby: u64,
    key: *const c_char,
    value: *const c_char,
) -> bool {
    // SAFETY: the caller passes two NUL-terminated strings.
    let (key, value) = unsafe { (arg(key), arg(value)) };
    script(|s| s.lobby_writes.push(("SetLobbyData", lobby, key, value)));
    allowed()
}

unsafe extern "C" fn fake_get_lobby_member_data(
    _: *mut ISteamMatchmaking,
    _: u64,
    _: u64,
    _: *const c_char,
) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_set_lobby_member_data(
    _: *mut ISteamMatchmaking,
    lobby: u64,
    key: *const c_char,
    value: *const c_char,
) {
    // SAFETY: the caller passes two NUL-terminated strings.
    let (key, value) = unsafe { (arg(key), arg(value)) };
    script(|s| {
        s.lobby_writes
            .push(("SetLobbyMemberData", lobby, key, value))
    });
}

unsafe extern "C" fn fake_send_lobby_chat_msg(
    _: *mut ISteamMatchmaking,
    _: u64,
    body: *const c_void,
    len: i32,
) -> bool {
    // SAFETY: the caller passes `len` readable bytes.
    let body =
        unsafe { core::slice::from_raw_parts(body.cast::<u8>(), usize::try_from(len).unwrap()) }
            .to_vec();
    script(|s| s.chat_sent.push(body));
    allowed()
}

unsafe extern "C" fn fake_get_lobby_chat_entry(
    _: *mut ISteamMatchmaking,
    _: u64,
    _: i32,
    sender: *mut u64,
    out: *mut c_void,
    capacity: i32,
    kind: *mut i32,
) -> i32 {
    script(|s| {
        let (from, entry_type, body, count) = &s.chat_entry;
        // SAFETY: the caller passes writable `sender` and `kind`, and
        // `capacity` writable bytes at `out`.
        unsafe {
            sender.write(*from);
            kind.write(*entry_type);
            let len = body.len().min(usize::try_from(capacity).unwrap());
            core::ptr::copy_nonoverlapping(body.as_ptr(), out.cast::<u8>(), len);
        }
        *count
    })
}

unsafe extern "C" fn fake_set_lobby_member_limit(
    _: *mut ISteamMatchmaking,
    lobby: u64,
    limit: i32,
) -> bool {
    script(|s| {
        s.lobby_writes.push((
            "SetLobbyMemberLimit",
            lobby,
            limit.to_string(),
            String::new(),
        ));
    });
    allowed()
}

unsafe extern "C" fn fake_get_lobby_member_limit(_: *mut ISteamMatchmaking, _: u64) -> i32 {
    script(|s| s.member_limit)
}

unsafe extern "C" fn fake_set_lobby_type(_: *mut ISteamMatchmaking, lobby: u64, kind: i32) -> bool {
    script(|s| {
        s.lobby_writes
            .push(("SetLobbyType", lobby, kind.to_string(), String::new()));
    });
    allowed()
}

unsafe extern "C" fn fake_set_lobby_joinable(
    _: *mut ISteamMatchmaking,
    lobby: u64,
    joinable: bool,
) -> bool {
    script(|s| {
        s.lobby_writes.push((
            "SetLobbyJoinable",
            lobby,
            joinable.to_string(),
            String::new(),
        ));
    });
    allowed()
}

unsafe extern "C" fn fake_get_lobby_owner(_: *mut ISteamMatchmaking, _: u64) -> u64 {
    script(|s| s.lobby_owner)
}

unsafe extern "C" fn fake_get_launch_command_line(
    _: *mut ISteamApps,
    out: *mut c_char,
    capacity: i32,
) -> i32 {
    script(|s| {
        let capacity = usize::try_from(capacity).unwrap();
        let len = s.launch_line.len().min(capacity);
        // SAFETY: the caller passes `capacity` writable bytes; the NUL goes
        // in only when it fits, as a C strncpy-shaped copy would leave it.
        unsafe {
            core::ptr::copy_nonoverlapping(s.launch_line.as_ptr(), out.cast::<u8>(), len);
            if len < capacity {
                out.cast::<u8>().add(len).write(0);
            }
        }
        i32::try_from(len).unwrap()
    })
}

/// A `SteamAPICallCompleted_t` on the pipe: `call` answered with callback
/// `id` of `size` bytes.
pub(crate) fn completion(call: SteamApiCall, id: i32, size: usize) -> FakeMsg {
    let mut bytes = call.to_le_bytes().to_vec();
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
    FakeMsg::payload(703, bytes)
}

/// A payload of `T`'s size, zeroed, with each `(offset, bytes)` written in.
pub(crate) fn payload<T>(fields: &[(usize, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0; size_of::<T>()];
    for &(at, field) in fields {
        bytes[at..at + field.len()].copy_from_slice(field);
    }
    bytes
}

/// A `LobbyCreated_t` answer.
pub(crate) fn lobby_created(result: i32, lobby: u64) -> Vec<u8> {
    use crate::ffi::structs::LobbyCreated;
    payload::<LobbyCreated>(&[
        (
            core::mem::offset_of!(LobbyCreated, result),
            &result.to_le_bytes(),
        ),
        (
            core::mem::offset_of!(LobbyCreated, lobby),
            &lobby.to_le_bytes(),
        ),
    ])
}

/// A `LobbyEnter_t` answer.
pub(crate) fn lobby_enter(lobby: u64, response: u32, locked: bool) -> Vec<u8> {
    use crate::ffi::structs::LobbyEnter;
    payload::<LobbyEnter>(&[
        (
            core::mem::offset_of!(LobbyEnter, lobby),
            &lobby.to_le_bytes(),
        ),
        (
            core::mem::offset_of!(LobbyEnter, locked),
            &[u8::from(locked)],
        ),
        (
            core::mem::offset_of!(LobbyEnter, response),
            &response.to_le_bytes(),
        ),
    ])
}

unsafe extern "C" fn fake_get_persona_state(_: *mut ISteamFriends) -> i32 {
    script(|s| s.persona_state)
}

unsafe extern "C" fn fake_get_friend_count(_: *mut ISteamFriends, flags: i32) -> i32 {
    script(|s| {
        s.friend_flags.push(flags);
        i32::try_from(s.friends.len()).unwrap()
    })
}

unsafe extern "C" fn fake_get_friend_by_index(
    _: *mut ISteamFriends,
    index: i32,
    flags: i32,
) -> u64 {
    script(|s| {
        s.friend_flags.push(flags);
        s.friends[usize::try_from(index).unwrap()]
    })
}

unsafe extern "C" fn fake_get_friend_persona_state(_: *mut ISteamFriends, _: u64) -> i32 {
    script(|s| s.persona_state)
}

unsafe extern "C" fn fake_get_friend_persona_name(_: *mut ISteamFriends, _: u64) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_activate_game_overlay(_: *mut ISteamFriends, dialog: *const c_char) {
    // SAFETY: the caller passes a NUL-terminated string.
    let dialog = unsafe { arg(dialog) };
    script(|s| s.overlays.push(("ActivateGameOverlay", dialog, 0)));
}

unsafe extern "C" fn fake_activate_game_overlay_to_user(
    _: *mut ISteamFriends,
    dialog: *const c_char,
    user: u64,
) {
    // SAFETY: the caller passes a NUL-terminated string.
    let dialog = unsafe { arg(dialog) };
    script(|s| s.overlays.push(("ActivateGameOverlayToUser", dialog, user)));
}

unsafe extern "C" fn fake_activate_game_overlay_to_web_page(
    _: *mut ISteamFriends,
    url: *const c_char,
    mode: i32,
) {
    // SAFETY: the caller passes a NUL-terminated string.
    let url = unsafe { arg(url) };
    let mode = u64::try_from(mode).unwrap();
    script(|s| s.overlays.push(("ActivateGameOverlayToWebPage", url, mode)));
}

unsafe extern "C" fn fake_get_small_friend_avatar(_: *mut ISteamFriends, _: u64) -> i32 {
    script(|s| s.avatar_handles[0])
}

unsafe extern "C" fn fake_get_medium_friend_avatar(_: *mut ISteamFriends, _: u64) -> i32 {
    script(|s| s.avatar_handles[1])
}

unsafe extern "C" fn fake_get_large_friend_avatar(_: *mut ISteamFriends, _: u64) -> i32 {
    script(|s| s.avatar_handles[2])
}

unsafe extern "C" fn fake_request_user_information(
    _: *mut ISteamFriends,
    user: u64,
    name_only: bool,
) -> bool {
    script(|s| {
        s.info_requests.push((user, name_only));
        s.info_pending
    })
}

unsafe extern "C" fn fake_get_friend_rich_presence(
    _: *mut ISteamFriends,
    _: u64,
    _: *const c_char,
) -> *const c_char {
    fake_string()
}

unsafe extern "C" fn fake_request_friend_rich_presence(_: *mut ISteamFriends, friend: u64) {
    script(|s| s.presence_requests.push(friend));
}

unsafe extern "C" fn fake_get_image_size(
    _: *mut ISteamUtils,
    image: i32,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    script(|s| {
        s.image_calls.push(("GetImageSize", image, 0));
        let Some((w, h, _)) = &s.image else {
            return false;
        };
        // SAFETY: the caller passes two writable `uint32`s.
        unsafe {
            width.write(*w);
            height.write(*h);
        }
        true
    })
}

unsafe extern "C" fn fake_get_image_rgba(
    _: *mut ISteamUtils,
    image: i32,
    out: *mut u8,
    size: i32,
) -> bool {
    script(|s| {
        s.image_calls.push(("GetImageRGBA", image, size));
        if s.refuse {
            return false;
        }
        let Some((_, _, pixels)) = &s.image else {
            return false;
        };
        let len = pixels.len().min(usize::try_from(size).unwrap());
        // SAFETY: the caller passes `size` writable bytes.
        unsafe { core::ptr::copy_nonoverlapping(pixels.as_ptr(), out, len) };
        true
    })
}

/// One connection in the fake's loop.
#[derive(Debug)]
pub(crate) struct FakeConnection {
    /// The other end, for a connection made by `ConnectP2P`.
    pub(crate) peer: Option<HSteamNetConnection>,
    /// Who is at the other end.
    pub(crate) remote: u64,
    pub(crate) listen_socket: HSteamListenSocket,
    pub(crate) state: i32,
    pub(crate) end_reason: i32,
    /// Messages waiting to be received: bytes and send flags.
    pub(crate) inbox: VecDeque<(Vec<u8>, i32)>,
}

/// The fake networking: an in-process loop in which `ConnectP2P` makes both
/// ends at once, connected, and a send lands in the other end's inbox.
#[derive(Debug, Default)]
pub(crate) struct FakeNet {
    pub(crate) relay_inits: u32,
    pub(crate) relay_availability: i32,
    pub(crate) next_handle: u32,
    pub(crate) connections: std::collections::BTreeMap<HSteamNetConnection, FakeConnection>,
    /// Every `(remote, port)` `ConnectP2P` was asked for.
    pub(crate) connects: Vec<(u64, i32)>,
    /// `ConnectP2P` and `CreateListenSocketP2P` answer the invalid handle.
    pub(crate) refuse: bool,
    pub(crate) listen_sockets: Vec<(HSteamListenSocket, i32)>,
    pub(crate) closed_listen_sockets: Vec<HSteamListenSocket>,
    pub(crate) accepted: Vec<HSteamNetConnection>,
    /// What `AcceptConnection` answers instead of `k_EResultOK`.
    pub(crate) accept_result: Option<i32>,
    /// Every `(connection, reason, linger)` `CloseConnection` received.
    pub(crate) closed: Vec<(HSteamNetConnection, i32, bool)>,
    /// How many times `SendMessageToConnection` ran.
    pub(crate) sends: u32,
    /// What `SendMessageToConnection` answers instead of delivering.
    pub(crate) send_result: Option<i32>,
    /// Every message handed out and not yet released, by address.
    pub(crate) outstanding: Vec<usize>,
    pub(crate) released: u32,
    /// `Release` on a message that was not outstanding.
    pub(crate) bad_releases: u32,
    /// How many times any networking function ran — for "no Steam call".
    pub(crate) calls: u32,
}

impl FakeNet {
    fn handle(&mut self) -> u32 {
        self.next_handle += 1;
        100 + self.next_handle
    }
}

/// The far end of a connection `ConnectP2P` made.
pub(crate) fn peer_of(connection: HSteamNetConnection) -> HSteamNetConnection {
    script(|s| {
        s.net.connections[&connection]
            .peer
            .expect("a ConnectP2P connection")
    })
}

/// A connection from `from` arriving on `listen_socket`, still connecting,
/// and the status callback announcing it.
pub(crate) fn arriving(
    listen_socket: HSteamListenSocket,
    from: u64,
) -> (HSteamNetConnection, FakeMsg) {
    let connection = script(|s| {
        let handle = s.net.handle();
        s.net.connections.insert(
            handle,
            FakeConnection {
                peer: None,
                remote: from,
                listen_socket,
                state: crate::net::state::CONNECTING,
                end_reason: 0,
                inbox: VecDeque::new(),
            },
        );
        handle
    });
    (connection, status_changed(connection))
}

/// A `SteamNetConnectionStatusChangedCallback_t` for `connection` as the fake
/// holds it now.
pub(crate) fn status_changed(connection: HSteamNetConnection) -> FakeMsg {
    use crate::ffi::structs::SteamNetConnectionStatusChanged as Status;
    use core::mem::offset_of;
    let (remote, listen_socket, state) = script(|s| {
        let c = &s.net.connections[&connection];
        (c.remote, c.listen_socket, c.state)
    });
    let info = offset_of!(Status, info);
    let identity = info + offset_of!(SteamNetConnectionInfo, identity);
    let bytes = payload::<Status>(&[
        (offset_of!(Status, connection), &connection.to_le_bytes()),
        (
            identity + offset_of!(SteamNetworkingIdentity, kind),
            &16i32.to_le_bytes(),
        ),
        (
            identity + offset_of!(SteamNetworkingIdentity, size),
            &8i32.to_le_bytes(),
        ),
        (
            identity + offset_of!(SteamNetworkingIdentity, data),
            &remote.to_le_bytes(),
        ),
        (
            info + offset_of!(SteamNetConnectionInfo, listen_socket),
            &listen_socket.to_le_bytes(),
        ),
        (
            info + offset_of!(SteamNetConnectionInfo, state),
            &state.to_le_bytes(),
        ),
    ]);
    FakeMsg::payload(1221, bytes)
}

/// Counts a networking call, whichever thread made it.
fn net_call() {
    script(|s| s.net.calls += 1);
}

unsafe extern "C" fn fake_net_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::NETWORKING_SOCKETS.accessor)
}

unsafe extern "C" fn fake_net_utils_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::NETWORKING_UTILS.accessor)
}

unsafe extern "C" fn fake_init_relay_network_access(_: *mut ISteamNetworkingUtils) {
    net_call();
    script(|s| s.net.relay_inits += 1);
}

unsafe extern "C" fn fake_get_relay_network_status(
    _: *mut ISteamNetworkingUtils,
    out: *mut SteamRelayNetworkStatus,
) -> i32 {
    net_call();
    let availability = script(|s| s.net.relay_availability);
    // SAFETY: the caller passes a writable `SteamRelayNetworkStatus_t`; the
    // write is unaligned because the struct is packed.
    unsafe {
        out.write_unaligned(SteamRelayNetworkStatus {
            availability,
            ping_measurement_in_progress: 0,
            network_config: availability,
            any_relay: availability,
            debug: [0; 256],
        });
    }
    availability
}

unsafe extern "C" fn fake_create_listen_socket_p2p(
    _: *mut ISteamNetworkingSockets,
    port: i32,
    _: i32,
    _: *const c_void,
) -> HSteamListenSocket {
    net_call();
    script(|s| {
        if s.net.refuse {
            return 0;
        }
        let handle = s.net.handle();
        s.net.listen_sockets.push((handle, port));
        handle
    })
}

unsafe extern "C" fn fake_connect_p2p(
    _: *mut ISteamNetworkingSockets,
    identity: *const SteamNetworkingIdentity,
    port: i32,
    _: i32,
    _: *const c_void,
) -> HSteamNetConnection {
    net_call();
    // SAFETY: the caller passes a live identity; read unaligned (packed).
    let identity = unsafe { identity.read_unaligned() };
    let remote = crate::net::remote_of(&identity).map_or(0, |id| id.0);
    script(|s| {
        s.net.connects.push((remote, port));
        if s.net.refuse {
            return 0;
        }
        let near = s.net.handle();
        let far = s.net.handle();
        let connected = crate::net::state::CONNECTED;
        let end = |peer, remote| FakeConnection {
            peer: Some(peer),
            remote,
            listen_socket: 0,
            state: connected,
            end_reason: 0,
            inbox: VecDeque::new(),
        };
        s.net.connections.insert(near, end(far, remote));
        s.net.connections.insert(far, end(near, STEAM_ID));
        near
    })
}

unsafe extern "C" fn fake_accept_connection(
    _: *mut ISteamNetworkingSockets,
    connection: HSteamNetConnection,
) -> i32 {
    net_call();
    script(|s| {
        s.net.accepted.push(connection);
        if let Some(result) = s.net.accept_result {
            return result;
        }
        if let Some(c) = s.net.connections.get_mut(&connection) {
            c.state = crate::net::state::CONNECTED;
        }
        1
    })
}

unsafe extern "C" fn fake_close_connection(
    _: *mut ISteamNetworkingSockets,
    connection: HSteamNetConnection,
    reason: i32,
    _: *const c_char,
    linger: bool,
) -> bool {
    net_call();
    script(|s| {
        s.net.closed.push((connection, reason, linger));
        let Some(closed) = s.net.connections.remove(&connection) else {
            return false;
        };
        if let Some(peer) = closed
            .peer
            .and_then(|peer| s.net.connections.get_mut(&peer))
        {
            peer.state = crate::net::state::CLOSED_BY_PEER;
            peer.end_reason = reason;
        }
        true
    })
}

unsafe extern "C" fn fake_close_listen_socket(
    _: *mut ISteamNetworkingSockets,
    socket: HSteamListenSocket,
) -> bool {
    net_call();
    script(|s| s.net.closed_listen_sockets.push(socket));
    true
}

unsafe extern "C" fn fake_send_message_to_connection(
    _: *mut ISteamNetworkingSockets,
    connection: HSteamNetConnection,
    data: *const c_void,
    len: u32,
    flags: i32,
    _: *mut i64,
) -> i32 {
    net_call();
    // SAFETY: the caller passes `len` readable bytes.
    let bytes =
        unsafe { core::slice::from_raw_parts(data.cast::<u8>(), usize::try_from(len).unwrap()) }
            .to_vec();
    script(|s| {
        s.net.sends += 1;
        if let Some(result) = s.net.send_result {
            return result;
        }
        let peer = match s.net.connections.get(&connection) {
            Some(c) if c.state == crate::net::state::CONNECTED => c.peer,
            // k_EResultNoConnection.
            _ => return 3,
        };
        if let Some(peer) = peer.and_then(|peer| s.net.connections.get_mut(&peer)) {
            // On a received message only the reliable bit is meaningful.
            peer.inbox.push_back((bytes, flags & 8));
        }
        1
    })
}

unsafe extern "C" fn fake_receive_messages_on_connection(
    _: *mut ISteamNetworkingSockets,
    connection: HSteamNetConnection,
    out: *mut *mut SteamNetworkingMessage,
    max: i32,
) -> i32 {
    net_call();
    script(|s| {
        let Some(c) = s.net.connections.get_mut(&connection) else {
            return -1;
        };
        let mut count = 0;
        while count < max {
            let Some((bytes, flags)) = c.inbox.pop_front() else {
                break;
            };
            let size = i32::try_from(bytes.len()).unwrap();
            let data = Box::into_raw(bytes.into_boxed_slice()).cast::<c_void>();
            let message = Box::into_raw(Box::new(SteamNetworkingMessage {
                data,
                size,
                connection,
                identity_peer: crate::net::identity_of(crate::SteamId(c.remote)),
                connection_user_data: 0,
                time_received: 0,
                message_number: 0,
                free_data: core::ptr::null(),
                release: core::ptr::null(),
                channel: 0,
                flags,
                user_data: 0,
                lane: 0,
                pad1: 0,
            }));
            s.net.outstanding.push(message as usize);
            // SAFETY: the caller passes room for `max` pointers.
            unsafe { out.add(usize::try_from(count).unwrap()).write(message) };
            count += 1;
        }
        count
    })
}

unsafe extern "C" fn fake_get_connection_info(
    _: *mut ISteamNetworkingSockets,
    connection: HSteamNetConnection,
    out: *mut SteamNetConnectionInfo,
) -> bool {
    net_call();
    script(|s| {
        let Some(c) = s.net.connections.get(&connection) else {
            return false;
        };
        let mut info = zeroed_info();
        info.identity = crate::net::identity_of(crate::SteamId(c.remote));
        info.listen_socket = c.listen_socket;
        info.state = c.state;
        info.end_reason = c.end_reason;
        // SAFETY: the caller passes a writable `SteamNetConnectionInfo_t`.
        unsafe { out.write_unaligned(info) };
        true
    })
}

/// An all-zero connection info.
fn zeroed_info() -> SteamNetConnectionInfo {
    crate::callbacks::read(&[0; size_of::<SteamNetConnectionInfo>()]).expect("exactly its size")
}

unsafe extern "C" fn fake_release_message(message: *mut SteamNetworkingMessage) {
    net_call();
    let known = script(|s| {
        let at = s
            .net
            .outstanding
            .iter()
            .position(|&m| m == message as usize);
        match at {
            Some(at) => {
                s.net.outstanding.swap_remove(at);
                s.net.released += 1;
                true
            }
            None => {
                s.net.bad_releases += 1;
                false
            }
        }
    });
    if known {
        // SAFETY: the fake made `message` and its data with `Box`, and it was
        // outstanding, so neither has been freed.
        unsafe {
            let message = Box::from_raw(message);
            let len = usize::try_from(message.size).unwrap();
            drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(
                message.data.cast::<u8>(),
                len,
            )));
        }
    }
}

/// Joins `lobby` through the fake — `JoinLobby` answered at the next pump —
/// and returns the held [`crate::Lobby`].
pub(crate) fn joined_lobby(steam: &mut crate::Steam, lobby: u64) -> crate::Lobby {
    use crate::call::private::Answer as _;
    script(|s| {
        s.next_call = 88;
        s.results.push((88, lobby_enter(lobby, 1, false), false));
    });
    let call = steam
        .matchmaking()
        .join_lobby(crate::LobbyId(lobby))
        .expect("the fake starts the call");
    let row = crate::LobbyEntered::ROW;
    script(|s| s.queue.push_back(completion(88, row.id(), row.size)));
    steam.pump();
    match steam.take(call) {
        crate::CallState::Ready(entered) => entered.lobby().expect("the fake admits"),
        other => panic!("the fake answers at once: {other:?}"),
    }
}
