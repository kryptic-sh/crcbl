//! What a device does that Metal will not answer a query for.
//!
//! Everything here keys on `MTLDevice::name`, which is **not** a capability
//! query and is not treated as one. A quirk earns a place in this module only
//! when a measurement on a real device contradicts a guarantee the API makes
//! unconditionally, and the entry has to say what was measured, on which device,
//! and what every other device does — otherwise the next reader has a name match
//! with no evidence behind it and no way to tell whether it still applies.
//! `crate::adapter`'s `device_type_of` declines to report
//! [`DeviceType::Virtual`](crcbl_hal::DeviceType::Virtual) from the same string
//! for the mirror-image reason: a name is evidence about one machine, never a
//! device class.
//!
//! `wgpu-hal` keys its own mesh-shader gate on the same substring, with the same
//! kind of comment beside it, so this is the ecosystem's answer to a device that
//! lies rather than an invention here.
//!
//! # Both halves of a withheld feature live here
//!
//! Withholding a [`Features`] flag is only honest if the backend then refuses
//! the call the flag governs — a backend that withholds the flag and makes the
//! call anyway has moved the lie somewhere harder to see. So the adapter's
//! decision and the pipeline's refusal are one module, and neither can be
//! changed without the other being in view.
//!
//! # Plain Rust, so every host runs it
//!
//! There is no Objective-C in this file, which is why it is compiled off macOS
//! under `cfg(test)`: the decisions below are the whole of the quirk, and this
//! is what puts them in reach of `cargo test` on a machine with no Metal at all.
//! `crate::present` is the crate's other module of that shape and its docs argue
//! the split.

use crcbl_hal::{BackendKind, Features, HalError, PrimitiveState};

/// Whether a device called `device_name` honours `MTLDepthClipMode::Clamp`.
///
/// # What was measured, and where
///
/// Metal documents `setDepthClipMode:` on every device and offers no query for
/// it, so this backend reported [`Features::DEPTH_CLAMP`] unconditionally. On
/// GitHub's `Apple Paravirtual device` (macOS 26.5.2) that is false:
/// `crate::device`'s `a_device_says_whether_it_clamps_depth_without_a_depth_attachment`
/// drew a triangle past the far plane under `MTLDepthClipMode::Clamp` and the
/// primitive was discarded, both with a `D32Float` depth attachment and without
/// one, while all four of that probe's controls passed — a triangle inside the
/// clip volume drew, and the same triangle past the far plane under
/// `MTLDepthClipMode::Clip` did not. The device accepts the call and ignores it.
///
/// **Real Metal honours it.** Dawn maps WebGPU's `unclippedDepth` onto
/// `MTLDepthClipMode::Clamp`, the WebGPU CTS covers this geometry, and Dawn
/// carries no Apple Silicon expectation for it — so this is one paravirtual GPU,
/// not the API, and every device that is not one keeps the flag and every call
/// behind it unchanged.
///
/// # Why the name, of all things
///
/// There is nothing else to ask. `wgpu-hal`'s two gates for the same mode are
/// `MTLFeatureSet::macOS_GPUFamily1_v1` — the *baseline* macOS feature set,
/// which every macOS device answers yes to — and "is this macOS"; neither can
/// come back false on the machine that fails. The substring is the same one
/// `wgpu-hal` matches to exclude a virtual device from mesh shaders, and
/// `crate::adapter`'s counter probe already prints it beside the family answers.
///
/// The cost of being wrong is bounded and one-directional: a device whose name
/// contains "virtual" and *does* clamp loses a feature it could have offered and
/// is told so, which is what [`Features`] is for. A device that silently
/// discards geometry is a wrong picture with a clean log.
pub(crate) fn honours_depth_clamp(device_name: &str) -> bool {
    !device_name.to_lowercase().contains("virtual")
}

/// The check `create_graphics_pipeline` makes before it puts
/// [`PrimitiveState::depth_clamp`] on an `MTLDepthClipMode`.
///
/// The second half of [`honours_depth_clamp`]: a device that withheld
/// [`Features::DEPTH_CLAMP`] must see the request refused rather than quietly
/// encoded, both because `crcbl_hal::pipeline` says a backend without the
/// feature refuses the state, and because the agnostic seam suite holds a
/// declared refusal to actually refusing.
///
/// # Errors
///
/// [`HalError::Unsupported`] — never [`HalError::InvalidDescriptor`] — when
/// `primitive` asks for depth clamping and `features` lacks
/// [`Features::DEPTH_CLAMP`]. The descriptor is not malformed and there is no
/// field to correct; `Unsupported` is the variant a caller matches on to take
/// the fallback, and the one the seam suite requires for a capability refusal.
pub(crate) fn check_depth_clamp(
    primitive: PrimitiveState,
    features: Features,
) -> Result<(), HalError> {
    if primitive.depth_clamp && !features.contains(Features::DEPTH_CLAMP) {
        return Err(HalError::Unsupported {
            backend: BackendKind::Metal,
            what: "depth clamping on a device without DEPTH_CLAMP: this backend withholds the flag \
                   from a paravirtual GPU, whose MTLDepthClipMode::Clamp was measured to discard \
                   the primitive anyway",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name is Apple's, and the match has to survive its capitalisation
    /// changing — "Paravirtual" is the spelling CI reports today and nothing
    /// promises it is the only one.
    #[test]
    fn a_device_that_calls_itself_virtual_is_not_trusted_to_clamp_depth() {
        for name in [
            "Apple Paravirtual device",
            "APPLE PARAVIRTUAL DEVICE",
            "apple paravirtual device",
            "Virtual GPU",
        ] {
            assert!(
                !honours_depth_clamp(name),
                "{name:?} kept DEPTH_CLAMP, so the measured quirk is not being applied"
            );
        }
    }

    /// The quirk is one device wide. Every real GPU this backend targets keeps
    /// the flag, which is the half that makes the withholding narrow rather than
    /// "paravirtual devices are broken".
    #[test]
    fn every_other_device_keeps_depth_clamp() {
        for name in [
            "Apple M1",
            "Apple M3 Max",
            "AMD Radeon Pro 5700 XT",
            "Intel Iris Plus Graphics",
        ] {
            assert!(
                honours_depth_clamp(name),
                "{name:?} lost DEPTH_CLAMP, and only a device that ignores the clip mode should"
            );
        }
    }

    /// Both directions, because a check that refuses everything would pass a
    /// test that only asked about the refusal.
    #[test]
    fn a_clamped_pipeline_is_refused_exactly_where_the_flag_is_clear() {
        let clamped = PrimitiveState {
            depth_clamp: true,
            ..PrimitiveState::default()
        };

        check_depth_clamp(clamped, Features::DEPTH_CLAMP)
            .expect("a device reporting DEPTH_CLAMP builds the pipeline it asked for");
        check_depth_clamp(PrimitiveState::default(), Features::empty())
            .expect("a pipeline that asks for no clamp needs no feature");

        let error = check_depth_clamp(clamped, Features::empty())
            .expect_err("a clamp on a device without the flag is refused");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
            "the refusal came back as {error}, and a caller branching on \
             HalError::Unsupported to pick a fallback would miss it"
        );
    }
}
