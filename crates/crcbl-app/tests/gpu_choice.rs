//! A saved GPU choice against real Vulkan drivers: each case opens an offscreen
//! presenter and reads what became of the choice it was given.
//!
//! Ignored: it needs a Vulkan loader with `VK_LUNARG_direct_driver_loading`,
//! and the cases that matter need two adapters on two drivers. Run on the
//! office laptop (Intel RPL-U on Mesa, NVIDIA MX550 on 615.78, lavapipe) with
//! none of the loader's driver variables set:
//!
//! ```text
//! cargo test -p crcbl-app --test gpu_choice -- --ignored --test-threads=1
//! ```

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use crcbl_app::adapter::PowerPreference;
use crcbl_app::backend::GpuBackend;
use crcbl_app::gpu_choice::{ChoiceOutcome, GpuChoice, Ignored};
use crcbl_app::present::{Presenter, PresenterDesc};
use crcbl_hal::{AdapterInfo, DeviceType};

const EXTENT: (u32, u32) = (64, 64);

/// An offscreen presenter on Vulkan with `choice`, preferring `preference`.
fn open(choice: Option<&GpuChoice>, preference: PowerPreference) -> Presenter {
    Presenter::open_offscreen(
        EXTENT,
        &PresenterDesc {
            backend: Some(GpuBackend::Vulkan),
            power_preference: preference,
            gpu_choice: choice,
            ..PresenterDesc::default()
        },
    )
    .expect("an offscreen presenter")
}

/// The adapter `presenter` opened on.
fn adapter(presenter: &Presenter) -> AdapterInfo {
    presenter.adapter().expect("an adapter")
}

/// **A choice is learnt, located, and opens on its driver alone**: an open
/// with none reports `NotGiven` and hands back the adapter it opened on, whose
/// driver the probe finds; given back, it is `Used` on the same adapter and
/// carries the driver on.
#[test]
#[ignore = "needs Vulkan drivers"]
fn a_saved_choice_reopens_its_adapter_through_its_driver() {
    let first = open(None, PowerPreference::LowPower);
    assert_eq!(first.gpu_choice_outcome(), &ChoiceOutcome::NotGiven);
    let mut choice = first.gpu_choice().clone();
    assert_eq!(choice.vulkan_driver(), None, "nothing located it yet");
    assert!(choice.locate_vulkan_driver(), "no driver shows {choice}");
    let driver = choice.vulkan_driver().expect("located").to_owned();
    eprintln!("located {}", driver.display());

    let again = open(Some(&choice), PowerPreference::LowPower);
    assert_eq!(again.gpu_choice_outcome(), &ChoiceOutcome::Used);
    assert!(choice.names(&adapter(&again)));
    assert_eq!(again.gpu_choice().vulkan_driver(), Some(driver.as_path()));
}

/// **A choice wins over the ranking**: the discrete adapter, saved, is
/// opened by a program that asks for low power, which would rank the
/// integrated one first — through its driver alone, and as the first
/// candidate of an instance on every driver when its driver is not located.
#[test]
#[ignore = "needs Vulkan drivers and a discrete adapter beside an integrated one"]
fn a_saved_choice_wins_over_the_power_preference() {
    let discrete = open(None, PowerPreference::HighPerformance);
    let chosen = adapter(&discrete);
    assert_eq!(
        chosen.device_type,
        DeviceType::Discrete,
        "no discrete adapter here"
    );
    let unlocated = discrete.gpu_choice().clone();
    let mut choice = unlocated.clone();
    assert!(choice.locate_vulkan_driver());
    drop(discrete);

    let low = open(None, PowerPreference::LowPower);
    assert!(
        !choice.names(&adapter(&low)),
        "low power already picks the discrete one"
    );
    drop(low);

    for saved in [&choice, &unlocated] {
        let reopened = open(Some(saved), PowerPreference::LowPower);
        assert_eq!(reopened.gpu_choice_outcome(), &ChoiceOutcome::Used);
        assert!(saved.names(&adapter(&reopened)), "{saved}");
    }
}

/// **A choice that no longer holds is `Stale`, and the open goes on**: one
/// whose driver's manifest is gone, and one naming an adapter that is not
/// here, each open on the adapter the ranking picks.
#[test]
#[ignore = "needs Vulkan drivers"]
fn a_stale_choice_is_reported_and_the_open_goes_on() {
    let first = open(None, PowerPreference::LowPower);
    let mut choice = first.gpu_choice().clone();
    assert!(choice.locate_vulkan_driver());
    let ranked = adapter(&first);
    drop(first);

    let gone = choice
        .clone()
        .with_vulkan_driver(PathBuf::from("/nowhere/gone_icd.json"));
    let opened = open(Some(&gone), PowerPreference::LowPower);
    assert!(
        matches!(opened.gpu_choice_outcome(), ChoiceOutcome::Stale(_)),
        "{:?}",
        opened.gpu_choice_outcome()
    );
    assert_eq!(adapter(&opened).name, ranked.name);
    drop(opened);

    let missing: GpuChoice = choice
        .to_string()
        .replace("name ", "name not this ")
        .parse()
        .expect("a choice");
    let opened = open(Some(&missing), PowerPreference::LowPower);
    assert!(
        matches!(opened.gpu_choice_outcome(), ChoiceOutcome::Stale(_)),
        "{:?}",
        opened.gpu_choice_outcome()
    );
    assert_eq!(adapter(&opened).name, ranked.name);
}

/// **A pin overrides a choice**, which is then not tried at all.
#[test]
#[ignore = "needs Vulkan drivers"]
fn a_pin_overrides_a_saved_choice() {
    let first = open(None, PowerPreference::LowPower);
    let choice = first.gpu_choice().clone();
    drop(first);
    let pinned = Presenter::open_offscreen(
        EXTENT,
        &PresenterDesc {
            backend: Some(GpuBackend::Vulkan),
            adapter_pin: Some("cpu"),
            gpu_choice: Some(&choice),
            ..PresenterDesc::default()
        },
    )
    .expect("a pinned presenter");
    assert!(
        matches!(
            pinned.gpu_choice_outcome(),
            ChoiceOutcome::Ignored(Ignored::Overridden(_))
        ),
        "{:?}",
        pinned.gpu_choice_outcome()
    );
    assert_eq!(adapter(&pinned).device_type, DeviceType::Cpu);
}
