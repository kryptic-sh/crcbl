//! The pointer drag: hit tests, grips, drops, grids that scroll, and drags
//! that cross from one grid to another.

use super::*;

/// A 4×3 grid of 10-pixel cells at `(100, 50)`.
const GRID: CellGrid = CellGrid {
    origin: Vec2::new(100.0, 50.0),
    cell: Vec2::splat(10.0),
    columns: 4,
    rows: 3,
    id_base: 0x1000,
    window: None,
};

/// A second grid, beside the first, with its own ids.
const OTHER: CellGrid = CellGrid {
    origin: Vec2::new(200.0, 50.0),
    cell: Vec2::splat(10.0),
    columns: 2,
    rows: 2,
    id_base: 0x2000,
    window: None,
};

/// The payload the tests drag: an item name, anchored at `ORIGIN`.
const ITEM: &str = "plate";
/// Where the item's footprint starts.
const ORIGIN: UVec2 = UVec2::new(1, 0);
/// The item's footprint: a 2×2, so it covers `(1, 0)..=(2, 1)`.
const SIZE: UVec2 = UVec2::splat(2);

fn covers(cell: UVec2) -> bool {
    cell.checked_sub(ORIGIN)
        .is_some_and(|offset| offset.cmplt(SIZE).all())
}

fn source(cell: UVec2) -> Option<Grip<&'static str>> {
    covers(cell).then_some(Grip {
        payload: ITEM,
        origin: ORIGIN,
    })
}

fn centre(grid: &CellGrid, cell: UVec2) -> Vec2 {
    let (at, to) = grid.cell_bounds(cell);
    (at + to) * 0.5
}

fn press(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: true,
        released: false,
        secondary_pressed: false,
    }
}

fn release(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: false,
        released: true,
        secondary_pressed: false,
    }
}

/// One frame over [`GRID`], with `accept` as its target.
fn frame(
    drag: &mut GridDrag<&'static str>,
    ui: &mut UiState,
    pointer: PointerInput,
    accept: bool,
) -> (GridResponse, Option<Dropped<&'static str>>) {
    let mut frame = drag.frame(ui, pointer);
    let response = frame.grid(&GRID, source, |_, _| accept);
    (response, frame.finish())
}

/// Press on `from`, move to `to`, release on `to`.
fn drag_between(
    drag: &mut GridDrag<&'static str>,
    ui: &mut UiState,
    from: UVec2,
    to: UVec2,
    accept: bool,
) -> (GridResponse, Option<Dropped<&'static str>>) {
    let (_, pressed) = frame(drag, ui, press(centre(&GRID, from)), accept);
    assert_eq!(pressed, None, "a press alone dropped something");
    let (_, moving) = frame(drag, ui, press(centre(&GRID, to)), accept);
    assert_eq!(moving, None, "a drag dropped before release");
    let (response, _) = frame(drag, ui, press(centre(&GRID, to)), accept);
    let (_, dropped) = frame(drag, ui, release(centre(&GRID, to)), accept);
    (response, dropped)
}

/// **Every cell's centre and corner hit-test back to it, and its id
/// round-trips.** The drawing and the hit test are one convention; a grid
/// whose two disagreed would drag the wrong cell.
#[test]
fn the_hit_test_and_the_drawing_agree_about_where_a_cell_is() {
    let cells: Vec<UVec2> = GRID.cells().collect();
    assert_eq!(cells.len(), 12, "a 4x3 grid has twelve cells");
    for cell in cells {
        let (at, _) = GRID.cell_bounds(cell);
        assert_eq!(GRID.cell_at(centre(&GRID, cell)), Some(cell));
        assert_eq!(GRID.cell_at(at + Vec2::splat(0.5)), Some(cell));
        assert_eq!(GRID.cell_of(GRID.id_of(cell)), Some(cell));
        assert_eq!(OTHER.cell_of(GRID.id_of(cell)), None);
    }
    assert_eq!(GRID.cell_at(GRID.origin - Vec2::ONE), None);
    assert_eq!(
        GRID.cell_at(Vec2::new(140.0, 60.0)),
        None,
        "past the last column"
    );
    assert_eq!(GRID.cell_at(Vec2::new(f32::NAN, 55.0)), None);
}

