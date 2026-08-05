//! What a failed `MTLCommandBuffer` is made to say, and the descriptor that
//! makes it able to say it.
//!
//! # A GPU fault that names no encoder is the failure mode this crate keeps
//! closing
//!
//! When the GPU faults, Metal fails the command buffer and hands back an
//! `NSError` whose text is one sentence — `Caused GPU Hang Error
//! (00000003:kIOGPUCommandBufferCallbackErrorHang)` and nothing else. A command
//! buffer holds several encoders, so that sentence says the submission died
//! without saying *where*, and the difference between "the render pass faulted"
//! and "the blit after it never started" is the whole of the investigation.
//! `crcbl_mtl::device`'s wait-before-signal refusal exists for the same class of
//! failure: the process is alive, the work is gone, and nothing is in any log.
//!
//! Metal's own answer is
//! [`MTLCommandBufferErrorOption::EncoderExecutionStatus`], set on the
//! `MTLCommandBufferDescriptor` a command buffer is created from. With it, a
//! failed command buffer's `NSError` carries an
//! `MTLCommandBufferEncoderInfoErrorKey` entry: one
//! [`MTLCommandBufferEncoderInfo`] per encoder, **in recorded order**, each with
//! the encoder's label, the debug signposts inserted into it, and an
//! [`MTLCommandEncoderErrorState`] — `Faulted` for the encoder that caused the
//! error, `Affected` for one caught up in it, `Pending` for one that never
//! started, `Completed` for one that finished.
//!
//! So [`command_buffer`] is the only way this backend creates one, and
//! [`describe`] is the only way it reads a failure back. Apple notes the option
//! "may increase CPU, GPU, and/or memory overhead on some platforms"; it is
//! taken unconditionally anyway, because a backend that can only be debugged
//! after someone rebuilds it with a flag is one nobody debugs from a CI log —
//! which is the only place a GPU fault on another machine is ever seen.
//!
//! **Every encoder is labelled**, and that is part of this and not decoration:
//! an encoder with no label reports as an empty string here, which turns the
//! whole diagnostic back into "one of them faulted".

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, AnyProtocol, ProtocolObject};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferDescriptor, MTLCommandBufferEncoderInfo,
    MTLCommandBufferEncoderInfoErrorKey, MTLCommandBufferErrorOption, MTLCommandEncoderErrorState,
    MTLCommandQueue, MTLDevice as _,
};

/// A labelled command buffer that reports per-encoder status when it fails.
///
/// `None` only when Metal returned nil, which is the caller's to report — the
/// two call sites word it differently and both are already saying it.
pub(crate) fn command_buffer(
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    label: &str,
) -> Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
    let descriptor = MTLCommandBufferDescriptor::new();
    descriptor.setErrorOptions(MTLCommandBufferErrorOption::EncoderExecutionStatus);
    // `retainedReferences` is left at its default of `true`: an unretained
    // command buffer requires the caller to keep every resource it touches
    // alive itself, and the seam's handles are destroyed by whoever holds them.
    let raw = queue.commandBufferWithDescriptor(&descriptor)?;
    raw.setLabel(Some(&NSString::from_str(label)));
    Some(raw)
}

/// Why a command buffer failed, with each of its encoders' fate beside it.
///
/// Called only on a command buffer whose `status` is
/// [`MTLCommandBufferStatus::Error`](objc2_metal::MTLCommandBufferStatus::Error),
/// where Metal guarantees an `NSError`; the "no reason given" arm is the
/// defensive one and should never be reached.
pub(crate) fn describe(command_buffer: &ProtocolObject<dyn MTLCommandBuffer>) -> String {
    let Some(error) = command_buffer.error() else {
        return "no reason given".to_string();
    };
    // The domain and code are carried alongside the localised text because they
    // are the machine-readable half: `MTLCommandBufferErrorTimeout` and
    // `MTLCommandBufferErrorPageFault` are different bugs with similar prose.
    //
    // The `MTLDevice`'s own name is here for a reason a local reader will not
    // feel: a fault report is read from a CI log, on a machine nobody can open,
    // and "which GPU" is the first question a fault that reproduces nowhere
    // else raises. A virtualised device answers it in one word.
    let mut out = format!(
        "{} [{} {}] on `{}`",
        error.localizedDescription(),
        error.domain(),
        error.code(),
        command_buffer.device().name()
    );
    match encoders(&error) {
        Some(encoders) if !encoders.is_empty() => {
            out.push_str("; encoders in recorded order: ");
            out.push_str(&encoders.join(", "));
        }
        // Not silence: "Metal named no encoder" and "this backend forgot to ask
        // for them" look identical in a log otherwise, and only one of them is
        // something to go and fix.
        _ => out.push_str(
            "; no per-encoder status in the error's userInfo, though this backend creates every \
             command buffer with MTLCommandBufferErrorOptionEncoderExecutionStatus",
        ),
    }
    out
}

