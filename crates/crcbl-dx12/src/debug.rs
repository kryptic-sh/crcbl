//! The D3D12 debug layer, the message queue that makes it readable in CI, and
//! the call that turns `DXGI_ERROR_DEVICE_REMOVED` into a *reason*.
//!
//! # The failure this module exists for
//!
//! `DXGI_ERROR_DEVICE_REMOVED` is reported at the **next** call, not at the one
//! that caused it. A device that serves a whole test suite and then fails a
//! buffer creation with `0x887A0005` was broken by something earlier, and the
//! code alone names neither the call nor the mistake. D3D12's own error text
//! says what to do about it — "Use GetDeviceRemovedReason to determine the
//! appropriate action" — and until this module existed nothing in this backend
//! asked. [`diagnosis`] is what every device-removed report now carries.
//!
//! # Two halves, and only one of them is the layer
//!
//! * [`removed_reason`] needs nothing installed. It is a vtable call on the
//!   device this crate already holds, so it answers on every Windows machine
//!   including a bare runner.
//! * The **debug layer** is an optional Windows component (the *Graphics Tools*
//!   feature on demand). Where it is present it reports the offending call *at*
//!   the call, which is the difference between "a buffer creation failed" and
//!   "this resource state transition declared a before-state the resource was
//!   not in". Where it is absent, [`enable_debug_layer`] warns and everything
//!   else still works.
//!
//! # And a third, which is where the *operation* comes from
//!
//! Neither half above names the GPU work that did it: the reason is one
//! `HRESULT`, and the debug layer reports API misuse rather than a command list
//! that stopped. [`crate::dred`] is the half that does — auto-breadcrumbs, per
//! command list, saying how many operations the GPU finished and which one it
//! was still inside. [`diagnosis`] appends it inside the removed-device arm and
//! nowhere else, because DRED's queries answer nothing on a live device.
//!
//! # The layer's output does not reach a CI log on its own
//!
//! `ID3D12Debug::EnableDebugLayer` sends its messages to the **debugger** —
//! `OutputDebugString` — and a GitHub runner has no debugger attached, so a
//! job that only enabled the layer would produce exactly the same log as one
//! that did not. [`read_queue`] is the half that makes it readable: the
//! layer also stores every message in an `ID3D12InfoQueue`, and this pulls them
//! out and puts them in the error a caller actually sees. `crcbl-vk` has the
//! same problem and solves it the same way, through a messenger callback into
//! [`crate::debug`]'s opposite number there.
//!
//! # When the layer is on
//!
//! Debug builds, which is every `cargo test` and every `cargo run` a developer
//! makes, and never a release build. [`VALIDATION_ENV_VAR`] overrides both
//! directions, exactly as `crcbl_vk::debug`'s `CRCBL_VK_VALIDATION` does.
//!
//! # "Zero validation errors" is a test result here too
//!
//! [`report`] is the machine-readable half [`diagnosis`] was not: severities as
//! an enum rather than a word inside a formatted line, counts beside the kept
//! messages, and a flag saying whether anything was checked at all.
//! [`ValidationReport::assert_clean`] turns it into a failing assertion with
//! `crcbl_vk::debug`'s semantics unchanged — **a layer that was not on fails
//! before the messages are even looked at**, because a suite that passed for
//! want of a layer proves nothing and that failure mode is otherwise invisible.
//!
//! The escape hatch is one variable and it is not a silent one. Windows'
//! *Graphics Tools* feature is optional, so `CRCBL_DX12_VALIDATION=0` says "this
//! machine has no debug layer, do not assert on it" — and the whole suite then
//! runs and states, on the line `tests/run-dx12-e2e.sh` reads, that it checked
//! no validation. Asking for the layer and not getting it stays a failure: that
//! is the case where a reader would otherwise believe the checking happened.
//!
//! # A message id is argued past one at a time, or not at all
//!
//! [`ALLOWED`] is the whole of it: a table of `D3D12_MESSAGE_ID`s with the
//! argument for each, applied **only at [`Severity::Warning`]**. It is not a
//! severity filter and must never become one — a run that stopped failing on
//! warnings would stop being the line `crcbl-vk` is held to, and every id in the
//! table would then be joined by every id nobody had looked at. An allowed
//! message is still counted, in [`ValidationReport::allowed`], so "the layer
//! said nothing" and "the layer said things this crate has an answer for" stay
//! different claims.
//!
//! # Nothing clears the info queue, and that is deliberate
//!
//! An earlier [`diagnosis`] drained the queue and cleared it, so the messages
//! belonged to the failure being reported. That is exactly wrong once anything
//! else reads the same queue: whoever ran first took the messages and everyone
//! after them saw a clean device. [`read_queue`] therefore reads from index zero
//! every time and clears nothing, so a validation error quoted inside a
//! `HalError` is the *same* error that fails the teardown assertion rather than
//! one that consumed it. [`attach`] still clears once, at device creation, which
//! is what makes a report mean "since this device was created".

#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D12::{
    D3D12_INFO_QUEUE_FILTER, D3D12_INFO_QUEUE_FILTER_DESC, D3D12_MESSAGE,
    D3D12_MESSAGE_ID_CLEARDEPTHSTENCILVIEW_MISMATCHINGCLEARVALUE,
    D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE,
    D3D12_MESSAGE_ID_CREATE_SAMPLER_COMPARISON_FUNC_IGNORED, D3D12_MESSAGE_SEVERITY,
    D3D12_MESSAGE_SEVERITY_CORRUPTION, D3D12_MESSAGE_SEVERITY_ERROR, D3D12_MESSAGE_SEVERITY_INFO,
    D3D12_MESSAGE_SEVERITY_MESSAGE, D3D12_MESSAGE_SEVERITY_WARNING, D3D12GetDebugInterface,
    ID3D12Debug, ID3D12Device, ID3D12InfoQueue,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    DXGI_ERROR_DRIVER_INTERNAL_ERROR, DXGI_ERROR_INVALID_CALL,
};
#[cfg(target_os = "windows")]
use windows::core::{HRESULT, Interface};

/// Overrides whether the D3D12 debug layer is turned on. `0`/`false`/`no`/`off`
/// to disable, anything else to enable.
pub(crate) const VALIDATION_ENV_VAR: &str = "CRCBL_DX12_VALIDATION";

