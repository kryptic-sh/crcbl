//! Which enumerated adapter a device is opened on.
//!
//! [`crate::backend`] answers "which backend"; this answers "which device
//! inside it", and `CRCBL_VK_ICD` answers "which driver behind Vulkan". Three
//! variables, one per thing a harness can be wrong about, and
//! [`ADAPTER_ENV_VAR`] is the middle one.
//!
//! # Why this exists, in one measurement
//!
//! `crate::screenshot`'s `OffscreenSetup` took `adapters().first()` and never
//! said which adapter that was. (Not a doc link: `screenshot` is not compiled
//! for wasm, where this module still is, and a link that resolves on three
//! targets and breaks the fourth is a red CI job for a hyperlink.) On
//! `windows-latest` the first adapter is not a device that works: the D3D12 HAL
//! suite passed 155/155 on WARP in the same CI job, and the frame that followed
//! failed on its first buffer with `DXGI_ERROR_DEVICE_REMOVED`, before anything
//! was drawn. `crcbl-dx12` has a pin of its own that would have avoided it, and
//! it could not be reached: it is `#[cfg(test)]` inside that crate, and the
//! harness that needed it lives in this one.
//!
//! So the pin is here, above the seam, where every backend's enumeration
//! arrives in the same [`AdapterInfo`] shape and one variable can mean the same
//! thing on all of them.
//!
//! # By device class, not by index
//!
//! An [`AdapterId`](crcbl_hal::AdapterId) is a position in *this* enumeration,
//! and the position moves: `crcbl-dx12` appends WARP after the hardware pass, so
//! its id changes when a GPU is added or removed, and no two drivers agree on
//! order anyway. A harness does not mean "adapter 2" — it means "the software
//! rasteriser", which is a [`DeviceType`] and is stable everywhere.
//!
//! # A miss is a hard failure
//!
//! [`select`] refuses rather than falling back, for the reason
//! `crates/crcbl-vk/tests/vulkan-icd.sh` gives about ICDs: a harness that asked
//! for the software rasteriser and silently got a discrete GPU produces a green
//! run that is evidence about a device nobody named. Falling back is the outcome
//! the pin exists to prevent, and it would arrive looking like a pass.
//!
//! Unset keeps the historical behaviour — the first enumerated adapter —
//! because running against whatever this machine has is a legitimate thing to
//! do deliberately.
//!
//! # `crcbl-dx12` keeps its own pin
//!
//! `crcbl_dx12::pin` and `CRCBL_DX12_ADAPTER` are unchanged and still serve that
//! crate's own suite, which opens instances directly and never reaches
//! [`crate::backend`]. It cannot defer to this module: `crcbl` depends on
//! `crcbl-dx12`, so sharing would mean putting adapter *policy* into
//! `crcbl-hal`, whose crate docs scope it to "traits plus POD descriptors" and
//! whose seam is frozen at P5 exit. The two are read by different processes —
//! `run-dx12-e2e.sh` and `run-render-e2e.sh` — and their vocabularies differ
//! (`warp` against a device class) precisely so neither reads as the other.

use crcbl_hal::{AdapterInfo, DeviceType};

/// The environment variable that says which enumerated adapter to open a device
/// on.
///
/// Its values are the [`DeviceType`] spellings [`select`] accepts: `cpu`,
/// `integrated`, `discrete` and `virtual`.
pub const ADAPTER_ENV_VAR: &str = "CRCBL_ADAPTER";

/// Every value [`ADAPTER_ENV_VAR`] accepts, and the class each one names.
///
/// One table, so the parser and the message a refusal carries cannot drift.
/// `DeviceType::Other` has no spelling on purpose: it is what a backend reports
/// when it *declined to classify* an adapter, so a pin naming it would be asking
/// for whatever could not be identified — the "some adapter nobody named"
/// outcome this module exists to refuse.
const ACCEPTED: [(&str, DeviceType); 4] = [
    ("cpu", DeviceType::Cpu),
    ("integrated", DeviceType::Integrated),
    ("discrete", DeviceType::Discrete),
    ("virtual", DeviceType::Virtual),
];

/// The device class a pin names, or `None` for a word that is not one.
///
/// Case and surrounding whitespace are a shell's doing, not a different
/// request.
#[must_use]
pub fn device_type_from_name(name: &str) -> Option<DeviceType> {
    let name = name.trim();
    ACCEPTED
        .iter()
        .find(|(spelling, _)| name.eq_ignore_ascii_case(spelling))
        .map(|&(_, device_type)| device_type)
}

