//! Spatial cue grammar — the learnable, deterministic 3D-to-stereo transform
//! that turns a relative position into perceptible direction.
//!
//! # Design principles
//!
//! * **Deterministic**: same relative position → exactly the same transform,
//!   every time, every sound.  No randomization.
//! * **Continuous**: cues interpolate smoothly across the sphere.
//! * **Learnable**: fixed, exaggerated cues that a skilled player can read.
//!
//! The four rules from [topic 13](docs/plan/13-audio.md):
//!
//! | # | Direction     | Cue                                                                     |
//! |---|---------------|-------------------------------------------------------------------------|
//! | 1 | Directly ahead| Reference signal: full volume, unprocessed.                             |
//! | 2 | Left / Right  | Far ear gets interaural time delay + lower far-ear volume (ILD).        |
//! | 3 | Behind        | Lower volume than front + small downward pitch shift.                   |
//! | 4 | Above / Below | Pitch shift: up for above, down for below.                              |
//!
//! # Coordinate convention
//!
//! +X = right, +Y = up, +Z = forward (same as the engine's world space).
//! The listener faces +Z.  Azimuth is measured from +Z in the XZ plane;
//! elevation is measured from the XZ plane toward +Y.

/// Tuneable parameters for the cue grammar.
///
/// Changing these mid-title breaks player skill — versioned like a save format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CueGrammar {
    /// Maximum ITD in **samples** at the given sample rate.
    /// For a 48 kHz rate, 30 samples ≈ 0.625 ms (typical human max ITD).
    pub max_itd_samples: f32,

    /// Far-ear attenuation in dB (ILD — interaural level difference).
    pub ild_db: f32,

    /// Behind-the-head volume attenuation in dB.
    pub rear_attenuation_db: f32,

    /// Pitch shift in **cents** for behind-the-head.
    /// Negative = lower pitch (more audible as "behind").
    pub rear_pitch_cents: f32,

    /// Pitch shift in **cents** for directly above.
    /// Positive = higher pitch.  Below is the inverse.
    pub elevation_pitch_cents: f32,

    /// Distance at which volume begins rolling off (in world units).
    pub rolloff_start: f32,

    /// Distance at which volume reaches silence (in world units).
    pub rolloff_end: f32,
}

impl Default for CueGrammar {
    fn default() -> Self {
        Self {
            max_itd_samples: 30.0,
            ild_db: 6.0,
            rear_attenuation_db: 3.0,
            rear_pitch_cents: -120.0, // roughly a semitone down
            elevation_pitch_cents: 80.0,
            rolloff_start: 1.0,
            rolloff_end: 100.0,
        }
    }
}

/// The result of computing a spatial cue for one emitter at one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialCue {
    /// Fractional ITD delay in **samples** for the right channel.
    /// Positive = delay right (source is to the left).
    /// Negative = delay left (source is to the right).
    /// Zero = centre.
    pub itd_samples: f32,

    /// Linear gain for the left channel.
    pub gain_left: f32,

    /// Linear gain for the right channel.
    pub gain_right: f32,

    /// Pitch ratio (1.0 = no shift, > 1.0 = higher, < 1.0 = lower).
    pub pitch_ratio: f32,

    /// Combined volume multiplier (distance rolloff only; panning is in the gains).
    pub volume: f32,
}

impl SpatialCue {
    /// The centre of the pan law: no direction encoded, full volume, no pitch
    /// shift.
    ///
    /// The gains are the constant-power centre (`1/√2`), not unity — under a
    /// constant-power pan law that *is* dead centre, and using unity here would
    /// put a 3 dB step at the boundary between this and any other direction.
    pub const REFERENCE: Self = Self {
        itd_samples: 0.0,
        gain_left: core::f32::consts::FRAC_1_SQRT_2,
        gain_right: core::f32::consts::FRAC_1_SQRT_2,
        pitch_ratio: 1.0,
        volume: 1.0,
    };
}

