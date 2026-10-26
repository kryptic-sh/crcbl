//! A saved GPU choice on D3D12: each case opens an offscreen presenter and
//! reads what became of the choice it was given.
//!
//! Every case but one runs on WARP alone, which every Windows has, so they run
//! by default there. The one that needs a GPU beside the integrated one is
//! ignored; run on the Windows desktop (RX 7900 XTX, Raphael iGPU, WARP):
//!
//! ```text
//! cargo test -p crcbl-app --test gpu_choice_dx12 -- --include-ignored --test-threads=1
//! ```

#![cfg(target_os = "windows")]

use std::time::{Duration, Instant};

use crcbl_app::adapter::PowerPreference;
use crcbl_app::backend::GpuBackend;
use crcbl_app::gpu_choice::{ChoiceOutcome, GpuChoice, Ignored};
use crcbl_app::present::{FrameOutcome, Presenter, PresenterDesc};
use crcbl_dx12::Dx12Instance;
use crcbl_hal::{AdapterInfo, CommandEncoderDesc, DeviceType, Instance as _};

const EXTENT: (u32, u32) = (64, 64);

/// An offscreen presenter on D3D12 with `choice` and `pin`, preferring
/// `preference`.
fn open_pinned(
    choice: Option<&GpuChoice>,
    pin: Option<&str>,
    preference: PowerPreference,
) -> Presenter {
    Presenter::open_offscreen(
        EXTENT,
        &PresenterDesc {
            backend: Some(GpuBackend::Dx12),
            adapter_pin: pin,
            power_preference: preference,
            gpu_choice: choice,
            ..PresenterDesc::default()
        },
    )
    .expect("an offscreen presenter")
}

/// [`open_pinned`] with no pin.
fn open(choice: Option<&GpuChoice>, preference: PowerPreference) -> Presenter {
    open_pinned(choice, None, preference)
}

/// The adapter `presenter` opened on.
fn adapter(presenter: &Presenter) -> AdapterInfo {
    presenter.adapter().expect("an adapter")
}

/// The choice of WARP, from a presenter pinned to it.
fn warp_choice() -> GpuChoice {
    let warp = open_pinned(None, Some("cpu"), PowerPreference::LowPower);
    assert_eq!(adapter(&warp).device_type, DeviceType::Cpu);
    warp.gpu_choice().clone()
}

