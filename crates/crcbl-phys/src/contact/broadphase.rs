//! The contact broadphase: split trees, fattened proxies, a move buffer and a
//! pair set — `docs/plan/36-contact-solver.md` decision 3, after Box2D v3.
//!
//! ```text
//!   static tree ── walls, pegs, shelves: never moves, never queries itself
//!   moving tree ── dynamic and kinematic bodies
//!   planes ─────── a short list, not a tree: an infinite plane has no bounds
//!
//!   tick:  each moving proxy whose tight bounds left its fat bounds is taken
//!          out and put back fatter, and joins the move buffer
//!          each proxy in the move buffer queries both trees and the planes
//!          a pair seen for the first time joins the pair set → a new contact
//! ```
//!
//! **A proxy's bounds are fat**: its shape's bounds grown by
//! [`fat_margin`] — five centimetres, or an eighth of its size if that is less.
//! A ball jittering in a pit stays inside its fat bounds and costs the tree
//! nothing; one that leaves them pays one removal and one insertion, each a
//! root-to-leaf walk of [`Bvh`]'s balanced tree.
//!
//! **Only proxies that moved look for new pairs.** Nothing in the move buffer
//! means nothing to find, so a settled pile's broadphase is a walk over its
//! bodies and no queries. A pair ends when the pipeline finds the two fat
//! bounds apart, not here.
//!
//! # Determinism
//!
//! The pair set is a [`HashSet`] used for membership and nothing else: nothing
//! iterates it, so its randomised order never reaches a result. New pairs come
//! out in move-buffer order, and within one proxy's query in the trees' own
//! traversal order, both of which follow from the calls that built them.
//!
//! # What this is not
//!
//! It is not the query world's tree. [`crate::world::PhysicsWorld`] still keeps
//! its own single tree over exact collider bounds for rays, sweeps and
//! overlaps, and a body in a system with contacts is in both. Folding the
//! queries onto these trees is future work.

use std::collections::HashSet;

use glam::DVec3;

use crate::broadphase::Bvh;
use crate::collider::Aabb;

/// A proxy's index in the broadphase.
pub(crate) type ProxyId = u32;

/// The most a proxy's bounds are fattened by, in metres.
pub(crate) const MAX_FAT_MARGIN: f64 = 0.05;

/// How a proxy's shape is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyKind {
    /// A body with no [`crate::RigidBody`]: in the static tree.
    Static,
    /// A dynamic or kinematic body: in the moving tree.
    Moving,
    /// A plane: in no tree.
    Plane {
        /// Its index in the pipeline's plane list.
        index: u32,
    },
}

/// One shape in the broadphase.
#[derive(Clone, Copy, Debug)]
struct Proxy {
    kind: ProxyKind,
    /// The fat bounds, or for a plane, unused.
    fat: Aabb,
    /// The element index in its tree, or unused for a plane.
    element: usize,
    /// Whether it is in the move buffer.
    moved: bool,
}

/// A plane's geometry, as the broadphase tests bounds against it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaneBounds {
    /// Unit outward normal.
    pub normal: DVec3,
    /// Distance from the origin along it.
    pub offset: f64,
}

impl PlaneBounds {
    /// Whether any point of `aabb` lies on or below the plane.
    pub(crate) fn reaches(&self, aabb: &Aabb) -> bool {
        let centre = aabb.centre();
        let half = aabb.extents() * 0.5;
        let lowest = self.normal.dot(centre) - self.normal.abs().dot(half);
        lowest <= self.offset
    }
}

/// How far a proxy with tight bounds `tight` is fattened: an eighth of its
/// largest extent, capped at [`MAX_FAT_MARGIN`].
pub(crate) fn fat_margin(tight: &Aabb) -> f64 {
    (tight.extents().max_element() * 0.125).min(MAX_FAT_MARGIN)
}

/// The split trees, the move buffer and the pair set.
#[derive(Debug)]
pub(crate) struct Broadphase {
    proxies: Vec<Option<Proxy>>,
    free: Vec<ProxyId>,
    static_tree: Bvh,
    moving_tree: Bvh,
    /// The planes' proxies, in the order they were made.
    planes: Vec<(ProxyId, PlaneBounds)>,
    move_buffer: Vec<ProxyId>,
    pairs: HashSet<u64>,
    stack: Vec<u32>,
    candidates: Vec<u32>,
}

