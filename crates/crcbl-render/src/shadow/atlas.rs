//! The shadow atlas's space, handed out a rectangle at a time.
//!
//! `docs/plan/45-shadows.md`'s atlas rung, first half: the image stops being a
//! fixed grid a light indexes into and becomes a **quadtree the renderer
//! allocates from**, so a far or small light can take a quarter or a sixteenth
//! of what a near one takes and the same image holds many more maps. The other
//! half is on the sampling side — a light's row names a slot and the shader
//! reads that slot's rectangle rather than deriving a cell from a grid — and it
//! is what lets a tile's size vary without a second sampling path.
//!
//! # A forest of quadtrees, one per [`TILE`]-sided cell
//!
//! The root of a quadtree has to be square and its levels are halvings, and the
//! atlas is neither square by construction nor a power of two: its extent is
//! [`super::ATLAS_COLUMNS`] by [`super::ATLAS_ROWS`] cells of [`TILE`] texels,
//! and all three are free to change. So the roots are those cells — [`TILES`]
//! of them, in [`tile_origin`] order — and each subdivides on its own. That is
//! the generalisation of the grid rather than a replacement for it: ask every
//! root for its whole self and the layout is the grid, texel for texel, which
//! is what a frame whose maps all take whole cells still gets and what keeps
//! every shadow golden where it was.
//!
//! # What a level is
//!
//! Level 0 is a whole root, [`TILE`] texels a side, and each level below it
//! halves that down to [`MIN_TILE`] at level [`TILE_LEVELS`] − 1. A halving is
//! a quarter of the area, so a light demoted one level costs a quarter of what
//! it did.
//!
//! **[`MIN_TILE`] is a floor nothing has measured.** That plan's priority rung
//! spends the levels above it — `super::tile_level` is the ladder — but the
//! finest a scene in this tree reaches is a quarter of a cell's side, so
//! nothing has yet shown where a halved map stops being worth its texels.
//! [`TILE_LEVELS`] is a starting point with two bounds behind it and no
//! measurement: the smallest tile is still far wider than the disc
//! `mesh.slang` filters with — a map narrow enough for that reach spends its
//! taps on its own edge clamp rather than on geometry — and one root divided
//! that far already holds more maps than the light region has slots, so a
//! deeper tree would be levels nothing could fill. `docs/backlog.md` carries
//! the sweep.
//!
//! # Determinism, because the atlas is in every golden
//!
//! [`AtlasAllocator::allocate`] takes the **lowest free node** of the level it
//! is asked for, and subdivides the lowest free node of the finest coarser
//! level when that level has none. Two frames handed the same requests in the
//! same order therefore get the same rectangles, which is what makes a shadow
//! atlas comparable across runs at all.

use super::{TILE, TILES, atlas_extent, tile_origin};

/// Sizes the allocator hands out: [`TILE`] and each halving below it, so the
/// finest is [`MIN_TILE`].
///
/// The depth of every root's quadtree. The module docs are where the floor is
/// argued, and they are plain that it is a starting point rather than a
/// measurement.
pub const TILE_LEVELS: usize = 4;

/// The smallest tile the allocator can hand out, in texels.
///
/// The last of [`TILE_LEVELS`]' halvings of [`TILE`]. A request finer than this
/// is refused rather than rounded, which is [`AtlasAllocator::allocate`]'s own
/// documented contract.
pub const MIN_TILE: u32 = TILE >> (TILE_LEVELS - 1);

const _: () = assert!(
    MIN_TILE > 0,
    "a tile of no texels is a shadow map with nothing in it; the atlas has \
     fewer halvings in it than TILE_LEVELS asks for"
);

const _: () = assert!(
    TILE.is_multiple_of(1u32 << (TILE_LEVELS - 1)),
    "every halving of a tile has to be a whole number of texels, or the \
     rectangles the allocator hands out do not tile the cell they came from"
);

/// A rectangle of the shadow atlas, in texels.
///
/// Square, because it is a node of a quadtree over a square cell — the two
/// axes are one number and a reader that took them apart would be describing
/// something this allocator cannot produce.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileRect {
    /// Texels from the atlas's left edge to this rectangle's.
    pub x: u32,
    /// Texels from its top edge to this rectangle's.
    pub y: u32,
    /// The rectangle's side, in texels. Zero for [`TileRect::EMPTY`].
    pub side: u32,
}

