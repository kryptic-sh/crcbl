//! The edit server: a document served over a host, every client's edit
//! applied through `Document::apply` and announced to the rest.
//!
//! Moved to `crcbl::scene_edit::serve` with the document it applies through,
//! so the CLI reaches the same server; re-exported here so the editor's own
//! paths do not churn. That module's docs hold the design.

pub use crcbl::scene_edit::serve::*;

#[cfg(test)]
mod tests;
