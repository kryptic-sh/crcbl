//! quarry on a device — `docs/plan/sample/14-quarry.md`'s milestones, asserted
//! rather than looked at.
//!
//! ```text
//! CRCBL_GPU=vk cargo test -p quarry --test device
//! ```
//!
//! # Which backend
//!
//! `Null` unless `CRCBL_GPU` names another, which is `apps/viewer`'s rule and
//! for its reason: the recording backend runs everywhere, so this is covered in
//! every job on every machine, and pinning a real one turns it into evidence
//! about a driver. An unparseable name panics rather than falling back — a run
//! that quietly substituted `Null` would be a green result about a backend
//! nobody asked for.
//!
//! **`Null` draws nothing, and these tests say so rather than skipping.** A
//! frame is recorded there in full — every pass, every barrier, the draw itself
//! — and that is what is asserted; the pixels and the per-cluster cut are
//! asserted wherever there are pixels and a stage that records one. Which of the
//! two ran is printed, because "not supported here" reported as "passed" is the
//! shape this repo keeps removing.

// The suite's areas, one module each, all in `tests/device/`. The root is
// `tests/device/main.rs`, so Cargo compiles the directory as one test binary
// named `device` and every `mod` here resolves beside the root.
mod dolly;
mod goldens;
mod paths;
mod residency;
mod shading;

// The fixture and the two readbacks, in a file of their own because every area
// opens the same ring and measures a frame the same two ways.
mod harness;
