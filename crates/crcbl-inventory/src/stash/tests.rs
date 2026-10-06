use crcbl_store::{MemoryStorage, NativeStorage};

use super::*;
use crate::Refusal;
use crate::command::{Command, Held};
use crate::test_items::{id, items};

/// Where the tests keep a stash.
const FILE: &str = "stash.crst";

fn a() -> PlayerId {
    PlayerId::from_seed(1)
}

fn b() -> PlayerId {
    PlayerId::from_seed(2)
}

fn layout() -> StashLayout {
    StashLayout::new(&[(3, 3), (2, 1)]).expect("a stash layout")
}

fn reach_anything(_: PlayerId, _: ContainerId) -> bool {
    true
}

/// A server's inventory: two players' stashes, filled, a split that moved the
/// mint, and a carried rig the file must not name.
fn served(catalog: &Catalog) -> Inventory {
    let mut inventory = Inventory::with_stash(layout());
    inventory.open_stash(a());
    inventory.open_stash(b());
    let rig = inventory
        .add(1, Access::Player(a()), Grid::new(4, 4, None).expect("4x4"))
        .expect("a new id");
    let mine = ContainerId::Stash {
        player: a(),
        grid: 0,
    };
    let theirs = ContainerId::Stash {
        player: b(),
        grid: 1,
    };
    let mut spawn = |into, name, count| {
        inventory
            .spawn(catalog, into, id(catalog, name), count)
            .expect("room")
    };
    let bandages = spawn(mine, "bandage", 9);
    spawn(mine, "brace", 1);
    spawn(theirs, "mag", 1);
    spawn(rig, "helmet", 1);
    inventory
        .apply(
            catalog,
            a(),
            Command::Split {
                stack: Held {
                    container: mine,
                    stack: bandages,
                },
                count: 4,
                to: ContainerId::Stash {
                    player: a(),
                    grid: 1,
                },
                at: Cell::new(1, 0),
                rotation: Rotation::Deg0,
            },
            reach_anything,
        )
        .expect("4 out of 9");
    inventory
}

/// The stash half of `inventory`: every stash container, as id and grid.
fn stashes(inventory: &Inventory) -> Vec<(ContainerId, Grid)> {
    inventory
        .containers()
        .filter(|(id, _)| matches!(id, ContainerId::Stash { .. }))
        .map(|(id, grid)| (id, grid.clone()))
        .collect()
}

/// **A stash round-trips through its file**, through the in-memory store and
/// through `NativeStorage`, whose write is `write_atomic` — every grid,
/// placement, count, rotation and id where it was, the mint where it was, and
/// the carried rig nowhere in it. Written back out, the loaded stash is the
/// same bytes.
#[test]
fn a_stash_round_trips_through_its_file() {
    let catalog = items();
    let inventory = served(&catalog);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let native = NativeStorage::at(dir.path().to_path_buf());
    let memory = MemoryStorage::new();
    let stores: [&dyn StorageSource; 2] = [&memory, &native];
    for storage in stores {
        let written = save(&inventory, &catalog, storage, Path::new(FILE)).expect("it saves");
        let read = load(&catalog, layout(), storage, Path::new(FILE)).expect("it loads");
        assert_eq!(stashes(&read), stashes(&inventory), "{storage:?}");
        assert_eq!(read.next_stack(), inventory.next_stack(), "the mint");
        assert_eq!(read.next_stack(), Some(StackId(5)), "four spawns, a split");
        assert_eq!(
            read.grid(ContainerId::Local(1)),
            None,
            "the rig is not the stash's"
        );
        assert_eq!(read.stash_layout(), &layout());
        let again = encode(&read, &catalog).expect("it encodes");
        assert_eq!(again.len(), written);
        assert_eq!(again, storage.read(Path::new(FILE)).expect("it is there"));
    }
    assert!(
        dir.path().join(FILE).is_file(),
        "NativeStorage wrote a file"
    );

    // A server that never saved has nothing to read, and says so.
    assert!(matches!(
        load(&catalog, layout(), &MemoryStorage::new(), Path::new(FILE)),
        Err(StashError::Storage(StorageError::NotFound(_)))
    ));
}

/// **One player's stash stays theirs across a restart**: after a reload each
/// player's [`Inventory::stash`] holds their own grids and items, and the
/// other player's command naming one is refused.
#[test]
fn a_reloaded_stash_is_still_its_players_alone() {
    let catalog = items();
    let bytes = encode(&served(&catalog), &catalog).expect("it encodes");
    let mut read = decode(&bytes, &catalog, layout()).expect("it decodes");
    let held = |inventory: &Inventory, player| -> Vec<StackId> {
        inventory
            .stash(player)
            .flat_map(|(_, grid)| grid.slots().map(|(_, placement)| placement.stack().id()))
            .collect()
    };
    assert_eq!(held(&read, a()), [StackId(0), StackId(1), StackId(4)]);
    assert_eq!(held(&read, b()), [StackId(2)]);

    let mine = ContainerId::Stash {
        player: a(),
        grid: 0,
    };
    let before = read.clone();
    assert_eq!(
        read.apply(
            &catalog,
            b(),
            Command::TakeAll {
                from: mine,
                to: ContainerId::Stash {
                    player: b(),
                    grid: 0,
                },
            },
            reach_anything,
        ),
        Err(Refusal::NotYours(mine))
    );
    assert_eq!(read, before);
}

