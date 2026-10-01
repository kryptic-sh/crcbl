use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;
use crcbl::store::MemoryStorage;
use crcbl::store::settings::SettingsStack;

use super::*;
use crate::command::Gesture;

const EXTENT: (u32, u32) = (800, 600);

/// A camera up and to the side of the origin, looking at it, so no axis
/// points along the view and no plane is seen edge-on.
fn camera() -> Camera {
    Camera {
        eye: Vec3::new(6.0, 5.0, 8.0),
        target: Vec3::ZERO,
        ..Camera::default()
    }
}

/// The line handles, as `(grip, from, to)`.
fn lines(handles: &[Handle]) -> Vec<(Grip, Vec2, Vec2)> {
    handles
        .iter()
        .filter_map(|handle| match handle.shape {
            Shape::Line { from, to } => Some((handle.grip, from, to)),
            Shape::Square { .. } => None,
        })
        .collect()
}

/// The square handles, as `(grip, centre, half)`.
fn squares(handles: &[Handle]) -> Vec<(Grip, Vec2, f32)> {
    handles
        .iter()
        .filter_map(|handle| match handle.shape {
            Shape::Square { centre, half } => Some((handle.grip, centre, half)),
            Shape::Line { .. } => None,
        })
        .collect()
}

/// The axis a line handle's grip names.
fn axis_of(grip: Grip) -> Axis {
    match grip {
        Grip::Move(axis) | Grip::Scale(axis) => axis,
        other => panic!("{other:?} is not an axis handle"),
    }
}

/// A ray from `eye` through `point`.
fn ray_through(eye: Vec3, point: Vec3) -> ViewRay {
    ViewRay {
        origin: eye,
        direction: (point - eye).normalize(),
    }
}

/// A ray straight down onto `x` on the X axis.
fn down_onto(x: f32) -> Pointer {
    Pointer {
        ray: ViewRay {
            origin: Vec3::new(x, 10.0, 0.0),
            direction: Vec3::NEG_Y,
        },
        at: Vec2::ZERO,
    }
}

/// The single write of `writes`, as `(path, value)`.
fn only(writes: &[Write]) -> (&str, f64) {
    let [write] = writes else {
        panic!("one write expected, got {writes:?}");
    };
    (write.path.as_str(), write.value)
}

/// **Each axis handle is [`HANDLE_PX`] long and points where its axis goes
/// on screen**, in both modes: the far end of a world step along the axis
/// projects onto the handle's own line, beyond its start.
#[test]
fn each_handle_points_along_its_axis_on_screen() {
    let camera = camera();
    for mode in [Mode::Translate, Mode::Scale] {
        let lines = lines(&handles(&camera, EXTENT, Vec3::ZERO, 1.0, mode));
        assert_eq!(lines.len(), 3, "{mode:?}");
        for (grip, from, to) in lines {
            assert_eq!(grip.mode(), mode);
            assert!((from.distance(to) - HANDLE_PX).abs() < 1e-3);
            let ahead = camera
                .pixel_of(narrow(axis_of(grip).unit()) * 2.0, EXTENT)
                .expect("in front");
            let along_handle = (to - from).normalize();
            let along_axis = (ahead - from).normalize();
            assert!(
                along_handle.dot(along_axis) > 0.999,
                "{grip:?} points {along_handle:?}, its axis goes {along_axis:?}",
            );
        }
    }
}

/// **A handle is the same size on screen however far away the selection
/// is** — the plan's constant screen-size scaling — and twice as many
/// physical pixels at twice the scale; squares included.
#[test]
fn a_handle_is_the_same_size_near_and_far() {
    let camera = camera();
    for origin in [Vec3::ZERO, Vec3::new(-20.0, -10.0, -40.0)] {
        for scale in [1.0, 2.0] {
            for mode in [Mode::Translate, Mode::Scale] {
                let handles = handles(&camera, EXTENT, origin, scale, mode);
                for (_, from, to) in lines(&handles) {
                    let length = from.distance(to);
                    assert!((length - HANDLE_PX * scale).abs() < 1e-3, "{length}");
                }
                for (grip, _, half) in squares(&handles) {
                    let side = match grip {
                        Grip::MovePlane(_) => PLANE_PX,
                        _ => CENTRE_PX,
                    };
                    assert!((half - side * scale).abs() < 1e-6, "{grip:?} {half}");
                }
            }
        }
    }
}

