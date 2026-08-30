//! Something to hear, so a fader is a level rather than a number.
//!
//! The rest of this sample proves that a gain reaches a *key*: the screen writes
//! `[engine.audio]`, `SAVE` writes the file and the next start reads it back.
//! What it could not show is that the key reaches a **gain stage**, because
//! nothing in the process made a sound. This module is that half.
//!
//! # Three buses, three kinds of content
//!
//! `docs/plan/sample/20-options.md`'s scope asks for "a music loop, a repeating
//! effect, a UI click on the widgets themselves", on the ground that "three
//! buses with obviously different content is the minimum that makes a mixer
//! legible". They are:
//!
//! | Bus | Content | How it behaves |
//! | --- | --- | --- |
//! | [`Bus::Music`] | a looping tone at [`BED_HZ`] | continuous, from start-up |
//! | [`Bus::Sfx`] | a noise tick every [`TICK_PERIOD`] seconds | periodic |
//! | [`Bus::Ui`] | a shorter, brighter noise click | only when a widget is used |
//!
//! Different in *behaviour*, not only in timbre — which is what makes the bus a
//! player pulls down identifiable by ear without reading the screen.
//! [`Bus::Master`] multiplies all three, and that is the routing being visible
//! too: pulling MASTER down takes the tick and the drone with it, pulling MUSIC
//! down leaves the tick where it was.
//!
//! # The two buses with nothing on them
//!
//! [`Bus::Voice`] and [`Bus::Ambience`] have no content here and their faders
//! move nothing you can hear. That is the plan's minimum honoured rather than
//! exceeded, and its exit criteria want a control with no implementation to
//! *say so* — so [`Audio::sounds`] answers which buses carry content and
//! `crate::app`'s frame writes the answer beside the groove.
//!
//! # Why the bed starts before the first frame
//!
//! [`Mixer::set_bus_gain`] takes effect on the next block for voices already
//! playing, so the order of "start the bed" against "apply the player's gains"
//! cannot make the bed permanently wrong — but it can make it *briefly* loud.
//! A player who saved MASTER at a tenth would hear one block at full level on
//! every start, which is exactly the setting they set in order not to hear.
//! [`Audio::new`] therefore applies all six gains before it plays anything.

use std::sync::Arc;

use crcbl::audio::AudioStream;
use crcbl::audio::mixer::{Bus, Mixer, SoundBank};
use crcbl::audio::synth;
use crcbl::ui::menu::Slider;

/// The music bed: one looping tone, playing for as long as the sample runs.
pub const SOUND_BED: u32 = 1;
/// The effects tick, raised every [`TICK_PERIOD`] seconds of simulated time.
pub const SOUND_TICK: u32 = 2;
/// The interface click, raised when a widget is used.
pub const SOUND_CLICK: u32 = 3;

/// How many cue ids there are, which is what [`Audio::plays`] counts.
const CUE_COUNT: usize = 3;

/// The rate every waveform here is synthesised at.
///
/// The mixer resamples, so this is the generator's rate and not the device's.
const SAMPLE_RATE: u32 = 48_000;

/// The music bed's pitch — A3, low enough to sit under the tick.
pub const BED_HZ: f32 = 220.0;
/// How long the bed's buffer is before it repeats.
///
/// [`synth::looped_sine`] takes whole cycles rather than seconds precisely so
/// the seam lands at phase zero, so the cycle count is derived from this and
/// [`BED_HZ`] rather than written down beside it.
const BED_SECONDS: f32 = 1.0;
/// The bed's own volume, under its bus and the master.
///
/// A sustained tone at the tick's level would be the only thing anyone heard;
/// this is the drone sitting under the transients rather than in front of them.
const BED_VOLUME: f32 = 0.35;

/// How long the effects tick lasts.
const TICK_SECONDS: f32 = 0.06;
/// Its decay, in nepers per second — see [`synth::noise_burst`].
const TICK_DECAY: f32 = 30.0;
/// The seed its noise is drawn from. Any value; a different one is a
/// different-sounding tick rather than a wrong one.
const TICK_SEED: u64 = 0x5449_434b_u64;
/// How much simulated time passes between two effects ticks.
pub const TICK_PERIOD: f64 = 1.0;
/// The tick's own volume, under its bus and the master.
const TICK_VOLUME: f32 = 0.7;

