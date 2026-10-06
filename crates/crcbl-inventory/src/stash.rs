//! The stash: a player's grids kept on the server, keyed by their
//! [`PlayerId`], and the file a server keeps them in.
//!
//! # Server-side, per server instance, never in the client profile
//!
//! `docs/plan/34-inventory.md` puts the persistent stash on the server
//! deliberately: a stash a client stored would be a client-authoritative item
//! source, which is free duplication. So this module writes through whatever
//! [`StorageSource`] the server hands it — its data directory, natively — and
//! nothing here names a profile, a config directory or a client. One file per
//! server instance holds every player's stash; a fleet of servers sharing one
//! stash is a backend's job, reached through the same seam (the plan's
//! correction of 2026-07-27).
//!
//! The stash's grids live in the server's one [`Inventory`], as
//! [`ContainerId::Stash`] containers, so taking an item out of a stash into a
//! rig is an ordinary transaction — see [`Inventory`]'s docs for why that is
//! what makes it atomic.
//!
//! # The file
//!
//! Little-endian throughout, and laid out like `crcbl-store`'s save container:
//! a magic, a format version, the body, and a checksum over everything before
//! it, written in one [`StorageSource::write`] — `write_atomic` natively, so a
//! crash mid-save leaves the previous file whole.
//!
//! | Size | Field |
//! | --- | --- |
//! | 4 | magic `MAGIC` |
//! | 2 | [`STASH_FORMAT_VERSION`] |
//! | 8 | the id mint: the next [`StackId`] the inventory would hand out |
//! | 4 | how many players follow |
//! | per player | their [`PlayerId`] (16 bytes), how many grids follow (1 byte), then each grid |
//! | per grid | its width and height (1 byte each), how many placements follow (2 bytes), then each placement |
//! | per placement | the item's stable key ([`Catalog::key`], 4), its [`StackId`] (4), its count (2), its cell (1 + 1) and its rotation (1, an index into [`Rotation::ALL`]) |
//! | 4 | CRC-32 ([`crcbl_store::crc32`]) of every byte before it |
//!
//! **The mint is written** because ids must never be handed out twice: a
//! server that restarted with its mint at zero would mint ids its stash
//! already holds. Ids minted after the last save and never saved die with the
//! process along with the stacks that carried them, so the written mint is
//! past every id the file holds and every id a restarted server mints is new.
//!
//! **A player's grids are as the file wrote them**, not as today's
//! [`StashLayout`] says: an operator who enlarges the stash changes what new
//! players open with and leaves an existing player's grids as they were,
//! rather than refusing the file. The layout is kept for the players still to
//! come.
//!
//! **Every grid is rebuilt through [`Grid::place`]**, as `apps/shard`'s save
//! is, so a file that disagrees with itself — two stacks in one cell, a stack
//! past its item's maximum, one stack id twice, an id the mint never reached —
//! reads as damaged rather than as a stash. A catalogue that has lost an item a
//! stash still holds also refuses the file: losing a player's item silently is
//! the failure a refusal exists to prevent.
//!
//! # Versions
//!
//! A file from a newer build is refused as [`StashError::Newer`] rather than
//! misread. There is no older version yet, so there is no migration chain yet:
//! the first bump of [`STASH_FORMAT_VERSION`] adds one, as a list of pure steps
//! over the body in the shape `crcbl_store::save`'s `migrate` module and
//! `crcbl_store::replay` already use.

use std::collections::BTreeSet;
use std::path::Path;

use crcbl_core::PlayerId;
use crcbl_store::crc32::crc32;
use crcbl_store::{StorageError, StorageSource};

use crate::catalog::{Catalog, ItemId};
use crate::command::{Access, ContainerId};
use crate::grid::{Grid, Stack, StackId};
use crate::inventory::{Container, Inventory};
use crate::shape::{Cell, Rotation};

/// The stash format version [`encode`] writes. Bump it on any change to the
/// layout in the module docs, and add the migration step from the version
/// before.
pub const STASH_FORMAT_VERSION: u16 = 1;

