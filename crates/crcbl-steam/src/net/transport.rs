//! One Steam P2P connection as a [`crcbl_net::Transport`].

use std::{collections::VecDeque, sync::Arc};

use crcbl_net::{Message, MessageKind, Transport, TransportError};

use super::{EndReason, identity, state};
use crate::{
    EResult, Steam, SteamId,
    client::Client,
    error::SteamError,
    ffi::structs::{SteamNetConnectionInfo, SteamNetworkingMessage},
    net::VirtualPort,
};

/// The largest payload a message may carry
/// (`k_cbMaxSteamNetworkingSocketsMessageSizeSend`, 512 KiB, in
/// `steamnetworkingtypes.h`); a larger one is
/// [`TransportError::MessageTooLarge`] before any call.
pub const MAX_MESSAGE_BYTES: usize = 512 * 1024;

/// `k_nSteamNetworkingSend_Reliable`: the send flag, and on a received
/// message the one flag that is meaningful.
const SEND_RELIABLE: i32 = 8;
/// `k_nSteamNetworkingSend_UnreliableNoNagle`: unreliable, sent at once —
/// what state snapshots want.
const SEND_UNRELIABLE_NO_NAGLE: i32 = 1;
/// How many messages one `ReceiveMessagesOnConnection` call may hand over.
const RECEIVE_BATCH: usize = 32;

/// What a `Send` surface answers when it is used off the pump thread.
const OFF_THREAD: &str =
    "SteamTransport used off the thread Steam was initialised on; Steam was not called";

/// One Steam P2P connection, implementing [`Transport`].
///
/// Made by [`connect`](Self::connect) on a joiner and by
/// [`SteamListener::accept`](super::SteamListener::accept) on a host.
///
/// **`Send`, because `Transport` requires it — but Steam is only called on
/// the pump thread**, the one [`Steam::init`](crate::Steam::init) ran on.
/// Off it, a send or receive is [`TransportError::Channel`] and
/// [`is_connected`](Transport::is_connected) is `false`, with no Steam call
/// made; a drop skips closing the connection (it closes at shutdown) and logs.
///
/// Received messages are copied out and released at once, reliable ones
/// queued ahead of unreliable ones, so no Steam-owned message outlives a
/// call. Dropping the transport closes the connection with
/// [`EndReason::ShuttingDown`], lingering so queued reliable messages still
/// go out; [`close`](Self::close) names another reason.
#[derive(Debug)]
pub struct SteamTransport {
    client: Arc<Client>,
    /// `0` once this end has closed it.
    connection: u32,
    remote: SteamId,
    reliable: VecDeque<Vec<u8>>,
    unreliable: VecDeque<Vec<u8>>,
    /// Why it ended, once it has.
    end: Option<EndReason>,
}

impl SteamTransport {
    /// Opens a P2P connection to `host` on `port` (`ConnectP2P`), starting
    /// relay access if nothing has. The connection comes up over the next
    /// frames; a send Steam will not take before then is
    /// [`TransportError::Backpressure`], to try again on a later frame.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam does not start the connection.
    pub fn connect(steam: &Steam, host: SteamId, port: VirtualPort) -> Result<Self, SteamError> {
        steam.networking().start_relay();
        let client = &steam.client;
        let remote = identity::of(host);
        // SAFETY: `client.net` is the non-null interface init resolved; the
        // identity is a live `SteamNetworkingIdentity` the call reads (the C++
        // reference is a pointer at the ABI); no options are passed. `Steam` is
        // `!Send`, so this is the pump thread.
        let connection = unsafe {
            (client.lib.fns.net.connect_p2p)(
                client.net,
                &raw const remote,
                port.0,
                0,
                core::ptr::null(),
            )
        };
        if connection == 0 {
            return Err(SteamError::Refused("ConnectP2P"));
        }
        Ok(Self::over(Arc::clone(client), connection, host))
    }

    /// A transport over a connection Steam has already opened.
    pub(crate) fn over(client: Arc<Client>, connection: u32, remote: SteamId) -> Self {
        Self {
            client,
            connection,
            remote,
            reliable: VecDeque::new(),
            unreliable: VecDeque::new(),
            end: None,
        }
    }

