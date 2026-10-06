use super::*;
use crate::InventoryError;
use crate::shape::{Cell, Rotation};
use crate::stash;
use crate::test_items::{id, items};

/// Player A: owns [`RIG_A`] and [`HEAD_A`].
fn a() -> PlayerId {
    PlayerId::from_seed(1)
}

/// Player B: owns [`RIG_B`].
fn b() -> PlayerId {
    PlayerId::from_seed(2)
}

/// A's carried rig, `4×4`.
const RIG_A: ContainerId = ContainerId::Local(1);
/// B's carried rig, `4×4`.
const RIG_B: ContainerId = ContainerId::Local(2);
/// A's helmet slot: a `2×2` grid filtered by `helmet`, the test helmet being
/// `2×2`.
const HEAD_A: ContainerId = ContainerId::Local(3);
/// A crate anyone may open, `3×3`.
const CRATE: ContainerId = ContainerId::Local(4);
/// The ground, a world container, `4×4`.
const GROUND: ContainerId = ContainerId::Local(5);

/// The world every test starts from, and the ids of what is in it.
struct World {
    catalog: Catalog,
    inventory: Inventory,
    /// Twelve bandages at `(0, 0)` of [`RIG_A`].
    bandages: StackId,
    /// A mag at `(1, 0)` of [`RIG_A`].
    mag: StackId,
    /// A helmet at `(2, 0)` of [`RIG_A`].
    helmet: StackId,
    /// Fifteen bandages at `(1, 0)` of [`CRATE`].
    loose: StackId,
    /// The L-shaped brace at `(0, 0)` of [`CRATE`], upright: column 0 and
    /// `(1, 2)`.
    brace: StackId,
    /// Five bandages at `(0, 0)` of [`RIG_B`].
    theirs: StackId,
}

fn world() -> World {
    let catalog = items();
    let helmet_tag = catalog.tag("helmet").expect("the helmet carries it");
    let mut inventory = Inventory::with_stash(StashLayout::new(&[(3, 3)]).expect("a stash layout"));
    for (local, access, grid) in [
        (1, Access::Player(a()), Grid::new(4, 4, None)),
        (2, Access::Player(b()), Grid::new(4, 4, None)),
        (3, Access::Player(a()), Grid::new(2, 2, Some(helmet_tag))),
        (4, Access::Open, Grid::new(3, 3, None)),
        (5, Access::Open, Grid::new(4, 4, None)),
    ] {
        inventory
            .add(local, access, grid.expect("no side is zero"))
            .expect("every id is new");
    }
    let mut spawn = |into, name, count| {
        inventory
            .spawn(&catalog, into, id(&catalog, name), count)
            .expect("the world has room for what it starts with")
    };
    let bandages = spawn(RIG_A, "bandage", 12);
    let mag = spawn(RIG_A, "mag", 1);
    let helmet = spawn(RIG_A, "helmet", 1);
    let brace = spawn(CRATE, "brace", 1);
    let loose = spawn(CRATE, "bandage", 15);
    let theirs = spawn(RIG_B, "bandage", 5);
    World {
        catalog,
        inventory,
        bandages,
        mag,
        helmet,
        loose,
        brace,
        theirs,
    }
}

/// Every container in reach of everyone.
fn anywhere(_: PlayerId, _: ContainerId) -> bool {
    true
}

fn held(container: ContainerId, stack: StackId) -> Held {
    Held { container, stack }
}

/// Where the stack `stack` sits in `container`, if it is there.
fn where_is(inventory: &Inventory, container: ContainerId, stack: StackId) -> Option<Cell> {
    let grid = inventory.grid(container)?;
    grid.slot(grid.find(stack)?).map(Placement::at)
}

/// The count of the stack `stack` in `container`, if it is there.
fn count_of(inventory: &Inventory, container: ContainerId, stack: StackId) -> Option<u16> {
    let grid = inventory.grid(container)?;
    grid.slot(grid.find(stack)?)
        .map(|placement| placement.stack().count())
}

