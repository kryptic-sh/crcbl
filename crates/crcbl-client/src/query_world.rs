//! The client's read-only physics world: what a camera boom sweeps and an
//! occlusion ray is cast against on the client, which never steps physics.
//!
//! ```text
//!   scene load ──▶ statics (a PhysicsWorld) ──▶ ClientQueryWorld::new      once
//!
//!   each frame ──▶ Client::interpolate_snapping ──▶ ClientQueryWorld::follow
//!                       │                              │
//!                       │   entity first seen ──▶ shape_of(entity) ──▶ add
//!                       │   entity still there ──▶ placed at its pose
//!                       └── entity gone ──────────▶ collider removed
//!
//!   game code ──▶ cast_ray / sweep_sphere / overlap_sphere_into / _ids_into
//! ```
//!
//! # A view, not the truth
//!
//! **Every answer is about the past, and none of it is authoritative.** The
//! dynamic colliders stand where the interpolated replicas are drawn — behind
//! the server by the playout delay ([`crate::playout`]) — so a query here
//! agrees with the picture on screen, which is what a camera and a sound want,
//! and disagrees with the server, which is what nothing a game decides may
//! rest on. A hit that matters to the simulation is the server's to find.
//!
//! # Where the shapes come from
//!
//! The snapshots carry a transform per entity and nothing else
//! ([`crcbl_phys::PhysicsSystem`]'s replicated form), and that stays so: a
//! game already knows what each kind of entity it replicates is shaped like —
//! the archetype it spawned, the scene row it loaded — so the shape is asked
//! of the game, through `shape_of`, the first time an entity appears, and no
//! byte is added to the wire for it. An entity `shape_of` answers `None` for
//! has no collider here, and is asked again on each frame it is still there.
//!
//! # Placed as the server places them
//!
//! A collider is added and moved through
//! [`PhysicsWorld::add_collider`] and [`PhysicsWorld::place_collider`], the
//! two [`crcbl_phys::PhysicsSystem`] places its own colliders through, so the
//! same component at the same pose is the same shape on both sides and a query
//! over it answers the same.

use std::collections::HashMap;

use crcbl_net::Transport;
use crcbl_phys::{
    ColliderComponent, ColliderId, OverlapHit, PhysicsWorld, QueryFilter, QueryScratch, Ray,
    Segment, ShapeHit,
};
use glam::DVec3;

use crate::{Client, InterpolatedState};

/// A replicated entity's collider, and the shape it was added as.
#[derive(Debug)]
struct Replica {
    collider: ColliderId,
    shape: ColliderComponent,
    /// The [`ClientQueryWorld::frame`] the entity was last in the state on.
    seen: u64,
}

/// A physics world the client queries and never steps: the scene's statics,
/// and a collider on each replicated entity at its interpolated pose.
///
/// **Read-only to game code.** What it holds changes only through
/// [`follow`](Self::follow); the rest of its surface is queries, which take
/// `&mut self` because the broadphase they read is rebuilt lazily, as
/// [`PhysicsWorld`]'s are. See the [module docs](self) for why an answer here
/// is the client's view and not the server's.
#[derive(Debug)]
pub struct ClientQueryWorld {
    world: PhysicsWorld,
    replicas: HashMap<u64, Replica>,
    /// Counts [`follow`](Self::follow) calls, so a replica missing from the
    /// state is the one whose `seen` is behind it.
    frame: u64,
    /// The buffers the ids-only overlap works in.
    scratch: QueryScratch,
}

impl ClientQueryWorld {
    /// A world holding `statics` — the colliders the loaded scene is built
    /// from, made once by the game's own scene loader, the way its server
    /// builds its own — and no replicas yet.
    #[must_use]
    pub fn new(statics: PhysicsWorld) -> Self {
        Self {
            world: statics,
            replicas: HashMap::new(),
            frame: 0,
            scratch: QueryScratch::new(),
        }
    }