/// How long the interface click lasts — shorter and brighter than the tick, so
/// the two are not the same sound on two buses.
const CLICK_SECONDS: f32 = 0.02;
/// Its decay, in nepers per second.
const CLICK_DECAY: f32 = 90.0;
/// The seed its noise is drawn from.
const CLICK_SEED: u64 = 0x0043_4c49_434b;
/// The click's own volume, under its bus and the master.
const CLICK_VOLUME: f32 = 0.6;
/// How wide a detent is on the groove — the spacing of the notches a fader
/// clicks on.
///
/// A fader under a pointer moves every frame, by whatever fraction of the
/// groove the pointer moved, so clicking on every change would be a per-frame
/// buzz rather than a click. A real fader clicks per *detent*, and the detent
/// here is the keyboard's own step, so one arrow press is one click.
///
/// Measured in handle position rather than in gain because that is where the
/// pointer and the key both live — [`crate::menu::gain_at`] is a square, so a
/// notch in gain would be wide at the top of the groove and invisible at the
/// bottom.
const CLICK_NOTCH: f32 = Slider::KEY_STEP;

/// Which detent `position` sits in.
///
/// **Rounded to an index rather than compared as a distance**, which is the
/// difference between a fader that clicks on every arrow press and one that
/// clicks on most of them: a key steps the handle by exactly [`CLICK_NOTCH`] in
/// `f32`, and the subtraction that would measure it back lands a rounding error
/// *below* the step about as often as above. An index cannot be off by a
/// rounding error, because the rounding is the operation.
fn notch_of(position: f32) -> i32 {
    // `as` saturates, so a non-finite position — which `Slider` already refuses,
    // but this function does not get to assume — lands at an end rather than
    // wrapping.
    (position / CLICK_NOTCH).round() as i32
}

/// The cues, the mixer they play into, and the stream that drains it.
#[derive(Debug)]
pub struct Audio {
    bank: SoundBank,
    mixer: Arc<Mixer>,
    stream: Option<AudioStream>,
    /// Whether this run asked for a device at all, which is what tells a silent
    /// headless run from a windowed one that found no output. See
    /// [`Audio::output`].
    headless: bool,
    /// Simulated seconds since the last effects tick.
    ///
    /// Simulated rather than wall-clock because [`Audio::advance`] is called
    /// from the fixed tick, so a paused loop stops the metronome — which is the
    /// behaviour a player expects and, incidentally, what makes the count below
    /// reproducible in a test.
    since_tick: f64,
    /// How many times each cue has been **emitted**, indexed as `id - 1`.
    ///
    /// Only ever increases. [`Audio::voices`] cannot answer "did this cue
    /// play?" — the audio thread reaps a voice as it finishes, so that number
    /// falls again on a clock nothing here controls.
    plays: [u64; CUE_COUNT],
    /// Which detent each fader was last seen in, in [`Bus::ALL`]'s order.
    /// See [`notch_of`].
    notches: [i32; Bus::ALL.len()],
}

