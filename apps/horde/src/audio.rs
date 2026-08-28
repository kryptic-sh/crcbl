//! Audio for horde: six procedural cues through `crcbl-audio`'s spatial
//! grammar and its mixer, and the first voice cap any sample has needed.
//!
//! The gun, an enemy coming apart, a gem banked, a potion drunk, a level gained
//! and the player dying, all synthesised at start-up — this sample has no sound
//! assets by design. The waveforms are banked in a [`SoundBank`]; the game thread plays
//! voices into a [`Mixer`] the audio thread fills from.
//!
//! # Where the listener stands, and why this game moves it
//!
//! **On the player**, which walks. Breakout and asteroids nail their listener
//! to a fixed camera and place it once at start-up; flappy's moves on one axis.
//! This arena is 96 × 72 units against a view of about 37 × 28 and the camera
//! follows the player, so the only listener that agrees with the picture is the
//! player's own position — the one sample whose listener moves on **both**
//! axes.
//!
//! That is [`Audio::set_listener`], called once a frame from `crate::game`
//! before the frame's cues are drained. [`Audio::play_at`] takes a world
//! position and nothing else, exactly as the other games' does: the
//! convention is a line in the frame rather than an argument on every cue.
//!
//! [`compute_cue`]: crcbl::audio::spatial::compute_cue
//!
//! # This game emits cues faster than any earlier one, so it caps its voices
//!
//! The other games raise a handful of cues a second and never think
//! about it. Here a kill is a cue, a gem is a cue, and the gun's cooldown floor
//! is [`crate::game::FIRE_COOLDOWN_FLOOR`] — a twentieth of a second — so a
//! late run raises up to about forty a second and every one of them is a voice
//! that lives until it runs out. [`Mixer`] has **no voice limit, no priority and
//! no stealing**: [`crcbl::audio::AudioSource::fill`] walks whatever is in the
//! list, so a game that plays faster than its sounds finish pays for all of it
//! on the audio thread.
//!
//! [`MAX_VOICES`] is this sample's answer and it is deliberately the crudest
//! one that is honest: refuse the new voice rather than steal an old one, and
//! count the refusal. It still counts the cue as **emitted** — see
//! [`Audio::plays`] — because "did this event happen" and "was there a speaker
//! free" are different questions and a test asking the first must not be
//! answered by the second.
//!
//! That the cap lives here rather than in the engine is a finding, not a
//! preference: `docs/backlog.md` carries it.
//!
//! # What this file used to be
//!
//! A hand-written `Sound`, `Voice`, `VoiceQueue` and `MixerSource` — the fourth
//! copy of the same four types, because `Mixer::play` wanted `&mut self` while
//! `AudioStream::open` consumed its source, so nothing could hold both ends.
//! `play` takes `&self` now and the stream takes an [`Arc`], so the playhead,
//! the queue and the mixing loop are the engine's. What is still local is the
//! sound design: the waveforms, the cue ids, the listener convention and the
//! cap.

use std::sync::Arc;

use crcbl::audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl::audio::spatial::{CueGrammar, Listener};
use crcbl::audio::synth;
use crcbl::audio::{AudioSample, AudioStream};
use crcbl::math::DVec3;

/// A bolt leaving the gun.
pub const SOUND_SHOT: u32 = 1;
/// An enemy coming apart.
pub const SOUND_KILL: u32 = 2;
/// A gem banked.
pub const SOUND_PICKUP: u32 = 3;
/// A level gained, and the screen that opens with it.
pub const SOUND_LEVEL: u32 = 4;
/// The player running out of hit points.
pub const SOUND_DEATH: u32 = 5;
/// A potion drunk, and hit points back with it.
///
/// **A sixth cue rather than a second use of [`SOUND_PICKUP`].** A gem is the
/// most common sound in the game — a player hears one for very nearly every
/// kill — and a potion is among the rarest, by
/// [`crate::game::POTION_DROP_CHANCE`]'s design. Playing the rarest event
/// through the sound of the most common one is the same as not playing it: the
/// blip would arrive in a stream of identical blips and the only notice of a
/// heal would be a bar the player is not looking at while a brute is on them.
pub const SOUND_HEAL: u32 = 6;

