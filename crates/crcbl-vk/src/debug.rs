//! Validation layers, the debug-utils messenger, and the thing that makes
//! "zero validation errors" a **test result** rather than an aspiration.
//!
//! `docs/plan/02-vulkan-backend.md`'s P1 exit criteria say "zero validation
//! errors/warnings". A messenger that only calls [`crcbl_core::log::error!`] cannot enforce
//! that: CI captures stderr and nobody reads it, so the criterion quietly
//! becomes a manual eyeball. So every message the layer emits is *also* counted
//! and kept here, and [`ValidationReport::assert_clean`] turns the count into a
//! failing assertion.
//!
//! ```text
//! VK_LAYER_KHRONOS_validation ──▶ messenger callback ──┬──▶ log::{error,warn,info,debug}
//!                                                      └──▶ ValidationSink (counted, kept)
//! ```
//!
//! # When validation is on
//!
//! Debug builds, which is every `cargo test` and every `cargo run` a developer
//! makes, and never a release build — the layer costs 2–10× and ships to nobody.
//! `CRCBL_VK_VALIDATION=0` turns it off in a debug build (for profiling a debug
//! binary) and `CRCBL_VK_VALIDATION=1` turns it on in a release one (for a
//! release-mode driver bug). A layer that is asked for and missing is a
//! **warning, not a failure**: a machine without the validation layers package
//! must still be able to run the engine.
//!
//! # Synchronisation validation
//!
//! `docs/plan/02-vulkan-backend.md` lists sync bugs as the stage's headline risk
//! and names "validation layers with sync-validation enabled in CI runs" as the
//! mitigation. It is off by default because it is expensive and noisy on WSI
//! paths, and on with `CRCBL_VK_SYNC_VALIDATION=1`, which is what the e2e
//! harness sets.

use core::ffi::{CStr, c_void};
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ash::vk;

/// The layer the messenger listens to, as the loader spells it.
pub(crate) const VALIDATION_LAYER_C: &CStr = c"VK_LAYER_KHRONOS_validation";

/// The layer's name, for error messages a reader can act on.
pub const VALIDATION_LAYER: &str = "VK_LAYER_KHRONOS_validation";

/// Overrides whether the validation layer is requested. `0`/`false`/`off` to
/// disable, anything else to enable.
pub const VALIDATION_ENV_VAR: &str = "CRCBL_VK_VALIDATION";

/// Set to a truthy value to add synchronisation validation on top.
pub const SYNC_VALIDATION_ENV_VAR: &str = "CRCBL_VK_SYNC_VALIDATION";

/// How severe a message the layer emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Chatter: object creation, layer startup.
    Verbose,
    /// Something worth knowing but not wrong.
    Info,
    /// Legal but suspect — a performance warning, or a best-practice miss.
    Warning,
    /// A specification violation. Always a bug in this crate or above it.
    Error,
}

impl Severity {
    fn from_flags(flags: vk::DebugUtilsMessageSeverityFlagsEXT) -> Self {
        if flags.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
            Self::Error
        } else if flags.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
            Self::Warning
        } else if flags.contains(vk::DebugUtilsMessageSeverityFlagsEXT::INFO) {
            Self::Info
        } else {
            Self::Verbose
        }
    }

    /// Whether a message at this severity fails
    /// [`ValidationReport::assert_clean`].
    #[must_use]
    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Error | Self::Warning)
    }
}

/// One message from the validation layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationMessage {
    /// How bad it is.
    pub severity: Severity,
    /// The layer's own VUID or message id, e.g. `"VUID-vkCmdDraw-None-02700"`.
    pub id: String,
    /// The full text.
    pub text: String,
}

/// Everything the validation layer said while an instance was alive.
///
/// Snapshot it with [`crate::VkInstance::validation_report`] and assert on it.
/// The messages are kept, not merely counted, because "one error" is not a
/// useful failure message and the VUID is the whole diagnosis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// Every message at [`Severity::Warning`] or [`Severity::Error`], in the
    /// order the layer emitted them. Capped — see [`ValidationSink::CAPACITY`].
    pub messages: Vec<ValidationMessage>,
    /// Errors seen, including any dropped past the cap.
    pub errors: u64,
    /// Warnings seen, including any dropped past the cap.
    pub warnings: u64,
    /// Whether the validation layer was actually loaded. A report from an
    /// instance without it is clean because nothing was checked, which is a
    /// very different thing from clean because nothing was wrong.
    pub enabled: bool,
}

