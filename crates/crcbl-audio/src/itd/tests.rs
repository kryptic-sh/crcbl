use super::{ITD_RAMP_FRAMES, ITD_SWEEP_LIMIT};
use crate::mixer::{Mixer, Voice, VoiceMix};
use crate::{AudioSource, CHANNELS, INTERNAL_SAMPLE_RATE};

/// The block every test renders in, and the boundary a re-aim lands on.
pub(crate) const BLOCK: usize = 256;

/// The tone's period in frames. A whole number, so the looped data joins
/// without a seam, and a power of two dividing [`CHANGE_FRAME`], so every
/// re-aim lands on an upward zero crossing: where the tone is steepest and a
/// delay that steps shows the most.
pub(crate) const PERIOD: usize = 128;

/// The tone's peak level.
const AMPLITUDE: f32 = 0.5;

/// The frame the ITD changes at, a block boundary.
const CHANGE_FRAME: usize = 8 * BLOCK;

/// How many frames a re-aim test renders: the change, room for a second
/// re-aim held behind a whole move and then its own move, and a settled block
/// after that to check where it settled.
const FRAMES: usize = CHANGE_FRAME + 3 * ITD_RAMP_FRAMES as usize;

/// How many frames the bit-identity test renders. Fixed rather than
/// [`FRAMES`], because its digests were captured over exactly this many.
const STILL_FRAMES: usize = 16 * BLOCK;

/// The first frame a step is measured from: by then the deepest delay a test
/// starts at has filled with tone, so the line's silent start is not counted.
pub(crate) const MEASURE_FROM: usize = 2 * BLOCK;

/// How far above the tone's own steepest step the output may step. The
/// smoothing adds a sliver to the slope (a sweep runs the tone a little faster
/// or slower, a crossfade moves between two taps), and a delay that steps adds
/// a multiple of it.
pub(crate) const STEP_MARGIN: f32 = 1.10;

/// How close the settled output must sit to the tone delayed by the new ITD
/// written out: the gain chain's rounding at unity gains.
const SETTLED_TOLERANCE: f32 = 1e-6;

/// One period's worth of sine, both channels the same, looped by the voice.
///
/// `crcbl_core::trig::sin` rather than `f64::sin`, so the samples are the same
/// bits on every target and the digest below can pin them.
pub(crate) fn tone() -> Vec<f32> {
    let mut out = Vec::with_capacity(PERIOD * CHANNELS);
    for n in 0..PERIOD {
        let phase = core::f64::consts::TAU * n as f64 / PERIOD as f64;
        let x = AMPLITUDE * crcbl_core::trig::sin(phase) as f32;
        out.extend([x; CHANNELS]);
    }
    out
}

/// The tone's sample at frame `n`, which may be before the start (silence).
fn tone_at(n: isize) -> f32 {
    if n < 0 {
        return 0.0;
    }
    tone()[(n as usize % PERIOD) * CHANNELS]
}

/// The tone's largest sample-to-sample step: the slope every bound is made of.
pub(crate) fn tone_slope() -> f32 {
    let x = tone();
    (0..PERIOD)
        .map(|n| (x[((n + 1) % PERIOD) * CHANNELS] - x[n * CHANNELS]).abs())
        .fold(0.0, f32::max)
}

fn at_itd(itd_samples: f32) -> VoiceMix {
    VoiceMix {
        itd_samples,
        ..VoiceMix::UNITY
    }
}

/// The tone played at `before`, re-aimed by `set_mix` at each `(frame, itd)`
/// of `changes` (each a block boundary), rendered for `frames` frames.
fn render(before: f32, changes: &[(usize, f32)], frames: usize) -> Vec<f32> {
    let mixer = Mixer::new();
    let id = mixer.play(Voice::new(tone()).with_looping().with_mix(at_itd(before)));
    let mut out = Vec::with_capacity(frames * CHANNELS);
    for start in (0..frames).step_by(BLOCK) {
        for &(frame, itd) in changes {
            if frame == start {
                assert!(mixer.set_mix(id, at_itd(itd)), "the voice ended");
            }
        }
        let mut buf = vec![0.0f32; BLOCK * CHANNELS];
        mixer.fill(&mut buf, INTERNAL_SAMPLE_RATE);
        out.extend_from_slice(&buf);
    }
    out
}

