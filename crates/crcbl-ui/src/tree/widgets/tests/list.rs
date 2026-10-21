//! The virtualized list: a bounded build whatever its length, focus stepping
//! past the rows shown, the focused row kept, and the rows its offset names.

use super::*;
use crate::tree::{LIST_OVERSCAN, Response};

/// A row's height, in pixels.
const ROW: f32 = 20.0;
/// The list's content-box height: five rows.
const VIEW: f32 = 100.0;
/// The most rows one frame may build: the rows the view can show, one more
/// for a row cut by each edge, the overscan on both sides, and the focused
/// row outside it all.
const BOUND: usize = (VIEW / ROW) as usize + 1 + 2 * LIST_OVERSCAN + 1;

/// What one frame of the list page built.
struct ListPage {
    list: Response,
    /// Every row index the row builder was called with, in order.
    built: Vec<usize>,
    /// How many nodes the frame built, the page's own included.
    nodes: u32,
}

/// A list of `rows` rows, [`VIEW`] tall with no border or padding.
fn list_page(ui: &mut Ui, rows: usize, pointer: PointerInput, nav: NavInput) -> ListPage {
    frame(ui, pointer, nav, |ui| {
        let mut built = Vec::new();
        let list = ui.list("#rows", rows, ROW, |_, index| built.push(index));
        let nodes = ui.style_stats().nodes;
        ListPage { list, built, nodes }
    })
}

/// The list's stylesheet: a fixed height, and nothing between its border box
/// and its content box, so row geometry is plain arithmetic.
fn sheet(ui: &mut Ui) {
    ui.add_stylesheet(
        "list.css",
        &format!("#rows {{ height: {VIEW}px; border-width: 0; padding: 0; }}"),
    );
}

/// The key of row `index` of `page`'s list.
fn row_key(ui: &Ui, page: &ListPage, index: usize) -> NodeKey {
    let content = children_of(ui, page.list.key)[0];
    children_of(ui, content)
        .into_iter()
        .find(|&key| {
            let node = ui.nodes.iter().find(|node| node.key == key).expect("built");
            node.style.inset.top == LengthAuto::Px(index as f32 * ROW)
        })
        .unwrap_or_else(|| panic!("row {index} was not built"))
}

fn offset(ui: &Ui, page: &ListPage) -> f32 {
    ui.store
        .by_key(page.list.key)
        .expect("stored")
        .scroll_offset
        .y
}

/// **A 100 000-row list builds no more rows a frame than its view and
/// overscan hold, and exactly as many nodes as a 100-row list does** — the
/// rows in the window and nothing else — while its content is as tall as all
/// of them.
#[test]
fn a_hundred_thousand_row_list_builds_a_bounded_window() {
    let mut long = Ui::new();
    sheet(&mut long);
    let mut short = Ui::new();
    sheet(&mut short);
    for _ in 0..3 {
        let page = list_page(&mut long, 100_000, idle(), NavInput::default());
        let small = list_page(&mut short, 100, idle(), NavInput::default());
        assert!(
            page.built.len() <= BOUND,
            "{} rows built: {:?}",
            page.built.len(),
            page.built
        );
        assert!(!page.built.is_empty(), "nothing was built");
        assert_eq!(page.built, (0..page.built.len()).collect::<Vec<_>>());
        assert_eq!(page.nodes, small.nodes, "the node count grew with the list");
    }
    let page = list_page(&mut long, 100_000, idle(), NavInput::default());
    let content = children_of(&long, page.list.key)[0];
    let (min, max) = rect(&long, content);
    assert_eq!(
        max.y - min.y,
        100_000.0 * ROW,
        "the content is not every row tall"
    );
    assert_eq!(
        long.len(),
        short.len(),
        "the store holds more than the window"
    );
}