/// Whether the debug layer should be turned on for this process.
///
/// Debug builds yes, release builds no, [`VALIDATION_ENV_VAR`] overrides both.
#[cfg(target_os = "windows")]
fn validation_wanted() -> bool {
    validation_policy(env_flag(VALIDATION_ENV_VAR))
}

/// The pure half of `validation_wanted`, so the *default* is testable.
///
/// It has to be reachable without the environment, for the reason
/// `crcbl_vk::debug`'s twin gives: a test that set [`VALIDATION_ENV_VAR`] would
/// be setting it for every other test in the binary, and one that only asserts
/// on [`parse_flag`] never reaches the fallback at all.
const fn validation_policy(override_: Option<bool>) -> bool {
    match override_ {
        Some(explicit) => explicit,
        None => cfg!(debug_assertions),
    }
}

/// Parses a boolean environment variable, tolerating the spellings people
/// actually type. An unset or empty variable is "no opinion".
#[cfg(target_os = "windows")]
fn env_flag(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    parse_flag(&value)
}

/// The pure half of `env_flag`, so the spelling table is testable.
fn parse_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

/// How many of the info queue's messages are kept verbatim in a report.
///
/// A cap rather than the whole queue: a device that has gone wrong emits the
/// same validation error once per call, and a panic message carrying hundreds
/// of copies is one nobody reads. The first are the ones nearest the cause, and
/// the counts stay exact past it — the shape `crcbl_vk::debug`'s `ValidationSink`
/// uses, for the same reason.
const MAX_KEPT_MESSAGES: usize = 32;

/// How the debug layer graded a message.
///
/// D3D12's own severities, in this crate's own enum rather than as the
/// `D3D12_MESSAGE_SEVERITY` newtype, so that everything a report is *asked* —
/// which of these fail a test, how one prints — is decidable on a machine with
/// no D3D12 at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Severity {
    /// The layer's own chatter, denied at the storage filter by [`attach`].
    Message,
    /// Object creation and destruction, likewise denied.
    Info,
    /// Legal but suspect.
    Warning,
    /// An API misuse. Always a bug in this crate or above it.
    Error,
    /// Memory the runtime found corrupted. Worse than an error.
    Corruption,
    /// A severity this build has no name for. Reported by number rather than
    /// folded into one of the above, which would rank it wrongly.
    Unknown(i32),
}

impl Severity {
    /// Whether a message at this severity fails
    /// [`ValidationReport::assert_clean`].
    ///
    /// Warnings included, because the line this backend is held to is
    /// `crcbl-vk`'s and that one is "zero errors **and** warnings". An
    /// unrecognised severity counts too: a newer runtime's grading is not
    /// something to pass by default.
    pub(crate) const fn is_failure(self) -> bool {
        !matches!(self, Self::Message | Self::Info)
    }

    /// The word that appears in a report line.
    fn as_str(self) -> String {
        match self {
            Self::Message => "MESSAGE".to_string(),
            Self::Info => "INFO".to_string(),
            Self::Warning => "WARNING".to_string(),
            Self::Error => "ERROR".to_string(),
            Self::Corruption => "CORRUPTION".to_string(),
            Self::Unknown(raw) => format!("SEVERITY {raw}"),
        }
    }
}

/// `D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE`.
///
/// Spelled as a number here for the reason [`Severity`] is this crate's own
/// enum: [`ALLOWED`] and everything asked of it have to be readable — and
/// testable — on a machine with no D3D12 at all. The `const` assertion below is
/// what keeps the number and the constant equal, and it is checked by the
/// `x86_64-pc-windows-msvc` build.
const CLEAR_VALUE_NOT_GIVEN: i32 = 820;

/// `D3D12_MESSAGE_ID_CLEARDEPTHSTENCILVIEW_MISMATCHINGCLEARVALUE`, the depth
/// twin of [`CLEAR_VALUE_NOT_GIVEN`]. See that constant for why it is a number.
const DEPTH_CLEAR_VALUE_NOT_GIVEN: i32 = 821;

/// `D3D12_MESSAGE_ID_CREATE_SAMPLER_COMPARISON_FUNC_IGNORED`. See
/// [`CLEAR_VALUE_NOT_GIVEN`] for why it is a number.
const SAMPLER_COMPARISON_FUNC_IGNORED: i32 = 1361;

// **The number is the constant, or this does not compile.** A table written
// against literals is one nothing checks; this is the check, and it runs
// wherever the crate is built for Windows.
#[cfg(target_os = "windows")]
const _: () = assert!(
    CLEAR_VALUE_NOT_GIVEN == D3D12_MESSAGE_ID_CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE.0
        && DEPTH_CLEAR_VALUE_NOT_GIVEN
            == D3D12_MESSAGE_ID_CLEARDEPTHSTENCILVIEW_MISMATCHINGCLEARVALUE.0
        && SAMPLER_COMPARISON_FUNC_IGNORED
            == D3D12_MESSAGE_ID_CREATE_SAMPLER_COMPARISON_FUNC_IGNORED.0,
    "a debug-layer message id in ALLOWED is not the D3D12 constant it names"
);

/// A warning this crate has an answer for, and the answer.
///
/// The `reason` is the whole point of the type: an id on its own is a number
/// somebody once decided to ignore, and a year later nobody can tell a settled
/// argument from a silenced failure.
struct AllowedMessage {
    /// The `D3D12_MESSAGE_ID` this covers.
    id: i32,
    /// Why a message with that id is not a defect in this backend.
    reason: &'static str,
}

