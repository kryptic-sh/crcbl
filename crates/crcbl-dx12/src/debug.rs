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
//! # The layer's output does not reach a CI log on its own
//!
//! `ID3D12Debug::EnableDebugLayer` sends its messages to the **debugger** —
//! `OutputDebugString` — and a GitHub runner has no debugger attached, so a
//! job that only enabled the layer would produce exactly the same log as one
//! that did not. [`drain_messages`] is the half that makes it readable: the
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

#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D12::{
    D3D12_INFO_QUEUE_FILTER, D3D12_INFO_QUEUE_FILTER_DESC, D3D12_MESSAGE, D3D12_MESSAGE_SEVERITY,
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

/// Messages pulled out of the info queue for one report.
///
/// A cap rather than the whole queue: a device that has gone wrong emits the
/// same validation error once per call, and a panic message carrying hundreds
/// of copies is one nobody reads. The first are the ones nearest the cause.
#[cfg(target_os = "windows")]
const MAX_DRAINED_MESSAGES: u64 = 32;

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
            log::debug!(
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
            log::warn!(
                "crcbl-dx12: {VALIDATION_ENV_VAR} asked for the debug layer and this machine does \
                 not have it ({error}) — install the Graphics Tools optional feature. Validation \
                 errors will keep arriving one call late."
            );
            return false;
        }
        let Some(debug) = debug else {
            log::warn!("crcbl-dx12: D3D12GetDebugInterface reported success and wrote no layer");
            return false;
        };
        // SAFETY: `debug` is the interface the call just returned. The method
        // takes nothing and returns nothing; the layer stays on for the process
        // after this interface is dropped, which is what makes it sound to drop
        // it here.
        unsafe { debug.EnableDebugLayer() };
        log::info!("crcbl-dx12: the D3D12 debug layer is on");
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
            log::warn!(
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
        log::debug!("crcbl-dx12: the info queue would not take a storage filter: {error}");
    }
    // SAFETY: as above. The call takes nothing and returns nothing.
    unsafe { queue.ClearStoredMessages() };
    log::info!("crcbl-dx12: this device's validation messages are readable");
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

/// Every message the debug layer has stored for this device, drained.
///
/// Empty when the layer is off, when Windows does not have it, or when nothing
/// has gone wrong — three states this cannot tell apart, which is why
/// [`diagnosis`] says which one applies rather than leaving a reader to guess
/// from an empty list.
#[cfg(target_os = "windows")]
fn drain_messages(device: &ID3D12Device) -> Vec<String> {
    let Ok(queue) = device.cast::<ID3D12InfoQueue>() else {
        return Vec::new();
    };
    // SAFETY: `queue` is the interface the QI above returned. Every call in
    // this function is on it, reads no pointer of ours except the message
    // buffer allocated below, and takes scalars otherwise.
    let stored = unsafe { queue.GetNumStoredMessages() };
    let mut messages = Vec::new();
    for index in 0..stored.min(MAX_DRAINED_MESSAGES) {
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
        messages.push(format!(
            "[{}] id {}: {text}",
            severity_name(record.Severity),
            record.ID.0
        ));
    }
    // SAFETY: as above. Draining is what makes the next report about the next
    // failure rather than about every failure so far.
    unsafe { queue.ClearStoredMessages() };
    messages
}

/// How the debug layer graded a message.
#[cfg(target_os = "windows")]
fn severity_name(severity: D3D12_MESSAGE_SEVERITY) -> &'static str {
    if severity == D3D12_MESSAGE_SEVERITY_CORRUPTION {
        "CORRUPTION"
    } else if severity == D3D12_MESSAGE_SEVERITY_ERROR {
        "ERROR"
    } else if severity == D3D12_MESSAGE_SEVERITY_WARNING {
        "WARNING"
    } else if severity == D3D12_MESSAGE_SEVERITY_INFO {
        "INFO"
    } else if severity == D3D12_MESSAGE_SEVERITY_MESSAGE {
        "MESSAGE"
    } else {
        "UNKNOWN"
    }
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
/// Draining the layer's messages is a side effect, and deliberately so: they
/// belong to the failure being reported, and leaving them queued would attach
/// them to the *next* one instead.
#[cfg(target_os = "windows")]
pub(crate) fn diagnosis(device: &ID3D12Device) -> String {
    let reason = removed_reason(device);
    let messages = drain_messages(device);
    if reason.is_none() && messages.is_empty() {
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
    }
    lines.extend(messages);
    if !debug_layer_on() {
        lines.push(format!(
            "the D3D12 debug layer is not on, so nothing here names the call that did this — \
             re-run with {VALIDATION_ENV_VAR}=1 on a machine with the Graphics Tools optional \
             feature"
        ));
    }
    format!("\n  {}", lines.join("\n  "))
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
}
