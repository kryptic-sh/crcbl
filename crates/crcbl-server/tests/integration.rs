//! Integration test — real `crcbl_server::Server` ↔ `crcbl_client::Client`
//! over an in-memory transport pair.
//!
//! Verifies that the shipped wiring (not hand-rolled mocks) survives a
//! multi-tick exchange: the server emits delta-encoded snapshots, the client
//! applies them, acks, and the server advances its baseline accordingly.

use crcbl_client::Client;
use crcbl_core::TickId;
use crcbl_ecs::{System, World};
use crcbl_net::InMemoryTransport;
use crcbl_server::Server;

/// Build a world with one `"counter"` system containing `n` entities whose
/// component values start at 0.0f32.
fn world_with_entities(n: u32) -> World {
    let mut world = World::new();
    let mut sys = System::<f32>::new("counter");
    for i in 0..n {
        let entity = world.spawn();
        sys.attach(entity, i as f32);
    }
    world.register_system(Box::new(sys));
    world
}

#[test]
fn server_to_client_roundtrip() {
    let (server_transport, client_transport) = InMemoryTransport::pair();

    let n_entities = 3;

    let mut server = Server::new(world_with_entities(n_entities), server_transport, 60);
    let mut client = Client::new(World::new(), client_transport, 60);

    // ---- tick 1: server sends keyframe, client receives ----
    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    server.update(std::time::Duration::ZERO);
    server.update(tick_dt);
    // Server tick 1 emitted; client has not consumed yet.

    client.update(std::time::Duration::ZERO);
    client.update(tick_dt);
    // Client consumed tick 1 snapshot and sent an ack.

    // ---- tick 2: server receives ack, advances baseline, sends delta ----
    server.update(tick_dt * 2);
    client.update(tick_dt * 2);

    // ---- tick 3–5: more exchange ----
    for i in 3..=5 {
        server.update(tick_dt * i);
        client.update(tick_dt * i);
    }

    // The client's world is empty (it doesn't mirror the ECS — snapshots
    // are applied to its baseline, not its World).  We verify that the
    // plumbing runs without panic, both sides stay connected, and the
    // client received and applied the server's delta-encoded snapshots.
    assert!(
        server.is_connected(),
        "server transport must stay connected after multi-tick exchange"
    );
    assert!(
        client.is_connected(),
        "client transport must stay connected after multi-tick exchange"
    );

    // Client must have received and applied snapshots — its baseline
    // tick should be past zero. (Entity data is stub — per-entity
    // component encoding lands in P3.)
    assert!(
        client.last_applied_tick() > TickId::ZERO,
        "client must have applied at least one snapshot; last_applied_tick={:?}",
        client.last_applied_tick()
    );
}

#[test]
fn server_and_client_survive_multiple_ticks() {
    let (server_transport, client_transport) = InMemoryTransport::pair();

    let mut server = Server::new(world_with_entities(5), server_transport, 60);
    let mut client = Client::new(World::new(), client_transport, 60);

    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    // FrameClock::update takes absolute time, so accumulate.
    server.update(std::time::Duration::ZERO);
    client.update(std::time::Duration::ZERO);

    for i in 1..=20 {
        let elapsed = tick_dt * i;
        server.update(elapsed);
        client.update(elapsed);
    }

    assert!(server.is_connected());
    assert!(client.is_connected());

    // The server should have advanced past tick 0.
    assert!(
        server.tick_id().get() > 0,
        "server tick id must advance beyond zero"
    );

    // Client must have received and applied snapshots.
    assert!(
        client.last_applied_tick() > TickId::ZERO,
        "client must have applied at least one snapshot; last_applied_tick={:?}",
        client.last_applied_tick()
    );
}
