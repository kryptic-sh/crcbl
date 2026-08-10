//! The full spatial cue → mixer chain, end to end.
//!
//! An emitter's world position becomes a cue, the cue drives a voice, and the
//! voices mix down to stereo. These hold the whole chain to being spatially
//! correct (centre is symmetric, right pans right) and deterministic (the same
//! event stream hashes the same, and a reordered one does not).
//!
//! # Why it is a separate target
//!
//! Every step above is a `pub` item — `compute_cue`, `CueGrammar`, `SoundBank`,
//! `Mixer` — and the claim is about the *seam between* them, not about any one.
//! Compiling as a separate crate is what keeps it that way: an in-crate module
//! could drive the mixer past the cue stage, which is the join most likely to
//! break.
//!
//! Circular motion is the input several of these use, because it sweeps every
//! pan position without a table of hand-picked ones. It is a fixture, not the
//! subject — the file is not about orbits, and "orbit" in this workspace is
//! `docs/plan/12-testing.md`'s anchor term for `crcbl-phys`'s analytic cases.

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
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    hash_buffer_into(&mut h, buf);
    h.finish()
}

/// Fold a stereo output buffer into a running hash.
///
/// Feeding every block into one hasher keeps the result sensitive to the
/// *order* of the blocks; combining per-block hashes with XOR would not, and a
/// spatialiser that played an orbit backwards would still match.
fn hash_buffer_into<H: std::hash::Hasher>(hasher: &mut H, buf: &[AudioSample]) {
    use std::hash::Hash;
    buf.len().hash(hasher);
    for &s in buf {
        s.to_bits().hash(hasher);
    }
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
        let mixer = Mixer::new();
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

    let mixer = Mixer::new();
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

    let mixer = Mixer::new();
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
fn an_orbit_hashes_the_same_twice_and_differently_in_reverse() {
    // Full orbit twice → same hash both times.
    let grammar = CueGrammar::default();

    let mut bank = SoundBank::new();
    bank.insert(1, tone_220hz(48_000, 256 * 20));

    fn run_orbit(bank: &SoundBank, grammar: &CueGrammar, events: &[AudioEvent]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let listener = [0.0f32, 0.0, 0.0];
        let block = 256;
        let sample_rate = 48_000;

        // One hasher fed in block order, so the digest depends on the sequence
        // and not merely on the set of blocks.
        let mut hasher = DefaultHasher::new();
        for event in events {
            let mixer = Mixer::new();
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
            hash_buffer_into(&mut hasher, &buf);
        }
        hasher.finish()
    }

    let events = orbit_events(5.0, 8, 1);
    let hash_a = run_orbit(&bank, &grammar, &events);
    let hash_b = run_orbit(&bank, &grammar, &events);
    assert_eq!(hash_a, hash_b, "orbit should be deterministic");

    // Order sensitivity: the same blocks in the reverse order are a different
    // orbit and must hash differently. An XOR fold reported them as identical.
    let mut reversed = events.clone();
    reversed.reverse();
    assert_ne!(
        hash_a,
        run_orbit(&bank, &grammar, &reversed),
        "orbit hash must depend on block order"
    );
}

/// Two voices over one banked sound share the samples and nothing else: the
/// volume set on the second must not reach the first.
#[test]
fn sound_bank_voices_share_samples_but_not_state() {
    let mut bank = SoundBank::new();
    bank.insert(1, vec![0.5f32; 64 * CHANNELS]);

    let voice_a = bank.create_voice(1).unwrap();
    let voice_b = bank.create_voice(1).unwrap().with_volume(0.0);

    // voice_a at full volume, voice_b silent — they don't interfere.
    let mixer = Mixer::new();
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
