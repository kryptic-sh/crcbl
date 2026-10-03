//! LAN play in the sandbox: `--host`, `--join` and `--browse`, over the
//! engine's own UDP transport, through `crcbl::lan`.
//!
//! The session machinery is the engine's — the listener, the announcer, the
//! accept loop feeding a [`crcbl::server::Host`], the client connecting by
//! address or browsing, the "lan" section of the F3 panel — and what is left
//! here is what is the sandbox's: which of the three the command line asked
//! for — or the lobby picked (`crate::lobby`), which watches a join it
//! started through `Standing` — and what the host's world holds.
//!
//! The host's world is the smallest one that replicates something: a
//! "players" system with an entity per admitted peer, and the cube the host
//! spins (`crate::spin`). The sandbox has no game, so there is nothing to play
//! beyond joining, receiving the host's snapshots, seeing the player count
//! change as others come and go — and setting `sv_spin_rate`, which the host
//! takes from its own console and refuses from a client.
//!
//! A host asked to (`--record <FILE>`) records its session through
//! `crcbl::lan::LanHost::record`, from before its first frame. The file is
//! whole, and a host built like this one does **not** re-simulate it: the
//! players system changes on the host's session events, outside its module
//! (`LanHost::apply`), and nothing replays those — `crcbl::replay_record`'s
//! module docs say so of any game that does.
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

