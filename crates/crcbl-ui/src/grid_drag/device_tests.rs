//! The drag's other hands — the pad's and the keyboard's carry cursor, the
//! finger's long press — and what every hand shares: the ghost, and the
//! quick actions asked of a focused cell.

use super::tests::{GRID, ITEM, ORIGIN, OTHER, centre, press, release, source};
use super::*;
use crcbl_core::input::TouchPhase;

/// What one frame over [`GRID`] and [`OTHER`] answered.
struct Frame {
    grid: GridResponse,
    other: GridResponse,
    ghost: Option<Ghost>,
    quick: Option<QuickMove<&'static str>>,
    ended: Option<Released<&'static str>>,
}

/// One frame of `input` over [`GRID`] and then [`OTHER`], each accepting
/// whatever it is asked about when `accept` says so; [`OTHER`] holds
/// nothing.
fn run(
    drag: &mut GridDrag<&'static str>,
    ui: &mut UiState,
    input: DragInput,
    accept: bool,
) -> Frame {
    let mut frame = drag.frame_with(ui, input);
    let grid = frame.grid(&GRID, source, |_, _| accept);
    let other = frame.grid(&OTHER, |_| None, |_, _| accept);
    let ghost = frame.ghost();
    let quick = frame.take_quick_move();
    let ended = frame.release();
    Frame {
        grid,
        other,
        ghost,
        quick,
        ended,
    }
}

/// The pad's input: `nav`, from a navigating device.
fn pad(nav: NavInput) -> DragInput {
    DragInput {
        nav,
        ..DragInput::default()
    }
}

/// A frame of nothing but `dt` passing.
fn waiting(dt: Duration) -> DragInput {
    DragInput {
        dt,
        ..DragInput::default()
    }
}

fn on_grid(cell: UVec2) -> GridCell {
    GridCell {
        grid: GRID.id_base,
        cell,
    }
}

/// A drag with [`GRID`] and [`OTHER`] run once, so the pad has grids to
/// land on, and with focus landed on [`GRID`]'s first cell.
fn focused() -> (GridDrag<&'static str>, UiState) {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    run(&mut drag, &mut ui, DragInput::default(), true);
    run(&mut drag, &mut ui, pad(NavInput::NAVIGATION), true);
    let landed = run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Down)),
        true,
    );
    assert_eq!(
        drag.focus(),
        Some(on_grid(UVec2::ZERO)),
        "focus did not land"
    );
    assert!(
        landed.grid.cell(UVec2::ZERO).focused,
        "the landing is not shown"
    );
    (drag, ui)
}

/// Steps from [`GRID`]'s first cell to `cell`, right then down.
fn walk_to(drag: &mut GridDrag<&'static str>, ui: &mut UiState, cell: UVec2) {
    let moves = [(Direction::Right, cell.x), (Direction::Down, cell.y)];
    for (direction, steps) in moves {
        for _ in 0..steps {
            run(drag, ui, pad(NavInput::toward(direction)), true);
        }
    }
    assert_eq!(drag.focus(), Some(on_grid(cell)));
}

/// **The pad picks up with accept on the focused cell, carries with moves,
/// and drops with accept**, reported as the same [`Released`] the pointer's
/// drop is. The accept that picked up does not also drop.
#[test]
fn the_pad_picks_up_moves_the_carry_cursor_and_drops_where_it_is() {
    let (mut drag, mut ui) = focused();
    let grabbed = ORIGIN + UVec2::new(1, 1);
    walk_to(&mut drag, &mut ui, grabbed);

    let picked = run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true);
    assert_eq!(picked.ended, None, "the accept that picked up dropped too");
    let held = drag.held().expect("accept on a focused item picked it up");
    assert_eq!(held.hand(), Hand::Cursor);
    assert_eq!(held.from(), on_grid(grabbed));
    assert_eq!(held.grab(), UVec2::ONE, "held by its bottom-right cell");

    let to = UVec2::new(3, 2);
    let moved = run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Right)),
        true,
    );
    let moved_on = run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Down)),
        true,
    );
    assert_eq!(drag.focus(), Some(on_grid(to)), "the cursor did not move");
    assert_eq!(
        moved.grid.cell(UVec2::new(3, 1)).drop,
        DropFeedback::Accepting
    );
    assert_eq!(moved_on.grid.cell(to).drop, DropFeedback::Accepting);
    assert!(
        moved_on.grid.cell(to).focused,
        "the carry cursor is not shown"
    );
    assert!(drag.held().is_some(), "a move let go");

    let dropped = run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true)
        .ended
        .expect("accept on the cursor ends the drag");
    assert!(dropped.accepted);
    assert_eq!(dropped.payload, ITEM);
    assert_eq!(dropped.from, on_grid(grabbed));
    assert_eq!(
        dropped.target,
        Some(DropTarget {
            at: on_grid(to),
            origin: to - UVec2::ONE,
        }),
        "the grab offset was not kept",
    );
    assert!(drag.held().is_none());
}