impl TileRect {
    /// The rectangle of an atlas slot **no map was rendered into**.
    ///
    /// Zero rather than a corner of the image, on
    /// `crate::forward`'s reason for writing the identity into a free tile's
    /// matrix: nothing samples a free slot — the rows that could name one carry
    /// [`NO_SHADOW_TILE`](crcbl_shaders::light::NO_SHADOW_TILE) — so what the
    /// value is for is a block dumped for debugging, where a side of zero says
    /// "empty" and a plausible rectangle would not.
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        side: 0,
    };

    /// The rectangle as the shader reads it: a scale into the atlas in `xy` and
    /// an offset in `zw`, so a point `t` of the tile's own `0..1` space is at
    /// `zw + t * xy`.
    ///
    /// Two scales rather than one even though the rectangle is square: the
    /// *atlas* need not be, and dividing a square by a rectangle's two extents
    /// is what puts a tile's `0..1` on both of the image's axes.
    #[must_use]
    pub fn to_uv(self) -> [f32; 4] {
        let (width, height) = atlas_extent();
        #[expect(
            clippy::cast_precision_loss,
            reason = "an atlas extent is a few thousand texels"
        )]
        let (width, height) = (width as f32, height as f32);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tile's origin and side are inside that extent"
        )]
        let (x, y, side) = (self.x as f32, self.y as f32, self.side as f32);
        [side / width, side / height, x / width, y / height]
    }
}

/// One tile the allocator handed out, and the only thing that names it.
///
/// A level and a node of that level, which is a *pure* description of a
/// rectangle — [`Tile::rect`] needs no allocator to answer and so cannot
/// disagree with one. What the allocator holds is whether the tile is still
/// out; giving it back is [`AtlasAllocator::release`], which takes this by
/// value so a caller cannot go on holding the handle it just returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Which of [`TILE_LEVELS`] sizes this is: 0 is [`TILE`], each one below
    /// halves it.
    level: usize,
    /// Which node of that level, in the order [`nodes_at`] lays them out.
    node: usize,
}

impl Tile {
    /// Which of [`TILE_LEVELS`] sizes this tile is: 0 a whole root cell, each
    /// one below a halving of it.
    ///
    /// Read by [`Selection::lay_out`](super::Selection) to decide whether the
    /// tile a slot already holds is the size that slot wants this frame — which
    /// is the whole of the static-caching rung's retention rule, and the reason
    /// a level is readable off a handle at all.
    #[must_use]
    pub const fn level(self) -> usize {
        self.level
    }

    /// This tile's side in texels: [`TILE`] halved once per level.
    #[must_use]
    pub const fn side(self) -> u32 {
        TILE >> self.level
    }

    /// Where this tile is in the atlas.
    ///
    /// A function of the handle alone. The root cell is [`tile_origin`]'s — so
    /// a level-0 tile is exactly the cell the fixed grid had — and the node's
    /// column and row inside it are the offset from there.
    #[must_use]
    pub fn rect(self) -> TileRect {
        let (root, column, row) = decompose(self.level, self.node);
        let (origin_x, origin_y) = tile_origin(root);
        let side = self.side();
        TileRect {
            x: origin_x + column * side,
            y: origin_y + row * side,
            side,
        }
    }
}

/// How many nodes level `level` has: [`TILES`] roots, each divided into four
/// once per level.
const fn nodes_at(level: usize) -> usize {
    TILES << (2 * level)
}

/// Which root, column and row node `node` of `level` is.
///
/// Row-major inside the root, roots in [`tile_origin`] order — the same
/// convention on both axes of the recursion, which is what lets [`compose`] be
/// its exact inverse.
fn decompose(level: usize, node: usize) -> (usize, u32, u32) {
    let across = 1usize << level;
    let root = node / (across * across);
    let inside = node % (across * across);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a column or row inside a root is at most 2^(TILE_LEVELS - 1)"
    )]
    let (column, row) = ((inside % across) as u32, (inside / across) as u32);
    (root, column, row)
}

