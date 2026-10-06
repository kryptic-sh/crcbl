use super::*;
use crate::mixer::{Mixer, Voice, VoiceMix};
use crate::{AudioSource, INTERNAL_SAMPLE_RATE};

/// How far the analytic response at the cutoff may sit from `1/√2`: the
/// coefficient is rounded to `f32` once, which moves the response by parts in
/// ten million, and this leaves room above that and none for a different
/// design (the naive `1 − e^−ω` misses by percent at these cutoffs).
const ANALYTIC_TOLERANCE: f64 = 1e-5;

/// How far a measured steady-state gain may sit from the formula: the
/// measurement reads a peak off sampled `f32` output, so it is good to the
/// sampling of the peak and the float recursion, not to the formula's own
/// precision.
const MEASURED_TOLERANCE: f64 = 2e-3;

/// How close a lowpass fed a constant must settle to it: `f32` rounding in the
/// recursion, after it has run many time constants.
const SETTLED_TOLERANCE: f32 = 1e-6;

/// The magnitude response of the one-pole with coefficient `a` at `omega`,
/// from the closed form in the module docs.
fn magnitude(a: f32, omega: f64) -> f64 {
    let a = f64::from(a);
    let b = 1.0 - a;
    a / (1.0 - 2.0 * b * omega.cos() + b * b).sqrt()
}

/// `voice` mixed alone for `frames` frames, cut into blocks of `block`.
fn render(mixer: &Mixer, frames: usize, block: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * CHANNELS);
    let mut remaining = frames;
    while remaining > 0 {
        let this = block.min(remaining);
        let mut buf = vec![0.0f32; this * CHANNELS];
        mixer.fill(&mut buf, INTERNAL_SAMPLE_RATE);
        out.extend_from_slice(&buf);
        remaining -= this;
    }
    out
}

/// A looping sine at `hz`, interleaved stereo, at the internal rate.
fn tone(hz: f32) -> Vec<f32> {
    crate::synth::looped_sine(hz, 64, INTERNAL_SAMPLE_RATE)
}

/// A loop alternating between a loud sample and one far smaller than it,
/// interleaved stereo: the input a rounded `y + 1·(x − y)` loses, since the
/// difference rounds to the loud sample's negation and the sum to zero.
fn edges() -> Vec<f32> {
    (0..4_096)
        .flat_map(|frame| {
            let value = if frame % 2 == 0 { 0.75 } else { 1e-9 };
            [value; CHANNELS]
        })
        .collect()
}

/// The left channel's root mean square over `samples`.
fn rms_left(samples: &[f32]) -> f32 {
    let left: Vec<f32> = samples.iter().step_by(CHANNELS).copied().collect();
    (left.iter().map(|s| s * s).sum::<f32>() / left.len() as f32).sqrt()
}

/// **The cutoff is exactly 3 dB down**, from the closed form, at cutoffs across
/// the presets' range and at both common device rates.
#[test]
fn the_coefficient_puts_the_cutoff_exactly_three_db_down() {
    for rate in [INTERNAL_SAMPLE_RATE, 44_100] {
        for cutoff in [MIN_CUTOFF_HZ, 400.0, 1_000.0, 3_500.0, 8_000.0, 16_000.0] {
            let a = one_pole_coefficient(cutoff, rate);
            let omega = core::f64::consts::TAU * f64::from(cutoff) / f64::from(rate);
            let at_cutoff = magnitude(a, omega);
            assert!(
                (at_cutoff - core::f64::consts::FRAC_1_SQRT_2).abs() < ANALYTIC_TOLERANCE,
                "{cutoff} Hz at {rate} Hz: |H| = {at_cutoff}, not 1/sqrt(2)",
            );
        }
    }
}

/// **A sine at the cutoff comes out at `1/√2` of its level**, measured by
/// running it through the filter, so the recursion is the one the formula
/// describes and not merely a coefficient that would be right in it.
#[test]
fn a_sine_at_the_cutoff_comes_out_three_db_down() {
    let rate = INTERNAL_SAMPLE_RATE;
    let cutoff = 1_000.0f32;
    let mut filter = VoiceOcclusion::new();
    filter.start_at(Occlusion {
        cutoff_hz: cutoff,
        gain: 1.0,
    });
    filter.begin_block(rate);
    let omega = core::f64::consts::TAU * f64::from(cutoff) / f64::from(rate);
    // Long enough for the start-up transient to decay to nothing, then a whole
    // number of periods (48 samples each at this rate), over which a sampled
    // sine's mean square is exactly half its amplitude squared.
    let settle = 4_800;
    let measure = 4_800;
    let mut square_sum = 0.0f64;
    for n in 0..settle + measure {
        let x = (omega * n as f64).sin() as f32;
        let coefficients = filter.step();
        let y = filter.process(0, x, coefficients);
        if n >= settle {
            square_sum += f64::from(y) * f64::from(y);
        }
    }
    let amplitude = (2.0 * square_sum / f64::from(measure)).sqrt();
    assert!(
        (amplitude - core::f64::consts::FRAC_1_SQRT_2).abs() < MEASURED_TOLERANCE,
        "a {cutoff} Hz sine came out at {amplitude}",
    );
}

