//! A whole client/server session: handshake, resume, replication — a real
//! `crcbl_server::Server` against a real `crcbl_client::Client` over an
//! in-memory transport pair.
//!
//! Verifies that the shipped wiring (not hand-rolled mocks) survives a
//! multi-tick exchange: the server emits delta-encoded snapshots, the client
//! applies them, acks, and the server advances its baseline accordingly.
//!
//! # Why it is a separate target
//!
//! It is the only place the two crates meet, and it compiles as a **separate
//! crate**, so it reaches `Server` and `Client` through exactly the surface a
//! game reaches them through — no `pub(crate)` shortcut into either one's
//! session state, no `#[cfg(test)]` constructor. Every `Hello`, token and
//! snapshot below therefore travels the shipped path, encoders and all. An
//! in-crate module could reach past all of that and would stop noticing when
//! the public wiring drifted from the private state it was checking.

use crcbl_client::Client;
use crcbl_core::TickId;
use crcbl_ecs::{System, World};
use crcbl_net::InMemoryTransport;
use crcbl_server::Server;

/// Explicit engine and schema identifiers, and the *shipped* protocol
/// version — pinning a literal here meant the default the crates actually
/// ship was never exercised end to end.
const COMPATIBILITY: crcbl_net::ProtocolCompatibility = crcbl_net::ProtocolCompatibility {
    protocol_version: crcbl_net::ProtocolCompatibility::DEFAULT.protocol_version,
    engine_build_id: 0x0043_5243_424C,
    schema_hash: 0x0050_3242,
};

fn server(world: World, transport: InMemoryTransport) -> Server<InMemoryTransport> {
    Server::try_new_with_compatibility(world, transport, 60, COMPATIBILITY)
        .expect("OS CSPRNG available")
}

fn client(world: World, transport: InMemoryTransport) -> Client<InMemoryTransport> {
    Client::new_with_compatibility(world, transport, 60, COMPATIBILITY)
}

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

fn world_with_systems(n: u32) -> World {
    let mut world = World::new();
    for i in 0..n {
        world.register_system(Box::new(System::<f32>::new(format!("system_{i}"))));
    }
    world
}

fn hello(generation: u64, session_token: Option<crcbl_net::ResumeToken>) -> crcbl_net::Hello {
    crcbl_net::Hello {
        protocol_version: COMPATIBILITY.protocol_version,
        engine_build_id: COMPATIBILITY.engine_build_id,
        schema_hash: COMPATIBILITY.schema_hash,
        generation,
        session_token,
    }
}

fn send_hello_with_generation(
    transport: &mut InMemoryTransport,
    generation: u64,
    session_token: Option<crcbl_net::ResumeToken>,
) {
    crcbl_net::Transport::send_reliable(
        transport,
        crcbl_net::Message {
            kind: crcbl_net::MessageKind::Reliable,
            payload: crcbl_net::encode_hello(&hello(generation, session_token)),
        },
    )
    .unwrap();
}

fn send_hello(transport: &mut InMemoryTransport, session_token: Option<crcbl_net::ResumeToken>) {
    send_hello_with_generation(transport, 1, session_token);
}

fn recv_handshake(transport: &mut InMemoryTransport) -> crcbl_net::HandshakeResult {
    loop {
        let message = crcbl_net::Transport::recv(transport).unwrap().unwrap();
        if let Ok(result) = crcbl_net::decode_handshake_result(&message.payload) {
            return result;
        }
    }
}

fn recv_handshake_generation(
    transport: &mut InMemoryTransport,
    expected_generation: u64,
) -> crcbl_net::HandshakeResult {
    let result = recv_handshake(transport);
    let generation = match &result {
        crcbl_net::HandshakeResult::Accept { generation, .. }
        | crcbl_net::HandshakeResult::Reject { generation, .. } => *generation,
    };
    assert_eq!(generation, expected_generation);
    result
}