/// The node of `level` at `column`, `row` of root `root`: [`decompose`]'s
/// inverse.
fn compose(level: usize, root: usize, column: u32, row: u32) -> usize {
    let across = 1usize << level;
    root * across * across + row as usize * across + column as usize
}

/// What the allocator knows about one node of one level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Node {
    /// The node does not exist as a unit of its own: a coarser node covers it.
    /// Every node below level 0 starts here.
    #[default]
    Absent,
    /// The node exists and nothing holds it.
    Free,
    /// The node exists and a caller holds it.
    Taken,
    /// The node was divided; its four children are the units that exist.
    Split,
}

/// The atlas's free space, as a quadtree over each of its [`TILES`] root cells.
///
/// [`Selection`](super::Selection) is this type's only caller in the renderer,
/// and it holds its tiles **across frames**: a slot that wants the size it
/// already has keeps the rectangle it has, and only a slot whose size changed
/// or whose map is gone hands one back through [`release`](Self::release).
/// `docs/plan/45-shadows.md`'s static-caching rung is why — an atlas re-laid
/// out from nothing every frame is one whose contents can never outlive the
/// frame that drew them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasAllocator {
    /// `nodes[level]` is the state of every node of that level, `nodes_at` of
    /// them, indexed as [`compose`] indexes them.
    ///
    /// A state per node rather than a free list per level: a free list would
    /// have to be searched anyway to keep [`allocate`](Self::allocate)
    /// deterministic, and the coalescing in [`release`](Self::release) asks
    /// about a node's three siblings, which a list cannot answer.
    nodes: Vec<Vec<Node>>,
}

