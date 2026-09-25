//! A host's P2P listen socket, admitting only its lobby.

use std::{
    collections::{BTreeSet, VecDeque},
    rc::Rc,
    sync::Arc,
};

use super::{EndReason, Incoming, IncomingQueue, SteamTransport, VirtualPort};
use crate::{
    EResult, Lobby, LobbyId, Steam, SteamId, client::Client, error::SteamError,
    matchmaking::members_of,
};

/// A host's listen socket (`CreateListenSocketP2P`) on one virtual port.
///
/// **It admits only current members of its lobby**, plus anyone
/// [`allow`](Self::allow)ed: a connection from anyone else is closed with
/// [`EndReason::NotAdmitted`] before it is accepted, so a stranger who learns
/// the host's `SteamId` cannot join. Membership is read from Steam when each
/// connection is considered, not remembered.
///
/// `!Send`: it lives with the [`Steam`] that pumps its arrivals. Dropping it
/// stops admitting — later arrivals are closed with
/// [`EndReason::ShuttingDown`] — and connections already accepted stay open.
/// The socket itself closes once the last of them is dropped too, because
/// `CloseListenSocket` closes every connection accepted on it, ungracefully;
/// until then its virtual port stays taken, and a new listener on that port
/// is refused.
#[derive(Debug)]
pub struct SteamListener {
    socket: Arc<ListenSocket>,
    lobby: LobbyId,
    allowed: BTreeSet<SteamId>,
    /// Filled by the pump; the `Weak` side is in `Steam::incoming`.
    arrivals: Rc<IncomingQueue>,
}

impl SteamListener {
    /// Opens a listen socket on `port` for members of `lobby`, starting relay
    /// access if nothing has.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam does not open the socket.
    pub fn open(steam: &mut Steam, lobby: &Lobby, port: VirtualPort) -> Result<Self, SteamError> {
        steam.networking().start_relay();
        let client = Arc::clone(&steam.client);
        // SAFETY: `client.net` is the non-null interface init resolved; no
        // options are passed; `Steam` is `!Send`, so this is the pump thread.
        let socket = unsafe {
            (client.lib.fns.net.create_listen_socket_p2p)(client.net, port.0, 0, core::ptr::null())
        };
        if socket == 0 {
            return Err(SteamError::Refused("CreateListenSocketP2P"));
        }
        let socket = Arc::new(ListenSocket { client, socket });
        let arrivals = Rc::new(IncomingQueue::new(VecDeque::new()));
        steam
            .incoming
            .sockets
            .retain(|(_, queue)| queue.strong_count() > 0);
        steam
            .incoming
            .sockets
            .push((socket.socket, Rc::downgrade(&arrivals)));
        Ok(Self {
            socket,
            lobby: lobby.id(),
            allowed: BTreeSet::new(),
            arrivals,
        })
    }

    /// Also admits `user`, lobby member or not — a host's own second client,
    /// say, or a friend invited outside the lobby.
    pub fn allow(&mut self, user: SteamId) {
        self.allowed.insert(user);
    }

    /// The next admitted connection, accepted, or `None` when none is
    /// waiting. Call after each [`pump`](Steam::pump); connections from
    /// anyone not admitted are closed along the way and never returned.
    pub fn accept(&mut self, steam: &Steam) -> Option<SteamTransport> {
        loop {
            let arrival = self.arrivals.borrow_mut().pop_front()?;
            if let Some(transport) = self.admit(steam, arrival) {
                return Some(transport);
            }
        }
    }

    /// Accepts `arrival` if it is admitted, and closes it otherwise.
    fn admit(&self, steam: &Steam, arrival: Incoming) -> Option<SteamTransport> {
        let client = &steam.client;
        let admitted = arrival.remote.filter(|remote| {
            self.allowed.contains(remote) || members_of(client, self.lobby).contains(remote)
        });
        let Some(remote) = admitted else {
            close(client, arrival.connection, EndReason::NotAdmitted);
            return None;
        };
        // SAFETY: `client.net` is the non-null interface; `Steam` is `!Send`,
        // so this is the pump thread.
        let accepted = EResult(unsafe {
            (client.lib.fns.net.accept_connection)(client.net, arrival.connection)
        });
        if accepted != EResult::OK {
            log::warn!("steam: AcceptConnection for {remote:?} answered {accepted}");
            close(client, arrival.connection, EndReason::Lost(0));
            return None;
        }
        Some(
            SteamTransport::over(Arc::clone(client), arrival.connection, remote)
                .accepted_on(Arc::clone(&self.socket)),
        )
    }
}