/// **DC passes at unity**: the design's `a / (1 − b)` is one, and a constant
/// run through a heavily muffled filter settles on itself.
#[test]
fn dc_passes_at_unity_gain() {
    let a = one_pole_coefficient(
        AcousticMaterial::CONCRETE.muffle_cutoff_hz,
        INTERNAL_SAMPLE_RATE,
    );
    let b = 1.0 - f64::from(a);
    assert!((f64::from(a) / (1.0 - b) - 1.0).abs() < ANALYTIC_TOLERANCE);

    let mut filter = VoiceOcclusion::new();
    filter.start_at(Occlusion {
        cutoff_hz: AcousticMaterial::CONCRETE.muffle_cutoff_hz,
        gain: 1.0,
    });
    filter.begin_block(INTERNAL_SAMPLE_RATE);
    let mut y = 0.0;
    for _ in 0..INTERNAL_SAMPLE_RATE {
        let coefficients = filter.step();
        y = filter.process(0, 0.5, coefficients);
    }
    assert!((y - 0.5).abs() < SETTLED_TOLERANCE, "DC settled at {y}");
}

/// **Clear is no filter**: an infinite cutoff, one at or past Nyquist, and a
/// zero rate all give a coefficient of exactly one.
#[test]
fn clear_and_past_nyquist_are_no_filter() {
    assert_eq!(
        one_pole_coefficient(f32::INFINITY, INTERNAL_SAMPLE_RATE),
        1.0
    );
    assert_eq!(one_pole_coefficient(24_000.0, INTERNAL_SAMPLE_RATE), 1.0);
    assert_eq!(one_pole_coefficient(400.0, 0), 1.0);
    assert!(one_pole_coefficient(23_999.0, INTERNAL_SAMPLE_RATE) < 1.0);
}

/// **A new target ramps, it never steps.** Every frame's coefficient and gain
/// move by at most one ramp step, the first frame is not yet the target, and
/// the last lands on it exactly.
#[test]
fn a_new_target_ramps_with_no_step() {
    let mut filter = VoiceOcclusion::new();
    filter.begin_block(INTERNAL_SAMPLE_RATE);
    assert_eq!(filter.step(), [1.0, 1.0], "a new voice starts clear");

    let target = Occlusion {
        cutoff_hz: AcousticMaterial::CONCRETE.muffle_cutoff_hz,
        gain: 0.25,
    };
    filter.set(target);
    filter.begin_block(INTERNAL_SAMPLE_RATE);
    let to = [
        one_pole_coefficient(target.cutoff_hz, INTERNAL_SAMPLE_RATE),
        target.gain,
    ];
    // One step of each, with room for the rounding of `from + (to − from)·t`.
    let bound = to.map(|end| (1.0 - end) / OCCLUSION_RAMP_FRAMES as f32 * (1.0 + 1e-3));
    let mut previous = [1.0f32, 1.0];
    for frame in 1..=OCCLUSION_RAMP_FRAMES {
        let now = filter.step();
        for k in 0..2 {
            assert!(
                (now[k] - previous[k]).abs() <= bound[k],
                "frame {frame}: {} jumped from {} to {}",
                ["coefficient", "gain"][k],
                previous[k],
                now[k],
            );
        }
        if frame == 1 {
            assert_ne!(now, to, "the first frame already sat on the target");
        }
        previous = now;
    }
    assert_eq!(previous, to, "the ramp did not land on its target");
    assert_eq!(filter.step(), to, "a settled ramp moved");
}

/// **Muffling a playing voice has no click in the mix.** A constant voice whose
/// gain is pulled down mid-play moves by at most one ramp step per frame.
#[test]
fn set_occlusion_fades_a_playing_voice_without_a_step() {
    let mixer = Mixer::new();
    let id = mixer.play(Voice::new(vec![
        0.5;
        2 * OCCLUSION_RAMP_FRAMES as usize
            * CHANNELS
    ]));
    let before = render(&mixer, 64, 64);
    assert!(before.iter().all(|&s| s == 0.5));

    let gain = 0.25;
    assert!(mixer.set_occlusion(
        id,
        Occlusion {
            cutoff_hz: f32::INFINITY,
            gain,
        },
    ));
    let after = render(&mixer, OCCLUSION_RAMP_FRAMES as usize + 64, 256);
    let step = 0.5 * (1.0 - gain) / OCCLUSION_RAMP_FRAMES as f32 * (1.0 + 1e-3);
    let mut previous = 0.5f32;
    for (frame, sample) in after.iter().step_by(CHANNELS).enumerate() {
        assert!(
            (sample - previous).abs() <= step,
            "frame {frame} stepped from {previous} to {sample}",
        );
        previous = *sample;
    }
    assert_eq!(previous, 0.5 * gain, "the gain never arrived");
}