/// **Back cancels the pad's drag to where it started**: reported over
/// nothing, with no target, though the cell under the cursor would have
/// taken it — and though accept was pressed with it, which back wins over.
#[test]
fn back_cancels_the_pads_drag_to_its_origin() {
    let (mut drag, mut ui) = focused();
    walk_to(&mut drag, &mut ui, ORIGIN);
    run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true);
    for _ in 0..2 {
        run(
            &mut drag,
            &mut ui,
            pad(NavInput::toward(Direction::Down)),
            true,
        );
    }
    let accepting = run(&mut drag, &mut ui, pad(NavInput::NAVIGATION), true);
    assert_eq!(
        accepting.grid.cell(UVec2::new(1, 2)).drop,
        DropFeedback::Accepting,
        "the control: the cursor is over a cell that would take it",
    );

    let cancelled = run(&mut drag, &mut ui, pad(NavInput::BACK), true)
        .ended
        .expect("back ends the drag");
    assert_eq!(
        (cancelled.over, cancelled.target, cancelled.accepted),
        (None, None, false),
        "a cancel reported a drop",
    );
    assert_eq!(cancelled.from, on_grid(ORIGIN));
    assert!(drag.held().is_none(), "back kept the drag");
    assert_eq!(
        drag.focus(),
        Some(on_grid(UVec2::new(1, 2))),
        "back moved focus"
    );

    // Picked up again where it is, and cancelled with accept pressed as well:
    // back wins.
    let anywhere = |cell: UVec2| {
        Some(Grip {
            payload: ITEM,
            origin: cell,
        })
    };
    let mut frame = drag.frame_with(&mut ui, pad(NavInput::ACCEPT));
    frame.grid(&GRID, anywhere, |_, _| true);
    frame.finish();
    assert!(drag.held().is_some(), "the second pick-up");
    let mut frame = drag.frame_with(&mut ui, pad(NavInput::toward(Direction::Left)));
    let moved = frame.grid(&GRID, anywhere, |_, _| true);
    frame.finish();
    assert_eq!(
        moved.cell(UVec2::new(0, 2)).drop,
        DropFeedback::Accepting,
        "the control: off its origin, over a cell that takes it",
    );
    let both = NavInput {
        accept: true,
        ..NavInput::BACK
    };
    let mut frame = drag.frame_with(&mut ui, pad(both));
    frame.grid(&GRID, anywhere, |_, _| true);
    let ended = frame.release().expect("back and accept end the drag");
    assert_eq!(
        (ended.target, ended.accepted),
        (None, false),
        "an accept pressed with back dropped",
    );
}

/// **The press that lands focus does nothing further**: with nothing
/// focused, an accept lands focus on the first grid's first cell and picks
/// nothing up there, though that cell holds something.
#[test]
fn the_press_that_lands_focus_picks_nothing_up() {
    let anywhere = |cell: UVec2| {
        Some(Grip {
            payload: ITEM,
            origin: cell,
        })
    };
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    for input in [DragInput::default(), pad(NavInput::ACCEPT)] {
        let mut frame = drag.frame_with(&mut ui, input);
        frame.grid(&GRID, anywhere, |_, _| true);
        frame.finish();
    }
    assert_eq!(drag.focus(), Some(on_grid(UVec2::ZERO)));
    assert!(
        drag.held().is_none(),
        "the landing press picked something up"
    );

    // The control: the next accept, on the cell focus landed on, does.
    let mut frame = drag.frame_with(&mut ui, pad(NavInput::ACCEPT));
    frame.grid(&GRID, anywhere, |_, _| true);
    frame.finish();
    assert!(
        drag.held().is_some(),
        "an accept on a held cell lifted nothing"
    );
}

