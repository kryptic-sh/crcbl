//! Audio for asteroids: three procedural cues through `crcbl-audio`'s spatial
//! grammar and its mixer.
//!
//! The engine, the gun and the rocks coming apart, all synthesised at start-up —
//! this sample has no sound assets by design. The waveforms are banked in a
//! [`SoundBank`]; the game thread plays voices into a [`Mixer`] the audio thread
//! fills from.
//!
//! # Where the listener stands, and why this game finally makes it matter
//!
//! At the origin, which is the middle of the field and where the camera sits —
//! see `crate::gpu`'s `camera`, which is fixed there and never moves.
//!
//! That is the same *place* breakout and flappy put their listener and a
//! completely different *situation*. Breakout pans on one axis with a listener
//! it does not name; flappy's bird sits a third of the way across a scrolling
//! view, so its pan is a small constant and only the distance moves. Here the
//! emitters are spread over the whole 32 × 24 field and cross it constantly: a
//! rock shattering at the left edge is hard left and quiet, the same rock a
//! second later is hard right, and the ship's engine tracks the ship. Rules 1
//! and 3 of the grammar are doing their full range of work for the first time,
//! and it is audible without being a gimmick.
//!
//! [`compute_cue`]: crcbl_audio::spatial::compute_cue
//!
//! # The engine is a *held* sound, and it is now held rather than faked
//!
//! Thrust is the one cue in any sample that is sustained rather than an edge —
//! the player holds the key. It used to be a one-shot re-fired on a timer the
//! *simulation* owned (`game::THRUST_CUE_PERIOD`), which put an audio
//! implementation detail inside the deterministic tick: the pulse counter was
//! tick state, so an audio decision was part of what a replay had to reproduce.
//!
//! It is one looping [`Voice`](crcbl_audio::mixer::Voice) now.
//! [`Audio::set_thrust`] starts it on the first burning tick, re-aims it with
//! [`Mixer::set_mix`] on every tick after that — the ship crosses the field, so
//! a pan frozen at ignition would be wrong within a second — and
//! [`Mixer::stop`]s it the tick the key comes up. The simulation is left with
//! one plain bool, `thrusting`, which is a fact about the ship rather than about
//! the speakers.
//!
//! # What this file used to be
//!
//! A hand-written `Sound`, `Voice`, `VoiceQueue` and `MixerSource`, copied into
//! all four samples, because `Mixer::play` wanted `&mut self` while
//! `AudioStream::open` consumed its source. What is still local is the sound
//! design: the waveforms, the cue ids and the listener convention above.

use std::sync::Arc;

use crcbl_audio::mixer::{Mixer, SoundBank, VoiceId, VoiceMix};
use crcbl_audio::spatial::{CueGrammar, compute_cue};
use crcbl_audio::{AudioSample, AudioStream};

/// The engine, while thrust is held. One looping voice; see the module docs.
pub const SOUND_THRUST: u32 = 1;
/// A shot leaving the gun.
pub const SOUND_SHOT: u32 = 2;
/// A rock coming apart, or the ship doing the same.
pub const SOUND_EXPLOSION: u32 = 3;

/// How many cue ids this game has, and how long [`Audio::plays`] is.
const SOUND_COUNT: usize = 3;

/// How loud a one-shot cue is against the volume the grammar asks for.
///
/// Halved: the grammar's `volume` is a distance rolloff that reaches 1.0 on top
/// of the listener, and several cues overlapping at full scale clip.
const MASTER_GAIN: f32 = 0.5;

/// The same, for the engine.
///
/// Lower than [`MASTER_GAIN`] because the engine is *continuous*: the old pulse
/// was audible about half the time it was burning, and a loop at the one-shot
/// level sits on top of the gun rather than under it.
const ENGINE_GAIN: f32 = 0.25;