/// The pair set's key for two proxies, the same whichever is named first.
fn pair_key(a: ProxyId, b: ProxyId) -> u64 {
    (u64::from(a.min(b)) << 32) | u64::from(a.max(b))
}

impl Broadphase {
    pub(crate) fn new() -> Self {
        Self {
            proxies: Vec::new(),
            free: Vec::new(),
            static_tree: Bvh::build(std::iter::empty()),
            moving_tree: Bvh::build(std::iter::empty()),
            planes: Vec::new(),
            move_buffer: Vec::new(),
            pairs: HashSet::new(),
            stack: Vec::new(),
            candidates: Vec::new(),
        }
    }

    /// Adds a body's proxy with tight bounds `tight`, in the move buffer so its
    /// first pairs are found on the next update.
    pub(crate) fn create(&mut self, kind: ProxyKind, tight: Aabb) -> ProxyId {
        debug_assert!(!matches!(kind, ProxyKind::Plane { .. }), "use create_plane");
        let fat = tight.inflated(fat_margin(&tight));
        let id = self.allocate();
        let element = self.tree_mut(kind).insert(fat, id);
        self.proxies[id as usize] = Some(Proxy {
            kind,
            fat,
            element,
            moved: false,
        });
        self.mark_moved(id);
        id
    }

    /// Adds a plane's proxy. Planes never move, so a plane finds its pairs
    /// when the bodies near it do.
    pub(crate) fn create_plane(&mut self, index: u32, bounds: PlaneBounds) -> ProxyId {
        let id = self.allocate();
        self.proxies[id as usize] = Some(Proxy {
            kind: ProxyKind::Plane { index },
            fat: Aabb::EMPTY,
            element: 0,
            moved: false,
        });
        self.planes.push((id, bounds));
        // Every body already here may be near it.
        for other in 0..self.proxies.len() {
            if other != id as usize
                && matches!(self.proxies[other], Some(p) if p.kind == ProxyKind::Moving)
            {
                self.mark_moved(other as ProxyId);
            }
        }
        id
    }

    /// Removes a proxy. Its pairs are the caller's to end first.
    pub(crate) fn destroy(&mut self, id: ProxyId) {
        let Some(proxy) = self.proxies.get_mut(id as usize).and_then(Option::take) else {
            return;
        };
        match proxy.kind {
            ProxyKind::Plane { .. } => self.planes.retain(|(plane, _)| *plane != id),
            kind => {
                self.tree_mut(kind).remove(proxy.element);
            }
        }
        self.move_buffer.retain(|moved| *moved != id);
        self.free.push(id);
    }

    /// Moves a proxy between the static and the moving tree, as a body gains
    /// or loses its [`crate::RigidBody`].
    pub(crate) fn set_kind(&mut self, id: ProxyId, kind: ProxyKind) {
        let Some(proxy) = self.proxies.get(id as usize).copied().flatten() else {
            return;
        };
        if proxy.kind == kind {
            return;
        }
        self.tree_mut(proxy.kind).remove(proxy.element);
        let element = self.tree_mut(kind).insert(proxy.fat, id);
        if let Some(Some(slot)) = self.proxies.get_mut(id as usize) {
            slot.kind = kind;
            slot.element = element;
        }
        self.mark_moved(id);
    }

    /// Tells the broadphase where a proxy's shape is now. Bounds still inside
    /// the fat ones cost nothing; bounds that left them re-insert the proxy,
    /// fattened afresh, and put it in the move buffer.
    pub(crate) fn update(&mut self, id: ProxyId, tight: Aabb) {
        let Some(proxy) = self.proxies.get(id as usize).copied().flatten() else {
            return;
        };
        if proxy.fat.contains(&tight) {
            return;
        }
        let fat = tight.inflated(fat_margin(&tight));
        let tree = self.tree_mut(proxy.kind);
        tree.remove(proxy.element);
        let element = tree.insert(fat, id);
        if let Some(Some(slot)) = self.proxies.get_mut(id as usize) {
            slot.fat = fat;
            slot.element = element;
        }
        self.mark_moved(id);
    }

