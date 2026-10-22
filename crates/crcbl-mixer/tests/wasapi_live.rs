//! The mixer against this machine's Windows audio, checked against what
//! WASAPI says when asked directly.
//!
//! `#[ignore]`d: it reads, and one test briefly changes, the real system
//! mixer. The witness is the `windows` crate called from the test itself —
//! the device enumerator, each endpoint's property store and volume — so the
//! mixer's tables, threads and callbacks are what is under test. Run with
//! `cargo test -p crcbl-mixer --test wasapi_live -- --ignored --nocapture
//! --test-threads=1`: one test changes the volume the other compares.

#![cfg(windows)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crcbl_mixer::{Action, Direction, Mixer, MixerState, Target};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
    eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ,
};

/// The test thread's membership of COM's multithreaded apartment.
struct Apartment;

impl Apartment {
    fn join() -> Self {
        // SAFETY: no reserved pointer; balanced by the `CoUninitialize` in
        // `drop`, on this thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .expect("the test thread joins COM");
        Self
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances `join`'s successful call, on the same thread, after
        // every COM object the test made has dropped (they are declared after
        // the apartment).
        unsafe { CoUninitialize() }
    }
}

/// What WASAPI says of one endpoint.
#[derive(Debug)]
struct Witness {
    id: String,
    direction: Direction,
    friendly_name: String,
    scalar: f32,
    muted: bool,
    default: bool,
}

fn enumerator() -> IMMDeviceEnumerator {
    // SAFETY: a CLSID constant and no outer object.
    unsafe { windows::Win32::System::Com::CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
        .expect("the device enumerator")
}

fn id(device: &IMMDevice) -> String {
    // SAFETY: COM hands over a terminated string the caller owns; it is read,
    // then freed once.
    unsafe {
        let text = device.GetId().expect("an endpoint id");
        let id = text.to_string().expect("UTF-16");
        CoTaskMemFree(Some(text.0.cast_const().cast()));
        id
    }
}

fn volume(device: &IMMDevice) -> IAudioEndpointVolume {
    // SAFETY: no activation parameters.
    unsafe { device.Activate(CLSCTX_ALL, None) }.expect("the endpoint's volume")
}

fn default_id(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    // SAFETY: plain values.
    unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) }
        .ok()
        .map(|device| id(&device))
}

/// Every active endpoint, asked of WASAPI directly.
fn witnesses(enumerator: &IMMDeviceEnumerator) -> Vec<Witness> {
    let mut found = Vec::new();
    for (flow, direction) in [(eRender, Direction::Output), (eCapture, Direction::Input)] {
        let default = default_id(enumerator, flow);
        // SAFETY: plain calls and reads; the index stays below the count.
        unsafe {
            let listed = enumerator
                .EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)
                .expect("the endpoints");
            for index in 0..listed.GetCount().expect("a count") {
                let device = listed.Item(index).expect("an endpoint");
                let volume = volume(&device);
                let id = id(&device);
                found.push(Witness {
                    default: default.as_deref() == Some(id.as_str()),
                    friendly_name: device
                        .OpenPropertyStore(STGM_READ)
                        .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    scalar: volume.GetMasterVolumeLevelScalar().expect("a volume"),
                    muted: volume.GetMute().expect("a mute").as_bool(),
                    direction,
                    id,
                });
            }
        }
    }
    found
}

fn print(state: &MixerState) {
    for device in &state.devices {
        println!(
            "{:?} {:>3} {:<40} {:>3}% muted {} default {} form {:?} bt {}",
            device.direction,
            device.id.0,
            device.description,
            device.volume,
            device.muted,
            device.default,
            device.form_factor,
            device.bluetooth
        );
    }
    for stream in &state.streams {
        println!(
            "stream {:>3} {:?} {:?} on {:?} {}% muted {}",
            stream.id.0,
            stream.direction,
            stream.application,
            stream.device,
            stream.volume,
            stream.muted
        );
    }
}

/// **Every active endpoint WASAPI reports, the mixer reports with the same
/// id string, friendly name, volume, mute and default**, for both
/// directions, and nothing more.
#[test]
#[ignore = "reads the real system mixer, through WASAPI as the witness"]
fn the_mixer_agrees_with_wasapi_on_every_endpoint() {
    let _apartment = Apartment::join();
    let enumerator = enumerator();
    let mixer = Mixer::open(|| {}).expect("the audio service is running");
    let state = mixer.state();
    print(&state);
    let witnesses = witnesses(&enumerator);
    assert!(!witnesses.is_empty(), "this machine has an audio endpoint");
    for witness in &witnesses {
        let device = state
            .devices
            .iter()
            .find(|device| device.name == witness.id)
            .unwrap_or_else(|| panic!("the mixer has no {witness:?}"));
        assert_eq!(device.direction, witness.direction, "{witness:?}");
        assert_eq!(device.description, witness.friendly_name, "{witness:?}");
        assert_eq!(
            device.volume,
            (witness.scalar * 100.0).round() as u32,
            "{witness:?}"
        );
        assert_eq!(device.muted, witness.muted, "{witness:?}");
        assert_eq!(device.default, witness.default, "{witness:?}");
        assert!(device.ports.is_empty() && device.card.is_none());
    }
    assert_eq!(state.devices.len(), witnesses.len(), "and nothing more");
    assert!(state.cards.is_empty(), "Windows reports no cards");
    for stream in &state.streams {
        assert!(
            stream.device.is_some_and(|id| state.device(id).is_some()),
            "{stream:?} is on an endpoint the mixer has"
        );
    }
    println!("{} endpoints agree", witnesses.len());
}