/// `choice`'s text with the line starting `field ` replaced by `field value`.
fn with_field(choice: &GpuChoice, field: &str, value: &str) -> GpuChoice {
    let prefix = format!("{field} ");
    let text = choice.to_string();
    assert!(text.lines().any(|line| line.starts_with(&prefix)), "{text}");
    text.lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                format!("{prefix}{value}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect::<String>()
        .parse()
        .expect("a choice")
}

/// **A choice is learnt and reopens its adapter alone**: an open with none
/// reports `NotGiven` and hands back the adapter it opened on, with its LUID;
/// given back, it is `Used`, and the instance holds that adapter and no other.
#[test]
fn a_saved_choice_reopens_its_adapter_alone() {
    let choice = warp_choice();
    assert!(choice.luid().is_some(), "{choice}");

    let again = open(Some(&choice), PowerPreference::HighPerformance);
    assert_eq!(again.gpu_choice_outcome(), &ChoiceOutcome::Used);
    assert!(choice.names(&adapter(&again)));
    assert_eq!(again.gpu_choice(), &choice);

    let alone = Dx12Instance::open_adapter(choice.luid(), |adapter| choice.names(adapter))
        .expect("the saved adapter is here");
    let listed = alone.adapters();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert!(choice.names(&listed[0]));
}

/// **A LUID from before a restart is not trusted**: the same adapter under a
/// LUID that names nothing is still found, by vendor, device and name.
#[test]
fn a_saved_choice_outlives_its_luid() {
    let choice = warp_choice();
    let restarted = with_field(&choice, "luid", "0xdead0000beef");
    let again = open(Some(&restarted), PowerPreference::HighPerformance);
    assert_eq!(again.gpu_choice_outcome(), &ChoiceOutcome::Used);
    assert!(choice.names(&adapter(&again)));
    // What the program saves next carries the LUID the adapter has now.
    assert_eq!(again.gpu_choice().luid(), choice.luid());
}

/// **A choice that no longer holds is `Stale`, and the open goes on**: one
/// naming an adapter that is not here opens where the ranking puts it, and
/// hands back that adapter's choice to save in its place.
#[test]
fn a_stale_choice_is_reported_and_the_open_goes_on() {
    let choice = warp_choice();
    let ranked = adapter(&open(None, PowerPreference::HighPerformance));

    let missing = with_field(&choice, "name", "not this adapter");
    let opened = open(Some(&missing), PowerPreference::HighPerformance);
    assert!(
        matches!(opened.gpu_choice_outcome(), ChoiceOutcome::Stale(_)),
        "{:?}",
        opened.gpu_choice_outcome()
    );
    assert_eq!(adapter(&opened).name, ranked.name);
    assert!(opened.gpu_choice().names(&ranked));
}

/// **A pin overrides a choice**, which is then not tried at all: a choice of
/// WARP under a pin of `cpu` is ignored, even though the pin opens WARP too.
#[test]
fn a_pin_overrides_a_saved_choice() {
    let choice = warp_choice();
    let pinned = open_pinned(Some(&choice), Some("cpu"), PowerPreference::LowPower);
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

/// **A choice wins over the ranking, and over a LUID naming another
/// adapter**: the discrete adapter, saved, is opened by a program that asks
/// for low power, which would rank the integrated one first — and still when
/// its saved LUID is the integrated adapter's, as a restart can leave it.
#[test]
#[ignore = "needs a discrete adapter beside an integrated one"]
fn a_saved_choice_wins_over_the_power_preference() {
    let discrete = open(None, PowerPreference::HighPerformance);
    assert_eq!(
        adapter(&discrete).device_type,
        DeviceType::Discrete,
        "no discrete adapter here"
    );
    let choice = discrete.gpu_choice().clone();
    drop(discrete);

    let low = open(None, PowerPreference::LowPower);
    let integrated = adapter(&low);
    assert!(
        !choice.names(&integrated),
        "low power already picks the discrete one"
    );
    let integrated_luid = low.gpu_choice().luid().expect("a D3D12 LUID");
    drop(low);

    let swapped = with_field(&choice, "luid", &format!("{integrated_luid:#x}"));
    for saved in [&choice, &swapped] {
        let reopened = open(Some(saved), PowerPreference::LowPower);
        assert_eq!(reopened.gpu_choice_outcome(), &ChoiceOutcome::Used);
        assert!(saved.names(&adapter(&reopened)), "{saved}");
    }
}

/// How long `open` takes to hand back a presenter and present its first
/// frame, as (to the presenter, to the first present).
fn first_present(open: impl FnOnce() -> Presenter) -> (Duration, Duration) {
    let started = Instant::now();
    let mut presenter = open();
    let opened = started.elapsed();
    let acquired = presenter
        .acquire()
        .expect("acquire")
        .expect("an offscreen frame is always due");
    let encoder = presenter
        .device()
        .create_command_encoder(&CommandEncoderDesc {
            label: Some("first present"),
            queue: presenter.queue(),
        });
    let command_buffer = encoder.finish().expect("an empty command buffer");
    assert_eq!(
        presenter
            .submit_and_present(&acquired, command_buffer)
            .expect("present"),
        FrameOutcome::Presented
    );
    presenter.drain().expect("drain");
    let presented = started.elapsed();
    presenter.destroy().expect("teardown");
    (opened, presented)
}

/// The median of `samples`.
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// **What a saved choice saves**, on each adapter here: instance open, and
/// presenter open to first present, with the choice against without. A
/// measurement, not a check; run with `--nocapture` and read the table.
#[test]
#[ignore = "a timing run, read by hand"]
fn what_a_saved_choice_saves() {
    const RUNS: usize = 15;
    let adapters = Dx12Instance::open().expect("D3D12").adapters();
    for info in adapters {
        let choice = GpuChoice::of(GpuBackend::Dx12, &info);
        let pin = match info.device_type {
            DeviceType::Cpu => "cpu",
            DeviceType::Discrete => "discrete",
            _ => "integrated",
        };
        let mut instance_all = Vec::new();
        let mut instance_alone = Vec::new();
        let mut present_all = Vec::new();
        let mut present_alone = Vec::new();
        for _ in 0..RUNS {
            let started = Instant::now();
            drop(Dx12Instance::open().expect("D3D12"));
            instance_all.push(started.elapsed());
            let started = Instant::now();
            drop(
                Dx12Instance::open_adapter(choice.luid(), |adapter| choice.names(adapter))
                    .expect("the adapter"),
            );
            instance_alone.push(started.elapsed());
            // Without a choice, the pin is what lands on this adapter.
            present_all
                .push(first_present(|| open_pinned(None, Some(pin), PowerPreference::LowPower)).1);
            present_alone.push(first_present(|| open(Some(&choice), PowerPreference::LowPower)).1);
        }
        eprintln!(
            "{name}: instance {all:?} -> {alone:?}; first present {pall:?} -> {palone:?} \
             (median of {RUNS})",
            name = info.name,
            all = median(instance_all),
            alone = median(instance_alone),
            pall = median(present_all),
            palone = median(present_alone),
        );
    }
}