/// Computes a spatial cue for `emitter` relative to `listener`.
///
/// Both positions are in world space.  Returns interpolated gains, ITD,
/// pitch, and distance-attenuated volume.
pub fn compute_cue(listener: [f32; 3], emitter: [f32; 3], grammar: &CueGrammar) -> SpatialCue {
    let dx = emitter[0] - listener[0];
    let dy = emitter[1] - listener[1];
    let dz = emitter[2] - listener[2];

    let dist_sq = dx * dx + dy * dy + dz * dz;
    let dist = dist_sq.sqrt();

    // ── Distance rolloff ─────────────────────────────────────────────────
    let volume = gain_from_distance(dist, grammar.rolloff_start, grammar.rolloff_end);

    if dist < 0.001 {
        // Co-located: reference signal.
        return SpatialCue {
            volume,
            ..SpatialCue::REFERENCE
        };
    }

    // Unit direction from listener to emitter.
    let inv_dist = 1.0 / dist;
    let nx = dx * inv_dist;
    let ny = dy * inv_dist;
    let nz = dz * inv_dist;

    // ── Horizontal angle (azimuth) ───────────────────────────────────────
    // In the XZ plane: angle from +Z.  cos_az = nz.
    // Pan = how far right (positive) or left (negative).
    // Cosine pan: 1.0 = dead centre, 0.0 = ±90°, -1.0 = directly behind.
    let cos_azimuth = nz; // projection onto forward axis
    let pan = nx; // projection onto right axis, [-1, +1]

    // ── Elevation ────────────────────────────────────────────────────────
    // ny is in [-1, +1] where +1 = directly above.
    let elevation = ny;

    // ── Rule 2: left/right pan (ILD + ITD) ──────────────────────────────
    let (gain_left, gain_right, itd) = pan_cue(pan, grammar);

    // ── Rule 3: behind attenuation + pitch shift ─────────────────────────
    // Behind: cos_azimuth < 0.  Blend factor = how far behind.
    let behind = (-cos_azimuth).clamp(0.0, 1.0);
    let rear_db = behind * grammar.rear_attenuation_db;
    let rear_pitch = behind * grammar.rear_pitch_cents;

    // ── Rule 4: elevation pitch shift ────────────────────────────────────
    let elevation_pitch = elevation * grammar.elevation_pitch_cents;

    // ── Combine ──────────────────────────────────────────────────────────
    let total_pitch_cents = rear_pitch + elevation_pitch;
    let pitch_ratio = cents_to_ratio(total_pitch_cents);

    let rear_gain = db_to_linear(-rear_db);
    let gain_left = gain_left * rear_gain;
    let gain_right = gain_right * rear_gain;

    SpatialCue {
        itd_samples: itd,
        gain_left,
        gain_right,
        pitch_ratio,
        volume,
    }
}

/// Computes left/right gains and ITD from a pan value in [-1, +1].
///
/// Uses constant-power panning for the gain split and a linear ITD mapping.
/// Every output is continuous in `pan`, including across zero — the module
/// promises continuity, and an emitter that crosses the centre line by a
/// fraction of a millimetre must not jump.
fn pan_cue(pan: f32, grammar: &CueGrammar) -> (f32, f32, f32) {
    // Constant-power: angle = (pan + 1) / 2 * 90° → angle in [0, π/2].
    // Right channel weight = sin(angle), left channel weight = cos(angle).
    // At pan = 0 both are 1/√2, which is the centre of this law.
    let pan = pan.clamp(-1.0, 1.0);
    let pan_weight = (pan + 1.0) * 0.5; // [0, 1]
    let angle = pan_weight * core::f32::consts::FRAC_PI_2;
    let (sin_a, cos_a) = angle.sin_cos();
    let mut gain_right = sin_a;
    let mut gain_left = cos_a;

    // Apply ILD: attenuate the far ear, scaled by how far off-centre the
    // source is. Applying the full `ild_db` the instant `pan` leaves zero
    // would be a 6 dB step in each ear — a 12 dB swing across the centre.
    let ild_linear = db_to_linear(-grammar.ild_db * pan.abs());
    if pan > 0.0 {
        gain_left *= ild_linear; // far ear = left when source is right
    } else if pan < 0.0 {
        gain_right *= ild_linear; // far ear = right when source is left
    }

    // ITD: linear in the pan angle.
    // pan=0 → ITD=0, pan=±1 → ITD=∓max.
    // Positive ITD = delay right channel (source left).
    // Source right (pan > 0) → delay left → ITD negative.
    let itd = -pan * grammar.max_itd_samples;

    (gain_left, gain_right, itd)
}

/// Simple distance rolloff: linear from `start` to `end`, clamped.
fn gain_from_distance(dist: f32, start: f32, end: f32) -> f32 {
    if dist <= start {
        1.0
    } else if dist >= end {
        0.0
    } else {
        1.0 - (dist - start) / (end - start)
    }
}

/// Converts cents to a pitch ratio.
/// +100 cents = 1 semitone up = ratio 2^(100/1200) = 2^(1/12).
fn cents_to_ratio(cents: f32) -> f32 {
    2.0_f32.powf(cents / 1200.0)
}

