//! Who each peer is: the [`PlayerId`] its hello carries, one session a
//! player, and the denylist that refuses one at the handshake.

use crcbl_client::{Client, Ended};
use crcbl_net::InMemoryTransport;

use super::tests::{
    COMPATIBILITY, TICK, TICK_HZ, next_player, reject_code, reply, say_hello_as, world,
};
use super::*;

/// A host of up to four players, its clock at zero.
fn host() -> Host {
    let mut host = Host::new(
        world(),
        HostConfig {
            max_peers: 4,
            tick_hz: TICK_HZ,
            compatibility: COMPATIBILITY,
        },
    );
    host.update(Duration::ZERO);
    host
}

/// A bare far end the test speaks for, and the host's clock moved on a tick
/// for each `step`.
struct Rig {
    host: Host,
    now: Duration,
}

impl Rig {
    fn new() -> Self {
        Self {
            host: host(),
            now: Duration::ZERO,
        }
    }

    fn step(&mut self) {
        self.now += TICK;
        self.host.update(self.now);
    }

    fn raw(&mut self) -> InMemoryTransport {
        let (near, far) = InMemoryTransport::pair();
        self.host.add(Box::new(far));
        near
    }

    /// A new link saying hello as `player` with `token`, answered.
    fn hello_as(
        &mut self,
        player: PlayerId,
        token: Option<ResumeToken>,
    ) -> (InMemoryTransport, HandshakeResult) {
        let mut link = self.raw();
        say_hello_as(&mut link, 1, token, player);
        self.step();
        let answer = reply(&mut link);
        (link, answer)
    }

    fn events(&mut self) -> Vec<PeerEvent> {
        self.host.events().collect()
    }
}

/// The reject's message, or a panic naming the accept.
fn refusal(result: &HandshakeResult) -> &RejectReason {
    match result {
        HandshakeResult::Reject { reason, .. } => reason,
        HandshakeResult::Accept { .. } => panic!("expected a refusal, got an accept"),
    }
}

/// **The hello carries the player, and the host names each peer by it** —
/// from a real client, so it is the client that sends its own id, and for
/// two players at once, so each peer has its own.
#[test]
fn the_hello_carries_the_player_and_the_host_names_each_peer_by_it() {
    let mut host = host();
    let mut clients = Vec::new();
    let mut now = Duration::ZERO;
    for _ in 0..2 {
        let (near, far) = InMemoryTransport::pair();
        host.add(Box::new(far));
        clients.push(Client::new_with_compatibility(
            World::new(),
            near,
            TICK_HZ,
            COMPATIBILITY,
            next_player(),
        ));
    }
    for _ in 0..4 {
        now += TICK;
        host.update(now);
        for client in &mut clients {
            client.update(now);
        }
    }
    let peers: Vec<PeerId> = host.peers().collect();
    assert_eq!(peers.len(), 2, "both admitted");
    for client in &clients {
        let peer = host
            .peer_of(client.player())
            .expect("the host knows the client's player");
        assert_eq!(host.player(peer), Some(client.player()));
    }
    assert_ne!(clients[0].player(), clients[1].player());
}

/// **A second session of a connected player is refused as a duplicate**, a
/// refusal the client retries rather than gives up on, and leaves the first
/// session where it was; another player is admitted beside it.
#[test]
fn a_second_session_of_a_connected_player_is_refused_as_a_duplicate() {
    let mut rig = Rig::new();
    let player = next_player();
    let (_first, accepted) = rig.hello_as(player, None);
    assert_eq!(reject_code(&accepted), None);
    let first = rig.host.peer_of(player).expect("admitted");

    let (_second, refused) = rig.hello_as(player, None);
    let reason = refusal(&refused);
    assert_eq!(reason.code, RejectReason::DUPLICATE_PLAYER);
    assert!(
        !reason.is_permanent(),
        "a duplicate clears when the link drops"
    );
    assert!(reason.msg.contains(&player.to_string()), "{}", reason.msg);
    assert_eq!(rig.host.peer_count(), 1);
    assert_eq!(rig.host.peer_of(player), Some(first));
    assert_eq!(rig.host.peer_state(first), Some(SessionState::Connected));

    let (_other, other) = rig.hello_as(next_player(), None);
    assert_eq!(reject_code(&other), None, "another player is no duplicate");
    assert_eq!(rig.host.peer_count(), 2);
}

/// **A player whose link dropped, saying hello afresh, takes the lost
/// session's place** — the old session ends, raising its leave, and the new
/// one is the player's — rather than waiting out the grace period.
#[test]
fn a_player_whose_link_dropped_rejoins_in_place_of_the_lost_session() {
    let mut rig = Rig::new();
    let player = next_player();
    let (first_link, _) = rig.hello_as(player, None);
    let lost = rig.host.peer_of(player).expect("admitted");
    drop(first_link);
    rig.step();
    assert_eq!(rig.host.peer_state(lost), Some(SessionState::Reconnecting));
    rig.events();

    let (_again, accepted) = rig.hello_as(player, None);
    assert_eq!(reject_code(&accepted), None, "{accepted:?}");
    let rejoined = rig.host.peer_of(player).expect("the player is in");
    assert_ne!(rejoined, lost);
    assert_eq!(
        rig.host.peer_count(),
        1,
        "the lost session's place was taken"
    );
    assert_eq!(
        rig.events(),
        [PeerEvent::Left(lost), PeerEvent::Joined(rejoined)]
    );
}

