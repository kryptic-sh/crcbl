//! **The no-dupe property** — `docs/plan/34-inventory.md`'s headline test.
//!
//! Thousands of seeded command streams, from several players at once, against
//! their rigs, their equipment slots, a crate and the ground they share, and
//! their stashes — every command kind, aimed well and badly, so that most of
//! them are refused. After every step:
//!
//! * **a refused command changed nothing**: the whole inventory compares
//!   equal to its copy from before the step, mint included;
//! * **the count of each item is conserved** by every command; only an
//!   explicit spawn adds to it and an explicit despawn takes from it;
//! * **the set of stack ids is conserved** except exactly where the protocol
//!   says: a split adds the one id it reports minting, and that id was never
//!   held before in the run; a merge that pours its source whole removes that
//!   source's id; a spawn adds its id and a despawn removes its own;
//! * **no id is held twice** and **no stack changes item**.
//!
//! The run then checks it reached every command both applied and refused, so
//! it cannot pass by generating nothing that lands.

use std::collections::{BTreeMap, BTreeSet};

use crcbl_rand::Rng;

use super::*;
use crate::catalog::ItemDef;
use crate::shape::{Cell, Rotation};
use crate::test_items::items;

/// How many command streams run, each from its own seed.
const SEQUENCES: u64 = 2_000;

/// How many steps each stream takes.
const STEPS: usize = 64;

/// The first stream's seed; stream `n` is seeded `SEED + n`, which is what a
/// failure prints.
const SEED: u64 = 0x5EED_0034;

/// How many steps a stream spends spawning before it issues commands.
const OPENING_SPAWNS: usize = 12;

/// How many players issue commands.
const PLAYERS: usize = 3;

/// The crate everyone but the last player can reach.
const CRATE: ContainerId = ContainerId::Local(1);

/// The ground, which everyone can reach.
const GROUND: ContainerId = ContainerId::Local(2);

/// A container no inventory here holds.
const NOWHERE: ContainerId = ContainerId::Local(99);

/// A stack id no inventory here mints within a run.
const GHOST: StackId = StackId(1_000_000);

/// The six commands, in the order the tally counts them.
const KINDS: [&str; 6] = ["move", "split", "merge", "equip", "drop", "take-all"];

fn player(index: usize) -> PlayerId {
    PlayerId::from_seed(u64::try_from(index).expect("a few players") + 100)
}

/// Player `index`'s rig.
fn rig(index: usize) -> ContainerId {
    ContainerId::Local(10 + u32::try_from(index).expect("a few players"))
}

/// Player `index`'s helmet slot.
fn slot(index: usize) -> ContainerId {
    ContainerId::Local(20 + u32::try_from(index).expect("a few players"))
}

/// The last player cannot reach the crate; everything else is in reach.
fn reach(who: PlayerId, container: ContainerId) -> bool {
    !(who == player(PLAYERS - 1) && container == CRATE)
}

/// A number below `n` from the stream.
fn below(rng: &mut Rng, n: usize) -> usize {
    usize::try_from(rng.next_u32()).expect("a u32 fits a usize") % n
}

/// `true` roughly `percent` times in a hundred.
fn chance(rng: &mut Rng, percent: usize) -> bool {
    below(rng, 100) < percent
}

/// Every stack the inventory holds, by id: where it is, what it is, how many.
/// Panics if one id is held twice, which is the duplication this test exists
/// to catch.
fn ledger(inventory: &Inventory) -> BTreeMap<StackId, (ContainerId, ItemId, u16)> {
    let mut stacks = BTreeMap::new();
    for (container, grid) in inventory.containers() {
        for (_, placement) in grid.slots() {
            let stack = placement.stack();
            let earlier = stacks.insert(stack.id(), (container, stack.item(), stack.count()));
            assert!(earlier.is_none(), "stack {} is held twice", stack.id().0);
        }
    }
    stacks
}

/// How many of each item the ledger holds.
fn totals(stacks: &BTreeMap<StackId, (ContainerId, ItemId, u16)>) -> BTreeMap<ItemId, u32> {
    let mut totals = BTreeMap::new();
    for &(_, item, count) in stacks.values() {
        *totals.entry(item).or_insert(0) += u32::from(count);
    }
    totals
}

/// The world a stream starts in: every player's rig and slot, the crate and
/// the ground, and every player's stash opened.
fn opening(catalog: &Catalog) -> Inventory {
    let helmet = catalog
        .tag("helmet")
        .expect("the test catalogue has the tag");
    let mut inventory =
        Inventory::with_stash(StashLayout::new(&[(3, 3), (2, 2)]).expect("a stash layout"));
    let local = |id: ContainerId| match id {
        ContainerId::Local(local) => local,
        ContainerId::Stash { .. } => unreachable!("only local ids are added"),
    };
    for index in 0..PLAYERS {
        let owner = Access::Player(player(index));
        inventory
            .add(
                local(rig(index)),
                owner,
                Grid::new(4, 3, None).expect("4x3"),
            )
            .expect("a new id");
        inventory
            .add(
                local(slot(index)),
                owner,
                Grid::new(2, 2, Some(helmet)).expect("2x2"),
            )
            .expect("a new id");
        inventory.open_stash(player(index));
    }
    inventory
        .add(
            local(CRATE),
            Access::Open,
            Grid::new(3, 3, None).expect("3x3"),
        )
        .expect("a new id");
    inventory
        .add(
            local(GROUND),
            Access::Open,
            Grid::new(4, 4, None).expect("4x4"),
        )
        .expect("a new id");
    inventory
}

