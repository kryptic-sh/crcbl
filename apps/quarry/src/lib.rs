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
//! Milestone 1 of four, and only its first half: [`face`] generates the
//! content. Nothing renders yet. The generator is separable from the renderer
//! and is what every later milestone stands on, so it is worth having proved on
//! its own — including that `crcbl_scene::build_meshlets` accepts it, which is
//! the whole premise of the sample.
//!
//! Still owed, from that document's milestones: the `MeshShader` path drawing
//! it, the QEM cluster hierarchy with per-cluster selection and hysteresis, the
//! two indirect paths and the forced-path comparison, and the skinned and
//! tiling cases with a Pages demo.

pub mod face;
pub mod scene;
