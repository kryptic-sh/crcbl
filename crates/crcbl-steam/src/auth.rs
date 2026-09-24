//! `ISteamUser`'s tickets (Steamworks slice 12): proving to
//! another player, to a web service or to a publisher backend that this is
//! who Steam says it is.
//!
//! - **Session tickets** ([`Auth::session_ticket`]) go to a peer, who hands
//!   the bytes to [`Auth::begin_session`] and hears Steam's verdict as
//!   [`SteamEvent::AuthSessionVerdict`](crate::SteamEvent::AuthSessionVerdict).
//!   An [`AuthGate`] turns those verdicts into admission for a listen host:
//!   provisional until Steam answers, then admitted, rejected, or timed out.
//! - **Web-API tickets** ([`Auth::web_api_ticket`]) go to a web service; the
//!   bytes arrive as [`SteamEvent::WebApiTicket`](crate::SteamEvent::WebApiTicket).
//! - **Encrypted app tickets** ([`Auth::request_encrypted_ticket`], then
//!   [`Auth::encrypted_ticket`]) go to a backend holding the app's key.
//!   Decrypting one needs Valve's separate `sdkencryptedappticket` library on
//!   that backend, which this crate does not load: the project runs no
//!   backend (`docs/notes/backends.md`, "Steam is a backend the project does
//!   not have to run").
//!
//! Every ticket and session is ended by the value that holds it — a
//! [`SessionTicket`] or [`WebApiTicket`] cancels itself on drop
//! (`CancelAuthTicket`), an [`AuthSession`] ends itself (`EndAuthSession`) —
//! exactly once.
//!
//! EW needs none of this (its requirement 2: no anti-cheat, no encrypted
//! tickets), and peer identity for a Steam P2P connection already comes from
//! the connection's certified identity (`SteamTransport::remote`).

mod gate;

pub use gate::{AuthGate, Verdict};

use std::{marker::PhantomData, sync::Arc};

use crate::{
    AppId, EResult, Steam, SteamCall, SteamError, SteamId,
    call::{CallRow, private::Answer},
    callbacks::{Base, read},
    client::Client,
    error::c_string,
    ffi::structs,
    net::identity,
};

/// The buffer a session ticket is read into: 1024 bytes, the size
/// `ISteamUser::GetAuthSessionTicket`'s documentation asks for.
pub const SESSION_TICKET_BYTES: usize = 1024;

/// The first buffer an encrypted app ticket is read into; Steam names the
/// size it needs when that is not enough.
const ENCRYPTED_TICKET_BYTES: usize = 1024;

/// The most an encrypted app ticket is read into. A size past it is a broken
/// library, refused rather than allocated.
const MAX_ENCRYPTED_TICKET_BYTES: usize = 8 * 1024;

/// A ticket's handle (`HAuthTicket`); zero is `k_HAuthTicketInvalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuthTicketId(pub u32);

/// Steam's verdict on a peer's ticket (`EAuthSessionResponse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthResponse {
    /// Valid, and the player is online (`k_EAuthSessionResponseOK`).
    Ok,
    /// `k_EAuthSessionResponseUserNotConnectedToSteam`.
    UserNotConnectedToSteam,
    /// `k_EAuthSessionResponseNoLicenseOrExpired`.
    NoLicenseOrExpired,
    /// `k_EAuthSessionResponseVACBanned`.
    VacBanned,
    /// `k_EAuthSessionResponseLoggedInElseWhere`.
    LoggedInElsewhere,
    /// `k_EAuthSessionResponseVACCheckTimedOut`.
    VacCheckTimedOut,
    /// The issuer cancelled it (`k_EAuthSessionResponseAuthTicketCanceled`).
    TicketCanceled,
    /// `k_EAuthSessionResponseAuthTicketInvalidAlreadyUsed`.
    TicketAlreadyUsed,
    /// `k_EAuthSessionResponseAuthTicketInvalid`.
    TicketInvalid,
    /// `k_EAuthSessionResponsePublisherIssuedBan`.
    PublisherIssuedBan,
    /// `k_EAuthSessionResponseAuthTicketNetworkIdentityFailure`.
    NetworkIdentityFailure,
    /// A value this crate does not name.
    Other(i32),
}

impl AuthResponse {
    /// Maps an `EAuthSessionResponse`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Ok,
            1 => Self::UserNotConnectedToSteam,
            2 => Self::NoLicenseOrExpired,
            3 => Self::VacBanned,
            4 => Self::LoggedInElsewhere,
            5 => Self::VacCheckTimedOut,
            6 => Self::TicketCanceled,
            7 => Self::TicketAlreadyUsed,
            8 => Self::TicketInvalid,
            9 => Self::PublisherIssuedBan,
            10 => Self::NetworkIdentityFailure,
            other => Self::Other(other),
        }
    }
}

