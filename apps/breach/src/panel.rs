//! The loadout panel: the rig and the pack the player carries, and the drag
//! that rearranges them.
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
//!  │ 3 items 2130 g │   the rig's summary: what the trigger can reach
//!  │ PACK           │
//!  │ ┌──┬──┬──┬──┐  │
//!  │ │  │  │  │  │  │   empty at spawn
//!  │ ├──┼──┼──┼──┤  │
//!  │ │  │  │  │  │  │
//!  │ └──┴──┴──┴──┘  │
//!  │                │   why the last move was refused, if it was
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
//! capture it rides on, the grab offset, the release that ended where it began
//! filtered out, the pad's and the keyboard's carry cursor, and the ghost. It
//! was hoisted out of this module and `apps/shard/src/panel.rs`, which had each
//! written it for themselves. What this module supplies is what only the game
//! knows — what a cell holds when a hand takes hold of it (the [`SlotId`] of
//! the placement covering it, gripped at that placement's origin) and whether
//! the container under the hand would take it there ([`Grid::can_move_within`]
//! in the same container, [`Grid::can_place`] in the other). The answer comes
//! back as [`DropFeedback`], which is what a refusing cell and the ghost's tint
//! are drawn from.
//!
//! # Every hand, one move
//!
//! The pointer drags; the pad and the keyboard pick up with `ui_accept` on the
//! focused cell, carry it with `ui_move` — down off the rig's last row is the
//! pack's first, as the two are [linked](GridDrag::link) — and drop it with
//! `ui_accept`, or take it back with `ui_back`. [`SEND`] is the quick action:
//! the focused stack to the other container without carrying it. Each comes
//! back as one [`PanelMove`] for `crate::app` to apply through the kit's
//! commands, which decide; the panel never changes a grid. A drop the panel
//! itself refused, and a move the kit refused, leave the containers as they
//! were and put the reason under the pack.
//!
//! **A finger is not wired here.** Breach is played with a mouse and a
//! keyboard, and a finger's primary contact reaches it as the pointer, which
//! drags on press; [`GridDrag::touch`]'s long press is for a game that offers
//! contacts itself.
//!
//! # There are no icons
//!
//! A cell is the item's [`colour`](crcbl::inventory::ItemDef::colour) with its
//! [`letter`](crcbl::inventory::ItemDef::letter) on it. `crcbl icon bake` is the
//! plan's answer and it is not a verb yet, and
//! `docs/plan/sample/11-breach.md`'s rule 11 is where the `.crpix` sheets that
//! replace them are owed.

use crcbl::inventory::{
    Applied, Cell, ContainerId, Grid, InventoryError, Placement, Refusal, SlotId,
};
use crcbl::math::{UVec2, Vec2};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::grid_drag::{
    CellGrid, DragInput, DropFeedback, Ghost, GridDrag, GridResponse, Grip, QuickAction,
};
use crcbl::ui::text::FontAtlas;
use crcbl::ui::tree::Direction;
use crcbl::ui::widget::{ButtonState, NATURAL_FONT_SIZE, UiState, WidgetId};

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
/// A cell a drag held over it would not land in, and the ghost over one.
const CELL_REFUSED: [f32; 4] = [0.32, 0.10, 0.10, 1.0];
/// The outline of the cell the pad and the keyboard are on.
pub(crate) const FOCUS: [f32; 4] = [0.85, 0.80, 0.45, 1.0];
/// How thick that outline is, in pixels: thicker than a cell's own, so it
/// reads over an item's colour.
const FOCUS_WIDTH: f32 = 2.0;
/// How opaque the ghost is: enough to read its colour, little enough to see
/// the cells it is over.
const GHOST_ALPHA: f32 = 0.6;
/// The title, the summary line and the pack's label.
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
/// Why the last move was refused.
const REFUSAL: [f32; 4] = [0.90, 0.45, 0.40, 1.0];
/// A letter or a count drawn over an item's own colour.
const GLYPH: [f32; 4] = [0.05, 0.06, 0.08, 1.0];

