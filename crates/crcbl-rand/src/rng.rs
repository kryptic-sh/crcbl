//! The deterministic, portable generator.

use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

use crate::Error;
use crate::entropy::entropy;

/// A deterministic, portable pseudo-random generator.
///
/// Built on [`ChaCha8Rng`], so a given seed yields byte-identical output on
/// every target the workspace builds for — the property the `golden_*` tests
/// pin. Use it for anything that must agree across machines (procedural
/// generation, noise, deterministic netcode). It is reproducible from its seed
/// and therefore **not** a source of secrets; for those, seed it from
/// [`entropy`] via [`Rng::from_entropy`] or use [`entropy`] directly.
///
/// It implements [`RngCore`] and [`SeedableRng`], so `rand`'s distributions and
/// adapters accept it. Convenience range/distribution helpers are deliberately
/// omitted until a consumer needs them.
#[derive(Clone, Debug)]
pub struct Rng(ChaCha8Rng);

impl Rng {
    /// Construct from a full 32-byte seed. Equal seeds produce equal streams.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Rng(ChaCha8Rng::from_seed(seed))
    }

    /// Construct from a `u64`, expanded to a full seed by `rand_core`'s portable
    /// [`SeedableRng::seed_from_u64`]. Deterministic and portable, but only 64
    /// bits of state — convenient for tests and fixed-seed content, not secret.
    pub fn from_u64(seed: u64) -> Self {
        Rng(ChaCha8Rng::seed_from_u64(seed))
    }

    /// Construct from a fresh 32-byte draw of secure [`entropy`], giving an
    /// unpredictable stream.
    ///
    /// # Errors
    ///
    /// Propagates any [`Error`] from [`entropy`] — an OS failure on native, or
    /// [`Error::Unseeded`] on `wasm32` before the host has seeded it.
    pub fn from_entropy() -> Result<Self, Error> {
        let mut seed = [0u8; 32];
        entropy(&mut seed)?;
        Ok(Rng::from_seed(seed))
    }

    /// The next random `u32` from the stream.
    pub fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    /// The next random `u64` from the stream.
    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Fill `dest` entirely with bytes from the stream.
    pub fn fill(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
}

impl RngCore for Rng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.0.fill_bytes(dst);
    }
}

impl SeedableRng for Rng {
    type Seed = [u8; 32];

    fn from_seed(seed: Self::Seed) -> Self {
        Rng(ChaCha8Rng::from_seed(seed))
    }

    fn seed_from_u64(state: u64) -> Self {
        Rng(ChaCha8Rng::seed_from_u64(state))
    }
}
