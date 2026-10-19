//! Which enumerated adapter a device is opened on.
//!
//! [`crate::backend`] answers "which backend"; this answers "which device
//! inside it", and `CRCBL_VK_ICD` answers "which driver behind Vulkan". Three
//! variables, one per thing a harness can be wrong about, and
//! [`ADAPTER_ENV_VAR`] is the middle one.
//!
//! # Why this exists, in one measurement
//!
//! `crcbl::screenshot`'s `OffscreenSetup` took `adapters().first()` and never
//! said which adapter that was. (Not a doc link: it is in the `crcbl` umbrella,
//! which depends on this crate, and is not compiled for wasm either.) On
//! `windows-latest` the first adapter is not a device that works: the D3D12 HAL
//! suite passed 155/155 on WARP in the same CI job, and the frame that followed
//! failed on its first buffer with `DXGI_ERROR_DEVICE_REMOVED`, before anything
//! was drawn. `crcbl-dx12` has a pin of its own that would have avoided it, and
//! it could not be reached: it is `#[cfg(test)]` inside that crate, and the
//! harness that needed it lives in `crcbl`.
//!
//! So the pin is here, above the seam, where every backend's enumeration
//! arrives in the same [`AdapterInfo`] shape and one variable can mean the same
//! thing on all of them.
//!
//! # By device class, not by index
//!
//! An [`AdapterId`] is a position in *this* enumeration,
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
//! Unset, [`select`] keeps the historical behaviour — the first enumerated
//! adapter — because running against whatever this machine has is a
//! legitimate thing to do deliberately. A device open goes through [`choose`]
//! instead, which ranks the adapters as below.
//!
//! # When nothing names an adapter
//!
//! Decided by the owner on 2026-10-19: the program chooses, by priority or
//! explicitly. [`choose`] takes, in order:
//!
//! 1. [`ADAPTER_ENV_VAR`], so a harness can aim any program;
//! 2. the program's own [`AdapterPick`];
//! 3. otherwise every adapter, ranked by [`rank`]: a display driven by a
//!    **discrete** adapter (a desktop, or a laptop's external monitor wired to
//!    the dGPU) puts that adapter first whatever the preference, which avoids
//!    copying every frame across adapters; otherwise the [`PowerPreference`]
//!    orders discrete against integrated — a hybrid laptop's panel, driven by
//!    the iGPU, renders on the dGPU under
//!    [`HighPerformance`](PowerPreference::HighPerformance) and stays on the
//!    iGPU under [`LowPower`](PowerPreference::LowPower), and so does a window
//!    whose display adapter the backend cannot tell.
//!
//! The first two are explicit picks: one that misses, or names an adapter
//! that cannot present, is refused. Only the ranked walk steps past an adapter
//! that cannot serve the window.
//!
//! # `crcbl-dx12` keeps its own pin
//!
//! `crcbl_dx12::pin` and `CRCBL_DX12_ADAPTER` are unchanged and still serve that
//! crate's own suite, which opens instances directly and never reaches
//! [`crate::backend`]. It cannot defer to this module: this crate depends on
//! `crcbl-dx12`, so sharing would mean putting adapter *policy* into
//! `crcbl-hal`, whose crate docs scope it to "traits plus POD descriptors" and
//! whose seam is frozen at P5 exit. The two are read by different processes —
//! `run-dx12-e2e.sh` and `run-render-e2e.sh` — and their vocabularies differ
//! (`warp` against a device class) precisely so neither reads as the other.

use crcbl_hal::{AdapterId, AdapterInfo, DeviceType};

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

/// Why [`select`] chose no adapter, or why the one it chose could not be used.
///
/// The message is the whole value — which pin missed, and what was enumerated
/// instead — because there is nothing a caller can do about it but report it:
/// the variable is wrong, or the machine is not the one it describes.
///
/// The second half is [`crate::present`]'s: a windowed open that pinned an adapter
/// which then cannot present to the window refuses with one of these rather
/// than walking on to an adapter nobody named.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PinMiss(String);