/// An axis pointing straight at the camera has no handle, nor has either
/// plane holding it; and a centre behind the eye has none at all.
#[test]
fn an_axis_along_the_view_and_a_centre_behind_the_eye_have_no_handle() {
    let looking_down_z = Camera {
        eye: Vec3::new(0.0, 0.0, 10.0),
        target: Vec3::ZERO,
        ..Camera::default()
    };
    let handles = handles(&looking_down_z, EXTENT, Vec3::ZERO, 1.0, Mode::Translate);
    let grips: Vec<Grip> = handles.iter().map(|handle| handle.grip).collect();
    assert_eq!(
        grips,
        [
            Grip::Move(Axis::X),
            Grip::Move(Axis::Y),
            Grip::MovePlane(Plane::Xy)
        ]
    );
    for mode in [Mode::Translate, Mode::Scale] {
        assert!(
            handles_behind(&looking_down_z, mode).is_empty(),
            "{mode:?} drew a centre behind the eye"
        );
    }
}

/// The handles for a centre behind `camera`'s eye.
fn handles_behind(camera: &Camera, mode: Mode) -> Vec<Handle> {
    handles(camera, EXTENT, Vec3::new(0.0, 0.0, 20.0), 1.0, mode)
}

/// **A plane handle sits in the corner between its own two arrows**: its
/// square's centre is a positive amount of each of their screen directions,
/// and of nothing else.
#[test]
fn a_plane_handle_sits_between_its_two_axes() {
    let handles = handles(&camera(), EXTENT, Vec3::ZERO, 1.0, Mode::Translate);
    let lines = lines(&handles);
    let squares = squares(&handles);
    assert_eq!(squares.len(), 3, "every plane is in view: {squares:?}");
    for (grip, centre, _) in squares {
        let Grip::MovePlane(plane) = grip else {
            panic!("translate has no {grip:?}");
        };
        let [a, b] = plane.axes().map(|axis| {
            let (_, from, to) = lines
                .iter()
                .find(|(grip, ..)| axis_of(*grip) == axis)
                .expect("every axis is in view");
            (*from, (*to - *from) / HANDLE_PX)
        });
        let (from, (da, db)) = (a.0, (a.1, b.1));
        // Solve `centre - from = s·da + t·db` for `s` and `t`.
        let offset = centre - from;
        let det = da.perp_dot(db);
        let s = offset.perp_dot(db) / det;
        let t = da.perp_dot(offset) / det;
        let expected = HANDLE_PX * PLANE_AT;
        assert!(
            (s - expected).abs() < 1e-3 && (t - expected).abs() < 1e-3,
            "{plane:?}'s square is {s} along {:?} and {t} along {:?}",
            plane.axes()[0],
            plane.axes()[1],
        );
    }
}

/// **A plane seen nearly edge-on has no handle**, though both its axes do —
/// its drag would move the entity metres a pixel.
#[test]
fn a_plane_seen_edge_on_has_no_handle() {
    // Level with the ZX plane, a little above it.
    let level = Camera {
        eye: Vec3::new(6.0, 0.5, 8.0),
        target: Vec3::ZERO,
        ..Camera::default()
    };
    let handles = handles(&level, EXTENT, Vec3::ZERO, 1.0, Mode::Translate);
    let axes: Vec<Axis> = lines(&handles)
        .into_iter()
        .map(|(grip, ..)| axis_of(grip))
        .collect();
    assert_eq!(axes, Axis::ALL, "the plane's own axes are both in view");
    let planes: Vec<Grip> = squares(&handles)
        .into_iter()
        .map(|(grip, ..)| grip)
        .collect();
    assert_eq!(
        planes,
        [Grip::MovePlane(Plane::Xy), Grip::MovePlane(Plane::Yz)]
    );
}

/// **Scale shows a line per axis and a square at the centre, and no
/// planes** — and a press at the centre takes the centre square although
/// every line starts there.
#[test]
fn scale_shows_an_axis_each_and_a_centre_that_wins_where_the_lines_meet() {
    let handles = handles(&camera(), EXTENT, Vec3::ZERO, 1.0, Mode::Scale);
    let grips: Vec<Grip> = handles.iter().map(|handle| handle.grip).collect();
    assert_eq!(
        grips,
        [
            Grip::Scale(Axis::X),
            Grip::Scale(Axis::Y),
            Grip::Scale(Axis::Z),
            Grip::ScaleAll
        ]
    );
    let (_, centre, _) = squares(&handles)[0];
    let lines_only: Vec<Handle> = handles
        .iter()
        .copied()
        .filter(|handle| matches!(handle.shape, Shape::Line { .. }))
        .collect();
    assert!(
        hit(&lines_only, centre, 1.0).is_some(),
        "no line reaches the centre, so the square has nothing to win over",
    );
    assert_eq!(hit(&handles, centre, 1.0), Some(Grip::ScaleAll));
}

