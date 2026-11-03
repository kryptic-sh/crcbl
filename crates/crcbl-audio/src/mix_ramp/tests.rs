use crate::itd::ITD_RAMP_FRAMES;
use crate::itd::tests::{
    BLOCK, MEASURE_FROM, PERIOD, STEP_MARGIN, digest, largest_step, tone, tone_slope,
};
use crate::mixer::{Mixer, Voice, VoiceMix};
use crate::{AudioSource, CHANNELS, INTERNAL_SAMPLE_RATE};

/// The frame the mix changes at, a block boundary.
const CHANGE_FRAME: usize = 8 * BLOCK;

/// How many frames a re-aim test renders: the change, a second re-aim a block
/// later and its whole ramp, and a settled block after that.
const FRAMES: usize = CHANGE_FRAME + 2 * ITD_RAMP_FRAMES as usize;

/// How many frames the bit-identity test renders. Fixed, because its digests
/// were captured over exactly this many.
const STILL_FRAMES: usize = 16 * BLOCK;

/// How close the settled output must sit to the tone scaled by the new mix
/// written out: the gain chain's rounding.
const SETTLED_TOLERANCE: f32 = 1e-6;

/// The tone started a quarter period in, so it begins at its positive peak,
/// and so does every block boundary — [`PERIOD`] divides [`BLOCK`]. A gain that
/// steps there jumps by the whole step times the peak, the most it can.
fn peak_tone() -> Vec<f32> {
    let mut out = tone();
    out.rotate_left(PERIOD / 4 * CHANNELS);
    out
}

/// `source`, looped, started at `start` and re-aimed by `set_mix` at each
/// `(frame, mix)` of `changes` (each a block boundary), rendered for `frames`
/// frames.
fn render(
    source: Vec<f32>,
    start: VoiceMix,
    changes: &[(usize, VoiceMix)],
    frames: usize,
) -> Vec<f32> {
    let mixer = Mixer::new();
    let id = mixer.play(Voice::new(source).with_looping().with_mix(start));
    let mut out = Vec::with_capacity(frames * CHANNELS);
    for block_start in (0..frames).step_by(BLOCK) {
        for &(frame, mix) in changes {
            if frame == block_start {
                assert!(mixer.set_mix(id, mix), "the voice ended");
            }
        }
        let mut buf = vec![0.0f32; BLOCK * CHANNELS];
        mixer.fill(&mut buf, INTERNAL_SAMPLE_RATE);
        out.extend_from_slice(&buf);
    }
    out
}

fn mix(volume: f32, gains: (f32, f32), pitch: f32) -> VoiceMix {
    VoiceMix {
        volume,
        gains,
        pitch,
        itd_samples: 0.0,
    }
}

/// That `out` never steps further than the tone's own steepest step allows.
fn assert_no_step(out: &[f32], what: &str) {
    let bound = tone_slope() * STEP_MARGIN;
    let (step, frame, ch) = largest_step(out);
    assert!(
        step <= bound,
        "{what}: channel {ch} stepped {step} at frame {frame} (re-aimed at \
         {CHANGE_FRAME}); the tone's own steepest step is {}, so the bound is {bound}",
        tone_slope(),
    );
}

/// That the last [`BLOCK`] frames of `out` are [`peak_tone`] at `volume` and
/// `gains`, written out.
fn assert_settled_at(out: &[f32], volume: f32, gains: (f32, f32)) {
    let source = peak_tone();
    let frames = out.as_chunks::<CHANNELS>().0;
    for (n, frame) in frames.iter().enumerate().skip(frames.len() - BLOCK) {
        let x = source[(n % PERIOD) * CHANNELS];
        for (ch, (got, gain)) in frame.iter().zip([gains.0, gains.1]).enumerate() {
            let expected = x * volume * gain;
            assert!(
                (got - expected).abs() < SETTLED_TOLERANCE,
                "frame {n} channel {ch}: expected {expected} at volume {volume} and \
                 gain {gain}, got {got}",
            );
        }
    }
}

/// **A volume cut ramps instead of stepping.** Full level to a tenth at the
/// tone's peak; stepped in one frame, that drops the output by most of the
/// peak, eighteen of the tone's steepest steps.
#[test]
fn a_volume_cut_does_not_step() {
    let quiet = mix(0.1, (1.0, 1.0), 1.0);
    let out = render(
        peak_tone(),
        VoiceMix::UNITY,
        &[(CHANGE_FRAME, quiet)],
        FRAMES,
    );
    assert_no_step(&out, "volume 1 → 0.1");
    assert_settled_at(&out, 0.1, (1.0, 1.0));
}