/// How wide one cell is, in pixels.
const CELL_PX: f32 = 34.0;
/// The panel's padding inside its own border, in pixels.
const PANEL_PAD: f32 = 8.0;
/// The height of the title row, the summary row, the pack's label row and the
/// refusal row, in pixels.
const ROW_HEIGHT: f32 = 18.0;
/// How many of those rows the panel has.
const ROWS: f32 = 4.0;
/// How thick the panel's border and the cell outlines are, in pixels.
const BORDER_WIDTH: f32 = 1.0;
/// The scale [`FontAtlas::text_width`] is measured at — the natural size, as
/// [`crate::page`] draws at.
const NATURAL_SCALE: f32 = 1.0;

/// The title.
const TITLE: &str = "LOADOUT";
/// The pack's label.
const PACK_LABEL: &str = "PACK";

/// The first widget id the rig's cells use.
///
/// Above [`crcbl::engine::FIRST_GAME_ID`] because these are the game's own
/// widgets, and far above it because the pause menu's ids live near it — a cell
/// and a menu row that shared a number would share a press capture.
const CELL_ID_BASE: WidgetId = 0x5_0000;
/// The first widget id the pack's cells use: past every one of the rig's.
const PACK_ID_BASE: WidgetId = CELL_ID_BASE + 0x100;

/// The panel's one quick action: the focused stack sent to the other
/// container, as [`loadout::send`]. Which key or button presses it is
/// `crate::app`'s binding.
pub const SEND: QuickAction = QuickAction(0);

/// A move the panel asks for, which `crate::app` applies through the kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelMove {
    /// A drag let go over a cell that takes it: the stack at `slot` of
    /// `from`, its origin onto `at` in `to`, grab offset applied.
    Drag {
        /// The container it was taken from.
        from: ContainerId,
        /// Its slot there.
        slot: SlotId,
        /// The container it was let go over.
        to: ContainerId,
        /// Where its origin lands.
        at: Cell,
    },
    /// [`SEND`] on the stack at `slot` of `from`.
    Send {
        /// The container it is in.
        from: ContainerId,
        /// Its slot there.
        slot: SlotId,
    },
}

/// What one frame of the panel drew, and what the player asked of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelStats {
    /// How many draw commands it produced.
    pub commands: usize,
    /// The move the player just asked for. `None` on every frame but the one
    /// a drag ends or a quick action is pressed on, on a release that ended
    /// where it began — which is a click, and this panel has nothing for a
    /// click to do — and on a release over a cell that would not take it.
    pub moved: Option<PanelMove>,
}

/// What a drag carries: which stack, by the container and slot it is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carried {
    from: ContainerId,
    slot: SlotId,
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
    let cells = f32::from(loadout::GRID_H + loadout::PACK_H) * CELL_PX;
    let height = 2.0f32.mul_add(PANEL_PAD, cells) + ROWS * ROW_HEIGHT;
    let min = Vec2::new(
        (extent.0 as f32 - width) * 0.5,
        (extent.1 as f32 - height) * 0.5,
    );
    (min, min + Vec2::new(width, height))
}

/// The rig's cells on a surface of `extent`: where cell `(0, 0)`'s top-left
/// corner is, how big a cell is, and the widget ids they answer to.
fn rig_cells(extent: (u32, u32)) -> CellGrid {
    let (min, _) = bounds(extent);
    CellGrid {
        origin: Vec2::new(min.x + PANEL_PAD, min.y + PANEL_PAD + ROW_HEIGHT),
        cell: Vec2::splat(CELL_PX),
        columns: u32::from(loadout::GRID_W),
        rows: u32::from(loadout::GRID_H),
        id_base: CELL_ID_BASE,
        window: None,
    }
}