/// What [`ADAPTER_ENV_VAR`] is set to for this process, or `None` when it is
/// unset or blank.
///
/// Blank counts as unset, which is what [`crate::backend::BACKEND_ENV_VAR`]
/// already does with an empty `CRCBL_GPU`: an exported-but-empty variable is a
/// shell that has nothing to say, not a request. A non-blank word that is not a
/// device class is a different matter and [`select`] refuses it.
#[must_use]
pub fn pin() -> Option<String> {
    match std::env::var(ADAPTER_ENV_VAR) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

/// Why [`select`] chose no adapter.
///
/// The message is the whole value — which pin missed, and what was enumerated
/// instead — because there is nothing a caller can do about it but report it:
/// the variable is wrong, or the machine is not the one it describes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PinMiss(String);

/// The adapter [`ADAPTER_ENV_VAR`] names, or why it names none.
///
/// `pin` is the variable's value — [`pin()`] reads it, and `None` keeps the
/// historical behaviour of taking whatever the backend enumerated first.
///
/// # Errors
///
/// [`PinMiss`] when `adapters` is empty, when `pin` is not one of the spellings
/// in [`device_type_from_name`], when no adapter is of the class it names, or
/// when more than one is — every arm naming what *was* enumerated, because that
/// is the only thing that diagnoses a pin that missed.
pub fn select<'a>(
    pin: Option<&str>,
    adapters: &'a [AdapterInfo],
) -> Result<&'a AdapterInfo, PinMiss> {
    let Some(first) = adapters.first() else {
        return Err(PinMiss(format!(
            "{ADAPTER_ENV_VAR}: this backend enumerated no adapter at all, so there is nothing \
             to open a device on"
        )));
    };
    let Some(pin) = pin else {
        return Ok(first);
    };

    let Some(want) = device_type_from_name(pin) else {
        return Err(PinMiss(format!(
            "{ADAPTER_ENV_VAR}={pin:?} is not a device class; the ones it accepts are {}. Unset \
             it to take the first enumerated adapter.\n{}",
            accepted(),
            listing(adapters)
        )));
    };

    let mut of_class = adapters.iter().filter(|info| info.device_type == want);
    let Some(chosen) = of_class.next() else {
        // The whole point of the pin. Taking `first` here would draw the frame
        // on a GPU while every log line said the software rasteriser.
        return Err(PinMiss(format!(
            "{ADAPTER_ENV_VAR}={pin} was asked for and this enumeration has no {want:?} adapter, \
             so there is nothing to fall back to that would still be the run that was asked \
             for\n{}",
            listing(adapters)
        )));
    };
    if let Some(second) = of_class.next() {
        return Err(PinMiss(format!(
            "{ADAPTER_ENV_VAR}={pin} is ambiguous here: adapters {} and {} are both {want:?}, and \
             picking one would be picking it silently. Unset it to take the first enumerated \
             adapter.\n{}",
            chosen.id.0,
            second.id.0,
            listing(adapters)
        )));
    }
    Ok(chosen)
}

