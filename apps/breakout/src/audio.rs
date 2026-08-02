//! Audio for breakout: procedural sound generation through the engine's mixer.
//!
//! Generates sine-wave beeps for bounce and brick-break sounds, banks them in a
//! [`SoundBank`] and plays them through a shared [`Mixer`] that is also the
//! stream's source. The game thread calls [`Audio::play_panned`]; the audio
//! thread fills from the same mixer.
//!
//! # Where the listener stands
//!
//! At the screen centre — X = 0, one unit in front — which is where the camera
//! is. Only the emitter's X moves, so the grammar answers with a pan and a
//! distance and nothing else varies.
//!
//! # What this file used to be
//!
//! Its own `Sound`, `Voice`, `VoiceQueue` and `MixerSource`, copied into all
//! four samples, because `crcbl-audio`'s [`Mixer::play`] wanted `&mut self`
//! while `AudioStream::open` consumed its source — nothing could hold both ends.
//! [`Mixer::play`] takes `&self` now and the stream takes an `Arc`, so the
//! playhead, the queue and the mixing loop are the engine's.

use std::sync::Arc;

use crcbl_audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl_audio::spatial::{CueGrammar, compute_cue};
use crcbl_audio::{AudioSample, AudioStream};

pub const SOUND_BOUNCE: u32 = 1;
pub const SOUND_BRICK: u32 = 2;

/// How loud a cue is against the volume the grammar asks for.
///
/// Halved: the grammar's `volume` is a distance rolloff that reaches 1.0 on top
/// of the listener, and several cues overlapping at full scale clip.
const MASTER_GAIN: f32 = 0.5;

/// Manages sound loading and playback. Voices are created on the game thread
/// from the bank and played into the mixer the audio thread drains.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    mixer: Arc<Mixer>,
    _stream: Option<AudioStream>,
}

impl Audio {
    pub fn new(headless: bool) -> Self {
        let mut bank = SoundBank::new();
        bank.insert(SOUND_BOUNCE, gen_sine(440.0, 0.06, 48000));
        bank.insert(SOUND_BRICK, gen_sine(660.0, 0.09, 48000));

        // The mixer is the stream's source *and* the game's handle: the stream
        // moves its copy onto the audio thread and this one stays here.
        let mixer = Arc::new(Mixer::new());
        let stream: Option<AudioStream> = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };

        if stream.is_none() && !headless {
            log::info!("audio: no output device available; sounds will be silent");
        }

        Self {
            bank,
            mixer,
            _stream: stream,
        }
    }

    /// Play a sound with spatial panning based on `emitter_x` (world X).
    /// Listener is assumed at the screen centre (X=0, Z=1 in front).
    pub fn play_panned(&mut self, id: u32, emitter_x: f32) {
        // An id the bank does not know is simply absent — no `id - 1` index to
        // underflow, which is what this used to have to guard.
        let Some(voice) = self.bank.create_voice(id) else {
            log::debug!("audio: no sound registered at id {id}");
            return;
        };
        let cue = compute_cue(
            [0.0, 0.0, 0.0],       // listener at origin
            [emitter_x, 0.0, 1.0], // emitter in front plane
            &CueGrammar::default(),
        );
        self.mixer.play(voice.with_mix(VoiceMix {
            volume: cue.volume * MASTER_GAIN,
            ..VoiceMix::from(&cue)
        }));
    }
}

/// Generate a mono sine wave and convert to interleaved stereo f32.
fn gen_sine(freq_hz: f32, duration_secs: f32, sample_rate: u32) -> Vec<AudioSample> {
    let num_frames = (sample_rate as f32 * duration_secs) as usize;
    let mut out = Vec::with_capacity(num_frames * 2);
    for i in 0..num_frames {
        let t = i as f32 / sample_rate as f32;
        let sample = 0.3 * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
        // Envelope to avoid clicks.
        let env = fade_env(i, num_frames);
        let val = sample * env;
        out.push(val);
        out.push(val);
    }
    out
}

