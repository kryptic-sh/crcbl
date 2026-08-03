//! Waveform generators: the small set of sounds a game makes before it has an
//! artist.
//!
//! ```text
//! sine(freq, seconds)        ──▶ a beep, faded at both ends
//! looped_sine(freq, cycles)  ──▶ a tone that joins to itself, un-faded
//! noise_burst(seconds, …)    ──▶ a hit or an explosion, decaying
//! ```
//!
//! Every generator returns **interleaved stereo** at the rate it is asked for,
//! which is what [`mixer::Voice`](crate::mixer::Voice) plays: a mono buffer
//! reaches the playhead as a stereo one of half the length, and comes out an
//! octave low and twice as fast. That mistake has been made in this workspace
//! before, so each generator's test asserts the pairing rather than the length
//! alone.
//!
//! # Why the engine owns this at all
//!
//! Because four games wrote it and three of them wrote it *identically*.
//! `fn sine` and its fade helper were byte-for-byte the same file in
//! `apps/flappy`, `apps/asteroids` and `apps/horde`, and `apps/breakout` had
//! the same two functions under the names `gen_sine` and `fade_env`. A mixer, a
//! sound bank and a spatial cue grammar were already here; the thing that makes
//! a *sound* was the one piece left to the caller, so every caller wrote it.
//!
//! # What is deliberately not here
//!
//! **A synthesiser.** No envelope generator with four segments, no filter
//! bank, no oscillator type to configure. These are three functions that
//! produce three sounds, because three is what the samples between them
//! actually use; a game wanting a fourth writes it, and when two of them want
//! the same fourth it comes here.
//!
//! **Loudness.** The amplitudes below are fixed, and a cue that should be
//! quieter is played quieter — [`VoiceMix::volume`](crate::mixer::VoiceMix)
//! is the control, and it applies after the spatial grammar's distance rolloff
//! rather than fighting it.

use crate::{AudioSample, CHANNELS};

/// Peak amplitude of the tone generators, before any voice gain.
///
/// Well under full scale because several cues overlap: a game that plays four
/// at once at 1.0 clips, and clipping is the one artefact a listener cannot
/// mistake for a choice.
///
/// Public because a game that writes a generator of its own has to sit at the
/// same level as the ones here, and a bank where one cue is written `0.3` and
/// another names this constant is a bank with two levels the day either moves.
/// `apps/horde`'s swept `rise` is the case that exists.
pub const TONE_AMPLITUDE: f32 = 0.3;

/// Peak amplitude of [`noise_burst`], before its decay.
///
/// Louder than [`TONE_AMPLITUDE`] because a burst is broadband and short: its
/// energy is spread over the spectrum and gone in a fraction of a second, so at
/// equal peak it reads as quieter than a tone does.
const NOISE_AMPLITUDE: f32 = 0.45;

/// The one-pole low-pass coefficient in [`noise_burst`]: `y += ALPHA * (x - y)`.
///
/// Lower is duller. This takes the hiss off the top so a burst reads as an
/// impact rather than as static.
const NOISE_LOWPASS_ALPHA: f32 = 0.16;

/// How many frames [`fade_gain`] takes at each end, unless the sound is shorter.
///
/// 60 frames is 1.25 ms at 48 kHz — long enough that the step from silence is
/// not a click, short enough that it does not soften an attack.
pub const FADE_FRAMES: usize = 60;

/// A linear fade in and out, so a cue starts and stops without a click.
///
/// Returns the gain to apply at `frame` of a sound `total` frames long: it
/// ramps up over the first [`FADE_FRAMES`], holds at 1.0, and ramps back down
/// over the last.
///
/// # Panics
///
/// In debug builds, if `frame` is not inside the sound. The gain outside a
/// sound is not zero or one, it is undefined, and returning either would hide
/// the caller's off-by-one.
#[must_use]
pub fn fade_gain(frame: usize, total: usize) -> f32 {
    debug_assert!(frame < total, "fade_gain is only defined inside the sound");
    // `min(total / 2)` because a sound shorter than two fades has no middle;
    // `max(1)` because a zero-length fade divides by zero.
    let fade = FADE_FRAMES.min(total / 2).max(1);
    let from_end = total - frame;
    if frame < fade {
        frame as f32 / fade as f32
    } else if from_end <= fade {
        from_end as f32 / fade as f32
    } else {
        1.0
    }
}

