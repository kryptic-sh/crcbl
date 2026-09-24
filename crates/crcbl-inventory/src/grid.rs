//! The one container: a field of cells, what sits in it, and the moves.

use serde::{Deserialize, Serialize};

use crate::InventoryError;
use crate::catalog::{Catalog, ItemDef, ItemId, Tag};
use crate::shape::{Cell, Rotation};

/// The occupancy entry for a cell nothing sits in.
///
/// [`SlotId`] is a `u16` index into a slot table that can never reach this
/// value — a grid is at most `255×255` cells and a placement covers at least
/// one of them, so at most `65_025` slots are ever live at once.
const FREE: u16 = u16::MAX;

/// One instance of an item: which item, how many, and who it is.
///
/// The id is minted by whoever produced the stack — a loot roll, a save being
/// read back — and this crate never invents one, because it has no clock, no
/// randomness and no allocator to mint from. `Copy`, because it is three
/// integers; keeping two copies of one id in two grids is a mistake this crate
/// does not police, and the minting side is where uniqueness is decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stack {
    item: ItemId,
    id: StackId,
    count: u16,
}

/// A stack's identity, stable across a save and a load.
///
/// `docs/plan/34-inventory.md` asks for item ids that survive persistence:
/// nothing regenerates a `StackId` on load, so a client that was holding an
/// item is holding the same item afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StackId(pub u32);

/// Where a placement sits in its grid's slot table.
///
/// Stable for as long as the placement lives: a slot freed by
/// [`Grid::remove`] is reused by a later [`Grid::place`], but no *other*
/// slot's id moves when one is freed. That is what lets a caller hold a
/// `SlotId` across an unrelated removal — a drag holds one for the length of
/// the drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SlotId(pub u16);

impl Stack {
    /// `count` of the item `item`, known as `id`.
    #[must_use]
    pub const fn new(item: ItemId, id: StackId, count: u16) -> Self {
        Self { item, id, count }
    }

    /// Which item this is a stack of.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    /// This stack's identity.
    #[must_use]
    pub const fn id(self) -> StackId {
        self.id
    }

    /// How many of the item it holds.
    #[must_use]
    pub const fn count(self) -> u16 {
        self.count
    }
}

/// A stack sitting somewhere in a grid.
///
/// [`at`](Self::at) is the cell the footprint's own origin lands on, so a
/// rotated L's `at` is still the top-left of its *rotated* bounding box and
/// not of the cell a player clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Placement {
    stack: Stack,
    at: Cell,
    rotation: Rotation,
}

impl Placement {
    /// What is sitting there.
    #[must_use]
    pub const fn stack(self) -> Stack {
        self.stack
    }

    /// The cell its rotated bounding box starts at.
    #[must_use]
    pub const fn at(self) -> Cell {
        self.at
    }

    /// How far it is turned from the way its definition spells it.
    #[must_use]
    pub const fn rotation(self) -> Rotation {
        self.rotation
    }
}

/// A `W×H` field of cells with an optional accept-filter: the crate's one
/// container, and the thing every "slot" in the plan actually is.
///
/// # Serde
///
/// A grid round-trips whole — its size, its filter, every placement's cell,
/// rotation, count and [`StackId`], and the occupancy map. The map is
/// derivable from the placements, but only against the catalogue that holds
/// their footprints, and a `Deserialize` has no catalogue; writing it is what
/// keeps a loaded grid usable without a second, catalogue-taking step that a
/// caller could forget. A save that has been tampered with below its own
/// checksum can therefore carry an occupancy map that disagrees with its
/// placements, and nothing here re-derives one; `docs/backlog.md` carries that.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    w: u8,
    h: u8,
    filter: Option<Tag>,
    occupancy: Vec<u16>,
    placements: Vec<Option<Placement>>,
}

impl Grid {
    /// An empty `w × h` grid that accepts anything, or only items carrying
    /// `filter`.
    ///
    /// A `1×1` grid with a filter is what the plan calls an equipment slot;
    /// there is no other kind.
    ///
    /// # Errors
    ///
    /// [`InventoryError::ShapeEmpty`] if either side is zero — a container
    /// with no cells is not one.
    pub fn new(w: u8, h: u8, filter: Option<Tag>) -> Result<Self, InventoryError> {
        if w == 0 || h == 0 {
            return Err(InventoryError::ShapeEmpty);
        }
        Ok(Self {
            w,
            h,
            filter,
            occupancy: vec![FREE; usize::from(w) * usize::from(h)],
            placements: Vec::new(),
        })
    }

    /// How many cells across.
    #[must_use]
    pub const fn width(&self) -> u8 {
        self.w
    }

    /// How many cells down.
    #[must_use]
    pub const fn height(&self) -> u8 {
        self.h
    }

    /// The one tag it accepts, if it is a filtered grid.
    #[must_use]
    pub const fn filter(&self) -> Option<Tag> {
        self.filter
    }

    /// Whether `item` could go in at `at` turned `rotation`.
    ///
    /// Allocates nothing.
    ///
    /// # Errors
    ///
    /// The reason it could not: [`NoSuchItem`](InventoryError::NoSuchItem),
    /// [`Filtered`](InventoryError::Filtered),
    /// [`OutOfBounds`](InventoryError::OutOfBounds) or
    /// [`Occupied`](InventoryError::Occupied), checked in that order.
    pub fn can_place(
        &self,
        catalog: &Catalog,
        item: ItemId,
        at: Cell,
        rotation: Rotation,
    ) -> Result<(), InventoryError> {
        let def = catalog.get(item).ok_or(InventoryError::NoSuchItem(item))?;
        self.check(def, at, rotation, None)
    }

