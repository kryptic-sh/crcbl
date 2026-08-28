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
//! The camera scrolls with the bird, so this is the sample whose listener
//! genuinely moves — on one axis. [`Audio::set_listener`] is that whole
//! convention, called once a frame from `crate::game` before the frame's cues
//! are drained; [`Audio::play_at`] takes a world position and nothing else.
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

use crcbl::audio::AudioStream;
use crcbl::audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl::audio::spatial::{CueGrammar, Listener};
use crcbl::audio::synth;
use crcbl::math::DVec3;

/// The wing-beat.
pub const SOUND_FLAP: u32 = 1;
/// The end of a run.
pub const SOUND_DEATH: u32 = 2;

/// How loud a cue is against the volume the grammar asks for. See breakout's.
const MASTER_GAIN: f32 = 0.5;

/// How far behind the play plane the listener stands. See breakout's.
const LISTENER_STANDOFF: f32 = 1.0;

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

/// The settings directory this sample reads its volumes out of.
///
/// The same spelling `crate::gpu` hands
/// [`GpuContextDesc::label`](crcbl::engine::GpuContextDesc::label), because it
/// is the same directory: a player has one settings file per game, and two
/// spellings of the name would be a video section and an audio section in
/// different files.
const APP_NAME: &str = "flappy";

impl Audio {
    pub fn new(headless: bool) -> Self {
        // A short high chirp for the flap, a longer low one for the end. Both
        // are short enough that a player flapping four times a second never
        // hears two of the same overlap.
        let mut bank = SoundBank::new();
        bank.insert(SOUND_FLAP, synth::sine(760.0, 0.05, 48_000));
        bank.insert(SOUND_DEATH, synth::sine(180.0, 0.30, 48_000));

        // The stream takes a handle, not the mixer: this copy is what stays
        // behind to play voices through.
        let mixer = Arc::new(Mixer::new());
        // Before the first cue: a voice started against the default gains is
        // computed once and keeps them, so it would be the one sound in the run
        // the player's settings did not reach.
        crcbl::engine::SettingsSource::for_run(headless).apply_audio_gains(APP_NAME, &mixer);
        let stream = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };
        if stream.is_none() && !headless {
            crcbl::log::info!("audio: no output device available; the game will be silent");
        }

