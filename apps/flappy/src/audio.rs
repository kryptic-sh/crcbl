//! Audio for flappy: two procedural cues through `crcbl-audio`'s spatial
//! grammar.
//!
//! A flap and a death, both synthesised at start-up — this sample has no assets
//! by design. The game thread pushes voices onto a shared queue; the audio
//! thread drains it.
//!
//! # Where the listener stands
//!
//! At the centre of the view, not at the bird. The bird sits a third of the way
//! across the screen and stays there, so a listener riding it would hear every
//! sound dead centre at a constant distance and the grammar would be doing
//! nothing observable.
//!
//! From the camera's centre the bird is off to the left by a fixed amount and
//! *above or below* by however far it has climbed, so [`compute_cue`] answers
//! with a small constant pan and a distance that moves with the bird's height:
//! a flap at the top of the screen is quieter and a shade higher than one near
//! the ground. That is rules 1 and 3 of the grammar doing what they are for,
//! and it is audible without being a gimmick.
//!
//! [`compute_cue`]: crcbl_audio::spatial::compute_cue
//!
//! # This file is nearly `apps/breakout/src/audio.rs`
//!
//! Deliberately not shared, and recorded as a finding rather than fixed here.
//! The queue, the voice, the interleaved-stereo playhead and the sine generator
//! are the same in both games because `crcbl-audio` offers a device, a stream
//! and a cue grammar but nothing in between — there is no "play this buffer
//! once, panned" that a game can call. The second consumer is what makes that a
//! gap rather than a preference; see `docs/plan/ROADMAP.md`'s S1B findings.

use std::sync::{Arc, Mutex};

use crcbl_audio::{AudioSample, AudioStream};

/// The wing-beat.
pub const SOUND_FLAP: u32 = 1;
/// The end of a run.
pub const SOUND_DEATH: u32 = 2;

/// A procedural sound: interleaved stereo f32 samples.
#[derive(Debug)]
struct Sound {
    data: Vec<AudioSample>,
}

/// A playing voice with its own playhead.
///
/// `playhead` counts **frames**, not samples: `Sound::data` is interleaved
/// stereo, so a playhead advancing one *index* per output frame plays every
/// sound at half speed and twice the length. Breakout shipped that bug once.
#[derive(Debug)]
struct Voice {
    sound: Arc<Sound>,
    playhead: f64,
    volume: f32,
    pitch: f32,
    gain_l: f32,
    gain_r: f32,
}

impl Voice {
    /// Mixes this voice into `buffer` and reports whether it has audio left.
    fn render_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        let data = &self.sound.data;
        let frames = data.len() / 2;
        // A ratio of 1.0 is one source frame per output frame. Non-finite or
        // non-positive ratios would stall the voice forever or index backwards.
        let step = if self.pitch.is_finite() && self.pitch > 0.0 {
            f64::from(self.pitch)
        } else {
            1.0
        };

        for out in buffer.chunks_exact_mut(2) {
            let frame = self.playhead as usize;
            if frame >= frames {
                return false;
            }
            out[0] += data[frame * 2] * self.volume * self.gain_l;
            out[1] += data[frame * 2 + 1] * self.volume * self.gain_r;
            self.playhead += step;
        }
        (self.playhead as usize) < frames
    }
}

/// Thread-safe voice queue. The game thread pushes, the audio thread drains.
#[derive(Debug)]
struct VoiceQueue {
    inner: Mutex<Vec<Voice>>,
}

/// The audio source fed to `AudioStream`, called from the audio thread.
struct MixerSource {
    queue: Arc<VoiceQueue>,
}

impl crcbl_audio::AudioSource for MixerSource {
    fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
        let mut voices = self.queue.inner.lock().unwrap_or_else(|e| e.into_inner());
        voices.retain_mut(|voice| voice.render_block(buffer));
    }
}

/// Owns the cues and the output stream.
#[derive(Debug)]
pub struct Audio {
    sounds: Vec<Arc<Sound>>,
    queue: Arc<VoiceQueue>,
    _stream: Option<AudioStream>,
}

