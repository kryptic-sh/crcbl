//! The portability proof for [`crcbl_rand::Rng`].
//!
//! These golden arrays were computed once on the author's machine and pasted in.
//! The tests are deliberately **not** `#[cfg]`-gated per platform: the same
//! arrays run on Linux, macOS, Windows and `wasm32` in CI, so any target whose
//! ChaCha8 output differs by even one byte turns its job red. That is what makes
//! "a seed yields byte-identical output everywhere" a checked fact rather than a
//! claim. If a rand_chacha upgrade ever changes the stream, these fail loudly
//! and the golden values must be regenerated deliberately, not silently.

use crcbl_rand::Rng;

// First eight `next_u64` draws from the all-`0x42` seed.
const SEED42_GOLDEN: [u64; 8] = [
    0x436D_536F_35E3_D7E5,
    0xD87D_6633_B898_DF08,
    0x7428_5337_AB45_A42C,
    0x5A26_BFCA_238A_07E2,
    0x32B9_2AC5_0734_9409,
    0x8AAD_60E9_B373_2EF6,
    0x4F2E_F6A7_BEA4_E078,
    0x385A_21E1_E735_E151,
];

// First eight `next_u64` draws from the `u64` seed `0xC0FFEE`, expanded through
// rand_core's portable `seed_from_u64`.
const U64_GOLDEN: [u64; 8] = [
    0xF1FA_9BD0_17FE_C535,
    0xA649_F284_22FB_4126,
    0x38C3_4F24_1349_E365,
    0x211C_ABA0_AE44_4B5E,
    0x884D_6896_9410_E74E,
    0x930D_BECB_24C5_759B,
    0x65B9_7B00_9CAB_7CA6,
    0x42C7_DBAE_21EE_7C3A,
];

#[test]
fn from_seed_is_portable_and_deterministic() {
    let mut rng = Rng::from_seed([0x42; 32]);
    let got: [u64; 8] = std::array::from_fn(|_| rng.next_u64());
    assert_eq!(got, SEED42_GOLDEN);
}

#[test]
fn from_u64_is_portable_and_deterministic() {
    let mut rng = Rng::from_u64(0xC0FFEE);
    let got: [u64; 8] = std::array::from_fn(|_| rng.next_u64());
    assert_eq!(got, U64_GOLDEN);
}