/// **A drag from A to B reports `(A, B)`.**
#[test]
fn a_drag_from_one_cell_to_another_reports_both() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let from = UVec2::new(1, 0);
    let to = UVec2::new(2, 2);
    let (_, dropped) = drag_between(&mut drag, &mut ui, from, to, true);
    let dropped = dropped.expect("the drag was not reported");
    assert_eq!(dropped.payload, ITEM);
    assert_eq!(
        dropped.from,
        GridCell {
            grid: GRID.id_base,
            cell: from
        }
    );
    assert_eq!(
        dropped.to.at,
        GridCell {
            grid: GRID.id_base,
            cell: to
        }
    );
    assert!(drag.held().is_none(), "the drag outlived its release");
    assert_eq!(ui.active(), None, "the capture outlived the press");
}

/// **A release where the drag began does nothing**, and nor does it light
/// the cell as a target.
#[test]
fn a_release_where_the_drag_began_drops_nothing() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let cell = UVec2::new(2, 1);
    let (response, dropped) = drag_between(&mut drag, &mut ui, cell, cell, true);
    assert_eq!(dropped, None, "a click was reported as a drop");
    assert_eq!(response.cell(cell).drop, DropFeedback::None);
    assert_eq!(response.cell(cell).state, ButtonState::Pressed);
    assert!(drag.held().is_none());
}

/// **A refusing target is drawn as refusing and drops nothing; the same
/// drag over an accepting one is drawn as accepting.**
#[test]
fn a_refused_target_gives_refusing_feedback_and_no_drop() {
    let from = UVec2::new(1, 1);
    let to = UVec2::new(3, 2);

    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let (response, dropped) = drag_between(&mut drag, &mut ui, from, to, false);
    assert_eq!(response.cell(to).drop, DropFeedback::Refusing);
    assert_eq!(response.cell(from).drop, DropFeedback::None);
    assert_eq!(dropped, None, "a refused target took the drop");
    assert!(
        drag.held().is_none(),
        "a refused release did not end the drag"
    );

    // The control: the same drag, accepted.
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let (response, dropped) = drag_between(&mut drag, &mut ui, from, to, true);
    assert_eq!(response.cell(to).drop, DropFeedback::Accepting);
    assert!(dropped.is_some(), "an accepting target dropped nothing");
}

/// **The grab offset is kept**: a 2×2 taken by its bottom-right cell and
/// let go with the pointer on `(3, 2)` lands its origin on `(2, 1)`, and a
/// grip that would push the origin off the top-left edge is refused
/// without asking the target.
#[test]
fn the_grab_offset_is_kept() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let grabbed = UVec2::new(2, 1);
    let (_, dropped) = drag_between(&mut drag, &mut ui, grabbed, UVec2::new(3, 2), true);
    let dropped = dropped.expect("the drag was not reported");
    assert_eq!(dropped.to.origin, UVec2::new(2, 1), "the grip moved");

    // Grabbed by its origin, the origin lands under the pointer.
    let (_, dropped) = drag_between(&mut drag, &mut ui, ORIGIN, UVec2::new(0, 2), true);
    assert_eq!(dropped.expect("a drop").to.origin, UVec2::new(0, 2));

    // Off the left edge: refused, and the target is not consulted.
    let mut asked = false;
    let mut frame = drag.frame(&mut ui, press(centre(&GRID, grabbed)));
    frame.grid(&GRID, source, |_, _| true);
    frame.finish();
    let mut frame = drag.frame(&mut ui, release(centre(&GRID, UVec2::new(0, 2))));
    let response = frame.grid(&GRID, source, |_, _| {
        asked = true;
        true
    });
    assert_eq!(frame.finish(), None, "an origin off the grid was dropped");
    assert_eq!(response.cell(UVec2::new(0, 2)).drop, DropFeedback::Refusing);
    assert!(!asked, "the target was asked about an origin at -1");
}

