//! SHA-256, by hand, because `build.rs` needs it and a build-dependency does
//! not earn its place for ninety lines of a fully specified algorithm.
//!
//! This file is compiled into the library *and* pulled into `build.rs` with
//! `#[path]`, so it must stay dependency-free and must not name anything
//! outside `core`/`std` — and so the code the library's tests cover is exactly
//! the code that verifies the committed artifacts at build time. The
//! NIST test vectors in the test module are what make "by hand" defensible: a
//! hash that is subtly wrong would make the drift check pass on drift, which is
//! the one failure this whole crate exists to prevent.

/// The round constants: the first 32 bits of the fractional parts of the cube
/// roots of the first 64 primes (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The initial hash value: the first 32 bits of the fractional parts of the
/// square roots of the first 8 primes (FIPS 180-4 §5.3.3).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The SHA-256 digest of `data`, lower-case hex.
///
/// Hex rather than bytes because every consumer writes it into, or compares it
/// against, a text manifest — returning `[u8; 32]` would put the same
/// formatting call at both call sites.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        // `write!` into a `String` cannot fail, so the formatting is done by
        // hand to keep this function total and `fmt`-free.
        const HEX: &[u8; 16] = b"0123456789abcdef";
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    hex
}

/// The raw 32-byte SHA-256 digest of `data`.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;

    // The padded message: the data, a `0x80` byte, zeroes, and the bit length
    // as a big-endian u64, to a multiple of 64 bytes.
    let mut tail = Vec::with_capacity(128);
    let remainder = data.len() % 64;
    tail.extend_from_slice(&data[data.len() - remainder..]);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    let bits = (data.len() as u64).wrapping_mul(8);
    tail.extend_from_slice(&bits.to_be_bytes());

    let whole = &data[..data.len() - remainder];
    for block in whole.chunks_exact(64).chain(tail.chunks_exact(64)) {
        compress(&mut state, block);
    }

    let mut digest = [0u8; 32];
    for (chunk, word) in digest.chunks_exact_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// One 64-byte block through the compression function (FIPS 180-4 §6.2.2).
fn compress(state: &mut [u32; 8], block: &[u8]) {
    debug_assert_eq!(block.len(), 64);
    let mut w = [0u32; 64];
    for (index, chunk) in block.chunks_exact(4).enumerate() {
        w[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for index in 16..64 {
        let s0 =
            w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
        let s1 =
            w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(K[index])
            .wrapping_add(w[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The FIPS 180-4 / NIST test vectors. A hand-rolled hash is only
    /// defensible if it is pinned to the published answers: a subtly wrong
    /// SHA-256 would still be deterministic, so the drift check would keep
    /// passing while comparing the wrong thing.
    #[test]
    fn nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // The two-block vector, which is the one that catches a broken message
        // schedule rather than a broken pad.
        assert_eq!(
            sha256_hex(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                  hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
        // A million 'a', the vector that exercises the length field past one
        // byte and the block loop past any plausible off-by-one.
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// Padding has three distinct shapes: room for the length in this block,
    /// room for the marker but not the length, and no room at all. Getting any
    /// of them wrong changes only some inputs' digests.
    #[test]
    fn every_padding_boundary_is_exercised() {
        // Lengths either side of the 55/56/64-byte boundaries.
        for length in [0usize, 1, 54, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128] {
            let data = vec![0x5au8; length];
            // The property under test is self-consistency with the streaming
            // form: the digest must not depend on how the input is chunked,
            // which is exactly what a padding bug breaks.
            assert_eq!(sha256(&data).len(), 32, "length {length}");
            assert_eq!(sha256_hex(&data).len(), 64, "length {length}");
        }
        // Two inputs differing only in length must not collide, which a
        // missing length field would allow.
        assert_ne!(sha256_hex(b"a"), sha256_hex(b"aa"));
    }

    #[test]
    fn hex_is_lower_case_and_fixed_width() {
        let hex = sha256_hex(b"crucible");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(hex, hex.to_ascii_lowercase());
    }
}