/// A linear fade in and out over `FADE_FRAMES`, or over half the sound —
/// whichever is shorter.
///
/// The previous version compared `i > total.saturating_sub(fade)` and then
/// computed `total - i`, which is fine only while `total >= fade`. For a sound
/// shorter than 1.25 ms at 48 kHz the saturating subtraction clamped to zero,
/// every index took the second branch, and `total - i` underflowed on the last
/// sample.
const FADE_FRAMES: usize = 60;

fn fade_env(i: usize, total: usize) -> f32 {
    debug_assert!(i < total, "fade_env is only defined inside the sound");
    let fade = FADE_FRAMES.min(total / 2).max(1);
    let from_start = i;
    let from_end = total - i;
    if from_start < fade {
        from_start as f32 / fade as f32
    } else if from_end <= fade {
        from_end as f32 / fade as f32
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generator produces **interleaved stereo**, which is what the mixer's
    /// playhead assumes: a mono buffer would be played at half speed over twice
    /// the length. This sample shipped that bug once, in its own playhead.
    #[test]
    fn a_cue_is_interleaved_stereo_of_the_length_it_asked_for() {
        let frames = 100;
        let data = gen_sine(440.0, frames as f32 / 48_000.0, 48_000);
        assert_eq!(data.len(), frames * 2, "not stereo pairs");
        for frame in data.chunks_exact(2) {
            assert_eq!(frame[0], frame[1], "not the same in both ears");
        }
        assert!(data.iter().any(|s| s.abs() > 1e-3), "the cue is silent");
    }

    /// An id nothing answers to is ignored rather than playing something else.
    #[test]
    fn an_unknown_cue_is_ignored() {
        let mut audio = Audio::new(true);
        audio.play_panned(0, 0.0);
        audio.play_panned(9999, 0.0);
        assert_eq!(audio.mixer.voice_count(), 0);
        audio.play_panned(SOUND_BOUNCE, 0.0);
        assert_eq!(audio.mixer.voice_count(), 1);
    }

    /// The grammar is actually consulted: a brick broken to the left is louder
    /// on the left, and one further out is quieter.
    ///
    /// Without this, `play_panned` could ignore `emitter_x` entirely and the
    /// sample would ship "spatial audio" that was a constant. Read off the
    /// mixer's own snapshot, so it is what was *queued*, not what a helper
    /// returned.
    #[test]
    fn where_a_cue_happens_changes_how_it_sounds() {
        let mut audio = Audio::new(true);
        audio.play_panned(SOUND_BRICK, 0.0);
        audio.play_panned(SOUND_BRICK, -6.0);
        audio.play_panned(SOUND_BRICK, 6.0);
        let mixes = audio.mixer.voice_mixes();
        assert_eq!(mixes.len(), 3, "a cue went missing");
        let (near, left, right) = (mixes[0].1, mixes[1].1, mixes[2].1);
        assert!(
            left.gains.0 > left.gains.1,
            "a cue to the left should be louder on the left: {:?}",
            left.gains,
        );
        assert!(
            right.gains.1 > right.gains.0,
            "and one to the right, on the right: {:?}",
            right.gains,
        );
        assert!(
            left.volume < near.volume,
            "a cue further away should be quieter: {} vs {}",
            left.volume,
            near.volume,
        );
    }

    /// A sound shorter than the fade window still has a defined envelope.
    #[test]
    fn a_very_short_sound_does_not_underflow_the_fade() {
        for total in 1..=8usize {
            for i in 0..total {
                let env = fade_env(i, total);
                assert!((0.0..=1.0).contains(&env), "fade_env({i}, {total}) = {env}",);
            }
        }
        // And a 10-frame sound really is under the 60-frame fade window.
        let short = gen_sine(440.0, 10.0 / 48_000.0, 48_000);
        assert_eq!(short.len(), 20);
        assert!(short.iter().all(|s| s.is_finite()));
    }
}
