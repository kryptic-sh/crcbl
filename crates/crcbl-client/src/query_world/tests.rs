//! The client's query world against the snapshots that feed it and the
//! server world it stands in for.

use std::collections::HashMap;
use std::time::Duration;

use crcbl_ecs::Entity;
use crcbl_net::InMemoryTransport;
use crcbl_net::auth::SessionCrypto;
use crcbl_phys::{
    BoxCollider, ColliderComponent, CompoundPart, CompoundShape, PhysicsSystem, PhysicsWorld,
    QueryFilter, Ray, Segment, Transform,
};
use glam::{DQuat, DVec3};

use super::ClientQueryWorld;
use crate::tests::{client, connect, keyframe_snapshot, physics, send_sealed};
use crate::{Client, InterpolatedState};

/// The replicated entity most tests follow.
const ENTITY: u64 = (1 << 32) | 7;

/// A second replicated entity.
const OTHER: u64 = (1 << 32) | 8;

/// The radius of the ball every test replica is.
const BALL_RADIUS: f64 = 0.5;

/// A [`max_step`](ClientQueryWorld::follow) no test entity moves past unless
/// it is meant to: a metre a tick.
const MAX_STEP: f64 = 1.0;

/// A ball centred on its entity.
fn ball() -> ColliderComponent {
    ColliderComponent::Sphere {
        offset: DVec3::ZERO,
        radius: BALL_RADIUS,
        is_trigger: false,
    }
}

/// Every entity is a ball.
fn every_entity_is_a_ball(_: u64) -> Option<ColliderComponent> {
    Some(ball())
}

/// Where a ray fired from far down `-X` along `+X` first meets something,
/// along `X`, or `None` for nothing.
fn first_along_x(world: &mut ClientQueryWorld, y: f64, z: f64) -> Option<f64> {
    world
        .cast_ray(
            &Ray::new(DVec3::new(-1000.0, y, z), DVec3::X),
            QueryFilter::ALL,
        )
        .map(|(_, hit)| hit.point.x)
}

/// A connected client with nothing yet received, and the server end of its
/// link.
fn connected() -> (Client<InMemoryTransport>, InMemoryTransport, SessionCrypto) {
    let (transport, mut peer) = InMemoryTransport::pair();
    let mut client = client(transport);
    let crypto = connect(&mut client, &mut peer, Duration::ZERO);
    (client, peer, crypto)
}

/// One keyframe at `tick` holding each entity at its position, in the
/// quantized wire form the server sends.
fn send_positions(
    client: &mut Client<InMemoryTransport>,
    peer: &mut InMemoryTransport,
    crypto: &mut SessionCrypto,
    tick: u64,
    entities: &[(u64, DVec3)],
) {
    let mut blob = Vec::new();
    for &(bits, position) in entities {
        let mut data = Vec::new();
        Transform::from_position(position).encode_wire(&mut data);
        crcbl_net::encode_entity_entry(&mut blob, bits, &data);
    }
    send_sealed(peer, crypto, &keyframe_snapshot(tick, &[(physics(), blob)]));
    client.update(Duration::from_nanos(tick));
}

/// **The scene's statics are there before any snapshot is.** A camera
/// sweeps them on the first frame, with nothing replicated yet.
#[test]
fn the_statics_are_there_before_any_snapshot() {
    let mut statics = PhysicsWorld::new();
    statics.add_box(BoxCollider::new(
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::splat(1.0),
    ));
    let mut world = ClientQueryWorld::new(statics);

    assert_eq!(first_along_x(&mut world, 0.0, 0.0), Some(3.0));

    // Following an empty state leaves them where they are: they are not
    // replicas, so a replica pass cannot remove them.
    let (client, _peer, _crypto) = connected();
    world.follow(&client, 1.0, MAX_STEP, every_entity_is_a_ball);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), Some(3.0));
}

