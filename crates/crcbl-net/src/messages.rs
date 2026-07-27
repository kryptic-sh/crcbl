//! Typed protocol messages and snapshot helpers.

use crcbl_core::TickId;

use crate::types::SectorId;

// ── Typed protocol messages ───────────────────────────────────────────────────

/// Messages sent from the client to the server.
#[derive(Debug)]
pub enum ClientToServer {
    /// An input tick — the client's input state at a given server tick.
    Input {
        /// The server tick this input is for.
        tick: TickId,
        /// Serialised input data.
        data: Vec<u8>,
    },
    /// A command — chat, ready-up, etc.
    Command {
        /// Serialised command payload.
        data: Vec<u8>,
    },
}

/// Messages sent from the server to the client.
#[derive(Debug)]
pub enum ServerToClient {
    /// A world snapshot for a given tick.
    Snapshot {
        /// The sector this snapshot covers.
        sector: SectorId,
        /// The server tick this snapshot represents.
        tick: TickId,
        /// Per-system snapshot data.
        systems: Vec<SystemSnapshot>,
    },
    /// An event — chat message, server notification, etc.
    Event {
        /// Serialised event payload.
        data: Vec<u8>,
    },
}

/// A single system's snapshot data.
#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    /// Unique identifier for the system that produced this data.
    pub system_id: u32,
    /// Serialised snapshot payload.
    pub data: Vec<u8>,
}

// ── Snapshot helpers ──────────────────────────────────────────────────────────

/// Builds a [`ServerToClient::Snapshot`] incrementally, system by system.
pub struct SnapshotWriter {
    systems: Vec<SystemSnapshot>,
    tick: TickId,
    sector: SectorId,
}

impl SnapshotWriter {
    /// Create a writer for the given server tick using the default (zero) sector.
    pub fn new(tick: TickId) -> Self {
        Self {
            systems: Vec::new(),
            tick,
            sector: SectorId::ZERO,
        }
    }

    /// Create a writer for the given sector and server tick.
    pub fn new_with_sector(sector: SectorId, tick: TickId) -> Self {
        Self {
            systems: Vec::new(),
            tick,
            sector,
        }
    }

    /// Append a system's snapshot data.
    pub fn write_system(&mut self, system_id: u32, data: Vec<u8>) {
        self.systems.push(SystemSnapshot { system_id, data });
    }

    /// Consume the writer and produce the finished [`ServerToClient`] message.
    pub fn finish(self) -> ServerToClient {
        ServerToClient::Snapshot {
            sector: self.sector,
            tick: self.tick,
            systems: self.systems,
        }
    }
}

impl std::fmt::Debug for SnapshotWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotWriter")
            .field("tick", &self.tick)
            .field("system_count", &self.systems.len())
            .finish()
    }
}

/// Reads per-system data out of a [`ServerToClient::Snapshot`].
pub struct SnapshotReader<'a> {
    sector: SectorId,
    tick: TickId,
    systems: &'a [SystemSnapshot],
}

impl<'a> SnapshotReader<'a> {
    /// Extract a reader from a server-to-client message.
    ///
    /// Returns `None` if the message is not a snapshot.
    pub fn from_snapshot(msg: &'a ServerToClient) -> Option<Self> {
        match msg {
            ServerToClient::Snapshot {
                sector,
                tick,
                systems,
            } => Some(Self {
                sector: *sector,
                tick: *tick,
                systems,
            }),
            ServerToClient::Event { .. } => None,
        }
    }

    /// The sector this snapshot covers.
    pub fn sector(&self) -> SectorId {
        self.sector
    }

    /// The server tick this snapshot represents.
    pub fn tick(&self) -> TickId {
        self.tick
    }

    /// Look up a system's data by its id.
    pub fn system_data(&self, system_id: u32) -> Option<&[u8]> {
        self.systems
            .iter()
            .find(|s| s.system_id == system_id)
            .map(|s| s.data.as_slice())
    }

    /// Iterate over every system snapshot, yielding `(system_id, data)`.
    pub fn iter_systems(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.systems
            .iter()
            .map(|s| (s.system_id, s.data.as_slice()))
    }
}

impl<'a> std::fmt::Debug for SnapshotReader<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotReader")
            .field("sector", &self.sector)
            .field("tick", &self.tick)
            .field("system_count", &self.systems.len())
            .finish()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_writer_reader_roundtrip() {
        let tick = TickId::from_raw(42);
        let sector = SectorId { x: 1, y: 2, z: 3 };
        let mut writer = SnapshotWriter::new_with_sector(sector, tick);

        writer.write_system(1, b"physics".to_vec());
        writer.write_system(2, b"render".to_vec());
        writer.write_system(3, b"audio".to_vec());

        let msg = writer.finish();
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();

        assert_eq!(reader.sector(), sector);
        assert_eq!(reader.tick(), tick);
        assert_eq!(reader.system_data(1), Some(b"physics".as_slice()));
        assert_eq!(reader.system_data(2), Some(b"render".as_slice()));
        assert_eq!(reader.system_data(3), Some(b"audio".as_slice()));
        assert_eq!(reader.system_data(99), None);
    }

    #[test]
    fn snapshot_reader_rejects_event() {
        let event = ServerToClient::Event {
            data: b"chat".to_vec(),
        };
        assert!(SnapshotReader::from_snapshot(&event).is_none());
    }

    #[test]
    fn snapshot_writer_empty() {
        let writer = SnapshotWriter::new(TickId::from_raw(1));
        let msg = writer.finish();

        let reader = SnapshotReader::from_snapshot(&msg).unwrap();
        assert_eq!(reader.tick(), TickId::from_raw(1));
        assert_eq!(reader.iter_systems().count(), 0);
    }

    #[test]
    fn snapshot_reader_iter_systems() {
        let mut writer = SnapshotWriter::new(TickId::from_raw(7));
        writer.write_system(10, vec![0]);
        writer.write_system(20, vec![1, 2]);

        let msg = writer.finish();
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();

        let systems: Vec<_> = reader.iter_systems().collect();
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0], (10, &[0][..]));
        assert_eq!(systems[1], (20, &[1, 2][..]));
    }

    #[test]
    fn debug_formatting() {
        let _ = format!(
            "{:?}",
            ClientToServer::Input {
                tick: TickId::from_raw(1),
                data: vec![4],
            }
        );
        let _ = format!("{:?}", ClientToServer::Command { data: vec![5] });
        let _ = format!(
            "{:?}",
            ServerToClient::Snapshot {
                sector: SectorId::ZERO,
                tick: TickId::from_raw(2),
                systems: vec![SystemSnapshot {
                    system_id: 1,
                    data: vec![6],
                }],
            }
        );
        let _ = format!("{:?}", ServerToClient::Event { data: vec![7] });
        let _ = format!(
            "{:?}",
            SystemSnapshot {
                system_id: 42,
                data: vec![8],
            }
        );

        let writer = SnapshotWriter::new(TickId::from_raw(3));
        let _ = format!("{writer:?}");

        let msg = ServerToClient::Snapshot {
            sector: SectorId::ZERO,
            tick: TickId::from_raw(3),
            systems: vec![],
        };
        let reader = SnapshotReader::from_snapshot(&msg).unwrap();
        let _ = format!("{reader:?}");
    }
}
