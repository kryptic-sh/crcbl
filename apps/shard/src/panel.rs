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
//!  │                │   why the last drop was refused, if it was
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
//! capture it rides on, the grab offset, the release that ended where it began
//! filtered out, the pad's and the keyboard's carry cursor, a finger's long
//! press, and the ghost. What this module supplies is what only the game knows
//! — what a cell holds when a hand takes hold of it (the [`SlotId`] of the
//! placement covering it, gripped at that placement's origin) and whether the
//! grid would take it under the hand ([`Grid::can_move_within`]). The answer
//! comes back as [`DropFeedback`], which is what a refusing cell and the
//! ghost's tint are drawn from.
//!
//! # Every hand, one move
//!
//! The pointer drags; the pad and the keyboard pick up with `ui_accept` on the
//! focused cell, carry it with `ui_move` and drop it with `ui_accept`, or take
//! it back with `ui_back`; a finger held still on a stack for
//! [`LONG_PRESS`](crcbl::ui::grid_drag::LONG_PRESS) lifts it, and lifting the
//! finger drops it. Each comes back as one [`PanelStats::dragged`] for
//! `crate::app` to hand to `Game::drag`, which decides; the panel never changes
//! the grid. A drop the panel refused, and one the grid refused, leave it as it
//! was and put the reason under the summary.
//!
//! **There is one grid, so nothing is [linked](GridDrag::link) and there is no
//! quick action.** The floor is not a container — a stack lying there is taken
//! by walking to it, through `Controls::pickup` — so there is no second grid
//! for a step off the edge to land in or a "send" to send to.
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

use crcbl::core::input::{ContactId, TouchPhase};
use crcbl::inventory::{Cell, Grid, InventoryError, Placement, SlotId};
use crcbl::math::{UVec2, Vec2};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::grid_drag::{CellGrid, DragInput, DropFeedback, Ghost, GridDrag, Grip};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::{ButtonState, NATURAL_FONT_SIZE, UiState, WidgetId};

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
/// A cell a drag held over it would not land in, and the ghost over one.
const CELL_REFUSED: [f32; 4] = [0.30, 0.09, 0.07, 1.0];
/// The outline of the cell the pad and the keyboard are on.
pub(crate) const FOCUS: [f32; 4] = [0.92, 0.84, 0.62, 1.0];
/// How thick that outline is, in pixels: thicker than a cell's own, so it
/// reads over an item's colour and its tier.
const FOCUS_WIDTH: f32 = 2.0;
/// How opaque the ghost is: enough to read its colour, little enough to see
/// the cells it is over.
pub(crate) const GHOST_ALPHA: f32 = 0.6;
/// The title and the summary line.
const LABEL: [f32; 4] = [0.72, 0.66, 0.58, 1.0];
/// Why the last drop was refused.
const REFUSAL: [f32; 4] = [0.90, 0.48, 0.36, 1.0];
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
/// The height of the title row, the summary row and the refusal row, in
/// pixels.
const ROW_HEIGHT: f32 = 18.0;
/// How many of those rows the panel has.
const ROWS: f32 = 3.0;
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

/// What one frame of the panel drew, and what the player asked of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelStats {
    /// How many draw commands it produced.
    pub commands: usize,
    /// The drag that just ended over a cell that takes it, from whichever hand
    /// carried it: the stack it took hold of and the cell that stack's origin
    /// lands on, grab offset applied. `None` on every frame but the one a drag
    /// ends on, on a release that ended where it began — which is a click, and
    /// this panel has nothing for a click to do — on a cancel, and on a
    /// release over a cell the grid would not take it in.
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
    let height = 2.0f32.mul_add(PANEL_PAD, f32::from(loot::GRID_H) * CELL_PX) + ROWS * ROW_HEIGHT;
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
/// the rotation it already has, and why not.
///
/// [`Grid::can_move_within`] does not count the stack's own cells as
/// occupied, so a one-cell nudge of the `2×2` is a move rather than a
/// collision with itself.
fn fits(grid: &Grid, slot: SlotId, at: Cell) -> Result<(), InventoryError> {
    let placement = grid.slot(slot).ok_or(InventoryError::NoSuchSlot(slot))?;
    grid.can_move_within(loot::catalog(), slot, at, placement.rotation())
}

/// What the panel keeps between frames: which cell owns the pointer press, the
/// drag riding on it — which stack it took hold of, and where — the pad's
/// focus, the finger being followed, and why the last drop was refused.
#[derive(Debug, Default)]
pub struct PanelState {
    ui: UiState,
    drag: GridDrag<SlotId>,
    refusal: Option<InventoryError>,
}

