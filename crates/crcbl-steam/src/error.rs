//! Why Steam could not be brought up.

use std::path::PathBuf;

use crate::client::AppId;

/// Why `Steam::init` failed. Every variant is a distinct failure a game may
/// want to report differently; all of them mean "run without Steam".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InitError {
    /// No Steam library could be opened. `tried` is every path, in search
    /// order; `loader` is what the OS loader said about each (`dlerror`, or
    /// `LoadLibraryExW`'s error code).
    #[error("no Steam library found (tried {}): {loader}", display_paths(tried))]
    NoLibrary {
        /// Every path tried, in order.
        tried: Vec<PathBuf>,
        /// The loader's reason for each, joined.
        loader: String,
    },
    /// The library lacks a function this crate binds: an SDK older than these
    /// declarations.
    #[error("the Steam library has no symbol {0} (an SDK older than crcbl-steam's declarations)")]
    NoSymbol(&'static str),
    /// An interface accessor returned null: the running Steam client cannot
    /// provide that interface revision.
    #[error("the Steam client does not provide {0}")]
    NoInterface(&'static str),
    /// `k_ESteamAPIInitResult_NoSteamClient`: Steam is not running, or not
    /// logged in, or the app id could not be determined. `message` is Valve's;
    /// `cwd` and `appid_file` say whether a development launch had a
    /// `steam_appid.txt` where Steam looks for one.
    #[error(
        "Steam is not running or could not identify the app: {message} (working directory {}, steam_appid.txt {})",
        cwd.as_ref().map_or_else(|| "unknown".to_owned(), |dir| dir.display().to_string()),
        if *appid_file { "present" } else { "absent" }
    )]
    NoSteamClient {
        /// Valve's message.
        message: String,
        /// The working directory, where Steam reads `steam_appid.txt` from on
        /// a launch it did not make; `None` if the OS would not say.
        cwd: Option<PathBuf>,
        /// Whether `steam_appid.txt` exists in `cwd`.
        appid_file: bool,
    },
    /// `k_ESteamAPIInitResult_VersionMismatch`: the client is older than an
    /// interface version in the handshake. Valve's message.
    #[error("the Steam client is older than crcbl-steam's interfaces: {0}")]
    VersionMismatch(String),
    /// `k_ESteamAPIInitResult_FailedGeneric`, or a result code this crate does
    /// not know (named in the message). Valve's message.
    #[error("Steam failed to initialise: {0}")]
    Failed(String),
    /// Steam came up as a different app than the game asked for — typically a
    /// `steam_appid.txt` left over from another project.
    #[error("Steam is running app {running:?}, but the game asked for {expected:?}")]
    WrongApp {
        /// What the game passed to `Steam::init`.
        expected: AppId,
        /// What `ISteamUtils::GetAppID` reports.
        running: AppId,
    },
    /// A `Steam` over this library is still alive; drop it first.
    #[error("Steam is already initialised")]
    AlreadyInitialised,
}

fn display_paths(paths: &[PathBuf]) -> String {
    let shown: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    shown.join(", ")
}

/// Steam's `EResult`: the outcome code most calls and call results carry.
///
/// A newtype over the raw value rather than an enum, because the SDK names
/// well over a hundred codes and adds more; the few this crate acts on are
/// named constants, and every other value is kept, never mapped to a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EResult(pub i32);

impl EResult {
    /// `k_EResultOK`.
    pub const OK: Self = Self(1);
    /// `k_EResultFail` — a generic failure.
    pub const FAIL: Self = Self(2);
    /// `k_EResultNoConnection` — the client has no connection to Steam's
    /// servers.
    pub const NO_CONNECTION: Self = Self(3);
    /// `k_EResultInvalidParam`.
    pub const INVALID_PARAM: Self = Self(8);
    /// `k_EResultPending` — not answered yet, as an inventory result is
    /// until it is ready.
    pub const PENDING: Self = Self(22);
    /// `k_EResultInvalidState`.
    pub const INVALID_STATE: Self = Self(11);
    /// `k_EResultAccessDenied`.
    pub const ACCESS_DENIED: Self = Self(15);
    /// `k_EResultTimeout`.
    pub const TIMEOUT: Self = Self(16);
    /// `k_EResultLimitExceeded` — too many of something, e.g. lobbies.
    pub const LIMIT_EXCEEDED: Self = Self(25);
    /// `k_EResultIgnored`.
    pub const IGNORED: Self = Self(41);
}

