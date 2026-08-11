//! Audio for breakout: procedural sound generation through the engine's mixer.
//!
//! Generates sine-wave beeps for bounce and brick-break sounds, banks them in a
//! [`SoundBank`] and plays them through a shared [`Mixer`] that is also the
//! stream's source. The game thread calls [`Audio::play_at`]; the audio
//! thread fills from the same mixer.
//!
//! # Where the listener stands
//!
//! At the screen centre, [`LISTENER_STANDOFF`] behind the play plane, which is
//! where the camera is. It never moves, so it is set on the [`Mixer`] once in
//! [`Audio::new`] and nothing in the frame touches it again: the whole listener
//! convention of this sample is that one line. Only the emitter's X moves, so
//! the grammar answers with a pan and a distance and nothing else varies.
//!
//! # What this file used to be
//!
//! Its own `Sound`, `Voice`, `VoiceQueue` and `MixerSource`, copied into all
//! four samples, because `crcbl-audio`'s [`Mixer::play`] wanted `&mut self`
//! while `AudioStream::open` consumed its source — nothing could hold both ends.
//! [`Mixer::play`] takes `&self` now and the stream takes an `Arc`, so the
//! playhead, the queue and the mixing loop are the engine's.

use std::sync::Arc;

use crcbl::audio::AudioStream;
use crcbl::audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl::audio::spatial::{CueGrammar, Listener};
use crcbl::audio::synth;
use crcbl::math::DVec3;

pub const SOUND_BOUNCE: u32 = 1;
pub const SOUND_BRICK: u32 = 2;

/// How loud a cue is against the volume the grammar asks for.
///
/// Halved: the grammar's `volume` is a distance rolloff that reaches 1.0 on top
/// of the listener, and several cues overlapping at full scale clip.
const MASTER_GAIN: f32 = 0.5;

/// How far behind the play plane the listener stands, in world units.
///
/// The game happens at `z = 0`, so a listener in that plane would hear a cue
/// raised on top of it as a zero-length direction — no side, no distance, and
/// the grammar's co-located fast path instead of a pan. Standing back one unit
/// gives every emitter a defined direction.
///
/// **It belongs to the listener**, which is why it is here and not added to
/// each emitter's Z at the call site: it is the camera's standoff from the
/// field, and there is one camera and many cues.
const LISTENER_STANDOFF: f32 = 1.0;

/// The camera, at the screen centre. See the module docs.
const LISTENER: Listener = Listener::new([0.0, 0.0, -LISTENER_STANDOFF]);

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
        bank.insert(SOUND_BOUNCE, synth::sine(440.0, 0.06, 48000));
        bank.insert(SOUND_BRICK, synth::sine(660.0, 0.09, 48000));

        // The mixer is the stream's source *and* the game's handle: the stream
        // moves its copy onto the audio thread and this one stays here.
        let mixer = Arc::new(Mixer::new());
        // This camera does not move, so the listener is placed once here rather
        // than pushed every frame. Set before any cue can be raised, so no cue
        // is ever computed against the mixer's default.
        mixer.set_listener(LISTENER);
        let stream: Option<AudioStream> = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };

        if stream.is_none() && !headless {
            crcbl::log::info!("audio: no output device available; sounds will be silent");
        }

        Self {
            bank,
            mixer,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `at` in world space.
    ///
    /// No listener argument: the [`Mixer`] holds it, and this sample's is fixed
    /// at the camera — see [`LISTENER`] and the module docs.
    ///
    /// The Y of `at` is always zero in practice, because the simulation's cue
    /// queue carries only the emitter's X. That is a limit of what breakout
    /// raises, not of what this takes.
    pub fn play_at(&mut self, id: u32, at: DVec3) {
        // An id the bank does not know is simply absent — no `id - 1` index to
        // underflow, which is what this used to have to guard.
        let Some(voice) = self.bank.create_voice(id) else {
            crcbl::log::debug!("audio: no sound registered at id {id}");
            return;
        };
        let cue = self.mixer.cue(
            [at.x as f32, at.y as f32, at.z as f32],
            &CueGrammar::default(),
        );
        self.mixer.play(voice.with_mix(VoiceMix {
            volume: cue.volume * MASTER_GAIN,
            ..VoiceMix::from(&cue)
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cue at `x`, on the play plane — what the game's queue amounts to.
    fn at(x: f64) -> DVec3 {
        DVec3::new(x, 0.0, 0.0)
    }

    /// An id nothing answers to is ignored rather than playing something else.
    #[test]
    fn an_unknown_cue_is_ignored() {
        let mut audio = Audio::new(true);
        audio.play_at(0, DVec3::ZERO);
        audio.play_at(9999, DVec3::ZERO);
        assert_eq!(audio.mixer.voice_count(), 0);
        audio.play_at(SOUND_BOUNCE, DVec3::ZERO);
        assert_eq!(audio.mixer.voice_count(), 1);
    }

    /// **The listener is placed before anything can be played through it**, so
    /// a cue raised on the first frame is heard from the camera and not from
    /// the mixer's default at the origin — which sits *in* the play plane, and
    /// so answers a cue on top of it with no direction at all.
    #[test]
    fn the_camera_is_the_listener_from_the_first_cue() {
        let audio = Audio::new(true);
        assert_eq!(audio.mixer.listener(), LISTENER);
        assert_eq!(
            audio.mixer.listener().position,
            [0.0, 0.0, -LISTENER_STANDOFF],
        );
    }

    /// The grammar is actually consulted: a brick broken to the left is louder
    /// on the left, and one further out is quieter.
    ///
    /// Without this, `play_at` could ignore its position entirely and the
    /// sample would ship "spatial audio" that was a constant. Read off the
    /// mixer's own snapshot, so it is what was *queued*, not what a helper
    /// returned.
    #[test]
    fn where_a_cue_happens_changes_how_it_sounds() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_BRICK, at(0.0));
        audio.play_at(SOUND_BRICK, at(-6.0));
        audio.play_at(SOUND_BRICK, at(6.0));
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
}
