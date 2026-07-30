//! Voice manager: play, stop, and track active audio sources.
//!
//! A [`Voice`] wraps a buffer of interleaved stereo samples plus playback
//! state (position, volume, looping flag). [`Mixer::fill`] advances every
//! active voice each audio block, mixing the result into the output buffer.

use std::cell::UnsafeCell;

use crate::{AudioSample, AudioSource};

/// A single playable sound.
#[derive(Debug)]
pub struct Voice {
    data: Vec<AudioSample>,
    /// Current playback position in samples (not frames).
    playhead: usize,
    /// Linear volume multiplier (0.0 = silent, 1.0 = full).
    volume: f32,
    /// Whether to loop back to the start when the voice finishes.
    looping: bool,
    /// Stopped voices are removed from the mixer on the next fill.
    stopped: bool,
}

impl Voice {
    /// Create a voice from interleaved stereo data.
    #[must_use]
    pub fn new(data: Vec<AudioSample>) -> Self {
        Self {
            data,
            playhead: 0,
            volume: 1.0,
            looping: false,
            stopped: false,
        }
    }

    /// Set playback volume `[0, 1]`.
    #[must_use]
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume.clamp(0.0, 1.0);
        self
    }

    /// Make the voice loop.
    #[must_use]
    pub fn with_looping(mut self) -> Self {
        self.looping = true;
        self
    }

    /// Stop playback after the current block.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// Whether the voice has finished (reached end, not looping).
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.playhead >= self.data.len() && !self.looping
    }

    /// Mix this voice into `buffer`, advancing the playhead.
    ///
    /// The samples in `buffer` are already zeroed so we accumulate.
    /// Returns `true` if the voice is still active after this block.
    fn mix_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        if self.stopped {
            return false;
        }
        let data_len = self.data.len();
        let mut pos = self.playhead;

        for sample in buffer.iter_mut() {
            if pos >= data_len {
                if self.looping {
                    pos = 0;
                } else {
                    return false;
                }
            }
            *sample += self.data[pos] * self.volume;
            pos += 1;
        }
        self.playhead = pos;
        true
    }
}

/// A mixer that holds active voices and fills an output buffer.
///
/// Uses interior mutability ([`UnsafeCell`]) so it implements
/// [`AudioSource`] with `&self` — the audio thread is the sole mutable
/// accessor.
///
/// Implements [`AudioSource`] so it can be handed directly to
/// [`AudioStream`](crate::AudioStream).
pub struct Mixer {
    voices: UnsafeCell<Vec<Voice>>,
}

// SAFETY: Mixer is Send + Sync because the UnsafeCell is only accessed
// from a single audio thread. The Arc<Mixer> is shared but the callback
// serialises all access.
unsafe impl Send for Mixer {}
unsafe impl Sync for Mixer {}

impl Mixer {
    /// Create an empty mixer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: UnsafeCell::new(Vec::new()),
        }
    }

    /// Add a voice. The mixer takes ownership.
    ///
    /// # Safety (internal)
    ///
    /// Only the audio thread writes to voices; the caller is expected to
    /// call this before streaming begins or from a thread that serialises
    /// with the audio callback.
    pub fn play(&mut self, voice: Voice) {
        self.voices.get_mut().push(voice);
    }

    /// Number of active voices.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        // SAFETY: read-only access from any thread.
        unsafe { &*self.voices.get() }.len()
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSource for Mixer {
    fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
        // SAFETY: fill is called from a single audio thread. The Arc<Mixer>
        // is shared but the audio callback serialises all access.
        let voices = unsafe { &mut *self.voices.get() };
        voices.retain_mut(|voice| voice.mix_block(buffer));
    }
}

impl std::fmt::Debug for Mixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.voice_count();
        f.debug_struct("Mixer")
            .field("voice_count", &count)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CHANNELS;

    /// Generate a simple 440 Hz sine wave for `sample_count` stereo samples.
    #[allow(dead_code)]
    fn sine_wave(frequency: f32, sample_rate: u32, sample_count: usize) -> Vec<AudioSample> {
        let mut out = Vec::with_capacity(sample_count * CHANNELS);
        for n in 0..sample_count {
            let t = n as f32 / sample_rate as f32;
            let v = (t * frequency * 2.0 * std::f32::consts::PI).sin();
            out.push(v);
            out.push(v);
        }
        out
    }

    #[test]
    fn voice_mixes_into_buffer() {
        let data = vec![0.5f32; 64 * CHANNELS]; // DC 0.5
        let mut voice = Voice::new(data);
        let mut buf = vec![0.0f32; 32 * CHANNELS];

        assert!(voice.mix_block(&mut buf));
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn voice_stops_at_end() {
        let data = vec![0.5f32; 16 * CHANNELS];
        let mut voice = Voice::new(data);
        let mut buf = vec![0.0f32; 16 * CHANNELS];

        assert!(voice.mix_block(&mut buf));
        assert!(!voice.mix_block(&mut buf));
    }

    #[test]
    fn voice_loops() {
        let data = vec![0.5f32; 16 * CHANNELS];
        let mut voice = Voice::new(data).with_looping();
        let mut buf = vec![0.0f32; 32 * CHANNELS];

        assert!(voice.mix_block(&mut buf));
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn voice_volume_scales_output() {
        let data = vec![1.0f32; 64 * CHANNELS];
        let mut mixer = Mixer::new();
        mixer.play(Voice::new(data).with_volume(0.25));
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 0.25).abs() < 0.001);
        }
    }

    #[test]
    fn mixer_removes_finished_voices() {
        let data = vec![0.5f32; 8 * CHANNELS];
        let mut mixer = Mixer::new();
        mixer.play(Voice::new(data));
        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn two_voices_sum() {
        let mut mixer = Mixer::new();
        mixer.play(Voice::new(vec![0.25f32; 64 * CHANNELS]));
        mixer.play(Voice::new(vec![0.25f32; 64 * CHANNELS]));
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001, "expected 0.5, got {s}");
        }
    }

    /// Golden-buffer test: a known mix → exact sample values.
    #[test]
    fn golden_buffer_dc_sum() {
        let mut mixer = Mixer::new();
        // Two DC voices at different volumes.
        mixer.play(Voice::new(vec![0.2f32; 16 * CHANNELS]).with_volume(1.0));
        mixer.play(Voice::new(vec![0.3f32; 16 * CHANNELS]).with_volume(1.0));

        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);

        // 0.2 + 0.3 = 0.5 per sample.
        for &s in &buf {
            assert!((s - 0.5).abs() < 1e-6, "golden mismatch: {s}");
        }

        // Second fill: voices exhausted → silence.
        let mut buf2 = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf2, 48_000);
        for &s in &buf2 {
            assert!(s.abs() < 1e-6, "expected silence, got {s}");
        }
        assert_eq!(mixer.voice_count(), 0);
    }
}
