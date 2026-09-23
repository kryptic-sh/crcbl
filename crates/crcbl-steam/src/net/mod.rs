//! Steam networking as a [`crcbl_net::Transport`]: P2P connections through
//! Steam's relay, addressed by `SteamId`.
//!
//! - [`SteamTransport`] is one connection, and implements `Transport`.
//! - [`SteamListener`] is a host's listen socket: it admits only members of
//!   its lobby and yields a `SteamTransport` per peer.
//! - [`EndReason`] says why a connection ended — the host leaving, a kick, a
//!   full server, or a network loss.
//! - [`Networking`] starts Steam's relay access and reports whether it is
//!   ready, so a lobby screen can wait for it rather than fail its first
//!   connect.
//!
//! Peer identity comes from the connection: Steam's relay authenticates each
//! end, so [`SteamTransport::remote`] is the certified `SteamId` of whoever
//! is on the other end — no auth ticket needed.

mod end_reason;
pub(crate) mod identity;
mod listener;
mod transport;

use std::{cell::RefCell, collections::VecDeque, rc::Weak};

pub use end_reason::EndReason;
pub use listener::SteamListener;
pub use transport::{MAX_MESSAGE_BYTES, SteamTransport};

use crate::{
    Steam, SteamId,
    callbacks::fixed_string,
    ffi::structs::{SteamNetworkingIdentity, SteamRelayNetworkStatus},
};

/// The Steam id an identity from a callback names, if it names one.
pub(crate) fn remote_of(identity: &SteamNetworkingIdentity) -> Option<SteamId> {
    identity::steam_id(identity)
}

/// The identity naming `user`, for the fake library.
#[cfg(test)]
pub(crate) fn identity_of(user: SteamId) -> SteamNetworkingIdentity {
    identity::of(user)
}

/// A P2P "virtual port": which listen socket on the host a connection is for.
/// A game with one kind of connection uses `VirtualPort(0)` on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualPort(pub i32);

/// `ESteamNetworkingConnectionState` values the transport acts on.
pub(crate) mod state {
    /// `k_ESteamNetworkingConnectionState_None` — no such connection.
    pub(crate) const NONE: i32 = 0;
    /// `k_ESteamNetworkingConnectionState_Connecting`.
    pub(crate) const CONNECTING: i32 = 1;
    /// `k_ESteamNetworkingConnectionState_FindingRoute`.
    pub(crate) const FINDING_ROUTE: i32 = 2;
    /// `k_ESteamNetworkingConnectionState_Connected`.
    pub(crate) const CONNECTED: i32 = 3;
    /// `k_ESteamNetworkingConnectionState_ClosedByPeer`.
    pub(crate) const CLOSED_BY_PEER: i32 = 4;
    /// `k_ESteamNetworkingConnectionState_ProblemDetectedLocally`.
    pub(crate) const PROBLEM_DETECTED_LOCALLY: i32 = 5;
}

/// How ready a Steam networking service is (`ESteamNetworkingAvailability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Availability {
    /// A dependency is missing — no Internet, no network config
    /// (`k_ESteamNetworkingAvailability_CannotTry`).
    CannotTry,
    /// Tried long enough to have succeeded, and never did (`_Failed`).
    Failed,
    /// Worked once, has a problem now (`_Previously`).
    Previously,
    /// Failed, and retrying (`_Retrying`).
    Retrying,
    /// Not checked yet (`_NeverTried`).
    NeverTried,
    /// Waiting on something it needs, such as logging on (`_Waiting`).
    Waiting,
    /// Trying now (`_Attempting`).
    Attempting,
    /// Ready (`_Current`).
    Current,
    /// A value this crate does not name, or Valve's `_Unknown` (0).
    Unknown(i32),
}

impl Availability {
    /// Maps an `ESteamNetworkingAvailability`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            -102 => Self::CannotTry,
            -101 => Self::Failed,
            -100 => Self::Previously,
            -10 => Self::Retrying,
            1 => Self::NeverTried,
            2 => Self::Waiting,
            3 => Self::Attempting,
            100 => Self::Current,
            other => Self::Unknown(other),
        }
    }
}

/// The relay network's readiness (`SteamRelayNetworkStatus_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayStatus {
    /// Overall: `Current` when a relayed connection can be made.
    pub availability: Availability,
    /// Whether the network configuration has been fetched.
    pub network_config: Availability,
    /// Whether any relay has been reached.
    pub any_relay: Availability,
    /// Steam's own description, for a log line.
    pub debug: String,
}

