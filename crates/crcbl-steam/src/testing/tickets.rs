//! The fake half of `ISteamUser` slice 12 binds: tickets issued from a
//! script, and every cancel, begin and end counted.

use std::ffi::{CStr, c_char, c_void};

use super::script;
use crate::ffi::{ISteamUser, SteamApiCall, structs::SteamNetworkingIdentity};

/// What the fake answers for tickets, and what it has seen.
#[derive(Debug, Default)]
pub(crate) struct FakeTickets {
    /// The handle the next ticket gets; `0` issues none.
    pub(crate) next: u32,
    /// The session ticket's bytes.
    pub(crate) bytes: Vec<u8>,
    /// What `GetAuthSessionTicket` reports writing instead of the bytes'
    /// length, when set.
    pub(crate) claimed: Option<u32>,
    /// The Steam id each session ticket was issued for, if any.
    pub(crate) verifiers: Vec<Option<u64>>,
    /// Every web-API identity asked for.
    pub(crate) web_identities: Vec<String>,
    /// Every ticket cancelled, in order.
    pub(crate) cancelled: Vec<u32>,
    /// What `BeginAuthSession` answers, and every `(ticket, user)` begun.
    pub(crate) begin_result: i32,
    pub(crate) begun: Vec<(Vec<u8>, u64)>,
    /// Every user whose session ended, in order.
    pub(crate) ended: Vec<u64>,
    /// What `UserHasLicenseForApp` answers, and every `(user, app)` asked.
    pub(crate) license: i32,
    pub(crate) license_asked: Vec<(u64, u32)>,
    /// What `RequestEncryptedAppTicket` answers, and the data it carried.
    pub(crate) encrypted_call: SteamApiCall,
    pub(crate) encrypted_data: Vec<u8>,
    /// The encrypted ticket's bytes; `GetEncryptedAppTicket` answers false,
    /// naming this size, when a buffer is too small.
    pub(crate) encrypted: Option<Vec<u8>>,
    /// Every buffer size `GetEncryptedAppTicket` was offered.
    pub(crate) encrypted_offered: Vec<i32>,
}

pub(super) unsafe extern "C" fn fake_get_auth_session_ticket(
    _: *mut ISteamUser,
    out: *mut c_void,
    capacity: i32,
    written: *mut u32,
    verifier: *const SteamNetworkingIdentity,
) -> u32 {
    let verifier = if verifier.is_null() {
        None
    } else {
        // SAFETY: a non-null identity points at one alive for the call.
        crate::net::identity::steam_id(unsafe { &*verifier }).map(|id| id.0)
    };
    script(|s| {
        let tickets = &mut s.tickets;
        tickets.verifiers.push(verifier);
        let n = tickets.bytes.len().min(usize::try_from(capacity).unwrap());
        // SAFETY: the caller passes `capacity` writable bytes and a writable
        // count.
        unsafe {
            core::ptr::copy_nonoverlapping(tickets.bytes.as_ptr(), out.cast::<u8>(), n);
            written.write(tickets.claimed.unwrap_or_else(|| u32::try_from(n).unwrap()));
        }
        tickets.next
    })
}

pub(super) unsafe extern "C" fn fake_get_auth_ticket_for_web_api(
    _: *mut ISteamUser,
    identity: *const c_char,
) -> u32 {
    // SAFETY: the caller passes a NUL-terminated string.
    let identity = unsafe { CStr::from_ptr(identity) }
        .to_string_lossy()
        .into_owned();
    script(|s| {
        s.tickets.web_identities.push(identity);
        s.tickets.next
    })
}

pub(super) unsafe extern "C" fn fake_begin_auth_session(
    _: *mut ISteamUser,
    ticket: *const c_void,
    len: i32,
    user: u64,
) -> i32 {
    let len = usize::try_from(len).unwrap();
    // SAFETY: the caller passes `len` readable bytes.
    let ticket = unsafe { core::slice::from_raw_parts(ticket.cast::<u8>(), len) }.to_vec();
    script(|s| {
        s.tickets.begun.push((ticket, user));
        s.tickets.begin_result
    })
}

pub(super) unsafe extern "C" fn fake_end_auth_session(_: *mut ISteamUser, user: u64) {
    script(|s| s.tickets.ended.push(user));
}

pub(super) unsafe extern "C" fn fake_cancel_auth_ticket(_: *mut ISteamUser, ticket: u32) {
    script(|s| s.tickets.cancelled.push(ticket));
}

pub(super) unsafe extern "C" fn fake_user_has_license_for_app(
    _: *mut ISteamUser,
    user: u64,
    app: u32,
) -> i32 {
    script(|s| {
        s.tickets.license_asked.push((user, app));
        s.tickets.license
    })
}

pub(super) unsafe extern "C" fn fake_request_encrypted_app_ticket(
    _: *mut ISteamUser,
    data: *mut c_void,
    len: i32,
) -> SteamApiCall {
    let len = usize::try_from(len).unwrap();
    // SAFETY: the caller passes `len` readable bytes.
    let data = unsafe { core::slice::from_raw_parts(data.cast::<u8>(), len) }.to_vec();
    script(|s| {
        s.tickets.encrypted_data = data;
        s.tickets.encrypted_call
    })
}

pub(super) unsafe extern "C" fn fake_get_encrypted_app_ticket(
    _: *mut ISteamUser,
    out: *mut c_void,
    capacity: i32,
    written: *mut u32,
) -> bool {
    script(|s| {
        let tickets = &mut s.tickets;
        tickets.encrypted_offered.push(capacity);
        let Some(bytes) = &tickets.encrypted else {
            return false;
        };
        let size = u32::try_from(bytes.len()).unwrap();
        // SAFETY: the caller passes a writable count.
        unsafe { written.write(size) };
        if bytes.len() > usize::try_from(capacity).unwrap() {
            return false;
        }
        // SAFETY: the caller passes `capacity` writable bytes, at least
        // `bytes.len()` of them.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len()) };
        true
    })
}
