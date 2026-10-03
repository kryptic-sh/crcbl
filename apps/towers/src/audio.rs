//! Audio for towers: a procedural sound per [`Sound`] through `crcbl-audio`'s
//! spatial grammar and its mixer.
//!
//! [`crate::cue`] says what happened and where; this file says what that
//! sounds like and plays it from there. The waveforms are synthesised at
//! start-up — this sample has no sound assets yet — and banked in a
//! [`SoundBank`]; the game thread plays voices into a [`Mixer`] the audio
//! thread, or a browser's `AudioWorklet`, fills from.
//!
//! # Where the listener stands: on the camera the frame is drawn from
//!
//! **At the eye, facing what it looks at** — [`Audio::hear_from`], called
//! every frame with the camera the frame is drawn through. On the overhead
//! view that is high over the near edge looking down the field, so a tower
//! firing on the left of the picture is heard on the left, and a creep
//! leaking at the far end is further off than one dying in front. The
//! breakout-and-horde convention — an ear stood off a flat play plane — has no
//! meaning for a camera that looks down at an angle, and
//! [`Listener::facing`](crcbl::audio::spatial::Listener::facing) is what the
//! grammar offers for exactly this.
//!
//! **The dev camera takes the ear with it**: flying or walking, the field is
//! heard from where it is seen. That is the one convention that never has the
//! picture and the sound disagree.
//!
//! # A voice budget, as horde has
//!
//! A full field fires several shots a second per tower, and a splash burst
//! hits every creep in its reach at once, so a late wave raises dozens of cues
//! a second. [`MAX_VOICES`] is the mixer's budget and [`priority`] ranks the
//! sounds: the routine shots and hits at the bottom, kills and builds above,
//! a life lost and a wave starting above those, and the run's end at the top.
//! A cue still counts as **played** whatever the budget did with it — see
//! [`Audio::plays`].
//!
//! # Headless plays nothing
//!
//! A headless run, a `--frames` run and a dedicated server build no `Audio`
//! at all (see `crate::app`): no device is opened and no cue is played,
//! because the cues are presentation and a run with nobody listening has none
//! to present.

use std::sync::Arc;

use crcbl::audio::AudioStream;
use crcbl::audio::mixer::{Mixer, SoundBank, VoiceMix};
use crcbl::audio::spatial::Listener;
use crcbl::audio::synth;
use crcbl::render::Camera;

use crate::cue::{Cue, Sound};
use crate::tower;

/// How loud a cue is against the volume the grammar asks for. See breakout's.
const MASTER_GAIN: f32 = 0.5;

/// How many voices may sound at once: the [`Mixer`]'s voice budget.
///
/// Horde's, for horde's reason: about a third of a second of the busiest
/// field's cues, long enough that a burst of hits reads as a burst and short
/// enough that the audio thread's per-block work stays bounded whatever the
/// field does.
pub const MAX_VOICES: usize = 16;

/// The sample rate every sound is synthesised at — the mixer's own.
const SAMPLE_RATE: u32 = crcbl::audio::INTERNAL_SAMPLE_RATE;

/// The seed the noise sounds are drawn from. Spells "TOWERSEE".
///
/// [`synth::noise_burst`] is deterministic from it, so the sound this build
/// ships is the sound every build ships.
const NOISE_SEED: u64 = 0x544F_5745_5253_4545;

/// The settings directory this sample reads its volumes out of — the same
/// spelling `crate::gpu` hands its context's label, because it is the same
/// directory.
const APP_NAME: &str = "towers";

/// How much `sound` matters when the voice budget is full. See the module
/// docs.
#[must_use]
pub const fn priority(sound: Sound) -> u8 {
    match sound {
        Sound::Won | Sound::Lost => 3,
        Sound::Leak | Sound::Wave => 2,
        Sound::Burst | Sound::Kill | Sound::Build | Sound::Upgrade | Sound::Refused => 1,
        Sound::Fire(_) | Sound::Hit => 0,
    }
}

/// The id `sound` is banked at: one more than its index, so no sound is id
/// zero.
const fn id(sound: Sound) -> u32 {
    sound.index() as u32 + 1
}

/// The waveform `sound` plays.
///
/// Tones for the things a player does and the towers do, each kind of tower
/// in its own register so a field can be told apart by ear; noise for the
/// things that come apart. Every routine sound is short, because the field
/// raises a great many of them; the run's end is long, because it is heard
/// once.
fn waveform(sound: Sound) -> Vec<crcbl::audio::AudioSample> {
    match sound {
        Sound::Fire(tower::Kind::Bolt) => synth::sine(880.0, 0.04, SAMPLE_RATE),
        Sound::Fire(tower::Kind::Splash) => synth::sine(330.0, 0.07, SAMPLE_RATE),
        Sound::Fire(tower::Kind::Slow) => synth::sine(523.0, 0.10, SAMPLE_RATE),
        Sound::Burst => synth::noise_burst(0.18, 10.0, NOISE_SEED, SAMPLE_RATE),
        Sound::Hit => synth::sine(1_244.0, 0.03, SAMPLE_RATE),
        Sound::Kill => synth::noise_burst(0.12, 14.0, NOISE_SEED, SAMPLE_RATE),
        Sound::Leak => synth::sine(196.0, 0.25, SAMPLE_RATE),
        Sound::Wave => synth::sine(440.0, 0.30, SAMPLE_RATE),
        Sound::Build => synth::sine(660.0, 0.08, SAMPLE_RATE),
        Sound::Upgrade => synth::sine(988.0, 0.10, SAMPLE_RATE),
        Sound::Refused => synth::sine(147.0, 0.12, SAMPLE_RATE),
        Sound::Won => synth::sine(784.0, 0.60, SAMPLE_RATE),
        Sound::Lost => synth::noise_burst(0.70, 3.0, NOISE_SEED, SAMPLE_RATE),
    }
}

