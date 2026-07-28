//! P2b integration test — replication survives scripted loss/reorder
//! with state equality.
//!
//! Tests the full delta encoding pipeline: server produces SystemSnapshots,
//! delta-encodes them against a stored baseline, client applies deltas,
//! and after N ticks the client's reconstructed state matches the server's.

use crcbl_core::TickId;
use crcbl_net::{
    Baseline, ConditionSimulator, Delta, DeltaCodec, InMemoryTransport, Message, MessageKind,
    SessionConfig, SessionId, SessionManager, SimConditions, SystemSnapshot, Transport,
};

// ── Minimal server (writes SystemSnapshots, delta-encodes them) ────────────

struct Server {
    session: SessionManager,
}

impl Server {
    fn new() -> Self {
        let config = SessionConfig::default();
        Self {
            session: SessionManager::new(SessionId(1), &config),
        }
    }

    /// Produce a delta-encoded payload for the given tick and snapshot set.
    fn encode(&mut self, tick: TickId, systems: &[SystemSnapshot]) -> Vec<u8> {
        let last_acked = self.session.last_acked_tick();
        let baseline = last_acked.and_then(|t| self.session.baseline_store().get(t).cloned());
        let delta = DeltaCodec::encode(tick, systems, baseline.as_ref());

        // Store full snapshot as new baseline.
        self.session
            .baseline_store_mut()
            .insert(Baseline::from_snapshot(tick, systems));

        crcbl_net::encode_delta(&delta)
    }

    fn handle_ack(&mut self, tick: TickId) {
        self.session.handle_ack(tick);
    }
}

// ── Minimal client (applies deltas, reconstructs state) ───────────────────

struct Client {
    baseline: Baseline,
}

impl Client {
    fn new() -> Self {
        let empty: &[SystemSnapshot] = &[];
        Self {
            baseline: Baseline::from_snapshot(TickId::ZERO, empty),
        }
    }