    /// Who is on the other end — certified by Steam's relay, so a host can
    /// key a participant on it without an auth ticket.
    #[must_use]
    pub const fn remote(&self) -> SteamId {
        self.remote
    }

    /// Why the connection ended, once a receive has seen it end.
    #[must_use]
    pub const fn end_reason(&self) -> Option<EndReason> {
        self.end
    }

    /// Closes the connection with `reason` — [`EndReason::HostLeft`] from a
    /// host that is quitting, say — lingering so queued reliable messages
    /// still go out.
    pub fn close(mut self, reason: EndReason) {
        self.close_now(reason);
    }

    /// `CloseConnection`, once, on the pump thread only.
    fn close_now(&mut self, reason: EndReason) {
        let connection = core::mem::take(&mut self.connection);
        if connection == 0 {
            return;
        }
        if !self.client.on_pump_thread() {
            log::warn!(
                "steam: a SteamTransport to {:?} was closed off the pump thread; the connection \
                 stays open until Steam shuts down",
                self.remote
            );
            return;
        }
        let client = &self.client;
        // SAFETY: `client.net` is the non-null interface; this is the pump
        // thread; the debug text is a static NUL-terminated string.
        unsafe {
            (client.lib.fns.net.close_connection)(
                client.net,
                connection,
                reason.code(),
                reason.debug_text().as_ptr(),
                true,
            );
        }
    }

    /// `Channel` off the pump thread.
    fn on_pump_thread(&self) -> Result<(), TransportError> {
        if self.client.on_pump_thread() {
            Ok(())
        } else {
            Err(TransportError::Channel(OFF_THREAD.to_owned()))
        }
    }

    /// `GetConnectionInfo`, or `None` for a handle Steam no longer knows.
    fn info(&self) -> Option<SteamNetConnectionInfo> {
        let client = &self.client;
        // SAFETY: an all-zero `SteamNetConnectionInfo` is a valid value: every
        // field is an integer or bytes (it is `callbacks::Pod`).
        let mut info: SteamNetConnectionInfo = unsafe { core::mem::zeroed() };
        // SAFETY: `client.net` is the non-null interface; callers checked the
        // pump thread; `info` is a writable `SteamNetConnectionInfo_t`.
        let known = unsafe {
            (client.lib.fns.net.get_connection_info)(client.net, self.connection, &raw mut info)
        };
        known.then_some(info)
    }

    /// Whether the connection has ended, recording why the first time.
    fn ended(&mut self) -> bool {
        if self.end.is_some() {
            return true;
        }
        if self.connection == 0 {
            self.end = Some(EndReason::ShuttingDown);
            return true;
        }
        let (state, code) = self
            .info()
            .map_or((state::NONE, 0), |info| (info.state, info.end_reason));
        if matches!(
            state,
            state::NONE | state::CLOSED_BY_PEER | state::PROBLEM_DETECTED_LOCALLY
        ) {
            self.end = Some(EndReason::from_code(code));
            return true;
        }
        false
    }

    /// Moves everything Steam has for this connection into the two queues,
    /// releasing each message as soon as its bytes are copied.
    fn fill(&mut self) {
        let client = Arc::clone(&self.client);
        let mut batch = [core::ptr::null_mut::<SteamNetworkingMessage>(); RECEIVE_BATCH];
        loop {
            // SAFETY: `client.net` is the non-null interface; callers checked
            // the pump thread; `batch` has room for `RECEIVE_BATCH` pointers.
            let count = unsafe {
                (client.lib.fns.net.receive_messages_on_connection)(
                    client.net,
                    self.connection,
                    batch.as_mut_ptr(),
                    i32::try_from(RECEIVE_BATCH).unwrap_or(1),
                )
            };
            let Ok(count) = usize::try_from(count) else {
                // Negative: an invalid handle. `ended` finds out why.
                return;
            };
            for &message in batch.iter().take(count.min(RECEIVE_BATCH)) {
                if message.is_null() {
                    continue;
                }
                // SAFETY: Steam handed over `message`, live until released
                // below; it is read by copy.
                let header = unsafe { message.read() };
                let bytes = match usize::try_from(header.size) {
                    Ok(len) if len > 0 && !header.data.is_null() => {
                        // SAFETY: `m_pData` is `m_cbSize` readable bytes, owned
                        // by the message, which is not yet released.
                        unsafe { core::slice::from_raw_parts(header.data.cast::<u8>(), len) }
                            .to_vec()
                    }
                    _ => Vec::new(),
                };
                // SAFETY: each message is released exactly once, here, and
                // nothing of it is used afterwards.
                unsafe { (client.lib.fns.net.release_message)(message) };
                if header.flags & SEND_RELIABLE != 0 {
                    self.reliable.push_back(bytes);
                } else {
                    self.unreliable.push_back(bytes);
                }
            }
            if count < RECEIVE_BATCH {
                return;
            }
        }
    }

