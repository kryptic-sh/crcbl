//! The camera a player plays from: fixed, overhead and looking down the field.
//!
//! # It has no controls, and that is a decision
//!
//! What a tower defense is read from is a view of the whole field at once, and
//! a camera a player had to fly into position would be one where nothing else
//! can be looked at until they have. So this module is a constant and a
//! projection, and the run opens on it. The fly and walk cameras
//! `docs/plan/sample/07-towers.md` asks for are a **dev** camera beside it —
//! [`crate::dev_camera`] — which `C` switches to and back, landing on this view
//! exactly.
//!
//! The one thing this does have to be is **wide enough**: every corner of
//! `crate::map`'s field has to be in frame at the default window's aspect, or
//! part of the path is somewhere a player cannot see a creep walking.
//! `the_whole_field_is_in_frame` asserts exactly that, by projecting the four
//! corners through the matrix the frame is drawn with.

use crcbl::math::Vec3;
use crcbl::render::{Camera, Projection};

/// Where the camera stands, in metres: high above the near edge of the field,
/// looking back at its centre.
pub const EYE: Vec3 = Vec3::new(0.0, 32.0, 23.0);

/// What it looks at: the middle of the field.
pub const TARGET: Vec3 = Vec3::ZERO;

/// The vertical field of view, in radians.
///
/// 55°, which is what it takes for the far corners of the field to be in frame
/// from [`EYE`] at a four-by-three window — see the module docs.
pub const FOV_Y: f32 = 55.0 * (core::f32::consts::PI / 180.0);

/// The near plane, in metres. The camera is thirty-odd metres from anything, so
/// there is nothing to be gained by putting it closer.
pub const NEAR: f32 = 0.5;

/// The view this sample is drawn from.
#[must_use]
pub fn camera() -> Camera {
    Camera {
        eye: EYE,
        target: TARGET,
        up: Vec3::Y,
        projection: Projection::Perspective {
            fov_y: FOV_Y,
            near: NEAR,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{HALF_DEPTH, HALF_WIDTH};

    /// The aspect of the default window — see `crcbl::args::Common`'s
    /// `--size`, whose default this sample inherits.
    const ASPECT: f32 = 960.0 / 720.0;

    /// **Every corner of the field is in frame**, projected through the very
    /// matrix `crate::gpu` draws with rather than reasoned about from the field
    /// of view.
    ///
    /// The margin is asserted too: a camera that fitted the field exactly would
    /// put the corner posts on the edge of the window, where a creep walking
    /// the outer leg is half off screen.
    #[test]
    fn the_whole_field_is_in_frame() {
        let view_projection = camera().view_projection(ASPECT);
        for x in [-HALF_WIDTH, HALF_WIDTH] {
            for z in [-HALF_DEPTH, HALF_DEPTH] {
                #[allow(clippy::cast_possible_truncation)]
                let corner = Vec3::new(x as f32, 0.0, z as f32);
                let clip = view_projection * corner.extend(1.0);
                assert!(clip.w > 0.0, "{corner:?} is behind the camera");
                let ndc = clip.truncate() / clip.w;
                assert!(
                    ndc.x.abs() < 0.95 && ndc.y.abs() < 0.95,
                    "{corner:?} lands at {ndc:?}, on or off the edge of the window",
                );
            }
        }
    }

    /// **The camera is above the field and looking at it**, which is what makes
    /// the overhead view overhead. Without this the check above would pass for
    /// a camera lying on the ground at one end.
    #[test]
    fn the_camera_looks_down_at_the_field() {
        let camera = camera();
        assert!(camera.eye.y > 2.0 * HALF_DEPTH as f32, "the camera is low");
        assert!(
            (camera.target - camera.eye).normalize().y < -0.5,
            "the camera is not looking down",
        );
    }
}
