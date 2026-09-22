//! The loadout panel: the rig the player carries, and the drag that rearranges
//! it.
//!
//! ```text
//!  ┌ LOADOUT ───────┐
//!  │ ┌──┬──┬──┬──┐  │
//!  │ │p │p │m │  │  │   p  the sidearm, two cells by one
//!  │ ├──┼──┼──┼──┤  │   m  a magazine, x2
//!  │ │g │  │m │  │  │   g  a frag, x2
//!  │ ├──┼──┼──┼──┤  │
//!  │ │  │  │  │  │  │
//!  │ └──┴──┴──┴──┘  │
//!  │ 3 items 2130 g │
//!  └────────────────┘
//! ```
//!
//! # Closed by default, and nothing on it ticks
//!
//! `I` opens it, and until it is opened this module draws nothing at all. Both
//! halves are load-bearing for the same reason [`crate::page`]'s are:
//! `web/tools/browser-e2e.mjs` asserts things about a canvas nobody has asked
//! for a panel on, so a panel that opened itself — or that drew a clock, a count
//! of frames or a flicker — would make those checks impossible to pass on a
//! working build. Everything drawn here is a fact about what the player is
//! carrying, which changes when the *player* does something and holds still
//! otherwise.
//!
//! # Opening it gives the pointer back
//!
//! Breach holds the pointer while it is being played — [`crate::app`]'s
//! `pointer_mode` asks for [`PointerMode::Locked`](crcbl::shell::PointerMode) —
//! and a locked pointer has no position to hit-test a cell with. So the panel
//! is the second state, beside the pause menu, in which this sample asks for
//! [`PointerMode::Free`](crcbl::shell::PointerMode), and while it is open the
//! mouse neither turns the view nor pulls the trigger. That is not a
//! concession to the panel: a click on a rig is a click on a rig.
//!
//! # The drag is the engine's; the payload and the rule are breach's
//!
//! [`crcbl::ui::grid_drag`] is the drag: the per-cell hit test, the press
//! capture it rides on, the grab offset, and the release that ended where it
//! began filtered out. It was hoisted out of this module and
//! `apps/shard/src/panel.rs`, which had each written it for themselves. What
//! this module supplies is what only the game knows — what a cell holds when a
//! press lands on it (the [`SlotId`] of the placement covering it, gripped at
//! that placement's origin) and whether the rig would take it where the
//! pointer is (a trial [`Grid::move_within`] on a copy). The answer comes back
//! as [`DropFeedback`], which is what a refusing cell is drawn from.
//!
//! # There are no icons
//!
//! A cell is the item's [`colour`](crcbl::inventory::ItemDef::colour) with its
//! [`letter`](crcbl::inventory::ItemDef::letter) on it. `crcbl icon bake` is the
//! plan's answer and it is not a verb yet, and
//! `docs/plan/sample/11-breach.md`'s rule 11 is where the `.crpix` sheets that
//! replace them are owed.

use crcbl::inventory::{Cell, Grid, SlotId};
use crcbl::math::{UVec2, Vec2};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::grid_drag::{CellGrid, DropFeedback, GridDrag, Grip};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::widget::{ButtonState, NATURAL_FONT_SIZE, PointerInput, UiState, WidgetId};

use crate::loadout;

/// The panel's own background, a shade darker and more opaque than
/// [`crate::page`]'s so the two read as different surfaces when they overlap.
const PANEL_BG: [f32; 4] = [0.04, 0.05, 0.08, 0.94];
/// The panel's border, and the grid's lines — [`crate::page`]'s border, so the
/// two panels are the same furniture.
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
/// An empty cell.
const CELL_BG: [f32; 4] = [0.09, 0.10, 0.14, 1.0];
/// A cell the pointer is over, one it is dragging from, or one a drag held
/// over it would land in.
const CELL_LIT: [f32; 4] = [0.18, 0.20, 0.26, 1.0];
/// A cell a drag held over it would not land in.
const CELL_REFUSED: [f32; 4] = [0.32, 0.10, 0.10, 1.0];
/// The title and the summary line.
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
/// A letter or a count drawn over an item's own colour.
const GLYPH: [f32; 4] = [0.05, 0.06, 0.08, 1.0];

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
const TITLE: &str = "LOADOUT";

/// The first widget id the cells use.
///
/// Above [`crcbl::engine::FIRST_GAME_ID`] because these are the game's own
/// widgets, and far above it because the pause menu's ids live near it — a cell
/// and a menu row that shared a number would share a press capture.
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
    /// and on a release over a cell the rig would not take it in.
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
    let width = 2.0f32.mul_add(PANEL_PAD, f32::from(loadout::GRID_W) * CELL_PX);
    let height = 2.0f32.mul_add(PANEL_PAD, f32::from(loadout::GRID_H) * CELL_PX) + 2.0 * ROW_HEIGHT;
    let min = Vec2::new(
        (extent.0 as f32 - width) * 0.5,
        (extent.1 as f32 - height) * 0.5,
    );
    (min, min + Vec2::new(width, height))
}

