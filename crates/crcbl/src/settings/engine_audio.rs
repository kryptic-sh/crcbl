//! The `[engine.audio]` section: one gain per mixer bus, read and written.
//! The [module docs](super) say why an audio key is the value rather than a
//! clamp on one.

use crcbl_audio::mixer::Bus;
use crcbl_store::StorageError;
use crcbl_store::settings::SettingsStack;

#[cfg(doc)]
use super::{set_render_scale, set_video, video_effects};

/// The `[engine.audio]` section, as a dotted key prefix.
pub const AUDIO_NAMESPACE: &str = "engine.audio";

/// Every bus gain the player's file can set, paired with the bus it moves.
///
/// Unity for a bus the file says nothing about, and the file's value clamped to
/// `[0, 1]` for one it does. The order is [`Bus::ALL`]'s, so a caller can hand
/// the whole thing to
/// [`Mixer::set_bus_gain`](crcbl_audio::mixer::Mixer::set_bus_gain) in a loop
/// without knowing which buses exist.
///
/// # A line that does nothing says so
///
/// A key holding something this cannot read as a number — `music_volume =
/// "half"` — leaves its bus at unity and **warns**, naming the key, on
/// [`video_effects`]' terms: a player who wrote a line, heard no change, and has
/// nothing to read that would tell them why is the failure worth a log line. A
/// key that is simply absent is not a mistake and does not warn.
#[must_use]
pub fn audio_gains(stack: &SettingsStack) -> [(Bus, f32); Bus::ALL.len()] {
    Bus::ALL.map(|bus| {
        let dotted = format!("{AUDIO_NAMESPACE}.{}", bus.settings_key());
        let unreadable = || {
            crcbl_core::log::warn!(
                "settings: `{dotted}` is not a usable number, so it does nothing; \
                 the {bus:?} bus stays where the game set it"
            );
            (bus, 1.0)
        };
        match stack.get::<f64>(&dotted) {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a gain is clamped to [0, 1], where every f64 has an f32 within an ulp"
            )]
            // Finite first: `clamp` answers NaN for NaN, and a NaN gain
            // multiplies every sample of the bus into silence-shaped garbage.
            Some(gain) if gain.is_finite() => (bus, gain.clamp(0.0, 1.0) as f32),
            Some(_) => unreadable(),
            None if stack.contains(&dotted) => unreadable(),
            None => (bus, 1.0),
        }
    })
}