use crate::grid::Placement;

/// **The world the tests start from is the one their comments describe.**
/// Every refusal below names a cell or a count, and a world that first-fit
/// had laid out differently would make those refusals true for the wrong
/// reason.
#[test]
fn the_starting_world_is_laid_out_as_the_tests_assume() {
    let w = world();
    let at = |container, stack| where_is(&w.inventory, container, stack);
    assert_eq!(at(RIG_A, w.bandages), Some(Cell::new(0, 0)));
    assert_eq!(at(RIG_A, w.mag), Some(Cell::new(1, 0)));
    assert_eq!(at(RIG_A, w.helmet), Some(Cell::new(2, 0)));
    assert_eq!(at(CRATE, w.brace), Some(Cell::new(0, 0)));
    assert_eq!(at(CRATE, w.loose), Some(Cell::new(1, 0)));
    assert_eq!(at(RIG_B, w.theirs), Some(Cell::new(0, 0)));
    assert_eq!(w.inventory.next_stack(), Some(StackId(6)), "six spawns");
}

/// **A move lands exactly where it was sent and keeps the stack's identity**,
/// within one container — where it keeps its slot id too — and across two.
#[test]
fn a_move_lands_where_it_was_sent_and_keeps_its_identity() {
    let mut w = world();
    let slot = w
        .inventory
        .grid(RIG_A)
        .and_then(|grid| grid.find(w.mag))
        .expect("the mag is in the rig");
    let moved = w.inventory.apply(
        &w.catalog,
        a(),
        Command::Move {
            stack: held(RIG_A, w.mag),
            to: RIG_A,
            at: Cell::new(0, 2),
            rotation: Rotation::Deg90,
        },
        anywhere,
    );
    assert_eq!(moved, Ok(Applied::Moved));
    let grid = w.inventory.grid(RIG_A).expect("the rig");
    assert_eq!(
        grid.find(w.mag),
        Some(slot),
        "the same slot within one grid"
    );
    assert_eq!(grid.at(Cell::new(1, 2)), Some(slot), "turned, it is 2x1");

    let crossed = w.inventory.apply(
        &w.catalog,
        a(),
        Command::Move {
            stack: held(RIG_A, w.mag),
            to: GROUND,
            at: Cell::new(3, 1),
            rotation: Rotation::Deg0,
        },
        anywhere,
    );
    assert_eq!(crossed, Ok(Applied::Moved));
    assert_eq!(
        where_is(&w.inventory, RIG_A, w.mag),
        None,
        "it left the rig"
    );
    assert_eq!(
        where_is(&w.inventory, GROUND, w.mag),
        Some(Cell::new(3, 1)),
        "and it is on the ground under the same id"
    );
}

