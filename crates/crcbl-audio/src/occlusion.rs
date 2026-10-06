//! Rule 5 of the cue grammar, occlusion: a voice heard through something is
//! muffled and quieter, by how much depending on what it is heard through.
//!
//! ```text
//! game glue (crcbl::occlusion)          this module                 the voice
//!   ray from ear to emitter ──▶ hits ──▶ AcousticMaterial per hit
//!                                         Occlusion::through ──▶ Mixer::set_occlusion
//!                                                                  │
//!                         per frame: ramp toward the target ◀──────┘
//!                         one-pole lowpass at the cutoff, then the gain
//! ```
//!
//! # Pure DSP: no ray is cast here
//!
//! What stands between an ear and an emitter is a physics question, and this
//! crate takes no physics dependency (see the crate docs). The ray walk and the
//! table saying what each collider is made of live where the client and audio
//! meet, in the umbrella's `crcbl::occlusion`. This module owns the two halves
//! that are sound: what a material does to a sound ([`AcousticMaterial`],
//! combined by [`Occlusion::through`]) and the per-voice filter that applies the
//! answer ([`Mixer::set_occlusion`](crate::mixer::Mixer::set_occlusion)).
//!
//! # The filter: a one-pole lowpass with an exact −3 dB point
//!
//! `y[n] = y[n−1] + a·(x[n] − y[n−1])`, the RC lowpass, with its coefficient
//! chosen so the magnitude response is exactly `1/√2` at the cutoff rather than
//! approximately. Writing `b = 1 − a` and `ω = 2π·f_c/f_s`, the response is
//! `|H(ω)|² = a² / (1 − 2b·cos ω + b²)`; setting it to one half and solving the
//! quadratic for the root below one gives
//!
//! ```text
//! y = 2 − cos ω,   b = y − √(y² − 1),   a = 1 − b
//! ```
//!
//! which is the standard "exact −3 dB one-pole" design. Its DC gain is
//! `a / (1 − b) = 1`, so a muffled voice keeps its level below the cutoff and
//! the attenuation is the gain stage's alone. One pole rather than a biquad:
//! a 6 dB/octave slope reads as "through a wall" and has no resonance to tune,
//! and the cutoffs a material asks for sit far below Nyquist where the one-pole
//! is closest to its analogue.
//!
//! `cos` is [`crcbl_core::trig::cos`], the engine's constructed one, and `√` is
//! exactly rounded by IEEE-754, so a coefficient is the same bits on every
//! target. The gain from [`Occlusion::through`] goes through `powf`, which is
//! not — the crate-wide transcendental policy in `docs/backlog.md` (_Audio's
//! transcendentals and deny_) covers it with every other `powf` here.
//!
//! # A clear voice is the voice it was before
//!
//! At [`Occlusion::CLEAR`] the coefficient is exactly one and the gain exactly
//! one, and the filter then passes each sample through untouched rather than
//! computing `y + 1·(x − y)`, which rounds. So a voice nobody occluded, and one
//! whose occlusion has ramped back to clear, mixes the same bits it mixed before
//! this module existed — no buffer moves unless a game turns occlusion on.
//!
//! # Moving occluders do not zipper
//!
//! A new target is not stepped to. Both the coefficient and the gain ramp
//! linearly to it over [`OCCLUSION_RAMP_FRAMES`] output frames, each frame's
//! value computed from the ramp's ends rather than accumulated, so where a ramp
//! is does not depend on how the device cut it into blocks, and its last frame
//! lands on the target exactly.

use crate::CHANNELS;

/// How many output frames a voice takes to move from one occlusion to the
/// next.
///
/// Long enough that a wall sliding across a sound's path is a sweep rather than
/// a click, short enough that a sound stepping out from behind cover is clear
/// before the step has finished being seen.
pub const OCCLUSION_RAMP_FRAMES: u32 = 2048;

/// The lowest cutoff a voice is filtered at, in hertz. A material asking for
/// less is filtered here: below it the one-pole passes almost nothing a
/// listener could still place.
pub const MIN_CUTOFF_HZ: f32 = 20.0;

/// What occlusion does to one voice: a lowpass cutoff and a linear gain.
///
/// The target a voice ramps toward. [`Occlusion::CLEAR`] is no filter and no
/// attenuation, which is what every voice has until a game says otherwise.
///
/// The fields are raw caller input, clamped where they reach a voice: the
/// cutoff to at least [`MIN_CUTOFF_HZ`] (anything at or above the device's
/// Nyquist frequency, infinity included, is no filter), the gain to `[0, 1]`.
/// A NaN in either is read as clear, for the reason
/// [`Mixer::set_bus_gain`](crate::mixer::Mixer::set_bus_gain) reads one as
/// unity: a broken query that silenced a voice would be a bug nobody could find
/// by listening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occlusion {
    /// The lowpass cutoff, in hertz: the frequency the voice is 3 dB down at.
    pub cutoff_hz: f32,
    /// The linear gain applied after the lowpass.
    pub gain: f32,
}

impl Occlusion {
    /// Nothing in the way: no lowpass and unity gain.
    pub const CLEAR: Self = Self {
        cutoff_hz: f32::INFINITY,
        gain: 1.0,
    };