impl PinMiss {
    /// A refusal whose message is `message`, for a caller in this crate that
    /// found the pinned adapter unusable after [`select`] had chosen it —
    /// [`Presenter`](crate::present::Presenter)'s adapter walk.
    #[must_use]
    pub(crate) const fn new(message: String) -> Self {
        Self(message)
    }
}

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

    the_one(
        adapters,
        &format!("{ADAPTER_ENV_VAR}={pin}"),
        &format!("{want:?}"),
        |info| info.device_type == want,
    )
}

/// The one adapter `matches` accepts, or a refusal naming `who` asked for it
/// and `what` it asked for.
///
/// Shared by the variable and the program's [`AdapterPick`], which refuse a
/// miss and an ambiguity for the same reasons and should say so the same way.
fn the_one<'a>(
    adapters: &'a [AdapterInfo],
    who: &str,
    what: &str,
    matches: impl Fn(&AdapterInfo) -> bool,
) -> Result<&'a AdapterInfo, PinMiss> {
    let mut matching = adapters.iter().filter(|info| matches(info));
    let Some(chosen) = matching.next() else {
        // The whole point of a pin. Taking the first adapter here would draw
        // the frame on a GPU while every log line said the software
        // rasteriser.
        return Err(PinMiss(format!(
            "{who} was asked for and this enumeration has no {what} adapter, so there is nothing \
             to fall back to that would still be the run that was asked for\n{}",
            listing(adapters)
        )));
    };
    if let Some(second) = matching.next() {
        return Err(PinMiss(format!(
            "{who} is ambiguous here: adapters {} and {} are both {what}, and picking one would \
             be picking it silently. Name one adapter, or name none to let the program rank \
             them.\n{}",
            chosen.id.0,
            second.id.0,
            listing(adapters)
        )));
    }
    Ok(chosen)
}

/// Which kind of adapter a program would rather run on when nothing names
/// one.
///
/// Games ask for [`HighPerformance`](Self::HighPerformance), as `crcbl`'s
/// engine does, and apps for [`LowPower`](Self::LowPower), the
/// [`Presenter`](crate::present::Presenter)'s default: a launcher or a status
/// bar has no business waking a discrete GPU. A display driven by a discrete
/// adapter overrules either; see [`rank`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PowerPreference {
    /// A discrete GPU ahead of an integrated one.
    HighPerformance,
    /// An integrated GPU ahead of a discrete one, so the discrete one can
    /// sleep.
    LowPower,
}

/// An adapter the program names itself, through the API rather than
/// [`ADAPTER_ENV_VAR`].
///
/// A pick wins over the [`PowerPreference`] and the display, and one that
/// names nothing here, names more than one adapter, or names an adapter that
/// cannot present is refused rather than fallen back from — the variable's
/// rule, for the variable's reason. The variable wins over a pick, so a
/// harness can aim any program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterPick<'a> {
    /// The adapter of this class.
    Class(DeviceType),
    /// The adapter whose [`AdapterInfo::name`] is exactly this: what a
    /// settings screen that listed the adapters stores.
    Name(&'a str),
}

impl AdapterPick<'_> {
    /// The adapter this pick names, or why it names none.
    fn resolve(self, adapters: &[AdapterInfo]) -> Result<&AdapterInfo, PinMiss> {
        match self {
            Self::Class(want) => the_one(
                adapters,
                &format!("the program's pick of a {want:?} adapter"),
                &format!("{want:?}"),
                |info| info.device_type == want,
            ),
            Self::Name(name) => the_one(
                adapters,
                &format!("the program's pick of {name:?}"),
                &format!("{name:?}"),
                |info| info.name == name,
            ),
        }
    }
}