/// **A replica stands where it is drawn, at the interpolated pose, and not at
/// either snapshot.** A quarter of the way from `x = 0` to `x = 10` is
/// `x = 2.5`, so the ray meets the ball's near side at `2.0`.
#[test]
fn a_replica_stands_at_the_interpolated_pose() {
    let (mut client, mut peer, mut crypto) = connected();
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        1,
        &[(ENTITY, DVec3::ZERO)],
    );
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        2,
        &[(ENTITY, DVec3::new(10.0, 0.0, 0.0))],
    );

    // Ten metres in the one tick between the snapshots, at a limit of ten: a
    // move at the limit is a move, and is lerped.
    let max_step = 10.0;
    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    world.follow(&client, 0.25, max_step, every_entity_is_a_ball);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), Some(2.5 - BALL_RADIUS));

    // And it moves on with playback, the same collider placed again.
    let collider = world.collider_of(ENTITY);
    world.follow(&client, 0.75, max_step, every_entity_is_a_ball);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), Some(7.5 - BALL_RADIUS));
    assert_eq!(world.collider_of(ENTITY), collider, "placed, not re-added");
}

/// **An entity `shape_of` gives no shape has no collider**, and a ray passes
/// where it stands.
#[test]
fn an_entity_with_no_shape_has_no_collider() {
    let (mut client, mut peer, mut crypto) = connected();
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        1,
        &[(ENTITY, DVec3::ZERO), (OTHER, DVec3::new(0.0, 0.0, 5.0))],
    );

    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    world.follow(&client, 1.0, MAX_STEP, |bits| (bits == OTHER).then(ball));
    assert_eq!(world.collider_of(ENTITY), None);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), None);
    assert_eq!(first_along_x(&mut world, 0.0, 5.0), Some(-BALL_RADIUS));
}

/// **A despawned entity's collider goes with it**, and the replicas that
/// stay keep theirs.
#[test]
fn a_despawned_replica_loses_its_collider() {
    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    let both = InterpolatedState {
        transforms: vec![
            (ENTITY, Transform::from_position(DVec3::ZERO)),
            (OTHER, Transform::from_position(DVec3::new(0.0, 0.0, 5.0))),
        ],
    };
    world.follow_state(&both, every_entity_is_a_ball);
    assert!(world.collider_of(ENTITY).is_some());

    let one = InterpolatedState {
        transforms: vec![(OTHER, Transform::from_position(DVec3::new(0.0, 0.0, 5.0)))],
    };
    world.follow_state(&one, every_entity_is_a_ball);
    assert_eq!(world.collider_of(ENTITY), None);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), None, "a ghost is left");
    assert_eq!(first_along_x(&mut world, 0.0, 5.0), Some(-BALL_RADIUS));
}

/// **Leaving interest scope is leaving.** Unsubscribing from the sector drops
/// its snapshots, and the next frame drops the colliders they placed.
#[test]
fn a_replica_out_of_scope_loses_its_collider() {
    let (mut client, mut peer, mut crypto) = connected();
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        1,
        &[(ENTITY, DVec3::ZERO)],
    );
    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    world.follow(&client, 1.0, MAX_STEP, every_entity_is_a_ball);
    assert!(world.collider_of(ENTITY).is_some());

    client.set_subscribed_sectors([]);
    world.follow(&client, 1.0, MAX_STEP, every_entity_is_a_ball);
    assert_eq!(world.collider_of(ENTITY), None);
    assert_eq!(first_along_x(&mut world, 0.0, 0.0), None);
}

/// **A jump is not a path.** An entity that moved a hundred metres in one tick
/// at a metre a tick at most did not cross the gap, so halfway through the
/// pair it stands where it landed and nothing stands halfway.
#[test]
fn a_teleport_does_not_sweep_through_the_gap() {
    let (mut client, mut peer, mut crypto) = connected();
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        1,
        &[(ENTITY, DVec3::ZERO)],
    );
    send_positions(
        &mut client,
        &mut peer,
        &mut crypto,
        2,
        &[(ENTITY, DVec3::new(100.0, 0.0, 0.0))],
    );

    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    world.follow(&client, 0.5, MAX_STEP, every_entity_is_a_ball);
    assert_eq!(
        first_along_x(&mut world, 0.0, 0.0),
        Some(100.0 - BALL_RADIUS)
    );
}