impl core::fmt::Display for EResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match *self {
            Self::OK => "OK",
            Self::FAIL => "Fail",
            Self::NO_CONNECTION => "NoConnection",
            Self::INVALID_PARAM => "InvalidParam",
            Self::INVALID_STATE => "InvalidState",
            Self::ACCESS_DENIED => "AccessDenied",
            Self::TIMEOUT => "Timeout",
            Self::PENDING => "Pending",
            Self::LIMIT_EXCEEDED => "LimitExceeded",
            Self::IGNORED => "Ignored",
            _ => return write!(f, "EResult {}", self.0),
        };
        write!(f, "{name} ({})", self.0)
    }
}

/// Why a Steam call made through an initialised [`Steam`](crate::Steam)
/// failed. Each variant names the argument or the call, so a log line says
/// which.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SteamError {
    /// A string argument contains a NUL, which would silently cut it short at
    /// the C boundary; it is refused rather than truncated.
    #[error("{0} contains a NUL byte")]
    InteriorNul(&'static str),
    /// An argument is longer than Steam accepts; refused before the call.
    #[error("{argument} is {len} bytes, over Steam's limit of {max}")]
    TooLong {
        /// Which argument.
        argument: &'static str,
        /// Its length in bytes.
        len: usize,
        /// The most Steam accepts.
        max: usize,
    },
    /// One more of something Steam holds a fixed number of; refused before
    /// the call.
    #[error("{what}: Steam holds at most {max}")]
    TooMany {
        /// What there would be too many of.
        what: &'static str,
        /// The most Steam holds.
        max: usize,
    },
    /// Steam answered `false`, or an invalid call handle, for the named call.
    #[error("Steam refused {0}")]
    Refused(&'static str),
    /// Steam answered with this `EResult`.
    #[error("Steam answered {0}")]
    Result(EResult),
    /// Joining a lobby failed with this `EChatRoomEnterResponse`.
    #[error("could not enter the lobby: {0:?}")]
    LobbyEnter(crate::matchmaking::EnterResponse),
    /// An image Steam reports is too large to allocate, or for its size to
    /// be handed back through the `int` `GetImageRGBA` takes.
    #[error("Steam reports a {width}x{height} image, too large to copy")]
    ImageTooLarge {
        /// The width Steam reported.
        width: u32,
        /// The height Steam reported.
        height: u32,
    },
    /// A stats call before the local user's stats have arrived
    /// (`UserStatsReceived_t`); Steam would answer `false`, which reads like
    /// a misspelt name.
    #[error("the local user's stats have not arrived from Steam yet")]
    StatsNotReady,
    /// A number outside what Steam accepts — a priority past its maximum, a
    /// non-finite offset, a size that does not add up; refused before the
    /// call. Names the argument.
    #[error("{0} is outside what Steam accepts")]
    OutOfRange(&'static str),
    /// What Steam returned filled the whole buffer, so it may have been cut
    /// short; the named call's answer is refused rather than guessed at.
    #[error("{0} filled its whole buffer and may be truncated")]
    Truncated(&'static str),
}

/// Copies a Rust string into a NUL-terminated one for a call, refusing an
/// interior NUL and anything longer than `max` bytes (the NUL not counted).
pub(crate) fn c_string(
    text: &str,
    argument: &'static str,
    max: usize,
) -> Result<std::ffi::CString, SteamError> {
    if text.len() > max {
        return Err(SteamError::TooLong {
            argument,
            len: text.len(),
            max,
        });
    }
    std::ffi::CString::new(text).map_err(|_| SteamError::InteriorNul(argument))
}