/// Why [`Auth::begin_session`] would not take a ticket
/// (`EBeginAuthSessionResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BeginAuthError {
    /// `k_EBeginAuthSessionResultInvalidTicket`.
    #[error("the ticket is not valid")]
    InvalidTicket,
    /// A ticket for this user is already being validated
    /// (`k_EBeginAuthSessionResultDuplicateRequest`).
    #[error("a ticket for this user is already being validated")]
    DuplicateRequest,
    /// `k_EBeginAuthSessionResultInvalidVersion`.
    #[error("the ticket is from an incompatible interface version")]
    InvalidVersion,
    /// `k_EBeginAuthSessionResultGameMismatch`.
    #[error("the ticket is for another game")]
    GameMismatch,
    /// `k_EBeginAuthSessionResultExpiredTicket`.
    #[error("the ticket has expired")]
    ExpiredTicket,
    /// A ticket longer than an `int` counts, refused before the call.
    #[error("the ticket is too long for Steam to take")]
    TooLong,
    /// A value this crate does not name.
    #[error("EBeginAuthSessionResult {0}")]
    Other(i32),
}

/// Whether a user has a licence for an app (`EUserHasLicenseForAppResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum License {
    /// `k_EUserHasLicenseResultHasLicense`.
    Has,
    /// `k_EUserHasLicenseResultDoesNotHaveLicense`.
    DoesNotHave,
    /// The user has no validated session to ask about
    /// (`k_EUserHasLicenseResultNoAuth`).
    NoAuth,
    /// A value this crate does not name.
    Other(i32),
}

/// `ISteamUser`'s tickets, borrowed from a [`Steam`]; from [`Steam::auth`].
#[derive(Debug)]
pub struct Auth<'a> {
    steam: &'a mut Steam,
}

impl Steam {
    /// Tickets and ticket validation.
    pub fn auth(&mut self) -> Auth<'_> {
        Auth { steam: self }
    }
}

/// Cancels a ticket once, when dropped: the part [`SessionTicket`] and
/// [`WebApiTicket`] share.
#[derive(Debug)]
struct TicketGuard {
    client: Arc<Client>,
    ticket: AuthTicketId,
    _not_send: PhantomData<*const ()>,
}

impl Drop for TicketGuard {
    fn drop(&mut self) {
        let client = &self.client;
        // SAFETY: `client.user` is live; `TicketGuard` is `!Send`, so this is
        // the pump thread; the ticket was issued by this session and is
        // cancelled here once.
        unsafe { (client.lib.fns.user.cancel_auth_ticket)(client.user, self.ticket.0) };
    }
}

/// A session ticket for a peer to validate: its bytes, and its handle.
/// Cancelled when dropped (`CancelAuthTicket`), which ends any session a peer
/// began with it.
#[derive(Debug)]
pub struct SessionTicket {
    guard: TicketGuard,
    bytes: Vec<u8>,
}

impl SessionTicket {
    /// The handle, which
    /// [`SteamEvent::AuthTicketReady`](crate::SteamEvent::AuthTicketReady)
    /// names.
    #[must_use]
    pub const fn id(&self) -> AuthTicketId {
        self.guard.ticket
    }

