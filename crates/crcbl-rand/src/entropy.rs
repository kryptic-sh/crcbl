//! The secure seed source, and — on `wasm32` — the host bridge that seeds it.
//!
//! # The `wasm32` seed bridge
//!
//! `wasm32` has no ambient OS entropy an engine with a no-imports ABI can reach,
//! so the JS host pushes a 32-byte seed *in* through exports, mirroring the
//! `__crcbl_web_*` input pattern the storage layer uses (write into a wasm
//! buffer, then call a commit export):
//!
//! 1. `const p = ex.__crcbl_web_seed_entropy_ptr();` — the address of a 32-byte
//!    input buffer in wasm memory.
//! 2. Fill all 32 bytes from a secure host source, e.g.
//!    `crypto.getRandomValues(new Uint8Array(memory.buffer, p, 32));`.
//! 3. `ex.__crcbl_web_seed_entropy();` — reads those bytes, seeds the CSPRNG,
//!    zeroes the input buffer, and returns `1` (seeded) or `0` (already seeded).
//!
//! Seeding is **idempotent**: once seeded, a second call is refused, so a page
//! cannot weaken an established stream by re-seeding it. Until step 3 succeeds,
//! [`entropy`] fails closed with [`Error::Unseeded`].

use crate::Error;

/// Fill `dest` with secure, unpredictable bytes.
///
/// # Errors
///
/// - Native: an [`Error::Os`] if the operating system's CSPRNG fails.
/// - `wasm32`: [`Error::Unseeded`] if the host has not seeded the source yet
///   (see the [crate docs](crate) for the seed bridge).
///
/// [`Error::Os`]: crate::Error::Os
#[cfg(not(target_arch = "wasm32"))]
pub fn entropy(dest: &mut [u8]) -> Result<(), Error> {
    getrandom::fill(dest).map_err(Error::from)
}

/// Fill `dest` with secure, unpredictable bytes.
///
/// # Errors
///
/// [`Error::Unseeded`] if the host has not seeded the source yet (see the
/// [crate docs](crate) for the seed bridge).
#[cfg(target_arch = "wasm32")]
pub fn entropy(dest: &mut [u8]) -> Result<(), Error> {
    ENTROPY.with(|cell| fill_from(&mut cell.borrow_mut(), dest))
}

// The seeded/unseeded core of the `wasm32` path, factored out so its
// fail-closed behaviour is unit-testable on any target — the `test` cfg keeps
// it (and its imports) compiled into the native test build, where the
// `fail_closed` test drives it directly with `None` and `Some` states, while an
// ordinary native build leaves it out entirely.
#[cfg(any(target_arch = "wasm32", test))]
use rand_chacha::ChaCha20Rng;

/// Draw `dest.len()` bytes from `state`, or fail closed if it is unseeded.
#[cfg(any(target_arch = "wasm32", test))]
fn fill_from(state: &mut Option<ChaCha20Rng>, dest: &mut [u8]) -> Result<(), Error> {
    use rand_core::RngCore;

    match state {
        Some(rng) => {
            rng.fill_bytes(dest);
            Ok(())
        }
        None => Err(Error::unseeded()),
    }
}

