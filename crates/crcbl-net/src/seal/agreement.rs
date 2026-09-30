//! X25519 (RFC 7748) between two peers' key pairs, feeding the key schedule.
//!
//! Each side builds a [`KeyPair`] from 32 secret bytes it was handed — this
//! crate draws no randomness, so the bytes come from whoever owns the socket,
//! out of the operating system — sends [`KeyPair::public_key`] to the peer,
//! and passes the peer's public key to [`agree_channel`]. Both sides then hold
//! matching [`Sealer`] / [`Opener`] pairs, one key per direction.
//!
//! The curve arithmetic is `x25519-dalek`'s; the secret is its
//! `StaticSecret`, which clamps on use (RFC 7748 §5) and is wiped on drop.
//! "Static" is the crate's name for a secret that can be used more than once,
//! not a claim about this protocol: a key pair is meant for one connection.

use std::fmt;

use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

use super::keys::{Role, SessionKeys, X25519_BYTES};
use super::{Opener, Sealer};

/// Why no channel could be keyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyAgreementError {
    /// The X25519 output is all zeros, which it is exactly when the peer sent
    /// a small-order point: the "shared" secret is then one an observer knows
    /// too. RFC 7748 §6.1 says to check for it and abort.
    #[error("the key agreement output is all zeros: the peer's public key is a small-order point")]
    NonContributory,
}

/// One side's X25519 key pair, for one connection.
///
/// `Debug` prints the public key and never the secret.
pub struct KeyPair {
    secret: StaticSecret,
    public: PublicKey,
}

