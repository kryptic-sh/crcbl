//! The split pane: dragging, the minimum sizes, the engaged step and cancel,
//! and state across rebuilds.

use super::*;
use crate::tree::{Engagement, Response, SPLIT_NAV_STEP, SplitAxis};

/// How wide the split is, in pixels.
const WIDTH: f32 = 200.0;
/// Each pane's minimum width.
const MIN: [f32; 2] = [40.0, 50.0];
/// `default.css`'s divider width for a row split.
const DIVIDER: f32 = 4.0;

/// The divider's response and both panes' keys.
struct SplitPage {
    divider: Response,
    first: NodeKey,
    second: NodeKey,
}

fn split_page(ui: &mut Ui, pointer: PointerInput, nav: NavInput) -> SplitPage {
    frame(ui, pointer, nav, |ui| {
        let (mut first, mut second) = (None, None);
        let mut divider = None;
        ui.block(
            "",
            &[
                Declaration::Width(LengthAuto::Px(WIDTH)),
                Declaration::Height(LengthAuto::Px(60.0)),
            ],
            |ui| {
                divider = Some(ui.split(
                    "#split",
                    SplitAxis::Row,
                    MIN,
                    |ui| first = ui.block("#left", &[], |_| {}).key.into(),
                    |ui| second = ui.block("#right", &[], |_| {}).key.into(),
                ));
            },
        );
        let pane = |inner: Option<NodeKey>| {
            let inner = inner.expect("built");
            ui.nodes
                .iter()
                .find(|node| node.key == inner)
                .and_then(|node| node.parent)
                .map(|parent| ui.nodes[parent].key)
                .expect("a pane holds it")
        };
        SplitPage {
            divider: divider.expect("built"),
            first: pane(first),
            second: pane(second),
        }
    })
}

fn width(ui: &Ui, key: NodeKey) -> f32 {
    let (min, max) = rect(ui, key);
    max.x - min.x
}

/// **Dragging the divider moves it with the pointer, from where the press
/// began, and never past either pane's minimum** — the second pane keeps its
/// fifty pixels at the right end and the first its forty at the left — and
/// the position survives rebuilds.
#[test]
fn dragging_the_divider_moves_it_and_clamps_to_both_minimums() {
    let mut ui = Ui::new();
    split_page(&mut ui, idle(), NavInput::default());
    let page = split_page(&mut ui, idle(), NavInput::default());
    let shared = (WIDTH - DIVIDER) / 2.0;
    assert_eq!(
        width(&ui, page.first),
        shared,
        "the panes did not start equal"
    );
    assert_eq!(width(&ui, page.divider.key), DIVIDER);

    let grip = centre(&ui, page.divider.key);
    split_page(&mut ui, press(grip), NavInput::default());
    let dragged = split_page(
        &mut ui,
        press(grip + Vec2::new(-30.0, 5.0)),
        NavInput::default(),
    );
    assert!(dragged.divider.changed);
    split_page(
        &mut ui,
        press(grip + Vec2::new(-30.0, 5.0)),
        NavInput::default(),
    );
    assert_eq!(
        width(&ui, page.first),
        shared - 30.0,
        "the drag did not carry the divider"
    );

    split_page(
        &mut ui,
        press(grip + Vec2::new(-500.0, 0.0)),
        NavInput::default(),
    );
    split_page(
        &mut ui,
        press(grip + Vec2::new(-500.0, 0.0)),
        NavInput::default(),
    );
    assert_eq!(
        width(&ui, page.first),
        MIN[0],
        "the first pane went under its minimum"
    );

    split_page(
        &mut ui,
        press(grip + Vec2::new(500.0, 0.0)),
        NavInput::default(),
    );
    assert_eq!(
        width(&ui, page.second),
        MIN[1],
        "the second pane went under its minimum"
    );
    assert_eq!(width(&ui, page.first), WIDTH - DIVIDER - MIN[1]);

    // Back to thirty pixels left, and released there — over the divider, which
    // followed the pointer — so the release is a click that ends a drag.
    let back = grip + Vec2::new(-30.0, 0.0);
    split_page(&mut ui, press(back), NavInput::default());
    let released = split_page(&mut ui, release(back), NavInput::default());
    assert!(
        released.divider.focused,
        "the release was not over the divider, so it proves nothing about engaging"
    );
    assert_eq!(ui.engaged(), None, "a drag engaged the divider");

    for _ in 0..4 {
        split_page(&mut ui, idle(), NavInput::default());
    }
    assert_eq!(
        width(&ui, page.first),
        shared - 30.0,
        "a rebuild moved the divider"
    );
}

/// **An engaged divider steps by `SPLIT_NAV_STEP` along its axis only, back
/// puts it where it was when it engaged, and accept keeps the step.**
#[test]
fn an_engaged_divider_steps_cancels_and_commits() {
    let mut ui = Ui::new();
    let page = split_page(&mut ui, idle(), NavInput::default());
    ui.set_focus(page.divider.key);
    split_page(&mut ui, idle(), NavInput::NAVIGATION);
    let start = width(&ui, page.first);

    let engaged = split_page(&mut ui, idle(), NavInput::ACCEPT);
    assert_eq!(engaged.divider.engagement, Engagement::Began);
    for nav in [RIGHT, RIGHT, UP, DOWN] {
        split_page(&mut ui, idle(), nav);
    }
    split_page(&mut ui, idle(), NavInput::default());
    assert_eq!(
        width(&ui, page.first),
        start + 2.0 * SPLIT_NAV_STEP,
        "two rights, and up and down, from the start"
    );

    split_page(&mut ui, idle(), NavInput::BACK);
    split_page(&mut ui, idle(), NavInput::default());
    assert_eq!(
        width(&ui, page.first),
        start,
        "back did not put the divider back"
    );

    split_page(&mut ui, idle(), NavInput::ACCEPT);
    split_page(&mut ui, idle(), LEFT);
    split_page(&mut ui, idle(), NavInput::ACCEPT);
    split_page(&mut ui, idle(), NavInput::BACK);
    split_page(&mut ui, idle(), NavInput::default());
    assert_eq!(
        width(&ui, page.first),
        start - SPLIT_NAV_STEP,
        "commit lost the step"
    );
}