/// How many cue ids this game has, and how long [`Audio::plays`] is.
const SOUND_COUNT: usize = 6;

/// How loud a cue is against the volume the grammar asks for. See breakout's.
const MASTER_GAIN: f32 = 0.5;

/// How far behind the play plane the listener stands. See breakout's.
const LISTENER_STANDOFF: f32 = 1.0;

/// How many voices may be sounding at once.
///
/// See this module's header. Sixteen is about a third of a second of this
/// game's worst-case emission rate, which is long enough that a burst of kills
/// still reads as a burst and short enough that the audio thread's per-block
/// work stays bounded no matter what the simulation does.
pub const MAX_VOICES: usize = 16;

/// Owns the cues and the output stream.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    /// How many times each cue has been **emitted**, indexed as `id - 1`.
    ///
    /// Only ever increases, and only from the game thread. [`Audio::voices`]
    /// cannot answer "was this cue played?" — it counts the voices still
    /// sounding, and the audio thread reaps each one as it finishes, so the
    /// number falls again on a clock nothing here controls. It cannot answer it
    /// here for a second reason too: [`MAX_VOICES`] refuses a voice on a busy
    /// frame, and the cue still happened.
    plays: Vec<u64>,
    /// Cues that were emitted with no free voice. See [`MAX_VOICES`].
    ///
    /// Instrumentation rather than mechanism — the debug overlay reads it and
    /// nothing decides anything on it — but it is the number that says whether
    /// the cap is being hit at all, which is the whole reason the cap is
    /// interesting.
    dropped: u64,
    /// Every `(id, x, y)` handed to [`Audio::play_at`], in order.
    ///
    /// **The only place a cue's world position still exists as a position.**
    /// `play_at` turns it into a pan and a volume immediately, and the game
    /// drains its cue queue inside the same `Game::tick` that filled it, so a
    /// test asking "was the kill heard where the enemy was" has nothing else to
    /// read. Test-only: a shipped build has no reason to keep the list.
    #[cfg(test)]
    played: Vec<(u32, f64, f64)>,
    mixer: Arc<Mixer>,
    _stream: Option<AudioStream>,
}

/// The sample rate every cue is synthesised at.
///
/// The stream resamples nothing, so a cue built at one rate and played at
/// another is simply the wrong pitch. 48 kHz is what the other games use and
/// what every device this has run on reports.
const SAMPLE_RATE: u32 = 48_000;

/// The settings directory this sample reads its volumes out of.
///
/// The same spelling `crate::gpu` hands
/// [`GpuContextDesc::label`](crcbl::engine::GpuContextDesc::label), because it
/// is the same directory: a player has one settings file per game, and two
/// spellings of the name would be a video section and an audio section in
/// different files.
const APP_NAME: &str = "horde";