impl KeyPair {
    /// The key pair whose secret is `secret`: 32 bytes from the operating
    /// system's entropy source, fresh for every connection. Any 32 bytes are a
    /// valid X25519 secret, since the scalar is clamped on use.
    #[must_use]
    pub fn from_secret_bytes(secret: [u8; X25519_BYTES]) -> Self {
        let secret = StaticSecret::from(secret);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// The public key to send the peer, RFC 7748 §6.1's `X25519(a, 9)`.
    #[must_use]
    pub fn public_key(&self) -> [u8; X25519_BYTES] {
        self.public.to_bytes()
    }

    /// RFC 7748 §6.1's `K`, refused when it is all zeros.
    fn shared_secret(
        &self,
        peer_public: &[u8; X25519_BYTES],
    ) -> Result<SharedSecret, KeyAgreementError> {
        let shared = self.secret.diffie_hellman(&PublicKey::from(*peer_public));
        // `was_contributory` is the all-zero check, done in constant time.
        if shared.was_contributory() {
            Ok(shared)
        } else {
            Err(KeyAgreementError::NonContributory)
        }
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", self.public.as_bytes())
            .finish_non_exhaustive()
    }
}

/// Key one side of a sealed link: this side's [`Sealer`] and [`Opener`], from
/// its own key pair and the public key the peer sent.
///
/// `protocol_id` is the build's [`crate::reliable::Endpoint`] protocol id, and
/// both sides must pass the same one. Call it once per connection, with a new
/// [`KeyPair`] for each: that is the rekey on reconnect, and a new sealer is
/// also the only way past [`super::SealError::CounterExhausted`]. Nothing here
/// authenticates `peer_public` — see [`super`]'s docs for what that leaves
/// open.
///
/// # Errors
///
/// [`KeyAgreementError::NonContributory`] when `peer_public` is a small-order
/// point.
pub fn agree_channel(
    role: Role,
    local: &KeyPair,
    peer_public: &[u8; X25519_BYTES],
    protocol_id: u32,
) -> Result<(Sealer, Opener), KeyAgreementError> {
    let shared = local.shared_secret(peer_public)?;
    let local_public = local.public_key();
    let (client_public, server_public) = match role {
        Role::Client => (&local_public, peer_public),
        Role::Server => (peer_public, &local_public),
    };
    Ok(
        SessionKeys::derive(shared.as_bytes(), client_public, server_public, protocol_id)
            .into_channel(role),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal::OpenError;
    use crate::seal::tests::hex;

    const PROTOCOL: u32 = 0x4352_4342;

    fn bytes(text: &str) -> [u8; 32] {
        hex(text).try_into().expect("32 bytes")
    }

    /// RFC 7748 §6.1's test vector, through the crate: both public keys from
    /// the private ones, and the same shared secret computed from each side.
    #[test]
    fn x25519_matches_the_rfc7748_vector() {
        let alice = KeyPair::from_secret_bytes(bytes(
            "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a",
        ));
        let bob = KeyPair::from_secret_bytes(bytes(
            "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb",
        ));
        let shared = bytes("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");

        assert_eq!(
            alice.public_key(),
            bytes("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        );
        assert_eq!(
            bob.public_key(),
            bytes("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
        );
        let alice_shared = alice.shared_secret(&bob.public_key()).unwrap();
        let bob_shared = bob.shared_secret(&alice.public_key()).unwrap();
        assert_eq!(*alice_shared.as_bytes(), shared);
        assert_eq!(*bob_shared.as_bytes(), shared);
    }

    fn client() -> KeyPair {
        KeyPair::from_secret_bytes([0x11; 32])
    }

    fn server() -> KeyPair {
        KeyPair::from_secret_bytes([0x22; 32])
    }

    /// Two parties, each with only its own secret and the other's public key,
    /// end up with channels that open each other's datagrams both ways.
    #[test]
    fn two_parties_agree_on_matching_channels() {
        let (client, server) = (client(), server());
        let (mut client_sealer, mut client_opener) =
            agree_channel(Role::Client, &client, &server.public_key(), PROTOCOL).unwrap();
        let (mut server_sealer, mut server_opener) =
            agree_channel(Role::Server, &server, &client.public_key(), PROTOCOL).unwrap();

        let upstream = client_sealer.seal(b"from the client").unwrap();
        assert_eq!(server_opener.open(&upstream).unwrap(), b"from the client");
        let downstream = server_sealer.seal(b"from the server").unwrap();
        assert_eq!(client_opener.open(&downstream).unwrap(), b"from the server");
    }

    /// A substituted public key — the middle of a MITM, seen from one end —
    /// or two sides that both think they are the client, or disagree on the
    /// protocol id, get channels that do not open each other's datagrams.
    #[test]
    fn a_mismatched_agreement_does_not_open() {
        let (client, server) = (client(), server());
        let intruder = KeyPair::from_secret_bytes([0x33; 32]);
        let sealed = |role, local: &KeyPair, peer: [u8; 32], protocol_id| {
            let (mut sealer, _) = agree_channel(role, local, &peer, protocol_id).unwrap();
            sealer.seal(b"packet").unwrap()
        };
        let (_, mut server_opener) =
            agree_channel(Role::Server, &server, &client.public_key(), PROTOCOL).unwrap();
        for (what, datagram) in [
            (
                "a substituted server key",
                sealed(Role::Client, &client, intruder.public_key(), PROTOCOL),
            ),
            (
                "both sides the client",
                sealed(Role::Client, &server, client.public_key(), PROTOCOL),
            ),
            (
                "another protocol id",
                sealed(Role::Client, &client, server.public_key(), PROTOCOL + 1),
            ),
        ] {
            assert_eq!(
                server_opener.open(&datagram),
                Err(OpenError::Forged),
                "{what}"
            );
        }
        let honest = sealed(Role::Client, &client, server.public_key(), PROTOCOL);
        assert_eq!(server_opener.open(&honest).unwrap(), b"packet");
    }

    /// Small-order public keys (RFC 7748 §7): zero and one among them. Each
    /// makes the X25519 output all zeros, and each is refused.
    #[test]
    fn a_small_order_public_key_is_refused() {
        let mut one = [0u8; 32];
        one[0] = 1;
        for peer in [[0u8; 32], one] {
            assert_eq!(
                agree_channel(Role::Client, &client(), &peer, PROTOCOL).map(|_| ()),
                Err(KeyAgreementError::NonContributory)
            );
        }
        assert!(agree_channel(Role::Client, &client(), &server().public_key(), PROTOCOL).is_ok());
    }

    #[test]
    fn debug_prints_the_public_key_and_not_the_secret() {
        let secret = [0xAB; 32];
        let pair = KeyPair::from_secret_bytes(secret);
        let printed = format!("{pair:?}");
        assert!(
            printed.contains(&format!("{:?}", pair.public_key())),
            "{printed}"
        );
        assert!(!printed.contains("171, 171, 171"), "{printed}");
    }
}