/// **Every command refuses with its named reason, and a refusal changes
/// nothing at all.** Each case is applied to the same world and the whole
/// inventory compared afterwards — every grid's occupancy, slot table and
/// counts, and the id mint — and so are the stash file's bytes, which is the
/// inventory as a server would write it.
#[test]
fn every_command_refuses_with_its_named_reason_and_changes_nothing() {
    let mut w = world();
    w.inventory.open_stash(a());
    let before = w.inventory.clone();
    let bytes = stash::encode(&w.inventory, &w.catalog).expect("it encodes");
    let tag = w.catalog.tag("helmet").expect("the helmet carries it");
    let ghost = StackId(99);
    let a_stash = ContainerId::Stash {
        player: a(),
        grid: 0,
    };
    let crate_out_of_reach = |_: PlayerId, container: ContainerId| container != CRATE;

    let cases: Vec<(&str, PlayerId, Command, Refusal)> = vec![
        (
            "a container that is not there",
            a(),
            Command::Move {
                stack: held(ContainerId::Local(77), w.mag),
                to: RIG_A,
                at: Cell::new(0, 3),
                rotation: Rotation::Deg0,
            },
            Refusal::NoSuchContainer(ContainerId::Local(77)),
        ),
        (
            "a stack that is not there",
            a(),
            Command::Move {
                stack: held(RIG_A, ghost),
                to: RIG_A,
                at: Cell::new(0, 3),
                rotation: Rotation::Deg0,
            },
            Refusal::NoSuchStack(held(RIG_A, ghost)),
        ),
        (
            "another player's rig",
            b(),
            Command::Move {
                stack: held(RIG_A, w.mag),
                to: RIG_B,
                at: Cell::new(3, 0),
                rotation: Rotation::Deg0,
            },
            Refusal::NotYours(RIG_A),
        ),
        (
            "into another player's stash",
            b(),
            Command::Move {
                stack: held(RIG_B, w.theirs),
                to: a_stash,
                at: Cell::new(0, 0),
                rotation: Rotation::Deg0,
            },
            Refusal::NotYours(a_stash),
        ),
        (
            "onto a cell another stack holds",
            a(),
            Command::Move {
                stack: held(RIG_A, w.mag),
                to: CRATE,
                at: Cell::new(1, 1),
                rotation: Rotation::Deg0,
            },
            Refusal::Grid(InventoryError::Occupied { x: 1, y: 2 }),
        ),
        (
            "off the edge of another container",
            a(),
            Command::Move {
                stack: held(RIG_A, w.mag),
                to: CRATE,
                at: Cell::new(2, 2),
                rotation: Rotation::Deg0,
            },
            Refusal::Grid(InventoryError::OutOfBounds {
                x: 2,
                y: 2,
                w: 1,
                h: 2,
            }),
        ),
        (
            "a split of the whole stack",
            a(),
            Command::Split {
                stack: held(RIG_A, w.bandages),
                count: 12,
                to: RIG_A,
                at: Cell::new(0, 3),
                rotation: Rotation::Deg0,
            },
            Refusal::Grid(InventoryError::BadSplit {
                count: 12,
                held: 12,
            }),
        ),
        (
            "a split onto a taken cell",
            a(),
            Command::Split {
                stack: held(RIG_A, w.bandages),
                count: 2,
                to: RIG_A,
                at: Cell::new(1, 1),
                rotation: Rotation::Deg0,
            },
            Refusal::Grid(InventoryError::Occupied { x: 1, y: 1 }),
        ),
        (
            "a merge of two different items",
            a(),
            Command::Merge {
                from: held(RIG_A, w.mag),
                into: held(CRATE, w.loose),
            },
            Refusal::Grid(InventoryError::NotMergeable {
                a: id(&w.catalog, "mag"),
                b: id(&w.catalog, "bandage"),
            }),
        ),
        (
            "an equip into a grid with no filter",
            a(),
            Command::Equip {
                stack: held(RIG_A, w.helmet),
                slot: GROUND,
            },
            Refusal::NotASlot(GROUND),
        ),
        (
            "an equip of what the slot is not for",
            a(),
            Command::Equip {
                stack: held(RIG_A, w.bandages),
                slot: HEAD_A,
            },
            Refusal::Grid(InventoryError::Filtered { filter: tag }),
        ),
        (
            "a drop into a player's own container",
            a(),
            Command::Drop {
                stack: held(RIG_A, w.mag),
                onto: HEAD_A,
            },
            Refusal::NotOpen(HEAD_A),
        ),
        (
            "a drop onto the container it is in",
            a(),
            Command::Drop {
                stack: held(CRATE, w.loose),
                onto: CRATE,
            },
            Refusal::SameContainer(CRATE),
        ),
        (
            "a take-all from an empty container",
            a(),
            Command::TakeAll {
                from: GROUND,
                to: RIG_A,
            },
            Refusal::Empty(GROUND),
        ),
        (
            "a take-all into a slot that takes neither",
            a(),
            Command::TakeAll {
                from: CRATE,
                to: HEAD_A,
            },
            Refusal::Grid(InventoryError::Filtered { filter: tag }),
        ),
    ];
    for (name, player, command, refusal) in cases {
        assert_eq!(
            w.inventory.apply(&w.catalog, player, command, anywhere),
            Err(refusal),
            "{name}"
        );
        assert_eq!(w.inventory, before, "{name} changed the inventory");
        assert_eq!(
            stash::encode(&w.inventory, &w.catalog).expect("it encodes"),
            bytes,
            "{name} changed a byte"
        );
    }

    // Reach is the caller's, asked for every container a command names.
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::TakeAll {
                from: CRATE,
                to: RIG_A,
            },
            crate_out_of_reach,
        ),
        Err(Refusal::OutOfReach(CRATE))
    );
    assert_eq!(
        w.inventory, before,
        "an unreachable crate changed something"
    );
}