    /// The bytes to send.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A web-API ticket's handle; its bytes arrive as
/// [`SteamEvent::WebApiTicket`](crate::SteamEvent::WebApiTicket). Cancelled
/// when dropped (`CancelAuthTicket`).
#[derive(Debug)]
pub struct WebApiTicket {
    guard: TicketGuard,
}

impl WebApiTicket {
    /// The handle the event names.
    #[must_use]
    pub const fn id(&self) -> AuthTicketId {
        self.guard.ticket
    }
}

/// A peer's ticket being validated, from [`Auth::begin_session`]. Ended when
/// dropped (`EndAuthSession`), once.
#[derive(Debug)]
pub struct AuthSession {
    client: Arc<Client>,
    user: SteamId,
    _not_send: PhantomData<*const ()>,
}

impl AuthSession {
    /// Whose ticket.
    #[must_use]
    pub const fn user(&self) -> SteamId {
        self.user
    }
}

impl Drop for AuthSession {
    fn drop(&mut self) {
        let client = &self.client;
        // SAFETY: `client.user` is live; `AuthSession` is `!Send`, so this is
        // the pump thread; the session was begun by this value and is ended
        // here once.
        unsafe { (client.lib.fns.user.end_auth_session)(client.user, self.user.0) };
    }
}

impl Auth<'_> {
    /// A ticket proving this player to `verifier` — the peer that will call
    /// [`begin_session`](Self::begin_session) with it — or to anyone, with
    /// `None` (`GetAuthSessionTicket`). Usable once
    /// [`SteamEvent::AuthTicketReady`](crate::SteamEvent::AuthTicketReady)
    /// names it.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam issues none;
    /// [`SteamError::Truncated`] when it claims more bytes than the
    /// [`SESSION_TICKET_BYTES`] buffer holds.
    pub fn session_ticket(&self, verifier: Option<SteamId>) -> Result<SessionTicket, SteamError> {
        let verifier = verifier.map(identity::of);
        let verifier_ptr = verifier
            .as_ref()
            .map_or(core::ptr::null(), core::ptr::from_ref);
        let mut bytes = vec![0_u8; SESSION_TICKET_BYTES];
        let capacity = i32::try_from(bytes.len())
            .map_err(|_| SteamError::Truncated("GetAuthSessionTicket"))?;
        let mut written = 0_u32;
        let client = &self.steam.client;
        // SAFETY: `client.user` is live; `Auth` borrows the `!Send` `Steam`,
        // so this is the pump thread; `bytes` is `capacity` writable bytes,
        // `written` is writable, and `verifier_ptr` is null or points at an
        // identity alive for the call.
        let handle = unsafe {
            (client.lib.fns.user.get_auth_session_ticket)(
                client.user,
                bytes.as_mut_ptr().cast(),
                capacity,
                &raw mut written,
                verifier_ptr,
            )
        };
        if handle == 0 {
            return Err(SteamError::Refused("GetAuthSessionTicket"));
        }
        let guard = TicketGuard {
            client: Arc::clone(client),
            ticket: AuthTicketId(handle),
            _not_send: PhantomData,
        };
        // A count past the buffer: the ticket is cancelled (by `guard`'s
        // drop) and refused, never read past.
        let written = usize::try_from(written)
            .ok()
            .filter(|&n| n <= bytes.len())
            .ok_or(SteamError::Truncated("GetAuthSessionTicket"))?;
        bytes.truncate(written);
        Ok(SessionTicket { guard, bytes })
    }

    /// A ticket for the web service named `identity`, whose bytes arrive as
    /// [`SteamEvent::WebApiTicket`](crate::SteamEvent::WebApiTicket)
    /// (`GetAuthTicketForWebApi`).
    ///
    /// # Errors
    ///
    /// [`SteamError::InteriorNul`]; [`SteamError::Refused`] when Steam issues
    /// none.
    pub fn web_api_ticket(&self, identity: &str) -> Result<WebApiTicket, SteamError> {
        let identity = c_string(identity, "identity", usize::MAX)?;
        let client = &self.steam.client;
        // SAFETY: as in `session_ticket`; `identity` is NUL-terminated for
        // the call.
        let handle = unsafe {
            (client.lib.fns.user.get_auth_ticket_for_web_api)(client.user, identity.as_ptr())
        };
        if handle == 0 {
            return Err(SteamError::Refused("GetAuthTicketForWebApi"));
        }
        Ok(WebApiTicket {
            guard: TicketGuard {
                client: Arc::clone(client),
                ticket: AuthTicketId(handle),
                _not_send: PhantomData,
            },
        })
    }

    /// Starts validating `user`'s `ticket` (`BeginAuthSession`); Steam's
    /// verdict arrives as
    /// [`SteamEvent::AuthSessionVerdict`](crate::SteamEvent::AuthSessionVerdict).
    ///
    /// # Errors
    ///
    /// Each [`BeginAuthError`] Steam answers — the ticket is refused on the
    /// spot — and [`BeginAuthError::TooLong`] before the call.
    pub fn begin_session(
        &self,
        ticket: &[u8],
        user: SteamId,
    ) -> Result<AuthSession, BeginAuthError> {
        let len = i32::try_from(ticket.len()).map_err(|_| BeginAuthError::TooLong)?;
        let client = &self.steam.client;
        // SAFETY: as in `session_ticket`; `ticket` is `len` readable bytes.
        let result = unsafe {
            (client.lib.fns.user.begin_auth_session)(
                client.user,
                ticket.as_ptr().cast(),
                len,
                user.0,
            )
        };
        match result {
            0 => Ok(AuthSession {
                client: Arc::clone(client),
                user,
                _not_send: PhantomData,
            }),
            1 => Err(BeginAuthError::InvalidTicket),
            2 => Err(BeginAuthError::DuplicateRequest),
            3 => Err(BeginAuthError::InvalidVersion),
            4 => Err(BeginAuthError::GameMismatch),
            5 => Err(BeginAuthError::ExpiredTicket),
            other => Err(BeginAuthError::Other(other)),
        }
    }