/// The most grids one player's stash holds: what the file's one-byte grid
/// count can say, and what [`ContainerId::Stash`]'s `grid` counts to.
pub const MAX_STASH_GRIDS: usize = u8::MAX as usize;

/// What a stash file starts with.
const MAGIC: &[u8; 4] = b"CRST";

/// The magic and the version, which are read before anything else.
const PREAMBLE: usize = MAGIC.len() + 2;

/// The CRC-32 at the end.
const CHECKSUM: usize = 4;

/// One player's fixed fields: their id and their grid count.
const PLAYER_HEAD: usize = PlayerId::BYTES + 1;

/// One placement: key, stack id, count, cell, rotation.
const PLACEMENT_BYTES: usize = 4 + 4 + 2 + 1 + 1 + 1;

/// The mint's largest honest value: every [`StackId`] handed out.
const MINT_SPENT: u64 = u32::MAX as u64 + 1;

/// How each player's stash is laid out when it is first opened: the size of
/// each of its grids, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashLayout {
    grids: Vec<(u8, u8)>,
}

impl StashLayout {
    /// No stash at all: what [`Inventory::new`] lays out, for a game that
    /// keeps nothing between sessions.
    pub const NONE: Self = Self { grids: Vec::new() };

    /// A stash of these grids, each `(width, height)`, unfiltered.
    ///
    /// # Errors
    ///
    /// [`StashError::Layout`] for no grids, more than [`MAX_STASH_GRIDS`], or
    /// a grid with a zero side.
    pub fn new(grids: &[(u8, u8)]) -> Result<Self, StashError> {
        if grids.is_empty() {
            return Err(StashError::Layout("a stash names no grid"));
        }
        if grids.len() > MAX_STASH_GRIDS {
            return Err(StashError::Layout("more grids than a stash counts"));
        }
        if grids.iter().any(|&(w, h)| w == 0 || h == 0) {
            return Err(StashError::Layout("a grid with no cells"));
        }
        Ok(Self {
            grids: grids.to_vec(),
        })
    }

    /// Each grid's `(width, height)`, in order.
    pub fn grids(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.grids.iter().copied()
    }
}

