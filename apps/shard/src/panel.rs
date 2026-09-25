//! The inventory panel: the carried grid, and the drag that rearranges it.
//!
//! ```text
//!  ┌ INVENTORY ─────┐
//!  │ ┌──┬──┬──┬──┐  │
//!  │ │p │p │  │b │  │   p  a warden plate, two cells by two
//!  │ ├──┼──┼──┼──┤  │   b  a bandage, x3
//!  │ │p │p │  │  │  │
//!  │ └──┴──┴──┴──┘  │
//!  │ 2 items 2430 g │
//!  └────────────────┘
//! ```
//!
//! # Closed by default, and nothing on it ticks
//!
//! `I` opens it, and until it is opened this module draws nothing at all. Both
//! halves are load-bearing for the same reason [`crate::page`]'s are:
//! `web/tools/browser-e2e.mjs` douses the zone's torches and then asserts the
//! **canvas stops changing**, so a panel that opened itself — or that drew a
//! clock, a count of frames or a flicker — would make that control impossible to
//! pass on a working build. Everything drawn here is a fact about what the
//! character is carrying, which changes when the *player* does something and
//! holds still otherwise.
//!
//! # The drag is the engine's; the payload and the rule are shard's
//!
//! [`crcbl::ui::grid_drag`] is the drag: the per-cell hit test, the press
//! capture it rides on, the grab offset, and the release that ended where it
//! began filtered out. What this module supplies is what only the game knows —
//! what a cell holds when a press lands on it (the [`SlotId`] of the placement
//! covering it, gripped at that placement's origin) and whether the grid would
//! take it where the pointer is ([`Grid::can_move_within`]). The
//! answer comes back as [`DropFeedback`], which is what a refusing cell is
//! drawn from.
//!
//! # There are no icons
//!
//! A cell is the item's [`colour`](crcbl::inventory::ItemDef::colour) with its
//! [`letter`](crcbl::inventory::ItemDef::letter) on it. `crcbl icon bake` is the
//! plan's answer and it is not a verb yet; the placeholder pair lives in the
//! catalogue rather than in a table of shard's own, because the catalogue is
//! already the one file that names every item.
//!
//! # A tier is the outline, and it is derived rather than held
//!
//! Every cell a stack covers is outlined in its [`Rarity`], so a rare find is
//! legible at a glance without a second glyph in a cell that already carries a
//! letter and a count. The tier is not something this module is handed per
//! stack: it is [`loot::rarity_of`] of the run's seed and the foe whose id the
//! stack carries, which is why [`draw`] takes a seed. `crate::loot` argues why
//! nothing stores it.

use crcbl::inventory::{Cell, Grid, SlotId};
use crcbl::math::{UVec2, Vec2};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::grid_drag::{CellGrid, DropFeedback, GridDrag, Grip};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::{ButtonState, NATURAL_FONT_SIZE, PointerInput, UiState, WidgetId};

use crate::foe::FOES;
use crate::loot::{self, Rarity};

/// The panel's own background, a shade darker than [`crate::page`]'s so the two
/// read as different surfaces when they overlap.
const PANEL_BG: [f32; 4] = [0.05, 0.04, 0.03, 0.92];
/// The panel's border, and the grid's lines.
const BORDER: [f32; 4] = [0.38, 0.32, 0.25, 1.0];
/// An empty cell.
const CELL_BG: [f32; 4] = [0.10, 0.09, 0.08, 1.0];
/// A cell the pointer is over, one it is dragging from, or one a drag held
/// over it would land in.
const CELL_LIT: [f32; 4] = [0.20, 0.18, 0.15, 1.0];
/// A cell a drag held over it would not land in.
const CELL_REFUSED: [f32; 4] = [0.30, 0.09, 0.07, 1.0];
/// The title and the summary line.
const LABEL: [f32; 4] = [0.72, 0.66, 0.58, 1.0];
/// A letter or a count drawn over an item's own colour.
const GLYPH: [f32; 4] = [0.06, 0.05, 0.04, 1.0];

/// What a [`Rarity::Common`] find's cells are outlined in — a shade off
/// [`BORDER`], so an ordinary find reads as ordinary without disappearing into
/// the grid it is sitting in.
const TIER_COMMON: [f32; 4] = [0.55, 0.52, 0.48, 1.0];
/// …a [`Rarity::Uncommon`] one's.
const TIER_UNCOMMON: [f32; 4] = [0.42, 0.78, 0.52, 1.0];
/// …and a [`Rarity::Rare`] one's, which is the only tier drawn brighter than
/// anything else on the panel.
const TIER_RARE: [f32; 4] = [0.98, 0.78, 0.30, 1.0];

