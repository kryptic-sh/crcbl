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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSDictionary;

    /// **The two states the whole report turns on say different things, and the
    /// right things.**
    ///
    /// Transposing the [`MTLCommandEncoderErrorState::Faulted`] and
    /// [`MTLCommandEncoderErrorState::Affected`] arms is a one-line edit that
    /// compiles, keeps every encoder in the log, and names the wrong one as the
    /// cause — which is the entire question a fault report is opened to answer.
    /// Each is asserted to carry its own sense *and not the other's*, so the
    /// swap goes red twice.
    ///
    /// The states are otherwise pairwise distinct: two arms rendering to one
    /// string would make two different fates indistinguishable in the log, and a
    /// `match` where a later arm is unreachable is exactly how that happens.
    #[test]
    fn the_faulted_encoder_and_the_affected_ones_are_not_described_alike() {
        let faulted = state(MTLCommandEncoderErrorState::Faulted);
        assert!(
            faulted.contains("caused"),
            "the encoder that caused the fault must say so: {faulted}"
        );
        assert!(
            !faulted.contains("affected"),
            "the cause is being described as a bystander: {faulted}"
        );

        let affected = state(MTLCommandEncoderErrorState::Affected);
        assert!(
            affected.contains("affected by another"),
            "a bystander encoder must say it was caught up in someone else's fault: {affected}"
        );
        assert!(
            !affected.contains("caused"),
            "a bystander is being blamed for the fault: {affected}"
        );

        let named = [
            MTLCommandEncoderErrorState::Completed,
            MTLCommandEncoderErrorState::Faulted,
            MTLCommandEncoderErrorState::Affected,
            MTLCommandEncoderErrorState::Pending,
            MTLCommandEncoderErrorState::Unknown,
        ];
        let mut described: Vec<String> = named.iter().copied().map(state).collect();
        assert_eq!(described.len(), named.len(), "nothing to check");
        described.sort_unstable();
        described.dedup();
        assert_eq!(
            described.len(),
            named.len(),
            "two encoder states render to one string, so their fates are indistinguishable \
             in a log: {described:?}"
        );

        // A state this build has no name for still reaches the log carrying its
        // number, rather than being folded into "unknown" — which is a real
        // state with a real meaning. The value is one past the last named state,
        // so it is exactly what a newer Metal would add.
        let unnamed = MTLCommandEncoderErrorState(MTLCommandEncoderErrorState::Faulted.0 + 1);
        let described = state(unnamed);
        assert!(
            described.contains(&unnamed.0.to_string()),
            "an unrecognised state lost its number: {described}"
        );
        assert_ne!(
            described,
            state(MTLCommandEncoderErrorState::Unknown),
            "an unrecognised state must not be reported as Metal's own Unknown"
        );
    }

    /// **A `userInfo` with no encoder array, and one whose value is not an
    /// array, both answer `None`** — which is what makes [`describe`] print the
    /// "no per-encoder status" sentence rather than an empty list.
    ///
    /// Built from a synthetic [`NSError`] because that half of this module needs
    /// no GPU: `encoders` only reads a dictionary. **The other half genuinely
    /// cannot be reached synthetically** — an `MTLCommandBufferEncoderInfo` is a
    /// private Metal class with no public initialiser, so no test can put a real
    /// one in the array, and the `Faulted`/`Affected` rendering is pinned
    /// directly on [`state`] above instead.
    ///
    /// The last case is the one with a bug behind it: a non-conforming element
    /// is *reported in place*, not skipped, because dropping it would shift
    /// every later encoder in a list whose order is the recording order.
    #[test]
    fn a_user_info_without_a_conforming_encoder_array_says_so_rather_than_pretending() {
        let domain = NSString::from_str("crcbl-mtl test domain");
        // SAFETY: `objc2` declares this as an `extern "C"` static, which Rust
        // requires an `unsafe` block to name. It is an immutable `NSString`
        // constant the Metal framework has initialised, and reading the
        // reference is the whole of the access — the same access `encoders`
        // makes.
        let key = unsafe { MTLCommandBufferEncoderInfoErrorKey };

        // SAFETY: the dictionary really is keyed by `NSErrorUserInfoKey` and
        // holds `AnyObject` values, which is the generic pairing this
        // constructor asks the caller to guarantee. Same for each call below.
        let no_key = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
        assert!(
            encoders(&no_key).is_none(),
            "an error with no userInfo named an encoder"
        );

        let string = NSString::from_str("not an array");
        let string_value: &AnyObject = &string;
        let not_an_array: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[string_value]);
        // SAFETY: as above.
        let wrong_type =
            unsafe { NSError::errorWithDomain_code_userInfo(&domain, 2, Some(&not_an_array)) };
        assert!(
            encoders(&wrong_type).is_none(),
            "a userInfo value that is not an NSArray was read as one anyway"
        );

        let strangers: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&[
            NSString::from_str("first"),
            NSString::from_str("second"),
        ]);
        let strangers_value: &AnyObject = &strangers;
        let with_strangers: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[key], &[strangers_value]);
        // SAFETY: as above.
        let stranger_error =
            unsafe { NSError::errorWithDomain_code_userInfo(&domain, 3, Some(&with_strangers)) };
        let described = encoders(&stranger_error).expect("the key holds an array");
        assert_eq!(
            described.len(),
            strangers.count(),
            "an element was dropped, which shifts every later encoder out of recorded \
             order: {described:?}"
        );
        for line in &described {
            assert!(
                line.contains("does not conform to MTLCommandBufferEncoderInfo"),
                "a non-encoder was reported as an encoder: {line}"
            );
        }
    }
}
