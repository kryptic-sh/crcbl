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
//! Wayland then X11 on Linux, and [`HeadlessShell`] everywhere — the latter
//! only when asked for by name. **P0.6 confirmed the property this module was
//! shaped to have**: adding a whole X11 backend touched
//! [`REGISTRY`](self) and these tests, and no other file outside `src/x11/`
//! (plus `src/linux/`, which is the evdev table and libxkbcommon moving up one
//! level to be shared rather than copied). No consumer changed, no trait method
//! was added, and no `#[cfg]` appeared anywhere above this crate.
//!
//! Headless is registered with `auto: false`, so [`open`] never selects it
//! implicitly. A game that silently ran headless because a compositor was
//! missing would look like a hang; failing with
//! [`ShellError::NoBackend`] names the actual problem.
//!
//! # Why both Linux entries are `dlopen`-backed
//!
//! The registry is a fall-through list, so every entry has to be able to *fail*
//! at runtime. A backend linked against `libwayland-client.so.0` — or
//! `libxcb.so.1` — with `DT_NEEDED` cannot: the process dies in `ld.so` before
//! `main`, on any machine without the library, and this list never runs.
//! `src/wayland/ffi.rs` has the full argument and `src/x11/ffi.rs` sharpens it,
//! since X11 is the *fallback* and is reached only after Wayland has already
//! failed.

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
/// The order is the preference order from `docs/plan/15-windowing.md`: Wayland
/// first, X11 as the fallback. On a session running both — which is every
/// XWayland-capable compositor — Wayland wins, and `CRCBL_SHELL=x11` is how a
/// developer reproduces an X11 bug without logging out.
static REGISTRY: &[Registration] = &[
    // `#[cfg]` on the element, not on the whole table: on macOS and Windows
    // the entry simply is not there, so nothing else in this file mentions a
    // platform and `open_auto` needs no conditional compilation of its own.
    #[cfg(target_os = "linux")]
    Registration {
        backend: ShellBackend::Wayland,
        auto: true,
        open: || Ok(Box::new(crate::wayland::WaylandShell::open()?)),
    },
    #[cfg(target_os = "linux")]
    Registration {
        backend: ShellBackend::X11,
        auto: true,
        open: || Ok(Box::new(crate::x11::X11Shell::open()?)),
    },
    Registration {
        backend: ShellBackend::Headless,
        auto: false,
        open: || Ok(Box::new(HeadlessShell::new())),
    },
];

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
/// On Linux, Wayland is tried and then X11; `CRCBL_SHELL=wayland` or
/// `CRCBL_SHELL=x11` forces one of them. On macOS and
/// Windows only [`HeadlessShell`] is compiled in and it is not auto-selected,
/// so this returns [`ShellError::NoBackend`] unless `CRCBL_SHELL=headless` is
/// set — the honest answer for a build with no window-system backend in it.
/// P14 changes that by adding a registration and nothing else.
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
    fn the_table_matches_the_platform_and_headless_is_never_automatic() {
        // The tripwire for every backend slice: adding a registration must be a
        // deliberate edit here too.
        let backends: Vec<ShellBackend> = REGISTRY.iter().map(|entry| entry.backend).collect();
        if cfg!(target_os = "linux") {
            assert_eq!(
                backends,
                [
                    ShellBackend::Wayland,
                    ShellBackend::X11,
                    ShellBackend::Headless
                ]
            );
        } else {
            assert_eq!(backends, [ShellBackend::Headless]);
        }
        let headless = REGISTRY
            .iter()
            .find(|entry| entry.backend == ShellBackend::Headless)
            .expect("headless is always compiled in");
        assert!(!headless.auto, "a game must never silently run headless");
        assert!(
            REGISTRY
                .iter()
                .filter(|entry| entry.auto)
                .all(|entry| entry.backend != ShellBackend::Headless)
        );
    }

    #[test]
    fn opening_by_name_works_and_unknown_backends_are_rejected() {
        let shell = open_backend(ShellBackend::Headless).expect("headless is registered");
        assert_eq!(shell.backend(), ShellBackend::Headless);

        // Win32 lands at P14, so it is the stable example of a backend this
        // build does not have. (X11 was that example until P0.6 registered it,
        // which is the point.)
        let error = open_backend(ShellBackend::Win32).expect_err("not registered yet");
        let ShellError::UnknownBackend {
            requested,
            available,
        } = error
        else {
            panic!("wrong variant");
        };
        assert_eq!(requested, "win32");
        assert!(available.contains("headless"), "{available}");
    }

    #[test]
    fn automatic_selection_reports_what_it_tried() {
        // On a machine with a compositor this genuinely connects, which is the
        // correct behaviour and not something to assert against; the point of
        // the test is the failure path's message.
        match open_auto() {
            Ok(shell) => assert!(
                matches!(shell.backend(), ShellBackend::Wayland | ShellBackend::X11),
                "the auto-selectable backends are the two Linux ones, and \
                 Wayland is tried first: {}",
                shell.backend()
            ),
            Err(ShellError::NoBackend { tried }) => {
                let expected = if cfg!(target_os = "linux") {
                    "wayland, x11"
                } else {
                    "none"
                };
                assert_eq!(tried, expected, "every attempt is named, in order");
            }
            Err(error) => panic!("unexpected error: {error}"),
        }
    }
}
