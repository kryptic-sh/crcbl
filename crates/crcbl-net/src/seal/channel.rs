//! Sealing and opening one datagram: the wire format in [`super`]'s docs.

use chacha20poly1305::{AeadInOut, Tag, XChaCha20Poly1305, XNonce};

use crate::auth::ReplayWindow;
use crate::reliable::{MAX_DATAGRAM_BYTES, MAX_PACKET_BYTES};

use super::nonce::{Direction, nonce};
use super::{
    COUNTER_BYTES, FIRST_COUNTER, SEAL_OVERHEAD, SEAL_PREFIX_BYTES, SEALED_TAG, TAG_BYTES,
};

/// Why a packet was not sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SealError {
    /// Every counter this key has was used. The sealer never wraps, because a
    /// repeated counter is a repeated nonce; key a new channel.
    #[error("seal counter space exhausted; the channel must be rekeyed")]
    CounterExhausted,
    /// Longer than [`MAX_PACKET_BYTES`], so sealed it would not fit
    /// [`MAX_DATAGRAM_BYTES`]. No [`crate::reliable::Endpoint`] emits one.
    #[error("packet of {size} bytes exceeds the {limit}-byte limit")]
    Oversized { size: usize, limit: usize },
}

/// Why a datagram did not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The first byte is not [`SEALED_TAG`]: not a sealed datagram at all.
    #[error("datagram is not sealed")]
    NotSealed,
    /// Shorter than the seal's own framing.
    #[error("sealed datagram of {size} bytes is shorter than its framing")]
    TooShort { size: usize },
    /// Longer than [`MAX_DATAGRAM_BYTES`]; refused before any work is spent
    /// on it.
    #[error("datagram of {size} bytes exceeds the {limit}-byte limit")]
    Oversized { size: usize, limit: usize },
    /// The tag does not verify: forged, corrupted, sealed under another key or
    /// for the other direction.
    #[error("sealed datagram failed authentication")]
    Forged,
    /// Authentic, but its counter was already opened or is too far behind the
    /// newest for the replay window to say.
    #[error("replayed or out-of-window seal counter: {0}")]
    Replayed(u64),
}

/// One direction's sending half: a key, and the counter the next datagram
/// seals under.
///
/// `Debug` shows the direction and counter; the cipher's own `Debug` prints
/// no key.
#[derive(Debug)]
pub struct Sealer {
    cipher: XChaCha20Poly1305,
    direction: Direction,
    /// `None` once the last counter was used.
    next_counter: Option<u64>,
}

impl Sealer {
    pub(crate) fn new(cipher: XChaCha20Poly1305, direction: Direction) -> Self {
        Self {
            cipher,
            direction,
            next_counter: Some(FIRST_COUNTER),
        }
    }

    /// Seal `packet` — one datagram from
    /// [`crate::reliable::Endpoint::poll_outgoing`] — under the next counter.
    ///
    /// # Errors
    ///
    /// [`SealError::Oversized`] for a packet past [`MAX_PACKET_BYTES`], and
    /// [`SealError::CounterExhausted`] once every counter was used. Neither
    /// consumes a counter.
    pub fn seal(&mut self, packet: &[u8]) -> Result<Vec<u8>, SealError> {
        if packet.len() > MAX_PACKET_BYTES {
            return Err(SealError::Oversized {
                size: packet.len(),
                limit: MAX_PACKET_BYTES,
            });
        }
        let counter = self.next_counter.ok_or(SealError::CounterExhausted)?;
        let mut out = Vec::with_capacity(SEAL_OVERHEAD + packet.len());
        out.push(SEALED_TAG);
        out.extend_from_slice(&counter.to_le_bytes());
        out.extend_from_slice(packet);
        let (clear, body) = out.split_at_mut(SEAL_PREFIX_BYTES);
        let tag = self
            .cipher
            .encrypt_inout_detached(
                &XNonce::from(nonce(self.direction, counter)),
                clear,
                body.into(),
            )
            // The cipher refuses only a message past ChaCha20's 256 GiB
            // keystream, and the size check above holds this to a datagram.
            .expect("a packet within MAX_PACKET_BYTES is within the cipher's message limit");
        out.extend_from_slice(&tag);
        self.next_counter = counter.checked_add(1);
        Ok(out)
    }

    /// Start the counter at `counter`, so a test can reach the end of the
    /// space without sealing its way there.
    #[cfg(test)]
    pub(crate) fn with_next_counter(mut self, counter: u64) -> Self {
        self.next_counter = Some(counter);
        self
    }
}

/// One direction's receiving half: a key, and the replay window over the
/// counters already opened.
///
/// `Debug` shows the direction and window; the cipher's own `Debug` prints no
/// key.
#[derive(Debug)]
pub struct Opener {
    cipher: XChaCha20Poly1305,
    direction: Direction,
    window: ReplayWindow,
}

impl Opener {
    pub(crate) fn new(cipher: XChaCha20Poly1305, direction: Direction) -> Self {
        Self {
            cipher,
            direction,
            window: ReplayWindow::default(),
        }
    }

    /// Authenticate and decrypt `datagram`, returning the packet to hand
    /// [`crate::reliable::Endpoint::receive_datagram`].
    ///
    /// Total on arbitrary bytes and never panics. The replay window is
    /// consulted only **after** the tag verifies, so a datagram with a forged
    /// counter — however far ahead — changes nothing.
    ///
    /// # Errors
    ///
    /// An [`OpenError`] saying which check refused it.
    pub fn open(&mut self, datagram: &[u8]) -> Result<Vec<u8>, OpenError> {
        if datagram.len() > MAX_DATAGRAM_BYTES {
            return Err(OpenError::Oversized {
                size: datagram.len(),
                limit: MAX_DATAGRAM_BYTES,
            });
        }
        if datagram.first() != Some(&SEALED_TAG) {
            return Err(OpenError::NotSealed);
        }
        if datagram.len() < SEAL_OVERHEAD {
            return Err(OpenError::TooShort {
                size: datagram.len(),
            });
        }
        let (clear, sealed) = datagram.split_at(SEAL_PREFIX_BYTES);
        let (ciphertext, tag) = sealed.split_at(sealed.len() - TAG_BYTES);
        let counter_bytes: [u8; COUNTER_BYTES] = clear[1..]
            .try_into()
            .expect("the prefix is the tag byte then the counter");
        let counter = u64::from_le_bytes(counter_bytes);
        let tag = Tag::try_from(tag).expect("split at the tag's length");
        let mut packet = ciphertext.to_vec();
        self.cipher
            .decrypt_inout_detached(
                &XNonce::from(nonce(self.direction, counter)),
                clear,
                packet.as_mut_slice().into(),
                &tag,
            )
            .map_err(|_| OpenError::Forged)?;
        if !self.window.accept(counter) {
            return Err(OpenError::Replayed(counter));
        }
        Ok(packet)
    }
}
