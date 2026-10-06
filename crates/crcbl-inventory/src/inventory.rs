//! The authority's side of the kit: every container one server holds, the
//! stack ids it mints, and the one way a player changes any of it.
//!
//! # A transaction cannot leave half of itself behind
//!
//! [`Inventory::apply`] runs a command against a draft: the first time a step
//! touches a container, the draft takes a copy of that container's grid, and
//! every step works on the copies. The command body borrows the inventory
//! **immutably**, so nothing it does can reach the real grids at all; only
//! the commit, after the last step has succeeded, writes the copies back and
//! advances the id mint. A refusal at any step drops the draft, and the
//! inventory is what it was down to the slot ids and the mint.
//!
//! That is the plan's "never remove, then add" made structural rather than a
//! convention each command has to keep: a cross-container move *is* a removal
//! and an insertion, and the removal only becomes real if the insertion did.
//! Copying is per touched container, at most two grids for any command, and
//! the server pays it once a command; a panel asking every frame whether a drag
//! would land uses [`Grid::can_move_within`], which copies nothing.
//!
//! # Who mints a stack id
//!
//! The inventory does, one id after another from a counter it owns
//! ([`Command::Split`] and [`Inventory::spawn`] are the only two that mint).
//! One inventory per server instance means one counter, so ids are unique
//! across everything that server holds — its players' carried rigs, the world's
//! containers and every stash — and a stack keeps its id when it crosses from
//! one to another. A container [added](Inventory::add) with ids minted
//! elsewhere (a loot roster, a starting kit) is checked against every id the
//! inventory holds, and the counter moves past the highest of them.
//!
//! # Reach is the caller's
//!
//! Whether a player can get at a container — arm's length, line of sight, a
//! corpse that has not despawned — is a fact about the world, and this crate
//! has no world. Every command therefore takes a reach check from its caller,
//! asked once for each container the command names, after the container is
//! known to exist and to be the player's to touch. The kit decides ownership
//! and every grid rule; the game decides distance.

use crcbl_core::PlayerId;

use crate::catalog::{Catalog, ItemId};
use crate::command::{Access, Applied, Command, ContainerId, Held, Refusal};
use crate::grid::{Grid, SlotId, Stack, StackId, poured};
use crate::stash::StashLayout;

/// One container: where it sits, who may touch it, and its grid.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Container {
    pub(crate) id: ContainerId,
    pub(crate) access: Access,
    pub(crate) grid: Grid,
}

/// Every container one authority holds, and the stack ids it has minted.
///
/// The stash is part of it: a player's stash grids are containers keyed by
/// [`ContainerId::Stash`], so a move between a carried rig and a stash is an
/// ordinary transaction over two containers of one inventory. A stash kept in
/// a second structure would need a two-store commit to move an item across
/// atomically; one inventory needs none. [`crate::stash`] writes and reads the
/// stash half of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    containers: Vec<Container>,
    layout: StashLayout,
    /// The next id the mint hands out. Wider than a [`StackId`] so that
    /// handing out the last one leaves a value that says the mint is spent.
    next: u64,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

/// The copies a command works on, and the mint as it will be if the command
/// lands. See the module docs.
struct Draft {
    touched: Vec<(usize, Grid)>,
    next: u64,
}

impl Draft {
    /// The draft of the container at `index`, copied from `containers` on
    /// first touch.
    fn grid<'d>(&'d mut self, containers: &[Container], index: usize) -> &'d mut Grid {
        let at = match self.touched.iter().position(|(held, _)| *held == index) {
            Some(at) => at,
            None => {
                self.touched.push((index, containers[index].grid.clone()));
                self.touched.len() - 1
            }
        };
        &mut self.touched[at].1
    }

    /// The next stack id, taken from the draft's mint.
    fn mint(&mut self) -> Result<StackId, Refusal> {
        let id = u32::try_from(self.next).map_err(|_| Refusal::IdsExhausted)?;
        self.next += 1;
        Ok(StackId(id))
    }
}