/// **Focus steps past the rows shown into rows that were not built when the
/// step began**, down a 100 000-row list: every step lands on the next row,
/// scrolls the list the least that shows it, and builds a bounded window —
/// and next continues the walk.
#[test]
fn focus_steps_past_the_view_and_scrolls_the_list() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    list_page(&mut ui, 100_000, idle(), NavInput::default());
    let page = list_page(&mut ui, 100_000, idle(), NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(row_key(&ui, &page, 0)));

    for index in 1..=40_usize {
        let nav = if index.is_multiple_of(2) {
            DOWN
        } else {
            NavInput::NEXT
        };
        let page = list_page(&mut ui, 100_000, idle(), nav);
        assert!(page.built.len() <= BOUND, "row {index}: {:?}", page.built);
        let row = row_key(&ui, &page, index);
        assert_eq!(
            ui.focused(),
            Some(row),
            "the step to row {index} went elsewhere"
        );
        let (row_min, row_max) = rect(&ui, row);
        let (view_min, view_max) = rect(&ui, page.list.key);
        assert!(
            row_min.y >= view_min.y && row_max.y <= view_max.y,
            "row {index} at {row_min}..{row_max} is outside the view"
        );
        let least = ((index + 1) as f32 * ROW - VIEW).max(0.0);
        assert_eq!(
            offset(&ui, &page),
            least,
            "row {index} scrolled more than it needed"
        );
    }
}

/// **The focused row is kept when the list scrolls away from it**, built in
/// tree order outside the window and still focused, while the window shows
/// exactly the rows the offset names — the row at the top of the view is the
/// offset over the row height.
#[test]
fn the_focused_row_is_kept_and_the_view_shows_the_rows_its_offset_names() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    list_page(&mut ui, 100_000, idle(), NavInput::default());
    let page = list_page(&mut ui, 100_000, idle(), NavInput::NAVIGATION);
    let first_row = row_key(&ui, &page, 0);
    assert_eq!(ui.focused(), Some(first_row));

    let slot = ui.store.find(page.list.key).expect("stored");
    ui.store.get_mut(slot).scroll_offset = Vec2::new(0.0, 1000.0);
    for _ in 0..3 {
        let page = list_page(&mut ui, 100_000, idle(), NavInput::default());
        assert_eq!(ui.focused(), Some(first_row), "scrolling away took focus");
        assert_eq!(
            page.built[0], 0,
            "the focused row is not first in tree order"
        );
        assert!(page.built.len() <= BOUND, "{:?}", page.built);
        let top = (offset(&ui, &page) / ROW) as usize;
        assert_eq!(top, 50);
        let (view_min, _) = rect(&ui, page.list.key);
        assert_eq!(
            rect(&ui, row_key(&ui, &page, top)).0.y,
            view_min.y,
            "the row the offset names is not at the top of the view"
        );
        assert!(
            page.built.contains(&(top + 4)) && !page.built.contains(&(top - LIST_OVERSCAN - 1)),
            "the window is not the offset's: {:?}",
            page.built
        );
    }
}

/// **One build after the row count or the offset changes builds exactly the
/// rows the view then shows**, and layout finds nothing missing: a list that
/// shrinks under a deep offset builds the rows it now ends with, not an empty
/// window past its new end, and a jump of the offset builds the rows it names.
#[test]
fn one_build_windows_this_frames_rows_and_offset() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let page = list_page(&mut ui, 100, idle(), NavInput::default());
    let list = page.list.key;

    // The rows the view shows at `offset` and the overscan on each side.
    let window = |offset: f32, rows: usize| {
        let top = (offset / ROW) as usize;
        let end = ((offset + VIEW) / ROW).ceil() as usize;
        top.saturating_sub(LIST_OVERSCAN)..(end + LIST_OVERSCAN).min(rows)
    };
    let check = |ui: &Ui, page: &ListPage, rows: usize, offset: f32| {
        assert_eq!(
            page.built,
            window(offset, rows).collect::<Vec<_>>(),
            "{rows} rows at {offset}"
        );
        assert_eq!(ui.scroll_offset_of(list).y, offset);
        let top = (offset / ROW) as usize;
        let (view_min, _) = rect(ui, list);
        assert_eq!(rect(ui, row_key(ui, page, top)).0.y, view_min.y);
        assert!(!ui.needs_rebuild(), "{rows} rows at {offset} missed a row");
    };

    // Jump to the end of a hundred rows, then shrink to four: the offset
    // clamps to nothing and the four rows are built.
    ui.set_scroll_offset_of(list, Vec2::new(0.0, 100.0 * ROW - VIEW));
    let page = list_page(&mut ui, 100, idle(), NavInput::default());
    check(&ui, &page, 100, 100.0 * ROW - VIEW);
    let page = list_page(&mut ui, 4, idle(), NavInput::default());
    assert_eq!(page.built, [0, 1, 2, 3]);
    check(&ui, &page, 4, 0.0);

    // Grow to fifty and jump to row thirty in the same build.
    ui.set_scroll_offset_of(list, Vec2::new(0.0, 30.0 * ROW));
    let page = list_page(&mut ui, 50, idle(), NavInput::default());
    check(&ui, &page, 50, 30.0 * ROW);

    // Shrink to ten under that offset: it clamps to the last five rows.
    let page = list_page(&mut ui, 10, idle(), NavInput::default());
    check(&ui, &page, 10, 10.0 * ROW - VIEW);
}

