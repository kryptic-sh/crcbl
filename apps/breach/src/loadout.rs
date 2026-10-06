//! What the player carries, as the grid-inventory kit's one container.
//!
//! ```text
//!   data/items.ron ──▶ Catalog ──▶ KIT ──▶ Stack ──▶ Grid ──▶ crate::panel
//!                          │                          │
//!                          └──▶ is_armed(grid) ───────┘──▶ crate::game's trigger
//! ```
//!
//! # The kit is consumed, not extended
//!
//! Every rule about where an item fits, what a footprint is once it is turned
//! and what a stack holds is [`crcbl::inventory`]'s —
//! `docs/plan/34-inventory.md`'s part 2, and `apps/breach` is its **second**
//! consumer. That is what this module is for: `apps/shard` forced the kit, and
//! a kit with one consumer is that consumer's shape wearing a kit's name. What
//! is breach's is the content — the item table, how big the rig is, and what a
//! player spawns holding — and nothing here reaches into the crate's internals.
//! Not one line of the engine changed on this sample's behalf; what breach
//! wanted from the kit and did not get is in `docs/backlog.md` as topic-34
//! findings.
//!
//! # The grid is the source of truth for the one thing it can decide
//!
//! [`is_armed`] is the whole of it: a trigger pulled with no item carrying the
//! [`WEAPON`] tag in the grid is not a shot, and `crate::game`'s tick asks the
//! grid rather than a field beside it. The starting kit holds the sidearm, so
//! nothing a player can do in milestone 0 disarms them — there is no drop verb
//! — and the range, the bots and the wire behave exactly as they did before the
//! grid existed. That is the point: this slice changed the container, not the
//! game.
//!
//! **The tag is read, not assumed.** `apps/shard` sets no filter and reads no
//! tag, so before this sample the vocabulary in a catalogue was a field nothing
//! asked about.
//!
//! # A drag is the kit's `Move` command
//!
//! The rig and the [`PACK`] are the two containers of an [`Inventory`], and a
//! drag inside either or from one to the other is [`drag`]: a
//! [`Command::Move`] applied as the kit's single transaction, the same call a
//! server makes when a client's move arrives — rather than this sample reaching
//! into a grid with a move of its own. The pad's quick action is [`send`], the
//! same command with the cell first-fit chooses in the other container. Neither
//! crosses the wire yet (`crate::game`'s `Game::drag` says why), so the command
//! is applied on behalf of [`owner`], the one player a milestone-0 run has, and
//! no reach check refuses it: both are on the player's back.
//!
//! **The pack is why a drag has somewhere else to go.** It holds nothing at
//! spawn and the trigger does not read it: a sidearm stowed in the pack is not
//! a sidearm in hand, so [`is_armed`] still asks the rig alone.
//!
//! # The table is a data file
//!
//! `data/items.ron` is `include_str!`-ed rather than read through an
//! [`crcbl::store::StorageSource`], for the reason `crate::web`'s docs give:
//! breach has no asset source at all, and every byte this sample draws with is
//! compiled into the module. A file is still the right shape — it is the format
//! [`crcbl::inventory::Catalog`] reads — and routing it through an asset source
//! is a change for the milestone that has assets, not for this one.
//!
//! # Every instance is one the spawn handed out
//!
//! A [`StackId`] here is an index into [`KIT`] plus one, so the starting kit
//! bounds how many instances can exist: one stack per entry of that table, with
//! the same ids however the session went. `StackId(0)` is never one this sample
//! mints, so a zeroed byte range cannot read as a stack.

use crcbl::core::PlayerId;
use crcbl::inventory::{
    Access, Applied, Catalog, Cell, Command, ContainerId, Grid, Held, Inventory, InventoryError,
    Placement, Refusal, SlotId, Stack, StackId,
};

/// The item table, compiled in. See the module docs for why it is not an asset.
const ITEMS_RON: &str = include_str!("../data/items.ron");

/// The tag an item has to carry for the trigger to do anything. See
/// [`is_armed`].
pub const WEAPON: &str = "weapon";

/// How many cells across the rig is.
pub const GRID_W: u8 = 4;

