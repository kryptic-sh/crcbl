//! `crcbl-steam` — Steamworks for the engine, over the SDK's flat C API.
//!
//! ```text
//! Steam::relaunch_via_steam(AppId)?  ── true: quit, Steam relaunches the game
//! Steam::init(AppId) ──▶ once per frame: pump() ──▶ events() ──▶ act
//!        │
//!        ├── user(): steam_id(), logged_on(), steam_level()
//!        ├── auth(): session_ticket() / begin_session() ──▶ AuthGate, web_api_ticket(), …
//!        ├── friends(): persona_name(), list(), name(), avatar(), set_rich_presence(),
//!        │              open_invite_dialog(), open_overlay(), …
//!        ├── apps(): subscribed(), game_language(), launch_command_line(), owner(),
//!        │           dlc_count() / dlc(), beta_count() / beta(), install_dir(), …
//!        ├── remote_play(): sessions(), user(), invite(), …
//!        ├── utils(): app_id(), steam_hardware(), overlay_enabled(), …,
//!        │            show_text_input() / show_floating_keyboard() (the Deck's keyboards)
//!        ├── matchmaking(): create_lobby() / join_lobby() ──▶ SteamCall<T>
//!        │                         └──▶ a later frame: steam.take(call) ──▶ Lobby
//!        ├── networking(): start_relay(), relay_status()
//!        ├── SteamListener::open(lobby) / SteamTransport::connect(owner): crcbl_net::Transport
//!        ├── stats(): achievement(), set_achievement(), set_i32(), store()
//!        ├── leaderboards(): find_or_create() / upload() / download() ──▶ SteamCall<T>
//!        ├── screenshots(): hook(), write() the game's own; tag_user(), set_location()
//!        ├── timeline(): set_game_mode(), instant_event(), range_start() ──▶ TimelineRange, phases
//!        ├── workshop(): query_all() ──▶ UgcQuery ──▶ send() ──▶ SteamCall<QueryPage> ──▶ results(),
//!        │               subscribe(), install_info(), create_item(), start_update() ──▶ submit()
//!        ├── voice(): capture() ──▶ VoiceCapture: set_transmitting(), poll() ──▶ packets
//!        │            decompress(packet, VOICE_SAMPLE_RATE) ──▶ mono f32 PCM
//!        ├── SteamCloudStorage::new(): crcbl_store::StorageSource
//!        └── SteamPads::open(manifest) ──▶ after each pump: poll() ──▶ crcbl_input::GamepadEvent
//!                                          glyph(id, control) ──▶ the button's PNG
//! ```
//!
//! `docs/plan/42-steam.md` is the design; this crate is its slices as they
//! land. What exists now is slices 1, 1b, 3a, 3b, 4, 5, 6, 7b, 7c, 9, 10, 11, 12 and 14: the library is
//! found and opened at runtime, Steam is initialised with a version
//! handshake, the callback pipe is drained by manual dispatch into a queue of
//! `SteamEvent`s, the local player's identity, the machine's basics and the
//! friends list — names, states, avatars, rich presence — are read,
//! asynchronous calls are typed tokens redeemed after the pump, lobbies are
//! created, joined, invited to and left, peers connect over Steam P2P as a
//! `crcbl_net::Transport`, files are kept in Steam Cloud as a
//! `crcbl_store::StorageSource`, voice is captured and decoded to PCM,
//! achievements, stats and leaderboards are read and written, controllers
//! arrive through Steam Input as the same gamepad events every pad backend
//! reports, with their buttons' glyphs, the Deck's on-screen keyboards hand
//! back typed text, screenshots are written to the player's library and
//! moments marked on Steam's game recording, ownership, DLC, betas and Remote
//! Play sessions are read, tickets prove a player to a peer or a service,
//! Workshop items are found, subscribed to, installed, made and updated, and
//! the
//! API is shut down exactly once, when
//! the last owner of it is gone. Every string Steam returns is
//! copied before the call that got it returns.
//!
//! # No SDK in the repository, no link-time dependency
//!
//! Valve's licence lets a game redistribute `redistributable_bin` beside its
//! executable and nothing else, so no header, no `steam_api.json` and no
//! library is committed. The function prototypes and struct layouts here are
//! this crate's own declarations of the C ABI, each carrying the C declaration
//! it was copied from (`ffi::manifest`). The library is opened by absolute
//! path when `Steam::init` runs — beside the executable first, then under
//! `$CRCBL_STEAM_SDK/redistributable_bin/<platform>/` for development — so a
//! machine without Steam gets an ordinary `Err` and the game runs on, and the
//! crate builds and tests on a CI that has never seen the SDK.
//!
//! # Where it compiles
//!
//! 64-bit Linux, Windows and macOS. Everywhere else — `wasm32`, Android, any
//! 32-bit target — the crate is this documentation and no items, so nothing
//! above it ever asks `cfg(target_os)` about Steam. Items that exist on only
//! some targets are named in backticks rather than linked, because `cargo doc`
//! is a `-D warnings` gate on the targets where they do not exist.
//!
//! # What is checked where
//!
//! The pump, init and loader run in CI against a fake library — a struct of
//! Rust function pointers with the exact types the real one is called through
//! (`testing`, test builds only). The struct layouts are asserted per
//! operating system, because Valve packs callback structs to 4 bytes on Linux
//! and macOS and 8 on Windows. What no CI job can check — that these
//! declarations match a real SDK, and that a real client accepts them — is the
//! `#[ignore]`d drift gate (`ffi::drift`) and `tests/smoke.rs`, run by hand
//! with the SDK and a Steam client present.