impl Audio {
    pub fn new(headless: bool) -> Self {
        // Short high blip for the gun, a filtered noise burst for an enemy
        // coming apart, a brighter and shorter blip for a gem, a short rise for
        // a potion, a longer and lower two-tone rise for a level, and a long low
        // burst for the player's own end. Every one of them is shorter than the
        // last sample's equivalents, because this game raises far more of them:
        // see `MAX_VOICES`.
        //
        // The potion and the level are the same *shape* — a sweep upwards, which
        // is what "something got better" sounds like — and are told apart by
        // being a fifth apart in register and a third of the length. Nothing
        // else here sweeps, so neither can be confused with a blip or a burst.
        //
        // Banked once. `SoundBank::create_voice` shares the buffer rather than
        // copying it, which at this game's cue rate is the difference between a
        // playhead and an allocation the size of the sound per kill.
        let mut bank = SoundBank::new();
        bank.insert(SOUND_SHOT, synth::sine(760.0, 0.045, SAMPLE_RATE));
        bank.insert(
            SOUND_KILL,
            synth::noise_burst(0.14, 12.0, NOISE_SEED, SAMPLE_RATE),
        );
        bank.insert(SOUND_PICKUP, synth::sine(1_320.0, 0.05, SAMPLE_RATE));
        bank.insert(SOUND_HEAL, rise(660.0, 990.0, 0.10, SAMPLE_RATE));
        bank.insert(SOUND_LEVEL, rise(440.0, 880.0, 0.30, SAMPLE_RATE));
        bank.insert(
            SOUND_DEATH,
            synth::noise_burst(0.55, 4.0, NOISE_SEED, SAMPLE_RATE),
        );
        debug_assert_eq!(bank.len(), SOUND_COUNT, "a cue id is missing from the bank");

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
            plays: vec![0; SOUND_COUNT],
            dropped: 0,
            #[cfg(test)]
            played: Vec::new(),
            mixer,
            _stream: stream,
        };
        // The player starts at the origin and the game moves the listener every
        // frame after; placing it here is what stops a cue raised before the
        // first `set_listener` being computed against the mixer's default, which
        // sits *in* the play plane rather than back from it.
        audio.set_listener(DVec3::ZERO);
        audio
    }

    /// Puts the listener on the player, at `at`. See the module docs.
    ///
    /// Called once a frame, before the frame's cues are played.
    pub fn set_listener(&self, at: DVec3) {
        self.mixer.set_listener(Listener::new([
            at.x as f32,
            at.y as f32,
            at.z as f32 - LISTENER_STANDOFF,
        ]));
    }

    /// Plays a cue for something happening at `at` in world space.
    ///
    /// No listener argument: [`Audio::set_listener`] put the ear on the player,
    /// and the [`Mixer`] has held it since.
    pub fn play_at(&mut self, id: u32, at: DVec3) {
        // An id the bank does not know is simply absent, so there is no `id - 1`
        // to underflow on the lookup — only on the counter below, which is
        // reached solely for an id the bank *did* answer to.
        let Some(voice) = self.bank.create_voice(id) else {
            crcbl::log::debug!("audio: no sound registered at id {id}");
            return;
        };
        // Counted before the cap, not after: the cue happened either way, and a
        // counter that only counted the audible ones could not tell a game that
        // never fired from one that fired into a full mixer.
        if let Some(count) = id
            .checked_sub(1)
            .and_then(|i| self.plays.get_mut(i as usize))
        {
            *count += 1;
        }
        #[cfg(test)]
        self.played.push((id, at.x, at.y));

        // The cap, read and acted on in two separate locks rather than one. The
        // game thread is the only one that adds and the audio thread only ever
        // removes, so the count can be *stale low* by the time the voice goes
        // in and never stale high: this refuses a cue that had just been made
        // room for, and never exceeds `MAX_VOICES`.
        if self.mixer.voice_count() >= MAX_VOICES {
            self.dropped += 1;
            return;
        }

        let cue = self.mixer.cue(
            [at.x as f32, at.y as f32, at.z as f32],
            &CueGrammar::default(),
        );
        self.mixer.play(voice.with_mix(VoiceMix {
            volume: cue.volume * MASTER_GAIN,
            ..VoiceMix::from(&cue)
        }));
    }

    /// Every cue played so far, with the world position it was played at.
    ///
    /// See [`Audio::played`]. Only the *emitted* cues appear: an id no sound
    /// answers to is refused before it gets here.
    #[cfg(test)]
    #[must_use]
    pub fn played(&self) -> &[(u32, f64, f64)] {
        &self.played
    }

    /// How many voices are **currently sounding**. For the debug overlay.
    ///
    /// Not a record of what was played: the audio thread drops each voice as it
    /// runs out, so this falls again on its own. Use [`Audio::plays`] to ask
    /// whether a cue happened.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.mixer.voice_count()
    }

    /// How many times cue `id` has been emitted since start-up.
    ///
    /// Monotonic, so it answers the question [`Audio::voices`] cannot: whether
    /// a cue was ever emitted, however long ago it finished and whether or not
    /// [`MAX_VOICES`] found it a voice. An id no sound answers to has never been
    /// played and reports zero.
    #[must_use]
    pub fn plays(&self, id: u32) -> u64 {
        id.checked_sub(1)
            .and_then(|i| self.plays.get(i as usize))
            .copied()
            .unwrap_or(0)
    }

    /// How many cues found no free voice. See [`MAX_VOICES`].
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// The panel's audio section: how many cues [`MAX_VOICES`] refused.
///
/// The cap refuses the **newest** voice and counts the refusal in
/// [`Audio::dropped`], and nothing else shows that count — the number lives on
/// [`Audio`] and no other system reads it. A player whose death cue is refused
/// by a field of kill cues raised on the same tick hears nothing, and the panel
/// is the only place the reason exists: one row, so the silence is attributable
/// rather than a mystery to debug by ear.
impl crcbl::ui::DebugModule for Audio {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("audio");
        section.row("dropped", format_args!("{}", self.dropped()));
    }
}