    /// `SendMessageToConnection` with `flags`.
    fn send(&mut self, payload: &[u8], flags: i32) -> Result<(), TransportError> {
        self.on_pump_thread()?;
        if payload.len() > MAX_MESSAGE_BYTES {
            return Err(TransportError::MessageTooLarge {
                size: payload.len(),
                limit: MAX_MESSAGE_BYTES,
            });
        }
        if self.end.is_some() || self.connection == 0 {
            return Err(TransportError::Disconnected);
        }
        let len = u32::try_from(payload.len()).map_err(|_| TransportError::MessageTooLarge {
            size: payload.len(),
            limit: MAX_MESSAGE_BYTES,
        })?;
        let client = &self.client;
        // SAFETY: `client.net` is the non-null interface; this is the pump
        // thread; `payload` is `len` readable bytes Steam copies before
        // returning; the message number is not wanted.
        let result = EResult(unsafe {
            (client.lib.fns.net.send_message_to_connection)(
                client.net,
                self.connection,
                payload.as_ptr().cast(),
                len,
                flags,
                core::ptr::null_mut(),
            )
        });
        match result {
            EResult::OK => Ok(()),
            EResult::LIMIT_EXCEEDED | EResult::IGNORED => Err(TransportError::Backpressure),
            // Not a state Steam takes messages in: still coming up, or gone.
            EResult::INVALID_STATE if !self.ended() => Err(TransportError::Backpressure),
            EResult::NO_CONNECTION | EResult::INVALID_STATE => {
                self.ended();
                Err(TransportError::Disconnected)
            }
            other => Err(TransportError::Channel(format!(
                "SendMessageToConnection: {other}"
            ))),
        }
    }
}

impl Transport for SteamTransport {
    fn send_reliable(&mut self, message: Message) -> Result<(), TransportError> {
        self.send(&message.payload, SEND_RELIABLE)
    }

    fn send_unreliable(&mut self, message: Message) -> Result<(), TransportError> {
        self.send(&message.payload, SEND_UNRELIABLE_NO_NAGLE)
    }

    fn recv_reliable(&mut self) -> Result<Option<Message>, TransportError> {
        self.on_pump_thread()?;
        if self.reliable.is_empty() && self.end.is_none() {
            self.fill();
        }
        if let Some(payload) = self.reliable.pop_front() {
            return Ok(Some(Message::reliable(payload)));
        }
        if self.ended() {
            return Err(TransportError::Disconnected);
        }
        Ok(None)
    }

    fn recv(&mut self) -> Result<Option<Message>, TransportError> {
        self.on_pump_thread()?;
        if self.reliable.is_empty() && self.unreliable.is_empty() && self.end.is_none() {
            self.fill();
        }
        if let Some(payload) = self.reliable.pop_front() {
            return Ok(Some(Message::reliable(payload)));
        }
        if let Some(payload) = self.unreliable.pop_front() {
            return Ok(Some(Message {
                kind: MessageKind::Unreliable,
                payload,
            }));
        }
        if self.ended() {
            return Err(TransportError::Disconnected);
        }
        Ok(None)
    }

    /// Whether the connection is up or coming up — Steam's own state, read
    /// directly, so it is right before any pump has run. `false` off the pump
    /// thread, where Steam cannot be asked.
    fn is_connected(&self) -> bool {
        if self.end.is_some() || self.connection == 0 || !self.client.on_pump_thread() {
            return false;
        }
        self.info().is_some_and(|info| {
            matches!(
                info.state,
                state::CONNECTING | state::FINDING_ROUTE | state::CONNECTED
            )
        })
    }
}

impl Drop for SteamTransport {
    fn drop(&mut self) {
        self.close_now(EndReason::ShuttingDown);
    }
}

