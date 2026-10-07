//! Physical sweeps followed by clearance sweeps when the body itself misses.

use glam::DVec3;

use crate::collider::{Capsule, LyingCapsule};
use crate::world::{CapsuleSweepShape, ColliderId, SweptContact};

use super::{Body, CharacterController, MIN_MOVE, WorldReader};

pub(super) struct MotionHit {
    pub(super) collider: ColliderId,
    pub(super) hit: SweptContact,
    pub(super) clearance: Option<f64>,
}

impl CharacterController {
    /// Keep physical contacts unchanged; a miss must also clear the skin.
    pub(super) fn sweep_body(
        &self,
        world: &mut WorldReader<'_, '_>,
        body: Body,
        delta: DVec3,
    ) -> Option<MotionHit> {
        let from = self.position;
        let physical = match body {
            Body::Upright => self
                .sweep(world, from, from + delta)
                .map(|(id, hit)| (id, hit.into())),
            Body::Lying(lying) => world.view.sweep_lying_capsule(
                &LyingCapsule {
                    head: from,
                    ..lying
                },
                delta,
                self.filter(),
                world.scratch,
            ),
        };
        if let Some((collider, hit)) = physical {
            return Some(MotionHit {
                collider,
                hit,
                clearance: None,
            });
        }
        if self.config.skin_width == 0.0 {
            return None;
        }
        let envelope = match body {
            Body::Upright => CapsuleSweepShape::Upright(Capsule::new(
                from,
                self.config.radius + self.config.skin_width,
                self.config.half_height,
            )),
            Body::Lying(lying) => CapsuleSweepShape::Lying(LyingCapsule {
                head: from,
                radius: lying.radius + self.config.skin_width,
                ..lying
            }),
        };
        let (collider, hit) = world.view.sweep_capsule_shape(
            envelope,
            delta,
            self.filter(),
            world.scratch,
            |hit| {
                // Ground snapping owns the vertical skin gap on walkable
                // slopes; a radial back-off here would add downhill motion.
                delta.dot(hit.normal) < -MIN_MOVE
                    && !(self.is_grounded() && self.is_walkable(hit.normal))
            },
        )?;
        Some(MotionHit {
            collider,
            hit: hit.contact,
            clearance: Some(hit.penetration),
        })
    }
}