/// The adapters a device open tries, in the order it tries them, and what
/// chose them. From [`choose`].
#[derive(Debug, PartialEq)]
pub enum Choice<'a> {
    /// [`ADAPTER_ENV_VAR`] named this adapter, and only it is tried.
    Pinned(&'a AdapterInfo),
    /// The program's [`AdapterPick`] named this adapter, and only it is
    /// tried.
    Picked(&'a AdapterInfo),
    /// Nothing named one: every adapter, in [`rank`]'s order, so an open can
    /// step past one that cannot serve the window.
    Ranked(Vec<&'a AdapterInfo>),
}

impl<'a> Choice<'a> {
    /// The adapters to try, best first.
    #[must_use]
    pub fn candidates(&self) -> &[&'a AdapterInfo] {
        match self {
            Self::Pinned(adapter) | Self::Picked(adapter) => core::slice::from_ref(adapter),
            Self::Ranked(ranked) => ranked,
        }
    }
}

/// Which adapters a device open tries, and in what order.
///
/// `pin` is [`ADAPTER_ENV_VAR`]'s value ([`pin()`] reads it), `pick` the
/// program's own choice, and `display` the adapter driving the display the
/// window is on, where the backend can tell
/// ([`Instance::display_adapter`](crcbl_hal::Instance::display_adapter)). The
/// module docs give the order they are taken in.
///
/// # Errors
///
/// [`PinMiss`] when `pin` or `pick` names no adapter here, or more than one.
pub fn choose<'a>(
    adapters: &'a [AdapterInfo],
    pin: Option<&str>,
    pick: Option<AdapterPick<'_>>,
    preference: PowerPreference,
    display: Option<AdapterId>,
) -> Result<Choice<'a>, PinMiss> {
    if pin.is_some() {
        return select(pin, adapters).map(Choice::Pinned);
    }
    if let Some(pick) = pick {
        return pick.resolve(adapters).map(Choice::Picked);
    }
    Ok(Choice::Ranked(rank(adapters, preference, display)))
}

/// Every adapter, best first for `preference` and the window's `display`.
///
/// The adapter driving the display goes first when it is discrete, whatever
/// the preference: rendering anywhere else copies every frame across to it.
/// Then the preference orders the classes — discrete and integrated in the
/// order it asks for, then virtual, then whatever the backend declined to
/// classify, then software, which is correct and slow and looks like a
/// performance bug when picked by accident. Within a class the display's own
/// adapter goes first, and otherwise the enumeration's order stands.
///
/// An unknown display (`None`) leaves the preference to decide, never fails.
#[must_use]
pub fn rank(
    adapters: &[AdapterInfo],
    preference: PowerPreference,
    display: Option<AdapterId>,
) -> Vec<&AdapterInfo> {
    let mut ranked: Vec<&AdapterInfo> = adapters.iter().collect();
    // Stable, so ties keep the enumeration's order.
    ranked.sort_by_key(|info| {
        let on_display = display == Some(info.id);
        let drives_display = on_display && info.device_type == DeviceType::Discrete;
        (
            !drives_display,
            class_rank(info.device_type, preference),
            !on_display,
        )
    });
    ranked
}