#[cfg(test)]
mod tests {
    use crcbl_net::conformance::{self, Link};

    use super::*;
    use crate::{
        AppId,
        client::init_on,
        net::state,
        testing::{self, script},
    };

    /// The host a joiner connects to.
    const HOST: SteamId = SteamId(76_561_197_960_287_931);

    /// Pairs of `SteamTransport` over one fake library: `ConnectP2P` makes
    /// both ends, and the loop delivers on send, so settling is nothing.
    struct Loop {
        steam: Steam,
    }

    impl Loop {
        fn new() -> Self {
            Self {
                steam: init_on(testing::fake_lib(), AppId(480)).unwrap(),
            }
        }
    }

    impl Link for Loop {
        type Transport = SteamTransport;

        fn pair(&mut self) -> (SteamTransport, SteamTransport) {
            let near = SteamTransport::connect(&self.steam, HOST, VirtualPort(0)).unwrap();
            let far = SteamTransport::over(
                Arc::clone(&self.steam.client),
                testing::peer_of(near.connection),
                SteamId(testing::STEAM_ID),
            );
            (near, far)
        }

        fn settle(&mut self) {}

        fn max_message_bytes(&self) -> usize {
            MAX_MESSAGE_BYTES
        }
    }

    #[test]
    fn conformance_reliable_is_received_before_unreliable() {
        conformance::reliable_is_received_before_unreliable(&mut Loop::new());
    }

    #[test]
    fn conformance_recv_reliable_never_returns_unreliable() {
        conformance::recv_reliable_never_returns_unreliable(&mut Loop::new());
    }

    #[test]
    fn conformance_the_send_method_sets_the_kind() {
        conformance::the_send_method_sets_the_kind(&mut Loop::new());
    }

    #[test]
    fn conformance_reliable_messages_arrive_in_order() {
        conformance::reliable_messages_arrive_in_order(&mut Loop::new());
    }

    #[test]
    fn conformance_an_oversized_message_names_its_size_and_the_limit() {
        conformance::an_oversized_message_names_its_size_and_the_limit(&mut Loop::new());
    }

    #[test]
    fn conformance_a_dropped_peer_is_disconnected() {
        conformance::a_dropped_peer_is_disconnected(&mut Loop::new());
    }

    #[test]
    fn a_transport_is_send() {
        fn send<T: Send>() {}
        send::<SteamTransport>();
    }

