//! The tab strip: one pane built, switching by pointer and by key, and the tab
//! that is showing remembered by its title across a rebuild that reorders them.

use super::*;
use crate::tree::Response;

/// What one frame of the tabs page built.
struct TabsPage {
    tabs: Response,
    /// The index `pane` was called with, if it was called.
    pane: Option<usize>,
    /// The key of the block the pane built, so it can be looked for.
    body: Option<NodeKey>,
}

fn tabs_page(ui: &mut Ui, titles: &[&str], pointer: PointerInput, nav: NavInput) -> TabsPage {
    tabs_page_enabled(ui, titles, pointer, nav, true)
}

fn tabs_page_enabled(
    ui: &mut Ui,
    titles: &[&str],
    pointer: PointerInput,
    nav: NavInput,
    enabled: bool,
) -> TabsPage {
    frame(ui, pointer, nav, |ui| {
        let (mut pane, mut body, mut tabs) = (None, None, None);
        ui.enabled(enabled, |ui| {
            tabs = Some(ui.tabs("#views", titles, |ui, index| {
                pane = Some(index);
                body = Some(ui.block("#body", &[], |_| {}).key);
            }));
        });
        TabsPage {
            tabs: tabs.expect("built"),
            pane,
            body,
        }
    })
}

/// The key of the tab whose label is `title`, from the frame just built.
fn tab_key(ui: &Ui, page: &TabsPage, titles: &[&str], title: &str) -> NodeKey {
    let strip = children_of(ui, page.tabs.key)[0];
    let index = titles
        .iter()
        .position(|&each| each == title)
        .expect("a tab");
    children_of(ui, strip)[index]
}

const TITLES: [&str; 3] = ["Scene", "Assets", "Log"];

/// **Only the showing tab's pane is built at all**, and a click on a tab shows
/// it — the one gesture, whichever tab it lands on.
#[test]
fn one_pane_is_built_and_a_click_shows_a_tab() {
    let mut ui = Ui::new();
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    assert_eq!(first.pane, Some(0), "the first tab did not show");
    let panes = children_of(&ui, first.tabs.key);
    assert_eq!(
        panes.len(),
        2,
        "not exactly a strip and one pane: {panes:?}"
    );
    assert_eq!(
        children_of(&ui, panes[0]).len(),
        TITLES.len(),
        "the strip is not one tab per title"
    );

    let log = centre(&ui, tab_key(&ui, &first, &TITLES, "Log"));
    tabs_page(&mut ui, &TITLES, press(log), NavInput::default());
    let shown = tabs_page(&mut ui, &TITLES, release(log), NavInput::default());
    assert_eq!(shown.pane, Some(2), "the click did not show the tab");
    assert!(shown.tabs.changed, "the switch was not reported");
    assert_eq!(
        children_of(&ui, shown.tabs.key).len(),
        2,
        "more than one pane was built"
    );

    let settled = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    assert_eq!(settled.pane, Some(2), "the tab did not stay shown");
    assert!(!settled.tabs.changed, "a still frame reported a switch");
}

/// **The tab that is showing survives a rebuild that reorders the strip**,
/// because it is remembered by its title and not by its index — and a title the
/// strip no longer holds falls back to the first tab.
#[test]
fn the_showing_tab_follows_its_title_through_a_reorder() {
    let mut ui = Ui::new();
    tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let assets = centre(&ui, tab_key(&ui, &first, &TITLES, "Assets"));
    tabs_page(&mut ui, &TITLES, press(assets), NavInput::default());
    let shown = tabs_page(&mut ui, &TITLES, release(assets), NavInput::default());
    assert_eq!(shown.pane, Some(1));

    // The same tabs, in a different order: "Assets" is now last.
    let reordered = ["Log", "Scene", "Assets"];
    let after = tabs_page(&mut ui, &reordered, idle(), NavInput::default());
    assert_eq!(
        after.pane,
        Some(2),
        "the showing tab followed the index, not the title"
    );
    assert!(!after.tabs.changed, "a reorder was reported as a switch");

    // Take the showing tab away, and the first shows instead.
    let without = ["Log", "Scene"];
    let gone = tabs_page(&mut ui, &without, idle(), NavInput::default());
    assert_eq!(gone.pane, Some(0), "a missing tab did not fall back");

    // No tabs at all: the strip is empty and the pane is not built.
    let empty = tabs_page(&mut ui, &[], idle(), NavInput::default());
    assert_eq!(empty.pane, None, "a pane was built with no tabs");
    assert_eq!(
        children_of(&ui, empty.tabs.key).len(),
        1,
        "a pane was built"
    );
}