/// One labelled state per encoder, in recording order, or `None` when the
/// error carries no encoder array at all.
fn encoders(error: &NSError) -> Option<Vec<String>> {
    // SAFETY: `objc2` declares this as an `extern "C"` static, which Rust
    // requires an `unsafe` block to name. It is an immutable `NSString`
    // constant the Metal framework has initialised before any Metal call can
    // return, and reading the reference is the whole of the access.
    let key = unsafe { MTLCommandBufferEncoderInfoErrorKey };
    let value = error.userInfo().objectForKey(key)?;
    // Apple documents the value as an `NSArray`; the class is checked rather
    // than assumed, because a userInfo dictionary is a bag of `id` and this is
    // the boundary where that stops being true.
    let infos = value.downcast_ref::<NSArray<AnyObject>>()?;
    let protocol = AnyProtocol::get(c"MTLCommandBufferEncoderInfo")?;
    let mut out = Vec::with_capacity(infos.count());
    for index in 0..infos.count() {
        let element = infos.objectAtIndex(index);
        if !element.class().conforms_to(protocol) {
            // Reported rather than skipped: a silently dropped element would
            // shift every later encoder's position in a list whose order is
            // the recording order, which is most of what it is read for.
            out.push(format!(
                "<a {}, which does not conform to MTLCommandBufferEncoderInfo>",
                element.class().name().to_string_lossy()
            ));
            continue;
        }
        // SAFETY: the element's class was just asked whether it conforms to
        // `MTLCommandBufferEncoderInfo`, which is the same question
        // `ProtocolObject::from_ref` answers at compile time for a type known
        // to implement it — and the only question this cast turns on, since a
        // `ProtocolObject` is a type-erased object header either way.
        let info: Retained<ProtocolObject<dyn MTLCommandBufferEncoderInfo>> =
            unsafe { Retained::cast_unchecked(element) };
        // **`debugSignposts` is deliberately not read**, and the reason is a
        // trap rather than a preference. `objc2` generates it as returning a
        // non-optional `Retained<NSArray<NSString>>`, but the real
        // `_MTLCommandBufferEncoderInfo` returns **nil** when an encoder
        // recorded no signposts — which is every encoder this backend produces,
        // since nothing here calls `insertDebugSignpost:`. Sending it therefore
        // panics inside the binding with "unexpected NULL returned", *replacing*
        // the GPU fault this function exists to report with an unrelated one.
        // Measured: it did exactly that on the macOS runner and hid the fault
        // for a whole CI round trip. If signposts are ever inserted, read this
        // through a nil-tolerant path rather than the generated accessor.
        out.push(format!("`{}` {}", info.label(), state(info.errorState())));
    }
    Some(out)
}

/// An [`MTLCommandEncoderErrorState`] in the words a reader needs.
///
/// Spelled out rather than printed as a number: the one thing a fault report
/// has to make obvious is which encoder is the *cause* and which were merely
/// caught up in it, and `4` versus `2` does not say that to anyone.
fn state(state: MTLCommandEncoderErrorState) -> String {
    match state {
        MTLCommandEncoderErrorState::Completed => "completed".to_string(),
        MTLCommandEncoderErrorState::Faulted => {
            "FAULTED — this encoder caused the error".to_string()
        }
        MTLCommandEncoderErrorState::Affected => "affected by another encoder's error".to_string(),
        MTLCommandEncoderErrorState::Pending => "never started".to_string(),
        MTLCommandEncoderErrorState::Unknown => "status unknown".to_string(),
        // A state added after this was written still reaches the log by number,
        // which is worth more than a match arm that silently calls it unknown.
        other => format!("error state {}, which this build does not name", other.0),
    }
}