#[test]
fn server_echoes_generation_for_accept_and_reject() {
    let (server_transport, mut peer) = InMemoryTransport::pair();
    let mut server = server(World::new(), server_transport);

    send_hello_with_generation(&mut peer, 41, None);
    server.update(std::time::Duration::ZERO);
    server.update(std::time::Duration::from_nanos(16_666_667));
    assert!(matches!(
        recv_handshake_generation(&mut peer, 41),
        crcbl_net::HandshakeResult::Accept { .. }
    ));

    send_hello_with_generation(
        &mut peer,
        42,
        Some(crcbl_net::ResumeToken::from_bytes([0; 32])),
    );
    server.update(std::time::Duration::from_nanos(33_333_334));
    assert!(matches!(
        recv_handshake_generation(&mut peer, 42),
        crcbl_net::HandshakeResult::Reject { .. }
    ));
}

#[test]
fn server_to_client_roundtrip() {
    let (server_transport, client_transport) = InMemoryTransport::pair();

    let n_entities = 3;

    let mut server = server(world_with_entities(n_entities), server_transport);
    let mut client = client(World::new(), client_transport);

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

    assert!(
        client.last_applied_tick() > TickId::ZERO,
        "client must have applied at least one snapshot; last_applied_tick={:?}",
        client.last_applied_tick()
    );
    assert_eq!(client.session_id(), Some(crcbl_net::SessionId(1)));
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
    assert_eq!(server.processing_error_count(), 0);
    assert_eq!(client.processing_error_count(), 0);
}

#[test]
fn server_snapshot_with_300_systems_reaches_client() {
    let (server_transport, client_transport) = InMemoryTransport::pair();
    let mut server = server(world_with_systems(300), server_transport);
    let mut client = client(World::new(), client_transport);
    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    server.update(std::time::Duration::ZERO);
    client.update(std::time::Duration::ZERO);
    server.update(tick_dt);
    client.update(tick_dt);
    server.update(tick_dt * 2);
    client.update(tick_dt * 2);

    assert_eq!(client.baseline_system_count(), 300);
    assert_eq!(client.baseline_entity_count(), 300);
    assert_eq!(server.processing_error_count(), 0);
    assert_eq!(client.processing_error_count(), 0);
}

#[test]
fn server_rejects_invalid_reconnect_tokens_and_resumes_matching_token() {
    let (server_transport, mut peer) = InMemoryTransport::pair();
    let mut server = server(world_with_entities(1), server_transport);
    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    send_hello(&mut peer, None);
    server.update(std::time::Duration::ZERO);
    server.update(tick_dt);
    let token = match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh handshake rejected: {reason:?}")
        }
    };
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);

    drop(peer);
    let (server_transport, mut peer) = InMemoryTransport::pair();
    server.update(tick_dt * 2);
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Reconnecting
    );
    server.reconnect(server_transport);

    for (index, token) in [None, Some(crcbl_net::ResumeToken::from_bytes([2; 32]))]
        .into_iter()
        .enumerate()
    {
        send_hello(&mut peer, token);
        server.update(tick_dt * (3 + index as u32));
        match recv_handshake(&mut peer) {
            crcbl_net::HandshakeResult::Reject { reason, .. } => {
                assert_eq!(reason.code, crcbl_net::RejectReason::INVALID_SESSION_TOKEN)
            }
            crcbl_net::HandshakeResult::Accept { .. } => panic!("invalid reconnect token accepted"),
        }
        assert_eq!(
            server.session_state(),
            crcbl_net::SessionState::Reconnecting
        );
    }

    send_hello(&mut peer, Some(token));
    server.update(tick_dt * 5);
    let rotated_token = match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("matching reconnect token rejected: {reason:?}")
        }
    };
    assert_ne!(rotated_token, token);
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);

    drop(peer);
    let (server_transport, mut peer) = InMemoryTransport::pair();
    server.update(tick_dt * 6);
    server.reconnect(server_transport);
    send_hello(&mut peer, Some(token));
    server.update(tick_dt * 7);
    assert!(matches!(
        recv_handshake(&mut peer),
        crcbl_net::HandshakeResult::Reject { .. }
    ));
    send_hello(&mut peer, Some(rotated_token));
    server.update(tick_dt * 8);
    assert!(matches!(
        recv_handshake(&mut peer),
        crcbl_net::HandshakeResult::Accept { .. }
    ));
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
    assert_eq!(server.processing_error_count(), 0);
}