/// **A cross-container move refused at its destination leaves its source
/// whole.** The test the plan's anti-dupe rule is about: a move written as a
/// removal followed by an insertion has already taken the stack out of the rig
/// by the time the crate refuses it, and the item is gone. The same for a
/// take-all whose first stack fits and whose second does not.
#[test]
fn a_cross_container_move_refused_at_its_destination_leaves_the_source_whole() {
    let mut w = world();
    let before = w.inventory.clone();
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::Move {
                stack: held(RIG_A, w.helmet),
                to: CRATE,
                at: Cell::new(1, 1),
                rotation: Rotation::Deg0,
            },
            anywhere,
        ),
        Err(Refusal::Grid(InventoryError::Occupied { x: 1, y: 2 }))
    );
    assert_eq!(
        where_is(&w.inventory, RIG_A, w.helmet),
        Some(Cell::new(2, 0)),
        "the helmet is still in the rig"
    );
    assert_eq!(w.inventory, before);

    // A take-all of the crate into a 2x3 pouch whose right column a mag
    // already fills down to the last row: the brace, first in slot order, fits
    // the L of cells left, and then the bandages have nowhere to go.
    let mut pouch = Grid::new(2, 3, None).expect("2x3");
    pouch
        .place(
            &w.catalog,
            Stack::new(id(&w.catalog, "mag"), StackId(60), 1),
            Cell::new(1, 0),
            Rotation::Deg0,
        )
        .expect("an empty pouch takes it");
    w.inventory
        .add(6, Access::Player(a()), pouch)
        .expect("a new id");
    let before = w.inventory.clone();
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::TakeAll {
                from: CRATE,
                to: ContainerId::Local(6),
            },
            anywhere,
        ),
        Err(Refusal::Grid(InventoryError::NoRoom))
    );
    assert_eq!(count_of(&w.inventory, CRATE, w.loose), Some(15));
    assert_eq!(w.inventory, before, "half a take-all happened");

    // The control: room for both, and both go.
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::TakeAll {
                from: CRATE,
                to: GROUND,
            },
            anywhere,
        ),
        Ok(Applied::TookAll { stacks: 2 })
    );
    assert!(w.inventory.grid(CRATE).expect("the crate").is_empty());
    assert_eq!(count_of(&w.inventory, GROUND, w.loose), Some(15));
    assert!(where_is(&w.inventory, GROUND, w.brace).is_some());
}