impl Inventory {
    /// An inventory holding nothing, with no stash: a game that keeps nothing
    /// for its players between sessions. [`open_stash`](Self::open_stash)
    /// opens no grid in it.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_stash(StashLayout::NONE)
    }

    /// An inventory holding nothing, whose players' stashes are laid out as
    /// `layout` when they are [opened](Self::open_stash).
    #[must_use]
    pub const fn with_stash(layout: StashLayout) -> Self {
        Self {
            containers: Vec::new(),
            layout,
            next: 0,
        }
    }

    /// Adds a game's own container as [`ContainerId::Local`]`(id)`, holding
    /// whatever `grid` already holds.
    ///
    /// # Errors
    ///
    /// [`Refusal::ContainerExists`] if the id is taken, and
    /// [`Refusal::DuplicateStack`] if `grid` holds a stack id the inventory
    /// already holds, or holds one twice. Either way nothing is added.
    pub fn add(&mut self, id: u32, access: Access, grid: Grid) -> Result<ContainerId, Refusal> {
        let id = ContainerId::Local(id);
        if self.index(id).is_some() {
            return Err(Refusal::ContainerExists(id));
        }
        let mut highest = None;
        for (index, (_, placement)) in grid.slots().enumerate() {
            let stack = placement.stack().id();
            let twice = grid
                .slots()
                .take(index)
                .any(|(_, earlier)| earlier.stack().id() == stack);
            if twice || self.holds(stack) {
                return Err(Refusal::DuplicateStack(stack));
            }
            highest = highest.max(Some(stack.0));
        }
        if let Some(highest) = highest {
            self.next = self.next.max(u64::from(highest) + 1);
        }
        self.containers.push(Container { id, access, grid });
        Ok(id)
    }

    /// Opens `player`'s stash: adds whichever of the [`StashLayout`]'s grids
    /// they do not have yet, empty. Opening a stash twice adds nothing.
    pub fn open_stash(&mut self, player: PlayerId) {
        for (grid, (w, h)) in (0..=u8::MAX).zip(self.layout.grids()) {
            let id = ContainerId::Stash { player, grid };
            if self.index(id).is_none() {
                self.containers.push(Container {
                    id,
                    access: Access::Player(player),
                    grid: Grid::new(w, h, None).expect("a stash layout holds no empty grid"),
                });
            }
        }
    }

    /// The grid of the container `id`.
    #[must_use]
    pub fn grid(&self, id: ContainerId) -> Option<&Grid> {
        self.index(id).map(|index| &self.containers[index].grid)
    }

    /// Who may act on the container `id`.
    #[must_use]
    pub fn access(&self, id: ContainerId) -> Option<Access> {
        self.index(id).map(|index| self.containers[index].access)
    }

    /// Every container, in the order it was added or opened.
    pub fn containers(&self) -> impl Iterator<Item = (ContainerId, &Grid)> {
        self.containers
            .iter()
            .map(|container| (container.id, &container.grid))
    }

    /// `player`'s stash grids and nobody else's, in grid order.
    pub fn stash(&self, player: PlayerId) -> impl Iterator<Item = (ContainerId, &Grid)> {
        self.containers().filter(
            move |(id, _)| matches!(id, ContainerId::Stash { player: owner, .. } if *owner == player),
        )
    }

    /// How a newly opened stash is laid out.
    #[must_use]
    pub const fn stash_layout(&self) -> &StashLayout {
        &self.layout
    }

    /// The id the next split or spawn will mint, if any is left.
    #[must_use]
    pub fn next_stack(&self) -> Option<StackId> {
        u32::try_from(self.next).ok().map(StackId)
    }

    /// Puts a new stack of `count` of `item` into `into`, wherever first-fit
    /// puts it, under a freshly minted id: **the explicit spawn**, and with
    /// [`despawn`](Self::despawn) the only way items enter or leave an
    /// inventory. A server's call — a loot roll, a reward — never a player's,
    /// so it takes no player and asks no reach.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchContainer`], [`Refusal::BadCount`] for `0` or past the
    /// item's stack maximum, [`Refusal::IdsExhausted`], or the grid's own
    /// refusal — [`InventoryError::NoSuchItem`](crate::InventoryError::NoSuchItem),
    /// `Filtered` or `NoRoom`. Nothing is minted unless the stack lands.
    pub fn spawn(
        &mut self,
        catalog: &Catalog,
        into: ContainerId,
        item: ItemId,
        count: u16,
    ) -> Result<StackId, Refusal> {
        let index = self.index(into).ok_or(Refusal::NoSuchContainer(into))?;
        let max = catalog
            .get(item)
            .ok_or(crate::InventoryError::NoSuchItem(item))?
            .stack_max();
        if count == 0 || count > max {
            return Err(Refusal::BadCount { count, max });
        }
        let id = u32::try_from(self.next).map_err(|_| Refusal::IdsExhausted)?;
        self.containers[index]
            .grid
            .insert(catalog, Stack::new(item, StackId(id), count))?;
        self.next += 1;
        Ok(StackId(id))
    }

    /// Takes the stack `held` out of the inventory and hands it back: **the
    /// explicit despawn** — consumed, sold, destroyed. A server's call, like
    /// [`spawn`](Self::spawn).
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchContainer`] or [`Refusal::NoSuchStack`].
    pub fn despawn(&mut self, held: Held) -> Result<Stack, Refusal> {
        let index = self
            .index(held.container)
            .ok_or(Refusal::NoSuchContainer(held.container))?;
        let slot = self.slot_of(index, held)?;
        Ok(self.containers[index].grid.remove(slot)?)
    }

    /// Applies `command` on `player`'s behalf as one transaction.
    ///
    /// Every container the command names must exist, be `player`'s to touch
    /// ([`Access`]), and pass `reach` — asked as `reach(player, container)` —
    /// before any grid is looked at. Then every step runs against copies of
    /// the containers it touches, and the copies replace the originals only
    /// when the last step has succeeded. See the module docs.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] that stopped it, and then **nothing changed**: no grid,
    /// no slot id, no count and not the id mint.
    pub fn apply(
        &mut self,
        catalog: &Catalog,
        player: PlayerId,
        command: Command,
        reach: impl Fn(PlayerId, ContainerId) -> bool,
    ) -> Result<Applied, Refusal> {
        let mut draft = Draft {
            touched: Vec::new(),
            next: self.next,
        };
        let applied = self.run(&mut draft, catalog, player, command, &reach)?;
        for (index, grid) in draft.touched {
            self.containers[index].grid = grid;
        }
        self.next = draft.next;
        Ok(applied)
    }

    /// The command body: every check and every step, against `draft` only.
    fn run(
        &self,
        draft: &mut Draft,
        catalog: &Catalog,
        player: PlayerId,
        command: Command,
        reach: &dyn Fn(PlayerId, ContainerId) -> bool,
    ) -> Result<Applied, Refusal> {
        let all = &self.containers;
        match command {
            Command::Move {
                stack,
                to,
                at,
                rotation,
            } => {
                let from = self.admit(player, stack.container, reach)?;
                let to = self.admit(player, to, reach)?;
                let slot = self.slot_of(from, stack)?;
                if from == to {
                    draft
                        .grid(all, from)
                        .move_within(catalog, slot, at, rotation)?;
                } else {
                    let moving = draft.grid(all, from).remove(slot)?;
                    draft.grid(all, to).place(catalog, moving, at, rotation)?;
                }
                Ok(Applied::Moved)
            }
            Command::Split {
                stack,
                count,
                to,
                at,
                rotation,
            } => {
                let from = self.admit(player, stack.container, reach)?;
                let to = self.admit(player, to, reach)?;
                let slot = self.slot_of(from, stack)?;
                let minted = draft.mint()?;
                let taken = draft.grid(all, from).split(slot, count, minted)?;
                draft.grid(all, to).place(catalog, taken, at, rotation)?;
                Ok(Applied::Split { minted })
            }
            Command::Merge { from, into } => {
                let source = self.admit(player, from.container, reach)?;
                let target = self.admit(player, into.container, reach)?;
                let from_slot = self.slot_of(source, from)?;
                let into_slot = self.slot_of(target, into)?;
                let left = if source == target {
                    draft
                        .grid(all, source)
                        .merge(catalog, from_slot, into_slot)?
                } else {
                    let pouring = self.stack_at(source, from_slot);
                    let filling = self.stack_at(target, into_slot);
                    let moved = poured(catalog, pouring, filling)?;
                    draft
                        .grid(all, target)
                        .recount(into_slot, filling.count() + moved);
                    let left = pouring.count() - moved;
                    if left == 0 {
                        draft.grid(all, source).remove(from_slot)?;
                    } else {
                        draft.grid(all, source).recount(from_slot, left);
                    }
                    left
                };
                Ok(Applied::Merged { left })
            }
            Command::Equip { stack, slot } => {
                let from = self.admit(player, stack.container, reach)?;
                let to = self.admit(player, slot, reach)?;
                if from == to {
                    return Err(Refusal::SameContainer(slot));
                }
                if all[to].grid.filter().is_none() {
                    return Err(Refusal::NotASlot(slot));
                }
                self.quick_move(draft, catalog, from, to, stack)?;
                Ok(Applied::Equipped)
            }
            Command::Drop { stack, onto } => {
                let from = self.admit(player, stack.container, reach)?;
                let to = self.admit(player, onto, reach)?;
                if from == to {
                    return Err(Refusal::SameContainer(onto));
                }
                if all[to].access != Access::Open {
                    return Err(Refusal::NotOpen(onto));
                }
                self.quick_move(draft, catalog, from, to, stack)?;
                Ok(Applied::Dropped)
            }
            Command::TakeAll { from, to } => {
                let source = self.admit(player, from, reach)?;
                let target = self.admit(player, to, reach)?;
                if source == target {
                    return Err(Refusal::SameContainer(to));
                }
                let slots: Vec<SlotId> = all[source].grid.slots().map(|(slot, _)| slot).collect();
                if slots.is_empty() {
                    return Err(Refusal::Empty(from));
                }
                for &slot in &slots {
                    let taking = draft.grid(all, source).remove(slot)?;
                    draft.grid(all, target).insert(catalog, taking)?;
                }
                Ok(Applied::TookAll {
                    stacks: slots.len(),
                })
            }
        }
    }

    /// The stack `held` out of the container at `from` and into the one at
    /// `to`, by first-fit: what [`Command::Equip`] and [`Command::Drop`] share.
    fn quick_move(
        &self,
        draft: &mut Draft,
        catalog: &Catalog,
        from: usize,
        to: usize,
        held: Held,
    ) -> Result<(), Refusal> {
        let slot = self.slot_of(from, held)?;
        let moving = draft.grid(&self.containers, from).remove(slot)?;
        draft.grid(&self.containers, to).insert(catalog, moving)?;
        Ok(())
    }

    /// The index of the container `id`, once it is known to exist, to be
    /// `player`'s to touch, and within their reach — checked in that order.
    fn admit(
        &self,
        player: PlayerId,
        id: ContainerId,
        reach: &dyn Fn(PlayerId, ContainerId) -> bool,
    ) -> Result<usize, Refusal> {
        let index = self.index(id).ok_or(Refusal::NoSuchContainer(id))?;
        if let Access::Player(owner) = self.containers[index].access
            && owner != player
        {
            return Err(Refusal::NotYours(id));
        }
        if !reach(player, id) {
            return Err(Refusal::OutOfReach(id));
        }
        Ok(index)
    }

    /// Which slot of the container at `index` holds `held`'s stack.
    fn slot_of(&self, index: usize, held: Held) -> Result<SlotId, Refusal> {
        self.containers[index]
            .grid
            .find(held.stack)
            .ok_or(Refusal::NoSuchStack(held))
    }

    /// The stack at a slot [`slot_of`](Self::slot_of) has just found.
    fn stack_at(&self, index: usize, slot: SlotId) -> Stack {
        self.containers[index]
            .grid
            .slot(slot)
            .expect("slot_of found this slot a line ago")
            .stack()
    }

    /// Where the container `id` sits in the table.
    fn index(&self, id: ContainerId) -> Option<usize> {
        self.containers
            .iter()
            .position(|container| container.id == id)
    }

    /// Whether any container holds the stack `id`.
    fn holds(&self, id: StackId) -> bool {
        self.containers
            .iter()
            .any(|container| container.grid.find(id).is_some())
    }

    /// The containers, for the stash writer.
    pub(crate) fn table(&self) -> &[Container] {
        &self.containers
    }

    /// The raw mint, for the stash writer.
    pub(crate) const fn mint(&self) -> u64 {
        self.next
    }

    /// An inventory as a stash file describes it: the stash grids it read, and
    /// the mint as it was written. The reader has already held every grid to
    /// the placement rules and every id to uniqueness and to the mint.
    pub(crate) fn restored(layout: StashLayout, containers: Vec<Container>, next: u64) -> Self {
        Self {
            containers,
            layout,
            next,
        }
    }
}

#[cfg(test)]
mod no_dupe;
#[cfg(test)]
mod tests;
