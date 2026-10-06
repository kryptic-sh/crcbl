//! The per-character runtime state: small, `Copy`, and hashed field by field.

use std::hash::{Hash, Hasher};

use super::asset::{BoolParam, FloatParam, MAX_PARAMETERS, StateId, TriggerParam};

/// A crossfade in flight: the state being faded out of, and how far through
/// the fade the machine is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub(crate) from: StateId,
    pub(crate) from_time: f32,
    pub(crate) elapsed: f32,
    pub(crate) duration: f32,
}

impl Fade {
    /// The state being faded out of.
    #[inline]
    #[must_use]
    pub const fn from(&self) -> StateId {
        self.from
    }

    /// That state's normalised time. It keeps advancing through the fade, so
    /// the outgoing motion does not freeze while it is faded out.
    #[inline]
    #[must_use]
    pub const fn from_time(&self) -> f32 {
        self.from_time
    }

    /// Seconds since the fade began.
    #[inline]
    #[must_use]
    pub const fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// How long the fade lasts, in seconds. Always above zero: a zero
    /// crossfade is a cut, and a cut leaves no fade in flight.
    #[inline]
    #[must_use]
    pub const fn duration(&self) -> f32 {
        self.duration
    }

    /// How much of the pose is the **incoming** state: `elapsed / duration`,
    /// in `0..1` while the fade is in flight.
    #[inline]
    #[must_use]
    pub fn weight(&self) -> f32 {
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }
}

/// One character's place in a [`StateMachine`](super::StateMachine): which
/// state it is in, how far through that state's cycle, the crossfade in
/// flight, and every parameter's value.
///
/// **Plain old data.** `Copy`, a fixed size whatever the asset, and no
/// pointers — the shape the design gives the server's animation state: stepped
/// on the tick by [`Machine::step`](super::Machine::step), copied to the
/// client, and folded into a tick hash by the [`Hash`] impl below. Pose
/// sampling reads it and never writes it, so the client can pose a character
/// from the copy it was sent.
///
/// Built by [`Machine::start`](super::Machine::start). The [`Default`] is the
/// all-zero state — the first state, at time zero, every parameter zero — which
/// a caller uses as a placeholder before the first copy arrives; it is a state
/// of every machine, since a machine has at least one state.
///
/// Parameter handles come from the machine this state was started from. A
/// handle from a *different* machine indexes the same fixed array, so it
/// cannot panic, but it reads or writes whichever parameter happens to share
/// its index.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MachineState {
    pub(crate) state: StateId,
    pub(crate) time: f32,
    pub(crate) fade: Option<Fade>,
    /// One value per parameter, by declaration index: a float as itself, a bool
    /// or a trigger as `0.0` or `1.0`. Slots past the declared count stay zero.
    pub(crate) values: [f32; MAX_PARAMETERS],
}

impl MachineState {
    /// The state the machine is in — the incoming one, during a crossfade.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> StateId {
        self.state
    }

    /// How far through the current state's cycle, normalised: `0..1` for a
    /// looping state, `0..=1` for one that holds at its end.
    #[inline]
    #[must_use]
    pub const fn time(&self) -> f32 {
        self.time
    }

    /// The crossfade in flight, if any.
    #[inline]
    #[must_use]
    pub const fn fade(&self) -> Option<Fade> {
        self.fade
    }

    /// A float parameter's value.
    #[inline]
    #[must_use]
    pub fn float(&self, parameter: FloatParam) -> f32 {
        self.values[usize::from(parameter.0)]
    }

    /// Sets a float parameter.
    ///
    /// A `NaN` is stored as given: every comparison against it is false, so no
    /// `Above` or `Below` condition on it holds, and a 1D blend driven by it
    /// holds its first stop — the same rule [`BlendSpace1d`](crate::BlendSpace1d)
    /// has. Deterministic either way.
    #[inline]
    pub fn set_float(&mut self, parameter: FloatParam, value: f32) {
        self.values[usize::from(parameter.0)] = value;
    }

    /// A bool parameter's value.
    #[inline]
    #[must_use]
    pub fn bool(&self, parameter: BoolParam) -> bool {
        self.values[usize::from(parameter.0)] != 0.0
    }

    /// Sets a bool parameter.
    #[inline]
    pub fn set_bool(&mut self, parameter: BoolParam, value: bool) {
        self.values[usize::from(parameter.0)] = f32::from(u8::from(value));
    }

    /// Whether a trigger is set and waiting for a transition to consume it.
    #[inline]
    #[must_use]
    pub fn is_triggered(&self, parameter: TriggerParam) -> bool {
        self.values[usize::from(parameter.0)] != 0.0
    }

    /// Sets a trigger. It stays set until a transition reading it fires.
    #[inline]
    pub fn trigger(&mut self, parameter: TriggerParam) {
        self.values[usize::from(parameter.0)] = 1.0;
    }

    /// Clears a trigger without a transition consuming it.
    #[inline]
    pub fn reset_trigger(&mut self, parameter: TriggerParam) {
        self.values[usize::from(parameter.0)] = 0.0;
    }
}

