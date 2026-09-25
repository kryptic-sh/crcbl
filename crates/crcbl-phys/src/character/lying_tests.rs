use std::f64::consts::{FRAC_PI_2, PI};

use crcbl_ecs::Entity;
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, LyingCapsule, Sphere};
use crate::components::{ColliderComponent, Transform};
use crate::compound_shape::CompoundShape;
use crate::system::PhysicsSystem;
use crate::world::{ColliderId, PhysicsWorld};

use super::{CharacterConfig, CharacterController};

/// How far behind the actor's origin a prone body's feet lie.
const BODY: f64 = 1.6;

/// A prone character as a game models one today: a controller with no
/// cylindrical section, a sphere at the actor's origin.
fn prone_config() -> CharacterConfig {
    CharacterConfig {
        half_height: 0.0,
        ..CharacterConfig::default()
    }
}

/// A floor whose top is `y = 0`, wide enough that nothing here reaches its
/// edge.
fn floor() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -1.0, 0.0),
        DVec3::new(50.0, 1.0, 50.0),
    ));
    world
}

/// A prone character settled on `world`'s floor at the origin: a skin width
/// above it, as every settled controller is.
fn settled(world: &mut PhysicsWorld) -> CharacterController {
    let config = prone_config();
    let mut character = CharacterController::new(config, DVec3::new(0.0, config.radius, 0.0));
    character.move_and_slide(world, DVec3::ZERO);
    assert!(character.is_grounded(), "the fixture starts on the floor");
    character
}

/// The body lying back from `character`'s origin with its head facing `yaw`.
fn body(character: &CharacterController, yaw: f64) -> LyingCapsule {
    LyingCapsule::new(character.position(), yaw, character.config().radius, BODY)
}

/// **A prone player's legs are not allowed into a wall behind them.** The
/// controller's own sphere is clear of the wall and settles without touching
/// it; the body lying back from it is not clear, until it turns to point away
/// from the wall or along it. The floor it lies on never blocks.
#[test]
fn a_prone_body_whose_legs_reach_into_a_wall_behind_it_does_not_fit() {
    let mut world = floor();
    let wall = world.add_box(BoxCollider::new(
        DVec3::new(0.0, 1.0, 2.0),
        DVec3::new(2.0, 1.0, 0.25),
    ));
    let character = settled(&mut world);

    assert_eq!(
        character.lying_blocker(&mut world, &body(&character, 0.0)),
        Some(wall)
    );
    assert_eq!(
        character.lying_blocker(&mut world, &body(&character, PI)),
        None
    );
    assert_eq!(
        character.lying_blocker(&mut world, &body(&character, FRAC_PI_2)),
        None
    );
}

/// **The character's own collider is left out**, as its moves leave it out:
/// the prone body starts at the controller's own sphere, so without the
/// exclusion every pose would be blocked by the character itself.
#[test]
fn the_characters_own_collider_does_not_block_its_prone_body() {
    let mut world = floor();
    let character = settled(&mut world);
    let own = world.add_sphere(Sphere::new(character.position(), character.config().radius));

    assert_eq!(
        character.lying_blocker(&mut world, &body(&character, 0.0)),
        Some(own),
        "without the binding the character's own sphere is inside its body"
    );
    let bound = character.clone().with_self_collider(own);
    assert_eq!(bound.lying_blocker(&mut world, &body(&bound, 0.0)), None);
}

/// **The query mask applies**, as it does to every other query the controller
/// makes: an item lying in the legs blocks only while the mask sees it.
#[test]
fn a_collider_off_the_query_mask_does_not_block_a_prone_body() {
    const ITEMS: u32 = 1 << 3;
    let mut world = floor();
    let item = world.add_sphere(Sphere::new(DVec3::new(0.0, 0.3, 1.0), 0.2));
    assert!(world.set_layers(item, ITEMS));
    let character = settled(&mut world);

    assert_eq!(
        character.lying_blocker(&mut world, &body(&character, 0.0)),
        Some(item)
    );
    let blind = character.clone().with_query_mask(!ITEMS);
    assert_eq!(blind.lying_blocker(&mut world, &body(&blind, 0.0)), None);
}

/// **A static compound blocks a prone body** through the query world a
/// [`PhysicsSystem`] keeps, where it is one box around its parts.
#[test]
fn a_static_compound_blocks_a_prone_body() {
    let mut phys = PhysicsSystem::new();
    let crate_stack = Entity::from_bits((1u64 << 32) | 1).expect("generation 1 is never zero");
    let transform = Transform::from_position(DVec3::new(0.0, 0.0, 2.0));
    phys.set_transform(crate_stack, transform);
    phys.set_collider(
        crate_stack,
        &ColliderComponent::Compound {
            offset: DVec3::ZERO,
            shape: CompoundShape::from_aabbs(&[
                Aabb::new(DVec3::new(-1.0, 0.0, -0.25), DVec3::new(1.0, 0.5, 0.25)),
                Aabb::new(DVec3::new(-0.25, 0.5, -0.25), DVec3::new(0.25, 1.0, 0.25)),
            ])
            .expect("valid parts"),
            is_trigger: false,
        },
        &transform,
    );
    let compound: ColliderId = phys.collider_of(crate_stack).expect("a collider");

    let config = prone_config();
    let character = CharacterController::new(config, DVec3::new(0.0, config.radius, 0.0));
    let world = phys.world_mut();
    assert_eq!(
        character.lying_blocker(world, &body(&character, 0.0)),
        Some(compound)
    );
    assert_eq!(character.lying_blocker(world, &body(&character, PI)), None);
}