/// The largest step either channel takes from [`MEASURE_FROM`] on, with the
/// frame and channel it was taken at.
pub(crate) fn largest_step(out: &[f32]) -> (f32, usize, usize) {
    let frames = out.as_chunks::<CHANNELS>().0;
    let mut largest = (0.0, 0, 0);
    for (n, pair) in frames.windows(2).enumerate().skip(MEASURE_FROM - 1) {
        for (ch, (now, before)) in pair[1].iter().zip(&pair[0]).enumerate() {
            let step = (now - before).abs();
            if step > largest.0 {
                largest = (step, n + 1, ch);
            }
        }
    }
    largest
}

/// The step bound, and that `out` keeps under it.
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

/// That the last [`BLOCK`] frames of `out` are the tone delayed by `itd`
/// written out: positive delays the right channel, and a fractional delay
/// interpolates between the two taps.
fn assert_settled_at(out: &[f32], itd: f32) {
    let delays = [(-itd).max(0.0), itd.max(0.0)];
    let frames = out.as_chunks::<CHANNELS>().0;
    for (n, frame) in frames.iter().enumerate().skip(frames.len() - BLOCK) {
        for (ch, (got, delay)) in frame.iter().zip(delays).enumerate() {
            let whole = delay.floor();
            let frac = delay - whole;
            let k = n as isize - whole as isize;
            let expected = tone_at(k) * (1.0 - frac) + tone_at(k - 1) * frac;
            assert!(
                (got - expected).abs() < SETTLED_TOLERANCE,
                "frame {n} channel {ch}: expected the tone {itd} samples late \
                 ({expected}), got {got}",
            );
        }
    }
}

/// **A small ITD change glides instead of stepping.** The right channel's
/// delay moves by a sample and a half at an upward zero crossing; stepped in
/// one frame, that jumps the channel by about two and a half of the tone's
/// steepest steps.
#[test]
fn a_small_itd_change_does_not_step() {
    let out = render(2.0, &[(CHANGE_FRAME, 0.5)], FRAMES);
    assert_no_step(&out, "2 → 0.5 samples");
    assert_settled_at(&out, 0.5);
}

/// **A large ITD jump on the same ear does not step either**, and does not
/// sweep: twenty-two samples in one frame is a jump of most of the tone's
/// peak-to-peak level.
#[test]
fn a_large_itd_jump_does_not_step() {
    let out = render(3.0, &[(CHANGE_FRAME, 25.0)], FRAMES);
    assert_no_step(&out, "3 → 25 samples");
    assert_settled_at(&out, 25.0);
}

/// **Swapping which ear leads does not step.** The right ear's delay falls
/// from twenty samples to none while the left's rises from none to twenty.
#[test]
fn an_ear_swap_does_not_step() {
    let out = render(20.0, &[(CHANGE_FRAME, -20.0)], FRAMES);
    assert_no_step(&out, "20 → −20 samples");
    assert_settled_at(&out, -20.0);
}

/// One click per [`PERIOD`] frames, both channels, looped by the voice.
fn clicks() -> Vec<f32> {
    let mut out = vec![0.0; PERIOD * CHANNELS];
    out[..CHANNELS].fill(1.0);
    out
}

