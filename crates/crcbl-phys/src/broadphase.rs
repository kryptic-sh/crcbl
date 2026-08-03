//! Broadphase spatial queries: bounding volume hierarchy (BVH).
//!
//! The [`Bvh`] is a *dynamic* axis-aligned bounding volume hierarchy that
//! accelerates ray, segment, and overlap queries. It can be built in bulk from
//! a collection of (AABB, element_id) pairs and then churned in place:
//! [`Bvh::insert`] and [`Bvh::remove`] each touch one root-to-leaf path
//! instead of rebuilding, and [`Bvh::update_aabb`] refits one in place.
//!
//! # Build strategy
//!
//! Bulk construction uses recursive spatial median splitting on the longest
//! axis of each node's bounding box. The tree is stored as a flat array; the
//! root is not necessarily index 0, because churn frees and
//! recycles node slots.
//!
//! # Churn strategy
//!
//! [`Bvh::insert`] picks the sibling to pair a new leaf with by the surface
//! area heuristic (SAH): it descends from the root, at each node comparing the
//! cost of stopping here against the cost of pushing the leaf into either
//! child, and stops when descending cannot pay for itself. [`Bvh::remove`]
//! deletes the leaf and its parent and promotes the leaf's sibling into the
//! parent's place. Both then walk back to the root, refitting bounds so every
//! query sees bounds tight for the elements actually present, and rotating
//! (AVL single rotation) so no node's two subtrees ever differ by more
//! than one in height.
//!
//! The rotation is what makes tree quality a property rather than a hope: the
//! surface area heuristic alone degenerates when elements pile into the same
//! region, because then every candidate site costs the same. [`Bvh::depth`] is
//! the observable, and `crates/crcbl-phys/tests/churn.rs` bounds it across
//! thousands of operations including that degenerate input.

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
    /// Parametric distance along the ray at the leaf AABB's entry point.
    /// This is a broadphase distance — refine it with an exact shape test.
    pub t: f64,
}

// ---------------------------------------------------------------------------
// BVH
// ---------------------------------------------------------------------------

/// Sentinel for "no node" — an absent parent, or an empty tree's root.
const NIL: u32 = u32::MAX;

/// A flat node in the BVH tree.
///
/// Nodes are stored in a flat array.  Bulk construction lays them out in
/// pre-order ([`Bvh::build_rec`] reserves a node's slot before recursing, so a
/// parent precedes its subtree), but [`Bvh::insert`] and [`Bvh::remove`]
/// recycle freed slots, so position carries no meaning once a tree has been
/// churned — follow `child_left` / `child_right` / `parent`, never `index ± 1`.
///
/// An internal node has `leaf_count == 0` and stores left/right child indices.
/// A leaf node has `leaf_count == 1` and stores its *element index* at
/// `child_left` (`child_right` is unused).  That element index addresses both
/// `element_indices` and `element_node`, and is the handle
/// [`Bvh::update_aabb`] and [`Bvh::remove`] take.
///
/// Every node except the root has a valid `parent` index.  The root's parent
/// is [`NIL`].
#[derive(Debug, Clone, Copy)]
struct Node {
    aabb: Aabb,
    /// Internal: left child index. Leaf: this leaf's element index.
    child_left: u32,
    /// Internal: right child index. Leaf: unused (0).
    child_right: u32,
    /// Parent node index, or [`NIL`] for the root.
    parent: u32,
    /// 0 = internal node, 1 = leaf node.
    leaf_count: u32,
    /// Longest path from here to a leaf below, in edges: a leaf is `0`, its
    /// parent `1`. Maintained by every structural change so
    /// [`Bvh::balance`] can compare two subtrees in `O(1)`.
    height: u32,
}