    /// Queries every proxy in the move buffer and appends each pair seen for
    /// the first time to `out`, lower proxy first. `accept` is asked about each
    /// candidate pair before it joins the set; a refused pair is asked again
    /// the next time either proxy moves.
    pub(crate) fn find_new_pairs(
        &mut self,
        mut accept: impl FnMut(ProxyId, ProxyId) -> bool,
        out: &mut Vec<(ProxyId, ProxyId)>,
    ) {
        let buffer = std::mem::take(&mut self.move_buffer);
        for &id in &buffer {
            let Some(proxy) = self.proxies.get(id as usize).copied().flatten() else {
                continue;
            };
            let mut consider = |this: &mut Self, other: ProxyId| {
                if other == id {
                    return;
                }
                // Both moved: the one with the lower id finds the pair, so it
                // is found once.
                if other < id && matches!(this.proxies[other as usize], Some(p) if p.moved) {
                    return;
                }
                let key = pair_key(id, other);
                if this.pairs.contains(&key) || !accept(id.min(other), id.max(other)) {
                    return;
                }
                this.pairs.insert(key);
                out.push((id.min(other), id.max(other)));
            };

            let mut candidates = std::mem::take(&mut self.candidates);
            if proxy.kind == ProxyKind::Moving {
                self.static_tree
                    .traverse_aabb_into(&proxy.fat, &mut self.stack, &mut candidates);
                for &other in &candidates {
                    consider(self, other);
                }
                for index in 0..self.planes.len() {
                    let (plane, bounds) = self.planes[index];
                    if bounds.reaches(&proxy.fat) {
                        consider(self, plane);
                    }
                }
            }
            self.moving_tree
                .traverse_aabb_into(&proxy.fat, &mut self.stack, &mut candidates);
            for &other in &candidates {
                consider(self, other);
            }
            self.candidates = candidates;
        }
        for &id in &buffer {
            if let Some(Some(proxy)) = self.proxies.get_mut(id as usize) {
                proxy.moved = false;
            }
        }
        self.move_buffer = buffer;
        self.move_buffer.clear();
    }

    /// Every static proxy and plane whose bounds reach `bounds`, into `out`,
    /// and every moving proxy's too if `moving` — what a sweep's path could
    /// meet. Static proxies come first in the tree's traversal order, then the
    /// planes in the order they were made, then the moving proxies.
    pub(crate) fn query_path(&mut self, bounds: &Aabb, moving: bool, out: &mut Vec<ProxyId>) {
        out.clear();
        let mut found = std::mem::take(&mut self.candidates);
        self.static_tree
            .traverse_aabb_into(bounds, &mut self.stack, &mut found);
        out.extend_from_slice(&found);
        out.extend(
            self.planes
                .iter()
                .filter(|(_, plane)| plane.reaches(bounds))
                .map(|(id, _)| *id),
        );
        if moving {
            self.moving_tree
                .traverse_aabb_into(bounds, &mut self.stack, &mut found);
            out.extend_from_slice(&found);
        }
        self.candidates = found;
    }

    /// Takes a pair out of the set, so the two can pair again later.
    pub(crate) fn remove_pair(&mut self, a: ProxyId, b: ProxyId) {
        self.pairs.remove(&pair_key(a, b));
    }

    /// Whether two proxies' fat bounds still overlap: the test that ends a
    /// pair.
    pub(crate) fn overlaps(&self, a: ProxyId, b: ProxyId) -> bool {
        let (Some(pa), Some(pb)) = (self.proxy(a), self.proxy(b)) else {
            return false;
        };
        match (pa.kind, pb.kind) {
            (ProxyKind::Plane { .. }, ProxyKind::Plane { .. }) => false,
            (ProxyKind::Plane { .. }, _) => {
                self.plane_bounds(a).is_some_and(|p| p.reaches(&pb.fat))
            }
            (_, ProxyKind::Plane { .. }) => {
                self.plane_bounds(b).is_some_and(|p| p.reaches(&pa.fat))
            }
            _ => pa.fat.intersects(&pb.fat),
        }
    }

    /// How many pairs are in the set.
    pub(crate) fn pair_count(&self) -> usize {
        self.pairs.len()
    }

    /// How many proxies are waiting in the move buffer.
    #[cfg(test)]
    pub(crate) fn moved_count(&self) -> usize {
        self.move_buffer.len()
    }

