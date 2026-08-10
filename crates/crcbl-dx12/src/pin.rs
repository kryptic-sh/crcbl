//! Which enumerated adapter this crate's tests open a device on.
//!
//! Every device test in this crate used to take `adapters()[0]`, which is
//! whatever DXGI put first — the discrete GPU on a workstation, and WARP only
//! because a CI runner happens to have nothing else. That makes a green run
//! evidence about an adapter nobody named, which is the failure
//! `crates/crcbl-vk/tests/vulkan-icd.sh` documents for Vulkan ICDs: a harness
//! that meant the software rasteriser and silently got something else.
//!
//! So `tests/run-dx12-e2e.sh` sets [`PIN_VAR`], and [`resolve`] is the one place
//! that turns it into an [`AdapterId`]. **An unresolvable pin is a hard failure**
//! rather than a fallback, for the same reason `crcbl_pin_vk_icd` refuses a
//! manifest it cannot find: falling back is exactly the outcome the pin exists to
//! prevent, and it would arrive looking like a pass.
//!
//! # Why this module has no `#[cfg(target_os = "windows")]`
//!
//! It holds no `windows` type — [`AdapterInfo`] is the seam's, and the decision
//! is over [`DeviceType`] — so it compiles and its tests run on every host,
//! including the Linux one this backend is developed on. That is deliberate:
//! nothing else in this crate can be exercised off Windows, and the pin is the
//! piece whose failure mode is silence, so it is the piece most worth being able
//! to run. It is still `#[cfg(test)]`, and that argument has been narrowed by
//! measurement: it used to be "the seam already publishes every adapter and lets
//! the caller choose, so an engine has no use for this". The caller that needed
//! to choose turned out to live in another crate — `crcbl::screenshot` took
//! `adapters().first()`, and on `windows-latest` that is not a device that works
//! — and `#[cfg(test)]` here could not reach it.
//!
//! What that caller uses is `crcbl::adapter` and `CRCBL_ADAPTER`: the same
//! decision over the same [`DeviceType`], above the seam, where one variable
//! means the same thing on every backend. This module keeps [`PIN_VAR`] rather
//! than deferring to it, because the direction of the dependency forbids sharing
//! — `crcbl` depends on `crcbl-dx12`, so a shared resolver would have to sit in
//! `crcbl-hal`, which is "traits plus POD descriptors" and frozen at P5 exit.
//! The two are read by different processes (`tests/run-dx12-e2e.sh` against this
//! crate's suite, `crates/crcbl/tests/run-render-e2e.sh` against the engine's),
//! and their vocabularies differ — `warp` against a device class — so neither
//! reads as the other.

use crcbl_hal::{AdapterId, AdapterInfo, DeviceType};

/// The environment variable `tests/run-dx12-e2e.sh` sets, and the only way to
/// aim this crate's device tests at a particular adapter.
pub(crate) const PIN_VAR: &str = "CRCBL_DX12_ADAPTER";

/// The one value [`PIN_VAR`] accepts: D3D12's software rasteriser, which ships
/// in Windows and is this backend's lavapipe.
///
/// There is no `hardware` spelling beside it because leaving the variable unset
/// already means "whatever this machine enumerated first", and WARP is appended
/// last — see `crcbl_dx12::instance`'s enumeration. A second value would be
/// machinery for a caller that does not exist.
pub(crate) const WARP: &str = "warp";

/// The adapter `pin` names, or why it names none.
///
/// `None` keeps the historical behaviour — the first enumerated adapter — and
/// says so in the failure when there are none at all.
pub(crate) fn resolve(pin: Option<&str>, adapters: &[AdapterInfo]) -> Result<AdapterId, String> {
    let Some(first) = adapters.first() else {
        return Err(format!(
            "{PIN_VAR}: DXGI enumerated no adapter at all, so there is nothing to open a device \
             on — on Windows this cannot happen while EnumWarpAdapter works"
        ));
    };
    let Some(pin) = pin else {
        return Ok(first.id);
    };

    if !pin.trim().eq_ignore_ascii_case(WARP) {
        return Err(format!(
            "{PIN_VAR}={pin:?} is not a value this crate understands; the only one is {WARP:?}. \
             Unset it to take the first enumerated adapter.\n{}",
            listing(adapters)
        ));
    }

    let mut software = adapters
        .iter()
        .filter(|info| info.device_type == DeviceType::Cpu);
    let Some(chosen) = software.next() else {
        // The whole point of the pin. Taking `first` here would run the suite on
        // a GPU while every log line said WARP.
        return Err(format!(
            "{PIN_VAR}={WARP} was asked for and this enumeration has no software adapter, so \
             there is nothing to fall back to that would still be the run CI makes\n{}",
            listing(adapters)
        ));
    };
    if let Some(second) = software.next() {
        return Err(format!(
            "{PIN_VAR}={WARP} is ambiguous here: adapters {} and {} are both software, and \
             picking one would be picking it silently\n{}",
            chosen.id.0,
            second.id.0,
            listing(adapters)
        ));
    }
    Ok(chosen.id)
}