/// How many cells down. More cells than [`KIT`] fills, so there is somewhere to
/// drag things to — which is the whole point of a grid rather than a list, and
/// `the_starting_kit_is_one_the_table_and_the_rig_can_hold` is what says the
/// room is still there.
pub const GRID_H: u8 = 3;

/// What a player spawns holding: an item name and how many of it.
///
/// The order is the order it is stowed in, which — through
/// [`Grid::insert`]'s first-fit — is what decides where each stack starts. Both
/// halves are checked against the shipped table by
/// `the_starting_kit_is_one_the_table_and_the_rig_can_hold`.
pub const KIT: [(&str, u16); 3] = [("sidearm", 1), ("magazine", 2), ("frag", 2)];

/// The item table every session reads, parsed once.
///
/// # Panics
///
/// If `data/items.ron` is not a catalogue. That is this crate's own file being
/// wrong rather than a state a run can be in — the same call
/// `apps/shard/src/loot.rs` makes of its own table — and
/// `the_shipped_table_is_one_the_kit_accepts` is what catches it before a page
/// does.
#[must_use]
pub fn catalog() -> &'static Catalog {
    static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        Catalog::from_ron(ITEMS_RON).expect("data/items.ron is this crate's own catalogue")
    })
}

/// The empty rig: [`GRID_W`] by [`GRID_H`], accepting anything.
///
/// Unfiltered, like shard's. A filter is what the plan makes an equipment slot
/// out of, and milestone 0 equips nothing: a rig that refused an item would be
/// refusing it for a rule this sample does not have. `docs/backlog.md` carries
/// that the filter is still a feature with no consumer.
///
/// # Panics
///
/// Never: the dimensions are constants and neither is zero, which is the only
/// thing [`Grid::new`] refuses.
#[must_use]
pub fn empty() -> Grid {
    Grid::new(GRID_W, GRID_H, None).expect("neither side of the rig is zero")
}

/// The rig a run starts with: [`empty`] with [`KIT`] stowed in it.
///
/// # Panics
///
/// If the table does not name an entry of [`KIT`], or if the kit does not fit
/// the rig. Both are this crate's own two files disagreeing rather than a state
/// a run can reach, and `the_starting_kit_is_one_the_table_and_the_rig_can_hold`
/// is what catches either before a page does.
#[must_use]
pub fn packed() -> Grid {
    let catalog = catalog();
    let mut grid = empty();
    for (index, (name, count)) in KIT.iter().enumerate() {
        let item = catalog
            .id_of(name)
            .expect("every kit entry is an item of this catalogue");
        grid.insert(catalog, Stack::new(item, stack_id(index), *count))
            .expect("the starting kit fits the rig it is carried in");
    }
    grid
}

/// The rig's local id in the [`Inventory`] [`carried`] builds.
const RIG_ID: u32 = 0;

/// The rig's place in the [`Inventory`] [`carried`] builds.
pub const RIG: ContainerId = ContainerId::Local(RIG_ID);

/// The pack's local id in the [`Inventory`] [`carried`] builds.
const PACK_ID: u32 = 1;

/// The pack's place in the [`Inventory`] [`carried`] builds: the second
/// container, empty at spawn.
pub const PACK: ContainerId = ContainerId::Local(PACK_ID);

/// How many cells across the pack is: the rig's width, so the panel draws the
/// two as one column.
pub const PACK_W: u8 = GRID_W;

/// How many cells down.
pub const PACK_H: u8 = 2;

/// The seed of [`owner`]'s id.
const OWNER_SEED: u64 = 0;

/// Who the rig belongs to: the one player a milestone-0 run has, under the
/// fixed id [`PlayerId::from_seed`] gives in-process sessions — the same every
/// run, because nothing this sample keeps is keyed by it.
#[must_use]
pub fn owner() -> PlayerId {
    PlayerId::from_seed(OWNER_SEED)
}

/// The inventory a run starts with: [`packed`] as the [`RIG`] and an empty
/// [`PACK`], both [`owner`]'s alone.
///
/// # Panics
///
/// Never: the two ids are distinct, the pack holds no stack to clash with
/// the rig's, and [`KIT`]'s ids are distinct by [`stack_id`]'s construction.
#[must_use]
pub fn carried() -> Inventory {
    let mut inventory = Inventory::new();
    inventory
        .add(RIG_ID, Access::Player(owner()), packed())
        .expect("an empty inventory takes the rig");
    let pack = Grid::new(PACK_W, PACK_H, None).expect("neither side of the pack is zero");
    inventory
        .add(PACK_ID, Access::Player(owner()), pack)
        .expect("the pack is a second id holding nothing");
    inventory
}