    /// Whether `user`, whose ticket is being validated, has a licence for
    /// `app` — DLC, say (`UserHasLicenseForApp`).
    #[must_use]
    pub fn user_has_license(&self, user: SteamId, app: AppId) -> License {
        let client = &self.steam.client;
        // SAFETY: as in `session_ticket`.
        match unsafe { (client.lib.fns.user.user_has_license_for_app)(client.user, user.0, app.0) }
        {
            0 => License::Has,
            1 => License::DoesNotHave,
            2 => License::NoAuth,
            other => License::Other(other),
        }
    }

    /// Asks Steam for an encrypted app ticket carrying `data`
    /// (`RequestEncryptedAppTicket`); once the call answers `EResult::OK`,
    /// [`encrypted_ticket`](Self::encrypted_ticket) reads it.
    ///
    /// # Errors
    ///
    /// [`SteamError::TooLong`] for data past an `int`;
    /// [`SteamError::Refused`] when Steam did not start the call.
    pub fn request_encrypted_ticket(
        &mut self,
        data: &[u8],
    ) -> Result<SteamCall<EncryptedTicketReady>, SteamError> {
        let len = i32::try_from(data.len()).map_err(|_| SteamError::TooLong {
            argument: "ticket data",
            len: data.len(),
            max: usize::try_from(i32::MAX).unwrap_or(usize::MAX),
        })?;
        // A copy: Steam takes the data through a mutable pointer.
        let mut data = data.to_vec();
        let client = &self.steam.client;
        // SAFETY: as in `session_ticket`; `data` is `len` writable bytes.
        let handle = unsafe {
            (client.lib.fns.user.request_encrypted_app_ticket)(
                client.user,
                data.as_mut_ptr().cast(),
                len,
            )
        };
        self.steam
            .calls
            .register(handle)
            .ok_or(SteamError::Refused("RequestEncryptedAppTicket"))
    }

    /// The encrypted app ticket the last request produced
    /// (`GetEncryptedAppTicket`), read again at the size Steam names while a
    /// buffer is too small.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam has none, or names a size past
    /// what this crate will read.
    pub fn encrypted_ticket(&self) -> Result<Vec<u8>, SteamError> {
        let client = &self.steam.client;
        let mut size = ENCRYPTED_TICKET_BYTES;
        loop {
            let mut bytes = vec![0_u8; size];
            let capacity =
                i32::try_from(size).map_err(|_| SteamError::Refused("GetEncryptedAppTicket"))?;
            let mut written = 0_u32;
            // SAFETY: as in `session_ticket`; `bytes` is `capacity` writable
            // bytes and `written` is writable.
            let read = unsafe {
                (client.lib.fns.user.get_encrypted_app_ticket)(
                    client.user,
                    bytes.as_mut_ptr().cast(),
                    capacity,
                    &raw mut written,
                )
            };
            let written = usize::try_from(written).unwrap_or(usize::MAX);
            if read && written <= size {
                bytes.truncate(written);
                return Ok(bytes);
            }
            // Refused, or a size Steam named: read again at it if it is
            // larger — so each round grows — and within bounds.
            if written <= size || written > MAX_ENCRYPTED_TICKET_BYTES {
                return Err(SteamError::Refused("GetEncryptedAppTicket"));
            }
            size = written;
        }
    }
}

/// The answer to [`Auth::request_encrypted_ticket`]
/// (`EncryptedAppTicketResponse_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptedTicketReady {
    /// `EResult::OK` when [`Auth::encrypted_ticket`] can read the ticket.
    pub result: EResult,
}

impl Answer for EncryptedTicketReady {
    const ROW: CallRow = CallRow {
        base: Base::User,
        offset: 54,
        #[cfg(test)]
        name: "EncryptedAppTicketResponse_t",
        size: size_of::<structs::EncryptedAppTicketResponse>(),
    };

    fn build(bytes: &[u8], _: &mut Steam) -> Option<Self> {
        let raw = read::<structs::EncryptedAppTicketResponse>(bytes)?;
        Some(Self {
            result: EResult(raw.result),
        })
    }

    fn abandon(_: &[u8], _: &Client) {}
}

#[cfg(test)]
mod tests;
