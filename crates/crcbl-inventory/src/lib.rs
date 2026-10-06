//! The grid-inventory kit: one container primitive, and everything else is data.
//!
//! This is part 2 of `docs/plan/34-inventory.md` — the model half, headless and
//! renderer-free. A [`Grid`] is a `W×H` field of cells with an occupancy map
//! and an optional accept-filter; a [`Catalog`] is the item table it is
//! checked against; a [`Placement`] is one [`Stack`] sitting at a [`Cell`] in a
//! [`Rotation`]. There is no second "slot" concept anywhere in the crate: a
//! helmet slot is a `1×1` grid filtered by the `helmet` tag, an optic mount is
//! the same grid filtered by `optic`, and a backpack is a `5×10` grid filtered
//! by nothing. One primitive, one placement algorithm, one persistence format.
//!
//! # Why a crate of its own
//!
//! Inventory is simulation, not presentation, and it is the one part of the
//! plan that both halves of a session need to agree on byte for byte: the
//! server decides whether a move is legal and the client predicts the same
//! answer. That only works if the code answering is *the same code*, which
//! means it cannot live in a game, cannot live in `crcbl-ui`, and cannot carry
//! anything a browser build refuses to link. The placement half — [`Grid`],
//! [`Catalog`], [`Shape`] — depends on `serde`, `ron` and `thiserror` and has
//! no IO and no clock. The authority half — the [`Inventory`] a [`Command`] is
//! applied to and the [`stash`] — names the two engine crates the plan names
//! for it: `crcbl-core`, for the [`PlayerId`](crcbl_core::PlayerId) a stash and
//! a container's owner are keyed by, and `crcbl-store`, for the
//! [`StorageSource`](crcbl_store::StorageSource) a stash is written through.
//! `crates/crcbl-inventory/Cargo.toml` argues each.
//!
//! The arrow runs *into* this crate from games and from `crcbl` (behind the
//! `inventory` feature), never out of it. `apps/shard` is the sample whose loot
//! loop forced the kit, and `apps/breach` the second consumer.
//!
//! # The decisions this crate is built on
//!
//! **A footprint is a bitmask in one `u64`**, cut from a square
//! [`MAX_SHAPE`] cells a side. Eight by eight is exactly sixty-four bits, so a
//! footprint is one word rather than a `Vec<bool>`: turning it is arithmetic on
//! that word and walking its cells is walking its set bits. The cap is
//! deliberately loose — the widest container the plan ships is the `5×10`
//! backpack, so an item that could not be described here is one that could not
//! be carried either. A footprint past it is
//! [refused](InventoryError::ShapeTooLarge) rather than truncated, because the
//! `x == 8` bit wraps into the next row.
//!
//! **There are four rotations, not two.** The plan calls rotation an
//! involution — twice is the identity — and for the rectangles the shipped
//! table lists, it is: a `1×2` turned twice is the `1×2` it started as. That is
//! the *rectangle* case, and a bitmask footprint admits an L. An L turned once
//! is not the L turned three times, so [`Rotation`] has four arms and
//! [`Rotation::ALL`] is the order auto-placement tries them in. Nothing in the
//! crate assumes a footprint is solid.
//!
//! **No `HashMap`, anywhere.** Iteration order over a hash map is not a thing
//! two machines agree on, and every answer this crate gives — which cell
//! first-fit picks, what order [`Grid::slots`] yields — is one a prediction has
//! to match. Lookups are linear scans over `Vec`s in file order, over tables
//! with tens of entries.
//!
//! **The read paths allocate nothing.** [`Grid::can_place`],
//! [`Grid::find_slot`], [`Grid::at`], [`Grid::slot`] and [`Grid::slots`] touch
//! no allocator: occupancy is a `Vec<u16>` sized once at [`Grid::new`], and a
//! footprint's cells are the set bits of a `u64`. Once [`Grid::new`] has sized
//! that map, only [`Grid::place`] grows anything, and only when no removed slot
//! is free to reuse.
//!
//! **A catalogue is written with one pinned newline.** [`Catalog::to_ron`]
//! fixes `\n` rather than taking [`ron::ser::PrettyConfig`]'s default, which is
//! `\r\n` on Windows, for the reason `crcbl_render::stack::CameraStack`'s
//! writer does: a writer whose output depends on the host is not one a data
//! file can be kept in git as.
//!
//! # The move protocol
//!
//! Every change a player makes is a [`Command`] — `Move`, `Split`, `Merge`,
//! `Equip`, `Drop`, `TakeAll` — applied by [`Inventory::apply`] as one
//! transaction: validated and run against copies of the containers it touches,
//! committed whole or refused with a [`Refusal`] and nothing changed. Items
//! enter and leave only through [`Inventory::spawn`] and
//! [`Inventory::despawn`]; a split is the only command that mints a
//! [`StackId`], and the inventory is the one mint. A player's stash is a set of
//! containers in the same inventory, keyed by their `PlayerId`, so a move
//! between a rig and a stash is the same single transaction. [`inventory`]'s
//! module docs argue each of these.
//!
//! # What is deliberately not here
//!
//! Nesting (a grid inside a grid) and the weight rollup through it, mounts and
//! coverage, items as entities with replicated identity, access grants (which
//! containers a client is sent the contents of), the commands' wire form and
//! replication, and client optimism. Every one of them is in the plan and none
//! of them is in this crate yet; [`Grid::weight_g`] is the flat sum of what one
//! grid holds, and says so. Reach and line of sight are not here by design: a
//! command takes the caller's reach check, because distance is the world's.
//! Icons are not here either — an [`ItemDef`] carries a
//! [`letter`](ItemDef::letter) and a [`colour`](ItemDef::colour), which is the
//! placeholder cell a panel can draw before `crcbl icon bake` exists.

