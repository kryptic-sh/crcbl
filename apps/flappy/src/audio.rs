//! Audio for flappy: two procedural cues through `crcbl-audio`'s spatial
//! grammar and its mixer.
//!
//! A flap and a death, both synthesised at start-up — this sample has no assets
//! by design. The waveforms are banked in a [`SoundBank`]; the game thread plays
//! voices into a [`Mixer`] the audio thread fills from.
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
//! [`compute_cue`]: crcbl::audio::spatial::compute_cue
//!
//! # What this file used to be
//!
//! A hand-written `Sound`, `Voice`, `VoiceQueue` and `MixerSource`, the same
//! four in all four samples, because `crcbl-audio`'s `Mixer::play` wanted
//! `&mut self` while `AudioStream::open` consumed its source. The engine holds
//! the playhead and the queue now; what is still local is the *sound design* —
//! the waveforms, the cue ids and the listener convention above.

use std::sync::Arc;

use crcbl::audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl::audio::spatial::{CueGrammar, compute_cue};
use crcbl::audio::{AudioSample, AudioStream};

/// The wing-beat.
pub const SOUND_FLAP: u32 = 1;
/// The end of a run.
pub const SOUND_DEATH: u32 = 2;

/// How loud a cue is against the volume the grammar asks for. See breakout's.
const MASTER_GAIN: f32 = 0.5;

/// Owns the cues and the output stream.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    /// How many times each cue has been **emitted**, indexed as `id - 1`.
    ///
    /// Only ever increases, and only from the game thread. [`Audio::voices`]
    /// cannot answer "was this cue played?" — it counts the voices still
    /// sounding, and the audio thread reaps each one as it finishes, so the
    /// number falls again on a clock nothing here controls.
    plays: Vec<u64>,
    mixer: Arc<Mixer>,
    _stream: Option<AudioStream>,
}

impl Audio {
    pub fn new(headless: bool) -> Self {
        // A short high chirp for the flap, a longer low one for the end. Both
        // are short enough that a player flapping four times a second never
        // hears two of the same overlap.
        let mut bank = SoundBank::new();
        bank.insert(SOUND_FLAP, sine(760.0, 0.05, 48_000));
        bank.insert(SOUND_DEATH, sine(180.0, 0.30, 48_000));

        // The stream takes a handle, not the mixer: this copy is what stays
        // behind to play voices through.
        let mixer = Arc::new(Mixer::new());
        let stream = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };
        if stream.is_none() && !headless {
            crcbl::log::info!("audio: no output device available; the game will be silent");
        }

        Self {
            bank,
            plays: vec![0; 2],
            mixer,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `(x, y)` in world space, heard
    /// from a listener at `listener_x` — see the module docs for why that is the
    /// camera's centre and not the bird.
    pub fn play_at(&mut self, id: u32, listener_x: f64, x: f64, y: f64) {
        // An id the bank does not know is simply absent, so there is no `id - 1`
        // to underflow on the lookup — only on the counter below, which is
        // reached solely for an id the bank *did* answer to.
        let Some(voice) = self.bank.create_voice(id) else {
            crcbl::log::debug!("audio: no sound registered at id {id}");
            return;
        };
        let cue = compute_cue(
            [0.0, 0.0, 0.0],
            [
                (x - listener_x) as f32,
                y as f32,
                // A metre in front, so a sound at the listener's own position is
                // still at a defined direction rather than a zero vector.
                1.0,
            ],
            &CueGrammar::default(),
        );
        self.mixer.play(voice.with_mix(VoiceMix {
            volume: cue.volume * MASTER_GAIN,
            ..VoiceMix::from(&cue)
        }));
        if let Some(count) = id
            .checked_sub(1)
            .and_then(|i| self.plays.get_mut(i as usize))
        {
            *count += 1;
        }
    }

    /// How many voices are **currently sounding**. For a debug HUD.
    ///
    /// Not a record of what was played: the audio thread drops each voice as it
    /// runs out, so this falls again on its own. Use [`Audio::plays`] to ask
    /// whether a cue happened.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.mixer.voice_count()
    }

