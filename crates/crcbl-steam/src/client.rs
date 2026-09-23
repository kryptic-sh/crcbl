//! Bringing Steam up, and taking it down exactly once.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt,
    marker::PhantomData,
    rc::Weak,
    sync::{Arc, atomic::Ordering},
    thread::ThreadId,
};

use crate::{
    SteamEvent,
    call::CallRegistry,
    error::InitError,
    ffi::{
        HSteamPipe, ISteamApps, ISteamFriends, ISteamInput, ISteamMatchmaking,
        ISteamNetworkingSockets, ISteamNetworkingUtils, ISteamRemotePlay, ISteamRemoteStorage,
        ISteamScreenshots, ISteamTimeline, ISteamUgc, ISteamUser, ISteamUserStats, ISteamUtils,
        Lib, SteamErrMsg, init_result, load, manifest, manifest::Accessor, versions,
    },
    input::PadQueue,
    matchmaking::Tracked,
    net::IncomingQueues,
    presence::PresenceKeys,
    pump::PumpDiagnostics,
};

/// A Steam app id — `AppId_t`. `480` is Valve's shared SpaceWar test app,
/// which every Steamworks developer may run under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AppId(pub u32);

/// A successful `SteamInternal_SteamAPI_Init` on one library, balanced by
/// `SteamAPI_Shutdown` when dropped.
///
/// Built the moment init succeeds, before anything else can fail, so an init
/// that fails later — an interface the client cannot provide, the wrong app —
/// still shuts down what it started. An init that fails at
/// `SteamInternal_SteamAPI_Init` itself never builds one and never calls
/// `SteamAPI_Shutdown`.
///
/// It also records the pump thread — the one init ran on — because the
/// shutdown is the last Steam call the `Send` surfaces can cause: when the
/// last owner of the [`Client`] is dropped on another thread, the shutdown is
/// skipped (and logged) rather than made there. The library then stays
/// initialised, and marked live, until the process exits.
struct Session {
    lib: &'static Lib,
    pump_thread: ThreadId,
}

impl Drop for Session {
    fn drop(&mut self) {
        if std::thread::current().id() != self.pump_thread {
            log::warn!(
                "steam: the last owner of the Steam session was dropped off the pump thread; \
                 SteamAPI_Shutdown was skipped and Steam stays initialised until exit"
            );
            return;
        }
        // SAFETY: this session's init succeeded and has not been shut down;
        // `SteamAPI_Shutdown` takes no arguments, and this is the thread init
        // ran on.
        unsafe { (self.lib.fns.lifecycle.shutdown)() };
        self.lib.live.store(false, Ordering::Release);
    }
}

