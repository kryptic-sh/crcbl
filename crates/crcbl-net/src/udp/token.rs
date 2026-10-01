//! Connection tokens: what a hello must carry before a [`super::UdpListener`]
//! spends an X25519 agreement or a pending slot on it.
//!
//! This is netcode.io's connect-token pattern in its direct-connect form: the
//! listener is the trusted source that mints them. A hello without a valid
//! token is answered with a [`super::Challenge`] carrying a fresh one, which
//! costs the listener one HMAC and no state; only a hello that presents it
//! back from the address it was minted for gets the real reply. So a flood of
//! hellos from spoofed addresses buys challenges sent to those addresses —
//! never longer than the hellos that asked for them — and nothing else: the
//! spoofer never sees a challenge, so it never holds a token that verifies.
//!
//! # Layout
//!
//! ```text
//! kind:    u8      SERVER_MINTED; 0 means the hello carries no token
//! expires: u64 LE  nanoseconds on the minting listener's clock
//! serial:  u64 LE  which token this is, unique under one key
//! mac:     [u8; TOKEN_MAC_BYTES]
//! ```
//!
//! The MAC is HMAC-SHA256, truncated as [`crate::auth`]'s is, keyed with the
//! [`TokenKey`] and taken over [`TOKEN_DOMAIN`], the kind, expiry and serial
//! bytes above, the protocol id (u32 LE), and the client's address: a family
//! byte (4 or 6), the IP's octets, the port (u16 LE). Every field is written
//! in that defined encoding, never read out of memory. The MAC is checked in
//! constant time by `hmac`'s `verify_truncated_left`, and before anything it
//! covers is believed: a forged token's expiry is never consulted.
//!
//! # What a token is bound to
//!
//! - **The address it was minted for.** A token presented from any other
//!   address does not verify, so one captured by an on-path observer gets a
//!   session only for the address it already names.
//! - **The key, which is per listener run.** [`super::UdpListener`] draws its
//!   key from the operating system at bind, so a restart invalidates every
//!   token the old run minted.
//! - **An expiry**, [`super::TOKEN_LIFETIME`] after minting.
//!
//! Single use is the listener's part, not the token's: it keeps the serials
//! it has spent until they expire (see [`super::MAX_SPENT_TOKENS`]).
//!
//! # A backend minting tokens later
//!
//! Verification needs only the key and the defined encoding above, so a
//! backend that holds the same key mints tokens this verifies unchanged. Two
//! things change with it, and the `kind` byte is what lets them arrive as a
//! second kind beside [`SERVER_MINTED`] instead of replacing it: the expiry
//! must be on a clock both share (UNIX time rather than the listener's own),
//! and the serial must be unique across every minter of the key.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use hmac::Mac;
use hmac::digest::zeroize::Zeroizing;

use crate::seal::kdf::{HmacSha256, keyed};

/// Bytes of a token: kind, expiry, serial and MAC.
pub const TOKEN_BYTES: usize = 1 + size_of::<u64>() + size_of::<u64>() + TOKEN_MAC_BYTES;

/// A hello's token field when it carries no token: the kind byte, and the
/// rest, zero.
pub const NO_TOKEN: [u8; TOKEN_BYTES] = [0; TOKEN_BYTES];

/// Bytes of a [`TokenKey`].
pub const TOKEN_KEY_BYTES: usize = 32;

/// The kind byte of a token a listener minted for itself.
pub const SERVER_MINTED: u8 = 1;

/// The kind byte of [`NO_TOKEN`].
const NO_TOKEN_KIND: u8 = 0;

/// Bytes of the truncated MAC a token carries: the 128 bits
/// [`crate::auth::MAC_BYTES`] keeps too.
const TOKEN_MAC_BYTES: usize = crate::auth::MAC_BYTES;

/// Domain separator for the token MAC, so no other HMAC under a key that
/// happened to be shared could be mistaken for a token.
pub const TOKEN_DOMAIN: &[u8] = b"crcbl connect token v1";

const EXPIRES_AT: usize = 1;
const SERIAL_AT: usize = EXPIRES_AT + size_of::<u64>();
const MAC_AT: usize = SERIAL_AT + size_of::<u64>();

const _: () = assert!(MAC_AT + TOKEN_MAC_BYTES == TOKEN_BYTES);
const _: () = assert!(NO_TOKEN_KIND != SERVER_MINTED);

