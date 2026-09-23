//! Two Steam clients, two machines, one lobby, one P2P connection — through
//! the public API only. Never run by CI: `#[ignore]`d, and run by hand as in
//! `tests/smoke.rs` (Steam running and logged in, the redistributable
//! reachable, `steam_appid.txt` containing `480`), once as each role:
//!
//! ```text
//! # machine A, account A
//! CRCBL_STEAM_ROLE=host cargo test -p crcbl-steam --test net_smoke -- --ignored --nocapture
//! # prints the lobby id; then on machine B, account B (a friend of A):
//! CRCBL_STEAM_ROLE=join CRCBL_STEAM_LOBBY=<id> cargo test -p crcbl-steam --test net_smoke -- --ignored --nocapture
//! ```
//!
//! The host creates a friends-only lobby, listens, and on the joiner's
//! connection exchanges one reliable message each way, then closes with
//! `EndReason::HostLeft`. The joiner joins, connects to the lobby's owner,
//! exchanges the same, and asserts it saw `HostLeft` rather than a loss.

#![cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]

use std::time::{Duration, Instant};

use crcbl_net::{Message, Transport, TransportError};
use crcbl_steam::{
    AppId, CallState, EndReason, LobbyId, LobbyKind, Steam, SteamListener, SteamTransport,
    VirtualPort,
};

/// How long either side waits for the other before failing.
const PATIENCE: Duration = Duration::from_secs(120);
/// The pause between frames.
const FRAME: Duration = Duration::from_millis(16);

/// Pumps until `ready` answers something, or `PATIENCE` runs out.
fn until<T>(steam: &mut Steam, what: &str, mut ready: impl FnMut(&mut Steam) -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        steam.pump();
        for event in steam.events() {
            println!("{event:?}");
        }
        if let Some(value) = ready(steam) {
            return value;
        }
        assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
        std::thread::sleep(FRAME);
    }
}

/// Receives one message, pumping between tries.
fn receive(steam: &mut Steam, link: &mut SteamTransport) -> Message {
    until(steam, "a message", |_| match link.recv() {
        Ok(message) => message,
        Err(error) => panic!("receive failed: {error}"),
    })
}

#[test]
#[ignore = "needs two Steam clients on two machines; see the module docs"]
fn a_lobby_host_and_a_joiner_exchange_messages_and_the_joiner_sees_host_left() {
    let role = std::env::var("CRCBL_STEAM_ROLE").expect("CRCBL_STEAM_ROLE=host or join");
    let mut steam = Steam::init(AppId(480)).unwrap_or_else(|err| panic!("Steam::init: {err}"));
    steam.networking().start_relay();
    match role.as_str() {
        "host" => host(&mut steam),
        "join" => join(&mut steam),
        other => panic!("CRCBL_STEAM_ROLE must be host or join, not {other}"),
    }
    let diagnostics = steam.diagnostics();
    println!("{diagnostics:?}");
    assert_eq!(diagnostics.decode_mismatches, 0);
}

fn host(steam: &mut Steam) {
    let mut call = Some(
        steam
            .matchmaking()
            .create_lobby(LobbyKind::FriendsOnly, 4)
            .expect("CreateLobby"),
    );
    let lobby = until(steam, "the lobby", |steam| {
        match steam.take(call.take().expect("taken once per frame")) {
            CallState::Pending(pending) => {
                call = Some(pending);
                None
            }
            CallState::Ready(created) => Some(created.lobby().expect("lobby created")),
            CallState::Failed(error) => panic!("CreateLobby failed: {error}"),
        }
    });
    println!(
        "lobby {} — start the joiner with CRCBL_STEAM_LOBBY={}",
        lobby.id().0,
        lobby.id().0
    );
    let mut listener = SteamListener::open(steam, &lobby, VirtualPort(0)).expect("listen");
    let mut peer = until(steam, "the joiner", |steam| listener.accept(steam));
    println!("joined by {:?}", peer.remote());
    peer.send_reliable(Message::reliable(b"hello from the host".to_vec()))
        .expect("send");
    let reply = receive(steam, &mut peer);
    println!("received {:?}", String::from_utf8_lossy(&reply.payload));
    peer.close(EndReason::HostLeft);
    // Keep pumping while the close, lingering, goes out.
    let linger = Instant::now() + Duration::from_secs(2);
    until(steam, "the close to go out", |_| {
        (Instant::now() >= linger).then_some(())
    });
}

fn join(steam: &mut Steam) {
    let id: u64 = std::env::var("CRCBL_STEAM_LOBBY")
        .expect("CRCBL_STEAM_LOBBY=<the host's lobby id>")
        .parse()
        .expect("a lobby id");
    let mut call = Some(
        steam
            .matchmaking()
            .join_lobby(LobbyId(id))
            .expect("JoinLobby"),
    );
    let lobby = until(steam, "the lobby", |steam| {
        match steam.take(call.take().expect("taken once per frame")) {
            CallState::Pending(pending) => {
                call = Some(pending);
                None
            }
            CallState::Ready(entered) => Some(entered.lobby().expect("lobby entered")),
            CallState::Failed(error) => panic!("JoinLobby failed: {error}"),
        }
    });
    let owner = lobby.owner(steam);
    println!("joined {:?}, owner {owner:?}", lobby.id());
    let mut link = SteamTransport::connect(steam, owner, VirtualPort(0)).expect("connect");
    let greeting = receive(steam, &mut link);
    println!("received {:?}", String::from_utf8_lossy(&greeting.payload));
    link.send_reliable(Message::reliable(b"hello from the joiner".to_vec()))
        .expect("send");
    until(steam, "the host to leave", |_| match link.recv() {
        Err(TransportError::Disconnected) => Some(()),
        _ => None,
    });
    assert_eq!(link.end_reason(), Some(EndReason::HostLeft));
}