/// **A split mints an id no stack has had, and a refused split mints none.**
/// The new stack's id is the mint's next, it differs from every id in the
/// inventory, and the mint moves past it — so the next split mints another.
#[test]
fn a_split_mints_an_id_no_stack_has_had() {
    let mut w = world();
    let next = w.inventory.next_stack().expect("ids are left");
    let refused = w.inventory.apply(
        &w.catalog,
        a(),
        Command::Split {
            stack: held(RIG_A, w.bandages),
            count: 3,
            to: RIG_A,
            at: Cell::new(2, 1),
            rotation: Rotation::Deg0,
        },
        anywhere,
    );
    assert_eq!(
        refused,
        Err(Refusal::Grid(InventoryError::Occupied { x: 2, y: 1 }))
    );
    assert_eq!(w.inventory.next_stack(), Some(next), "a refusal minted");

    let mut minted = Vec::new();
    for (at, to) in [(Cell::new(0, 3), RIG_A), (Cell::new(2, 2), GROUND)] {
        let applied = w.inventory.apply(
            &w.catalog,
            a(),
            Command::Split {
                stack: held(RIG_A, w.bandages),
                count: 3,
                to,
                at,
                rotation: Rotation::Deg0,
            },
            anywhere,
        );
        let Ok(Applied::Split { minted: id }) = applied else {
            panic!("a split of 3 out of a stack that holds more was refused: {applied:?}");
        };
        assert_eq!(count_of(&w.inventory, to, id), Some(3));
        minted.push(id);
    }
    assert_eq!(minted, [next, StackId(next.0 + 1)], "one after another");
    assert_eq!(count_of(&w.inventory, RIG_A, w.bandages), Some(6));
    let every: Vec<StackId> = w
        .inventory
        .containers()
        .flat_map(|(_, grid)| grid.slots().map(|(_, placement)| placement.stack().id()))
        .collect();
    for id in &minted {
        assert_eq!(
            every.iter().filter(|held| *held == id).count(),
            1,
            "stack {} is held twice",
            id.0
        );
    }
}

/// **A merge stops at the item's stack maximum, across containers as within
/// one.** Twelve and fifteen bandages against a maximum of twenty: the rig
/// fills to twenty and seven stay in the crate; a second pour finds the rig
/// full; and a pour that fits whole takes the source's id with it.
#[test]
fn a_merge_stops_at_the_stack_maximum_across_containers() {
    let mut w = world();
    let (loose, bandages) = (w.loose, w.bandages);
    let merge = |w: &mut World, from, into| {
        w.inventory
            .apply(&w.catalog, a(), Command::Merge { from, into }, anywhere)
    };
    assert_eq!(
        merge(&mut w, held(CRATE, loose), held(RIG_A, bandages)),
        Ok(Applied::Merged { left: 7 })
    );
    assert_eq!(count_of(&w.inventory, RIG_A, w.bandages), Some(20));
    assert_eq!(count_of(&w.inventory, CRATE, w.loose), Some(7));

    let before = w.inventory.clone();
    assert_eq!(
        merge(&mut w, held(CRATE, loose), held(RIG_A, bandages)),
        Err(Refusal::Grid(InventoryError::StackOverflow {
            max: 20,
            count: 20
        }))
    );
    assert_eq!(w.inventory, before);

    assert_eq!(
        merge(&mut w, held(RIG_A, bandages), held(CRATE, loose)),
        Ok(Applied::Merged { left: 7 }),
        "20 into 7 moves 13 and leaves 7"
    );
    assert_eq!(count_of(&w.inventory, CRATE, w.loose), Some(20));
    assert_eq!(count_of(&w.inventory, RIG_A, w.bandages), Some(7));

    // Within one container, the grid's own merge.
    let split = w
        .inventory
        .apply(
            &w.catalog,
            a(),
            Command::Split {
                stack: held(RIG_A, w.bandages),
                count: 2,
                to: RIG_A,
                at: Cell::new(0, 3),
                rotation: Rotation::Deg0,
            },
            anywhere,
        )
        .expect("2 out of 7");
    let Applied::Split { minted } = split else {
        panic!("a split answered {split:?}");
    };
    assert_eq!(
        merge(&mut w, held(RIG_A, minted), held(RIG_A, bandages)),
        Ok(Applied::Merged { left: 0 })
    );
    assert_eq!(count_of(&w.inventory, RIG_A, w.bandages), Some(7));
    assert_eq!(where_is(&w.inventory, RIG_A, minted), None, "poured whole");
}