/// Owns the cues and the output stream.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    /// How many times each cue has been **emitted**, indexed as `id - 1`.
    ///
    /// Only ever increases, and only from the game thread. [`Audio::voices`]
    /// cannot answer "was this cue played?" — it counts the voices still
    /// sounding, and the audio thread reaps each one as it finishes, so the
    /// number falls again on a clock nothing here controls. Flappy had to add
    /// this to test its two cues; breakout still has no equivalent.
    ///
    /// [`SOUND_THRUST`] is counted once per **burn**, not once per block of
    /// engine noise: it is one voice that runs until the key comes up.
    plays: Vec<u64>,
    /// Every `(id, x, y)` handed to [`Audio::play_at`], in order, plus the
    /// position each burn of the engine *started* at.
    ///
    /// **The only place a cue's world position still exists as a position.**
    /// `play_at` turns it into a pan and a volume immediately, and the game
    /// drains its cue queue inside the same `Game::tick` that filled it, so a
    /// test asking "was the shot heard where the gun is" has nothing else to
    /// read. Test-only: a shipped build has no reason to keep the list.
    #[cfg(test)]
    played: Vec<(u32, f64, f64)>,
    /// The engine, while it is burning. See [`Audio::set_thrust`].
    thrust: Option<VoiceId>,
    mixer: Arc<Mixer>,
    _stream: Option<AudioStream>,
}

/// The sample rate every cue is synthesised at.
///
/// The stream resamples nothing, so a cue built at one rate and played at
/// another is simply the wrong pitch. 48 kHz is what the other two samples use
/// and what every device this has run on reports.
const SAMPLE_RATE: u32 = 48_000;

/// How many cycles of the engine tone one loop of it holds.
///
/// Eleven at [`ENGINE_HZ`] over [`SAMPLE_RATE`] is 4 800 frames — a tenth of a
/// second, and a whole number of both, which is what makes the loop seamless.
const ENGINE_CYCLES: u32 = 11;

/// The engine's pitch.
const ENGINE_HZ: f32 = 110.0;

impl Audio {
    pub fn new(headless: bool) -> Self {
        // A low tone for the engine, a short high blip for the gun, and a
        // filtered noise burst for a rock coming apart. The engine's is built by
        // `looped_sine` rather than `sine`: it is the one cue that plays as a
        // loop, so it must not be faded to zero at its ends.
        let mut bank = SoundBank::new();
        bank.insert(
            SOUND_THRUST,
            looped_sine(ENGINE_HZ, ENGINE_CYCLES, SAMPLE_RATE),
        );
        bank.insert(SOUND_SHOT, sine(900.0, 0.05, SAMPLE_RATE));
        bank.insert(SOUND_EXPLOSION, noise(0.32, SAMPLE_RATE));
        debug_assert_eq!(bank.len(), SOUND_COUNT, "a cue id is missing from the bank");

        // The stream takes a handle, not the mixer: this copy is what stays
        // behind to play voices through.
        let mixer = Arc::new(Mixer::new());
        let stream = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };
        if stream.is_none() && !headless {
            log::info!("audio: no output device available; the game will be silent");
        }