/// What happened over the whole run, so it can say it reached everything.
#[derive(Default)]
struct Tally {
    applied: [usize; KINDS.len()],
    refused: [usize; KINDS.len()],
    spawned: usize,
    despawned: usize,
}

/// A stack to aim a command at: usually one that is there, sometimes a real
/// id named in the wrong container, sometimes an id nothing holds.
fn aim(
    rng: &mut Rng,
    stacks: &BTreeMap<StackId, (ContainerId, ItemId, u16)>,
    containers: &[ContainerId],
) -> Held {
    let real: Vec<(&StackId, &(ContainerId, ItemId, u16))> = stacks.iter().collect();
    let roll = below(rng, 100);
    if real.is_empty() || roll >= 92 {
        return Held {
            container: containers[below(rng, containers.len())],
            stack: GHOST,
        };
    }
    let (&stack, &(container, _, _)) = real[below(rng, real.len())];
    let container = if roll >= 80 {
        containers[below(rng, containers.len())]
    } else {
        container
    };
    Held { container, stack }
}

/// One random command: its kind's index into [`KINDS`], who issues it, and the
/// command.
fn command(
    rng: &mut Rng,
    inventory: &Inventory,
    stacks: &BTreeMap<StackId, (ContainerId, ItemId, u16)>,
    containers: &[ContainerId],
) -> (usize, PlayerId, Command) {
    let stack = aim(rng, stacks, containers);
    // Usually whoever owns the stack's container, so that ownership refusals
    // do not drown everything else out.
    let actor = match inventory.access(stack.container) {
        Some(Access::Player(owner)) if chance(rng, 70) => owner,
        _ => player(below(rng, PLAYERS)),
    };
    let anywhere = |rng: &mut Rng| containers[below(rng, containers.len())];
    // A cell inside the destination, or one past its edge now and then.
    let cell = |rng: &mut Rng, to: ContainerId| {
        let (w, h) = inventory
            .grid(to)
            .map_or((1, 1), |grid| (grid.width(), grid.height()));
        Cell::new(
            u8::try_from(below(rng, usize::from(w) + 1)).expect("small"),
            u8::try_from(below(rng, usize::from(h) + 1)).expect("small"),
        )
    };
    // A count inside the stack most of the time, so splits land.
    let held = stacks.get(&stack.stack).map_or(1, |&(_, _, count)| count);
    let count = |rng: &mut Rng| {
        let count = if chance(rng, 70) && held > 1 {
            1 + below(rng, usize::from(held) - 1)
        } else {
            below(rng, 22)
        };
        u16::try_from(count).expect("small")
    };
    let rotation = |rng: &mut Rng| Rotation::ALL[below(rng, Rotation::ALL.len())];
    let kind = below(rng, KINDS.len());
    let issued = match kind {
        0 => {
            let to = anywhere(rng);
            Command::Move {
                stack,
                to,
                at: cell(rng, to),
                rotation: rotation(rng),
            }
        }
        1 => {
            let to = anywhere(rng);
            Command::Split {
                stack,
                count: count(rng),
                to,
                at: cell(rng, to),
                rotation: rotation(rng),
            }
        }
        2 => {
            // Half the time a stack of the same item, so merges land.
            let item = stacks.get(&stack.stack).map(|&(_, item, _)| item);
            let same: Vec<(&StackId, &(ContainerId, ItemId, u16))> = stacks
                .iter()
                .filter(|&(_, &(_, other, _))| Some(other) == item)
                .collect();
            let into = if !same.is_empty() && chance(rng, 50) {
                let (&id, &(container, _, _)) = same[below(rng, same.len())];
                Held {
                    container,
                    stack: id,
                }
            } else {
                aim(rng, stacks, containers)
            };
            Command::Merge { from: stack, into }
        }
        3 => Command::Equip {
            stack,
            slot: if chance(rng, 70) {
                let owner = (0..PLAYERS).find(|&index| player(index) == actor);
                owner.map_or_else(|| anywhere(rng), slot)
            } else {
                anywhere(rng)
            },
        },
        4 => Command::Drop {
            stack,
            onto: if chance(rng, 70) {
                [CRATE, GROUND][below(rng, 2)]
            } else {
                anywhere(rng)
            },
        },
        _ => Command::TakeAll {
            from: anywhere(rng),
            to: anywhere(rng),
        },
    };
    (kind, actor, issued)
}