#[test]
fn server_rejects_fresh_hello_with_session_token_and_retries_are_idempotent() {
    let (server_transport, mut peer) = InMemoryTransport::pair();
    let mut server = server(world_with_entities(1), server_transport);
    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    send_hello(
        &mut peer,
        Some(crcbl_net::ResumeToken::from_bytes([99; 32])),
    );
    server.update(std::time::Duration::ZERO);
    server.update(tick_dt);
    match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            assert_eq!(reason.code, crcbl_net::RejectReason::INVALID_SESSION_TOKEN)
        }
        crcbl_net::HandshakeResult::Accept { .. } => panic!("fresh stale token accepted"),
    }
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Disconnected
    );

    send_hello(&mut peer, None);
    server.update(tick_dt * 2);
    let token = match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh handshake rejected: {reason:?}")
        }
    };
    send_hello(&mut peer, Some(token));
    server.update(tick_dt * 3);
    assert!(matches!(
        recv_handshake(&mut peer),
        crcbl_net::HandshakeResult::Accept { .. }
    ));
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
    assert_eq!(server.processing_error_count(), 0);
}

#[test]
fn server_and_client_resume_session_on_replacement_transport() {
    let (server_transport, client_transport) = InMemoryTransport::pair();
    let mut server = server(world_with_entities(1), server_transport);
    let mut client = client(World::new(), client_transport);
    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    server.update(std::time::Duration::ZERO);
    client.update(std::time::Duration::ZERO);
    server.update(tick_dt);
    client.update(tick_dt);
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
    assert_eq!(client.session_id(), Some(crcbl_net::SessionId(1)));

    let (server_transport, client_transport) = InMemoryTransport::pair();
    client.reconnect(client_transport);
    server.update(tick_dt * 2);
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Reconnecting
    );

    server.reconnect(server_transport);
    client.update(tick_dt * 2);
    server.update(tick_dt * 3);
    client.update(tick_dt * 3);

    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
    assert_eq!(client.session_id(), Some(crcbl_net::SessionId(1)));
    assert_eq!(server.processing_error_count(), 0);
    assert_eq!(client.processing_error_count(), 0);
}

#[test]
fn independently_created_servers_reject_each_others_resume_tokens() {
    let (a_transport, mut a_peer) = InMemoryTransport::pair();
    let (b_transport, mut b_peer) = InMemoryTransport::pair();
    let mut server_a = server(world_with_entities(1), a_transport);
    let mut server_b = server(world_with_entities(1), b_transport);

    send_hello(&mut a_peer, None);
    send_hello(&mut b_peer, None);
    server_a.update(std::time::Duration::ZERO);
    server_b.update(std::time::Duration::ZERO);
    server_a.update(std::time::Duration::from_nanos(16_666_667));
    server_b.update(std::time::Duration::from_nanos(16_666_667));
    let token_a = match recv_handshake(&mut a_peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("server A rejected: {reason:?}")
        }
    };
    let token_b = match recv_handshake(&mut b_peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("server B rejected: {reason:?}")
        }
    };
    assert_ne!(token_a, token_b);

    let (replacement, mut replacement_peer) = InMemoryTransport::pair();
    drop(b_peer);
    server_b.update(std::time::Duration::from_secs(1));
    server_b.reconnect(replacement);
    send_hello(&mut replacement_peer, Some(token_a));
    server_b.update(std::time::Duration::from_secs(2));
    match recv_handshake(&mut replacement_peer) {
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            assert_eq!(reason.code, crcbl_net::RejectReason::INVALID_SESSION_TOKEN)
        }
        crcbl_net::HandshakeResult::Accept { .. } => {
            panic!("server B accepted server A credential")
        }
    }
}