/// An open listen socket, shared by its [`SteamListener`] and every
/// connection accepted on it, and closed when the last of them is dropped.
///
/// `Send`, because an accepted [`SteamTransport`] is: a drop off the pump
/// thread skips `CloseListenSocket` and logs, as the transport's own drop
/// does, and the socket then stays open until Steam shuts down.
#[derive(Debug)]
pub(crate) struct ListenSocket {
    client: Arc<Client>,
    socket: u32,
}

impl Drop for ListenSocket {
    fn drop(&mut self) {
        let client = &self.client;
        if !client.on_pump_thread() {
            log::warn!(
                "steam: the last user of listen socket {} was dropped off the pump thread; \
                 the socket stays open until Steam shuts down",
                self.socket
            );
            return;
        }
        // SAFETY: `client.net` is the non-null interface; this is the pump
        // thread; the socket is this value's, closed once.
        unsafe { (client.lib.fns.net.close_listen_socket)(client.net, self.socket) };
    }
}

/// `CloseConnection` for a connection this crate will not accept.
fn close(client: &Client, connection: u32, reason: EndReason) {
    // SAFETY: `client.net` is the non-null interface; callers are on the pump
    // thread; the debug text is a static NUL-terminated string.
    unsafe {
        (client.lib.fns.net.close_connection)(
            client.net,
            connection,
            reason.code(),
            reason.debug_text().as_ptr(),
            false,
        );
    }
}

