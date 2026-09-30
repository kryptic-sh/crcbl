//! HKDF-SHA256 (RFC 5869), and the HMAC-SHA256 it is built on.
//!
//! The Noise Protocol Framework's `HKDF(chaining_key, input_key_material,
//! num_outputs)` (§4.3) is this construction with an empty `info`: its
//! `temp_key` is RFC 5869's pseudorandom key, and its outputs are the expand
//! step's `T(1)`, `T(2)`, … — `HMAC(temp_key, output1 || 0x02)` is `T(2)` with
//! nothing between the previous block and the counter. [`super::keys`] uses it
//! exactly that way. The RFC 5869 Appendix A SHA-256 vectors pin both halves,
//! the empty-salt, empty-info case among them.
//!
//! The primitives are RustCrypto's `hmac` and `sha2`; what is written here is
//! the RFC's loop and nothing else. [`crate::auth`]'s MAC is built on the same
//! [`HmacSha256`].

use hmac::digest::zeroize::Zeroizing;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// HMAC keyed with SHA-256 (RFC 2104).
pub(crate) type HmacSha256 = Hmac<Sha256>;

/// SHA-256's output length, RFC 5869's `HashLen`.
pub(crate) const HASH_BYTES: usize = 32;

/// The longest output one expand may produce: RFC 5869 §2.3 caps `L` at
/// `255 * HashLen`, because the block counter is one octet.
const MAX_EXPAND_BYTES: usize = 255 * HASH_BYTES;

/// HMAC-SHA256 of the concatenation of `parts`, keyed with `key`.
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; HASH_BYTES] {
    let mut mac = keyed(key);
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// An HMAC-SHA256 instance keyed with `key`, for a caller that verifies
/// rather than computes.
pub(crate) fn keyed(key: &[u8]) -> HmacSha256 {
    // HMAC hashes a key longer than its block and pads a shorter one, so no
    // length is invalid; `new_from_slice` is fallible only for the MACs whose
    // key size is fixed.
    HmacSha256::new_from_slice(key).expect("HMAC takes a key of any length")
}

/// RFC 5869 §2.2, `HKDF-Extract(salt, IKM)`.
///
/// An empty `salt` is the RFC's "not provided": HMAC pads its key with zeros
/// to the block size, so it keys identically to the `HashLen` zeros the RFC
/// substitutes.
pub(crate) fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Zeroizing<[u8; HASH_BYTES]> {
    Zeroizing::new(hmac_sha256(salt, &[ikm]))
}

/// RFC 5869 §2.3, `HKDF-Expand(PRK, info, L)`, with `L` the length of `okm`.
///
/// # Panics
///
/// If `okm` is longer than RFC 5869 allows. Every caller asks for a fixed
/// length far inside it.
pub(crate) fn hkdf_expand(prk: &[u8; HASH_BYTES], info: &[u8], okm: &mut [u8]) {
    assert!(
        okm.len() <= MAX_EXPAND_BYTES,
        "HKDF-Expand output of {} bytes exceeds RFC 5869's {MAX_EXPAND_BYTES}",
        okm.len()
    );
    let mut previous = Zeroizing::new([0u8; HASH_BYTES]);
    for (index, chunk) in okm.chunks_mut(HASH_BYTES).enumerate() {
        let counter = u8::try_from(index + 1).expect("the length check bounds the block count");
        // T(0) is the empty string.
        let before: &[u8] = if index == 0 { &[] } else { &previous[..] };
        let block = Zeroizing::new(hmac_sha256(prk, &[before, info, &[counter]]));
        chunk.copy_from_slice(&block[..chunk.len()]);
        *previous = *block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seal::tests::hex;

    /// One RFC 5869 Appendix A case: extract, then expand to the case's `L`.
    fn check(ikm: &str, salt: &str, info: &str, prk: &str, okm: &str) {
        let got_prk = hkdf_extract(&hex(salt), &hex(ikm));
        assert_eq!(got_prk[..], hex(prk)[..], "PRK");
        let expected = hex(okm);
        let mut got = vec![0u8; expected.len()];
        hkdf_expand(&got_prk, &hex(info), &mut got);
        assert_eq!(got, expected, "OKM");
    }

    /// RFC 5869 Appendix A.1: basic test case with SHA-256.
    #[test]
    fn hkdf_matches_rfc5869_test_case_1() {
        check(
            "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b",
            "000102030405060708090a0b0c",
            "f0f1f2f3f4f5f6f7f8f9",
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5",
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf\
             34007208d5b887185865",
        );
    }

    /// RFC 5869 Appendix A.2: longer inputs and outputs.
    #[test]
    fn hkdf_matches_rfc5869_test_case_2() {
        check(
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\
             202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f\
             404142434445464748494a4b4c4d4e4f",
            "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f\
             808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f\
             a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
            "b0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecf\
             d0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeef\
             f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
            "06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244",
            "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
             59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
             cc30c58179ec3e87c14c01d5c1f3434f1d87",
        );
    }

    /// RFC 5869 Appendix A.3: zero-length salt and info — the shape Noise's
    /// `HKDF` has.
    #[test]
    fn hkdf_matches_rfc5869_test_case_3() {
        check(
            "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b",
            "",
            "",
            "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04",
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
             9d201395faa4b61a96c8",
        );
    }

    #[test]
    #[should_panic(expected = "exceeds RFC 5869")]
    fn an_expand_past_the_rfc_limit_is_refused() {
        let mut okm = vec![0u8; MAX_EXPAND_BYTES + 1];
        hkdf_expand(&[0; HASH_BYTES], b"", &mut okm);
    }
}
