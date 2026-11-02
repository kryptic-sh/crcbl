//! Velocities at animation-to-physics handoff.

use crcbl_core::trig::atan2;
use glam::{DQuat, DVec3, Mat4};

use super::{RagdollBinding, RagdollError, affine_inverse, rigid_inverse, validate_pose};
use crate::{Palette, Pose};

/// A body's initial world pose and velocity, in physics units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyMotion {
    /// Body centre in world space.
    pub position: DVec3,
    /// Body orientation in world space.
    pub rotation: DQuat,
    /// Body-centre displacement per second, including character movement.
    pub linear_velocity: DVec3,
    /// Shortest rotation between samples, in world radians per second.
    pub angular_velocity: DVec3,
}

/// Below this length of a rotation's vector part, [`scaled_axis`] reads it as
/// no rotation at all: glam's own cut in `DQuat::to_axis_angle`, kept so the
/// result matches glam's everywhere but the last bits of the angle.
const NEAR_IDENTITY: f64 = 1.0e-8;

/// `rotation`'s axis scaled by its angle in radians: glam's
/// `DQuat::to_scaled_axis` with [`crcbl_core::trig::atan2`] in place of the
/// platform's.
///
/// The platform's `atan2` is the C runtime's on `x86_64-pc-windows-msvc` and
/// rounds its last bit differently from glibc's and Apple's, so a handoff
/// spawn would start at different bits by target.
///
/// One difference from glam: a NaN in the vector part gives a NaN result,
/// which [`RagdollBinding::motion_from_previous_pose`] reports as
/// [`RagdollError::InvalidMotion`]. glam tests `length >= 1e-8`, which a NaN
/// length fails, so its `to_axis_angle` answered the zero rotation and a NaN
/// turn passed as no turn at all.
fn scaled_axis(rotation: DQuat) -> DVec3 {
    let vector = rotation.xyz();
    let length = vector.length();
    if length < NEAR_IDENTITY {
        return DVec3::ZERO;
    }
    vector / length * (2.0 * atan2(length, rotation.w))
}

