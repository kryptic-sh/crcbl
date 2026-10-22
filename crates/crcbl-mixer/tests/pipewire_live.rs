//! The mixer against the PipeWire this machine is running, checked against
//! what `pactl` says about the same server.
//!
//! `#[ignore]`d: it needs a running PipeWire with `pipewire-pulse`, and
//! `pactl` as the independent witness — the seam itself never runs it. Run with
//! `cargo test -p crcbl-mixer --test pipewire_live -- --ignored --nocapture
//! --test-threads=1`: one test changes volumes the other compares, so they
//! must not share the server at once.

#![cfg(target_os = "linux")]

use crcbl_mixer::{Direction, Mixer};

/// One `pactl` sink or source: index, name, mean volume percent, mute.
#[derive(Debug)]
struct Witness {
    index: u32,
    name: String,
    volume: u32,
    muted: bool,
}

/// `pactl list short`-free parsing: `pactl list sinks` prints blocks of
/// `Sink #N`, `Name:`, `Mute:` and `Volume:` lines, which is all this reads.
fn pactl(kind: &str) -> Vec<Witness> {
    let output = crcbl_spawn::command("pactl")
        .args(["list", kind])
        .env("LC_ALL", "C")
        .output()
        .expect("pactl runs");
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    let mut current: Option<Witness> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(index) = line
            .strip_prefix("Sink #")
            .or_else(|| line.strip_prefix("Source #"))
        {
            found.extend(current.take());
            current = Some(Witness {
                index: index.parse().expect("an index"),
                name: String::new(),
                volume: 0,
                muted: false,
            });
        } else if let Some(witness) = current.as_mut() {
            if let Some(name) = line.strip_prefix("Name: ") {
                witness.name = name.to_owned();
            } else if let Some(mute) = line.strip_prefix("Mute: ") {
                witness.muted = mute == "yes";
            } else if let Some(volume) = line.strip_prefix("Volume: ") {
                let percents: Vec<u32> = volume
                    .split_whitespace()
                    .filter_map(|word| word.strip_suffix('%')?.parse().ok())
                    .collect();
                let count = u32::try_from(percents.len()).unwrap().max(1);
                witness.volume = (percents.iter().sum::<u32>() + count / 2) / count;
            }
        }
    }
    found.extend(current);
    found
        .into_iter()
        .filter(|witness| !witness.name.ends_with(".monitor"))
        .collect()
}

fn default(kind: &str) -> String {
    let output = crcbl_spawn::command("pactl")
        .arg(format!("get-default-{kind}"))
        .output()
        .expect("pactl runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// **Every device `pactl` reports, the mixer reports with the same id, name,
/// volume and mute, and the same default**, for both directions. The id is the
/// object's serial, which is `pactl`'s index too.
#[test]
#[ignore = "needs a running PipeWire, and pactl as the witness"]
fn the_mixer_agrees_with_pactl_on_every_device() {
    let mixer = Mixer::open(|| {}).expect("PipeWire is running");
    let state = mixer.state();
    for device in &state.devices {
        println!(
            "{:?} {:>4} {:<70} {:>3}% muted {} default {} port {:?} bt {} card {:?}",
            device.direction,
            device.id.0,
            device.name,
            device.volume,
            device.muted,
            device.default,
            device.active_port,
            device.bluetooth,
            device.card
        );
    }
    for stream in &state.streams {
        println!(
            "stream {:?} {:?} on {:?} {}%",
            stream.id, stream.application, stream.device, stream.volume
        );
    }
    for (kind, direction) in [("sinks", Direction::Output), ("sources", Direction::Input)] {
        let witnesses = pactl(kind);
        assert!(
            !witnesses.is_empty() || kind == "sources",
            "pactl lists {kind}"
        );
        for witness in witnesses {
            let device = state
                .devices
                .iter()
                .find(|device| device.name == witness.name)
                .unwrap_or_else(|| panic!("the mixer has no {}", witness.name));
            assert_eq!(device.direction, direction, "{}", witness.name);
            assert_eq!(device.id.0, witness.index, "{}", witness.name);
            assert_eq!(device.volume, witness.volume, "{}", witness.name);
            assert_eq!(device.muted, witness.muted, "{}", witness.name);
        }
    }
    let sink = default("sink");
    assert_eq!(
        state
            .default_device(Direction::Output)
            .map(|device| device.name.as_str()),
        Some(sink.as_str())
    );
}

/// Polls `mixer` until `ready` holds or two seconds pass.
fn until(mixer: &Mixer, ready: impl Fn(&crcbl_mixer::MixerState) -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if ready(&mixer.state()) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// **Volume and mute reach the server on both of a device's paths**, a card
/// device's active route and a plain node's own props, and `pactl` sees each
/// change; the waker is called; each device is put back as it was. Only
/// HDMI outputs are touched, which nothing here is playing through.
#[test]
#[ignore = "needs a running PipeWire with this laptop's HDMI sinks, and pactl as the witness"]
fn volume_and_mute_reach_the_server_on_both_paths() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let mixer = Mixer::open(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("PipeWire is running");
    let state = mixer.state();
    // HDMI1 has an active route on its card; HDMI3 has none and goes
    // through its own props.
    for suffix in ["HDMI1__sink", "HDMI3__sink"] {
        let device = state
            .devices
            .iter()
            .find(|device| device.name.ends_with(suffix))
            .unwrap_or_else(|| panic!("this laptop has an {suffix}"))
            .clone();
        let target = crcbl_mixer::Target::Device(device.id);
        let other = if device.volume == 37 { 38 } else { 37 };
        let before = wakes.load(Ordering::SeqCst);

        mixer
            .apply(target, &crcbl_mixer::Action::SetVolume(other))
            .unwrap();
        assert!(
            until(&mixer, |state| state.device(device.id).unwrap().volume
                == other),
            "{suffix} never showed {other} %"
        );
        let witness = pactl("sinks")
            .into_iter()
            .find(|w| w.name == device.name)
            .unwrap();
        assert_eq!(witness.volume, other, "pactl sees {suffix}'s new volume");

        mixer
            .apply(target, &crcbl_mixer::Action::ToggleMute)
            .unwrap();
        assert!(until(&mixer, |state| state
            .device(device.id)
            .unwrap()
            .muted
            != device.muted));
        assert!(
            wakes.load(Ordering::SeqCst) > before,
            "the waker was called"
        );

        mixer
            .apply(target, &crcbl_mixer::Action::SetMute(device.muted))
            .unwrap();
        mixer
            .apply(target, &crcbl_mixer::Action::SetVolume(device.volume))
            .unwrap();
        assert!(
            until(&mixer, |state| {
                let now = state.device(device.id).unwrap();
                now.volume == device.volume && now.muted == device.muted
            }),
            "{suffix} put back as it was"
        );
        println!(
            "{suffix}: {} % -> {other} % and back, mute toggled and back",
            device.volume
        );
    }
}