    fn proxy(&self, id: ProxyId) -> Option<Proxy> {
        self.proxies.get(id as usize).copied().flatten()
    }

    fn plane_bounds(&self, id: ProxyId) -> Option<PlaneBounds> {
        self.planes
            .iter()
            .find(|(plane, _)| *plane == id)
            .map(|(_, bounds)| *bounds)
    }

    fn allocate(&mut self) -> ProxyId {
        if let Some(id) = self.free.pop() {
            id
        } else {
            self.proxies.push(None);
            (self.proxies.len() - 1) as ProxyId
        }
    }

    fn mark_moved(&mut self, id: ProxyId) {
        if let Some(Some(proxy)) = self.proxies.get_mut(id as usize)
            && !proxy.moved
        {
            proxy.moved = true;
            self.move_buffer.push(id);
        }
    }

    fn tree_mut(&mut self, kind: ProxyKind) -> &mut Bvh {
        match kind {
            ProxyKind::Static => &mut self.static_tree,
            ProxyKind::Moving | ProxyKind::Plane { .. } => &mut self.moving_tree,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(at: DVec3) -> Aabb {
        Aabb::from_centre_half(at, DVec3::splat(0.5))
    }

    /// Two moving proxies that overlap pair once, a static one pairs with the
    /// moving one and not with another static one, and nothing moving finds
    /// nothing new.
    #[test]
    fn moved_proxies_find_each_pair_once_and_statics_never_pair_with_statics() {
        let mut bp = Broadphase::new();
        let a = bp.create(ProxyKind::Moving, cube(DVec3::ZERO));
        let b = bp.create(ProxyKind::Moving, cube(DVec3::new(0.9, 0.0, 0.0)));
        let s = bp.create(ProxyKind::Static, cube(DVec3::new(-0.9, 0.0, 0.0)));
        let t = bp.create(ProxyKind::Static, cube(DVec3::new(-0.9, 0.5, 0.0)));
        let mut out = Vec::new();
        bp.find_new_pairs(|_, _| true, &mut out);
        out.sort_unstable();
        assert_eq!(out, [(a, b), (a, s), (a, t)]);
        assert_eq!(bp.pair_count(), 3);

        out.clear();
        bp.update(a, cube(DVec3::new(0.01, 0.0, 0.0)));
        assert_eq!(bp.moved_count(), 0, "a move inside the fat bounds is free");
        bp.find_new_pairs(|_, _| true, &mut out);
        assert!(out.is_empty(), "nothing moved, so nothing is new: {out:?}");
    }

    /// A proxy that leaves its fat bounds is re-inserted where it went and
    /// pairs there, and a refused pair is not remembered.
    #[test]
    fn a_proxy_that_leaves_its_fat_bounds_pairs_where_it_went() {
        let mut bp = Broadphase::new();
        let a = bp.create(ProxyKind::Moving, cube(DVec3::ZERO));
        let far = bp.create(ProxyKind::Static, cube(DVec3::new(10.0, 0.0, 0.0)));
        let mut out = Vec::new();
        bp.find_new_pairs(|_, _| true, &mut out);
        assert!(out.is_empty());

        bp.update(a, cube(DVec3::new(9.5, 0.0, 0.0)));
        bp.find_new_pairs(|_, _| false, &mut out);
        assert!(out.is_empty(), "refused");
        assert!(bp.overlaps(a, far));

        bp.update(a, cube(DVec3::new(9.6, 0.0, 0.0)));
        bp.find_new_pairs(|_, _| true, &mut out);
        assert_eq!(out, [(a, far)]);
    }

    /// A plane pairs with a body whose fat bounds reach below it, and not with
    /// one above.
    #[test]
    fn a_plane_pairs_with_what_reaches_it() {
        let mut bp = Broadphase::new();
        let low = bp.create(ProxyKind::Moving, cube(DVec3::new(0.0, 0.52, 0.0)));
        let high = bp.create(ProxyKind::Moving, cube(DVec3::new(5.0, 3.0, 0.0)));
        let floor = bp.create_plane(
            0,
            PlaneBounds {
                normal: DVec3::Y,
                offset: 0.0,
            },
        );
        let mut out = Vec::new();
        bp.find_new_pairs(|_, _| true, &mut out);
        assert_eq!(out, [(low, floor)]);
        assert!(!bp.overlaps(high, floor));
    }
}
