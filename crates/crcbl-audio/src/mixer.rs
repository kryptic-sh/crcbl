//! Voice manager: play, stop, and track active audio sources.
//!
//! A [`Voice`] wraps a buffer of interleaved stereo samples plus playback
//! state (position, volume, looping flag). [`Mixer::fill`] advances every
//! active voice each audio block, mixing the result into the output buffer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{AudioSample, AudioSource, CHANNELS, INTERNAL_SAMPLE_RATE};

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

/// Per-channel delay-line length. One past the longest ITD the mixer will
/// serve: the clamp below guarantees both fractional taps of any delay stay
/// inside the buffer.
const DELAY_CAPACITY: usize = 64;

/// Largest ITD in samples, per channel. `(capacity - 2)`: a delay of exactly
/// this reads taps at `62` and `63` samples ago, so `capacity - 1` is the
/// deepest tap that still lands in-buffer. Well above the cue grammar's
/// [`CueGrammar::max_itd_samples`](crate::spatial::CueGrammar) default of 30.
const MAX_ITD_DELAY: f32 = (DELAY_CAPACITY - 2) as f32;

/// A per-channel circular buffer that answers a fractional delay.
///
/// `push_and_read` writes `sample` and returns the value `delay` samples
/// ago, linearly interpolated between the taps at `floor(delay)` and
/// `ceil(delay)` — the standard one-multiply fractional delay. The buffer
/// starts silent, so a channel whose delay is `d` fades in over its first
/// `d` frames, which is what an interaural delay is: the far ear's first
/// arrival is later, not quieter.
#[derive(Debug)]
struct DelayLine {
    buf: Vec<f32>,
    /// The slot the next push overwrites.
    write: usize,
}

impl DelayLine {
    /// A silent line of `capacity` slots.
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0.0; capacity],
            write: 0,
        }
    }

    /// Write `sample`, answer the value `delay` samples ago.
    ///
    /// The caller guarantees `delay ∈ [0, capacity - 2]` (that is what
    /// [`MAX_ITD_DELAY`] is for), so both taps are always in-buffer and the
    /// second never aliases the just-written slot.
    fn push_and_read(&mut self, sample: f32, delay: f32) -> f32 {
        self.buf[self.write] = sample;
        let k = delay.floor() as usize;
        let frac = delay - k as f32;
        let at = |k: usize| self.buf[(self.write + self.buf.len() - k) % self.buf.len()];
        let out = at(k) * (1.0 - frac) + at(k + 1) * frac;
        self.write = (self.write + 1) % self.buf.len();
        out
    }
}

/// How a voice sits in the mix: level, pan and speed.
///
/// The three parameters a spatialiser produces, in one value, so a caller can
/// hand the same thing to [`Voice::with_mix`] when the voice starts and to
/// [`Mixer::set_mix`] every tick the emitter moves after that. An emitter that
/// crosses the field while one held voice plays is the case a per-voice setter
/// cannot serve, because the caller no longer owns the [`Voice`].
///
/// The fields are raw caller input. Both entry points clamp them exactly as
/// [`Voice::with_volume`], [`Voice::with_gains`] and [`Voice::with_pitch`] do,
/// so a NaN out of a divide-by-zero distance cannot reach the audio thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceMix {
    /// Linear volume multiplier, clamped to `[0, 1]`.
    pub volume: f32,
    /// Per-channel gains `(left, right)`, each clamped to `[0, 1]`.
    pub gains: (f32, f32),
    /// Varispeed ratio, clamped to `[0.25, 4.0]`.
    pub pitch: f32,
    /// Fractional interaural delay in samples: positive delays the right
    /// channel, negative the left (the [`crate::spatial::SpatialCue`]
    /// contract), clamped to `±MAX_ITD_DELAY`.
    pub itd_samples: f32,
}

impl VoiceMix {
    /// Full volume, centre, normal speed — what a bare [`Voice::new`] plays at.
    pub const UNITY: Self = Self {
        volume: 1.0,
        gains: (1.0, 1.0),
        pitch: 1.0,
        itd_samples: 0.0,
    };
}

impl Default for VoiceMix {
    fn default() -> Self {
        Self::UNITY
    }
}

/// The spatialiser's answer, as something a voice can be played at.
///
/// The glue every game was writing by hand: [`compute_cue`] produces gains, a
/// pitch ratio, a volume and an interaural time difference, and this is what
/// carries them to [`Mixer::play`].
///
/// Nothing is dropped: the pan is the gain difference and the delay is the
/// timing, and a [`Voice`] applies both — the ITD through its per-channel
/// delay line.
///
/// [`compute_cue`]: crate::spatial::compute_cue
impl From<&crate::spatial::SpatialCue> for VoiceMix {
    fn from(cue: &crate::spatial::SpatialCue) -> Self {
        Self {
            volume: cue.volume,
            gains: (cue.gain_left, cue.gain_right),
            pitch: cue.pitch_ratio,
            itd_samples: cue.itd_samples,
        }
    }
}

/// A handle to a voice a [`Mixer`] is playing.
///
/// Handed out by [`Mixer::play`] and accepted by [`Mixer::stop`],
/// [`Mixer::set_mix`] and [`Mixer::is_playing`]. Ids are never reused, so a
/// handle kept past the end of its sound is *stale* rather than aimed at
/// whatever started next — every one of those calls answers `false` for it,
/// which is what lets a caller hold one across the frames a sound is playing
/// without tracking whether it finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId(u64);