#[test]
fn client_and_server_reject_compatibility_mismatches() {
    let server_compatibility = crcbl_net::ProtocolCompatibility {
        protocol_version: crcbl_net::ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 0xA1,
        schema_hash: 0xB1,
    };
    for client_compatibility in [
        crcbl_net::ProtocolCompatibility {
            engine_build_id: 0xA2,
            ..server_compatibility
        },
        crcbl_net::ProtocolCompatibility {
            schema_hash: 0xB2,
            ..server_compatibility
        },
    ] {
        let (server_transport, client_transport) = InMemoryTransport::pair();
        let mut server = Server::try_new_with_compatibility(
            world_with_entities(1),
            server_transport,
            60,
            server_compatibility,
        )
        .expect("OS CSPRNG available");
        let mut client = Client::new_with_compatibility(
            World::new(),
            client_transport,
            60,
            client_compatibility,
        );
        client.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::ZERO);
        server.update(std::time::Duration::from_nanos(16_666_667));
        client.update(std::time::Duration::from_nanos(16_666_667));
        assert_eq!(
            server.session_state(),
            crcbl_net::SessionState::Disconnected
        );
        assert_eq!(client.processing_error_count(), 1);
    }
}

#[test]
fn injected_time_expires_reconnect_without_sleep() {
    let (server_transport, mut peer) = InMemoryTransport::pair();
    let mut server = server(world_with_entities(1), server_transport);
    server.set_session_config(crcbl_net::SessionConfig {
        reconnect_grace_period: std::time::Duration::from_secs(1),
        baseline_ring_capacity: 64,
    });
    send_hello(&mut peer, None);
    server.update(std::time::Duration::ZERO);
    server.update(std::time::Duration::from_nanos(16_666_667));
    let token = match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh handshake rejected: {reason:?}")
        }
    };
    drop(peer);
    let (replacement, mut replacement_peer) = InMemoryTransport::pair();
    server.update(std::time::Duration::from_secs(1));
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Reconnecting
    );
    server.reconnect(replacement);
    server.update(std::time::Duration::from_secs(3));
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Disconnected
    );
    send_hello(&mut replacement_peer, None);
    server.update(std::time::Duration::from_secs(4));
    let new_token = match recv_handshake(&mut replacement_peer) {
        crcbl_net::HandshakeResult::Accept {
            session_id,
            resume_token,
            ..
        } => {
            assert_ne!(session_id, crcbl_net::SessionId(1));
            resume_token
        }
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh session rejected: {reason:?}")
        }
    };
    assert_ne!(token, new_token);

    drop(replacement_peer);
    let (replacement, mut replacement_peer) = InMemoryTransport::pair();
    server.update(std::time::Duration::from_secs(5));
    server.reconnect(replacement);
    send_hello(&mut replacement_peer, Some(token));
    server.update(std::time::Duration::from_secs(6));
    match recv_handshake(&mut replacement_peer) {
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            assert_eq!(reason.code, crcbl_net::RejectReason::INVALID_SESSION_TOKEN)
        }
        crcbl_net::HandshakeResult::Accept { .. } => panic!("expired reconnect accepted"),
    }
}

