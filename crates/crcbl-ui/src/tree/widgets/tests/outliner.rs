//! The virtualized outliner: the flattened model against a known tree, a
//! bounded build whatever the tree's size, expand and collapse by pointer and
//! by key, and selection by pointer and by key.

use super::*;
use crate::tree::{
    LIST_OVERSCAN, OutlinerBuilder, OutlinerId, OutlinerOptions, OutlinerRow, OutlinerState,
    Response, SelectMode,
};

/// A row's height, in pixels.
const ROW: f32 = 20.0;
/// The outliner's content-box height: five rows.
const VIEW: f32 = 100.0;
/// The most rows one frame may build: the rows the view can show, one more for
/// a row cut by each edge, the overscan on both sides, and the focused row
/// outside it all.
const BOUND: usize = (VIEW / ROW) as usize + 1 + 2 * LIST_OVERSCAN + 1;

/// An item's id.
const fn id(item: u64) -> OutlinerId {
    OutlinerId(item)
}

/// The fixture tree, six items deep enough to have a grandchild:
///
/// ```text
/// 0 root
///   1 a          (leaf)
///   2 b          (branch)
///     3 c        (leaf)
///     4 d        (leaf)
///   5 e          (leaf)
/// ```
fn fixture(out: &mut OutlinerBuilder<'_>) {
    out.branch(id(0), |out| {
        out.leaf(id(1));
        out.branch(id(2), |out| {
            out.leaf(id(3));
            out.leaf(id(4));
        });
        out.leaf(id(5));
    });
}

/// A tree of one expanded root over `leaves` leaves, for the bound tests.
fn wide(leaves: u64) -> impl Fn(&mut OutlinerBuilder<'_>) {
    move |out| {
        out.branch(id(0), |out| {
            for leaf in 1..=leaves {
                out.leaf(id(leaf));
            }
        });
    }
}

/// What one frame of the outliner page built.
struct OutlinerPage {
    outliner: Response,
    /// Every row the row builder was called with, in order.
    built: Vec<OutlinerRow>,
    /// How many nodes the frame built, the page's own included.
    nodes: u32,
}

/// The outliner's stylesheet: a fixed height and nothing between its border box
/// and its content box, so row geometry is plain arithmetic.
fn sheet(ui: &mut Ui) {
    ui.add_stylesheet(
        "outliner.css",
        &format!("#tree {{ height: {VIEW}px; border-width: 0; padding: 0; }}"),
    );
}

fn options(select: SelectMode) -> OutlinerOptions {
    OutlinerOptions {
        row_height: ROW,
        select,
        ..OutlinerOptions::default()
    }
}