#![warn(missing_docs)]

// The one target gate — 64-bit Linux, Windows and macOS, the targets Valve
// ships a 64-bit `steam_api` for — written on each item, as `crcbl-dx12`
// writes its own. Not a macro wrapping the list: rustfmt does not look inside
// macro invocations, so a module declared in one is never formatted.
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod apps;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod auth;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod avatar;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod call;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod callbacks;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod client;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod cloud;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod error;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod ffi;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod friends;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod input;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod keyboard;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod leaderboard;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod matchmaking;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod net;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod presence;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod pump;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod remote_play;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod screenshots;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod stats;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod strings;
#[cfg(all(
    test,
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod testing;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod timeline;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod user;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod utils;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod voice;
#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod workshop;

#[cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
pub use crate::{
    apps::{
        Apps, Beta, BetaCount, BetaFlags, CONNECT_LOBBY, Dlc, FileDetails, MAX_TEXT_BYTES,
        connect_lobby,
    },
    auth::{
        Auth, AuthGate, AuthResponse, AuthSession, AuthTicketId, BeginAuthError,
        EncryptedTicketReady, License, SESSION_TICKET_BYTES, SessionTicket, Verdict, WebApiTicket,
    },
    avatar::{AvatarSize, Rgba},
    call::{CallError, CallResult, CallState, SteamCall},
    callbacks::SteamEvent,
    client::{AppId, Steam},
    cloud::{CloudQuota, MAX_CLOUD_FILE_BYTES, MAX_CLOUD_PATH_BYTES, SteamCloudStorage},
    error::{EResult, InitError, SteamError},
    friends::{
        FriendFlags, Friends, OverlayDialog, PersonaChange, PersonaState, UserDialog, WebPageMode,
    },
    input::{GlyphSize, InputError, PAD_MANIFEST, PAD_MANIFEST_FILE, PadControl, SteamPads},
    keyboard::{FloatingKeyboardMode, TextField, TextInputLines, TextInputMode, TextInputRequest},
    leaderboard::{
        Entries, Entry, Leaderboard, LeaderboardDisplay, LeaderboardFound, LeaderboardSort,
        Leaderboards, MAX_LEADERBOARD_DETAILS, MAX_LEADERBOARD_NAME_LENGTH, Range, ScoreUploaded,
        UploadMethod,
    },
    matchmaking::{
        EnterResponse, Lobby, LobbyCreated, LobbyEntered, LobbyId, LobbyKind,
        MAX_LOBBY_CHAT_MESSAGE, MAX_LOBBY_KEY_LENGTH, Matchmaking, MemberChange,
    },
    net::{
        Availability, EndReason, MAX_MESSAGE_BYTES, Networking, RelayStatus, SteamListener,
        SteamTransport, VirtualPort,
    },
    presence::{
        MAX_RICH_PRESENCE_KEY_LENGTH, MAX_RICH_PRESENCE_KEYS, MAX_RICH_PRESENCE_VALUE_LENGTH,
    },
    pump::PumpDiagnostics,
    remote_play::{FormFactor, RemotePlay, RemotePlaySession},
    screenshots::{ScreenshotId, Screenshots},
    stats::{Achieved, MAX_STAT_NAME_LENGTH, Stats},
    timeline::{
        ClipPriority, EventRecording, GameMode, MAX_PHASE_ID_LENGTH, MAX_TIMELINE_EVENT_SECONDS,
        MAX_TIMELINE_PRIORITY, PhaseRecording, Timeline, TimelineEvent, TimelineEventId,
        TimelineRange,
    },
    user::{SteamId, User},
    utils::{HardwareDefaultConfig, NotificationCorner, SteamHardware, Utils},
    voice::{
        MAX_VOICE_SAMPLE_RATE, MIN_VOICE_SAMPLE_RATE, VOICE_SAMPLE_RATE, Voice, VoiceCapture,
        VoiceError,
    },
    workshop::{
        DownloadProgress, FileType, InstallInfo, ItemCreated, ItemDeleted, ItemDetails, ItemId,
        ItemState, ItemSubmitted, ItemUpdate, MAX_CHANGE_NOTE_LENGTH, MAX_ITEM_DESCRIPTION_LENGTH,
        MAX_ITEM_METADATA_LENGTH, MAX_ITEM_TAG_LENGTH, MAX_ITEM_TITLE_LENGTH, MatchingType,
        QueryOrder, QueryPage, Submission, Subscribed, UGC_RESULTS_PER_PAGE, UgcQuery,
        Unsubscribed, UpdateProgress, UpdateStatus, UserList, UserListOrder, Visibility, Workshop,
    },
};