#[test]
fn stale_reconnect_hello_after_grace_expiry_rotates_session_on_fresh_join() {
    let (server_transport, mut peer) = InMemoryTransport::pair();
    let mut server = server(world_with_entities(1), server_transport);
    server.set_session_config(crcbl_net::SessionConfig {
        reconnect_grace_period: std::time::Duration::from_secs(1),
        baseline_ring_capacity: 64,
    });
    send_hello(&mut peer, None);
    server.update(std::time::Duration::ZERO);
    server.update(std::time::Duration::from_nanos(16_666_667));
    let token = match recv_handshake(&mut peer) {
        crcbl_net::HandshakeResult::Accept { resume_token, .. } => resume_token,
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh handshake rejected: {reason:?}")
        }
    };

    drop(peer);
    let (replacement, mut replacement_peer) = InMemoryTransport::pair();
    server.update(std::time::Duration::from_secs(1));
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Reconnecting
    );
    server.reconnect(replacement);

    // Queue the token-bearing hello before the expiry update, so the hello
    // itself trips the grace deadline (the bug's shape): the session expires
    // without `session_terminated` being set, and the fresh join that follows
    // silently re-issues the dead session's credential.
    send_hello(&mut replacement_peer, Some(token));
    server.update(std::time::Duration::from_secs(3));
    match recv_handshake(&mut replacement_peer) {
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            assert_eq!(reason.code, crcbl_net::RejectReason::INVALID_SESSION_TOKEN)
        }
        crcbl_net::HandshakeResult::Accept { .. } => panic!("expired reconnect accepted"),
    }
    assert_eq!(
        server.session_state(),
        crcbl_net::SessionState::Disconnected
    );

    // A fresh token-less join must rotate the session — new id, new token —
    // never re-issue the expired session's credential.
    send_hello(&mut replacement_peer, None);
    server.update(std::time::Duration::from_secs(4));
    match recv_handshake(&mut replacement_peer) {
        crcbl_net::HandshakeResult::Accept {
            session_id,
            resume_token,
            ..
        } => {
            assert_ne!(session_id, crcbl_net::SessionId(1));
            assert_ne!(resume_token, token);
        }
        crcbl_net::HandshakeResult::Reject { reason, .. } => {
            panic!("fresh session rejected: {reason:?}")
        }
    }
    assert_eq!(server.session_state(), crcbl_net::SessionState::Connected);
}

#[test]
fn server_and_client_survive_multiple_ticks() {
    let (server_transport, client_transport) = InMemoryTransport::pair();

    let mut server = server(world_with_entities(5), server_transport);
    let mut client = client(World::new(), client_transport);

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

#[test]
fn physics_transforms_replicate_and_interpolate_end_to_end() {
    // Server world: a `PhysicsSystem` with one entity whose transform moves
    // +10 m/tick along X (driven by hand to keep the test deterministic).
    let mut world = World::new();
    let mut phys = crcbl_phys::PhysicsSystem::new();
    let entity = world.spawn();
    phys.set_transform(
        entity,
        crcbl_phys::Transform::from_position(glam::DVec3::ZERO),
    );
    world.register_system(Box::new(phys));

    let (server_transport, client_transport) = InMemoryTransport::pair();
    let mut server = server(world, server_transport);
    let mut client = client(World::new(), client_transport);

    let tick_dt = std::time::Duration::from_nanos(16_666_667);

    // Drive 4 ticks; before each, advance the entity's transform by 10 m.
    // The physics system is the only registered system.
    for i in 1..=4u32 {
        {
            let world = server.world_mut();
            let schedule = world.schedule_mut();
            let phys = schedule
                .iter_mut()
                .find_map(|s| s.as_any_mut().downcast_mut::<crcbl_phys::PhysicsSystem>())
                .expect("physics system registered");
            phys.set_transform(
                entity,
                crcbl_phys::Transform::from_position(glam::DVec3::new(
                    f64::from(i) * 10.0,
                    0.0,
                    0.0,
                )),
            );
        }
        server.update(tick_dt * i);
        client.update(tick_dt * i);
    }

    assert_eq!(server.processing_error_count(), 0);
    assert_eq!(client.processing_error_count(), 0);
    assert!(client.last_applied_tick() >= TickId::from_raw(3));

    // The newest two buffered snapshots are ticks 3 and 4 (positions 30 and
    // 40). alpha = 0.25 must land at 32.5 — interpolation, not a snapshot
    // copy.
    let state = client.interpolate(0.25);
    assert_eq!(state.transforms.len(), 1);
    let (bits, transform) = state.transforms[0];
    assert_eq!(bits, entity.to_bits());
    assert!(
        (transform.position.x - 32.5).abs() < 1e-9,
        "interpolated x was {}",
        transform.position.x
    );
}