fn page(
    ui: &mut Ui,
    state: &mut OutlinerState,
    pointer: PointerInput,
    nav: NavInput,
    select: SelectMode,
    tree: impl FnOnce(&mut OutlinerBuilder<'_>),
) -> OutlinerPage {
    frame(ui, pointer, nav, |ui| {
        let mut built = Vec::new();
        let outliner = ui.outliner_with("#tree", state, &options(select), tree, |ui, row| {
            built.push(*row);
            ui.span(".label", "x", &[]);
        });
        let nodes = ui.style_stats().nodes;
        OutlinerPage {
            outliner,
            built,
            nodes,
        }
    })
}

/// The fixture page, selecting one row at a time.
fn fixture_page(
    ui: &mut Ui,
    state: &mut OutlinerState,
    pointer: PointerInput,
    nav: NavInput,
) -> OutlinerPage {
    page(ui, state, pointer, nav, SelectMode::Replace, fixture)
}

/// The key of the row for `item`, from the frame just built.
fn row_key(ui: &Ui, page: &OutlinerPage, item: OutlinerId) -> NodeKey {
    let index = page
        .built
        .iter()
        .position(|row| row.id == item)
        .unwrap_or_else(|| panic!("{item:?} was not built"));
    let content = children_of(ui, page.outliner.key)[0];
    // The rows are built in the order the builder saw them, bar a focused row
    // kept outside the window, which is built first.
    children_of(ui, content)[index]
}

/// The ids of `state`'s flattened rows, in order.
fn flat(state: &OutlinerState) -> Vec<u64> {
    state.rows().iter().map(|row| row.id.0).collect()
}

/// **The flattened model is exactly the visible rows of a known tree, at each
/// expansion**, and a collapsed branch's children are never walked — the
/// builder's own `branch` never runs their closure.
#[test]
fn the_flattened_model_is_the_visible_rows_of_a_known_tree() {
    let mut state = OutlinerState::new();
    let walked = std::cell::Cell::new(0_u32);
    let counted = |out: &mut OutlinerBuilder<'_>| {
        out.branch(id(0), |out| {
            walked.set(walked.get() + 1);
            out.leaf(id(1));
            out.branch(id(2), |out| {
                walked.set(walked.get() + 1);
                out.leaf(id(3));
                out.leaf(id(4));
            });
            out.leaf(id(5));
        });
    };

    state.flatten(counted);
    assert_eq!(flat(&state), [0], "a collapsed root is the whole model");
    assert_eq!(walked.get(), 0, "a collapsed root's children were walked");
    assert_eq!(state.rows()[0].depth, 0);
    assert!(state.rows()[0].branch && !state.rows()[0].open);

    state.set_expanded(id(0), true);
    state.flatten(counted);
    assert_eq!(flat(&state), [0, 1, 2, 5]);
    assert_eq!(walked.get(), 1, "the collapsed child was walked");
    let depths: Vec<u16> = state.rows().iter().map(|row| row.depth).collect();
    assert_eq!(depths, [0, 1, 1, 1], "depth is not the nesting");
    let parents: Vec<Option<u64>> = state
        .rows()
        .iter()
        .map(|row| row.parent.map(|parent| parent.0))
        .collect();
    assert_eq!(parents, [None, Some(0), Some(0), Some(0)]);

    state.set_expanded(id(2), true);
    state.flatten(counted);
    assert_eq!(flat(&state), [0, 1, 2, 3, 4, 5]);
    assert_eq!(walked.get(), 3, "both branches were not walked");
    let depths: Vec<u16> = state.rows().iter().map(|row| row.depth).collect();
    assert_eq!(depths, [0, 1, 1, 2, 2, 1]);

    state.set_expanded(id(0), false);
    state.flatten(counted);
    assert_eq!(
        flat(&state),
        [0],
        "collapsing the root did not hide its subtree"
    );
    assert_eq!(walked.get(), 3, "a collapsed root's children were walked");

    // The expansion under the closed root is kept, not forgotten.
    state.set_expanded(id(0), true);
    state.flatten(counted);
    assert_eq!(flat(&state), [0, 1, 2, 3, 4, 5], "the inner branch shut");
}

/// **`is_stale` is set by every expansion change and cleared by the flatten**,
/// so a still frame walks nothing at all.
#[test]
fn the_model_is_flattened_only_when_the_expansion_or_the_caller_says_so() {
    let mut state = OutlinerState::new();
    assert!(state.is_stale(), "a new state was not stale");
    state.flatten(fixture);
    assert!(!state.is_stale());

    assert!(
        state.set_expanded(id(0), true),
        "the change was not reported"
    );
    assert!(state.is_stale(), "expanding did not make the model stale");
    state.flatten(fixture);
    assert!(!state.set_expanded(id(0), true), "a no-op was reported");
    assert!(!state.is_stale(), "a no-op made the model stale");

    state.invalidate();
    assert!(state.is_stale());
}

