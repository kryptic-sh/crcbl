//! Crucible's one randomness seam.
//!
//! The workspace draws randomness from here and nowhere else, so the two very
//! different things "random" can mean stay separated and each is hard to use
//! wrong:
//!
//! - [`Rng`] is a **deterministic, portable** pseudo-random generator built on
//!   [`rand_chacha::ChaCha8Rng`]. A given seed produces the *same bytes* on
//!   Linux, macOS, Windows and `wasm32` — there are no platform-specific paths
//!   in it — which is what lets it drive procedural generation, noise and
//!   netcode that must agree across machines. It is **not** a source of secrets:
//!   anyone who learns the seed can reproduce the whole stream.
//! - [`entropy()`] is the **secure, unpredictable** seed source. On native
//!   targets
//!   it is the operating system's CSPRNG (via `getrandom`). On `wasm32` — where
//!   there is no ambient OS entropy an engine with a no-imports ABI may call —
//!   it is a host-seeded [`rand_chacha::ChaCha20Rng`] that **fails closed**:
//!   until the host has pushed 32 bytes of seed through the
//!   `__crcbl_web_seed_entropy` export, every draw returns [`Error::Unseeded`]
//!   rather than a guessable value.
//!
//! ChaCha8 backs [`Rng`] because it is fast and high-volume and its security
//! margin is irrelevant to a public seed; ChaCha20 backs the `wasm32` entropy
//! bridge because *there* it is the security-critical link and gets the full
//! margin.
//!
//! [`Rng`] implements [`rand_core::RngCore`] and [`rand_core::SeedableRng`], so
//! it plugs into the wider `rand` ecosystem. Range and distribution helpers are
//! intentionally not provided until a consumer needs them; [`RngCore`] is the
//! surface for now.
//!
//! [`RngCore`]: rand_core::RngCore

mod entropy;
mod error;
mod rng;

pub use entropy::entropy;
pub use error::Error;
pub use rng::Rng;

#[cfg(target_arch = "wasm32")]
pub use entropy::{__crcbl_web_seed_entropy, __crcbl_web_seed_entropy_ptr, seed_entropy};