/// Every message id `ValidationReport::assert_clean` passes over, with the
/// argument for each.
///
/// **Per id, and warnings only.** Neither half is incidental: an entry that
/// covered a whole severity would be the blanket suppression this gate exists to
/// prevent, and an entry that covered every severity of one id would let the
/// same number arrive as an `ERROR` and still pass. [`allowance`] enforces both.
const ALLOWED: &[AllowedMessage] = &[
    AllowedMessage {
        id: CLEAR_VALUE_NOT_GIVEN,
        reason: "ClearRenderTargetView on an image created with no pOptimizedClearValue. The \
                 layer grades it itself — 'the clear operation is typically slower as a result; \
                 but will still clear to the desired value' — so it is a fast-path advisory and \
                 not a misuse. Passing a value is not the fix: D3D12 treats one as a promise and \
                 reports this same id again, as MISMATCHINGCLEARVALUE, whenever a pass clears to \
                 anything else, and the seam learns the colour at begin_render_pass from \
                 ColorAttachment::clear, which is after the image exists. Device::create_image is \
                 the one place that passes None; a backend that started passing a value would \
                 have to revisit this entry, because the mismatch form is a real defect.",
    },
    AllowedMessage {
        id: DEPTH_CLEAR_VALUE_NOT_GIVEN,
        reason: "ClearDepthStencilView on an image created with no pOptimizedClearValue. Every \
                 word of the entry above applies unchanged — same advisory, same grading by the \
                 layer, same Device::create_image passing None, and the same reason passing a \
                 value would trade an advisory for a real MISMATCHINGCLEARVALUE the moment a pass \
                 cleared to anything else. It is a second id because D3D12 files the depth and \
                 colour forms under different numbers and this table is per id by design; the two \
                 entries move together. `crcbl-render`'s D3D12 frame is where it was first \
                 measured, and the mesh-cluster probe in crate::device is the test here that \
                 clears a depth attachment at all.",
    },
    AllowedMessage {
        id: SAMPLER_COMPARISON_FUNC_IGNORED,
        reason: "CreateSampler with a ComparisonFunc beside a filter that does not compare. The \
                 layer grades this one too — 'This is OK, as the ComparisonFunc will simply be \
                 ignored' — and ignored is what the seam means: SamplerDesc::compare == None is \
                 'this sampler does not compare', so the sampler behaves exactly as asked and \
                 the field Device::create_sampler fills is one D3D12 never reads. \
                 D3D12_COMPARISON_FUNC_NONE (0) is the enumerant that would say it outright, and \
                 deleting this entry for it is the right end state — but this backend asks for \
                 feature level 11_0 and nothing here has established which runtimes take a zero \
                 in a classic D3D12_SAMPLER_DESC rather than filing CREATE_SAMPLER_INVALID for \
                 it, which would be trading an advisory for an error on the machines this crate \
                 cannot test on.",
    },
];

/// The argument for passing over this message, or `None` if there is not one.
///
/// Warnings only, so an id in [`ALLOWED`] arriving as an `ERROR` or a
/// `CORRUPTION` fails exactly as it would have without the table.
fn allowance(severity: Severity, id: i32) -> Option<&'static str> {
    if !matches!(severity, Severity::Warning) {
        return None;
    }
    ALLOWED
        .iter()
        .find(|allowed| allowed.id == id)
        .map(|allowed| allowed.reason)
}

/// One message the debug layer stored against a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidationMessage {
    /// How bad it is.
    pub(crate) severity: Severity,
    /// The `D3D12_MESSAGE_ID` the layer filed it under, as its raw number —
    /// which is what a `D3D12_MESSAGE_ID_…` name resolves to and what a filter
    /// would be written against.
    pub(crate) id: i32,
    /// The layer's own sentence.
    pub(crate) text: String,
}

impl core::fmt::Display for ValidationMessage {
    /// The one-line form. `crate::device`'s `still_alive` matches on the
    /// bracketed severity, so the shape is load-bearing outside this module.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "[{}] id {}: {}",
            self.severity.as_str(),
            self.id,
            self.text
        )
    }
}

/// Everything the debug layer said about one device.
///
/// The counterpart of `crcbl_vk::debug`'s `ValidationReport`, and deliberately
/// the same shape: counts that stay exact past the message cap, and an
/// `enabled` flag, because a report that is silent for want of a layer and one
/// that is silent because nothing was wrong are very different claims.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidationReport {
    /// Up to [`MAX_KEPT_MESSAGES`] messages at [`Severity::is_failure`], in the
    /// order the layer stored them.
    pub(crate) messages: Vec<ValidationMessage>,
    /// Corruption reports seen, including any past the cap.
    pub(crate) corruption: u64,
    /// Errors seen, including any past the cap.
    pub(crate) errors: u64,
    /// Warnings seen, including any past the cap.
    pub(crate) warnings: u64,
    /// Warnings [`allowance`] had an argument for, counted rather than dropped
    /// silently: a report that passed over two messages and one that saw none
    /// are different claims about a device.
    pub(crate) allowed: u64,
    /// Whether the debug layer was on **and** this device's messages were
    /// readable. False for either half missing, since neither leaves anything
    /// to report.
    pub(crate) enabled: bool,
}

impl ValidationReport {
    /// Whether the layer graded nothing at [`Severity::is_failure`].
    pub(crate) const fn is_clean(&self) -> bool {
        self.corruption == 0 && self.errors == 0 && self.warnings == 0
    }

    /// Fails the calling test unless the layer was on *and* silent.
    ///
    /// The "on" half is not pedantry, and it is the half that fails today: a
    /// run that passed because Windows has no *Graphics Tools* feature proves
    /// nothing about validation, and nothing else in a green log says so.
    /// `docs/plan/12-testing.md` calls the equivalent for e2e jobs — a suite
    /// that silently skips everything — a known trap.
    ///
    /// # Panics
    ///
    /// If the layer was not on, or if it graded anything a warning or worse.
    #[cfg(test)]
    pub(crate) fn assert_clean(&self) {
        assert!(
            self.enabled,
            "the D3D12 debug layer was not on, so this proves nothing — install Windows' \
             Graphics Tools optional feature, or set {VALIDATION_ENV_VAR}=0 to state plainly \
             that this run checks no validation",
        );
        assert!(
            self.is_clean(),
            "the D3D12 debug layer reported {} corruption, {} error(s) and {} warning(s) \
             ({} more were allowed by name in crcbl_dx12::debug::ALLOWED):\n{}",
            self.corruption,
            self.errors,
            self.warnings,
            self.allowed,
            self.summary(),
        );
    }

