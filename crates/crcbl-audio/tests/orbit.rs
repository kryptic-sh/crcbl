//! Grammar-trainer integration tests: full spatial cue → mixer chain.
//!
//! These exercise the audio pipeline end-to-end: emitter orbiting the
//! listener produces deterministic, spatially correct stereo output.

use crcbl_audio::AudioSource;
use crcbl_audio::event::AudioEvent;
use crcbl_audio::mixer::{Mixer, SoundBank};
use crcbl_audio::spatial::{CueGrammar, compute_cue};
use crcbl_audio::{AudioSample, CHANNELS};

/// Generate a 220 Hz monotone sine wave — easy to spot in golden output.
fn tone_220hz(sample_rate: u32, duration_samples: usize) -> Vec<AudioSample> {
    let freq = 220.0;
    let mut out = Vec::with_capacity(duration_samples * CHANNELS);
    for n in 0..duration_samples {
        let t = n as f32 / sample_rate as f32;
        let v = (t * freq * 2.0 * std::f32::consts::PI).sin() * 0.3;
        out.push(v);
        out.push(v);
    }
    out
}

/// Orbit a mono emitter around the listener at a fixed radius and rate.
/// Returns pre-computed `AudioEvent`s, one per step.
fn orbit_events(radius: f32, steps: usize, sound_id: u32) -> Vec<AudioEvent> {
    (0..steps)
        .map(|i| {
            let angle = (i as f32 / steps as f32) * 2.0 * std::f32::consts::PI;
            let x = radius * angle.sin();
            let z = radius * angle.cos();
            AudioEvent {
                sound_id,
                position: [x, 0.0, z],
                range: 0.0,
                volume: 1.0,
                pitch: 1.0,
            }
        })
        .collect()
}

/// Hash a stereo output buffer for determinism checks.
fn hash_buffer(buf: &[AudioSample]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for &s in buf {
        s.to_bits().hash(&mut h);
    }
    h.finish()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[test]
fn orbit_cue_changes_over_time() {
    // A single voice, panned around the listener each block, should
    // produce different hash per block as the cue changes.
    let sample_rate = 48_000;
    let block = 256;
    let grammar = CueGrammar::default();

    let mut bank = SoundBank::new();
    bank.insert(1, tone_220hz(sample_rate, block * 10)); // 10 blocks worth

    let events = orbit_events(5.0, 8, 1);
    assert_eq!(events.len(), 8, "8 steps in orbit");

    let mut hashes = Vec::new();
    let listener = [0.0f32, 0.0, 0.0];

    for event in &events {
        let mut mixer = Mixer::new();
        let cue = compute_cue(listener, event.position, &grammar);
        let voice = bank
            .create_voice(event.sound_id)
            .unwrap()
            .with_gains(cue.gain_left, cue.gain_right)
            .with_pitch(cue.pitch_ratio)
            .with_volume(cue.volume * event.volume);

        mixer.play(voice);

        let mut buf = vec![0.0f32; block * CHANNELS];
        mixer.fill(&mut buf, sample_rate);
        hashes.push(hash_buffer(&buf));
    }

    // At least one hash should differ from the first — the cue changes.
    assert!(
        hashes[1..].iter().any(|h| *h != hashes[0]),
        "orbit should produce different cues over time"
    );
}

#[test]
fn centre_position_is_symmetric() {
    // Dead centre: left == right.
    let sample_rate = 48_000;
    let block = 256;
    let grammar = CueGrammar::default();

    let mut bank = SoundBank::new();
    bank.insert(1, tone_220hz(sample_rate, block * 2));

    let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, 2.0], &grammar);
    let voice = bank
        .create_voice(1)
        .unwrap()
        .with_gains(cue.gain_left, cue.gain_right)
        .with_volume(cue.volume);

    let mut mixer = Mixer::new();
    mixer.play(voice);

    let mut buf = vec![0.0f32; block * CHANNELS];
    mixer.fill(&mut buf, sample_rate);

    // Left and right channels should be equal (symmetric, no pan).
    for chunk in buf.chunks_exact(2) {
        assert!((chunk[0] - chunk[1]).abs() < 1e-6, "centre: left != right");
    }
}

#[test]
fn right_position_pans_to_right() {
    // Source to the right → right channel should be stronger.
    let sample_rate = 48_000;
    let block = 256;
    let grammar = CueGrammar::default();

    let mut bank = SoundBank::new();
    // Pure DC for simple power comparison.
    let dc: Vec<AudioSample> = vec![0.5; block * 2 * CHANNELS];
    bank.insert(1, dc);

    let cue = compute_cue([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], &grammar);
    let voice = bank
        .create_voice(1)
        .unwrap()
        .with_gains(cue.gain_left, cue.gain_right)
        .with_volume(cue.volume);

    let mut mixer = Mixer::new();
    mixer.play(voice);

    let mut buf = vec![0.0f32; block * CHANNELS];
    mixer.fill(&mut buf, sample_rate);

    let mut left_sum = 0.0f64;
    let mut right_sum = 0.0f64;
    for chunk in buf.chunks_exact(2) {
        left_sum += chunk[0] as f64;
        right_sum += chunk[1] as f64;
    }
    assert!(
        right_sum > left_sum,
        "right source: right should be louder (L={left_sum}, R={right_sum})"
    );
}

#[test]
fn orbit_integration_deterministic() {
    // Full orbit twice → same hash both times.
    let grammar = CueGrammar::default();

    let mut bank = SoundBank::new();
    bank.insert(1, tone_220hz(48_000, 256 * 20));

    fn run_orbit(bank: &SoundBank, grammar: &CueGrammar) -> u64 {
        let events = orbit_events(5.0, 8, 1);
        let listener = [0.0f32, 0.0, 0.0];
        let block = 256;
        let sample_rate = 48_000;

        let mut final_hash = 0u64;
        for event in &events {
            let mut mixer = Mixer::new();
            let cue = compute_cue(listener, event.position, grammar);
            let voice = bank
                .create_voice(event.sound_id)
                .unwrap()
                .with_gains(cue.gain_left, cue.gain_right)
                .with_pitch(cue.pitch_ratio)
                .with_volume(cue.volume * event.volume);
            mixer.play(voice);
            let mut buf = vec![0.0f32; block * CHANNELS];
            mixer.fill(&mut buf, sample_rate);
            final_hash ^= hash_buffer(&buf);
        }
        final_hash
    }

    let hash_a = run_orbit(&bank, &grammar);
    let hash_b = run_orbit(&bank, &grammar);
    assert_eq!(hash_a, hash_b, "orbit should be deterministic");
}

#[test]
fn sound_bank_clone_voice_independent() {
    let mut bank = SoundBank::new();
    bank.insert(1, vec![0.5f32; 64 * CHANNELS]);

    let voice_a = bank.create_voice(1).unwrap();
    let voice_b = bank.create_voice(1).unwrap().with_volume(0.0);

    // voice_a at full volume, voice_b silent — they don't interfere.
    let mut mixer = Mixer::new();
    mixer.play(voice_a);
    mixer.play(voice_b);

    let mut buf = vec![0.0f32; 32 * CHANNELS];
    mixer.fill(&mut buf, sample_rate());

    for &s in &buf {
        assert!((s - 0.5).abs() < 0.001);
    }
}

fn sample_rate() -> u32 {
    48_000
}
