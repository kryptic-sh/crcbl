//! FNV-1a: a hash whose output is the same on every target, toolchain and run.
//!
//! For an identifier derived from a name and written to a save or sent over
//! the wire, and for a digest pinned in a test. A `std` [`Hasher`] promises
//! neither: `DefaultHasher`'s algorithm may change between Rust releases, and
//! `RandomState` is seeded per process. FNV-1a is defined over bytes by its
//! reference implementation, so the same bytes give the same value anywhere.
//!
//! It is not a defence against an adversary who picks the input — a collision
//! is easy to find on purpose — so a caller whose ids an opponent chooses
//! checks for collisions, as the server does for replicated system ids.

use core::hash::Hasher;

/// FNV-1a's 32-bit offset basis: the hash of no bytes.
pub const FNV1A_32_OFFSET_BASIS: u32 = 0x811c_9dc5;

/// FNV-1a's 32-bit prime.
pub const FNV1A_32_PRIME: u32 = 0x0100_0193;

/// FNV-1a's 64-bit offset basis: the hash of no bytes.
pub const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a's 64-bit prime.
pub const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The 32-bit FNV-1a hash of `bytes`: from the offset basis, each byte XORed
/// in and the result multiplied by the prime, wrapping.
#[must_use]
pub const fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash = FNV1A_32_OFFSET_BASIS;
    let mut at = 0;
    while at < bytes.len() {
        hash = (hash ^ bytes[at] as u32).wrapping_mul(FNV1A_32_PRIME);
        at += 1;
    }
    hash
}

/// The 64-bit FNV-1a hash of `bytes`, as [`fnv1a_32`] is the 32-bit one.
/// [`Fnv1a64`] gives the same value for bytes written to it in pieces.
#[must_use]
pub const fn fnv1a_64(bytes: &[u8]) -> u64 {
    fnv1a_64_from(FNV1A_64_OFFSET_BASIS, bytes)
}

/// `bytes` folded into the 64-bit FNV-1a state `hash`.
const fn fnv1a_64_from(mut hash: u64, bytes: &[u8]) -> u64 {
    let mut at = 0;
    while at < bytes.len() {
        hash = (hash ^ bytes[at] as u64).wrapping_mul(FNV1A_64_PRIME);
        at += 1;
    }
    hash
}

/// The 64-bit FNV-1a hash as a [`Hasher`], for a digest built up in pieces or
/// through [`Hash`](core::hash::Hash) impls.
///
/// [`finish`](Hasher::finish) is [`fnv1a_64`] of every byte written so far, in
/// order. The bytes a `Hash` impl writes for an integer are its native-endian
/// bytes, and a `str` or slice also writes a length or a terminator; write
/// [`to_le_bytes`](u64::to_le_bytes) yourself when the digest must also agree
/// between targets of different endianness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv1a64(u64);

impl Fnv1a64 {
    /// A hasher that has seen no bytes.
    #[must_use]
    pub const fn new() -> Self {
        Self(FNV1A_64_OFFSET_BASIS)
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Fnv1a64 {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = fnv1a_64_from(self.0, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The reference vectors**, from the FNV reference implementation's
    /// test suite: no bytes is the offset basis, and `"a"` and `"foobar"`.
    #[test]
    fn the_hashes_match_the_published_vectors() {
        assert_eq!(fnv1a_32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a_32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a_32(b"foobar"), 0xbf9c_f968);
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    /// **The hasher is the function, however the bytes arrive**: whole, in
    /// pieces, or none.
    #[test]
    fn the_hasher_agrees_with_the_function_across_pieces() {
        let mut hasher = Fnv1a64::new();
        assert_eq!(hasher.finish(), fnv1a_64(b""));
        hasher.write(b"foo");
        hasher.write(b"");
        hasher.write(b"bar");
        assert_eq!(hasher.finish(), fnv1a_64(b"foobar"));
        assert_eq!(Fnv1a64::default(), Fnv1a64::new());
    }
}