/// **A step off a grid's edge goes to the grid linked that way**, on the
/// nearest row, and a step off an unlinked edge stays put. A drag carried
/// across lands on the other grid.
#[test]
fn a_step_off_an_edge_crosses_to_the_linked_grid_and_the_drag_with_it() {
    let (mut drag, mut ui) = focused();
    drag.link(&GRID, Direction::Right, &OTHER);
    drag.link(&OTHER, Direction::Left, &GRID);

    walk_to(&mut drag, &mut ui, UVec2::new(3, 2));
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Right)),
        true,
    );
    assert_eq!(
        drag.focus(),
        Some(GridCell {
            grid: OTHER.id_base,
            cell: UVec2::new(0, 1),
        }),
        "the step right of the last column did not reach the linked grid's nearest row",
    );
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Right)),
        true,
    );
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Down)),
        true,
    );
    assert_eq!(
        drag.focus().map(|focus| focus.cell),
        Some(UVec2::new(1, 1)),
        "the other grid's unlinked edges let focus off it",
    );
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Left)),
        true,
    );
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Left)),
        true,
    );
    assert_eq!(
        drag.focus(),
        Some(on_grid(UVec2::new(3, 1))),
        "the link back"
    );

    // Carried across: the item, taken by its origin, onto the other grid.
    walk_back_to(&mut drag, &mut ui, ORIGIN);
    run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true);
    for _ in 0..3 {
        run(
            &mut drag,
            &mut ui,
            pad(NavInput::toward(Direction::Right)),
            true,
        );
    }
    let crossed = run(&mut drag, &mut ui, pad(NavInput::NAVIGATION), true);
    assert_eq!(
        crossed.other.cell(UVec2::ZERO).drop,
        DropFeedback::Accepting
    );
    let dropped = run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true)
        .ended
        .expect("the drop");
    assert_eq!(
        dropped.target.map(|target| target.at),
        Some(GridCell {
            grid: OTHER.id_base,
            cell: UVec2::ZERO,
        }),
    );
}

/// Steps left and up from wherever focus is on [`GRID`] to `cell`.
fn walk_back_to(drag: &mut GridDrag<&'static str>, ui: &mut UiState, cell: UVec2) {
    let at = drag.focus().expect("focused").cell;
    let moves = [
        (Direction::Left, at.x - cell.x),
        (Direction::Up, at.y - cell.y),
    ];
    for (direction, steps) in moves {
        for _ in 0..steps {
            run(drag, ui, pad(NavInput::toward(direction)), true);
        }
    }
    assert_eq!(drag.focus(), Some(on_grid(cell)));
}

/// **Focus is shown only while the pad or the keyboard is driving**, and a
/// pointer press focuses what it pressed, so the pad carries on from there.
#[test]
fn focus_is_shown_in_navigation_mode_and_follows_a_pointer_press() {
    let (mut drag, mut ui) = focused();
    let shown = run(&mut drag, &mut ui, pad(NavInput::NAVIGATION), true);
    assert!(shown.grid.cell(UVec2::ZERO).focused);
    let hidden = run(&mut drag, &mut ui, DragInput::default(), true);
    assert!(
        !hidden.grid.cell(UVec2::ZERO).focused,
        "focus was shown to a pointer",
    );

    let pressed = UVec2::new(2, 2);
    let input = DragInput {
        pointer: press(centre(&GRID, pressed)),
        ..DragInput::default()
    };
    run(&mut drag, &mut ui, input, true);
    assert_eq!(
        drag.focus(),
        Some(on_grid(pressed)),
        "a click did not focus"
    );
}

