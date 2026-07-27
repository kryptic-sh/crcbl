//! Core primitives: typed handles, slot pools, sector-tiled world positions, frame clock, logging.
//!
//! Placeholder crate — the workspace skeleton lands before the code so later
//! slices add implementations, not structure.

#[cfg(test)]
mod tests {
    /// PLACEHOLDER: replaced in P0.2 by the handle/pool and `WorldPos` rebase
    /// property tests listed in `docs/plan/12-testing.md`.
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "crcbl-core");
    }
}