/// Write one bus's gain into `[engine.audio]`, clamped to what
/// [`audio_gains`] reads.
///
/// The bus is spelled by [`Bus::settings_key`], which is the reader's spelling
/// too — a settings screen and a start-up disagreeing about the name of the
/// music slider is a volume the player sets once and never hears again.
///
/// One bus rather than the whole array, unlike [`set_video`]: an audio key
/// **is** the gain, so a bus the file does not mention is already at unity and
/// there is no row a writer has to state to mean "on".
///
/// # Errors
///
/// [`set_video`]'s, and a `gain` that is not finite, for [`set_render_scale`]'s
/// reason.
pub fn set_audio_gain(stack: &mut SettingsStack, bus: Bus, gain: f32) -> Result<(), StorageError> {
    let dotted = format!("{AUDIO_NAMESPACE}.{}", bus.settings_key());
    if !gain.is_finite() {
        return Err(StorageError::Other(format!(
            "settings: `{dotted}` cannot be written as {gain}"
        )));
    }
    stack.set(&dotted, &f64::from(gain.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::tests::{round_trip, stack_from};

    /// **A gain reaches its own bus and no other**, under
    /// [`Bus::settings_key`]'s spelling on both sides.
    #[test]
    fn a_saved_gain_reads_back_on_its_own_bus() {
        let (reloaded, _) = round_trip(|stack| {
            set_audio_gain(stack, Bus::Sfx, 0.25).expect("a fresh user layer accepts every key");
        });
        for (bus, gain) in audio_gains(&reloaded) {
            let wanted = if bus == Bus::Sfx { 0.25 } else { 1.0 };
            assert!(
                (gain - wanted).abs() < f32::EPSILON,
                "{bus:?} reads {gain}, wanted {wanted}"
            );
        }
    }

    /// The gain the reader answers for `bus`, off a file holding `toml`.
    fn gain_of(toml: &str, bus: Bus) -> f32 {
        audio_gains(&stack_from(toml))
            .into_iter()
            .find(|(named, _)| *named == bus)
            .expect("every bus is answered for")
            .1
    }

    /// **A bus the file says nothing about is at unity.**
    ///
    /// The audio half of `an_absent_key_clamps_nothing`, and the reason it is
    /// worth its own test: an absent video key leaves an effect *on* because
    /// that layer may only clamp downward, while an absent audio key is unity
    /// because the key **is** the gain. The two arrive at "nothing changes" for
    /// different reasons, so an implementation that confused them would still
    /// pass one of these.
    #[test]
    fn a_file_with_no_audio_section_leaves_every_bus_at_unity() {
        for (bus, gain) in audio_gains(&stack_from("")) {
            assert!(
                (gain - 1.0).abs() < f32::EPSILON,
                "{bus:?} reads {gain} off a file that never mentions it"
            );
        }
    }

    /// **A key moves its own bus and no other**, and it is read under the
    /// spelling [`Bus::settings_key`] gives it.
    #[test]
    fn a_volume_key_moves_its_own_bus() {
        let toml = "[engine.audio]\nmusic_volume = 0.25\n";
        assert!((gain_of(toml, Bus::Music) - 0.25).abs() < f32::EPSILON);
        for bus in Bus::ALL {
            if bus == Bus::Music {
                continue;
            }
            let gain = gain_of(toml, bus);
            assert!(
                (gain - 1.0).abs() < f32::EPSILON,
                "setting the music volume moved {bus:?} to {gain}"
            );
        }
    }

    /// Every bus is reachable from a file, under a key nothing else claims.
    ///
    /// The guard against a bus added to [`Bus::ALL`] whose key is a copy of a
    /// neighbour's: the symptom is one bus unreachable and another moving two
    /// gains at once, and a per-bus test would not see it.
    #[test]
    fn every_bus_is_reachable_from_a_file_under_its_own_key() {
        for bus in Bus::ALL {
            let toml = format!("[engine.audio]\n{} = 0.5\n", bus.settings_key());
            for (named, gain) in audio_gains(&stack_from(&toml)) {
                let expected = if named == bus { 0.5 } else { 1.0 };
                assert!(
                    (gain - expected).abs() < f32::EPSILON,
                    "with only {}'s key set, {named:?} reads {gain} rather than {expected}",
                    bus.settings_key()
                );
            }
        }
    }

    /// **A whole number is a gain**, which is what a hand-edited file holds.
    ///
    /// TOML makes `= 1` an integer and `= 1.0` a float, and a reader that could
    /// not take the first would leave that key doing nothing — the failure the
    /// warn branch exists to make visible, arriving on the spelling a person is
    /// most likely to write.
    #[test]
    fn a_volume_written_as_a_whole_number_is_read() {
        assert!(gain_of("[engine.audio]\nmaster_volume = 0\n", Bus::Master).abs() < f32::EPSILON);
        let one = gain_of("[engine.audio]\nmaster_volume = 1\n", Bus::Master);
        assert!((one - 1.0).abs() < f32::EPSILON, "read {one}");
    }

    /// A gain outside `[0, 1]` is clamped rather than refused.
    ///
    /// Both ends, because they fail differently: a negative gain inverts every
    /// sample on the bus, and one above unity is the clipping the mixer has no
    /// limiter for.
    #[test]
    fn a_volume_outside_the_range_is_clamped_to_it() {
        assert!(gain_of("[engine.audio]\nsfx_volume = -2.0\n", Bus::Sfx).abs() < f32::EPSILON);
        let loud = gain_of("[engine.audio]\nsfx_volume = 40.0\n", Bus::Sfx);
        assert!((loud - 1.0).abs() < f32::EPSILON, "read {loud}");
    }

    /// A key holding something that is not a number leaves its bus at unity.
    ///
    /// `a_key_holding_something_that_is_not_a_boolean_clamps_nothing_and_warns`
    /// is the same claim on the video side; the direction here is the safe one
    /// for the same reason, since silence is what a broken line must not cause.
    #[test]
    fn a_volume_that_is_not_a_number_leaves_its_bus_alone() {
        let gain = gain_of("[engine.audio]\nui_volume = \"half\"\n", Bus::Ui);
        assert!((gain - 1.0).abs() < f32::EPSILON, "read {gain}");
    }
}