/// **A turned payload keeps its grip inside its new footprint.**
/// **A quarter turn keeps the grabbed cell under the pointer.** Cell
/// `(column, row)` of a `w × h` footprint becomes `(h − 1 − row, column)`
/// of the `h × w` one: EW's `1×2` held by its top cell is held by `(1, 0)`
/// once it lies `2×1`, and four turns come back to where they began.
/// **A cell may be any rectangle.** A 300×78 one-cell slot — EW's weapon
/// card — is hit anywhere inside it and nowhere past it, and a grid of
/// 20×10 cells places and finds each cell by its own width and height; a
/// cell of no size, or a non-finite one, is over nothing.
#[test]
fn a_cell_may_be_any_rectangle() {
    let slot = CellGrid {
        origin: Vec2::new(40.0, 20.0),
        cell: Vec2::new(300.0, 78.0),
        columns: 1,
        rows: 1,
        id_base: 0x3000,
        window: None,
    };
    assert_eq!(
        slot.cell_bounds(UVec2::ZERO),
        (Vec2::new(40.0, 20.0), Vec2::new(340.0, 98.0))
    );
    assert_eq!(slot.cell_at(Vec2::new(339.0, 97.0)), Some(UVec2::ZERO));
    assert_eq!(slot.cell_at(Vec2::new(200.0, 99.0)), None, "below the card");
    assert_eq!(slot.cell_at(Vec2::new(341.0, 30.0)), None, "past its right");

    let wide = CellGrid {
        origin: Vec2::ZERO,
        cell: Vec2::new(20.0, 10.0),
        columns: 4,
        rows: 3,
        id_base: 0x4000,
        window: None,
    };
    for cell in wide.cells() {
        let (min, max) = wide.cell_bounds(cell);
        assert_eq!(max - min, Vec2::new(20.0, 10.0));
        assert_eq!(wide.cell_at((min + max) / 2.0), Some(cell), "{cell}");
    }
    assert_eq!(wide.cell_at(Vec2::new(25.0, 15.0)), Some(UVec2::new(1, 1)));

    for cell in [Vec2::new(20.0, 0.0), Vec2::new(f32::NAN, 10.0)] {
        let degenerate = CellGrid { cell, ..wide };
        assert_eq!(degenerate.cell_at(Vec2::new(5.0, 5.0)), None, "{cell}");
    }
}

/// One frame over [`GRID`] ended with [`DragFrame::release`].
fn release_frame(
    drag: &mut GridDrag<&'static str>,
    ui: &mut UiState,
    pointer: PointerInput,
    accept: bool,
) -> Option<Released<&'static str>> {
    let mut frame = drag.frame(ui, pointer);
    frame.grid(&GRID, source, |_, _| accept);
    frame.release()
}

/// **An overlapping grid's refusal does not undo another's acceptance.**
/// EW's optic strip is a one-cell grid drawn over the weapon card's: both
/// run each frame, and a drop either takes happens — whichever runs first,
/// and whichever of the two accepts.
#[test]
fn a_later_refusal_over_an_overlapping_grid_keeps_the_drop() {
    // Grabbed at its origin, so a one-cell slot can take it.
    let from = ORIGIN;
    let to = UVec2::new(3, 2);
    let (slot_min, _) = GRID.cell_bounds(to);
    let slot = CellGrid {
        origin: slot_min,
        cell: GRID.cell,
        columns: 1,
        rows: 1,
        id_base: 0x5000,
        window: None,
    };
    for (slot_first, slot_accepts) in [(true, true), (false, true), (true, false), (false, false)] {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let mut frame = drag.frame(&mut ui, press(centre(&GRID, from)));
        frame.grid(&GRID, source, |_, _| false);
        frame.finish();

        let mut frame = drag.frame(&mut ui, release(centre(&GRID, to)));
        let run_slot = |frame: &mut DragFrame<'_, &'static str>| {
            frame.grid(&slot, |_| None, |_, _| slot_accepts);
        };
        if slot_first {
            run_slot(&mut frame);
            frame.grid(&GRID, source, |_, _| !slot_accepts);
        } else {
            frame.grid(&GRID, source, |_, _| !slot_accepts);
            run_slot(&mut frame);
        }
        let ended = frame.release().expect("the release ends the drag");
        let case = format!("slot first {slot_first}, slot accepts {slot_accepts}");
        assert!(ended.accepted, "{case}: the acceptance was overwritten");
        let accepting = if slot_accepts {
            slot.id_base
        } else {
            GRID.id_base
        };
        assert_eq!(ended.over.map(|over| over.grid), Some(accepting), "{case}");
    }
}