/// Why a token did not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// The hello carries no token: its first challenge has not come back yet.
    #[error("no token")]
    Absent,
    /// Not [`TOKEN_BYTES`] long, or a kind this listener does not mint.
    #[error("not a token this listener mints")]
    Malformed,
    /// The MAC does not verify: minted under another key — another listener,
    /// or this one before a restart — for another address or protocol id, or
    /// altered.
    #[error("token authentication failed")]
    Forged,
    /// Authentic, and past its expiry.
    #[error("token expired")]
    Expired,
}

/// What a verified token says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedToken {
    /// Which token this is: unique under the key that minted it.
    pub serial: u64,
    /// When it stops verifying, on the minting listener's clock.
    pub expires: Duration,
}

/// The key tokens are minted and verified under. Wiped on drop, and never
/// printed.
pub struct TokenKey(Zeroizing<[u8; TOKEN_KEY_BYTES]>);

impl TokenKey {
    /// The key whose bytes are `secret`, which should come from the operating
    /// system's entropy source.
    #[must_use]
    pub fn from_secret_bytes(secret: [u8; TOKEN_KEY_BYTES]) -> Self {
        Self(Zeroizing::new(secret))
    }

    /// A token for `client` speaking `protocol_id`, verifying until `expires`
    /// on the verifier's clock, numbered `serial`.
    #[must_use]
    pub fn mint(
        &self,
        client: SocketAddr,
        protocol_id: u32,
        expires: Duration,
        serial: u64,
    ) -> [u8; TOKEN_BYTES] {
        let expires = u64::try_from(expires.as_nanos()).unwrap_or(u64::MAX);
        let mut token = [0u8; TOKEN_BYTES];
        token[0] = SERVER_MINTED;
        token[EXPIRES_AT..SERIAL_AT].copy_from_slice(&expires.to_le_bytes());
        token[SERIAL_AT..MAC_AT].copy_from_slice(&serial.to_le_bytes());
        let mac = self
            .mac(&token[..MAC_AT], client, protocol_id)
            .finalize()
            .into_bytes();
        token[MAC_AT..].copy_from_slice(&mac[..TOKEN_MAC_BYTES]);
        token
    }

    /// Whether `token` is one this key minted for `client` and `protocol_id`
    /// and still valid at `now`. Total on arbitrary bytes and never panics.
    ///
    /// # Errors
    ///
    /// A [`TokenError`] saying which check refused it. The MAC is checked
    /// before the expiry, so [`TokenError::Expired`] is only ever said of a
    /// token this key really minted.
    pub fn verify(
        &self,
        token: &[u8],
        client: SocketAddr,
        protocol_id: u32,
        now: Duration,
    ) -> Result<VerifiedToken, TokenError> {
        let token: &[u8; TOKEN_BYTES] = token.try_into().map_err(|_| TokenError::Malformed)?;
        match token[0] {
            NO_TOKEN_KIND => return Err(TokenError::Absent),
            SERVER_MINTED => {}
            _ => return Err(TokenError::Malformed),
        }
        self.mac(&token[..MAC_AT], client, protocol_id)
            .verify_truncated_left(&token[MAC_AT..])
            .map_err(|_| TokenError::Forged)?;
        let field = |at: usize| {
            u64::from_le_bytes(
                token[at..at + size_of::<u64>()]
                    .try_into()
                    .expect("a u64 field inside a fixed-size token"),
            )
        };
        let expires = Duration::from_nanos(field(EXPIRES_AT));
        if now >= expires {
            return Err(TokenError::Expired);
        }
        Ok(VerifiedToken {
            serial: field(SERIAL_AT),
            expires,
        })
    }

    /// The MAC over `fields` — the token up to its MAC — and what it is bound
    /// to, in the encoding the module docs define.
    fn mac(&self, fields: &[u8], client: SocketAddr, protocol_id: u32) -> HmacSha256 {
        let mut mac = keyed(&self.0[..]);
        mac.update(TOKEN_DOMAIN);
        mac.update(fields);
        mac.update(&protocol_id.to_le_bytes());
        match client {
            SocketAddr::V4(address) => {
                mac.update(&[4]);
                mac.update(&address.ip().octets());
            }
            SocketAddr::V6(address) => {
                mac.update(&[6]);
                mac.update(&address.ip().octets());
            }
        }
        mac.update(&client.port().to_le_bytes());
        mac
    }
}