/// **A long press under the threshold lifts nothing**, and the finger let
/// go then is given up.
#[test]
fn a_long_press_short_of_the_threshold_lifts_nothing() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let finger = ContactId(7);
    let at = centre(&GRID, ORIGIN);
    assert!(
        drag.touch(finger, TouchPhase::Began, at),
        "the finger was refused"
    );
    let short = LONG_PRESS - Duration::from_millis(1);
    let pressed = run(&mut drag, &mut ui, waiting(short), true);
    assert!(
        drag.held().is_none(),
        "a press short of the threshold lifted"
    );
    assert_eq!(pressed.ghost, None);

    assert!(drag.touch(finger, TouchPhase::Ended, at));
    run(&mut drag, &mut ui, waiting(LONG_PRESS), true);
    assert!(
        drag.held().is_none(),
        "a tap lifted something once it was over"
    );
}

/// **At the threshold the finger lifts what is under it**, carries it as it
/// moves, and drops it where it lifts — with the grab offset kept.
#[test]
fn a_long_press_at_the_threshold_lifts_then_drags_and_drops() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let finger = ContactId(3);
    let grabbed = ORIGIN + UVec2::new(1, 0);
    assert!(drag.touch(finger, TouchPhase::Began, centre(&GRID, grabbed)));
    run(&mut drag, &mut ui, waiting(LONG_PRESS / 2), true);
    assert!(drag.held().is_none(), "lifted at half the threshold");
    run(&mut drag, &mut ui, waiting(LONG_PRESS / 2), true);
    let held = drag.held().expect("the threshold lifted the item");
    assert_eq!(held.hand(), Hand::Touch(finger));
    assert_eq!(held.grab(), UVec2::new(1, 0));

    let to = UVec2::new(3, 2);
    assert!(drag.touch(finger, TouchPhase::Moved, centre(&GRID, to)));
    let carried = run(&mut drag, &mut ui, waiting(Duration::ZERO), true);
    assert_eq!(carried.grid.cell(to).drop, DropFeedback::Accepting);
    assert!(drag.touch(finger, TouchPhase::Ended, centre(&GRID, to)));
    let dropped = run(&mut drag, &mut ui, waiting(Duration::ZERO), true)
        .ended
        .expect("lifting the finger ends the drag");
    assert!(dropped.accepted);
    assert_eq!(
        dropped.target.map(|target| target.origin),
        Some(UVec2::new(2, 2))
    );
    assert!(drag.held().is_none());
    assert!(
        drag.touch(ContactId(4), TouchPhase::Began, Vec2::ZERO),
        "the drag kept following a finger that had lifted",
    );
}

/// **A finger that wanders past the slop before the threshold is given up**
/// — it was a scroll or a swipe — and one that is cancelled while carrying
/// takes the drag back to where it started.
#[test]
fn a_wandering_finger_is_given_up_and_a_cancelled_one_cancels() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let finger = ContactId(1);
    let at = centre(&GRID, ORIGIN);
    drag.touch(finger, TouchPhase::Began, at);
    let near = at + Vec2::new(LONG_PRESS_SLOP, 0.0);
    assert!(
        drag.touch(finger, TouchPhase::Moved, near),
        "within the slop"
    );
    let far = at + Vec2::new(LONG_PRESS_SLOP + 1.0, 0.0);
    assert!(
        !drag.touch(finger, TouchPhase::Moved, far),
        "kept past the slop"
    );
    run(&mut drag, &mut ui, waiting(LONG_PRESS), true);
    assert!(drag.held().is_none(), "a swipe lifted something");

    drag.touch(finger, TouchPhase::Began, at);
    run(&mut drag, &mut ui, waiting(LONG_PRESS), true);
    assert!(drag.held().is_some());
    drag.touch(finger, TouchPhase::Moved, centre(&GRID, UVec2::new(3, 2)));
    drag.touch(
        finger,
        TouchPhase::Cancelled,
        centre(&GRID, UVec2::new(3, 2)),
    );
    let cancelled = run(&mut drag, &mut ui, waiting(Duration::ZERO), true)
        .ended
        .expect("a cancelled contact ends the drag");
    assert_eq!(
        (cancelled.over, cancelled.target, cancelled.accepted),
        (None, None, false),
        "a cancelled contact dropped",
    );
}