/// **An equip takes only what the slot is for**, and the slot is nothing but a
/// filtered grid: the bandage is refused by the tag, the helmet goes in, and a
/// second helmet finds the slot full rather than filtered.
#[test]
fn an_equip_respects_the_slot_filter() {
    let mut w = world();
    let (bandages, helmet) = (w.bandages, w.helmet);
    let tag = w.catalog.tag("helmet").expect("the helmet carries it");
    let equip = |w: &mut World, stack| {
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::Equip {
                stack,
                slot: HEAD_A,
            },
            anywhere,
        )
    };
    assert_eq!(
        equip(&mut w, held(RIG_A, bandages)),
        Err(Refusal::Grid(InventoryError::Filtered { filter: tag }))
    );
    assert_eq!(equip(&mut w, held(RIG_A, helmet)), Ok(Applied::Equipped));
    assert_eq!(
        where_is(&w.inventory, HEAD_A, w.helmet),
        Some(Cell::new(0, 0))
    );
    assert_eq!(where_is(&w.inventory, RIG_A, w.helmet), None);

    let spare = w
        .inventory
        .spawn(&w.catalog, RIG_A, id(&w.catalog, "helmet"), 1)
        .expect("the rig has room again");
    assert_eq!(
        equip(&mut w, held(RIG_A, spare)),
        Err(Refusal::Grid(InventoryError::NoRoom))
    );
}

/// **A drop lands in a world container, by first-fit, under the same id**, and
/// a take-all brings it back.
#[test]
fn a_drop_lands_in_the_world_and_a_take_all_brings_it_back() {
    let mut w = world();
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::Drop {
                stack: held(RIG_A, w.helmet),
                onto: GROUND,
            },
            anywhere,
        ),
        Ok(Applied::Dropped)
    );
    assert_eq!(
        where_is(&w.inventory, GROUND, w.helmet),
        Some(Cell::new(0, 0))
    );

    // B picks it up: the ground is anyone's.
    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            b(),
            Command::TakeAll {
                from: GROUND,
                to: RIG_B,
            },
            anywhere,
        ),
        Ok(Applied::TookAll { stacks: 1 })
    );
    assert_eq!(
        where_is(&w.inventory, RIG_B, w.helmet),
        Some(Cell::new(1, 0))
    );
}

/// **A stash keyed by one player is invisible to another.** Each player's
/// [`Inventory::stash`] yields their own grids only, a command from the other
/// player naming one is refused as not theirs, and the owner moves an item in
/// and out of it — rig to stash and back — as ordinary single transactions.
#[test]
fn a_stash_keyed_by_one_player_is_invisible_to_another() {
    let mut w = world();
    w.inventory.open_stash(a());
    w.inventory.open_stash(b());
    w.inventory.open_stash(a());
    let mine = ContainerId::Stash {
        player: a(),
        grid: 0,
    };
    let theirs = ContainerId::Stash {
        player: b(),
        grid: 0,
    };
    let ids = |player| -> Vec<ContainerId> {
        w.inventory
            .stash(player)
            .map(|(container, _)| container)
            .collect()
    };
    assert_eq!(ids(a()), [mine], "A's stash and nothing else, opened once");
    assert_eq!(ids(b()), [theirs]);
    assert!(
        ids(PlayerId::from_seed(3)).is_empty(),
        "an unopened stash is none"
    );
    assert_eq!(w.inventory.access(mine), Some(Access::Player(a())));

    let send = Command::Move {
        stack: held(RIG_A, w.mag),
        to: mine,
        at: Cell::new(2, 1),
        rotation: Rotation::Deg0,
    };
    assert_eq!(
        w.inventory.apply(&w.catalog, a(), send, anywhere),
        Ok(Applied::Moved),
        "send to stash"
    );
    assert_eq!(where_is(&w.inventory, mine, w.mag), Some(Cell::new(2, 1)));

    let before = w.inventory.clone();
    for (name, command) in [
        (
            "take from another's stash",
            Command::Move {
                stack: held(mine, w.mag),
                to: RIG_B,
                at: Cell::new(3, 0),
                rotation: Rotation::Deg0,
            },
        ),
        (
            "take all of another's stash",
            Command::TakeAll {
                from: mine,
                to: theirs,
            },
        ),
    ] {
        assert_eq!(
            w.inventory.apply(&w.catalog, b(), command, anywhere),
            Err(Refusal::NotYours(mine)),
            "{name}"
        );
        assert_eq!(w.inventory, before, "{name} changed something");
    }

    assert_eq!(
        w.inventory.apply(
            &w.catalog,
            a(),
            Command::Move {
                stack: held(mine, w.mag),
                to: RIG_A,
                at: Cell::new(3, 2),
                rotation: Rotation::Deg0,
            },
            anywhere,
        ),
        Ok(Applied::Moved),
        "take from stash"
    );
    assert!(w.inventory.grid(mine).expect("A's stash").is_empty());
}

