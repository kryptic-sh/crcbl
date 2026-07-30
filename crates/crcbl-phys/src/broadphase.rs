//! Broadphase spatial queries: bounding volume hierarchy (BVH).
//!
//! The [`Bvh`] is a static (immutable after construction) axis-aligned bounding
//! volume hierarchy that accelerates ray, segment, and overlap queries. It is
//! built once from a collection of (AABB, element_id) pairs and then traversed
//! read-only.
//!
//! # Build strategy
//!
//! Construction uses recursive spatial median splitting on the longest axis of
//! each node's bounding box. The tree is stored as a flat array (node at index
//! 0 is the root).

use crate::collider::Aabb;
use glam::DVec3;

// ---------------------------------------------------------------------------
// Primitive queries
// ---------------------------------------------------------------------------

/// A ray defined by an origin and a direction (not normalised).
///
/// The direction may have any non-zero length; the traversal code normalises
/// internally. `t_min` / `t_max` bound how far along the ray intersections are
/// accepted, in units of the ray's direction length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Origin in simulation space.
    pub origin: DVec3,
    /// Direction vector (not necessarily unit length).
    pub dir: DVec3,
    /// Minimum parametric distance to accept.
    pub t_min: f64,
    /// Maximum parametric distance to accept.
    pub t_max: f64,
}

impl Ray {
    /// Create a ray from origin and direction, with default bounds `(0, +inf)`.
    #[inline]
    #[must_use]
    pub fn new(origin: DVec3, dir: DVec3) -> Self {
        Self {
            origin,
            dir,
            t_min: 0.0,
            t_max: f64::INFINITY,
        }
    }

    /// Set the parametric bounds.
    #[inline]
    #[must_use]
    pub fn with_bounds(mut self, t_min: f64, t_max: f64) -> Self {
        self.t_min = t_min;
        self.t_max = t_max;
        self
    }
}

/// A finite segment (ray clamped between origin and endpoint).
///
/// Equivalent to a ray with `t_max` set to the segment length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Start point.
    pub start: DVec3,
    /// End point.
    pub end: DVec3,
}

impl Segment {
    /// Create a segment from start to end.
    #[inline]
    #[must_use]
    pub fn new(start: DVec3, end: DVec3) -> Self {
        Self { start, end }
    }

    /// Direction vector (end - start).
    #[inline]
    #[must_use]
    pub fn dir(&self) -> DVec3 {
        self.end - self.start
    }

    /// Length of the segment.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.dir().length()
    }
}

/// A hit returned by BVH traversal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BvhHit {
    /// The element_id supplied at build time.
    pub element_id: u32,
    /// Parametric distance along the ray at the hit point.
    pub t: f64,
    /// The world-space hit position.
    pub point: DVec3,
}

// ---------------------------------------------------------------------------
// BVH
// ---------------------------------------------------------------------------

/// A flat node in the BVH tree.
///
/// Nodes are stored in a flat array (post-order layout).  An internal node has
/// `leaf_count == 0` and stores left/right child indices.  A leaf node has
/// `leaf_count > 0` and stores the start index into the element-indices array
/// at `child_left` (child_right is unused).
#[derive(Debug, Clone, Copy)]
struct Node {
    aabb: Aabb,
    /// Internal: left child index. Leaf: start index in element_indices.
    child_left: u32,
    /// Internal: right child index. Leaf: unused (0).
    child_right: u32,
    /// 0 = internal node, > 0 = leaf node with this many elements.
    leaf_count: u32,
}

/// A bounding volume hierarchy for spatial queries.
///
/// Construct once via [`Bvh::build`]; then query repeatedly with
/// [`Bvh::traverse_ray`] or [`Bvh::traverse_segment`].
#[derive(Debug, Clone)]
pub struct Bvh {
    nodes: Vec<Node>,
    element_indices: Vec<u32>,
    root: u32,
}

impl Bvh {
    /// Build a BVH from (AABB, element_id) pairs.
    ///
    /// The element_id is opaque to the BVH — it is returned as-is in hit
    /// results so the caller can look up the actual collider or entity.
    ///
    /// An empty iterator produces an empty BVH (no root node).
    #[must_use]
    pub fn build(elements: impl IntoIterator<Item = (Aabb, u32)>) -> Self {
        let mut items: Vec<_> = elements.into_iter().collect();
        if items.is_empty() {
            return Self {
                nodes: Vec::new(),
                element_indices: Vec::new(),
                root: 0,
            };
        }

        let mut nodes = Vec::with_capacity(2 * items.len());
        let mut element_indices = Vec::with_capacity(items.len());

        let count = items.len();
        let root = Self::build_rec(&mut items, &mut nodes, &mut element_indices, 0, count);

        Self {
            nodes,
            element_indices,
            root,
        }
    }