/// A single playable sound.
#[derive(Debug)]
pub struct Voice {
    /// Interleaved stereo sample data, shared with whoever holds the sound.
    ///
    /// `Arc<[_]>` rather than `Vec`: [`SoundBank::create_voice`] hands the same
    /// buffer to every voice it makes, and a game raising tens of cues a second
    /// copied the whole sound on each one while this was owned data.
    data: Arc<[AudioSample]>,
    /// Current playback position in **frames** (one frame = [`CHANNELS`]
    /// interleaved samples).
    playhead: usize,
    /// Linear volume multiplier (0.0 = silent, 1.0 = full).
    volume: f32,
    /// Whether to loop back to the start when the voice finishes.
    looping: bool,
    /// Stopped voices are removed from the mixer on the next fill.
    stopped: bool,
    /// Releasing voices mix one final block with a linear fade to silence,
    /// then are dropped by the caller — [`Mixer::stop`] sets this, so a loud
    /// voice stopped mid-cycle ramps down instead of clicking.
    releasing: bool,
    /// Per-channel gains applied during mixing: `[left, right]`.
    /// Defaults to `(1.0, 1.0)` — un-panned, centre.
    gains: (f32, f32),
    /// Pitch ratio for varispeed playback (1.0 = normal).
    /// Values > 1.0 play faster/higher; < 1.0 play slower/lower.
    /// Applied by advancing the playhead at the adjusted rate.
    pitch: f32,
    /// Fractional interaural delay in samples: positive delays the right
    /// channel, negative the left (the [`crate::spatial::SpatialCue`]
    /// contract), clamped to `±MAX_ITD_DELAY`.
    itd_samples: f32,
    /// One fractional delay line per output channel, fed by `itd_samples`.
    delay: [DelayLine; CHANNELS],
}

impl Voice {
    /// Create a voice from interleaved stereo data.
    #[must_use]
    pub fn new(data: Vec<AudioSample>) -> Self {
        Self::from_shared(data.into())
    }

