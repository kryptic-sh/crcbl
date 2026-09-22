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