/// **A container added holding a stack the inventory already holds is
/// refused**, and one added with ids minted elsewhere moves the mint past
/// them, so the next split cannot mint one of them again.
#[test]
fn an_added_container_cannot_duplicate_a_stack_and_moves_the_mint_past_its_ids() {
    let mut w = world();
    let catalog = &w.catalog;
    let mut copy = Grid::new(2, 2, None).expect("2x2");
    copy.insert(catalog, Stack::new(id(catalog, "mag"), w.mag, 1))
        .expect("room");
    let before = w.inventory.clone();
    assert_eq!(
        w.inventory.add(7, Access::Open, copy),
        Err(Refusal::DuplicateStack(w.mag)),
        "a copy of the rig's mag"
    );
    let mut twice = Grid::new(2, 2, None).expect("2x2");
    for _ in 0..2 {
        twice
            .insert(catalog, Stack::new(id(catalog, "bandage"), StackId(50), 1))
            .expect("room");
    }
    assert_eq!(
        w.inventory.add(7, Access::Open, twice),
        Err(Refusal::DuplicateStack(StackId(50))),
        "one id twice in one grid"
    );
    assert_eq!(
        w.inventory
            .add(1, Access::Open, Grid::new(1, 1, None).expect("1x1")),
        Err(Refusal::ContainerExists(RIG_A))
    );
    assert_eq!(w.inventory, before);

    let mut adopted = Grid::new(2, 2, None).expect("2x2");
    adopted
        .insert(catalog, Stack::new(id(catalog, "bandage"), StackId(40), 4))
        .expect("room");
    w.inventory
        .add(7, Access::Open, adopted)
        .expect("ids nothing else holds");
    assert_eq!(w.inventory.next_stack(), Some(StackId(41)));
}

/// **Spawn and despawn are the only doors**, and a refused spawn mints
/// nothing: a count no stack makes, an item the catalogue lacks and a full
/// grid are each refused with the mint where it was.
#[test]
fn a_refused_spawn_mints_nothing_and_a_despawn_hands_the_stack_back() {
    let mut w = world();
    let next = w.inventory.next_stack();
    let bandage = id(&w.catalog, "bandage");
    let before = w.inventory.clone();
    for (count, refusal) in [
        (0, Refusal::BadCount { count: 0, max: 20 }),
        (21, Refusal::BadCount { count: 21, max: 20 }),
    ] {
        assert_eq!(
            w.inventory.spawn(&w.catalog, RIG_A, bandage, count),
            Err(refusal)
        );
    }
    assert_eq!(
        w.inventory.spawn(&w.catalog, RIG_A, ItemId(9), 1),
        Err(Refusal::Grid(InventoryError::NoSuchItem(ItemId(9))))
    );
    assert_eq!(
        w.inventory.spawn(&w.catalog, HEAD_A, bandage, 1),
        Err(Refusal::Grid(InventoryError::Filtered {
            filter: w.catalog.tag("helmet").expect("the helmet carries it")
        }))
    );
    assert_eq!(w.inventory, before);
    assert_eq!(w.inventory.next_stack(), next);

    let gone = w
        .inventory
        .despawn(held(CRATE, w.brace))
        .expect("the brace is in the crate");
    assert_eq!(gone.id(), w.brace);
    assert_eq!(
        w.inventory.despawn(held(CRATE, w.brace)),
        Err(Refusal::NoSuchStack(held(CRATE, w.brace)))
    );
}