/// **A 100 000-row outliner builds no more rows a frame than its view and
/// overscan hold, and exactly as many nodes as a 100-row one does**, while its
/// content is as tall as every row.
#[test]
fn a_hundred_thousand_row_outliner_builds_a_bounded_window() {
    let (mut long, mut short) = (Ui::new(), Ui::new());
    sheet(&mut long);
    sheet(&mut short);
    let mut many = OutlinerState::new();
    let mut few = OutlinerState::new();
    many.set_expanded(id(0), true);
    few.set_expanded(id(0), true);

    for _ in 0..3 {
        let big = page(
            &mut long,
            &mut many,
            idle(),
            NavInput::default(),
            SelectMode::Replace,
            wide(100_000),
        );
        let small = page(
            &mut short,
            &mut few,
            idle(),
            NavInput::default(),
            SelectMode::Replace,
            wide(100),
        );
        assert_eq!(many.rows().len(), 100_001, "the model is not every row");
        assert!(
            big.built.len() <= BOUND,
            "{} rows built: {:?}",
            big.built.len(),
            big.built
        );
        assert!(!big.built.is_empty(), "nothing was built");
        assert_eq!(big.nodes, small.nodes, "the node count grew with the tree");
    }

    let big = page(
        &mut long,
        &mut many,
        idle(),
        NavInput::default(),
        SelectMode::Replace,
        wide(100_000),
    );
    let content = children_of(&long, big.outliner.key)[0];
    let (min, max) = rect(&long, content);
    assert_eq!(
        max.y - min.y,
        100_001.0 * ROW,
        "the content is not every row"
    );
    assert_eq!(
        long.len(),
        short.len(),
        "the store holds more than the window"
    );
}

/// **A click on a row's toggle expands and collapses it, and the children show
/// in the same frame** — the row's own click selects instead, so the two
/// gestures never collide.
#[test]
fn a_click_on_the_toggle_expands_and_a_click_on_the_row_selects() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    let first = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    assert_eq!(flat(&state), [0]);

    let root = row_key(&ui, &first, id(0));
    let toggle = children_of(&ui, root)[0];
    let grip = centre(&ui, toggle);
    fixture_page(&mut ui, &mut state, press(grip), NavInput::default());
    let opened = fixture_page(&mut ui, &mut state, release(grip), NavInput::default());
    assert!(
        state.is_expanded(id(0)),
        "the click did not expand the root"
    );
    assert_eq!(
        opened.built.iter().map(|row| row.id.0).collect::<Vec<_>>(),
        [0, 1, 2, 5],
        "the children did not show in the frame the click landed"
    );
    assert!(opened.outliner.changed, "the expansion was not reported");
    assert_eq!(state.selected_len(), 0, "the toggle click also selected");

    // And the same click closes it again.
    fixture_page(&mut ui, &mut state, press(grip), NavInput::default());
    let shut = fixture_page(&mut ui, &mut state, release(grip), NavInput::default());
    assert!(!state.is_expanded(id(0)));
    assert_eq!(shut.built.len(), 1, "the subtree stayed built");

    // A click on the row itself, past the toggle, selects it and nothing else.
    let (min, max) = rect(&ui, root);
    let body = Vec2::new(max.x - 2.0, (min.y + max.y) * 0.5);
    fixture_page(&mut ui, &mut state, press(body), NavInput::default());
    let picked = fixture_page(&mut ui, &mut state, release(body), NavInput::default());
    assert!(state.is_selected(id(0)), "the row click did not select it");
    assert!(!state.is_expanded(id(0)), "the row click also expanded it");
    assert!(picked.outliner.changed, "the selection was not reported");
}

/// **Right expands a focused row and left collapses it, as the WAI-ARIA tree
/// view pattern says**, and accept selects it: the whole outliner is drivable
/// with no pointer at all.
#[test]
fn keys_expand_collapse_and_select_a_focused_row() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    fixture_page(&mut ui, &mut state, idle(), NavInput::NAVIGATION);
    let at_root = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    assert_eq!(
        ui.focused(),
        Some(row_key(&ui, &at_root, id(0))),
        "focus did not land on the root row"
    );

    let opened = fixture_page(&mut ui, &mut state, idle(), RIGHT);
    assert!(state.is_expanded(id(0)), "right did not expand the root");
    assert_eq!(
        opened.built.iter().map(|row| row.id.0).collect::<Vec<_>>(),
        [0, 1, 2, 5],
        "right's children did not show in the same frame"
    );

    // Right again steps into the first child, as the pattern says.
    let inside = fixture_page(&mut ui, &mut state, idle(), RIGHT);
    assert_eq!(
        ui.focused(),
        Some(row_key(&ui, &inside, id(1))),
        "right on an open row did not step to its first child"
    );

    // Left from a child goes to its parent, and left again closes it.
    let back = fixture_page(&mut ui, &mut state, idle(), LEFT);
    assert_eq!(ui.focused(), Some(row_key(&ui, &back, id(0))));
    let shut = fixture_page(&mut ui, &mut state, idle(), LEFT);
    assert!(!state.is_expanded(id(0)), "left did not collapse the root");
    assert_eq!(shut.built.len(), 1, "the subtree stayed built");

    // Accept selects the focused row.
    assert_eq!(state.selected_len(), 0);
    fixture_page(&mut ui, &mut state, idle(), NavInput::ACCEPT);
    assert!(state.is_selected(id(0)), "accept did not select the row");
}

