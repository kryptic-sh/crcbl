//! Choosing a backend at runtime.
//!
//! # The decision: runtime selection, one registry, `Box<dyn Shell>`
//!
//! `docs/plan/15-windowing.md` requires it in one sentence — "backend selection
//! at runtime, not compile time, on Linux: try Wayland socket → fall back to
//! X11 (both compiled in; `CRCBL_SHELL=x11` override)" — and that sentence has
//! consequences the API has to absorb *now*, because retrofitting them costs a
//! rewrite of every consumer:
//!
//! 1. **[`Shell`] must be object-safe.** A generic `Shell` would make the
//!    backend a compile-time parameter, and "try Wayland, fall back to X11"
//!    would then have to monomorphise every consumer twice and pick between the
//!    two instantiations with an `enum` wrapper — which is a hand-written
//!    vtable with extra steps. `crcbl-hal` reached the same conclusion for the
//!    same reason and states the costs; they apply here in a milder form, since
//!    the shell is called a handful of times per frame rather than per draw.
//! 2. **The entry point cannot be a constructor.** `WaylandShell::new()` puts
//!    the platform name in the caller's source, which is the regression the
//!    whole seam exists to prevent. So the entry point is [`open`], and the
//!    only concrete shell type a consumer ever names is
//!    [`HeadlessShell`] — deliberately, because a test
//!    asking for determinism is asking for a *specific* implementation.
//!
//! # What is registered today
//!
//! Only [`HeadlessShell`], and only when asked for by
//! name. P0.5 and P0.6 add Wayland and X11 to this module's registry and nothing else in
//! this crate changes — that is the property this module is shaped to have.
//!
//! Headless is registered with `auto: false`, so [`open`] never selects it
//! implicitly. A game that silently ran headless because a compositor was
//! missing would look like a hang; failing with
//! [`ShellError::NoBackend`] names the actual problem.

use crate::{HeadlessShell, Shell, ShellError};

/// Which implementation is behind the [`Shell`] trait.
///
/// For logs, error messages and bug reports. Shell *behaviour* must key off
/// [`ShellCaps`](crate::ShellCaps), never off this — that is the difference
/// between a capability system and platform sniffing, and it is the same rule
/// `crcbl-hal`'s `BackendKind` carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellBackend {
    /// Wayland, through libwayland-client and our own protocol layer (P0.5).
    Wayland,
    /// X11, through libxcb and our own request/event layer (P0.6).
    X11,
    /// Win32 (P14).
    Win32,
    /// AppKit (P14).
    AppKit,
    /// A browser canvas, through the hand-rolled JS shim (P5).
    Web,
    /// [`HeadlessShell`] — no window system at all.
    Headless,
}

impl ShellBackend {
    /// The lowercase name used in `CRCBL_SHELL` and in log lines.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Win32 => "win32",
            Self::AppKit => "appkit",
            Self::Web => "web",
            Self::Headless => "headless",
        }
    }

    /// Parses a `CRCBL_SHELL` value.
    ///
    /// Accepts `x11` and its common misspelling `xcb`, because the backend is
    /// built on libxcb and someone will type it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wayland" | "wl" => Some(Self::Wayland),
            "x11" | "xcb" => Some(Self::X11),
            "win32" | "windows" => Some(Self::Win32),
            "appkit" | "macos" | "cocoa" => Some(Self::AppKit),
            "web" | "canvas" => Some(Self::Web),
            "headless" | "null" => Some(Self::Headless),
            _ => None,
        }
    }
}

impl core::fmt::Display for ShellBackend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The environment variable that overrides automatic backend selection.
pub const BACKEND_ENV_VAR: &str = "CRCBL_SHELL";

/// One entry in the backend table.
struct Registration {
    backend: ShellBackend,
    /// Whether [`open`] may select this entry without being asked for it by
    /// name. See the [module docs](self) for why headless is `false`.
    auto: bool,
    open: fn() -> Result<Box<dyn Shell>, ShellError>,
}

/// Every backend compiled into this build, in the order [`open`] tries them.
///
/// P0.5 inserts Wayland ahead of X11 here; P0.6 inserts X11. The order is the
/// preference order from `docs/plan/15-windowing.md`: Wayland first, X11 as the
/// fallback.
static REGISTRY: &[Registration] = &[Registration {
    backend: ShellBackend::Headless,
    auto: false,
    open: || Ok(Box::new(HeadlessShell::new())),
}];