impl RelayStatus {
    /// Reads the struct, counting a lossy debug message.
    pub(crate) fn from_raw(raw: &SteamRelayNetworkStatus) -> (Self, bool) {
        // Copies, never references: the struct is packed.
        let debug = raw.debug;
        let (debug, lossy) = fixed_string(&debug);
        (
            Self {
                availability: Availability::from_raw(raw.availability),
                network_config: Availability::from_raw(raw.network_config),
                any_relay: Availability::from_raw(raw.any_relay),
                debug,
            },
            lossy,
        )
    }
}

/// A connection arriving on a listen socket, waiting for its listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Incoming {
    pub(crate) connection: u32,
    /// The certified identity of whoever is connecting, if it is a Steam id.
    pub(crate) remote: Option<SteamId>,
}

/// One listener's queue of arrivals, shared with the pump.
pub(crate) type IncomingQueue = RefCell<VecDeque<Incoming>>;

/// Where the pump puts arrivals: one queue per open listen socket, held
/// weakly so a dropped `SteamListener` stops receiving.
#[derive(Debug, Default)]
pub(crate) struct IncomingQueues {
    pub(crate) sockets: Vec<(u32, Weak<IncomingQueue>)>,
}

/// `ISteamNetworkingUtils`, borrowed from a [`Steam`]; from
/// [`Steam::networking`].
#[derive(Debug, Clone, Copy)]
pub struct Networking<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// Steam's relay network.
    #[must_use]
    pub fn networking(&self) -> Networking<'_> {
        Networking { steam: self }
    }
}

impl Networking<'_> {
    /// Starts reaching Steam's relays (`InitRelayNetworkAccess`), so the first
    /// connection does not wait for it. Connecting and listening start it too;
    /// calling it early, at a lobby screen, is what saves the wait. Once per
    /// session: later calls do nothing.
    pub fn start_relay(&self) {
        if self.steam.relay_started.replace(true) {
            return;
        }
        let client = &self.steam.client;
        // SAFETY: `client.net_utils` is the non-null interface init resolved,
        // and `Networking` borrows the `!Send` `Steam`, so this is the pump
        // thread.
        unsafe { (client.lib.fns.net_utils.init_relay_network_access)(client.net_utils) };
    }

    /// The relay network's readiness now (`GetRelayNetworkStatus`); a
    /// change also arrives as
    /// [`SteamEvent::RelayStatusChanged`](crate::SteamEvent::RelayStatusChanged).
    #[must_use]
    pub fn relay_status(&self) -> RelayStatus {
        let client = &self.steam.client;
        // Zeroed: every field is an integer or bytes.
        let mut raw = SteamRelayNetworkStatus {
            availability: 0,
            ping_measurement_in_progress: 0,
            network_config: 0,
            any_relay: 0,
            debug: [0; 256],
        };
        // SAFETY: as in `start_relay`; `raw` is a writable
        // `SteamRelayNetworkStatus_t`.
        unsafe {
            (client.lib.fns.net_utils.get_relay_network_status)(client.net_utils, &raw mut raw);
        }
        let (status, lossy) = RelayStatus::from_raw(&raw);
        if lossy {
            self.steam
                .lossy_strings
                .set(self.steam.lossy_strings.get() + 1);
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn availability_maps_valves_values_and_keeps_the_rest() {
        let named = [
            (-102, Availability::CannotTry),
            (-101, Availability::Failed),
            (-100, Availability::Previously),
            (-10, Availability::Retrying),
            (1, Availability::NeverTried),
            (2, Availability::Waiting),
            (3, Availability::Attempting),
            (100, Availability::Current),
        ];
        for (raw, expected) in named {
            assert_eq!(Availability::from_raw(raw), expected);
        }
        assert_eq!(Availability::from_raw(0), Availability::Unknown(0));
    }

    #[test]
    fn relay_access_starts_once_and_its_status_is_read() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        steam.networking().start_relay();
        steam.networking().start_relay();
        assert_eq!(testing::script(|s| s.net.relay_inits), 1);
        testing::script(|s| s.net.relay_availability = 100);
        let status = steam.networking().relay_status();
        assert_eq!(status.availability, Availability::Current);
    }
}