/// Where `class` sits under `preference`, lowest first.
///
/// A `match` over both, so a new [`DeviceType`] has to be placed here rather
/// than landing silently at either end.
const fn class_rank(class: DeviceType, preference: PowerPreference) -> u8 {
    match (class, preference) {
        (DeviceType::Discrete, PowerPreference::HighPerformance)
        | (DeviceType::Integrated, PowerPreference::LowPower) => 0,
        (DeviceType::Integrated, PowerPreference::HighPerformance)
        | (DeviceType::Discrete, PowerPreference::LowPower) => 1,
        (DeviceType::Virtual, _) => 2,
        (DeviceType::Other, _) => 3,
        (DeviceType::Cpu, _) => 4,
    }
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
            luid: None,
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

    // ---- choosing when nothing is pinned -----------------------------------

    use PowerPreference::{HighPerformance, LowPower};

    /// The ids `choose` would try, in order, with no pin and no pick.
    fn ranked(
        adapters: &[AdapterInfo],
        preference: PowerPreference,
        display: Option<u32>,
    ) -> Vec<u32> {
        match choose(adapters, None, None, preference, display.map(AdapterId)) {
            Ok(Choice::Ranked(order)) => order.iter().map(|info| info.id.0).collect(),
            other => panic!("nothing named an adapter, so the walk is ranked: {other:?}"),
        }
    }

    /// **A desktop: the display is on the discrete GPU, so it goes first
    /// whatever the preference.** `LowPower` choosing the iGPU here would
    /// copy every frame across to the card that shows it.
    #[test]
    fn a_display_on_a_discrete_gpu_puts_it_first_whatever_the_preference() {
        let adapters = workstation();
        assert_eq!(ranked(&adapters, HighPerformance, Some(0)), [0, 1, 2]);
        assert_eq!(ranked(&adapters, LowPower, Some(0)), [0, 1, 2]);
    }

    /// A laptop: the iGPU enumerates first, the dGPU beside it, and the
    /// software rasteriser last.
    fn laptop() -> Vec<AdapterInfo> {
        vec![
            adapter(0, "Integrated GPU", DeviceType::Integrated),
            adapter(1, "Discrete GPU", DeviceType::Discrete),
            adapter(2, "WARP", DeviceType::Cpu),
        ]
    }

    /// **A hybrid laptop's panel is driven by the iGPU, and the preference
    /// decides.** `HighPerformance` renders on the dGPU and accepts the copy;
    /// `LowPower` stays on the iGPU and lets the dGPU sleep.
    #[test]
    fn a_hybrid_laptop_panel_follows_the_preference() {
        assert_eq!(ranked(&laptop(), HighPerformance, Some(0)), [1, 0, 2]);
        assert_eq!(ranked(&laptop(), LowPower, Some(0)), [0, 1, 2]);
    }

    /// **A laptop's external monitor wired to the dGPU takes the dGPU even
    /// under `LowPower`**: it is awake for the display anyway, and the iGPU
    /// would copy every frame to it.
    #[test]
    fn an_external_monitor_on_the_dgpu_takes_it_even_under_low_power() {
        assert_eq!(ranked(&laptop(), LowPower, Some(1)), [1, 0, 2]);
        assert_eq!(ranked(&laptop(), HighPerformance, Some(1)), [1, 0, 2]);
    }

    /// **An unknown display leaves the preference to decide**, and does not
    /// fail: a backend that cannot say, and an offscreen surface, both
    /// arrive as `None`.
    #[test]
    fn an_unknown_display_lets_the_preference_pick() {
        let adapters = workstation();
        assert_eq!(ranked(&adapters, HighPerformance, None), [0, 1, 2]);
        assert_eq!(ranked(&adapters, LowPower, None), [1, 0, 2]);
        // An id the enumeration does not have is as good as unknown.
        assert_eq!(ranked(&adapters, LowPower, Some(9)), [1, 0, 2]);
    }

    /// **With no discrete adapter, both preferences take the iGPU**, and
    /// software stays last.
    #[test]
    fn with_no_discrete_adapter_both_preferences_take_the_integrated_one() {
        let thin = vec![
            adapter(0, "WARP", DeviceType::Cpu),
            adapter(1, "Integrated GPU", DeviceType::Integrated),
        ];
        for preference in [HighPerformance, LowPower] {
            for display in [None, Some(0), Some(1)] {
                assert_eq!(
                    ranked(&thin, preference, display),
                    [1, 0],
                    "{preference:?} {display:?}"
                );
            }
        }
    }

    /// **Software alone is still a choice**: the runner with no GPU.
    #[test]
    fn only_a_software_adapter_is_still_ranked() {
        let runner = vec![adapter(0, "WARP", DeviceType::Cpu)];
        for preference in [HighPerformance, LowPower] {
            assert_eq!(ranked(&runner, preference, None), [0]);
            assert_eq!(ranked(&runner, preference, Some(0)), [0]);
        }
        assert_eq!(ranked(&[], LowPower, None), Vec::<u32>::new());
    }

    /// **A display on a software or virtual adapter does not jump the
    /// hardware**: only a discrete one overrules the preference. A basic
    /// display adapter driving the monitor is no reason to render on WARP.
    #[test]
    fn only_a_discrete_display_adapter_overrules_the_preference() {
        let adapters = vec![
            adapter(0, "Basic Display", DeviceType::Cpu),
            adapter(1, "Discrete GPU", DeviceType::Discrete),
            adapter(2, "Virtual GPU", DeviceType::Virtual),
        ];
        assert_eq!(ranked(&adapters, LowPower, Some(0)), [1, 2, 0]);
        assert_eq!(ranked(&adapters, LowPower, Some(2)), [1, 2, 0]);
    }

    /// **Within a class the display's adapter goes first**, and otherwise
    /// the enumeration's order stands.
    #[test]
    fn the_display_adapter_breaks_a_tie_within_its_class() {
        let adapters = vec![
            adapter(0, "Discrete GPU", DeviceType::Discrete),
            adapter(1, "Integrated A", DeviceType::Integrated),
            adapter(2, "Integrated B", DeviceType::Integrated),
        ];
        assert_eq!(ranked(&adapters, LowPower, Some(2)), [2, 1, 0]);
        assert_eq!(ranked(&adapters, HighPerformance, Some(2)), [0, 2, 1]);
        assert_eq!(ranked(&adapters, LowPower, None), [1, 2, 0]);
    }

    /// **Every class has a place under both preferences**: discrete and
    /// integrated in the order asked for, then virtual, then unclassified,
    /// then software.
    #[test]
    fn every_class_is_ranked_under_both_preferences() {
        let all = vec![
            adapter(0, "cpu", DeviceType::Cpu),
            adapter(1, "other", DeviceType::Other),
            adapter(2, "virtual", DeviceType::Virtual),
            adapter(3, "integrated", DeviceType::Integrated),
            adapter(4, "discrete", DeviceType::Discrete),
        ];
        assert_eq!(ranked(&all, HighPerformance, None), [4, 3, 2, 1, 0]);
        assert_eq!(ranked(&all, LowPower, None), [3, 4, 2, 1, 0]);
    }

    /// **An explicit pick wins over the display and the preference**, and
    /// the variable wins over a pick.
    #[test]
    fn an_explicit_pick_wins_and_the_variable_wins_over_it() {
        let adapters = workstation();
        let picked = |pin, pick| {
            choose(&adapters, pin, pick, HighPerformance, Some(AdapterId(0))).map(|choice| {
                (
                    matches!(choice, Choice::Pinned(_)),
                    choice
                        .candidates()
                        .iter()
                        .map(|info| info.id)
                        .collect::<Vec<_>>(),
                )
            })
        };
        assert_eq!(
            picked(None, Some(AdapterPick::Class(DeviceType::Cpu))),
            Ok((false, vec![AdapterId(2)]))
        );
        assert_eq!(
            picked(None, Some(AdapterPick::Name("Integrated GPU"))),
            Ok((false, vec![AdapterId(1)]))
        );
        assert_eq!(
            picked(Some("cpu"), Some(AdapterPick::Name("Discrete GPU"))),
            Ok((true, vec![AdapterId(2)]))
        );
    }

    /// **A pick that misses, or names two, is refused** and says what was
    /// there, exactly as the variable is.
    #[test]
    fn a_pick_that_misses_or_is_ambiguous_is_refused() {
        let adapters = workstation();
        let why = choose(
            &adapters,
            None,
            Some(AdapterPick::Name("nope")),
            LowPower,
            None,
        )
        .expect_err("no adapter is called that");
        let why = why.to_string();
        assert!(why.contains("\"nope\""), "{why}");
        assert!(why.contains("type=Discrete"), "{why}");

        let doubled = vec![
            adapter(0, "GPU", DeviceType::Discrete),
            adapter(1, "GPU", DeviceType::Discrete),
        ];
        for pick in [
            AdapterPick::Name("GPU"),
            AdapterPick::Class(DeviceType::Discrete),
        ] {
            let why = choose(&doubled, None, Some(pick), LowPower, None)
                .expect_err("two candidates is not a choice");
            assert!(
                why.to_string().contains("adapters 0 and 1"),
                "{pick:?}: {why}"
            );
        }
    }
}
