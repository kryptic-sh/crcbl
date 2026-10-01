use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;

use super::*;
use crate::command::Gesture;

const EXTENT: (u32, u32) = (800, 600);

/// A camera up and to the side of the origin, looking at it, so no axis
/// points along the view.
fn camera() -> Camera {
    Camera {
        eye: Vec3::new(6.0, 5.0, 8.0),
        target: Vec3::ZERO,
        ..Camera::default()
    }
}

/// **Each handle is [`HANDLE_PX`] long and points where its axis goes on
/// screen**: the far end of a world step along the axis projects onto the
/// handle's own line, beyond its start.
#[test]
fn each_handle_points_along_its_axis_on_screen() {
    let camera = camera();
    let handles = handles(&camera, EXTENT, Vec3::ZERO, 1.0);
    assert_eq!(handles.len(), 3);
    for handle in &handles {
        assert!((handle.from.distance(handle.to) - HANDLE_PX).abs() < 1e-3);
        let ahead = camera
            .pixel_of(narrow(handle.axis.unit()) * 2.0, EXTENT)
            .expect("in front");
        let along_handle = (handle.to - handle.from).normalize();
        let along_axis = (ahead - handle.from).normalize();
        assert!(
            along_handle.dot(along_axis) > 0.999,
            "{:?} points {along_handle:?}, its axis goes {along_axis:?}",
            handle.axis,
        );
    }
}

/// **The handle is the same size on screen however far away the selection
/// is** — the plan's constant screen-size scaling — and twice as many
/// physical pixels at twice the scale.
#[test]
fn a_handle_is_the_same_size_near_and_far() {
    let camera = camera();
    for origin in [Vec3::ZERO, Vec3::new(-20.0, -10.0, -40.0)] {
        for scale in [1.0, 2.0] {
            for handle in handles(&camera, EXTENT, origin, scale) {
                let length = handle.from.distance(handle.to);
                assert!((length - HANDLE_PX * scale).abs() < 1e-3, "{length}");
            }
        }
    }
}

/// An axis pointing straight at the camera has no handle, and a centre
/// behind the eye has none at all.
#[test]
fn an_axis_along_the_view_and_a_centre_behind_the_eye_have_no_handle() {
    let looking_down_z = Camera {
        eye: Vec3::new(0.0, 0.0, 10.0),
        target: Vec3::ZERO,
        ..Camera::default()
    };
    let axes: Vec<Axis> = handles(&looking_down_z, EXTENT, Vec3::ZERO, 1.0)
        .iter()
        .map(|handle| handle.axis)
        .collect();
    assert_eq!(axes, [Axis::X, Axis::Y]);
    assert!(handles(&looking_down_z, EXTENT, Vec3::new(0.0, 0.0, 20.0), 1.0).is_empty());
}

/// A press on a handle's line takes it, one beside the line misses, and
/// the nearest of two takes it where they meet.
#[test]
fn a_press_takes_the_handle_under_it() {
    let handles = handles(&camera(), EXTENT, Vec3::ZERO, 1.0);
    for handle in &handles {
        let middle = (handle.from + handle.to) * 0.5;
        assert_eq!(hit(&handles, middle, 1.0), Some(handle.axis));
        let normal = (handle.to - handle.from).normalize().perp();
        let beside = handle.to + normal * (HIT_PX * 3.0);
        assert_ne!(hit(&handles, beside, 1.0), Some(handle.axis));
    }
    let far = handles[0].from + Vec2::splat(HANDLE_PX * 4.0);
    assert_eq!(hit(&handles, far, 1.0), None);
}

/// **The closest point on the axis is the one a ray aimed at it passes
/// through**, against values worked by hand.
#[test]
fn along_finds_the_closest_point_on_the_axis() {
    // A ray straight down onto x = 3 meets the X axis there.
    let down = ViewRay {
        origin: Vec3::new(3.0, 10.0, 0.0),
        direction: Vec3::NEG_Y,
    };
    let t = along(&down, DVec3::ZERO, Axis::X).expect("not parallel");
    assert!((t - 3.0).abs() < 1e-9, "{t}");
    // From an anchor at x = 1 the same point is 2 along.
    let t = along(&down, DVec3::new(1.0, 0.0, 0.0), Axis::X).expect("not parallel");
    assert!((t - 2.0).abs() < 1e-9, "{t}");
    // A skew ray passing above the Z axis at z = -4.
    let skew = ViewRay {
        origin: Vec3::new(-5.0, 2.0, -4.0),
        direction: Vec3::X,
    };
    let t = along(&skew, DVec3::ZERO, Axis::Z).expect("not parallel");
    assert!((t + 4.0).abs() < 1e-9, "{t}");
    // An oblique ray through x = 5 on the axis, so the `b` term is not
    // zero: every ray above is square to its axis and would pass with
    // that term's sign wrong.
    let oblique = ViewRay {
        origin: Vec3::new(1.0, 2.0, 2.0),
        direction: Vec3::new(4.0, -2.0, -2.0).normalize(),
    };
    let t = along(&oblique, DVec3::ZERO, Axis::X).expect("not parallel");
    assert!((t - 5.0).abs() < 1e-5, "{t}");
    // And a ray along the axis has no closest point.
    let parallel = ViewRay {
        origin: Vec3::new(0.0, 1.0, 0.0),
        direction: Vec3::X,
    };
    assert_eq!(along(&parallel, DVec3::ZERO, Axis::X), None);
}

/// **A drag moves by as far as the cursor moved along the axis, from where
/// it grabbed**, and snaps that distance to whole steps.
#[test]
fn a_drag_moves_by_the_distance_along_the_axis() {
    let at = |x: f32| ViewRay {
        origin: Vec3::new(x, 10.0, 0.0),
        direction: Vec3::NEG_Y,
    };
    let drag = Drag::begin(
        SceneEntityId(1),
        Axis::X,
        Gesture(1),
        -3.0,
        DVec3::new(-3.0, 0.25, 0.0),
        &at(-2.5),
    )
    .expect("not parallel");
    assert_eq!(
        drag.value(&at(-2.5), false),
        Some(-3.0),
        "no move, no change"
    );
    let moved = drag.value(&at(-1.3), false).expect("not parallel");
    assert!((moved - (-1.8)).abs() < 1e-6, "{moved}");
    let snapped = drag.value(&at(-1.3), true).expect("not parallel");
    assert!((snapped - (-1.75)).abs() < 1e-12, "{snapped}");
}