    /// Move every replica's collider to where `client` shows its entity at
    /// `alpha` — the value [`Client::update`] returned — adding a collider for
    /// each entity seen for the first time that `shape_of` gives a shape, and
    /// removing the collider of each entity no longer in the state, whether it
    /// despawned or left the sectors this client is subscribed to.
    ///
    /// `max_step` is the farthest anything in the game moves in one server
    /// tick, in metres: a replica that moved farther is read as a jump and is
    /// placed where it landed rather than lerped across the gap — see
    /// [`Client::interpolate_snapping`], which panics on a negative or `NaN`
    /// one.
    ///
    /// `shape_of` takes the entity's bits ([`crcbl_ecs::Entity::to_bits`]).
    /// It is asked once per entity, when it arrives: an entity keeps the shape
    /// it arrived with until it leaves.
    pub fn follow<T: Transport>(
        &mut self,
        client: &Client<T>,
        alpha: f32,
        max_step: f64,
        shape_of: impl FnMut(u64) -> Option<ColliderComponent>,
    ) {
        let state = client.interpolate_snapping(alpha, max_step);
        self.follow_state(&state, shape_of);
    }

    /// [`follow`](Self::follow)'s work, on a state already interpolated.
    fn follow_state(
        &mut self,
        state: &InterpolatedState,
        mut shape_of: impl FnMut(u64) -> Option<ColliderComponent>,
    ) {
        self.frame += 1;
        let frame = self.frame;
        for (entity_bits, transform) in &state.transforms {
            if let Some(replica) = self.replicas.get_mut(entity_bits) {
                self.world
                    .place_collider(replica.collider, &replica.shape, transform);
                replica.seen = frame;
            } else if let Some(shape) = shape_of(*entity_bits) {
                let collider = self.world.add_collider(&shape, transform);
                self.replicas.insert(
                    *entity_bits,
                    Replica {
                        collider,
                        shape,
                        seen: frame,
                    },
                );
            }
        }
        let world = &mut self.world;
        self.replicas.retain(|_, replica| {
            let present = replica.seen == frame;
            if !present {
                world.remove(replica.collider);
            }
            present
        });
    }

    /// The collider standing on the replicated entity `entity_bits`, if it has
    /// one: what a [`QueryFilter`] leaves out so the local player's own
    /// replica does not stop its camera.
    #[must_use]
    pub fn collider_of(&self, entity_bits: u64) -> Option<ColliderId> {
        self.replicas
            .get(&entity_bits)
            .map(|replica| replica.collider)
    }

    /// The closest solid collider `ray` meets that `filter` admits:
    /// [`PhysicsWorld::cast_ray_filtered`] on this view.
    #[must_use]
    pub fn cast_ray(&mut self, ray: &Ray, filter: QueryFilter) -> Option<(ColliderId, ShapeHit)> {
        self.world.cast_ray_filtered(ray, filter)
    }

    /// The closest solid collider a sphere of `radius` meets moving along
    /// `segment` that `filter` admits, its `t` the share of the segment
    /// covered: [`PhysicsWorld::sweep_sphere_filtered`] on this view.
    #[must_use]
    pub fn sweep_sphere(
        &mut self,
        segment: &Segment,
        radius: f64,
        filter: QueryFilter,
    ) -> Option<(ColliderId, ShapeHit)> {
        self.world.sweep_sphere_filtered(segment, radius, filter)
    }

    /// Every collider `filter` admits that a sphere is inside, each with its
    /// hit, written into `out` (cleared first):
    /// [`PhysicsWorld::overlap_sphere_filtered_into`] on this view.
    pub fn overlap_sphere_into(
        &mut self,
        centre: DVec3,
        radius: f64,
        filter: QueryFilter,
        out: &mut Vec<(ColliderId, OverlapHit)>,
    ) {
        self.world
            .overlap_sphere_filtered_into(centre, radius, filter, out);
    }

    /// [`overlap_sphere_into`](Self::overlap_sphere_into) naming the colliders
    /// and dropping their hits, over every collider:
    /// [`crcbl_phys::OverlapQueries::overlap_sphere_ids_into`], the fast path
    /// for a caller that only asks which.
    pub fn overlap_sphere_ids_into(
        &mut self,
        centre: DVec3,
        radius: f64,
        out: &mut Vec<ColliderId>,
    ) {
        self.world.overlap_queries().overlap_sphere_ids_into(
            centre,
            radius,
            &mut self.scratch,
            out,
        );
    }
}

#[cfg(test)]
mod tests;