        Self {
            bank,
            plays: vec![0; SOUND_COUNT],
            #[cfg(test)]
            played: Vec::new(),
            thrust: None,
            mixer,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `(x, y)` in world space.
    ///
    /// The listener is the camera, at the origin — see the module docs. There is
    /// no listener argument because, unlike in flappy, there is nothing for it
    /// to vary with: this camera does not move.
    ///
    /// One-shots only. The engine is [`Audio::set_thrust`]'s.
    pub fn play_at(&mut self, id: u32, x: f64, y: f64) {
        // An id the bank does not know is simply absent, so there is no `id - 1`
        // to underflow on the lookup — only on the counter below, which is
        // reached solely for an id the bank *did* answer to.
        let Some(voice) = self.bank.create_voice(id) else {
            log::debug!("audio: no sound registered at id {id}");
            return;
        };
        self.mixer
            .play(voice.with_mix(self.cue_mix(x, y, MASTER_GAIN)));
        self.count(id, x, y);
    }

    /// Starts, re-aims or stops the engine.
    ///
    /// Called once a tick with the ship's own position. `burning` is
    /// `game::Game::thrusting` — whether the ship is under power this tick —
    /// and everything about how that becomes a sound lives here rather than in
    /// the simulation. See the module docs.
    ///
    /// Re-aiming every tick is the point: the ship crosses a 32-unit field in a
    /// couple of seconds, so a pan and a volume fixed at ignition would be
    /// audibly wrong long before the burn ends.
    pub fn set_thrust(&mut self, burning: bool, x: f64, y: f64) {
        if !burning {
            if let Some(id) = self.thrust.take() {
                self.mixer.stop(id);
            }
            return;
        }

        let mix = self.cue_mix(x, y, ENGINE_GAIN);
        // `set_mix` answers `false` for a handle whose voice is gone, which is
        // the restart condition: a looping voice only ends if something stopped
        // it, so this is also what recovers if anything ever does.
        if let Some(id) = self.thrust
            && self.mixer.set_mix(id, mix)
        {
            return;
        }
        let Some(voice) = self.bank.create_voice(SOUND_THRUST) else {
            return;
        };
        self.thrust = Some(self.mixer.play(voice.with_looping().with_mix(mix)));
        self.count(SOUND_THRUST, x, y);
    }

    /// Whether the engine is sounding right now.
    ///
    /// Asked of the mixer, not of the handle: a voice that was stopped is gone
    /// whatever this end still remembers.
    #[must_use]
    pub fn thrust_playing(&self) -> bool {
        self.thrust.is_some_and(|id| self.mixer.is_playing(id))
    }

    /// Drives a second of audio through the mixer and answers what is *still*
    /// sounding after it.
    ///
    /// The only honest way to ask "did anything leak". [`Audio::thrust_playing`]
    /// reads the handle this end kept, so a `set_thrust` that dropped the handle
    /// without stopping the voice would answer "silent" while a looping engine
    /// ran for the rest of the process — the exact shape of a check that cannot
    /// fail. Nothing survives this but a loop: every one-shot in this game is
    /// under a third of a second.
    #[cfg(test)]
    #[must_use]
    pub fn voices_after_a_second(&self) -> usize {
        let mut block = vec![0.0f32; SAMPLE_RATE as usize * 2];
        crcbl_audio::AudioSource::fill(self.mixer.as_ref(), &mut block, SAMPLE_RATE);
        self.mixer.voice_count()
    }

    /// The mix for a cue at `(x, y)`, heard from the camera at the origin.
    fn cue_mix(&self, x: f64, y: f64, gain: f32) -> VoiceMix {
        let cue = compute_cue(
            [0.0, 0.0, 0.0],
            [
                x as f32, y as f32,
                // A metre in front, so a sound at the listener's own position is
                // still at a defined direction rather than a zero vector.
                1.0,
            ],
            &CueGrammar::default(),
        );
        VoiceMix {
            volume: cue.volume * gain,
            ..VoiceMix::from(&cue)
        }
    }

    /// Records that cue `id` was emitted at `(x, y)`.
    fn count(&mut self, id: u32, x: f64, y: f64) {
        // `plays` is as long as the bank and neither ever grows, so an index
        // that found a sound finds a counter.
        if let Some(count) = id
            .checked_sub(1)
            .and_then(|i| self.plays.get_mut(i as usize))
        {
            *count += 1;
        }
        #[cfg(test)]
        self.played.push((id, x, y));
        #[cfg(not(test))]
        let _ = (x, y);
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
    /// answers to has never been played and reports zero. [`SOUND_THRUST`] is
    /// one per burn.
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

/// A sine that can be played end-to-end forever without a click.
///
/// The generator this sample adds, and the one that only a *looping* voice
/// needs. Two things make the seam inaudible and both are the opposite of what
/// [`sine`] does:
///
/// * **A whole number of cycles**, so the waveform arrives back at phase zero
///   exactly as the buffer runs out. The phase is stepped as
///   `2π · cycles · i / frames` rather than as `2π · f · t`, which makes that
///   exact by construction however `frames` rounds — the effective frequency
///   moves by a fraction of a hertz instead of the phase jumping.
/// * **No fade.** A fade to zero at each end is what stops a *one-shot*
///   clicking; on a loop it is a hole punched in the tone ten times a second.
fn looped_sine(freq_hz: f32, cycles: u32, sample_rate: u32) -> Vec<AudioSample> {
    let frames = ((cycles as f32 * sample_rate as f32) / freq_hz).round() as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let phase = 2.0 * std::f32::consts::PI * cycles as f32 * (i as f32 / frames as f32);
        let value = 0.3 * phase.sin();
        out.push(value);
        out.push(value);
    }
    out
}

/// A burst of low-passed noise that decays, as interleaved stereo.
///
/// The one generator the other two samples do not have, and the reason this file
/// is not *byte* identical to `apps/flappy/src/audio.rs`: a rock coming apart is
/// the one cue in three games that a sine cannot stand in for — a tone reads as
/// a beep, and a beep reads as scoring rather than as destruction.
///
/// Deterministic, from a fixed seed through the same splitmix64 mix the
/// simulation uses, so the sound a build ships is the sound every build ships
/// and a golden buffer would be possible later. The one-pole low pass takes the
/// hiss off the top; the exponential decay is what makes it a *burst*.
fn noise(seconds: f32, sample_rate: u32) -> Vec<AudioSample> {
    /// How fast the burst decays, in nepers per second. `e^-9t` is down to a
    /// twentieth of its peak by a fifth of a second.
    const DECAY: f32 = 9.0;
    /// The one-pole coefficient: `y += ALPHA * (x - y)`. Lower is duller.
    const ALPHA: f32 = 0.16;

    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    let mut state = 0x4173_7465_726F_6964_u64;
    let mut low = 0.0f32;
    for i in 0..frames {
        // splitmix64, as `game::hash_unit` uses it. The top 24 bits are the ones
        // it mixes best, and 24 is exactly an `f32`'s mantissa, so every value
        // this produces is representable rather than rounded.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let white = (z >> 40) as f32 / 8_388_608.0 - 1.0;

        low += ALPHA * (white - low);
        let t = i as f32 / sample_rate as f32;
        let value = 0.45 * low * (-DECAY * t).exp() * fade(i, frames);
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

    /// Every generator here produces **interleaved stereo**, which is what the
    /// mixer's playhead assumes: an odd-length or mono buffer would be played at
    /// half speed over twice the length. Breakout shipped that bug once.
    #[test]
    fn every_cue_is_interleaved_stereo_of_the_length_it_asked_for() {
        for (name, data, frames) in [
            ("sine", sine(900.0, 0.05, SAMPLE_RATE), 2_400usize),
            ("noise", noise(0.32, SAMPLE_RATE), 15_360),
            (
                "looped_sine",
                looped_sine(ENGINE_HZ, ENGINE_CYCLES, SAMPLE_RATE),
                4_800,
            ),
        ] {
            assert_eq!(data.len(), frames * 2, "{name} is not stereo pairs");
            for frame in data.chunks_exact(2) {
                assert_eq!(frame[0], frame[1], "{name} is not the same in both ears");
            }
        }
    }

    /// **The engine's buffer is a bare tone: a whole number of cycles, and no
    /// envelope on it.** Read out of the bank, so it is the buffer the game will
    /// actually loop and not whatever this test asks the generator for.
    ///
    /// Compared against the closed form rather than against neighbouring
    /// samples, because what goes wrong here is quiet: [`FADE_FRAMES`] is 60 of
    /// these 4 800, so a faded buffer arrives at *zero* on both ends — the seam
    /// looks smoother than the tone does, and a seam-only check passes while the
    /// loop has a hole punched in it ten times a second. Restating the waveform
    /// is the point; it is the contract this cue has to meet, and a `sine` in
    /// the bank instead of a `looped_sine` fails it at the sixtieth sample.
    #[test]
    fn the_engine_loops_a_whole_number_of_cycles_with_no_envelope() {
        let audio = Audio::new(true);
        let data = audio
            .bank
            .sound(SOUND_THRUST)
            .expect("the engine is banked");
        let frames = data.len() / 2;
        assert_eq!(frames, 4_800, "a tenth of a second at {SAMPLE_RATE} Hz");

        for (i, frame) in data.chunks_exact(2).enumerate() {
            let phase =
                2.0 * std::f32::consts::PI * ENGINE_CYCLES as f32 * (i as f32 / frames as f32);
            let expected = 0.3 * phase.sin();
            assert!(
                (frame[0] - expected).abs() < 1e-6,
                "frame {i} is {} and the bare tone is {expected}",
                frame[0],
            );
        }

        // Whole cycles, so playing the buffer end to end continues the tone
        // rather than jumping phase — and at the pitch that was asked for.
        let crossings = (1..frames)
            .filter(|i| (data[i * 2] < 0.0) != (data[(i - 1) * 2] < 0.0))
            .count();
        assert_eq!(
            crossings,
            2 * ENGINE_CYCLES as usize - 1,
            "not whole cycles"
        );
        let effective = ENGINE_CYCLES as f32 * SAMPLE_RATE as f32 / frames as f32;
        assert!(
            (effective - ENGINE_HZ).abs() < 0.5,
            "the loop runs at {effective} Hz, not {ENGINE_HZ}",
        );
    }

    /// An id nothing answers to is ignored rather than underflowing or panicking.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_at(0, 0.0, 0.0);
        audio.play_at(9999, 0.0, 0.0);
        assert_eq!(audio.voices(), 0);
        // `plays` still spells `id - 1`, so it still has the underflow to avoid,
        // and it must not report a play for a cue that was refused.
        assert_eq!(audio.plays(0), 0);
        assert_eq!(audio.plays(9999), 0);
        assert_eq!(audio.plays(SOUND_SHOT), 0);

        audio.play_at(SOUND_SHOT, 0.0, 0.0);
        assert_eq!(audio.voices(), 1);
        assert_eq!(audio.plays(SOUND_SHOT), 1);
        assert_eq!(audio.plays(SOUND_THRUST), 0, "only the shot was played");
        assert_eq!(audio.plays(SOUND_EXPLOSION), 0, "only the shot was played");
    }

    /// `plays` counts emissions, not the voices still sounding — the whole
    /// reason it exists. One that merely reported `voices()` would agree with
    /// this test right up until the audio thread reaped the voice.
    #[test]
    fn a_cue_stays_counted_after_its_voice_is_gone() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_SHOT, 0.0, 0.0);
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
            crcbl_audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        }
        assert_eq!(
            audio.plays(SOUND_SHOT),
            1,
            "the shot stopped being counted once it stopped sounding"
        );
    }

    /// **The engine is one voice that survives its own sample data**, and it
    /// ends when the key comes up rather than when the buffer runs out.
    ///
    /// The engine buffer is a tenth of a second; this drives ten seconds of
    /// audio through the mixer by hand. A one-shot — which is what this cue used
    /// to be — is gone after the first block, so `thrust_playing` going false
    /// anywhere in that loop is the pulse hack coming back.
    #[test]
    fn the_engine_is_one_looping_voice_that_outlives_its_buffer() {
        let mut audio = Audio::new(true);
        assert!(!audio.thrust_playing(), "the engine started on its own");

        audio.set_thrust(true, 0.0, 0.0);
        assert!(audio.thrust_playing());
        assert_eq!(audio.plays(SOUND_THRUST), 1);
        assert_eq!(audio.voices(), 1);

        let mut block = vec![0.0f32; 4_800 * 2]; // a tenth of a second
        for tenth in 0..100 {
            block.fill(0.0);
            crcbl_audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
            assert!(
                audio.thrust_playing(),
                "the engine stopped after {tenth} tenths of a second",
            );
            assert!(
                block.iter().any(|s| s.abs() > 1e-3),
                "the engine went silent after {tenth} tenths of a second",
            );
            // Held, so the game keeps saying so — and it is still one voice.
            audio.set_thrust(true, 0.0, 0.0);
            assert_eq!(audio.voices(), 1, "a second engine voice was started");
            assert_eq!(audio.plays(SOUND_THRUST), 1, "the burn was recounted");
        }

        audio.set_thrust(false, 0.0, 0.0);
        assert!(
            !audio.thrust_playing(),
            "the key came up and it kept running"
        );
        assert_eq!(audio.voices(), 0);
        block.fill(0.0);
        crcbl_audio::AudioSource::fill(audio.mixer.as_ref(), &mut block, 48_000);
        assert!(
            block.iter().all(|s| *s == 0.0),
            "the stopped engine is still audible",
        );

        // A second burn is a second play, and a new voice.
        audio.set_thrust(true, 0.0, 0.0);
        assert_eq!(audio.plays(SOUND_THRUST), 2);
        assert!(audio.thrust_playing());
    }

    /// **The engine follows the ship**, which is what a re-aimable voice buys
    /// over a stack of one-shots: one voice, two pans.
    ///
    /// Without the `set_mix` call in `set_thrust` this fails — the mix would
    /// stay whatever it was at ignition — and no other test would notice.
    #[test]
    fn the_engine_pans_with_the_ship_without_restarting() {
        let mut audio = Audio::new(true);
        audio.set_thrust(true, -14.0, 0.0);
        let left = audio.mixer.voice_mixes();
        assert_eq!(left.len(), 1);
        assert!(
            left[0].1.gains.0 > left[0].1.gains.1,
            "a ship on the left should be louder on the left: {:?}",
            left[0].1.gains,
        );

        audio.set_thrust(true, 14.0, 0.0);
        let right = audio.mixer.voice_mixes();
        assert_eq!(right.len(), 1, "the engine restarted instead of turning");
        assert_eq!(right[0].0, left[0].0, "a new voice, not the same one");
        assert!(
            right[0].1.gains.1 > right[0].1.gains.0,
            "a ship on the right should be louder on the right: {:?}",
            right[0].1.gains,
        );
        assert_eq!(audio.plays(SOUND_THRUST), 1, "one burn, one play");
    }

    /// The grammar is actually consulted: a cue away from the listener is not
    /// the same cue as one on top of it.
    ///
    /// Without this, `play_at` could ignore its coordinates entirely and every
    /// test above would still pass — the sample would ship "spatial audio" that
    /// was a constant. It bites harder here than in flappy: this game's emitters
    /// really do cross the whole field, so both halves of the assertion are the
    /// normal case rather than an edge one.
    #[test]
    fn where_a_cue_happens_changes_how_it_sounds() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_EXPLOSION, 0.0, 0.0);
        audio.play_at(SOUND_EXPLOSION, -14.0, 6.0);
        audio.play_at(SOUND_EXPLOSION, 14.0, 6.0);
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

    /// The explosion is noise rather than a tone, decays, and is finite.
    ///
    /// The generator this sample added, so it gets its own check rather than
    /// riding on the sine's. "Decays" is measured as the second half being
    /// quieter than the first, which a sine would fail.
    #[test]
    fn the_explosion_is_a_decaying_burst_of_noise() {
        let data = noise(0.32, 48_000);
        assert!(!data.is_empty());
        assert!(data.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        let frames = data.len() / 2;
        let peak =
            |range: std::ops::Range<usize>| range.fold(0.0f32, |acc, i| acc.max(data[i * 2].abs()));
        let early = peak(0..frames / 4);
        let late = peak(frames * 3 / 4..frames);
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
    }
}