/// A dynamic bounding volume hierarchy for spatial queries.
///
/// Build in bulk via [`Bvh::build`], then query with [`Bvh::traverse_ray`],
/// [`Bvh::traverse_segment`] or [`Bvh::traverse_aabb`].
///
/// The element set is not frozen by construction:
///
/// * [`Bvh::insert`] adds one element, returning the element index that names
///   it from then on.
/// * [`Bvh::remove`] takes one back out.
/// * [`Bvh::update_aabb`] moves one that stays.
///
/// Each is one root-to-leaf walk plus a refit of that path, which is what lets
/// a caller that spawns and kills elements every tick — bullets, debris —
/// avoid paying `O(n log n)` for a rebuild each time.
#[derive(Debug, Clone)]
pub struct Bvh {
    nodes: Vec<Node>,
    /// For each element index, the caller-supplied `element_id` reported by
    /// queries.  Slots of removed elements are stale and are never read,
    /// because `element_node` marks them [`NIL`] and no leaf points at them.
    element_indices: Vec<u32>,
    /// For each element index, the leaf node that owns it, or [`NIL`] if that
    /// element index is free.  Bulk construction reorders elements by
    /// centroid, so leaf order and *build* order differ; this is indexed by
    /// build order because that is the handle callers hold on to.
    element_node: Vec<u32>,
    /// Element indices freed by [`Bvh::remove`], available for reuse.
    free_elements: Vec<u32>,
    /// Node slots freed by [`Bvh::remove`], available for reuse.
    free_nodes: Vec<u32>,
    /// Number of live elements (leaves).
    live_elements: usize,
    /// Root node index, or [`NIL`] when the tree is empty.  Not necessarily 0:
    /// removing the root's subtree promotes a sibling into its place.
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
        // Each item carries its position in the input array as a third field so
        // the centroid sort below cannot detach `element_node` from the build
        // order that `update_aabb` is indexed by.
        let mut items: Vec<(Aabb, u32, u32)> = elements
            .into_iter()
            .enumerate()
            .map(|(build_index, (aabb, element_id))| (aabb, element_id, build_index as u32))
            .collect();
        if items.is_empty() {
            return Self {
                nodes: Vec::new(),
                element_indices: Vec::new(),
                element_node: Vec::new(),
                free_elements: Vec::new(),
                free_nodes: Vec::new(),
                live_elements: 0,
                root: NIL,
            };
        }

        let mut nodes = Vec::with_capacity(2 * items.len());
        let mut element_indices = vec![0u32; items.len()];
        let mut element_node = vec![NIL; items.len()];

        let count = items.len();
        let root = Self::build_rec(
            &mut items,
            &mut nodes,
            &mut element_indices,
            &mut element_node,
            0,
            count,
            NIL,
        );