impl RagdollBinding {
    /// Infers initial body motion from a pose sampled before the handoff.
    ///
    /// `previous_model` places that earlier pose in world space. `seconds`
    /// is the interval from that sample to the pose passed to [`Self::new`].
    /// Results use the original binding order. Sampling must resolve turns
    /// smaller than half a revolution; complete turns between samples cannot
    /// be recovered from orientations alone.
    ///
    /// This allocates at handoff, not during simulation. Animation inputs are
    /// still single precision; double precision outputs do not make animation
    /// evaluation deterministic. Invalid samples return [`RagdollError`].
    pub fn motion_from_previous_pose(
        &self,
        previous: &Pose,
        previous_model: Mat4,
        seconds: f64,
    ) -> Result<Vec<BodyMotion>, RagdollError> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(RagdollError::InvalidInterval);
        }
        validate_pose(&self.skeleton, previous)?;
        affine_inverse(previous_model).ok_or(RagdollError::InvalidModel)?;
        let mut previous_palette = Palette::new(&self.skeleton);
        previous_palette.compute(&self.skeleton, previous);
        let mut current_palette = Palette::new(&self.skeleton);
        current_palette.compute(&self.skeleton, &self.handoff);
        let mut output = vec![
            BodyMotion {
                position: DVec3::ZERO,
                rotation: DQuat::IDENTITY,
                linear_velocity: DVec3::ZERO,
                angular_velocity: DVec3::ZERO,
            };
            self.offsets.len()
        ];
        for (joint, body) in self.body_for_joint.iter().enumerate() {
            let Some(body) = *body else { continue };
            let offset_inverse =
                affine_inverse(self.offsets[body]).ok_or(RagdollError::InvalidBody { body })?;
            let before = previous_model * previous_palette.globals()[joint] * offset_inverse;
            let current = self.handoff_model * current_palette.globals()[joint] * offset_inverse;
            rigid_inverse(before).ok_or(RagdollError::InvalidBody { body })?;
            rigid_inverse(current).ok_or(RagdollError::InvalidBody { body })?;
            let (_, before_rotation, before_position) = before.to_scale_rotation_translation();
            let (_, current_rotation, current_position) = current.to_scale_rotation_translation();
            let rotation = current_rotation.as_dquat().normalize();
            let mut turn =
                (rotation * before_rotation.as_dquat().normalize().conjugate()).normalize();
            if turn.w < 0.0 {
                turn = -turn;
            }
            let linear_velocity =
                (current_position.as_dvec3() - before_position.as_dvec3()) / seconds;
            let angular_velocity = scaled_axis(turn) / seconds;
            if !linear_velocity.is_finite() || !angular_velocity.is_finite() {
                return Err(RagdollError::InvalidMotion { body });
            }
            output[body] = BodyMotion {
                position: current_position.as_dvec3(),
                rotation,
                linear_velocity,
                angular_velocity,
            };
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::Hasher;

    /// Unit quaternions with `w >= 0`, as the handoff hands [`scaled_axis`]
    /// them: vector parts from a seeded generator, shrunk by a power of two
    /// from `2^0` to `2^-40` so the sweep crosses [`NEAR_IDENTITY`], built and
    /// normalized in `f64` arithmetic alone so they are the same everywhere.
    fn for_each_turn(mut visit: impl FnMut(DQuat)) {
        let mut rng = crcbl_rand::Rng::from_u64(0x005c_a1ed_a415);
        let mut unit = || (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64 * 2.0 - 1.0;
        for shrink in 0..=40 {
            let scale = f64::from_bits((1023 - shrink as u64) << 52);
            for _ in 0..500 {
                let vector = DVec3::new(unit(), unit(), unit()) * scale;
                let w = unit().abs();
                visit(DQuat::from_xyzw(vector.x, vector.y, vector.z, w).normalize());
            }
        }
    }

    /// How many `f64`s lie between `a` and `b`, which must share a sign.
    fn ulps_apart(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        assert_eq!(
            a.is_sign_negative(),
            b.is_sign_negative(),
            "{a:e} against {b:e}"
        );
        a.to_bits().abs_diff(b.to_bits())
    }

    /// glam's `to_scaled_axis` is the yardstick here and nowhere else. The axis
    /// is the same division in both, so the only difference is the platform's
    /// `atan2` against [`atan2`], and the product rounding it.
    ///
    /// The bound is derived, not measured, because the platform differs by
    /// target: [`atan2`] is within `MAX_ATAN2_ULP` of the truth and a platform's
    /// within one more the other way, so the angles sit up to `MAX_ATAN2_ULP +
    /// 1` of the angle's ulps apart. Doubling is exact. That is up to twice as
    /// many of a component's ulps, whose significand can sit in the other half
    /// of its binade, and each side's product rounds once more. glibc measured
    /// 2 on 2026-11-01.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "glam's platform-atan2 version is the reference being matched"
    )]
    fn the_scaled_axis_is_glams_to_within_its_bound() {
        let mut visited = 0;
        let mut zeros = 0;
        let mut worst = 0;
        for_each_turn(|turn| {
            visited += 1;
            let ours = scaled_axis(turn);
            let glams = turn.to_scaled_axis();
            if ours == DVec3::ZERO {
                zeros += 1;
            }
            for (a, b) in ours.to_array().into_iter().zip(glams.to_array()) {
                worst = worst.max(ulps_apart(a, b));
            }
        });
        assert!(visited > 20_000, "the sweep covered only {visited} turns");
        assert!(
            zeros > 0 && zeros < visited / 2,
            "{zeros} of {visited} turns took the near-identity branch"
        );
        let bound = 2 * (crcbl_core::trig::MAX_ATAN2_ULP + 1) + 1;
        assert!(
            worst <= bound,
            "scaled_axis is {worst} ulp from glam's, over {bound}"
        );
    }

    #[test]
    fn a_nan_turn_is_a_nan_scaled_axis() {
        let turn = DQuat::from_xyzw(f64::NAN, 0.0, 0.0, 1.0);
        assert!(scaled_axis(turn).is_nan());
    }

    #[test]
    fn a_half_turn_and_no_turn_are_exact() {
        let half = DQuat::from_xyzw(0.0, 0.0, 1.0, 0.0);
        assert_eq!(
            scaled_axis(half),
            DVec3::new(0.0, 0.0, core::f64::consts::PI)
        );
        let quarter = DQuat::from_xyzw(
            core::f64::consts::FRAC_1_SQRT_2,
            0.0,
            0.0,
            core::f64::consts::FRAC_1_SQRT_2,
        );
        assert_eq!(
            scaled_axis(quarter),
            DVec3::new(core::f64::consts::FRAC_PI_2, 0.0, 0.0)
        );
        assert_eq!(scaled_axis(DQuat::IDENTITY), DVec3::ZERO);
    }

    /// **The same bits on every target**: [`scaled_axis`] over the sweep, as
    /// an FNV-1a digest pinned on x86-64 Linux and checked by CI on aarch64
    /// macOS and x86-64 Windows, where the platform's `atan2` differs.
    #[test]
    fn the_scaled_axes_hash_to_the_pinned_digest() {
        /// The digest on x86-64 Linux, 2026-11-01.
        const PINNED: u64 = 0xb2f0_8efd_af5a_a4cd;
        let mut hash = crcbl_core::hash::Fnv1a64::new();
        for_each_turn(|turn| {
            for value in scaled_axis(turn).to_array() {
                hash.write(&value.to_bits().to_le_bytes());
            }
        });
        let digest = hash.finish();
        assert_eq!(digest, PINNED, "the scaled axes hash to {digest:#018x}");
    }
}