/// The pack's cells: under the rig's, past its summary row and the pack's
/// label.
fn pack_cells(extent: (u32, u32)) -> CellGrid {
    let rig = rig_cells(extent);
    let below = rig.origin.y + f32::from(loadout::GRID_H) * CELL_PX + 2.0 * ROW_HEIGHT;
    CellGrid {
        origin: Vec2::new(rig.origin.x, below),
        columns: u32::from(loadout::PACK_W),
        rows: u32::from(loadout::PACK_H),
        id_base: PACK_ID_BASE,
        ..rig
    }
}

/// The container whose cells answer to `id_base`.
fn container_of(id_base: WidgetId) -> ContainerId {
    if id_base == PACK_ID_BASE {
        loadout::PACK
    } else {
        loadout::RIG
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

/// Where `cell` of the rig is drawn, in pixels.
#[must_use]
pub fn cell_bounds(extent: (u32, u32), cell: Cell) -> (Vec2, Vec2) {
    rig_cells(extent).cell_bounds(to_drag(cell))
}

/// Where `cell` of the pack is drawn, in pixels.
#[must_use]
pub fn pack_cell_bounds(extent: (u32, u32), cell: Cell) -> (Vec2, Vec2) {
    pack_cells(extent).cell_bounds(to_drag(cell))
}

/// Which cell of the rig `pos` is over, or `None` for a point outside it.
///
/// The panel's hit test and the one a test aims with, so a check that clicks a
/// cell is clicking the cell the frame drew rather than a rectangle it worked
/// out for itself.
#[must_use]
pub fn cell_at(extent: (u32, u32), pos: Vec2) -> Option<Cell> {
    rig_cells(extent).cell_at(pos).and_then(from_drag)
}

/// The two containers the panel draws.
#[derive(Clone, Copy, Debug)]
struct Containers<'a> {
    rig: &'a Grid,
    pack: &'a Grid,
}

impl Containers<'_> {
    fn grid(&self, container: ContainerId) -> &Grid {
        if container == loadout::PACK {
            self.pack
        } else {
            self.rig
        }
    }

    /// What a hand takes hold of on `cell` of `container`: the stack covering
    /// it, gripped at its origin.
    fn grip(&self, container: ContainerId, cell: UVec2) -> Option<Grip<Carried>> {
        let grid = self.grid(container);
        let slot = grid.at(from_drag(cell)?)?;
        Some(Grip {
            payload: Carried {
                from: container,
                slot,
            },
            origin: to_drag(grid.slot(slot)?.at()),
        })
    }

    /// Whether `to` would take `carried` with its origin on `at`, in the
    /// rotation it already has, and why not.
    ///
    /// In its own container [`Grid::can_move_within`] does not count the
    /// stack's own cells as occupied, so a one-cell nudge of the `2×1` sidearm
    /// is a move rather than a collision with itself.
    fn fits(&self, carried: Carried, to: ContainerId, at: Cell) -> Result<(), InventoryError> {
        let from = self.grid(carried.from);
        let placement = from
            .slot(carried.slot)
            .ok_or(InventoryError::NoSuchSlot(carried.slot))?;
        let catalog = loadout::catalog();
        if carried.from == to {
            from.can_move_within(catalog, carried.slot, at, placement.rotation())
        } else {
            self.grid(to)
                .can_place(catalog, placement.stack().item(), at, placement.rotation())
        }
    }

    /// The placement a drag is carrying.
    fn carried(&self, carried: Carried) -> Option<Placement> {
        self.grid(carried.from).slot(carried.slot)
    }
}

/// What the panel keeps between frames: which cell owns the pointer press, the
/// drag riding on it — which stack it took hold of, and where — the pad's
/// focus, and why the last move was refused.
#[derive(Debug, Default)]
pub struct PanelState {
    ui: UiState,
    drag: GridDrag<Carried>,
    refusal: Option<Refusal>,
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

    /// What became of the move the panel last asked for: a refusal is shown
    /// under the pack until a later move lands.
    pub fn note(&mut self, outcome: Result<Applied, Refusal>) {
        self.refusal = outcome.err();
    }

    /// Why the last move was refused, if it was.
    #[must_use]
    pub const fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }
}

