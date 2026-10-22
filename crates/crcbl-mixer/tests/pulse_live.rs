//! The mixer's PulseAudio backend against a real PulseAudio daemon — not
//! `pipewire-pulse` — checked against what `pactl` says about it.
//!
//! `#[ignore]`d, and it changes the server it runs against: defaults, volumes,
//! mutes, a stream's sink. So it refuses to run unless `PULSE_SERVER` names the
//! server, never the session's own. `docs/backlog.md` ("System audio control")
//! has the recipe for a disposable daemon in a container, with two null sinks
//! and a null source: run with `PULSE_SERVER=unix:… PULSE_COOKIE=…
//! CRCBL_MIXER_BACKEND=pulse cargo test -p crcbl-mixer --test pulse_live --
//! --ignored --test-threads=1`.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crcbl_mixer::{Action, Direction, Mixer, Target};

#[path = "support/pactl.rs"]
mod pactl;
use pactl::{default, pactl, sink_inputs, until};

/// The mixer on the server `PULSE_SERVER` names, and how often it woke.
fn open() -> (Mixer, Arc<AtomicUsize>) {
    assert!(
        std::env::var_os("PULSE_SERVER").is_some(),
        "this test changes the server it runs on; name a disposable one in PULSE_SERVER"
    );
    assert_eq!(
        std::env::var("CRCBL_MIXER_BACKEND").as_deref(),
        Ok("pulse"),
        "the PulseAudio backend is what this tests"
    );
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let mixer = Mixer::open(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("the server admits the cookie");
    (mixer, wakes)
}

/// **Every action a desktop mixer takes reaches a PulseAudio daemon, and
/// `pactl` and the mixer then agree**: a volume and a mute on an output and
/// on an input, the default output, and a stream moved between outputs. Each
/// is put back.
#[test]
#[ignore = "needs a disposable PulseAudio daemon named by PULSE_SERVER, and pactl"]
fn every_action_reaches_a_pulseaudio_daemon() {
    let (mixer, wakes) = open();
    let state = mixer.state();
    let first_default = default("sink");
    let output = state
        .devices
        .iter()
        .find(|device| device.direction == Direction::Output && !device.default)
        .expect("an output besides the default")
        .clone();
    let input = state
        .devices
        .iter()
        .find(|device| device.direction == Direction::Input)
        .expect("an input")
        .clone();

    for (device, kind) in [(&output, "sinks"), (&input, "sources")] {
        let witness = |name: &str| {
            pactl(kind)
                .into_iter()
                .find(|witness| witness.name == name)
                .expect("pactl lists it")
        };
        let target = Target::Device(device.id);
        mixer.apply(target, &Action::SetVolume(37)).unwrap();
        mixer.apply(target, &Action::SetMute(true)).unwrap();
        assert!(until(&mixer, |state| state
            .device(device.id)
            .is_some_and(|now| now.volume == 37 && now.muted)));
        let seen = witness(&device.name);
        assert_eq!(seen.index, device.id.0, "the id is pactl's index");
        assert_eq!((seen.volume, seen.muted), (37, true), "{}", device.name);
        mixer
            .apply(target, &Action::SetVolume(device.volume))
            .unwrap();
        mixer.apply(target, &Action::SetMute(device.muted)).unwrap();
        assert!(until(&mixer, |state| state.device(device.id).is_some_and(
            |now| now.volume == device.volume && now.muted == device.muted
        )));
    }

    mixer
        .apply(Target::Device(output.id), &Action::MakeDefault)
        .unwrap();
    assert!(until(&mixer, |state| state
        .default_device(Direction::Output)
        .is_some_and(|now| now.id == output.id)));
    assert_eq!(default("sink"), output.name);
    let back = state
        .devices
        .iter()
        .find(|device| device.name == first_default)
        .expect("the first default");
    mixer
        .apply(Target::Device(back.id), &Action::MakeDefault)
        .unwrap();
    assert!(until(&mixer, |state| state
        .default_device(Direction::Output)
        .is_some_and(|now| now.id == back.id)));

    let mut player = crcbl_spawn::command("pacat")
        .args(["--playback", "--raw", "--device", &back.name, "/dev/zero"])
        .spawn()
        .expect("pacat runs");
    assert!(
        until(&mixer, |state| state
            .streams
            .iter()
            .any(|stream| stream.device == Some(back.id))),
        "the mixer sees pacat's stream"
    );
    let stream = mixer
        .state()
        .streams
        .into_iter()
        .find(|stream| stream.device == Some(back.id))
        .expect("pacat's stream");
    mixer
        .apply(Target::Stream(stream.id), &Action::MoveTo(output.id))
        .unwrap();
    assert!(until(&mixer, |state| state
        .stream(stream.id)
        .is_some_and(|now| now.device == Some(output.id))));
    assert!(
        sink_inputs().contains(&(stream.id.0, output.id.0)),
        "pactl sees it moved: {:?}",
        sink_inputs()
    );
    player.kill().expect("pacat stops");
    player.wait().expect("pacat is reaped");
    assert!(until(&mixer, |state| state.stream(stream.id).is_none()));
    assert!(wakes.load(Ordering::SeqCst) > 0, "the changes woke the app");
}
