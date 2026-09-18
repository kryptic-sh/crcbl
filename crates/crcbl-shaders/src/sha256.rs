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
    let mut tail = [0u8; 128];
    let remainder = data.len() % 64;
    tail[..remainder].copy_from_slice(&data[data.len() - remainder..]);
    tail[remainder] = 0x80;
    let tail_len = if remainder < 56 { 64 } else { 128 };
    let bits = (data.len() as u64).wrapping_mul(8);
    tail[tail_len - 8..tail_len].copy_from_slice(&bits.to_be_bytes());

    let whole = &data[..data.len() - remainder];
    for block in whole
        .chunks_exact(64)
        .chain(tail[..tail_len].chunks_exact(64))
    {
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
    fn sha256_matches_the_published_nist_test_vectors() {
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
        // Expected answers were generated independently with Python hashlib.
        for (length, expected) in [
            (
                0usize,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                1usize,
                "bbeebd879e1dff6918546dc0c179fdde505f2a21591c9a9c96e36b054ec5af83",
            ),
            (
                54usize,
                "bbc9cf58477cda4f72a4d20d1ae6a95241ac924c3f310b86c4c8fb903e89d1ff",
            ),
            (
                55usize,
                "5f25f149aa92e3e13093aed8216072fae623f35e26ca605b6cce17e04b7ccf44",
            ),
            (
                56usize,
                "301c69927f1603720c9f847b7e5e3bef77a7b9f75344490fe9039f13c36b842a",
            ),
            (
                57usize,
                "30ab35131f9b368e840dc65fc1eb832706e748e3c5e44ec40bc19cd1ce5c0dc2",
            ),
            (
                63usize,
                "939765b120205cbedae2ed31256b1967c38b6bdd9b0220535224cbc0b906d333",
            ),
            (
                64usize,
                "cc7321cce5e4409bd8077d58422e1214969059bbd40b4eeb0de0a642f40f7282",
            ),
            (
                65usize,
                "b8de0db62b6c87db61345504a8038bf973d987e8d2111abd8beb407c0bf3d9db",
            ),
            (
                119usize,
                "a96851d641310ce032ff832b6f08125878deed2a825fe515dd1ba414afe95f7e",
            ),
            (
                120usize,
                "60ec7f280e45d0c7bf77b70ff16958b1c1701a9fb7faa12b798207cf120ec6ee",
            ),
            (
                127usize,
                "f4651f880655488aadc1ea0287ef8954296d9e7487a642bd4800744e15ee3771",
            ),
            (
                128usize,
                "349d65e9ba1de7b0a13f9a3eadcc5b0202f15d6008fe9477f2a7b80f6194b20f",
            ),
        ] {
            let data = vec![0x5au8; length];
            let expected_bytes: [u8; 32] = std::array::from_fn(|index| {
                u8::from_str_radix(&expected[index * 2..index * 2 + 2], 16)
                    .expect("expected digest is hex")
            });
            assert_eq!(sha256(&data), expected_bytes, "length {length}");
            assert_eq!(sha256_hex(&data), expected, "length {length}");
        }
    }

    #[test]
    fn patterned_padding_and_whole_blocks_match_expected_digests() {
        // Expected answers were generated independently with Python hashlib.
        for (length, expected) in [
            (
                0usize,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                1usize,
                "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
            ),
            (
                54usize,
                "0b35cded48f546833dc4e93133ffc3d7e05c7e41842f4accc39c6e04204c8dde",
            ),
            (
                55usize,
                "2900465fcb533e05a158fd2b3be0e5e3b03740d83060aa3580e0d98a96bf2384",
            ),
            (
                56usize,
                "31454ff48ef36af2f08fd511bdc37d9d5855ac23e992e5ff5445cb6b7674a674",
            ),
            (
                57usize,
                "bcc0a5d3791b985b7550e04ca660a6c63a589ba1edd2283c8e110e5b515df124",
            ),
            (
                63usize,
                "5f6401b96532c36de4e65beec0409b69b1d181864c8009b7a04f43e5d56350d1",
            ),
            (
                64usize,
                "94eb5de4943613fd048dc93393ab06877405faa39c11f53e9386083339833e7e",
            ),
            (
                65usize,
                "fc518669b6eb4b4dd91827ecacef86689c725bd5bab888fd3b26dbb196eec954",
            ),
            (
                119usize,
                "b0dc41b1a384e2f1203f0351b38fbeaafceef577ce1191d5bfc25da39f721eae",
            ),
            (
                120usize,
                "5df24dd802ac26132ce608dcb5f09841eef039ee0f152acf98d26d17fe4e88e6",
            ),
            (
                121usize,
                "5ed5a129bb49444fe2585d785920135a02f64350edf57c5f3ce7605b86f39039",
            ),
            (
                127usize,
                "0fe729ff19257bd6fec853acc2ea355f6b34b58e6c0f684c3e188fcdfcd9baae",
            ),
            (
                128usize,
                "0aedd4856f8eba0963627336ad5144a9a7dbe12498e6066f0165fc97d8ddee4c",
            ),
            (
                129usize,
                "4f1757ae4bffbae86d775b831765b75af154d52f7deaa46dd378051a2d3ad57f",
            ),
            (
                183usize,
                "f02d75586ed6601b4df292c0621e19522bdd32ba4ecd709d2c3e04f612e76130",
            ),
            (
                184usize,
                "91e6c9af893df592ddfb4ff96f6a5a2c245235da64ff10c1ff58b5535e4fd836",
            ),
            (
                185usize,
                "b96cdf70048d716f9eab5e0b27bfb6ee029df2a5cc5a86708e84a7d51fe1e88c",
            ),
            (
                191usize,
                "af3693744312c85b48ece39c2b1826d4ce6cd8aa7ff0db2709f1e95b95364077",
            ),
            (
                192usize,
                "0dc88f7329df669b2e5baf95676c6a50dfb5c1f618c853eba2f9090ee6b03a0d",
            ),
            (
                193usize,
                "0310be0872eec1a03d7d96f571958490aa7372f42ad1e48ce30864694f5c0f6d",
            ),
            (
                255usize,
                "3c835ac0bba7147eaa568a76183d465e72ac456df24b55e01d44dc87be05a971",
            ),
            (
                256usize,
                "3ef33734daae0e353f132ff5f3241d8f86ba81f851c0b9685149f079c16eb45b",
            ),
            (
                257usize,
                "08570fb3dfab53bcb3e8f4ca60b7331d9c603460a45d978226e18dd019468d07",
            ),
            (
                511usize,
                "4c51b4e1960cc998269d0d784119827cb8bf0a5eb077905093cfd359003a21f1",
            ),
            (
                512usize,
                "08ac48e649b513d133de8324a7c75f166f3347490afcbe447e8df6debf09208b",
            ),
            (
                513usize,
                "e35cd430689e2ecbee05fe150b03307cf9c28520b4137e29f60229dee702deb1",
            ),
            (
                1024usize,
                "ffbad8f947474cfdd5b2bb22d7e0bf5ee8ba2b7af859d0c2bb28622db6a4be47",
            ),
        ] {
            let data: Vec<u8> = (0..length)
                .map(|index| (index as u8).wrapping_mul(37).wrapping_add(11))
                .collect();
            let expected_bytes: [u8; 32] = std::array::from_fn(|index| {
                u8::from_str_radix(&expected[index * 2..index * 2 + 2], 16)
                    .expect("expected digest is hex")
            });
            assert_eq!(sha256(&data), expected_bytes, "length {length}");
            assert_eq!(sha256_hex(&data), expected, "length {length}");
        }
    }

    #[test]
    fn hex_is_lower_case_and_fixed_width() {
        let hex = sha256_hex(b"crucible");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(hex, hex.to_ascii_lowercase());
    }
}