    /// Puts `stack` in at `at`, turned `rotation`.
    ///
    /// # Errors
    ///
    /// Whatever [`can_place`](Self::can_place) refuses it for. [`Stack`] is
    /// `Copy`, so a refusal costs the caller nothing to retry elsewhere.
    pub fn place(
        &mut self,
        catalog: &Catalog,
        stack: Stack,
        at: Cell,
        rotation: Rotation,
    ) -> Result<SlotId, InventoryError> {
        let def = catalog
            .get(stack.item)
            .ok_or(InventoryError::NoSuchItem(stack.item))?;
        self.check(def, at, rotation, None)?;
        let placement = Placement {
            stack,
            at,
            rotation,
        };
        let slot = self.take_slot(placement);
        self.paint(&placement, def, slot.0);
        Ok(slot)
    }

    /// Takes the placement at `slot` out, and hands its stack back.
    ///
    /// # Errors
    ///
    /// [`InventoryError::NoSuchSlot`] if nothing is held there.
    pub fn remove(&mut self, slot: SlotId) -> Result<Stack, InventoryError> {
        let placement = self.slot(slot).ok_or(InventoryError::NoSuchSlot(slot))?;
        self.erase(slot.0);
        self.placements[usize::from(slot.0)] = None;
        Ok(placement.stack)
    }

    /// Whether [`move_within`](Self::move_within) would move the placement at
    /// `slot` to `at`, turned `rotation`: the same check, cells the placement
    /// itself covers counting as free, without moving anything.
    ///
    /// What a drag's `can_accept` asks every frame it hovers a cell — the
    /// question used to be answered by cloning the grid and trying the move.
    /// Allocates nothing.
    ///
    /// # Errors
    ///
    /// As [`move_within`](Self::move_within).
    pub fn can_move_within(
        &self,
        catalog: &Catalog,
        slot: SlotId,
        at: Cell,
        rotation: Rotation,
    ) -> Result<(), InventoryError> {
        let placement = self.slot(slot).ok_or(InventoryError::NoSuchSlot(slot))?;
        let def = catalog
            .get(placement.stack.item)
            .ok_or(InventoryError::NoSuchItem(placement.stack.item))?;
        self.check(def, at, rotation, Some(slot))
    }

    /// Moves the placement at `slot` to `at`, turned `rotation`.
    ///
    /// **Atomic.** A refused move leaves the grid exactly as it was, down to
    /// the slot id — which is why this is one call and not
    /// [`remove`](Self::remove) followed by [`place`](Self::place). Written
    /// that way, a move that ran off the edge would delete the item and hand
    /// back a stack the caller had not asked for, and a move that succeeded
    /// would hand back a *different* slot id than the one the drag was holding.
    /// The cells the placement itself covers do not count as occupied, so a
    /// one-cell nudge of a `1×2` is legal rather than a collision with itself.
    ///
    /// # Errors
    ///
    /// [`InventoryError::NoSuchSlot`] if nothing is held there, otherwise
    /// whatever [`can_place`](Self::can_place) refuses the destination for.
    pub fn move_within(
        &mut self,
        catalog: &Catalog,
        slot: SlotId,
        at: Cell,
        rotation: Rotation,
    ) -> Result<(), InventoryError> {
        self.can_move_within(catalog, slot, at, rotation)?;
        let placement = self.slot(slot).ok_or(InventoryError::NoSuchSlot(slot))?;
        let def = catalog
            .get(placement.stack.item)
            .ok_or(InventoryError::NoSuchItem(placement.stack.item))?;

        self.erase(slot.0);
        let moved = Placement {
            stack: placement.stack,
            at,
            rotation,
        };
        self.placements[usize::from(slot.0)] = Some(moved);
        self.paint(&moved, def, slot.0);
        Ok(())
    }

    /// Turns the placement at `slot` one quarter clockwise, in place.
    ///
    /// One turn, not "the first turn that fits": a player pressing the rotate
    /// key twice on an item with room for only one orientation should see it
    /// refuse and come back, not skip to the other side.
    ///
    /// # Errors
    ///
    /// [`InventoryError::NoSuchSlot`], or whatever the turned footprint is
    /// refused for — in which case nothing moved.
    pub fn rotate(&mut self, catalog: &Catalog, slot: SlotId) -> Result<Rotation, InventoryError> {
        let placement = self.slot(slot).ok_or(InventoryError::NoSuchSlot(slot))?;
        let turned = placement.rotation.turned();
        self.move_within(catalog, slot, placement.at, turned)?;
        Ok(turned)
    }

    /// The first cell and rotation `item` fits in: row-major from `(0, 0)`,
    /// trying [`Rotation::ALL`] in order at each cell.
    ///
    /// Deterministic and allocation-free, which is what makes it usable as the
    /// answer a client predicts and a server confirms.
    #[must_use]
    pub fn find_slot(&self, catalog: &Catalog, item: ItemId) -> Option<(Cell, Rotation)> {
        let def = catalog.get(item)?;
        for y in 0..self.h {
            for x in 0..self.w {
                for rotation in Rotation::ALL {
                    let at = Cell::new(x, y);
                    if self.check(def, at, rotation, None).is_ok() {
                        return Some((at, rotation));
                    }
                }
            }
        }
        None
    }

    /// Puts `stack` wherever [`find_slot`](Self::find_slot) says: the plan's
    /// `take all` and quick-move.
    ///
    /// # Errors
    ///
    /// [`NoSuchItem`](InventoryError::NoSuchItem) if the stack is not this
    /// catalogue's, [`Filtered`](InventoryError::Filtered) if the grid does not
    /// take that item at all, and [`NoRoom`](InventoryError::NoRoom) if it
    /// would but nothing fits. [`Stack`] is `Copy`, so a refusal leaves the
    /// caller holding it.
    pub fn insert(&mut self, catalog: &Catalog, stack: Stack) -> Result<SlotId, InventoryError> {
        let def = catalog
            .get(stack.item)
            .ok_or(InventoryError::NoSuchItem(stack.item))?;
        if let Some(filter) = self.filter
            && !def.has_tag(filter)
        {
            return Err(InventoryError::Filtered { filter });
        }
        let (at, rotation) = self
            .find_slot(catalog, stack.item)
            .ok_or(InventoryError::NoRoom)?;
        self.place(catalog, stack, at, rotation)
    }