impl fmt::Debug for TokenKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenKey([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal::kdf::hmac_sha256;

    const PROTOCOL: u32 = 0x4352_4342;
    const NOW: Duration = Duration::from_secs(7);
    const EXPIRES: Duration = Duration::from_secs(9);

    fn key() -> TokenKey {
        TokenKey::from_secret_bytes([0xA5; TOKEN_KEY_BYTES])
    }

    fn client() -> SocketAddr {
        "192.0.2.7:4100".parse().expect("an address")
    }

    /// The layout and MAC input in the module docs, rebuilt by hand, so any
    /// minter that follows them — a backend's among them — makes the same
    /// token.
    #[test]
    fn a_token_is_laid_out_and_authenticated_as_documented() {
        let token = key().mint(client(), PROTOCOL, EXPIRES, 0x0102_0304_0506_0708);
        let mut fields = vec![SERVER_MINTED];
        fields.extend_from_slice(&9_000_000_000u64.to_le_bytes());
        fields.extend_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        let mac = hmac_sha256(
            &[0xA5; TOKEN_KEY_BYTES],
            &[
                TOKEN_DOMAIN,
                &fields,
                &PROTOCOL.to_le_bytes(),
                &[4, 192, 0, 2, 7],
                &4100u16.to_le_bytes(),
            ],
        );
        assert_eq!(token[..MAC_AT], fields[..]);
        assert_eq!(token[MAC_AT..], mac[..TOKEN_MAC_BYTES]);
        assert_eq!(
            key().verify(&token, client(), PROTOCOL, NOW),
            Ok(VerifiedToken {
                serial: 0x0102_0304_0506_0708,
                expires: EXPIRES,
            })
        );
    }

    /// Valid strictly before its expiry, and not at it.
    #[test]
    fn a_token_expires_at_its_expiry() {
        let token = key().mint(client(), PROTOCOL, EXPIRES, 1);
        let just_before = EXPIRES - Duration::from_nanos(1);
        assert!(
            key()
                .verify(&token, client(), PROTOCOL, just_before)
                .is_ok()
        );
        assert_eq!(
            key().verify(&token, client(), PROTOCOL, EXPIRES),
            Err(TokenError::Expired)
        );
    }

    /// Everything the MAC binds refuses the token when it differs: the key,
    /// the address's IP, port and family, the protocol id, and every byte of
    /// the token itself — the expiry among them, so a token cannot be
    /// extended.
    #[test]
    fn every_bound_input_refuses_the_token() {
        let token = key().mint(client(), PROTOCOL, EXPIRES, 1);
        let other_key = TokenKey::from_secret_bytes([0x5A; TOKEN_KEY_BYTES]);
        for (what, verdict) in [
            (
                "another key",
                other_key.verify(&token, client(), PROTOCOL, NOW),
            ),
            (
                "another IP",
                key().verify(&token, "192.0.2.8:4100".parse().unwrap(), PROTOCOL, NOW),
            ),
            (
                "another port",
                key().verify(&token, "192.0.2.7:4101".parse().unwrap(), PROTOCOL, NOW),
            ),
            (
                "the IPv4-mapped IPv6 form",
                key().verify(
                    &token,
                    "[::ffff:192.0.2.7]:4100".parse().unwrap(),
                    PROTOCOL,
                    NOW,
                ),
            ),
            (
                "another protocol id",
                key().verify(&token, client(), PROTOCOL + 1, NOW),
            ),
        ] {
            assert_eq!(verdict, Err(TokenError::Forged), "{what}");
        }
        for index in 1..TOKEN_BYTES {
            let mut altered = token;
            altered[index] ^= 0x01;
            assert_eq!(
                key().verify(&altered, client(), PROTOCOL, NOW),
                Err(TokenError::Forged),
                "byte {index}"
            );
        }
    }

    #[test]
    fn no_token_another_kind_or_another_length_is_refused_by_name() {
        let token = key().mint(client(), PROTOCOL, EXPIRES, 1);
        let verify = |bytes: &[u8]| key().verify(bytes, client(), PROTOCOL, NOW);
        assert_eq!(verify(&NO_TOKEN), Err(TokenError::Absent));
        let mut other_kind = token;
        other_kind[0] = SERVER_MINTED + 1;
        assert_eq!(verify(&other_kind), Err(TokenError::Malformed));
        assert_eq!(
            verify(&token[..TOKEN_BYTES - 1]),
            Err(TokenError::Malformed)
        );
        assert_eq!(
            verify(&[token.as_slice(), &[0]].concat()),
            Err(TokenError::Malformed)
        );
        assert_eq!(verify(&[]), Err(TokenError::Malformed));
    }

    #[test]
    fn debug_redacts_the_key() {
        assert_eq!(format!("{:?}", key()), "TokenKey([REDACTED])");
    }
}
