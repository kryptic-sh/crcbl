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
//! the observable, and `crates/crcbl-phys/tests/broadphase_churn.rs` bounds it across
//! thousands of operations including that degenerate input.
//!
//! # Layers
//!
//! Every element carries a `u32` of layer bits ([`Bvh::insert_layered`],
//! [`Bvh::build_layered`], [`Bvh::set_layers`]; the unlayered forms put it on
//! every bit), and every internal node the union of its subtree's. The masked
//! traversals ([`Bvh::traverse_aabb_masked_into`],
//! [`Bvh::traverse_ray_masked_into`]) skip any subtree whose union shares no
//! bit with the query's mask, which is what lets a query that looks at a few
//! static colliders walk past a crowd of characters on other layers without
//! descending to each of them. A leaf is reported only if an unmasked
//! traversal would report it *and* its own bits meet the mask, in the same
//! order, because every ancestor of such a leaf carries its bits too.

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

/// The layer bits an element gets from the unlayered [`Bvh::build`] and
/// [`Bvh::insert`]: every one.
const ALL_BITS: u32 = u32::MAX;

/// One element on its way into [`Bvh::build_layered`]: its position in the
/// input travels with it so the centroid sort cannot lose it.
#[derive(Debug, Clone, Copy)]
struct BuildItem {
    aabb: Aabb,
    element_id: u32,
    build_index: u32,
    layers: u32,
}

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
    /// Leaf: its element's layer bits. Internal: the union of its two
    /// children's, so a subtree with no bit in a query's mask holds no leaf
    /// that has one. Maintained by every structural change and by
    /// [`Bvh::set_layers`]; [`Bvh::update_aabb`] moves no node and leaves it.
    layers: u32,
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
        Self::build_layered(
            elements
                .into_iter()
                .map(|(aabb, element_id)| (aabb, element_id, ALL_BITS)),
        )
    }

    /// [`Bvh::build`] from (AABB, element_id, layers) triples, each element on
    /// the layer bits given rather than on every one.
    #[must_use]
    pub fn build_layered(elements: impl IntoIterator<Item = (Aabb, u32, u32)>) -> Self {
        // Each item carries its position in the input array so the centroid
        // sort below cannot detach `element_node` from the build order that
        // `update_aabb` is indexed by.
        let mut items: Vec<BuildItem> = elements
            .into_iter()
            .enumerate()
            .map(|(build_index, (aabb, element_id, layers))| BuildItem {
                aabb,
                element_id,
                build_index: build_index as u32,
                layers,
            })
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
    /// Leaves record their node and their element id against
    /// [`BuildItem::build_index`], never against the leaf's own position, so
    /// that a leaf's `child_left` is the same element index that
    /// [`Bvh::update_aabb`] and [`Bvh::remove`] take.
    fn build_rec(
        items: &mut [BuildItem],
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
            aabb = aabb.union(item.aabb);
        }

        if count == 1 {
            // Leaf node: the element index *is* the build index, so one number
            // addresses the leaf, `element_indices` and `element_node` alike.
            let item = items[range_start];
            let elem_idx = item.build_index;
            element_indices[elem_idx as usize] = item.element_id;
            let node_idx = nodes.len() as u32;
            element_node[elem_idx as usize] = node_idx;
            nodes.push(Node {
                aabb,
                child_left: elem_idx,
                child_right: 0,
                parent,
                leaf_count: 1,
                height: 0,
                layers: item.layers,
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

        // `total_cmp`, not `partial_cmp().unwrap_or(Equal)`. The old form
        // neither panicked nor broke `sort_by`'s contract, but a `NaN` centroid
        // made the comparator non-transitive — `NaN` compares Equal to
        // everything while the finite keys still order among themselves — and a
        // non-transitive comparator turns this spatial median split into an
        // arbitrary partition. `total_cmp` is a total order over every `f64`
        // including `NaN`, so the split degrades to "the NaN sorts to one end"
        // rather than to nothing. `crcbl_render::shadow` and
        // `crcbl_scene::simplify` already choose it for the same reason.
        items[range_start..range_end].sort_by(|a, b| {
            let ka = a.aabb.centre().to_array()[axis];
            let kb = b.aabb.centre().to_array()[axis];
            ka.total_cmp(&kb)
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
            layers: 0,
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
        nodes[node_idx as usize].layers =
            nodes[left as usize].layers | nodes[right as usize].layers;
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
        self.aabb_walk::<false>(query, ALL_BITS, stack, out);
    }

    /// [`traverse_aabb_into`](Self::traverse_aabb_into) reporting only the
    /// elements whose layer bits share at least one bit with `mask`, and never
    /// descending into a subtree that holds none.
    ///
    /// `out` is exactly what the unmasked form writes with every element whose
    /// layers miss `mask` taken out, in the same order — the pruning only ever
    /// skips subtrees whose leaves would all have been taken out. A mask of `0`
    /// reports nothing.
    pub fn traverse_aabb_masked_into(
        &self,
        query: &Aabb,
        mask: u32,
        stack: &mut Vec<u32>,
        out: &mut Vec<u32>,
    ) {
        self.aabb_walk::<true>(query, mask, stack, out);
    }

    /// The one AABB descent, pruning on `mask` when `MASKED`. A const
    /// parameter rather than a mask of every bit for the unmasked form,
    /// because an element on no layer at all is still reported by that one.
    fn aabb_walk<const MASKED: bool>(
        &self,
        query: &Aabb,
        mask: u32,
        stack: &mut Vec<u32>,
        out: &mut Vec<u32>,
    ) {
        out.clear();
        stack.clear();
        if self.root == NIL {
            return;
        }
        stack.push(self.root);

        while let Some(node_idx) = stack.pop() {
            let node = &self.nodes[node_idx as usize];
            if (MASKED && node.layers & mask == 0) || !node.aabb.intersects(query) {
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
        let mut hits = Vec::new();
        self.traverse_ray_into(ray, &mut Vec::with_capacity(64), &mut hits);
        hits
    }

    /// [`traverse_ray`](Self::traverse_ray) writing into buffers the caller
    /// owns, for the same reason
    /// [`traverse_aabb_into`](Self::traverse_aabb_into) exists — and
    /// additionally because [`crate::world::OverlapQueries`] holds only a
    /// shared borrow of the world and so cannot reach the world's own buffers.
    ///
    /// Both `out` and the descent `stack` are **cleared** and then filled, so
    /// what they held before never reaches an answer.
    pub fn traverse_ray_into(&self, ray: &Ray, stack: &mut Vec<u32>, out: &mut Vec<BvhHit>) {
        self.ray_walk::<false>(ray, ALL_BITS, stack, out);
    }

    /// [`traverse_ray_into`](Self::traverse_ray_into) reporting only the
    /// elements whose layer bits share at least one bit with `mask`, and never
    /// descending into a subtree that holds none — the ray counterpart of
    /// [`traverse_aabb_masked_into`](Self::traverse_aabb_masked_into), with the
    /// same guarantee: the unmasked answer with the other elements taken out,
    /// in the same order, at the same `t`.
    pub fn traverse_ray_masked_into(
        &self,
        ray: &Ray,
        mask: u32,
        stack: &mut Vec<u32>,
        out: &mut Vec<BvhHit>,
    ) {
        self.ray_walk::<true>(ray, mask, stack, out);
    }

    /// The one ray descent, pruning on `mask` when `MASKED`, as
    /// [`aabb_walk`](Self::aabb_walk) does.
    fn ray_walk<const MASKED: bool>(
        &self,
        ray: &Ray,
        mask: u32,
        stack: &mut Vec<u32>,
        out: &mut Vec<BvhHit>,
    ) {
        out.clear();
        stack.clear();
        if self.root == NIL {
            return;
        }

        let inv_dir = ray.dir.recip();
        let dir_is_neg = [ray.dir.x < 0.0, ray.dir.y < 0.0, ray.dir.z < 0.0];
        let t_min = ray.t_min;
        let t_max = ray.t_max;

        stack.push(self.root);

        while let Some(node_idx) = stack.pop() {
            let node = &self.nodes[node_idx as usize];
            if (MASKED && node.layers & mask == 0)
                || !node
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
                    out.push(BvhHit {
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

    /// Put one element on the layer bits `layers`, replacing what it was on,
    /// and bring every ancestor's union up to date. Returns `false` if the
    /// index names no element, as [`Bvh::update_aabb`] does.
    ///
    /// O(depth) at most, and it stops at the first ancestor whose union comes
    /// out unchanged, since every one above that is unchanged too.
    pub fn set_layers(&mut self, element_index: usize, layers: u32) -> bool {
        let Some(leaf_node) = self.leaf_of(element_index) else {
            return false;
        };
        self.nodes[leaf_node as usize].layers = layers;
        let mut current = self.nodes[leaf_node as usize].parent;
        while current != NIL {
            let node = self.nodes[current as usize];
            let union = self.nodes[node.child_left as usize].layers
                | self.nodes[node.child_right as usize].layers;
            if union == node.layers {
                break;
            }
            self.nodes[current as usize].layers = union;
            current = node.parent;
        }
        true
    }

    /// Put every node on every layer bit, so the masked walks prune nothing:
    /// the unpruned reference the query world's layer tests compare against.
    #[cfg(test)]
    pub(crate) fn saturate_layers(&mut self) {
        for node in &mut self.nodes {
            node.layers = ALL_BITS;
        }
    }

    // ── Churn: insert / remove ─────────────────────────────────────────

    /// Insert one element, returning the element index that names it.
    ///
    /// The returned index is the handle for [`Bvh::update_aabb`],
    /// [`Bvh::set_layers`] and [`Bvh::remove`], and it is in the same space as
    /// the build-order indices
    /// [`Bvh::build`] hands out. Indices of removed elements are recycled, so
    /// an index is only meaningful until the element it names is removed.
    ///
    /// The new leaf is paired with the sibling chosen by
    /// the surface area heuristic; a fresh internal
    /// node takes the sibling's place and adopts both, and the path back to the
    /// root is refit and rebalanced. Cost is one root-to-leaf descent plus one
    /// walk back up.
    pub fn insert(&mut self, aabb: Aabb, element_id: u32) -> usize {
        self.insert_layered(aabb, element_id, ALL_BITS)
    }

    /// [`Bvh::insert`] with the element on the layer bits `layers` rather than
    /// on every one. The returned index is the same handle, for
    /// [`Bvh::update_aabb`], [`Bvh::set_layers`] and [`Bvh::remove`].
    pub fn insert_layered(&mut self, aabb: Aabb, element_id: u32, layers: u32) -> usize {
        let element_index = self.alloc_element(element_id);
        let leaf = self.alloc_node(Node {
            aabb,
            child_left: element_index,
            child_right: 0,
            parent: NIL,
            leaf_count: 1,
            height: 0,
            layers,
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
            layers: layers | self.nodes[sibling as usize].layers,
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

    /// Allocated node slots, including those a [`Bvh::remove`] recycled.
    ///
    /// A balanced binary tree over `n` leaves holds about `2n − 1` nodes; the
    /// number drifting up past that is the memory footprint of churn, which is
    /// what [`PhysicsWorld::broadphase_stats`](crate::world::PhysicsWorld::broadphase_stats)
    /// reports it for.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The longest root-to-leaf path, counted in nodes (a one-element tree has
    /// depth 1, an empty one 0).
    ///
    /// This is the tree-quality observable: query cost is bounded by how far a
    /// traversal can descend, so a depth that grows past `O(log n)` under churn
    /// is the shape of degradation, and this is what
    /// `crates/crcbl-phys/tests/broadphase_churn.rs` asserts a bound on. It walks the
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

    /// Sum of the half-surface areas of every live internal node.
    ///
    /// A refit keeps every box tight around its children but does not move a
    /// leaf back beside its new neighbours. The resulting overlap makes this
    /// sum grow even while [`depth`](Self::depth) and the node count stay
    /// unchanged, so this is the diagnostic that distinguishes a balanced but
    /// spatially loose tree from a freshly partitioned one.
    ///
    /// Only nodes reachable from the root are counted. Removed nodes leave
    /// recycled slots in `nodes`, and folding those stale boxes in would make
    /// churn look like query work that traversal cannot reach.
    ///
    /// This walks the tree in `O(n)` and allocates a traversal stack. It is for
    /// diagnostics and policy measurements, not for each query.
    #[must_use]
    pub fn surface_area_cost(&self) -> f64 {
        if self.root == NIL {
            return 0.0;
        }
        let mut cost = 0.0;
        let mut stack = vec![self.root];
        while let Some(index) = stack.pop() {
            let node = self.nodes[index as usize];
            if node.leaf_count == 0 {
                cost += node.aabb.half_surface_area();
                stack.push(node.child_right);
                stack.push(node.child_left);
            }
        }
        cost
    }

    /// Internal-node surface-area cost relative to the root's surface area.
    ///
    /// The quotient is unchanged when the whole world scales uniformly, so a
    /// quality policy does not mistake a larger but equally partitioned world
    /// for a looser tree. A point-sized tree has no traversal surface and
    /// therefore no degradation signal.
    pub(crate) fn normalized_surface_area_cost(&self) -> f64 {
        if self.root == NIL {
            return 0.0;
        }
        let root_area = self.nodes[self.root as usize].aabb.half_surface_area();
        if root_area == 0.0 {
            0.0
        } else {
            self.surface_area_cost() / root_area
        }
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
            self.nodes[current as usize].layers = left.layers | right.layers;
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

    /// Recompute one internal node's bounds, height and layers from its
    /// children.
    fn recompute(&mut self, index: u32) {
        let node = self.nodes[index as usize];
        let left = self.nodes[node.child_left as usize];
        let right = self.nodes[node.child_right as usize];
        self.nodes[index as usize].aabb = left.aabb.union(right.aabb);
        self.nodes[index as usize].height = 1 + left.height.max(right.height);
        self.nodes[index as usize].layers = left.layers | right.layers;
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
    use crcbl_core::rand::hash_unit;

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
    fn surface_area_cost_sums_the_internal_boxes() {
        let bvh = Bvh::build([
            unit_box_at(DVec3::ZERO),
            unit_box_at(DVec3::new(2.0, 0.0, 0.0)),
        ]);
        assert_eq!(bvh.surface_area_cost(), 7.0);
    }

    #[test]
    fn normalized_surface_area_cost_is_scale_invariant() {
        let build = |scale: f64| {
            Bvh::build([
                (
                    Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(0.5 * scale)),
                    0,
                ),
                (
                    Aabb::from_centre_half(
                        DVec3::new(2.0 * scale, 0.0, 0.0),
                        DVec3::splat(0.5 * scale),
                    ),
                    1,
                ),
            ])
        };
        assert_eq!(
            build(1.0).normalized_surface_area_cost(),
            build(10.0).normalized_surface_area_cost()
        );
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
    fn a_segment_crossing_a_leaf_reports_that_leaf_as_a_hit() {
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
            assert_eq!(
                node.layers,
                bvh.nodes[left as usize].layers | bvh.nodes[right as usize].layers,
                "node {index} has a stale layer union"
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

    /// The layers, in the 4 bits the masked tests deal them from: few enough
    /// that subtrees with no bit of a mask are common, so pruning happens.
    fn dealt_layers(seed: u64, index: u64) -> u32 {
        (hash_unit(seed, index) * 16.0) as u32
    }

    fn dealt_box(seed: u64, index: u64) -> Aabb {
        let at = |axis: u64| hash_unit(seed, 4 * index + axis) * 40.0;
        Aabb::from_centre_half(
            DVec3::new(at(0), at(1), at(2)),
            DVec3::splat(0.25 + hash_unit(seed, 4 * index + 3)),
        )
    }

    /// The masked walks against the unmasked ones filtered by hand, for every
    /// mask of the dealt bits, and the number of answers the masked walk kept,
    /// so a caller can tell the comparison was not over empty lists.
    fn assert_masked_walks_filter_the_unmasked(bvh: &Bvh, layers_of: &[u32], probe: u64) -> usize {
        let mut stack = Vec::new();
        let (mut all, mut masked) = (Vec::new(), Vec::new());
        let (mut all_hits, mut masked_hits) = (Vec::new(), Vec::new());
        let query = dealt_box(probe, 0).inflated(6.0);
        let centre = query.centre();
        let ray = Ray::new(
            centre - DVec3::new(30.0, 7.0, 3.0),
            DVec3::new(30.0, 7.0, 3.0),
        );
        let mut kept = 0;
        for mask in 0..16u32 {
            bvh.traverse_aabb_into(&query, &mut stack, &mut all);
            bvh.traverse_aabb_masked_into(&query, mask, &mut stack, &mut masked);
            all.retain(|&id| layers_of[id as usize] & mask != 0);
            assert_eq!(masked, all, "aabb walk under mask {mask:#b}");
            kept += masked.len();

            bvh.traverse_ray_into(&ray, &mut stack, &mut all_hits);
            bvh.traverse_ray_masked_into(&ray, mask, &mut stack, &mut masked_hits);
            all_hits.retain(|hit| layers_of[hit.element_id as usize] & mask != 0);
            assert_eq!(masked_hits, all_hits, "ray walk under mask {mask:#b}");
            kept += masked_hits.len();
        }
        kept
    }

    /// Pruning on the layer unions changes no answer, through a bulk build
    /// and then through every kind of churn: inserts, removals, refits and
    /// layer changes, with the invariants — the unions among them — checked
    /// after each.
    #[test]
    fn masked_walks_are_the_unmasked_walks_filtered_through_churn() {
        const SEED: u64 = 0x001a_7e25;
        const START: u64 = 64;
        const STEPS: u64 = 400;
        // `layers_of[element_id]`, element ids being the order they were made.
        let mut layers_of: Vec<u32> = (0..START).map(|i| dealt_layers(SEED, i)).collect();
        let mut bvh = Bvh::build_layered(
            (0..START).map(|i| (dealt_box(SEED, i), i as u32, layers_of[i as usize])),
        );
        // (element index, element id) of each live element.
        let mut live: Vec<(usize, u32)> = (0..START as usize).map(|i| (i, i as u32)).collect();
        assert_invariants(&bvh);
        let mut kept = assert_masked_walks_filter_the_unmasked(&bvh, &layers_of, SEED);

        for step in 0..STEPS {
            let roll = hash_unit(SEED ^ 0xc0de, step);
            let pick = (hash_unit(SEED ^ 0xfeed, step) * live.len() as f64) as usize;
            if roll < 0.3 || live.is_empty() {
                let id = layers_of.len() as u32;
                layers_of.push(dealt_layers(SEED ^ 0xabc, step));
                let index =
                    bvh.insert_layered(dealt_box(SEED ^ 0xabc, step), id, layers_of[id as usize]);
                live.push((index, id));
            } else if roll < 0.5 {
                let (index, _) = live.swap_remove(pick);
                assert!(bvh.remove(index));
            } else if roll < 0.75 {
                let (index, _) = live[pick];
                assert!(bvh.update_aabb(index, dealt_box(SEED ^ 0x5e7, step)));
            } else {
                let (index, id) = live[pick];
                layers_of[id as usize] = dealt_layers(SEED ^ 0x1a7, step);
                assert!(bvh.set_layers(index, layers_of[id as usize]));
            }
            assert_invariants(&bvh);
            kept += assert_masked_walks_filter_the_unmasked(&bvh, &layers_of, SEED ^ step);
        }
        assert!(kept > 1000, "the walks compared almost nothing: {kept}");
    }

    /// A subtree whose union holds no bit of the mask is skipped, and one
    /// element moved onto that bit after the build is found again: the
    /// union it lives under picked the bit up.
    #[test]
    fn set_layers_brings_the_unions_above_it_up_to_date() {
        let mut bvh = Bvh::build_layered((0..16).map(|i| {
            (
                unit_box_at(DVec3::new(f64::from(i) * 2.0, 0.0, 0.0)).0,
                i,
                0b01,
            )
        }));
        let everything = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(100.0));
        let (mut stack, mut out) = (Vec::new(), Vec::new());
        bvh.traverse_aabb_masked_into(&everything, 0b10, &mut stack, &mut out);
        assert!(out.is_empty(), "nothing is on the second bit yet: {out:?}");

        assert!(bvh.set_layers(11, 0b10));
        assert_invariants(&bvh);
        bvh.traverse_aabb_masked_into(&everything, 0b10, &mut stack, &mut out);
        assert_eq!(out, [11]);

        // And off it again: the bit leaves every union it was the only
        // reason for.
        assert!(bvh.set_layers(11, 0b01));
        assert_invariants(&bvh);
        assert_eq!(bvh.nodes[bvh.root as usize].layers, 0b01);
        assert!(!bvh.set_layers(99, 0b10), "no such element");
    }

    /// The node as it was before it carried a layer union.
    #[allow(dead_code, reason = "only its layout is measured")]
    struct NodeWithoutLayers {
        aabb: Aabb,
        child_left: u32,
        child_right: u32,
        parent: u32,
        leaf_count: u32,
        height: u32,
    }

    /// Where `f64` is 8-aligned — 64-bit targets and wasm32 — the layer union
    /// rides in what was the node's padding, and the node is the size it was
    /// before. Where `f64` is 4-aligned (i686) there was no padding, and the
    /// node grows by the one `u32`.
    #[test]
    fn the_layer_union_costs_no_node_memory_where_f64_is_8_aligned() {
        let before = std::mem::size_of::<NodeWithoutLayers>();
        let after = std::mem::size_of::<Node>();
        if std::mem::align_of::<f64>() == 8 {
            assert_eq!(after, before);
        } else {
            assert_eq!(after, before + std::mem::size_of::<u32>());
        }
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
