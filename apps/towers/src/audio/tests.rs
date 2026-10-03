use crcbl::audio::AudioSource;
use crcbl::math::DVec3;
use crcbl::ui::DebugModule;

use super::*;

/// A cue of `sound` at `at`.
const fn cue(sound: Sound, at: DVec3) -> Cue {
    Cue { sound, at }
}

/// The left and right ends of the committed field's middle line, and its far
/// edge's middle — all on the ground.
const LEFT: DVec3 = DVec3::new(-crate::map::HALF_WIDTH, 0.0, 0.0);
const RIGHT: DVec3 = DVec3::new(crate::map::HALF_WIDTH, 0.0, 0.0);
const NEAR: DVec3 = DVec3::new(0.0, 0.0, crate::map::HALF_DEPTH);
const FAR: DVec3 = DVec3::new(0.0, 0.0, -crate::map::HALF_DEPTH);

/// `seconds` of the mixer's output, as the audio thread would have filled it.
fn render(audio: &Audio, seconds: f32) -> Vec<f32> {
    let mut block = vec![0.0; (SAMPLE_RATE as f32 * seconds) as usize * 2];
    audio.mixer().fill(&mut block, SAMPLE_RATE);
    block
}

/// **Every sound is banked, and plays.** Each cue counts once under its own
/// sound and no other, and is a voice in the mixer.
#[test]
fn every_sound_is_banked_and_counted_under_its_own_name() {
    let mut audio = Audio::offline();
    for (played, sound) in Sound::ALL.into_iter().enumerate() {
        audio.play(cue(sound, DVec3::ZERO));
        for other in Sound::ALL {
            let expected = u64::from(other.index() <= played);
            assert_eq!(audio.plays(other), expected, "{}", other.label());
        }
        assert_eq!(audio.mixer().voice_count(), played + 1);
    }
    assert_eq!(audio.played_total(), Sound::ALL.len() as u64);
    assert_eq!(audio.played().len(), Sound::ALL.len());
}

/// **A cue is heard from where it happened, by an ear on the overhead
/// camera**: a tower on the left of the picture louder in the left ear and
/// one on the right in the right, a creep at the far edge quieter than one at
/// the near edge — read off the mixer's own voices, so it is what was queued.
#[test]
fn a_cue_is_heard_from_where_it_happened_on_the_overhead_view() {
    let mut audio = Audio::offline();
    for at in [LEFT, RIGHT, NEAR, FAR] {
        audio.play(cue(Sound::Kill, at));
    }
    let mixes = audio.mixer().voice_mixes();
    let [left, right, near, far] = [mixes[0].1, mixes[1].1, mixes[2].1, mixes[3].1];
    assert!(left.gains.0 > left.gains.1, "left: {:?}", left.gains);
    assert!(right.gains.1 > right.gains.0, "right: {:?}", right.gains);
    assert!(
        far.volume < near.volume,
        "the far edge is not quieter: {} vs {}",
        far.volume,
        near.volume
    );
}

/// **The ear follows the camera.** On the overhead view it is at the eye; on
/// a camera turned to face the other way, the field's left is on the right.
#[test]
fn the_ear_follows_the_camera_the_frame_is_drawn_from() {
    let mut audio = Audio::offline();
    let overhead = crate::camera::camera();
    assert_eq!(
        audio.mixer().listener().position,
        overhead.eye.to_array(),
        "the ear is not on the overhead camera from the start"
    );

    let mut turned = overhead;
    turned.eye.z = -overhead.eye.z;
    audio.hear_from(&turned);
    assert_eq!(audio.mixer().listener().position, turned.eye.to_array());
    audio.play(cue(Sound::Kill, LEFT));
    let heard = audio.mixer().voice_mixes()[0].1;
    assert!(
        heard.gains.1 > heard.gains.0,
        "seen from the far side, the field's left is not on the right: {:?}",
        heard.gains
    );
}

/// **The voice budget holds, every cue is still counted, and the run's end
/// is heard over a field of hits** raised on the same frame.
#[test]
fn the_budget_holds_and_the_runs_end_is_heard_over_a_flood_of_hits() {
    let mut audio = Audio::offline();
    audio.play(cue(Sound::Lost, DVec3::ZERO));
    for _ in 0..2 * MAX_VOICES {
        audio.play(cue(Sound::Hit, LEFT));
    }
    assert_eq!(audio.mixer().voice_count(), MAX_VOICES);
    assert_eq!(audio.plays(Sound::Hit), 2 * MAX_VOICES as u64);
    assert_eq!(audio.mixer().refused_count(), 0, "a hit steals another hit");
    // Past every hit, short of the loss.
    render(&audio, 0.3);
    assert_eq!(
        audio.mixer().voice_count(),
        1,
        "the loss was not the voice left sounding"
    );
}

/// **The same cues mix to the same samples, and the mix is spatial**: one
/// cue sequence rendered twice is the same buffer to the bit, and a kill on
/// the left of the field puts more of itself in the left channel than the
/// right. The engine's own golden-buffer suite holds the mixer to its
/// reference; this holds towers' sounds and listener to being deterministic
/// and to panning the way the picture does.
#[test]
fn the_same_cues_mix_to_the_same_buffer_and_pan_as_the_picture() {
    let sequence = [
        cue(Sound::Fire(tower::Kind::Splash), NEAR),
        cue(Sound::Burst, LEFT),
        cue(Sound::Kill, LEFT),
        cue(Sound::Wave, FAR),
    ];
    let mixed = || {
        let mut audio = Audio::offline();
        for cue in sequence {
            audio.play(cue);
        }
        render(&audio, 0.5)
    };
    let (first, second) = (mixed(), mixed());
    assert!(
        first.iter().any(|sample| *sample != 0.0),
        "the mix is silent"
    );
    assert!(
        first
            .iter()
            .zip(&second)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "one cue sequence mixed to two buffers"
    );

    let mut audio = Audio::offline();
    audio.play(cue(Sound::Kill, LEFT));
    let buffer = render(&audio, 0.2);
    let energy = |channel: usize| -> f32 {
        buffer
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[channel] * frame[channel])
            .sum()
    };
    assert!(
        energy(0) > energy(1),
        "a kill on the left is not louder on the left: {} vs {}",
        energy(0),
        energy(1)
    );
}

/// **The debug section counts the cues and says what the budget did**, at
/// zero on a fresh mixer and moving once it is pushed.
#[test]
fn the_debug_section_counts_cues_voices_and_the_budget() {
    let rows = |audio: &Audio| {
        let mut section = crcbl::ui::DebugSection::new("audio");
        audio.debug_section(&mut section);
        assert_eq!(section.title(), "audio");
        section
            .rows()
            .iter()
            .map(|row| (row.label.clone(), row.value.clone()))
            .collect::<Vec<_>>()
    };
    let spelled = |values: [&str; 4]| {
        ["cues", "voices", "dropped", "stolen"]
            .into_iter()
            .zip(values)
            .map(|(label, value)| (label.to_string(), value.to_string()))
            .collect::<Vec<_>>()
    };
    let mut audio = Audio::offline();
    assert_eq!(rows(&audio), spelled(["0", "0", "0", "0"]));

    for _ in 0..MAX_VOICES {
        audio.play(cue(Sound::Won, DVec3::ZERO));
    }
    audio.play(cue(Sound::Hit, DVec3::ZERO));
    audio.play(cue(Sound::Won, DVec3::ZERO));
    let total = (MAX_VOICES + 2).to_string();
    let voices = MAX_VOICES.to_string();
    assert_eq!(rows(&audio), spelled([&total, &voices, "1", "1"]));
}