/// **The strip is keyboard reachable**: focus walks to every tab in tree order
/// and accept shows the focused one, with no pointer at all.
#[test]
fn focus_walks_the_strip_and_accept_shows_a_tab() {
    let mut ui = Ui::new();
    tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::NAVIGATION);
    let keys: Vec<NodeKey> = TITLES
        .iter()
        .map(|&title| tab_key(&ui, &first, &TITLES, title))
        .collect();
    assert_eq!(ui.focused(), Some(keys[0]), "focus did not land on a tab");

    tabs_page(&mut ui, &TITLES, idle(), NavInput::NEXT);
    tabs_page(&mut ui, &TITLES, idle(), NavInput::NEXT);
    assert_eq!(ui.focused(), Some(keys[2]), "next did not walk the strip");

    let shown = tabs_page(&mut ui, &TITLES, idle(), NavInput::ACCEPT);
    assert_eq!(shown.pane, Some(2), "accept did not show the focused tab");
    assert!(shown.tabs.changed);

    // The pane's own contents are built, and only inside the showing pane.
    assert!(shown.body.is_some(), "the pane builder did not run");
    let pane = children_of(&ui, shown.tabs.key)[1];
    assert_eq!(
        children_of(&ui, pane),
        [shown.body.expect("built")],
        "the pane holds something other than its builder's block"
    );
}

/// **The showing tab is `:checked` and the others are not**, so `default.css`
/// paints exactly one tab in the accent.
#[test]
fn only_the_showing_tab_is_marked() {
    let accent = linear("#3d8bfd");
    let mut ui = Ui::new();
    tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let marked = |ui: &Ui, page: &TabsPage| -> Vec<NodeKey> {
        TITLES
            .iter()
            .map(|&title| tab_key(ui, page, &TITLES, title))
            .filter(|&key| style_of(ui, key).background == accent)
            .collect()
    };
    assert_eq!(
        marked(&ui, &first),
        [tab_key(&ui, &first, &TITLES, "Scene")]
    );

    let log = centre(&ui, tab_key(&ui, &first, &TITLES, "Log"));
    tabs_page(&mut ui, &TITLES, press(log), NavInput::default());
    tabs_page(&mut ui, &TITLES, release(log), NavInput::default());
    let shown = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    assert_eq!(
        marked(&ui, &shown),
        [tab_key(&ui, &shown, &TITLES, "Log")],
        "not exactly the showing tab marked"
    );
}

#[test]
fn disabling_tabs_on_release_keeps_the_showing_pane() {
    let mut ui = Ui::new();
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let log = centre(&ui, tab_key(&ui, &first, &TITLES, "Log"));
    tabs_page(&mut ui, &TITLES, press(log), NavInput::default());
    let disabled = tabs_page_enabled(&mut ui, &TITLES, release(log), NavInput::default(), false);
    assert_eq!(disabled.pane, Some(0), "a disabled tab took the release");
    assert!(!disabled.tabs.changed, "a disabled tab reported a switch");

    tabs_page_enabled(&mut ui, &TITLES, press(log), NavInput::default(), false);
    let still_disabled =
        tabs_page_enabled(&mut ui, &TITLES, release(log), NavInput::default(), false);
    assert_eq!(still_disabled.pane, Some(0));
    assert!(!still_disabled.tabs.changed);

    let enabled = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    assert_eq!(enabled.pane, Some(0), "the disabled click was deferred");
    assert!(!enabled.tabs.changed);
    tabs_page(&mut ui, &TITLES, press(log), NavInput::default());
    let switched = tabs_page(&mut ui, &TITLES, release(log), NavInput::default());
    assert_eq!(switched.pane, Some(2), "re-enabling did not restore clicks");
    assert!(switched.tabs.changed);
}

