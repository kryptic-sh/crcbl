//! Audio for horde: five procedural cues through `crcbl-audio`'s spatial
//! grammar, and the first voice cap any sample has needed.
//!
//! The gun, an enemy coming apart, a gem banked, a level gained and the player
//! dying, all synthesised at start-up — this sample has no sound assets by
//! design. The game thread pushes voices onto a shared queue; the audio thread
//! drains it.
//!
//! # Where the listener stands, and why this game moves it
//!
//! **On the player**, which walks. Breakout pans on one axis with a listener it
//! does not name; flappy's bird sits a third of the way across a scrolling view,
//! so its pan is a small constant; asteroids puts the listener at the origin
//! because its camera is nailed there. This arena is 96 × 72 units against a
//! view of about 37 × 28 and the camera follows the player, so the only listener
//! that agrees with the picture is the player's own position — which means
//! [`Audio::play_at`] takes **both** coordinates of it, and that is a fourth
//! convention in four games. See the S3 findings note in
//! `docs/plan/ROADMAP.md`.
//!
//! [`compute_cue`]: crcbl_audio::spatial::compute_cue
//!
//! # This game emits cues faster than any earlier one, so it caps its voices
//!
//! The other three samples raise a handful of cues a second and never think
//! about it. Here a kill is a cue, a gem is a cue, and the gun's cooldown floor
//! is [`crate::game::FIRE_COOLDOWN_FLOOR`] — a twentieth of a second — so a
//! late run raises up to about forty a second and every one of them is a voice
//! that lives until it runs out. `crcbl-audio` has **no voice limit, no
//! priority and no stealing**: `AudioSource::fill` walks whatever is in the
//! queue, so a game that pushes faster than its sounds finish pays for all of
//! it on the audio thread.
//!
//! [`MAX_VOICES`] is this sample's answer and it is deliberately the crudest
//! one that is honest: refuse the new voice rather than steal an old one, and
//! count the refusal. It still counts the cue as **emitted** — see
//! [`Audio::plays`] — because "did this event happen" and "was there a speaker
//! free" are different questions and a test asking the first must not be
//! answered by the second.
//!
//! # This is the *fourth* copy of this file, and the copies have drifted
//!
//! `docs/plan/ROADMAP.md`'s S1B finding 5 says `crcbl-audio` offers a device, a
//! stream, a decoder and a cue grammar, and nothing in between — no "play this
//! buffer once, panned" — so each sample writes its own voice queue, its own
//! mixer source and its own interleaved-stereo playhead. Writing it a fourth
//! time confirms it and adds one thing the third could not: the crate has no
//! answer for **too many** sounds either.
//!
//! The four copies now spell their entry point three different ways —
//! `play_panned(id, emitter_x)` in breakout, `play_at(id, listener_x, x, y)` in
//! flappy, `play_at(id, x, y)` in asteroids and `play_at(id, listener, x, y)`
//! here — and breakout still has no play counter at all. A duplication that
//! drifts is worse than one that does not: reading any one copy no longer tells
//! you what the pattern is.
//!
//! None of it is fixed here: growing the engine is what a sample is meant to
//! *reveal* the need for. It is owed by P10.

use std::sync::{Arc, Mutex};

use crcbl_audio::{AudioSample, AudioStream};
use glam::DVec3;

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

/// How many voices may be sounding at once.
///
/// See this module's header. Sixteen is about a third of a second of this
/// game's worst-case emission rate, which is long enough that a burst of kills
/// still reads as a burst and short enough that the audio thread's per-block
/// work stays bounded no matter what the simulation does.
pub const MAX_VOICES: usize = 16;

/// A procedural sound: interleaved stereo f32 samples.
#[derive(Debug)]
struct Sound {
    data: Vec<AudioSample>,
}

/// A playing voice with its own playhead.
///
/// `playhead` counts **frames**, not samples: `Sound::data` is interleaved
/// stereo, so a playhead advancing one *index* per output frame plays every
/// sound at half speed and twice the length. Breakout shipped that bug once and
/// all three later copies carry the comment rather than the bug.
#[derive(Debug)]
struct Voice {
    sound: Arc<Sound>,
    playhead: f64,
    volume: f32,
    pitch: f32,
    gain_l: f32,
    gain_r: f32,
}