/// The rig in `inventory`, which [`carried`] put there.
///
/// # Panics
///
/// If `inventory` is not one [`carried`] built: nothing removes a container
/// from an [`Inventory`], so one that ever held the rig holds it still.
#[must_use]
pub fn rig(inventory: &Inventory) -> &Grid {
    inventory
        .grid(RIG)
        .expect("the inventory carried() built holds the rig")
}

/// The pack in `inventory`, which [`carried`] put there.
///
/// # Panics
///
/// As [`rig`]: if `inventory` is not one [`carried`] built.
#[must_use]
pub fn pack(inventory: &Inventory) -> &Grid {
    inventory
        .grid(PACK)
        .expect("the inventory carried() built holds the pack")
}

/// The container a quick action sends a stack in `from` to: the pack from the
/// rig, and the rig from anything else.
#[must_use]
pub fn other(from: ContainerId) -> ContainerId {
    if from == RIG { PACK } else { RIG }
}

/// Moves the stack at `slot` of `from` so its origin is `at` in `to` — the
/// same container or the other — as the kit's [`Command::Move`], keeping its
/// rotation.
///
/// # Errors
///
/// [`Refusal::NoSuchContainer`] for a container `inventory` does not hold,
/// [`Refusal::Grid`] with [`NoSuchSlot`](InventoryError::NoSuchSlot) for an
/// empty slot, or whatever the move is refused for — in which case both
/// containers are exactly as they were, down to the slot ids the panel is
/// holding.
pub fn drag(
    inventory: &mut Inventory,
    from: ContainerId,
    slot: SlotId,
    to: ContainerId,
    at: Cell,
) -> Result<Applied, Refusal> {
    let placement = placement(inventory, from, slot)?;
    let command = Command::Move {
        stack: Held {
            container: from,
            stack: placement.stack().id(),
        },
        to,
        at,
        rotation: placement.rotation(),
    };
    inventory.apply(catalog(), owner(), command, |_, _| true)
}

/// The pad's quick action: the stack at `slot` of `from` sent to the [`other`]
/// container, wherever [`Grid::find_slot`]'s first-fit puts it — the kit's
/// [`Command::Move`], so it is refused or applied whole as a drag is.
///
/// # Errors
///
/// As [`drag`], and [`Refusal::Grid`] with [`NoRoom`](InventoryError::NoRoom)
/// when nothing in the other container fits it.
pub fn send(
    inventory: &mut Inventory,
    from: ContainerId,
    slot: SlotId,
) -> Result<Applied, Refusal> {
    let to = other(from);
    let placement = placement(inventory, from, slot)?;
    let (at, rotation) = inventory
        .grid(to)
        .ok_or(Refusal::NoSuchContainer(to))?
        .find_slot(catalog(), placement.stack().item())
        .ok_or(InventoryError::NoRoom)?;
    let command = Command::Move {
        stack: Held {
            container: from,
            stack: placement.stack().id(),
        },
        to,
        at,
        rotation,
    };
    inventory.apply(catalog(), owner(), command, |_, _| true)
}

/// The placement at `slot` of `from`.
fn placement(inventory: &Inventory, from: ContainerId, slot: SlotId) -> Result<Placement, Refusal> {
    Ok(inventory
        .grid(from)
        .ok_or(Refusal::NoSuchContainer(from))?
        .slot(slot)
        .ok_or(InventoryError::NoSuchSlot(slot))?)
}

/// The identity of [`KIT`]'s `index`th entry: **one-based**, so `StackId(0)` is
/// never one this sample minted.
#[must_use]
pub fn stack_id(index: usize) -> StackId {
    StackId(index as u32 + 1)
}