    /// One indented line per kept message, for an assertion or a log dump.
    #[cfg(test)]
    pub(crate) fn summary(&self) -> String {
        self.messages
            .iter()
            .map(|message| format!("  {message}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Records one message, keeping it if there is room and counting it either
    /// way.
    ///
    /// [`Severity::is_failure`] decides what is counted at all, rather than a
    /// second list of severities here that could disagree with it. Chatter is
    /// denied at the storage filter [`attach`] files, so reaching here at all
    /// means the filter did not take.
    ///
    /// A warning [`allowance`] answers is counted as such and kept out of both
    /// the failure counts and the quoted list — the argument for it is in
    /// [`ALLOWED`] rather than in a panic message nobody is meant to read.
    fn record(&mut self, message: ValidationMessage) {
        if !message.severity.is_failure() {
            return;
        }
        if allowance(message.severity, message.id).is_some() {
            self.allowed += 1;
            return;
        }
        match message.severity {
            Severity::Corruption => self.corruption += 1,
            Severity::Warning => self.warnings += 1,
            _ => self.errors += 1,
        }
        if self.messages.len() < MAX_KEPT_MESSAGES {
            self.messages.push(message);
        }
    }
}

/// Whether the debug layer was asked for, and whether it was there.
///
/// A [`OnceLock`] because `ID3D12Debug::EnableDebugLayer` has to run **before**
/// the first `D3D12CreateDevice` and is process-wide once it has: a second
/// instance opening later must not re-enable it, and must not log a second time
/// about it either.
#[cfg(target_os = "windows")]
static DEBUG_LAYER: OnceLock<bool> = OnceLock::new();

/// Turns the D3D12 debug layer on, if this build wants it and Windows has it.
///
/// **Call this before the first `D3D12CreateDevice`.** The layer is chosen per
/// device at creation, so enabling it afterwards leaves every device already
/// open unvalidated. [`crate::Dx12Instance`]'s `open` is where that ordering is
/// discharged, because it is the first thing in this crate to create a device.
///
/// Returns whether the layer is on. A layer that was asked for and is missing
/// is a **warning, not a failure**: `Graphics Tools` is an optional Windows
/// feature and a machine without it must still be able to run the engine.
#[cfg(target_os = "windows")]
pub(crate) fn enable_debug_layer() -> bool {
    *DEBUG_LAYER.get_or_init(|| {
        if !validation_wanted() {
            crcbl_core::log::debug!(
                "crcbl-dx12: the D3D12 debug layer is off; set {VALIDATION_ENV_VAR}=1 to turn it on"
            );
            return false;
        }
        let mut debug: Option<ID3D12Debug> = None;
        // SAFETY: `debug` is a live out-parameter the call writes an interface
        // through, and `ID3D12Debug` is the IID it is asked for — so the QI
        // either succeeds or the call fails, and a failure leaves it `None`,
        // which is why it is read back rather than assumed.
        if let Err(error) = unsafe { D3D12GetDebugInterface(&mut debug) } {
            crcbl_core::log::warn!(
                "crcbl-dx12: {VALIDATION_ENV_VAR} asked for the debug layer and this machine does \
                 not have it ({error}) — install the Graphics Tools optional feature. Validation \
                 errors will keep arriving one call late."
            );
            return false;
        }
        let Some(debug) = debug else {
            crcbl_core::log::warn!(
                "crcbl-dx12: D3D12GetDebugInterface reported success and wrote no layer"
            );
            return false;
        };
        // SAFETY: `debug` is the interface the call just returned. The method
        // takes nothing and returns nothing; the layer stays on for the process
        // after this interface is dropped, which is what makes it sound to drop
        // it here.
        unsafe { debug.EnableDebugLayer() };
        crcbl_core::log::info!("crcbl-dx12: the D3D12 debug layer is on");
        true
    })
}

/// Whether [`enable_debug_layer`] found the layer. `false` before it has run.
#[cfg(target_os = "windows")]
fn debug_layer_on() -> bool {
    DEBUG_LAYER.get().copied().unwrap_or(false)
}

/// Reports whether the info queue behind the debug layer is reachable on a
/// freshly created device, files the storage filter, and clears whatever the
/// queue already holds.
///
/// The clear is what makes a later [`diagnosis`] mean "since this device was
/// created" rather than "since the process started". The log line is the half
/// `crcbl_vk::debug`'s `ValidationReport::enabled` exists for: a report that is
/// silent because nothing was checked is a very different thing from one that
/// is silent because nothing was wrong, and only this line tells them apart.
#[cfg(target_os = "windows")]
pub(crate) fn attach(device: &ID3D12Device) {
    let Ok(queue) = device.cast::<ID3D12InfoQueue>() else {
        if debug_layer_on() {
            crcbl_core::log::warn!(
                "crcbl-dx12: the debug layer is on and this device has no ID3D12InfoQueue, so no \
                 validation message can be read back"
            );
        }
        return;
    };
    // **The queue is told not to keep chatter, and that is what keeps it
    // useful.** The layer emits an `INFO` message per object created and
    // destroyed, so a device doing ordinary work fills the queue's
    // message-count limit and D3D12 then discards *new* messages — which are
    // the ones a failure is about. Denying them at the storage filter is the
    // only place that stops it; skipping them at the read leaves the overflow.
    let mut denied = [D3D12_MESSAGE_SEVERITY_INFO, D3D12_MESSAGE_SEVERITY_MESSAGE];
    let filter = D3D12_INFO_QUEUE_FILTER {
        AllowList: D3D12_INFO_QUEUE_FILTER_DESC::default(),
        DenyList: D3D12_INFO_QUEUE_FILTER_DESC {
            NumSeverities: denied
                .len()
                .try_into()
                .unwrap_or_else(|_| unreachable!("two severities fit in a u32")),
            // The binding spells the list `*mut` because D3D12's header does;
            // the call reads it and never writes through it.
            pSeverityList: denied.as_mut_ptr(),
            ..D3D12_INFO_QUEUE_FILTER_DESC::default()
        },
    };
    // A failure costs the filter, not the queue: without it errors are still
    // stored, the queue simply fills faster.
    //
    // SAFETY: `queue` is the interface the QI above returned, and `filter` is a
    // fully initialised local whose severity list points at `denied`, which
    // outlives the call. `PushStorageFilter` copies the filter it is given, so
    // neither has to outlive it.
    if let Err(error) = unsafe { queue.PushStorageFilter(&raw const filter) } {
        crcbl_core::log::debug!(
            "crcbl-dx12: the info queue would not take a storage filter: {error}"
        );
    }
    // SAFETY: as above. The call takes nothing and returns nothing.
    unsafe { queue.ClearStoredMessages() };
    crcbl_core::log::info!("crcbl-dx12: this device's validation messages are readable");
}

/// The reason D3D12 gives for a device having been removed, or `None` while it
/// is healthy.
///
/// `GetDeviceRemovedReason` answers `S_OK` on a live device, which
/// `windows-rs` spells as `Ok(())` — so the `Some` arm here is exactly "this
/// device is gone".
#[cfg(target_os = "windows")]
fn removed_reason(device: &ID3D12Device) -> Option<windows::core::Error> {
    // SAFETY: `device` is a live `ID3D12Device` this crate owns a reference to.
    // The call reads no pointer of ours and returns an `HRESULT` by value.
    unsafe { device.GetDeviceRemovedReason() }.err()
}

/// The name D3D12 documentation uses for a removal reason, and what it means.
///
/// Spelled out rather than left as a code, because the code is what a reader
/// already has and cannot act on. An `if` chain rather than a `match`: these
/// are associated constants of a wrapper type, not patterns.
#[cfg(target_os = "windows")]
fn reason_name(code: HRESULT) -> &'static str {
    if code == DXGI_ERROR_DEVICE_HUNG {
        "DXGI_ERROR_DEVICE_HUNG — the application's own command list hung the \
         adapter; an infinite shader loop or a bad indirect argument"
    } else if code == DXGI_ERROR_DEVICE_REMOVED {
        "DXGI_ERROR_DEVICE_REMOVED — the adapter itself went away; a driver \
         update, a disconnected external GPU, or a reset the driver could not \
         recover this device from"
    } else if code == DXGI_ERROR_DEVICE_RESET {
        "DXGI_ERROR_DEVICE_RESET — the adapter was reset by something other \
         than this application"
    } else if code == DXGI_ERROR_DRIVER_INTERNAL_ERROR {
        "DXGI_ERROR_DRIVER_INTERNAL_ERROR — the driver hit a fault of its own"
    } else if code == DXGI_ERROR_INVALID_CALL {
        "DXGI_ERROR_INVALID_CALL — an earlier call from this application was \
         invalid and the runtime took the device down for it"
    } else {
        "an HRESULT D3D12 does not document as a removal reason"
    }
}

/// Everything the debug layer has stored against this device, read without
/// taking it.
///
/// **Nothing is cleared.** Two callers read this queue — [`report`] at a test's
/// teardown and [`diagnosis`] inside a failure's own message — and a read that
/// consumed the messages would leave whichever ran second looking at a clean
/// device. The module docs argue it; the cost is that a second `diagnosis`
/// repeats the first one's lines, which is worth far less than a report that
/// silently lost them.
#[cfg(target_os = "windows")]
fn read_queue(queue: &ID3D12InfoQueue) -> ValidationReport {
    // SAFETY: `queue` is a live `ID3D12InfoQueue` the caller holds a reference
    // to. Every call in this function is on it, reads no pointer of ours except
    // the message buffer allocated below, and takes scalars otherwise.
    let stored = unsafe { queue.GetNumStoredMessages() };
    let mut report = ValidationReport {
        enabled: debug_layer_on(),
        ..ValidationReport::default()
    };
    for index in 0..stored {
        let mut bytes: usize = 0;
        // The two-call idiom the API documents: `None` asks how large the
        // record is, because a message is a header followed by its description
        // in one allocation and the length is not knowable in advance.
        //
        // SAFETY: as above. `bytes` is a live out-parameter.
        if unsafe { queue.GetMessage(index, None, &mut bytes) }.is_err()
            || bytes < size_of::<D3D12_MESSAGE>()
        {
            continue;
        }
        // A `u64` buffer rather than a `u8` one, so the storage is aligned for
        // the header that is about to be written into it. `D3D12_MESSAGE`
        // carries a pointer, so its alignment is the pointer's.
        let mut storage: Vec<u64> = vec![0; bytes.div_ceil(size_of::<u64>())];
        let record = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        // SAFETY: as above, plus `record` points at `storage`, which is at
        // least `bytes` long and aligned for a `D3D12_MESSAGE` — the size the
        // call itself just asked for.
        if unsafe { queue.GetMessage(index, Some(record), &mut bytes) }.is_err() {
            continue;
        }
        // SAFETY: the call above returned success, so it wrote a whole
        // `D3D12_MESSAGE` at `record`, and `storage` outlives this borrow.
        let record = unsafe { &*record };
        let text = if record.pDescription.is_null() || record.DescriptionByteLength == 0 {
            String::new()
        } else {
            // SAFETY: the runtime wrote the description into the same
            // allocation and reported its length; the slice is read before
            // `storage` is dropped.
            let description = unsafe {
                core::slice::from_raw_parts(record.pDescription, record.DescriptionByteLength)
            };
            // The length includes the terminator, which is not part of the text.
            let description = description.strip_suffix(b"\0").unwrap_or(description);
            String::from_utf8_lossy(description).into_owned()
        };
        report.record(ValidationMessage {
            severity: severity_of(record.Severity),
            id: record.ID.0,
            text,
        });
    }
    report
}

/// This crate's word for a `D3D12_MESSAGE_SEVERITY`.
///
/// An `if` chain rather than a `match`: these are associated constants of a
/// wrapper type, not patterns.
#[cfg(target_os = "windows")]
fn severity_of(severity: D3D12_MESSAGE_SEVERITY) -> Severity {
    if severity == D3D12_MESSAGE_SEVERITY_CORRUPTION {
        Severity::Corruption
    } else if severity == D3D12_MESSAGE_SEVERITY_ERROR {
        Severity::Error
    } else if severity == D3D12_MESSAGE_SEVERITY_WARNING {
        Severity::Warning
    } else if severity == D3D12_MESSAGE_SEVERITY_INFO {
        Severity::Info
    } else if severity == D3D12_MESSAGE_SEVERITY_MESSAGE {
        Severity::Message
    } else {
        Severity::Unknown(severity.0)
    }
}

/// What the debug layer has said about this device, ready to assert on.
///
/// A device whose messages cannot be read at all — the layer is off, or Windows
/// has no *Graphics Tools* feature, so the `ID3D12InfoQueue` query fails —
/// reports `enabled: false` and no messages, which
/// [`ValidationReport::assert_clean`] treats as the failure it is rather than as
/// a clean run.
#[cfg(target_os = "windows")]
pub(crate) fn report(device: &ID3D12Device) -> ValidationReport {
    device
        .cast::<ID3D12InfoQueue>()
        .map_or_else(|_| ValidationReport::default(), |queue| read_queue(&queue))
}

/// Everything this backend can say about *why* a call failed, ready to append
/// to the failure's own message.
///
/// The empty string when the device is healthy and the layer has nothing to
/// report, so an ordinary refusal — a descriptor D3D12 will not take — reads
/// exactly as it did before. What it is for is the other case: a removed
/// device, where the failing call is not the offending one and the message
/// without this is a code and no diagnosis.
///
/// **It takes nothing.** An earlier version cleared the info queue so that a
/// failure carried its own messages and not the next one's, which is a fine
/// property right up until something else reads the same queue — and
/// [`ValidationReport::assert_clean`] now does. See the module docs.
#[cfg(target_os = "windows")]
pub(crate) fn diagnosis(device: &ID3D12Device) -> String {
    let reason = removed_reason(device);
    let layer = report(device);
    if reason.is_none() && layer.messages.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    if let Some(reason) = reason {
        let code = reason.code();
        lines.push(format!(
            "GetDeviceRemovedReason: {} ({:#010X})",
            reason_name(code),
            code.0.cast_unsigned()
        ));
        // **Only on a removed device.** DRED's two queries answer
        // `DXGI_ERROR_NOT_CURRENTLY_AVAILABLE` on a healthy one, so asking
        // unconditionally would put a "nothing to report" line on every
        // ordinary refusal — a descriptor D3D12 will not take — which is
        // precisely the case this module already returns the empty string for.
        lines.extend(crate::dred::diagnosis(device));
    }
    lines.extend(layer.messages.iter().map(ToString::to_string));
    if !layer.is_clean() {
        // The counts, because the quoted lines are capped and the cap is
        // silent: "36 error(s)" beside 32 messages is the difference between a
        // list that is the whole story and one that is the start of it.
        lines.push(format!(
            "the debug layer holds {} corruption, {} error(s) and {} warning(s) for this device, \
             plus {} allowed by name in crcbl_dx12::debug::ALLOWED",
            layer.corruption, layer.errors, layer.warnings, layer.allowed
        ));
    }
    if !layer.enabled {
        // Not `debug_layer_on()`: this also covers the layer being on while
        // this particular device has no `ID3D12InfoQueue`, which reads to a
        // caller exactly like a device that had nothing to say.
        lines.push(format!(
            "the D3D12 debug layer is not on, so nothing here names the call that did this — \
             re-run with {VALIDATION_ENV_VAR}=1 on a machine with the Graphics Tools optional \
             feature"
        ));
    }
    format!("\n  {}", lines.join("\n  "))
}

/// A device test's instance, which fails the test at teardown unless the debug
/// layer was on and silent.
///
/// **This is where "zero validation errors" stops being an aspiration for this
/// backend.** `crcbl-vk`'s suite ends every test in `Headless::finish`, which
/// asserts a clean report; this crate's device tests are ordinary `#[test]`
/// functions in `src/`, all of them opening through
/// `crate::device::tests::open_device`, and there is no line they all run. So
/// the assertion is hung on the one thing they all do do — drop what
/// `open_device` returned.
///
/// # Why it holds the info queue rather than the device
///
/// The messages live on the `ID3D12Device`, so they cannot be read after that
/// object is gone — where `crcbl-vk` reads its report from the *instance* and
/// can therefore drop the device first. Keeping an `ID3D12InfoQueue` reference
/// here keeps the D3D12 device object alive by itself, so the report still
/// answers after `Dx12Device` has released everything it owns. That is strictly
/// more than Vulkan's version sees: anything the layer says *about the teardown*
/// is in this report too.
///
/// A `let (_instance, device) = open_device();` drops `device` first — locals go
/// in reverse declaration order, and a tuple pattern declares in order — so by
/// the time this runs the resources really are released.
#[cfg(all(test, target_os = "windows"))]
pub(crate) struct Validated {
    instance: crate::Dx12Instance,
    /// `None` when the debug layer is off or Windows has no *Graphics Tools*
    /// feature, which is the case [`ValidationReport::assert_clean`] refuses.
    queue: Option<ID3D12InfoQueue>,
    /// Cleared by [`Validated::defuse`] for the tests whose report is dirty on
    /// purpose.
    armed: core::cell::Cell<bool>,
}

#[cfg(all(test, target_os = "windows"))]
impl Validated {
    /// Wraps the instance a device was opened on, capturing that device's info
    /// queue.
    pub(crate) fn new(instance: crate::Dx12Instance, device: &ID3D12Device) -> Self {
        Self {
            instance,
            queue: device.cast::<ID3D12InfoQueue>().ok(),
            armed: core::cell::Cell::new(true),
        }
    }

    /// What the layer has said so far.
    pub(crate) fn report(&self) -> ValidationReport {
        self.queue
            .as_ref()
            .map_or_else(ValidationReport::default, read_queue)
    }

    /// Says that this test's report is dirty on purpose, so the teardown
    /// assertion below stands down.
    ///
    /// The counterpart of the `crcbl-vk` gate tests that deliberately do not
    /// call `Headless::finish`. Only the two validation-gate tests want it.
    pub(crate) fn defuse(&self) {
        self.armed.set(false);
    }
}

#[cfg(all(test, target_os = "windows"))]
impl core::ops::Deref for Validated {
    type Target = crate::Dx12Instance;

    /// So that a test written against `Dx12Instance` needs no edit to gain the
    /// assertion — `instance.create_surface(…)` and `pinned_adapter(&instance)`
    /// both still resolve.
    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}

#[cfg(all(test, target_os = "windows"))]
impl Drop for Validated {
    fn drop(&mut self) {
        if !self.armed.get() {
            return;
        }
        // A panic raised while another is unwinding aborts the process, taking
        // the real failure's message with it. The test has already failed and
        // has a better message than this one would add.
        if std::thread::panicking() {
            return;
        }
        // Nothing was asked for, so there is nothing to assert and nothing to
        // claim. The harness's `debug layer=` line is what says the run checked
        // no validation.
        if !validation_wanted() {
            return;
        }
        self.report().assert_clean();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The spellings a person actually types, and the ones that mean "no
    /// opinion".**
    ///
    /// Red if `off` or `no` starts meaning enabled, or if an unset variable
    /// starts meaning disabled rather than deferring to the build profile.
    #[test]
    fn the_validation_flag_reads_the_spellings_people_type() {
        for off in ["0", "false", "no", "off", "OFF", " False "] {
            assert_eq!(parse_flag(off), Some(false), "{off:?}");
        }
        for on in ["1", "true", "yes", "on", "ON", " anything "] {
            assert_eq!(parse_flag(on), Some(true), "{on:?}");
        }
        assert_eq!(parse_flag(""), None);
        assert_eq!(parse_flag("   "), None);
    }

    /// **The default follows the build profile, and the variable beats it both
    /// ways.**
    ///
    /// Asserted through the pure half rather than by setting the environment,
    /// because a test that set [`VALIDATION_ENV_VAR`] would set it for every
    /// other test in this binary — including the ones that open a device.
    #[test]
    fn validation_defaults_to_the_build_profile_and_the_variable_overrides_it() {
        assert_eq!(
            validation_policy(None),
            cfg!(debug_assertions),
            "an unset {VALIDATION_ENV_VAR} means 'on in debug, off in release'"
        );
        assert!(validation_policy(Some(true)));
        assert!(!validation_policy(Some(false)));
    }

    /// **The line `tests/run-dx12-e2e.sh` reads the layer's verdict off, and a
    /// freshly opened device that has not already been removed.**
    ///
    /// Two things nothing else in this crate can say:
    ///
    /// * **Whether validation was actually on.** A suite that passed because
    ///   the layer was missing proves nothing about validation, and that
    ///   failure mode is invisible without a line saying so — the trap
    ///   `crcbl_vk::debug`'s `ValidationReport::enabled` exists for, and
    ///   `docs/plan/12-testing.md` names for e2e jobs generally. The harness
    ///   fails when this line is absent, so the check cannot be lost by
    ///   renaming the test.
    /// * **That `GetDeviceRemovedReason` answers.** It is asserted on a device
    ///   created moments earlier, so a healthy `S_OK` is the only right answer
    ///   and a machine where the call itself is broken says so here rather than
    ///   inside a failure it was meant to explain.
    ///
    /// nextest captures a passing test's stdout, so read it with
    /// `--success-output immediate` — which is what the harness passes.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_fresh_device_says_whether_it_is_validated_and_is_not_already_removed() {
        let (_instance, device) = crate::device::tests::open_device();
        let raw = device.raw();
        let readable = raw.cast::<ID3D12InfoQueue>().is_ok();
        println!(
            "crcbl-dx12 e2e: debug layer={} messages readable={readable}",
            debug_layer_on()
        );
        assert!(
            removed_reason(raw).is_none(),
            "a device that was just created is already gone:{}",
            diagnosis(raw)
        );
    }

    fn message(severity: Severity, id: i32) -> ValidationMessage {
        ValidationMessage {
            severity,
            id,
            text: "something happened".to_string(),
        }
    }

    fn recorded(messages: &[ValidationMessage], enabled: bool) -> ValidationReport {
        let mut report = ValidationReport {
            enabled,
            ..ValidationReport::default()
        };
        for message in messages {
            report.record(message.clone());
        }
        report
    }

    /// **Which severities fail a test, and which are chatter.**
    ///
    /// The line is `crcbl-vk`'s — zero errors *and* zero warnings — so a
    /// warning failing is the assertion, not an accident. An unrecognised
    /// severity fails too: a grading this build has no name for is not
    /// something to wave through.
    #[test]
    fn a_warning_fails_a_test_and_the_layers_own_chatter_does_not() {
        assert!(Severity::Corruption.is_failure());
        assert!(Severity::Error.is_failure());
        assert!(
            Severity::Warning.is_failure(),
            "the line this backend is held to is zero errors AND zero warnings"
        );
        assert!(Severity::Unknown(9).is_failure());
        assert!(!Severity::Info.is_failure());
        assert!(!Severity::Message.is_failure());

        // Each severity prints as its own word, or two gradings would be
        // indistinguishable in the one place anybody reads them.
        let mut words: Vec<String> = [
            Severity::Message,
            Severity::Info,
            Severity::Warning,
            Severity::Error,
            Severity::Corruption,
            Severity::Unknown(9),
        ]
        .iter()
        .map(|severity| severity.as_str())
        .collect();
        let named = words.len();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), named, "two severities print alike: {words:?}");
    }

