//! Quarry — the geometry acceptance fixture.
//!
//! `docs/plan/sample/14-quarry.md`, phase S4C. One dense scene drawn on every
//! [`GeometryPath`](crcbl::hal::GeometryPath), with the cluster hierarchy made
//! visible. Not a game: the geometry is the content.
//!
//! Where `apps/lumen` proves the two lighting paths agree, this proves the
//! three geometry paths do — and it is the only place the QEM generator's
//! output is *looked at* rather than measured, because an error metric can be
//! inside its budget while the mesh is visibly wrong at a seam.
//!
//! # What is here so far
//!
//! Milestone 1, and the first half of milestone 2. [`face`] generates the
//! content, [`scene`] describes it as one flat mesh and [`dag`] as a cluster
//! hierarchy, and `tests/residency.rs` draws both through the real renderer on
//! an offscreen context — so the `MeshShader` path rendering the scene is
//! asserted rather than looked at.
//!
//! Per-cluster selection over that hierarchy is asserted too: the face draws
//! from more than one level at once, and no level dominates the cut.
//!
//! The fixed dolly runs too, on one renderer so that hysteresis is in play, and
//! detail measurably arrives as the camera closes without the cut jumping.
//!
//! Still owed, from that document's milestones: the LOD tint and heatmap
//! overlays; the two indirect paths and the forced-path comparison; and the
//! skinned and tiling cases with a Pages demo. There is no window, so there are
//! no golden frames yet.

pub mod dag;
pub mod face;
pub mod scene;
