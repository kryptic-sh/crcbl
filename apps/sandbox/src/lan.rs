//! LAN play in the sandbox: `--host`, `--join` and `--browse`, over the
//! engine's own UDP transport, through `crcbl::lan`.
//!
//! The session machinery is the engine's — the listener, the announcer, the
//! accept loop feeding a [`crcbl::server::Host`], the client connecting by
//! address or browsing, the "lan" section of the F3 panel — and what is left
//! here is what is the sandbox's: which of the three the command line asked
//! for, and what the host's world holds.
//!
//! The host's world is the smallest one that replicates something: a
//! "players" system with an entity per admitted peer. The sandbox has no
//! game, so there is nothing to play beyond joining, receiving the host's
//! snapshots, and seeing the player count change as others come and go.
//!
//! # One datagram per snapshot
//!
//! This world's snapshot sits far under
//! [`MAX_UNRELIABLE_PAYLOAD`](crcbl::net::reliable::MAX_UNRELIABLE_PAYLOAD)
//! (a test holds that); `crcbl::lan` says what happens to one that grows
//! past it.
//!
//! # Native only
//!
//! Web builds have no networking, by the LOCKED rule in
//! `docs/notes/simulation.md`, and the transport is compiled out of them. On
//! `wasm32` the three flags are not parsed at all and [`Lan`] is inert, as
//! `crate::steam`'s link is without its feature.