/// **A lost snapshot is not a teleport.** A metre and a half between two
/// snapshots two ticks apart is three quarters of a metre a tick, under the
/// limit, so it is lerped; the same move in one tick is over it.
#[test]
fn the_jump_limit_scales_with_the_ticks_between_snapshots() {
    let moved = DVec3::new(1.5, 0.0, 0.0);
    for (ticks_apart, expected) in [(2, 0.75), (1, 1.5)] {
        let (mut client, mut peer, mut crypto) = connected();
        send_positions(
            &mut client,
            &mut peer,
            &mut crypto,
            1,
            &[(ENTITY, DVec3::ZERO)],
        );
        send_positions(
            &mut client,
            &mut peer,
            &mut crypto,
            1 + ticks_apart,
            &[(ENTITY, moved)],
        );

        let mut world = ClientQueryWorld::new(PhysicsWorld::new());
        world.follow(&client, 0.5, MAX_STEP, every_entity_is_a_ball);
        assert_eq!(
            first_along_x(&mut world, 0.0, 0.0),
            Some(expected - BALL_RADIUS),
            "{ticks_apart} ticks apart"
        );
    }
}

/// **The local player's own replica can be left out**, so a camera boom
/// swept from inside the player's capsule reaches the wall behind it.
#[test]
fn a_query_can_leave_a_replica_out() {
    let mut statics = PhysicsWorld::new();
    statics.add_box(BoxCollider::new(
        DVec3::new(10.0, 0.0, 0.0),
        DVec3::splat(1.0),
    ));
    let mut world = ClientQueryWorld::new(statics);
    world.follow_state(
        &InterpolatedState {
            transforms: vec![(ENTITY, Transform::from_position(DVec3::ZERO))],
        },
        every_entity_is_a_ball,
    );

    let boom = Segment::new(DVec3::ZERO, DVec3::new(20.0, 0.0, 0.0));
    let own = world.collider_of(ENTITY);
    let (_, inside) = world
        .sweep_sphere(&boom, 0.1, QueryFilter::ALL)
        .expect("the sweep starts inside the player");
    assert!(inside.started_inside);
    let (_, wall) = world
        .sweep_sphere(&boom, 0.1, QueryFilter::excluding(own))
        .expect("the wall is behind the player");
    assert!(!wall.started_inside);
    assert!((wall.point.x - 9.0).abs() < 1e-9, "{wall:?}");
}

/// The server-side shapes the parity test places, one of each kind a
/// replica can be, offset and turned.
fn shapes() -> Vec<(u64, ColliderComponent, Transform)> {
    let quarter = DQuat::from_xyzw(
        0.0,
        core::f64::consts::FRAC_1_SQRT_2,
        0.0,
        core::f64::consts::FRAC_1_SQRT_2,
    );
    let tilt = DQuat::from_xyzw(0.0, 0.0, 0.6, 0.8);
    let compound = CompoundShape::new(vec![
        CompoundPart::new(
            DVec3::new(-0.6, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::splat(0.4),
        ),
        CompoundPart::new(DVec3::new(0.6, 0.3, 0.0), tilt, DVec3::new(0.5, 0.2, 0.3)),
    ])
    .expect("two valid parts");
    vec![
        (
            (1 << 32) | 1,
            ColliderComponent::Sphere {
                offset: DVec3::new(0.0, 0.5, 0.25),
                radius: 0.75,
                is_trigger: false,
            },
            Transform::new(DVec3::new(-6.0, 0.0, 0.0), tilt),
        ),
        (
            (1 << 32) | 2,
            ColliderComponent::Box {
                offset: DVec3::new(0.25, 0.0, 0.0),
                half_extents: DVec3::new(1.0, 0.5, 0.25),
                is_trigger: false,
            },
            Transform::new(DVec3::new(-2.0, 0.0, 1.0), quarter),
        ),
        (
            (1 << 32) | 3,
            ColliderComponent::Capsule {
                offset: DVec3::new(0.0, 0.25, 0.0),
                radius: 0.4,
                half_height: 0.8,
                is_trigger: false,
            },
            Transform::new(DVec3::new(2.0, 0.0, -1.0), tilt),
        ),
        (
            (1 << 32) | 4,
            ColliderComponent::Compound {
                offset: DVec3::new(0.0, 0.0, 0.1),
                shape: compound,
                is_trigger: false,
            },
            Transform::new(DVec3::new(6.0, 0.0, 0.5), quarter),
        ),
        (
            (1 << 32) | 5,
            ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: true,
            },
            Transform::from_position(DVec3::new(0.0, 0.0, 4.0)),
        ),
    ]
}