/// **Where a ramp is does not depend on how the device cuts its blocks**: the
/// same voice, muffled the same way, mixed in blocks of 64 and of 1000 frames,
/// is the same bits.
#[test]
fn a_ramp_is_the_same_however_the_blocks_are_cut() {
    let run = |block: usize| {
        let mixer = Mixer::new();
        let id = mixer.play(Voice::new(tone(1_500.0)).with_looping());
        let _ = render(&mixer, 100, 100);
        mixer.set_occlusion(id, Occlusion::through([AcousticMaterial::THIN_WOOD]));
        render(&mixer, 3 * OCCLUSION_RAMP_FRAMES as usize, block)
    };
    let small = run(64);
    let large = run(1_000);
    assert!(
        small
            .iter()
            .zip(&large)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "the block size moved the ramp",
    );
}

/// **A voice nobody occluded mixes the bits it did before**, and so does one
/// whose occlusion has ramped back to clear: once settled at clear, the filter
/// returns its input rather than a rounded recursion.
///
/// The reference is a voice that never touched occlusion, panned and delayed so
/// the delay line and the gains are in the comparison too. Its sound is
/// [`edges`], because a smooth tone survives the recursion at a coefficient of
/// one with its bits intact and could not tell the bypass from its absence.
#[test]
fn a_clear_voice_is_bit_identical_to_one_never_occluded() {
    let mix = VoiceMix {
        volume: 0.8,
        gains: (0.9, 0.4),
        pitch: 1.0,
        itd_samples: 7.25,
    };
    let reference = Mixer::new();
    reference.play(Voice::new(edges()).with_looping().with_mix(mix));

    let started_clear = Mixer::new();
    let clear_id = started_clear.play(
        Voice::new(edges())
            .with_looping()
            .with_mix(mix)
            .with_occlusion(Occlusion::CLEAR),
    );
    started_clear.set_occlusion(clear_id, Occlusion::CLEAR);

    let returned = Mixer::new();
    let returned_id = returned.play(
        Voice::new(edges())
            .with_looping()
            .with_mix(mix)
            .with_occlusion(Occlusion::through([AcousticMaterial::CONCRETE])),
    );

    let frames = 4 * OCCLUSION_RAMP_FRAMES as usize;
    let expected = render(&reference, frames, 512);
    // The reference runs through the same filter code, so it is pinned to the
    // gain chain as it stood before occlusion existed: the left channel has no
    // delay, and its sample is the source times the voice gains, unfaded, on
    // unity buses.
    let source = edges();
    for (frame, left) in expected.iter().step_by(CHANNELS).enumerate() {
        let x = source[(frame * CHANNELS) % source.len()];
        let before = x * mix.volume * mix.gains.0 * 1.0 * 1.0 * 1.0;
        assert_eq!(
            left.to_bits(),
            before.to_bits(),
            "frame {frame}: a never-occluded voice mixed {left}, not {before}",
        );
    }
    let first = render(&started_clear, frames, 512);
    assert!(
        expected
            .iter()
            .zip(&first)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "a voice set to clear mixed different bits from one never occluded",
    );

    let muffled = render(&returned, frames, 512);
    assert!(
        rms_left(&muffled) < 0.5 * rms_left(&expected),
        "the concrete voice was not muffled, so it cannot show a return to clear",
    );
    returned.set_occlusion(returned_id, Occlusion::CLEAR);
    // Through the ramp, then compare what follows against the reference at the
    // same playhead.
    let _ = render(&returned, OCCLUSION_RAMP_FRAMES as usize, 512);
    let _ = render(&reference, OCCLUSION_RAMP_FRAMES as usize, 512);
    let after = render(&returned, frames, 512);
    let reference_after = render(&reference, frames, 512);
    assert!(
        reference_after
            .iter()
            .zip(&after)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "a voice ramped back to clear did not return to the unoccluded bits",
    );
}