/// Everything Steam hands out that must outlive every user of it and then die
/// exactly once: the pipe and the interface pointers, plus the session whose
/// drop is `SteamAPI_Shutdown`.
///
/// Shared behind an `Arc`, so that a surface which must outlive the pump
/// owner keeps Steam alive by holding a clone rather than calling into a
/// shut-down API — and so the one `Send` surface, `SteamTransport` (whose
/// `crcbl_net::Transport` bound requires `Send`), can hold it.
///
/// # Why `Send` and `Sync` are sound here
///
/// Valve documents no thread safety for the interfaces this holds. So nothing
/// dereferences them off the thread init ran on: every call a `Send` surface
/// makes goes through [`on_pump_thread`](Self::on_pump_thread) first, and off
/// that thread it returns an error or, in a `Drop`, skips the call and logs
/// — without touching Steam. Everything else that holds a `Client` (`Steam`,
/// `Lobby`, `SteamListener`) is `!Send`, so it only ever runs on the pump
/// thread. The pointers are therefore only ever used from one thread, which
/// is the property the `unsafe impl`s below assert.
///
/// Nominally `pub` inside this private module, and never exported: the
/// sealed `CallResult` trait names it in a method only this crate can call,
/// and a sealed trait may only mention types as visible as itself.
pub struct Client {
    /// The loaded library.
    pub(crate) lib: &'static Lib,
    /// The pipe manual dispatch drains.
    pub(crate) pipe: HSteamPipe,
    /// `SteamAPI_SteamUser_v023()`; never null.
    pub(crate) user: *mut ISteamUser,
    /// `SteamAPI_SteamUtils_v011()`; never null.
    pub(crate) utils: *mut ISteamUtils,
    /// `SteamAPI_SteamFriends_v018()`; never null.
    pub(crate) friends: *mut ISteamFriends,
    /// `SteamAPI_SteamApps_v009()`; never null.
    pub(crate) apps: *mut ISteamApps,
    /// `SteamAPI_SteamMatchmaking_v009()`; never null.
    pub(crate) matchmaking: *mut ISteamMatchmaking,
    /// `SteamAPI_SteamNetworkingSockets_SteamAPI_v013()`; never null.
    pub(crate) net: *mut ISteamNetworkingSockets,
    /// `SteamAPI_SteamNetworkingUtils_SteamAPI_v004()`; never null.
    pub(crate) net_utils: *mut ISteamNetworkingUtils,
    /// `SteamAPI_SteamRemoteStorage_v016()`; never null.
    pub(crate) remote_storage: *mut ISteamRemoteStorage,
    /// `SteamAPI_SteamUserStats_v013()`; never null.
    pub(crate) user_stats: *mut ISteamUserStats,
    /// `SteamAPI_SteamInput_v007()`; never null.
    pub(crate) input: *mut ISteamInput,
    /// `SteamAPI_SteamRemotePlay_v004()`; never null.
    pub(crate) remote_play: *mut ISteamRemotePlay,
    /// `SteamAPI_SteamScreenshots_v003()`; never null.
    pub(crate) screenshots: *mut ISteamScreenshots,
    /// `SteamAPI_SteamTimeline_v004()`; never null.
    pub(crate) timeline: *mut ISteamTimeline,
    /// `SteamAPI_SteamUGC_v021()`; never null.
    pub(crate) ugc: *mut ISteamUgc,
    /// Dropped last, after every other field: the shutdown.
    session: Session,
}

// SAFETY: see "Why `Send` and `Sync` are sound here" above — the pointers
// are only dereferenced on the pump thread, which every `Send` holder checks
// before any Steam call, and `Session`'s `Drop` checks for itself.
unsafe impl Send for Client {}
// SAFETY: as above; `Client` has no interior mutability of its own.
unsafe impl Sync for Client {}

impl Client {
    /// Whether the calling thread is the one Steam was initialised on — the
    /// only thread any Steam call is made from.
    pub(crate) fn on_pump_thread(&self) -> bool {
        std::thread::current().id() == self.session.pump_thread
    }
}

