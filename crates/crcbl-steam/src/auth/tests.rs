//! Tickets, sessions and the gate over the fake.

use std::time::{Duration, Instant};

use super::*;
use crate::{
    CallState, SteamEvent,
    client::init_on,
    ffi::structs::CSteamId,
    testing::{self, FakeMsg, FakeTickets, completion},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn fake<R>(f: impl FnOnce(&mut FakeTickets) -> R) -> R {
    testing::script(|s| f(&mut s.tickets))
}

const PEER: SteamId = SteamId(76_561_197_960_287_931);

#[test]
fn a_session_ticket_carries_its_bytes_and_verifier_and_cancels_once() {
    let mut steam = steam();
    fake(|f| {
        f.next = 11;
        f.bytes = vec![7; 240];
    });
    let ticket = steam.auth().session_ticket(Some(PEER)).unwrap();
    assert_eq!(ticket.id(), AuthTicketId(11));
    assert_eq!(ticket.bytes(), &[7; 240][..]);
    let open = steam.auth().session_ticket(None).unwrap();
    assert_eq!(fake(|f| f.verifiers.clone()), [Some(PEER.0), None]);
    assert!(fake(|f| f.cancelled.is_empty()));
    drop(ticket);
    drop(open);
    assert_eq!(fake(|f| f.cancelled.clone()), [11, 11], "each once");
}

/// **The ticket buffer is Steam's documented size**, and a count past it is
/// refused — the ticket cancelled, nothing read past the buffer.
#[test]
fn a_ticket_claiming_more_than_the_buffer_is_refused_and_cancelled() {
    let mut steam = steam();
    fake(|f| {
        f.next = 12;
        f.bytes = vec![1; 16];
        f.claimed = Some(u32::try_from(SESSION_TICKET_BYTES + 1).unwrap());
    });
    assert!(matches!(
        steam.auth().session_ticket(None),
        Err(SteamError::Truncated("GetAuthSessionTicket"))
    ));
    assert_eq!(fake(|f| f.cancelled.clone()), [12]);

    fake(|f| {
        f.claimed = None;
        f.bytes = vec![1; SESSION_TICKET_BYTES];
    });
    let full = steam.auth().session_ticket(None).unwrap();
    assert_eq!(
        full.bytes().len(),
        SESSION_TICKET_BYTES,
        "the whole buffer is fine"
    );
}

#[test]
fn a_ticket_steam_does_not_issue_is_refused_and_nothing_is_cancelled() {
    let mut steam = steam();
    assert!(matches!(
        steam.auth().session_ticket(None),
        Err(SteamError::Refused("GetAuthSessionTicket"))
    ));
    assert!(matches!(
        steam.auth().web_api_ticket("my-service"),
        Err(SteamError::Refused("GetAuthTicketForWebApi"))
    ));
    assert!(fake(|f| f.cancelled.is_empty()));
}

#[test]
fn a_web_api_ticket_cancels_once_and_its_bytes_arrive_as_an_event() {
    let mut steam = steam();
    fake(|f| f.next = 13);
    let ticket = steam.auth().web_api_ticket("my-service").unwrap();
    assert_eq!(fake(|f| f.web_identities.clone()), ["my-service"]);
    let mut payload = vec![0_u8; 2572];
    payload[..4].copy_from_slice(&13_u32.to_le_bytes());
    payload[4..8].copy_from_slice(&1_i32.to_le_bytes());
    payload[8..12].copy_from_slice(&3_i32.to_le_bytes());
    payload[12..15].copy_from_slice(&[9, 8, 7]);
    testing::script(|s| s.queue.push_back(FakeMsg::payload(168, payload)));
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [SteamEvent::WebApiTicket {
            ticket: AuthTicketId(13),
            result: EResult::OK,
            bytes: vec![9, 8, 7],
        }]
    );
    drop(ticket);
    assert_eq!(fake(|f| f.cancelled.clone()), [13]);
}