impl Audio {
    /// Banks the cues, applies `gains`, and starts the music bed.
    ///
    /// `gains` is the screen's own array in [`Bus::ALL`]'s order — the gains it
    /// opened the player's file on — rather than a second read of that file.
    /// Two readers of one file is how a screen and its mixer come to disagree
    /// about what the player set.
    #[must_use]
    pub fn new(headless: bool, gains: &[f32; Bus::ALL.len()]) -> Self {
        let mut bank = SoundBank::new();
        bank.insert(
            SOUND_BED,
            synth::looped_sine(BED_HZ, (BED_HZ * BED_SECONDS) as u32, SAMPLE_RATE),
        );
        bank.insert(
            SOUND_TICK,
            synth::noise_burst(TICK_SECONDS, TICK_DECAY, TICK_SEED, SAMPLE_RATE),
        );
        bank.insert(
            SOUND_CLICK,
            synth::noise_burst(CLICK_SECONDS, CLICK_DECAY, CLICK_SEED, SAMPLE_RATE),
        );

        let mixer = Arc::new(Mixer::new());
        // Before the bed and before the stream. See the module docs.
        for (bus, gain) in Bus::ALL.into_iter().zip(gains) {
            mixer.set_bus_gain(bus, *gain);
        }

        let stream = if headless {
            Some(AudioStream::open_null(Arc::clone(&mixer)))
        } else {
            AudioStream::open(Arc::clone(&mixer))
        };
        if stream.is_none() {
            crcbl::log::info!("audio: no output device available; the faders will be silent");
        }

        let mut audio = Self {
            bank,
            mixer,
            stream,
            headless,
            since_tick: 0.0,
            plays: [0; CUE_COUNT],
            notches: std::array::from_fn(|i| notch_of(crate::menu::handle_at(gains[i]))),
        };
        audio.play(SOUND_BED, Bus::Music, BED_VOLUME, true);
        audio
    }

    /// Moves `bus`'s gain stage to `gain`.
    ///
    /// Called from the one place the screen changes a gain, so what a player
    /// hears and what `SAVE` would write cannot come apart. The bed is already
    /// playing and gets quieter with the fader rather than on the next cue —
    /// see [`Mixer::set_bus_gain`], which is the whole reason a bus exists.
    pub fn set_bus_gain(&self, bus: Bus, gain: f32) {
        self.mixer.set_bus_gain(bus, gain);
    }

    /// Advances the metronome by `dt` simulated seconds, raising a tick each
    /// time it crosses [`TICK_PERIOD`].
    ///
    /// A `dt` longer than the period raises **one** tick and drops the ones it
    /// missed, keeping only the remainder: a loop resumed after a long stall
    /// would otherwise fire every tick it slept through at once, which is a
    /// noise nobody asked for.
    pub fn advance(&mut self, dt: f64) {
        self.since_tick += dt;
        if self.since_tick >= TICK_PERIOD {
            self.since_tick %= TICK_PERIOD;
            self.play(SOUND_TICK, Bus::Sfx, TICK_VOLUME, false);
        }
    }

    /// Raises the interface click — a button was pressed.
    pub fn click(&mut self) {
        self.play(SOUND_CLICK, Bus::Ui, CLICK_VOLUME, false);
    }

    /// A fader was dragged or stepped to `position`; clicks if that is a
    /// different detent from the one it was in.
    pub fn fader_moved(&mut self, bus: Bus, position: f32) {
        let notch = notch_of(position);
        if notch != self.notches[bus.index()] {
            self.notches[bus.index()] = notch;
            self.click();
        }
    }

    /// A fader was *placed* at `position` — by `RESET`, or by the first frame
    /// walking the handles down to the player's file — rather than moved by
    /// hand.
    ///
    /// Silent, and it carries the detent with the handle. Without it a reset
    /// from the bottom of the groove to unity would leave the fader recorded in
    /// a detent it is nowhere near, and the next drag would click on its very
    /// first frame.
    pub fn fader_placed(&mut self, bus: Bus, position: f32) {
        self.notches[bus.index()] = notch_of(position);
    }

    /// Whether `bus` carries anything audible in this sample.
    ///
    /// False for [`Bus::Voice`] and [`Bus::Ambience`], whose faders move a key
    /// and nothing else. The screen says so on the row rather than leaving a
    /// player to wonder whether their audio is broken.
    #[must_use]
    pub const fn sounds(bus: Bus) -> bool {
        matches!(bus, Bus::Master | Bus::Music | Bus::Sfx | Bus::Ui)
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

    /// How many voices are **currently sounding**.
    ///
    /// Never zero while the bed is looping, which is the cheapest way to see
    /// from outside that the mixer is still running.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.mixer.voice_count()
    }

    /// `bus`'s gain as the mixer holds it — the number the samples are actually
    /// multiplied by, rather than the one the screen believes it set.
    #[must_use]
    pub fn bus_gain(&self, bus: Bus) -> f32 {
        self.mixer.bus_gain(bus)
    }