/// A mono sine wave, faded at both ends, as interleaved stereo.
///
/// The one-shot generator: a beep, a bounce, a pickup. For a sound that has to
/// play end-to-end without a seam, use [`looped_sine`] — the fade this applies
/// is exactly what a loop must not have.
#[must_use]
pub fn sine(freq_hz: f32, seconds: f32, sample_rate: u32) -> Vec<AudioSample> {
    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * CHANNELS);
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let value = TONE_AMPLITUDE
            * (2.0 * std::f32::consts::PI * freq_hz * t).sin()
            * fade_gain(i, frames);
        out.push(value);
        out.push(value);
    }
    out
}

/// A sine that can be played end-to-end forever without a click.
///
/// Two things make the seam inaudible, and both are the opposite of what
/// [`sine`] does:
///
/// * **A whole number of cycles**, so the waveform arrives back at phase zero
///   exactly as the buffer runs out. The phase is stepped as
///   `2π · cycles · i / frames` rather than as `2π · f · t`, which makes that
///   exact by construction however `frames` rounds — the effective frequency
///   moves by a fraction of a hertz instead of the phase jumping.
/// * **No fade.** A fade to zero at each end is what stops a *one-shot*
///   clicking; on a loop it is a hole punched in the tone once per repeat.
#[must_use]
pub fn looped_sine(freq_hz: f32, cycles: u32, sample_rate: u32) -> Vec<AudioSample> {
    let frames = ((cycles as f32 * sample_rate as f32) / freq_hz).round() as usize;
    let mut out = Vec::with_capacity(frames * CHANNELS);
    for i in 0..frames {
        let phase = 2.0 * std::f32::consts::PI * cycles as f32 * (i as f32 / frames as f32);
        let value = TONE_AMPLITUDE * phase.sin();
        out.push(value);
        out.push(value);
    }
    out
}