/// What was enumerated, one adapter a line, for a failure to carry.
///
/// A pin that missed is diagnosed by what *was* there, and nothing else in a
/// harness log says it.
fn listing(adapters: &[AdapterInfo]) -> String {
    adapters
        .iter()
        .map(|info| {
            format!(
                "  adapter {id} \"{name}\" type={kind:?}",
                id = info.id.0,
                name = info.name,
                kind = info.device_type
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{BackendKind, DeviceCaps, Features, Limits};

    /// One enumerated adapter, with only the two fields [`resolve`] reads
    /// varying: its position and its class.
    fn adapter(index: u32, name: &str, device_type: DeviceType) -> AdapterInfo {
        AdapterInfo {
            id: AdapterId(index),
            name: name.to_owned(),
            vendor_id: 0x1414,
            device_id: 0x008c,
            device_type,
            driver: "test".to_owned(),
            backend: BackendKind::Dx12,
            caps: DeviceCaps {
                features: Features::empty(),
                limits: Limits::minimum(),
            },
        }
    }

    /// The enumeration a Windows workstation produces: a GPU first, WARP
    /// appended last. Every case below is a question about this shape.
    fn workstation() -> Vec<AdapterInfo> {
        vec![
            adapter(0, "Discrete GPU", DeviceType::Discrete),
            adapter(1, "Integrated GPU", DeviceType::Integrated),
            adapter(2, "Microsoft Basic Render Driver", DeviceType::Cpu),
        ]
    }

    #[test]
    fn an_unset_pin_takes_the_adapter_enumeration_put_first() {
        assert_eq!(resolve(None, &workstation()), Ok(AdapterId(0)));
    }

    /// The pin's whole job: WARP is *appended* after the hardware pass, so a
    /// resolver that returned the first adapter would pass a test written
    /// against a runner with no GPU and fail nowhere else.
    #[test]
    fn the_warp_pin_finds_the_software_adapter_wherever_it_was_appended() {
        assert_eq!(resolve(Some(WARP), &workstation()), Ok(AdapterId(2)));

        // And it is not "the last one" either.
        let warp_first = vec![
            adapter(0, "Microsoft Basic Render Driver", DeviceType::Cpu),
            adapter(1, "Discrete GPU", DeviceType::Discrete),
        ];
        assert_eq!(resolve(Some(WARP), &warp_first), Ok(AdapterId(0)));
    }

    /// The failure the pin exists to produce, rather than the fallback it exists
    /// to prevent.
    #[test]
    fn the_warp_pin_refuses_rather_than_falling_back_to_hardware() {
        let hardware = vec![adapter(0, "Discrete GPU", DeviceType::Discrete)];
        let why = resolve(Some(WARP), &hardware).expect_err("there is no software adapter");
        assert!(why.contains(PIN_VAR), "{why}");
        // The diagnosis is what *was* enumerated; without it the message says
        // only that something is missing.
        assert!(why.contains("Discrete GPU"), "{why}");
        assert!(why.contains("type=Discrete"), "{why}");
    }

    /// Two software adapters is ambiguous, not "the first one".
    ///
    /// `windows-latest` really does list two Basic Render Driver entries — see
    /// `crcbl_dx12::instance`'s de-duplication note — so this is the shape a
    /// runner can produce, not a hypothetical.
    #[test]
    fn two_software_adapters_are_ambiguous_rather_than_silently_first() {
        let doubled = vec![
            adapter(0, "Microsoft Basic Render Driver", DeviceType::Cpu),
            adapter(1, "Microsoft Basic Render Driver", DeviceType::Cpu),
        ];
        let why = resolve(Some(WARP), &doubled).expect_err("two candidates is not a choice");
        assert!(why.contains("ambiguous"), "{why}");
        assert!(why.contains('0') && why.contains('1'), "{why}");
    }

    /// A misspelling is refused and says what would have worked, because the
    /// alternative is a typo that quietly runs on the GPU.
    #[test]
    fn a_pin_this_crate_does_not_understand_is_refused_and_says_what_is_accepted() {
        for typo in ["lavapipe", "software", "WARP0", "0", ""] {
            let Err(why) = resolve(Some(typo), &workstation()) else {
                panic!("{typo:?} was accepted as a pin instead of being refused");
            };
            assert!(why.contains(PIN_VAR), "{typo:?}: {why}");
            assert!(why.contains(WARP), "{typo:?}: {why}");
        }
    }

    /// Case and stray whitespace are a shell's doing, not a different request.
    #[test]
    fn the_warp_pin_ignores_case_and_surrounding_space() {
        for spelling in ["warp", "WARP", "Warp", " warp", "warp\n", "\twarp "] {
            assert_eq!(
                resolve(Some(spelling), &workstation()),
                Ok(AdapterId(2)),
                "{spelling:?}"
            );
        }
    }

    /// An empty enumeration is answered rather than indexed into, whichever way
    /// the pin is set.
    #[test]
    fn an_empty_enumeration_is_refused_rather_than_indexed() {
        for pin in [None, Some(WARP), Some("nonsense")] {
            let why = resolve(pin, &[]).expect_err("there is no adapter to name");
            assert!(why.contains("no adapter at all"), "{pin:?}: {why}");
        }
    }
}
