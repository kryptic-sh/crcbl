//! Voice manager: play, stop, and track active audio sources.
//!
//! A [`Voice`] wraps a buffer of interleaved stereo samples plus playback
//! state (position, volume, looping flag). [`Mixer::fill`] advances every
//! active voice each audio block, mixing the result into the output buffer.

use std::sync::Mutex;

use crate::{AudioSample, AudioSource, CHANNELS};

/// Clamp a caller-supplied parameter, substituting `fallback` when it is NaN or
/// infinite.
///
/// `f32::clamp` propagates NaN, and a NaN pitch makes `playhead` stick at zero
/// forever — an immortal voice inside the audio callback. Non-finite values are
/// never meaningful here, so they are replaced at the boundary.
fn finite_or(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// A single playable sound.
#[derive(Debug)]
pub struct Voice {
    /// Interleaved stereo sample data.
    data: Vec<AudioSample>,
    /// Current playback position in **frames** (one frame = [`CHANNELS`]
    /// interleaved samples).
    playhead: usize,
    /// Linear volume multiplier (0.0 = silent, 1.0 = full).
    volume: f32,
    /// Whether to loop back to the start when the voice finishes.
    looping: bool,
    /// Stopped voices are removed from the mixer on the next fill.
    stopped: bool,
    /// Per-channel gains applied during mixing: `[left, right]`.
    /// Defaults to `(1.0, 1.0)` — un-panned, centre.
    gains: (f32, f32),
    /// Pitch ratio for varispeed playback (1.0 = normal).
    /// Values > 1.0 play faster/higher; < 1.0 play slower/lower.
    /// Applied by advancing the playhead at the adjusted rate.
    pitch: f32,
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
            gains: (1.0, 1.0),
            pitch: 1.0,
        }
    }

    /// Set per-channel gains (left, right). Both clamped to `[0, 1]`;
    /// non-finite values fall back to unity.
    #[must_use]
    pub fn with_gains(mut self, left: f32, right: f32) -> Self {
        self.gains = (
            finite_or(left, 1.0, 0.0, 1.0),
            finite_or(right, 1.0, 0.0, 1.0),
        );
        self
    }

    /// Set pitch ratio (varispeed). Clamped to `[0.25, 4.0]`; non-finite
    /// values fall back to normal speed.
    #[must_use]
    pub fn with_pitch(mut self, ratio: f32) -> Self {
        self.pitch = finite_or(ratio, 1.0, 0.25, 4.0);
        self
    }

    /// Set playback volume `[0, 1]`; non-finite values fall back to silence.
    #[must_use]
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = finite_or(volume, 0.0, 0.0, 1.0);
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

    /// Mix this voice into `buffer`, advancing the playhead.
    ///
    /// `buffer` is interleaved stereo: `[L0,R0, L1,R1, …]`. The voice
    /// applies per-channel gains and a varispeed pitch ratio.
    /// Returns `true` if the voice is still active after this block.
    ///
    /// The playhead steps by *frames*, not samples: stepping by samples would
    /// make any `pitch != 1.0` read one interleaved channel into both outputs.
    fn mix_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        if self.stopped {
            return false;
        }
        // An empty (or sub-frame) buffer has nothing to play, and `% frames`
        // below would divide by zero for a looping voice.
        let frames = self.data.len() / CHANNELS;
        if frames == 0 {
            return false;
        }

        // f64 playhead for sub-sample precision under pitch shift.
        let mut pos = self.playhead as f64;
        let step = self.pitch as f64;

        for out in buffer.chunks_exact_mut(CHANNELS) {
            if pos as usize >= frames {
                if self.looping {
                    pos %= frames as f64;
                } else {
                    self.playhead = frames;
                    return false;
                }
            }
            let base = (pos as usize) * CHANNELS;
            out[0] += self.data[base] * self.volume * self.gains.0;
            out[1] += self.data[base + 1] * self.volume * self.gains.1;
            pos += step;
        }
        self.playhead = (pos as usize).min(frames);
        true
    }
}

