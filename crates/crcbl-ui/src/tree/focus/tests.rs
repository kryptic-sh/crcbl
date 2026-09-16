//! Focus through the tree: moves, overrides, wrapping, scopes and their
//! memory, the modal trap, scrolling into view, engagement and its snapshot,
//! the pseudo-classes it sets, the mixed-input rule and the debug overlay —
//! each held to the behaviour it exists for.

use glam::Vec2;

use super::super::tests::idle;
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::style::Declaration;
use crate::text::FontAtlas;
use crate::tree::{AvailableSpace, FlexDirection, Length, LengthAuto, NodeStyle, Position};
use crate::widget::PointerInput;

/// A node of a fixture page: its selector, where it is on screen as
/// `[x, y, width, height]`, how it takes part in focus, and its children.
#[derive(Clone, Copy)]
struct Spec {
    selector: &'static str,
    rect: [f32; 4],
    behavior: Behavior,
    children: &'static [Spec],
}

const fn leaf(selector: &'static str, rect: [f32; 4], behavior: Behavior) -> Spec {
    Spec {
        selector,
        rect,
        behavior,
        children: &[],
    }
}

const fn group(
    selector: &'static str,
    rect: [f32; 4],
    behavior: Behavior,
    children: &'static [Spec],
) -> Spec {
    Spec {
        selector,
        rect,
        behavior,
        children,
    }
}

/// Every node a fixture frame built, by the id in its selector.
struct Page(Vec<(&'static str, Response)>);

impl Page {
    fn get(&self, name: &str) -> Response {
        self.0
            .iter()
            .find(|(own, _)| *own == name)
            .unwrap_or_else(|| panic!("no node `{name}`"))
            .1
    }

    fn key(&self, name: &str) -> NodeKey {
        self.get(name).key
    }

    fn name(&self, key: Option<NodeKey>) -> Option<&'static str> {
        let key = key?;
        self.0
            .iter()
            .find(|(_, response)| response.key == key)
            .map(|(name, _)| *name)
    }
}

fn id_of(selector: &'static str) -> &'static str {
    selector
        .strip_prefix('#')
        .expect("every fixture node has an id")
        .split('.')
        .next()
        .expect("split yields one part")
}

fn absolute(rect: [f32; 4], parent: [f32; 4]) -> [Declaration; 5] {
    [
        Declaration::Position(Position::Absolute),
        Declaration::Inset(
            crate::style::Sides::Left,
            LengthAuto::Px(rect[0] - parent[0]),
        ),
        Declaration::Inset(
            crate::style::Sides::Top,
            LengthAuto::Px(rect[1] - parent[1]),
        ),
        Declaration::Width(LengthAuto::Px(rect[2])),
        Declaration::Height(LengthAuto::Px(rect[3])),
    ]
}

fn build(ui: &mut Ui, specs: &[Spec], parent: [f32; 4], page: &mut Vec<(&'static str, Response)>) {
    for spec in specs {
        let response = ui.block_with(
            spec.selector,
            &absolute(spec.rect, parent),
            spec.behavior,
            |ui| build(ui, spec.children, spec.rect, page),
        );
        page.push((id_of(spec.selector), response));
    }
}

/// The surface every fixture page is laid out on.
const SURFACE: [f32; 4] = [0.0, 0.0, 1000.0, 1000.0];

/// One frame of `specs`, under a 1000 × 1000 root.
fn step(ui: &mut Ui, specs: &[Spec], pointer: PointerInput, nav: NavInput) -> Page {
    let mut page = Vec::new();
    ui.begin_frame_with(pointer, nav);
    ui.block(
        "#surface",
        &[
            Declaration::Width(LengthAuto::Px(SURFACE[2])),
            Declaration::Height(LengthAuto::Px(SURFACE[3])),
        ],
        |ui| build(ui, specs, SURFACE, &mut page),
    );
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );
    Page(page)
}

/// Builds `specs` once to lay them out, then feeds `inputs` a frame each and
/// returns the focused node's name after each.
fn walk(ui: &mut Ui, specs: &[Spec], inputs: &[NavInput]) -> Vec<Option<&'static str>> {
    let first = step(ui, specs, idle(), NavInput::default());
    let mut names = Vec::new();
    for &nav in inputs {
        step(ui, specs, idle(), nav);
        names.push(first.name(ui.focused()));
    }
    names
}

fn press(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: true,
        released: false,
    }
}

fn release(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: false,
        released: true,
    }
}

/// The middle of a fixture rectangle.
fn centre(rect: [f32; 4]) -> Vec2 {
    Vec2::new(rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5)
}

const B: Behavior = Behavior::BUTTON;
const UP: NavInput = NavInput::toward(Direction::Up);
const DOWN: NavInput = NavInput::toward(Direction::Down);
const LEFT: NavInput = NavInput::toward(Direction::Left);
const RIGHT: NavInput = NavInput::toward(Direction::Right);

/// A 3 × 3 grid of 40 × 20 buttons, 10 apart, inside `#grid`.
const GRID_CELLS: &[Spec] = &[
    leaf("#a0", [100.0, 100.0, 40.0, 20.0], B),
    leaf("#a1", [150.0, 100.0, 40.0, 20.0], B),
    leaf("#a2", [200.0, 100.0, 40.0, 20.0], B),
    leaf("#b0", [100.0, 130.0, 40.0, 20.0], B),
    leaf("#b1", [150.0, 130.0, 40.0, 20.0], B),
    leaf("#b2", [200.0, 130.0, 40.0, 20.0], B),
    leaf("#c0", [100.0, 160.0, 40.0, 20.0], B),
    leaf("#c1", [150.0, 160.0, 40.0, 20.0], B),
    leaf("#c2", [200.0, 160.0, 40.0, 20.0], B),
];
const GRID: &[Spec] = &[group(
    "#grid",
    [100.0, 100.0, 140.0, 80.0],
    Behavior::NONE,
    GRID_CELLS,
)];

