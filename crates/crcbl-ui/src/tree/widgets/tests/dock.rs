//! Dock layouts: the value's own editing, a nested layout's geometry, a drag
//! written back into the value, the clamping a nested divider keeps, and a
//! layout saved and restored into a tree that never saw it.

use super::*;
use crate::tree::{DockLayout, DockSide, Response, SplitAxis};

/// How wide the dock is, in pixels.
const WIDTH: f32 = 240.0;
/// How tall it is.
const HEIGHT: f32 = 120.0;
/// Each pane's minimum length along its split.
const MIN: [f32; 2] = [30.0, 40.0];
/// `default.css`'s divider thickness.
const DIVIDER: f32 = 4.0;

/// What one frame of the dock page built.
struct DockPage {
    dock: Response,
    /// The block each pane's builder made, by the pane's name, in build order.
    panes: Vec<(String, NodeKey)>,
}

impl DockPage {
    /// The key of the block the pane called `name` built.
    fn pane(&self, name: &str) -> NodeKey {
        self.panes
            .iter()
            .find(|(each, _)| each == name)
            .map(|(_, key)| *key)
            .unwrap_or_else(|| panic!("{name} was not built"))
    }

    /// Every pane's name, in build order.
    fn names(&self) -> Vec<&str> {
        self.panes.iter().map(|(name, _)| name.as_str()).collect()
    }
}

fn dock_page(
    ui: &mut Ui,
    layout: &mut DockLayout,
    pointer: PointerInput,
    nav: NavInput,
) -> DockPage {
    frame(ui, pointer, nav, |ui| {
        let mut panes = Vec::new();
        let mut dock = None;
        ui.block(
            "",
            &[
                Declaration::Width(LengthAuto::Px(WIDTH)),
                Declaration::Height(LengthAuto::Px(HEIGHT)),
            ],
            |ui| {
                dock = Some(ui.dock("#docks", layout, MIN, |ui, name| {
                    let key = ui.block("", &[], |_| {}).key;
                    panes.push((name.to_owned(), key));
                }));
            },
        );
        DockPage {
            dock: dock.expect("the dock is built inside the page"),
            panes,
        }
    })
}

/// The three-pane layout the geometry tests use:
///
/// ```text
/// ┌────────────┬──────────────┐
/// │            │   viewport   │
/// │  outliner  ├──────────────┤
/// │            │   console    │
/// └────────────┴──────────────┘
/// ```
fn three_panes() -> DockLayout {
    DockLayout::split(
        SplitAxis::Row,
        DockLayout::pane("outliner"),
        DockLayout::split(
            SplitAxis::Column,
            DockLayout::pane("viewport"),
            DockLayout::pane("console"),
        ),
    )
}

/// The pane block that holds `inner`: the `.dock-pane` its builder built into.
fn pane_of(ui: &Ui, inner: NodeKey) -> NodeKey {
    let index = ui
        .nodes
        .iter()
        .position(|node| node.key == inner)
        .expect("built this frame");
    let parent = ui.nodes[index].parent.expect("a pane holds it");
    ui.nodes[parent].key
}

fn width(ui: &Ui, key: NodeKey) -> f32 {
    let (min, max) = rect(ui, key);
    max.x - min.x
}

fn height(ui: &Ui, key: NodeKey) -> f32 {
    let (min, max) = rect(ui, key);
    max.y - min.y
}

/// **The layout value edits as a tree of splits**: a pane comes out by
/// collapsing the split that held it into its sibling, docks back beside
/// another on any side, and every refused edit changes nothing at all.
#[test]
fn the_layout_value_removes_docks_and_moves_panes() {
    let mut layout = three_panes();
    assert_eq!(layout.panes(), ["outliner", "viewport", "console"]);
    assert!(layout.holds("console") && !layout.holds("assets"));

    let mut without = layout.clone();
    assert!(without.remove_pane("viewport"));
    assert_eq!(
        without,
        DockLayout::split(
            SplitAxis::Row,
            DockLayout::pane("outliner"),
            DockLayout::pane("console"),
        ),
        "the split that held it did not collapse into its sibling"
    );
    assert!(!without.remove_pane("viewport"), "removed twice");

    let mut only = DockLayout::pane("solo");
    assert!(!only.remove_pane("solo"), "the layout was emptied");
    assert_eq!(only, DockLayout::pane("solo"));

    let mut docked = layout.clone();
    assert!(docked.dock("assets", "console", DockSide::Bottom));
    assert_eq!(
        docked.panes(),
        ["outliner", "viewport", "console", "assets"]
    );
    assert!(!docked.dock("assets", "console", DockSide::Bottom), "twice");
    assert!(
        !docked.dock("new", "nowhere", DockSide::Left),
        "no such pane"
    );
    assert!(!docked.dock("console", "console", DockSide::Left), "itself");

    // A move is the two together, and a refused one leaves the value alone.
    let before = layout.clone();
    assert!(!layout.move_pane("console", "nowhere", DockSide::Left));
    assert_eq!(layout, before, "a refused move changed the layout");
    assert!(layout.move_pane("console", "outliner", DockSide::Left));
    assert_eq!(
        layout,
        DockLayout::split(
            SplitAxis::Row,
            DockLayout::split(
                SplitAxis::Row,
                DockLayout::pane("console"),
                DockLayout::pane("outliner"),
            ),
            DockLayout::pane("viewport"),
        ),
        "the pane did not move to the left of the outliner"
    );
}