impl ValidationReport {
    /// Whether the layer produced no errors and no warnings.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.errors == 0 && self.warnings == 0
    }

    /// Fails the calling test unless validation was enabled *and* silent.
    ///
    /// The "enabled" half is not pedantry: a test that passes because the layer
    /// was missing proves nothing, and that failure mode is invisible without
    /// this check. `docs/plan/12-testing.md` calls the equivalent for e2e jobs —
    /// a suite that silently skips everything — a known trap.
    ///
    /// # Panics
    ///
    /// If the layer was not loaded, or if it emitted any error or warning.
    pub fn assert_clean(&self) {
        assert!(
            self.enabled,
            "validation was not enabled, so this proves nothing — install \
             {VALIDATION_LAYER} (Arch: vulkan-validation-layers, Debian/Ubuntu: \
             vulkan-validationlayers) or unset {VALIDATION_ENV_VAR}",
        );
        assert!(
            self.is_clean(),
            "the validation layer reported {} error(s) and {} warning(s):\n{}",
            self.errors,
            self.warnings,
            self.summary(),
        );
    }

    /// One line per kept message, for an assertion message or a log dump.
    #[must_use]
    pub fn summary(&self) -> String {
        self.messages
            .iter()
            .map(|message| {
                format!(
                    "  [{:?}] {}: {}",
                    message.severity, message.id, message.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The shared side table the messenger writes into.
///
/// An [`Arc`] rather than a global: two instances in one process (the seam's
/// `dyn` decision exists so `crcbl-vk` and `crcbl-wgpu` can coexist, and a test
/// may well open two Vulkan instances) must not pollute each other's report.
/// The pointer is handed to the driver as the messenger's `pUserData`, which is
/// why it is leaked and reclaimed explicitly rather than borrowed.
#[derive(Clone, Debug, Default)]
pub struct ValidationSink {
    inner: Arc<SinkInner>,
}

#[derive(Debug, Default)]
struct SinkInner {
    errors: AtomicU64,
    warnings: AtomicU64,
    /// Bounded, so a shader spewing one message per draw cannot exhaust memory
    /// while still leaving the counters exact.
    messages: Mutex<Vec<ValidationMessage>>,
}

impl SinkInner {
    fn record(&self, message: ValidationMessage) {
        match message.severity {
            Severity::Error => {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
            Severity::Warning => {
                self.warnings.fetch_add(1, Ordering::Relaxed);
            }
            Severity::Info | Severity::Verbose => return,
        }
        // A poisoned mutex here would mean a panic inside the messenger, which
        // is already a lost cause; dropping the message beats a second panic
        // raised from inside a driver callback.
        if let Ok(mut messages) = self.messages.lock()
            && messages.len() < ValidationSink::CAPACITY
        {
            messages.push(message);
        }
    }
}

impl ValidationSink {
    /// How many messages are kept verbatim. Beyond this only the counts grow —
    /// the first few hundred are the diagnosis and the rest are noise.
    pub const CAPACITY: usize = 256;

    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one message.
    pub fn record(&self, message: ValidationMessage) {
        self.inner.record(message);
    }

    /// A snapshot of everything recorded so far.
    #[must_use]
    pub fn report(&self, enabled: bool) -> ValidationReport {
        ValidationReport {
            messages: self
                .inner
                .messages
                .lock()
                .map(|messages| messages.clone())
                .unwrap_or_default(),
            errors: self.inner.errors.load(Ordering::Relaxed),
            warnings: self.inner.warnings.load(Ordering::Relaxed),
            enabled,
        }
    }
}

/// Whether the validation layer should be requested for this process.
///
/// Debug builds yes, release builds no, [`VALIDATION_ENV_VAR`] overrides both.
#[must_use]
pub(crate) fn validation_wanted() -> bool {
    validation_policy(env_flag(VALIDATION_ENV_VAR))
}

/// The pure half of [`validation_wanted`], so the *default* is testable.
///
/// It has to be reachable without the environment: a test that set
/// [`VALIDATION_ENV_VAR`] would be setting it for every other test in this
/// binary, and one that only asserts on [`parse_flag`] never reaches the
/// fallback at all — which is how this policy went unguarded.
const fn validation_policy(override_: Option<bool>) -> bool {
    match override_ {
        Some(explicit) => explicit,
        None => cfg!(debug_assertions),
    }
}

/// Whether synchronisation validation should be layered on top. Opt-in only.
#[must_use]
pub(crate) fn sync_validation_wanted() -> bool {
    sync_validation_policy(env_flag(SYNC_VALIDATION_ENV_VAR))
}

/// The pure half of [`sync_validation_wanted`]; see [`validation_policy`].
const fn sync_validation_policy(override_: Option<bool>) -> bool {
    match override_ {
        Some(explicit) => explicit,
        None => false,
    }
}

/// Parses a boolean environment variable, tolerating the spellings people
/// actually type. An unset or empty variable is "no opinion".
fn env_flag(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    parse_flag(&value)
}

/// The pure half of [`env_flag`], so the spelling table is testable.
fn parse_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

/// The messenger's `pfnUserCallback`.
///
/// Routes into `log` with severity mapping (§2.1 of the Vulkan plan) and into
/// the sink that makes the P1 gate checkable.
///
/// # Safety
///
/// Called by the Vulkan loader with a live `callback_data` pointer and the
/// `user_data` this crate passed to `vkCreateDebugUtilsMessengerEXT`, which is
/// an `Arc<SinkInner>` raw pointer that outlives the messenger.
unsafe extern "system" fn messenger_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    kinds: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user_data: *mut c_void,
) -> vk::Bool32 {
    // **Nothing may unwind out of here.** This is an `extern "system"` frame
    // the *driver* called; a panic crossing it is undefined behaviour, and the
    // runtime's answer is to abort the process. That is not hypothetical: the
    // body below allocates two `String`s and then dispatches into whatever
    // `log` backend the application installed, which is arbitrary third-party
    // code running on a driver thread. A logger that panics — a poisoned mutex,
    // a full channel, a `write!` to a closed pipe — would take the process down
    // from inside `vkQueueSubmit`, with a backtrace pointing at the driver.
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: discharged by this function's own contract, unchanged.
        unsafe { messenger_body(severity, kinds, callback_data, user_data) };
    }));
    if caught.is_err() {
        // Deliberately not `log`: the logger is the prime suspect. `eprintln!`
        // can itself panic on a closed stderr, so even that is guarded.
        let _ = std::panic::catch_unwind(|| {
            eprintln!(
                "crcbl-vk: a panic escaped the Vulkan debug messenger callback and was \
                 contained; the validation message it was reporting is lost"
            );
        });
    }
    // The spec requires `VK_FALSE` from an application callback: `VK_TRUE`
    // aborts the offending call, which turns every warning into a crash.
    vk::FALSE
}

/// The messenger's actual work, called only from inside a [`catch_unwind`].
///
/// # Safety
///
/// As [`messenger_callback`].
///
/// [`catch_unwind`]: std::panic::catch_unwind
unsafe fn messenger_body(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    kinds: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user_data: *mut c_void,
) {
    // SAFETY: the loader guarantees `callback_data` is a live, fully populated
    // struct for the duration of the call. A null pointer would be a loader
    // bug, and is checked for rather than trusted because this callback runs
    // inside someone else's code and must not fault.
    let Some(data) = (unsafe { callback_data.as_ref() }) else {
        return;
    };
    // SAFETY: the two `*const c_char` fields are either null or NUL-terminated
    // strings owned by the layer and valid for this call only, which is why
    // both are copied into owned `String`s before anything else happens.
    let (id, text) = unsafe {
        (
            cstr_or_empty(data.p_message_id_name),
            cstr_or_empty(data.p_message),
        )
    };

    let severity = Severity::from_flags(severity);
    let kind = if kinds.contains(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION) {
        "validation"
    } else if kinds.contains(vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE) {
        "performance"
    } else {
        "general"
    };
    match severity {
        Severity::Error => crcbl_core::log::error!("vk {kind}: {id}: {text}"),
        Severity::Warning => crcbl_core::log::warn!("vk {kind}: {id}: {text}"),
        Severity::Info => crcbl_core::log::info!("vk {kind}: {id}: {text}"),
        Severity::Verbose => crcbl_core::log::debug!("vk {kind}: {id}: {text}"),
    }

    if !user_data.is_null() {
        // SAFETY: `user_data` is the pointer produced by `Arc::into_raw` in
        // `messenger_user_data`. The instance keeps that allocation alive until
        // strictly after `vkDestroyDebugUtilsMessengerEXT`, so it is live for
        // the whole time the driver can call this. A shared reference is enough
        // — never an owned `Arc`, which would double-free — because `SinkInner`
        // is `Sync` and every field is interior-mutable.
        let sink = unsafe { &*user_data.cast::<SinkInner>() };
        sink.record(ValidationMessage { severity, id, text });
    }
}

/// Copies a possibly-null C string into an owned `String`.
///
/// # Safety
///
/// `raw` must be null or point at a NUL-terminated string valid for this call.
unsafe fn cstr_or_empty(raw: *const core::ffi::c_char) -> String {
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: the caller promises a NUL-terminated string; `to_string_lossy`
    // copies before the layer reclaims it.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

/// The messenger creation info this crate always uses.
///
/// Split out so the same severities and types are requested for the
/// instance-creation-time messenger (chained into `VkInstanceCreateInfo`, which
/// is the only way to see errors *during* `vkCreateInstance`) and for the
/// persistent one created afterwards.
pub(crate) fn messenger_create_info<'a>(
    user_data: *mut c_void,
) -> vk::DebugUtilsMessengerCreateInfoEXT<'a> {
    vk::DebugUtilsMessengerCreateInfoEXT::default()
        // Errors and warnings only. `INFO` is where the *loader* narrates its
        // own manifest search — a hundred lines before `vkCreateInstance`
        // returns — and none of it is about this engine. The P1 gate is "zero
        // errors and warnings", so those are exactly the two severities worth
        // routing, and `VK_LOADER_DEBUG=all` is the tool for the rest.
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(messenger_callback))
        .user_data(user_data)
}

