//! Workspace smoke test.
//!
//! PLACEHOLDER: this exists so the `nextest` job is not vacuous while the
//! engine crates are still empty. Later slices replace it with the real
//! integration suites described in `docs/plan/12-testing.md` (NullBackend
//! graph-compile, sim e2e, golden images).

// Force a real link against the umbrella crate — without this the test binary
// never references `crcbl` and the assertions below only describe themselves.
use crcbl as _;

#[test]
fn umbrella_crate_links() {
    assert_eq!(env!("CARGO_PKG_NAME"), "crcbl");
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
}