/// The outline a tier is drawn in.
const fn tier_colour(rarity: Rarity) -> [f32; 4] {
    match rarity {
        Rarity::Common => TIER_COMMON,
        Rarity::Uncommon => TIER_UNCOMMON,
        Rarity::Rare => TIER_RARE,
    }
}

/// How wide one cell is, in pixels.
const CELL_PX: f32 = 34.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// The height of the title row and of the summary row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// How thick the panel's border and the cell outlines are, in pixels.
const BORDER_WIDTH: f32 = 1.0;
/// The scale [`FontAtlas::text_width`] is measured at — the natural size, as
/// [`crate::page`] draws at.
const NATURAL_SCALE: f32 = 1.0;

/// The title.
const TITLE: &str = "INVENTORY";

/// The first widget id the cells use.
///
/// Above [`crcbl::engine::FIRST_GAME_ID`] because these are the game's own
/// widgets, and far above it because the menu ids live near it — a cell and a
/// menu row that shared a number would share a press capture.
const CELL_ID_BASE: WidgetId = 0x5_0000;

/// What one frame of the panel drew, and what the pointer asked of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelStats {
    /// How many draw commands it produced.
    pub commands: usize,
    /// The drag the pointer just finished: the stack it took hold of and the
    /// cell that stack's origin lands on, grab offset applied. `None` on every
    /// frame but the one a drag ends on, on a release that ended where it
    /// began — which is a click, and this panel has nothing for a click to do —
    /// and on a release over a cell the grid would not take it in.
    pub dragged: Option<(SlotId, Cell)>,
}

/// Where the panel's outer rectangle sits on a surface of `extent`.
///
/// Centred, because it is the thing being looked at while it is open, and
/// derived from the extent for [`crate::page`]'s reason: a layout written
/// against a fixed size is one that is wrong in a resized window and in the
/// headless ring at whatever `--size` asked for.
#[must_use]
pub fn bounds(extent: (u32, u32)) -> (Vec2, Vec2) {
    let width = 2.0f32.mul_add(PANEL_PAD, f32::from(loot::GRID_W) * CELL_PX);
    let height = 2.0f32.mul_add(PANEL_PAD, f32::from(loot::GRID_H) * CELL_PX) + 2.0 * ROW_HEIGHT;
    let min = Vec2::new(
        (extent.0 as f32 - width) * 0.5,
        (extent.1 as f32 - height) * 0.5,
    );
    (min, min + Vec2::new(width, height))
}

/// The grid's cells on a surface of `extent`: where cell `(0, 0)`'s top-left
/// corner is, how big a cell is, and the widget ids they answer to.
fn cells(extent: (u32, u32)) -> CellGrid {
    let (min, _) = bounds(extent);
    CellGrid {
        origin: Vec2::new(min.x + PANEL_PAD, min.y + PANEL_PAD + ROW_HEIGHT),
        cell: Vec2::splat(CELL_PX),
        columns: u32::from(loot::GRID_W),
        rows: u32::from(loot::GRID_H),
        id_base: CELL_ID_BASE,
        window: None,
    }
}

/// A kit cell as the drag's coordinates.
fn to_drag(cell: Cell) -> UVec2 {
    UVec2::new(u32::from(cell.x), u32::from(cell.y))
}

/// The drag's coordinates as a kit cell, or `None` past what a `u8` holds.
fn from_drag(cell: UVec2) -> Option<Cell> {
    Some(Cell::new(
        u8::try_from(cell.x).ok()?,
        u8::try_from(cell.y).ok()?,
    ))
}

/// Where `cell` is drawn, in pixels.
#[must_use]
pub fn cell_bounds(extent: (u32, u32), cell: Cell) -> (Vec2, Vec2) {
    cells(extent).cell_bounds(to_drag(cell))
}

/// Which cell `pos` is over, or `None` for a point outside the grid.
///
/// The panel's hit test and the one a test aims with, so a check that clicks a
/// cell is clicking the cell the frame drew rather than a rectangle it worked
/// out for itself.
#[must_use]
pub fn cell_at(extent: (u32, u32), pos: Vec2) -> Option<Cell> {
    cells(extent).cell_at(pos).and_then(from_drag)
}