    /// Recursive build using spatial median split on the longest axis.
    fn build_rec(
        items: &mut [(Aabb, u32)],
        nodes: &mut Vec<Node>,
        element_indices: &mut Vec<u32>,
        range_start: usize,
        range_end: usize,
    ) -> u32 {
        let count = range_end - range_start;

        // Compute union AABB of this range.
        let mut aabb = Aabb::EMPTY;
        for item in &items[range_start..range_end] {
            aabb = aabb.union(item.0);
        }

        if count == 1 {
            // Leaf node: store element index.
            let idx = element_indices.len() as u32;
            element_indices.push(items[range_start].1);
            let node_idx = nodes.len() as u32;
            nodes.push(Node {
                aabb,
                child_left: idx,
                child_right: 0,
                leaf_count: 1,
            });
            return node_idx;
        }

        // Internal node: split by spatial median on the longest axis.
        let extents = aabb.extents();
        let axis = if extents.x >= extents.y && extents.x >= extents.z {
            0usize
        } else if extents.y >= extents.z {
            1
        } else {
            2
        };

        items[range_start..range_end].sort_by(|a, b| {
            let ka = a.0.centre().to_array()[axis];
            let kb = b.0.centre().to_array()[axis];
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mid = range_start + count / 2;
        let left = Self::build_rec(items, nodes, element_indices, range_start, mid);
        let right = Self::build_rec(items, nodes, element_indices, mid, range_end);

        let node_idx = nodes.len() as u32;
        nodes.push(Node {
            aabb,
            child_left: left,
            child_right: right,
            leaf_count: 0,
        });
        node_idx
    }

    /// Walk the BVH with a ray, returning all intersected element ids and
    /// hit positions.
    ///
    /// Results are unsorted (tree order). Duplicate element ids are possible
    /// if the same element was inserted twice.
    #[must_use]
    pub fn traverse_ray(&self, ray: &Ray) -> Vec<BvhHit> {
        if self.nodes.is_empty() {
            return Vec::new();
        }

        let inv_dir = ray.dir.recip();
        let dir_is_neg = [ray.dir.x < 0.0, ray.dir.y < 0.0, ray.dir.z < 0.0];
        let t_min = ray.t_min;
        let t_max = ray.t_max;

        let mut hits = Vec::new();
        let mut stack = Vec::with_capacity(64);
        stack.push(self.root);

        while let Some(node_idx) = stack.pop() {
            let node = &self.nodes[node_idx as usize];
            if !node
                .aabb
                .intersect_ray(ray.origin, inv_dir, dir_is_neg, t_min, t_max)
            {
                continue;
            }
            if node.leaf_count > 0 {
                // Leaf: report each element at the AABB entry point.
                let t_entry = node
                    .aabb
                    .intersect_ray_entry(ray.origin, inv_dir, dir_is_neg);
                if t_entry > t_max {
                    continue;
                }
                let t = t_entry.max(t_min);
                let point = ray.origin + ray.dir * t;
                let end = (node.child_left + node.leaf_count) as usize;
                for i in node.child_left as usize..end {
                    hits.push(BvhHit {
                        element_id: self.element_indices[i],
                        t,
                        point,
                    });
                }
            } else {
                // Internal: push right then left (left will be popped first).
                stack.push(node.child_right);
                stack.push(node.child_left);
            }
        }

        hits
    }

    /// Walk the BVH with a segment, returning all intersected element ids.
    ///
    /// This is a convenience wrapper around [`Bvh::traverse_ray`] that converts
    /// the segment to a ray first.
    #[must_use]
    pub fn traverse_segment(&self, segment: &Segment) -> Vec<BvhHit> {
        let dir = segment.end - segment.start;
        let length = dir.length();
        if length <= 0.0 {
            return Vec::new();
        }
        let ray = Ray {
            origin: segment.start,
            dir,
            t_min: 0.0,
            t_max: 1.0,
        };
        self.traverse_ray(&ray)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers on Aabb for ray entry point
// ---------------------------------------------------------------------------

impl Aabb {
    /// Returns the near intersection `t` along the ray, or `f64::INFINITY` if
    /// the ray misses. Requires the slab-method precomputed values.
    fn intersect_ray_entry(&self, origin: DVec3, inv_dir: DVec3, dir_is_neg: [bool; 3]) -> f64 {
        let (lox, _hix) = if dir_is_neg[0] {
            (self.max.x, self.min.x)
        } else {
            (self.min.x, self.max.x)
        };
        let (loy, _hiy) = if dir_is_neg[1] {
            (self.max.y, self.min.y)
        } else {
            (self.min.y, self.max.y)
        };
        let (loz, _hiz) = if dir_is_neg[2] {
            (self.max.z, self.min.z)
        } else {
            (self.min.z, self.max.z)
        };

        let tmin = (lox - origin.x) * inv_dir.x;
        let tymin = (loy - origin.y) * inv_dir.y;
        let tzmin = (loz - origin.z) * inv_dir.z;

        tmin.max(tymin).max(tzmin)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box_at(centre: DVec3) -> (Aabb, u32) {
        (Aabb::from_centre_half(centre, DVec3::splat(0.5)), 0)
    }

    #[test]
    fn empty_bvh_returns_no_hits() {
        let bvh = Bvh::build([]);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert!(hits.is_empty());
    }

    #[test]
    fn bvh_hits_single_element() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].element_id, 0);
    }

    #[test]
    fn bvh_hits_multiple_elements() {
        let elements = [
            unit_box_at(DVec3::new(5.0, 0.0, 0.0)),
            (
                Aabb::from_centre_half(DVec3::new(10.0, 0.0, 0.0), DVec3::splat(0.5)),
                1,
            ),
        ];
        let bvh = Bvh::build(elements);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn bvh_misses_when_ray_goes_other_way() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::NEG_X));
        assert!(hits.is_empty());
    }