    /// What a sound heard through every one of `materials` comes out as.
    ///
    /// **The rule: attenuations add, and the lowest cutoff wins.** Two walls
    /// cost a sound both walls' loss — decibels add where linear gains multiply
    /// — while a lowpass behind a lower one takes nothing more away that a
    /// listener can hear, so the duller material sets the tone. Nothing at all
    /// is [`Occlusion::CLEAR`].
    ///
    /// The materials are summed in the order given, which is why
    /// `crcbl::occlusion` hands them over in the order the ray met them: the
    /// float sum is then the same every time for the same scene.
    ///
    /// [`AcousticMaterial::density`] is not read: see its own docs.
    #[must_use]
    pub fn through(materials: impl IntoIterator<Item = AcousticMaterial>) -> Self {
        let mut any = false;
        let mut cutoff_hz = f32::INFINITY;
        let mut attenuation_db = 0.0f32;
        for material in materials {
            any = true;
            cutoff_hz = cutoff_hz.min(material.muffle_cutoff_hz);
            attenuation_db += material.attenuation_db;
        }
        if !any {
            return Self::CLEAR;
        }
        Self {
            cutoff_hz,
            gain: crate::spatial::db_to_linear(-attenuation_db),
        }
    }

    /// Whether this is [`Occlusion::CLEAR`] once clamped: a voice at it is the
    /// voice it would be with no occlusion at all.
    #[must_use]
    pub fn is_clear(self) -> bool {
        let clamped = self.clamped();
        clamped.cutoff_hz == f32::INFINITY && clamped.gain == 1.0
    }

    /// The clamp described on the type.
    fn clamped(self) -> Self {
        let cutoff_hz = if self.cutoff_hz.is_nan() {
            f32::INFINITY
        } else {
            self.cutoff_hz.max(MIN_CUTOFF_HZ)
        };
        let gain = if self.gain.is_finite() {
            self.gain.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Self { cutoff_hz, gain }
    }
}

impl Default for Occlusion {
    fn default() -> Self {
        Self::CLEAR
    }
}

/// What a sound loses passing through one thing: a cutoff and an attenuation.
///
/// **Game data, and a fixed, versioned preset.** The audio rules make a
/// material learnable the way the cue grammar is — players learn "behind wood"
/// against "behind concrete" — so the presets below are numbers a title ships
/// and keeps, and [`AcousticMaterial::PRESETS_VERSION`] moves when any of them
/// does. A game with materials of its own writes them as values of this type;
/// which collider is made of what is the game's table, not physics' (see
/// `crcbl::occlusion`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcousticMaterial {
    /// Mass density, in kilograms per cubic metre.
    ///
    /// **Not read by [`Occlusion::through`].** It is part of the preset because
    /// transmission loss through a real partition follows its mass per unit
    /// area (the mass law), which is density times thickness — and a ray that
    /// reports where it enters a collider and not where it leaves has no
    /// thickness to multiply by. `docs/backlog.md` (_Occlusion (rule 5)_)
    /// carries that as owed; until then the cutoff and attenuation are the
    /// whole of what a crossing costs.
    pub density: f32,
    /// The lowpass cutoff a sound heard through this is filtered at, in hertz.
    pub muffle_cutoff_hz: f32,
    /// How much quieter a sound heard through this is, in decibels: positive is
    /// quieter.
    pub attenuation_db: f32,
}

impl AcousticMaterial {
    /// Which revision of the presets below this build ships. Moves whenever a
    /// preset's numbers do, because a player's ear was trained on the old ones.
    pub const PRESETS_VERSION: u32 = 1;

    /// Leaves, a hedge, a canvas wall: barely there. Takes the air off the top
    /// and little else.
    pub const FOLIAGE: Self = Self {
        density: 80.0,
        muffle_cutoff_hz: 6_000.0,
        attenuation_db: 2.0,
    };

    /// A window pane: the highs dulled, the level down a little.
    pub const GLASS: Self = Self {
        density: 2_500.0,
        muffle_cutoff_hz: 3_500.0,
        attenuation_db: 5.0,
    };

    /// A door, a plank wall, a crate.
    pub const THIN_WOOD: Self = Self {
        density: 600.0,
        muffle_cutoff_hz: 1_800.0,
        attenuation_db: 8.0,
    };

    /// Sheet steel: a hull, a container, a shutter.
    pub const METAL: Self = Self {
        density: 7_850.0,
        muffle_cutoff_hz: 1_000.0,
        attenuation_db: 12.0,
    };

    /// A masonry wall, a bunker, packed earth: what a sound is mostly stopped
    /// by.
    pub const CONCRETE: Self = Self {
        density: 2_400.0,
        muffle_cutoff_hz: 400.0,
        attenuation_db: 18.0,
    };

    /// Every preset, lightest to heaviest by what it costs a sound.
    pub const PRESETS: [Self; 5] = [
        Self::FOLIAGE,
        Self::GLASS,
        Self::THIN_WOOD,
        Self::METAL,
        Self::CONCRETE,
    ];
}

