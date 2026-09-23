use glam::DVec3;

use crate::collider::BoxCollider;
use crate::world::{ColliderId, PhysicsWorld};

use super::{CharacterConfig, CharacterController};

/// The layer the fixtures put their item on.
const ITEMS: u32 = 1 << 3;

/// A floor whose top is `y = 0`, and an item on [`ITEMS`] standing on it
/// across `+X`: a metre tall, far over the step offset, so a character that
/// sees it cannot walk over it.
fn floor_and_item() -> (PhysicsWorld, ColliderId) {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    let item = world.add_box(BoxCollider::new(
        DVec3::new(1.0, 0.5, 0.0),
        DVec3::new(0.25, 0.5, 2.0),
    ));
    assert!(world.set_layers(item, ITEMS));
    (world, item)
}

/// A character standing at the origin on `floor_and_item`'s floor.
fn standing(world: &mut PhysicsWorld, mask: u32) -> CharacterController {
    let config = CharacterConfig::default();
    let centre = config.radius + config.half_height;
    let mut character =
        CharacterController::new(config, DVec3::new(0.0, centre, 0.0)).with_query_mask(mask);
    character.move_and_slide(world, DVec3::ZERO);
    assert!(character.is_grounded(), "the fixture starts on the floor");
    character
}

/// Walks 3 m along `+X` in tenth-of-a-metre moves, returning whether any of
/// them reported a wall.
fn walk(character: &mut CharacterController, world: &mut PhysicsWorld) -> bool {
    let mut hit_wall = false;
    for _ in 0..30 {
        hit_wall |= character
            .move_and_slide(world, DVec3::new(0.1, 0.0, 0.0))
            .hit_wall;
    }
    hit_wall
}

#[test]
fn a_character_masked_off_the_item_layer_walks_through_an_item() {
    let (mut world, _) = floor_and_item();
    let mut character = standing(&mut world, !ITEMS);
    assert_eq!(character.query_mask(), !ITEMS);

    let hit_wall = walk(&mut character, &mut world);
    assert!(!hit_wall, "a masked-out item is not a wall");
    assert!(
        character.position().x > 2.9,
        "the whole walk is covered: x = {}",
        character.position().x
    );
    assert!(character.is_grounded(), "and the floor still holds it up");
}

#[test]
fn a_character_with_the_default_mask_is_blocked_by_an_item() {
    let (mut world, _) = floor_and_item();
    let mut character = standing(&mut world, crate::ALL_LAYERS);

    let hit_wall = walk(&mut character, &mut world);
    assert!(hit_wall, "the item is a wall to a character that sees it");
    // The item's -X face is at x = 0.75, and the capsule's radius is kept
    // off it.
    let stop = 0.75 - character.config().radius;
    assert!(
        character.position().x <= stop + 1e-9,
        "the character must stop at the item: x = {}, face-less-radius {stop}",
        character.position().x
    );
}

#[test]
fn a_ground_probe_ignores_an_item_off_the_mask() {
    let (mut world, item) = floor_and_item();
    let config = CharacterConfig::default();
    // Feet a skin width above the item's top, at y = 1: standing on the item.
    let above_item = DVec3::new(
        1.0,
        1.0 + config.skin_width + config.radius + config.half_height,
        0.0,
    );
    let probe = config.skin_width * 2.0;

    let sees = CharacterController::new(config, above_item);
    assert_eq!(
        sees.probe_ground(&mut world, probe)
            .map(|found| found.contact.collider),
        Some(item),
        "with the default mask the item is the ground"
    );

    let blind = CharacterController::new(config, above_item).with_query_mask(!ITEMS);
    assert!(
        blind.probe_ground(&mut world, probe).is_none(),
        "masked off, the item is not ground and the floor is a metre down"
    );
    let floor = blind
        .probe_ground(&mut world, 1.5)
        .expect("the floor is below the item");
    assert_ne!(floor.contact.collider, item, "the probe found the floor");
    assert!(
        (floor.distance - (1.0 + config.skin_width)).abs() < 1e-9,
        "a metre and a skin down: {}",
        floor.distance
    );
}

#[test]
fn a_character_is_not_pushed_out_of_an_item_off_its_mask() {
    let (mut world, _) = floor_and_item();
    let config = CharacterConfig::default();
    // Centred in the item, feet a skin width off the floor so the floor itself
    // is no contact.
    let inside = DVec3::new(
        1.0,
        config.skin_width + config.radius + config.half_height,
        0.0,
    );

    let mut blind = CharacterController::new(config, inside).with_query_mask(!ITEMS);
    let outcome = blind.move_and_slide(&mut world, DVec3::ZERO);
    assert_eq!(
        outcome.depenetration,
        DVec3::ZERO,
        "a masked-out item has no push-out"
    );

    let mut sees = CharacterController::new(config, inside);
    let outcome = sees.move_and_slide(&mut world, DVec3::ZERO);
    assert!(
        outcome.depenetration.length() > 0.25,
        "the default mask digs the character out of the item: {:?}",
        outcome.depenetration
    );
}
