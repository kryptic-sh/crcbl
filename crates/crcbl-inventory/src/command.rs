//! The move protocol: what a player may ask an [`Inventory`](crate::Inventory)
//! to do, what it answers when it does, and the named reason when it will not.
//!
//! Every mutation a player can cause is one of the six [`Command`]s
//! `docs/plan/34-inventory.md` lists, and each is applied by
//! [`Inventory::apply`](crate::Inventory::apply) as **one transaction**:
//! validated whole, then committed whole, or refused with a [`Refusal`] and
//! nothing changed — not a count, not a slot id, not the id mint. A command
//! that spans two containers, or a player's carried rig and their stash, is
//! the same single transaction and not a removal followed by an insertion.

use std::fmt;

use crcbl_core::PlayerId;

use crate::InventoryError;
use crate::grid::StackId;
use crate::shape::{Cell, Rotation};

/// Where a container sits in an [`Inventory`](crate::Inventory).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainerId {
    /// A container the game named and added: a carried rig, an equipment
    /// slot, a corpse, a crate, the ground at a player's feet.
    Local(u32),
    /// One of a player's stash grids, `grid` counting through the
    /// [`StashLayout`](crate::StashLayout) it was opened with.
    Stash {
        /// Whose stash.
        player: PlayerId,
        /// Which of its grids.
        grid: u8,
    },
}

impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(id) => write!(f, "container {id}"),
            Self::Stash { player, grid } => write!(f, "stash grid {grid} of player {player}"),
        }
    }
}

/// Who may act on a container.
///
/// A stash grid is always its own player's; a game's own containers say
/// which of these they are when they are [added](crate::Inventory::add).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Only this player: their rig, their equipment slots.
    Player(PlayerId),
    /// Anyone the caller's reach check lets near it: a corpse, a crate, the
    /// ground. Two players looting one corpse serialise on it, and the second
    /// command finds the first one's result.
    Open,
}

/// One stack, named by the container it is in and its identity.
///
/// By [`StackId`] rather than by [`SlotId`](crate::SlotId): a slot is reused
/// once its placement leaves, so a command naming a slot could land on
/// whatever another player's command put there first. Naming the stack makes
/// a stale command miss, as [`Refusal::NoSuchStack`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Held {
    /// The container the stack is in.
    pub container: ContainerId,
    /// The stack.
    pub stack: StackId,
}

impl fmt::Display for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stack {} in {}", self.stack.0, self.container)
    }
}

/// Everything a player can do to the items an inventory holds.
///
/// The explicit drags ([`Move`](Self::Move), [`Split`](Self::Split)) carry an
/// exact cell and rotation; the quick-moves ([`Equip`](Self::Equip),
/// [`Drop`](Self::Drop), [`TakeAll`](Self::TakeAll)) place by the grid's
/// deterministic first-fit, which is the answer a client predicts and the
/// server confirms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The whole stack to `at`, turned `rotation`, in `to` — its own
    /// container or another. Within one container it keeps its slot id.
    Move {
        /// What moves.
        stack: Held,
        /// Where it goes.
        to: ContainerId,
        /// The cell its rotated footprint starts at.
        at: Cell,
        /// How far it is turned.
        rotation: Rotation,
    },
    /// `count` taken out of a stack as a new stack — the only command that
    /// mints a [`StackId`] — placed at `at` in `to`.
    Split {
        /// What is divided.
        stack: Held,
        /// How many leave it: at least one, and fewer than it holds.
        count: u16,
        /// Where the new stack goes.
        to: ContainerId,
        /// The cell its rotated footprint starts at.
        at: Cell,
        /// How far it is turned.
        rotation: Rotation,
    },
    /// `from` poured into `into`, up to the item's stack maximum; what does not
    /// fit stays in `from`, and a `from` poured whole is gone.
    Merge {
        /// What pours.
        from: Held,
        /// What it pours into.
        into: Held,
    },
    /// The stack into an equipment slot — a grid with a tag filter, which is
    /// all an equipment slot is — wherever first-fit puts it.
    Equip {
        /// What is equipped.
        stack: Held,
        /// The filtered grid it goes into.
        slot: ContainerId,
    },
    /// The stack into a world container — one with [`Access::Open`], such as
    /// the ground pile a game keeps at a player's feet — wherever first-fit
    /// puts it.
    Drop {
        /// What is dropped.
        stack: Held,
        /// The world container it lands in.
        onto: ContainerId,
    },
    /// Every stack in `from` into `to`, in slot order, each wherever
    /// first-fit puts it — **all of them or none**: a take-all that runs out
    /// of room part-way is refused with the grid's reason, and the stacks
    /// that would have fitted before it stay where they were.
    TakeAll {
        /// The container emptied.
        from: ContainerId,
        /// The container filled.
        to: ContainerId,
    },
}

/// What a command did, once it had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// A [`Command::Move`] landed.
    Moved,
    /// A [`Command::Split`] landed, and the new stack is known as `minted`.
    Split {
        /// The id the inventory minted for the new stack.
        minted: StackId,
    },
    /// A [`Command::Merge`] landed, and `left` is what stayed in the source:
    /// `0` means it poured whole and is gone.
    Merged {
        /// What is still in the source stack.
        left: u16,
    },
    /// A [`Command::Equip`] landed.
    Equipped,
    /// A [`Command::Drop`] landed.
    Dropped,
    /// A [`Command::TakeAll`] landed, moving this many stacks.
    TookAll {
        /// How many stacks moved.
        stacks: usize,
    },
}

/// Why a command, a spawn or an added container was refused. Nothing changed.
///
/// Every variant names what the player would otherwise have to be told
/// without: the container that was not theirs, the stack that was not there,
/// the placement rule that said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The inventory holds no container by that id.
    #[error("there is no {0}")]
    NoSuchContainer(ContainerId),
    /// The container holds no stack by that id: it moved, merged away or never
    /// was there.
    #[error("there is no {0}")]
    NoSuchStack(Held),
    /// The container is another player's.
    #[error("{0} belongs to another player")]
    NotYours(ContainerId),
    /// The caller's reach check said the player cannot get at it.
    #[error("{0} is out of reach")]
    OutOfReach(ContainerId),
    /// An equip named a grid with no tag filter, which is not a slot.
    #[error("{0} is not an equipment slot")]
    NotASlot(ContainerId),
    /// A drop named a container that is not the world's.
    #[error("{0} is not a world container")]
    NotOpen(ContainerId),
    /// A quick-move from a container into itself, which would only shuffle it.
    #[error("the stack is already in {0}")]
    SameContainer(ContainerId),
    /// A take-all from a container holding nothing.
    #[error("{0} holds nothing to take")]
    Empty(ContainerId),
    /// A container was added under an id the inventory already holds.
    #[error("{0} is already in this inventory")]
    ContainerExists(ContainerId),
    /// A container was added holding a stack id already in the inventory, or
    /// holding one id twice: two copies of one stack are a duplicated item.
    #[error("stack {} is already in this inventory", .0.0)]
    DuplicateStack(StackId),
    /// A spawn asked for a count no stack of that item can hold.
    #[error("a stack of {count} is not one this item makes; it holds 1 to {max}")]
    BadCount {
        /// The count asked for.
        count: u16,
        /// The item's stack maximum.
        max: u16,
    },
    /// The mint has handed out every [`StackId`] there is.
    #[error("every stack id has been minted")]
    IdsExhausted,
    /// A grid's own placement or stacking rule.
    #[error(transparent)]
    Grid(#[from] InventoryError),
}