/// Draws the panel and resolves this frame's input against it.
///
/// `state` is what a drag cannot do without, and it is the caller's because it
/// has to survive between frames. `rig` and `pack` are the two containers, as
/// the game holds them.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    containers: (&Grid, &Grid),
    state: &mut PanelState,
    input: DragInput,
) -> PanelStats {
    let containers = Containers {
        rig: containers.0,
        pack: containers.1,
    };
    let (min, max) = bounds(extent);
    list.rect(min, max, PANEL_BG);
    list.rect_outline(min, max, BORDER_WIDTH, BORDER);
    list.text(
        Vec2::new(min.x + PANEL_PAD, min.y + PANEL_PAD * 0.5),
        TITLE.to_string(),
        LABEL,
        NATURAL_FONT_SIZE,
    );

    let rig = rig_cells(extent);
    let pack = pack_cells(extent);
    // Restated every frame, which replaces rather than adds: the links are
    // part of the layout, and the layout is this function's.
    state.drag.link(&rig, Direction::Down, &pack);
    state.drag.link(&pack, Direction::Up, &rig);

    let mut frame = state.drag.frame_with(&mut state.ui, input);
    let mut answers = [GridResponse::default(); 2];
    for (answer, (cells, container)) in answers
        .iter_mut()
        .zip([(&rig, loadout::RIG), (&pack, loadout::PACK)])
    {
        *answer = frame.grid(
            cells,
            |cell| containers.grip(container, cell),
            |carried, target| {
                from_drag(target.origin)
                    .is_some_and(|at| containers.fits(*carried, container, at).is_ok())
            },
        );
    }
    let ghost = frame.ghost();
    let quick = frame.take_quick_move();
    let ended = frame.release();

    let mut moved = quick
        .filter(|quick| quick.action == SEND)
        .map(|quick| PanelMove::Send {
            from: quick.payload.from,
            slot: quick.payload.slot,
        });
    if let Some(ended) = ended
        && let Some(target) = ended.target
        && let Some(at) = from_drag(target.origin)
    {
        let to = container_of(target.at.grid);
        if ended.accepted {
            moved = Some(PanelMove::Drag {
                from: ended.payload.from,
                slot: ended.payload.slot,
                to,
                at,
            });
        } else if let Err(why) = containers.fits(ended.payload, to, at) {
            state.refusal = Some(Refusal::Grid(why));
        }
    }

    for (cells, answer, container) in [
        (&rig, &answers[0], loadout::RIG),
        (&pack, &answers[1], loadout::PACK),
    ] {
        draw_cells(list, cells, answer);
        draw_items(list, atlas, cells, containers.grid(container));
    }
    let carried = state
        .drag
        .held()
        .and_then(|held| containers.carried(*held.payload()));
    if let (Some(ghost), Some(placement)) = (ghost, carried) {
        draw_ghost(list, ghost, placement);
    }

    // The summary: what the rig holds and what it weighs, under the rig. Both
    // are facts about the grid, so neither moves on a frame the player did
    // nothing on.
    let rig_bottom = rig.origin.y + f32::from(loadout::GRID_H) * CELL_PX;
    list.text(
        Vec2::new(min.x + PANEL_PAD, rig_bottom),
        summary(containers.rig),
        LABEL,
        NATURAL_FONT_SIZE,
    );
    list.text(
        Vec2::new(min.x + PANEL_PAD, rig_bottom + ROW_HEIGHT),
        PACK_LABEL.to_string(),
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
        moved,
    }
}

/// Every cell of `cells`, filled by what this frame's drag answered for it and
/// outlined — in [`FOCUS`] where the pad and the keyboard are.
fn draw_cells(list: &mut DrawList, cells: &CellGrid, answer: &GridResponse) {
    for cell in cells.cells() {
        let (at, to) = cells.cell_bounds(cell);
        let answer = answer.cell(cell);
        let fill = match answer.drop {
            DropFeedback::Refusing => CELL_REFUSED,
            DropFeedback::Accepting => CELL_LIT,
            DropFeedback::None if answer.state != ButtonState::Idle => CELL_LIT,
            DropFeedback::None => CELL_BG,
        };
        list.rect(at, to, fill);
        list.rect_outline(at, to, BORDER_WIDTH, BORDER);
        if answer.focused {
            list.rect_outline(at, to, FOCUS_WIDTH, FOCUS);
        }
    }
}