fn narrow_ui() -> Ui {
    let mut ui = Ui::new();
    ui.add_stylesheet("narrow.css", "#views { width: 100px; }");
    ui
}

#[test]
fn navigating_a_narrow_tab_strip_reveals_the_focused_tab() {
    let mut ui = narrow_ui();
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let strip = children_of(&ui, first.tabs.key)[0];
    let log = tab_key(&ui, &first, &TITLES, "Log");
    assert!(rect(&ui, log).1.x > rect(&ui, strip).1.x);

    tabs_page(&mut ui, &TITLES, idle(), NavInput::NAVIGATION);
    tabs_page(&mut ui, &TITLES, idle(), NavInput::NEXT);
    let reached = tabs_page(&mut ui, &TITLES, idle(), NavInput::NEXT);
    assert_eq!(ui.focused(), Some(log));
    assert_eq!(reached.pane, Some(0), "focus activated the tab");
    assert!(
        ui.scroll_offset_of(strip).x > 0.0,
        "the strip did not scroll"
    );
    let (left, right) = rect(&ui, strip);
    let (min, max) = rect(&ui, log);
    assert!(
        min.x >= left.x && max.x <= right.x,
        "the focused tab is clipped"
    );
    let shown = tabs_page(&mut ui, &TITLES, idle(), NavInput::ACCEPT);
    assert_eq!(shown.pane, Some(2));

    tabs_page(&mut ui, &TITLES, idle(), NavInput::PREV);
    let returned = tabs_page(&mut ui, &TITLES, idle(), NavInput::PREV);
    let scene = tab_key(&ui, &returned, &TITLES, "Scene");
    assert_eq!(ui.focused(), Some(scene));
    assert_eq!(ui.scroll_offset_of(strip), Vec2::ZERO);
    assert_eq!(returned.pane, Some(2), "focus changed the showing pane");
}

#[test]
fn a_horizontal_wheel_reveals_a_tab_for_pointer_activation() {
    let mut ui = narrow_ui();
    let first = tabs_page(&mut ui, &TITLES, idle(), NavInput::default());
    let strip = children_of(&ui, first.tabs.key)[0];
    let over_strip = PointerInput::hovering(centre(&ui, strip));
    tabs_page(&mut ui, &TITLES, over_strip, NavInput::default());
    assert!(ui.scroll_wheel(Vec2::new(PAGE, 0.0)));
    let scrolled = tabs_page(&mut ui, &TITLES, over_strip, NavInput::default());
    assert_eq!(scrolled.pane, Some(0), "scrolling activated a tab");
    assert!(
        !ui.scroll_wheel(Vec2::new(PAGE, 0.0)),
        "the strip overscrolled"
    );
    let log = tab_key(&ui, &scrolled, &TITLES, "Log");
    let (left, right) = rect(&ui, strip);
    let (min, max) = rect(&ui, log);
    assert!(
        min.x >= left.x && max.x <= right.x,
        "the tab stayed clipped"
    );
    let at = centre(&ui, log);
    tabs_page(&mut ui, &TITLES, press(at), NavInput::default());
    let shown = tabs_page(&mut ui, &TITLES, release(at), NavInput::default());
    assert_eq!(shown.pane, Some(2), "the revealed tab was not clickable");
    assert!(shown.tabs.changed);
}