/// **The ghost follows the pointer, grab offset kept and tinted by the cell
/// under it**, keeps the source grid's cell size over nothing, and is gone
/// on the frame the drag ends.
#[test]
fn the_ghost_follows_the_pointer() {
    let mut drag = GridDrag::new();
    let mut ui = UiState::new();
    let grabbed = ORIGIN + UVec2::ONE;
    let pointer = |pointer| DragInput {
        pointer,
        ..DragInput::default()
    };
    let taken = run(
        &mut drag,
        &mut ui,
        pointer(press(centre(&GRID, grabbed))),
        true,
    );
    let ghost = taken.ghost.expect("a ghost from the frame it was taken");
    assert_eq!(
        ghost.origin,
        GRID.cell_bounds(ORIGIN).0,
        "it jumped when taken"
    );

    let at = centre(&GRID, UVec2::new(3, 2)) + Vec2::new(2.0, -3.0);
    for accept in [true, false] {
        let ghost = run(&mut drag, &mut ui, pointer(press(at)), accept)
            .ghost
            .expect("a ghost mid-drag");
        assert_eq!(
            ghost.origin,
            at - 1.5 * GRID.cell,
            "the ghost left the grab"
        );
        assert_eq!(ghost.cell, GRID.cell);
        let tint = if accept {
            DropFeedback::Accepting
        } else {
            DropFeedback::Refusing
        };
        assert_eq!(ghost.drop, tint);
    }

    let nowhere = Vec2::new(10.0, 10.0);
    let ghost = run(&mut drag, &mut ui, pointer(press(nowhere)), true)
        .ghost
        .expect("a ghost over nothing");
    assert_eq!(
        (ghost.origin, ghost.cell, ghost.drop),
        (nowhere - 1.5 * GRID.cell, GRID.cell, DropFeedback::None),
    );
    let ended = run(&mut drag, &mut ui, pointer(release(nowhere)), true);
    assert_eq!(ended.ghost, None, "a ghost on the frame the drag ended");
}

/// **The ghost snaps to the carry cursor's cell** on the pad, grab offset
/// kept, and is tinted by the cursor's cell.
#[test]
fn the_ghost_follows_the_carry_cursor() {
    let (mut drag, mut ui) = focused();
    let grabbed = ORIGIN + UVec2::new(0, 1);
    walk_to(&mut drag, &mut ui, grabbed);
    run(&mut drag, &mut ui, pad(NavInput::ACCEPT), true);
    let cursor = UVec2::new(2, 2);
    run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Right)),
        true,
    );
    let moved = run(
        &mut drag,
        &mut ui,
        pad(NavInput::toward(Direction::Down)),
        false,
    );
    let ghost = moved.ghost.expect("a ghost on the cursor");
    assert_eq!(
        ghost.origin,
        GRID.cell_bounds(cursor - UVec2::new(0, 1)).0,
        "the ghost is not on the cursor's cell less the grab",
    );
    assert_eq!(ghost.drop, DropFeedback::Refusing);
}

/// **A quick action reports the focused cell's payload without a drag**,
/// and asks nothing of an empty cell.
#[test]
fn a_quick_action_reports_the_focused_payload_without_a_drag() {
    let (mut drag, mut ui) = focused();
    let send = QuickAction(3);
    let quick = |action| DragInput {
        nav: NavInput::NAVIGATION,
        quick: Some(action),
        ..DragInput::default()
    };
    let empty = run(&mut drag, &mut ui, quick(send), true);
    assert_eq!(empty.quick, None, "an empty cell answered a quick action");

    walk_to(&mut drag, &mut ui, ORIGIN);
    let asked = run(&mut drag, &mut ui, quick(send), true);
    assert_eq!(
        asked.quick,
        Some(QuickMove {
            action: send,
            payload: ITEM,
            from: on_grid(ORIGIN),
        }),
    );
    assert!(drag.held().is_none(), "a quick action picked the item up");
}