/// **A nested layout lays its panes out where its splits say**: every pane in
/// the value is built once, each inside its own `.dock-pane`, and the nested
/// column split divides the right-hand side of the row split.
#[test]
fn a_nested_layout_builds_every_pane_where_its_splits_say() {
    let mut ui = Ui::new();
    let mut layout = three_panes();
    dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let page = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    assert_eq!(page.names(), ["outliner", "viewport", "console"]);

    let outliner = pane_of(&ui, page.pane("outliner"));
    let viewport = pane_of(&ui, page.pane("viewport"));
    let console = pane_of(&ui, page.pane("console"));

    let shared = (WIDTH - DIVIDER) / 2.0;
    assert_eq!(width(&ui, outliner), shared, "the row split is not even");
    assert_eq!(width(&ui, viewport), shared);
    assert_eq!(width(&ui, console), shared);
    assert_eq!(height(&ui, outliner), HEIGHT, "the left pane is not full");
    assert_eq!(
        height(&ui, viewport),
        (HEIGHT - DIVIDER) / 2.0,
        "the column split is not even"
    );
    assert_eq!(rect(&ui, viewport).1.y + DIVIDER, rect(&ui, console).0.y);
    assert_eq!(rect(&ui, outliner).1.x + DIVIDER, rect(&ui, viewport).0.x);
}

/// **Dragging a divider moves both panes and writes the new position into the
/// layout value**, which is what makes the layout the thing a save writes; and
/// a nested divider still clamps to both panes' minimums.
#[test]
fn a_drag_moves_both_panes_writes_the_value_and_clamps() {
    let mut ui = Ui::new();
    let mut layout = three_panes();
    dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let page = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let DockLayout::Split { position, .. } = &layout else {
        panic!("the root is a split");
    };
    assert_eq!(*position, None, "a still layout carries a position");

    let outer = pane_of(&ui, page.pane("outliner"));
    let right = pane_of(&ui, page.pane("viewport"));
    let grip = centre(&ui, outer) + Vec2::new(width(&ui, outer) / 2.0 + DIVIDER / 2.0, 0.0);
    let before = (width(&ui, outer), width(&ui, right));

    dock_page(&mut ui, &mut layout, press(grip), NavInput::default());
    let dragged = dock_page(
        &mut ui,
        &mut layout,
        press(grip + Vec2::new(-40.0, 0.0)),
        NavInput::default(),
    );
    assert!(dragged.dock.changed, "the drag was not reported");
    let laid = dock_page(
        &mut ui,
        &mut layout,
        release(grip + Vec2::new(-40.0, 0.0)),
        NavInput::default(),
    );
    assert_eq!(
        width(&ui, pane_of(&ui, laid.pane("outliner"))),
        before.0 - 40.0,
        "the first pane did not follow the drag"
    );
    assert_eq!(
        width(&ui, pane_of(&ui, laid.pane("viewport"))),
        before.1 + 40.0,
        "the second pane did not follow the drag"
    );
    let DockLayout::Split { position, .. } = &layout else {
        panic!("the root is a split");
    };
    assert_eq!(
        *position,
        Some(before.0 - 40.0),
        "the drag was not written into the layout"
    );

    // The nested divider clamps to the minimums of the two panes it divides.
    // Its own position lives in the nested `Split`, not the root's.
    // The grip moves with the divider, so each drag takes it from the frame it
    // starts on.
    let drag = |ui: &mut Ui, layout: &mut DockLayout, by: f32| {
        let settled = dock_page(ui, layout, idle(), NavInput::default());
        let viewport = pane_of(ui, settled.pane("viewport"));
        let grip =
            centre(ui, viewport) + Vec2::new(0.0, height(ui, viewport) / 2.0 + DIVIDER / 2.0);
        let to = grip + Vec2::new(0.0, by);
        dock_page(ui, layout, press(grip), NavInput::default());
        dock_page(ui, layout, press(to), NavInput::default());
        dock_page(ui, layout, release(to), NavInput::default())
    };
    let down = drag(&mut ui, &mut layout, 500.0);
    assert_eq!(
        height(&ui, pane_of(&ui, down.pane("console"))),
        MIN[1],
        "the second pane of the nested split went under its minimum"
    );
    let up = drag(&mut ui, &mut layout, -500.0);
    assert_eq!(
        height(&ui, pane_of(&ui, up.pane("viewport"))),
        MIN[0],
        "the first pane of the nested split went under its minimum"
    );
    let DockLayout::Split { second, .. } = &layout else {
        panic!("the root is a split");
    };
    let DockLayout::Split { position, .. } = &**second else {
        panic!("the right-hand side is a split");
    };
    assert_eq!(
        *position,
        Some(MIN[0]),
        "the nested divider's clamped position is not in the layout"
    );
}