/// A press near a handle's tip takes it, one beside the line misses, a press
/// on a plane square takes the plane, and a press far from all of them takes
/// nothing.
#[test]
fn a_press_takes_the_handle_under_it() {
    let handles = handles(&camera(), EXTENT, Vec3::ZERO, 1.0, Mode::Translate);
    for (grip, from, to) in lines(&handles) {
        // Towards the tip, clear of the plane squares in the corners.
        let near_tip = from + (to - from) * 0.85;
        assert_eq!(hit(&handles, near_tip, 1.0), Some(grip));
        let normal = (to - from).normalize().perp();
        let beside = to + normal * (HIT_PX * 3.0);
        assert_ne!(hit(&handles, beside, 1.0), Some(grip));
    }
    for (grip, centre, _) in squares(&handles) {
        assert_eq!(hit(&handles, centre, 1.0), Some(grip));
    }
    let (_, from, _) = lines(&handles)[0];
    let far = from + Vec2::splat(HANDLE_PX * 4.0);
    assert_eq!(hit(&handles, far, 1.0), None);
}

/// **Where a plane square lies over an axis's line, the square takes the
/// press** — the line keeps the rest of its length. Laid out by hand, so the
/// overlap is certain rather than a property of one camera.
#[test]
fn a_press_on_a_plane_square_takes_it_over_the_axis_behind_it() {
    let handles = [
        Handle {
            grip: Grip::Move(Axis::X),
            shape: Shape::Line {
                from: Vec2::ZERO,
                to: Vec2::new(HANDLE_PX, 0.0),
            },
        },
        Handle {
            grip: Grip::MovePlane(Plane::Xy),
            shape: Shape::Square {
                centre: Vec2::new(30.0, 4.0),
                half: PLANE_PX,
            },
        },
    ];
    let on_both = Vec2::new(30.0, 0.0);
    assert_eq!(
        hit(&handles[..1], on_both, 1.0),
        Some(Grip::Move(Axis::X)),
        "the line does not reach the press, so the square won nothing",
    );
    assert_eq!(
        hit(&handles, on_both, 1.0),
        Some(Grip::MovePlane(Plane::Xy))
    );
    assert_eq!(
        hit(&handles, Vec2::new(70.0, 0.0), 1.0),
        Some(Grip::Move(Axis::X)),
        "the line was lost outside the square too",
    );
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

/// **A ray crosses a plane where it is aimed**, and a ray along the plane
/// or pointing away from it crosses nowhere.
#[test]
fn on_plane_finds_where_the_ray_crosses() {
    let anchor = DVec3::new(0.0, 0.0, 0.5);
    let down = ViewRay {
        origin: Vec3::new(1.0, 2.0, 10.0),
        direction: Vec3::NEG_Z,
    };
    let crossing = on_plane(&down, anchor, Axis::Z).expect("crosses");
    assert!(
        (crossing - DVec3::new(1.0, 2.0, 0.5)).length() < 1e-9,
        "{crossing}"
    );
    let oblique = ray_through(Vec3::new(6.0, 5.0, 8.0), Vec3::new(-1.0, 3.0, 0.5));
    let crossing = on_plane(&oblique, anchor, Axis::Z).expect("crosses");
    assert!(
        (crossing - DVec3::new(-1.0, 3.0, 0.5)).length() < 1e-5,
        "{crossing}"
    );
    let level = ViewRay {
        origin: Vec3::new(0.0, 0.0, 0.5),
        direction: Vec3::X,
    };
    assert_eq!(on_plane(&level, anchor, Axis::Z), None);
    let away = ViewRay {
        origin: Vec3::new(1.0, 2.0, 10.0),
        direction: Vec3::Z,
    };
    assert_eq!(on_plane(&away, anchor, Axis::Z), None);
}

/// **An axis drag moves by as far as the cursor moved along the axis, from
/// where it grabbed**, and writes that one leaf.
#[test]
fn a_drag_moves_by_the_distance_along_the_axis() {
    let drag = Drag::begin(
        SceneEntityId(1),
        Grip::Move(Axis::X),
        Gesture(1),
        [-3.0, 0.25, 0.0],
        DVec3::new(-3.0, 0.25, 0.0),
        &down_onto(-2.5),
        1.0,
    )
    .expect("not parallel");
    let writes = drag.writes(&down_onto(-2.5), None).expect("not parallel");
    assert_eq!(only(&writes), ("position.0", -3.0), "no move, no change");
    let writes = drag.writes(&down_onto(-1.3), None).expect("not parallel");
    let (path, moved) = only(&writes);
    assert_eq!(path, "position.0");
    assert!((moved - (-1.8)).abs() < 1e-6, "{moved}");
}

/// **A plane drag moves in exactly that plane**: both its axes by as far as
/// the crossing moved, through oblique rays, and the third never written.
#[test]
fn a_plane_drag_moves_in_exactly_that_plane() {
    let eye = Vec3::new(6.0, 5.0, 8.0);
    let start = [0.5, 1.0, -2.0];
    let origin = DVec3::from_array(start);
    let at = |x: f32, y: f32| Pointer {
        ray: ray_through(eye, Vec3::new(x, y, -2.0)),
        at: Vec2::ZERO,
    };
    let drag = Drag::begin(
        SceneEntityId(1),
        Grip::MovePlane(Plane::Xy),
        Gesture(1),
        start,
        origin,
        &at(1.0, 1.0),
        1.0,
    )
    .expect("the ray crosses the plane");
    let writes = drag.writes(&at(2.5, -0.5), None).expect("crosses");
    let paths: Vec<&str> = writes.iter().map(|write| write.path.as_str()).collect();
    assert_eq!(
        paths,
        ["position.0", "position.1"],
        "it wrote off its plane"
    );
    assert!((writes[0].value - 2.0).abs() < 1e-4, "{writes:?}");
    assert!((writes[1].value - (-0.5)).abs() < 1e-4, "{writes:?}");

    // Across ZX the same way: X and Z move, Y does not.
    let at = |x: f32, z: f32| Pointer {
        ray: ray_through(eye, Vec3::new(x, 1.0, z)),
        at: Vec2::ZERO,
    };
    let drag = Drag::begin(
        SceneEntityId(1),
        Grip::MovePlane(Plane::Zx),
        Gesture(1),
        start,
        origin,
        &at(0.5, -2.0),
        1.0,
    )
    .expect("the ray crosses the plane");
    let writes = drag.writes(&at(1.5, -1.0), None).expect("crosses");
    let paths: Vec<&str> = writes.iter().map(|write| write.path.as_str()).collect();
    assert_eq!(
        paths,
        ["position.2", "position.0"],
        "it wrote off its plane"
    );
    assert!((writes[0].value - (-1.0)).abs() < 1e-4, "{writes:?}");
    assert!((writes[1].value - 1.5).abs() < 1e-4, "{writes:?}");
}

/// **A scale axis adds the cursor's travel along its axis to that half
/// extent alone.**
#[test]
fn a_scale_drag_changes_one_half_extent_by_the_distance() {
    let drag = Drag::begin(
        SceneEntityId(1),
        Grip::Scale(Axis::X),
        Gesture(1),
        [1.2, 0.25, 1.5],
        DVec3::ZERO,
        &down_onto(1.0),
        1.0,
    )
    .expect("not parallel");
    let writes = drag.writes(&down_onto(1.5), None).expect("not parallel");
    let (path, half) = only(&writes);
    assert_eq!(path, "half_extents.0");
    assert!((half - 1.7).abs() < 1e-6, "{half}");
}

/// **The centre scales all three half extents in proportion**: a pointer
/// half a handle to the right is half as large again.
#[test]
fn the_centre_scales_every_half_extent_in_proportion() {
    let pixel = |x: f32| Pointer {
        ray: down_onto(0.0).ray,
        at: Vec2::new(x, 50.0),
    };
    for scale in [1.0, 2.0] {
        let drag = Drag::begin(
            SceneEntityId(1),
            Grip::ScaleAll,
            Gesture(1),
            [1.2, 0.25, 1.5],
            DVec3::ZERO,
            &pixel(100.0),
            scale,
        )
        .expect("the centre always takes hold");
        let writes = drag
            .writes(&pixel(100.0 + HANDLE_PX * scale * 0.5), None)
            .expect("the centre has no ray to lose");
        let paths: Vec<&str> = writes.iter().map(|write| write.path.as_str()).collect();
        assert_eq!(
            paths,
            ["half_extents.0", "half_extents.1", "half_extents.2"]
        );
        for (write, expected) in writes.iter().zip([1.8, 0.375, 2.25]) {
            assert!(
                (write.value - expected).abs() < 1e-9,
                "{write:?} at {scale}"
            );
        }
    }
}

/// **A half extent is never written at or below zero**: an axis dragged
/// through the centre, the centre dragged far left, and a snapped value
/// that rounds to nothing all stop at [`MIN_HALF_EXTENT`].
#[test]
fn a_half_extent_stops_at_the_minimum() {
    let axis = Drag::begin(
        SceneEntityId(1),
        Grip::Scale(Axis::X),
        Gesture(1),
        [1.2, 0.25, 1.5],
        DVec3::ZERO,
        &down_onto(1.0),
        1.0,
    )
    .expect("not parallel");
    let writes = axis.writes(&down_onto(-5.0), None).expect("not parallel");
    assert_eq!(only(&writes), ("half_extents.0", MIN_HALF_EXTENT));

    let small = Drag::begin(
        SceneEntityId(1),
        Grip::Scale(Axis::X),
        Gesture(1),
        [0.05, 0.25, 1.5],
        DVec3::ZERO,
        &down_onto(1.0),
        1.0,
    )
    .expect("not parallel");
    let writes = small
        .writes(&down_onto(0.99), Some(Snap::default()))
        .expect("not parallel");
    assert_eq!(only(&writes), ("half_extents.0", MIN_HALF_EXTENT));

    let pixel = |x: f32| Pointer {
        ray: down_onto(0.0).ray,
        at: Vec2::new(x, 0.0),
    };
    let centre = Drag::begin(
        SceneEntityId(1),
        Grip::ScaleAll,
        Gesture(1),
        [1.2, 0.25, 1.5],
        DVec3::ZERO,
        &pixel(0.0),
        1.0,
    )
    .expect("the centre always takes hold");
    let writes = centre.writes(&pixel(-1000.0), None).expect("no ray");
    assert!(
        writes.iter().all(|write| write.value == MIN_HALF_EXTENT),
        "{writes:?}"
    );
}

/// **Snapping lands on the absolute grid, not on steps from the press**: a
/// centre that starts off the grid snaps onto it, which steps from the
/// press never would, and a half extent lands on the scale step.
#[test]
fn snapping_lands_on_absolute_grid_multiples_from_an_off_grid_start() {
    let snap = Snap::default();
    let drag = Drag::begin(
        SceneEntityId(1),
        Grip::Move(Axis::X),
        Gesture(1),
        [-2.9, 0.0, 0.0],
        DVec3::new(-2.9, 0.0, 0.0),
        &down_onto(-2.9),
        1.0,
    )
    .expect("not parallel");
    let writes = drag
        .writes(&down_onto(-2.6), Some(snap))
        .expect("not parallel");
    // Unsnapped this is -2.6, and a step from the press would be -2.65.
    assert_eq!(only(&writes), ("position.0", -2.5));

    let eye = Vec3::new(6.0, 5.0, 8.0);
    let at = |x: f32, y: f32| Pointer {
        ray: ray_through(eye, Vec3::new(x, y, 0.0)),
        at: Vec2::ZERO,
    };
    let plane = Drag::begin(
        SceneEntityId(1),
        Grip::MovePlane(Plane::Xy),
        Gesture(1),
        [0.1, 0.3, 0.0],
        DVec3::new(0.1, 0.3, 0.0),
        &at(0.1, 0.3),
        1.0,
    )
    .expect("crosses");
    let writes = plane.writes(&at(0.3, 0.5), Some(snap)).expect("crosses");
    let values: Vec<f64> = writes.iter().map(|write| write.value).collect();
    assert_eq!(values, [0.25, 0.5], "unsnapped (0.3, 0.5)");

    let scale = Drag::begin(
        SceneEntityId(1),
        Grip::Scale(Axis::X),
        Gesture(1),
        [1.2, 0.25, 1.5],
        DVec3::ZERO,
        &down_onto(1.0),
        1.0,
    )
    .expect("not parallel");
    let writes = scale
        .writes(&down_onto(1.33), Some(snap))
        .expect("not parallel");
    assert_eq!(only(&writes), ("half_extents.0", 1.5), "unsnapped 1.53");
}

/// **The steps are read from the settings**, and one that is not a
/// positive number — or not set — is the default.
#[test]
fn the_snap_steps_come_from_the_settings_or_their_defaults() {
    let mut stack = SettingsStack::from_storage(&MemoryStorage::new());
    assert_eq!(Snap::load(&stack), Snap::default(), "nothing set");

    stack.set(GRID_KEY, &0.5).expect("a writable stack");
    stack.set(SCALE_KEY, &0.1).expect("a writable stack");
    let snap = Snap::load(&stack);
    assert_eq!((snap.grid_step(), snap.scale_step()), (0.5, 0.1));
    assert_eq!(snap.grid(-2.9), -3.0);

    for (grid, scale) in [(0.0, "fine"), (-1.0, "0.1")] {
        stack.set(GRID_KEY, &grid).expect("a writable stack");
        stack.set(SCALE_KEY, &scale).expect("a writable stack");
        assert_eq!(
            Snap::load(&stack),
            Snap::default(),
            "a grid of {grid} or a scale of {scale:?} was taken",
        );
    }
}
