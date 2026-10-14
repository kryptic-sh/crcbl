//! [`PhysicsSystem::debug_lines`]: the physics debug view as line segments,
//! for a debug draw layer to show.
//!
//! The crate depends on no renderer, and [`DebugCtx`](crcbl_ecs::DebugCtx)
//! carries nothing to draw into, so the system's
//! [`SystemTrait::debug_draw`](crcbl_ecs::SystemTrait::debug_draw) still draws
//! nothing: a caller asks for the segments instead and colours each by the
//! [`PhysicsDebugLine`] it comes with.

use glam::DVec3;

use super::{BodySet, PhysicsSystem};

/// What a segment from [`PhysicsSystem::debug_lines`] shows, for the caller
/// to colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysicsDebugLine {
    /// An edge of an entity's collider's bounding box in the world.
    Bounds {
        /// The body sleeps.
        sleeping: bool,
        /// Its island, as a number to colour by: one for every body the solver
        /// steps together, and reused once that island is gone. [`None`] for
        /// an entity in no island — one with no body, a body not yet stepped,
        /// and every body in a system without contacts.
        island: Option<u32>,
    },
    /// From a contact point, [`CONTACT_NORMAL_LENGTH`] along the contact's
    /// normal, which points from its lower-ranked shape into the other.
    Contact {
        /// The surfaces touch or overlap there, rather than standing near
        /// enough apart that the solver keeps the point in case they meet.
        touching: bool,
        /// The normal impulse the last step left on the point: its last
        /// substep's, so a body at rest carries its weight over one substep
        /// between its points.
        impulse: f64,
        /// The manifold feature the point belongs to, stable while the shapes
        /// stay in the same configuration: one that changes every frame on a
        /// still pile is warm starting lost.
        feature: u32,
    },
}

/// How long a drawn contact normal is, in metres.
pub const CONTACT_NORMAL_LENGTH: f64 = 0.1;

impl PhysicsSystem {
    /// Hands `line` every segment of the physics debug view as the last
    /// [`step`](Self::step) left it, each tagged with the [`PhysicsDebugLine`]
    /// it is: the twelve edges of each entity's collider's bounding box,
    /// telling sleeping bodies and islands apart, then each contact point's
    /// normal, with its impulse and its feature. A plane has no box drawn; a
    /// contact with one has its normal drawn as any other.
    pub fn debug_lines(&self, line: &mut impl FnMut(DVec3, DVec3, PhysicsDebugLine)) {
        for (_, record) in self.records.iter() {
            let Some(aabb) = record
                .collider
                .as_ref()
                .and_then(|(collider, _)| self.world.aabb_of(*collider))
            else {
                continue;
            };
            let kind = PhysicsDebugLine::Bounds {
                sleeping: record.set == BodySet::Sleeping,
                island: record.island,
            };
            let corner = |index: usize| {
                let pick = |bit: usize, axis: usize| {
                    if index & bit == 0 {
                        aabb.min[axis]
                    } else {
                        aabb.max[axis]
                    }
                };
                DVec3::new(pick(1, 0), pick(2, 1), pick(4, 2))
            };
            for index in 0..8 {
                for bit in [1, 2, 4] {
                    if index & bit == 0 {
                        line(corner(index), corner(index | bit), kind);
                    }
                }
            }
        }
        let Some(pipeline) = &self.contacts else {
            return;
        };
        for report in pipeline.reports(&self.records) {
            let normal = report.manifold.normal * CONTACT_NORMAL_LENGTH;
            for (point, &impulse) in report.manifold.points().iter().zip(&report.normal_impulses) {
                let kind = PhysicsDebugLine::Contact {
                    touching: point.separation <= 0.0,
                    impulse,
                    feature: point.id,
                };
                line(point.point, point.point + normal, kind);
            }
        }
    }
}