/// A burst of low-passed noise that decays, as interleaved stereo.
///
/// The cue a sine cannot stand in for: a tone reads as a beep, and a beep reads
/// as scoring rather than as destruction.
///
/// `decay` is in nepers per second — `e^-9t` is down to a twentieth of its peak
/// by a fifth of a second — so a kill wants a fast one and a death wants a slow
/// one.
///
/// **Deterministic**, from `seed` through [`crcbl_core::rand`]'s splitmix64
/// mix, so the sound a build ships is the sound every build ships and a golden
/// buffer is possible. A different `seed` is a different-sounding burst, not a
/// wrong one.
#[must_use]
pub fn noise_burst(seconds: f32, decay: f32, seed: u64, sample_rate: u32) -> Vec<AudioSample> {
    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * CHANNELS);

    let mut low = 0.0f32;
    for i in 0..frames {
        // The engine's hash, walked as a sequence: stepping splitmix64's state
        // by its gamma is the same thing as hashing successive indices, which
        // `crcbl_core::rand`'s `stepping_the_state_is_hashing_the_index` pins.
        // The top 24 bits are the ones it mixes best, and 24 is exactly an
        // `f32`'s mantissa, so every value here is representable rather than
        // rounded.
        let z = crcbl_core::rand::hash_u64(seed, i as u64 + 1);
        let white = (z >> 40) as f32 / 8_388_608.0 - 1.0;

        low += NOISE_LOWPASS_ALPHA * (white - low);
        let t = i as f32 / sample_rate as f32;
        let value = NOISE_AMPLITUDE * low * (-decay * t).exp() * fade_gain(i, frames);
        out.push(value);
        out.push(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Interleaving is what the mixer's playhead assumes, and getting it wrong
    /// produces a sound rather than a failure — which is why it is asserted
    /// here for every generator rather than left to a listener.
    #[test]
    fn every_generator_produces_interleaved_stereo_pairs() {
        for (name, data) in [
            ("sine", sine(440.0, 0.05, RATE)),
            ("looped_sine", looped_sine(440.0, 4, RATE)),
            ("noise_burst", noise_burst(0.05, 9.0, 1, RATE)),
        ] {
            assert_eq!(data.len() % CHANNELS, 0, "{name} is not whole frames");
            assert!(!data.is_empty(), "{name} produced nothing");
            for frame in data.chunks_exact(CHANNELS) {
                assert_eq!(frame[0], frame[1], "{name} differs between the ears");
            }
            assert!(
                data.iter().any(|s| s.abs() > 1e-3),
                "{name} is silent, so nothing above proves anything"
            );
        }
    }

    #[test]
    fn a_one_shot_is_the_length_it_asked_for() {
        let frames = 100;
        let data = sine(440.0, frames as f32 / RATE as f32, RATE);
        assert_eq!(data.len(), frames * CHANNELS);
    }

    /// The fade is the whole reason a one-shot does not click, so both ends are
    /// checked against silence and the middle against full scale.
    ///
    /// The **last** frame is not exactly zero and should not be asserted to be:
    /// [`fade_gain`] is `from_end / fade`, which is `1 / FADE_FRAMES` on the
    /// final frame and reaches zero one frame past the end. That is the right
    /// shape — the envelope is a ramp over the samples that exist — and it caps
    /// the last frame at one fade step of full scale, which is what actually
    /// has to hold for the join to silence to be inaudible.
    #[test]
    fn a_one_shot_starts_at_silence_ends_within_a_fade_step_and_is_loud_between() {
        let data = sine(440.0, 0.2, RATE);
        let frames = data.len() / CHANNELS;
        assert_eq!(data[0], 0.0, "the first frame is not silent");

        let step = TONE_AMPLITUDE / FADE_FRAMES as f32;
        let last = data[data.len() - CHANNELS].abs();
        assert!(
            last <= step,
            "the last frame is {last}, past the one-step bound of {step}"
        );

        let mid = data[(frames / 2) * CHANNELS..]
            .chunks_exact(CHANNELS)
            .take(RATE as usize / 100)
            .map(|f| f[0].abs())
            .fold(0.0f32, f32::max);
        assert!(
            mid > TONE_AMPLITUDE * 0.9,
            "the middle of the sound is quiet ({mid}), so the fade never lets go"
        );
    }

    /// The property that makes a loop a loop: the sample after the last is the
    /// first, so playing the buffer twice has no discontinuity at the join.
    ///
    /// Asserted as a *step* between the join's two sides rather than as a value
    /// at either, because it is the jump that is audible.
    #[test]
    fn a_looped_tone_joins_to_itself_without_a_step() {
        let data = looped_sine(440.0, 10, RATE);
        let last = data[data.len() - CHANNELS];
        let first = data[0];
        // One sample of a 440 Hz tone at 48 kHz moves by at most
        // `A · 2π · f / rate`, which is the step any two adjacent samples show.
        let per_sample = TONE_AMPLITUDE * 2.0 * std::f32::consts::PI * 440.0 / RATE as f32;
        assert!(
            (first - last).abs() <= per_sample * 1.5,
            "the join steps by {} against a per-sample bound of {per_sample}",
            (first - last).abs()
        );
    }

    /// A loop must **not** be faded, which is the trap: `sine`'s envelope makes
    /// a perfectly good one-shot and a hole in a loop.
    #[test]
    fn a_looped_tone_is_not_faded() {
        let data = looped_sine(440.0, 10, RATE);
        let head = data[..FADE_FRAMES * CHANNELS]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            head > TONE_AMPLITUDE * 0.9,
            "the first {FADE_FRAMES} frames peak at {head}, which is a fade"
        );
    }

    /// Same seed, same bytes — the claim that makes a golden buffer possible.
    /// A different seed has to differ, or `seed` is not wired to anything.
    #[test]
    fn a_burst_is_reproducible_from_its_seed_and_varies_with_it() {
        let a = noise_burst(0.05, 9.0, 0x4173_7465_726F_6964, RATE);
        let b = noise_burst(0.05, 9.0, 0x4173_7465_726F_6964, RATE);
        assert_eq!(a, b, "the same seed produced two different sounds");

        let c = noise_burst(0.05, 9.0, 0x484F_5244_4553_4545, RATE);
        assert_ne!(a, c, "the seed reaches nothing");
    }

    /// The decay is what makes it a burst rather than a wash, and it is an
    /// argument rather than a constant so that a kill and a death can differ.
    #[test]
    fn a_faster_decay_is_quieter_later() {
        let peak_after = |decay: f32| {
            let data = noise_burst(0.4, decay, 1, RATE);
            let frames = data.len() / CHANNELS;
            data[(frames * 3 / 4) * CHANNELS..]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        let fast = peak_after(20.0);
        let slow = peak_after(2.0);
        assert!(
            fast < slow,
            "a decay of 20 left {fast} where a decay of 2 left {slow}"
        );
    }

    /// The fade holds for a sound far shorter than two fades, which is where
    /// the arithmetic used to underflow: `total - frame` on the last frame of a
    /// sound shorter than `FADE_FRAMES`.
    #[test]
    fn the_fade_holds_for_a_sound_shorter_than_itself() {
        for total in 1..=FADE_FRAMES * 2 {
            for frame in 0..total {
                let gain = fade_gain(frame, total);
                assert!(
                    (0.0..=1.0).contains(&gain),
                    "gain {gain} at {frame}/{total} is outside [0, 1]"
                );
            }
        }
    }
}