/// The seed the kill and death bursts are drawn from. Spells "HORDESEE", and is
/// the value this game's `DEFAULT_SEED` uses.
///
/// A different seed here is a different-sounding burst, not a wrong one —
/// [`synth::noise_burst`] is deterministic from it, so the sound this build
/// ships is the sound every build ships.
const NOISE_SEED: u64 = 0x484F_5244_4553_4545;

/// A tone that sweeps from `from_hz` to `to_hz`, as interleaved stereo.
///
/// The generator this sample adds, and the one cue here that is not an *event*
/// so much as an announcement: a level is the only thing in the game that stops
/// it. The phase is integrated rather than computed as `2πft` with a moving `f`
/// — that spelling sweeps at twice the intended rate and puts a discontinuity
/// nowhere in particular, which is audible as a click.
fn rise(from_hz: f32, to_hz: f32, seconds: f32, sample_rate: u32) -> Vec<AudioSample> {
    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    let mut phase = 0.0f32;
    for i in 0..frames {
        let t = if frames > 1 {
            i as f32 / (frames - 1) as f32
        } else {
            0.0
        };
        let freq = from_hz + (to_hz - from_hz) * t;
        phase += 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
        let value = synth::TONE_AMPLITUDE * phase.sin() * synth::fade_gain(i, frames);
        out.push(value);
        out.push(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::DebugModule;

    /// `rise` produces **interleaved stereo**, which is what the mixer's
    /// playhead assumes: an odd-length or mono buffer would be played at half
    /// speed over twice the length. Breakout shipped that bug once.
    ///
    /// Only `rise` is checked here. The engine's generators carry the same
    /// assertion in `crcbl_audio::synth`, and this is the one horde wrote.
    #[test]
    fn the_swept_cue_is_interleaved_stereo_of_the_length_it_asked_for() {
        let seconds = 0.30f32;
        let data = rise(440.0, 880.0, seconds, SAMPLE_RATE);
        let frames = (SAMPLE_RATE as f32 * seconds) as usize;
        assert_eq!(data.len(), frames * 2, "rise is not stereo pairs");
        for frame in data.chunks_exact(2) {
            assert_eq!(frame[0], frame[1], "rise is not the same in both ears");
        }
    }

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
        assert_eq!(audio.plays(SOUND_SHOT), 0);

        audio.play_at(SOUND_SHOT, DVec3::ZERO);
        assert_eq!(audio.voices(), 1);
        assert_eq!(audio.plays(SOUND_SHOT), 1);
        for other in [
            SOUND_KILL,
            SOUND_PICKUP,
            SOUND_HEAL,
            SOUND_LEVEL,
            SOUND_DEATH,
        ] {
            assert_eq!(audio.plays(other), 0, "only the shot was played");
        }
    }

    /// `plays` counts emissions, not the voices still sounding — the whole
    /// reason it exists. One that merely reported `voices()` would agree with
    /// this test right up until the audio thread reaped the voice.
    #[test]
    fn a_cue_stays_counted_after_its_voice_is_gone() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_SHOT, DVec3::ZERO);
        assert_eq!(audio.plays(SOUND_SHOT), 1);

        // Reap it by hand rather than waiting on the audio thread, so the test
        // is not itself a race: `fill` is exactly what that thread calls.
        let mut block = vec![0.0f32; 256 * 2];
        let start = std::time::Instant::now();
        while audio.voices() > 0 {
            assert!(
                start.elapsed().as_secs() < 5,
                "the shot voice never finished"
            );
            block.fill(0.0);
            crcbl::audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        }
        assert_eq!(
            audio.plays(SOUND_SHOT),
            1,
            "the shot stopped being counted once it stopped sounding"
        );
    }

    /// **The cap refuses a voice and still counts the cue.**
    ///
    /// The distinction the whole of `plays` rests on, and the one this sample
    /// is the first to need: a game that raised forty cues a second into a full
    /// mixer must still be able to answer "did the kill happen".
    #[test]
    fn a_full_mixer_refuses_the_voice_and_still_counts_the_cue() {
        let mut audio = Audio::new(true);
        for _ in 0..MAX_VOICES {
            audio.play_at(SOUND_KILL, DVec3::ZERO);
        }
        assert_eq!(audio.voices(), MAX_VOICES);
        assert_eq!(audio.plays(SOUND_KILL), MAX_VOICES as u64);
        assert_eq!(audio.dropped(), 0, "the cap fired early");

        for _ in 0..7 {
            audio.play_at(SOUND_KILL, DVec3::ZERO);
        }
        assert_eq!(audio.voices(), MAX_VOICES, "the cap let a voice through");
        assert_eq!(
            audio.plays(SOUND_KILL),
            MAX_VOICES as u64 + 7,
            "a refused voice stopped the cue being counted",
        );
        assert_eq!(audio.dropped(), 7);

        // …and a voice that finishes makes room again, or the cap is a mute
        // button rather than a limit.
        let mut block = vec![0.0f32; 48_000 * 2];
        crcbl::audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        assert_eq!(audio.voices(), 0, "a whole second did not drain the queue");
        audio.play_at(SOUND_KILL, DVec3::ZERO);
        assert_eq!(audio.voices(), 1);
    }

    /// **The debug section reports what [`Audio::dropped`] counts** — the
    /// refusal number is on [`Audio`] and nowhere else, so this is the row that
    /// tells a player why a cue (their death, say) was silent: sixteen kill cues
    /// on one tick refuse the seventeenth, and the panel is the only place that
    /// says so.
    #[test]
    fn the_debug_section_shows_the_refusal_count() {
        let mut audio = Audio::new(true);
        for _ in 0..MAX_VOICES + 3 {
            audio.play_at(SOUND_KILL, DVec3::ZERO);
        }
        assert_eq!(audio.dropped(), 3, "the cap refused the wrong number");

        let mut section = crcbl::ui::DebugSection::new("audio");
        audio.debug_section(&mut section);
        assert_eq!(section.title(), "audio");
        assert_eq!(
            section.rows(),
            &[crcbl::ui::DebugRow {
                label: "dropped".into(),
                value: "3".into(),
            }],
            "the section must contain exactly the dropped row",
        );
    }

    /// A fresh [`Audio`] shows the row at zero — nothing refused must still be
    /// reported, or the panel reads the same for "no cap pressure" and "not
    /// wired up".
    #[test]
    fn a_fresh_audio_reports_zero_dropped() {
        let audio = Audio::new(true);
        let mut section = crcbl::ui::DebugSection::new("audio");
        audio.debug_section(&mut section);
        assert_eq!(section.title(), "audio");
        assert_eq!(
            section.rows(),
            &[crcbl::ui::DebugRow {
                label: "dropped".into(),
                value: "0".into(),
            }],
        );
    }

    /// **The listener is placed before anything can be played through it.** The
    /// game puts it on the player every frame, but a cue raised before the first
    /// frame must not be computed against the mixer's default at the origin,
    /// which sits *in* the play plane and so answers a cue on top of it with no
    /// direction.
    #[test]
    fn the_listener_is_behind_the_play_plane_from_the_first_cue() {
        let audio = Audio::new(true);
        assert_eq!(
            audio.mixer.listener().position,
            [0.0, 0.0, -LISTENER_STANDOFF],
        );

        // …and it follows the player rather than staying put.
        audio.set_listener(DVec3::new(-7.0, 3.0, 0.0));
        assert_eq!(
            audio.mixer.listener().position,
            [-7.0, 3.0, -LISTENER_STANDOFF],
        );
    }

    /// The grammar is actually consulted: a cue away from the listener is not
    /// the same cue as one on top of it, **and the listener is the player**.
    ///
    /// Without this, `play_at` could ignore its coordinates entirely and every
    /// test above would still pass — the sample would ship "spatial audio" that
    /// was a constant. The second half is this game's own: an identical world
    /// position must sound different depending on where the player is standing,
    /// which is the half a copy of asteroids' fixed-camera version would fail —
    /// and which is now the half that says [`Audio::set_listener`] reaches the
    /// cue at all, since the listener has stopped being an argument.
    #[test]
    fn where_a_cue_happens_and_where_the_player_stands_both_change_how_it_sounds() {
        let mut audio = Audio::new(true);
        let far_corner = DVec3::new(-14.0, 6.0, 0.0);
        audio.set_listener(DVec3::ZERO);
        audio.play_at(SOUND_KILL, DVec3::ZERO);
        audio.play_at(SOUND_KILL, far_corner);
        audio.play_at(SOUND_KILL, DVec3::new(14.0, 6.0, 0.0));
        // The same emitter as the second, heard by a player who has walked
        // right on top of it.
        audio.set_listener(far_corner);
        audio.play_at(SOUND_KILL, far_corner);
        let mixes = audio.mixer.voice_mixes();
        assert_eq!(mixes.len(), 4, "a cue went missing");
        let (near, left, right, moved) = (mixes[0].1, mixes[1].1, mixes[2].1, mixes[3].1);
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
            near.volume
        );
        assert!(
            moved.volume > left.volume,
            "walking onto the emitter did not make it louder: {} vs {}",
            moved.volume,
            left.volume,
        );
    }

    /// The kill is noise rather than a tone, decays, and is finite — and the
    /// decay argument is actually used, which a constant-decay copy would not
    /// show.
    #[test]
    fn the_kill_is_a_decaying_burst_of_noise_and_the_decay_is_a_knob() {
        let data = synth::noise_burst(0.14, 12.0, NOISE_SEED, 48_000);
        assert!(!data.is_empty());
        assert!(data.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        let frames = data.len() / 2;
        let peak = |data: &[f32], range: std::ops::Range<usize>| {
            range.fold(0.0f32, |acc, i| acc.max(data[i * 2].abs()))
        };
        let early = peak(&data, 0..frames / 4);
        let late = peak(&data, frames * 3 / 4..frames);
        assert!(early > 0.0, "the burst is silent");
        assert!(
            late < early * 0.5,
            "the burst does not decay: {early} then {late}"
        );
        // Two consecutive frames of a tone are close together; of noise, mostly
        // are not. This is what says the generator is not a sine.
        let jumps = (1..frames)
            .filter(|i| (data[i * 2] - data[(i - 1) * 2]).abs() > 1e-4)
            .count();
        assert!(
            jumps > frames / 4,
            "only {jumps} of {frames} frames changed; this is not noise"
        );

        // The same length at a tenth of the decay is still ringing where the
        // fast one has gone. Without this the argument could be ignored.
        let slow = synth::noise_burst(0.14, 1.2, NOISE_SEED, 48_000);
        let slow_late = peak(&slow, frames * 3 / 4..frames);
        assert!(
            slow_late > late,
            "the decay argument does nothing: {slow_late} vs {late}",
        );
    }

    /// The level cue really sweeps: its second half is a higher pitch than its
    /// first, which a plain sine at either endpoint would fail.
    ///
    /// Measured as zero crossings per half, which is a frequency without
    /// needing a transform.
    #[test]
    fn the_level_cue_rises_in_pitch() {
        let data = rise(440.0, 880.0, 0.30, 48_000);
        let frames = data.len() / 2;
        assert!(frames > 0);
        assert!(data.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        let crossings = |range: std::ops::Range<usize>| {
            range
                .clone()
                .skip(1)
                .filter(|i| (data[i * 2] < 0.0) != (data[(i - 1) * 2] < 0.0))
                .count()
        };
        let first = crossings(0..frames / 2);
        let second = crossings(frames / 2..frames);
        assert!(
            second > first + first / 4,
            "the sweep is flat: {first} crossings then {second}",
        );
    }
}