/// **Every way a held drag ends is reported by `release`**: dropped,
/// refused where the item would have landed, let go over no grid, let go
/// with the origin off the grid's edge, and a click — while a drag that
/// goes on, or no drag at all, reports nothing.
#[test]
fn release_reports_every_ending() {
    let from = ORIGIN + UVec2::new(1, 1);
    let to = UVec2::new(3, 2);
    let begin = |drag: &mut GridDrag<&'static str>, ui: &mut UiState| {
        assert_eq!(
            release_frame(drag, ui, press(centre(&GRID, from)), true),
            None,
            "a drag that has just begun has not ended"
        );
    };
    let expect_target = DropTarget {
        at: GridCell {
            grid: GRID.id_base,
            cell: to,
        },
        origin: to - UVec2::ONE,
    };

    for accept in [true, false] {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        begin(&mut drag, &mut ui);
        let ended = release_frame(&mut drag, &mut ui, release(centre(&GRID, to)), accept)
            .expect("a release ends the drag");
        assert_eq!(ended.payload, ITEM);
        assert_eq!(ended.from.cell, from);
        assert_eq!(ended.over, Some(expect_target.at));
        assert_eq!(
            ended.target,
            Some(expect_target),
            "the refused target is kept"
        );
        assert_eq!(ended.accepted, accept);
    }

    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    begin(&mut drag, &mut ui);
    let outside = release_frame(&mut drag, &mut ui, release(Vec2::new(-50.0, -50.0)), true)
        .expect("let go over nothing still ends it");
    assert_eq!(
        (outside.over, outside.target, outside.accepted),
        (None, None, false)
    );

    // Grabbed one cell in, let go on column 0: the origin would be off the
    // left edge, so there is a cell but no target.
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    begin(&mut drag, &mut ui);
    let edge = release_frame(
        &mut drag,
        &mut ui,
        release(centre(&GRID, UVec2::new(0, 2))),
        true,
    )
    .expect("ended");
    assert!(edge.over.is_some());
    assert_eq!((edge.target, edge.accepted), (None, false));

    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    begin(&mut drag, &mut ui);
    let click = release_frame(&mut drag, &mut ui, release(centre(&GRID, from)), true)
        .expect("a click ends it too");
    assert_eq!(click.over.map(|over| over.cell), Some(from));
    assert_eq!((click.target, click.accepted), (None, false));

    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    assert_eq!(
        release_frame(&mut drag, &mut ui, release(centre(&GRID, to)), true),
        None,
        "nothing was held"
    );
}

#[test]
fn a_quarter_turn_keeps_the_grabbed_cell() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let mut frame = drag.frame(&mut ui, press(centre(&GRID, ORIGIN)));
    frame.grid(&GRID, source, |_, _| true);
    frame.finish();
    let held = drag.held_mut().expect("the press took hold");
    assert_eq!(held.grab(), UVec2::ZERO);
    assert!(!held.changed());

    held.turn_quarter(UVec2::new(1, 2));
    assert_eq!(
        held.grab(),
        UVec2::new(1, 0),
        "the 1x2's top cell, lying 2x1"
    );
    assert!(held.changed(), "a turn is a change");

    // Taken as a 3×2 now, held by (1, 0): each turn follows the rule, and
    // four turns of alternating sizes come back to where they began.
    held.refit(UVec2::new(3, 2));
    let start = held.grab();
    let mut sizes = [UVec2::new(3, 2), UVec2::new(2, 3)].into_iter().cycle();
    let mut grab = start;
    for _ in 0..4 {
        let size = sizes.next().expect("cycles");
        let expected = UVec2::new(size.y - 1 - grab.y, grab.x);
        held.turn_quarter(size);
        grab = expected;
        assert_eq!(held.grab(), grab);
    }
    assert_eq!(grab, start, "four quarter turns are a whole one");
}

