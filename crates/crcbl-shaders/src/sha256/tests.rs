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