/// The accepted spellings, for a refusal to carry.
fn accepted() -> String {
    ACCEPTED
        .iter()
        .map(|(spelling, _)| *spelling)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What was enumerated, one adapter a line, for a failure to carry.
///
/// A pin that missed is diagnosed by what *was* there, and on a machine nobody
/// on this team can log into, nothing else says it.
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
    use crcbl_hal::{AdapterId, BackendKind, DeviceCaps, Features, Limits};

    /// One enumerated adapter, with only the two fields [`select`] reads
    /// varying: its position and its class.
    fn adapter(index: u32, name: &str, device_type: DeviceType) -> AdapterInfo {
        AdapterInfo {
            id: AdapterId(index),
            name: name.to_owned(),
            vendor_id: 0x1002,
            device_id: 0x744c,
            device_type,
            driver: "test".to_owned(),
            backend: BackendKind::Null,
            caps: DeviceCaps {
                features: Features::empty(),
                limits: Limits::minimum(),
            },
        }
    }

    /// The enumeration a workstation produces: the GPU first and the software
    /// rasteriser behind it. Every case below is a question about this shape.
    fn workstation() -> Vec<AdapterInfo> {
        vec![
            adapter(0, "Discrete GPU", DeviceType::Discrete),
            adapter(1, "Integrated GPU", DeviceType::Integrated),
            adapter(2, "llvmpipe", DeviceType::Cpu),
        ]
    }

    #[test]
    fn an_unset_pin_takes_the_adapter_enumeration_put_first() {
        let adapters = workstation();
        assert_eq!(
            select(None, &adapters).map(|info| info.id),
            Ok(AdapterId(0))
        );
    }

    /// The pin's whole job: the software rasteriser is *last* here — and on
    /// D3D12 it is appended last by construction — so a resolver that returned
    /// the first adapter would pass on a runner with no GPU and fail nowhere
    /// else.
    #[test]
    fn a_pinned_class_is_found_wherever_the_enumeration_put_it() {
        let adapters = workstation();
        assert_eq!(
            select(Some("cpu"), &adapters).map(|info| info.id),
            Ok(AdapterId(2))
        );
        assert_eq!(
            select(Some("integrated"), &adapters).map(|info| info.id),
            Ok(AdapterId(1))
        );
        assert_eq!(
            select(Some("discrete"), &adapters).map(|info| info.id),
            Ok(AdapterId(0))
        );

        // And it is not "the last one" either.
        let software_first = vec![
            adapter(0, "llvmpipe", DeviceType::Cpu),
            adapter(1, "Discrete GPU", DeviceType::Discrete),
        ];
        assert_eq!(
            select(Some("cpu"), &software_first).map(|info| info.id),
            Ok(AdapterId(0))
        );
    }

    /// Every class the seam can report either has a spelling or is deliberately
    /// without one.
    ///
    /// The test below iterates [`ACCEPTED`], so it shrinks silently with the
    /// table and cannot see a hole in it; this is what sees one. The `match` is
    /// the tripwire: a new [`DeviceType`] variant makes it non-exhaustive, so
    /// the decision about whether it is pinnable is a compile error here rather
    /// than an omission nobody notices.
    #[test]
    fn the_table_covers_every_device_class_the_seam_can_report() {
        let classes = [
            DeviceType::Cpu,
            DeviceType::Integrated,
            DeviceType::Discrete,
            DeviceType::Virtual,
            DeviceType::Other,
        ];
        for class in classes {
            let pinnable = match class {
                DeviceType::Cpu
                | DeviceType::Integrated
                | DeviceType::Discrete
                | DeviceType::Virtual => true,
                // "The backend declined to say" is the absence of a class, and
                // pinning it would be asking for whatever could not be
                // identified. See [`ACCEPTED`].
                DeviceType::Other => false,
            };
            assert_eq!(
                ACCEPTED.iter().any(|&(_, listed)| listed == class),
                pinnable,
                "{class:?}"
            );
        }
        assert_eq!(ACCEPTED.len(), classes.len() - 1);
    }

    /// Every spelling in the table reaches the class it names, and reaches it
    /// wherever the enumeration put it.
    #[test]
    fn every_accepted_spelling_selects_the_class_it_names() {
        for (spelling, want) in ACCEPTED {
            let adapters = vec![
                adapter(0, "something else", DeviceType::Other),
                adapter(1, "the one", want),
            ];
            let chosen = select(Some(spelling), &adapters)
                .unwrap_or_else(|why| panic!("{spelling:?} names {want:?}: {why}"));
            assert_eq!(chosen.device_type, want, "{spelling:?}");
            assert_eq!(chosen.id, AdapterId(1), "{spelling:?}");
        }
    }

    /// The failure the pin exists to produce, rather than the fallback it
    /// exists to prevent — and it names what was there instead.
    #[test]
    fn a_class_this_machine_does_not_have_is_refused_rather_than_fallen_back_from() {
        let hardware = vec![adapter(0, "Discrete GPU", DeviceType::Discrete)];
        let why = select(Some("cpu"), &hardware).expect_err("there is no software adapter");
        let why = why.to_string();
        assert!(why.contains(ADAPTER_ENV_VAR), "{why}");
        assert!(why.contains("no Cpu adapter"), "{why}");
        // The diagnosis is what *was* enumerated; without it the message says
        // only that something is missing.
        assert!(why.contains("Discrete GPU"), "{why}");
        assert!(why.contains("type=Discrete"), "{why}");
    }

    /// Two adapters of one class is ambiguous, not "the first one".
    #[test]
    fn two_adapters_of_the_pinned_class_are_ambiguous_rather_than_silently_first() {
        let doubled = vec![
            adapter(0, "GPU one", DeviceType::Discrete),
            adapter(1, "GPU two", DeviceType::Discrete),
        ];
        let why = select(Some("discrete"), &doubled).expect_err("two candidates is not a choice");
        let why = why.to_string();
        assert!(why.contains("ambiguous"), "{why}");
        assert!(why.contains("adapters 0 and 1"), "{why}");
    }

    /// A misspelling is refused and says what would have worked, because the
    /// alternative is a typo that quietly runs on the wrong device.
    ///
    /// `other` is in the list deliberately: it is a [`DeviceType`] variant and
    /// still not a pin, for the reason [`ACCEPTED`] gives.
    #[test]
    fn a_word_that_is_not_a_device_class_is_refused_and_says_what_is_accepted() {
        let adapters = workstation();
        for typo in ["warp", "lavapipe", "software", "gpu", "other", "0", ""] {
            let Err(why) = select(Some(typo), &adapters) else {
                panic!("{typo:?} was accepted as a pin instead of being refused");
            };
            let why = why.to_string();
            assert!(why.contains(ADAPTER_ENV_VAR), "{typo:?}: {why}");
            for (spelling, _) in ACCEPTED {
                assert!(why.contains(spelling), "{typo:?}: {why}");
            }
        }
    }

    /// Case and stray whitespace are a shell's doing, not a different request.
    #[test]
    fn a_pin_ignores_case_and_surrounding_space() {
        let adapters = workstation();
        for spelling in ["cpu", "CPU", "Cpu", " cpu", "cpu\n", "\tcpu "] {
            assert_eq!(
                select(Some(spelling), &adapters).map(|info| info.id),
                Ok(AdapterId(2)),
                "{spelling:?}"
            );
        }
    }

    /// An empty enumeration is answered rather than indexed into, whichever way
    /// the pin is set.
    #[test]
    fn an_empty_enumeration_is_refused_rather_than_indexed() {
        for pin in [None, Some("cpu"), Some("nonsense")] {
            let why = select(pin, &[]).expect_err("there is no adapter to name");
            assert!(
                why.to_string().contains("no adapter at all"),
                "{pin:?}: {why}"
            );
        }
    }
}
