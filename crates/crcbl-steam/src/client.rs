//! Bringing Steam up, and taking it down exactly once.

use std::{cell::Cell, collections::VecDeque, fmt, rc::Rc, sync::atomic::Ordering};

use crate::{
    SteamEvent,
    error::InitError,
    ffi::{
        HSteamPipe, ISteamApps, ISteamFriends, ISteamUser, ISteamUtils, Lib, SteamErrMsg,
        init_result, load, manifest, manifest::Accessor, versions,
    },
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
struct Session {
    lib: &'static Lib,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: this session's init succeeded and has not been shut down;
        // `SteamAPI_Shutdown` takes no arguments.
        unsafe { (self.lib.fns.lifecycle.shutdown)() };
        self.lib.live.store(false, Ordering::Release);
    }
}

/// Everything Steam hands out that must outlive every user of it and then die
/// exactly once: the pipe and the interface pointers, plus the session whose
/// drop is `SteamAPI_Shutdown`.
///
/// Shared, so that a surface which must outlive the pump owner keeps Steam
/// alive by holding a clone rather than calling into a shut-down API. An `Rc`
/// while every holder is `!Send`: its raw pointers make it `!Send` and
/// `!Sync`, and so everything holding it. `docs/plan/42-steam.md`'s first
/// `Send` surface (slice 4's transport) is what turns it into an `Arc`, with
/// the pump-thread check that makes sharing it across threads sound.
pub(crate) struct Client {
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
    /// Dropped last, after every other field: the shutdown.
    _session: Session,
}

/// The Steam API, initialised: the pump owner and the way to every interface.
///
/// One live `Steam` per loaded library — a second [`init`](Self::init) while
/// one lives is [`InitError::AlreadyInitialised`]. `!Send` and `!Sync`: one
/// owner, one pump thread. Dropping it shuts Steam down.
///
/// Each frame: [`pump`](Self::pump), then drain [`events`](Self::events),
/// then act on them.
pub struct Steam {
    pub(crate) client: Rc<Client>,
    pub(crate) queue: VecDeque<SteamEvent>,
    pub(crate) diagnostics: PumpDiagnostics,
    /// Strings Steam returned that were not intact UTF-8. A `Cell` because
    /// strings are read through `&Steam`; `Steam` is `!Sync` regardless.
    pub(crate) lossy_strings: Cell<u64>,
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
    let session = match start(lib) {
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
    let friends =
        interface(lib.fns.friends.accessor, &versions::FRIENDS)?.cast::<ISteamFriends>();
    let apps = interface(lib.fns.apps.accessor, &versions::APPS)?.cast::<ISteamApps>();

    // SAFETY: `utils` is a live, non-null `ISteamUtils`.
    let running = AppId(unsafe { (lib.fns.utils.get_app_id)(utils) });
    if running != app {
        return Err(InitError::WrongApp {
            expected: app,
            running,
        });
    }

    Ok(Steam {
        client: Rc::new(Client {
            lib,
            pipe,
            user,
            utils,
            friends,
            apps,
            _session: session,
        }),
        queue: VecDeque::new(),
        diagnostics: PumpDiagnostics::default(),
        lossy_strings: Cell::new(0),
    })
}

/// `SteamInternal_SteamAPI_Init` with the bound interfaces' versions.
fn start(lib: &'static Lib) -> Result<Session, InitError> {
    let versions = versions::handshake(manifest::INTERFACES);
    let mut message: SteamErrMsg = [0; 1024];
    // SAFETY: `versions` is a NUL-separated, double-NUL-terminated list that
    // outlives the call; `message` is a writable `SteamErrMsg`.
    let result = unsafe { (lib.fns.lifecycle.init)(versions.as_ptr().cast(), &raw mut message) };
    let message = err_msg(&message);
    match result {
        init_result::OK => Ok(Session { lib }),
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
            b"SteamUser023\0SteamFriends018\0STEAMAPPS_INTERFACE_VERSION009\0SteamUtils011\0\0"
        );
        assert_eq!(script(|s| s.calls.dispatch_init), 1);
        assert_eq!(steam.client.pipe, testing::PIPE);
        assert!(!steam.client.user.is_null());
        assert!(!steam.client.utils.is_null());
        assert!(!steam.client.friends.is_null());
        assert!(!steam.client.apps.is_null());
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
        let clone = Rc::clone(&steam.client);
        drop(steam);
        assert_eq!(script(|s| s.calls.shutdown), 0);
        drop(clone);
        assert_eq!(script(|s| s.calls.shutdown), 1);
    }
}
