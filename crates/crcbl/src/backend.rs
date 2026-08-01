//! Choosing a GPU backend at runtime.
//!
//! # Why the registry lives here
//!
//! This crate's manifest used to say `crcbl-vk` "is deliberately absent and
//! stays absent", citing `docs/plan/11-cli-headless.md`'s rule that **a sample
//! linking `crcbl-vk` directly** is an architecture regression. P1.1 is the
//! slice that had to resolve that, and the resolution is the one
//! `crcbl-shell` already uses for window-system backends:
//!
//! * The rule protects *consumers*: nothing above the seam may name a backend,
//!   because that turns a runtime choice into a compile-time one and puts a
//!   platform name in a game's source. `apps/sandbox` still names none — it
//!   asks for [`GpuBackend::Vulkan`] or [`GpuBackend::Null`] by *value* and gets
//!   a `Box<dyn Instance>`.
//! * Something has to depend on the backends, or "try Vulkan, fall back" cannot
//!   be written at all. `crcbl-shell` puts that dependency in the crate that
//!   owns [`open`](crate::shell::open); the umbrella is the equivalent here,
//!   because it is already the one dependency a game names.
//!
//! The alternative — putting it in `crcbl-render` — is better and is where this
//! should move once that crate exists (P1.3). It is not a reason to make
//! `apps/sandbox` depend on `crcbl-vk` in the meantime, which is the thing the
//! rule actually forbids.
//!
//! # Selection is explicit, never a silent fallback
//!
//! [`open`] tries the auto-selectable backends in order and **does not fall
//! back to [`GpuBackend::Null`]**, for the same reason `crcbl-shell` refuses to
//! auto-select `HeadlessShell`: a game that silently rendered nothing because a
//! driver was missing would look like a black screen, and
//! [`GpuError::NoBackend`] names the actual problem. Null is reachable only by
//! asking for it.

use crcbl_hal::{HalError, Instance};

/// Which backend is behind the [`Instance`] seam.
///
/// For selection, logs and bug reports. Renderer *behaviour* must key off
/// [`DeviceCaps`](crcbl_hal::DeviceCaps) and
/// [`Features`](crcbl_hal::Features), never off this — the same rule
/// [`BackendKind`](crcbl_hal::BackendKind) and
/// [`ShellBackend`](crcbl_shell::ShellBackend) carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuBackend {
    /// `crcbl-vk` — Vulkan 1.3 through `ash`.
    Vulkan,
    /// `crcbl-hal`'s recording no-op backend. Renders nothing; never selected
    /// automatically.
    Null,
    /// `crcbl-wgpu` — wgpu, native or WebGPU (P5).
    Wgpu,
}

impl GpuBackend {
    /// The lowercase name used in [`BACKEND_ENV_VAR`], on the command line and
    /// in log lines.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vulkan => "vk",
            Self::Null => "null",
            Self::Wgpu => "wgpu",
        }
    }

    /// Parses a backend name.
    ///
    /// Accepts the spellings people type: `vk` and `vulkan` are the same thing,
    /// and so are `null` and `none`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "vk" | "vulkan" => Some(Self::Vulkan),
            "null" | "none" => Some(Self::Null),
            "wgpu" | "webgpu" => Some(Self::Wgpu),
            _ => None,
        }
    }
}

impl core::fmt::Display for GpuBackend {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The environment variable that overrides automatic backend selection.
pub const BACKEND_ENV_VAR: &str = "CRCBL_GPU";

/// Why no backend could be opened.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    /// Nothing auto-selectable could be opened.
    #[error("no GPU backend available (tried: {tried}); last error: {last}")]
    NoBackend {
        /// The backends that were tried, in order.
        tried: String,
        /// Why the last one failed.
        last: String,
    },
    /// A backend was asked for by name and this build does not have it.
    #[error("unknown GPU backend `{requested}` (available: {available})")]
    UnknownBackend {
        /// What was asked for.
        requested: String,
        /// What this build has.
        available: String,
    },
    /// A backend was asked for by name and failed to open.
    #[error("the {backend} backend could not open: {source}")]
    Backend {
        /// Which one.
        backend: GpuBackend,
        /// Its own error.
        #[source]
        source: HalError,
    },
}

/// One entry in the backend table.
struct Registration {
    backend: GpuBackend,
    /// Whether [`open`] may select this entry without being asked for it by
    /// name. `false` for [`GpuBackend::Null`] — see the module docs.
    auto: bool,
    open: fn() -> Result<Box<dyn Instance>, HalError>,
}