    /// Pours `from` into `into` and answers what is **left in `from`**.
    ///
    /// `Ok(0)` means `from` poured entirely and its slot is gone; anything else
    /// is the remainder still sitting there. The total count across the grid is
    /// the same either way — this moves numbers between two stacks and never
    /// invents or drops one.
    ///
    /// # Errors
    ///
    /// [`NoSuchSlot`](InventoryError::NoSuchSlot) for either id,
    /// [`NotMergeable`](InventoryError::NotMergeable) if they are different
    /// items or the same slot twice, and
    /// [`StackOverflow`](InventoryError::StackOverflow) if `into` is already at
    /// its definition's maximum, which includes every item that does not stack
    /// at all.
    pub fn merge(
        &mut self,
        catalog: &Catalog,
        from: SlotId,
        into: SlotId,
    ) -> Result<u16, InventoryError> {
        let source = self
            .slot(from)
            .ok_or(InventoryError::NoSuchSlot(from))?
            .stack;
        let target = self
            .slot(into)
            .ok_or(InventoryError::NoSuchSlot(into))?
            .stack;
        if from == into || source.item != target.item {
            return Err(InventoryError::NotMergeable {
                a: source.item,
                b: target.item,
            });
        }
        let max = catalog
            .get(target.item)
            .ok_or(InventoryError::NoSuchItem(target.item))?
            .stack_max();
        if target.count >= max {
            return Err(InventoryError::StackOverflow {
                max,
                count: target.count,
            });
        }

        let moved = source.count.min(max - target.count);
        self.stack_mut(into).count += moved;
        let left = source.count - moved;
        if left == 0 {
            self.remove(from)?;
        } else {
            self.stack_mut(from).count = left;
        }
        Ok(left)
    }

    /// Takes `count` out of the stack at `slot` as a loose stack known as `id`.
    ///
    /// The new id is the caller's to supply for the same reason [`Stack`]'s is:
    /// this crate mints nothing. Taking it as an argument is what makes a split
    /// that forgot to re-id impossible to write rather than merely documented.
    ///
    /// # Errors
    ///
    /// [`NoSuchSlot`](InventoryError::NoSuchSlot), or
    /// [`BadSplit`](InventoryError::BadSplit) for `0` and for the whole stack —
    /// both are the move that leaves one side empty, and neither is a split.
    pub fn split(
        &mut self,
        slot: SlotId,
        count: u16,
        id: StackId,
    ) -> Result<Stack, InventoryError> {
        let held = self
            .slot(slot)
            .ok_or(InventoryError::NoSuchSlot(slot))?
            .stack;
        if count == 0 || count >= held.count {
            return Err(InventoryError::BadSplit {
                count,
                held: held.count,
            });
        }
        self.stack_mut(slot).count = held.count - count;
        Ok(Stack::new(held.item, id, count))
    }

    /// Which placement covers `cell`, if any. Allocates nothing.
    #[must_use]
    pub fn at(&self, cell: Cell) -> Option<SlotId> {
        if cell.x >= self.w || cell.y >= self.h {
            return None;
        }
        match self.occupancy[self.index(cell.x, cell.y)] {
            FREE => None,
            slot => Some(SlotId(slot)),
        }
    }

    /// The placement held at `slot`. Allocates nothing.
    #[must_use]
    pub fn slot(&self, slot: SlotId) -> Option<Placement> {
        self.placements.get(usize::from(slot.0)).copied().flatten()
    }

    /// Every placement, by ascending slot id. Allocates nothing.
    pub fn slots(&self) -> impl Iterator<Item = (SlotId, Placement)> {
        self.placements
            .iter()
            .enumerate()
            .filter_map(|(index, held)| Some((SlotId(u16::try_from(index).ok()?), (*held)?)))
    }

    /// What everything in the grid weighs, in grams.
    ///
    /// The flat sum of what this one grid holds. The plan's rollup through
    /// nesting is not here, because nesting is not here.
    #[must_use]
    pub fn weight_g(&self, catalog: &Catalog) -> u64 {
        self.slots()
            .map(|(_, placement)| {
                let each = catalog
                    .get(placement.stack.item)
                    .map_or(0, ItemDef::weight_g);
                u64::from(each) * u64::from(placement.stack.count)
            })
            .sum()
    }