/// **A payload changed while held drops where it started**; unchanged,
/// the same release there is a click. EW turns an item in place: press
/// it, turn it, let go on the same cell.
#[test]
fn a_changed_payload_released_where_it_began_is_a_drop() {
    let cell = ORIGIN;
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    frame(&mut drag, &mut ui, press(centre(&GRID, cell)), true);
    drag.held_mut().expect("held").payload_mut();
    let (response, dropped) = frame(&mut drag, &mut ui, release(centre(&GRID, cell)), true);
    let dropped = dropped.expect("a changed payload let go in place drops");
    assert_eq!(dropped.to.at.cell, cell);
    assert_eq!(dropped.to.origin, ORIGIN);
    assert_eq!(response.cell(cell).drop, DropFeedback::Accepting);

    // The control: the same press and release with nothing changed.
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    frame(&mut drag, &mut ui, press(centre(&GRID, cell)), true);
    let (_, dropped) = frame(&mut drag, &mut ui, release(centre(&GRID, cell)), true);
    assert_eq!(dropped, None, "an unchanged click dropped");
}

#[test]
fn a_refit_grip_stays_inside_the_footprint() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let mut frame = drag.frame(&mut ui, press(centre(&GRID, UVec2::new(2, 1))));
    frame.grid(&GRID, source, |_, _| true);
    frame.finish();
    let held = drag.held_mut().expect("the press took hold");
    assert_eq!(held.grab(), UVec2::new(1, 1));
    held.refit(UVec2::new(1, 3));
    assert_eq!(held.grab(), UVec2::new(0, 1));
    held.refit(UVec2::new(3, 1));
    assert_eq!(held.grab(), UVec2::new(0, 0));
}

/// **A press on one grid and a release on another is one drag**, reported
/// with each end's grid; a release over neither drops nothing.
#[test]
fn a_drag_crosses_from_one_grid_to_another() {
    let run = |to: Vec2| {
        let mut drag = GridDrag::new();
        let mut ui = UiState::new();
        let mut dropped = None;
        for pointer in [press(centre(&GRID, ORIGIN)), press(to), release(to)] {
            let mut frame = drag.frame(&mut ui, pointer);
            frame.grid(&GRID, source, |_, _| true);
            frame.grid(
                &OTHER,
                |_| None,
                |_, target| target.origin == UVec2::new(1, 1),
            );
            dropped = frame.finish();
        }
        (dropped, drag.held().is_some())
    };

    let (dropped, still_held) = run(centre(&OTHER, UVec2::new(1, 1)));
    let dropped = dropped.expect("the cross-grid drag was not reported");
    assert_eq!(dropped.from.grid, GRID.id_base);
    assert_eq!(
        dropped.to.at,
        GridCell {
            grid: OTHER.id_base,
            cell: UVec2::new(1, 1)
        }
    );
    assert!(!still_held);

    // The other grid refuses `(0, 0)`, and nothing is under `(0, 0)` on screen.
    assert_eq!(run(centre(&OTHER, UVec2::ZERO)).0, None);
    let (dropped, still_held) = run(Vec2::ZERO);
    assert_eq!(dropped, None, "a release over nothing dropped");
    assert!(!still_held, "a release over nothing kept the drag");
}

/// **A capture cleared mid-drag takes the drag with it**, so a panel torn
/// down mid-press does not reopen holding an item.
#[test]
fn a_cleared_capture_drops_the_drag() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    frame(&mut drag, &mut ui, press(centre(&GRID, ORIGIN)), true);
    assert!(drag.held().is_some());
    ui.clear();
    let (_, dropped) = frame(
        &mut drag,
        &mut ui,
        release(centre(&GRID, UVec2::new(0, 2))),
        true,
    );
    assert_eq!(dropped, None);
    assert!(drag.held().is_none());
}

/// **A press over a cell holding nothing starts no drag**, and a later
/// release elsewhere drops nothing.
#[test]
fn a_press_on_an_empty_cell_takes_hold_of_nothing() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let (_, dropped) = drag_between(&mut drag, &mut ui, UVec2::new(0, 2), UVec2::new(1, 0), true);
    assert_eq!(dropped, None);
}

/// A 3-wide, 10-row stash showing rows 4 to 6 at `(300, 50)`.
const STASH: CellGrid = CellGrid {
    origin: Vec2::new(300.0, 50.0),
    cell: Vec2::splat(10.0),
    columns: 3,
    rows: 10,
    id_base: 0x3000,
    window: Some(GridWindow {
        first: UVec2::new(0, 4),
        size: UVec2::new(3, 3),
    }),
};