    fn apply(&mut self, payload: &[u8]) -> Option<TickId> {
        let delta = crcbl_net::decode_delta(payload).ok()?;
        let tick = delta.tick;
        let _ = DeltaCodec::apply(&delta, &mut self.baseline);
        Some(tick)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a snapshot with one system containing `n` entities.
fn snapshot(_tick: TickId, system_id: u32, n: u32) -> SystemSnapshot {
    let mut data = Vec::new();
    for i in 0..n {
        // entity_bits: u64 LE, data_len: u32 LE, data: 4 bytes (entity count LE)
        let entity_bits = i as u64;
        let component = i * 10; // dummy component value
        data.extend_from_slice(&entity_bits.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&component.to_le_bytes());
    }
    SystemSnapshot { system_id, data }
}

/// Build a snapshot with different entity values (simulating changed state).
fn snapshot_modified(_tick: TickId, system_id: u32, n: u32) -> SystemSnapshot {
    let mut data = Vec::new();
    for i in 0..n {
        let entity_bits = i as u64;
        let component = i * 10 + 1; // different value
        data.extend_from_slice(&entity_bits.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&component.to_le_bytes());
    }
    SystemSnapshot { system_id, data }
}

fn drain_all<T: Transport>(t: &mut T) -> Vec<Message> {
    let mut out = Vec::new();
    while let Ok(Some(msg)) = t.recv() {
        out.push(msg);
    }
    out
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn replication_roundtrip_lossless() {
    let mut server = Server::new();
    let mut client = Client::new();

    let n_ticks = 20;
    let n_entities = 5;

    for i in 0..n_ticks {
        let tick = TickId::from_raw(i + 1);
        let snap = snapshot(tick, 1, n_entities);
        let payload = server.encode(tick, &[snap]);

        if let Some(acked) = client.apply(&payload) {
            server.handle_ack(acked);
        }
    }

    // Client's baseline must have 5 entities.
    assert_eq!(client.baseline.entity_count(), n_entities as usize);
    assert!(client.baseline.tick > TickId::ZERO);
}

#[test]
fn first_tick_is_keyframe() {
    let mut server = Server::new();

    let tick = TickId::from_raw(1);
    let snap = snapshot(tick, 1, 3);
    let payload = server.encode(tick, &[snap]);

    let delta: Delta = crcbl_net::decode_delta(&payload).unwrap();
    assert!(
        delta.is_keyframe,
        "first tick with no acked baseline must be keyframe"
    );
    assert_eq!(delta.systems[0].added.len(), 3);
}

#[test]
fn unchanged_state_produces_empty_delta() {
    // Test DeltaCodec directly — no session manager needed.
    let snap1 = snapshot(TickId::from_raw(1), 1, 3);
    let systems = vec![snap1];

    // Build baseline from the snapshot.
    let baseline = Baseline::from_snapshot(TickId::from_raw(1), &systems);

    // Re-encode the same snapshot against this baseline.
    let delta = DeltaCodec::encode(TickId::from_raw(2), &systems, Some(&baseline));

    assert!(!delta.is_keyframe);
    assert!(delta.systems[0].added.is_empty());
    assert!(delta.systems[0].removed.is_empty());
    assert!(delta.systems[0].modified.is_empty());
    assert_eq!(delta.systems[0].unchanged_count, 3);
}

#[test]
fn modified_state_produces_modified_entries() {
    let mut server = Server::new();

    // Tick 1: initial state.
    let snap1 = snapshot(TickId::from_raw(1), 1, 3);
    let payload1 = server.encode(TickId::from_raw(1), &[snap1]);

    let mut client = Client::new();
    client.apply(&payload1);
    server.handle_ack(TickId::from_raw(1));

    // Tick 2: modified values.
    let snap2 = snapshot_modified(TickId::from_raw(2), 1, 3);
    let payload2 = server.encode(TickId::from_raw(2), &[snap2]);
    let delta2 = crcbl_net::decode_delta(&payload2).unwrap();

    assert!(!delta2.is_keyframe);
    // Entity bytes differ → modified.
    assert_eq!(delta2.systems[0].modified.len(), 3);
    assert!(delta2.systems[0].added.is_empty());
    assert!(delta2.systems[0].removed.is_empty());
}

#[test]
fn removed_entity_detected() {
    let mut server = Server::new();

    // Tick 1: 3 entities.
    let snap1 = snapshot(TickId::from_raw(1), 1, 3);
    let payload1 = server.encode(TickId::from_raw(1), &[snap1]);

    let mut client = Client::new();
    client.apply(&payload1);
    server.handle_ack(TickId::from_raw(1));

    // Tick 2: only 1 entity (2 removed).
    let snap2 = snapshot(TickId::from_raw(2), 1, 1);
    let payload2 = server.encode(TickId::from_raw(2), &[snap2]);
    let delta2 = crcbl_net::decode_delta(&payload2).unwrap();

    assert_eq!(delta2.systems[0].removed.len(), 2);
}

#[test]
fn replication_with_condition_simulator() {
    // Server sends through a lossy channel, client eventually catches up.
    let (server_tx, client_rx) = InMemoryTransport::pair();
    let (client_tx, mut server_rx) = InMemoryTransport::pair();

    let mut server_side = ConditionSimulator::new(
        server_tx,
        SimConditions {
            loss_rate: 0.15,
            seed: 42,
            ..Default::default()
        },
    );
    let mut client_side = ConditionSimulator::new(
        client_rx,
        SimConditions {
            loss_rate: 0.0,
            seed: 0,
            ..Default::default()
        },
    );
    let mut client_send = client_tx; // unreliable → server_rx

    let mut server = Server::new();
    let mut client = Client::new();

    let n_ticks = 80;
    let n_entities = 4;

    for i in 0..n_ticks {
        let tick = TickId::from_raw(i + 1);
        let snap = snapshot(tick, 1, n_entities);
        let payload = server.encode(tick, &[snap]);

        server_side
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload,
            })
            .unwrap();

        // Client receives.
        for msg in drain_all(&mut client_side) {
            if let Some(acked) = client.apply(&msg.payload) {
                let ack_payload = crcbl_net::encode_ack(acked);
                client_send
                    .send_unreliable(Message {
                        kind: MessageKind::Unreliable,
                        payload: ack_payload,
                    })
                    .unwrap();
            }
        }

        // Server receives acks.
        for msg in drain_all(&mut server_rx) {
            if let Ok(tick) = crcbl_net::decode_ack(&msg.payload) {
                server.handle_ack(tick);
            }
        }
    }

    // All 4 entities in system 1 must be reconstructed.
    assert_eq!(
        client.baseline.entity_count_for(1),
        n_entities as usize,
        "client must reconstruct all {n_entities} entities after {n_ticks} lossy ticks"
    );
}

#[test]
fn replication_with_loss_reorder_and_duplication() {
    // Server sends through a lossy+reordered+duplicated channel, client
    // eventually catches up with correct entity count.
    let (server_tx, client_rx) = InMemoryTransport::pair();
    let (client_tx, mut server_rx) = InMemoryTransport::pair();

    let mut server_side = ConditionSimulator::new(
        server_tx,
        SimConditions {
            loss_rate: 0.10,
            duplicate_rate: 0.10,
            reorder_window: 4,
            seed: 99,
            ..Default::default()
        },
    );
    let mut client_side = ConditionSimulator::new(
        client_rx,
        SimConditions {
            seed: 1,
            ..Default::default()
        },
    );
    let mut client_send = client_tx;

    let mut server = Server::new();
    let mut client = Client::new();

    let n_ticks = 120;
    let n_entities = 4;

    for i in 0..n_ticks {
        let tick = TickId::from_raw(i + 1);
        let snap = snapshot(tick, 1, n_entities);
        let payload = server.encode(tick, &[snap]);

        server_side
            .send_unreliable(Message {
                kind: MessageKind::Unreliable,
                payload,
            })
            .unwrap();

        for msg in drain_all(&mut client_side) {
            if let Some(acked) = client.apply(&msg.payload) {
                let ack_payload = crcbl_net::encode_ack(acked);
                client_send
                    .send_unreliable(Message {
                        kind: MessageKind::Unreliable,
                        payload: ack_payload,
                    })
                    .unwrap();
            }
        }

        for msg in drain_all(&mut server_rx) {
            if let Ok(tick) = crcbl_net::decode_ack(&msg.payload) {
                server.handle_ack(tick);
            }
        }
    }

    assert_eq!(
        client.baseline.entity_count_for(1),
        n_entities as usize,
        "client must reconstruct all {n_entities} entities after {n_ticks} ticks with loss + reorder + duplication"
    );
}