/// **An occluded voice is quieter and duller**: a high tone heard through
/// concrete loses more than the attenuation alone, because the lowpass takes
/// its share too, while a tone well under the cutoff loses about the
/// attenuation and no more.
#[test]
fn an_occluded_voice_is_quieter_and_duller() {
    let concrete = Occlusion::through([AcousticMaterial::CONCRETE]);
    let level = |hz: f32, occlusion: Occlusion| {
        let mixer = Mixer::new();
        mixer.play(
            Voice::new(tone(hz))
                .with_looping()
                .with_occlusion(occlusion),
        );
        let _ = render(&mixer, 4_800, 480);
        rms_left(&render(&mixer, 4_800, 480))
    };
    let high_ratio = level(4_000.0, concrete) / level(4_000.0, Occlusion::CLEAR);
    let low_ratio = level(50.0, concrete) / level(50.0, Occlusion::CLEAR);
    assert!(
        (low_ratio - concrete.gain).abs() < 0.05 * concrete.gain,
        "a 50 Hz tone through concrete kept {low_ratio}, not about the gain {}",
        concrete.gain,
    );
    assert!(
        high_ratio < 0.25 * low_ratio,
        "a 4 kHz tone kept {high_ratio} against the 50 Hz tone's {low_ratio}",
    );
}

/// **Nothing in the way is clear.**
#[test]
fn no_material_is_clear() {
    assert_eq!(Occlusion::through([]), Occlusion::CLEAR);
    assert!(Occlusion::CLEAR.is_clear());
    assert!(!Occlusion::through([AcousticMaterial::FOLIAGE]).is_clear());
}

/// **Every preset gives its own target**: no two share a cutoff or a gain, so
/// a player can tell any two apart by tone and by level.
#[test]
fn every_preset_gives_a_distinct_target() {
    let targets = AcousticMaterial::PRESETS.map(|material| Occlusion::through([material]));
    for (i, a) in targets.iter().enumerate() {
        for b in &targets[i + 1..] {
            assert_ne!(a.cutoff_hz, b.cutoff_hz, "{a:?} and {b:?} share a cutoff");
            assert_ne!(a.gain, b.gain, "{a:?} and {b:?} share a gain");
        }
    }
    let wood = Occlusion::through([AcousticMaterial::THIN_WOOD]);
    let concrete = Occlusion::through([AcousticMaterial::CONCRETE]);
    assert!(
        concrete.cutoff_hz < wood.cutoff_hz && concrete.gain < wood.gain,
        "concrete ({concrete:?}) is not heavier than thin wood ({wood:?})",
    );
}

/// **Several materials combine by the stated rule**: the attenuations add and
/// the lowest cutoff wins, whatever order they arrive in.
#[test]
fn several_materials_add_their_loss_and_keep_the_lowest_cutoff() {
    let wood = AcousticMaterial::THIN_WOOD;
    let glass = AcousticMaterial::GLASS;
    for pair in [[wood, glass], [glass, wood]] {
        let both = Occlusion::through(pair);
        assert_eq!(both.cutoff_hz, wood.muffle_cutoff_hz);
        let expected = crate::spatial::db_to_linear(-(wood.attenuation_db + glass.attenuation_db));
        assert_eq!(both.gain, expected);
        // Decibels add where linear gains multiply.
        let product = Occlusion::through([wood]).gain * Occlusion::through([glass]).gain;
        assert!(
            (both.gain - product).abs() < 1e-6,
            "{} against {product}",
            both.gain
        );
    }
    let twice = Occlusion::through([wood, wood]);
    assert!(
        twice.gain < Occlusion::through([wood]).gain,
        "a second wall cost nothing"
    );
}

/// **Caller input is clamped at the voice**: a NaN is clear, a cutoff below the
/// floor sits on it, and a gain past one is one.
#[test]
fn caller_input_is_clamped() {
    let nan = Occlusion {
        cutoff_hz: f32::NAN,
        gain: f32::NAN,
    };
    assert!(nan.is_clear());
    let voice = Voice::new(vec![0.0; CHANNELS]).with_occlusion(Occlusion {
        cutoff_hz: 1.0,
        gain: 4.0,
    });
    assert_eq!(
        voice.occlusion(),
        Occlusion {
            cutoff_hz: MIN_CUTOFF_HZ,
            gain: 1.0,
        },
    );
}

/// **A stale handle steers nothing**, as for every other per-voice call.
#[test]
fn set_occlusion_answers_false_for_a_voice_that_is_not_playing() {
    let mixer = Mixer::new();
    let id = mixer.play(Voice::new(vec![0.0; CHANNELS]));
    assert_eq!(mixer.occlusion(id), Some(Occlusion::CLEAR));
    let _ = render(&mixer, 4, 4);
    assert!(!mixer.set_occlusion(id, Occlusion::through([AcousticMaterial::GLASS])));
    assert_eq!(mixer.occlusion(id), None);
}