    /// Where this run's audio is going, for the panel.
    #[must_use]
    pub const fn output(&self) -> &'static str {
        match (self.headless, self.stream.is_some()) {
            (true, _) => "null (headless)",
            (false, true) => "device",
            (false, false) => "none",
        }
    }

    /// Plays `id` on `bus`, counting the emission.
    fn play(&mut self, id: u32, bus: Bus, volume: f32, looping: bool) {
        let Some(voice) = self.bank.create_voice(id) else {
            crcbl::log::debug!("audio: no sound registered at id {id}");
            return;
        };
        let voice = voice.with_bus(bus).with_volume(volume);
        self.mixer
            .play(if looping { voice.with_looping() } else { voice });
        if let Some(count) = id
            .checked_sub(1)
            .and_then(|i| self.plays.get_mut(i as usize))
        {
            *count += 1;
        }
    }
}

/// The panel's audio section: what each cue has done, and where it went.
///
/// The rows are this sample's own facts rather than the other samples'. Flappy's
/// section counts two gameplay cues; horde's counts what its voice cap refused.
/// What matters here is whether a **bus** is a gain stage, so the section
/// carries the two ticking counters and the master the screen is multiplying
/// through — the number a reader compares against the fader they just moved.
impl crcbl::ui::DebugModule for Audio {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("audio");
        section.row("output", format_args!("{}", self.output()));
        section.row("ticks", format_args!("{}", self.plays(SOUND_TICK)));
        section.row("clicks", format_args!("{}", self.plays(SOUND_CLICK)));
        section.row("voices", format_args!("{}", self.voices()));
        section.row(
            "master gain",
            format_args!("{}", crate::menu::percent(self.bus_gain(Bus::Master))),
        );
    }
}