/// A web-API ticket claiming more bytes than its array holds is a decode
/// mismatch, not a read past the array.
#[test]
fn a_web_api_ticket_count_past_its_array_is_a_mismatch() {
    let mut steam = steam();
    let mut payload = vec![0_u8; 2572];
    payload[8..12].copy_from_slice(&2561_i32.to_le_bytes());
    testing::script(|s| s.queue.push_back(FakeMsg::payload(168, payload)));
    steam.pump();
    assert_eq!(steam.events().count(), 0);
    assert_eq!(steam.diagnostics().decode_mismatches, 1);
}

#[test]
fn begin_session_maps_each_refusal_and_ends_a_begun_session_once() {
    let mut steam = steam();
    let session = steam.auth().begin_session(&[1, 2, 3], PEER).unwrap();
    assert_eq!(session.user(), PEER);
    assert_eq!(fake(|f| f.begun.clone()), [(vec![1, 2, 3], PEER.0)]);
    drop(session);
    assert_eq!(fake(|f| f.ended.clone()), [PEER.0]);

    for (raw, error) in [
        (1, BeginAuthError::InvalidTicket),
        (2, BeginAuthError::DuplicateRequest),
        (3, BeginAuthError::InvalidVersion),
        (4, BeginAuthError::GameMismatch),
        (5, BeginAuthError::ExpiredTicket),
        (9, BeginAuthError::Other(9)),
    ] {
        fake(|f| f.begin_result = raw);
        assert_eq!(
            steam.auth().begin_session(&[1], PEER).unwrap_err(),
            error,
            "{raw}"
        );
    }
    assert_eq!(fake(|f| f.ended.len()), 1, "a refused session is not ended");
}

#[test]
fn licences_and_verdicts_map() {
    let mut steam = steam();
    for (raw, license) in [
        (0, License::Has),
        (1, License::DoesNotHave),
        (2, License::NoAuth),
        (7, License::Other(7)),
    ] {
        fake(|f| f.license = raw);
        assert_eq!(steam.auth().user_has_license(PEER, AppId(481)), license);
    }
    assert_eq!(fake(|f| f.license_asked[0]), (PEER.0, 481));
    for (raw, response) in [
        (0, AuthResponse::Ok),
        (3, AuthResponse::VacBanned),
        (6, AuthResponse::TicketCanceled),
        (10, AuthResponse::NetworkIdentityFailure),
        (11, AuthResponse::Other(11)),
    ] {
        assert_eq!(AuthResponse::from_raw(raw), response, "{raw}");
    }
}

/// The two ticket callbacks a validating host and an issuing peer hear.
#[test]
fn the_ticket_callbacks_decode_to_events() {
    let mut steam = steam();
    let id = |raw: u64| -> CSteamId { raw.to_ne_bytes() };
    let mut verdict = id(PEER.0).to_vec();
    verdict.extend_from_slice(&6_i32.to_le_bytes());
    verdict.extend_from_slice(&id(42));
    let mut ready = 11_u32.to_le_bytes().to_vec();
    ready.extend_from_slice(&1_i32.to_le_bytes());
    testing::script(|s| {
        s.queue.push_back(FakeMsg::payload(143, verdict));
        s.queue.push_back(FakeMsg::payload(163, ready));
    });
    steam.pump();
    assert_eq!(
        steam.events().collect::<Vec<_>>(),
        [
            SteamEvent::AuthSessionVerdict {
                user: PEER,
                response: AuthResponse::TicketCanceled,
                owner: SteamId(42),
            },
            SteamEvent::AuthTicketReady {
                ticket: AuthTicketId(11),
                result: EResult::OK,
            },
        ]
    );
}