/// Whether `grid` holds anything carrying the [`WEAPON`] tag.
///
/// **The one question the simulation asks the container**, and the reason the
/// grid is the loadout rather than a picture of one: `crate::game`'s trigger is
/// gated on this, so a rig with no weapon in it fires nothing.
///
/// `false` for a catalogue with no such tag at all, which is the shipped table
/// having lost its weapon rather than a run's state —
/// `the_shipped_table_is_one_the_kit_accepts` is what says the tag is there.
#[must_use]
pub fn is_armed(grid: &Grid) -> bool {
    let catalog = catalog();
    let Some(weapon) = catalog.tag(WEAPON) else {
        return false;
    };
    grid.slots().any(|(_, placement)| {
        catalog
            .get(placement.stack().item())
            .is_some_and(|def| def.has_tag(weapon))
    })
}

/// What the rig weighs, in grams — the kit's flat sum over one grid.
#[must_use]
pub fn weight_g(grid: &Grid) -> u64 {
    grid.weight_g(catalog())
}

/// How a rig is written out: how many stacks it holds and what they weigh.
///
/// Here rather than in either of its two readouts — `crate::panel`'s summary
/// row and the `carried` row of `crate::game`'s debug section — because two
/// spellings of one line are two spellings that can drift apart.
#[must_use]
pub fn summary(items: usize, grams: u64) -> String {
    format!("{items} items {grams} g")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The shipped table is one the kit accepts, and it names the tag the
    /// trigger reads.** [`catalog`] panics on a file that is not a catalogue,
    /// so without this the first failure would be a page that does not open —
    /// and without the tag half, a table whose `weapon` had been renamed would
    /// leave [`is_armed`] answering `false` for every rig a player could carry.
    #[test]
    fn the_shipped_table_is_one_the_kit_accepts() {
        let catalog = catalog();
        assert!(!catalog.is_empty(), "an empty table arms nobody");
        assert!(
            catalog.tag(WEAPON).is_some(),
            "the table names no {WEAPON} tag, so nothing in it can be fired",
        );
        for (id, item) in catalog.items() {
            assert!(!item.name().is_empty(), "item {} has no name", id.0);
            assert!(
                item.stack_max() > 0,
                "{} cannot be held at all",
                item.name()
            );
            assert!(
                item.shape().width() <= GRID_W && item.shape().height() <= GRID_H,
                "{} is {}x{}, which no rotation fits in a {GRID_W}x{GRID_H} rig",
                item.name(),
                item.shape().width(),
                item.shape().height(),
            );
        }
    }

    /// **The starting kit is items this table has, in counts they stack to, and
    /// the rig holds all of it at once.** [`packed`] panics on any of the
    /// three, and it is called from `Stage::new` — so without this the first
    /// failure would be a run that never reached its first tick.
    #[test]
    fn the_starting_kit_is_one_the_table_and_the_rig_can_hold() {
        let catalog = catalog();
        for (name, count) in KIT {
            let item = catalog
                .id_of(name)
                .unwrap_or_else(|| panic!("the table has no {name}"));
            let def = catalog.get(item).expect("id_of named an item");
            assert!(
                count > 0 && count <= def.stack_max(),
                "{name} spawns as {count}, against a stack maximum of {}",
                def.stack_max(),
            );
        }
        let grid = packed();
        assert_eq!(
            grid.len(),
            KIT.len(),
            "a kit entry went missing on the way in"
        );
        // …and there is a cell left over to drag something into, which is what
        // makes the rig a container rather than a shelf that is exactly full.
        let free = (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| Cell::new(x, y)))
            .filter(|cell| grid.at(*cell).is_none())
            .count();
        assert!(free > 0, "the rig starts with every cell covered");
    }

    /// **A rig with the sidearm in it is armed and one without it is not.**
    ///
    /// The second half is the control and it is the whole check: a build whose
    /// [`is_armed`] answered `true` for anything at all would pass the first
    /// half with the grid ignored entirely, which is the state this slice
    /// replaced.
    #[test]
    fn a_rig_is_armed_by_what_is_in_it_and_not_by_anything_else() {
        let mut grid = packed();
        assert!(is_armed(&grid), "the starting kit carries no weapon");

        let weapon = catalog().tag(WEAPON).expect("the table names the tag");
        let armed: Vec<_> = grid
            .slots()
            .filter(|(_, placement)| {
                catalog()
                    .get(placement.stack().item())
                    .is_some_and(|def| def.has_tag(weapon))
            })
            .map(|(slot, _)| slot)
            .collect();
        assert_eq!(armed.len(), 1, "milestone 0 carries one weapon");
        for slot in armed {
            grid.remove(slot).expect("a slot the grid just yielded");
        }
        assert!(!grid.is_empty(), "the rest of the kit went with it");
        assert!(
            !is_armed(&grid),
            "a rig holding a magazine and a grenade reports a weapon",
        );

        // …and an empty rig is not armed either, which is what the trigger
        // check in `crate::game` is written against.
        assert!(!is_armed(&empty()));
    }

    /// **What the rig weighs is what is in it, and taking something out makes
    /// it lighter.** The control for a readout that summed a constant.
    #[test]
    fn the_rig_weighs_what_it_holds() {
        let mut grid = packed();
        let full = weight_g(&grid);
        assert!(full > 0, "a rig holding three stacks weighs nothing");
        assert_eq!(weight_g(&empty()), 0, "an empty rig weighs something");

        let (slot, placement) = grid.slots().next().expect("the kit is in there");
        let def = catalog()
            .get(placement.stack().item())
            .expect("a stack of this catalogue");
        let each = u64::from(def.weight_g()) * u64::from(placement.stack().count());
        grid.remove(slot).expect("a slot the grid just yielded");
        assert_eq!(
            weight_g(&grid),
            full - each,
            "taking the {} out did not take its weight with it",
            def.name(),
        );
    }

    /// **Send moves a stack whole into the other container and back**, as one
    /// [`Command::Move`]: the sidearm leaves the rig — which then fires
    /// nothing — lands where first-fit puts it in the pack keeping its stack
    /// id, and comes back the same way.
    #[test]
    fn send_moves_a_stack_into_the_other_container_and_back() {
        let mut inventory = carried();
        let slot = rig(&inventory).at(Cell::new(0, 0)).expect("the sidearm");
        let stack = rig(&inventory).slot(slot).expect("held").stack();

        assert_eq!(send(&mut inventory, RIG, slot), Ok(Applied::Moved));
        assert_eq!(rig(&inventory).find(stack.id()), None, "still in the rig");
        let stowed = pack(&inventory)
            .find(stack.id())
            .expect("the sidearm is not in the pack");
        assert_eq!(
            pack(&inventory).slot(stowed).expect("held").at(),
            Cell::new(0, 0),
            "not where first-fit puts it",
        );
        assert_eq!(pack(&inventory).slot(stowed).expect("held").stack(), stack);
        assert!(!is_armed(rig(&inventory)), "a stowed sidearm still fires");

        assert_eq!(send(&mut inventory, PACK, stowed), Ok(Applied::Moved));
        assert!(pack(&inventory).is_empty(), "the pack kept a copy");
        assert!(is_armed(rig(&inventory)), "the sidearm did not come back");
    }

    /// **A send with nowhere to go, or of nothing, changes nothing.** A pack
    /// one cell big has no room for the `2×1` sidearm in either rotation, and
    /// a slot nothing holds is refused by name; both containers are as they
    /// were after each.
    #[test]
    fn a_refused_send_leaves_both_containers_as_they_were() {
        let mut inventory = Inventory::new();
        inventory
            .add(RIG_ID, Access::Player(owner()), packed())
            .expect("the rig");
        let tiny = Grid::new(1, 1, None).expect("1x1 is a grid");
        inventory
            .add(PACK_ID, Access::Player(owner()), tiny)
            .expect("the pack");
        let before = (rig(&inventory).clone(), pack(&inventory).clone());
        let sidearm = rig(&inventory).at(Cell::new(0, 0)).expect("the sidearm");

        assert_eq!(
            send(&mut inventory, RIG, sidearm),
            Err(Refusal::Grid(InventoryError::NoRoom)),
        );
        assert_eq!((rig(&inventory).clone(), pack(&inventory).clone()), before);

        let nothing = SlotId(u16::MAX);
        assert_eq!(
            send(&mut inventory, RIG, nothing),
            Err(Refusal::Grid(InventoryError::NoSuchSlot(nothing))),
        );
        assert_eq!((rig(&inventory).clone(), pack(&inventory).clone()), before);
    }
}