    /// Create a voice over sample data something else owns.
    ///
    /// The buffer is shared, the playback state is not: two voices over one
    /// sound have their own playhead, volume, pan and pitch.
    #[must_use]
    pub fn from_shared(data: Arc<[AudioSample]>) -> Self {
        Self {
            data,
            playhead: 0,
            volume: 1.0,
            looping: false,
            stopped: false,
            releasing: false,
            gains: (1.0, 1.0),
            pitch: 1.0,
            itd_samples: 0.0,
            delay: std::array::from_fn(|_| DelayLine::new(DELAY_CAPACITY)),
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

    /// Set level, pan and speed together, clamped as the individual setters do.
    #[must_use]
    pub fn with_mix(mut self, mix: VoiceMix) -> Self {
        self.apply_mix(mix);
        self
    }

    /// The parameters currently in force.
    #[must_use]
    pub fn mix(&self) -> VoiceMix {
        VoiceMix {
            volume: self.volume,
            gains: self.gains,
            pitch: self.pitch,
            itd_samples: self.itd_samples,
        }
    }

    /// Whether the voice loops rather than ending.
    #[must_use]
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// The shared clamp behind [`Voice::with_mix`] and [`Mixer::set_mix`].
    fn apply_mix(&mut self, mix: VoiceMix) {
        self.volume = finite_or(mix.volume, 0.0, 0.0, 1.0);
        self.gains = (
            finite_or(mix.gains.0, 1.0, 0.0, 1.0),
            finite_or(mix.gains.1, 1.0, 0.0, 1.0),
        );
        self.pitch = finite_or(mix.pitch, 1.0, 0.25, 4.0);
        self.itd_samples = finite_or(mix.itd_samples, 0.0, -MAX_ITD_DELAY, MAX_ITD_DELAY);
    }

    /// Stop playback after the current block.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// Mix this voice into `buffer`, advancing the playhead.
    ///
    /// `buffer` is interleaved stereo: `[L0,R0, L1,R1, …]`. The voice
    /// applies per-channel gains and a varispeed pitch ratio. `sample_rate`
    /// is the rate `buffer` is being produced at: voices are authored for
    /// [`INTERNAL_SAMPLE_RATE`], so a device at any other rate steps the
    /// playhead by the ratio rather than one source frame per output frame,
    /// keeping pitch and duration right on any hardware.
    /// Returns `true` if the voice is still active after this block.
    ///
    /// The playhead steps by *frames*, not samples: stepping by samples would
    /// make any `pitch != 1.0` read one interleaved channel into both outputs.
    fn mix_block(&mut self, buffer: &mut [AudioSample], sample_rate: u32) -> bool {
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
        // Voices are authored for INTERNAL_SAMPLE_RATE; a device at some other
        // rate must not step one source frame per output frame or every sound
        // detunes (147 cents at 44.1 kHz — the exact failure the browser path
        // resamples to avoid). Stepping by the ratio keeps pitch and duration
        // right on any device; when the rates match the ratio is 1.0 and the
        // playhead behaves exactly as before.
        let ratio = if sample_rate == 0 {
            1.0
        } else {
            INTERNAL_SAMPLE_RATE as f64 / f64::from(sample_rate)
        };
        let step = self.pitch as f64 * ratio;

        // The release ramp: a releasing voice mixes ONE final block, faded
        // linearly to silence, and is then dropped by the caller. With `N`
        // frames in this block, frame `i` (0-based) keeps `(N - 1 - i) / (N - 1)`
        // of its level — the first frame is unchanged (fade 1.0, no step down)
        // and the last is exactly silence. A one-frame block is degenerate:
        // the fade is 0.0.
        let block_frames = buffer.len() / CHANNELS;
        // The ITD is fixed for the block. Positive delays the right channel
        // (source left), negative the left (source right) — each channel's
        // line delays its own ear.
        let itd = self.itd_samples;
        let delay = [(-itd).max(0.0), itd.max(0.0)];

        for (i, out) in buffer.chunks_exact_mut(CHANNELS).enumerate() {
            if pos as usize >= frames {
                if self.looping {
                    pos %= frames as f64;
                } else {
                    self.playhead = frames;
                    return false;
                }
            }
            let base = (pos as usize) * CHANNELS;
            let fade = if self.releasing {
                if block_frames <= 1 {
                    0.0
                } else {
                    (block_frames - 1 - i) as f32 / (block_frames - 1) as f32
                }
            } else {
                1.0
            };
            let sample = self.delay[0].push_and_read(self.data[base], delay[0]);
            out[0] += sample * self.volume * self.gains.0 * fade;
            let sample = self.delay[1].push_and_read(self.data[base + 1], delay[1]);
            out[1] += sample * self.volume * self.gains.1 * fade;
            pos += step;
        }
        self.playhead = (pos as usize).min(frames);
        !self.releasing
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
/// [`AudioStream`](crate::AudioStream) — through an [`Arc`], which is what
/// leaves the game a handle to go on playing through. See [`Mixer::play`].
pub struct Mixer {
    voices: Mutex<Vec<(VoiceId, Voice)>>,
    /// Voices mid-release: stopped but playing their one fade-to-silence
    /// block. [`Mixer::voice_count`] and [`Mixer::is_playing`] do not count
    /// them — [`Mixer::stop`] frees the slot on the spot — and the next
    /// [`fill`](AudioSource::fill) mixes the ramp and drops each one.
    releasing: Mutex<Vec<(VoiceId, Voice)>>,
    /// The next handle to hand out. Monotonic; ids are never reused, so a
    /// stale [`VoiceId`] can never name a later voice.
    next_id: AtomicU64,
}

impl Mixer {
    /// Create an empty mixer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            voices: Mutex::new(Vec::new()),
            releasing: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Start a voice, and answer with the handle that steers it.
    ///
    /// **Takes `&self`, which is the whole reason a game can use this mixer at
    /// all.** [`AudioStream::open`](crate::AudioStream::open) consumes its
    /// source, so once the stream is running the only reference left is a
    /// shared one — a `&mut self` here meant nothing could ever play a sound.
    /// The voice list is already behind a [`Mutex`] for [`AudioSource::fill`],
    /// so this costs the same uncontended lock that path costs.
    ///
    /// The handle is worth keeping only for a sound that outlives the call:
    /// [`Mixer::stop`] and [`Mixer::set_mix`] need it. A one-shot cue can drop
    /// it, which is why this is not `#[must_use]`.
    pub fn play(&self, voice: Voice) -> VoiceId {
        let id = VoiceId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.lock().push((id, voice));
        id
    }

    /// Stop `id` and fade it out. Answers whether it was still playing.
    ///
    /// The accounting is still immediate — [`Mixer::voice_count`] and
    /// [`Mixer::is_playing`] drop on the spot and the cap slot frees — but
    /// the sound itself is not cut: it mixes one final block with a linear
    /// fade to silence (the release ramp), then is dropped by the next fill.
    ///
    /// A mixer nothing is filling keeps its accounting right: the stopped
    /// voice is already out of the list [`Mixer::voice_count`] reads, and its
    /// release data waits in `releasing` until a fill reaps it — the same
    /// shape as a finished one-shot waiting in `voices`.
    pub fn stop(&self, id: VoiceId) -> bool {
        let mut voices = self.lock();
        let Some(index) = voices.iter().position(|(voice_id, _)| *voice_id == id) else {
            return false;
        };
        let (_, mut voice) = voices.remove(index);
        voice.releasing = true;
        self.lock_releasing().push((id, voice));
        true
    }

    /// Whether `id` is still sounding.
    ///
    /// `false` for a one-shot that has run out, for a voice
    /// [`stopped`](Mixer::stop), and for a handle this mixer never issued.
    #[must_use]
    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.lock().iter().any(|(voice_id, _)| *voice_id == id)
    }

    /// Re-aim a playing voice. Answers whether it was still playing.
    ///
    /// What a held sound needs: a looping engine on a ship that crosses the
    /// field is one voice whose pan and volume move under it, not a new voice
    /// per frame. Clamped exactly as [`Voice::with_mix`] is.
    pub fn set_mix(&self, id: VoiceId, mix: VoiceMix) -> bool {
        let mut voices = self.lock();
        let Some((_, voice)) = voices.iter_mut().find(|(voice_id, _)| *voice_id == id) else {
            return false;
        };
        voice.apply_mix(mix);
        true
    }

    /// A snapshot of every sounding voice's handle and mix parameters.
    ///
    /// For debug overlays and for tests asking what a spatialiser actually
    /// queued. A snapshot rather than a closure over the live list because the
    /// [`Mutex`] is not reentrant: a caller that reached [`Mixer::voice_count`]
    /// from inside a borrow of the voices would deadlock the audio thread.
    #[must_use]
    pub fn voice_mixes(&self) -> Vec<(VoiceId, VoiceMix)> {
        self.lock()
            .iter()
            .map(|(id, voice)| (*id, voice.mix()))
            .collect()
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
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(VoiceId, Voice)>> {
        self.voices.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lock the releasing list, recovering from poisoning, as [`Mixer::lock`]
    /// does for the voice list.
    fn lock_releasing(&self) -> std::sync::MutexGuard<'_, Vec<(VoiceId, Voice)>> {
        self.releasing.lock().unwrap_or_else(|e| e.into_inner())
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
/// A [`SoundBank`] holds raw interleaved stereo samples that new [`Voice`]s are
/// opened over on demand. It is the server↔client bridge for audio events: the
/// server sends `(sound_id, position)` and the client creates a spatial voice
/// from the bank.
///
/// The buffers are shared, not copied — see [`SoundBank::create_voice`].
#[derive(Debug, Clone)]
pub struct SoundBank {
    sounds: std::collections::HashMap<u32, Arc<[AudioSample]>>,
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
        self.sounds.insert(id, data.into());
    }

    /// Register a sound whose buffer something else already holds.
    pub fn insert_shared(&mut self, id: u32, data: Arc<[AudioSample]>) {
        self.sounds.insert(id, data);
    }

    /// The samples registered at `id`, if any.
    ///
    /// A read-back, so a caller can check what it banked — the length a cue
    /// actually came out at, or whether a buffer meant to loop is fit to.
    #[must_use]
    pub fn sound(&self, id: u32) -> Option<&Arc<[AudioSample]>> {
        self.sounds.get(&id)
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

    /// Create a new [`Voice`] over the stored sound data.
    ///
    /// **The buffer is shared, not copied.** A voice is a playhead and a set of
    /// mix parameters over the bank's samples, so a game raising cues at its
    /// frame rate does not allocate a sound per cue.
    ///
    /// Returns `None` if `id` is not registered, or if its data is too short to
    /// hold a single interleaved frame — there is no sound to play, and a
    /// looping voice over it would divide by zero.
    #[must_use]
    pub fn create_voice(&self, id: u32) -> Option<Voice> {
        self.sounds
            .get(&id)
            .filter(|data| data.len() >= CHANNELS)
            .map(|data| Voice::from_shared(Arc::clone(data)))
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
    fn fill(&self, buffer: &mut [AudioSample], sample_rate: u32) {
        self.lock()
            .retain_mut(|(_, voice)| voice.mix_block(buffer, sample_rate));
        // Releasing voices contribute their one fade-to-silence block and are
        // then dropped — `mix_block` answers `false` for them at the end.
        self.lock_releasing()
            .retain_mut(|(_, voice)| voice.mix_block(buffer, sample_rate));

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
        let releasing = self.lock_releasing().len();
        f.debug_struct("Mixer")
            .field("voice_count", &count)
            .field("releasing_count", &releasing)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::spatial::{CueGrammar, SpatialCue, compute_cue};

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

        assert!(voice.mix_block(&mut buf, 48_000));
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn voice_stops_at_end() {
        let data = vec![0.5f32; 16 * CHANNELS];
        let mut voice = Voice::new(data);
        let mut buf = vec![0.0f32; 16 * CHANNELS];

        assert!(voice.mix_block(&mut buf, 48_000));
        assert!(!voice.mix_block(&mut buf, 48_000));
    }

    #[test]
    fn voice_loops() {
        let data = vec![0.5f32; 16 * CHANNELS];
        let mut voice = Voice::new(data).with_looping();
        let mut buf = vec![0.0f32; 32 * CHANNELS];

        assert!(voice.mix_block(&mut buf, 48_000));
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn voice_volume_scales_output() {
        let data = vec![1.0f32; 64 * CHANNELS];
        let mixer = Mixer::new();
        mixer.play(Voice::new(data).with_volume(0.25));
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 0.25).abs() < 0.001);
        }
    }

    /// **The gap that made the whole mixer unusable.** A stream owns its source,
    /// so a game only ever holds a shared handle; a `play` that wanted `&mut`
    /// could not be called at all once the audio was running.
    ///
    /// The observable is the *output*, not the voice count: a `play` through the
    /// shared handle that pushed onto a different mixer — or onto a copy — would
    /// leave this buffer silent.
    #[test]
    fn a_shared_handle_both_plays_and_fills() {
        let mixer = Arc::new(Mixer::new());
        let source: &dyn AudioSource = &Arc::clone(&mixer);

        mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));

        let mut buf = vec![0.0f32; 32 * CHANNELS];
        source.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!(
                (s - 0.5).abs() < 1e-6,
                "the shared handle played nothing: {s}"
            );
        }
    }

    /// A handle stops the voice it was issued for — the accounting immediately,
    /// with no fill in between, because a mixer nothing is filling still has
    /// to answer — while the sound itself fades out over the next block
    /// rather than cutting.
    #[test]
    fn a_handle_stops_its_own_voice_and_leaves_the_others() {
        let mixer = Mixer::new();
        let first = mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));
        let second = mixer.play(Voice::new(vec![0.25f32; 64 * CHANNELS]));
        assert!(mixer.is_playing(first) && mixer.is_playing(second));

        assert!(mixer.stop(first), "the first voice was not playing");
        assert!(!mixer.is_playing(first));
        assert!(mixer.is_playing(second), "stop took the wrong voice");
        assert_eq!(mixer.voice_count(), 1);

        // The first fill still hears the stopped voice — it is fading, not
        // cut. 16 frames, so frame `i` keeps `(15 - i) / 15` of first's 0.5
        // on top of the second voice's constant 0.25: the first frame is
        // 0.75 (fade starts at 1.0, no step down) and the last is exactly
        // 0.25 (first's fade ends at silence).
        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(
            (buf[0] - 0.75).abs() < 1e-6,
            "the ramp must start at the voice's own level: {}",
            buf[0],
        );
        assert!(
            (buf[buf.len() - CHANNELS] - 0.25).abs() < 1e-6,
            "the ramp must end at silence for the stopped voice: {}",
            buf[buf.len() - CHANNELS],
        );
        for (i, &s) in buf.iter().enumerate() {
            assert!(
                (0.25 - 1e-6..=0.75 + 1e-6).contains(&s),
                "frame {}: expected 0.25..=0.75, got {s}",
                i / CHANNELS,
            );
        }

        // A second fill hears only the second voice: first's release block is
        // done and it has been dropped.
        buf.fill(0.0);
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!(
                (s - 0.25).abs() < 1e-6,
                "the stopped voice is still audible after its release block: {s}",
            );
        }

        assert!(!mixer.stop(first), "stopping twice reported a second kill");
    }