/// **A layout saved and restored lays out exactly as it did**: the value
/// carries every divider, so a tree that has never seen the layout — a fresh
/// `Ui`, as a restart is — puts every pane where the saved one had it.
#[test]
fn a_saved_layout_restores_into_a_tree_that_never_saw_it() {
    let mut ui = Ui::new();
    let mut layout = three_panes();
    dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let page = dock_page(&mut ui, &mut layout, idle(), NavInput::default());

    // Move both dividers, so neither is where an even share would put it.
    let outer = pane_of(&ui, page.pane("outliner"));
    let grip = centre(&ui, outer) + Vec2::new(width(&ui, outer) / 2.0 + DIVIDER / 2.0, 0.0);
    for offset in [Vec2::ZERO, Vec2::new(-52.0, 0.0), Vec2::new(-52.0, 0.0)] {
        dock_page(
            &mut ui,
            &mut layout,
            press(grip + offset),
            NavInput::default(),
        );
    }
    let moved = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let viewport = pane_of(&ui, moved.pane("viewport"));
    let inner = centre(&ui, viewport) + Vec2::new(0.0, height(&ui, viewport) / 2.0 + DIVIDER / 2.0);
    for offset in [Vec2::ZERO, Vec2::new(0.0, -21.0), Vec2::new(0.0, -21.0)] {
        dock_page(
            &mut ui,
            &mut layout,
            press(inner + offset),
            NavInput::default(),
        );
    }
    let shown = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let want: Vec<(Vec2, Vec2)> = shown
        .names()
        .iter()
        .map(|name| rect(&ui, pane_of(&ui, shown.pane(name))))
        .collect();
    let even = (WIDTH - DIVIDER) / 2.0;
    assert!(
        (want[0].1.x - want[0].0.x - even).abs() > 1.0,
        "the dividers never moved, so restoring proves nothing"
    );

    // The save: the value, and only the value.
    let saved = layout.clone();
    drop(ui);

    let mut restored = saved.clone();
    let mut fresh = Ui::new();
    dock_page(&mut fresh, &mut restored, idle(), NavInput::default());
    let after = dock_page(&mut fresh, &mut restored, idle(), NavInput::default());
    let got: Vec<(Vec2, Vec2)> = after
        .names()
        .iter()
        .map(|name| rect(&fresh, pane_of(&fresh, after.pane(name))))
        .collect();
    assert_eq!(after.names(), shown.names());
    assert_eq!(got, want, "the restored layout is not the saved one");
    assert_eq!(restored, saved, "showing a layout changed it");
}

/// **A pane moved between docks takes its content with it and nothing else
/// moves**: the layout is edited as a value and the next frame builds it.
#[test]
fn moving_a_pane_between_docks_rebuilds_it_where_the_value_says() {
    let mut ui = Ui::new();
    let mut layout = three_panes();
    dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let before = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let console = pane_of(&ui, before.pane("console"));
    assert!(
        rect(&ui, console).0.x > rect(&ui, pane_of(&ui, before.pane("outliner"))).1.x,
        "the console did not start on the right"
    );

    assert!(layout.move_pane("console", "outliner", DockSide::Bottom));
    dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    let after = dock_page(&mut ui, &mut layout, idle(), NavInput::default());
    assert_eq!(after.names(), ["outliner", "console", "viewport"]);
    let outliner = pane_of(&ui, after.pane("outliner"));
    let console = pane_of(&ui, after.pane("console"));
    let viewport = pane_of(&ui, after.pane("viewport"));
    assert_eq!(
        rect(&ui, outliner).1.y + DIVIDER,
        rect(&ui, console).0.y,
        "the console is not under the outliner"
    );
    assert_eq!(
        height(&ui, viewport),
        HEIGHT,
        "the viewport did not take the whole right side"
    );
}
