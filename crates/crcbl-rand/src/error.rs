//! The error type for the entropy source.

use core::fmt;

/// Why an [`entropy`](crate::entropy) draw failed.
///
/// The variants are target-specific because the two entropy sources are:
/// [`Os`](Error::Os) exists only where there is an OS CSPRNG to fail, and
/// [`Unseeded`](Error::Unseeded) only on `wasm32`, where the seed arrives from
/// the host and may not have yet.
#[derive(Debug)]
pub enum Error {
    /// The operating system's entropy source returned an error. Native targets
    /// only; carries the underlying [`getrandom::Error`].
    #[cfg(not(target_arch = "wasm32"))]
    Os(getrandom::Error),

    /// The `wasm32` entropy source was drawn before the host seeded it.
    ///
    /// This is the **fail-closed** state: a draw here refuses rather than
    /// returning a predictable value, so nothing derives a secret from a source
    /// that carries none. The host seeds it through the
    /// `__crcbl_web_seed_entropy` export (see the crate-level docs).
    Unseeded,
}

impl Error {
    /// The [`Unseeded`](Error::Unseeded) error.
    ///
    /// A named constructor so the `wasm32` entropy path — and the native test
    /// that exercises the same fail-closed logic — build it the same way the OS
    /// path builds its error through [`From`].
    pub fn unseeded() -> Self {
        Error::Unseeded
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Error::Os(source) => write!(f, "OS entropy source failed: {source}"),
            Error::Unseeded => f.write_str(
                "wasm entropy source is not seeded; the host must call \
                 __crcbl_web_seed_entropy before entropy is drawn",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Error::Os(source) => Some(source),
            Error::Unseeded => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<getrandom::Error> for Error {
    fn from(source: getrandom::Error) -> Self {
        Error::Os(source)
    }
}