    #[test]
    fn segment_hits() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        let seg = Segment::new(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
        let hits = bvh.traverse_segment(&seg);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn segment_that_misses_returns_nothing() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        let seg = Segment::new(DVec3::ZERO, DVec3::new(0.0, 5.0, 0.0));
        let hits = bvh.traverse_segment(&seg);
        assert!(hits.is_empty());
    }

    #[test]
    fn zero_length_segment_is_safe() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        let seg = Segment::new(DVec3::ZERO, DVec3::ZERO);
        let hits = bvh.traverse_segment(&seg);
        assert!(hits.is_empty());
    }

    #[test]
    fn many_elements_bvh_builds_and_queries() {
        let mut elements = Vec::new();
        for i in 0..100u32 {
            let x = (i as f64) * 2.0 + 1.0;
            elements.push((
                Aabb::from_centre_half(DVec3::new(x, 0.0, 0.0), DVec3::splat(0.4)),
                i,
            ));
        }
        let bvh = Bvh::build(elements);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 100);
    }

    #[test]
    fn bvh_handles_negative_direction_ray() {
        let elements = [
            (
                Aabb::from_centre_half(DVec3::new(-5.0, 0.0, 0.0), DVec3::splat(0.5)),
                0,
            ),
            (
                Aabb::from_centre_half(DVec3::new(-10.0, 0.0, 0.0), DVec3::splat(0.5)),
                1,
            ),
        ];
        let bvh = Bvh::build(elements);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::NEG_X));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn ray_outside_t_bounds_returns_nothing() {
        let bvh = Bvh::build([unit_box_at(DVec3::new(5.0, 0.0, 0.0))]);
        // Ray that would hit at t=5, but bounds say t_max=3.
        let ray = Ray::new(DVec3::ZERO, DVec3::X).with_bounds(0.0, 3.0);
        let hits = bvh.traverse_ray(&ray);
        assert!(hits.is_empty());
    }

    #[test]
    fn element_ids_are_preserved() {
        let elements = [
            (
                Aabb::from_centre_half(DVec3::new(3.0, 0.0, 0.0), DVec3::splat(0.5)),
                42u32,
            ),
            (
                Aabb::from_centre_half(DVec3::new(7.0, 0.0, 0.0), DVec3::splat(0.5)),
                99u32,
            ),
        ];
        let bvh = Bvh::build(elements);
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        let mut ids: Vec<_> = hits.iter().map(|h| h.element_id).collect();
        ids.sort();
        assert_eq!(ids, vec![42, 99]);
    }

    #[test]
    fn ray_origin_inside_bvh_still_reports_hits() {
        let bvh = Bvh::build([(
            Aabb::from_centre_half(DVec3::new(0.0, 0.0, 0.0), DVec3::splat(1.0)),
            42,
        )]);
        // Origin is inside the AABB.
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].element_id, 42);
    }

    #[test]
    fn t_values_are_correct() {
        let bvh = Bvh::build([(
            Aabb::from_centre_half(DVec3::new(5.0, 0.0, 0.0), DVec3::splat(0.5)),
            1,
        )]);
        // AABB spans x in [4.5, 5.5], ray from origin along x hits at t=4.5.
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert!(
            (hits[0].t - 4.5).abs() < 0.001,
            "expected ~4.5, got {}",
            hits[0].t
        );
    }
}