/// **A pan swap ramps instead of stepping**, on each channel: the left falls
/// from full to silence while the right rises from silence to full.
#[test]
fn a_pan_swap_does_not_step() {
    let out = render(
        peak_tone(),
        mix(1.0, (1.0, 0.0), 1.0),
        &[(CHANGE_FRAME, mix(1.0, (0.0, 1.0), 1.0))],
        FRAMES,
    );
    assert_no_step(&out, "gains (1, 0) → (0, 1)");
    assert_settled_at(&out, 1.0, (0.0, 1.0));
}

/// **A re-aim that arrives mid-ramp starts from where the ramp has got to.**
/// A cut, then back up a block later; a ramp that restarted from the first
/// target, or from the old one, would step at the second re-aim.
#[test]
fn a_re_aim_during_a_ramp_starts_from_where_it_got_to() {
    let out = render(
        peak_tone(),
        VoiceMix::UNITY,
        &[
            (CHANGE_FRAME, mix(0.1, (1.0, 0.25), 1.0)),
            (CHANGE_FRAME + BLOCK, mix(0.8, (0.5, 1.0), 1.0)),
        ],
        FRAMES,
    );
    assert_no_step(&out, "1 → 0.1 → 0.8, pan swung and back");
    assert_settled_at(&out, 0.8, (0.5, 1.0));
}

/// **`Voice::with_mix` starts at its mix**, with no ramp from unity: a voice
/// that starts quiet and to one side was never heard anywhere else.
#[test]
fn with_mix_starts_at_its_mix_without_a_ramp() {
    let (volume, gains) = (0.25, (0.5, 0.75));
    let out = render(peak_tone(), mix(volume, gains, 1.0), &[], BLOCK);
    let source = peak_tone();
    for (n, frame) in out.as_chunks::<CHANNELS>().0.iter().enumerate() {
        let x = source[(n % PERIOD) * CHANNELS];
        assert_eq!(
            *frame,
            [x * volume * gains.0, x * volume * gains.1],
            "frame {n}: not the tone at the voice's own mix",
        );
    }
}

/// The left channel's zero crossings from [`MEASURE_FROM`] on, each placed
/// between its two frames by linear interpolation.
fn zero_crossings(out: &[f32]) -> Vec<f64> {
    let left: Vec<f64> = out
        .iter()
        .step_by(CHANNELS)
        .map(|&s| f64::from(s))
        .collect();
    let mut crossings = Vec::new();
    for n in MEASURE_FROM..left.len() {
        let (before, now) = (left[n - 1], left[n]);
        if (before >= 0.0) != (now >= 0.0) {
            crossings.push((n - 1) as f64 + before / (before - now));
        }
    }
    crossings
}

/// The pitch the pitch test glides between.
const PITCH_FROM: f32 = 1.0;
const PITCH_TO: f32 = 2.0;

/// How far one half period may differ from the one before it, in frames.
///
/// A pitch that steps from [`PITCH_FROM`] to [`PITCH_TO`] at a crossing
/// halves the half period from one to the next, a jump of a quarter of
/// [`PERIOD`]. Ramped over [`ITD_RAMP_FRAMES`], consecutive half periods differ
/// by the ramp's slope across one of them (`PERIOD² / (4·N·p³)` at pitch `p`,
/// at most four frames here), plus the jitter of reading the nearest source
/// frame rather than interpolating, which moves each crossing by up to a frame.
const HALF_PERIOD_JUMP: f64 = 8.0;

/// How close the first and last half periods must sit to the tone's at the
/// old and new pitch: reading the nearest source frame moves a crossing by
/// less than one.
const HALF_PERIOD_TOLERANCE: f64 = 1.0;

/// **A pitch change glides instead of stepping.** The output's half periods,
/// measured between zero crossings, shorten gradually from the tone's at the
/// old pitch to its at the new one; a pitch that took effect in one frame
/// would halve them from one crossing to the next.
#[test]
fn a_pitch_change_glides() {
    let out = render(
        tone(),
        mix(1.0, (1.0, 1.0), PITCH_FROM),
        &[(CHANGE_FRAME, mix(1.0, (1.0, 1.0), PITCH_TO))],
        FRAMES,
    );
    let crossings = zero_crossings(&out);
    let halves: Vec<f64> = crossings.windows(2).map(|w| w[1] - w[0]).collect();
    let half_at = |pitch: f32| PERIOD as f64 / 2.0 / f64::from(pitch);
    let (first, last) = (halves[0], halves[halves.len() - 1]);
    assert!(
        (first - half_at(PITCH_FROM)).abs() < HALF_PERIOD_TOLERANCE
            && (last - half_at(PITCH_TO)).abs() < HALF_PERIOD_TOLERANCE,
        "half periods ran from {first} to {last}, not from {} to {}",
        half_at(PITCH_FROM),
        half_at(PITCH_TO),
    );
    for (k, pair) in halves.windows(2).enumerate() {
        let jump = (pair[1] - pair[0]).abs();
        assert!(
            jump <= HALF_PERIOD_JUMP,
            "half period {k} at frame {:.1} was {:.2} frames and the next {:.2}: a \
             jump of {jump:.2}, over the bound {HALF_PERIOD_JUMP}",
            crossings[k + 1],
            pair[0],
            pair[1],
        );
    }
}