impl PanelState {
    /// Nothing pressed, nothing held, nothing refused.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the press and the drag with it — for a panel torn down mid-press,
    /// which is what [`UiState::clear`] and [`GridDrag::cancel`] are for. The
    /// pad's focus is kept, so the panel opens again where it was left.
    pub fn clear(&mut self) {
        self.ui.clear();
        self.drag.cancel();
    }

    /// Offers one contact event, in framebuffer pixels, to the drag's long
    /// press — see [`GridDrag::touch`]. Answers whether the drag is following
    /// that finger.
    pub fn touch(&mut self, contact: ContactId, phase: TouchPhase, at: Vec2) -> bool {
        self.drag.touch(contact, phase, at)
    }

    /// What became of the drop the panel last asked for: a refusal is shown
    /// under the summary until a later drop lands.
    pub fn note(&mut self, outcome: Result<(), InventoryError>) {
        self.refusal = outcome.err();
    }

    /// Why the last drop was refused, if it was.
    #[must_use]
    pub const fn refusal(&self) -> Option<InventoryError> {
        self.refusal
    }
}

/// Draws the panel and resolves this frame's input against it.
///
/// `state` is what a drag cannot do without, and it is the caller's because it
/// has to survive between frames. `seed` is the run's loot seed, which is what
/// a stack's tier is rolled from — see the module docs. `input` is every hand
/// but a finger, whose contacts reach `state` between frames through
/// [`PanelState::touch`].
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    grid: &Grid,
    seed: u32,
    state: &mut PanelState,
    input: DragInput,
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
    let mut frame = state.drag.frame_with(&mut state.ui, input);
    let response = frame.grid(
        &cells,
        |cell| {
            let slot = grid.at(from_drag(cell)?)?;
            Some(Grip {
                payload: slot,
                origin: to_drag(grid.slot(slot)?.at()),
            })
        },
        |slot, target| from_drag(target.origin).is_some_and(|at| fits(grid, *slot, at).is_ok()),
    );
    let ghost = frame.ghost();
    let ended = frame.release();

    let mut dragged = None;
    if let Some(ended) = ended
        && let Some(target) = ended.target
        && let Some(at) = from_drag(target.origin)
    {
        if ended.accepted {
            dragged = Some((ended.payload, at));
        } else if let Err(why) = fits(grid, ended.payload, at) {
            state.refusal = Some(why);
        }
    }

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

    // After the items, unlike the fills: an item's colour and its tier outline
    // cover the whole cell, and a focus drawn under them would be hidden on
    // exactly the cells worth picking up.
    for cell in cells.cells() {
        if response.cell(cell).focused {
            let (at, to) = cells.cell_bounds(cell);
            list.rect_outline(at, to, FOCUS_WIDTH, FOCUS);
        }
    }

    let carried = state
        .drag
        .held()
        .and_then(|held| grid.slot(*held.payload()));
    if let (Some(ghost), Some(placement)) = (ghost, carried) {
        draw_ghost(list, ghost, placement);
    }

    // The summary: what is held and what it weighs, under the grid. Both are
    // facts about the grid, so neither moves on a frame the player did nothing
    // on.
    let grid_bottom = cells.origin.y + f32::from(loot::GRID_H) * CELL_PX;
    list.text(
        Vec2::new(min.x + PANEL_PAD, grid_bottom),
        summary(grid),
        LABEL,
        NATURAL_FONT_SIZE,
    );
    if let Some(refusal) = state.refusal {
        list.text(
            Vec2::new(min.x + PANEL_PAD, max.y - PANEL_PAD - ROW_HEIGHT),
            refusal.to_string(),
            REFUSAL,
            NATURAL_FONT_SIZE,
        );
    }

    PanelStats {
        commands: list.len(),
        dragged,
    }
}

