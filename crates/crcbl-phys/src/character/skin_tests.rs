use super::{CharacterConfig, CharacterController};
use crate::collider::BoxCollider;
use crate::world::PhysicsWorld;
use glam::DVec3;

fn beside_wall() -> (PhysicsWorld, CharacterController) {
    let config = CharacterConfig::default();
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -0.5, 0.0),
        DVec3::new(10.0, 0.5, 10.0),
    ));
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, -(config.radius + config.skin_width + 0.1)),
        DVec3::new(5.0, 1.0, 0.1),
    ));
    let mut character = CharacterController::new(
        config,
        DVec3::Y * (config.radius + config.half_height + config.skin_width),
    );
    assert!(
        character
            .move_and_slide(&mut world, DVec3::NEG_Y * 0.1)
            .grounded
    );
    (world, character)
}

#[test]
fn wall_skin_is_independent_of_displacement_partition() {
    let (mut coarse_world, mut coarse) = beside_wall();
    let (mut split_world, mut split) = beside_wall();
    let increment = CharacterConfig::default().skin_width * 0.5;
    coarse.move_and_slide(&mut coarse_world, DVec3::NEG_Z * (increment * 15.0));
    for _ in 0..15 {
        split.move_and_slide(&mut split_world, DVec3::NEG_Z * increment);
    }
    assert!(
        (coarse.position() - split.position()).length() < 1e-9,
        "coarse {:?}, split {:?}",
        coarse.position(),
        split.position()
    );
}

#[test]
fn short_oblique_moves_preserve_clearance_and_tangent_travel() {
    let skin = CharacterConfig::default().skin_width;
    for gap in [0.0, skin * 0.25, skin * 2.0] {
        for fraction in [0.1, 0.5, 0.9] {
            let (mut coarse_world, mut coarse) = beside_wall();
            let (mut split_world, mut split) = beside_wall();
            coarse.set_position(coarse.position() + DVec3::Z * gap);
            split.set_position(split.position() + DVec3::Z * gap);
            let step = DVec3::new(skin * 0.3, 0.0, -skin * fraction);
            coarse.move_and_slide(&mut coarse_world, step * 40.0);
            for _ in 0..40 {
                split.move_and_slide(&mut split_world, step);
                assert!(split.position().z >= -1e-9, "{:?}", split.position());
            }
            assert!(
                (coarse.position() - split.position()).length() < 1e-9,
                "gap {gap}, fraction {fraction}: coarse {:?}, split {:?}",
                coarse.position(),
                split.position()
            );
            assert!((split.position().x - step.x * 40.0).abs() < 1e-9);
        }
    }
}

#[test]
fn clearance_contacts_report_envelope_fraction_and_match_unrecorded_motion() {
    let skin = CharacterConfig::default().skin_width;
    for gap in [skin * 0.25, -skin * 0.5] {
        let (mut world, mut character) = beside_wall();
        let (mut plain_world, mut plain) = beside_wall();
        character.set_position(character.position() + DVec3::Z * gap);
        plain.set_position(plain.position() + DVec3::Z * gap);
        let motion = DVec3::NEG_Z * skin * 0.5;
        let mut contacts = Vec::new();
        let result = character.move_and_slide_into(&mut world, motion, &mut contacts);
        assert_eq!(result, plain.move_and_slide(&mut plain_world, motion));
        assert_eq!(character.position(), plain.position());
        assert!(result.hit_wall);
        assert_eq!(contacts.len(), 1);
        let contact = contacts[0];
        assert!(contact.clearance_only);
        assert_eq!(contact.started_inside, gap < 0.0);
        let expected_fraction = if gap < 0.0 { 0.0 } else { 0.5 };
        assert!((contact.fraction - expected_fraction).abs() < 1e-9);
        assert!((contact.applied - DVec3::NEG_Z * gap).length() < 1e-9);
        assert!(character.position().z.abs() < 1e-9);
    }
}