/// **A list that leaves the page and comes back builds its whole view in one
/// build**, though nothing of it was laid out: a list sized by a percentage
/// takes the viewport as its view, and layout finds nothing missing. Only the
/// very first frame, with no viewport known yet, asks for a rebuild.
#[test]
fn a_list_coming_back_builds_its_view_in_one_build() {
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "list.css",
        &format!(
            ".holder {{ height: {VIEW}px; flex-shrink: 0; }}
             #rows {{ height: 100%; border-width: 0; padding: 0; }}"
        ),
    );
    let build = |ui: &mut Ui, rows: Option<usize>| {
        frame(ui, idle(), NavInput::default(), |ui| {
            let mut built = Vec::new();
            ui.block(".holder", &[], |ui| match rows {
                Some(rows) => {
                    ui.list("#rows", rows, ROW, |_, index| built.push(index));
                }
                None => {
                    ui.span(".empty", "no rows", &[]);
                }
            });
            built
        })
    };

    assert_eq!(build(&mut ui, Some(4)), [0, 1], "no viewport, no view");
    assert!(ui.needs_rebuild(), "the first frame missed rows unreported");
    assert_eq!(build(&mut ui, Some(4)), [0, 1, 2, 3]);
    assert!(!ui.needs_rebuild());

    for _ in 0..2 {
        assert!(build(&mut ui, None).is_empty());
        assert!(!ui.needs_rebuild());
        assert_eq!(
            build(&mut ui, Some(4)),
            [0, 1, 2, 3],
            "the list came back short"
        );
        assert!(!ui.needs_rebuild(), "the list came back short");
    }
}

/// **A list sized by its content that grows past its overscan asks for a
/// rebuild**, since the view layout gives it was not knowable when it was
/// built — and the rebuild windows the view layout measured.
#[test]
fn a_list_that_grows_taller_asks_for_a_rebuild() {
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "list.css",
        &format!("#rows {{ max-height: {VIEW}px; border-width: 0; padding: 0; }}"),
    );
    list_page(&mut ui, 1, idle(), NavInput::default());
    let page = list_page(&mut ui, 1, idle(), NavInput::default());
    assert_eq!(page.built, [0]);
    assert!(!ui.needs_rebuild());
    assert_eq!(
        rect(&ui, page.list.key).1.y - rect(&ui, page.list.key).0.y,
        ROW
    );

    let page = list_page(&mut ui, 40, idle(), NavInput::default());
    assert_eq!(
        page.built,
        [0, 1, 2],
        "the view it was built for was one row"
    );
    assert!(ui.needs_rebuild(), "the grown view's rows went unreported");

    let page = list_page(&mut ui, 40, idle(), NavInput::default());
    let rows = (VIEW / ROW) as usize + LIST_OVERSCAN;
    assert_eq!(page.built, (0..rows).collect::<Vec<_>>());
    assert!(!ui.needs_rebuild());
}

/// **A selectable list builds its selected row `:checked`, and only that row**,
/// so a stylesheet draws the application's current row as `.list-row:checked`;
/// a plain list's rows are never checked.
#[test]
fn a_selectable_lists_selected_row_is_checked() {
    let accent = linear("#c03030");
    let mut ui = Ui::new();
    sheet(&mut ui);
    ui.add_stylesheet("selected.css", ".list-row:checked { background: #c03030; }");
    let page = frame(&mut ui, idle(), NavInput::default(), |ui| {
        let list = ui.selectable_list("#rows", 3, ROW, Some(1), |_, _| {});
        ListPage {
            list,
            built: Vec::new(),
            nodes: 0,
        }
    });
    let checked: Vec<bool> = (0..3)
        .map(|index| style_of(&ui, row_key(&ui, &page, index)).background == accent)
        .collect();
    assert_eq!(checked, [false, true, false]);

    let plain = list_page(&mut ui, 3, idle(), NavInput::default());
    let checked: Vec<bool> = (0..3)
        .map(|index| style_of(&ui, row_key(&ui, &plain, index)).background == accent)
        .collect();
    assert_eq!(checked, [false; 3], "a plain list has no selection");
}