    /// A stopped voice is not cut: it plays one release block — a linear fade
    /// to silence — and is gone after it. The fade starts at the voice's own
    /// level (no step down) and ends exactly at silence.
    #[test]
    fn stop_starts_a_release_ramp_not_a_cut() {
        let mixer = Mixer::new();
        // Plenty of data: the pre-stop fill and the full 64-frame release
        // block must both fit, or the ramp would be cut short by the sound
        // ending rather than by the fade.
        let id = mixer.play(Voice::new(vec![0.5f32; 128 * CHANNELS]));

        let mut buf = vec![0.0f32; 8 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 0.5).abs() < 1e-6, "pre-stop level: {s}");
        }

        assert!(mixer.stop(id));
        assert!(
            !mixer.is_playing(id),
            "the accounting must drop on the spot"
        );
        assert_eq!(mixer.voice_count(), 0);

        // One release block of 64 frames: frame `i` keeps (63 - i) / 63 of
        // the voice's 0.5.
        let n = 64;
        buf = vec![0.0f32; n * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(
            (buf[0] - 0.5).abs() < 1e-6,
            "the ramp must start at the voice's own level, not step down: {}",
            buf[0],
        );
        assert!(
            buf[buf.len() - CHANNELS].abs() < 1e-6,
            "the ramp must end exactly at silence: {}",
            buf[buf.len() - CHANNELS],
        );
        for frame in buf.chunks_exact(CHANNELS) {
            assert!(
                (frame[0] - frame[1]).abs() < 1e-6,
                "the ramp must stay in its channels: {frame:?}",
            );
        }
        // Monotone down along the block: the ramp never rises. Both channels
        // carry the same DC, so a per-frame magnitude is a single number.
        let mut last = f32::INFINITY;
        for (i, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let magnitude = (frame[0] * frame[0] + frame[1] * frame[1]).sqrt();
            assert!(
                magnitude <= last + 1e-6,
                "frame {i} rose: magnitude {magnitude} > {last}",
            );
            last = magnitude;
        }

        // Gone after exactly one release block.
        buf.fill(0.0);
        mixer.fill(&mut buf, 48_000);
        assert!(
            buf.iter().all(|s| *s == 0.0),
            "the voice survived its release block",
        );
        assert_eq!(mixer.voice_count(), 0);
    }

    /// `stop` frees the cap slot and the countable list on the spot — with no
    /// fill in between — even though the sound itself keeps playing its
    /// release block. This pins the property the backlog demands; it passes
    /// on the old drop-now code too.
    #[test]
    fn stop_frees_the_slot_immediately_without_a_fill() {
        let mixer = Mixer::new();
        let first = mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));
        let second = mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));
        assert_eq!(mixer.voice_count(), 2);

        assert!(mixer.stop(first));
        assert!(!mixer.is_playing(first), "the stopped voice still counts");
        assert_eq!(mixer.voice_count(), 1, "the cap slot must free on the spot");
        assert!(mixer.is_playing(second), "stop took the wrong voice");
    }

    /// Ids are never reused, so a handle held past the end of its sound is inert
    /// rather than aimed at whatever started next.
    #[test]
    fn a_stale_handle_never_steers_a_later_voice() {
        let mixer = Mixer::new();
        let spent = mixer.play(Voice::new(vec![0.5f32; 4 * CHANNELS]));
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(!mixer.is_playing(spent), "the voice did not run out");

        let live = mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));
        assert_ne!(spent, live, "an id was reused");
        assert!(!mixer.stop(spent), "a stale handle stopped a later voice");
        assert!(
            !mixer.set_mix(spent, VoiceMix::UNITY),
            "a stale handle re-aimed a later voice",
        );
        assert!(mixer.is_playing(live), "the later voice was collateral");
    }

    /// **`set_mix` reaches a voice that is already sounding**, which is what a
    /// held sound on a moving emitter needs.
    ///
    /// Measured on the output either side of the call: hard left, then hard
    /// right, from one voice that never restarts. A `set_mix` that silently
    /// missed would leave the second block panned left like the first.
    #[test]
    fn set_mix_re_aims_a_voice_that_is_already_playing() {
        let mixer = Mixer::new();
        let id = mixer.play(
            Voice::new(vec![0.5f32; 4096 * CHANNELS]).with_mix(VoiceMix {
                volume: 1.0,
                gains: (1.0, 0.0),
                pitch: 1.0,
                itd_samples: 0.0,
            }),
        );

        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        for frame in buf.chunks_exact(CHANNELS) {
            assert!((frame[0] - 0.5).abs() < 1e-6, "left silent: {frame:?}");
            assert!(frame[1].abs() < 1e-6, "right should be muted: {frame:?}");
        }

        assert!(mixer.set_mix(
            id,
            VoiceMix {
                volume: 1.0,
                gains: (0.0, 1.0),
                pitch: 1.0,
                itd_samples: 0.0,
            },
        ));

        buf.fill(0.0);
        mixer.fill(&mut buf, 48_000);
        for frame in buf.chunks_exact(CHANNELS) {
            assert!(frame[0].abs() < 1e-6, "left should be muted now: {frame:?}");
            assert!((frame[1] - 0.5).abs() < 1e-6, "right silent: {frame:?}");
        }
        assert_eq!(
            mixer.voice_mixes(),
            vec![(
                id,
                VoiceMix {
                    volume: 1.0,
                    gains: (0.0, 1.0),
                    pitch: 1.0,
                    itd_samples: 0.0,
                }
            )],
        );
    }

    /// Non-finite parameters are clamped on the way *in through a live voice*
    /// too, not only when the voice is built.
    #[test]
    fn set_mix_clamps_what_it_is_given() {
        let mixer = Mixer::new();
        let id = mixer.play(Voice::new(vec![0.5f32; 64 * CHANNELS]));
        assert!(mixer.set_mix(
            id,
            VoiceMix {
                volume: f32::NAN,
                gains: (2.0, f32::INFINITY),
                pitch: -1.0,
                itd_samples: f32::NAN,
            },
        ));

        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(buf.iter().all(|s| s.is_finite()), "{buf:?}");
        assert_eq!(
            mixer.voice_mixes()[0].1,
            VoiceMix {
                volume: 0.0,
                gains: (1.0, 1.0),
                pitch: 0.25,
                itd_samples: 0.0,
            },
        );
    }

    /// Interleaved ramp data whose channels are far apart — left holds `n`,
    /// right holds `1000 + n` — so a delay that crossed channels would be
    /// unmissable.
    fn ramp_channels(frames: usize) -> Vec<AudioSample> {
        let mut out = Vec::with_capacity(frames * CHANNELS);
        for n in 0..frames {
            out.push(n as f32);
            out.push(1000.0 + n as f32);
        }
        out
    }

    /// The [`From`] impl carries the cue's ITD instead of dropping it: an
    /// emitter off to one side mixes at its own delay, and anything
    /// reference-derived mixes with none.
    #[test]
    fn from_cue_carries_the_itd() {
        let grammar = CueGrammar::default();
        let right = compute_cue([0.0, 0.0, 0.0], [5.0, 0.0, 0.0], &grammar);
        assert!(right.itd_samples < 0.0, "right emitter delays the left ear");
        assert_eq!(VoiceMix::from(&right).itd_samples, right.itd_samples);

        let ahead = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, 0.5], &grammar);
        assert_eq!(VoiceMix::from(&ahead).itd_samples, 0.0);
        assert_eq!(VoiceMix::from(&SpatialCue::REFERENCE).itd_samples, 0.0);
    }

    /// A positive ITD delays the right channel by exactly that many frames,
    /// scaled by the mix's volume. The line starts silent, so the first two
    /// frames read zero; frame `k` reads data frame `k - 2`. The left channel
    /// is untouched. The old code read `self.data[base + ch]` directly and
    /// answered `1000 + k` in the right channel, so this fails on it.
    #[test]
    fn a_positive_itd_delays_the_right_channel() {
        let mut voice = Voice::new(ramp_channels(64)).with_mix(VoiceMix {
            volume: 0.5,
            itd_samples: 2.0,
            ..VoiceMix::UNITY
        });
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        assert!(voice.mix_block(&mut buf, 48_000));

        for (k, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let expected_right = if k >= 2 {
                (1000.0 + (k - 2) as f32) * 0.5
            } else {
                0.0
            };
            assert!(
                (frame[0] - k as f32 * 0.5).abs() < 1e-6,
                "frame {k}: left was delayed: {}",
                frame[0],
            );
            assert!(
                (frame[1] - expected_right).abs() < 1e-6,
                "frame {k}: right expected {expected_right}, got {}",
                frame[1],
            );
        }
    }

    /// The sign convention pinned the other way: a negative ITD delays the
    /// left channel, and the right is untouched.
    #[test]
    fn a_negative_itd_delays_the_left_channel() {
        let mut voice = Voice::new(ramp_channels(64)).with_mix(VoiceMix {
            volume: 0.5,
            itd_samples: -2.0,
            ..VoiceMix::UNITY
        });
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        assert!(voice.mix_block(&mut buf, 48_000));

        for (k, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let expected_left = if k >= 2 { (k - 2) as f32 * 0.5 } else { 0.0 };
            assert!(
                (frame[0] - expected_left).abs() < 1e-6,
                "frame {k}: left expected {expected_left}, got {}",
                frame[0],
            );
            assert!(
                (frame[1] - (1000.0 + k as f32) * 0.5).abs() < 1e-6,
                "frame {k}: right was delayed: {}",
                frame[1],
            );
        }
    }

    /// A fractional ITD interpolates between the two taps: frame `k` reads
    /// `0.5·x[k] + 0.5·x[k−1]`, where `x` is the delayed channel's ramp and
    /// the missing `x[−1]` is the silent start of the line.
    #[test]
    fn a_fractional_itd_interpolates_between_taps() {
        let mut voice = Voice::new(ramp_channels(64)).with_mix(VoiceMix {
            itd_samples: 0.5,
            ..VoiceMix::UNITY
        });
        let mut buf = vec![0.0f32; 32 * CHANNELS];
        assert!(voice.mix_block(&mut buf, 48_000));

        for (k, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let prev = if k >= 1 { 1000.0 + (k - 1) as f32 } else { 0.0 };
            let expected_right = 0.5 * (1000.0 + k as f32) + 0.5 * prev;
            assert!(
                (frame[1] - expected_right).abs() < 1e-6,
                "frame {k}: right expected {expected_right}, got {}",
                frame[1],
            );
            assert!(
                (frame[0] - k as f32).abs() < 1e-6,
                "frame {k}: left was delayed: {}",
                frame[0],
            );
        }
    }

    /// The clamp bounds the delay at `±MAX_ITD_DELAY`, which is what keeps
    /// both fractional taps of any admitted delay inside the line. The getter
    /// reports the clamped value.
    #[test]
    fn itd_is_clamped_to_the_delay_line_capacity() {
        let voice = Voice::new(vec![0.0f32; 8 * CHANNELS]).with_mix(VoiceMix {
            itd_samples: 1e9,
            ..VoiceMix::UNITY
        });
        assert_eq!(voice.mix().itd_samples, MAX_ITD_DELAY);

        let voice = Voice::new(vec![0.0f32; 8 * CHANNELS]).with_mix(VoiceMix {
            itd_samples: -1e9,
            ..VoiceMix::UNITY
        });
        assert_eq!(voice.mix().itd_samples, -MAX_ITD_DELAY);
    }

    /// **A looping voice outlives its own sample data**, which is the whole
    /// point of one: it is what a held cue is made of.
    ///
    /// Sixteen blocks of a four-frame sound. A one-shot is gone after the first
    /// and every later block is silence, so both halves of this fail loudly.
    #[test]
    fn a_looping_voice_keeps_sounding_past_the_end_of_its_sound() {
        let mixer = Mixer::new();
        let id = mixer.play(Voice::new(vec![0.5f32; 4 * CHANNELS]).with_looping());

        let mut buf = vec![0.0f32; 64 * CHANNELS];
        for block in 0..16 {
            buf.fill(0.0);
            mixer.fill(&mut buf, 48_000);
            assert!(mixer.is_playing(id), "the loop ended at block {block}");
            for &s in &buf {
                assert!((s - 0.5).abs() < 1e-6, "silence at block {block}: {s}");
            }
        }

        // …and it ends when, and only when, it is told to. Stopping starts a
        // one-block release ramp rather than a cut: the fill after `stop`
        // fades from the loop's level to silence, and the voice is gone
        // afterwards.
        assert!(mixer.stop(id));
        buf.fill(0.0);
        mixer.fill(&mut buf, 48_000);
        assert!(
            (buf[0] - 0.5).abs() < 1e-6,
            "the release ramp must start at the loop's level: {}",
            buf[0],
        );
        assert!(
            buf[buf.len() - CHANNELS].abs() < 1e-6,
            "the release ramp must end at silence: {}",
            buf[buf.len() - CHANNELS],
        );
        assert!(!mixer.is_playing(id), "the loop survived stop");
        buf.fill(0.0);
        mixer.fill(&mut buf, 48_000);
        assert!(
            buf.iter().all(|s| *s == 0.0),
            "the loop survived its release block",
        );
    }

    /// A bank hands out voices over one buffer rather than a copy each.
    ///
    /// The strong count is the observable: this test is here because
    /// `create_voice` used to clone the samples, which at a game's cue rate is
    /// an allocation the size of the sound per cue.
    #[test]
    fn a_bank_shares_one_buffer_with_every_voice_it_makes() {
        let mut bank = SoundBank::new();
        bank.insert(1, vec![0.5f32; 64 * CHANNELS]);
        let stored = Arc::clone(bank.sounds.get(&1).expect("just inserted"));
        assert_eq!(Arc::strong_count(&stored), 2, "the bank plus this handle");

        let voices: Vec<Voice> = (0..8).map(|_| bank.create_voice(1).unwrap()).collect();
        assert_eq!(
            Arc::strong_count(&stored),
            10,
            "a voice copied the buffer instead of sharing it",
        );
        for voice in &voices {
            assert!(
                Arc::ptr_eq(&voice.data, &stored),
                "a voice is playing some other buffer",
            );
        }
    }

    #[test]
    fn mixer_removes_finished_voices() {
        let data = vec![0.5f32; 8 * CHANNELS];
        let mixer = Mixer::new();
        mixer.play(Voice::new(data));
        let mut buf = vec![0.0f32; 16 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert_eq!(mixer.voice_count(), 0);
    }

    #[test]
    fn two_voices_sum() {
        let mixer = Mixer::new();
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
        let mixer = Mixer::new();
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
        assert!(voice.mix_block(&mut buf, 48_000));

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
        assert!(voice.mix_block(&mut buf, 48_000));

        // Output frame i must be input frame 2i.
        for (i, frame) in buf.chunks_exact(CHANNELS).enumerate() {
            let expected = (2 * i) as f32 / frames as f32;
            assert!((frame[0] - expected).abs() < 1e-6, "frame {i}: {frame:?}");
        }
    }

    /// Voices are authored for [`INTERNAL_SAMPLE_RATE`]; a device at some
    /// other rate must not step one source frame per output frame, or a 0.1 s
    /// sound takes 4800/44100 s on 44.1 kHz hardware.
    #[test]
    fn voices_run_at_the_internal_rate_regardless_of_the_device_rate() {
        let mixer = Mixer::new();
        // A 0.1-second sound at the internal rate: 4800 frames.
        mixer.play(Voice::new(vec![0.5f32; 4800 * CHANNELS]));
        let mut buf = vec![0.0f32; 100 * CHANNELS];
        let mut fills = 0;
        while mixer.voice_count() > 0 {
            buf.fill(0.0);
            mixer.fill(&mut buf, 44_100);
            fills += 1;
        }
        assert_eq!(
            fills, 45,
            "a 0.1 s sound ends after 0.1 s at 44.1 kHz (4410 output frames = 44.1 blocks), not after 48 fills"
        );

        // The same sound at the internal rate takes 49 fills: 48 blocks emit
        // the 4800 frames, then the end-check fires on the first frame of the
        // 49th. The two rates agree in wall time — 45 × 100 / 44100 ≈
        // 49 × 100 / 48000 ≈ 0.102 s of output either way.
        let mixer = Mixer::new();
        mixer.play(Voice::new(vec![0.5f32; 4800 * CHANNELS]));
        let mut fills = 0;
        while mixer.voice_count() > 0 {
            buf.fill(0.0);
            mixer.fill(&mut buf, 48_000);
            fills += 1;
        }
        assert_eq!(
            fills, 49,
            "same 0.1 s at 48 kHz: 48 full blocks plus the reap block"
        );
    }

    #[test]
    fn looping_voice_over_empty_data_does_not_panic() {
        // `pos % 0` used to panic inside the audio callback.
        let mut voice = Voice::new(Vec::new()).with_looping();
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        assert!(!voice.mix_block(&mut buf, 48_000));
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
        let mixer = Mixer::new();
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
        let mixer = Mixer::new();
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
        let mixer = Mixer::new();
        mixer.play(Voice::new(vec![f32::NAN; 8 * CHANNELS]));
        let mut buf = vec![0.0f32; 8 * CHANNELS];
        mixer.fill(&mut buf, 48_000);
        assert!(buf.iter().all(|s| *s == 0.0), "{buf:?}");
    }

    #[test]
    fn mixer_is_sync_and_fill_is_serialised() {
        use std::sync::Arc;

        let mixer = Mixer::new();
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

    /// The game thread races the audio thread. In a real game the game thread
    /// calls `play`/`stop`/`set_mix` while the audio callback runs `fill`; this
    /// test drives exactly that shape — a filler looping on one thread while
    /// the other plays, re-aims and stops — and asserts the two halves agree
    /// when it is over. The `Mutex` is what makes it safe by construction; this
    /// is the test that says so.
    #[test]
    fn play_stop_and_set_mix_race_a_live_fill() {
        use std::sync::{Arc, Barrier};

        let mixer = Arc::new(Mixer::new());
        let start = Arc::new(Barrier::new(2));

        let filler = {
            let mixer: Arc<Mixer> = Arc::clone(&mixer);
            let start: Arc<Barrier> = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                let mut buf = vec![0.0f32; 256 * CHANNELS];
                for _ in 0..200 {
                    mixer.fill(&mut buf, 48_000);
                }
            })
        };

        // Played before the race and never stopped: must still be sounding.
        let keepers: Vec<VoiceId> = (0..8)
            .map(|_| mixer.play(Voice::new(vec![0.1f32; 4096 * CHANNELS]).with_looping()))
            .collect();
        // Played before the race and stopped during it: must be gone.
        let doomed: Vec<VoiceId> = (0..8)
            .map(|_| mixer.play(Voice::new(vec![0.1f32; 4096 * CHANNELS]).with_looping()))
            .collect();

        start.wait();
        for _ in 0..50 {
            for id in &keepers {
                mixer.set_mix(*id, VoiceMix::default());
            }
            // `stop` may answer false — a previous turn of this loop already
            // took the voice — but it must never leave one behind.
            for id in &doomed {
                mixer.set_mix(*id, VoiceMix::default());
                mixer.stop(*id);
            }
            mixer.play(Voice::new(vec![0.1f32; 4096 * CHANNELS]).with_looping());
        }
        filler.join().unwrap();

        for id in keepers {
            assert!(mixer.is_playing(id), "a keeper was lost to the fill thread");
        }
        for id in doomed {
            assert!(!mixer.is_playing(id), "a stopped voice came back");
        }
    }
}
