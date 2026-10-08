use super::*;

#[test]
fn local_axes_and_planes_translate_and_snap_in_their_rotated_frame() {
    let frame = DQuat::from_rotation_y(0.7) * DQuat::from_rotation_z(0.3);
    let local_start = DVec3::new(0.25, 0.5, 0.75);
    let start = frame * local_start;
    let eye = (start + frame * DVec3::new(6.0, 5.0, 8.0)).as_vec3();
    let pointer = |delta: DVec3| Pointer {
        ray: ray_through(eye, (start + frame * delta).as_vec3()),
        at: Vec2::ZERO,
    };
    for (grip, delta, expected) in [
        (
            Grip::Move(Axis::X),
            DVec3::new(1.27, 0.0, 0.0),
            DVec3::new(1.5, 0.5, 0.75),
        ),
        (
            Grip::MovePlane(Plane::Xy),
            DVec3::new(1.27, -0.63, 0.0),
            DVec3::new(1.5, -0.25, 0.75),
        ),
    ] {
        let drag = Drag::begin(
            SceneEntityId(1),
            grip,
            Gesture(1),
            start.to_array(),
            start,
            frame,
            &pointer(DVec3::ZERO),
            1.0,
        )
        .unwrap();
        let read = |snap| {
            let writes = drag.translated(&pointer(delta), snap).unwrap();
            let mut value = start;
            for (axis, number) in writes {
                value[axis.index()] = number;
            }
            value
        };
        assert!(read(None).abs_diff_eq(start + frame * delta, 1e-5));
        assert!(read(Some(Snap::default())).abs_diff_eq(frame * expected, 1e-5));

        let group = Group {
            members: vec![
                Member {
                    entity: SceneEntityId(1),
                    system: "blocks".into(),
                    start: start.to_array(),
                },
                Member {
                    entity: SceneEntityId(2),
                    system: "blocks".into(),
                    start: (start + DVec3::Y).to_array(),
                },
            ],
        };
        let writes = drag.spread(&group, &pointer(delta), None).unwrap();
        for (member, write) in writes {
            let axis: usize = write
                .path
                .strip_prefix("position.")
                .unwrap()
                .parse()
                .unwrap();
            let expected = DVec3::from_array(member.start) + frame * delta;
            assert!((write.value - expected[axis]).abs() < 1e-5);
        }
    }
}

#[test]
fn local_rings_rotate_about_the_objects_axis() {
    let frame = DQuat::from_rotation_y(0.7);
    let centre = Vec2::new(300.0, 200.0);
    let from = Turn {
        rotation: frame,
        position: None,
        centre,
        facing: 1.0,
    };
    let drag = Drag::turn(
        SceneEntityId(1),
        Axis::X,
        Gesture(1),
        DVec3::ZERO,
        centre + Vec2::X * 60.0,
        from,
        Space::Local,
    );
    let writes = drag
        .writes(&pointer_at(centre - Vec2::Y * 60.0), None)
        .unwrap();
    let expected = frame * DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2);
    assert!(rotation_written(&writes).abs_diff_eq(expected, 1e-12));
}

#[test]
fn drawn_local_arrows_and_rings_follow_the_same_frame_as_drags() {
    let camera = camera();
    let frame = DQuat::from_rotation_y(0.7);
    let centre = camera.pixel_of(Vec3::ZERO, EXTENT).unwrap();
    let handles = handles(&camera, EXTENT, Vec3::ZERO, frame, 1.0, Mode::Translate);
    let arrows = lines(&handles);
    assert!(!arrows.is_empty());
    for (grip, from, to) in arrows {
        let axis = axis_of(grip);
        let projected = camera
            .pixel_of((frame * axis.unit()).as_vec3() * 0.1, EXTENT)
            .unwrap();
        assert!(
            (to - from)
                .normalize()
                .abs_diff_eq((projected - centre).normalize(), 1e-5)
        );
    }
    let quarter = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
    let local = ring(&camera, EXTENT, Vec3::ZERO, quarter, Axis::Z, 1.0).unwrap();
    let world = ring(&camera, EXTENT, Vec3::ZERO, DQuat::IDENTITY, Axis::X, 1.0).unwrap();
    for point in local {
        assert!(distance_to_ring(point, &world) < 0.1);
    }
}