/// **An ear swap crossfades two taps; it does not sweep the delay.** Sweeping
/// twenty samples would read every delay in between, and a delay that moves
/// is a pitch that bends (see [`super`]). So each click of a train, through
/// the whole move, must come out only at the old delay and the new one,
/// weighted between them — never at a delay between the two. The step bound
/// above cannot see this: a twenty-sample sweep steepens the tone by only a
/// couple of percent.
///
/// The large jump on one ear is held to the same, for the same reason.
#[test]
fn an_ear_swap_crossfades_two_taps_instead_of_sweeping() {
    for (before, after) in [(20.0f32, -20.0f32), (3.0, 25.0)] {
        let mixer = Mixer::new();
        let id = mixer.play(Voice::new(clicks()).with_looping().with_mix(at_itd(before)));
        let mut out = vec![0.0f32; FRAMES * CHANNELS];
        let (head, tail) = out.split_at_mut(CHANGE_FRAME * CHANNELS);
        mixer.fill(head, INTERNAL_SAMPLE_RATE);
        assert!(mixer.set_mix(id, at_itd(after)), "the voice ended");
        mixer.fill(tail, INTERNAL_SAMPLE_RATE);

        let taps = |itd: f32| [(-itd).max(0.0) as usize, itd.max(0.0) as usize];
        let (old, new) = (taps(before), taps(after));
        let frames = out.as_chunks::<CHANNELS>().0;
        let mut heard_both = [false; CHANNELS];
        for (n, frame) in frames.iter().enumerate().skip(CHANGE_FRAME) {
            for ch in 0..CHANNELS {
                if frame[ch] == 0.0 {
                    continue;
                }
                let offset = n % PERIOD;
                assert!(
                    offset == old[ch] || offset == new[ch],
                    "{before} → {after}: channel {ch} heard a click {offset} frames \
                     late at frame {n}, between the old delay {} and the new {}",
                    old[ch],
                    new[ch],
                );
                heard_both[ch] |= old[ch] != new[ch] && frame[ch] < 1.0;
            }
        }
        for ch in 0..CHANNELS {
            if old[ch] != new[ch] {
                assert!(
                    heard_both[ch],
                    "{before} → {after}: channel {ch} never blended its two taps",
                );
            }
        }
    }
}

/// **A re-aim that arrives mid-change still lands, without a step.** Two
/// large jumps a block apart, the second while the first is still under way.
#[test]
fn a_re_aim_during_a_change_still_lands_without_a_step() {
    let out = render(
        20.0,
        &[(CHANGE_FRAME, -20.0), (CHANGE_FRAME + BLOCK, 10.0)],
        FRAMES,
    );
    assert_no_step(&out, "20 → −20 → 10 samples");
    assert_settled_at(&out, 10.0);
}

/// The FNV-1a digest of `out`'s sample bits.
pub(crate) fn digest(out: &[f32]) -> u64 {
    let bytes: Vec<u8> = out.iter().flat_map(|s| s.to_bits().to_le_bytes()).collect();
    crcbl_core::hash::fnv1a_64(&bytes)
}

/// **A constant ITD renders the bits it rendered before smoothing existed**,
/// with `set_mix` re-sending the same mix every block as a game re-aiming a
/// still emitter does. The digests were captured from the mixer as it stood
/// before the ramp and crossfade were written; every operation behind them is
/// one IEEE-754 rounds exactly and the tone is built from the engine's own
/// `sin`, so they hold on every target.
#[test]
fn a_constant_itd_renders_the_bits_it_did_before_smoothing() {
    let cases: [(f32, u64); 3] = [
        (0.0, 0x5ea2_887b_34eb_1925),
        (-7.25, 0x4cc1_e532_13a6_ba91),
        (12.5, 0x4f6a_00aa_0311_a29b),
    ];
    let digests: Vec<(f32, u64)> = cases
        .iter()
        .map(|&(itd, _)| {
            let changes: Vec<(usize, f32)> =
                (0..STILL_FRAMES).step_by(BLOCK).map(|f| (f, itd)).collect();
            (itd, digest(&render(itd, &changes, STILL_FRAMES)))
        })
        .collect();
    let expected: Vec<(f32, u64)> = cases.to_vec();
    assert_eq!(
        digests, expected,
        "digests now (left) against before (right)"
    );
}

/// The smallest pitch change a listener can hear, in cents: the usual figure
/// for a sustained tone is about five.
const AUDIBLE_CENTS: f64 = 5.0;

/// **A sweep bends pitch by less than a listener hears.** A delay moving by
/// `Δ` samples over `N` frames plays at `1 ± Δ/N` of normal speed, and the
/// largest swept `Δ` is [`ITD_SWEEP_LIMIT`]; anything larger crossfades, which
/// does not move a tap at all.
#[test]
fn a_sweep_bends_pitch_by_less_than_a_listener_hears() {
    let speed = 1.0 + f64::from(ITD_SWEEP_LIMIT) / f64::from(ITD_RAMP_FRAMES);
    let cents = 1200.0 * speed.log2();
    assert!(
        cents < AUDIBLE_CENTS,
        "a {ITD_SWEEP_LIMIT}-sample sweep over {ITD_RAMP_FRAMES} frames bends \
         {cents} cents",
    );
}
