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
struct Voice {
    sound: Arc<Sound>,
    playhead: usize,
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
    fn render_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        let data = &self.sound.data;
        let data_len = data.len();
        let mut pos = self.playhead as f64;
        let step = self.pitch as f64;

        for frame in buffer.chunks_exact_mut(2) {
            if pos as usize >= data_len {
                return false; // finished
            }
            let idx = pos as usize & !1; // even index for stereo pair
            let s_l = data[idx];
            let s_r = data.get(idx + 1).copied().unwrap_or(0.0);
            frame[0] += s_l * self.volume * self.gain_l;
            frame[1] += s_r * self.volume * self.gain_r;
            pos += step;
        }

        self.playhead = pos as usize;
        self.playhead < data_len
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
        let idx = id as usize - 1;
        if let Some(sound) = self.sounds.get(idx) {
            let cue = crcbl_audio::spatial::compute_cue(
                [0.0, 0.0, 0.0],       // listener at origin
                [emitter_x, 0.0, 1.0], // emitter in front plane
                &crcbl_audio::spatial::CueGrammar::default(),
            );
            self.queue.push(Voice {
                sound: Arc::clone(sound),
                playhead: 0,
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

fn fade_env(i: usize, total: usize) -> f32 {
    let fade = 60usize;
    if i < fade {
        i as f32 / fade as f32
    } else if i > total.saturating_sub(fade) {
        (total - i) as f32 / fade as f32
    } else {
        1.0
    }
}