    /// **A report counts every failure and keeps chatter out of the messages.**
    #[test]
    fn the_report_counts_what_matters_and_drops_what_does_not() {
        let report = recorded(
            &[
                message(Severity::Info, 1),
                message(Severity::Message, 2),
                message(Severity::Warning, 3),
                message(Severity::Error, 4),
                message(Severity::Corruption, 5),
            ],
            true,
        );
        assert_eq!(
            (report.corruption, report.errors, report.warnings),
            (1, 1, 1)
        );
        assert_eq!(report.messages.len(), 3, "chatter is not kept");
        assert!(!report.is_clean());
        assert!(
            report.summary().contains("[CORRUPTION] id 5"),
            "{}",
            report.summary()
        );

        assert!(recorded(&[message(Severity::Info, 1)], true).is_clean());
    }

    /// **A message storm cannot exhaust memory and cannot make the counts
    /// lie.** The cap is on what is quoted, never on what is counted.
    #[test]
    fn kept_messages_are_capped_but_the_counts_are_exact() {
        let storm: Vec<ValidationMessage> = (0..MAX_KEPT_MESSAGES + 50)
            .map(|index| {
                message(
                    Severity::Error,
                    i32::try_from(index).expect("the storm fits in an i32"),
                )
            })
            .collect();
        let report = recorded(&storm, true);
        assert_eq!(report.messages.len(), MAX_KEPT_MESSAGES);
        assert_eq!(report.errors as usize, storm.len());
    }