impl Audio {
    pub fn new(headless: bool) -> Self {
        let queue = Arc::new(VoiceQueue {
            inner: Mutex::new(Vec::new()),
        });
        // A short high chirp for the flap, a longer low one for the end. Both
        // are short enough that a player flapping four times a second never
        // hears two of the same overlap.
        let sounds = vec![
            Arc::new(Sound {
                data: sine(760.0, 0.05, 48_000),
            }),
            Arc::new(Sound {
                data: sine(180.0, 0.30, 48_000),
            }),
        ];

        let source = MixerSource {
            queue: Arc::clone(&queue),
        };
        let stream = if headless {
            Some(AudioStream::open_null(source))
        } else {
            AudioStream::open(source)
        };
        if stream.is_none() && !headless {
            log::info!("audio: no output device available; the game will be silent");
        }

        Self {
            sounds,
            queue,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `(x, y)` in world space, heard
    /// from a listener at `listener_x` — see the module docs for why that is the
    /// camera's centre and not the bird.
    pub fn play_at(&mut self, id: u32, listener_x: f64, x: f64, y: f64) {
        // Ids are 1-based; `id - 1` on a `u32` underflows to `u32::MAX` for id
        // zero rather than simply missing the table.
        let Some(index) = id.checked_sub(1).map(|i| i as usize) else {
            log::debug!("audio: sound id 0 is not a sound");
            return;
        };
        let Some(sound) = self.sounds.get(index) else {
            return;
        };
        let cue = crcbl_audio::spatial::compute_cue(
            [0.0, 0.0, 0.0],
            [
                (x - listener_x) as f32,
                y as f32,
                // A metre in front, so a sound at the listener's own position is
                // still at a defined direction rather than a zero vector.
                1.0,
            ],
            &crcbl_audio::spatial::CueGrammar::default(),
        );
        self.queue
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Voice {
                sound: Arc::clone(sound),
                playhead: 0.0,
                volume: cue.volume * 0.5,
                pitch: cue.pitch_ratio,
                gain_l: cue.gain_left,
                gain_r: cue.gain_right,
            });
    }

    /// How many voices are queued. For tests and for a debug HUD.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.queue
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// A mono sine wave, faded at both ends, as interleaved stereo.
fn sine(freq_hz: f32, seconds: f32, sample_rate: u32) -> Vec<AudioSample> {
    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let value = 0.3 * (2.0 * std::f32::consts::PI * freq_hz * t).sin() * fade(i, frames);
        out.push(value);
        out.push(value);
    }
    out
}

/// How many frames the fade in and out take, unless the sound is shorter.
const FADE_FRAMES: usize = 60;

/// A linear fade in and out, so a cue starts and stops without a click.
fn fade(i: usize, total: usize) -> f32 {
    debug_assert!(i < total, "fade is only defined inside the sound");
    // `min(total / 2)` because a sound shorter than two fades has no middle;
    // `max(1)` because a zero-length fade divides by zero.
    let fade = FADE_FRAMES.min(total / 2).max(1);
    let from_end = total - i;
    if i < fade {
        i as f32 / fade as f32
    } else if from_end <= fade {
        from_end as f32 / fade as f32
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sound is interleaved stereo, so one output frame must advance the
    /// playhead by one *source frame*.
    #[test]
    fn a_voice_plays_at_the_rate_it_was_recorded() {
        let frames = 100;
        let sound = Arc::new(Sound {
            data: sine(440.0, frames as f32 / 48_000.0, 48_000),
        });
        assert_eq!(sound.data.len(), frames * 2);

        let mut voice = Voice {
            sound: Arc::clone(&sound),
            playhead: 0.0,
            volume: 1.0,
            pitch: 1.0,
            gain_l: 1.0,
            gain_r: 1.0,
        };
        let mut buffer = vec![0.0f32; frames * 2];
        voice.render_block(&mut buffer);
        assert!(
            (voice.playhead - frames as f64).abs() < 1.0,
            "playhead at {} after {frames} frames",
            voice.playhead
        );
        assert_eq!(buffer[0], sound.data[0]);
        assert_eq!(buffer[2], sound.data[2], "every frame was played twice");
    }

    /// An id nothing answers to is ignored rather than underflowing or panicking.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_at(0, 0.0, 0.0, 0.0);
        audio.play_at(9999, 0.0, 0.0, 0.0);
        assert_eq!(audio.voices(), 0);
        audio.play_at(SOUND_FLAP, 0.0, 0.0, 0.0);
        assert_eq!(audio.voices(), 1);
    }

    /// The grammar is actually consulted: a cue away from the listener is not
    /// the same cue as one on top of it.
    ///
    /// Without this, `play_at` could ignore its coordinates entirely and every
    /// test above would still pass — the sample would ship "spatial audio" that
    /// was a constant.
    #[test]
    fn where_a_cue_happens_changes_how_it_sounds() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_FLAP, 0.0, 0.0, 0.0);
        audio.play_at(SOUND_FLAP, 0.0, -8.0, 5.0);
        let voices = audio.queue.inner.lock().expect("no other thread");
        let near = &voices[0];
        let far = &voices[1];
        assert!(
            far.gain_l > far.gain_r,
            "a cue to the left should be louder on the left: {} vs {}",
            far.gain_l,
            far.gain_r
        );
        assert!(
            far.volume < near.volume,
            "a cue further away should be quieter: {} vs {}",
            far.volume,
            near.volume
        );
    }

    /// A sound shorter than the fade window still has a defined envelope.
    #[test]
    fn a_very_short_sound_does_not_underflow_the_fade() {
        for total in 1..=8usize {
            for i in 0..total {
                let env = fade(i, total);
                assert!((0.0..=1.0).contains(&env), "fade({i}, {total}) = {env}");
            }
        }
    }
}