/// Converts dB to linear gain.
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn grammar() -> CueGrammar {
        CueGrammar::default()
    }

    #[test]
    fn a_source_straight_ahead_is_centred_with_no_itd_and_no_pitch_shift() {
        // Within rolloff_start: full volume, no ITD, no pitch shift, and the
        // constant-power centre in both ears (not unity — see `REFERENCE`).
        let centre = core::f32::consts::FRAC_1_SQRT_2;
        let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, 0.5], &grammar());
        assert_eq!(cue.itd_samples, 0.0);
        assert!((cue.gain_left - centre).abs() < 0.01, "{}", cue.gain_left);
        assert!((cue.gain_right - centre).abs() < 0.01, "{}", cue.gain_right);
        assert!((cue.pitch_ratio - 1.0).abs() < 0.01);
        assert!((cue.volume - 1.0).abs() < 0.01);
        assert!((cue.gain_left - cue.gain_right).abs() < 1e-6);
    }

    #[test]
    fn pan_is_continuous_across_centre() {
        // A 0.1 mm lateral move used to swing each ear by 6 dB: the centre
        // fast path returned unity and the ILD switched ears in one step.
        let grammar = grammar();
        let sample = |x: f32| compute_cue([0.0, 0.0, 0.0], [x, 0.0, 5.0], &grammar);

        let left = sample(-1e-4);
        let centre = sample(0.0);
        let right = sample(1e-4);

        for (a, b) in [(left, centre), (centre, right), (left, right)] {
            assert!(
                (a.gain_left - b.gain_left).abs() < 1e-3,
                "left gain jumped: {} → {}",
                a.gain_left,
                b.gain_left
            );
            assert!(
                (a.gain_right - b.gain_right).abs() < 1e-3,
                "right gain jumped: {} → {}",
                a.gain_right,
                b.gain_right
            );
            // ITD is in samples, so it is scaled by `max_itd_samples` (30);
            // the gains are order 1. A tenth of a sample is inaudible.
            assert!(
                (a.itd_samples - b.itd_samples).abs() < 0.1,
                "ITD jumped: {} → {}",
                a.itd_samples,
                b.itd_samples
            );
        }
    }

    #[test]
    fn ild_grows_monotonically_with_pan() {
        // The far ear must fade in gradually, not switch on at |pan| > 0.
        let grammar = grammar();
        let mut previous = 0.0f32;
        for step in 0..=20 {
            let x = step as f32 / 20.0 * 5.0;
            let cue = compute_cue([0.0, 0.0, 0.0], [x, 0.0, 5.0], &grammar);
            let ratio_db = 20.0 * (cue.gain_right / cue.gain_left).log10();
            assert!(
                ratio_db >= previous - 1e-3,
                "L/R ratio went backwards at x={x}: {previous} → {ratio_db}"
            );
            previous = ratio_db;
        }
    }

    #[test]
    fn right_is_louder_on_right() {
        let cue = compute_cue([0.0, 0.0, 0.0], [5.0, 0.0, 0.0], &grammar());
        // Right channel should be louder than left.
        assert!(cue.gain_right > cue.gain_left);
        // Source is right (+X) → ITD negative (delay left channel).
        assert!(cue.itd_samples < 0.0);
    }

    #[test]
    fn behind_is_quieter_and_lower_pitch() {
        let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, -5.0], &grammar());
        // Behind: volume lower, pitch lower.
        assert!(cue.gain_left < 1.0);
        assert!(cue.gain_right < 1.0);
        assert!(cue.pitch_ratio < 1.0);
    }

    #[test]
    fn above_is_higher_pitch() {
        let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 5.0, 0.0], &grammar());
        assert!(cue.pitch_ratio > 1.0);
    }

    #[test]
    fn below_is_lower_pitch() {
        let cue = compute_cue([0.0, 5.0, 0.0], [0.0, 0.0, 0.0], &grammar());
        // Listener at (0,5,0), emitter at (0,0,0) → emitter is below.
        assert!(cue.pitch_ratio < 1.0);
    }

    #[test]
    fn co_located_is_reference() {
        let cue = compute_cue([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], &grammar());
        assert_eq!(cue.itd_samples, 0.0);
        assert!((cue.pitch_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn distance_rolloff_reaches_zero() {
        let grammar = CueGrammar {
            rolloff_start: 1.0,
            rolloff_end: 10.0,
            ..CueGrammar::default()
        };
        let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, 15.0], &grammar);
        assert_eq!(cue.volume, 0.0);
    }

    #[test]
    fn the_same_listener_and_source_produce_a_bit_identical_cue() {
        let grammar = grammar();
        let a = compute_cue([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], &grammar);
        let b = compute_cue([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], &grammar);
        // Floating-point equality: same input → same bits.
        assert_eq!(a.itd_samples.to_bits(), b.itd_samples.to_bits());
    }

    #[test]
    fn itd_zero_at_centre() {
        let cue = compute_cue([0.0, 0.0, 0.0], [0.0, 0.0, 5.0], &grammar());
        assert!((cue.itd_samples - 0.0).abs() < 0.01);
    }

    #[test]
    fn itd_continuous_near_centre() {
        // Just slightly to the right — ITD should be small but non-zero.
        let cue = compute_cue([0.0, 0.0, 0.0], [0.1, 0.0, 5.0], &grammar());
        assert!(
            cue.itd_samples < 0.0,
            "right source → negative ITD (delay left)"
        );
        assert!(cue.itd_samples > -grammar().max_itd_samples);
    }
}
