//! Audio for breakout: procedural sound generation and output stream.
//!
//! Generates sine-wave beeps for bounce and brick-break sounds. Uses
//! `crcbl-audio`'s `AudioStream` with a custom source that drains a
//! shared voice queue. The game thread pushes; the audio thread mixes.

use std::sync::{Arc, Mutex};

use crcbl_audio::{AudioSample, AudioStream};

pub const SOUND_BOUNCE: u32 = 1;
pub const SOUND_BRICK: u32 = 2;

/// A procedural sound: interleaved stereo f32 samples.
#[derive(Debug, Clone)]
struct Sound {
    data: Vec<AudioSample>,
}

/// A playing voice with its own playhead.
///
/// `playhead` counts **frames**, not samples. `Sound::data` is interleaved
/// stereo, so a frame is two `f32`s — a playhead that advanced one *index* per
/// output frame played every sound at half speed and twice the length.
struct Voice {
    sound: Arc<Sound>,
    playhead: f64,
    volume: f32,
    pitch: f32,
    gain_l: f32,
    gain_r: f32,
}

/// Thread-safe voice queue. Game thread pushes, audio thread drains.
struct VoiceQueue {
    inner: Mutex<Vec<Voice>>,
}

impl VoiceQueue {
    fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }
    fn push(&self, v: Voice) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).push(v);
    }
}
/// The audio source fed to `AudioStream`. Called from the audio thread.
struct MixerSource {
    queue: Arc<VoiceQueue>,
}

impl crcbl_audio::AudioSource for MixerSource {
    fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
        let mut voices = self.queue.inner.lock().unwrap_or_else(|e| e.into_inner());
        voices.retain_mut(|voice| {
            if !voice.render_block(buffer) {
                return false;
            }
            true
        });
    }
}

impl Voice {
    /// Mixes this voice into `buffer` and reports whether it still has audio
    /// left. `buffer` is interleaved stereo, same as the source.
    fn render_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        let data = &self.sound.data;
        let source_frames = data.len() / 2;
        // A ratio of 1.0 means one source frame per output frame: unity pitch.
        // Non-finite or non-positive ratios would either stall the voice
        // forever or index backwards, so they play at unity instead.
        let step = if self.pitch.is_finite() && self.pitch > 0.0 {
            f64::from(self.pitch)
        } else {
            1.0
        };

        for out in buffer.chunks_exact_mut(2) {
            let frame = self.playhead as usize;
            if frame >= source_frames {
                return false;
            }
            let idx = frame * 2;
            out[0] += data[idx] * self.volume * self.gain_l;
            out[1] += data[idx + 1] * self.volume * self.gain_r;
            self.playhead += step;
        }

        (self.playhead as usize) < source_frames
    }
}

/// Manages sound loading and playback. Creates voices on the game thread
/// by cloning pre-built `Voice` templates.
pub struct Audio {
    sounds: Vec<Arc<Sound>>,
    queue: Arc<VoiceQueue>,
    _stream: Option<AudioStream>,
}

impl Audio {
    pub fn new(headless: bool) -> Self {
        let queue = Arc::new(VoiceQueue::new());
        let bounce = Arc::new(Sound {
            data: gen_sine(440.0, 0.06, 48000),
        });
        let brick = Arc::new(Sound {
            data: gen_sine(660.0, 0.09, 48000),
        });

        let sounds = vec![Arc::clone(&bounce), Arc::clone(&brick)];

        let source = MixerSource {
            queue: queue.clone(),
        };

        let stream: Option<AudioStream> = if headless {
            Some(AudioStream::open_null(source))
        } else {
            AudioStream::open(source)
        };

        if stream.is_none() && !headless {
            log::info!("audio: no output device available; sounds will be silent");
        }

        Self {
            sounds,
            queue,
            _stream: stream,
        }
    }

    /// Play a sound with spatial panning based on `emitter_x` (world X).
    /// Listener is assumed at the screen centre (X=0, Z=1 in front).
    pub fn play_panned(&mut self, id: u32, emitter_x: f32) {
        // Sound ids are 1-based; `id - 1` on a `u32` underflows to `u32::MAX`
        // for id 0 rather than missing the table.
        let Some(idx) = id.checked_sub(1).map(|i| i as usize) else {
            log::debug!("audio: sound id 0 is not a sound");
            return;
        };
        if let Some(sound) = self.sounds.get(idx) {
            let cue = crcbl_audio::spatial::compute_cue(
                [0.0, 0.0, 0.0],       // listener at origin
                [emitter_x, 0.0, 1.0], // emitter in front plane
                &crcbl_audio::spatial::CueGrammar::default(),
            );
            self.queue.push(Voice {
                sound: Arc::clone(sound),
                playhead: 0.0,
                volume: cue.volume * 0.5,
                pitch: cue.pitch_ratio,
                gain_l: cue.gain_left,
                gain_r: cue.gain_right,
            });
        }
    }
}

/// Generate a mono sine wave and convert to interleaved stereo f32.
fn gen_sine(freq_hz: f32, duration_secs: f32, sample_rate: u32) -> Vec<f32> {
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

    /// The sound is interleaved stereo, so one output frame must advance the
    /// playhead by one *source frame*. It used to advance by one sample index
    /// and mask the pair, so a 440 Hz beep came out at 220 Hz over twice the
    /// length.
    #[test]
    fn a_voice_plays_at_the_rate_it_was_recorded() {
        let frames = 100;
        let sound = Arc::new(Sound {
            data: gen_sine(440.0, frames as f32 / 48_000.0, 48_000),
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
        // Exactly the whole sound in one block: the voice must be spent.
        let mut buffer = vec![0.0f32; frames * 2];
        assert!(!voice.render_block(&mut buffer) || voice.playhead >= frames as f64);
        assert!(
            (voice.playhead - frames as f64).abs() < 1.0,
            "playhead at {} after {frames} frames",
            voice.playhead,
        );
        // And the samples came out in source order, not each one twice.
        assert_eq!(buffer[0], sound.data[0]);
        assert_eq!(buffer[2], sound.data[2]);
    }

    /// A pitch ratio of 2 consumes two source frames per output frame.
    #[test]
    fn the_pitch_ratio_scales_the_playhead() {
        let sound = Arc::new(Sound {
            data: gen_sine(440.0, 200.0 / 48_000.0, 48_000),
        });
        let mut voice = Voice {
            sound,
            playhead: 0.0,
            volume: 1.0,
            pitch: 2.0,
            gain_l: 1.0,
            gain_r: 1.0,
        };
        let mut buffer = vec![0.0f32; 50 * 2];
        voice.render_block(&mut buffer);
        assert!((voice.playhead - 100.0).abs() < 1e-6, "{}", voice.playhead);
    }

    /// Sound ids are 1-based and `id - 1` underflows a `u32` at zero.
    #[test]
    fn sound_id_zero_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_panned(0, 0.0);
        audio.play_panned(9999, 0.0);
        assert_eq!(audio.queue.inner.lock().unwrap().len(), 0);
        audio.play_panned(SOUND_BOUNCE, 0.0);
        assert_eq!(audio.queue.inner.lock().unwrap().len(), 1);
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