#[cfg(not(target_arch = "wasm32"))]
pub use imp::LanMode;
pub use imp::{Lan, LanError};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::HashMap;
    #[cfg(test)]
    use std::net::SocketAddr;
    use std::time::Duration;

    use crcbl::ecs::{Entity, System, World};
    use crcbl::lan::{LanBind, LanGame};
    pub use crcbl::lan::{LanClient, LanError, LanMode};
    use crcbl::net::ProtocolCompatibility;
    #[cfg(test)]
    use crcbl::net::udp::discovery::Announcement;
    use crcbl::server::{PeerEvent, PeerId};
    use crcbl::ui::{DebugModule, DebugPanel, DebugSection};

    /// The endpoint protocol id the sandbox's links speak — `SBOX`. A
    /// listener or browser on another answers nothing and lists nothing.
    pub const PROTOCOL_ID: u32 = u32::from_be_bytes(*b"SBOX");

    /// What the session handshake gates on. The schema hash spells `SBOX`,
    /// so no other sample's client is admitted.
    pub const COMPATIBILITY: ProtocolCompatibility = ProtocolCompatibility {
        protocol_version: ProtocolCompatibility::DEFAULT.protocol_version,
        engine_build_id: 0x0043_5243_424C,
        schema_hash: 0x0000_5342_4F58,
    };

    /// The most players a sandbox host admits, and what it announces.
    pub const MAX_PLAYERS: u16 = 8;

    /// The sandbox's LAN session, as [`crcbl::lan`] knows it.
    pub const SANDBOX: LanGame = LanGame {
        app: "sandbox",
        host_name: "crcbl sandbox",
        protocol_id: PROTOCOL_ID,
        compatibility: COMPATIBILITY,
        max_players: MAX_PLAYERS,
    };

    /// The replicated system holding one entity per admitted peer.
    const PLAYERS: &str = "players";

    /// The sandbox's side of a LAN session, or none.
    #[derive(Debug)]
    pub struct Lan {
        role: Role,
        /// Wall time since the session started, summed from each frame's
        /// `render_dt`, so a paused sandbox keeps its links alive.
        now: Duration,
    }

    #[derive(Debug)]
    enum Role {
        Off,
        Host(Box<LanHost>),
        Client(Box<LanClient>),
    }

    impl Lan {
        /// No networking.
        pub const fn off() -> Self {
            Self {
                role: Role::Off,
                now: Duration::ZERO,
            }
        }

        /// Starts what `mode` asks for, ticking at `tick_hz` — a host and
        /// its clients must agree on it. A host prints where it listens,
        /// since `--join` needs the port.
        ///
        /// # Errors
        ///
        /// [`LanError`] when a socket could not be bound or a connect could
        /// not start.
        pub fn start(mode: LanMode, tick_hz: u32) -> Result<Self, LanError> {
            let role = match mode {
                LanMode::Off => Role::Off,
                LanMode::Host { port } => Role::Host(Box::new(LanHost::open_with(
                    LanBind::on_the_lan(port),
                    tick_hz,
                )?)),
                LanMode::Join(addr) => {
                    Role::Client(Box::new(LanClient::join(SANDBOX, addr, tick_hz)?))
                }
                LanMode::Browse => {
                    Role::Client(Box::new(LanClient::browse_the_lan(SANDBOX, tick_hz)?))
                }
            };
            Ok(Self {
                role,
                now: Duration::ZERO,
            })
        }

        /// Serves the session for one frame covering `render_dt`.
        pub fn frame(&mut self, render_dt: Duration) {
            self.now += render_dt;
            match &mut self.role {
                Role::Off => {}
                Role::Host(host) => host.frame(self.now),
                Role::Client(client) => client.frame(self.now),
            }
        }

        /// Adds the "lan" section to the F3 panel — only with a session, so
        /// a run without one has the panel it always had.
        pub fn debug_sections(&self, panel: &mut DebugPanel) {
            match &self.role {
                Role::Off => {}
                Role::Host(host) => panel.add(host.as_ref()),
                Role::Client(client) => panel.add(client.as_ref()),
            }
        }
    }

    /// The engine's LAN host, and the "players" system its peers fill.
    #[derive(Debug)]
    pub struct LanHost {
        lan: crcbl::lan::LanHost,
        /// Each admitted peer's entity in the "players" system.
        players: HashMap<PeerId, Entity>,
    }

    impl LanHost {
        /// A host listening on `listen`, announcing from `announce_at` and
        /// broadcasting to `broadcast_to` (or answering queries only, for
        /// `None`), ticking at `tick_hz`.
        ///
        /// # Errors
        ///
        /// As [`crcbl::lan::LanHost::open`].
        #[cfg(test)]
        pub fn open(
            listen: SocketAddr,
            announce_at: SocketAddr,
            broadcast_to: Option<SocketAddr>,
            tick_hz: u32,
        ) -> Result<Self, LanError> {
            Self::open_with(
                LanBind {
                    listen,
                    announce_at,
                    broadcast_to,
                },
                tick_hz,
            )
        }

        fn open_with(bind: LanBind, tick_hz: u32) -> Result<Self, LanError> {
            Ok(Self {
                lan: crcbl::lan::LanHost::open(SANDBOX, bind, world(), tick_hz)?,
                players: HashMap::new(),
            })
        }

        /// The port the listener is bound to.
        #[cfg(test)]
        pub fn game_port(&self) -> u16 {
            self.lan.game_port()
        }

        /// The session host.
        #[cfg(test)]
        pub fn host(&self) -> &crcbl::server::Host {
            self.lan.host()
        }

        /// What the host announces, while it announces.
        #[cfg(test)]
        pub fn announcement(&self) -> Option<&Announcement> {
            self.lan.announcement()
        }

        /// Where the announcer answers queries, while it announces.
        #[cfg(test)]
        pub fn announcer_addr(&self) -> Option<SocketAddr> {
            self.lan.announcer_addr()
        }

        /// Serves the session to `now`, and keeps the players system in step
        /// with who is in.
        pub fn frame(&mut self, now: Duration) {
            for event in self.lan.frame(now) {
                self.apply(event);
            }
        }

        /// One session change, reflected in the players system: an entity
        /// per admitted peer, marked connected or not.
        fn apply(&mut self, event: PeerEvent) {
            let world = self.lan.host_mut().world_mut();
            match event {
                PeerEvent::Joined(peer) => {
                    let entity = world.spawn();
                    players(world).attach(entity, true);
                    self.players.insert(peer, entity);
                }
                PeerEvent::Lost(peer) | PeerEvent::Resumed(peer) => {
                    if let Some(connected) = self
                        .players
                        .get(&peer)
                        .and_then(|&entity| players(world).get_mut(entity))
                    {
                        *connected = matches!(event, PeerEvent::Resumed(_));
                    }
                }
                PeerEvent::Left(peer) => {
                    if let Some(entity) = self.players.remove(&peer) {
                        world.despawn(entity);
                    }
                }
            }
        }
    }

    impl DebugModule for LanHost {
        fn debug_section(&self, out: &mut DebugSection) {
            self.lan.debug_section(out);
        }
    }

    /// The host's world: an empty "players" system the peers fill.
    fn world() -> World {
        let mut world = World::new();
        world.register_system(Box::new(System::<bool>::new(PLAYERS)));
        world
    }

    /// The players system, which [`world`] always registers.
    fn players(world: &mut World) -> &mut System<bool> {
        world
            .system_mut::<System<bool>>()
            .expect("the host's world registers the players system")
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::time::Duration;

    use crcbl::ui::DebugPanel;

    /// No networking in a web build.
    #[derive(Debug)]
    pub struct Lan;

    impl Lan {
        /// No session.
        pub const fn off() -> Self {
            Self
        }

        /// Nothing to serve.
        pub fn frame(&mut self, _render_dt: Duration) {}

        /// No section without a session.
        pub fn debug_sections(&self, _panel: &mut DebugPanel) {}
    }

    /// Nothing to fail at: a web build starts no LAN session.
    #[derive(Debug)]
    pub enum LanError {}

    impl std::fmt::Display for LanError {
        fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match *self {}
        }
    }
}