/// **A banned player is refused with the ban's reason**, a refusal the
/// client stops on, and **an unban lets the same player in**.
#[test]
fn a_banned_player_is_refused_with_the_reason_and_an_unban_lets_them_in() {
    let mut rig = Rig::new();
    let player = next_player();
    assert_eq!(
        rig.host.ban(player, "griefing the base"),
        None,
        "not in session"
    );

    let (_link, refused) = rig.hello_as(player, None);
    let reason = refusal(&refused);
    assert_eq!(reason.code, RejectReason::BANNED);
    assert!(reason.is_permanent(), "retrying does not lift a ban");
    assert_eq!(reason.msg, "banned from this server: griefing the base");
    assert_eq!(rig.host.peer_count(), 0);

    let (_other, other) = rig.hello_as(next_player(), None);
    assert_eq!(reject_code(&other), None, "the ban is the one player's");

    assert!(rig.host.unban(player));
    let (_back, accepted) = rig.hello_as(player, None);
    assert_eq!(reject_code(&accepted), None, "{accepted:?}");
    assert!(rig.host.peer_of(player).is_some());
}

/// **Banning a player in session kicks them**: the client is told it was
/// removed, and the ban is on the list.
#[test]
fn banning_a_player_in_session_kicks_them() {
    let mut host = host();
    let (near, far) = InMemoryTransport::pair();
    host.add(Box::new(far));
    let player = next_player();
    let mut client =
        Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY, player);
    let mut now = Duration::ZERO;
    for _ in 0..4 {
        now += TICK;
        host.update(now);
        client.update(now);
    }
    let peer = host.peer_of(player).expect("admitted");

    assert_eq!(host.ban(player, "spam"), Some(peer));
    assert_eq!(host.peer_count(), 0);
    client.update(now + TICK);
    assert_eq!(
        client.ended(),
        Some(Ended::ByServer(SessionEndReason::KICKED))
    );
    assert_eq!(host.denylist().reason(player), Some("spam"));
}

/// **A denylist read at start kicks whoever it bans**, and nobody else.
#[test]
fn a_denylist_set_in_session_kicks_whoever_it_bans() {
    let mut rig = Rig::new();
    let (banned, kept) = (next_player(), next_player());
    let _links = [rig.hello_as(banned, None).0, rig.hello_as(kept, None).0];
    let banned_peer = rig.host.peer_of(banned).expect("admitted");
    let mut list = crate::Denylist::new();
    list.ban(banned, "");
    assert_eq!(rig.host.set_denylist(list), [banned_peer]);
    assert_eq!(rig.host.peer_of(banned), None);
    assert!(rig.host.peer_of(kept).is_some());

    let (_again, refused) = rig.hello_as(banned, None);
    assert_eq!(refusal(&refused).msg, "banned from this server");
}

/// **A session is its player's**: a resume naming another player is refused
/// whatever token it holds, as is a hello on the session's own link naming
/// another — and the session's own player resumes it.
#[test]
fn a_session_answers_only_its_own_player() {
    let mut rig = Rig::new();
    let (owner, stranger) = (next_player(), next_player());
    let (mut link, accepted) = rig.hello_as(owner, None);
    let HandshakeResult::Accept { resume_token, .. } = accepted else {
        panic!("the owner is admitted");
    };
    say_hello_as(&mut link, 2, Some(resume_token), stranger);
    rig.step();
    let refused = reply(&mut link);
    assert_eq!(
        reject_code(&refused),
        Some(RejectReason::INVALID_SESSION_TOKEN)
    );
    assert!(refusal(&refused).msg.contains("another player"));

    drop(link);
    rig.step();
    let (_thief, stolen) = rig.hello_as(stranger, Some(resume_token));
    assert_eq!(
        reject_code(&stolen),
        Some(RejectReason::INVALID_SESSION_TOKEN)
    );
    assert!(refusal(&stolen).msg.contains("another player"));

    let (_owner, resumed) = rig.hello_as(owner, Some(resume_token));
    assert_eq!(reject_code(&resumed), None, "{resumed:?}");
    assert!(
        rig.events()
            .iter()
            .any(|event| matches!(event, PeerEvent::Resumed(_)))
    );
}

/// **A banned client stops and shows the reason; a duplicate one retries and
/// gets in once the other session's link drops** — the two refusals' meaning
/// to the client that reads them.
#[test]
fn a_banned_client_stops_on_the_reason_and_a_duplicate_retries() {
    let mut host = host();
    let (banned, twice) = (next_player(), next_player());
    host.ban(banned, "cheating");
    let mut client_of = |player| {
        let (near, far) = InMemoryTransport::pair();
        host.add(Box::new(far));
        Client::new_with_compatibility(World::new(), near, TICK_HZ, COMPATIBILITY, player)
    };
    let mut refused = client_of(banned);
    let mut first = client_of(twice);
    let mut second = client_of(twice);
    let mut now = Duration::ZERO;
    let mut run = |host: &mut Host, clients: &mut [&mut Client<InMemoryTransport>], ticks| {
        for _ in 0..ticks {
            now += TICK;
            host.update(now);
            for client in clients.iter_mut() {
                client.update(now);
            }
        }
    };
    run(&mut host, &mut [&mut refused, &mut first, &mut second], 10);
    let shown = refused.handshake_refusal().expect("the ban stopped it");
    assert_eq!(shown.code, RejectReason::BANNED);
    assert!(shown.msg.ends_with("cheating"), "{}", shown.msg);
    assert!(first.session_id().is_some(), "the first session is in");
    assert!(second.session_id().is_none(), "the duplicate is not");
    assert!(!second.handshake_blocked(), "a duplicate is retried");

    drop(first);
    // The duplicate's retry backs off; give it time to come round.
    run(&mut host, &mut [&mut second], 600);
    assert!(second.session_id().is_some(), "the retry got in");
    assert_eq!(host.peer_count(), 1);
}