impl Default for AtlasAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl AtlasAllocator {
    /// An allocator with the whole atlas free.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: (0..TILE_LEVELS)
                .map(|level| {
                    let state = if level == 0 { Node::Free } else { Node::Absent };
                    vec![state; nodes_at(level)]
                })
                .collect(),
        }
    }

    /// A tile of `level` — [`TILE`] halved `level` times — or `None` if the
    /// atlas has no room for one.
    ///
    /// A level past [`TILE_LEVELS`] is refused rather than clamped: a caller
    /// that asked for a map smaller than [`MIN_TILE`] and was handed a larger
    /// one would be told it fit a budget it does not.
    pub fn allocate(&mut self, level: usize) -> Option<Tile> {
        let node = self.free_node(level)?;
        self.nodes[level][node] = Node::Taken;
        Some(Tile { level, node })
    }

    /// Gives `tile`'s space back, merging it into its parent wherever that
    /// leaves all four of a node's children free.
    ///
    /// The merge is what makes the space *reusable at the size it was*: without
    /// it an atlas that had once handed out a quarter tile could never hand out
    /// a whole one again, however empty it was.
    ///
    /// `Selection::lay_out` is the caller: a slot whose map is gone, or whose
    /// map wants a size other than the one it holds, gives its tile back here
    /// before the frame's remaining requests are spent, and every other slot
    /// keeps the rectangle it already had. Freeing one light's tiles while its
    /// neighbours keep theirs is exactly what a tile with a lifetime longer
    /// than one frame needs from an allocator.
    pub fn release(&mut self, tile: Tile) {
        let mut level = tile.level;
        let mut node = tile.node;
        self.nodes[level][node] = Node::Free;
        while level > 0 {
            let (root, column, row) = decompose(level, node);
            let (parent_column, parent_row) = (column / 2, row / 2);
            let siblings = [
                (parent_column * 2, parent_row * 2),
                (parent_column * 2 + 1, parent_row * 2),
                (parent_column * 2, parent_row * 2 + 1),
                (parent_column * 2 + 1, parent_row * 2 + 1),
            ];
            if !siblings.iter().all(|&(column, row)| {
                self.nodes[level][compose(level, root, column, row)] == Node::Free
            }) {
                break;
            }
            for &(column, row) in &siblings {
                let sibling = compose(level, root, column, row);
                self.nodes[level][sibling] = Node::Absent;
            }
            let parent = compose(level - 1, root, parent_column, parent_row);
            self.nodes[level - 1][parent] = Node::Free;
            level -= 1;
            node = parent;
        }
    }

    /// The lowest free node of `level`, subdividing a coarser one if that level
    /// has none free.
    ///
    /// The recursion is what makes the split lazy: a level-3 request against an
    /// empty atlas splits one root, then one quarter, then one sixteenth, and
    /// leaves the three siblings at each step free for the next request of that
    /// size rather than dividing the whole image up front.
    fn free_node(&mut self, level: usize) -> Option<usize> {
        if level >= TILE_LEVELS {
            return None;
        }
        if let Some(node) = self.nodes[level]
            .iter()
            .position(|node| *node == Node::Free)
        {
            return Some(node);
        }
        if level == 0 {
            return None;
        }
        let parent = self.free_node(level - 1)?;
        let (root, column, row) = decompose(level - 1, parent);
        self.nodes[level - 1][parent] = Node::Split;
        for (across, down) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let child = compose(level, root, column * 2 + across, row * 2 + down);
            self.nodes[level][child] = Node::Free;
        }
        Some(compose(level, root, column * 2, row * 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadow::{ATLAS_COLUMNS, ATLAS_ROWS};

    /// Every rectangle the allocator hands out is inside the atlas, and no two
    /// of them overlap — **at four different sizes at once**, which is the
    /// whole of what a quadtree buys over the fixed grid it replaced.
    ///
    /// The overlap test is a texel-by-texel occupancy map rather than a
    /// pairwise rectangle intersection, because an occupancy map also answers
    /// the question a pairwise test cannot: how much of the image the
    /// allocation actually covered. A run that allocated nothing would pass a
    /// pairwise test perfectly.
    #[test]
    fn a_quadtree_hands_out_four_sizes_that_do_not_overlap() {
        let mut allocator = AtlasAllocator::new();
        let (width, height) = atlas_extent();
        let mut covered = vec![false; (width * height) as usize];
        let mut handed_out = 0usize;
        let mut area = 0u64;
        // One of every size, twice round, so a level is asked for both when its
        // own free list is empty — which is the split — and when a previous
        // request has already left a sibling free, which is not.
        for _ in 0..2 {
            for level in 0..TILE_LEVELS {
                let tile = allocator
                    .allocate(level)
                    .unwrap_or_else(|| panic!("an empty atlas has room for a level-{level} tile"));
                let rect = tile.rect();
                assert_eq!(rect.side, TILE >> level, "a level-{level} tile's side");
                assert!(
                    rect.x + rect.side <= width && rect.y + rect.side <= height,
                    "{rect:?} runs off an atlas of {width}x{height}"
                );
                for row in rect.y..rect.y + rect.side {
                    for column in rect.x..rect.x + rect.side {
                        let texel = (row * width + column) as usize;
                        assert!(
                            !covered[texel],
                            "texel ({column}, {row}) is in {rect:?} and in a tile handed out \
                             before it"
                        );
                        covered[texel] = true;
                    }
                }
                handed_out += 1;
                area += u64::from(rect.side) * u64::from(rect.side);
            }
        }
        assert_eq!(
            handed_out,
            2 * TILE_LEVELS,
            "the loop above stopped handing out tiles"
        );
        assert_eq!(
            area,
            covered.iter().filter(|texel| **texel).count() as u64,
            "the tiles' areas and the texels they covered disagree, so two of them \
             overlapped or one was counted twice"
        );
    }

    /// A released tile's space comes back — **at the size it was**, which is
    /// the merge and not merely the free.
    ///
    /// The four quarters of one root are taken and given back, and the atlas is
    /// then emptied of every whole cell: the last of those requests can only be
    /// satisfied by the root that was subdivided, so it fails unless the four
    /// quarters merged back into it.
    #[test]
    fn a_released_tile_s_space_is_reusable_at_its_own_size() {
        let mut allocator = AtlasAllocator::new();
        let quarters: Vec<Tile> = (0..4)
            .map(|index| {
                allocator
                    .allocate(1)
                    .unwrap_or_else(|| panic!("an empty atlas has room for quarter {index}"))
            })
            .collect();
        // All four came out of one root, which is what makes the merge below
        // the thing under test rather than an accident of where they landed.
        let root = quarters[0].rect();
        for quarter in &quarters {
            let rect = quarter.rect();
            assert_eq!(rect.side, TILE / 2, "a level-1 tile's side");
            assert!(
                rect.x / TILE == root.x / TILE && rect.y / TILE == root.y / TILE,
                "{rect:?} is not in the cell the first quarter came from"
            );
        }
        for quarter in quarters {
            allocator.release(quarter);
        }
        let whole: Vec<TileRect> = (0..TILES)
            .map(|index| {
                allocator
                    .allocate(0)
                    .unwrap_or_else(|| {
                        panic!("cell {index} of {TILES} is not free after every quarter came back")
                    })
                    .rect()
            })
            .collect();
        assert_eq!(whole.len(), TILES, "the loop above stopped allocating");
        assert_eq!(
            allocator.allocate(0),
            None,
            "the atlas handed out more whole cells than it has"
        );
    }

    /// Level 0 is the fixed grid this allocator replaced, cell for cell.
    ///
    /// The behaviour every shadow golden in the tree was blessed under: ask for
    /// [`TILES`] whole tiles in order and get [`tile_origin`]'s cells in order.
    /// It is what says the machinery generalised the grid rather than moving
    /// it.
    #[test]
    fn whole_tiles_come_out_where_the_fixed_grid_put_them() {
        let mut allocator = AtlasAllocator::new();
        for index in 0..TILES {
            let rect = allocator
                .allocate(0)
                .unwrap_or_else(|| panic!("cell {index} of {TILES}"))
                .rect();
            let (x, y) = tile_origin(index);
            assert_eq!(
                rect,
                TileRect { x, y, side: TILE },
                "cell {index} is not where `tile_origin` puts it"
            );
        }
    }

    /// The atlas is exactly the roots the allocator divides, and a request
    /// finer than [`MIN_TILE`] is refused rather than rounded up.
    #[test]
    fn the_roots_tile_the_atlas_and_a_finer_request_is_refused() {
        assert_eq!(
            atlas_extent(),
            (TILE * ATLAS_COLUMNS, TILE * ATLAS_ROWS),
            "the allocator's roots do not cover the image they are cut from"
        );
        assert_eq!(nodes_at(0), TILES, "level 0 is one node per root cell");
        let mut allocator = AtlasAllocator::new();
        assert_eq!(
            allocator.allocate(TILE_LEVELS),
            None,
            "a request past the finest level was answered with a tile larger than it \
             asked for"
        );
    }

    /// A tile's `0..1` maps onto exactly its own rectangle of the atlas — the
    /// arithmetic `mesh.slang` performs, read back on the host.
    ///
    /// **Exactly**, with no tolerance: every side and origin here is a whole
    /// number of texels over an extent that is a power of two times the tile,
    /// so each of these quotients is a dyadic rational a `f32` holds without
    /// rounding. A comparison with a tolerance would pass on a scale that had
    /// picked up the wrong axis's extent.
    #[test]
    fn a_rectangle_s_uv_puts_a_tile_s_corners_on_its_own_texels() {
        let (width, height) = atlas_extent();
        #[expect(
            clippy::cast_precision_loss,
            reason = "an atlas extent is a few thousand texels"
        )]
        let extent = [width as f32, height as f32];
        let mut allocator = AtlasAllocator::new();
        for level in 0..TILE_LEVELS {
            let rect = allocator
                .allocate(level)
                .unwrap_or_else(|| panic!("a level-{level} tile"))
                .rect();
            let uv = rect.to_uv();
            for (axis, extent) in extent.iter().enumerate() {
                let origin = if axis == 0 { rect.x } else { rect.y };
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an origin and a side are inside that extent"
                )]
                let (origin, side) = (origin as f32, rect.side as f32);
                assert_eq!(
                    uv[2 + axis] * extent,
                    origin,
                    "a level-{level} tile's offset on axis {axis}"
                );
                assert_eq!(
                    (uv[2 + axis] + uv[axis]) * extent,
                    origin + side,
                    "a level-{level} tile's far edge on axis {axis}"
                );
            }
        }
    }
}