/// Seed `state` from `seed`, unless it is already seeded.
///
/// Returns whether this call did the seeding: `true` the first time, `false`
/// for every idempotent no-op after — which is what the export reports so the
/// host can tell a fresh seed from a rejected re-seed.
#[cfg(any(target_arch = "wasm32", test))]
fn seed_into(state: &mut Option<ChaCha20Rng>, seed: [u8; 32]) -> bool {
    use rand_core::SeedableRng;

    if state.is_some() {
        return false;
    }
    *state = Some(ChaCha20Rng::from_seed(seed));
    true
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{ChaCha20Rng, seed_into};
    use core::cell::RefCell;

    thread_local! {
        /// The host-seeded CSPRNG the `wasm32` [`entropy`](super::entropy) draws
        /// from. `None` until seeded — the fail-closed state. Thread-local
        /// because `wasm32` is single-threaded and the engine, the shim and the
        /// draws all run on that one thread.
        pub(super) static ENTROPY: RefCell<Option<ChaCha20Rng>> = const { RefCell::new(None) };

        /// The 32-byte buffer JS writes a seed into before calling
        /// [`__crcbl_web_seed_entropy`]. Its address is
        /// [`__crcbl_web_seed_entropy_ptr`]; it is zeroed once consumed.
        static SEED_INPUT: RefCell<[u8; 32]> = const { RefCell::new([0u8; 32]) };
    }

    /// Seed the process-wide `wasm32` entropy CSPRNG from `seed`.
    ///
    /// Idempotent: the first call seeds it, later calls are no-ops, so a page
    /// cannot weaken an established stream by re-seeding it.
    pub fn seed_entropy(seed: [u8; 32]) {
        ENTROPY.with(|cell| {
            seed_into(&mut cell.borrow_mut(), seed);
        });
    }

    /// The address of the 32-byte seed input buffer for JS to write into.
    ///
    /// Stable for the module's life. Write exactly 32 bytes of secure host
    /// entropy there, then call [`__crcbl_web_seed_entropy`].
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_seed_entropy_ptr() -> *mut u8 {
        SEED_INPUT.with(|buf| buf.borrow_mut().as_mut_ptr())
    }

    /// Consume the 32 bytes in the seed input buffer: seed the CSPRNG, zero the
    /// buffer, and report the outcome.
    ///
    /// Returns `1` if this call seeded the source, `0` if it was already seeded
    /// (an idempotent no-op). The buffer is zeroed either way, so a rejected
    /// re-seed leaves no seed material sitting in memory.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_seed_entropy() -> u32 {
        let seed = SEED_INPUT.with(|buf| *buf.borrow());
        let seeded = ENTROPY.with(|cell| seed_into(&mut cell.borrow_mut(), seed));
        SEED_INPUT.with(|buf| buf.borrow_mut().fill(0));
        u32::from(seeded)
    }
}

#[cfg(target_arch = "wasm32")]
use wasm::ENTROPY;
#[cfg(target_arch = "wasm32")]
pub use wasm::{__crcbl_web_seed_entropy, __crcbl_web_seed_entropy_ptr, seed_entropy};

#[cfg(test)]
mod tests {
    use super::{entropy, fill_from, seed_into};
    use rand_chacha::ChaCha20Rng;
    use rand_core::{RngCore, SeedableRng};

    // Native OS entropy fills, and two successive draws differ (probabilistic,
    // as any randomness test is — a fixed collision here is astronomically
    // unlikely and would itself be the bug worth failing on).
    #[test]
    fn native_entropy_fills_and_varies() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        entropy(&mut a).expect("OS entropy should be available");
        entropy(&mut b).expect("OS entropy should be available");
        assert_ne!(a, b, "two OS entropy draws should differ");
    }

    // The security invariant, unit-tested on native: the wasm entropy core
    // fails closed when unseeded and yields bytes once seeded. If `fill_from`
    // ever stopped erroring on `None`, this test goes red — that is the whole
    // point of erroring instead of returning zeros.
    #[test]
    fn fail_closed_until_seeded() {
        let mut state: Option<ChaCha20Rng> = None;
        let mut buf = [0u8; 32];

        // Unseeded: refused.
        assert!(
            fill_from(&mut state, &mut buf).is_err(),
            "an unseeded source must fail, never fill"
        );
        assert_eq!(buf, [0u8; 32], "a refused draw must not touch the buffer");

        // Seeding takes the first time only (idempotent).
        assert!(seed_into(&mut state, [7u8; 32]), "first seed should take");
        assert!(
            !seed_into(&mut state, [9u8; 32]),
            "a second seed must be refused"
        );

        // Seeded: fills, and with non-zero bytes distinct from the zero start.
        fill_from(&mut state, &mut buf).expect("a seeded source must fill");
        assert_ne!(buf, [0u8; 32], "a seeded draw must produce bytes");

        // The rejected re-seed did not swap the stream: the state is still the
        // one seeded with `[7; 32]`, so its output matches a fresh generator on
        // that seed continued past the 32 bytes already drawn.
        let mut reference = ChaCha20Rng::from_seed([7u8; 32]);
        let mut skip = [0u8; 32];
        reference.fill_bytes(&mut skip);
        let mut want = [0u8; 32];
        reference.fill_bytes(&mut want);
        let mut got = [0u8; 32];
        fill_from(&mut state, &mut got).expect("seeded source fills");
        assert_eq!(got, want, "the [7; 32] stream must survive the re-seed");
    }
}