/// The bits a float is hashed by: its own, except that both zeros hash as
/// `+0.0` and every `NaN` as one canonical `NaN`.
///
/// A value's identity is its logical value and not its encoding — `-0.0` and
/// `+0.0` are the same time, and two `NaN`s with different payloads are the
/// same "no number" — so two states that agree on every value must not hash
/// apart over a sign bit or a payload.
fn canonical_bits(value: f32) -> u32 {
    if value == 0.0 {
        0
    } else if value.is_nan() {
        f32::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

/// **Field by field, through a defined encoding** — never the struct's bytes,
/// which would fold in padding and the `Option`'s layout. Floats go through
/// `canonical_bits`; the fade is a tag byte and then its fields.
impl Hash for MachineState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u16(self.state.0);
        state.write_u32(canonical_bits(self.time));
        match self.fade {
            None => state.write_u8(0),
            Some(fade) => {
                state.write_u8(1);
                state.write_u16(fade.from.0);
                state.write_u32(canonical_bits(fade.from_time));
                state.write_u32(canonical_bits(fade.elapsed));
                state.write_u32(canonical_bits(fade.duration));
            }
        }
        for &value in &self.values {
            state.write_u32(canonical_bits(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Fade, MachineState, StateId};
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn hash_of(state: &MachineState) -> u64 {
        let mut hasher = DefaultHasher::new();
        state.hash(&mut hasher);
        hasher.finish()
    }

    /// Both zeros are the same time, so they hash the same.
    #[test]
    fn a_signed_zero_hashes_as_zero() {
        let positive = MachineState::default();
        let negative = MachineState {
            time: -0.0,
            ..MachineState::default()
        };
        assert_eq!(hash_of(&positive), hash_of(&negative));
    }

    /// **Every field reaches the hash.** Changing any one of them moves it — a
    /// hash that skipped a field would call two different states equal.
    #[test]
    fn every_field_moves_the_hash() {
        let base = MachineState {
            fade: Some(Fade {
                from: StateId(1),
                from_time: 0.25,
                elapsed: 0.1,
                duration: 0.2,
            }),
            ..MachineState::default()
        };
        let fade = base.fade.expect("built with one");
        let variants = [
            MachineState {
                state: StateId(2),
                ..base
            },
            MachineState { time: 0.5, ..base },
            MachineState { fade: None, ..base },
            MachineState {
                fade: Some(Fade {
                    from: StateId(3),
                    ..fade
                }),
                ..base
            },
            MachineState {
                fade: Some(Fade {
                    from_time: 0.75,
                    ..fade
                }),
                ..base
            },
            MachineState {
                fade: Some(Fade {
                    elapsed: 0.15,
                    ..fade
                }),
                ..base
            },
            MachineState {
                fade: Some(Fade {
                    duration: 0.3,
                    ..fade
                }),
                ..base
            },
        ];
        for variant in &variants {
            assert_ne!(
                hash_of(variant),
                hash_of(&base),
                "{variant:?} hashed as {base:?}"
            );
        }
        for slot in 0..base.values.len() {
            let mut moved = base;
            moved.values[slot] = 1.0;
            assert_ne!(
                hash_of(&moved),
                hash_of(&base),
                "parameter slot {slot} is not hashed"
            );
        }
    }
}