/// What [`indices`] divides each index by: a power of two, so the division is
/// exact, and above every index it writes, so no sample reaches the mixer's
/// clip at one.
const INDEX_SCALE: f32 = 65_536.0;

/// A source whose every frame holds its own index over [`INDEX_SCALE`], both
/// channels: the output then reads out where the voice read.
fn indices(frames: usize) -> Vec<f32> {
    assert!((frames as f32) < INDEX_SCALE, "{frames} frames would clip");
    (0..frames)
        .flat_map(|n| [n as f32 / INDEX_SCALE; CHANNELS])
        .collect()
}

/// The pitch the read-position test moves to: one whose advance per block is
/// not a whole number of frames, which a playhead kept in whole frames loses
/// the fraction of at every block boundary.
const PITCH_UNEVEN: f32 = 1.37;

/// How many blocks the read-position test plays after its ramp has settled.
const SETTLED_BLOCKS: usize = 16;

/// How far the last frame read may sit from the read position worked out by
/// hand: the output reads the frame below the position, and the per-frame
/// rate is rounded to `f32` along the ramp.
const READ_TOLERANCE: f64 = 1.5;

/// **A pitch ramp moves the read position continuously, and keeps it.** The
/// voice plays a source of its own frame indices at normal speed, then ramps to
/// [`PITCH_UNEVEN`]. Wherever it ends up, it must have read what a rate rising
/// linearly over [`ITD_RAMP_FRAMES`] and then held integrates to — not jumped
/// to the new rate at once, and not dropped part of a frame at each block.
#[test]
fn a_pitch_ramp_reads_the_source_continuously() {
    let ramp = ITD_RAMP_FRAMES as usize;
    let frames = CHANGE_FRAME + ramp + SETTLED_BLOCKS * BLOCK;
    let out = render(
        indices(4 * frames),
        mix(1.0, (1.0, 1.0), 1.0),
        &[(CHANGE_FRAME, mix(1.0, (1.0, 1.0), PITCH_UNEVEN))],
        frames,
    );
    let read: Vec<f64> = out
        .iter()
        .step_by(CHANNELS)
        .map(|&s| f64::from(s * INDEX_SCALE))
        .collect();
    for (n, pair) in read.windows(2).enumerate() {
        assert!(
            pair[1] >= pair[0],
            "frame {}: the read position went back from {} to {}",
            n + 1,
            pair[0],
            pair[1],
        );
    }
    // The last frame read sits at the change, plus a ramp whose `k`-th frame
    // advances by `1 + (p − 1)·k/N`, plus the frames held at `p` after it.
    let (p, n) = (f64::from(PITCH_UNEVEN), ramp as f64);
    let held = (frames - 1 - CHANGE_FRAME - ramp) as f64;
    let expected = CHANGE_FRAME as f64 + n + (p - 1.0) * (n + 1.0) / 2.0 + p * held;
    let last = read[read.len() - 1];
    assert!(
        (last - expected).abs() < READ_TOLERANCE,
        "the last frame read was {last}; a continuous ramp to {p} reads about {expected}",
    );
}

/// **A mix that does not change renders the bits it rendered before smoothing
/// existed**, with `set_mix` re-sending the same mix every block as a game
/// re-aiming a still emitter does. The digests were captured from the mixer as
/// it stood before level, pan and pitch were ramped; each pitch advances a
/// whole number of frames per block, the only case that mixer kept exactly.
#[test]
fn an_unchanged_mix_renders_the_bits_it_did_before_smoothing() {
    let cases: [(VoiceMix, u64); 4] = [
        (VoiceMix::UNITY, 0x5ea2_887b_34eb_1925),
        (mix(0.7, (0.3, 0.9), 1.0), 0x812d_95c1_630f_d0a5),
        (
            VoiceMix {
                itd_samples: -4.0,
                ..mix(0.5, (1.0, 0.25), 1.5)
            },
            0xc3bc_afc6_401e_09e2,
        ),
        (
            VoiceMix {
                itd_samples: 2.5,
                ..mix(0.25, (0.6, 0.6), 0.75)
            },
            0x29e4_9ebb_5d38_3545,
        ),
    ];
    let digests: Vec<u64> = cases
        .iter()
        .map(|&(still, _)| {
            let changes: Vec<(usize, VoiceMix)> = (0..STILL_FRAMES)
                .step_by(BLOCK)
                .map(|f| (f, still))
                .collect();
            digest(&render(tone(), still, &changes, STILL_FRAMES))
        })
        .collect();
    let expected: Vec<u64> = cases.iter().map(|&(_, digest)| digest).collect();
    assert_eq!(
        digests, expected,
        "digests now (left) against before (right)"
    );
}