        Self {
            nodes,
            element_indices,
            element_node,
            free_elements: Vec::new(),
            free_nodes: Vec::new(),
            live_elements: count,
            root,
        }
    }

    /// Recursive build using spatial median split on the longest axis.
    ///
    /// `items` are `(aabb, element_id, build_index)`; leaves record their node
    /// and their element id against `build_index`, never against the leaf's
    /// own position, so that a leaf's `child_left` is the same element index
    /// that [`Bvh::update_aabb`] and [`Bvh::remove`] take.
    fn build_rec(
        items: &mut [(Aabb, u32, u32)],
        nodes: &mut Vec<Node>,
        element_indices: &mut [u32],
        element_node: &mut [u32],
        range_start: usize,
        range_end: usize,
        parent: u32,
    ) -> u32 {
        let count = range_end - range_start;

        // Compute union AABB of this range.
        let mut aabb = Aabb::EMPTY;
        for item in &items[range_start..range_end] {
            aabb = aabb.union(item.0);
        }

        if count == 1 {
            // Leaf node: the element index *is* the build index, so one number
            // addresses the leaf, `element_indices` and `element_node` alike.
            let elem_idx = items[range_start].2;
            element_indices[elem_idx as usize] = items[range_start].1;
            let node_idx = nodes.len() as u32;
            element_node[elem_idx as usize] = node_idx;
            nodes.push(Node {
                aabb,
                child_left: elem_idx,
                child_right: 0,
                parent,
                leaf_count: 1,
                height: 0,
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
        let node_idx = nodes.len() as u32;
        // Reserve the node slot so children's back-references are valid.
        nodes.push(Node {
            aabb,
            child_left: 0,
            child_right: 0,
            parent,
            leaf_count: 0,
            height: 0,
        });

        let left = Self::build_rec(
            items,
            nodes,
            element_indices,
            element_node,
            range_start,
            mid,
            node_idx,
        );
        let right = Self::build_rec(
            items,
            nodes,
            element_indices,
            element_node,
            mid,
            range_end,
            node_idx,
        );

        nodes[node_idx as usize].child_left = left;
        nodes[node_idx as usize].child_right = right;
        nodes[node_idx as usize].height = 1 + nodes[left as usize]
            .height
            .max(nodes[right as usize].height);
        node_idx
    }

    /// Walk the BVH and return all element ids whose AABB intersects the
    /// query AABB. The BVH itself tests AABB↔AABB, so this is a broadphase
    /// overlap query — the caller must refine with exact shape tests.
    ///
    /// Results are unsorted (tree order).
    #[must_use]
    pub fn traverse_aabb(&self, query: &Aabb) -> Vec<u32> {
        let mut out = Vec::new();
        self.traverse_aabb_into(query, &mut Vec::with_capacity(64), &mut out);
        out
    }

    /// [`traverse_aabb`](Self::traverse_aabb) writing into buffers the caller
    /// owns, for a consumer that runs one query per body per tick.
    ///
    /// Both `out` and the descent `stack` are **cleared** and then filled, so a
    /// caller hoists one of each out of its loop and the whole pass allocates
    /// nothing in the steady state. The owned form allocates two `Vec`s per
    /// call, which at a crowd sample's ten thousand agents is 1.2 million
    /// allocations a second doing nothing.
    pub fn traverse_aabb_into(&self, query: &Aabb, stack: &mut Vec<u32>, out: &mut Vec<u32>) {
        out.clear();
        stack.clear();
        if self.root == NIL {
            return;
        }
        stack.push(self.root);

        while let Some(node_idx) = stack.pop() {
            let node = &self.nodes[node_idx as usize];
            if !node.aabb.intersects(query) {
                continue;
            }
            if node.leaf_count > 0 {
                let end = (node.child_left + node.leaf_count) as usize;
                for i in node.child_left as usize..end {
                    out.push(self.element_indices[i]);
                }
            } else {
                stack.push(node.child_right);
                stack.push(node.child_left);
            }
        }
    }

    /// Walk the BVH with a ray, returning all intersected element ids and
    /// hit positions.
    ///
    /// Results are unsorted (tree order). Duplicate element ids are possible
    /// if the same element was inserted twice.
    #[must_use]
    pub fn traverse_ray(&self, ray: &Ray) -> Vec<BvhHit> {
        if self.root == NIL {
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
                    .ray_slab(ray.origin, inv_dir, dir_is_neg)
                    .map_or(f64::INFINITY, |(t_near, _)| t_near);
                if t_entry > t_max {
                    continue;
                }
                let t = t_entry.max(t_min);
                let end = (node.child_left + node.leaf_count) as usize;
                for i in node.child_left as usize..end {
                    hits.push(BvhHit {
                        element_id: self.element_indices[i],
                        t,
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

    /// Update the AABB of a single element and refit the tree upwards.
    ///
    /// `element_index` is the handle [`Bvh::insert`] returned, or — for
    /// elements that came from [`Bvh::build`] — the position in the array that
    /// was built from. Returns `true` if the element is live and the tree was
    /// refit, `false` if the index names no element (never inserted, or
    /// removed since).
    ///
    /// This is O(depth) — much cheaper than a full rebuild after each move.
    /// The caller must ensure the new AABB *contains* the shape it represents;
    /// the BVH only sees AABBs.
    ///
    /// This does **not** re-pick the element's place in the tree. An element
    /// that moves far from where it was inserted stays where it was, growing
    /// its ancestors' bounds; that is the cost of a refit, and it is why
    /// callers whose elements travel — rather than jitter — should remove and
    /// re-insert instead.
    pub fn update_aabb(&mut self, element_index: usize, new_aabb: Aabb) -> bool {
        let Some(leaf_node) = self.leaf_of(element_index) else {
            return false;
        };

        // Update leaf, then walk up to the root refitting.
        self.nodes[leaf_node as usize].aabb = new_aabb;
        self.refit_from(self.nodes[leaf_node as usize].parent);
        true
    }

    // ── Churn: insert / remove ─────────────────────────────────────────

    /// Insert one element, returning the element index that names it.
    ///
    /// The returned index is the handle for [`Bvh::update_aabb`] and
    /// [`Bvh::remove`], and it is in the same space as the build-order indices
    /// [`Bvh::build`] hands out. Indices of removed elements are recycled, so
    /// an index is only meaningful until the element it names is removed.
    ///
    /// The new leaf is paired with the sibling chosen by
    /// the surface area heuristic; a fresh internal
    /// node takes the sibling's place and adopts both, and the path back to the
    /// root is refit and rebalanced. Cost is one root-to-leaf descent plus one
    /// walk back up.
    pub fn insert(&mut self, aabb: Aabb, element_id: u32) -> usize {
        let element_index = self.alloc_element(element_id);
        let leaf = self.alloc_node(Node {
            aabb,
            child_left: element_index,
            child_right: 0,
            parent: NIL,
            leaf_count: 1,
            height: 0,
        });
        self.element_node[element_index as usize] = leaf;
        self.live_elements += 1;

        // First element: it is the whole tree.
        if self.root == NIL {
            self.root = leaf;
            return element_index as usize;
        }

        let sibling = self.find_best_sibling(aabb);
        let old_parent = self.nodes[sibling as usize].parent;
        let new_parent = self.alloc_node(Node {
            aabb: aabb.union(self.nodes[sibling as usize].aabb),
            child_left: sibling,
            child_right: leaf,
            parent: old_parent,
            leaf_count: 0,
            height: 1 + self.nodes[sibling as usize].height,
        });
        self.nodes[sibling as usize].parent = new_parent;
        self.nodes[leaf as usize].parent = new_parent;

        if old_parent == NIL {
            self.root = new_parent;
        } else {
            self.replace_child(old_parent, sibling, new_parent);
        }
        self.refit_and_balance_from(new_parent);
        element_index as usize
    }

    /// Remove the element named by `element_index`.
    ///
    /// Returns `false` if the index names no live element — the same answer a
    /// double remove gets, which is what makes it safe for a caller sweeping
    /// deferred destructions to ask twice.
    ///
    /// The leaf and its parent both go; the leaf's sibling is promoted into the
    /// parent's slot, so the tree keeps its "every internal node has two
    /// children" shape and never accumulates single-child chains. The freed
    /// node slots are recycled by later inserts.
    pub fn remove(&mut self, element_index: usize) -> bool {
        let Some(leaf) = self.leaf_of(element_index) else {
            return false;
        };
        self.element_node[element_index] = NIL;
        self.free_elements.push(element_index as u32);
        self.live_elements -= 1;

        // Sole element: the tree becomes empty.
        if leaf == self.root {
            self.root = NIL;
            self.free_nodes.push(leaf);
            return true;
        }

        let parent = self.nodes[leaf as usize].parent;
        let parent_node = self.nodes[parent as usize];
        let sibling = if parent_node.child_left == leaf {
            parent_node.child_right
        } else {
            parent_node.child_left
        };
        let grandparent = parent_node.parent;

        self.nodes[sibling as usize].parent = grandparent;
        if grandparent == NIL {
            self.root = sibling;
        } else {
            self.replace_child(grandparent, parent, sibling);
            self.refit_and_balance_from(grandparent);
        }

        self.free_nodes.push(parent);
        self.free_nodes.push(leaf);
        true
    }

    // ── Observables ────────────────────────────────────────────────────

    /// Number of live elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live_elements
    }

    /// Whether the tree holds no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live_elements == 0
    }

    /// The longest root-to-leaf path, counted in nodes (a one-element tree has
    /// depth 1, an empty one 0).
    ///
    /// This is the tree-quality observable: query cost is bounded by how far a
    /// traversal can descend, so a depth that grows past `O(log n)` under churn
    /// is the shape of degradation, and this is what
    /// `crates/crcbl-phys/tests/churn.rs` asserts a bound on. It walks the
    /// whole tree — `O(n)` — so it is for tests and diagnostics, not for a
    /// per-frame check.
    #[must_use]
    pub fn depth(&self) -> usize {
        if self.root == NIL {
            return 0;
        }
        let mut deepest = 0;
        let mut stack = vec![(self.root, 1usize)];
        while let Some((node_idx, depth)) = stack.pop() {
            let node = &self.nodes[node_idx as usize];
            if node.leaf_count > 0 {
                deepest = deepest.max(depth);
            } else {
                stack.push((node.child_left, depth + 1));
                stack.push((node.child_right, depth + 1));
            }
        }
        deepest
    }

    // ── Internal helpers ───────────────────────────────────────────────

    /// The leaf node owning `element_index`, or `None` if that index names no
    /// live element.
    fn leaf_of(&self, element_index: usize) -> Option<u32> {
        match self.element_node.get(element_index).copied() {
            Some(leaf) if leaf != NIL => Some(leaf),
            _ => None,
        }
    }

    /// Take a free element index, or grow the element arrays by one.
    fn alloc_element(&mut self, element_id: u32) -> u32 {
        if let Some(index) = self.free_elements.pop() {
            self.element_indices[index as usize] = element_id;
            index
        } else {
            let index = self.element_indices.len() as u32;
            self.element_indices.push(element_id);
            self.element_node.push(NIL);
            index
        }
    }

    /// Take a free node slot, or grow the node array by one.
    fn alloc_node(&mut self, node: Node) -> u32 {
        if let Some(index) = self.free_nodes.pop() {
            self.nodes[index as usize] = node;
            index
        } else {
            let index = self.nodes.len() as u32;
            self.nodes.push(node);
            index
        }
    }

    /// Point `parent`'s child link at `new_child` wherever it pointed at
    /// `old_child`.
    fn replace_child(&mut self, parent: u32, old_child: u32, new_child: u32) {
        let node = &mut self.nodes[parent as usize];
        if node.child_left == old_child {
            node.child_left = new_child;
        } else {
            node.child_right = new_child;
        }
    }

    /// Re-union the bounds of `start` and every ancestor above it.
    ///
    /// For structural changes use [`Bvh::refit_and_balance_from`]; this one is
    /// for [`Bvh::update_aabb`], which moves a leaf's bounds without moving any
    /// node, and therefore leaves every `height` already correct.
    fn refit_from(&mut self, start: u32) {
        let mut current = start;
        while current != NIL {
            let node = self.nodes[current as usize];
            let left = self.nodes[node.child_left as usize].aabb;
            let right = self.nodes[node.child_right as usize].aabb;
            self.nodes[current as usize].aabb = left.union(right);
            current = node.parent;
        }
    }

    /// Walk from `start` to the root, rebalancing each node on the way and
    /// recomputing its bounds and height.
    ///
    /// Insertion picks where a leaf goes by bounds alone, which says nothing
    /// about shape: elements that pile into the same region give the surface
    /// area heuristic nothing to separate them by, every candidate costs the
    /// same, and the tie sends every one of them down the same side. Measured
    /// on 1024 coincident boxes under 20k insert/remove pairs, that reached
    /// depth 623 against an ideal of 11 — a "tree" that is very nearly a linked
    /// list, and queries that scan it. Rebalancing on the way back up is what
    /// makes the ideal hold regardless of where elements sit.
    fn refit_and_balance_from(&mut self, start: u32) {
        let mut current = start;
        while current != NIL {
            current = self.balance(current);
            let node = self.nodes[current as usize];
            let left = self.nodes[node.child_left as usize];
            let right = self.nodes[node.child_right as usize];
            self.nodes[current as usize].aabb = left.aabb.union(right.aabb);
            self.nodes[current as usize].height = 1 + left.height.max(right.height);
            current = self.nodes[current as usize].parent;
        }
    }

    /// Rotate `index`'s taller grandchild up if its two subtrees differ in
    /// height by more than one, returning whichever node now sits in `index`'s
    /// place.
    ///
    /// This is the AVL single rotation, chosen over the alternatives — a
    /// periodic full rebuild, or tree rotations picked by SAH cost — because it
    /// is the one that makes a *statement*: no node's subtrees ever differ by
    /// more than one in height, so depth is `O(log n)` by construction rather
    /// than by hoping the inputs are well spread. A rebuild policy would only
    /// bound the damage between rebuilds, and would put an `O(n log n)` spike
    /// in a frame that was trying to spawn a bullet.
    ///
    /// The rotated node's bounds and height are fixed up here; its new parent's
    /// are fixed by the caller, which is mid-walk up to the root.
    fn balance(&mut self, index: u32) -> u32 {
        let node = self.nodes[index as usize];
        if node.leaf_count > 0 || node.height < 2 {
            return index;
        }

        let left = node.child_left;
        let right = node.child_right;
        let skew =
            self.nodes[right as usize].height as i64 - self.nodes[left as usize].height as i64;
        if skew > 1 {
            self.rotate_up(index, right, left)
        } else if skew < -1 {
            self.rotate_up(index, left, right)
        } else {
            index
        }
    }

    /// Pull `child` (a child of `index`) above `index`, leaving `index` holding
    /// `keep` and whichever of `child`'s own children is shorter.
    ///
    /// Returns `child`, which now occupies the slot `index` did.
    fn rotate_up(&mut self, index: u32, child: u32, keep: u32) -> u32 {
        let child_node = self.nodes[child as usize];
        let (tall, short) = {
            let a = child_node.child_left;
            let b = child_node.child_right;
            if self.nodes[a as usize].height > self.nodes[b as usize].height {
                (a, b)
            } else {
                (b, a)
            }
        };

        // `child` takes `index`'s place under the grandparent.
        let grandparent = self.nodes[index as usize].parent;
        self.nodes[child as usize].parent = grandparent;
        self.nodes[index as usize].parent = child;
        if grandparent == NIL {
            self.root = child;
        } else {
            self.replace_child(grandparent, index, child);
        }

        // `child` keeps its taller subtree and adopts `index`; `index` keeps
        // `keep` and adopts the shorter one.  Which side each lands on is the
        // side it came from, so a left-heavy rotation stays left-heavy.
        self.set_children(child, index, tall);
        if self.nodes[index as usize].child_left == keep {
            self.set_children(index, keep, short);
        } else {
            self.set_children(index, short, keep);
        }
        self.nodes[short as usize].parent = index;

        // `index` is now below `child`, so it is refit first.
        self.recompute(index);
        self.recompute(child);
        child
    }

    /// Point `parent` at exactly these two children, preserving the order they
    /// are given in.
    fn set_children(&mut self, parent: u32, left: u32, right: u32) {
        let node = &mut self.nodes[parent as usize];
        node.child_left = left;
        node.child_right = right;
    }

    /// Recompute one internal node's bounds and height from its children.
    fn recompute(&mut self, index: u32) {
        let node = self.nodes[index as usize];
        let left = self.nodes[node.child_left as usize];
        let right = self.nodes[node.child_right as usize];
        self.nodes[index as usize].aabb = left.aabb.union(right.aabb);
        self.nodes[index as usize].height = 1 + left.height.max(right.height);
    }

    /// Pick the node a new leaf with bounds `aabb` should become a sibling of.
    ///
    /// Descends from the root. At each internal node it compares the cost of
    /// stopping — pairing with this whole subtree, which prices at the area of
    /// the box that would have to enclose both — against the cost of pushing
    /// the leaf into either child, which is that child's area growth plus the
    /// area every node on the path above would inherit. It stops as soon as
    /// descending cannot pay for itself, so the walk is bounded by the depth.
    ///
    /// Costs are compared, never accumulated across calls, so the dropped
    /// factor of two in [`Aabb::half_surface_area`] is invisible here.
    fn find_best_sibling(&self, aabb: Aabb) -> u32 {
        let mut index = self.root;
        while self.nodes[index as usize].leaf_count == 0 {
            let node = self.nodes[index as usize];
            let area = node.aabb.half_surface_area();
            let combined_area = node.aabb.union(aabb).half_surface_area();

            // Stop here: a new parent enclosing this subtree and the new leaf.
            let stop_cost = 2.0 * combined_area;
            // Every ancestor of the descent point grows by this much.
            let inheritance_cost = 2.0 * (combined_area - area);

            let descend_cost = |child: u32| {
                let child = self.nodes[child as usize];
                let grown = child.aabb.union(aabb).half_surface_area();
                if child.leaf_count > 0 {
                    grown + inheritance_cost
                } else {
                    grown - child.aabb.half_surface_area() + inheritance_cost
                }
            };
            let left_cost = descend_cost(node.child_left);
            let right_cost = descend_cost(node.child_right);

            if stop_cost < left_cost && stop_cost < right_cost {
                break;
            }
            index = if left_cost < right_cost {
                node.child_left
            } else {
                node.child_right
            };
        }
        index
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

    // ── AABB traversal ─────────────────────────────────────────────────

    #[test]
    fn traverse_aabb_finds_overlapping() {
        let elements = [
            (
                Aabb::from_centre_half(DVec3::new(3.0, 0.0, 0.0), DVec3::splat(1.0)),
                10,
            ),
            (
                Aabb::from_centre_half(DVec3::new(10.0, 0.0, 0.0), DVec3::splat(1.0)),
                20,
            ),
        ];
        let bvh = Bvh::build(elements);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(5.0));
        let ids = bvh.traverse_aabb(&query);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], 10);
    }

    #[test]
    fn traverse_aabb_empty_bvh() {
        let bvh = Bvh::build([]);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(10.0));
        assert!(bvh.traverse_aabb(&query).is_empty());
    }

    #[test]
    fn traverse_aabb_misses_all() {
        let bvh = Bvh::build([(
            Aabb::from_centre_half(DVec3::new(100.0, 0.0, 0.0), DVec3::splat(1.0)),
            1,
        )]);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(5.0));
        assert!(bvh.traverse_aabb(&query).is_empty());
    }

    // ── BVH refit ───────────────────────────────────────────────────────

    #[test]
    fn update_aabb_moves_element_in_ray_query() {
        let elements = vec![
            (
                Aabb::from_centre_half(DVec3::new(5.0, 0.0, 0.0), DVec3::splat(1.0)),
                100,
            ),
            (
                Aabb::from_centre_half(DVec3::new(100.0, 0.0, 0.0), DVec3::splat(1.0)),
                200,
            ),
        ];
        let mut bvh = Bvh::build(elements);

        // Element 0 hits at x=5.
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 2);

        // Move element 0 far away.
        let new_aabb = Aabb::from_centre_half(DVec3::new(-50.0, 0.0, 0.0), DVec3::splat(1.0));
        assert!(bvh.update_aabb(0, new_aabb));

        // Ray going +X should miss it now, only hit element 1.
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].element_id, 200);
    }

    #[test]
    fn update_aabb_refit_keeps_tree_consistent() {
        // Build a larger tree to exercise deeper refit paths.
        let mut elements = Vec::new();
        for i in 0..20u32 {
            elements.push((
                Aabb::from_centre_half(DVec3::new(i as f64 * 2.0, 0.0, 0.0), DVec3::splat(0.5)),
                i,
            ));
        }
        let mut bvh = Bvh::build(elements);

        // Update element 10.
        let new_aabb = Aabb::from_centre_half(DVec3::new(10.0 * 2.0, 0.0, 0.0), DVec3::splat(1.0));
        assert!(bvh.update_aabb(10, new_aabb));

        // The BVH should still find it.
        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 20);
    }

    #[test]
    fn update_aabb_indexes_build_order_not_leaf_order() {
        // Elements are supplied in *descending* centroid order, so the
        // centroid sort in `build_rec` reverses them.  `update_aabb(0)` must
        // still move the element that was passed first.
        let elements = vec![
            (
                Aabb::from_centre_half(DVec3::new(20.0, 0.0, 0.0), DVec3::splat(1.0)),
                100,
            ),
            (
                Aabb::from_centre_half(DVec3::new(5.0, 0.0, 0.0), DVec3::splat(1.0)),
                200,
            ),
        ];
        let mut bvh = Bvh::build(elements);
        assert_eq!(bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X)).len(), 2);

        // Move build-order element 0 (the one at x=20) behind the ray origin.
        let new_aabb = Aabb::from_centre_half(DVec3::new(-50.0, 0.0, 0.0), DVec3::splat(1.0));
        assert!(bvh.update_aabb(0, new_aabb));

        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 1, "only the untouched element remains ahead");
        assert_eq!(hits[0].element_id, 200);
    }

    #[test]
    fn update_aabb_invalid_index_returns_false() {
        let mut bvh = Bvh::build([unit_box_at(DVec3::ZERO)]);
        assert!(!bvh.update_aabb(999, Aabb::EMPTY));
    }

    // ── Churn: structural invariants ────────────────────────────────────

    /// Check every invariant the tree's own code relies on and no query can
    /// reveal on its own.
    ///
    /// A query only ever descends, so it cannot notice a wrong `parent`, a
    /// stale `height` or a subtree that has become lopsided — but `remove` and
    /// `balance` walk *up* and read exactly those, so a tree that is wrong
    /// here answers correctly today and corrupts itself later. Checking them
    /// from inside the module is the point: they are private, and a test that
    /// could only see the public surface would be checking the queries again.
    fn assert_invariants(bvh: &Bvh) {
        if bvh.root == NIL {
            assert_eq!(bvh.live_elements, 0, "empty tree claims live elements");
            return;
        }
        assert_eq!(
            bvh.nodes[bvh.root as usize].parent, NIL,
            "the root has a parent"
        );

        let mut leaves = 0usize;
        let mut stack = vec![bvh.root];
        while let Some(index) = stack.pop() {
            let node = bvh.nodes[index as usize];
            if node.leaf_count > 0 {
                leaves += 1;
                assert_eq!(node.height, 0, "leaf {index} has non-zero height");
                assert_eq!(
                    bvh.element_node[node.child_left as usize], index,
                    "leaf {index} and its element disagree about who owns whom"
                );
                continue;
            }

            let (left, right) = (node.child_left, node.child_right);
            assert_ne!(left, right, "node {index} has the same child twice");
            for child in [left, right] {
                assert_eq!(
                    bvh.nodes[child as usize].parent, index,
                    "child {child} of {index} points at a different parent"
                );
            }

            let (lh, rh) = (
                bvh.nodes[left as usize].height,
                bvh.nodes[right as usize].height,
            );
            assert_eq!(
                node.height,
                1 + lh.max(rh),
                "node {index} has a stale height"
            );
            assert!(
                lh.abs_diff(rh) <= 1,
                "node {index} is unbalanced: heights {lh} and {rh}"
            );
            assert_eq!(
                node.aabb,
                bvh.nodes[left as usize]
                    .aabb
                    .union(bvh.nodes[right as usize].aabb),
                "node {index} has stale bounds"
            );

            stack.push(left);
            stack.push(right);
        }

        assert_eq!(leaves, bvh.live_elements, "leaf count disagrees with len()");
    }

    #[test]
    fn bulk_build_satisfies_the_invariants() {
        for count in [1usize, 2, 3, 5, 8, 33, 64] {
            let bvh = Bvh::build((0..count).map(|i| {
                (
                    Aabb::from_centre_half(
                        DVec3::new(i as f64 * 3.0, (i % 7) as f64, (i % 3) as f64),
                        DVec3::splat(0.5),
                    ),
                    i as u32,
                )
            }));
            assert_eq!(bvh.len(), count);
            assert_invariants(&bvh);
        }
    }

    #[test]
    fn insert_into_empty_tree_makes_a_one_leaf_tree() {
        let mut bvh = Bvh::build([]);
        assert!(bvh.is_empty());
        assert_eq!(bvh.depth(), 0);

        let index = bvh.insert(unit_box_at(DVec3::new(5.0, 0.0, 0.0)).0, 7);
        assert_eq!(index, 0);
        assert_eq!(bvh.len(), 1);
        assert_eq!(bvh.depth(), 1);
        assert_invariants(&bvh);

        let hits = bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].element_id, 7);
    }

    #[test]
    fn removing_the_last_element_empties_the_tree() {
        let mut bvh = Bvh::build([unit_box_at(DVec3::ZERO)]);
        assert!(bvh.remove(0));
        assert!(bvh.is_empty());
        assert_eq!(bvh.depth(), 0);
        assert!(
            bvh.traverse_aabb(&Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(100.0)))
                .is_empty()
        );
        assert!(
            bvh.traverse_ray(&Ray::new(DVec3::ZERO, DVec3::X))
                .is_empty()
        );
        assert_invariants(&bvh);

        // And it comes back to life.
        let index = bvh.insert(unit_box_at(DVec3::ZERO).0, 5);
        assert_eq!(bvh.len(), 1);
        assert_invariants(&bvh);
        assert!(bvh.update_aabb(index, unit_box_at(DVec3::new(9.0, 0.0, 0.0)).0));
    }

    #[test]
    fn remove_reports_false_for_indices_that_name_nothing() {
        let mut bvh = Bvh::build([unit_box_at(DVec3::ZERO), unit_box_at(DVec3::X * 10.0)]);
        assert!(!bvh.remove(99), "index past the end");
        assert!(bvh.remove(1));
        assert!(!bvh.remove(1), "already removed");
        assert!(!bvh.update_aabb(1, Aabb::EMPTY), "refit of a dead element");
        assert_eq!(bvh.len(), 1);
        assert_invariants(&bvh);
    }

    #[test]
    fn churn_preserves_the_invariants() {
        // Insert-heavy, then remove-heavy, then alternating: each phase drives
        // a different rotation case, and the invariants are checked after
        // every single operation rather than at the end, so a violation is
        // reported at the operation that caused it.
        let mut bvh = Bvh::build([]);
        let mut live: Vec<usize> = Vec::new();
        let mut counter = 0u32;

        let spawn = |bvh: &mut Bvh, live: &mut Vec<usize>, counter: &mut u32| {
            let n = *counter as f64;
            let aabb = Aabb::from_centre_half(
                DVec3::new((n * 7.3) % 50.0, (n * 3.1) % 30.0, (n * 1.7) % 20.0),
                DVec3::splat(0.75),
            );
            live.push(bvh.insert(aabb, *counter));
            *counter += 1;
        };

        for _ in 0..40 {
            spawn(&mut bvh, &mut live, &mut counter);
            assert_invariants(&bvh);
        }
        // Remove from the front, which is the oldest and deepest-buried.
        for _ in 0..20 {
            let index = live.remove(0);
            assert!(bvh.remove(index));
            assert_invariants(&bvh);
        }
        for step in 0..40 {
            if step % 2 == 0 {
                spawn(&mut bvh, &mut live, &mut counter);
            } else {
                let index = live.swap_remove(step % live.len());
                assert!(bvh.remove(index));
            }
            assert_invariants(&bvh);
        }
        assert_eq!(bvh.len(), live.len());
    }

    #[test]
    fn insert_and_remove_do_not_rebuild_the_node_array() {
        // The point of incremental churn is that steady-state spawning and
        // killing does not keep allocating. One removal frees two node slots
        // (the leaf and its parent) and one insertion claims them back, so a
        // balanced sequence must leave the array exactly the size it was.
        let mut bvh =
            Bvh::build((0..32).map(|i| (unit_box_at(DVec3::new(i as f64 * 2.0, 0.0, 0.0)).0, i)));
        let nodes_after_build = bvh.nodes.len();
        let mut live: Vec<usize> = (0..32).collect();

        for i in 0..200u32 {
            let index = live.swap_remove((i as usize * 7) % live.len());
            assert!(bvh.remove(index));
            live.push(bvh.insert(
                unit_box_at(DVec3::new((i % 40) as f64 * 2.0, 1.0, 0.0)).0,
                1000 + i,
            ));
        }

        assert_eq!(
            bvh.nodes.len(),
            nodes_after_build,
            "churn grew the node array instead of recycling slots"
        );
        assert_eq!(bvh.element_indices.len(), 32, "element slots leaked");
        assert_invariants(&bvh);
    }

    #[test]
    fn ray_on_face_with_zero_dir_through_bvh_hits() {
        // Ray starts on +Y face of element with zero Y dir component.
        // Exercises intersect_ray_entry NaN fix.
        let centre = DVec3::new(5.0, 0.0, 0.0);
        let bvh = Bvh::build([unit_box_at(centre)]);
        // AABB spans: min = (4.5, -0.5, -0.5), max = (5.5, 0.5, 0.5).
        // Ray origin on +Y face, dir = +X (zero Y component).
        let ray = Ray::new(DVec3::new(0.0, 0.5, 0.0), DVec3::X);
        let hits = bvh.traverse_ray(&ray);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].element_id, 0);
    }
}