/// Leaks a sink's inner allocation into a raw pointer for `pUserData`.
///
/// The instance reclaims it with [`drop_messenger_user_data`] after destroying
/// the messenger — never before, because the driver may still call back.
#[must_use]
pub(crate) fn messenger_user_data(sink: &ValidationSink) -> *mut c_void {
    Arc::into_raw(Arc::clone(&sink.inner)).cast_mut().cast()
}

/// Reclaims a pointer from [`messenger_user_data`].
///
/// # Safety
///
/// `raw` must be a pointer from [`messenger_user_data`] that has not been
/// reclaimed, and the messenger using it must already be destroyed.
pub(crate) unsafe fn drop_messenger_user_data(raw: *mut c_void) {
    if raw.is_null() {
        return;
    }
    // SAFETY: the caller promises this is the matching `Arc::into_raw` pointer
    // and that no messenger can still call back through it.
    drop(unsafe { Arc::from_raw(raw.cast::<SinkInner>()) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(severity: Severity, id: &str) -> ValidationMessage {
        ValidationMessage {
            severity,
            id: id.to_string(),
            text: "something happened".to_string(),
        }
    }

    #[test]
    fn severity_maps_from_the_layers_flags_and_picks_the_worst() {
        assert_eq!(
            Severity::from_flags(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR),
            Severity::Error
        );
        assert_eq!(
            Severity::from_flags(vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE),
            Severity::Verbose
        );
        // The layer may set several bits at once; the worst one wins, or a
        // warning bundled with an error would be reported as a warning.
        assert_eq!(
            Severity::from_flags(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
            ),
            Severity::Error
        );
        assert!(Severity::Error.is_failure());
        assert!(
            Severity::Warning.is_failure(),
            "the P1 gate is zero errors AND zero warnings"
        );
        assert!(!Severity::Info.is_failure());
    }

    #[test]
    fn the_sink_counts_failures_and_ignores_chatter() {
        let sink = ValidationSink::new();
        sink.record(message(Severity::Verbose, "chatter"));
        sink.record(message(Severity::Info, "note"));
        assert!(sink.report(true).is_clean());

        sink.record(message(Severity::Warning, "VUID-warn"));
        sink.record(message(Severity::Error, "VUID-err"));
        sink.record(message(Severity::Error, "VUID-err-2"));
        let report = sink.report(true);
        assert_eq!((report.errors, report.warnings), (2, 1));
        assert!(!report.is_clean());
        assert_eq!(report.messages.len(), 3, "chatter is not kept");
        assert!(
            report.summary().contains("VUID-err-2"),
            "{}",
            report.summary()
        );
    }

    /// A message storm must not be able to exhaust memory, and must not be able
    /// to make the counters lie either.
    #[test]
    fn kept_messages_are_capped_but_the_counts_are_exact() {
        let sink = ValidationSink::new();
        let storm = ValidationSink::CAPACITY + 50;
        for index in 0..storm {
            sink.record(message(Severity::Error, &format!("VUID-{index}")));
        }
        let report = sink.report(true);
        assert_eq!(report.messages.len(), ValidationSink::CAPACITY);
        assert_eq!(report.errors as usize, storm);
    }

    /// The trap this whole module exists to close: a clean report from an
    /// instance that never loaded the layer proves nothing, and must fail.
    #[test]
    #[should_panic(expected = "validation was not enabled")]
    fn a_clean_report_from_a_disabled_layer_still_fails() {
        ValidationSink::new().report(false).assert_clean();
    }

    #[test]
    #[should_panic(expected = "1 error(s)")]
    fn assert_clean_names_the_counts() {
        let sink = ValidationSink::new();
        sink.record(message(Severity::Error, "VUID-vkCmdDraw-None-02700"));
        sink.report(true).assert_clean();
    }

    #[test]
    fn a_silent_enabled_layer_passes() {
        ValidationSink::new().report(true).assert_clean();
    }

    /// Two instances in one process must not see each other's messages — the
    /// reason the sink is an `Arc` and not a global counter.
    #[test]
    fn sinks_are_independent_but_clones_share() {
        let first = ValidationSink::new();
        let second = ValidationSink::new();
        let alias = first.clone();
        alias.record(message(Severity::Error, "VUID-1"));
        assert_eq!(first.report(true).errors, 1);
        assert_eq!(second.report(true).errors, 0);
    }

    #[test]
    fn the_environment_override_accepts_the_spellings_people_type() {
        for off in ["0", "false", "no", "OFF", " off "] {
            assert_eq!(parse_flag(off), Some(false), "{off}");
        }
        for on in ["1", "true", "yes", "on", "anything"] {
            assert_eq!(parse_flag(on), Some(true), "{on}");
        }
        assert_eq!(parse_flag(""), None, "unset means 'no opinion'");
        assert_eq!(parse_flag("   "), None);
    }

    /// The default policy, stated as a test so flipping it is a deliberate
    /// edit: on in debug, off in release. This is the guard on the oracle the
    /// whole e2e suite leans on — `assert_clean` can only fail when the layer
    /// was asked for — so it asserts against [`validation_policy`] itself
    /// rather than restating `cfg!(debug_assertions)` back at itself.
    ///
    /// A debug run is what distinguishes the fallback from a hardcoded `false`;
    /// under `--release` the two are the same function, and nothing can tell
    /// them apart.
    #[test]
    fn validation_defaults_follow_the_build_profile() {
        assert_eq!(
            validation_policy(None),
            cfg!(debug_assertions),
            "an unset CRCBL_VK_VALIDATION means 'on in debug, off in release'"
        );
        assert!(
            !validation_policy(Some(false)),
            "an explicit answer beats the profile"
        );
        assert!(validation_policy(Some(true)));

        // Sync validation is opt-in on every profile, debug included.
        assert!(!sync_validation_policy(None));
        assert!(sync_validation_policy(Some(true)));
        assert!(!sync_validation_policy(Some(false)));
    }

    /// The user-data pointer round trip. If `into_raw`/`from_raw` ever stop
    /// pairing, the messenger writes into freed memory — which is why the
    /// reclaim is a named function and not an inline cast.
    #[test]
    fn messenger_user_data_round_trips_without_leaking_the_sink() {
        let sink = ValidationSink::new();
        let raw = messenger_user_data(&sink);
        assert!(!raw.is_null());
        // SAFETY: `raw` came from `messenger_user_data` above and no messenger
        // was ever created with it, so nothing can call back through it.
        unsafe { drop_messenger_user_data(raw) };
        // The original is still usable, which is the property that matters: the
        // reclaim dropped a clone, not the sink.
        sink.record(message(Severity::Error, "VUID-after"));
        assert_eq!(sink.report(true).errors, 1);

        // A null pointer is tolerated, because the instance passes one when the
        // layer is absent.
        // SAFETY: explicitly the null case the function documents.
        unsafe { drop_messenger_user_data(core::ptr::null_mut()) };
    }
}