impl Steam {
    /// Hands a connection arriving on a listen socket to its listener, or,
    /// with no listener open for it, closes it.
    pub(crate) fn route_incoming(&mut self, socket: u32, arrival: Incoming) {
        self.incoming
            .sockets
            .retain(|(_, queue)| queue.strong_count() > 0);
        let queue = self
            .incoming
            .sockets
            .iter()
            .find(|(listening, _)| *listening == socket)
            .and_then(|(_, queue)| queue.upgrade());
        match queue {
            Some(queue) => queue.borrow_mut().push_back(arrival),
            None => close(&self.client, arrival.connection, EndReason::ShuttingDown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppId,
        client::init_on,
        testing::{self, script},
    };

    /// The lobby the host listens for.
    const LOBBY: u64 = 0x0186_0000_0000_0042;
    /// A lobby member.
    const MEMBER: u64 = 76_561_197_960_287_931;
    /// Someone who learned the host's id and is not in the lobby.
    const STRANGER: u64 = 76_561_197_960_287_999;

    /// A host holding `LOBBY`, listening on port 0, with `MEMBER` in the lobby.
    fn host() -> (Steam, Lobby, SteamListener) {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let lobby = testing::joined_lobby(&mut steam, LOBBY);
        script(|s| s.members = vec![testing::STEAM_ID, MEMBER]);
        let listener = SteamListener::open(&mut steam, &lobby, VirtualPort(0)).unwrap();
        (steam, lobby, listener)
    }

    /// Announces a connection from `from` and pumps it to the listener.
    fn arrive(steam: &mut Steam, from: u64) -> u32 {
        let socket = script(|s| s.net.listen_sockets[0].0);
        let (connection, status) = testing::arriving(socket, from);
        script(|s| s.queue.push_back(status));
        steam.pump();
        connection
    }

    #[test]
    fn a_lobby_member_is_accepted_and_named_by_their_certified_id() {
        let (mut steam, _lobby, mut listener) = host();
        let connection = arrive(&mut steam, MEMBER);
        let peer = listener.accept(&steam).expect("a member is admitted");
        assert_eq!(peer.remote(), SteamId(MEMBER));
        assert_eq!(script(|s| s.net.accepted.clone()), [connection]);
        assert!(listener.accept(&steam).is_none(), "one arrival, one peer");
    }

    #[test]
    fn a_stranger_is_closed_as_not_admitted_and_never_accepted() {
        let (mut steam, _lobby, mut listener) = host();
        let stranger = arrive(&mut steam, STRANGER);
        let member = arrive(&mut steam, MEMBER);
        let peer = listener
            .accept(&steam)
            .expect("the member behind the stranger");
        assert_eq!(peer.remote(), SteamId(MEMBER));
        assert_eq!(script(|s| s.net.accepted.clone()), [member]);
        assert_eq!(
            script(|s| s.net.closed.clone()),
            [(stranger, EndReason::NotAdmitted.code(), false)]
        );
    }

    /// **Neither P2P call passes Steam a connection option.** Steam's
    /// networking seals every packet unless a caller sets its `Unencrypted`
    /// option, and the every-packet-sealed rule rests on that (the decision
    /// in `docs/backlog.md`), so an option added to either call has to come
    /// past this first.
    #[test]
    fn no_p2p_call_passes_steam_a_connection_option() {
        let (steam, _lobby, _listener) = host();
        crate::net::SteamTransport::connect(&steam, SteamId(MEMBER), VirtualPort(7)).unwrap();
        let options = script(|s| s.net.options.clone());
        assert_eq!(
            options,
            [
                ("CreateListenSocketP2P", 0, false),
                ("ConnectP2P", 0, false)
            ]
        );
    }

    #[test]
    fn an_allowed_user_is_admitted_outside_the_lobby() {
        let (mut steam, _lobby, mut listener) = host();
        listener.allow(SteamId(STRANGER));
        arrive(&mut steam, STRANGER);
        assert_eq!(
            listener.accept(&steam).map(|peer| peer.remote()),
            Some(SteamId(STRANGER))
        );
    }

    #[test]
    fn a_refused_accept_closes_the_connection_and_yields_nothing() {
        let (mut steam, _lobby, mut listener) = host();
        script(|s| s.net.accept_result = Some(2));
        let connection = arrive(&mut steam, MEMBER);
        assert!(listener.accept(&steam).is_none());
        assert_eq!(script(|s| s.net.closed.clone()), [(connection, 0, false)]);
    }

    #[test]
    fn with_no_listener_an_arrival_is_closed() {
        let (mut steam, _lobby, listener) = host();
        let socket = script(|s| s.net.listen_sockets[0].0);
        drop(listener);
        assert_eq!(script(|s| s.net.closed_listen_sockets.clone()), [socket]);
        let connection = arrive(&mut steam, MEMBER);
        assert_eq!(
            script(|s| s.net.closed.clone()),
            [(connection, EndReason::ShuttingDown.code(), false)]
        );
    }

    /// `CloseListenSocket` closes every connection accepted on the socket,
    /// ungracefully (`isteamnetworkingsockets.h`), so the socket stays open
    /// while any of them does.
    #[test]
    fn accepted_connections_outlive_their_listener() {
        let (mut steam, _lobby, mut listener) = host();
        let socket = script(|s| s.net.listen_sockets[0].0);
        arrive(&mut steam, MEMBER);
        let peer = listener.accept(&steam).expect("a member is admitted");
        drop(listener);
        assert!(
            script(|s| s.net.closed_listen_sockets.is_empty()),
            "closing the socket would drop the accepted peer"
        );
        // It no longer admits anyone.
        let late = arrive(&mut steam, MEMBER);
        assert_eq!(
            script(|s| s.net.closed.clone()),
            [(late, EndReason::ShuttingDown.code(), false)]
        );
        drop(peer);
        assert_eq!(script(|s| s.net.closed_listen_sockets.clone()), [socket]);
    }

    #[test]
    fn a_refused_listen_socket_is_an_error() {
        let mut steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let lobby = testing::joined_lobby(&mut steam, LOBBY);
        script(|s| s.net.refuse = true);
        assert_eq!(
            SteamListener::open(&mut steam, &lobby, VirtualPort(0)).unwrap_err(),
            SteamError::Refused("CreateListenSocketP2P")
        );
    }
}