    /// **The trap this whole thing exists to close.** A clean report from a
    /// device whose layer was never on proves nothing, and must fail rather
    /// than pass quietly — the same assertion `crcbl_vk::debug` makes, and the
    /// reason its wording names the way out.
    #[test]
    #[should_panic(expected = "the D3D12 debug layer was not on")]
    fn a_clean_report_from_a_layer_that_was_never_on_still_fails() {
        ValidationReport::default().assert_clean();
    }

    #[test]
    #[should_panic(expected = "1 error(s)")]
    fn assert_clean_names_the_counts() {
        recorded(&[message(Severity::Error, 599)], true).assert_clean();
    }

    #[test]
    fn a_silent_layer_that_was_on_passes() {
        recorded(&[], true).assert_clean();
    }

    /// **An allow-listed id is passed over at `WARNING` and at nothing else.**
    ///
    /// The second half is the one that keeps the table from being a suppression:
    /// the debug layer files more than one condition under
    /// [`CLEAR_VALUE_NOT_GIVEN`] — the advisory this crate has an answer for,
    /// and `MISMATCHINGCLEARVALUE`, which it does not — so an id that stopped
    /// failing whatever its severity would be the second one going quiet too.
    #[test]
    fn an_allow_listed_warning_is_passed_over_and_a_louder_one_is_not() {
        let passed = recorded(&[message(Severity::Warning, CLEAR_VALUE_NOT_GIVEN)], true);
        assert!(passed.is_clean(), "{}", passed.summary());
        assert_eq!(passed.allowed, 1, "an allowed message is counted, not lost");
        assert!(passed.messages.is_empty());
        passed.assert_clean();

        for severity in [Severity::Error, Severity::Corruption, Severity::Unknown(9)] {
            let report = recorded(&[message(severity, CLEAR_VALUE_NOT_GIVEN)], true);
            assert!(
                !report.is_clean(),
                "{severity:?} {CLEAR_VALUE_NOT_GIVEN} was waved through by an allow-list that \
                 only covers warnings"
            );
            assert_eq!(report.allowed, 0);
        }

        // And an id nobody argued for still fails at the severity the table
        // does cover, or the table would be a severity filter wearing a list.
        let unlisted = recorded(&[message(Severity::Warning, 1)], true);
        assert!(!unlisted.is_clean());
    }