/// **The caller's own expansion wins over the store**: collapsing a row from
/// outside, while that row holds focus, is not undone by the widget reading
/// last frame's row back — only a left or right the tree view rule answered
/// this frame moves the expansion.
#[test]
fn an_expansion_the_caller_changed_is_not_undone() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    fixture_page(&mut ui, &mut state, idle(), NavInput::NAVIGATION);
    fixture_page(&mut ui, &mut state, idle(), RIGHT);
    assert!(state.is_expanded(id(0)), "right did not expand the root");
    let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    assert_eq!(
        ui.focused(),
        Some(row_key(&ui, &built, id(0))),
        "the row the caller is about to collapse does not hold focus"
    );

    state.set_expanded(id(0), false);
    let shut = fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    assert!(!state.is_expanded(id(0)), "the widget re-expanded the row");
    assert_eq!(shut.built.len(), 1, "the subtree was built again");
}

/// **Selection follows its mode**: replace leaves one row selected, toggle adds
/// and removes, and a range takes every row between the anchor and the row it
/// lands on.
#[test]
fn the_selection_modes_replace_toggle_and_range() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    state.set_expanded(id(2), true);
    state.flatten(fixture);
    assert_eq!(flat(&state), [0, 1, 2, 3, 4, 5]);

    assert!(state.select(id(1), SelectMode::Replace));
    assert_eq!(state.selected().map(|id| id.0).collect::<Vec<_>>(), [1]);
    assert_eq!(state.anchor(), Some(id(1)));

    assert!(state.select(id(4), SelectMode::Toggle));
    assert_eq!(state.selected().map(|id| id.0).collect::<Vec<_>>(), [1, 4]);
    assert!(state.select(id(4), SelectMode::Toggle), "toggle off");
    assert_eq!(state.selected().map(|id| id.0).collect::<Vec<_>>(), [1]);
    assert_eq!(
        state.anchor(),
        Some(id(4)),
        "toggle did not move the anchor"
    );

    // Replace takes the rows already selected away with it, not just adds.
    state.select(id(3), SelectMode::Toggle);
    assert_eq!(state.selected().map(|id| id.0).collect::<Vec<_>>(), [1, 3]);
    assert!(state.select(id(5), SelectMode::Replace));
    assert_eq!(
        state.selected().map(|id| id.0).collect::<Vec<_>>(),
        [5],
        "replace left the rows that were selected before it"
    );

    state.select(id(1), SelectMode::Replace);
    assert!(state.select(id(4), SelectMode::Range));
    assert_eq!(
        state.selected().map(|id| id.0).collect::<Vec<_>>(),
        [1, 2, 3, 4],
        "the range is not the rows between the anchor and the row"
    );
    assert_eq!(state.anchor(), Some(id(1)), "a range moved the anchor");
    assert!(
        !state.select(id(4), SelectMode::Range),
        "a no-op was reported"
    );

    // The range runs backwards too, and the frame's mode is what the widget
    // applies: a click with `Range` set takes the run, not just the row.
    let mut ui_state = OutlinerState::new();
    ui_state.set_expanded(id(0), true);
    page(
        &mut ui,
        &mut ui_state,
        idle(),
        NavInput::default(),
        SelectMode::Replace,
        fixture,
    );
    let built = page(
        &mut ui,
        &mut ui_state,
        idle(),
        NavInput::default(),
        SelectMode::Replace,
        fixture,
    );
    let click = |ui: &mut Ui, state: &mut OutlinerState, at: Vec2, mode: SelectMode| {
        page(ui, state, press(at), NavInput::default(), mode, fixture);
        page(ui, state, release(at), NavInput::default(), mode, fixture);
    };
    let body = |ui: &Ui, key: NodeKey| {
        let (min, max) = rect(ui, key);
        Vec2::new(max.x - 2.0, (min.y + max.y) * 0.5)
    };
    let fifth = body(&ui, row_key(&ui, &built, id(5)));
    let second = body(&ui, row_key(&ui, &built, id(1)));
    click(&mut ui, &mut ui_state, fifth, SelectMode::Replace);
    assert_eq!(ui_state.selected().map(|id| id.0).collect::<Vec<_>>(), [5]);
    click(&mut ui, &mut ui_state, second, SelectMode::Range);
    assert_eq!(
        ui_state.selected().map(|id| id.0).collect::<Vec<_>>(),
        [1, 2, 5],
        "a backwards range did not take the rows between"
    );
}