impl Voice {
    /// Mixes this voice into `buffer` and reports whether it has audio left.
    fn render_block(&mut self, buffer: &mut [AudioSample]) -> bool {
        let data = &self.sound.data;
        let frames = data.len() / 2;
        // A ratio of 1.0 is one source frame per output frame. Non-finite or
        // non-positive ratios would stall the voice forever or index backwards.
        let step = if self.pitch.is_finite() && self.pitch > 0.0 {
            f64::from(self.pitch)
        } else {
            1.0
        };

        for out in buffer.chunks_exact_mut(2) {
            let frame = self.playhead as usize;
            if frame >= frames {
                return false;
            }
            out[0] += data[frame * 2] * self.volume * self.gain_l;
            out[1] += data[frame * 2 + 1] * self.volume * self.gain_r;
            self.playhead += step;
        }
        (self.playhead as usize) < frames
    }
}

/// Thread-safe voice queue. The game thread pushes, the audio thread drains.
#[derive(Debug)]
struct VoiceQueue {
    inner: Mutex<Vec<Voice>>,
}

/// The audio source fed to `AudioStream`, called from the audio thread.
struct MixerSource {
    queue: Arc<VoiceQueue>,
}

impl crcbl_audio::AudioSource for MixerSource {
    fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
        let mut voices = self.queue.inner.lock().unwrap_or_else(|e| e.into_inner());
        voices.retain_mut(|voice| voice.render_block(buffer));
    }
}

/// Owns the cues and the output stream.
#[derive(Debug)]
pub struct Audio {
    sounds: Vec<Arc<Sound>>,
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
    queue: Arc<VoiceQueue>,
    _stream: Option<AudioStream>,
}

/// The sample rate every cue is synthesised at.
///
/// The stream resamples nothing, so a cue built at one rate and played at
/// another is simply the wrong pitch. 48 kHz is what the other three samples
/// use and what every device this has run on reports.
const SAMPLE_RATE: u32 = 48_000;

