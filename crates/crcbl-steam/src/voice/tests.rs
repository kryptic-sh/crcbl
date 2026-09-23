//! Voice over the fake microphone and decoder.

use super::*;
use crate::{AppId, client::init_on, testing};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn voice<R>(f: impl FnOnce(&mut testing::FakeVoice) -> R) -> R {
    testing::script(|s| f(&mut s.voice))
}

#[test]
fn samples_convert_at_the_endpoints() {
    let bytes: Vec<u8> = [i16::MIN, -16384, 0, 16384, i16::MAX]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let out = samples(&bytes).unwrap();
    assert_eq!(out[0], -1.0);
    assert_eq!(out[1], -0.5);
    assert_eq!(out[2], 0.0);
    assert_eq!(out[3], 0.5);
    assert_eq!(out[4], 32767.0 / 32768.0);
    assert!(out[4] < 1.0);
    assert_eq!(samples(&[1, 2, 3]), Err(VoiceError::OddLength(3)));
}

#[test]
fn each_voice_result_is_its_own_error() {
    for (raw, expected) in [
        (1, VoiceError::NotInitialized),
        (5, VoiceError::DataCorrupted),
        (6, VoiceError::Restricted),
        (7, VoiceError::UnsupportedCodec),
        (8, VoiceError::ReceiverOutOfDate),
        (9, VoiceError::ReceiverDidNotAnswer),
        (42, VoiceError::Other(42)),
    ] {
        assert_eq!(VoiceError::from_result(raw), expected, "{raw}");
    }
}

#[test]
fn push_to_talk_starts_and_stops_only_on_edges() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    mic.set_transmitting(false);
    mic.set_transmitting(true);
    mic.set_transmitting(true);
    mic.set_transmitting(false);
    mic.set_transmitting(false);
    mic.set_transmitting(true);
    assert_eq!(voice(|v| v.log.clone()), ["start", "stop", "start"]);
    drop(mic);
    assert_eq!(
        voice(|v| v.log.clone()),
        ["start", "stop", "start", "stop"],
        "dropping a transmitting capture stops it"
    );
}

#[test]
fn an_idle_capture_calls_nothing_and_one_at_a_time() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    assert_eq!(
        steam.voice().capture().err(),
        Some(VoiceError::AlreadyCapturing)
    );
    voice(|v| v.packets.push_back(vec![1]));
    assert_eq!(mic.poll(), Ok(None), "not transmitting, so not recording");
    assert_eq!(
        voice(|v| v.available_calls),
        0,
        "an idle capture asks Steam nothing"
    );
    drop(mic);
    assert!(voice(|v| v.log.is_empty()), "never started, never stopped");
    assert!(steam.voice().capture().is_ok(), "free again once dropped");
}

#[test]
fn packets_keep_coming_after_release_until_steam_stops_recording() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    mic.set_transmitting(true);
    assert_eq!(mic.poll(), Ok(None), "nothing yet");
    voice(|v| v.packets.extend([vec![1, 2, 3], vec![4], vec![5, 6]]));
    assert_eq!(mic.poll(), Ok(Some(vec![1, 2, 3])));

    mic.set_transmitting(false);
    assert!(mic.recording(), "the tail is still coming");
    assert_eq!(mic.poll(), Ok(Some(vec![4])));
    assert_eq!(mic.poll(), Ok(Some(vec![5, 6])));
    assert_eq!(mic.poll(), Ok(None));
    assert!(!mic.recording(), "Steam said NotRecording");
    assert_eq!(voice(|v| v.deprecated_misuse), 0);
}

#[test]
fn not_recording_while_transmitting_keeps_polling() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    mic.set_transmitting(true);
    voice(|v| v.recording = false);
    assert_eq!(mic.poll(), Ok(None));
    assert!(mic.recording(), "the start has not taken hold; poll again");
    voice(|v| {
        v.recording = true;
        v.packets.push_back(vec![9]);
    });
    assert_eq!(mic.poll(), Ok(Some(vec![9])));
}

#[test]
fn a_small_buffer_grows_and_one_that_never_fits_is_an_error() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    mic.set_transmitting(true);
    voice(|v| {
        v.packets.push_back(vec![7; 10]);
        v.too_small = 2;
    });
    assert_eq!(mic.poll(), Ok(Some(vec![7; 10])));
    assert_eq!(voice(|v| v.offered.clone()), [10, 20, 40]);

    voice(|v| {
        v.packets.push_back(vec![8; 10]);
        v.too_small = u32::MAX;
    });
    assert!(matches!(
        mic.poll(),
        Err(VoiceError::BufferTooSmall { needed }) if needed > MAX_PACKET_BYTES
    ));
}

#[test]
fn a_restricted_account_is_an_error_from_poll() {
    let steam = steam();
    let mut mic = steam.voice().capture().unwrap();
    mic.set_transmitting(true);
    voice(|v| v.available_result = Some(6));
    assert_eq!(mic.poll(), Err(VoiceError::Restricted));
}

#[test]
fn decompress_asks_for_the_rate_and_retries_once_at_steams_size() {
    let steam = steam();
    let pcm: Vec<u8> = [0_i16, i16::MIN]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    voice(|v| v.pcm.clone_from(&pcm));
    assert_eq!(
        steam.voice().decompress(&[1, 2], VOICE_SAMPLE_RATE),
        Ok(vec![0.0, -1.0])
    );
    assert_eq!(
        voice(|v| v.decompressions.clone()),
        [(u32::try_from(DECOMPRESS_START_BYTES).unwrap(), 48_000)]
    );

    // Larger than the first buffer: one retry at the size Steam named.
    let big = vec![0_u8; DECOMPRESS_START_BYTES + 100];
    voice(|v| {
        v.pcm.clone_from(&big);
        v.decompressions.clear();
    });
    assert_eq!(
        steam
            .voice()
            .decompress(&[1], VOICE_SAMPLE_RATE)
            .map(|s| s.len()),
        Ok(big.len() / 2)
    );
    assert_eq!(voice(|v| v.decompressions.len()), 2);

    // A library that is never satisfied: an error, after two tries.
    voice(|v| {
        v.decompress_never_fits = true;
        v.decompressions.clear();
    });
    assert!(matches!(
        steam.voice().decompress(&[1], VOICE_SAMPLE_RATE),
        Err(VoiceError::BufferTooSmall { .. })
    ));
    assert_eq!(voice(|v| v.decompressions.len()), 2);
}

#[test]
fn decompress_refuses_a_rate_outside_the_decoder_before_any_call() {
    let steam = steam();
    for rate in [0, MIN_VOICE_SAMPLE_RATE - 1, MAX_VOICE_SAMPLE_RATE + 1] {
        assert_eq!(
            steam.voice().decompress(&[1], rate),
            Err(VoiceError::SampleRate(rate))
        );
    }
    assert!(voice(|v| v.decompressions.is_empty()));
    voice(|v| v.decompress_result = Some(5));
    assert_eq!(
        steam.voice().decompress(&[1], MIN_VOICE_SAMPLE_RATE),
        Err(VoiceError::DataCorrupted)
    );
}

#[test]
fn the_optimal_rate_is_steams() {
    let steam = steam();
    voice(|v| v.optimal_rate = 24_000);
    assert_eq!(steam.voice().optimal_sample_rate(), 24_000);
}
