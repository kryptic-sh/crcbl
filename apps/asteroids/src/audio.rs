//! Audio for asteroids: three procedural cues through `crcbl-audio`'s spatial
//! grammar.
//!
//! The engine, the gun and the rocks coming apart, all synthesised at start-up —
//! this sample has no sound assets by design. The game thread pushes voices onto
//! a shared queue; the audio thread drains it.
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
//! # This is the *third* copy of this file, and the copies have drifted
//!
//! `docs/plan/ROADMAP.md`'s S1B finding 5 says `crcbl-audio` offers a device, a
//! stream, a decoder and a cue grammar, and nothing in between — no "play this
//! buffer once, panned" — so each sample writes its own voice queue, its own
//! mixer source and its own interleaved-stereo playhead. Writing it a third time
//! confirms that and adds three things the second copy could not show:
//!
//! * **The copies do not stay in step.** `apps/breakout/src/audio.rs` still
//!   spells its entry point `play_panned(id, emitter_x)` — one axis, no `y`, no
//!   listener argument — and has no play counter at all, so breakout's cues
//!   cannot be asserted about; `apps/flappy/src/audio.rs` has `play_at`, a `y`,
//!   a listener and [`Audio::plays`]. They were the same file and are not any
//!   more. Nothing brought the counter back to breakout, because nothing links
//!   the two. A duplication that drifts is worse than one that does not: reading
//!   either copy no longer tells you what the pattern *is*.
//! * **There is no listener anywhere in the crate.**
//!   [`compute_cue`](crcbl_audio::spatial::compute_cue) takes the listener's
//!   position as an argument on every single call, so "where the ears are" is a
//!   convention each game invents and re-derives at each call site. Three games,
//!   three conventions, and two of them undocumented until this file said so.
//! * **A held sound has no representation at all.** Thrust is the first cue in
//!   any sample that is *sustained* rather than an edge — the player holds the
//!   key — and the crate has one-shot voices and nothing else: no looping voice,
//!   no start/stop handle, no way to say "this is playing until I say
//!   otherwise". So [`SOUND_THRUST`] is faked as a one-shot re-fired on a timer
//!   the *simulation* owns (`game::THRUST_CUE_PERIOD`), which puts an audio
//!   implementation detail inside the deterministic tick. That is the one thing
//!   here that is not merely duplication.
//!
//! None of it is fixed here: growing the engine is what a sample is meant to
//! *reveal* the need for. It is owed by P10.

use std::sync::{Arc, Mutex};

use crcbl_audio::{AudioSample, AudioStream};

/// The engine, while thrust is held. Re-fired on a timer; see the module docs.
pub const SOUND_THRUST: u32 = 1;
/// A shot leaving the gun.
pub const SOUND_SHOT: u32 = 2;
/// A rock coming apart, or the ship doing the same.
pub const SOUND_EXPLOSION: u32 = 3;

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
/// both later copies carry the comment rather than the bug.
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
    /// number falls again on a clock nothing here controls. Flappy had to add
    /// this to test its two cues; breakout still has no equivalent.
    plays: Vec<u64>,
    /// Every `(id, x, y)` handed to [`Audio::play_at`], in order.
    ///
    /// **The only place a cue's world position still exists as a position.**
    /// `play_at` turns it into a pan and a volume immediately, and the game
    /// drains its cue queue inside the same `Game::tick` that filled it, so a
    /// test asking "was the shot heard where the gun is" has nothing else to
    /// read. Test-only: a shipped build has no reason to keep the list.
    #[cfg(test)]
    played: Vec<(u32, f64, f64)>,
    queue: Arc<VoiceQueue>,
    _stream: Option<AudioStream>,
}

/// The sample rate every cue is synthesised at.
///
/// The stream resamples nothing, so a cue built at one rate and played at
/// another is simply the wrong pitch. 48 kHz is what the other two samples use
/// and what every device this has run on reports.
const SAMPLE_RATE: u32 = 48_000;

impl Audio {
    pub fn new(headless: bool) -> Self {
        let queue = Arc::new(VoiceQueue {
            inner: Mutex::new(Vec::new()),
        });
        // A low pulse for the engine, a short high blip for the gun, and a
        // filtered noise burst for a rock coming apart. The thrust cue is
        // deliberately a shade shorter than `game::THRUST_CUE_PERIOD`, so a held
        // key is a pulsing engine rather than a stack of overlapping voices that
        // grows for as long as the player holds it.
        let sounds = vec![
            Arc::new(Sound {
                data: sine(110.0, 0.10, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: sine(900.0, 0.05, SAMPLE_RATE),
            }),
            Arc::new(Sound {
                data: noise(0.32, SAMPLE_RATE),
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
            #[cfg(test)]
            played: Vec::new(),
            queue,
            _stream: stream,
        }
    }

    /// Plays a cue for something happening at `(x, y)` in world space.
    ///
    /// The listener is the camera, at the origin — see the module docs. There is
    /// no listener argument because, unlike in flappy, there is nothing for it
    /// to vary with: this camera does not move.
    pub fn play_at(&mut self, id: u32, x: f64, y: f64) {
        // Ids are 1-based; `id - 1` on a `u32` underflows to `u32::MAX` for id
        // zero rather than simply missing the table.
        let Some(index) = id.checked_sub(1).map(|i| i as usize) else {
            log::debug!("audio: sound id 0 is not a sound");
            return;
        };
        let Some(sound) = self.sounds.get(index).map(Arc::clone) else {
            return;
        };
        let cue = crcbl_audio::spatial::compute_cue(
            [0.0, 0.0, 0.0],
            [
                x as f32, y as f32,
                // A metre in front, so a sound at the listener's own position is
                // still at a defined direction rather than a zero vector.
                1.0,
            ],
            &crcbl_audio::spatial::CueGrammar::default(),
        );
        self.queue
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Voice {
                sound,
                playhead: 0.0,
                volume: cue.volume * 0.5,
                pitch: cue.pitch_ratio,
                gain_l: cue.gain_left,
                gain_r: cue.gain_right,
            });
        // `plays` is as long as `sounds` and neither ever grows, so an index
        // that found a sound finds a counter.
        self.plays[index] += 1;
        #[cfg(test)]
        self.played.push((id, x, y));
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
        self.queue
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
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
        audio.play_at(0, 0.0, 0.0);
        audio.play_at(9999, 0.0, 0.0);
        assert_eq!(audio.voices(), 0);
        // `plays` shares `play_at`'s `id - 1`, so it has the same underflow to
        // avoid, and it must not report a play for a cue that was refused.
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
        let voices = audio.queue.inner.lock().expect("no other thread");
        let (near, left, right) = (&voices[0], &voices[1], &voices[2]);
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