/// **A selected row is `:checked` and an expanded one `:open`, and the
/// stylesheet paints only those** — the selected row is the only row filled in
/// the selection colour, whatever the offset shows.
#[test]
fn only_the_selected_row_is_marked() {
    let selection = linear("#2f5a9e");
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    state.select(id(2), SelectMode::Replace);
    fixture_page(&mut ui, &mut state, idle(), NavInput::default());
    let built = fixture_page(&mut ui, &mut state, idle(), NavInput::default());

    let marked: Vec<NodeKey> = built
        .built
        .iter()
        .map(|row| row_key(&ui, &built, row.id))
        .filter(|&key| style_of(&ui, key).background == selection)
        .collect();
    assert_eq!(
        marked,
        [row_key(&ui, &built, id(2))],
        "not exactly the selected row marked"
    );

    // Indentation is the depth, in `padding-left`, and nothing else.
    for row in &built.built {
        let key = row_key(&ui, &built, row.id);
        let (min, _) = rect(&ui, key);
        let (outer, _) = rect(&ui, built.outliner.key);
        let toggle = rect(&ui, children_of(&ui, key)[0]).0;
        assert_eq!(min.x, outer.x, "a row does not span the outliner");
        assert_eq!(
            toggle.x - min.x,
            f32::from(row.depth) * OutlinerOptions::default().indent,
            "{:?} is not indented by its depth",
            row.id
        );
    }
}

/// **Focus steps past the rows shown into rows that were not built**, down a
/// long outliner, and the focused row is kept when the offset moves away.
#[test]
fn focus_steps_past_the_view_and_keeps_its_row() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let mut state = OutlinerState::new();
    state.set_expanded(id(0), true);
    let long = wide(100_000);
    let step = |ui: &mut Ui, state: &mut OutlinerState, nav| {
        page(ui, state, idle(), nav, SelectMode::Replace, &long)
    };
    step(&mut ui, &mut state, NavInput::default());
    let first = step(&mut ui, &mut state, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(row_key(&ui, &first, id(0))));

    for index in 1..=40_u64 {
        let built = step(&mut ui, &mut state, DOWN);
        assert!(built.built.len() <= BOUND, "row {index}: {:?}", built.built);
        let row = row_key(&ui, &built, id(index));
        assert_eq!(
            ui.focused(),
            Some(row),
            "the step to row {index} went astray"
        );
        let (row_min, row_max) = rect(&ui, row);
        let (view_min, view_max) = rect(&ui, built.outliner.key);
        assert!(
            row_min.y >= view_min.y && row_max.y <= view_max.y,
            "row {index} at {row_min}..{row_max} is outside the view"
        );
    }

    // Scrolled away from the focused row, it is still built and still focused.
    let focused = ui.focused().expect("a row holds focus");
    let slot = ui.store.find(first.outliner.key).expect("stored");
    ui.store.get_mut(slot).scroll_offset = Vec2::new(0.0, 100_000.0);
    for _ in 0..3 {
        let built = step(&mut ui, &mut state, NavInput::default());
        assert_eq!(ui.focused(), Some(focused), "scrolling away took focus");
        assert!(built.built.len() <= BOUND, "{:?}", built.built);
        assert_eq!(built.built[0].id, id(40), "the kept row is not first");
    }
}
