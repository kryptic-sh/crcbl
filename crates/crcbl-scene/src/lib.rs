//! Scene format and glTF import.
//!
//! **This crate is empty on purpose and nothing depends on it.** The workspace
//! skeleton lands before the code so later slices add implementations rather
//! than structure — but a crate with no items and no dependents is also a build
//! target every `cargo build --workspace` compiles for nothing, so the tradeoff
//! is worth naming rather than leaving to be rediscovered.
//!
//! It stays because `docs/plan/01-foundations.md`'s workspace layout lists it,
//! because deleting and re-adding it would churn `Cargo.lock` and every
//! workspace manifest, and because the cost is one empty rlib. When the scene
//! phase starts, this file grows a `Scene` type and the crate stops being a
//! placeholder; if that phase is ever cut, this crate goes with it.