/// Every stack in `grid`: its colour over each cell it covers, its letter on
/// its origin, and its count on its last cell when it holds more than one.
fn draw_items(list: &mut DrawList, atlas: &FontAtlas, cells: &CellGrid, grid: &Grid) {
    let catalog = loadout::catalog();
    let bounds = |cell: Cell| cells.cell_bounds(to_drag(cell));
    for (_, placement) in grid.slots() {
        let stack = placement.stack();
        let Some(def) = catalog.get(stack.item()) else {
            continue;
        };
        let shape = def.shape().rotated(placement.rotation());
        for covered in shape.cells() {
            let cell = Cell::new(placement.at().x + covered.x, placement.at().y + covered.y);
            let (at, to) = bounds(cell);
            list.rect(
                at + Vec2::splat(BORDER_WIDTH),
                to - Vec2::splat(BORDER_WIDTH),
                def.colour(),
            );
        }
        let (at, _) = bounds(placement.at());
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
            let (_, end) = bounds(corner);
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
}

/// The carried stack's footprint where `ghost` says, in its own colour — or in
/// [`CELL_REFUSED`] over a cell that would not take it — at [`GHOST_ALPHA`].
fn draw_ghost(list: &mut DrawList, ghost: Ghost, placement: Placement) {
    let Some(def) = loadout::catalog().get(placement.stack().item()) else {
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

/// The summary row: how many stacks the rig holds and what they weigh, in the
/// one spelling [`loadout::summary`] owns.
fn summary(grid: &Grid) -> String {
    loadout::summary(grid.len(), loadout::weight_g(grid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::ui::draw_list::DrawCommand;
    use crcbl::ui::tree::NavInput;
    use crcbl::ui::widget::PointerInput;

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

    /// The pack a run starts with: empty.
    fn empty_pack() -> Grid {
        loadout::pack(&loadout::carried()).clone()
    }

    /// The frame's input from the pointer alone.
    fn pointing(pointer: PointerInput) -> DragInput {
        DragInput {
            pointer,
            ..DragInput::default()
        }
    }

    /// The frame's input from the pad or the keyboard alone.
    fn pad(nav: NavInput) -> DragInput {
        DragInput {
            nav,
            ..DragInput::default()
        }
    }

    /// One frame of the starting rig and an empty pack.
    fn frame(state: &mut PanelState, input: DragInput) -> (PanelStats, DrawList) {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let stats = draw(
            &mut list,
            &atlas,
            EXTENT,
            (&loadout::packed(), &empty_pack()),
            state,
            input,
        );
        (stats, list)
    }

    /// **Every cell is inside the panel, and every point in a cell hit-tests
    /// back to it; the pack is under the rig, clear of it.** The two halves of
    /// one convention: a drag aims with [`cell_at`] and the frame draws with
    /// [`cell_bounds`], and a panel whose two disagreed would move the wrong
    /// item on every drag.
    #[test]
    fn the_hit_test_and_the_drawing_agree_about_where_a_cell_is() {
        let (min, max) = bounds(EXTENT);
        let inside = |(at, to): (Vec2, Vec2)| {
            at.x >= min.x && at.y >= min.y && to.x <= max.x && to.y <= max.y
        };
        for y in 0..loadout::GRID_H {
            for x in 0..loadout::GRID_W {
                let cell = Cell::new(x, y);
                let (at, to) = cell_bounds(EXTENT, cell);
                assert!(
                    inside((at, to)),
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
                let grid = rig_cells(EXTENT);
                assert_eq!(
                    grid.cell_of(grid.id_of(to_drag(cell))).and_then(from_drag),
                    Some(cell),
                    "the id of {cell:?}",
                );
            }
        }
        let rig_bottom = cell_bounds(EXTENT, Cell::new(0, loadout::GRID_H - 1)).1.y;
        let pack = pack_cells(EXTENT);
        for cell in pack.cells() {
            let drawn = pack.cell_bounds(cell);
            assert!(inside(drawn), "pack {cell} is drawn outside the panel");
            assert!(drawn.0.y > rig_bottom, "pack {cell} overlaps the rig");
            assert_eq!(rig_cells(EXTENT).cell_at((drawn.0 + drawn.1) * 0.5), None);
            assert!(
                rig_cells(EXTENT).cell_of(pack.id_of(cell)).is_none(),
                "pack {cell} shares an id with the rig",
            );
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
            (&loadout::empty(), &empty_pack()),
            &mut ui,
            DragInput::default(),
        );
        assert!(bare.commands > 0, "the panel drew nothing at all");

        let mut list = DrawList::new();
        let full = draw(
            &mut list,
            &atlas,
            EXTENT,
            (&loadout::packed(), &empty_pack()),
            &mut ui,
            DragInput::default(),
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
        assert!(
            drawn.contains(&PACK_LABEL.to_string()),
            "no pack: {drawn:?}"
        );
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
        let commands = || {
            let mut ui = PanelState::new();
            let input = pointing(PointerInput::hovering(Vec2::new(4.0, 4.0)));
            format!("{:?}", frame(&mut ui, input).1.commands())
        };
        assert_eq!(
            commands(),
            commands(),
            "the panel draws something that ticks"
        );
    }

    /// The middle of `cell` of the rig.
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

    /// Presses over `from`, drags to `to` and lets go there, one frame each;
    /// answers the frame the pointer was held over `to` and the release.
    fn drag_across(
        state: &mut PanelState,
        from: Cell,
        to: Cell,
    ) -> ((PanelStats, DrawList), (PanelStats, DrawList)) {
        let (pressed, _) = frame(state, pointer(centre(from), true));
        assert_eq!(pressed.moved, None, "a press alone moved something");
        let held = frame(state, pointer(centre(to), true));
        assert_eq!(held.0.moved, None, "a drag moved something before release");
        let done = frame(state, pointer(centre(to), false));
        (held, done)
    }

    /// The fill `cell` of the rig was drawn with.
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

    /// The sidearm's slot in the starting rig.
    fn sidearm() -> SlotId {
        loadout::packed().at(Cell::new(0, 0)).expect("the sidearm")
    }

    /// **A press on one cell and a release on another is a drag; a press and a
    /// release on the same cell is not.**
    ///
    /// The second half is the control, and it is what the capture is for: a
    /// panel that reported the cell under the pointer at release would call
    /// every click a drag onto itself, and the kit's `Move` would then be
    /// asked to move every item onto its own cell on every click.
    #[test]
    fn a_press_on_one_cell_and_a_release_on_another_is_a_drag() {
        let mut state = PanelState::new();
        let from = Cell::new(0, 0);
        let to = Cell::new(1, 2);
        let ((_, held), (done, _)) = drag_across(&mut state, from, to);
        assert_eq!(
            fill(&held, to),
            CELL_LIT,
            "a free cell was not lit as a target"
        );
        assert_eq!(
            done.moved,
            Some(PanelMove::Drag {
                from: loadout::RIG,
                slot: sidearm(),
                to: loadout::RIG,
                at: to,
            }),
            "the drag was not reported"
        );
        assert_eq!(state.ui.active(), None, "the capture outlived the press");

        // The control: a press and a release over one cell is a click, and this
        // panel has nothing for a click to do.
        let (_, (clicked, _)) = drag_across(&mut state, from, from);
        assert_eq!(clicked.moved, None, "a click was reported as a drag");
    }

    /// **A stack lands where the hand let go, whatever part of it the hand
    /// took hold of.** The `2×1` sidearm grabbed by its right-hand cell and let
    /// go one cell right of that moves its origin by one — a panel that put the
    /// *origin* under the pointer would land it on `(2, 2)`.
    #[test]
    fn a_drag_keeps_the_grip_it_started_with() {
        let mut state = PanelState::new();
        let (_, (done, _)) = drag_across(&mut state, Cell::new(1, 0), Cell::new(2, 2));
        assert_eq!(
            done.moved,
            Some(PanelMove::Drag {
                from: loadout::RIG,
                slot: sidearm(),
                to: loadout::RIG,
                at: Cell::new(1, 2),
            })
        );
    }

    /// **A cell the rig would not take the stack in is drawn refusing, letting
    /// go there moves nothing, and the panel says why.** The sidearm with its
    /// origin on the last column runs off the right edge; the control is the
    /// free cell of the first test, drawn lit rather than refusing, with no
    /// reason under the pack.
    #[test]
    fn a_refused_cell_is_drawn_refusing_drops_nothing_and_says_why() {
        let mut state = PanelState::new();
        let refused = Cell::new(3, 2);
        let ((_, held), (done, list)) = drag_across(&mut state, Cell::new(0, 0), refused);
        assert_eq!(fill(&held, refused), CELL_REFUSED);
        assert_eq!(done.moved, None, "a refused drop was reported");
        let why = Refusal::Grid(InventoryError::OutOfBounds {
            x: 3,
            y: 2,
            w: 2,
            h: 1,
        });
        assert_eq!(state.refusal(), Some(why));
        assert!(
            text(&list).contains(&why.to_string()),
            "the refusal is not on the panel: {:?}",
            text(&list),
        );

        // The control, and what clears it: a move that lands.
        state.note(Ok(Applied::Moved));
        let (_, list) = frame(&mut state, DragInput::default());
        assert!(!text(&list).contains(&why.to_string()), "a stale refusal");
    }

    /// **A refusal the kit made is shown as the panel's own is**: `crate::app`
    /// notes what the command answered, and the reason is drawn.
    #[test]
    fn a_refusal_the_kit_made_is_shown() {
        let mut state = PanelState::new();
        let why = Refusal::Grid(InventoryError::NoRoom);
        let (_, before) = frame(&mut state, DragInput::default());
        assert!(!text(&before).contains(&why.to_string()));
        state.note(Err(why));
        let (_, list) = frame(&mut state, DragInput::default());
        assert!(text(&list).contains(&why.to_string()), "{:?}", text(&list));
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

    /// **The ghost follows the pointer in the carried stack's colour, held
    /// where it was taken hold of, and turns refusing over a cell that will
    /// not take it**; it is gone once the drag ends.
    #[test]
    fn the_ghost_follows_the_pointer_tinted_by_the_cell_under_it() {
        let colour = loadout::catalog()
            .get(
                loadout::packed()
                    .slot(sidearm())
                    .expect("held")
                    .stack()
                    .item(),
            )
            .expect("the sidearm's definition")
            .colour();
        let mut state = PanelState::new();
        frame(&mut state, pointer(centre(Cell::new(1, 0)), true));
        let at = centre(Cell::new(2, 2)) + Vec2::new(3.0, 1.0);
        let (_, held) = frame(&mut state, pointer(at, true));
        let grab = Vec2::new(1.5, 0.5) * CELL_PX;
        assert_eq!(
            ghost_cells(&held, colour),
            vec![at - grab, at - grab + Vec2::new(CELL_PX, 0.0)],
            "the ghost is not the sidearm's two cells, held by the right-hand one",
        );

        let refused = centre(Cell::new(3, 1));
        let (_, over) = frame(&mut state, pointer(refused, true));
        assert_eq!(
            ghost_cells(&over, CELL_REFUSED).len(),
            2,
            "no refusing tint"
        );
        assert!(ghost_cells(&over, colour).is_empty());

        let (_, ended) = frame(&mut state, pointer(refused, false));
        assert!(
            ghost_cells(&ended, CELL_REFUSED).is_empty(),
            "a ghost outlived the drag"
        );
    }

    /// **The pad carries a stack off the rig's bottom edge into the pack**:
    /// the first step lands focus on the rig's first cell, accept picks the
    /// sidearm up, three steps down cross into the pack, two right carry it
    /// over pack cells whose rig cells are taken, and accept drops it there —
    /// one [`PanelMove::Drag`] between the two containers, judged by the
    /// pack's cells and not the rig's.
    #[test]
    fn the_pad_carries_a_stack_from_the_rig_into_the_pack() {
        let mut state = PanelState::new();
        frame(&mut state, DragInput::default());
        let (_, landed) = frame(&mut state, pad(NavInput::toward(Direction::Down)));
        let first = cell_bounds(EXTENT, Cell::new(0, 0));
        assert_eq!(
            focus_outlines(&landed),
            vec![first.0],
            "the landing is not outlined on the rig's first cell",
        );
        let (picked, _) = frame(&mut state, pad(NavInput::ACCEPT));
        assert_eq!(picked.moved, None, "the pick-up moved something");
        for _ in 0..loadout::GRID_H {
            frame(&mut state, pad(NavInput::toward(Direction::Down)));
        }
        let to = Cell::new(2, 0);
        for _ in 0..to.x {
            frame(&mut state, pad(NavInput::toward(Direction::Right)));
        }
        assert!(
            loadout::packed().at(to).is_some(),
            "the control: the rig's cell of the same number is taken",
        );
        let (dropped, _) = frame(&mut state, pad(NavInput::ACCEPT));
        assert_eq!(
            dropped.moved,
            Some(PanelMove::Drag {
                from: loadout::RIG,
                slot: sidearm(),
                to: loadout::PACK,
                at: to,
            }),
        );
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

    /// **Back takes the pad's drag back and asks for nothing**, though the
    /// cursor is over a cell that would take it.
    #[test]
    fn back_takes_the_pads_drag_back_and_moves_nothing() {
        let mut state = PanelState::new();
        frame(&mut state, DragInput::default());
        frame(&mut state, pad(NavInput::toward(Direction::Down)));
        frame(&mut state, pad(NavInput::ACCEPT));
        frame(&mut state, pad(NavInput::toward(Direction::Down)));
        let (_, held) = frame(&mut state, pad(NavInput::toward(Direction::Down)));
        assert_eq!(
            fill(&held, Cell::new(0, 2)),
            CELL_LIT,
            "the control: it fits"
        );
        let (back, _) = frame(&mut state, pad(NavInput::BACK));
        assert_eq!(back.moved, None, "back moved something");
        assert_eq!(state.refusal(), None, "back was reported as a refusal");
        assert!(state.drag.held().is_none(), "back kept the drag");
    }

    /// **[`SEND`] on the focused stack asks for it to go to the other
    /// container**, with no drag; on an empty cell it asks for nothing.
    #[test]
    fn send_asks_for_the_focused_stack_to_go_to_the_other_container() {
        let mut state = PanelState::new();
        let send = |nav| DragInput {
            nav,
            quick: Some(SEND),
            ..DragInput::default()
        };
        frame(&mut state, DragInput::default());
        frame(&mut state, pad(NavInput::toward(Direction::Down)));
        let (sent, _) = frame(&mut state, send(NavInput::NAVIGATION));
        assert_eq!(
            sent.moved,
            Some(PanelMove::Send {
                from: loadout::RIG,
                slot: sidearm(),
            }),
        );
        assert!(state.drag.held().is_none(), "send picked the stack up");

        // The control: the cell under the sidearm's row, past the magazine,
        // holds nothing.
        frame(&mut state, pad(NavInput::toward(Direction::Down)));
        frame(&mut state, pad(NavInput::toward(Direction::Down)));
        let (nothing, _) = frame(&mut state, send(NavInput::NAVIGATION));
        assert_eq!(nothing.moved, None, "an empty cell was sent");
    }
}