        let audio = Self {
            bank,
            plays: vec![0; 2],
            mixer,
            _stream: stream,
        };
        // The camera starts at the origin and the game moves it every frame
        // after; placing it here is what stops a cue raised before the first
        // `set_listener` being computed against the mixer's default, which sits
        // *in* the play plane rather than back from it.
        audio.set_listener(0.0);
        audio
    }

    /// Puts the listener at `x`, the camera's centre — the one axis this
    /// sample's listener moves on. See the module docs.
    ///
    /// Called once a frame, before the frame's cues are played.
    pub fn set_listener(&self, x: f64) {
        self.mixer
            .set_listener(Listener::new([x as f32, 0.0, -LISTENER_STANDOFF]));
    }

    /// Plays a cue for something happening at `at` in world space.
    ///
    /// No listener argument: [`Audio::set_listener`] said where the ear is, and
    /// the [`Mixer`] has held it since.
    pub fn play_at(&mut self, id: u32, at: DVec3) {
        // An id the bank does not know is simply absent, so there is no `id - 1`
        // to underflow on the lookup — only on the counter below, which is
        // reached solely for an id the bank *did* answer to.
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

/// The panel's audio section: the two cues, and what is sounding now.
///
/// **This is what [`Audio::plays`] was for.** It has been counted since the
/// counter landed and nothing read it outside this file's tests — a monotonic
/// record of every cue emitted, which is the only thing that can answer "the
/// death cue did not play" after the voice has been reaped. `ROADMAP.md`'s
/// standing requirement 1 names it by name as flappy's contribution to the
/// modular panel.
///
/// [`Audio::voices`] sits beside it because the two disagree in the case that
/// matters: a cue that was emitted and is already silent reads as one play and
/// no voice, which is a working game, while a play count that never moves is a
/// game whose cues are not reaching the mixer at all.
///
/// Deliberately **not** shared with the other samples' audio modules. Horde's
/// reports one row, `dropped`, because it is the only sample with a voice cap
/// to refuse anything; asteroids' reports three cues and whether the held engine
/// loop is running; breakout's `Audio` keeps no counter and so contributes no
/// section. Four samples, four different facts — the resemblance is the shape of
/// a `label: value` row and not knowledge that would change in one place.
impl crcbl::ui::DebugModule for Audio {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("audio");
        section.row("flaps", format_args!("{}", self.plays(SOUND_FLAP)));
        section.row("deaths", format_args!("{}", self.plays(SOUND_DEATH)));
        section.row("voices", format_args!("{}", self.voices()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An id nothing answers to is ignored rather than underflowing or panicking.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_at(0, DVec3::ZERO);
        audio.play_at(9999, DVec3::ZERO);
        assert_eq!(audio.voices(), 0);
        // `plays` still spells `id - 1`, so it still has the underflow to avoid,
        // and it must not report a play for a cue that was refused.
        assert_eq!(audio.plays(0), 0);
        assert_eq!(audio.plays(9999), 0);
        assert_eq!(audio.plays(SOUND_FLAP), 0);

        audio.play_at(SOUND_FLAP, DVec3::ZERO);
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
        audio.play_at(SOUND_FLAP, DVec3::ZERO);
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

    /// **The audio section reports what [`Audio::plays`] counts**, per cue and
    /// not in total: a section that summed them would read the same for a run
    /// that flapped twice and one that flapped once and died.
    #[test]
    fn the_audio_section_counts_each_cue_separately() {
        use crcbl::ui::DebugModule as _;

        let mut audio = Audio::new(true);
        let mut section = crcbl::ui::DebugSection::new("audio");
        audio.debug_section(&mut section);
        assert_eq!(section.title(), "audio");
        assert_eq!(
            section.rows(),
            &[row("flaps", "0"), row("deaths", "0"), row("voices", "0")],
            "a fresh mixer has played nothing and is sounding nothing",
        );

        audio.play_at(SOUND_FLAP, DVec3::ZERO);
        audio.play_at(SOUND_FLAP, DVec3::ZERO);
        audio.play_at(SOUND_DEATH, DVec3::ZERO);
        section.clear();
        audio.debug_section(&mut section);
        assert_eq!(
            section.rows(),
            &[row("flaps", "2"), row("deaths", "1"), row("voices", "3")],
            "two flaps and a death, all three still sounding",
        );

        // **The play counts survive the voices.** Reaped by hand rather than by
        // waiting on the audio thread, exactly as the test above does it: this
        // is the difference the `plays` counter exists for, and a section built
        // on `voices()` alone would read zeroes here.
        let mut block = vec![0.0f32; 256 * 2];
        let start = std::time::Instant::now();
        while audio.voices() > 0 {
            assert!(start.elapsed().as_secs() < 5, "a voice never finished");
            block.fill(0.0);
            crcbl::audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        }
        section.clear();
        audio.debug_section(&mut section);
        assert_eq!(
            section.rows(),
            &[row("flaps", "2"), row("deaths", "1"), row("voices", "0")],
            "the cues stopped being counted once they stopped sounding",
        );
    }

    /// One expected row, spelled once.
    fn row(label: &str, value: &str) -> crcbl::ui::DebugRow {
        crcbl::ui::DebugRow {
            label: label.into(),
            value: value.into(),
        }
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
        audio.play_at(SOUND_FLAP, DVec3::ZERO);
        audio.play_at(SOUND_FLAP, DVec3::new(-8.0, 5.0, 0.0));
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

    /// **The listener is placed before anything can be played through it.** The
    /// game pushes it every frame, but a cue raised before the first frame must
    /// not be computed against the mixer's default at the origin, which sits
    /// *in* the play plane and so answers a cue on top of it with no direction.
    #[test]
    fn the_listener_is_behind_the_play_plane_from_the_first_cue() {
        let audio = Audio::new(true);
        assert_eq!(
            audio.mixer.listener().position,
            [0.0, 0.0, -LISTENER_STANDOFF],
        );
    }

    /// **Where the camera is changes how a fixed cue sounds**, which is the
    /// half of this sample's convention that moves.
    ///
    /// One emitter, never moved, played from either side of it. The listener is
    /// no longer an argument, so this is the only test that can say the
    /// [`Audio::set_listener`] call in `crate::game`'s frame does anything: a
    /// `set_listener` that stored the value without the mixer using it leaves
    /// the two mixes identical.
    #[test]
    fn where_the_camera_is_changes_how_a_fixed_cue_sounds() {
        let mut audio = Audio::new(true);
        let emitter = DVec3::new(0.0, 0.0, 0.0);

        audio.set_listener(-6.0);
        audio.play_at(SOUND_FLAP, emitter);
        audio.set_listener(6.0);
        audio.play_at(SOUND_FLAP, emitter);

        let mixes = audio.mixer.voice_mixes();
        assert_eq!(mixes.len(), 2, "a cue went missing");
        let (from_left, from_right) = (mixes[0].1, mixes[1].1);
        assert!(
            from_left.gains.1 > from_left.gains.0,
            "a camera left of the flap hears it on the right: {:?}",
            from_left.gains,
        );
        assert!(
            from_right.gains.0 > from_right.gains.1,
            "and a camera right of it, on the left: {:?}",
            from_right.gains,
        );
        assert!(
            from_left.itd_samples < 0.0 && from_right.itd_samples > 0.0,
            "the interaural delay did not swap ears: {} then {}",
            from_left.itd_samples,
            from_right.itd_samples,
        );
    }
}