/// A mixer that holds active voices and fills an output buffer.
///
/// The voice list lives behind a [`Mutex`], so [`AudioSource::fill`] can take
/// `&self` without the mixer having to promise that only one thread ever calls
/// it — a promise nothing could enforce, since `fill` is safe and the mixer is
/// shared through an `Arc`. The audio callback is the only hot path and an
/// uncontended mutex costs a pair of atomics.
///
/// Implements [`AudioSource`] so it can be handed directly to
/// [`AudioStream`](crate::AudioStream).
pub struct Mixer {
    voices: Mutex<Vec<Voice>>,
}

impl Mixer {
    /// Create an empty mixer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: Mutex::new(Vec::new()),
        }
    }

    /// Add a voice. The mixer takes ownership.
    pub fn play(&mut self, voice: Voice) {
        self.voices_mut().push(voice);
    }

    /// Number of active voices.
    #[must_use]
    pub fn voice_count(&self) -> usize {
        self.lock().len()
    }

    /// Lock the voice list, recovering from poisoning.
    ///
    /// A panic in one `fill` must not silently kill audio for the rest of the
    /// process; the voice list is a plain `Vec` and is left consistent.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Voice>> {
        self.voices.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Unlocked access, available because `&mut self` proves exclusivity.
    fn voices_mut(&mut self) -> &mut Vec<Voice> {
        self.voices.get_mut().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SoundBank
// ---------------------------------------------------------------------------

/// Pre-loaded sound data keyed by numeric id.
///
/// A [`SoundBank`] holds raw interleaved stereo samples that can be
/// cloned into new [`Voice`]s on demand. It is the server↔client bridge
/// for audio events: the server sends `(sound_id, position)` and the
/// client creates a spatial voice from the bank.
#[derive(Debug, Clone)]
pub struct SoundBank {
    sounds: std::collections::HashMap<u32, Vec<AudioSample>>,
}

impl SoundBank {
    /// Create an empty sound bank.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sounds: std::collections::HashMap::new(),
        }
    }

    /// Register a sound at `id` with the given interleaved stereo data.
    pub fn insert(&mut self, id: u32, data: Vec<AudioSample>) {
        self.sounds.insert(id, data);
    }

    /// Number of registered sounds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    /// Whether the bank is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    /// Create a new [`Voice`] from the stored sound data.
    ///
    /// Returns `None` if `id` is not registered, or if its data is too short to
    /// hold a single interleaved frame — there is no sound to play, and a
    /// looping voice over it would divide by zero.
    #[must_use]
    pub fn create_voice(&self, id: u32) -> Option<Voice> {
        self.sounds
            .get(&id)
            .filter(|data| data.len() >= CHANNELS)
            .map(|data| Voice::new(data.clone()))
    }
}

impl Default for SoundBank {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SoundBank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SoundBank({} sounds)", self.sounds.len())
    }
}

impl AudioSource for Mixer {
    fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
        self.lock().retain_mut(|voice| voice.mix_block(buffer));