/// **A query here answers what the server's world answers for the same
/// colliders at the same pose**: every ray, sweep and overlap below meets the
/// same entity at the same point, with the same normal, part and depth.
///
/// The server's side is a [`PhysicsSystem`] given each collider as its own
/// colliders are given — the placement the snapshot's transform came from.
#[test]
fn queries_match_the_servers_world_at_the_same_pose() {
    let mut server = PhysicsSystem::new();
    let mut client_entities = HashMap::new();
    let mut state = InterpolatedState {
        transforms: Vec::new(),
    };
    for (bits, component, transform) in shapes() {
        let entity = Entity::from_bits(bits).expect("a live entity");
        server.set_collider(entity, &component, &transform);
        client_entities.insert(bits, component);
        state.transforms.push((bits, transform));
    }
    let mut world = ClientQueryWorld::new(PhysicsWorld::new());
    world.follow_state(&state, |bits| client_entities.get(&bits).cloned());

    // The server's id for each of the client's, through the entity they share.
    let mut server_id = HashMap::new();
    for (bits, _, _) in shapes() {
        let entity = Entity::from_bits(bits).expect("a live entity");
        server_id.insert(
            world.collider_of(bits).expect("a client collider"),
            server.collider_of(entity).expect("a server collider"),
        );
    }

    // What the probes met, so a battery that missed everything fails rather
    // than agreeing about nothing.
    let mut met = std::collections::HashSet::new();
    for x in -8..=8 {
        for (y, z) in [(-0.3, 0.0), (0.2, 0.9), (0.6, -0.4), (0.0, 4.2)] {
            let x = f64::from(x) * 0.95;
            for direction in [
                DVec3::NEG_Z,
                DVec3::NEG_Y,
                DVec3::new(0.3, -0.2, -1.0).normalize(),
            ] {
                let origin = DVec3::new(x, y, z) - direction * 10.0;
                let ray = Ray::new(origin, direction);
                let ours = world.cast_ray(&ray, QueryFilter::ALL);
                let theirs = server.world_mut().cast_ray(&ray);
                assert_eq!(
                    ours.map(|(id, hit)| (server_id[&id], hit)),
                    theirs,
                    "ray {ray:?}"
                );
                met.extend(theirs.map(|(id, _)| ("ray", id)));

                let segment = Segment::new(origin, origin + direction * 20.0);
                let ours = world.sweep_sphere(&segment, 0.3, QueryFilter::ALL);
                let theirs = server.world_mut().sweep_sphere(&segment, 0.3);
                assert_eq!(
                    ours.map(|(id, hit)| (server_id[&id], hit)),
                    theirs,
                    "sweep {segment:?}"
                );
                met.extend(theirs.map(|(id, _)| ("sweep", id)));
            }

            let centre = DVec3::new(x, y, z);
            let mut ours = Vec::new();
            world.overlap_sphere_into(centre, 0.9, QueryFilter::ALL, &mut ours);
            let theirs = server.world_mut().overlap_sphere(centre, 0.9);
            let ours: Vec<_> = ours.iter().map(|(id, hit)| (server_id[id], *hit)).collect();
            assert_eq!(ours, theirs, "overlap at {centre}");

            let mut ids = Vec::new();
            world.overlap_sphere_ids_into(centre, 0.9, &mut ids);
            let ids: Vec<_> = ids.iter().map(|id| server_id[id]).collect();
            let named: Vec<_> = theirs.iter().map(|(id, _)| *id).collect();
            assert_eq!(ids, named, "ids-only overlap at {centre}");
            met.extend(named.into_iter().map(|id| ("overlap", id)));
        }
    }

    for kind in ["ray", "sweep", "overlap"] {
        for id in server_id.values() {
            let solid = shapes().iter().any(|(bits, component, _)| {
                server.collider_of(Entity::from_bits(*bits).expect("live")) == Some(*id)
                    && !matches!(
                        component,
                        ColliderComponent::Sphere {
                            is_trigger: true,
                            ..
                        }
                    )
            });
            if kind == "overlap" || solid {
                assert!(met.contains(&(kind, *id)), "no {kind} met {id:?}");
            }
        }
    }
}