/// The one seam this sample can apply a settings key through.
///
/// `crcbl::settings::apply` writes the key and then hands the write to a
/// [`Stage`](crcbl::settings::Stage); this is options' answer to that. The gain
/// rows move a bus **as they move**, which is the claim the audio half of the
/// sample exists to make; every video row inherits the trait's default and
/// reports `Unsupported`, which is exactly what `crate::app`'s docs already say
/// about them — this screen draws no scene and builds no loop, so there is no
/// renderer and no clock here to put one into force on.
impl crcbl::settings::Stage for Audio {
    fn set_bus_gain(&mut self, bus: Bus, gain: f32) -> Result<(), crcbl::settings::Unsupported> {
        Self::set_bus_gain(self, bus, gain);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bus at unity, which is what an empty settings file means.
    fn unity() -> [f32; Bus::ALL.len()] {
        [1.0; Bus::ALL.len()]
    }

    /// A headless [`Audio`], which takes the null stream rather than a device.
    fn headless(gains: &[f32; Bus::ALL.len()]) -> Audio {
        Audio::new(true, gains)
    }

    /// The bed is banked and playing before anything else happens.
    ///
    /// `play` returns early and counts nothing for an id the bank does not
    /// answer to, so a non-zero count is the whole chain — synthesised,
    /// inserted, long enough for `create_voice`, routed and playing.
    #[test]
    fn the_music_bed_is_sounding_from_the_first_moment() {
        let audio = headless(&unity());
        assert_eq!(audio.plays(SOUND_BED), 1, "the bed never reached the mixer");
        assert_eq!(audio.voices(), 1, "the bed is not holding a voice");
    }

    /// **The gains a screen opened on are the mixer's, every bus of them.**
    ///
    /// Not the ordering against the bed — nothing here can see inside `new`,
    /// and the module docs argue that half rather than asserting it. What this
    /// does catch is the failure that ordering argument is a refinement of: a
    /// player's saved level never reaching the gain stage at all, which is the
    /// whole sample silently proving nothing.
    #[test]
    fn a_saved_gain_is_the_gain_stage_from_the_start() {
        let mut gains = unity();
        gains[Bus::Music.index()] = 0.25;
        gains[Bus::Master.index()] = 0.5;
        let audio = headless(&gains);

        for (bus, gain) in Bus::ALL.into_iter().zip(gains) {
            assert!(
                (audio.bus_gain(bus) - gain).abs() < f32::EPSILON,
                "{bus:?} opened at {} rather than {gain}",
                audio.bus_gain(bus),
            );
        }
    }

    /// One block of the mix, as the audio thread would take it, measured.
    ///
    /// A block rather than a sample: a sine crosses zero, so any one sample is
    /// as likely to be silence at full gain as at a tenth. Long enough to cover
    /// several cycles of the bed, so the figure does not move with wherever the
    /// null stream's own thread has left the playhead.
    fn block_rms(audio: &Audio) -> f32 {
        use crcbl::audio::AudioSource as _;

        let mut block = vec![0.0f32; 4096];
        audio.mixer.fill(&mut block, SAMPLE_RATE);
        (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt()
    }

    /// **The bus is a gain stage and not a number.**
    ///
    /// Every other test here reads a gain back from wherever it was written,
    /// which a mixer that stored the number and multiplied by something else
    /// would pass. This one reads the samples: the same bed, one block apart,
    /// with nothing changed but the fader.
    #[test]
    fn pulling_the_music_bus_down_makes_the_mix_quieter() {
        let audio = headless(&unity());
        let loud = block_rms(&audio);
        assert!(loud > 0.0, "the bed is writing silence at unity");

        audio.set_bus_gain(Bus::Music, 0.25);
        let quiet = block_rms(&audio);
        assert!(
            quiet < loud * 0.5,
            "a quarter of the gain measured {quiet} against {loud} at unity",
        );

        audio.set_bus_gain(Bus::Music, 1.0);
        assert!(
            block_rms(&audio) > loud * 0.5,
            "the bed did not come back when the fader did",
        );
    }

    /// A moved fader moves the stage the voices are multiplied by, not a copy.
    #[test]
    fn moving_a_bus_moves_what_the_mixer_multiplies_by() {
        let audio = headless(&unity());
        audio.set_bus_gain(Bus::Sfx, 0.125);
        assert!((audio.bus_gain(Bus::Sfx) - 0.125).abs() < f32::EPSILON);
        assert!(
            (audio.bus_gain(Bus::Music) - 1.0).abs() < f32::EPSILON,
            "moving one bus moved another",
        );
    }

    /// One tick per period, and a stall that slept through several raises one.
    #[test]
    fn the_metronome_raises_one_tick_a_period_and_drops_the_ones_it_slept() {
        let mut audio = headless(&unity());
        assert_eq!(audio.plays(SOUND_TICK), 0, "a tick before any time passed");

        // Two halves of a period: the first raises nothing, the second raises
        // one — which is the accumulator being an accumulator rather than a
        // comparison against `dt` alone.
        audio.advance(TICK_PERIOD / 2.0);
        assert_eq!(audio.plays(SOUND_TICK), 0);
        audio.advance(TICK_PERIOD / 2.0);
        assert_eq!(audio.plays(SOUND_TICK), 1);

        // Three periods in one step is still one tick.
        audio.advance(TICK_PERIOD * 3.0);
        assert_eq!(
            audio.plays(SOUND_TICK),
            2,
            "a long step fired the periods it slept through",
        );
    }

    /// **Every arrow press is a click, the whole way down the groove.**
    ///
    /// One press is not enough to ask, and asking it that way is what made an
    /// earlier version of this test green over the wrong implementation: the
    /// first step down from unity happens to land a rounding error *over* a
    /// whole [`CLICK_NOTCH`], so a distance comparison passes it and then drops
    /// presses further down, where the sum rounds under instead. Walking the
    /// groove is what shows that; [`notch_of`] is why it cannot happen.
    #[test]
    fn every_keyboard_step_down_the_groove_is_a_click() {
        let mut audio = headless(&unity());
        // `Slider::nudge`'s arithmetic: a step off the current position, put
        // back through the widget's own clamp.
        let mut position = 1.0f32;
        let mut expected = audio.plays(SOUND_CLICK);
        while position > 0.0 {
            position = Slider::new(position - Slider::KEY_STEP).position();
            expected += 1;
            audio.fader_moved(Bus::Music, position);
            assert_eq!(
                audio.plays(SOUND_CLICK),
                expected,
                "the press that left the handle at {position} was silent",
            );
        }
    }

    /// A drag inside one detent is silent, and crossing detents clicks once
    /// each — so a pointer holding a fader is a fader and not a buzz.
    #[test]
    fn a_drag_clicks_once_a_detent_and_not_once_a_frame() {
        let mut audio = headless(&unity());
        let before = audio.plays(SOUND_CLICK);

        // Ten frames of a pointer that has barely moved.
        for step in 1..=10 {
            audio.fader_moved(Bus::Music, 1.0 - CLICK_NOTCH * 0.02 * step as f32);
        }
        assert_eq!(
            audio.plays(SOUND_CLICK),
            before,
            "a pointer inside one detent clicked",
        );

        // A sweep down the groove, sampled far finer than the detents are wide.
        let samples = 200;
        for step in 1..=samples {
            audio.fader_moved(Bus::Music, 1.0 - step as f32 / samples as f32);
        }
        let clicks = audio.plays(SOUND_CLICK) - before;
        let detents = (1.0 / CLICK_NOTCH) as u64;
        assert_eq!(
            clicks, detents,
            "a full sweep clicked {clicks} times over {detents} detents",
        );
    }

    /// A placed fader is silent however far it moved, and leaves the next drag
    /// with a detent to be inside of.
    #[test]
    fn a_placed_fader_is_silent_and_carries_its_detent_with_it() {
        // Opened at unity, so the detent this starts in is the top of the
        // groove and a placement at the bottom has somewhere to move it to.
        let mut audio = headless(&unity());
        let before = audio.plays(SOUND_CLICK);

        // The whole groove, in one placement — a `RESET` seen from the bottom.
        audio.fader_placed(Bus::Music, 0.0);
        assert_eq!(audio.plays(SOUND_CLICK), before, "a placed fader clicked");

        // And the next frame, with the pointer resting where the placement put
        // the handle, is silent too. **This is the half a placement that
        // recorded nothing fails**: it would leave the fader marked at the top
        // of the groove with its handle at the bottom, and read the very next
        // frame as a drag across twenty detents.
        audio.fader_moved(Bus::Music, 0.0);
        assert_eq!(
            audio.plays(SOUND_CLICK),
            before,
            "the placement left the detent where the handle is not",
        );
    }

    /// The buses with content are exactly the ones something plays on.
    ///
    /// The two halves are asserted against each other rather than against a
    /// list written twice: a cue added to a silent bus without
    /// [`Audio::sounds`] being told would leave the screen calling it silent,
    /// and a bus dropped from the routing would leave the screen calling it
    /// audible. Master is the exception the mixer defines — every voice passes
    /// through it — so it is audible with no cue of its own.
    #[test]
    fn the_buses_the_screen_calls_audible_are_the_ones_with_cues() {
        let routed = [Bus::Music, Bus::Sfx, Bus::Ui];
        for bus in Bus::ALL {
            let expected = routed.contains(&bus) || bus == Bus::Master;
            assert_eq!(
                Audio::sounds(bus),
                expected,
                "{bus:?} is described the wrong way round",
            );
        }
    }

    /// An id nothing answers to is ignored rather than underflowing.
    #[test]
    fn an_unknown_cue_is_ignored_rather_than_underflowing() {
        let mut audio = headless(&unity());
        audio.play(0, Bus::Ui, 1.0, false);
        audio.play(u32::MAX, Bus::Ui, 1.0, false);
        assert_eq!(audio.plays(0), 0);
        assert_eq!(audio.plays(u32::MAX), 0);
    }

    /// A headless run says where its audio went, and it is not a device.
    #[test]
    fn a_headless_run_reports_the_null_stream_rather_than_a_device() {
        let audio = headless(&unity());
        assert!(
            audio.output().contains("null"),
            "a headless run reported {:?}",
            audio.output(),
        );
    }
}