fn registry_names(entries: impl Iterator<Item = ShellBackend>) -> String {
    let names: Vec<&str> = entries.map(ShellBackend::as_str).collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

/// Opens the best available shell for this process.
///
/// Reads [`BACKEND_ENV_VAR`]: when it names a backend, that backend is opened
/// and a failure is *not* fallen back from — someone who set the variable meant
/// it, and quietly running on a different backend turns a debugging session
/// into a wild goose chase. Otherwise the compiled-in backends are tried in
/// preference order and the first one that connects wins.
///
/// # Errors
///
/// * [`ShellError::UnknownBackend`] if `CRCBL_SHELL` names something this build
///   does not have.
/// * The backend's own error if a named backend fails to open.
/// * [`ShellError::NoBackend`] if nothing connected.
///
/// # Today
///
/// Only [`HeadlessShell`] is registered, and it is not
/// auto-selected, so this returns [`ShellError::NoBackend`] unless
/// `CRCBL_SHELL=headless` is set. That is the honest answer for a build with no
/// window-system backend in it; P0.5 changes it by adding a registration and
/// nothing else.
pub fn open() -> Result<Box<dyn Shell>, ShellError> {
    match std::env::var(BACKEND_ENV_VAR) {
        Ok(value) if !value.trim().is_empty() => {
            let Some(requested) = ShellBackend::from_name(&value) else {
                return Err(ShellError::UnknownBackend {
                    requested: value,
                    available: registry_names(REGISTRY.iter().map(|entry| entry.backend)),
                });
            };
            open_backend(requested)
        }
        _ => open_auto(),
    }
}

/// Opens a specific backend, ignoring [`BACKEND_ENV_VAR`].
///
/// For `crcbl` CLI subcommands that must be headless whatever the environment
/// says, and for tests.
///
/// # Errors
///
/// [`ShellError::UnknownBackend`] if this build has no such backend, or the
/// backend's own error if it fails to open.
pub fn open_backend(backend: ShellBackend) -> Result<Box<dyn Shell>, ShellError> {
    match REGISTRY.iter().find(|entry| entry.backend == backend) {
        Some(entry) => (entry.open)(),
        None => Err(ShellError::UnknownBackend {
            requested: backend.as_str().to_string(),
            available: registry_names(REGISTRY.iter().map(|entry| entry.backend)),
        }),
    }
}

/// Tries every auto-selectable backend in preference order.
fn open_auto() -> Result<Box<dyn Shell>, ShellError> {
    let mut tried = Vec::new();
    for entry in REGISTRY.iter().filter(|entry| entry.auto) {
        tried.push(entry.backend);
        match (entry.open)() {
            Ok(shell) => {
                log::info!("opened the {} shell backend", entry.backend);
                return Ok(shell);
            }
            // A backend that is compiled in but cannot connect is the normal
            // case on a Wayland-only or X11-only session, so it is a debug line
            // rather than a warning — but it is never silent, because "why did
            // it pick X11?" is a question someone will ask.
            Err(error) => log::debug!("{} shell backend unavailable: {error}", entry.backend),
        }
    }
    Err(ShellError::NoBackend {
        tried: registry_names(tried.into_iter()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_names_round_trip() {
        let backends = [
            ShellBackend::Wayland,
            ShellBackend::X11,
            ShellBackend::Win32,
            ShellBackend::AppKit,
            ShellBackend::Web,
            ShellBackend::Headless,
        ];
        for backend in backends {
            assert_eq!(ShellBackend::from_name(backend.as_str()), Some(backend));
            assert_eq!(backend.to_string(), backend.as_str());
        }
        // Aliases someone will actually type.
        assert_eq!(ShellBackend::from_name("XCB"), Some(ShellBackend::X11));
        assert_eq!(
            ShellBackend::from_name(" WaYlAnD "),
            Some(ShellBackend::Wayland)
        );
        assert_eq!(ShellBackend::from_name("mir"), None);
    }

    #[test]
    fn only_headless_is_registered_today_and_it_is_not_automatic() {
        // This test is the tripwire for P0.5/P0.6: adding a registration must
        // be a deliberate edit here too.
        assert_eq!(REGISTRY.len(), 1);
        assert_eq!(REGISTRY[0].backend, ShellBackend::Headless);
        assert!(!REGISTRY[0].auto, "a game must never silently run headless");
        assert!(REGISTRY.iter().all(|entry| !entry.auto));
    }

    #[test]
    fn opening_by_name_works_and_unknown_backends_are_rejected() {
        let shell = open_backend(ShellBackend::Headless).expect("headless is registered");
        assert_eq!(shell.backend(), ShellBackend::Headless);

        let error = open_backend(ShellBackend::Wayland).expect_err("not registered yet");
        let ShellError::UnknownBackend {
            requested,
            available,
        } = error
        else {
            panic!("wrong variant");
        };
        assert_eq!(requested, "wayland");
        assert_eq!(available, "headless");
    }

    #[test]
    fn automatic_selection_reports_what_it_tried() {
        let error = open_auto().expect_err("nothing is auto-selectable yet");
        let ShellError::NoBackend { tried } = error else {
            panic!("wrong variant");
        };
        assert_eq!(tried, "none");
    }
}
