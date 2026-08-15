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
//! Wayland then X11 on Linux, Win32 on Windows, AppKit on macOS, and
//! [`HeadlessShell`] everywhere — the last only when asked for by name. **P0.6
//! confirmed the property this module was shaped to have and P5C confirmed it
//! twice more**: adding a whole X11 backend touched [`REGISTRY`](self) and these
//! tests, and no other file outside `src/x11/` (plus `src/linux/`, which is the
//! evdev table and libxkbcommon moving up one level to be shared rather than
//! copied); adding the Win32 backend touched this file, `src/lib.rs` and
//! `src/win32/`, and adding the AppKit one touched this file, `src/lib.rs` and
//! `src/appkit/`. No consumer changed, no trait method was added, and no
//! `#[cfg]` appeared anywhere above this crate.
//!
//! **Every variant of [`ShellBackend`] is now registered somewhere**, which
//! changes how the "a backend this build does not have" property is tested. It
//! used to be asserted against whichever variant had not landed yet — X11 until
//! P0.6, Win32 until P5C, AppKit until this slice — and there is no fourth
//! candidate. The tests below now derive the absent set from the table itself
//! and assert the property per platform, which is what was actually meant all
//! along: *asking for a backend this build lacks is an honest
//! [`UnknownBackend`](ShellError::UnknownBackend)*. It also cannot go stale
//! again — a future backend is covered the day it is added, on the platforms
//! that do not have it.
//!
//! Headless is registered with `auto: false`, so [`open`] never selects it
//! implicitly. A game that silently ran headless because a compositor was
//! missing would look like a hang; failing with
//! [`ShellError::NoBackend`] names the actual problem.
//!
//! # Why both Linux entries are `dlopen`-backed, and the Windows one is not
//!
//! The registry is a fall-through list, so every entry has to be able to *fail*
//! at runtime. A backend linked against `libwayland-client.so.0` — or
//! `libxcb.so.1` — with `DT_NEEDED` cannot: the process dies in `ld.so` before
//! `main`, on any machine without the library, and this list never runs.
//! `src/wayland/ffi.rs` has the full argument and `src/x11/ffi.rs` sharpens it,
//! since X11 is the *fallback* and is reached only after Wayland has already
//! failed.
//!
//! The Win32 and AppKit entries link their libraries instead, and the
//! difference is this list's shape rather than a change of mind: each of those
//! platforms has one backend, so nothing falls through to anything, and neither
//! `user32.dll` nor AppKit can be missing from a system that is running a
//! process at all. `src/win32/ffi.rs` states it in full and `src/appkit/ffi.rs`
//! repeats it for the frameworks.

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
    /// Win32, through hand-written `extern "system"` declarations (P5C).
    Win32,
    /// AppKit (P5C).
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
    // Windows has exactly one backend, so nothing here falls through to
    // anything — which is what lets `src/win32/ffi.rs` link its libraries
    // instead of `dlopen`ing them. See that module for the asymmetry.
    #[cfg(target_os = "windows")]
    Registration {
        backend: ShellBackend::Win32,
        auto: true,
        open: || Ok(Box::new(crate::win32::Win32Shell::open()?)),
    },
    // macOS likewise has exactly one backend, so `src/appkit/ffi.rs` links its
    // frameworks for the same reason `src/win32/ffi.rs` links its DLLs.
    #[cfg(target_os = "macos")]
    Registration {
        backend: ShellBackend::AppKit,
        auto: true,
        open: || Ok(Box::new(crate::appkit::AppKitShell::open()?)),
    },
    Registration {
        backend: ShellBackend::Headless,
        auto: false,
        open: || Ok(Box::new(HeadlessShell::new())),
    },
    Registration {
        backend: ShellBackend::Web,
        auto: false,
        open: web_open,
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
/// `CRCBL_SHELL=x11` forces one of them. On Windows and on macOS there is one
/// backend each and it is selected automatically — although the AppKit one
/// requires the process's **main thread** and returns
/// [`ShellError::Backend`] naming that rule anywhere else, which is a failure
/// this list falls through rather than one it hides. Every other target has
/// only [`HeadlessShell`], which is not auto-selected, so this returns
/// [`ShellError::NoBackend`] there unless `CRCBL_SHELL=headless` is set — the
/// honest answer for a build with no window-system backend in it.
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

/// Opens the web backend, which exists only inside a browser.
///
/// Registered on every target — unlike the two Linux entries, which are
/// `#[cfg]`ed out where they cannot work — because the failure it has to produce
/// off wasm is a *diagnostic*. `CRCBL_SHELL=web` on a desktop used to hand back
/// a shell that could never receive an event, so the application hung in its
/// configure loop with nothing in the log; `UnknownBackend` would have been
/// almost as bad, since "web is not in this build" is not what is wrong. The
/// error names the actual problem instead. This is the same argument the module
/// docs make for why headless is `auto: false`.
///
/// The canvas id comes from the shim's `__crcbl_web_canvas`, not from an
/// environment variable: `std::env::var` on `wasm32-unknown-unknown` always
/// answers `NotPresent`, so a `CRCBL_CANVAS_ID` lookup silently always produced
/// the fallback.
fn web_open() -> Result<Box<dyn Shell>, ShellError> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Box::new(crate::web::open(crate::web::canvas_id())?))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Err(ShellError::Unsupported {
            backend: ShellBackend::Web,
            what: "the web backend outside a browser — build for wasm32-unknown-unknown",
        })
    }
}