    #[test]
    fn connecting_names_the_host_and_starts_the_relay() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        let link = SteamTransport::connect(&steam, HOST, VirtualPort(7)).unwrap();
        assert_eq!(link.remote(), HOST);
        assert_eq!(script(|s| s.net.connects.clone()), [(HOST.0, 7)]);
        assert_eq!(script(|s| s.net.relay_inits), 1);
        script(|s| s.net.refuse = true);
        assert_eq!(
            SteamTransport::connect(&steam, HOST, VirtualPort(7)).unwrap_err(),
            SteamError::Refused("ConnectP2P")
        );
    }

    #[test]
    fn every_received_message_is_released_exactly_once() {
        let mut link = Loop::new();
        let (mut near, mut far) = link.pair();
        // More than one receive batch.
        let sent = RECEIVE_BATCH * 2 + 3;
        for i in 0..sent {
            near.send_reliable(Message::reliable(vec![u8::try_from(i % 256).unwrap()]))
                .unwrap();
        }
        let mut received = 0;
        while let Some(message) = far.recv().unwrap() {
            assert_eq!(message.payload, [u8::try_from(received % 256).unwrap()]);
            received += 1;
        }
        assert_eq!(received, sent);
        let (released, outstanding, bad) =
            script(|s| (s.net.released, s.net.outstanding.len(), s.net.bad_releases));
        assert_eq!(
            (usize::try_from(released).unwrap(), outstanding, bad),
            (sent, 0, 0)
        );
    }

    #[test]
    fn send_results_map_to_transport_errors() {
        let mut link = Loop::new();
        let (mut near, _far) = link.pair();
        let send = |near: &mut SteamTransport| near.send_unreliable(Message::unreliable(vec![1]));
        script(|s| s.net.send_result = Some(25));
        assert!(matches!(send(&mut near), Err(TransportError::Backpressure)));
        script(|s| s.net.send_result = Some(8));
        assert!(
            matches!(send(&mut near), Err(TransportError::Channel(message)) if message.contains("InvalidParam"))
        );
        // Invalid state on a connection still coming up: try again later.
        script(|s| {
            s.net.send_result = Some(11);
            s.net.connections.get_mut(&near.connection).unwrap().state = state::CONNECTING;
        });
        assert!(matches!(send(&mut near), Err(TransportError::Backpressure)));
        script(|s| s.net.send_result = Some(3));
        assert!(matches!(send(&mut near), Err(TransportError::Disconnected)));
    }

    #[test]
    fn is_connected_follows_steams_state() {
        let mut link = Loop::new();
        let (near, far) = link.pair();
        assert!(far.is_connected());
        for (state, connected) in [
            (state::CONNECTING, true),
            (state::FINDING_ROUTE, true),
            (state::CONNECTED, true),
            (state::CLOSED_BY_PEER, false),
            (state::PROBLEM_DETECTED_LOCALLY, false),
        ] {
            script(|s| s.net.connections.get_mut(&far.connection).unwrap().state = state);
            assert_eq!(far.is_connected(), connected, "state {state}");
        }
        drop(near);
    }

    #[test]
    fn the_end_reason_says_whether_the_host_left_or_the_link_was_lost() {
        let mut link = Loop::new();
        let (near, mut far) = link.pair();
        assert_eq!(far.end_reason(), None);
        let closed = near.connection;
        near.close(EndReason::HostLeft);
        assert!(matches!(far.recv(), Err(TransportError::Disconnected)));
        assert_eq!(far.end_reason(), Some(EndReason::HostLeft));
        assert_eq!(
            script(|s| s.net.closed.last().copied()),
            Some((closed, 1000, true)),
            "closed with the app-range code, lingering"
        );

        let (_near, mut far) = link.pair();
        script(|s| {
            let c = s.net.connections.get_mut(&far.connection).unwrap();
            c.state = state::PROBLEM_DETECTED_LOCALLY;
            c.end_reason = 5001;
        });
        assert!(matches!(
            far.recv_reliable(),
            Err(TransportError::Disconnected)
        ));
        assert_eq!(far.end_reason(), Some(EndReason::Lost(5001)));
    }

    #[test]
    fn messages_queued_before_the_peer_left_are_still_received() {
        let mut link = Loop::new();
        let (mut near, mut far) = link.pair();
        near.send_reliable(Message::reliable(b"goodbye".to_vec()))
            .unwrap();
        near.close(EndReason::HostLeft);
        assert_eq!(far.recv().unwrap().unwrap().payload, b"goodbye");
        assert!(matches!(far.recv(), Err(TransportError::Disconnected)));
    }

    #[test]
    fn off_the_pump_thread_nothing_calls_steam() {
        let mut link = Loop::new();
        let (mut near, far) = link.pair();
        let before = script(|s| s.net.calls);
        let calls_there = std::thread::spawn(move || {
            let mut far = far;
            assert!(matches!(
                far.send_reliable(Message::reliable(vec![1])),
                Err(TransportError::Channel(_))
            ));
            assert!(matches!(far.recv(), Err(TransportError::Channel(_))));
            assert!(matches!(
                far.recv_reliable(),
                Err(TransportError::Channel(_))
            ));
            assert!(!far.is_connected());
            // Dropped here, off the pump thread: no close either.
            drop(far);
            script(|s| s.net.calls)
        })
        .join()
        .unwrap();
        assert_eq!(calls_there, 0, "a Steam call was made off the pump thread");
        assert_eq!(
            script(|s| s.net.calls),
            before,
            "nor on this thread's behalf"
        );
        assert!(script(|s| s.net.closed.is_empty()));
        // The pump thread's own end still works.
        near.send_reliable(Message::reliable(vec![2])).unwrap();
    }

    #[test]
    fn the_last_owner_dropped_off_the_pump_thread_does_not_shut_down() {
        let mut link = Loop::new();
        let (near, far) = link.pair();
        drop(near);
        drop(link);
        let shutdowns_there = std::thread::spawn(move || {
            drop(far);
            script(|s| s.calls.shutdown)
        })
        .join()
        .unwrap();
        assert_eq!(shutdowns_there, 0);
        assert_eq!(script(|s| s.calls.shutdown), 0);
    }
}