#[test]
fn an_encrypted_ticket_is_requested_through_the_registry_and_read_at_its_size() {
    let mut steam = steam();
    fake(|f| f.encrypted_call = 88);
    let call = steam.auth().request_encrypted_ticket(b"match 12").unwrap();
    assert_eq!(fake(|f| f.encrypted_data.clone()), b"match 12");
    testing::script(|s| {
        s.results.push((88, 1_i32.to_le_bytes().to_vec(), false));
        s.queue.push_back(completion(88, 154, 4));
    });
    steam.pump();
    let CallState::Ready(ready) = steam.take(call) else {
        panic!("answered");
    };
    assert_eq!(ready.result, EResult::OK);

    fake(|f| f.encrypted = Some(vec![5; 1500]));
    assert_eq!(steam.auth().encrypted_ticket(), Ok(vec![5; 1500]));
    assert_eq!(fake(|f| f.encrypted_offered.clone()), [1024, 1500]);

    fake(|f| f.encrypted = Some(vec![5; 9000]));
    assert_eq!(
        steam.auth().encrypted_ticket(),
        Err(SteamError::Refused("GetEncryptedAppTicket")),
        "past what this crate will read"
    );
    fake(|f| f.encrypted = None);
    assert_eq!(
        steam.auth().encrypted_ticket(),
        Err(SteamError::Refused("GetEncryptedAppTicket"))
    );
}

fn verdict(user: SteamId, response: AuthResponse) -> SteamEvent {
    SteamEvent::AuthSessionVerdict {
        user,
        response,
        owner: user,
    }
}

/// **Validated**: provisional from the begin, admitted on Steam's `OK`.
#[test]
fn the_gate_admits_a_user_steam_validates() {
    let start = Instant::now();
    let mut gate = AuthGate::new(Duration::from_secs(10));
    gate.begin(PEER, start);
    assert!(gate.is_provisional(PEER));
    assert_eq!(
        gate.observe(&verdict(PEER, AuthResponse::Ok)),
        Some(Verdict::Admitted(PEER))
    );
    assert!(gate.is_admitted(PEER));
    assert_eq!(
        gate.expire(start + Duration::from_secs(60)),
        [],
        "admitted users do not time out"
    );
    assert_eq!(
        gate.observe(&verdict(PEER, AuthResponse::Ok)),
        None,
        "a repeat is nothing new"
    );
}

/// **Rejected late**: a refusal after admission — a cancelled ticket, a ban
/// — still rejects, as does one before it.
#[test]
fn the_gate_rejects_on_a_refusal_before_or_after_admission() {
    let start = Instant::now();
    let other = SteamId(7);
    let mut gate = AuthGate::new(Duration::from_secs(10));
    gate.begin(PEER, start);
    gate.begin(other, start);
    gate.observe(&verdict(PEER, AuthResponse::Ok));
    assert_eq!(
        gate.observe(&verdict(PEER, AuthResponse::TicketCanceled)),
        Some(Verdict::Rejected(PEER, AuthResponse::TicketCanceled))
    );
    assert!(!gate.is_admitted(PEER));
    assert_eq!(
        gate.observe(&verdict(other, AuthResponse::VacBanned)),
        Some(Verdict::Rejected(other, AuthResponse::VacBanned))
    );
    assert_eq!(
        gate.observe(&verdict(PEER, AuthResponse::Ok)),
        None,
        "a user no longer tracked"
    );
}

/// **Never answered**: a provisional user times out exactly at the deadline,
/// not before, and not twice.
#[test]
fn the_gate_times_out_a_user_steam_never_answers() {
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    let mut gate = AuthGate::new(timeout);
    gate.begin(PEER, start);
    assert_eq!(gate.expire(start + timeout - Duration::from_millis(1)), []);
    assert_eq!(gate.expire(start + timeout), [Verdict::TimedOut(PEER)]);
    assert_eq!(gate.expire(start + timeout * 2), []);
    assert!(!gate.is_provisional(PEER));
}

#[test]
fn the_gate_ignores_strangers_and_other_events_and_forgets_on_request() {
    let start = Instant::now();
    let mut gate = AuthGate::new(Duration::from_secs(10));
    assert_eq!(gate.observe(&verdict(PEER, AuthResponse::Ok)), None);
    assert_eq!(gate.observe(&SteamEvent::NewLaunchParameters), None);
    gate.begin(PEER, start);
    gate.forget(PEER);
    assert_eq!(gate.expire(start + Duration::from_secs(60)), []);
    gate.begin(PEER, start);
    gate.begin(PEER, start + Duration::from_secs(5));
    assert_eq!(
        gate.expire(start + Duration::from_secs(10)),
        [],
        "begun again: its deadline moved"
    );
}