        // Clip once, here, where the finished mix is written: N voices summing
        // past ±1.0 would otherwise wrap or distort in the device. A NaN that
        // reached the buffer becomes silence rather than propagating — the
        // `f32::clamp` on its own returns NaN for NaN.
        for sample in buffer.iter_mut() {
            *sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
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

    /// Interleaved stereo data whose left and right channels differ, so a
    /// decoder that reads the wrong channel is visible.
    fn split_channels(frames: usize) -> Vec<AudioSample> {
        let mut out = Vec::with_capacity(frames * CHANNELS);
        for _ in 0..frames {
            out.push(1.0);
            out.push(-1.0);
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

    #[test]
    fn pitch_shift_keeps_channels_separate() {
        // Stepping by samples instead of frames made a pitched voice read one
        // channel into both outputs; L and R here have opposite signs.
        let mut voice = Voice::new(split_channels(64)).with_pitch(2.0);
        let mut buf = vec![0.0f32; 16 * CHANNELS];
        assert!(voice.mix_block(&mut buf));

        for frame in buf.chunks_exact(CHANNELS) {
            assert!((frame[0] - 1.0).abs() < 1e-6, "left: {}", frame[0]);
            assert!((frame[1] + 1.0).abs() < 1e-6, "right: {}", frame[1]);
        }
    }

    #[test]
    fn pitch_shift_advances_at_the_requested_rate() {
        // A ramp: frame n holds sample value n in both channels.
        let frames = 64;
        let mut data = Vec::new();
        for n in 0..frames {
            let v = n as f32 / frames as f32;
            data.push(v);
            data.push(v);
        }
        let mut voice = Voice::new(data).with_pitch(2.0);
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        assert!(voice.mix_block(&mut buf));

        // Output frame i must be input frame 2i.
        for (i, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let expected = (2 * i) as f32 / frames as f32;
            assert!((frame[0] - expected).abs() < 1e-6, "frame {i}: {frame:?}");
        }
    }

    #[test]
    fn looping_voice_over_empty_data_does_not_panic() {
        // `pos % 0` used to panic inside the audio callback.
        let mut voice = Voice::new(Vec::new()).with_looping();
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        assert!(!voice.mix_block(&mut buf));
        assert!(buf.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn sound_bank_refuses_a_voice_over_an_empty_sound() {
        let mut bank = SoundBank::new();
        bank.insert(1, Vec::new());
        bank.insert(2, vec![0.5; CHANNELS]);
        assert!(bank.create_voice(1).is_none());
        assert!(bank.create_voice(2).is_some());
    }

    #[test]
    fn mix_is_clipped_to_unit_range() {
        let mut mixer = Mixer::new();
        for _ in 0..8 {
            mixer.play(Voice::new(vec![0.9f32; 16 * CHANNELS]));
        }
        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 1.0).abs() < 1e-6, "expected clipped 1.0, got {s}");
        }
    }

    #[test]
    fn non_finite_voice_params_are_rejected() {
        // NaN gain/volume/pitch used to reach the mix: `f32::clamp` returns NaN
        // for NaN and `pos += NaN` makes the voice immortal.
        let mut mixer = Mixer::new();
        mixer.play(
            Voice::new(vec![0.5f32; 4 * CHANNELS])
                .with_gains(f32::NAN, f32::INFINITY)
                .with_pitch(f32::NAN)
                .with_volume(1.0),
        );

        let mut buf = vec![0.0f32; 8 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(buf.iter().all(|s| s.is_finite()), "{buf:?}");
        // Pitch fell back to 1.0, so the 4-frame voice is exhausted by an
        // 8-frame block rather than looping forever at position zero.
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn non_finite_sample_data_never_reaches_the_output() {
        let mut mixer = Mixer::new();
        mixer.play(Voice::new(vec![f32::NAN; 8 * CHANNELS]));
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(buf.iter().all(|s| *s == 0.0), "{buf:?}");
    }

    #[test]
    fn mixer_is_sync_and_fill_is_serialised() {
        use std::sync::Arc;

        let mut mixer = Mixer::new();
        for _ in 0..16 {
            mixer.play(Voice::new(vec![0.1f32; 4096 * CHANNELS]).with_looping());
        }
        let mixer = Arc::new(mixer);

        let threads: Vec<_> = (0..4)
            .map(|_| {
                let mixer: Arc<Mixer> = Arc::clone(&mixer);
                std::thread::spawn(move || {
                    let mut buf = vec![0.0f32; 256 * CHANNELS];
                    for _ in 0..50 {
                        mixer.fill(&mut buf, 48_000);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(mixer.voice_count(), 16);
    }
}