pub use imp::{Lan, LanError, SimRoute};
#[cfg(not(target_arch = "wasm32"))]
pub use imp::{LanMode, SANDBOX, Standing};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::path::Path;
    use std::time::Duration;

    use crcbl::console::{Fault, SimSet};
    use crcbl::ecs::{Entity, System, World};
    use crcbl::lan::{LanBind, LanGame};
    pub use crcbl::lan::{LanClient, LanError, LanMode};
    #[cfg(test)]
    use crcbl::net::udp::discovery::Announcement;
    use crcbl::net::{ConsoleSet, ProtocolCompatibility};
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

    /// What became of a simulation set handed to [`Lan::route_sim_set`].
    #[derive(Debug)]
    pub enum SimRoute {
        /// No session: the set is the sandbox's own simulation's to apply.
        Offline(SimSet),
        /// Handed to the session's host; its answer is printed when it comes.
        Sent,
    }

    /// Where a session this sandbox joined stands.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Standing {
        /// On the way in: connecting or hand-shaking.
        Joining,
        /// Admitted: the host's snapshots are arriving.
        InSession,
        /// Over, and how, in words: the host refused the join, ended the
        /// session, or the link ended.
        Over(String),
    }

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
            Self::in_role(Role::Off)
        }

        /// Starts what `mode` asks for, ticking at `tick_hz` — a host and
        /// its clients must agree on it — a host recording to the new file
        /// `record` names, if it names one. A host prints where it listens,
        /// since `--join` needs the port.
        ///
        /// # Errors
        ///
        /// [`LanError`] when a socket could not be bound, a connect could not
        /// start or the recording would not.
        pub fn start(mode: LanMode, tick_hz: u32, record: Option<&Path>) -> Result<Self, LanError> {
            match mode {
                LanMode::Off => Ok(Self::off()),
                LanMode::Host { port } => Self::host(LanBind::on_the_lan(port), tick_hz, record),
                LanMode::Join(addr) => Self::join(addr, tick_hz),
                LanMode::Browse => Ok(Self::in_role(Role::Client(Box::new(
                    LanClient::browse_the_lan(SANDBOX, tick_hz)?,
                )))),
            }
        }

        /// Hosts a session bound where `bind` says, ticking at `tick_hz`,
        /// recording it to the new file `record` names, if it names one.
        ///
        /// # Errors
        ///
        /// [`LanError`] when the listener could not be bound or the recording
        /// would not start.
        pub fn host(bind: LanBind, tick_hz: u32, record: Option<&Path>) -> Result<Self, LanError> {
            Ok(Self::in_role(Role::Host(Box::new(LanHost::open_with(
                bind, tick_hz, record,
            )?))))
        }

        /// Joins the sandbox host at `addr`, ticking at `tick_hz`.
        ///
        /// # Errors
        ///
        /// [`LanError`] when the connect could not start.
        pub fn join(addr: SocketAddr, tick_hz: u32) -> Result<Self, LanError> {
            Ok(Self::in_role(Role::Client(Box::new(LanClient::join(
                SANDBOX, addr, tick_hz,
            )?))))
        }

        const fn in_role(role: Role) -> Self {
            Self {
                role,
                now: Duration::ZERO,
            }
        }

        /// The host this sandbox joined, once it has chosen one.
        #[cfg(test)]
        pub fn joined(&self) -> Option<SocketAddr> {
            match &self.role {
                Role::Client(client) => client.host(),
                Role::Off | Role::Host(_) => None,
            }
        }

        /// Where the session this sandbox joined stands, or `None` with no
        /// session or a hosted one. A browse still looking is joining.
        pub fn standing(&self) -> Option<Standing> {
            let Role::Client(lan) = &self.role else {
                return None;
            };
            let (Some(host), Some(client)) = (lan.host(), lan.client()) else {
                return Some(Standing::Joining);
            };
            Some(if let Some(refusal) = client.handshake_refusal() {
                Standing::Over(format!("{host} refused the join: {}", refusal.msg))
            } else if let Some(ended) = client.ended() {
                Standing::Over(crcbl::lan::how_it_ended(ended, client))
            } else if client.session_id().is_some() {
                Standing::InSession
            } else {
                Standing::Joining
            })
        }

        /// Serves the session for one frame covering `render_dt`, and prints
        /// the answers to this sandbox's simulation sets.
        pub fn frame(&mut self, render_dt: Duration) {
            self.now += render_dt;
            match &mut self.role {
                Role::Off => {}
                Role::Host(host) => {
                    host.frame(self.now);
                    for reply in host.lan.host_mut().take_console_replies() {
                        crcbl::log::console::print(&reply.to_string());
                    }
                }
                Role::Client(client) => {
                    client.frame(self.now);
                    if let Some(client) = client.client_mut() {
                        for reply in client.console_replies() {
                            crcbl::log::console::print(&reply.to_string());
                        }
                    }
                }
            }
        }

        /// Hands `set` to the session's simulation: a host applies it at the
        /// start of its next tick, and a client sends it to its host, which
        /// refuses it — only the host sets a simulation variable. With no
        /// session the set is given back, for the sandbox's own simulation.
        ///
        /// # Errors
        ///
        /// A [`Fault`] when a client has no session to send it on yet, or the
        /// send failed — never applied locally, since the simulation the set
        /// was meant for is the host's.
        pub fn route_sim_set(&mut self, set: SimSet) -> Result<SimRoute, Fault> {
            let console_set = ConsoleSet {
                name: set.name().to_owned(),
                value: set.value_text(),
            };
            match &mut self.role {
                Role::Off => Ok(SimRoute::Offline(set)),
                Role::Host(host) => {
                    host.lan.host_mut().submit_console_set(console_set);
                    Ok(SimRoute::Sent)
                }
                Role::Client(lan) => {
                    let Some(client) = lan.client_mut() else {
                        return Err(Fault::new(format!(
                            "{set}: not in a session with a host yet"
                        )));
                    };
                    client
                        .send_console_set(&console_set)
                        .map(|()| SimRoute::Sent)
                        .map_err(|error| Fault::new(format!("{set}: {error}")))
                }
            }
        }

        /// How far the session's cube has spun — the host's own, or what the
        /// joined host last replicated — or `None` with no session or before
        /// a snapshot arrived.
        pub fn cube_seconds(&mut self) -> Option<f32> {
            match &mut self.role {
                Role::Off => None,
                Role::Host(host) => crate::spin::hosted_seconds(host.lan.host_mut().world_mut()),
                Role::Client(lan) => lan.client().and_then(crate::spin::replicated_seconds),
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
                None,
            )
        }

        fn open_with(bind: LanBind, tick_hz: u32, record: Option<&Path>) -> Result<Self, LanError> {
            let mut lan = crcbl::lan::LanHost::open(SANDBOX, bind, world(), tick_hz)?;
            serve_spin(lan.host_mut());
            if let Some(path) = record {
                lan.record(path)?;
            }
            Ok(Self {
                lan,
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
                // The same session on the same link, connected all along: its
                // entity is already there, and the sandbox sends its players
                // no event that a restarted key could have lost.
                PeerEvent::Reaccepted(_) => {}
            }
        }
    }

    impl DebugModule for LanHost {
        fn debug_section(&self, out: &mut DebugSection) {
            self.lan.debug_section(out);
        }
    }

    /// The host's world: an empty "players" system the peers fill, and the
    /// cube the host spins.
    pub(super) fn world() -> World {
        let mut world = World::new();
        world.register_system(Box::new(System::<bool>::new(PLAYERS)));
        crate::spin::HostedSpin::install(&mut world);
        world
    }

    /// Has `host` spin its world's cube, taking `sv_spin_rate` from its own
    /// console and its own player.
    pub(super) fn serve_spin(host: &mut crcbl::server::Host) {
        host.set_module(Box::new(crate::spin::SpinModule));
        host.set_sim_registry(crate::spin::sim_registry());
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod sim_tests;

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::time::Duration;

    use crcbl::ui::DebugPanel;

    use crcbl::console::{Fault, SimSet};

    /// No networking in a web build.
    #[derive(Debug)]
    pub struct Lan;

    /// What became of a simulation set handed to [`Lan::route_sim_set`].
    #[derive(Debug)]
    pub enum SimRoute {
        /// No session — a web build has none — so the set is the sandbox's
        /// own simulation's to apply.
        Offline(SimSet),
    }

    impl Lan {
        /// No session.
        pub const fn off() -> Self {
            Self
        }

        /// Gives `set` back: there is no session to hand it to.
        ///
        /// # Errors
        ///
        /// Never; the signature is the native one's.
        pub fn route_sim_set(&mut self, set: SimSet) -> Result<SimRoute, Fault> {
            Ok(SimRoute::Offline(set))
        }

        /// No session, so no session's cube.
        pub fn cube_seconds(&mut self) -> Option<f32> {
            None
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