// ---------------------------------------------------------------------------
// Moves
// ---------------------------------------------------------------------------

/// **The pad lands focus on the first button and every step after it goes
/// to the adjacent one**; a move off the grid's edge stays put, and a first
/// press only lands.
#[test]
fn a_grid_is_walked_cell_by_cell_and_a_move_off_its_edge_stays_put() {
    let mut ui = Ui::new();
    let walked = walk(
        &mut ui,
        GRID,
        &[
            RIGHT, RIGHT, RIGHT, RIGHT, DOWN, DOWN, DOWN, LEFT, UP, LEFT, LEFT,
        ],
    );
    assert_eq!(
        walked,
        [
            Some("a0"),
            Some("a1"),
            Some("a2"),
            Some("a2"),
            Some("b2"),
            Some("c2"),
            Some("c2"),
            Some("c1"),
            Some("b1"),
            Some("b0"),
            Some("b0"),
        ]
    );
    assert!(
        ui.nav_scores().is_empty(),
        "a move that found nothing scored something: {:?}",
        ui.nav_scores()
    );
}

/// **`nav-*` sends a move to the id it names, `none` keeps focus where it is,
/// and an id nothing focusable has falls back to the search with one
/// warning**, however many times it is pressed.
#[test]
fn nav_properties_redirect_a_move_stop_it_or_fall_back_with_one_warning() {
    let logs = crcbl_core::log::capture();
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "nav.css",
        "#a0 { nav-right: #c2; } #c2 { nav-up: none; } #c1 { nav-left: #nowhere; }",
    );
    let walked = walk(&mut ui, GRID, &[RIGHT, RIGHT, UP, LEFT, LEFT, RIGHT, LEFT]);
    assert_eq!(
        walked,
        [
            Some("a0"),
            Some("c2"),
            Some("c2"),
            Some("c1"),
            Some("c0"),
            Some("c1"),
            Some("c0"),
        ]
    );
    let warnings = logs
        .records()
        .iter()
        .filter(|record| record.message.contains("nav-left names an id"))
        .count();
    assert_eq!(warnings, 1, "{:#?}", logs.records());
}