    /// How many placements it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.placements.iter().flatten().count()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placements.iter().all(Option::is_none)
    }

    /// The occupancy entry for a cell inside the grid.
    const fn index(&self, x: u8, y: u8) -> usize {
        y as usize * self.w as usize + x as usize
    }

    /// The whole placement rule, in one read-only pass.
    ///
    /// `ignoring` is the slot whose own cells do not count as taken, which is
    /// what makes [`move_within`](Self::move_within) atomic without a
    /// mutate-and-restore path: nothing has moved by the time this answers.
    fn check(
        &self,
        def: &ItemDef,
        at: Cell,
        rotation: Rotation,
        ignoring: Option<SlotId>,
    ) -> Result<(), InventoryError> {
        if let Some(filter) = self.filter
            && !def.has_tag(filter)
        {
            return Err(InventoryError::Filtered { filter });
        }
        let shape = def.shape().rotated(rotation);
        if u16::from(at.x) + u16::from(shape.width()) > u16::from(self.w)
            || u16::from(at.y) + u16::from(shape.height()) > u16::from(self.h)
        {
            return Err(InventoryError::OutOfBounds {
                x: at.x,
                y: at.y,
                w: shape.width(),
                h: shape.height(),
            });
        }
        let ignored = ignoring.map_or(FREE, |slot| slot.0);
        for cell in shape.cells() {
            let x = at.x + cell.x;
            let y = at.y + cell.y;
            let held = self.occupancy[self.index(x, y)];
            if held != FREE && held != ignored {
                return Err(InventoryError::Occupied { x, y });
            }
        }
        Ok(())
    }

    /// Writes `slot` into every cell the placement covers.
    fn paint(&mut self, placement: &Placement, def: &ItemDef, slot: u16) {
        for cell in def.shape().rotated(placement.rotation).cells() {
            let index = self.index(placement.at.x + cell.x, placement.at.y + cell.y);
            self.occupancy[index] = slot;
        }
    }

    /// Frees every cell `slot` holds, wherever they are.
    fn erase(&mut self, slot: u16) {
        for held in &mut self.occupancy {
            if *held == slot {
                *held = FREE;
            }
        }
    }

    /// The lowest free entry in the slot table, reusing one a removal left.
    fn take_slot(&mut self, placement: Placement) -> SlotId {
        if let Some(index) = self.placements.iter().position(Option::is_none) {
            self.placements[index] = Some(placement);
            return SlotId(u16::try_from(index).expect("a freed slot had a valid id"));
        }
        let index = self.placements.len();
        debug_assert!(
            index < usize::from(FREE),
            "the slot table cannot outgrow a grid's cells"
        );
        self.placements.push(Some(placement));
        SlotId(u16::try_from(index).expect("a grid holds fewer placements than it has cells"))
    }

    /// The stack at a slot the caller has already found.
    fn stack_mut(&mut self, slot: SlotId) -> &mut Stack {
        self.placements[usize::from(slot.0)]
            .as_mut()
            .map(|placement| &mut placement.stack)
            .expect("the caller read this slot a line ago")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::shape::Shape;

    /// Four items covering the shapes the grid rules turn on: a `1×1` that
    /// stacks, the `1×2` the plan's pocket is sized for, a filtered `2×2`, and
    /// an L, which is the only footprint that can tell a quarter turn from
    /// three of them.
    const ITEMS: &str = r###"Catalog(
    items: [
        Item(
            name: "bandage",
            shape: ["#"],
            stack_max: 20,
            weight_g: 30,
            letter: 'b',
            colour: (0.9, 0.2, 0.2, 1.0),
        ),
        Item(
            name: "mag",
            shape: ["#", "#"],
            stack_max: 1,
            weight_g: 250,
            letter: 'm',
            colour: (0.5, 0.5, 0.5, 1.0),
        ),
        Item(
            name: "helmet",
            shape: ["##", "##"],
            tags: ["helmet"],
            stack_max: 1,
            weight_g: 1200,
            letter: 'h',
            colour: (0.3, 0.4, 0.5, 1.0),
        ),
        Item(
            name: "brace",
            shape: ["#.", "#.", "##"],
            stack_max: 1,
            weight_g: 400,
            letter: 'l',
            colour: (0.7, 0.6, 0.2, 1.0),
        ),
    ],
)"###;

    fn items() -> Catalog {
        Catalog::from_ron(ITEMS).expect("the test catalogue parses")
    }

    /// The id of `name`, which every test in here knows is in [`ITEMS`].
    fn id(catalog: &Catalog, name: &str) -> ItemId {
        catalog.id_of(name).expect("the test catalogue has it")
    }

    /// `count` of `name`, with `id` as its identity.
    fn stack(catalog: &Catalog, name: &str, identity: u32, count: u16) -> Stack {
        Stack::new(id(catalog, name), StackId(identity), count)
    }

    /// **A cell belongs to one placement.** The second half is what makes this
    /// a test of the *footprint* rather than of the origin: the mag's own cell
    /// is free, and it is refused because its lower half lands on the bandage.
    /// A `place` that computed the answer and then forgot to write the
    /// occupancy map passes neither.
    #[test]
    fn two_items_cannot_hold_the_same_cell() {
        let catalog = items();
        let mut grid = Grid::new(3, 3, None).expect("3x3 is a grid");
        let first = grid
            .place(
                &catalog,
                stack(&catalog, "bandage", 1, 1),
                Cell::new(1, 1),
                Rotation::Deg0,
            )
            .expect("an empty grid takes it");
        assert_eq!(grid.at(Cell::new(1, 1)), Some(first));
        assert_eq!(grid.len(), 1);

        assert_eq!(
            grid.place(
                &catalog,
                stack(&catalog, "bandage", 2, 1),
                Cell::new(1, 1),
                Rotation::Deg0
            ),
            Err(InventoryError::Occupied { x: 1, y: 1 }),
            "the same cell twice"
        );
        assert_eq!(
            grid.place(
                &catalog,
                stack(&catalog, "mag", 3, 1),
                Cell::new(1, 0),
                Rotation::Deg0
            ),
            Err(InventoryError::Occupied { x: 1, y: 1 }),
            "the mag's origin is free and its second cell is not"
        );
        assert_eq!(grid.len(), 1, "neither refusal put anything in");
        assert_eq!(grid.at(Cell::new(1, 0)), None);
    }

    /// **A turned item fits exactly where its turned footprint has room.** The
    /// `2×1` gap is a shape the `1×2` mag cannot occupy at rest and can
    /// occupy turned, which is the plan's "rotation is 90° and free" in the
    /// smallest grid that can state it.
    #[test]
    fn a_rotated_item_fits_exactly_where_its_turned_footprint_has_room() {
        let catalog = items();
        let mut grid = Grid::new(2, 1, None).expect("2x1 is a grid");
        let mag = id(&catalog, "mag");

        assert_eq!(
            grid.can_place(&catalog, mag, Cell::new(0, 0), Rotation::Deg0),
            Err(InventoryError::OutOfBounds {
                x: 0,
                y: 0,
                w: 1,
                h: 2
            }),
            "a 1x2 does not fit a 2x1 gap upright"
        );
        assert_eq!(
            grid.can_place(&catalog, mag, Cell::new(0, 0), Rotation::Deg90),
            Ok(())
        );
        assert_eq!(
            grid.can_place(&catalog, mag, Cell::new(0, 0), Rotation::Deg270),
            Ok(())
        );

        let slot = grid
            .place(
                &catalog,
                stack(&catalog, "mag", 1, 1),
                Cell::new(0, 0),
                Rotation::Deg90,
            )
            .expect("turned, it fits");
        assert_eq!(grid.at(Cell::new(0, 0)), Some(slot));
        assert_eq!(grid.at(Cell::new(1, 0)), Some(slot), "both cells are its");
        assert_eq!(
            grid.rotate(&catalog, slot),
            Err(InventoryError::OutOfBounds {
                x: 0,
                y: 0,
                w: 1,
                h: 2
            }),
            "there is no room to turn it back"
        );
        assert_eq!(
            grid.slot(slot).expect("it is still there").rotation(),
            Rotation::Deg90,
            "a refused turn did not turn it"
        );
    }

    /// **A merge stops at the definition's maximum and leaves the rest.** The
    /// numbers are the ones a stack rule gets wrong in both directions: a merge
    /// that ignored the cap would make a stack of 27, and one that moved the
    /// whole source regardless would lose 7. The conservation assertion is what
    /// catches either, and it is asserted over the grid rather than over the
    /// two stacks so a merge that dropped the remainder's slot fails too.
    #[test]
    fn a_merge_stops_at_the_definitions_maximum_and_leaves_the_rest() {
        let catalog = items();
        let mut grid = Grid::new(5, 10, None).expect("5x10 is a grid");
        let into = grid
            .place(
                &catalog,
                stack(&catalog, "bandage", 1, 12),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("an empty grid takes it");
        let from = grid
            .place(
                &catalog,
                stack(&catalog, "bandage", 2, 15),
                Cell::new(1, 0),
                Rotation::Deg0,
            )
            .expect("the next cell is free");
        let before: u32 = grid
            .slots()
            .map(|(_, held)| u32::from(held.stack().count()))
            .sum();

        assert_eq!(
            grid.merge(&catalog, from, into),
            Ok(7),
            "the definition's max is 20"
        );
        assert_eq!(
            grid.slot(into).expect("it is still there").stack().count(),
            20
        );
        assert_eq!(
            grid.slot(from)
                .expect("the remainder stayed")
                .stack()
                .count(),
            7
        );
        let after: u32 = grid
            .slots()
            .map(|(_, held)| u32::from(held.stack().count()))
            .sum();
        assert_eq!(
            after, before,
            "a merge moves numbers, it does not make them"
        );
        assert_eq!(after, 27);

        assert_eq!(
            grid.merge(&catalog, from, into),
            Err(InventoryError::StackOverflow { max: 20, count: 20 }),
            "the destination is full"
        );
        assert_eq!(
            grid.merge(&catalog, from, from),
            Err(InventoryError::NotMergeable {
                a: id(&catalog, "bandage"),
                b: id(&catalog, "bandage")
            }),
            "a stack does not merge into itself"
        );

        let mag = grid
            .place(
                &catalog,
                stack(&catalog, "mag", 3, 1),
                Cell::new(2, 0),
                Rotation::Deg0,
            )
            .expect("there is room");
        assert_eq!(
            grid.merge(&catalog, mag, from),
            Err(InventoryError::NotMergeable {
                a: id(&catalog, "mag"),
                b: id(&catalog, "bandage")
            }),
        );

        // The rest pours in, and the emptied slot goes with it.
        assert_eq!(grid.remove(into).expect("it is there").count(), 20);
        let room = grid
            .place(
                &catalog,
                stack(&catalog, "bandage", 4, 1),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("the cell is free again");
        assert_eq!(
            grid.merge(&catalog, from, room),
            Ok(0),
            "7 into a stack of 1"
        );
        assert_eq!(grid.slot(from), None, "an emptied source is not a slot");
        assert_eq!(grid.slot(room).expect("it grew").stack().count(), 8);
        assert_eq!(
            grid.merge(&catalog, from, room),
            Err(InventoryError::NoSuchSlot(from))
        );
    }

    /// **A split of the whole stack or of nothing is refused.** Both are the
    /// move that leaves one side holding zero, which is a stack the grid would
    /// then have to represent and does not: taking everything is a *move*, and
    /// taking nothing is not an operation. The good split beside them is the
    /// control — without it a `split` that refused everything would pass.
    #[test]
    fn a_split_of_the_whole_stack_or_of_nothing_is_refused() {
        let catalog = items();
        let mut grid = Grid::new(5, 10, None).expect("5x10 is a grid");
        let slot = grid
            .place(
                &catalog,
                stack(&catalog, "bandage", 1, 6),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("an empty grid takes it");

        assert_eq!(
            grid.split(slot, 0, StackId(2)),
            Err(InventoryError::BadSplit { count: 0, held: 6 })
        );
        assert_eq!(
            grid.split(slot, 6, StackId(2)),
            Err(InventoryError::BadSplit { count: 6, held: 6 })
        );
        assert_eq!(
            grid.split(slot, 7, StackId(2)),
            Err(InventoryError::BadSplit { count: 7, held: 6 })
        );
        assert_eq!(
            grid.slot(slot).expect("it is still there").stack().count(),
            6,
            "no refusal moved a number"
        );

        let taken = grid
            .split(slot, 2, StackId(2))
            .expect("2 out of 6 is a split");
        assert_eq!(taken.count(), 2);
        assert_eq!(taken.id(), StackId(2), "the caller's id, not the source's");
        assert_eq!(taken.item(), id(&catalog, "bandage"));
        assert_eq!(grid.slot(slot).expect("it shrank").stack().count(), 4);
        assert_eq!(
            grid.split(SlotId(9), 1, StackId(3)),
            Err(InventoryError::NoSuchSlot(SlotId(9)))
        );
    }

    /// **First-fit is the same answer every time, and it is the first in
    /// row-major order.** The sequence is spelled out rather than checked for a
    /// property, because "deterministic" is not the claim — *which* placement
    /// is. Twenty-five `1×2` mags fill a `5×10` two rows at a time, left to
    /// right, upright, and the twenty-sixth has nowhere to go.
    #[test]
    fn first_fit_is_the_same_answer_every_time_and_the_first_in_row_major_order() {
        let catalog = items();
        let mut grid = Grid::new(5, 10, None).expect("5x10 is a grid");
        let mag = id(&catalog, "mag");

        let mut found = Vec::new();
        for step in 0..25u32 {
            let (at, rotation) = grid.find_slot(&catalog, mag).expect("there is still room");
            found.push((at, rotation));
            grid.place(&catalog, Stack::new(mag, StackId(step), 1), at, rotation)
                .expect("what find_slot answered has to be placeable");
        }
        let expected: Vec<(Cell, Rotation)> = (0..25u8)
            .map(|step| (Cell::new(step % 5, 2 * (step / 5)), Rotation::Deg0))
            .collect();
        assert_eq!(found, expected, "row-major, upright, two rows at a time");
        assert_eq!(grid.find_slot(&catalog, mag), None, "the grid is full");
        assert_eq!(
            grid.insert(&catalog, Stack::new(mag, StackId(99), 1)),
            Err(InventoryError::NoRoom)
        );
        assert_eq!(grid.weight_g(&catalog), 25 * 250);

        // Rotations are tried in `Rotation::ALL` order at each cell, so a gap
        // only the turned footprint fits answers with the turn.
        let narrow = Grid::new(2, 1, None).expect("2x1 is a grid");
        assert_eq!(
            narrow.find_slot(&catalog, mag),
            Some((Cell::new(0, 0), Rotation::Deg90))
        );
    }

    /// **A refused move leaves the grid as it was.** Every failing move is run
    /// against a clone of the grid it started from and the whole structure is
    /// compared — cells, slot ids, counts and rotations — because a
    /// `move_within` written as [`Grid::remove`] then [`Grid::place`] loses the
    /// item on the failing path and renumbers its slot on the passing one, and
    /// neither shows up in a test that only asks whether the call returned an
    /// error.
    #[test]
    fn a_refused_move_leaves_the_grid_as_it_was() {
        let catalog = items();
        let mut grid = Grid::new(4, 4, None).expect("4x4 is a grid");
        let mag = grid
            .place(
                &catalog,
                stack(&catalog, "mag", 1, 1),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("an empty grid takes it");
        let brace = grid
            .place(
                &catalog,
                stack(&catalog, "brace", 2, 1),
                Cell::new(2, 0),
                Rotation::Deg0,
            )
            .expect("the right half is free");
        let before = grid.clone();

        for (name, slot, at, rotation) in [
            ("off the right edge", mag, Cell::new(4, 0), Rotation::Deg0),
            ("off the bottom", mag, Cell::new(0, 3), Rotation::Deg0),
            ("onto the brace", mag, Cell::new(2, 0), Rotation::Deg0),
            ("no such slot", SlotId(7), Cell::new(0, 0), Rotation::Deg0),
            (
                "turned onto the brace",
                mag,
                Cell::new(2, 0),
                Rotation::Deg90,
            ),
        ] {
            assert!(
                grid.move_within(&catalog, slot, at, rotation).is_err(),
                "{name} has to be refused"
            );
            assert_eq!(grid, before, "{name} moved something");
        }

        // The control: a move that is legal does move, and keeps its slot id.
        assert_eq!(
            grid.move_within(&catalog, mag, Cell::new(1, 1), Rotation::Deg0),
            Ok(())
        );
        assert_ne!(grid, before);
        assert_eq!(grid.at(Cell::new(1, 1)), Some(mag), "the same slot id");
        assert_eq!(grid.at(Cell::new(1, 2)), Some(mag));
        assert_eq!(grid.at(Cell::new(0, 0)), None, "and the old cells are free");
        assert_eq!(
            grid.at(Cell::new(3, 2)),
            Some(brace),
            "the brace never moved"
        );

        // A one-cell nudge overlaps the item's own footprint, which is not a
        // collision with itself.
        assert_eq!(
            grid.move_within(&catalog, mag, Cell::new(1, 2), Rotation::Deg0),
            Ok(())
        );
        assert_eq!(grid.at(Cell::new(1, 1)), None);
        assert_eq!(grid.at(Cell::new(1, 3)), Some(mag));
    }

    /// **`can_move_within` answers without moving anything.** Each verdict is
    /// asserted by value — a nudge over the item's own cells and a clear move
    /// accepted, a collision, an edge and an unknown slot refused with their
    /// reasons — and the grid compared whole afterwards. `move_within` runs this
    /// same check, so the refused-move test above covers it too; this one pins
    /// that asking is free of the move.
    #[test]
    fn can_move_within_answers_without_moving_anything() {
        let catalog = items();
        let mut grid = Grid::new(4, 4, None).expect("4x4 is a grid");
        let mag = grid
            .place(
                &catalog,
                stack(&catalog, "mag", 1, 1),
                Cell::new(0, 0),
                Rotation::Deg0,
            )
            .expect("an empty grid takes it");
        grid.place(
            &catalog,
            stack(&catalog, "brace", 2, 1),
            Cell::new(2, 0),
            Rotation::Deg0,
        )
        .expect("the right half is free");
        let before = grid.clone();

        let ask =
            |slot, x, y| grid.can_move_within(&catalog, slot, Cell::new(x, y), Rotation::Deg0);
        assert_eq!(ask(mag, 0, 1), Ok(()), "a nudge over its own cell");
        assert_eq!(ask(mag, 1, 1), Ok(()), "a clear move");
        assert_eq!(
            ask(mag, 2, 0),
            Err(InventoryError::Occupied { x: 2, y: 0 }),
            "onto the brace"
        );
        assert_eq!(
            ask(mag, 4, 0),
            Err(InventoryError::OutOfBounds {
                x: 4,
                y: 0,
                w: 1,
                h: 2
            }),
            "off the edge"
        );
        assert_eq!(
            ask(SlotId(7), 0, 0),
            Err(InventoryError::NoSuchSlot(SlotId(7))),
            "an empty slot"
        );
        assert_eq!(grid, before, "asking moved something");
    }

    /// **A grid round-trips through serde with every placement where it was.**
    /// Asserted field by field as well as whole, so a `Grid` that compared
    /// equal while losing a rotation — which nothing but a rotated item would
    /// notice — fails here.
    #[test]
    fn a_grid_round_trips_through_serde_with_every_placement_where_it_was() {
        let catalog = items();
        let mut grid = Grid::new(5, 10, None).expect("5x10 is a grid");
        grid.place(
            &catalog,
            stack(&catalog, "brace", 11, 1),
            Cell::new(0, 0),
            Rotation::Deg90,
        )
        .expect("an empty grid takes it");
        grid.place(
            &catalog,
            stack(&catalog, "bandage", 12, 9),
            Cell::new(4, 9),
            Rotation::Deg0,
        )
        .expect("the far corner is free");
        let mag = grid
            .place(
                &catalog,
                stack(&catalog, "mag", 13, 1),
                Cell::new(0, 3),
                Rotation::Deg0,
            )
            .expect("there is room");
        grid.remove(mag).expect("it is there");

        let text = ron::to_string(&grid).expect("a grid serialises");
        let read: Grid = ron::from_str(&text).expect("what was written parses");
        assert_eq!(read, grid, "the whole grid, occupancy included");
        assert_eq!(read.width(), 5);
        assert_eq!(read.height(), 10);
        assert_eq!(read.filter(), None);
        assert_eq!(
            read.len(),
            2,
            "the removed slot is still a hole, not a placement"
        );

        let held: Vec<(SlotId, Placement)> = read.slots().collect();
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].1.stack().id(), StackId(11));
        assert_eq!(held[0].1.at(), Cell::new(0, 0));
        assert_eq!(held[0].1.rotation(), Rotation::Deg90, "the turn survived");
        assert_eq!(held[1].1.stack().id(), StackId(12));
        assert_eq!(held[1].1.stack().count(), 9);
        assert_eq!(held[1].1.at(), Cell::new(4, 9));
        assert_eq!(
            read.at(Cell::new(2, 0)),
            Some(held[0].0),
            "the turned L's far cell"
        );
        assert_eq!(read.weight_g(&catalog), 400 + 9 * 30);

        // A filtered grid, so the `Option<Tag>` is carried as a `Some` by
        // something rather than only as the `None` above.
        let helmet_tag = catalog.tag("helmet").expect("the helmet carries it");
        let mut worn = Grid::new(2, 2, Some(helmet_tag)).expect("2x2 is a grid");
        worn.insert(&catalog, stack(&catalog, "helmet", 14, 1))
            .expect("that is what the slot is for");
        let read: Grid = ron::from_str(&ron::to_string(&worn).expect("it serialises"))
            .expect("what was written parses");
        assert_eq!(read.filter(), Some(helmet_tag), "the filter survived");
        assert_eq!(read, worn);
    }

    /// **A filtered grid takes only what it is for.** The `1×1` helmet slot is
    /// the plan's equipment slot, and the point of the model is that it is not
    /// a separate concept — so the refusal has to come from the tag and not
    /// from the size, which is why the bandage tried here also fits.
    #[test]
    fn a_filtered_grid_takes_only_what_it_is_for() {
        let catalog = items();
        let helmet_tag = catalog.tag("helmet").expect("the helmet carries it");
        let mut slot = Grid::new(2, 2, Some(helmet_tag)).expect("2x2 is a grid");
        assert_eq!(slot.filter(), Some(helmet_tag));

        assert_eq!(
            slot.can_place(
                &catalog,
                id(&catalog, "bandage"),
                Cell::new(0, 0),
                Rotation::Deg0
            ),
            Err(InventoryError::Filtered { filter: helmet_tag }),
            "it would fit, and it is not a helmet"
        );
        assert_eq!(
            slot.insert(&catalog, stack(&catalog, "bandage", 1, 1)),
            Err(InventoryError::Filtered { filter: helmet_tag }),
            "auto-placement reports the filter, not an empty grid full"
        );
        assert!(slot.is_empty());

        let worn = slot
            .insert(&catalog, stack(&catalog, "helmet", 2, 1))
            .expect("that is what the slot is for");
        assert_eq!(slot.slot(worn).expect("it went in").at(), Cell::new(0, 0));
        assert_eq!(slot.len(), 1);
        assert_eq!(
            slot.insert(&catalog, stack(&catalog, "helmet", 3, 1)),
            Err(InventoryError::NoRoom),
            "a full slot is out of room, not filtered"
        );

        let unfiltered = Grid::new(2, 2, None).expect("2x2 is a grid");
        assert_eq!(
            unfiltered.can_place(
                &catalog,
                id(&catalog, "bandage"),
                Cell::new(0, 0),
                Rotation::Deg0
            ),
            Ok(()),
            "the tag is only a rule where a grid asks for it"
        );
    }

    /// **An item this catalogue does not hold is refused rather than indexed.**
    /// [`ItemId`] is a position, so an id from another catalogue is an index
    /// into this one, and every entry point that takes one has to say no.
    #[test]
    fn an_item_from_another_catalogue_is_not_in_this_one() {
        let catalog = items();
        let mut grid = Grid::new(2, 2, None).expect("2x2 is a grid");
        let stranger = ItemId(9);
        assert_eq!(
            grid.can_place(&catalog, stranger, Cell::new(0, 0), Rotation::Deg0),
            Err(InventoryError::NoSuchItem(stranger))
        );
        assert_eq!(
            grid.place(
                &catalog,
                Stack::new(stranger, StackId(1), 1),
                Cell::new(0, 0),
                Rotation::Deg0
            ),
            Err(InventoryError::NoSuchItem(stranger))
        );
        assert_eq!(
            grid.insert(&catalog, Stack::new(stranger, StackId(1), 1)),
            Err(InventoryError::NoSuchItem(stranger))
        );
        assert_eq!(grid.find_slot(&catalog, stranger), None);
        assert_eq!(Grid::new(0, 4, None), Err(InventoryError::ShapeEmpty));
        assert_eq!(Grid::new(4, 0, None), Err(InventoryError::ShapeEmpty));
    }

    /// A grid with `count` items dropped into it wherever they landed, so the
    /// property below runs against occupancy maps nobody designed.
    fn littered(catalog: &Catalog, w: u8, h: u8, drops: &[(u16, u8, u8, usize)]) -> Grid {
        let mut grid = Grid::new(w, h, None).expect("w and h are both at least one");
        for (index, &(item, x, y, rotation)) in drops.iter().enumerate() {
            let item = ItemId(item % 4);
            let identity = StackId(u32::try_from(index).expect("a short list"));
            let _ = grid.place(
                catalog,
                Stack::new(item, identity, 1),
                Cell::new(x, y),
                Rotation::ALL[rotation],
            );
        }
        grid
    }

    proptest! {
        /// **If any placement exists, `find_slot` finds one.** The brute-force
        /// sweep is the definition of "rotation-complete" from
        /// `docs/plan/34-inventory.md`'s Testing section, and it is the half a
        /// hand-written table cannot cover: a first-fit that skipped a rotation,
        /// stopped a row early, or refused the last column would still pass
        /// every case someone thought to write down. The answer it gives is
        /// checked as well as its existence, so a `find_slot` that returned a
        /// placement the grid then refuses fails too.
        #[test]
        fn if_any_placement_exists_find_slot_finds_one(
            w in 1u8..=6,
            h in 1u8..=6,
            drops in prop::collection::vec((0u16..4, 0u8..6, 0u8..6, 0usize..4), 0..10),
            probe in 0u16..4,
        ) {
            let catalog = items();
            let grid = littered(&catalog, w, h, &drops);
            let item = ItemId(probe);

            let mut brute = None;
            'sweep: for y in 0..h {
                for x in 0..w {
                    for rotation in Rotation::ALL {
                        if grid.can_place(&catalog, item, Cell::new(x, y), rotation).is_ok() {
                            brute = Some((Cell::new(x, y), rotation));
                            break 'sweep;
                        }
                    }
                }
            }

            let found = grid.find_slot(&catalog, item);
            prop_assert_eq!(found.is_some(), brute.is_some());
            prop_assert_eq!(found, brute, "and it is the first one in sweep order");
            if let Some((at, rotation)) = found {
                prop_assert!(grid.can_place(&catalog, item, at, rotation).is_ok());
            }
        }

        /// **Every cell belongs to at most one placement, whatever was dropped
        /// in.** The occupancy map is the grid's own answer to `at`, and this
        /// walks it back against the footprints that produced it — so a `place`
        /// that painted a cell it had not checked, or a `remove` that freed one
        /// it did not own, shows up as a cell claiming a slot that does not
        /// cover it.
        #[test]
        fn occupancy_never_disagrees_with_the_placements_that_wrote_it(
            w in 1u8..=6,
            h in 1u8..=6,
            drops in prop::collection::vec((0u16..4, 0u8..6, 0u8..6, 0usize..4), 0..10),
        ) {
            let catalog = items();
            let grid = littered(&catalog, w, h, &drops);

            let mut claimed = vec![None; usize::from(w) * usize::from(h)];
            for (slot, placement) in grid.slots() {
                let def = catalog.get(placement.stack().item()).expect("it was placed from here");
                for cell in def.shape().rotated(placement.rotation()).cells() {
                    let index = grid.index(placement.at().x + cell.x, placement.at().y + cell.y);
                    prop_assert_eq!(claimed[index], None, "two placements over one cell");
                    claimed[index] = Some(slot);
                }
            }
            for y in 0..h {
                for x in 0..w {
                    prop_assert_eq!(grid.at(Cell::new(x, y)), claimed[grid.index(x, y)]);
                }
            }
            prop_assert_eq!(grid.len(), grid.slots().count());
            prop_assert_eq!(grid.is_empty(), grid.slots().next().is_none());
        }
    }

    /// A shape that is not in the catalogue, used nowhere but here: the module
    /// re-exports [`Shape`] and a test that never names it would let the
    /// re-export rot.
    #[test]
    fn the_grid_and_the_shape_agree_about_a_footprint() {
        let catalog = items();
        let brace = catalog
            .get(id(&catalog, "brace"))
            .expect("it is in the file");
        assert_eq!(
            brace.shape(),
            Shape::from_rows(&["#.", "#.", "##"]).expect("2x3")
        );
        assert_eq!(
            brace.shape().count(),
            4,
            "an L covers four of its six cells"
        );
    }
}