/// The rig's cells on a surface of `extent`: where cell `(0, 0)`'s top-left
/// corner is, how big a cell is, and the widget ids they answer to.
fn cells(extent: (u32, u32)) -> CellGrid {
    let (min, _) = bounds(extent);
    CellGrid {
        origin: Vec2::new(min.x + PANEL_PAD, min.y + PANEL_PAD + ROW_HEIGHT),
        cell: CELL_PX,
        columns: u32::from(loadout::GRID_W),
        rows: u32::from(loadout::GRID_H),
        id_base: CELL_ID_BASE,
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

/// Which cell `pos` is over, or `None` for a point outside the rig.
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
/// Asked of a copy, because [`Grid::move_within`] is the one check that does
/// not count the stack's own cells as occupied — so a one-cell nudge of the
/// `2×1` sidearm is a move rather than a collision with itself — and it only
/// exists as the move.
fn accepts(grid: &Grid, slot: SlotId, at: Cell) -> bool {
    let Some(placement) = grid.slot(slot) else {
        return false;
    };
    grid.clone()
        .move_within(loadout::catalog(), slot, at, placement.rotation())
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
/// has to survive between frames.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    grid: &Grid,
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

    let catalog = loadout::catalog();
    for (_, placement) in grid.slots() {
        let stack = placement.stack();
        let Some(def) = catalog.get(stack.item()) else {
            continue;
        };
        let shape = def.shape().rotated(placement.rotation());
        for covered in shape.cells() {
            let cell = Cell::new(placement.at().x + covered.x, placement.at().y + covered.y);
            let (at, to) = cell_bounds(extent, cell);
            list.rect(
                at + Vec2::splat(BORDER_WIDTH),
                to - Vec2::splat(BORDER_WIDTH),
                def.colour(),
            );
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

/// The summary row: how many stacks the rig holds and what they weigh, in the
/// one spelling [`loadout::summary`] owns.
fn summary(grid: &Grid) -> String {
    loadout::summary(grid.len(), loadout::weight_g(grid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::draw_list::DrawCommand;

    /// The extent every sample's headless ring opens at.
    const EXTENT: (u32, u32) = (960, 720);

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
        for y in 0..loadout::GRID_H {
            for x in 0..loadout::GRID_W {
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

    /// **The panel draws what the rig holds**, and a bigger item covers more
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
            &loadout::empty(),
            &mut ui,
            PointerInput::default(),
        );
        assert!(bare.commands > 0, "the panel drew nothing at all");

        let mut list = DrawList::new();
        let full = draw(
            &mut list,
            &atlas,
            EXTENT,
            &loadout::packed(),
            &mut ui,
            PointerInput::default(),
        );
        // Every cell the kit covers, a letter for each stack, and a count for
        // each stack of more than one.
        assert!(
            full.commands > bare.commands + 5,
            "a rig holding the starting kit drew {} commands against an empty one's {}",
            full.commands,
            bare.commands,
        );
        let drawn = text(&list);
        assert!(drawn.contains(&TITLE.to_string()), "no title: {drawn:?}");
        assert!(drawn.contains(&"p".to_string()), "no sidearm: {drawn:?}");
        assert!(drawn.contains(&"2".to_string()), "no count: {drawn:?}");
        assert!(
            drawn.contains(&summary(&loadout::packed())),
            "no summary: {drawn:?}",
        );
    }

    /// **Nothing on this panel moves when only the clock does.** The half of
    /// [`crate::page`]'s still-frame rule that this module owes: with the
    /// pointer where it was and nothing dragged, two draws are the same
    /// commands.
    #[test]
    fn the_panel_is_identical_between_two_frames_nothing_happened_in() {
        let atlas = FontAtlas::built_in();
        let grid = loadout::packed();
        let commands = || {
            let mut ui = PanelState::new();
            let mut list = DrawList::new();
            draw(
                &mut list,
                &atlas,
                EXTENT,
                &grid,
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
        let grid = loadout::packed();
        let sidearm = grid.at(Cell::new(0, 0)).expect("the sidearm");
        let mut state = PanelState::new();

        let from = Cell::new(0, 0);
        let to = Cell::new(1, 2);
        let ((_, held), done) = drag_across(&grid, &mut state, from, to);
        assert_eq!(
            fill(&held, to),
            CELL_LIT,
            "a free cell was not lit as a target"
        );
        assert_eq!(
            done.dragged,
            Some((sidearm, to)),
            "the drag was not reported"
        );
        assert_eq!(state.ui.active(), None, "the capture outlived the press");

        // The control: a press and a release over one cell is a click, and this
        // panel has nothing for a click to do.
        let (_, clicked) = drag_across(&grid, &mut state, from, from);
        assert_eq!(clicked.dragged, None, "a click was reported as a drag");
    }

    /// **A stack lands where the hand let go, whatever part of it the hand
    /// took hold of.** The `2×1` sidearm grabbed by its right-hand cell and let
    /// go one cell right of that moves its origin by one — a panel that put the
    /// *origin* under the pointer would land it on `(2, 2)`.
    #[test]
    fn a_drag_keeps_the_grip_it_started_with() {
        let grid = loadout::packed();
        let sidearm = grid.at(Cell::new(0, 0)).expect("the sidearm");
        let mut state = PanelState::new();
        let (_, done) = drag_across(&grid, &mut state, Cell::new(1, 0), Cell::new(2, 2));
        assert_eq!(done.dragged, Some((sidearm, Cell::new(1, 2))));
    }

    /// **A cell the rig would not take the stack in is drawn refusing, and
    /// letting go there moves nothing.** The sidearm with its origin on the
    /// last column runs off the right edge; the control is the free cell of the
    /// first test, drawn lit rather than refusing.
    #[test]
    fn a_refused_cell_is_drawn_refusing_and_drops_nothing() {
        let grid = loadout::packed();
        let mut state = PanelState::new();
        let refused = Cell::new(3, 2);
        let ((_, held), done) = drag_across(&grid, &mut state, Cell::new(0, 0), refused);
        assert_eq!(fill(&held, refused), CELL_REFUSED);
        assert_eq!(done.dragged, None, "a refused drop was reported");
    }
}