/// `body` with its checksum recomputed, so a test can say what a file that
/// disagrees with itself *inside* a valid checksum reads as.
fn sealed(mut body: Vec<u8>) -> Vec<u8> {
    let checksum = crc32(&body);
    body.extend_from_slice(&checksum.to_le_bytes());
    body
}

/// **A stash file the reader cannot stand behind is refused, naming why.** A
/// flipped bit fails the checksum; a foreign file fails the magic; a newer
/// file is refused rather than misread; and inside a valid checksum, one stack
/// twice, an id past the mint, two stacks in one cell, a count past the item's
/// maximum and trailing bytes each read as damaged.
#[test]
fn a_stash_file_that_disagrees_with_itself_is_refused() {
    let catalog = items();
    let mut inventory = Inventory::with_stash(layout());
    inventory.open_stash(a());
    let mine = ContainerId::Stash {
        player: a(),
        grid: 0,
    };
    for count in [3, 2] {
        inventory
            .spawn(&catalog, mine, id(&catalog, "bandage"), count)
            .expect("room");
    }
    let good = encode(&inventory, &catalog).expect("it encodes");
    decode(&good, &catalog, layout()).expect("the control decodes");
    let body = good[..good.len() - CHECKSUM].to_vec();

    // The offsets of the fields the cases below rewrite.
    let mint_at = PREAMBLE;
    let first_placement = PREAMBLE + 8 + 4 + PLAYER_HEAD + 4;
    let second_placement = first_placement + PLACEMENT_BYTES;
    let id_at = |placement: usize| placement + 4;
    let count_at = |placement: usize| placement + 8;
    let cell_at = |placement: usize| placement + 10;

    let mut flipped = good.clone();
    flipped[first_placement] ^= 1;
    let mut newer = body.clone();
    newer[MAGIC.len()..PREAMBLE].copy_from_slice(&(STASH_FORMAT_VERSION + 1).to_le_bytes());
    let mut twice = body.clone();
    twice.copy_within(
        id_at(first_placement)..id_at(first_placement) + 4,
        id_at(second_placement),
    );
    let mut unminted = body.clone();
    unminted[mint_at..mint_at + 8].copy_from_slice(&1u64.to_le_bytes());
    let mut stacked = body.clone();
    stacked.copy_within(
        cell_at(first_placement)..cell_at(first_placement) + 2,
        cell_at(second_placement),
    );
    let mut heavy = body.clone();
    heavy[count_at(first_placement)..count_at(first_placement) + 2]
        .copy_from_slice(&21u16.to_le_bytes());
    let mut trailing = body;
    trailing.push(0);

    let cases: [(&str, Vec<u8>, &str); 8] = [
        ("a flipped bit", flipped, "checksum"),
        ("a foreign file", sealed(b"NOPE\x01\x00".to_vec()), "magic"),
        ("a newer file", sealed(newer), "newer"),
        ("one stack twice", sealed(twice), "one stack twice"),
        (
            "an id past the mint",
            sealed(unminted),
            "a stack id the mint never handed out",
        ),
        (
            "two stacks in one cell",
            sealed(stacked),
            "a placement its grid refuses",
        ),
        (
            "a count past the maximum",
            sealed(heavy),
            "a count its item does not stack to",
        ),
        (
            "trailing bytes",
            sealed(trailing),
            "bytes after the last player",
        ),
    ];
    for (name, bytes, expected) in cases {
        let reason = match decode(&bytes, &catalog, layout()) {
            Err(StashError::Checksum) => "checksum",
            Err(StashError::Magic) => "magic",
            Err(StashError::Newer { found, current }) => {
                assert_eq!(
                    (found, current),
                    (STASH_FORMAT_VERSION + 1, STASH_FORMAT_VERSION)
                );
                "newer"
            }
            Err(StashError::Damaged(reason)) => reason,
            other => panic!("{name} read as {other:?}"),
        };
        assert_eq!(reason, expected, "{name}");
    }
}

/// **A layout that is not one is refused**, and one that is opens each
/// player's grids at its sizes.
#[test]
fn a_stash_layout_is_held_to_its_rules() {
    let too_many = [(1, 1); MAX_STASH_GRIDS + 1];
    for (grids, expected) in [
        (&[][..], "a stash names no grid"),
        (&[(3, 0)][..], "a grid with no cells"),
        (&too_many[..], "more grids than a stash counts"),
    ] {
        assert!(
            matches!(StashLayout::new(grids), Err(StashError::Layout(reason)) if reason == expected),
            "{expected}"
        );
    }
    StashLayout::new(&[(1, 1); MAX_STASH_GRIDS]).expect("the most grids a stash counts");

    let mut inventory = Inventory::with_stash(layout());
    inventory.open_stash(a());
    let sizes: Vec<(u8, u8)> = inventory
        .stash(a())
        .map(|(_, grid)| (grid.width(), grid.height()))
        .collect();
    assert_eq!(sizes, layout().grids().collect::<Vec<_>>());

    // A game with no stash opens none.
    let mut none = Inventory::new();
    none.open_stash(a());
    assert_eq!(none.stash(a()).count(), 0, "a stash with no layout");
}