    /// **Every entry in [`ALLOWED`] carries an argument, and no id is listed
    /// twice.**
    ///
    /// An empty reason is the shape this table fails as: a number added in a
    /// hurry reads exactly like a settled one, and [`allowance`] would find it
    /// either way. A duplicate id is the other: two arguments for one number
    /// means one of them is dead and nobody can tell which.
    #[test]
    fn every_allowed_message_carries_the_argument_for_it() {
        /// Shorter than this is a label, not an argument: the entries here name
        /// the layer's own wording, what the fix would be, and why it is not
        /// taken, and none of that fits in a sentence.
        const MIN_REASON_CHARS: usize = 200;

        assert!(!ALLOWED.is_empty(), "an empty table has nothing to check");
        let mut ids: Vec<i32> = Vec::with_capacity(ALLOWED.len());
        for allowed in ALLOWED {
            assert!(
                allowed.reason.len() >= MIN_REASON_CHARS,
                "message id {} is allowed with no argument for it: {:?}",
                allowed.id,
                allowed.reason
            );
            assert_eq!(
                allowance(Severity::Warning, allowed.id),
                Some(allowed.reason),
                "message id {} is in the table and not found in it",
                allowed.id
            );
            ids.push(allowed.id);
        }
        let listed = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), listed, "an id is allowed twice: {ids:?}");
    }

    /// A buffer two rows tall, which D3D12 refuses and the debug layer objects
    /// to by name.
    ///
    /// **Made against the raw `ID3D12Device` on purpose.** `crcbl-vk`'s twin
    /// provokes its layer through the seam, because that backend records an
    /// out-of-bounds copy and lets the layer catch it; this one cannot, because
    /// `crate::command`'s `copy_buffer_to_buffer` bounds-checks the same copy
    /// and refuses it as `HalError::InvalidDescriptor` before D3D12 is called
    /// at all. Every violation the seam can express is refused that way, which
    /// is a good property of the backend and leaves the gate no route but this
    /// one.
    ///
    /// It is a *creation* call, so nothing is ever recorded, submitted or
    /// executed: the runtime refuses it with `E_INVALIDARG`, the layer files a
    /// message, and the device is untouched. `crcbl-vk`'s gate takes the same
    /// care — a specification violation is undefined behaviour, and a software
    /// rasteriser is under no obligation to survive one.
    #[cfg(target_os = "windows")]
    fn provoke(device: &ID3D12Device) -> windows::core::Result<()> {
        use windows::Win32::Graphics::Direct3D12::{
            D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES,
            D3D12_HEAP_TYPE_DEFAULT, D3D12_MEMORY_POOL_UNKNOWN, D3D12_RESOURCE_DESC,
            D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON,
            D3D12_TEXTURE_LAYOUT_ROW_MAJOR, ID3D12Resource,
        };
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

        let properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: 256,
            // **The violation, and the whole of it.** A `BUFFER` is
            // one-dimensional and D3D12 requires its `Height` to be 1.
            Height: 2,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: both descriptors are live, fully initialised locals borrowed
        // for the call, and `resource` is a live `Option` the call writes
        // through. The call is expected to fail and to write nothing, which is
        // the whole point; a `None` afterwards is therefore not a surprise.
        unsafe {
            device.CreateCommittedResource(
                &properties,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )
        }
    }

    /// **The gate underneath every other device test in this crate: proof the
    /// debug layer is wired, not merely quiet.**
    ///
    /// Every one of them now ends in [`Validated`]'s `Drop`, which asserts a
    /// clean report — and every one of them would pass just as happily against
    /// a layer that reported into a queue nobody reads, which is the failure
    /// mode that turns "zero validation errors" into a slogan. So this commits
    /// a deliberate violation and asserts the layer *noticed*.
    ///
    /// The report is dirty by construction, so the teardown assertion is stood
    /// down at the end — `crcbl-vk`'s gate skips `Headless::finish` for exactly
    /// the same reason.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_deliberate_violation_is_caught_by_the_layer() {
        let (instance, device) = crate::device::tests::open_device();
        let refused = provoke(device.raw())
            .expect_err("D3D12 does not create a buffer that is two rows tall");

        let report = instance.report();
        assert!(
            report.enabled,
            "this suite is meaningless without the debug layer: {refused}"
        );
        assert!(
            !report.is_clean(),
            "the layer must have objected to a two-row buffer; if it did not, the clean report \
             every other device test rests on is proving nothing"
        );
        assert!(
            report
                .messages
                .iter()
                .any(|message| message.severity.is_failure()
                    && message.text.contains("CreateCommittedResource")),
            "the message should name the call that did it:\n{}",
            report.summary()
        );

        instance.defuse();
    }

    /// **The other half: the teardown assertion actually fires.**
    ///
    /// `a_deliberate_violation_is_caught_by_the_layer` proves the layer sees
    /// the violation and [`Validated::report`] can read it. It does not prove
    /// that a test which *ignores* a dirty report fails — and since every other
    /// device test in this crate relies on precisely that, it is asserted here
    /// by committing the same violation and declining to stand the guard down.
    ///
    /// The panic arrives from [`Validated`]'s `Drop`, at the end of the
    /// function rather than inside it, which is a normal return path and so is
    /// caught by `should_panic` like any other.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    #[should_panic(expected = "the D3D12 debug layer reported")]
    fn a_dirty_report_fails_a_test_that_says_nothing_about_it() {
        let (_instance, device) = crate::device::tests::open_device();
        provoke(device.raw()).expect_err("D3D12 does not create a buffer that is two rows tall");
    }
}