#[test]
fn clearance_envelope_allows_parallel_and_departing_motion() {
    let skin = CharacterConfig::default().skin_width;
    for gap in [0.0, -skin * 0.5] {
        for motion in [DVec3::X * skin * 0.25, DVec3::Z * skin * 0.25] {
            let (mut world, mut character) = beside_wall();
            character.set_position(character.position() + DVec3::Z * gap);
            let start = character.position();
            let mut contacts = Vec::new();
            let result = character.move_and_slide_into(&mut world, motion, &mut contacts);
            assert!(!result.hit_wall);
            assert!(contacts.is_empty(), "{contacts:?}");
            assert!((character.position() - start - motion).length() < 1e-9);
        }
    }
}

#[test]
fn short_moves_into_a_corner_keep_both_clearances_with_bounded_slides() {
    let config = CharacterConfig::default();
    let (mut world, mut character) = beside_wall();
    world.add_box(BoxCollider::new(
        DVec3::new(config.radius + config.skin_width + 0.1, 1.0, 0.0),
        DVec3::new(0.1, 1.0, 5.0),
    ));
    let start = character.position();
    let mut contacts = Vec::new();
    for _ in 0..40 {
        let outcome = character.move_and_slide_into(
            &mut world,
            (DVec3::X + DVec3::NEG_Z) * config.skin_width * 0.5,
            &mut contacts,
        );
        assert!(outcome.hit_wall);
        assert_eq!(contacts.len(), outcome.slides as usize);
        assert!(outcome.slides <= config.max_slides);
        assert!(
            contacts
                .iter()
                .any(|hit| hit.normal.dot(DVec3::NEG_X) > 0.99)
        );
        assert!(contacts.iter().any(|hit| hit.normal.dot(DVec3::Z) > 0.99));
        assert!((character.position() - start).length() < 1e-9);
    }
}

#[test]
fn lying_short_moves_keep_wall_clearance_and_record_the_same_motion() {
    use super::lying_move_tests::{floor, lying_at, prone_config};

    let config = prone_config();
    let fixture = || {
        let mut world = floor();
        world.add_box(BoxCollider::new(
            DVec3::new(0.0, 1.0, -(config.radius + config.skin_width + 0.1)),
            DVec3::new(5.0, 1.0, 0.1),
        ));
        let (character, body) = lying_at(&mut world, 0.0, 0.0, 0.0);
        (world, character, body)
    };
    let (mut world, mut character, mut body) = fixture();
    let (mut plain_world, mut plain, mut plain_body) = fixture();
    let (mut coarse_world, mut coarse, coarse_body) = fixture();
    let step = DVec3::NEG_Z * config.skin_width * 0.5;
    coarse.move_lying(&mut coarse_world, &coarse_body, step * 15.0);
    let mut contacts = Vec::new();
    for _ in 0..15 {
        let outcome = character.move_lying_into(&mut world, &body, step, &mut contacts);
        let plain_outcome = plain.move_lying(&mut plain_world, &plain_body, step);
        assert_eq!(outcome, plain_outcome);
        assert_eq!(character.position(), plain.position());
        assert!(outcome.hit_wall);
        assert!(contacts.iter().any(|hit| hit.clearance_only));
        assert!(character.position().z.abs() < 1e-9);
        body = outcome.body;
        plain_body = plain_outcome.body;
    }
    assert!((character.position() - coarse.position()).length() < 1e-9);
}