/// **A scrolled grid hits and draws in content cells, inside its window
/// only**: the window's first cell is drawn at the origin, and a point
/// below the window is over nothing though the grid goes on there.
#[test]
fn a_scrolled_grid_hits_content_cells_inside_its_window_only() {
    assert_eq!(
        STASH.cell_at(Vec2::new(305.0, 55.0)),
        Some(UVec2::new(0, 4))
    );
    assert_eq!(
        STASH.cell_at(Vec2::new(325.0, 75.0)),
        Some(UVec2::new(2, 6))
    );
    assert_eq!(
        STASH.cell_at(Vec2::new(305.0, 85.0)),
        None,
        "row 7 is scrolled off"
    );
    assert_eq!(
        STASH.cell_at(Vec2::new(305.0, 45.0)),
        None,
        "row 3 is above the window"
    );
    assert_eq!(
        STASH.cell_bounds(UVec2::new(1, 5)),
        (Vec2::new(310.0, 60.0), Vec2::new(320.0, 70.0))
    );
    assert_eq!(
        STASH.cell_bounds(UVec2::new(0, 2)).0,
        Vec2::new(300.0, 30.0),
        "a cell above the window, where it would be drawn"
    );
    let shown: Vec<UVec2> = STASH.visible_cells().collect();
    assert_eq!(shown.len(), 9);
    assert!(
        shown
            .iter()
            .all(|&cell| STASH.cell_at(centre(&STASH, cell)) == Some(cell))
    );
    assert!(!STASH.shows(UVec2::new(0, 3)) && !STASH.shows(UVec2::new(0, 7)));
}

/// **An item partly scrolled off is grabbed by the part that shows**: a
/// `1×3` anchored on row 3, above the window, is pressed on row 4, and its
/// grab and drop stay in content cells.
#[test]
fn an_item_partly_scrolled_off_is_grabbed_by_its_visible_part() {
    let anchor = UVec2::new(0, 3);
    let tall = |cell: UVec2| {
        (cell.x == 0 && (3..6).contains(&cell.y)).then_some(Grip {
            payload: ITEM,
            origin: anchor,
        })
    };
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let mut dropped = None;
    for pointer in [
        press(centre(&STASH, UVec2::new(0, 4))),
        press(centre(&STASH, UVec2::new(2, 5))),
        release(centre(&STASH, UVec2::new(2, 5))),
    ] {
        let mut frame = drag.frame(&mut ui, pointer);
        frame.grid(&STASH, tall, |_, _| true);
        dropped = dropped.or(frame.finish());
    }
    let dropped = dropped.expect("the drop landed");
    assert_eq!(dropped.from.cell, UVec2::new(0, 4));
    assert_eq!(dropped.to.at.cell, UVec2::new(2, 5));
    assert_eq!(dropped.to.origin, UVec2::new(2, 4), "grabbed one row down");
}

/// **A drag held while the grid scrolls under it follows the content**:
/// the pointer stays put, the window moves down a row, and the cell under
/// the pointer — and the drop — is the next row of the grid.
#[test]
fn a_held_drag_follows_the_grid_as_it_scrolls_under_the_pointer() {
    let item = |cell: UVec2| {
        (cell == UVec2::new(1, 4)).then_some(Grip {
            payload: ITEM,
            origin: cell,
        })
    };
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let at = centre(&STASH, UVec2::new(0, 5));
    let mut scrolled = STASH;
    let mut frames = [
        (STASH, press(centre(&STASH, UVec2::new(1, 4)))),
        (STASH, press(at)),
        (scrolled, press(at)),
        (scrolled, release(at)),
    ];
    scrolled.window = Some(GridWindow {
        first: UVec2::new(0, 5),
        size: UVec2::new(3, 3),
    });
    frames[2].0 = scrolled;
    frames[3].0 = scrolled;
    let mut dropped = None;
    for (grid, pointer) in frames {
        let mut frame = drag.frame(&mut ui, pointer);
        let response = frame.grid(&grid, item, |_, _| true);
        if grid == scrolled {
            assert_eq!(response.hovered, Some(UVec2::new(0, 6)));
        }
        dropped = dropped.or(frame.finish());
    }
    let dropped = dropped.expect("the drop landed");
    assert_eq!(dropped.to.at.cell, UVec2::new(0, 6));
}
