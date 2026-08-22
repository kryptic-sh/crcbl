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

/// The longest a generated sound may be: a hostile `seconds`/`cycles` must not
/// overflow the frame count (f32 arithmetic saturates to `usize::MAX`, and
/// `frames * CHANNELS` then wraps or aborts). A minute at the internal rate is
/// far beyond anything a cue needs.
const MAX_FRAMES: usize = 48_000 * 60;

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
    // f64 so the product cannot overflow f32; floored to whole frames; capped
    // so a hostile `seconds` cannot overflow `frames * CHANNELS`.
    let frames = ((sample_rate as f64 * f64::from(seconds)).floor() as usize).min(MAX_FRAMES);
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
    // A zero or non-finite frequency is a degenerate request (divide by zero
    // below); an empty buffer is what the sound bank already refuses.
    if !freq_hz.is_finite() || freq_hz <= 0.0 {
        return Vec::new();
    }
    // f64 so the product cannot overflow f32; rounded to the nearest whole
    // frame; capped so a hostile `cycles` cannot overflow `frames * CHANNELS`.
    let frames = ((cycles as f64 * f64::from(sample_rate) / f64::from(freq_hz)).round() as usize)
        .min(MAX_FRAMES);
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
/// **Deterministic from `seed`** through [`crcbl_core::rand`]'s splitmix64 mix:
/// the same seed gives the same burst, and a different one is a
/// different-sounding burst rather than a wrong one.
///
/// **Deterministic is not bit-identical across platforms**, and this used to
/// claim it was. The noise source is integer and `white` divides by a power of
/// two, so that half is exact everywhere — but `(-decay * t).exp()` is libm's,
/// and glibc, Apple's and the MSVC runtime disagree in the last bit, which the
/// filter's recursion then carries forward. A byte-exact golden is therefore
/// not possible; a tolerance one is, and
/// `a_burst_is_the_waveform_that_shipped` is it.
#[must_use]
pub fn noise_burst(seconds: f32, decay: f32, seed: u64, sample_rate: u32) -> Vec<AudioSample> {
    // f64 so the product cannot overflow f32; floored to whole frames; capped
    // so a hostile `seconds` cannot overflow `frames * CHANNELS`.
    let frames = ((sample_rate as f64 * f64::from(seconds)).floor() as usize).min(MAX_FRAMES);
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

    /// Asteroids' explosion, the longest burst any sample plays and the one
    /// `tests/burst-reference.wav` holds. Named here rather than at each use
    /// because the reference and the test that reads it have to ask the
    /// generator for the same sound, and a golden compared against a different
    /// request fails for a reason that is not a regression.
    const BURST_SEED: u64 = 0x4173_7465_726F_6964;
    const BURST_SECONDS: f32 = 0.32;
    const BURST_DECAY: f32 = 9.0;
    /// `BURST_SECONDS * RATE` lands a frame short of 15_360 because `0.32` is
    /// not exact in `f32` and the product is floored.
    const BURST_FRAMES: usize = 15_359;

    /// The committed waveform: one channel of the burst above, as an
    /// IEEE-float WAV this crate's own encoder wrote and its own decoder reads.
    ///
    /// A WAV rather than an array of literals so that it can be *played*. The
    /// generator's output had never been listened to by anyone; a reference a
    /// human can open in an editor and hear is the only kind that catches the
    /// class of change every numeric assertion here passes through.
    ///
    /// Rewritten by `the_burst_reference_is_rewritten_from_the_generator`,
    /// below, which names the command.
    const BURST_REFERENCE: &[u8] = include_bytes!("../tests/burst-reference.wav");

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

    /// The largest `|x[n+1] - 2cos(w)x[n] + x[n-1]|` a correct generator leaves
    /// behind, over the buffers the tests below build.
    ///
    /// Two constants because the two generators step phase differently and the
    /// error follows: [`looped_sine`] steps `2pi*cycles*i/frames`, which stays
    /// small, while [`sine`] evaluates `2pi*f*t` outright and reaches hundreds
    /// of radians by the end of a fifth of a second, where an `f32` sine has
    /// fewer bits left. Measured at 2.3e-6 and 3.6e-5; these leave headroom and
    /// are still two orders below the ~1e-3 a change of waveform produces.
    const LOOP_RESIDUAL: f32 = 1e-5;
    const SINE_RESIDUAL: f32 = 2e-4;

    /// Every sinusoid, and nothing else, satisfies
    /// `x[n+1] = 2cos(w)x[n] - x[n-1]`.
    ///
    /// Returns the largest violation over `range`, reading the left channel.
    /// The caller supplies `w` because the two generators reach it differently.
    fn worst_recurrence(data: &[AudioSample], w: f32, range: std::ops::Range<usize>) -> f32 {
        let k = 2.0 * w.cos();
        range.fold(0.0f32, |worst, i| {
            let residual = (data[(i + 1) * CHANNELS] - k * data[i * CHANNELS]
                + data[(i - 1) * CHANNELS])
                .abs();
            worst.max(residual)
        })
    }

    /// **The one-shot is a sine, not merely a thing of the right size.**
    ///
    /// Every other assertion about [`sine`] here is a property a triangle wave
    /// at the same amplitude and fade satisfies too: it starts at silence, it
    /// ends inside a fade step, it is loud in between, and it is the length it
    /// asked for. The recurrence is what separates a sinusoid from every other
    /// shape, and it is checked rather than the closed form recomputed, so this
    /// is a statement about the waveform and not a restatement of the code that
    /// produced it.
    ///
    /// Read over the held region only. Inside a fade the samples are a sinusoid
    /// times a ramp, which is a different signal and does not satisfy this.
    #[test]
    fn a_one_shot_is_a_sinusoid_and_not_just_the_right_shape() {
        let data = sine(440.0, 0.2, RATE);
        let frames = data.len() / CHANNELS;
        let held = FADE_FRAMES + 1..frames - FADE_FRAMES - 1;
        assert!(
            held.end > held.start + RATE as usize / 100,
            "the held region is {held:?}, too short for this to be saying anything"
        );

        let w = 2.0 * std::f32::consts::PI * 440.0 / RATE as f32;
        let worst = worst_recurrence(&data, w, held);
        assert!(
            worst <= SINE_RESIDUAL,
            "the held region violates the sinusoid recurrence by {worst:e}, past \
             {SINE_RESIDUAL:e} - this is some other waveform"
        );
    }

    /// **The loop is a sinusoid, at the frequency its own phase step implies.**
    ///
    /// [`looped_sine`] does not step `2pi*f*t`; it steps `2pi*cycles*i/frames`
    /// so the buffer lands back on phase zero however `frames` rounded, which
    /// moves the effective frequency by a fraction of a hertz. That is the
    /// frequency asserted here, so this fails both if the shape stops being a
    /// sinusoid and if the phase step stops being the one the doc claims.
    ///
    /// No fade to skip: a loop must not have one, which
    /// `a_looped_tone_is_not_faded` covers, so the whole buffer is fair game.
    #[test]
    fn a_looped_tone_is_a_sinusoid_at_the_frequency_it_steps() {
        let cycles = 10;
        let data = looped_sine(440.0, cycles, RATE);
        let frames = data.len() / CHANNELS;

        let w = 2.0 * std::f32::consts::PI * cycles as f32 / frames as f32;
        let worst = worst_recurrence(&data, w, 1..frames - 1);
        assert!(
            worst <= LOOP_RESIDUAL,
            "the loop violates the sinusoid recurrence by {worst:e}, past {LOOP_RESIDUAL:e}"
        );
    }

    /// **The burst is the sound that shipped, to the precision a float buffer
    /// can promise across platforms.**
    ///
    /// This started life pinning a digest of every sample's `f32::to_bits`, and
    /// CI failed it on macOS and on Windows the first time it ran. That was the
    /// test being wrong, not the generator: the splitmix64 source is integer
    /// and identical everywhere, and `white` is exact because it divides by a
    /// power of two — but the samples then pass through a one-pole filter and
    /// `(-decay * t).exp()`, and `exp` is libm's. glibc, Apple's and the MSVC
    /// runtime do not agree to the last bit, so the bytes differ by around an
    /// ULP and the recursion carries that forward. **It failed on Windows as
    /// well as macOS, which is what rules out an architecture cause**: Windows
    /// is `x86_64` like the Linux runner, so the difference is the maths
    /// library and not the instruction set.
    ///
    /// So the bytes are not pinnable, and this checks the waveform instead —
    /// every frame of it, against `BURST_REFERENCE`, at `PROBE_TOLERANCE`. That
    /// tolerance is roughly fifty times the drift an `exp` an ULP out can
    /// produce through a filter whose pole *contracts* error rather than
    /// growing it. Contracting is why the bound does not have to widen as the
    /// comparison goes from a handful of samples to all of them: the error a
    /// frame inherits from the frames before it decays instead of accumulating,
    /// so the worst frame of the whole burst is bounded by the same figure as
    /// the worst of eight probes was. And it still sits far below any change
    /// worth catching — nudging [`NOISE_LOWPASS_ALPHA`] by a tenth of a
    /// percent, which is nothing a listener could hear, already moves the worst
    /// frame by 1.45e-4, and a different seed, decay or amplitude moves these
    /// samples by their own magnitude rather than by their last digit.
    ///
    /// **The total energy stays, alongside the per-sample comparison**, because
    /// it is not the same check made twice. A per-sample bound is blind to a
    /// small *coherent* drift spread over the whole buffer: a change that moves
    /// every sample by less than `PROBE_TOLERANCE` passes it however many
    /// samples it moves, and the energy moves by the sum. The band where that
    /// matters is real, and was measured rather than argued: a **thousandth**
    /// of a percent on [`NOISE_AMPLITUDE`] leaves the worst frame at 2.31e-6,
    /// four times inside the tolerance, and moves the energy by two parts in a
    /// hundred thousand — twenty times outside `ENERGY_TOLERANCE`. Only the
    /// second assertion sees it.
    ///
    /// The argument against keeping it is that a differing libm produces that
    /// same coherent shape, so it is the assertion most exposed to a platform
    /// we cannot run here. That is measured rather than guessed: this figure at
    /// `ENERGY_TOLERANCE` has been green on the macOS and Windows runners since
    /// it landed, which is the evidence the byte digest never had.
    ///
    /// A failure names the worst frame and by how much it missed, because the
    /// platforms that can fail this are ones we cannot reproduce locally: that
    /// message is the whole diagnosis, and "a sample differed" spends a CI
    /// round trip to learn nothing.
    #[test]
    fn a_burst_is_the_waveform_that_shipped() {
        /// Absolute, on a burst that peaks at about 0.23.
        const PROBE_TOLERANCE: f32 = 1e-5;
        /// Sum of squares over both channels, in `f64`. Read back out of the
        /// committed reference rather than typed from the old test.
        const ENERGY: f64 = 32.695_741_201;
        /// Relative, on a sum of two squares per frame.
        const ENERGY_TOLERANCE: f64 = 1e-6;

        let reference = crate::wav::decode(BURST_REFERENCE)
            .expect("the committed reference is not a WAV this crate can read");
        assert_eq!(
            reference.channels, 1,
            "the reference is not mono, so its samples are not frames"
        );
        assert_eq!(
            reference.sample_rate, RATE,
            "the reference was written at another rate, so it is another sound"
        );
        assert_eq!(
            reference.samples.len(),
            BURST_FRAMES,
            "the reference is a different length from the burst it pins"
        );
        // A reference of silence would agree with a generator that had stopped
        // producing anything, and the comparison below would pass.
        let peak = reference.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 1e-3,
            "the reference peaks at {peak}, which is silence — nothing below is saying anything"
        );

        let data = noise_burst(BURST_SECONDS, BURST_DECAY, BURST_SEED, RATE);
        assert_eq!(
            data.len(),
            BURST_FRAMES * CHANNELS,
            "the burst changed length, so it is a different sound from the reference"
        );

        let mut worst_frame = 0;
        let mut worst = 0.0f32;
        for (i, &want) in reference.samples.iter().enumerate() {
            let got = data[i * CHANNELS];
            assert_eq!(
                got,
                data[i * CHANNELS + 1],
                "frame {i}'s two channels differ, and this generator is mono in stereo"
            );
            let deviation = (got - want).abs();
            // The `is_nan` arm is load-bearing: every comparison against a NaN
            // is false, so `>` alone would walk past one and report the buffer
            // as its best frame.
            if deviation.is_nan() || deviation > worst {
                worst_frame = i;
                worst = deviation;
            }
        }
        // Captured unless the run asks for `--nocapture`, and shown by the
        // harness when the assertion below fails. On the machine that wrote the
        // reference this is exactly zero, so the number only says anything on a
        // platform whose libm differs from that machine's.
        eprintln!("worst frame deviates by {worst:e}, against {PROBE_TOLERANCE:e}");
        assert!(
            worst <= PROBE_TOLERANCE,
            "frame {worst_frame} is {got}, not {want} — the worst of {BURST_FRAMES} frames, \
             off by {worst:e} against a tolerance of {PROBE_TOLERANCE:e}, which is far more \
             than a differing libm can explain",
            got = data[worst_frame * CHANNELS],
            want = reference.samples[worst_frame],
        );

        let energy: f64 = data.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        assert!(
            (energy - ENERGY).abs() <= ENERGY * ENERGY_TOLERANCE,
            "the burst carries {energy} of energy, not {ENERGY} — every frame is inside \
             {PROBE_TOLERANCE:e} of the reference and the total still moved, which is the \
             coherent drift the per-frame bound cannot see"
        );
    }

    /// Rewrites `tests/burst-reference.wav` from the generator as it stands:
    ///
    /// ```text
    /// cargo test -p crcbl-audio --lib -- --ignored --exact \
    ///     synth::tests::the_burst_reference_is_rewritten_from_the_generator
    /// ```
    ///
    /// `#[ignore]` because it is not a check. It makes
    /// `a_burst_is_the_waveform_that_shipped` pass by construction, so running
    /// it is a decision that the burst *should* now sound different — never a
    /// way to clear a failure. **Play the file it writes before committing
    /// it**; that a human can hear the reference is half of why it is a WAV.
    ///
    /// Mono, because `noise_burst` puts one signal in both ears and the test
    /// above asserts that frame by frame. The second copy would double the blob
    /// and pin nothing the first does not.
    #[test]
    #[ignore = "rewrites the committed reference rather than checking it"]
    fn the_burst_reference_is_rewritten_from_the_generator() {
        let data = noise_burst(BURST_SECONDS, BURST_DECAY, BURST_SEED, RATE);
        let left: Vec<AudioSample> = data.chunks_exact(CHANNELS).map(|frame| frame[0]).collect();
        let bytes = crate::wav::encode(&crate::wav::WavFile {
            samples: left,
            sample_rate: RATE,
            channels: 1,
        })
        .expect("the burst does not fit a WAV, which is a change to the generator");
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/burst-reference.wav");
        std::fs::write(path, bytes).expect("could not write the reference");
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

    /// A hostile `seconds` must not overflow the frame count and abort the
    /// process; the generators cap at [`MAX_FRAMES`].
    #[test]
    fn hostile_parameters_are_capped_not_overflowed() {
        let sine_buf = sine(f32::MAX, 1e30, RATE);
        assert!(sine_buf.len() / CHANNELS <= MAX_FRAMES);

        let noise = noise_burst(1e30, 1.0, 0, RATE);
        assert!(noise.len() / CHANNELS <= MAX_FRAMES);
    }

    /// A zero frequency would divide by zero; the degenerate request is an
    /// empty buffer rather than a panic.
    #[test]
    fn looped_sine_with_zero_frequency_is_empty() {
        assert!(looped_sine(0.0, 10, RATE).is_empty());
    }

    /// The legitimate path is unchanged: a whole-second-asking request still
    /// gets exactly the frames it asked for.
    #[test]
    fn sine_still_produces_exactly_the_frames_asked() {
        assert_eq!(sine(440.0, 0.1, RATE).len(), 4800 * CHANNELS);
    }
}