/// Checks one step's invariants. `before` is the ledger the step started from
/// and `ever` every id held at any point of the run so far.
fn check_applied(
    context: &str,
    applied: Applied,
    before: &BTreeMap<StackId, (ContainerId, ItemId, u16)>,
    after: &BTreeMap<StackId, (ContainerId, ItemId, u16)>,
    from: Option<Held>,
    ever: &BTreeSet<StackId>,
) {
    assert_eq!(
        totals(after),
        totals(before),
        "{context}: an item count moved"
    );
    let had: BTreeSet<StackId> = before.keys().copied().collect();
    let has: BTreeSet<StackId> = after.keys().copied().collect();
    let mut expected = had.clone();
    match applied {
        Applied::Split { minted } => {
            assert!(
                !ever.contains(&minted),
                "{context}: minted {} again",
                minted.0
            );
            expected.insert(minted);
        }
        Applied::Merged { left: 0 } => {
            let source = from.expect("a merge names its source").stack;
            expected.remove(&source);
        }
        _ => {}
    }
    assert_eq!(has, expected, "{context}: the stack ids changed");
    for (id, &(_, item, _)) in after {
        if let Some(&(_, was, _)) = before.get(id) {
            assert_eq!(item, was, "{context}: stack {} changed item", id.0);
        }
    }
}

/// Plays one stream from `seed`.
fn play(seed: u64, catalog: &Catalog, tally: &mut Tally) {
    let mut rng = Rng::from_u64(seed);
    let mut inventory = opening(catalog);
    let containers: Vec<ContainerId> = inventory
        .containers()
        .map(|(id, _)| id)
        .chain([NOWHERE])
        .collect();
    let item_ids: Vec<ItemId> = catalog.items().map(|(id, _)| id).collect();
    let mut ever: BTreeSet<StackId> = BTreeSet::new();

    for step in 0..STEPS {
        let context = format!("seed {seed:#x}, step {step}");
        let before = inventory.clone();
        let stacks = ledger(&inventory);
        ever.extend(stacks.keys().copied());
        let roll = below(&mut rng, 100);

        // A stream opens with a run of spawns, so its first commands have
        // something to move.
        if roll < 12 || step < OPENING_SPAWNS {
            // The explicit spawn: the one way an item enters.
            let item = item_ids[below(&mut rng, item_ids.len())];
            // Mostly a count the item stacks to, now and then one it does not.
            let max = catalog.get(item).map_or(1, ItemDef::stack_max);
            let count = if chance(&mut rng, 90) {
                1 + below(&mut rng, usize::from(max))
            } else {
                below(&mut rng, usize::from(max) + 2)
            };
            let count = u16::try_from(count).expect("small");
            let into = containers[below(&mut rng, containers.len())];
            match inventory.spawn(catalog, into, item, count) {
                Ok(id) => {
                    tally.spawned += 1;
                    assert!(!ever.contains(&id), "{context}: spawned {} again", id.0);
                    let after = ledger(&inventory);
                    let mut grown = totals(&stacks);
                    *grown.entry(item).or_insert(0) += u32::from(count);
                    assert_eq!(totals(&after), grown, "{context}: a spawn miscounted");
                    assert_eq!(after.len(), stacks.len() + 1);
                }
                Err(_) => assert_eq!(inventory, before, "{context}: a refused spawn changed"),
            }
            continue;
        }
        if roll < 15 {
            // The explicit despawn: the one way an item leaves.
            let held = aim(&mut rng, &stacks, &containers);
            match inventory.despawn(held) {
                Ok(stack) => {
                    tally.despawned += 1;
                    let after = ledger(&inventory);
                    let mut shrunk = totals(&stacks);
                    *shrunk.entry(stack.item()).or_insert(0) -= u32::from(stack.count());
                    shrunk.retain(|_, count| *count > 0);
                    assert_eq!(totals(&after), shrunk, "{context}: a despawn miscounted");
                    assert!(!after.contains_key(&stack.id()));
                }
                Err(_) => assert_eq!(inventory, before, "{context}: a refused despawn changed"),
            }
            continue;
        }

        let (kind, actor, issued) = command(&mut rng, &inventory, &stacks, &containers);
        let context = format!("{context}, {issued:?} by {actor}");
        match inventory.apply(catalog, actor, issued, reach) {
            Ok(applied) => {
                tally.applied[kind] += 1;
                let from = match issued {
                    Command::Merge { from, .. } => Some(from),
                    _ => None,
                };
                check_applied(&context, applied, &stacks, &ledger(&inventory), from, &ever);
            }
            Err(_) => {
                tally.refused[kind] += 1;
                assert_eq!(inventory, before, "{context}: a refusal changed something");
            }
        }
    }
}

/// **No command stream duplicates or loses an item.** See the module docs for
/// every invariant each step is held to.
#[test]
fn no_command_stream_duplicates_or_loses_an_item() {
    let catalog = items();
    let mut tally = Tally::default();
    for sequence in 0..SEQUENCES {
        play(SEED + sequence, &catalog, &mut tally);
    }
    for (index, kind) in KINDS.iter().enumerate() {
        assert!(tally.applied[index] > 0, "no {kind} ever landed");
        assert!(tally.refused[index] > 0, "no {kind} was ever refused");
    }
    assert!(tally.spawned > 0, "nothing was ever spawned");
    assert!(tally.despawned > 0, "nothing was ever despawned");
}