/// The carried stack's footprint where `ghost` says, in its own colour — or in
/// [`CELL_REFUSED`] over a cell that would not take it — at [`GHOST_ALPHA`].
fn draw_ghost(list: &mut DrawList, ghost: Ghost, placement: Placement) {
    let Some(def) = loot::catalog().get(placement.stack().item()) else {
        return;
    };
    let [r, g, b, _] = match ghost.drop {
        DropFeedback::Refusing => CELL_REFUSED,
        DropFeedback::Accepting | DropFeedback::None => def.colour(),
    };
    let colour = [r, g, b, GHOST_ALPHA];
    for covered in def.shape().rotated(placement.rotation()).cells() {
        let at = ghost.origin + Vec2::new(f32::from(covered.x), f32::from(covered.y)) * ghost.cell;
        list.rect(at, at + ghost.cell, colour);
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
    use crcbl::ui::grid_drag::LONG_PRESS;
    use crcbl::ui::tree::{Direction, NavInput};
    use crcbl::ui::widget::PointerInput;
    use std::time::Duration;

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

    /// The frame's input from the pointer alone.
    fn pointing(pointer: PointerInput) -> DragInput {
        DragInput {
            pointer,
            ..DragInput::default()
        }
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
            DragInput::default(),
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
            DragInput::default(),
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
                pointing(PointerInput::hovering(Vec2::new(4.0, 4.0))),
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
                DragInput::default(),
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

    /// One frame of `grid` under `input`.
    fn frame_of(grid: &Grid, state: &mut PanelState, input: DragInput) -> (PanelStats, DrawList) {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(&mut list, &atlas, EXTENT, grid, SEED, state, input);
        (stats, list)
    }

    /// The middle of `cell`.
    fn centre(cell: Cell) -> Vec2 {
        let (at, to) = cell_bounds(EXTENT, cell);
        (at + to) * 0.5
    }

    /// The pointer at `pos`, held or let go.
    fn pointer(pos: Vec2, down: bool) -> DragInput {
        pointing(PointerInput {
            pos,
            down,
            released: !down,
            secondary_pressed: false,
        })
    }

    /// The frame's input from the pad or the keyboard alone.
    fn pad(nav: NavInput) -> DragInput {
        DragInput {
            nav,
            ..DragInput::default()
        }
    }

    /// Presses over `from`, drags to `to` and lets go there, one frame each;
    /// answers the frame the pointer was held over `to` and the release.
    fn drag_across(
        grid: &Grid,
        state: &mut PanelState,
        from: Cell,
        to: Cell,
    ) -> ((PanelStats, DrawList), (PanelStats, DrawList)) {
        let (pressed, _) = frame_of(grid, state, pointer(centre(from), true));
        assert_eq!(pressed.dragged, None, "a press alone moved something");
        let held = frame_of(grid, state, pointer(centre(to), true));
        assert_eq!(
            held.0.dragged, None,
            "a drag moved something before release"
        );
        let done = frame_of(grid, state, pointer(centre(to), false));
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
        let ((_, held), (done, _)) = drag_across(&grid, &mut state, from, to);
        assert_eq!(
            fill(&held, to),
            CELL_LIT,
            "a free cell was not lit as a target"
        );
        assert_eq!(done.dragged, Some((plate, to)), "the drag was not reported");
        assert_eq!(state.ui.active(), None, "the capture outlived the press");

        // The control: a press and a release over one cell is a click, and this
        // panel has nothing for a click to do.
        let (_, (clicked, _)) = drag_across(&grid, &mut state, from, from);
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
        let (_, (done, _)) = drag_across(&grid, &mut state, Cell::new(1, 1), Cell::new(2, 2));
        assert_eq!(done.dragged, Some((plate, Cell::new(1, 1))));
    }

    /// The refusal the plate with its origin on `(3, 1)` earns: a `2×2` there
    /// runs off the right edge.
    const OFF_THE_EDGE: InventoryError = InventoryError::OutOfBounds {
        x: 3,
        y: 1,
        w: 2,
        h: 2,
    };

    /// **A cell the grid would not take the stack in is drawn refusing,
    /// letting go there moves nothing, and the panel says why.** The plate
    /// over the bandage's column runs off the right edge; the control is the
    /// free cell of the first test, drawn lit rather than refusing, and a drop
    /// that lands clearing the reason.
    #[test]
    fn a_refused_cell_is_drawn_refusing_drops_nothing_and_says_why() {
        let grid = packed();
        let mut state = PanelState::new();
        let refused = Cell::new(3, 1);
        let ((_, held), (done, list)) = drag_across(&grid, &mut state, Cell::new(0, 0), refused);
        assert_eq!(fill(&held, refused), CELL_REFUSED);
        assert_eq!(done.dragged, None, "a refused drop was reported");
        assert_eq!(state.refusal(), Some(OFF_THE_EDGE));
        assert!(
            text(&list).contains(&OFF_THE_EDGE.to_string()),
            "the refusal is not on the panel: {:?}",
            text(&list),
        );

        // The control, and what clears it: a drop that lands.
        state.note(Ok(()));
        let (_, list) = frame_of(&grid, &mut state, DragInput::default());
        assert!(
            !text(&list).contains(&OFF_THE_EDGE.to_string()),
            "a stale refusal"
        );
    }

    /// **A refusal the grid made is shown as the panel's own is**: `crate::app`
    /// notes what `Game::drag` answered, and the reason is drawn.
    #[test]
    fn a_refusal_the_grid_made_is_shown() {
        let grid = packed();
        let mut state = PanelState::new();
        let why = InventoryError::Occupied { x: 3, y: 0 };
        let (_, before) = frame_of(&grid, &mut state, DragInput::default());
        assert!(!text(&before).contains(&why.to_string()));
        state.note(Err(why));
        let (_, list) = frame_of(&grid, &mut state, DragInput::default());
        assert!(text(&list).contains(&why.to_string()), "{:?}", text(&list));
    }

    /// Where every [`FOCUS`] outline was drawn.
    fn focus_outlines(list: &DrawList) -> Vec<Vec2> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::RectOutline { min, color, .. } if *color == FOCUS => Some(*min),
                _ => None,
            })
            .collect()
    }

    /// The pad's first step, which only lands focus, and `ui_accept` on the
    /// cell it landed on: the plate, picked up by its origin.
    fn pick_up_the_plate(grid: &Grid, state: &mut PanelState) {
        frame_of(grid, state, DragInput::default());
        let (_, landed) = frame_of(grid, state, pad(NavInput::toward(Direction::Down)));
        assert_eq!(
            focus_outlines(&landed),
            vec![cell_bounds(EXTENT, Cell::new(0, 0)).0],
            "the landing is not outlined on the first cell",
        );
        let (picked, _) = frame_of(grid, state, pad(NavInput::ACCEPT));
        assert_eq!(picked.dragged, None, "the pick-up moved something");
        assert!(state.drag.held().is_some(), "accept picked nothing up");
    }

    /// **The pad picks the plate up, carries it and drops it**: one right and
    /// two down carry the origin to `(1, 2)`, and `ui_accept` there is the same
    /// [`PanelStats::dragged`] a pointer's drop is.
    #[test]
    fn the_pad_picks_a_stack_up_carries_it_and_drops_it() {
        let grid = packed();
        let plate = grid.at(Cell::new(0, 0)).expect("the plate");
        let mut state = PanelState::new();
        pick_up_the_plate(&grid, &mut state);
        frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Right)));
        frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        let (_, held) = frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        let to = Cell::new(1, 2);
        assert_eq!(fill(&held, to), CELL_LIT, "the carry cursor's cell fits");
        let (dropped, _) = frame_of(&grid, &mut state, pad(NavInput::ACCEPT));
        assert_eq!(dropped.dragged, Some((plate, to)));
        assert!(state.drag.held().is_none(), "the drop kept the drag");
    }

    /// **Back takes the pad's drag back and asks for nothing**, though the
    /// cursor is over a cell that would take it.
    #[test]
    fn back_takes_the_pads_drag_back_and_moves_nothing() {
        let grid = packed();
        let mut state = PanelState::new();
        pick_up_the_plate(&grid, &mut state);
        frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        let (_, held) = frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        assert_eq!(
            fill(&held, Cell::new(0, 2)),
            CELL_LIT,
            "the control: it fits"
        );
        let (back, _) = frame_of(&grid, &mut state, pad(NavInput::BACK));
        assert_eq!(back.dragged, None, "back moved something");
        assert_eq!(state.refusal(), None, "back was reported as a refusal");
        assert!(state.drag.held().is_none(), "back kept the drag");
    }

    /// **The pad's drop on a cell that will not take it moves nothing and
    /// says why**, as the pointer's does.
    #[test]
    fn a_pads_refused_drop_says_why() {
        let grid = packed();
        let mut state = PanelState::new();
        pick_up_the_plate(&grid, &mut state);
        for _ in 0..3 {
            frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Right)));
        }
        frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        let (dropped, list) = frame_of(&grid, &mut state, pad(NavInput::ACCEPT));
        assert_eq!(dropped.dragged, None, "a refused drop was reported");
        assert_eq!(state.refusal(), Some(OFF_THE_EDGE));
        assert!(text(&list).contains(&OFF_THE_EDGE.to_string()));
    }

    /// Every filled rectangle drawn in `colour`'s hue at [`GHOST_ALPHA`]: the
    /// ghost's cells.
    fn ghost_cells(list: &DrawList, colour: [f32; 4]) -> Vec<Vec2> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, color, .. }
                    if color[..3] == colour[..3] && color[3] == GHOST_ALPHA =>
                {
                    Some(*min)
                }
                _ => None,
            })
            .collect()
    }

    /// The plate's colour.
    fn plate_colour() -> [f32; 4] {
        let catalog = loot::catalog();
        catalog
            .get(catalog.id_of("warden plate").expect("a plate"))
            .expect("its definition")
            .colour()
    }

    /// The four cells of a `2×2` ghost whose top-left corner is `at`.
    fn square(at: Vec2) -> Vec<Vec2> {
        [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(x, y)| at + Vec2::new(x, y) * CELL_PX)
            .collect()
    }

    /// **The ghost follows the pointer in the carried stack's colour, held
    /// where it was taken hold of, and turns refusing over a cell that will
    /// not take it**; it is gone once the drag ends.
    #[test]
    fn the_ghost_follows_the_pointer_tinted_by_the_cell_under_it() {
        let grid = packed();
        let mut state = PanelState::new();
        frame_of(&grid, &mut state, pointer(centre(Cell::new(1, 1)), true));
        let at = centre(Cell::new(2, 2)) + Vec2::new(3.0, 1.0);
        let (_, held) = frame_of(&grid, &mut state, pointer(at, true));
        let grab = Vec2::splat(1.5) * CELL_PX;
        assert_eq!(
            ghost_cells(&held, plate_colour()),
            square(at - grab),
            "the ghost is not the plate's four cells, held by the bottom-right one",
        );

        let refused = centre(Cell::new(3, 1));
        let (_, over) = frame_of(&grid, &mut state, pointer(refused, true));
        assert_eq!(
            ghost_cells(&over, CELL_REFUSED).len(),
            4,
            "no refusing tint"
        );
        assert!(ghost_cells(&over, plate_colour()).is_empty());

        let (_, ended) = frame_of(&grid, &mut state, pointer(refused, false));
        assert!(
            ghost_cells(&ended, CELL_REFUSED).is_empty(),
            "a ghost outlived the drag"
        );
    }

    /// **The ghost follows the pad's carry cursor**, snapped to the cell it is
    /// on, a step at a time.
    #[test]
    fn the_ghost_follows_the_carry_cursor() {
        let grid = packed();
        let mut state = PanelState::new();
        pick_up_the_plate(&grid, &mut state);
        let (_, right) = frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Right)));
        assert_eq!(
            ghost_cells(&right, plate_colour()),
            square(cell_bounds(EXTENT, Cell::new(1, 0)).0),
        );
        let (_, down) = frame_of(&grid, &mut state, pad(NavInput::toward(Direction::Down)));
        assert_eq!(
            ghost_cells(&down, plate_colour()),
            square(cell_bounds(EXTENT, Cell::new(1, 1)).0),
            "the ghost stayed where the cursor was",
        );
    }

    /// The frame's input with `dt` of time passed and no hand on it but a
    /// finger's.
    fn waited(dt: Duration) -> DragInput {
        DragInput {
            dt,
            ..DragInput::default()
        }
    }

    /// **A finger held still on the plate for [`LONG_PRESS`] lifts it, its
    /// ghost follows the finger, and lifting the finger drops it**; held a
    /// moment less, it has lifted nothing.
    ///
    /// The first half is the control for the threshold: a panel that lifted on
    /// the touch landing would be lifting under every scroll that started on a
    /// stack.
    #[test]
    fn a_long_press_lifts_a_stack_and_lifting_the_finger_drops_it() {
        let grid = packed();
        let plate = grid.at(Cell::new(0, 0)).expect("the plate");
        let mut state = PanelState::new();
        let finger = ContactId(1);
        frame_of(&grid, &mut state, DragInput::default());
        assert!(state.touch(finger, TouchPhase::Began, centre(Cell::new(1, 1))));
        let almost = LONG_PRESS - Duration::from_millis(1);
        frame_of(&grid, &mut state, waited(almost));
        assert!(
            state.drag.held().is_none(),
            "lifted before the long press was up"
        );
        frame_of(&grid, &mut state, waited(LONG_PRESS - almost));
        assert!(state.drag.held().is_some(), "the long press lifted nothing");

        let at = centre(Cell::new(2, 2)) + Vec2::new(2.0, -1.0);
        assert!(state.touch(finger, TouchPhase::Moved, at));
        let (_, carried) = frame_of(&grid, &mut state, DragInput::default());
        assert_eq!(
            ghost_cells(&carried, plate_colour()),
            square(at - Vec2::splat(1.5) * CELL_PX),
            "the ghost does not follow the finger",
        );
        assert!(state.touch(finger, TouchPhase::Ended, at));
        let (dropped, _) = frame_of(&grid, &mut state, DragInput::default());
        assert_eq!(dropped.dragged, Some((plate, Cell::new(1, 1))));
    }
}