/// Owns the sounds, the mixer and the output stream.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    /// How many times each sound has been **played**, indexed by
    /// [`Sound::index`]. Monotonic: the budget refusing or cutting a voice
    /// short does not take a play back, and the audio thread reaping a
    /// finished voice does not either.
    plays: [u64; Sound::ALL.len()],
    /// Every cue played, in order — the only place a cue's world position
    /// survives being turned into a pan and a volume, for the tests that ask
    /// where a sound was heard.
    #[cfg(test)]
    played: Vec<Cue>,
    mixer: Arc<Mixer>,
    _stream: Option<AudioStream>,
}

impl Audio {
    /// Opens the default output device — natively a real-time stream, in a
    /// browser the page's `AudioWorklet`, which the page starts on the first
    /// click or key press as it does for every demo.
    ///
    /// The player's `[engine.audio]` volumes are read from their settings
    /// file. A machine with no device plays nothing and says so once.
    #[must_use]
    pub fn open() -> Self {
        let audio = Self::silent(crcbl::engine::SettingsSource::Platform);
        let stream = AudioStream::open(Arc::clone(&audio.mixer));
        if stream.is_none() {
            crcbl::log::info!("audio: no output device available; the field will be silent");
        }
        Self {
            _stream: stream,
            ..audio
        }
    }

    /// The sounds and the mixer with no stream at all: what this crate's
    /// tests play through and read back from, filling the mixer by hand.
    #[cfg(test)]
    #[must_use]
    pub fn offline() -> Self {
        Self::silent(crcbl::engine::SettingsSource::None)
    }

    /// Everything but the stream, with the volumes `settings` holds.
    fn silent(settings: crcbl::engine::SettingsSource<'_>) -> Self {
        let mut bank = SoundBank::new();
        for sound in Sound::ALL {
            bank.insert(id(sound), waveform(sound));
        }
        let mixer = Arc::new(Mixer::new());
        mixer.set_voice_budget(Some(MAX_VOICES));
        // Before the first cue: a voice started against the default gains keeps
        // them, so it would be the one sound the player's settings missed.
        settings.apply_audio_gains(APP_NAME, &mixer);
        let audio = Self {
            bank,
            plays: [0; Sound::ALL.len()],
            #[cfg(test)]
            played: Vec::new(),
            mixer,
            _stream: None,
        };
        // The overhead view, until the first frame says otherwise: a cue is
        // never computed against the mixer's default ear at the origin, which
        // stands on the field itself.
        audio.hear_from(&crate::camera::camera());
        audio
    }

    /// Puts the ear on `camera`: at its eye, facing what it looks at. See the
    /// module docs.
    pub fn hear_from(&self, camera: &Camera) {
        self.mixer.set_listener(Listener::facing(
            camera.eye.to_array(),
            (camera.target - camera.eye).to_array(),
        ));
    }

    /// Moves `bus`'s gain on the mixer already playing — what
    /// [`crcbl::engine::HostedGame::set_bus_gain`] forwards to, so a volume
    /// typed at the debug console is heard on the voices sounding now.
    pub fn set_bus_gain(&self, bus: crcbl::audio::mixer::Bus, gain: f32) {
        self.mixer.set_bus_gain(bus, gain);
    }

    /// Plays `cue`'s sound from where it happened, heard from the camera
    /// [`Audio::hear_from`] last put the ear on.
    pub fn play(&mut self, cue: Cue) {
        let Some(voice) = self.bank.create_voice(id(cue.sound)) else {
            // Every sound is banked in `silent`, so this is a sound added to
            // `Sound::ALL` without a waveform — said, not played as another.
            crcbl::log::warn!("audio: no sound banked for {}", cue.sound.label());
            return;
        };
        self.plays[cue.sound.index()] += 1;
        #[cfg(test)]
        self.played.push(cue);
        let at = cue.at.as_vec3().to_array();
        let spatial = self.mixer.cue(at);
        self.mixer.play(
            voice
                .with_mix(VoiceMix {
                    volume: spatial.volume * MASTER_GAIN,
                    ..VoiceMix::from(&spatial)
                })
                .with_priority(priority(cue.sound)),
        );
    }

    /// How many times `sound` has been played. See [`Audio::plays`]'s field.
    #[must_use]
    pub fn plays(&self, sound: Sound) -> u64 {
        self.plays[sound.index()]
    }

    /// How many cues have been played, every sound together.
    #[must_use]
    pub fn played_total(&self) -> u64 {
        self.plays.iter().sum()
    }

    /// Every cue played so far, in order.
    #[cfg(test)]
    #[must_use]
    pub fn played(&self) -> &[Cue] {
        &self.played
    }

    /// The mixer the cues are played into, for this crate's tests to fill.
    #[cfg(test)]
    #[must_use]
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }
}

/// The panel's audio section: how many cues the field raised, and what the
/// voice budget did with them — a silence the player notices is attributable
/// here rather than a mystery to debug by ear.
impl crcbl::ui::DebugModule for Audio {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("audio");
        section.row("cues", format_args!("{}", self.played_total()));
        section.row("voices", format_args!("{}", self.mixer.voice_count()));
        section.row("dropped", format_args!("{}", self.mixer.refused_count()));
        section.row("stolen", format_args!("{}", self.mixer.stolen_count()));
    }
}

#[cfg(test)]
mod tests;
