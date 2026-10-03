//! Every edit the editor makes, as a value — and the log that walks them back.
//!
//! Moved to `crcbl::scene::edit`, beside the scene format it edits, so that
//! anything carrying a command reaches the one vocabulary without linking this
//! crate; re-exported here so the editor's own paths do not churn. That
//! module's docs hold the design.

pub use crcbl::scene::edit::*;