/// Whether `grid` would take the stack at `slot` with its origin on `at`, in
/// the rotation it already has.
///
/// [`Grid::can_move_within`] does not count the stack's own cells as
/// occupied, so a one-cell nudge of the `2×2` is a move rather than a
/// collision with itself.
fn accepts(grid: &Grid, slot: SlotId, at: Cell) -> bool {
    let Some(placement) = grid.slot(slot) else {
        return false;
    };
    grid.can_move_within(loot::catalog(), slot, at, placement.rotation())
        .is_ok()
}

/// What the panel keeps between frames: which cell owns the pointer press, and
/// the drag riding on that press — which stack it took hold of, and where.
#[derive(Debug, Default)]
pub struct PanelState {
    ui: UiState,
    drag: GridDrag<SlotId>,
}

impl PanelState {
    /// Nothing pressed and nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the press and the drag with it — for a panel torn down mid-press,
    /// which is what [`UiState::clear`] is for.
    pub fn clear(&mut self) {
        self.ui.clear();
        self.drag = GridDrag::new();
    }
}

/// Draws the panel and resolves this frame's pointer against it.
///
/// `state` is what a drag cannot do without, and it is the caller's because it
/// has to survive between frames. `seed` is the run's loot seed, which is what
/// a stack's tier is rolled from — see the module docs.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    grid: &Grid,
    seed: u32,
    state: &mut PanelState,
    pointer: PointerInput,
) -> PanelStats {
    let (min, max) = bounds(extent);
    list.rect(min, max, PANEL_BG);
    list.rect_outline(min, max, BORDER_WIDTH, BORDER);
    list.text(
        Vec2::new(min.x + PANEL_PAD, min.y + PANEL_PAD * 0.5),
        TITLE.to_string(),
        LABEL,
        NATURAL_FONT_SIZE,
    );

    let cells = cells(extent);
    let mut frame = state.drag.frame(&mut state.ui, pointer);
    let response = frame.grid(
        &cells,
        |cell| {
            let slot = grid.at(from_drag(cell)?)?;
            Some(Grip {
                payload: slot,
                origin: to_drag(grid.slot(slot)?.at()),
            })
        },
        |slot, target| from_drag(target.origin).is_some_and(|at| accepts(grid, *slot, at)),
    );
    let dropped = frame.finish();

    for cell in cells.cells() {
        let (at, to) = cells.cell_bounds(cell);
        let answer = response.cell(cell);
        let fill = match answer.drop {
            DropFeedback::Refusing => CELL_REFUSED,
            DropFeedback::Accepting => CELL_LIT,
            DropFeedback::None if answer.state != ButtonState::Idle => CELL_LIT,
            DropFeedback::None => CELL_BG,
        };
        list.rect(at, to, fill);
        list.rect_outline(at, to, BORDER_WIDTH, BORDER);
    }

    let catalog = loot::catalog();
    for (_, placement) in grid.slots() {
        let stack = placement.stack();
        let Some(def) = catalog.get(stack.item()) else {
            continue;
        };
        // The tier of the foe that minted this stack. `None` is an id no foe of
        // this roster mints, which `crate::save`'s decoder refuses and a played
        // session cannot produce — such a stack is drawn without a tier rather
        // than with a guessed one.
        let tier = loot::foe_of(stack.id(), FOES).map(|foe| loot::rarity_of(seed, foe));
        let shape = def.shape().rotated(placement.rotation());
        for covered in shape.cells() {
            let cell = Cell::new(placement.at().x + covered.x, placement.at().y + covered.y);
            let (at, to) = cell_bounds(extent, cell);
            list.rect(
                at + Vec2::splat(BORDER_WIDTH),
                to - Vec2::splat(BORDER_WIDTH),
                def.colour(),
            );
            // Over the grid line the cell loop already drew, so the footprint —
            // an L included, because this follows the covered cells rather than
            // a bounding box — is outlined in its own tier.
            if let Some(tier) = tier {
                list.rect_outline(at, to, BORDER_WIDTH, tier_colour(tier));
            }
        }
        let (at, _) = cell_bounds(extent, placement.at());
        list.text(
            at + Vec2::splat(PANEL_PAD * 0.5),
            def.letter().to_string(),
            GLYPH,
            NATURAL_FONT_SIZE,
        );
        if stack.count() > 1 {
            let count = format!("{}", stack.count());
            let corner = Cell::new(
                placement.at().x + shape.width() - 1,
                placement.at().y + shape.height() - 1,
            );
            let (_, end) = cell_bounds(extent, corner);
            list.text(
                Vec2::new(
                    end.x - PANEL_PAD * 0.5 - atlas.text_width(&count, NATURAL_SCALE),
                    end.y - ROW_HEIGHT,
                ),
                count,
                GLYPH,
                NATURAL_FONT_SIZE,
            );
        }
    }

    // The summary: what is held and what it weighs. Both are facts about the
    // grid, so neither moves on a frame the player did nothing on.
    list.text(
        Vec2::new(min.x + PANEL_PAD, max.y - ROW_HEIGHT),
        summary(grid),
        LABEL,
        NATURAL_FONT_SIZE,
    );

    PanelStats {
        commands: list.len(),
        dragged: dropped.and_then(|dropped| Some((dropped.payload, from_drag(dropped.to.origin)?))),
    }
}

