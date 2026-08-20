//! Quarry's two cameras: the fixed dolly, and the free-fly camera a reviewer
//! walks the face with.
//!
//! `docs/plan/sample/14-quarry.md`'s Scope asks for "free-fly camera plus a
//! fixed dolly for goldens and the hysteresis check". Both live here, in the
//! library, rather than in the front end or in the device suite — because **the
//! window and the goldens have to fly the same path**. A windowed run whose
//! fixed pose is not the pose `tests/golden/` was blessed from produces a
//! picture nobody can hold against the committed one, which is the entire use a
//! fixed camera has. `tests/device/harness.rs` imports these rather than
//! keeping its own copy for that reason.
//!
//! # The free camera is the engine's, at this sample's scale
//!
//! [`Flyer`] is `crcbl-render`'s. What is this sample's is how fast it walks:
//! the engine's [`SPEED`](crcbl::render::SPEED) is sized for a room and this
//! face is [`face::DEPTH_METRES`] deep, so a reviewer flying it at the default
//! would spend over a minute reaching the far end. [`FLY_SPEED`] is the number
//! that fixes that, and it is derived from the face rather than typed in.

use crcbl::math::Vec3;
use crcbl::render::{Camera, Flyer, Projection};

use crate::face;

/// Where the dolly starts: outside the near edge, above the face.
///
/// The face occupies X ∈ ±[`face::WIDTH_METRES`]/2, Y ∈
/// 0..[`face::HEIGHT_METRES`] and Z ∈ 0..[`face::DEPTH_METRES`], and its
/// winding is counter-clockwise seen from +Y — so a camera has to be above it
/// and looking along +Z or it sees the back of every triangle, which is the
/// difference between an empty frame and a picture.
pub const DOLLY_START: f32 = 0.0;

/// Where the dolly ends: most of the way down the face, so the near clusters
/// have gone past the camera and the far ones have come close.
pub const DOLLY_END: f32 = 1.0;

/// The camera `at` along the dolly, `0.0` at [`DOLLY_START`] and `1.0` at
/// [`DOLLY_END`].
///
/// A straight run down the face's own axis, which is the shape
/// `docs/plan/sample/14-quarry.md` asks for: "the fixed dolly". It stays the
/// same height throughout so that what changes between frames is distance and
/// nothing else.
#[must_use]
pub fn dolly(at: f32) -> Camera {
    let z = -30.0 + at * face::DEPTH_METRES * 0.5;
    Camera {
        eye: Vec3::new(0.0, face::HEIGHT_METRES, z),
        target: Vec3::new(0.0, 0.0, z + face::DEPTH_METRES * 0.5),
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// How long a held `W` takes to fly the face end to end, in seconds.
///
/// Twelve: long enough that a reviewer can stop on a cluster boundary and look
/// at it, short enough that getting from the near benches to the far wall is
/// not a wait. It is the input to [`FLY_SPEED`] rather than a number anybody
/// reads on its own.
const FLY_SECONDS: f32 = 12.0;

/// How fast the free camera walks, in metres a second.
///
/// The face's [`face::DEPTH_METRES`] in `FLY_SECONDS` seconds — written as
/// that division so the speed follows the face if the face ever changes size,
/// and so the sentence and the number cannot drift apart. The engine's
/// [`SPEED`](crcbl::render::SPEED) is a brisk walk for a room-sized scene and
/// is far too slow here; `the_fly_speed_is_sized_for_this_face` is what pins
/// the difference.
pub const FLY_SPEED: f32 = face::DEPTH_METRES / FLY_SECONDS;

/// The engine's room speed is not this sample's, checked at compile time.
///
/// A `const` assertion rather than a test, because both sides are constants: a
/// number that made this false would be one nobody could fly the face at, and
/// failing to build is a stronger answer than failing to run. Lower
/// [`FLY_SECONDS`] past a hundred and a half and this is what stops it.
const _: () = assert!(
    FLY_SPEED > crcbl::render::SPEED,
    "a face measured in hundreds of metres cannot be flown at the engine's room-sized default",
);

/// The free camera, standing at [`DOLLY_START`] and walking at [`FLY_SPEED`].
///
/// Starting at the dolly's own first pose is what makes the two cameras' first
/// frame identical: a reviewer who switches to the free camera is looking at
/// the golden's framing until they press a key.
#[must_use]
pub fn flyer() -> Flyer {
    Flyer::at(&dolly(DOLLY_START)).with_speed(FLY_SPEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dolly translates **into** the face, from above it, looking along it.
    ///
    /// All three are what stop the frame being empty: the face's winding is
    /// counter-clockwise from +Y, so a camera below it or looking along −Z sees
    /// backfaces and nothing else.
    #[test]
    fn the_dolly_runs_down_the_face_from_above_it() {
        let start = dolly(DOLLY_START);
        let end = dolly(DOLLY_END);
        assert!(
            end.eye.z > start.eye.z,
            "the dolly must move along +Z, and it went {} → {}",
            start.eye.z,
            end.eye.z,
        );
        for (name, camera) in [("start", start), ("end", end)] {
            assert!(
                camera.eye.y >= face::HEIGHT_METRES,
                "the {name} pose is inside the rock at y={}",
                camera.eye.y,
            );
            assert!(
                camera.target.z > camera.eye.z,
                "the {name} pose looks back down the face",
            );
        }
    }

    /// **The speed is the face's depth over the stated time**, and the flyer is
    /// built with it.
    ///
    /// The second half is the one that catches the real mistake: a
    /// [`Flyer::at`] with no `with_speed` compiles, flies, and takes a minute
    /// and a quarter to cross this face. That it is faster than the engine's
    /// room-sized default is the `const` assertion above, which fails the build
    /// rather than a test.
    #[test]
    fn the_fly_speed_is_sized_for_this_face() {
        assert!(
            (FLY_SPEED * FLY_SECONDS - face::DEPTH_METRES).abs() < 1e-3,
            "{FLY_SPEED} m/s for {FLY_SECONDS}s does not cross {} m",
            face::DEPTH_METRES,
        );
        assert!(
            (flyer().speed() - FLY_SPEED).abs() < f32::EPSILON,
            "the flyer was built without the speed this sample sized for it",
        );
    }

    /// The free camera starts where the fixed one does, so the first frame of
    /// either is the same picture.
    #[test]
    fn the_free_camera_starts_at_the_dolly_start() {
        let fixed = dolly(DOLLY_START);
        let free = flyer().camera(fixed.projection);
        assert_eq!(free.eye, fixed.eye);
        assert!(
            (free.target - free.eye)
                .normalize()
                .dot((fixed.target - fixed.eye).normalize())
                > 0.999,
            "the free camera looks somewhere else: {:?} against {:?}",
            free.target - free.eye,
            fixed.target - fixed.eye,
        );
        assert!(!flyer().has_moved());
    }
}
