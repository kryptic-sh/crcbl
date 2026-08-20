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
//! `open` tries the auto-selectable backends in order and **does not fall
//! back to [`GpuBackend::Null`]**, for the same reason `crcbl-shell` refuses to
//! auto-select `HeadlessShell`: a game that silently rendered nothing because a
//! driver was missing would look like a black screen, and
//! [`GpuError::NoBackend`] names the actual problem. Null is reachable only by
//! asking for it.
//!
//! The same rule is why [`GpuBackend::WebGpu`] is **registered on native and
//! refuses there** rather than being absent. Leaving the name unregistered
//! would report it as unknown, which reads as a typo rather than as a backend
//! this host cannot have. So the entry always exists, and off `wasm32` it
//! refuses with `WEBGPU_NOT_IMPLEMENTED`: there is no browser there to encode
//! the command stream against, so a native `open` of `webgpu` says so rather
//! than hanging on a future nothing drains. (It briefly aliased onto the
//! since-deleted `crcbl-wgpu`, which made every `CRCBL_GPU=webgpu` run report
//! success about a different backend — that is the mistake this shape exists to
//! prevent, and it outlived the crate.)
//!
//! On `wasm32` it is the browser's one GPU backend, opened through
//! `crcbl_webgpu::WebGpuInstanceOpen`. There is no feature to turn that on: see
//! "the table is not the same on every target" below.
//!
//! # Opening is polled, because the web says so
//!
//! [`request_open`] returns a [`PendingInstance`] whose [`poll`](PendingInstance::poll)
//! yields `None` (ask again) or the instance — the same shape `crcbl-hal` uses
//! for [`request_device`](crcbl_hal::Instance::request_device) and for readback,
//! and for the same reason. `wgpu`'s adapter enumeration is
//! `GPUAdapter`-shaped: on the web it is a `Promise` and the main thread cannot
//! block on it, so the registry stores a *future factory* rather than a
//! function that returns an instance. `open` and `open_backend` are the
//! blocking wrappers over it, unchanged for every native caller.
//!
//! Fallback belongs to the pending object, not to the caller: a poll that
//! observes an auto-selectable backend failing moves on to the next entry and
//! reports `Pending`, so "try Vulkan, then wgpu" reads the same whether it is
//! being driven by a `while` loop at start-up or by a rAF callback.
//!
//! # The table is not the same on every target
//!
//! There is no Vulkan in a browser, so on `wasm32` the [`GpuBackend::Vulkan`]
//! entry is `#[cfg]`-ed out and `crcbl-vk` is not even a dependency — see this
//! crate's manifest. [`GpuBackend::Metal`] is the mirror image: `crcbl-mtl` has
//! no public items off macOS, so its entry exists only there, and
//! [`GpuBackend::Dx12`] is the same shape on Windows. The `#[cfg]` is on
//! the *element*, exactly as `crcbl_shell::backend`'s table gates Wayland and
//! X11 to Linux, so nothing else in this file mentions a target and the walk in
//! [`PendingInstance::poll`] needs no conditional compilation of its own.
//!
//! What that leaves is one automatic backend per target with no feature
//! deciding it: **[`GpuBackend::WebGpu`] on `wasm32`**, Metal on macOS, Vulkan
//! on the rest of native. The browser needs an automatic backend or an `open()`
//! there could never succeed, and it now has exactly one candidate. There used
//! to be a `webgpu` feature here choosing between `wgpu` and `webgpu` in the
//! browser; it went when the choice did.
//!
//! The other is **Metal, and only Metal, on macOS**. `docs/plan/09-backends-
//! metal-dx12.md`'s 2026-08-05 correction settles the platform question: Apple
//! platforms are Metal only, `crcbl-vk` is not expected to reach a device there,
//! and a Mac without MoltenVK installed has no `libvulkan.dylib` to `dlopen` at
//! all. So the Vulkan entry is registered on macOS but **not automatic**: an
//! `open()` there tries Metal and stops, while `CRCBL_GPU=vk` still reaches the
//! Vulkan backend by name for whoever has a loader and means it. Elsewhere on
//! native the order is unchanged — Vulkan, and nothing else automatically.
//!
//! **Windows is the case where two backends are registered and neither
//! displaces the other.** `crcbl-dx12`'s own crate docs say D3D12 is not a
//! replacement for `crcbl-vk` there — it is the same engine reaching a different
//! loader — so the D3D12 entry is registered and **not** automatic, and
//! `CRCBL_GPU=dx12` is the only thing that selects it. Making it automatic would
//! silently move every existing Windows run onto a backend whose feature set is
//! the smaller of the two.
//!
//! The blocking wrappers `open` and `open_backend` do not exist on `wasm32`
//! either, for the reason `Instance::create_device` does not: the browser's
//! main thread may not block, so reaching for one there is a *compile* error
//! pointing at [`request_open`] rather than a hang on the first frame.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

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
    /// `crcbl-mtl` — Metal through `objc2-metal`, and the only path to a GPU on
    /// macOS (P14).
    Metal,
    /// `crcbl-dx12` — Direct3D 12 through `windows-rs`, Windows' second path to
    /// a GPU beside [`Vulkan`](Self::Vulkan) (P14).
    Dx12,
    /// `crcbl-hal`'s recording no-op backend. Renders nothing; never selected
    /// automatically.
    Null,
    /// `crcbl-webgpu` — WebGPU reached straight from wasm, and the browser's
    /// only GPU backend.
    ///
    /// Automatic on `wasm32`, where it opens a real device through the command
    /// stream. Off `wasm32` there is no browser to encode that stream against,
    /// so the entry is registered — `CRCBL_GPU=webgpu` must not read as a typo —
    /// and refuses with [`GpuError::Backend`] rather than quietly succeeding.
    WebGpu,
}