/// Every backend compiled into this build, in the order [`open`] tries them.
static REGISTRY: &[Registration] = &[
    Registration {
        backend: GpuBackend::Vulkan,
        auto: true,
        open: || {
            crcbl_vk::VkInstance::open()
                .map(|instance| Box::new(instance) as Box<dyn Instance>)
                .map_err(HalError::from)
        },
    },
    Registration {
        backend: GpuBackend::Wgpu,
        auto: false,
        open: || match crcbl_wgpu::create_native() {
            Some(instance) => Ok(Box::new(instance) as Box<dyn Instance>),
            None => Err(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Wgpu,
                what: "no wgpu adapter found",
            }),
        },
    },
    Registration {
        backend: GpuBackend::Null,
        auto: false,
        // Tier A so the null backend exercises the same code paths a real
        // device does — timeline semaphores, explicit acquire semaphores — and
        // a `--backend null` run is a meaningful rehearsal rather than a
        // different program.
        open: || Ok(Box::new(crcbl_hal::null::NullInstance::tier_a())),
    },
];

fn registry_names(entries: impl Iterator<Item = GpuBackend>) -> String {
    let names: Vec<&str> = entries.map(GpuBackend::as_str).collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

/// Opens the best available GPU backend for this process.
///
/// Reads [`BACKEND_ENV_VAR`]: when it names a backend, that backend is opened
/// and a failure is *not* fallen back from — someone who set the variable meant
/// it. Otherwise the auto-selectable backends are tried in order.
///
/// # Errors
///
/// [`GpuError::UnknownBackend`] for a name this build does not have,
/// [`GpuError::Backend`] when a named backend refused, or
/// [`GpuError::NoBackend`] when nothing auto-selectable worked.
pub fn open() -> Result<Box<dyn Instance>, GpuError> {
    match std::env::var(BACKEND_ENV_VAR) {
        Ok(value) if !value.trim().is_empty() => {
            let Some(requested) = GpuBackend::from_name(&value) else {
                return Err(GpuError::UnknownBackend {
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
/// # Errors
///
/// [`GpuError::UnknownBackend`] if this build has no such backend, or
/// [`GpuError::Backend`] if it failed to open.
pub fn open_backend(backend: GpuBackend) -> Result<Box<dyn Instance>, GpuError> {
    let entry = lookup(REGISTRY, backend)?;
    (entry.open)().map_err(|source| GpuError::Backend { backend, source })
}

/// Finds `backend` in `registry`, or names what the registry does have.
///
/// Split out and taking the registry as an argument so the "this build has no
/// such backend" arm is reachable from a test. Against [`REGISTRY`] it is
/// unreachable today — every [`GpuBackend`] variant is registered — but the
/// table is the thing that will grow `#[cfg]`s (there is no `crcbl-vk` on
/// wasm), and an error path that has never once been executed is not an error
/// path anyone should trust.
fn lookup(
    registry: &'static [Registration],
    backend: GpuBackend,
) -> Result<&'static Registration, GpuError> {
    registry
        .iter()
        .find(|entry| entry.backend == backend)
        .ok_or_else(|| GpuError::UnknownBackend {
            requested: backend.as_str().to_string(),
            available: registry_names(registry.iter().map(|entry| entry.backend)),
        })
}

/// Tries every auto-selectable backend in order.
fn open_auto() -> Result<Box<dyn Instance>, GpuError> {
    let mut tried = Vec::new();
    let mut last = "nothing was tried".to_string();
    for entry in REGISTRY.iter().filter(|entry| entry.auto) {
        tried.push(entry.backend);
        match (entry.open)() {
            Ok(instance) => {
                log::info!("opened the {} GPU backend", entry.backend);
                return Ok(instance);
            }
            // Normal on a machine with no driver for this backend, so a debug
            // line — but never silent, because "why did it pick that one?" is a
            // question someone will ask.
            Err(error) => {
                log::debug!("{} GPU backend unavailable: {error}", entry.backend);
                last = error.to_string();
            }
        }
    }
    Err(GpuError::NoBackend {
        tried: registry_names(tried.into_iter()),
        last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `GpuBackend` variant resolves in this build. The moment one is
    /// `#[cfg]`-ed out — there is no `crcbl-vk` on wasm — this fails and points
    /// at the arm that has to start being reachable.
    #[test]
    fn the_registry_covers_every_backend_this_build_claims() {
        for backend in [GpuBackend::Vulkan, GpuBackend::Wgpu, GpuBackend::Null] {
            assert!(
                lookup(REGISTRY, backend).is_ok(),
                "{backend} is a variant with no registration",
            );
        }
    }

    /// And the "no such backend" arm reports what *is* available rather than
    /// an empty message. Exercised against a registry that really is missing
    /// one, because the shipped table is not.
    #[test]
    fn an_absent_backend_names_the_ones_that_are_there() {
        static ONLY_NULL: &[Registration] = &[Registration {
            backend: GpuBackend::Null,
            auto: false,
            open: || Ok(Box::new(crcbl_hal::null::NullInstance::tier_a())),
        }];

        let Err(error) = lookup(ONLY_NULL, GpuBackend::Vulkan) else {
            panic!("vulkan is not in a null-only registry");
        };
        match error {
            GpuError::UnknownBackend {
                requested,
                available,
            } => {
                assert_eq!(requested, GpuBackend::Vulkan.as_str());
                assert_eq!(available, "null");
            }
            other => panic!("wrong error: {other}"),
        }
        assert!(lookup(ONLY_NULL, GpuBackend::Null).is_ok());
    }

    #[test]
    fn backend_names_round_trip_and_accept_what_people_type() {
        for backend in [GpuBackend::Vulkan, GpuBackend::Null, GpuBackend::Wgpu] {
            assert_eq!(GpuBackend::from_name(backend.as_str()), Some(backend));
            assert_eq!(backend.to_string(), backend.as_str());
        }
        assert_eq!(GpuBackend::from_name("vulkan"), Some(GpuBackend::Vulkan));
        assert_eq!(GpuBackend::from_name(" VK "), Some(GpuBackend::Vulkan));
        assert_eq!(GpuBackend::from_name("none"), Some(GpuBackend::Null));
        assert_eq!(GpuBackend::from_name("metal"), None);
    }

    /// The tripwire for every backend slice: adding a registration must be a
    /// deliberate edit here too. And the rule that matters — a game must never
    /// silently render nothing because a driver was missing.
    #[test]
    fn the_table_is_what_it_says_and_null_is_never_automatic() {
        let backends: Vec<GpuBackend> = REGISTRY.iter().map(|entry| entry.backend).collect();
        assert_eq!(
            backends,
            [GpuBackend::Vulkan, GpuBackend::Wgpu, GpuBackend::Null]
        );
        let null = REGISTRY
            .iter()
            .find(|entry| entry.backend == GpuBackend::Null)
            .expect("null is always compiled in");
        assert!(!null.auto, "a game must never silently render nothing");
    }

    #[test]
    fn opening_null_by_name_works_and_reports_itself() {
        let instance = open_backend(GpuBackend::Null).expect("null is registered");
        assert_eq!(instance.backend(), crcbl_hal::BackendKind::Null);
        assert!(!instance.adapters().is_empty());
    }

    /// Wgpu is registered but not auto-selectable; asking for it by name
    /// either opens it (when a GPU is present) or produces a descriptive
    /// error (when no adapter was found).
    #[test]
    fn wgpu_is_registered_and_reachable_by_name() {
        match open_backend(GpuBackend::Wgpu) {
            Ok(instance) => {
                assert_eq!(instance.backend(), crcbl_hal::BackendKind::Wgpu);
            }
            Err(GpuError::Backend { backend, source: _ }) => {
                assert_eq!(backend, GpuBackend::Wgpu);
                // Expected when no GPU/driver is present.
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    /// On a machine with a driver this genuinely opens Vulkan, which is correct
    /// and not something to assert against; the point is the failure path's
    /// message, which is what a developer with no driver actually sees.
    #[test]
    fn automatic_selection_reports_what_it_tried() {
        match open_auto() {
            Ok(instance) => assert_eq!(
                instance.backend(),
                crcbl_hal::BackendKind::Vulkan,
                "vulkan is the only auto-selectable backend today"
            ),
            Err(GpuError::NoBackend { tried, last }) => {
                assert_eq!(tried, "vk", "every attempt is named, in order");
                assert!(!last.is_empty(), "and the last failure explains itself");
            }
            Err(error) => panic!("unexpected error: {error}"),
        }
    }
}
