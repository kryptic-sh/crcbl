use crate::itd::ITD_RAMP_FRAMES;
use crate::itd::tests::{BLOCK, PERIOD, STEP_MARGIN, digest, largest_step, tone, tone_slope};
use crate::mix_ramp::tests::peak_tone;
use crate::mixer::{Bus, Mixer, Voice};
use crate::{AudioSource, CHANNELS, INTERNAL_SAMPLE_RATE};

/// The frame the gain changes at, a block boundary.
const CHANGE_FRAME: usize = 8 * BLOCK;

/// How many frames a fader test renders: the change, a second move a block
/// later and its whole ramp, and a settled block after that.
const FRAMES: usize = CHANGE_FRAME + 2 * ITD_RAMP_FRAMES as usize;

/// How many frames the bit-identity test renders. Fixed, because its digests
/// were captured over exactly this many.
const STILL_FRAMES: usize = 16 * BLOCK;

/// How close the settled output must sit to the tone scaled by the new gains
/// written out: the gain chain's rounding.
const SETTLED_TOLERANCE: f32 = 1e-6;

/// One bus gain set: which bus, and to what.
type Fader = (Bus, f32);

/// `source`, looped on `route`, with `start` set before the first block and
/// each `(frame, fader)` of `changes` set at that frame (a block boundary),
/// rendered for `frames` frames.
fn render(
    source: Vec<f32>,
    route: Bus,
    start: &[Fader],
    changes: &[(usize, Fader)],
    frames: usize,
) -> Vec<f32> {
    let mixer = Mixer::new();
    for &(bus, gain) in start {
        mixer.set_bus_gain(bus, gain);
    }
    mixer.play(Voice::new(source).with_looping().with_bus(route));
    let mut out = Vec::with_capacity(frames * CHANNELS);
    for block_start in (0..frames).step_by(BLOCK) {
        for &(frame, (bus, gain)) in changes {
            if frame == block_start {
                mixer.set_bus_gain(bus, gain);
            }
        }
        let mut buf = vec![0.0f32; BLOCK * CHANNELS];
        mixer.fill(&mut buf, INTERNAL_SAMPLE_RATE);
        out.extend_from_slice(&buf);
    }
    out
}

/// That `out` never steps further than the tone's own steepest step allows.
fn assert_no_step(out: &[f32], what: &str) {
    let bound = tone_slope() * STEP_MARGIN;
    let (step, frame, ch) = largest_step(out);
    assert!(
        step <= bound,
        "{what}: channel {ch} stepped {step} at frame {frame} (the fader moved at \
         {CHANGE_FRAME}); the tone's own steepest step is {}, so the bound is {bound}",
        tone_slope(),
    );
}

/// That the last [`BLOCK`] frames of `out` are [`peak_tone`] at `gain`
/// overall, written out.
fn assert_settled_at(out: &[f32], gain: f32) {
    let source = peak_tone();
    let frames = out.as_chunks::<CHANNELS>().0;
    for (n, frame) in frames.iter().enumerate().skip(frames.len() - BLOCK) {
        let expected = source[(n % PERIOD) * CHANNELS] * gain;
        for (ch, got) in frame.iter().enumerate() {
            assert!(
                (got - expected).abs() < SETTLED_TOLERANCE,
                "frame {n} channel {ch}: expected {expected} at gain {gain}, got {got}",
            );
        }
    }
}

/// **A bus fader pulled down ramps instead of stepping.** Full to a tenth at
/// the tone's peak; stepped in one frame, that drops the output by most of
/// the peak, eighteen of the tone's steepest steps.
#[test]
fn a_bus_cut_does_not_step() {
    let out = render(
        peak_tone(),
        Bus::Music,
        &[],
        &[(CHANGE_FRAME, (Bus::Music, 0.1))],
        FRAMES,
    );
    assert_no_step(&out, "music bus 1 → 0.1");
    assert_settled_at(&out, 0.1);
}

/// **The master fader ramps too**: it is a bus like the others, read on its
/// own path as the last multiply.
#[test]
fn a_master_cut_does_not_step() {
    let out = render(
        peak_tone(),
        Bus::Sfx,
        &[],
        &[(CHANGE_FRAME, (Bus::Master, 0.1))],
        FRAMES,
    );
    assert_no_step(&out, "master 1 → 0.1");
    assert_settled_at(&out, 0.1);
}

/// **A fader moved again mid-ramp starts from where the ramp has got to.** A
/// cut, then back up a block later; a ramp that restarted from the first
/// target, or from the old one, would step at the second move.
#[test]
fn a_fader_moved_during_a_ramp_starts_from_where_it_got_to() {
    let out = render(
        peak_tone(),
        Bus::Music,
        &[],
        &[
            (CHANGE_FRAME, (Bus::Music, 0.1)),
            (CHANGE_FRAME + BLOCK, (Bus::Music, 0.8)),
        ],
        FRAMES,
    );
    assert_no_step(&out, "music bus 1 → 0.1 → 0.8");
    assert_settled_at(&out, 0.8);
}

/// **A gain set before the first block is the gain from the first frame**, as
/// [`Voice::with_mix`] is a voice's: a player's saved level was never heard
/// at any other.
#[test]
fn a_gain_set_before_audio_starts_takes_effect_without_a_ramp() {
    let (music, master) = (0.3f32, 0.7f32);
    let out = render(
        peak_tone(),
        Bus::Music,
        &[(Bus::Music, music), (Bus::Master, master)],
        &[],
        BLOCK,
    );
    let source = peak_tone();
    for (n, frame) in out.as_chunks::<CHANNELS>().0.iter().enumerate() {
        let x = source[(n % PERIOD) * CHANNELS] * music * master;
        assert_eq!(
            *frame, [x; CHANNELS],
            "frame {n}: not the tone at its gains"
        );
    }
}

/// **A gain that does not change renders the bits it rendered before bus
/// gains were ramped**, with `set_bus_gain` re-sending the same gains every
/// block as a settings screen redrawn each frame does. The digests were
/// captured from the mixer as it stood before the ramp was written.
#[test]
fn an_unchanged_bus_gain_renders_the_bits_it_did_before_smoothing() {
    let cases: [(Bus, [Fader; 2], u64); 3] = [
        (
            Bus::Sfx,
            [(Bus::Sfx, 1.0), (Bus::Master, 1.0)],
            0x5ea2_887b_34eb_1925,
        ),
        (
            Bus::Music,
            [(Bus::Music, 0.3), (Bus::Master, 0.7)],
            0xa46a_8659_0ab8_db25,
        ),
        (
            Bus::Master,
            [(Bus::Sfx, 0.2), (Bus::Master, 0.45)],
            0xba72_b581_a34f_1e25,
        ),
    ];
    let digests: Vec<u64> = cases
        .iter()
        .map(|(route, faders, _)| {
            let changes: Vec<(usize, Fader)> = (0..STILL_FRAMES)
                .step_by(BLOCK)
                .flat_map(|f| faders.map(|fader| (f, fader)))
                .collect();
            digest(&render(tone(), *route, faders, &changes, STILL_FRAMES))
        })
        .collect();
    let expected: Vec<u64> = cases.iter().map(|&(_, _, digest)| digest).collect();
    assert_eq!(
        digests, expected,
        "digests now (left) against before (right)"
    );
}
