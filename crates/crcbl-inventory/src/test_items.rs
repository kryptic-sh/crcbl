//! The test catalogue every unit test in the crate is written against.

use crate::catalog::{Catalog, ItemId};

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

/// [`ITEMS`], parsed.
pub(crate) fn items() -> Catalog {
    Catalog::from_ron(ITEMS).expect("the test catalogue parses")
}

/// The id of `name`, which every test knows is in [`ITEMS`].
pub(crate) fn id(catalog: &Catalog, name: &str) -> ItemId {
    catalog.id_of(name).expect("the test catalogue has it")
}