/// Puts the default output's exact scalar volume and mute back when dropped,
/// a panic included.
struct Restore {
    volume: IAudioEndpointVolume,
    scalar: f32,
    muted: bool,
}

impl Drop for Restore {
    fn drop(&mut self) {
        // SAFETY: plain values and no event context, on the thread that made
        // the volume control.
        let put_back = unsafe {
            self.volume
                .SetMasterVolumeLevelScalar(self.scalar, std::ptr::null())
                .and_then(|()| self.volume.SetMute(self.muted, std::ptr::null()))
        };
        // Not a panic: this runs while a failed assertion unwinds too, where
        // a second panic would abort before the report.
        if let Err(error) = put_back {
            eprintln!(
                "could not put the default output back to {} muted {}: {error}",
                self.scalar, self.muted
            );
        }
    }
}

/// Polls until `ready` holds of the mixer's state or two seconds pass.
fn until(mixer: &Mixer, ready: impl Fn(&MixerState) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if ready(&mixer.state()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// **Volume and mute set through the mixer reach Windows, wake the app and
/// show in the state**, on the default output, which is then put back to its
/// exact scalar and mute. The volume goes down a little rather than up, so
/// whatever is playing gets no louder.
#[test]
#[ignore = "changes the real default output's volume and mute for a moment, then restores them"]
fn volume_and_mute_reach_windows_and_wake_the_app() {
    let _apartment = Apartment::join();
    let enumerator = enumerator();
    // SAFETY: plain values.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .expect("this machine has a default output");
    let id = id(&device);
    let witness = volume(&device);
    // SAFETY: plain reads.
    let (scalar, muted) = unsafe {
        (
            witness.GetMasterVolumeLevelScalar().expect("a volume"),
            witness.GetMute().expect("a mute").as_bool(),
        )
    };
    let restore = Restore {
        volume: witness.clone(),
        scalar,
        muted,
    };

    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let mixer = Mixer::open(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("the audio service is running");
    let before = mixer
        .state()
        .default_device(Direction::Output)
        .cloned()
        .expect("the mixer has the default output");
    assert_eq!(before.name, id);
    let other = if before.volume >= 5 {
        before.volume - 3
    } else {
        before.volume + 3
    };
    let target = Target::Default(Direction::Output);

    mixer.apply(target, &Action::SetVolume(other)).unwrap();
    assert!(
        until(&mixer, |state| state.device(before.id).unwrap().volume
            == other),
        "the state never showed {other} %"
    );
    // SAFETY: a plain read.
    let seen = unsafe { witness.GetMasterVolumeLevelScalar() }.unwrap();
    assert_eq!((seen * 100.0).round() as u32, other, "WASAPI sees it");
    let after_volume = wakes.load(Ordering::SeqCst);
    assert!(after_volume > 0, "the waker was called");

    mixer.apply(target, &Action::ToggleMute).unwrap();
    assert!(
        until(&mixer, |state| state.device(before.id).unwrap().muted
            != muted),
        "the state never showed the mute toggled"
    );
    // SAFETY: a plain read.
    let seen = unsafe { witness.GetMute() }.unwrap().as_bool();
    assert_eq!(seen, !muted, "WASAPI sees the mute");
    assert!(wakes.load(Ordering::SeqCst) > after_volume, "woken again");

    let stepped = other.saturating_sub(2);
    mixer.apply(target, &Action::Step(-2)).unwrap();
    assert!(
        until(&mixer, |state| state.device(before.id).unwrap().volume
            == stepped),
        "the state never showed the step to {stepped} %"
    );

    drop(restore);
    // SAFETY: plain reads.
    let (now, now_muted) = unsafe {
        (
            witness.GetMasterVolumeLevelScalar().unwrap(),
            witness.GetMute().unwrap().as_bool(),
        )
    };
    assert!(
        now.to_bits() == scalar.to_bits() && now_muted == muted,
        "put back exactly: {now} muted {now_muted}, was {scalar} muted {muted}"
    );
    assert!(
        until(&mixer, |state| {
            let device = state.device(before.id).unwrap();
            device.volume == before.volume && device.muted == before.muted
        }),
        "the state shows it put back"
    );
    println!(
        "{}: {} % -> {other} % -> {stepped} % and back (scalar {scalar}), mute {muted} -> {} and back, {} wakes",
        before.description,
        before.volume,
        !muted,
        wakes.load(Ordering::SeqCst)
    );
}