/// **`nav-wrap` wraps a move that finds nothing inside its container round to
/// the far side, on the axes it names only** — and without it the same move
/// stays put.
#[test]
fn nav_wrap_wraps_only_the_axes_it_names() {
    type Case = (&'static str, &'static [NavInput], [Option<&'static str>; 3]);
    let cases: [Case; 3] = [
        ("", &[LEFT, UP, DOWN], [Some("a0"), Some("a0"), Some("b0")]),
        (
            "#grid { nav-wrap: horizontal; }",
            &[LEFT, UP, LEFT],
            [Some("a2"), Some("a2"), Some("a1")],
        ),
        (
            "#grid { nav-wrap: both; }",
            &[UP, RIGHT, LEFT],
            [Some("c0"), Some("c1"), Some("c0")],
        ),
    ];
    for (css, inputs, want) in cases {
        let mut ui = Ui::new();
        ui.add_stylesheet("wrap.css", css);
        let mut all = vec![NavInput::NAVIGATION];
        all.extend_from_slice(inputs);
        let walked = walk(&mut ui, GRID, &all);
        assert_eq!(walked[0], Some("a0"), "`{css}` landed elsewhere");
        assert_eq!(walked[1..], want, "`{css}`");
    }
}

/// **Tree order visits every focusable node in build order, wrapping at both
/// ends, and skips the disabled and the unfocusable** — and a plain block that
/// opts in is visited.
#[test]
fn tree_order_wraps_and_skips_what_cannot_take_focus() {
    const DISABLED: Behavior = Behavior {
        disabled: true,
        ..Behavior::BUTTON
    };
    const OPTED_OUT: Behavior = Behavior {
        focusable: Some(false),
        ..Behavior::BUTTON
    };
    const OPTED_IN: Behavior = Behavior {
        focusable: Some(true),
        ..Behavior::NONE
    };
    const ROW: &[Spec] = &[
        leaf("#first", [0.0, 0.0, 10.0, 10.0], B),
        leaf("#disabled", [20.0, 0.0, 10.0, 10.0], DISABLED),
        leaf("#out", [40.0, 0.0, 10.0, 10.0], OPTED_OUT),
        leaf("#plain", [60.0, 0.0, 10.0, 10.0], Behavior::NONE),
        leaf("#in", [80.0, 0.0, 10.0, 10.0], OPTED_IN),
        leaf("#last", [100.0, 0.0, 10.0, 10.0], Behavior::ENGAGE),
    ];
    let mut ui = Ui::new();
    let next = NavInput::NEXT;
    let prev = NavInput::PREV;
    let walked = walk(&mut ui, ROW, &[next, next, next, next, prev, prev]);
    assert_eq!(
        walked,
        [
            Some("first"),
            Some("in"),
            Some("last"),
            Some("first"),
            Some("last"),
            Some("in"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Scopes
// ---------------------------------------------------------------------------

/// Two panes side by side, three rows each, and a stray button that belongs
/// to neither pane but sits inside the left one's column.
const PANES: &[Spec] = &[
    group(
        "#left",
        [0.0, 0.0, 100.0, 320.0],
        Behavior::SCOPE,
        &[
            leaf("#l0", [0.0, 0.0, 100.0, 20.0], B),
            leaf("#l1", [0.0, 150.0, 100.0, 20.0], B),
            leaf("#l2", [0.0, 300.0, 100.0, 20.0], B),
        ],
    ),
    group(
        "#right",
        [200.0, 0.0, 100.0, 320.0],
        Behavior::SCOPE,
        &[
            leaf("#r0", [200.0, 0.0, 100.0, 20.0], B),
            leaf("#r1", [200.0, 150.0, 100.0, 20.0], B),
            leaf("#r2", [200.0, 300.0, 100.0, 20.0], B),
        ],
    ),
    leaf("#stray", [0.0, 60.0, 100.0, 20.0], B),
];

/// **A move searches inside the focused node's scope before anything
/// outside it**: from `l0` the stray button is in the beam and nearer than
/// `l1`, and `l1` still wins — and once the scope has nothing that way, the
/// move widens past it.
#[test]
fn a_scope_is_searched_before_what_lies_outside_it() {
    let mut ui = Ui::new();
    let walked = walk(
        &mut ui,
        PANES,
        &[NavInput::NAVIGATION, DOWN, DOWN, DOWN, UP, UP, UP],
    );
    assert_eq!(
        walked,
        [
            Some("l0"),
            Some("l1"),
            Some("l2"),
            Some("l2"),
            Some("l1"),
            Some("l0"),
            Some("l0"),
        ],
        "the stray button outside the scope took a move inside it"
    );
    // Out of the scope's top: the stray button is below, so nothing is found;
    // from the stray button itself, up finds `l0` in the wider search.
    let mut ui = Ui::new();
    let page = step(&mut ui, PANES, idle(), NavInput::default());
    ui.set_focus(page.key("stray"));
    step(&mut ui, PANES, idle(), NavInput::NAVIGATION);
    assert_eq!(page.name(ui.focused()), Some("stray"));
    step(&mut ui, PANES, idle(), UP);
    assert_eq!(page.name(ui.focused()), Some("l0"));

    // Back to the stray button and down: the left pane remembers `l0`, which
    // lies behind the press, so the move takes the geometric `l1`.
    ui.set_focus(page.key("stray"));
    step(&mut ui, PANES, idle(), NavInput::NAVIGATION);
    step(&mut ui, PANES, idle(), DOWN);
    assert_eq!(
        page.name(ui.focused()),
        Some("l1"),
        "a scope's memory pulled focus against the press"
    );
}

/// **A scope remembers the last node focused inside it, and a move that
/// enters it resumes there** instead of at the geometric winner — in both
/// directions across the gap.
#[test]
fn entering_a_scope_resumes_at_the_node_it_remembers() {
    let mut ui = Ui::new();
    let walked = walk(
        &mut ui,
        PANES,
        &[
            NavInput::NAVIGATION,
            RIGHT,
            DOWN,
            DOWN,
            LEFT,
            RIGHT,
            LEFT,
            DOWN,
            RIGHT,
        ],
    );
    assert_eq!(
        walked,
        [
            Some("l0"),
            // First entry: nothing remembered yet, so the geometric winner.
            Some("r0"),
            Some("r1"),
            Some("r2"),
            // `r2`'s geometric neighbour is `l2`; the left pane remembers `l0`.
            Some("l0"),
            // `l0`'s geometric neighbour is `r0`; the right pane remembers `r2`.
            Some("r2"),
            Some("l0"),
            Some("l1"),
            Some("r2"),
        ]
    );
}

/// Two buttons behind a modal, and the modal's two buttons over them.
fn modal_page(open: bool) -> &'static [Spec] {
    const BEHIND: [Spec; 2] = [
        leaf("#under0", [0.0, 0.0, 100.0, 40.0], B),
        leaf("#under1", [0.0, 300.0, 100.0, 40.0], B),
    ];
    const OPEN: &[Spec] = &[
        BEHIND[0],
        BEHIND[1],
        group(
            "#modal",
            [150.0, 100.0, 300.0, 100.0],
            Behavior::MODAL,
            &[
                leaf("#yes", [160.0, 140.0, 100.0, 40.0], B),
                leaf("#no", [300.0, 140.0, 100.0, 40.0], B),
            ],
        ),
    ];
    const CLOSED: &[Spec] = &BEHIND;
    if open { OPEN } else { CLOSED }
}

/// **A modal pulls focus in, keeps every move, every tree-order step and every
/// click inside it, and when it closes focus returns to what was focused
/// before it opened.**
#[test]
fn a_modal_traps_focus_and_hands_it_back_when_it_closes() {
    let mut ui = Ui::new();
    let closed = step(&mut ui, modal_page(false), idle(), NavInput::default());
    step(&mut ui, modal_page(false), idle(), NavInput::NAVIGATION);
    step(&mut ui, modal_page(false), idle(), DOWN);
    assert_eq!(closed.name(ui.focused()), Some("under1"));

    // The frame that first builds the modal cannot see it yet; the next can.
    let open = step(&mut ui, modal_page(true), idle(), NavInput::NAVIGATION);
    assert_eq!(open.name(ui.focused()), Some("under1"));
    let open = step(&mut ui, modal_page(true), idle(), NavInput::NAVIGATION);
    assert_eq!(
        open.name(ui.focused()),
        Some("yes"),
        "the modal did not take focus"
    );

    for (nav, want) in [
        (LEFT, "yes"),
        (UP, "yes"),
        (DOWN, "yes"),
        (RIGHT, "no"),
        (RIGHT, "no"),
        (NavInput::NEXT, "yes"),
        (NavInput::NEXT, "no"),
        (NavInput::PREV, "yes"),
    ] {
        step(&mut ui, modal_page(true), idle(), nav);
        assert_eq!(
            open.name(ui.focused()),
            Some(want),
            "{nav:?} left the modal or went astray"
        );
    }

    let under = centre([0.0, 300.0, 100.0, 40.0]);
    step(&mut ui, modal_page(true), press(under), NavInput::default());
    let clicked = step(
        &mut ui,
        modal_page(true),
        release(under),
        NavInput::default(),
    );
    assert!(
        clicked.get("under1").clicked,
        "the click itself still lands"
    );
    assert_eq!(
        open.name(ui.focused()),
        Some("yes"),
        "a click outside the modal moved focus out of it"
    );
    ui.set_focus(open.key("under0"));
    step(&mut ui, modal_page(true), idle(), NavInput::NAVIGATION);
    assert_eq!(open.name(ui.focused()), Some("yes"), "set_focus leaked out");

    step(&mut ui, modal_page(false), idle(), NavInput::NAVIGATION);
    step(&mut ui, modal_page(false), idle(), NavInput::NAVIGATION);
    assert_eq!(
        closed.name(ui.focused()),
        Some("under1"),
        "closing the modal did not return focus to its opener"
    );
}

// ---------------------------------------------------------------------------
// Scrolling into view
// ---------------------------------------------------------------------------

const ROW_HEIGHT: f32 = 20.0;
const ROWS: usize = 10;
const VIEW_HEIGHT: f32 = 64.0;
const VIEW_PADDING: f32 = 4.0;

/// An `overflow: scroll` column [`VIEW_HEIGHT`] tall with [`VIEW_PADDING`] on
/// every side, holding [`ROWS`] focusable rows of [`ROW_HEIGHT`], and a button
/// below it. Returns the list, every row in order, and the button.
fn scrolled(
    ui: &mut Ui,
    nav: NavInput,
    offset: Option<Vec2>,
) -> (Response, Vec<Response>, Response) {
    ui.begin_frame_with(idle(), nav);
    let mut rows = Vec::new();
    let mut list = None;
    let mut below = None;
    ui.block(
        "",
        &NodeStyle {
            flex_direction: FlexDirection::Column,
            ..NodeStyle::DEFAULT
        }
        .declarations(),
        |ui| {
            list = Some(ui.block(
                "#list",
                &[
                    Declaration::Overflow(Overflow::Scroll),
                    Declaration::FlexDirection(FlexDirection::Column),
                    Declaration::Width(LengthAuto::Px(100.0)),
                    Declaration::Height(LengthAuto::Px(VIEW_HEIGHT)),
                    Declaration::Padding(crate::style::Sides::All, Length::Px(VIEW_PADDING)),
                    Declaration::BorderWidth(crate::style::Sides::All, 2.0),
                ],
                |ui| {
                    if let Some(offset) = offset {
                        ui.set_scroll_offset(offset);
                    }
                    for row in 0..ROWS {
                        rows.push(ui.block_keyed_with(
                            row,
                            "",
                            &[
                                Declaration::Height(LengthAuto::Px(ROW_HEIGHT)),
                                Declaration::FlexShrink(0.0),
                            ],
                            Behavior::BUTTON,
                            |_| {},
                        ));
                    }
                },
            ));
            below = Some(ui.block_with(
                "#below",
                &[Declaration::Height(LengthAuto::Px(ROW_HEIGHT))],
                Behavior::BUTTON,
                |_| {},
            ));
        },
    );
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::MAX_CONTENT,
        &FontAtlas::built_in(),
    );
    (list.expect("built"), rows, below.expect("built"))
}

/// **Every step down a scroll container brings the focused row inside its
/// content box, scrolling the least that does, and the offset stops at the
/// content's reach** — and an offset set past that reach is clamped.
#[test]
fn a_scroll_container_scrolls_the_focused_row_into_view_and_clamps_its_offset() {
    let mut ui = Ui::new();
    scrolled(&mut ui, NavInput::default(), None);
    let (list, rows, _) = scrolled(&mut ui, NavInput::NAVIGATION, None);
    assert_eq!(ui.focused(), Some(rows[0].key));

    let (min, max) = ui.rect(list.key).expect("laid out");
    let border = 2.0;
    let content = (min.y + border + VIEW_PADDING, max.y - border - VIEW_PADDING);
    // Rows, both paddings and nothing else: what the content box cannot show.
    let reach = ROWS as f32 * ROW_HEIGHT + 2.0 * VIEW_PADDING - (VIEW_HEIGHT - 2.0 * border);
    let mut scrolled_at_all = false;
    for index in 1..ROWS {
        let (_, rows, _) = scrolled(&mut ui, DOWN, None);
        assert_eq!(ui.focused(), Some(rows[index].key), "row {index}");
        let (row_min, row_max) = ui.rect(rows[index].key).expect("laid out");
        assert!(
            row_min.y >= content.0 && row_max.y <= content.1,
            "row {index} at {row_min}..{row_max} is outside the content box {content:?}"
        );
        let offset = ui.store.by_key(list.key).expect("stored").scroll_offset.y;
        assert!(
            (0.0..=reach).contains(&offset),
            "offset {offset} is past the reach {reach}"
        );
        // The least scroll: the row sits flush with the content box's bottom.
        if offset > 0.0 && offset < reach {
            assert_eq!(
                row_max.y, content.1,
                "row {index} scrolled further than needed"
            );
        }
        scrolled_at_all |= offset > 0.0;
    }
    assert!(scrolled_at_all, "nothing ever scrolled");
    let (_, _, below) = scrolled(&mut ui, DOWN, None);
    assert_eq!(
        ui.focused(),
        Some(below.key),
        "a move past the list's last row did not widen out of the list"
    );

    scrolled(&mut ui, NavInput::default(), Some(Vec2::new(500.0, 900.0)));
    let (list, _, _) = scrolled(&mut ui, NavInput::default(), None);
    let stored = ui.store.by_key(list.key).expect("stored");
    assert_eq!(
        stored.scroll_offset,
        Vec2::new(0.0, reach),
        "the offset was not clamped"
    );
    assert_eq!(stored.scroll_max, Vec2::new(0.0, reach));
}

// ---------------------------------------------------------------------------
// Engagement
// ---------------------------------------------------------------------------

const ENGAGE_PAGE: &[Spec] = &[
    leaf("#volume.slider", [0.0, 0.0, 100.0, 20.0], Behavior::ENGAGE),
    leaf("#pitch.slider", [0.0, 40.0, 100.0, 20.0], Behavior::ENGAGE),
    leaf("#ok", [0.0, 80.0, 100.0, 20.0], B),
    leaf("#empty", [300.0, 300.0, 100.0, 20.0], Behavior::NONE),
];

/// One frame of [`ENGAGE_PAGE`] with a widget's value: every step the engaged
/// volume took adds one, and its snapshot is kept and restored.
fn slide(ui: &mut Ui, pointer: PointerInput, nav: NavInput, volume: &mut f32) -> Page {
    let page = step(ui, ENGAGE_PAGE, pointer, nav);
    let response = page.get("volume");
    ui.snapshot(&response, volume);
    if response.captured.is_some() {
        *volume += 1.0;
    }
    page
}

/// **Focus alone never takes navigation; accept engages, the engaged widget
/// takes every step, back cancels to the snapshot, accept commits the edit,
/// and a click engages too** — with `:engaged` on the widget only while it
/// is engaged, and focus on it throughout.
#[test]
fn engaging_takes_navigation_and_commit_keeps_the_edit_while_cancel_restores_it() {
    let mut ui = Ui::new();
    ui.add_stylesheet("engage.css", ".slider:engaged { background: #ff0000; }");
    let mut volume = 5.0;
    let first = slide(&mut ui, idle(), NavInput::default(), &mut volume);
    slide(&mut ui, idle(), NavInput::NAVIGATION, &mut volume);
    assert_eq!(first.name(ui.focused()), Some("volume"));

    // Focused, not engaged: down moves focus past it.
    slide(&mut ui, idle(), DOWN, &mut volume);
    assert_eq!(first.name(ui.focused()), Some("pitch"));
    slide(&mut ui, idle(), UP, &mut volume);
    assert_eq!(volume, 5.0, "a focused widget took a step");

    let page = slide(&mut ui, idle(), NavInput::ACCEPT, &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Began);
    assert_eq!(ui.engaged(), Some(page.key("volume")));
    assert_eq!(
        style_of(&ui, page.key("volume")).background,
        [1.0, 0.0, 0.0, 1.0],
        "`:engaged` did not reach the cascade"
    );
    for nav in [RIGHT, DOWN, NavInput::NEXT] {
        let page = slide(&mut ui, idle(), nav, &mut volume);
        assert_eq!(page.get("volume").engagement, Engagement::Engaged);
        assert_eq!(
            first.name(ui.focused()),
            Some("volume"),
            "{nav:?} moved focus"
        );
    }
    assert_eq!(volume, 8.0);

    let page = slide(&mut ui, idle(), NavInput::BACK, &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Cancelled);
    assert_eq!(volume, 5.0, "cancel did not restore the snapshot");
    assert_eq!(ui.engaged(), None);
    assert!(!ui.back_requested(), "the cancel's back was also handed on");
    assert_eq!(first.name(ui.focused()), Some("volume"));
    let page = slide(&mut ui, idle(), NavInput::NAVIGATION, &mut volume);
    assert_eq!(style_of(&ui, page.key("volume")).background, [0.0; 4]);

    slide(&mut ui, idle(), NavInput::ACCEPT, &mut volume);
    slide(&mut ui, idle(), LEFT, &mut volume);
    let page = slide(&mut ui, idle(), NavInput::ACCEPT, &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Committed);
    assert_eq!(volume, 6.0, "commit lost the edit");
    slide(&mut ui, idle(), NavInput::BACK, &mut volume);
    assert_eq!(volume, 6.0, "a back after the commit restored something");
    assert!(
        ui.back_requested(),
        "back with nothing engaged was swallowed"
    );

    let on_volume = centre([0.0, 0.0, 100.0, 20.0]);
    slide(&mut ui, press(on_volume), NavInput::default(), &mut volume);
    let page = slide(
        &mut ui,
        release(on_volume),
        NavInput::default(),
        &mut volume,
    );
    assert_eq!(
        page.get("volume").engagement,
        Engagement::Began,
        "a click did not engage"
    );
}

/// **One engaged node per context**: clicking a second widget commits the
/// first in the same frame the second begins, and a click on nothing commits
/// the engaged one.
#[test]
fn engaging_a_second_widget_commits_the_first_and_a_click_away_commits() {
    let click = |ui: &mut Ui, rect: [f32; 4]| {
        step(ui, ENGAGE_PAGE, press(centre(rect)), NavInput::default());
        step(ui, ENGAGE_PAGE, release(centre(rect)), NavInput::default())
    };
    let mut ui = Ui::new();
    step(&mut ui, ENGAGE_PAGE, idle(), NavInput::default());
    let page = click(&mut ui, [0.0, 0.0, 100.0, 20.0]);
    assert_eq!(page.get("volume").engagement, Engagement::Began);

    let page = click(&mut ui, [0.0, 40.0, 100.0, 20.0]);
    assert_eq!(page.get("volume").engagement, Engagement::Committed);
    assert_eq!(page.get("pitch").engagement, Engagement::Began);
    assert_eq!(ui.engaged(), Some(page.key("pitch")));
    assert_eq!(page.name(ui.focused()), Some("pitch"));

    let page = click(&mut ui, [300.0, 300.0, 100.0, 20.0]);
    assert_eq!(page.get("pitch").engagement, Engagement::Committed);
    assert_eq!(ui.engaged(), None);
    assert_eq!(
        page.name(ui.focused()),
        Some("pitch"),
        "a click on nothing focusable moved focus"
    );

    // A request for focus elsewhere commits too.
    let page = click(&mut ui, [0.0, 0.0, 100.0, 20.0]);
    assert_eq!(page.get("volume").engagement, Engagement::Began);
    ui.set_focus(page.key("ok"));
    let page = step(&mut ui, ENGAGE_PAGE, idle(), NavInput::default());
    assert_eq!(page.get("volume").engagement, Engagement::Committed);
    assert_eq!(page.name(ui.focused()), Some("ok"));
}

/// **`Ui::clear_focus` is the only way focus leaves the tree**, which is the
/// claim it exists for: a click on a node the tree cannot focus — a panel's own
/// background, an application's viewport — commits whatever was engaged and
/// leaves focus exactly where it was, because the resolution has no node to
/// move it to. An application that knows the click went somewhere the tree
/// knows nothing about is the only thing that can say so.
///
/// And the engaged node is **committed**, as a click on another node commits
/// it, so a text input keeps what was typed rather than cancelling back.
#[test]
fn clearing_focus_takes_it_away_and_commits_what_was_engaged() {
    let mut ui = Ui::new();
    let mut volume = 5.0;
    let on_volume = centre([0.0, 0.0, 100.0, 20.0]);
    // A press on `#empty`, which has no `Behavior` and so is a node no focus
    // move can land on: the tree's stand-in for a click outside every widget.
    let nowhere = centre([300.0, 300.0, 100.0, 20.0]);
    let click = |ui: &mut Ui, at: Vec2, volume: &mut f32| {
        slide(ui, press(at), NavInput::default(), volume);
        slide(ui, release(at), NavInput::default(), volume)
    };

    slide(&mut ui, idle(), NavInput::default(), &mut volume);
    let page = click(&mut ui, on_volume, &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Began);
    assert_eq!(page.name(ui.focused()), Some("volume"));

    let page = click(&mut ui, nowhere, &mut volume);
    assert_eq!(
        page.name(ui.focused()),
        Some("volume"),
        "a click on an unfocusable node took focus away on its own, \
         so this call would not be needed",
    );
    ui.clear_focus();
    assert_eq!(ui.focused(), None, "focus was not taken away");
    let page = slide(&mut ui, idle(), NavInput::default(), &mut volume);
    assert_eq!(page.name(ui.focused()), None, "focus came back");

    // And the engaged half, which that click had already committed: engage
    // again and take it away with nothing else touching the tree.
    let page = click(&mut ui, on_volume, &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Began);
    assert_eq!(ui.engaged(), Some(page.get("volume").key));

    ui.clear_focus();
    assert_eq!(ui.engaged(), None, "the engagement was not taken away");
    let page = slide(&mut ui, idle(), NavInput::default(), &mut volume);
    assert_eq!(
        page.get("volume").engagement,
        Engagement::Committed,
        "the engaged widget was not told it had been committed",
    );
    assert_eq!(volume, 5.0, "the commit restored the snapshot as a cancel");
    let page = slide(&mut ui, idle(), NavInput::default(), &mut volume);
    assert_eq!(page.get("volume").engagement, Engagement::Idle);
    assert_eq!(page.name(ui.focused()), None);
}

/// **A button fires on accept through `clicked`, with no engaged state**, and
/// a disabled button is neither focused, accepted nor clicked.
#[test]
fn a_button_fires_on_accept_and_a_disabled_one_never_fires() {
    const DISABLED: Behavior = Behavior {
        disabled: true,
        ..Behavior::BUTTON
    };
    const PAGE: &[Spec] = &[
        leaf("#ok", [0.0, 0.0, 100.0, 20.0], B),
        leaf("#off", [0.0, 40.0, 100.0, 20.0], DISABLED),
    ];
    let mut ui = Ui::new();
    step(&mut ui, PAGE, idle(), NavInput::default());
    step(&mut ui, PAGE, idle(), NavInput::NAVIGATION);
    let page = step(&mut ui, PAGE, idle(), NavInput::ACCEPT);
    let ok = page.get("ok");
    assert!(ok.clicked && ok.focused, "{ok:?}");
    assert_eq!(ok.engagement, Engagement::Idle);
    assert_eq!(ui.engaged(), None);
    let page = step(&mut ui, PAGE, idle(), NavInput::default());
    assert!(!page.get("ok").clicked, "accept fired for a second frame");

    step(&mut ui, PAGE, idle(), DOWN);
    assert_eq!(
        page.name(ui.focused()),
        Some("ok"),
        "focus rested on a disabled button"
    );
    let off = centre([0.0, 40.0, 100.0, 20.0]);
    step(&mut ui, PAGE, press(off), NavInput::default());
    let page = step(&mut ui, PAGE, release(off), NavInput::default());
    assert!(!page.get("off").clicked, "a disabled button was clicked");
    assert_eq!(page.name(ui.focused()), Some("ok"));
}

// ---------------------------------------------------------------------------
// The cascade
// ---------------------------------------------------------------------------

/// The style `key` was built with this frame.
fn style_of(ui: &Ui, key: NodeKey) -> NodeStyle {
    ui.nodes
        .iter()
        .find(|node| node.key == key)
        .expect("built this frame")
        .style
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// **`:focus`, `:engaged` and `:disabled` reach the cascade, and each change
/// re-resolves exactly the nodes whose rules test it**: a move re-resolves the
/// node focus left and the node it reached, an engagement the engaged node,
/// a node turning disabled that node, and a frame where nothing changed
/// nothing — with plain nodes on the page that never re-resolve.
#[test]
fn focus_engaged_and_disabled_reach_the_cascade_and_re_resolve_only_their_dependents() {
    const SHEET: &str = "
        .b:focus { background: #ff0000; }
        .e:engaged { background: #0000ff; }
        .d:disabled { color: #0000ff; }
    ";
    let page = |disabled: bool| -> Vec<Spec> {
        let off = Behavior {
            disabled,
            ..Behavior::BUTTON
        };
        vec![
            leaf("#b0.b", [0.0, 0.0, 50.0, 20.0], B),
            leaf("#b1.b", [60.0, 0.0, 50.0, 20.0], B),
            leaf("#e.e", [120.0, 0.0, 50.0, 20.0], Behavior::ENGAGE),
            leaf("#d.d", [0.0, 40.0, 50.0, 20.0], off),
            leaf("#p0", [0.0, 80.0, 50.0, 20.0], Behavior::NONE),
            leaf("#p1", [60.0, 80.0, 50.0, 20.0], Behavior::NONE),
        ]
    };
    let enabled = page(false);
    let disabled = page(true);
    let mut ui = Ui::new();
    ui.add_stylesheet("pseudo.css", SHEET);
    let first = step(&mut ui, &enabled, idle(), NavInput::default());
    let resolves = |ui: &Ui| ui.style_stats().resolves;

    step(&mut ui, &enabled, idle(), NavInput::default());
    assert_eq!(resolves(&ui), 0, "an unchanged frame resolved something");

    // Landing on b0 in navigation mode: b0 alone.
    step(&mut ui, &enabled, idle(), NavInput::NAVIGATION);
    assert_eq!(resolves(&ui), 1);
    assert_eq!(style_of(&ui, first.key("b0")).background, RED);

    step(&mut ui, &enabled, idle(), RIGHT);
    assert_eq!(
        resolves(&ui),
        2,
        "a move re-resolved more than the two it touched"
    );
    assert_eq!(style_of(&ui, first.key("b0")).background, [0.0; 4]);
    assert_eq!(style_of(&ui, first.key("b1")).background, RED);

    step(&mut ui, &enabled, idle(), NavInput::NAVIGATION);
    assert_eq!(resolves(&ui), 0);

    // Onto the engage widget: `.e` has no `:focus` rule, so the move
    // re-resolves only b1; then accept re-resolves only e.
    step(&mut ui, &enabled, idle(), RIGHT);
    assert_eq!(resolves(&ui), 1);
    step(&mut ui, &enabled, idle(), NavInput::ACCEPT);
    assert_eq!(
        resolves(&ui),
        1,
        "engaging re-resolved more than the widget"
    );
    assert_eq!(style_of(&ui, first.key("e")).background, BLUE);
    step(&mut ui, &enabled, idle(), NavInput::ACCEPT);
    assert_eq!(resolves(&ui), 1);

    step(&mut ui, &disabled, idle(), NavInput::NAVIGATION);
    assert_eq!(resolves(&ui), 1, "disabling re-resolved more than the node");
    assert_eq!(style_of(&ui, first.key("d")).color, BLUE);
    assert_eq!(
        style_of(&ui, first.key("b0")).color,
        NodeStyle::DEFAULT.color
    );
}

/// **The mixed-input rule**: while the pointer drives, hover styles show and
/// the focus ring does not, though a click still sets focus; once the pad
/// speaks, the ring shows on what the click focused and hover does not; and
/// a pad press with nothing focused lands on the hovered button and moves no
/// further.
#[test]
fn the_pointer_shows_hover_the_pad_shows_focus_and_a_click_sets_focus() {
    const SHEET: &str = ".b:hover { background: #0000ff; } .b:focus { background: #ff0000; }";
    const ROW: &[Spec] = &[
        leaf("#b0.b", [0.0, 0.0, 50.0, 20.0], B),
        leaf("#b1.b", [60.0, 0.0, 50.0, 20.0], B),
        leaf("#b2.b", [120.0, 0.0, 50.0, 20.0], B),
        leaf("#b3.b", [180.0, 0.0, 50.0, 20.0], B),
    ];
    let on = |x: f32| Vec2::new(x, 10.0);
    let mut ui = Ui::new();
    ui.add_stylesheet("mixed.css", SHEET);
    let page = step(&mut ui, ROW, idle(), NavInput::default());
    let background = |ui: &Ui, name: &str| style_of(ui, page.key(name)).background;

    step(&mut ui, ROW, press(on(80.0)), NavInput::default());
    step(&mut ui, ROW, release(on(80.0)), NavInput::default());
    assert_eq!(
        page.name(ui.focused()),
        Some("b1"),
        "a click did not set focus"
    );
    step(
        &mut ui,
        ROW,
        PointerInput::hovering(on(20.0)),
        NavInput::default(),
    );
    assert_eq!(
        background(&ui, "b0"),
        BLUE,
        "hover did not show for the pointer"
    );
    assert_eq!(
        background(&ui, "b1"),
        [0.0; 4],
        "the ring showed for the pointer"
    );

    step(
        &mut ui,
        ROW,
        PointerInput::hovering(on(20.0)),
        NavInput::NAVIGATION,
    );
    assert_eq!(
        background(&ui, "b1"),
        RED,
        "the ring did not show for the pad"
    );
    assert_eq!(background(&ui, "b0"), [0.0; 4], "hover showed for the pad");
    step(&mut ui, ROW, PointerInput::hovering(on(20.0)), RIGHT);
    assert_eq!(
        page.name(ui.focused()),
        Some("b2"),
        "the pad did not continue from the click"
    );

    let mut fresh = Ui::new();
    step(&mut fresh, ROW, idle(), NavInput::default());
    step(
        &mut fresh,
        ROW,
        PointerInput::hovering(on(140.0)),
        NavInput::default(),
    );
    step(&mut fresh, ROW, PointerInput::hovering(on(140.0)), RIGHT);
    assert_eq!(
        page.name(fresh.focused()),
        Some("b2"),
        "a first press did not land on the hovered button, or moved past it"
    );
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

fn emitted(ui: &Ui) -> DrawList {
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

/// **A focus ring from the stylesheet is drawn round the focused node and no
/// other**: one outline in the ring's colour, `outline-offset` plus its width
/// outside the node's border box.
#[test]
fn the_focus_ring_is_drawn_round_the_focused_node_only() {
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "ring.css",
        "#grid > * { outline-color: #ff0000; outline-offset: 1px; } #grid > *:focus { outline-width: 2px; }",
    );
    let page = step(&mut ui, GRID, idle(), NavInput::default());
    step(&mut ui, GRID, idle(), NavInput::NAVIGATION);
    step(&mut ui, GRID, idle(), DOWN);
    step(&mut ui, GRID, idle(), NavInput::NAVIGATION);
    let rings: Vec<(Vec2, Vec2, f32)> = emitted(&ui)
        .commands()
        .iter()
        .filter_map(|command| match *command {
            DrawCommand::RectOutline {
                min,
                max,
                thickness,
                color,
            } if color == RED => Some((min, max, thickness)),
            _ => None,
        })
        .collect();
    let (min, max) = ui.rect(page.key("b0")).expect("laid out");
    assert_eq!(
        rings,
        [(min - Vec2::splat(3.0), max + Vec2::splat(3.0), 2.0)],
        "the ring is not exactly one, round b0"
    );

    // Two filled rows that touch, the first focused: its ring overlaps the
    // second, which is built after it, and must still be drawn over it.
    const ROWS: &[Spec] = &[
        leaf("#top.row", [0.0, 0.0, 50.0, 20.0], B),
        leaf("#next.row", [0.0, 20.0, 50.0, 20.0], B),
    ];
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "rows.css",
        ".row { background: #0000ff; } .row:focus { outline: 2px #ff0000; }",
    );
    step(&mut ui, ROWS, idle(), NavInput::default());
    step(&mut ui, ROWS, idle(), NavInput::NAVIGATION);
    let list = emitted(&ui);
    let position = |wanted: [f32; 4]| {
        list.commands().iter().rposition(|command| match *command {
            DrawCommand::Rect { color, .. } | DrawCommand::RectOutline { color, .. } => {
                color == wanted
            }
            _ => false,
        })
    };
    assert!(
        position(RED) > position(BLUE),
        "the ring is drawn before the sibling that covers it: {:?}",
        list.commands()
    );
}

/// **The debug overlay draws nothing until it is switched on**, and then
/// outlines the focus path and every scored candidate with its score, the
/// chosen one in its own colour.
#[test]
fn the_debug_overlay_is_off_by_default_and_draws_every_scored_candidate() {
    let mut ui = Ui::new();
    let page = step(&mut ui, GRID, idle(), NavInput::default());
    step(&mut ui, GRID, idle(), NavInput::NAVIGATION);
    step(&mut ui, GRID, idle(), DOWN);
    assert_eq!(page.name(ui.focused()), Some("b0"));
    let scored = ui.nav_scores().to_vec();
    assert_eq!(
        scored.len(),
        6,
        "every cell below a0 lies downward: {scored:?}"
    );
    assert_eq!(
        scored
            .iter()
            .filter(|score| score.chosen)
            .map(|score| score.key)
            .collect::<Vec<_>>(),
        [page.key("b0")]
    );

    let debug_colours = [
        NAV_DEBUG_PATH,
        NAV_DEBUG_CHOSEN,
        NAV_DEBUG_BEAM,
        NAV_DEBUG_OUTSIDE,
    ];
    let off = emitted(&ui);
    let coloured = |list: &DrawList| {
        list.commands()
            .iter()
            .filter(|command| match command {
                DrawCommand::RectOutline { color, .. } | DrawCommand::Text { color, .. } => {
                    debug_colours.contains(color)
                }
                _ => false,
            })
            .count()
    };
    assert_eq!(coloured(&off), 0, "the overlay drew while off");

    ui.set_nav_debug(true);
    let on = emitted(&ui);
    let texts: Vec<(String, [f32; 4])> = on
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts
            .iter()
            .filter(|(_, colour)| *colour != NAV_DEBUG_PATH)
            .count(),
        scored.len(),
        "not one score per candidate: {texts:?}"
    );
    assert!(
        texts.contains(&("#surface > #grid > #b0".to_owned(), NAV_DEBUG_PATH)),
        "the focus path is missing: {texts:?}"
    );
    let chosen = format!(
        "b{:.0}",
        scored
            .iter()
            .find(|s| s.chosen)
            .expect("one")
            .score
            .distance
    );
    assert!(
        texts.contains(&(chosen.clone(), NAV_DEBUG_CHOSEN)),
        "the chosen candidate's score `{chosen}` is missing: {texts:?}"
    );
    assert_eq!(
        coloured(&on),
        // An outline and a text per candidate; an outline per path node and
        // the path's text.
        2 * scored.len() + 3 + 1
    );
}