pub mod catalog;
pub mod command;
pub mod grid;
pub mod inventory;
pub mod shape;
pub mod stash;
#[cfg(test)]
mod test_items;

pub use catalog::{Catalog, CatalogError, ItemDef, ItemId, Tag};
pub use command::{Access, Applied, Command, ContainerId, Held, Refusal};
pub use grid::{Grid, Placement, SlotId, Stack, StackId};
pub use inventory::Inventory;
pub use shape::{Cell, MAX_SHAPE, Rotation, Shape};
pub use stash::{StashError, StashLayout};

/// Why a footprint, a placement or a stack operation was refused.
///
/// Every variant names the numbers a caller would otherwise have to re-derive
/// to report the refusal — the cell that was taken, the filter that rejected
/// it, the maximum a stack would have exceeded — because the caller that shows
/// a player *why* is the only reason this is an enum rather than a `bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    /// A footprint wider or taller than [`MAX_SHAPE`], refused rather than
    /// truncated into the mask.
    #[error("a {w}x{h} footprint does not fit an {MAX_SHAPE}x{MAX_SHAPE} mask")]
    ShapeTooLarge {
        /// The width asked for.
        w: u8,
        /// The height asked for.
        h: u8,
    },
    /// A footprint with no cells in it, or a grid with no cells in it.
    #[error("a shape with no cells is not a footprint")]
    ShapeEmpty,
    /// The footprint would have run off the grid: `x`/`y` is the cell asked
    /// for and `w`/`h` the size of the rotated footprint placed there.
    #[error("a {w}x{h} footprint at ({x}, {y}) runs off the grid")]
    OutOfBounds {
        /// The column asked for.
        x: u8,
        /// The row asked for.
        y: u8,
        /// The rotated footprint's width.
        w: u8,
        /// The rotated footprint's height.
        h: u8,
    },
    /// A cell the footprint covers already belongs to another placement.
    #[error("cell ({x}, {y}) is taken")]
    Occupied {
        /// The column of the first taken cell, in grid coordinates.
        x: u8,
        /// The row of the first taken cell, in grid coordinates.
        y: u8,
    },
    /// The grid accepts one tag and the item does not carry it.
    #[error("this grid only takes tag {}", filter.0)]
    Filtered {
        /// The tag the grid was built with.
        filter: Tag,
    },
    /// No placement is held under that slot id — it was removed, merged away,
    /// or never existed.
    #[error("no slot {}", .0.0)]
    NoSuchSlot(SlotId),
    /// No item is held under that id in the catalogue the call was given.
    #[error("no item {} in this catalogue", .0.0)]
    NoSuchItem(ItemId),
    /// The stack being merged into is already at its definition's maximum, so
    /// nothing could move.
    #[error("a stack of {count} is already at its maximum of {max}")]
    StackOverflow {
        /// The definition's [`stack_max`](ItemDef::stack_max).
        max: u16,
        /// What the destination stack already holds.
        count: u16,
    },
    /// A split of nothing, or of the whole stack: both are the move that
    /// leaves one of the two sides empty, and neither is a split.
    #[error("cannot split {count} out of a stack of {held}")]
    BadSplit {
        /// The count asked for.
        count: u16,
        /// What the stack holds.
        held: u16,
    },
    /// Two stacks that cannot become one: different items, or a stack asked to
    /// merge into itself.
    #[error("item {} and item {} are not the same stack", a.0, b.0)]
    NotMergeable {
        /// The item the source holds.
        a: ItemId,
        /// The item the destination holds.
        b: ItemId,
    },
    /// Auto-placement swept every cell and every rotation and found none that
    /// the footprint fits in.
    #[error("nothing in this grid fits")]
    NoRoom,
}