/// Tries every auto-selectable backend in preference order.
fn open_auto() -> Result<Box<dyn Shell>, ShellError> {
    let mut tried = Vec::new();
    for entry in REGISTRY.iter().filter(|entry| entry.auto) {
        tried.push(entry.backend);
        match (entry.open)() {
            Ok(shell) => {
                crcbl_core::log::info!("opened the {} shell backend", entry.backend);
                return Ok(shell);
            }
            // A backend that is compiled in but cannot connect is the normal
            // case on a Wayland-only or X11-only session, so it is a debug line
            // rather than a warning — but it is never silent, because "why did
            // it pick X11?" is a question someone will ask.
            Err(error) => {
                crcbl_core::log::debug!("{} shell backend unavailable: {error}", entry.backend)
            }
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
                    ShellBackend::Headless,
                    ShellBackend::Web,
                ]
            );
        } else if cfg!(target_os = "windows") {
            assert_eq!(
                backends,
                [
                    ShellBackend::Win32,
                    ShellBackend::Headless,
                    ShellBackend::Web,
                ]
            );
        } else if cfg!(target_os = "macos") {
            assert_eq!(
                backends,
                [
                    ShellBackend::AppKit,
                    ShellBackend::Headless,
                    ShellBackend::Web,
                ]
            );
        } else {
            assert_eq!(backends, [ShellBackend::Headless, ShellBackend::Web]);
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

        // Registered everywhere, but it only *works* in a browser — and off
        // wasm it says so rather than handing back a shell that can never
        // produce an event.
        if !cfg!(target_arch = "wasm32") {
            let error = open_backend(ShellBackend::Web).expect_err("no browser here");
            let ShellError::Unsupported { backend, what } = error else {
                panic!("wrong variant");
            };
            assert_eq!(backend, ShellBackend::Web);
            assert!(what.contains("wasm32"), "{what}");
        }

        // **Every backend this build does not have**, derived from the table
        // rather than named. X11 was the example until P0.6 registered it,
        // Win32 until P5C did, and AppKit until the slice after that — at which
        // point every variant is registered *somewhere* and there is no fourth
        // candidate to promote. The property being asserted was never about a
        // particular backend: it is that asking for one this build lacks is an
        // honest `UnknownBackend` rather than a silent fallback onto something
        // else. Reading the absent set out of `REGISTRY` says exactly that, and
        // covers a future backend on the day it is added.
        let registered: Vec<ShellBackend> = REGISTRY.iter().map(|entry| entry.backend).collect();
        let absent: Vec<ShellBackend> = [
            ShellBackend::Wayland,
            ShellBackend::X11,
            ShellBackend::Win32,
            ShellBackend::AppKit,
            ShellBackend::Web,
            ShellBackend::Headless,
        ]
        .into_iter()
        .filter(|backend| !registered.contains(backend))
        .collect();
        // A loop over an empty set asserts nothing, and this one would be empty
        // if the `#[cfg]`s ever compiled every entry in at once.
        assert!(
            !absent.is_empty(),
            "every backend is registered on this platform, so nothing here checks the \
             UnknownBackend path: {registered:?}"
        );
        for backend in absent {
            let error = open_backend(backend).expect_err("not in this build");
            let ShellError::UnknownBackend {
                requested,
                available,
            } = error
            else {
                panic!("{backend} is not registered here, so it must answer UnknownBackend");
            };
            assert_eq!(requested, backend.as_str());
            assert!(available.contains("headless"), "{available}");
        }

        // And the other half, which is what makes the first half meaningful:
        // this platform's own backend **is** registered, so asking for it by
        // name gets a real shell or its own failure — never `UnknownBackend`.
        #[cfg(target_os = "windows")]
        {
            let shell = open_backend(ShellBackend::Win32).expect("registered on Windows");
            assert_eq!(shell.backend(), ShellBackend::Win32);
        }
        #[cfg(target_os = "macos")]
        {
            // Not `expect`: AppKit requires the process's main thread, and a
            // test body never is one — see `appkit::app`. The point here is the
            // *variant*, and the failure that is legal is the backend's own.
            match open_backend(ShellBackend::AppKit) {
                Ok(shell) => assert_eq!(shell.backend(), ShellBackend::AppKit),
                Err(error) => assert!(
                    !matches!(error, ShellError::UnknownBackend { .. }),
                    "AppKit is registered on macOS, so this must be its own failure: {error}"
                ),
            }
        }
    }

    #[test]
    fn automatic_selection_reports_what_it_tried() {
        // On a machine with a compositor this genuinely connects, which is the
        // correct behaviour and not something to assert against; the point of
        // the test is the failure path's message.
        match open_auto() {
            Ok(shell) => assert!(
                matches!(
                    shell.backend(),
                    ShellBackend::Wayland
                        | ShellBackend::X11
                        | ShellBackend::Win32
                        | ShellBackend::AppKit
                ),
                "the auto-selectable backends are the two Linux ones — Wayland \
                 first — and the single Windows and macOS ones: {}",
                shell.backend()
            ),
            Err(ShellError::NoBackend { tried }) => {
                let expected = if cfg!(target_os = "linux") {
                    "wayland, x11"
                } else if cfg!(target_os = "windows") {
                    "win32"
                } else if cfg!(target_os = "macos") {
                    // Reached on every macOS test run, because a test body is
                    // never the main thread and `AppKitShell::open` refuses
                    // there — so this branch is the *normal* path on that
                    // platform rather than the degraded one, and it is still
                    // asserting the same thing: the failure names what it tried.
                    "appkit"
                } else {
                    "none"
                };
                assert_eq!(tried, expected, "every attempt is named, in order");
            }
            Err(error) => panic!("unexpected error: {error}"),
        }
    }
}