/// The one-pole coefficient `a` whose response is exactly 3 dB down at
/// `cutoff_hz` when run at `sample_rate` — the design in the
/// [module docs](self).
///
/// One — no filtering at all — for a cutoff at or above Nyquist, which
/// includes [`Occlusion::CLEAR`]'s infinity, and for a zero rate, which has no
/// frequencies to filter.
pub(crate) fn one_pole_coefficient(cutoff_hz: f32, sample_rate: u32) -> f32 {
    let rate = f64::from(sample_rate);
    let cutoff = f64::from(cutoff_hz);
    if sample_rate == 0 || cutoff >= rate / 2.0 {
        return 1.0;
    }
    let omega = core::f64::consts::TAU * cutoff / rate;
    let y = 2.0 - crcbl_core::trig::cos(omega);
    let b = y - (y * y - 1.0).sqrt();
    (1.0 - b) as f32
}

/// The occlusion state one voice carries: its target, the ramp toward it, and
/// the lowpass's memory per channel.
///
/// The coefficient is computed per device rate, so a target set from the game
/// thread — which does not know the rate — is resolved at the start of the
/// next block, in [`VoiceOcclusion::begin_block`].
#[derive(Debug)]
pub(crate) struct VoiceOcclusion {
    /// What the voice is ramping toward, clamped.
    target: Occlusion,
    /// The target changed since the last block resolved it.
    retarget: bool,
    /// The next resolution jumps straight to the target instead of ramping:
    /// a voice that *starts* behind a wall starts muffled.
    snap: bool,
    /// The device rate `to`'s coefficient was computed for.
    rate: u32,
    /// `[coefficient, gain]` where the ramp began.
    from: [f32; 2],
    /// `[coefficient, gain]` where the ramp ends.
    to: [f32; 2],
    /// `[coefficient, gain]` in force for the frame just processed.
    now: [f32; 2],
    /// Frames into the ramp; [`OCCLUSION_RAMP_FRAMES`] once settled.
    frame: u32,
    /// The lowpass output per channel.
    state: [f32; CHANNELS],
}

impl VoiceOcclusion {
    /// Clear, and settled there.
    pub(crate) const fn new() -> Self {
        Self {
            target: Occlusion::CLEAR,
            retarget: false,
            snap: false,
            rate: 0,
            from: [1.0; 2],
            to: [1.0; 2],
            now: [1.0; 2],
            frame: OCCLUSION_RAMP_FRAMES,
            state: [0.0; CHANNELS],
        }
    }

    /// The target, as clamped.
    pub(crate) const fn target(&self) -> Occlusion {
        self.target
    }

    /// Ramp toward `occlusion` from wherever the voice is now.
    pub(crate) fn set(&mut self, occlusion: Occlusion) {
        self.target = occlusion.clamped();
        self.retarget = true;
    }

    /// Start at `occlusion`, with no ramp from clear.
    pub(crate) fn start_at(&mut self, occlusion: Occlusion) {
        self.set(occlusion);
        self.snap = true;
    }

    /// Resolve a new target, or a new device rate, into the ramp's end.
    ///
    /// A rate change re-resolves only a finite cutoff: at clear the coefficient
    /// is one at every rate.
    pub(crate) fn begin_block(&mut self, sample_rate: u32) {
        let rate_moved = sample_rate != self.rate && self.target.cutoff_hz.is_finite();
        if !self.retarget && !rate_moved {
            return;
        }
        self.to = [
            one_pole_coefficient(self.target.cutoff_hz, sample_rate),
            self.target.gain,
        ];
        self.rate = sample_rate;
        if self.snap {
            self.now = self.to;
            self.from = self.to;
            self.frame = OCCLUSION_RAMP_FRAMES;
        } else {
            self.from = self.now;
            self.frame = 0;
        }
        self.retarget = false;
        self.snap = false;
    }

    /// Advance the ramp one frame and answer `[coefficient, gain]` for it.
    pub(crate) fn step(&mut self) -> [f32; 2] {
        if self.frame < OCCLUSION_RAMP_FRAMES {
            self.frame += 1;
            if self.frame == OCCLUSION_RAMP_FRAMES {
                self.now = self.to;
            } else {
                let t = self.frame as f32 / OCCLUSION_RAMP_FRAMES as f32;
                for (now, (from, to)) in self.now.iter_mut().zip(self.from.iter().zip(&self.to)) {
                    *now = from + (to - from) * t;
                }
            }
        }
        self.now
    }

    /// Filter one channel's sample at the frame's `[coefficient, gain]`.
    ///
    /// A coefficient of exactly one passes the sample through rather than
    /// computing the recursion, which would round; a gain of exactly one is an
    /// exact multiply. Together they are what keeps a clear voice bit-identical.
    pub(crate) fn process(
        &mut self,
        channel: usize,
        sample: f32,
        [coefficient, gain]: [f32; 2],
    ) -> f32 {
        let state = &mut self.state[channel];
        *state = if coefficient == 1.0 {
            sample
        } else {
            *state + coefficient * (sample - *state)
        };
        *state * gain
    }
}

#[cfg(test)]
mod tests;