impl GpuBackend {
    /// The lowercase name used in [`BACKEND_ENV_VAR`], on the command line and
    /// in log lines.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vulkan => "vk",
            Self::Metal => "mtl",
            Self::Dx12 => "dx12",
            Self::Null => "null",
            Self::WebGpu => "webgpu",
        }
    }

    /// Parses a backend name.
    ///
    /// Accepts the spellings people type: `vk` and `vulkan` are the same thing,
    /// and so are `mtl` and `metal`, `dx12` and `d3d12`, and `null` and `none`.
    ///
    /// `wgpu` is **not** accepted, and that is deliberate rather than an
    /// omission. It named `crcbl-wgpu`, which was deleted once the browser
    /// backend replaced it; answering `None` makes `CRCBL_GPU=wgpu` report an
    /// unknown backend, which is what it now is. Aliasing it onto
    /// [`WebGpu`](Self::WebGpu) would repeat the mistake this function was
    /// changed to stop — a name quietly opening a different backend and
    /// reporting success.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "vk" | "vulkan" => Some(Self::Vulkan),
            "mtl" | "metal" => Some(Self::Metal),
            "dx12" | "d3d12" => Some(Self::Dx12),
            "null" | "none" => Some(Self::Null),
            "webgpu" => Some(Self::WebGpu),
            _ => None,
        }
    }

    /// Every backend, in the order a user is offered them.
    ///
    /// Held complete by `position` below, whose `match` is exhaustive: a new
    /// variant fails to compile there until it is placed, and
    /// `every_backend_is_in_all_at_its_own_position` is what ties the two
    /// together. An array on its own would not do it — adding a variant beside
    /// a hand-written list compiles, which is exactly how the name lists this
    /// exists to feed went stale.
    pub const ALL: [Self; 5] = [
        Self::Vulkan,
        Self::Metal,
        Self::Dx12,
        Self::Null,
        Self::WebGpu,
    ];

    /// Where `self` sits in [`ALL`](Self::ALL).
    ///
    /// A completeness device rather than an identity — nothing should depend on
    /// a backend's ordinal — so it is private and compiled only for the test
    /// that reads it. **That narrows where the compile failure lands and does
    /// not remove it:** a seventh variant breaks this `match` under
    /// `cargo test` and under `cargo clippy --all-targets`, which are two of
    /// the three gates every change to this workspace goes through and both run
    /// in CI. Left ungated it is dead code in a normal build, and `-D warnings`
    /// is the other thing every gate here runs.
    #[cfg(test)]
    const fn position(self) -> usize {
        match self {
            Self::Vulkan => 0,
            Self::Metal => 1,
            Self::Dx12 => 2,
            Self::Null => 3,
            Self::WebGpu => 4,
        }
    }

    /// The canonical names as an English list, for the message shown when a
    /// name is not one of them: `vk`, `mtl`, `dx12`, `null`, `wgpu` or
    /// `webgpu`.
    ///
    /// Built from [`ALL`](Self::ALL) rather than written out, because it was
    /// written out in two places and one of them named four backends of six.
    /// The aliases [`from_name`](Self::from_name) also accepts are deliberately
    /// absent: a rejection offering `vulkan` beside `vk` reads as more backends
    /// rather than as one more spelling, and
    /// [`COMMON_OPTIONS_HELP`](crate::args::COMMON_OPTIONS_HELP) is where every
    /// spelling is listed.
    #[must_use]
    pub fn name_list() -> String {
        let names: Vec<String> = Self::ALL
            .iter()
            .map(|backend| format!("`{}`", backend.as_str()))
            .collect();
        match names.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => String::new(),
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

/// A backend being opened.
///
/// Boxed rather than a concrete future because the registry is one static table
/// of function pointers over three unrelated backends. Not `Send`: an instance
/// is opened once, on the thread that will drive the frame loop, and on the web
/// there is only one thread anyway.
type InstanceFuture = Pin<Box<dyn Future<Output = Result<Box<dyn Instance>, HalError>>>>;

/// Wraps an already-decided result as a future that completes on its first poll.
///
/// What a backend whose open really is synchronous returns; the seam's
/// `crcbl-vk` half says the same thing about `vkCreateDevice`.
fn ready(result: Result<Box<dyn Instance>, HalError>) -> InstanceFuture {
    Box::pin(core::future::ready(result))
}

/// One entry in the backend table.
struct Registration {
    backend: GpuBackend,
    /// Whether [`open`] may select this entry without being asked for it by
    /// name. `false` for [`GpuBackend::Null`] — see the module docs.
    auto: bool,
    /// Starts opening this backend. Returning a future rather than an instance
    /// is what makes the web path expressible; two of the three complete on
    /// their first poll.
    open: fn() -> InstanceFuture,
}

/// Why [`GpuBackend::WebGpu`] refuses when it is not the active backend, worded
/// so a log reader can tell "not turned on here" from a broken driver.
///
/// It is what the entry returns off `wasm32`: there is no browser there to
/// encode the command stream against, so `webgpu` refuses by name. On `wasm32`
/// the backend is openable and this string is never produced, which is why it is
/// `#[cfg]`-ed to the targets that use it — an ungated `const` would be dead
/// code in every browser build. Named rather than written into the table, so the
/// registry entry and the test that proves the refusal point at one string
/// instead of two that can drift.
#[cfg(not(target_arch = "wasm32"))]
const WEBGPU_NOT_IMPLEMENTED: &str = "the crcbl-webgpu backend is not active in this build — it \
    reaches a device only on wasm32, where it is the automatic backend";

/// Every backend compiled into this build, in the order [`request_open`] tries
/// them.
static REGISTRY: &[Registration] = &[
    // First on the platform that has it, because it is the only thing that can
    // reach a GPU there. `crcbl-mtl` has no public items off macOS — the whole
    // backend is `#[cfg(target_os = "macos")]` — so the entry is gated the same
    // way, on the element.
    //
    // `MetalInstance::open` returns `Option`, not `Result`: the only failure it
    // has is "the system reports no Metal device", which is the case a registry
    // falls through on. It is turned into the `Unsupported` the walk needs here
    // rather than in the backend, exactly as the wgpu entry below does with its
    // own `Option`.
    #[cfg(target_os = "macos")]
    Registration {
        backend: GpuBackend::Metal,
        auto: true,
        open: || {
            ready(
                crcbl_mtl::MetalInstance::open()
                    .map(|instance| Box::new(instance) as Box<dyn Instance>)
                    .ok_or(HalError::Unsupported {
                        backend: crcbl_hal::BackendKind::Metal,
                        what: "no Metal device on this system",
                    }),
            )
        },
    },
    // `#[cfg]` on the element, not on the whole table — the shape
    // `crcbl_shell::backend`'s registry uses for Wayland and X11. There is no
    // Vulkan in a browser and `crcbl-vk` is not a dependency there at all, so
    // on wasm32 this entry simply is not present.
    #[cfg(not(target_arch = "wasm32"))]
    Registration {
        backend: GpuBackend::Vulkan,
        // Registered on macOS and not automatic there: Apple platforms are
        // Metal only per `docs/plan/09-backends-metal-dx12.md`, and a Mac
        // without MoltenVK has no `libvulkan.dylib` for `ash` to `dlopen`, so
        // auto-selecting it would spend every start-up failing a load before
        // reaching the backend that works. `CRCBL_GPU=vk` still gets it.
        auto: !cfg!(target_os = "macos"),
        open: || {
            ready(
                crcbl_vk::VkInstance::open()
                    .map(|instance| Box::new(instance) as Box<dyn Instance>)
                    .map_err(HalError::from),
            )
        },
    },
    // Windows' second path to a GPU, gated on the element exactly as Metal is:
    // `crcbl-dx12` is `#[cfg(target_os = "windows")]` from its crate root down
    // and has no public items anywhere else.
    //
    // `Dx12Instance::open` returns `Option` for the same reason
    // `MetalInstance::open` does — "DXGI started nothing this can open" is the
    // case a registry falls through on — and it is turned into `Unsupported`
    // here rather than in the backend, for the same reason.
    #[cfg(target_os = "windows")]
    Registration {
        backend: GpuBackend::Dx12,
        // Registered and never automatic. Windows already reaches a GPU through
        // `crcbl-vk`, and `crcbl-dx12`'s crate docs say D3D12 is not a
        // replacement for it: auto-selecting it would move every Windows run
        // onto the backend with the smaller feature set without anyone asking.
        auto: false,
        open: || {
            ready(
                crcbl_dx12::Dx12Instance::open()
                    .map(|instance| Box::new(instance) as Box<dyn Instance>)
                    .ok_or(HalError::Unsupported {
                        backend: crcbl_hal::BackendKind::Dx12,
                        what: "no D3D12 adapter on this system",
                    }),
            )
        },
    },
    // The browser's backend, and registered everywhere so the *name* always
    // resolves. Off `wasm32` it refuses: `CRCBL_GPU=webgpu` on a desktop has to
    // say "not here" rather than read as a typo, and the two failure modes that
    // would make it unreadable are a silent fall-through onto wgpu and a panic —
    // this entry is neither.
    //
    // On `wasm32` its `open` starts a real `crcbl_webgpu::WebGpuInstanceOpen`,
    // the future the browser's frame loop drives to a device; on native it
    // returns [`WEBGPU_NOT_IMPLEMENTED`]. The `cfg` on the arms is what keeps
    // the native build from naming `crcbl_webgpu` at all (it is a `wasm32`-only
    // dependency) and from parking on a future no executor here would poll.
    //
    // Auto exactly where it can open: `wasm32`, where it is the only GPU backend
    // in the table. An automatic entry that always refuses would add a failed
    // attempt in front of every native start-up.
    Registration {
        backend: GpuBackend::WebGpu,
        auto: cfg!(target_arch = "wasm32"),
        open: || {
            #[cfg(target_arch = "wasm32")]
            {
                Box::pin(async {
                    crcbl_webgpu::WebGpuInstanceOpen::start()
                        .await
                        .map(|instance| Box::new(instance) as Box<dyn Instance>)
                })
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                ready(Err(HalError::Unsupported {
                    backend: crcbl_hal::BackendKind::WebGpu,
                    what: WEBGPU_NOT_IMPLEMENTED,
                }))
            }
        },
    },
    Registration {
        backend: GpuBackend::Null,
        auto: false,
        // Tier A so the null backend exercises the same code paths a real
        // device does — timeline semaphores, explicit acquire semaphores — and
        // a `--backend null` run is a meaningful rehearsal rather than a
        // different program.
        open: || ready(Ok(Box::new(crcbl_hal::null::NullInstance::gpu_driven()))),
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

/// A backend open in flight.
///
/// The non-blocking half of `open` / `open_backend`, for a caller that must
/// not block — the browser's rAF loop. Poll it until it hands over the instance;
/// see the module docs for why it exists in this shape.
pub struct PendingInstance {
    /// The entry being tried, and its future. `None` once the instance has been
    /// handed over or every candidate has failed.
    current: Option<(GpuBackend, InstanceFuture)>,
    /// Auto-selectable entries not tried yet. Empty for a by-name open, whose
    /// failure is final.
    remaining: &'static [Registration],
    /// Whether a failure falls through to `remaining` rather than being fatal.
    fallback: bool,
    tried: Vec<GpuBackend>,
    last: String,
}

impl core::fmt::Debug for PendingInstance {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PendingInstance")
            .field(
                "trying",
                &self.current.as_ref().map(|(backend, _)| *backend),
            )
            .field("tried", &self.tried)
            .finish_non_exhaustive()
    }
}

impl PendingInstance {
    /// Advances the open. `Ok(None)` means "not yet, poll again".
    ///
    /// A no-op waker, for the reason `crcbl-wgpu` gives for the same choice: the
    /// browser's event loop resolves the promise on its own and the caller's
    /// loop is the executor, so there is nobody for a waker to wake.
    ///
    /// # Errors
    ///
    /// [`GpuError::Backend`] when a backend asked for **by name** refused, or
    /// [`GpuError::NoBackend`] when every auto-selectable one did. Polling after
    /// the instance was handed over reports [`GpuError::NoBackend`] naming that.
    pub fn poll(&mut self) -> Result<Option<Box<dyn Instance>>, GpuError> {
        loop {
            let Some((backend, future)) = self.current.as_mut() else {
                return Err(GpuError::NoBackend {
                    tried: registry_names(self.tried.iter().copied()),
                    last: self.last.clone(),
                });
            };
            let backend = *backend;
            let waker = Waker::noop();
            match future.as_mut().poll(&mut Context::from_waker(waker)) {
                Poll::Pending => return Ok(None),
                Poll::Ready(Ok(instance)) => {
                    self.current = None;
                    log::info!("opened the {backend} GPU backend");
                    return Ok(Some(instance));
                }
                Poll::Ready(Err(error)) => {
                    if !self.fallback {
                        self.current = None;
                        self.last = error.to_string();
                        return Err(GpuError::Backend {
                            backend,
                            source: error,
                        });
                    }
                    // Normal on a machine with no driver for this backend, so a
                    // debug line — but never silent, because "why did it pick
                    // that one?" is a question someone will ask.
                    log::debug!("{backend} GPU backend unavailable: {error}");
                    self.last = error.to_string();
                    self.advance();
                }
            }
        }
    }

    /// Moves to the next auto-selectable candidate, if there is one.
    fn advance(&mut self) {
        while let Some((entry, rest)) = self.remaining.split_first() {
            self.remaining = rest;
            if !entry.auto {
                continue;
            }
            self.tried.push(entry.backend);
            self.current = Some((entry.backend, (entry.open)()));
            return;
        }
        self.current = None;
    }

    /// Polls until the instance arrives. The blocking half of [`open`].
    ///
    /// Absent on `wasm32`: the browser's main thread may not block, and the
    /// only executor there is the rAF loop that would be spinning in here.
    #[cfg(not(target_arch = "wasm32"))]
    fn block(mut self) -> Result<Box<dyn Instance>, GpuError> {
        loop {
            if let Some(instance) = self.poll()? {
                return Ok(instance);
            }
            std::thread::yield_now();
        }
    }
}

/// Starts opening the best available GPU backend, without blocking.
///
/// The non-blocking form of `open`, with the same [`BACKEND_ENV_VAR`]
/// handling. Drive the returned [`PendingInstance`] until it yields.
///
/// # Errors
///
/// [`GpuError::UnknownBackend`] for a name this build does not have. Everything
/// else is reported from [`PendingInstance::poll`], because it is not known yet.
pub fn request_open() -> Result<PendingInstance, GpuError> {
    match std::env::var(BACKEND_ENV_VAR) {
        Ok(value) if !value.trim().is_empty() => {
            let Some(requested) = GpuBackend::from_name(&value) else {
                return Err(GpuError::UnknownBackend {
                    requested: value,
                    available: registry_names(REGISTRY.iter().map(|entry| entry.backend)),
                });
            };
            request_open_backend(requested)
        }
        _ => Ok(request_open_auto(REGISTRY)),
    }
}

/// Starts opening a specific backend, without blocking and ignoring
/// [`BACKEND_ENV_VAR`].
///
/// A failure is **not** fallen back from: someone who named a backend meant it.
///
/// # Errors
///
/// [`GpuError::UnknownBackend`] if this build has no such backend.
pub fn request_open_backend(backend: GpuBackend) -> Result<PendingInstance, GpuError> {
    let entry = lookup(REGISTRY, backend)?;
    Ok(PendingInstance {
        current: Some((backend, (entry.open)())),
        remaining: &[],
        fallback: false,
        tried: vec![backend],
        last: "nothing was tried".to_string(),
    })
}

/// The auto-selection pending object over an arbitrary registry, so the
/// "nothing auto-selectable worked" arm is reachable from a test.
fn request_open_auto(registry: &'static [Registration]) -> PendingInstance {
    let mut pending = PendingInstance {
        current: None,
        remaining: registry,
        fallback: true,
        tried: Vec::new(),
        last: "nothing was tried".to_string(),
    };
    pending.advance();
    pending
}

/// Opens the best available GPU backend for this process.
///
/// Reads [`BACKEND_ENV_VAR`]: when it names a backend, that backend is opened
/// and a failure is *not* fallen back from — someone who set the variable meant
/// it. Otherwise the auto-selectable backends are tried in order.
///
/// Blocks until a backend answers. **Absent on `wasm32`**, where the main
/// thread may not block: browser code polls [`request_open`] instead and gets a
/// compile error rather than a hung first frame if it forgets. Same rule, same
/// reason as [`Instance::create_device`].
///
/// # Errors
///
/// [`GpuError::UnknownBackend`] for a name this build does not have,
/// [`GpuError::Backend`] when a named backend refused, or
/// [`GpuError::NoBackend`] when nothing auto-selectable worked.
#[cfg(not(target_arch = "wasm32"))]
pub fn open() -> Result<Box<dyn Instance>, GpuError> {
    request_open()?.block()
}

/// Opens a specific backend, ignoring [`BACKEND_ENV_VAR`].
///
/// Blocks, so like [`open`] it is **absent on `wasm32`**; the polled form is
/// [`request_open_backend`].
///
/// # Errors
///
/// [`GpuError::UnknownBackend`] if this build has no such backend, or
/// [`GpuError::Backend`] if it failed to open.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_backend(backend: GpuBackend) -> Result<Box<dyn Instance>, GpuError> {
    request_open_backend(backend)?.block()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The backends this target really has, in registry order. One list, used
    /// by every test that asserts about the table, so a new registration is a
    /// single deliberate edit here.
    const REGISTERED: &[GpuBackend] = &[
        #[cfg(target_os = "macos")]
        GpuBackend::Metal,
        #[cfg(not(target_arch = "wasm32"))]
        GpuBackend::Vulkan,
        #[cfg(target_os = "windows")]
        GpuBackend::Dx12,
        GpuBackend::WebGpu,
        GpuBackend::Null,
    ];

    /// Every [`GpuBackend`] variant, so the tests that walk "the ones this
    /// target does *not* register" walk all of them. One list rather than a
    /// literal per test, because a variant missing from one of them is a case
    /// silently never checked.
    const EVERY_BACKEND: &[GpuBackend] = &[
        GpuBackend::Vulkan,
        GpuBackend::Metal,
        GpuBackend::Dx12,
        GpuBackend::WebGpu,
        GpuBackend::Null,
    ];

    /// Every backend this build claims resolves. `GpuBackend::Vulkan` is a
    /// variant on every target — it is what `CRCBL_GPU=vk` parses to — but it
    /// is *registered* only where `crcbl-vk` is a dependency, so on wasm32 the
    /// honest answer is [`GpuError::UnknownBackend`] and that is asserted below
    /// rather than left untested.
    #[test]
    fn the_registry_covers_every_backend_this_build_claims() {
        for backend in REGISTERED {
            assert!(
                lookup(REGISTRY, *backend).is_ok(),
                "{backend} is a variant with no registration",
            );
        }
    }

    /// The other half: a variant that this target does *not* register is
    /// rejected by name, with a message naming what it does have.
    ///
    /// **This is the arm that carries the `CRCBL_GPU=dx12` promise off
    /// Windows**, and it is a real arm on every development machine: `Dx12` is
    /// a variant everywhere, because it is what `CRCBL_GPU=dx12` parses to, and
    /// it is *registered* only where `crcbl-dx12` is a dependency. So on Linux
    /// and macOS the honest answer is [`GpuError::UnknownBackend`] naming what
    /// this build does have, and that is what is asserted rather than left to a
    /// Windows runner.
    #[test]
    fn a_variant_this_target_lacks_is_rejected_by_name() {
        let mut checked = 0;
        for backend in EVERY_BACKEND.iter().copied() {
            if REGISTERED.contains(&backend) {
                continue;
            }
            checked += 1;
            let Err(GpuError::UnknownBackend {
                requested,
                available,
            }) = lookup(REGISTRY, backend)
            else {
                panic!("{backend} is not in this build's registry but resolved anyway");
            };
            // The message has to be actionable on its own: what was asked for,
            // and what this build would have accepted instead. An `available`
            // that merely existed would pass a length check and tell a
            // developer nothing.
            assert_eq!(requested, backend.as_str());
            for present in REGISTERED {
                assert!(
                    available.contains(present.as_str()),
                    "asking for {backend} listed `{available}`, which omits {present}"
                );
            }
        }
        // Every native target lacks at least one variant — wasm32 lacks Vulkan,
        // Linux lacks Metal and Dx12 — so a run that checked nothing means the
        // loop's own filter matched everything and the assertions above never
        // executed.
        assert!(checked > 0, "no unregistered variant was reached");
    }

    /// And the "no such backend" arm reports what *is* available rather than
    /// an empty message. Exercised against a registry that really is missing
    /// one, because the shipped table is not.
    #[test]
    fn an_absent_backend_names_the_ones_that_are_there() {
        static ONLY_NULL: &[Registration] = &[Registration {
            backend: GpuBackend::Null,
            auto: false,
            open: || ready(Ok(Box::new(crcbl_hal::null::NullInstance::gpu_driven()))),
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
        for backend in EVERY_BACKEND.iter().copied() {
            assert_eq!(GpuBackend::from_name(backend.as_str()), Some(backend));
            assert_eq!(backend.to_string(), backend.as_str());
        }
        assert_eq!(GpuBackend::from_name("vulkan"), Some(GpuBackend::Vulkan));
        assert_eq!(GpuBackend::from_name(" VK "), Some(GpuBackend::Vulkan));
        assert_eq!(GpuBackend::from_name("metal"), Some(GpuBackend::Metal));
        assert_eq!(GpuBackend::from_name("d3d12"), Some(GpuBackend::Dx12));
        assert_eq!(GpuBackend::from_name(" DX12 "), Some(GpuBackend::Dx12));
        assert_eq!(GpuBackend::from_name("none"), Some(GpuBackend::Null));
        assert_eq!(GpuBackend::from_name("opengl"), None);
        // **`wgpu` is not a spelling of `webgpu`, and now it is not a backend
        // either.** It named `crcbl-wgpu`, deleted on 2026-08-21 once the
        // browser backend replaced it. `from_name` answering `None` is what
        // makes `CRCBL_GPU=wgpu` report an unknown backend rather than quietly
        // opening a different one — the mistake the alias made when `webgpu`
        // pointed at `Wgpu`, and the reason this assertion is kept after the
        // variant is gone rather than deleted with it.
        assert_eq!(GpuBackend::from_name("webgpu"), Some(GpuBackend::WebGpu));
        assert_eq!(GpuBackend::from_name(" WebGPU "), Some(GpuBackend::WebGpu));
        assert_eq!(GpuBackend::WebGpu.to_string(), "webgpu");
        assert_eq!(
            GpuBackend::from_name("wgpu"),
            None,
            "`wgpu` named a backend that no longer exists; aliasing it onto \
             another one is what this test exists to stop"
        );
        // Every name is distinct, so `CRCBL_GPU` can never name two backends —
        // which an `as_str` arm copied from its neighbour would make possible
        // while every round-trip above still passed.
        let names: std::collections::HashSet<&str> = EVERY_BACKEND
            .iter()
            .map(|backend| backend.as_str())
            .collect();
        assert_eq!(
            names.len(),
            EVERY_BACKEND.len(),
            "two variants share a name"
        );
    }

    /// The tripwire for every backend slice: adding a registration must be a
    /// deliberate edit here too. And the rule that matters — a game must never
    /// silently render nothing because a driver was missing.
    #[test]
    fn the_table_is_what_it_says_and_null_is_never_automatic() {
        let backends: Vec<GpuBackend> = REGISTRY.iter().map(|entry| entry.backend).collect();
        assert_eq!(backends, REGISTERED);
        let null = REGISTRY
            .iter()
            .find(|entry| entry.backend == GpuBackend::Null)
            .expect("null is always compiled in");
        assert!(!null.auto, "a game must never silently render nothing");
    }

    /// Auto-selection differs by target *on purpose*, and there is exactly one
    /// automatic entry everywhere: Metal on macOS, because Apple platforms are
    /// Metal only; Vulkan on the rest of native; and `webgpu` in a browser,
    /// which is the only GPU backend a browser build links at all.
    /// Asserting it here is what stops a later edit from quietly making wgpu
    /// automatic on native — which would change which GPU path every existing
    /// game takes — or from leaving Vulkan
    /// automatic on a Mac, where it means a failed `dlopen` before every
    /// successful start-up.
    ///
    /// Windows is the target where this says the most, because it is the one
    /// with two native backends registered: the answer there is still Vulkan
    /// alone, so adding D3D12 changed which backends can be *asked for* and not
    /// which one an unconfigured game gets.
    #[test]
    fn exactly_one_backend_is_auto_selectable_and_it_depends_on_the_target() {
        let auto: Vec<GpuBackend> = REGISTRY
            .iter()
            .filter(|entry| entry.auto)
            .map(|entry| entry.backend)
            .collect();
        #[cfg(target_os = "macos")]
        assert_eq!(auto, [GpuBackend::Metal]);
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "macos")))]
        assert_eq!(auto, [GpuBackend::Vulkan]);
        // A browser has one GPU backend and it is `crcbl-webgpu`. No feature
        // decides this any more: `crcbl-wgpu` is not a dependency on wasm32, so
        // there is no second candidate a build flag could reach.
        #[cfg(target_arch = "wasm32")]
        assert_eq!(auto, [GpuBackend::WebGpu]);
    }

    /// Blocking, so native-only: `open_backend` and `PendingInstance::block`
    /// do not exist on wasm32.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn opening_null_by_name_works_and_reports_itself() {
        let instance = open_backend(GpuBackend::Null).expect("null is registered");
        assert_eq!(instance.backend(), crcbl_hal::BackendKind::Null);
        assert!(!instance.adapters().is_empty());
    }

    /// **`webgpu` is refused loudly off `wasm32`, and is never another backend
    /// standing in for it.**
    ///
    /// The transition this variant exists for only means anything if a run that
    /// asked for `webgpu` and got a picture really was `crcbl-webgpu` drawing
    /// it. The two ways that could go wrong are a fall-through onto another
    /// backend and a panic; this asserts it is neither, and that the message
    /// names the reason rather than reading like an absent driver.
    ///
    /// The name is resolved through [`GpuBackend::from_name`] rather than
    /// written as the variant, because what `CRCBL_GPU=webgpu` rests on is the
    /// *string* reaching this variant — and it used to reach the
    /// since-deleted `crcbl-wgpu`.
    ///
    /// Native only. On `wasm32` the backend opens — the browser is the one host
    /// where the command stream has something to encode against — so there is no
    /// refusal to observe there; the browser gate is what exercises the wasm
    /// path. On every native target `open_backend(WebGpu)` returns
    /// `WEBGPU_NOT_IMPLEMENTED`, which is what this reads.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn webgpu_is_registered_and_refuses_by_name_without_falling_back() {
        let requested = GpuBackend::from_name("webgpu").expect("the name CRCBL_GPU=webgpu carries");
        assert_eq!(requested, GpuBackend::WebGpu);

        let mut pending = request_open_backend(requested)
            .expect("webgpu is registered, so the name resolves rather than reading as a typo");
        let mut polls = 0;
        let error = loop {
            polls += 1;
            assert!(polls < 64, "a refusal must not poll forever");
            match pending.poll() {
                Ok(Some(_)) => panic!("webgpu handed over an instance it does not have"),
                Ok(None) => continue,
                Err(error) => break error,
            }
        };

        let text = error.to_string();
        let GpuError::Backend { backend, source } = &error else {
            panic!(
                "a named backend that refuses is final, so this is `Backend`, never the \
                 `NoBackend` of a walk that fell through: {error}"
            );
        };
        assert_eq!(
            *backend,
            GpuBackend::WebGpu,
            "the refusal must name webgpu, not the backend it is replacing"
        );
        // Typed, not merely a string: the arm carries `BackendKind::WebGpu`, so
        // a caller can tell this apart from a driver that would not load.
        assert!(
            matches!(
                source,
                HalError::Unsupported {
                    backend: crcbl_hal::BackendKind::WebGpu,
                    ..
                }
            ),
            "{source}"
        );
        assert!(
            text.contains(WEBGPU_NOT_IMPLEMENTED),
            "the log line has to say this is a not-yet: {text}"
        );
    }

    /// **`CRCBL_GPU=dx12` reaches a real D3D12 instance on Windows**, which is
    /// the half of the registration that only a Windows machine can settle.
    ///
    /// The name is resolved through [`GpuBackend::from_name`] rather than
    /// written as the variant, because "the variant is registered" and "the
    /// string a harness exports parses to it" are two claims and only the
    /// second is what `CRCBL_GPU=dx12` depends on.
    ///
    /// WARP ships in Windows, so `Ok` is the expected answer; the `Backend` arm
    /// is kept for a session where DXGI itself will not start — a headless
    /// service account, say — and it still asserts that the *registry* reached
    /// D3D12 rather than something else. [`GpuError::UnknownBackend`] is not
    /// tolerated by either arm: that is the answer this test exists to
    /// distinguish from a working one.
    #[cfg(target_os = "windows")]
    #[test]
    fn dx12_is_registered_on_windows_and_reachable_by_the_name_crcbl_gpu_takes() {
        let requested = GpuBackend::from_name("dx12").expect("the name CRCBL_GPU=dx12 carries");
        assert_eq!(requested, GpuBackend::Dx12);
        match open_backend(requested) {
            Ok(instance) => {
                assert_eq!(instance.backend(), crcbl_hal::BackendKind::Dx12);
                assert!(
                    !instance.adapters().is_empty(),
                    "a Windows machine reports at least WARP"
                );
            }
            Err(GpuError::Backend { backend, source }) => {
                assert_eq!(backend, GpuBackend::Dx12);
                println!("crcbl: the D3D12 backend is registered and would not open: {source}");
            }
            Err(other) => panic!("dx12 is registered on Windows: {other}"),
        }
    }

    /// On a machine with a driver this genuinely opens the platform's backend,
    /// which is correct and not something to assert against; the point is the
    /// failure path's message, which is what a developer with no driver actually
    /// sees. Which backend that is comes from [`REGISTERED`]'s first entry
    /// rather than a second hard-coded list, so a target added above is covered
    /// here without a matching edit that could be forgotten.
    /// Blocking, so native-only: `open_backend` and `PendingInstance::block`
    /// do not exist on wasm32.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn automatic_selection_reports_what_it_tried() {
        let expected = REGISTRY
            .iter()
            .find(|entry| entry.auto)
            .expect("every native target has one automatic backend")
            .backend;
        // Exhaustive on purpose: the two enums are separate types — one is the
        // registry's, one is the seam's — and a new backend must be taught the
        // correspondence rather than defaulted into one.
        let expected_kind = match expected {
            GpuBackend::Vulkan => crcbl_hal::BackendKind::Vulkan,
            GpuBackend::Metal => crcbl_hal::BackendKind::Metal,
            GpuBackend::Dx12 => crcbl_hal::BackendKind::Dx12,
            GpuBackend::WebGpu => crcbl_hal::BackendKind::WebGpu,
            GpuBackend::Null => crcbl_hal::BackendKind::Null,
        };
        match request_open_auto(REGISTRY).block() {
            Ok(instance) => assert_eq!(
                instance.backend(),
                expected_kind,
                "the auto-selectable backend is the one that opened"
            ),
            Err(GpuError::NoBackend { tried, last }) => {
                assert_eq!(tried, expected.as_str(), "every attempt is named, in order");
                assert!(!last.is_empty(), "and the last failure explains itself");
            }
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    /// The polled form is the one the browser will use, and it must reach the
    /// same instance the blocking one does — through a real `Pending` when the
    /// backend's future is not ready on its first poll.
    #[test]
    fn the_polled_open_reaches_the_same_instance_as_the_blocking_one() {
        let mut pending = request_open_backend(GpuBackend::Null).expect("null is registered");
        let mut polls = 0;
        let instance = loop {
            polls += 1;
            assert!(polls < 64, "the null backend must not poll forever");
            if let Some(instance) = pending.poll().expect("null always opens") {
                break instance;
            }
        };
        assert_eq!(instance.backend(), crcbl_hal::BackendKind::Null);

        // And a second poll is the caller bug it is, not a second instance.
        let error = pending.poll().expect_err("the instance was already taken");
        assert!(matches!(error, GpuError::NoBackend { .. }), "{error}");
    }

    /// Auto-selection over a registry whose only auto entry always fails: the
    /// fallback walk lives in `PendingInstance::poll`, so this is the arm that
    /// proves it reports every attempt rather than the first.
    /// Blocking, so native-only: `open_backend` and `PendingInstance::block`
    /// do not exist on wasm32.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_polled_auto_open_walks_the_table_and_then_gives_up() {
        static ALWAYS_FAILS: &[Registration] = &[
            Registration {
                backend: GpuBackend::Vulkan,
                auto: true,
                open: || {
                    ready(Err(HalError::Unsupported {
                        backend: crcbl_hal::BackendKind::Vulkan,
                        what: "no driver in this test",
                    }))
                },
            },
            Registration {
                backend: GpuBackend::WebGpu,
                auto: true,
                open: || {
                    ready(Err(HalError::Unsupported {
                        backend: crcbl_hal::BackendKind::WebGpu,
                        what: "no adapter in this test",
                    }))
                },
            },
            // Not auto: it would succeed, and it must never be reached.
            Registration {
                backend: GpuBackend::Null,
                auto: false,
                open: || ready(Ok(Box::new(crcbl_hal::null::NullInstance::gpu_driven()))),
            },
        ];

        let error = request_open_auto(ALWAYS_FAILS)
            .block()
            .expect_err("every auto entry refuses");
        match error {
            GpuError::NoBackend { tried, last } => {
                assert_eq!(tried, "vk, webgpu", "both attempts, in registry order");
                assert!(last.contains("no adapter in this test"), "{last}");
            }
            other => panic!("wrong error: {other}"),
        }
    }

    /// A backend asked for **by name** does not fall back — that is the whole
    /// difference between `request_open_backend` and `request_open_auto`.
    #[test]
    fn a_named_backend_that_refuses_is_final() {
        static ONLY_A_BROKEN_VK: &[Registration] = &[Registration {
            backend: GpuBackend::Vulkan,
            auto: true,
            open: || {
                ready(Err(HalError::Unsupported {
                    backend: crcbl_hal::BackendKind::Vulkan,
                    what: "no driver in this test",
                }))
            },
        }];

        let entry = lookup(ONLY_A_BROKEN_VK, GpuBackend::Vulkan).expect("it is right there");
        let mut pending = PendingInstance {
            current: Some((GpuBackend::Vulkan, (entry.open)())),
            remaining: &[],
            fallback: false,
            tried: vec![GpuBackend::Vulkan],
            last: "nothing was tried".to_string(),
        };
        match pending.poll().expect_err("it refused") {
            GpuError::Backend { backend, source } => {
                assert_eq!(backend, GpuBackend::Vulkan);
                assert!(source.to_string().contains("no driver in this test"));
            }
            other => panic!("wrong error: {other}"),
        }
    }

    /// **`ALL` really is all of them, and in the order `position` says.**
    ///
    /// The half a `match` cannot do. `GpuBackend::position` fails to compile
    /// when a variant is added, which forces the author to give the new backend
    /// an index — but nothing in the language then makes them put it in `ALL`,
    /// and an index handed out twice or pointing past the array is the same
    /// silence. Round-tripping every element through both directions is what
    /// closes it: a variant missing from `ALL` leaves some index unclaimed, and
    /// the length assertion is what notices.
    #[test]
    fn every_backend_is_in_all_at_its_own_position() {
        for (index, backend) in GpuBackend::ALL.iter().enumerate() {
            assert_eq!(
                backend.position(),
                index,
                "{backend} sits at {index} in `ALL` and answers {} for its position",
                backend.position(),
            );
            assert_eq!(
                GpuBackend::from_name(backend.as_str()),
                Some(*backend),
                "`{backend}` does not parse back to itself",
            );
        }
    }

    /// **The rejection message names every backend and nothing else.**
    ///
    /// Not "the string is what it was last time" — that is a snapshot, and it
    /// would pass a list that had gone stale in exactly the way this function
    /// exists to prevent. The claim is against `ALL`, so adding a backend
    /// changes both sides together.
    #[test]
    fn the_name_list_offers_every_backend() {
        let listed = GpuBackend::name_list();
        for backend in GpuBackend::ALL {
            assert!(
                listed.contains(&format!("`{backend}`")),
                "`{backend}` is a backend and `{listed}` does not offer it",
            );
        }
        assert_eq!(
            listed.matches('`').count(),
            GpuBackend::ALL.len() * 2,
            "`{listed}` quotes something that is not one of the {} backends",
            GpuBackend::ALL.len(),
        );
        assert!(
            listed.ends_with("or `webgpu`"),
            "the list must read as English, not as a comma-separated tail: `{listed}`",
        );
    }
}