impl Audio {
    pub fn new(headless: bool) -> Self {
        let queue = Arc::new(VoiceQueue {
            inner: Mutex::new(Vec::new()),
        });
        // Short high blip for the gun, a filtered noise burst for an enemy
        // coming apart, a brighter and shorter blip for a gem, a two-tone rise
        // for a level, and a long low burst for the player's own end. Every one
        // of them is shorter than the last sample's equivalents, because this
        // game raises far more of them: see `MAX_VOICES`.
        let sounds = vec![
            Arc::new(Sound {
                data: sine(760.0, 0.045, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: noise(0.14, 12.0, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: sine(1_320.0, 0.05, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: rise(440.0, 880.0, 0.30, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: noise(0.55, 4.0, SAMPLE_RATE),
            }),
        ];

        let source = MixerSource {
            queue: Arc::clone(&queue),
        };
        let stream = if headless {
            Some(AudioStream::open_null(source))
        } else {
            AudioStream::open(source)
        };
        if stream.is_none() && !headless {
            log::info!("audio: no output device available; the game will be silent");
        }

        Self {
            plays: vec![0; sounds.len()],
            sounds,
            dropped: 0,
            #[cfg(test)]
            played: Vec::new(),
            queue,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `at`, heard from `listener`.
    ///
    /// Both positions, unlike in the other three samples, because this game's
    /// listener moves: it is the player, and the camera follows them. See the
    /// module docs.
    pub fn play_at(&mut self, id: u32, listener: DVec3, at: DVec3) {
        // Ids are 1-based; `id - 1` on a `u32` underflows to `u32::MAX` for id
        // zero rather than simply missing the table.
        let Some(index) = id.checked_sub(1).map(|i| i as usize) else {
            log::debug!("audio: sound id 0 is not a sound");
            return;
        };
        let Some(sound) = self.sounds.get(index).map(Arc::clone) else {
            return;
        };
        // Counted before the cap, not after: the cue happened either way, and a
        // counter that only counted the audible ones could not tell a game that
        // never fired from one that fired into a full mixer.
        //
        // `plays` is as long as `sounds` and neither ever grows, so an index
        // that found a sound finds a counter.
        self.plays[index] += 1;
        #[cfg(test)]
        self.played.push((id, at.x, at.y));

        let cue = crcbl_audio::spatial::compute_cue(
            [0.0, 0.0, 0.0],
            [
                (at.x - listener.x) as f32,
                (at.y - listener.y) as f32,
                // A metre in front, so a sound at the listener's own position is
                // still at a defined direction rather than a zero vector.
                1.0,
            ],
            &crcbl_audio::spatial::CueGrammar::default(),
        );
        let mut voices = self.queue.inner.lock().unwrap_or_else(|e| e.into_inner());
        if voices.len() >= MAX_VOICES {
            drop(voices);
            self.dropped += 1;
            return;
        }
        voices.push(Voice {
            sound,
            playhead: 0.0,
            volume: cue.volume * 0.5,
            pitch: cue.pitch_ratio,
            gain_l: cue.gain_left,
            gain_r: cue.gain_right,
        });
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
        self.queue
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
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
        let value = 0.3 * phase.sin() * fade(i, frames);
        out.push(value);
        out.push(value);
    }
    out
}

/// A burst of low-passed noise that decays, as interleaved stereo.
///
/// `decay` is in nepers per second: `e^-9t` is down to a twentieth of its peak
/// by a fifth of a second. A kill wants a fast one and the player's own end
/// wants a slow one, which is why it is an argument here and a constant in
/// `apps/asteroids/src/audio.rs`.
///
/// Deterministic, from a fixed seed through the same splitmix64 mix the
/// simulation uses, so the sound a build ships is the sound every build ships
/// and a golden buffer would be possible later. The one-pole low pass takes the
/// hiss off the top; the exponential decay is what makes it a *burst*.
fn noise(seconds: f32, decay: f32, sample_rate: u32) -> Vec<AudioSample> {
    /// The one-pole coefficient: `y += ALPHA * (x - y)`. Lower is duller.
    const ALPHA: f32 = 0.16;

    let frames = (sample_rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    let mut state = 0x484F_5244_4553_4545_u64;
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
        let value = 0.45 * low * (-decay * t).exp() * fade(i, frames);
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

    /// The sound is interleaved stereo, so one output frame must advance the
    /// playhead by one *source frame*.
    #[test]
    fn a_voice_plays_at_the_rate_it_was_recorded() {
        let frames = 100;
        let sound = Arc::new(Sound {
            data: sine(440.0, frames as f32 / 48_000.0, 48_000),
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
        let mut buffer = vec![0.0f32; frames * 2];
        voice.render_block(&mut buffer);
        assert!(
            (voice.playhead - frames as f64).abs() < 1.0,
            "playhead at {} after {frames} frames",
            voice.playhead
        );
        assert_eq!(buffer[0], sound.data[0]);
        assert_eq!(buffer[2], sound.data[2], "every frame was played twice");
    }

    /// An id nothing answers to is ignored rather than underflowing or panicking.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = Audio::new(true);
        audio.play_at(0, DVec3::ZERO, DVec3::ZERO);
        audio.play_at(9999, DVec3::ZERO, DVec3::ZERO);
        assert_eq!(audio.voices(), 0);
        // `plays` shares `play_at`'s `id - 1`, so it has the same underflow to
        // avoid, and it must not report a play for a cue that was refused.
        assert_eq!(audio.plays(0), 0);
        assert_eq!(audio.plays(9999), 0);
        assert_eq!(audio.plays(SOUND_SHOT), 0);

        audio.play_at(SOUND_SHOT, DVec3::ZERO, DVec3::ZERO);
        assert_eq!(audio.voices(), 1);
        assert_eq!(audio.plays(SOUND_SHOT), 1);
        for other in [SOUND_KILL, SOUND_PICKUP, SOUND_LEVEL, SOUND_DEATH] {
            assert_eq!(audio.plays(other), 0, "only the shot was played");
        }
    }

    /// `plays` counts emissions, not the voices still sounding — the whole
    /// reason it exists. One that merely reported `voices()` would agree with
    /// this test right up until the audio thread reaped the voice.
    #[test]
    fn a_cue_stays_counted_after_its_voice_is_gone() {
        let mut audio = Audio::new(true);
        audio.play_at(SOUND_SHOT, DVec3::ZERO, DVec3::ZERO);
        assert_eq!(audio.plays(SOUND_SHOT), 1);

        // Reap it by hand rather than waiting on the audio thread, so the test
        // is not itself a race: `fill` is exactly what that thread calls.
        let source = MixerSource {
            queue: Arc::clone(&audio.queue),
        };
        let mut block = vec![0.0f32; 256 * 2];
        let start = std::time::Instant::now();
        while audio.voices() > 0 {
            assert!(
                start.elapsed().as_secs() < 5,
                "the shot voice never finished"
            );
            block.fill(0.0);
            crcbl_audio::AudioSource::fill(&source, &mut block, 48_000);
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
            audio.play_at(SOUND_KILL, DVec3::ZERO, DVec3::ZERO);
        }
        assert_eq!(audio.voices(), MAX_VOICES);
        assert_eq!(audio.plays(SOUND_KILL), MAX_VOICES as u64);
        assert_eq!(audio.dropped(), 0, "the cap fired early");

        for _ in 0..7 {
            audio.play_at(SOUND_KILL, DVec3::ZERO, DVec3::ZERO);
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
        let source = MixerSource {
            queue: Arc::clone(&audio.queue),
        };
        let mut block = vec![0.0f32; 48_000 * 2];
        crcbl_audio::AudioSource::fill(&source, &mut block, 48_000);
        assert_eq!(audio.voices(), 0, "a whole second did not drain the queue");
        audio.play_at(SOUND_KILL, DVec3::ZERO, DVec3::ZERO);
        assert_eq!(audio.voices(), 1);
    }

    /// The grammar is actually consulted: a cue away from the listener is not
    /// the same cue as one on top of it, **and the listener is the player**.
    ///
    /// Without this, `play_at` could ignore its coordinates entirely and every
    /// test above would still pass — the sample would ship "spatial audio" that
    /// was a constant. The second half is this game's own: an identical world
    /// position must sound different depending on where the player is standing,
    /// which is the half a copy of asteroids' listener-at-the-origin version
    /// would fail.
    #[test]
    fn where_a_cue_happens_and_where_the_player_stands_both_change_how_it_sounds() {
        let mut audio = Audio::new(true);
        let origin = DVec3::ZERO;
        audio.play_at(SOUND_KILL, origin, origin);
        audio.play_at(SOUND_KILL, origin, DVec3::new(-14.0, 6.0, 0.0));
        audio.play_at(SOUND_KILL, origin, DVec3::new(14.0, 6.0, 0.0));
        // The same emitter as the second, heard by a player standing right on
        // top of it.
        audio.play_at(
            SOUND_KILL,
            DVec3::new(-14.0, 6.0, 0.0),
            DVec3::new(-14.0, 6.0, 0.0),
        );
        let voices = audio.queue.inner.lock().expect("no other thread");
        let (near, left, right, moved) = (&voices[0], &voices[1], &voices[2], &voices[3]);
        assert!(
            left.gain_l > left.gain_r,
            "a cue to the left should be louder on the left: {} vs {}",
            left.gain_l,
            left.gain_r
        );
        assert!(
            right.gain_r > right.gain_l,
            "and one to the right, on the right: {} vs {}",
            right.gain_l,
            right.gain_r
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

    /// The kill is noise rather than a tone, decays, and is finite — and the
    /// decay argument is actually used, which a constant-decay copy would not
    /// show.
    #[test]
    fn the_kill_is_a_decaying_burst_of_noise_and_the_decay_is_a_knob() {
        let data = noise(0.14, 12.0, 48_000);
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
        let slow = noise(0.14, 1.2, 48_000);
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