    /// How many times cue `id` has been played since start-up.
    ///
    /// Monotonic, so it answers the question [`Audio::voices`] cannot: whether
    /// a cue was ever emitted, however long ago it finished. An id no sound
    /// answers to has never been played and reports zero.
    #[must_use]
    pub fn plays(&self, id: u32) -> u64 {
        id.checked_sub(1)
            .and_then(|i| self.plays.get(i as usize))
            .copied()
            .unwrap_or(0)
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

    /// The generator produces **interleaved stereo**, which is what the mixer's
    /// playhead assumes: a mono buffer would be played at half speed over twice
    /// the length. Breakout shipped that bug once, in its own playhead.
    #[test]
    fn a_cue_is_interleaved_stereo_of_the_length_it_asked_for() {
        let frames = 100;
        let data = sine(440.0, frames as f32 / 48_000.0, 48_000);
        assert_eq!(data.len(), frames * 2, "not stereo pairs");
        for frame in data.chunks_exact(2) {
            assert_eq!(frame[0], frame[1], "not the same in both ears");
        }
        assert!(data.iter().any(|s| s.abs() > 1e-3), "the cue is silent");
    }

    /// An id nothing answers to is ignored rather than underflowing or panicking.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_at(0, 0.0, 0.0, 0.0);
        audio.play_at(9999, 0.0, 0.0, 0.0);
        assert_eq!(audio.voices(), 0);
        // `plays` still spells `id - 1`, so it still has the underflow to avoid,
        // and it must not report a play for a cue that was refused.
        assert_eq!(audio.plays(0), 0);
        assert_eq!(audio.plays(9999), 0);
        assert_eq!(audio.plays(SOUND_FLAP), 0);

        audio.play_at(SOUND_FLAP, 0.0, 0.0, 0.0);
        assert_eq!(audio.voices(), 1);
        assert_eq!(audio.plays(SOUND_FLAP), 1);
        assert_eq!(audio.plays(SOUND_DEATH), 0, "only the flap was played");
    }

    /// `plays` counts emissions, not the voices still sounding — the whole
    /// reason it exists. One that merely reported `voices()` would agree with
    /// this test right up until the audio thread reaped the voice.
    #[test]
    fn a_cue_stays_counted_after_its_voice_is_gone() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_FLAP, 0.0, 0.0, 0.0);
        assert_eq!(audio.plays(SOUND_FLAP), 1);

        // Reap it by hand rather than waiting on the audio thread, so the test
        // is not itself a race: `fill` is exactly what that thread calls.
        let mut block = vec![0.0f32; 256 * 2];
        let start = std::time::Instant::now();
        while audio.voices() > 0 {
            assert!(
                start.elapsed().as_secs() < 5,
                "the flap voice never finished"
            );
            block.fill(0.0);
            crcbl::audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        }
        assert_eq!(
            audio.plays(SOUND_FLAP),
            1,
            "the flap stopped being counted once it stopped sounding"
        );
    }

    /// The grammar is actually consulted: a cue away from the listener is not
    /// the same cue as one on top of it.
    ///
    /// Without this, `play_at` could ignore its coordinates entirely and every
    /// test above would still pass — the sample would ship "spatial audio" that
    /// was a constant. Read off the mixer, so it is what was actually queued.
    #[test]
    fn where_a_cue_happens_changes_how_it_sounds() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_FLAP, 0.0, 0.0, 0.0);
        audio.play_at(SOUND_FLAP, 0.0, -8.0, 5.0);
        let mixes = audio.mixer.voice_mixes();
        assert_eq!(mixes.len(), 2, "a cue went missing");
        let (near, far) = (mixes[0].1, mixes[1].1);
        assert!(
            far.gains.0 > far.gains.1,
            "a cue to the left should be louder on the left: {:?}",
            far.gains,
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