#[test]
fn touching_mesh_floor_does_not_hide_incoming_wall_clearance() {
    use crate::{Transform, TriangleMesh};

    let config = CharacterConfig::default();
    let wall_z = -(config.radius + config.skin_width);
    let mesh = TriangleMesh::new(
        &[
            DVec3::new(-10.0, 0.0, -10.0),
            DVec3::new(10.0, 0.0, -10.0),
            DVec3::new(10.0, 0.0, 10.0),
            DVec3::new(-10.0, 0.0, 10.0),
            DVec3::new(-10.0, 0.0, wall_z),
            DVec3::new(10.0, 0.0, wall_z),
            DVec3::new(10.0, 3.0, wall_z),
            DVec3::new(-10.0, 3.0, wall_z),
        ],
        &[[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
    )
    .unwrap();
    let mut world = PhysicsWorld::new();
    let collider = world.add_mesh(mesh, Transform::IDENTITY);
    let mut character = CharacterController::new(
        config,
        DVec3::Y * (config.radius + config.half_height + config.skin_width),
    );
    assert!(
        character
            .move_and_slide(&mut world, DVec3::NEG_Y * 0.1)
            .grounded
    );
    character.set_position(character.position() - DVec3::Y * config.skin_width * 0.25);
    let mut contacts = Vec::new();
    for _ in 0..15 {
        let outcome = character.move_and_slide_into(
            &mut world,
            DVec3::NEG_Z * config.skin_width * 0.5,
            &mut contacts,
        );
        assert!(outcome.hit_wall);
        assert!(
            contacts
                .iter()
                .any(|hit| hit.collider == collider && hit.clearance_only)
        );
        assert!(
            character.position().z.abs() < 1e-9,
            "{:?}",
            character.position()
        );
    }
}

#[test]
fn sub_skin_moves_climb_steps_and_follow_walkable_slopes() {
    use super::tests::{dome_world, on_dome, stepped_world};

    let config = CharacterConfig::default();
    let step = config.skin_width * 0.5;
    for height in [config.step_offset * 0.5, config.step_offset] {
        let mut world = stepped_world(height);
        let mut character = CharacterController::new(
            config,
            DVec3::new(-1.0, config.radius + config.half_height, 0.0),
        );
        assert!(character.move_and_slide(&mut world, DVec3::ZERO).grounded);
        let mut climbed = false;
        for _ in 0..300 {
            let outcome = character.move_and_slide(&mut world, DVec3::X * step);
            climbed |= outcome.stepped_up;
            assert!(outcome.grounded);
        }
        assert!(climbed);
        assert!(
            (character.position().x - 0.5).abs() < 1e-9,
            "height {height}: {:?}",
            character.position()
        );
        let feet = character.position().y - config.radius - config.half_height;
        assert!((feet - height - config.skin_width).abs() < 1e-9);
    }
    for direction in [-1.0, 1.0] {
        let mut world = dome_world();
        let mut character =
            CharacterController::new(config, on_dome(&config, 30.0_f64.to_radians()));
        assert!(character.move_and_slide(&mut world, DVec3::ZERO).grounded);
        let start = character.position();
        for _ in 0..40 {
            let outcome = character.move_and_slide(&mut world, DVec3::X * step * direction);
            assert!(outcome.grounded);
        }
        let moved = character.position() - start;
        assert!(
            (moved.x - step * direction * 40.0).abs() < 1e-9,
            "direction {direction}: {moved:?}"
        );
        assert!(moved.y * direction < 0.0);
    }
}

#[test]
fn clearance_past_a_box_edge_does_not_cancel_free_fall() {
    let config = CharacterConfig::default();
    for outward in [DVec3::X, DVec3::NEG_X, DVec3::Z, DVec3::NEG_Z] {
        for gap in [0.25, 0.5, 0.75] {
            let mut world = PhysicsWorld::new();
            world.add_box(BoxCollider::new(
                DVec3::new(0.0, -0.5, 0.0),
                DVec3::new(1.0, 0.5, 1.0),
            ));
            let start = outward * (1.0 + config.radius + config.skin_width * gap)
                + DVec3::Y * (config.half_height + config.radius + config.skin_width);
            let mut character = CharacterController::new(config, start);
            let motion = DVec3::NEG_Y * config.skin_width * 0.5;
            let mut contacts = Vec::new();
            let result = character.move_and_slide_into(&mut world, motion, &mut contacts);
            assert!(contacts.is_empty(), "false ledge contacts: {contacts:?}");
            assert!(!result.grounded);
            assert!((character.position() - start - motion).length() < 1e-9);
        }
    }
}

#[test]
fn departing_a_platform_keeps_downward_and_outward_motion() {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 3.9, 13.0),
        DVec3::new(4.0, 0.1, 4.0),
    ));
    let mut character = CharacterController::new(
        CharacterConfig::default(),
        DVec3::new(0.0, 4.91, 8.699999998),
    );
    let motion = DVec3::new(0.0, -0.02, -0.21);
    let mut contacts = Vec::new();
    let outcome = character.move_and_slide_into(&mut world, motion, &mut contacts);
    assert!(!outcome.grounded);
    assert!(
        contacts.is_empty(),
        "false departure contacts: {contacts:?}"
    );
    assert!((outcome.motion - motion).length() < 1e-9, "{outcome:?}");
}