/// The Steam API, initialised: the pump owner and the way to every interface.
///
/// One live `Steam` per loaded library — a second [`init`](Self::init) while
/// one lives is [`InitError::AlreadyInitialised`]. `!Send` and `!Sync`: one
/// owner, and the thread that initialised it is the pump thread. Dropping it
/// shuts Steam down, once nothing else — a `SteamTransport`, a `Lobby` — still
/// holds the session.
///
/// Each frame: [`pump`](Self::pump), then drain [`events`](Self::events),
/// then act on them.
pub struct Steam {
    pub(crate) client: Arc<Client>,
    pub(crate) queue: VecDeque<SteamEvent>,
    pub(crate) diagnostics: PumpDiagnostics,
    /// Strings Steam returned that were not intact UTF-8. A `Cell` because
    /// strings are read through `&Steam`; `Steam` is `!Sync` regardless.
    pub(crate) lossy_strings: Cell<u64>,
    /// The asynchronous calls waiting on an answer.
    pub(crate) calls: CallRegistry,
    /// The lobbies a `Lobby` value is held for, and their last-seen owner.
    pub(crate) lobbies: Vec<Tracked>,
    /// The rich-presence keys set, for the key limit.
    pub(crate) presence_keys: PresenceKeys,
    /// Whether `InitRelayNetworkAccess` has been called.
    pub(crate) relay_started: Cell<bool>,
    /// Where each open `SteamListener` receives its incoming connections.
    pub(crate) incoming: IncomingQueues,
    /// The app Steam is running, as init checked it.
    pub(crate) app: AppId,
    /// Whether the local user's stats have arrived (`UserStatsReceived_t`).
    pub(crate) stats_ready: bool,
    /// The live `VoiceCapture`'s token, if one is open. A `RefCell` because
    /// a capture is opened through `&Steam`.
    pub(crate) voice_capture: RefCell<Weak<()>>,
    /// The open `SteamPads`' queue of device changes, if one is open: the
    /// pump runs `ISteamInput::RunFrame` while it is, and hands it every
    /// device callback.
    pub(crate) pads: Weak<PadQueue>,
    /// `Steam` stays on the thread that made it, whatever its fields allow.
    pub(crate) _not_send: PhantomData<*const ()>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("pipe", &self.pipe)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Steam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Steam")
            .field("pipe", &self.client.pipe)
            .field("queued", &self.queue.len())
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl Steam {
    /// Loads the Steam library, initialises the API with a version handshake,
    /// switches to manual callback dispatch, and resolves every bound
    /// interface.
    ///
    /// The library is looked for beside the executable, then under
    /// `$CRCBL_STEAM_SDK/redistributable_bin/<platform>/`; it is loaded once
    /// per process and never unloaded. The handshake passes the
    /// interface-version string of every interface this crate binds, so a
    /// client too old for them refuses rather than handing out an interface
    /// this crate would misread.
    ///
    /// In development, when Steam did not launch the process, Steam reads the
    /// app id from `steam_appid.txt` in the working directory. This crate never
    /// writes that file or sets `SteamAppId`; if it is missing the error says
    /// so. `app` must match what Steam reports, or init fails with
    /// [`InitError::WrongApp`].
    ///
    /// # Errors
    ///
    /// Every [`InitError`] variant; all of them mean "run without Steam".
    pub fn init(app: AppId) -> Result<Self, InitError> {
        init_on(load::real()?, app)
    }

    /// The ships-through-Steam guard (`SteamAPI_RestartAppIfNecessary`):
    /// `Ok(true)` means Steam is relaunching the game through the client, and
    /// this process should quit now; `Ok(false)` means carry on.
    ///
    /// Called before [`init`](Self::init), and needs no running `Steam`. It
    /// loads the library into the same never-unloaded cache `init` uses, so
    /// calling both opens it once. **Always `Ok(false)` while a
    /// `steam_appid.txt` is in the working directory** — right for
    /// development, and why a shipped build must not carry one.
    ///
    /// # Errors
    ///
    /// [`InitError::NoLibrary`] or [`InitError::NoSymbol`] when the library
    /// cannot be loaded; a game that ships through Steam treats either as
    /// "not launched properly".
    pub fn relaunch_via_steam(app: AppId) -> Result<bool, InitError> {
        Ok(relaunch_on(load::real()?, app))
    }
}

/// `Steam::relaunch_via_steam` over a given library.
pub(crate) fn relaunch_on(lib: &'static Lib, app: AppId) -> bool {
    // SAFETY: takes the app id by value and needs no prior init.
    unsafe { (lib.fns.lifecycle.restart_app_if_necessary)(app.0) }
}

/// `Steam::init` over a given library: the real one, or a test's fake.
pub(crate) fn init_on(lib: &'static Lib, app: AppId) -> Result<Steam, InitError> {
    if lib.live.swap(true, Ordering::AcqRel) {
        return Err(InitError::AlreadyInitialised);
    }
    let session = match start(lib, std::thread::current().id()) {
        Ok(session) => session,
        Err(err) => {
            lib.live.store(false, Ordering::Release);
            return Err(err);
        }
    };
    // From here every early return drops `session`, which shuts down.

    // SAFETY: init succeeded; neither call takes arguments.
    let pipe = unsafe {
        (lib.fns.dispatch.init)();
        (lib.fns.lifecycle.get_pipe)()
    };
    let user = interface(lib.fns.user.accessor, &versions::USER)?.cast::<ISteamUser>();
    let utils = interface(lib.fns.utils.accessor, &versions::UTILS)?.cast::<ISteamUtils>();
    let friends = interface(lib.fns.friends.accessor, &versions::FRIENDS)?.cast::<ISteamFriends>();
    let apps = interface(lib.fns.apps.accessor, &versions::APPS)?.cast::<ISteamApps>();
    let matchmaking = interface(lib.fns.matchmaking.accessor, &versions::MATCHMAKING)?
        .cast::<ISteamMatchmaking>();
    let net = interface(lib.fns.net.accessor, &versions::NETWORKING_SOCKETS)?
        .cast::<ISteamNetworkingSockets>();
    let net_utils = interface(lib.fns.net_utils.accessor, &versions::NETWORKING_UTILS)?
        .cast::<ISteamNetworkingUtils>();
    let remote_storage = interface(lib.fns.remote_storage.accessor, &versions::REMOTE_STORAGE)?
        .cast::<ISteamRemoteStorage>();
    let user_stats =
        interface(lib.fns.user_stats.accessor, &versions::USER_STATS)?.cast::<ISteamUserStats>();
    let input = interface(lib.fns.input.accessor, &versions::INPUT)?.cast::<ISteamInput>();
    let remote_play =
        interface(lib.fns.remote_play.accessor, &versions::REMOTE_PLAY)?.cast::<ISteamRemotePlay>();
    let screenshots = interface(lib.fns.screenshots.accessor, &versions::SCREENSHOTS)?
        .cast::<ISteamScreenshots>();
    let timeline =
        interface(lib.fns.timeline.accessor, &versions::TIMELINE)?.cast::<ISteamTimeline>();
    let ugc = interface(lib.fns.ugc.accessor, &versions::UGC)?.cast::<ISteamUgc>();

    // SAFETY: `utils` is a live, non-null `ISteamUtils`.
    let running = AppId(unsafe { (lib.fns.utils.get_app_id)(utils) });
    if running != app {
        return Err(InitError::WrongApp {
            expected: app,
            running,
        });
    }

    Ok(Steam {
        client: Arc::new(Client {
            lib,
            pipe,
            user,
            utils,
            friends,
            apps,
            matchmaking,
            net,
            net_utils,
            remote_storage,
            user_stats,
            input,
            remote_play,
            screenshots,
            timeline,
            ugc,
            session,
        }),
        queue: VecDeque::new(),
        diagnostics: PumpDiagnostics::default(),
        lossy_strings: Cell::new(0),
        calls: CallRegistry::default(),
        lobbies: Vec::new(),
        presence_keys: PresenceKeys::default(),
        relay_started: Cell::new(false),
        incoming: IncomingQueues::default(),
        app: running,
        stats_ready: false,
        voice_capture: RefCell::new(Weak::new()),
        pads: Weak::new(),
        _not_send: PhantomData,
    })
}

/// `SteamInternal_SteamAPI_Init` with the bound interfaces' versions.
fn start(lib: &'static Lib, pump_thread: ThreadId) -> Result<Session, InitError> {
    let versions = versions::handshake(manifest::INTERFACES);
    let mut message: SteamErrMsg = [0; 1024];
    // SAFETY: `versions` is a NUL-separated, double-NUL-terminated list that
    // outlives the call; `message` is a writable `SteamErrMsg`.
    let result = unsafe { (lib.fns.lifecycle.init)(versions.as_ptr().cast(), &raw mut message) };
    let message = err_msg(&message);
    match result {
        init_result::OK => Ok(Session { lib, pump_thread }),
        init_result::NO_STEAM_CLIENT => {
            let cwd = std::env::current_dir().ok();
            let appid_file = cwd
                .as_ref()
                .is_some_and(|dir| dir.join("steam_appid.txt").is_file());
            Err(InitError::NoSteamClient {
                message,
                cwd,
                appid_file,
            })
        }
        init_result::VERSION_MISMATCH => Err(InitError::VersionMismatch(message)),
        init_result::FAILED_GENERIC => Err(InitError::Failed(message)),
        other => Err(InitError::Failed(format!(
            "ESteamAPIInitResult {other}: {message}"
        ))),
    }
}

/// Copies Valve's message out of a `SteamErrMsg`: up to the first NUL, or
/// the whole buffer if Steam left none, never past it.
fn err_msg(buffer: &SteamErrMsg) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Calls an interface accessor, refusing null.
fn interface(
    accessor: Accessor,
    iface: &versions::Interface,
) -> Result<*mut core::ffi::c_void, InitError> {
    // SAFETY: init succeeded, and the accessor takes no arguments.
    let raw = unsafe { accessor() };
    if raw.is_null() {
        return Err(InitError::NoInterface(iface.accessor));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, script};

    #[test]
    fn init_passes_the_bound_versions_and_resolves_every_interface() {
        let lib = testing::fake_lib();
        let steam = init_on(lib, AppId(480)).unwrap();
        assert_eq!(
            script(|s| s.handshake.clone()),
            Some(versions::handshake(manifest::INTERFACES))
        );
        // Spelled out once, so a row dropped from the manifest shows up here
        // rather than vanishing from both sides of the comparison above.
        assert_eq!(
            versions::handshake(manifest::INTERFACES),
            b"SteamUser023\0SteamFriends018\0SteamMatchMaking009\0\
              SteamNetworkingSockets013\0SteamNetworkingUtils004\0\
              STEAMAPPS_INTERFACE_VERSION009\0STEAMREMOTEPLAY_INTERFACE_VERSION004\0STEAMREMOTESTORAGE_INTERFACE_VERSION016\0\
              STEAMUSERSTATS_INTERFACE_VERSION013\0SteamInput007\0\
              STEAMSCREENSHOTS_INTERFACE_VERSION003\0STEAMUGC_INTERFACE_VERSION021\0\
              SteamUtils011\0\0"
        );
        assert_eq!(script(|s| s.calls.dispatch_init), 1);
        assert_eq!(steam.client.pipe, testing::PIPE);
        assert!(!steam.client.user.is_null());
        assert!(!steam.client.utils.is_null());
        assert!(!steam.client.friends.is_null());
        assert!(!steam.client.apps.is_null());
        assert!(!steam.client.remote_storage.is_null());
        assert!(!steam.client.user_stats.is_null());
        assert!(!steam.client.input.is_null());
        assert!(!steam.client.screenshots.is_null());
        assert!(!steam.client.remote_play.is_null());
        assert!(!steam.client.timeline.is_null());
        assert!(!steam.client.ugc.is_null());
        assert!(!steam.client.matchmaking.is_null());
    }

    #[test]
    fn each_init_result_is_its_own_error_with_valves_message() {
        for (code, expect) in [
            (
                init_result::FAILED_GENERIC,
                InitError::Failed("valve says".into()),
            ),
            (
                init_result::VERSION_MISMATCH,
                InitError::VersionMismatch("valve says".into()),
            ),
            (
                42,
                InitError::Failed("ESteamAPIInitResult 42: valve says".into()),
            ),
        ] {
            let lib = testing::fake_lib();
            script(|s| {
                s.init_result = code;
                s.init_message = b"valve says".to_vec();
            });
            assert_eq!(init_on(lib, AppId(480)).unwrap_err(), expect, "code {code}");
        }
    }

    #[test]
    fn no_steam_client_says_where_steam_looked_for_the_app_id() {
        let lib = testing::fake_lib();
        script(|s| {
            s.init_result = init_result::NO_STEAM_CLIENT;
            s.init_message = b"no client".to_vec();
        });
        let cwd = std::env::current_dir().ok();
        let present = cwd
            .as_ref()
            .is_some_and(|dir| dir.join("steam_appid.txt").is_file());
        assert_eq!(
            init_on(lib, AppId(480)).unwrap_err(),
            InitError::NoSteamClient {
                message: "no client".into(),
                cwd,
                appid_file: present,
            }
        );
    }

    #[test]
    fn a_message_filling_the_whole_buffer_is_read_to_its_end_and_no_further() {
        let lib = testing::fake_lib();
        script(|s| {
            s.init_result = init_result::FAILED_GENERIC;
            s.init_message = vec![b'x'; 1024];
        });
        assert_eq!(
            init_on(lib, AppId(480)).unwrap_err(),
            InitError::Failed("x".repeat(1024))
        );
    }

    #[test]
    fn a_failed_init_never_shuts_down_and_can_be_retried() {
        let lib = testing::fake_lib();
        script(|s| s.init_result = init_result::FAILED_GENERIC);
        assert!(init_on(lib, AppId(480)).is_err());
        assert_eq!(script(|s| s.calls.shutdown), 0);
        script(|s| s.init_result = init_result::OK);
        assert!(init_on(lib, AppId(480)).is_ok());
    }

    #[test]
    fn a_null_interface_is_no_interface_naming_it_and_shuts_down_once() {
        for iface in manifest::INTERFACES {
            let lib = testing::fake_lib();
            script(|s| s.null_accessor = Some(iface.accessor));
            assert_eq!(
                init_on(lib, AppId(480)).unwrap_err(),
                InitError::NoInterface(iface.accessor)
            );
            assert_eq!(script(|s| s.calls.shutdown), 1, "{}", iface.accessor);
            script(|s| s.calls.shutdown = 0);
        }
        assert!(!manifest::INTERFACES.is_empty(), "the loop checked nothing");
    }

    #[test]
    fn relaunch_passes_the_app_and_steams_answer_through() {
        let lib = testing::fake_lib();
        for answer in [false, true] {
            script(|s| s.restart = answer);
            assert_eq!(relaunch_on(lib, AppId(480)), answer);
        }
        assert_eq!(script(|s| s.restart_asked.clone()), [480, 480]);
        // Needs no init, and starts none.
        assert_eq!(script(|s| (s.calls.init, s.calls.shutdown)), (0, 0));
    }

    #[test]
    fn the_wrong_app_is_refused_and_shuts_down() {
        let lib = testing::fake_lib();
        assert_eq!(
            init_on(lib, AppId(12)).unwrap_err(),
            InitError::WrongApp {
                expected: AppId(12),
                running: AppId(480),
            }
        );
        assert_eq!(script(|s| s.calls.shutdown), 1);
    }

    #[test]
    fn one_live_steam_per_library() {
        let lib = testing::fake_lib();
        let first = init_on(lib, AppId(480)).unwrap();
        assert_eq!(
            init_on(lib, AppId(480)).unwrap_err(),
            InitError::AlreadyInitialised
        );
        // The refused second init touched nothing.
        assert_eq!(script(|s| s.calls.init), 1);
        drop(first);
        assert!(init_on(lib, AppId(480)).is_ok());
        // A second library is independent of the first.
        let _other = init_on(testing::fake_lib(), AppId(480)).unwrap();
    }

    #[test]
    fn shutdown_runs_once_when_the_last_owner_drops_and_not_before() {
        let lib = testing::fake_lib();
        let steam = init_on(lib, AppId(480)).unwrap();
        let clone = Arc::clone(&steam.client);
        drop(steam);
        assert_eq!(script(|s| s.calls.shutdown), 0);
        drop(clone);
        assert_eq!(script(|s| s.calls.shutdown), 1);
    }
}
