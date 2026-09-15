//! Water as engine data: a body of water, the medium it is made of, and the
//! mesh its surface is drawn with.
//!
//! `docs/plan/55-water.md`'s first decision is that **water is an engine system,
//! not a sample's shader**. A body is data three consumers read — the renderer
//! draws it, the physics world floats things on it, and a server that links no
//! renderer answers questions about it — so the data lives here, in a crate that
//! opens no device, and `crcbl_render`'s `water` module is one reader of it.
//!
//! ```text
//! WaterBody     an outline on the XZ plane, a level, and a Medium
//! Medium        per-channel absorption and scattering, in 1/m
//! surface_mesh  the body's surface as a flat grid clipped to its outline
//! ```
//!
//! # What this rung has, and what it does not
//!
//! Rung 1 of that document's ladder: a **still** body. The surface is the plane
//! `y = level` inside the outline, and the mesher lays a grid over it so that a
//! later rung's waves have interior vertices to displace. There is no kind enum
//! yet — a still body is the only kind — and no wave field, flow, shore cook or
//! query; each arrives with the rung that first reads it.
//!
//! # Coordinates
//!
//! Metres, in the engine's right-handed, `+Y`-up world. An outline point
//! `[x, z]` is the world point `(x, level, z)`.

mod body;
mod mesh;

pub use body::{BodyError, Medium, WaterBody};
pub use mesh::{MAX_GRID_CELLS, SurfaceMesh, surface_mesh};