/// The summary row: how many stacks the grid holds and what they weigh.
fn summary(grid: &Grid) -> String {
    format!("{} items {} g", grid.len(), loot::weight_g(grid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::inventory::{Rotation, Stack};
    use crcbl::ui::draw_list::DrawCommand;

    /// The extent every sample's headless ring opens at.
    const EXTENT: (u32, u32) = (960, 720);

    /// The seed the published page runs, so what these tests draw is what a
    /// visitor sees.
    const SEED: u32 = loot::DEFAULT_SEED;

    /// A grid holding the plate at the top-left and a stack of bandages beside
    /// it — a `2×2` and a `1×1`, which is what the drawing has to handle.
    fn packed() -> Grid {
        let catalog = loot::catalog();
        let mut grid = loot::carried();
        grid.place(
            catalog,
            Stack::new(
                catalog.id_of("warden plate").expect("a plate"),
                loot::stack_id(0),
                1,
            ),
            Cell::new(0, 0),
            Rotation::Deg0,
        )
        .expect("an empty grid takes a 2x2");
        grid.place(
            catalog,
            Stack::new(
                catalog.id_of("bandage").expect("a bandage"),
                loot::stack_id(1),
                3,
            ),
            Cell::new(3, 0),
            Rotation::Deg0,
        )
        .expect("and a 1x1 beside it");
        grid
    }

    /// Every `Text` command in a list.
    fn text(list: &DrawList) -> Vec<String> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **Every cell is inside the panel, and every point in a cell hit-tests
    /// back to it.** The two halves of one convention: a drag aims with
    /// [`cell_at`] and the frame draws with [`cell_bounds`], and a panel whose
    /// two disagreed would move the wrong item on every drag.
    #[test]
    fn the_hit_test_and_the_drawing_agree_about_where_a_cell_is() {
        let (min, max) = bounds(EXTENT);
        for y in 0..loot::GRID_H {
            for x in 0..loot::GRID_W {
                let cell = Cell::new(x, y);
                let (at, to) = cell_bounds(EXTENT, cell);
                assert!(
                    at.x >= min.x && at.y >= min.y && to.x <= max.x && to.y <= max.y,
                    "{cell:?} is drawn outside the panel: {at:?}..{to:?} of {min:?}..{max:?}",
                );
                let centre = (at + to) * 0.5;
                assert_eq!(
                    cell_at(EXTENT, centre),
                    Some(cell),
                    "the centre of {cell:?}"
                );
                assert_eq!(
                    cell_at(EXTENT, at + Vec2::splat(0.5)),
                    Some(cell),
                    "the top-left corner of {cell:?}",
                );
                let grid = cells(EXTENT);
                assert_eq!(
                    grid.cell_of(grid.id_of(to_drag(cell))).and_then(from_drag),
                    Some(cell),
                    "the id of {cell:?}",
                );
            }
        }
        // …and a point outside is nobody's, rather than the nearest cell's.
        assert_eq!(cell_at(EXTENT, min), None, "the panel's own corner");
        assert_eq!(
            cell_at(EXTENT, Vec2::ZERO),
            None,
            "the top-left of the screen"
        );
        assert_eq!(cell_at(EXTENT, max), None, "the bottom-right of the panel");
    }

    /// **The panel draws what the grid holds**, and a bigger item covers more
    /// cells than a smaller one.
    #[test]
    fn the_panel_draws_a_cell_for_every_cell_an_item_covers() {
        let atlas = FontAtlas::built_in();
        let mut ui = PanelState::new();
        let mut empty = DrawList::new();
        let bare = draw(
            &mut empty,
            &atlas,
            EXTENT,
            &loot::carried(),
            SEED,
            &mut ui,
            PointerInput::default(),
        );
        assert!(bare.commands > 0, "the panel drew nothing at all");

        let mut list = DrawList::new();
        let full = draw(
            &mut list,
            &atlas,
            EXTENT,
            &packed(),
            SEED,
            &mut ui,
            PointerInput::default(),
        );
        // Four cells of plate, one of bandage, a letter each and the count.
        assert!(
            full.commands > bare.commands + 5,
            "a grid holding two items drew {} commands against an empty one's {}",
            full.commands,
            bare.commands,
        );
        let drawn = text(&list);
        assert!(drawn.contains(&TITLE.to_string()), "no title: {drawn:?}");
        assert!(drawn.contains(&"p".to_string()), "no plate: {drawn:?}");
        assert!(drawn.contains(&"3".to_string()), "no count: {drawn:?}");
        assert!(drawn.contains(&summary(&packed())), "no summary: {drawn:?}",);
    }

    /// **Nothing on this panel moves when only the clock does.** The half of
    /// [`crate::page`]'s still-frame rule that this module owes: with the
    /// character standing still and the pointer where it was, two draws are the
    /// same commands.
    #[test]
    fn the_panel_is_identical_between_two_frames_nothing_happened_in() {
        let atlas = FontAtlas::built_in();
        let grid = packed();
        let commands = || {
            let mut ui = PanelState::new();
            let mut list = DrawList::new();
            draw(
                &mut list,
                &atlas,
                EXTENT,
                &grid,
                SEED,
                &mut ui,
                PointerInput::hovering(Vec2::new(4.0, 4.0)),
            );
            format!("{:?}", list.commands())
        };
        assert_eq!(
            commands(),
            commands(),
            "the panel draws something that ticks"
        );
    }

    /// **Every cell a stack covers is outlined in the tier the seed rolled for
    /// it, and a seed that rolls another tier draws another colour.**
    ///
    /// The second half is the control and it is the whole check: without it, a
    /// build that outlined everything in one constant would pass "the tier
    /// colour is on the panel" for every stack on every seed. The colours are
    /// looked up through [`tier_colour`] rather than written out here, so this
    /// asserts the *correspondence* — that the cells carrying a stack carry its
    /// own tier — rather than restating a palette.
    #[test]
    fn a_stack_is_outlined_in_the_tier_its_seed_rolled_for_it() {
        let atlas = FontAtlas::built_in();
        let grid = packed();
        let outlines = |seed: u32| {
            let mut ui = PanelState::new();
            let mut list = DrawList::new();
            draw(
                &mut list,
                &atlas,
                EXTENT,
                &grid,
                seed,
                &mut ui,
                PointerInput::default(),
            );
            let found: Vec<(Vec2, [f32; 4])> = list
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::RectOutline { min, color, .. } => Some((*min, *color)),
                    _ => None,
                })
                .collect();
            found
        };

        // Where every cell of every placement is drawn, and which tier owns it.
        let footprint = |seed: u32| {
            let catalog = loot::catalog();
            let mut cells: Vec<(Vec2, [f32; 4])> = Vec::new();
            for (_, placement) in grid.slots() {
                let def = catalog.get(placement.stack().item()).expect("an item");
                let foe = loot::foe_of(placement.stack().id(), FOES).expect("a foe minted it");
                let colour = tier_colour(loot::rarity_of(seed, foe));
                for covered in def.shape().rotated(placement.rotation()).cells() {
                    let cell =
                        Cell::new(placement.at().x + covered.x, placement.at().y + covered.y);
                    cells.push((cell_bounds(EXTENT, cell).0, colour));
                }
            }
            cells
        };

        let drawn = outlines(SEED);
        for (at, colour) in footprint(SEED) {
            assert!(
                drawn.contains(&(at, colour)),
                "no {colour:?} outline at {at:?}: {drawn:?}",
            );
        }

        // The control: a seed that rolls a different tier for the husk's drop
        // draws a different colour on the same cells.
        let other = (0..4096u32)
            .find(|seed| loot::rarity_of(*seed, 0) != loot::rarity_of(SEED, 0))
            .expect("some seed rolls the husk's drop another tier");
        let elsewhere = outlines(other);
        let moved = footprint(other)
            .into_iter()
            .filter(|entry| !drawn.contains(entry))
            .count();
        assert!(
            moved > 0,
            "the tier outline is the same on every seed, so it is not a tier",
        );
        for entry in footprint(other) {
            assert!(
                elsewhere.contains(&entry),
                "seed {other} did not outline {entry:?}",
            );
        }
    }

    /// Presses over `from`, drags to `to` and lets go there, one frame each;
    /// answers the frame the pointer was held over `to` and the release.
    fn drag_across(
        grid: &Grid,
        state: &mut PanelState,
        from: Cell,
        to: Cell,
    ) -> ((PanelStats, DrawList), PanelStats) {
        let atlas = FontAtlas::built_in();
        let centre = |cell: Cell| {
            let (at, to) = cell_bounds(EXTENT, cell);
            (at + to) * 0.5
        };
        let mut frame = |pos: Vec2, down: bool| {
            let mut list = DrawList::new();
            let stats = draw(
                &mut list,
                &atlas,
                EXTENT,
                grid,
                SEED,
                state,
                PointerInput {
                    pos,
                    down,
                    released: !down,
                },
            );
            (stats, list)
        };
        let (pressed, _) = frame(centre(from), true);
        assert_eq!(pressed.dragged, None, "a press alone moved something");
        let held = frame(centre(to), true);
        assert_eq!(
            held.0.dragged, None,
            "a drag moved something before release"
        );
        let (done, _) = frame(centre(to), false);
        (held, done)
    }

    /// The fill `cell` was drawn with.
    fn fill(list: &DrawList, cell: Cell) -> [f32; 4] {
        let (at, _) = cell_bounds(EXTENT, cell);
        list.commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Rect { min, color, .. } if *min == at => Some(*color),
                _ => None,
            })
            .expect("every cell is filled")
    }

    /// **A press on one cell and a release on another is a drag; a press and a
    /// release on the same cell is not.**
    ///
    /// The second half is the control, and it is what the capture is for: a
    /// panel that reported the cell under the pointer at release would call
    /// every click a drag onto itself, and `Grid::move_within` would then be
    /// asked to move every item onto its own cell on every click.
    #[test]
    fn a_press_on_one_cell_and_a_release_on_another_is_a_drag() {
        let grid = packed();
        let plate = grid.at(Cell::new(0, 0)).expect("the plate");
        let mut state = PanelState::new();

        let from = Cell::new(0, 0);
        let to = Cell::new(1, 2);
        let ((_, held), done) = drag_across(&grid, &mut state, from, to);
        assert_eq!(
            fill(&held, to),
            CELL_LIT,
            "a free cell was not lit as a target"
        );
        assert_eq!(done.dragged, Some((plate, to)), "the drag was not reported");
        assert_eq!(state.ui.active(), None, "the capture outlived the press");

        // The control: a press and a release over one cell is a click, and this
        // panel has nothing for a click to do.
        let (_, clicked) = drag_across(&grid, &mut state, from, from);
        assert_eq!(clicked.dragged, None, "a click was reported as a drag");
    }

    /// **A stack lands where the hand let go, whatever part of it the hand
    /// took hold of.** The plate grabbed by its bottom-right cell and let go one
    /// cell down and right moves its origin by one — a panel that put the
    /// *origin* under the pointer would land it on `(2, 2)`.
    #[test]
    fn a_drag_keeps_the_grip_it_started_with() {
        let grid = packed();
        let plate = grid.at(Cell::new(0, 0)).expect("the plate");
        let mut state = PanelState::new();
        let (_, done) = drag_across(&grid, &mut state, Cell::new(1, 1), Cell::new(2, 2));
        assert_eq!(done.dragged, Some((plate, Cell::new(1, 1))));
    }

    /// **A cell the grid would not take the stack in is drawn refusing, and
    /// letting go there moves nothing.** The plate over the bandage's column
    /// runs off the right edge; the control is the free cell of the first
    /// test, drawn lit rather than refusing.
    #[test]
    fn a_refused_cell_is_drawn_refusing_and_drops_nothing() {
        let grid = packed();
        let mut state = PanelState::new();
        let refused = Cell::new(3, 1);
        let ((_, held), done) = drag_across(&grid, &mut state, Cell::new(0, 0), refused);
        assert_eq!(fill(&held, refused), CELL_REFUSED);
        assert_eq!(done.dragged, None, "a refused drop was reported");
    }
}