/// Why a stash could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum StashError {
    /// The storage refused. [`StorageError::NotFound`] on a read is a server
    /// that has never saved a stash, which is a fresh one rather than a fault.
    #[error("the stash could not be read or written: {0}")]
    Storage(#[from] StorageError),
    /// The bytes do not start like a stash file.
    #[error("this is not a stash file")]
    Magic,
    /// The checksum does not match the bytes before it.
    #[error("the stash file's checksum does not match its bytes")]
    Checksum,
    /// The file is from a newer build.
    #[error("the stash file is format version {found}, newer than this build's {current}")]
    Newer {
        /// The version the file says.
        found: u16,
        /// [`STASH_FORMAT_VERSION`].
        current: u16,
    },
    /// The file disagrees with itself or with the catalogue; the reason names
    /// how.
    #[error("the stash file is damaged: {0}")]
    Damaged(&'static str),
    /// A stash holds an item the catalogue it was written against does not.
    #[error("item {} is not in this catalogue", .0.0)]
    UnknownItem(ItemId),
    /// A [`StashLayout`] that is not one.
    #[error("not a stash layout: {0}")]
    Layout(&'static str),
}

/// Writes every stash `inventory` holds to `path` in `storage`, in one write.
/// Answers how many bytes it wrote.
///
/// # Errors
///
/// [`encode`]'s, or the storage's.
pub fn save(
    inventory: &Inventory,
    catalog: &Catalog,
    storage: &dyn StorageSource,
    path: &Path,
) -> Result<usize, StashError> {
    let bytes = encode(inventory, catalog)?;
    storage.write(path, &bytes)?;
    Ok(bytes.len())
}

/// Reads the stash file at `path` in `storage` back into an inventory whose
/// new players open stashes laid out as `layout`.
///
/// # Errors
///
/// The storage's — [`StorageError::NotFound`] for a server that never saved —
/// or [`decode`]'s.
pub fn load(
    catalog: &Catalog,
    layout: StashLayout,
    storage: &dyn StorageSource,
    path: &Path,
) -> Result<Inventory, StashError> {
    decode(&storage.read(path)?, catalog, layout)
}

/// The stash file for `inventory`: its stash containers and its mint, and
/// none of its other containers.
///
/// Byte-identical for equal inventories: players in the order their stashes
/// were opened, each player's grids in grid order, placements in slot order.
///
/// # Errors
///
/// [`StashError::UnknownItem`] if a stash holds an item `catalog` does not —
/// the inventory was filled against another catalogue.
pub fn encode(inventory: &Inventory, catalog: &Catalog) -> Result<Vec<u8>, StashError> {
    let mut players: Vec<PlayerId> = Vec::new();
    for container in inventory.table() {
        if let ContainerId::Stash { player, .. } = container.id
            && !players.contains(&player)
        {
            players.push(player);
        }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&STASH_FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&inventory.mint().to_le_bytes());
    let count = u32::try_from(players.len()).expect("fewer players than a u32 counts");
    bytes.extend_from_slice(&count.to_le_bytes());

    for player in players {
        let mut grids: Vec<(u8, &Grid)> = inventory
            .table()
            .iter()
            .filter_map(|container| match container.id {
                ContainerId::Stash {
                    player: owner,
                    grid,
                } if owner == player => Some((grid, &container.grid)),
                _ => None,
            })
            .collect();
        grids.sort_by_key(|&(index, _)| index);
        bytes.extend_from_slice(&player.to_bytes());
        bytes.push(u8::try_from(grids.len()).expect("a stash grid index is a u8"));
        for (_, grid) in grids {
            write_grid(&mut bytes, grid, catalog)?;
        }
    }

    let checksum = crc32(&bytes);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    Ok(bytes)
}

/// One grid's size and placements onto `bytes`.
fn write_grid(bytes: &mut Vec<u8>, grid: &Grid, catalog: &Catalog) -> Result<(), StashError> {
    bytes.push(grid.width());
    bytes.push(grid.height());
    let placements = u16::try_from(grid.len()).expect("a grid holds fewer placements than cells");
    bytes.extend_from_slice(&placements.to_le_bytes());
    for (_, placement) in grid.slots() {
        let stack = placement.stack();
        let key = catalog
            .key(stack.item())
            .ok_or(StashError::UnknownItem(stack.item()))?;
        let turn = Rotation::ALL
            .iter()
            .position(|&rotation| rotation == placement.rotation())
            .expect("every rotation is in Rotation::ALL");
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&stack.id().0.to_le_bytes());
        bytes.extend_from_slice(&stack.count().to_le_bytes());
        bytes.push(placement.at().x);
        bytes.push(placement.at().y);
        bytes.push(u8::try_from(turn).expect("four rotations"));
    }
    Ok(())
}

/// The inventory a stash file describes, holding its stashes and nothing
/// else, with `layout` for the players still to open one.
///
/// # Errors
///
/// [`StashError::Magic`], [`StashError::Checksum`], [`StashError::Newer`], or
/// [`StashError::Damaged`] naming the first thing the reader could not stand
/// behind — see the module docs for the list.
pub fn decode(
    bytes: &[u8],
    catalog: &Catalog,
    layout: StashLayout,
) -> Result<Inventory, StashError> {
    if bytes.len() < PREAMBLE + CHECKSUM || !bytes.starts_with(MAGIC) {
        return Err(StashError::Magic);
    }
    let (body, trailer) = bytes.split_at(bytes.len() - CHECKSUM);
    let written = u32::from_le_bytes(trailer.try_into().expect("CHECKSUM bytes"));
    if crc32(body) != written {
        return Err(StashError::Checksum);
    }
    let mut reader = Reader {
        bytes: body,
        at: MAGIC.len(),
    };
    let version = reader.u16()?;
    if version > STASH_FORMAT_VERSION {
        return Err(StashError::Newer {
            found: version,
            current: STASH_FORMAT_VERSION,
        });
    }
    if version != STASH_FORMAT_VERSION {
        return Err(StashError::Damaged("a format version no build wrote"));
    }

    let mint = reader.u64()?;
    if mint > MINT_SPENT {
        return Err(StashError::Damaged("a mint past every stack id"));
    }
    let players = reader.u32()?;
    if usize::try_from(players).map_or(true, |players| players > reader.left() / PLAYER_HEAD) {
        return Err(StashError::Damaged(
            "more players than the file has bytes for",
        ));
    }

    let mut seen_players: Vec<PlayerId> = Vec::new();
    let mut seen_stacks: BTreeSet<StackId> = BTreeSet::new();
    let mut containers = Vec::new();
    for _ in 0..players {
        let player = PlayerId::from_bytes(
            reader
                .take(PlayerId::BYTES)?
                .try_into()
                .expect("PlayerId::BYTES bytes"),
        );
        if seen_players.contains(&player) {
            return Err(StashError::Damaged("one player twice"));
        }
        seen_players.push(player);
        let grids = reader.u8()?;
        if grids == 0 {
            return Err(StashError::Damaged("a player with no stash grid"));
        }
        for grid in 0..grids {
            let read = read_grid(&mut reader, catalog, mint, &mut seen_stacks)?;
            containers.push(Container {
                id: ContainerId::Stash { player, grid },
                access: Access::Player(player),
                grid: read,
            });
        }
    }
    if reader.left() != 0 {
        return Err(StashError::Damaged("bytes after the last player"));
    }
    Ok(Inventory::restored(layout, containers, mint))
}

/// One grid off `reader`, every placement held to the rules the module docs
/// list. `seen` is every stack id read so far, across every player.
fn read_grid(
    reader: &mut Reader<'_>,
    catalog: &Catalog,
    mint: u64,
    seen: &mut BTreeSet<StackId>,
) -> Result<Grid, StashError> {
    let w = reader.u8()?;
    let h = reader.u8()?;
    let mut grid =
        Grid::new(w, h, None).map_err(|_| StashError::Damaged("a grid with no cells"))?;
    let placements = reader.u16()?;
    if usize::from(placements) > usize::from(w) * usize::from(h) {
        return Err(StashError::Damaged(
            "more placements than the grid has cells",
        ));
    }
    if usize::from(placements) > reader.left() / PLACEMENT_BYTES {
        return Err(StashError::Damaged(
            "more placements than the file has bytes for",
        ));
    }
    for _ in 0..placements {
        let key = reader.u32()?;
        let id = StackId(reader.u32()?);
        let count = reader.u16()?;
        let at = Cell::new(reader.u8()?, reader.u8()?);
        let rotation = *Rotation::ALL
            .get(usize::from(reader.u8()?))
            .ok_or(StashError::Damaged(
                "a rotation that is not one of the four",
            ))?;
        let item = catalog
            .by_key(key)
            .ok_or(StashError::Damaged("an item key no catalogue entry has"))?;
        let max = catalog.get(item).map_or(0, crate::ItemDef::stack_max);
        if count == 0 || count > max {
            return Err(StashError::Damaged("a count its item does not stack to"));
        }
        if u64::from(id.0) >= mint {
            return Err(StashError::Damaged("a stack id the mint never handed out"));
        }
        if !seen.insert(id) {
            return Err(StashError::Damaged("one stack twice"));
        }
        grid.place(catalog, Stack::new(item, id, count), at, rotation)
            .map_err(|_| StashError::Damaged("a placement its grid refuses"))?;
    }
    Ok(grid)
}

/// A cursor over a stash file's body that refuses to read past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// The next `len` bytes.
    fn take(&mut self, len: usize) -> Result<&'a [u8], StashError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(StashError::Damaged(
                "the file ends part-way through a field",
            ))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// How many bytes are left.
    const fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn u8(&mut self) -> Result<u8, StashError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, StashError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, StashError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, StashError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
}

#[cfg(test)]
mod tests;
