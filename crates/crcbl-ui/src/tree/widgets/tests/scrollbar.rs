//! The scrollbar: a handle sized and placed by its container's view, moved by
//! a drag that scrolls the container, and absent with nothing to scroll.

use super::*;
use crate::tree::{MIN_SCROLL_HANDLE, Response};

/// A row's height, in pixels.
const ROW: f32 = 20.0;
/// The list's and the track's height: five rows.
const VIEW: f32 = 100.0;
/// The track's width.
const TRACK: f32 = 10.0;

/// What one frame of the page built.
struct Page {
    list: Response,
    /// The scrollbar's response: its handle's, when it has one.
    bar: Response,
    /// The track: the pane's second child.
    track: NodeKey,
}

impl Page {
    /// The handle, if the track holds one.
    fn handle(&self, ui: &Ui) -> Option<NodeKey> {
        children_of(ui, self.track).first().copied()
    }
}

/// One frame of a list of `rows` rows with a scrollbar beside it.
fn page(ui: &mut Ui, rows: usize, pointer: PointerInput) -> Page {
    frame(ui, pointer, NavInput::default(), |ui| {
        let mut built = None;
        let pane = ui.block(".pane", &[], |ui| {
            let list = ui.list("#rows", rows, ROW, |_, _| {});
            let bar = ui.scrollbar("#bar", list.key);
            built = Some((list, bar));
        });
        let (list, bar) = built.expect("built");
        let track = children_of(ui, pane.key)[1];
        Page { list, bar, track }
    })
}

/// Nothing between any border box and its content box, so the handle's
/// geometry is plain arithmetic.
fn sheet(ui: &mut Ui) {
    ui.add_stylesheet(
        "scrollbar.css",
        &format!(
            ".pane {{ flex-direction: row; height: {VIEW}px; }}
             #rows {{ width: 100px; height: {VIEW}px; border-width: 0; padding: 0; }}
             #bar {{ width: {TRACK}px; height: {VIEW}px; border-width: 0; padding: 0; }}"
        ),
    );
}

/// The handle's top, relative to the track, and its height.
fn span(ui: &Ui, page: &Page) -> (f32, f32) {
    let handle = page.handle(ui).expect("a handle");
    let (track, _) = rect(ui, page.track);
    let (top, bottom) = rect(ui, handle);
    (top.y - track.y, bottom.y - top.y)
}

/// **The handle is the view's share of the track, as far down its spare
/// length as the offset is down the reach.** Twenty rows are four hundred
/// pixels of content in a hundred-pixel view: a quarter, so a twenty-five
/// pixel handle with seventy-five to travel over three hundred of reach.
/// Offsets are picked to land the handle on whole pixels, which is what
/// layout rounds every rectangle to.
#[test]
fn the_handle_is_the_views_share_at_the_offsets_share() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    let first = page(&mut ui, 20, idle());
    assert_eq!(
        first.handle(&ui),
        None,
        "no handle before the track has been laid out"
    );
    let shown = page(&mut ui, 20, idle());
    assert_eq!(span(&ui, &shown), (0.0, 25.0));

    ui.set_scroll_offset_of(shown.list.key, Vec2::new(0.0, 120.0));
    let halfway = page(&mut ui, 20, idle());
    assert_eq!(span(&ui, &halfway), (30.0, 25.0));
    assert!(!halfway.bar.changed, "nothing dragged it");
}

/// **A drag keeps the handle under the pointer**: fifteen pixels down the
/// track is fifteen seventy-fifths of the reach, from where the offset was
/// when the press began; and a drag past the end stops at the end.
#[test]
fn dragging_the_handle_scrolls_the_list_by_the_same_share() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    page(&mut ui, 20, idle());
    let shown = page(&mut ui, 20, idle());
    ui.set_scroll_offset_of(shown.list.key, Vec2::new(0.0, 120.0));
    let at = page(&mut ui, 20, idle());
    let grip = centre(&ui, at.handle(&ui).expect("a handle"));

    let pressed = page(&mut ui, 20, press(grip));
    assert!(
        !pressed.bar.changed,
        "a press that has not moved scrolls nothing"
    );
    let dragged = page(&mut ui, 20, press(grip + Vec2::new(0.0, 15.0)));
    assert!(dragged.bar.changed);
    assert_eq!(ui.scroll_offset_of(dragged.list.key).y, 180.0);
    assert_eq!(span(&ui, &dragged).0, 45.0, "the handle followed");
    let held = page(&mut ui, 20, press(grip + Vec2::new(0.0, 15.0)));
    assert_eq!(
        ui.scroll_offset_of(held.list.key).y,
        180.0,
        "holding still scrolls no further: the drag is measured from the press"
    );
    assert!(!held.bar.changed);

    let past = page(&mut ui, 20, press(grip + Vec2::new(0.0, 500.0)));
    assert_eq!(ui.scroll_offset_of(past.list.key).y, 300.0);
    assert_eq!(span(&ui, &past).0, 75.0);

    let released = page(&mut ui, 20, release(grip + Vec2::new(0.0, 500.0)));
    let after = page(&mut ui, 20, idle());
    assert_eq!(ui.scroll_offset_of(after.list.key).y, 300.0);
    assert!(!released.bar.changed && !after.bar.changed);
}

/// A thousand rows would give a handle of half a pixel; it is drawn at
/// [`MIN_SCROLL_HANDLE`], and a list that fits has no handle at all.
#[test]
fn a_handle_is_never_shorter_than_its_minimum_nor_there_with_nothing_to_scroll() {
    let mut ui = Ui::new();
    sheet(&mut ui);
    page(&mut ui, 1000, idle());
    let long = page(&mut ui, 1000, idle());
    assert_eq!(span(&ui, &long).1, MIN_SCROLL_HANDLE);

    let mut ui = Ui::new();
    sheet(&mut ui);
    page(&mut ui, 3, idle());
    let short = page(&mut ui, 3, idle());
    assert_eq!(short.handle(&ui), None);
    assert_eq!(
        short.bar.key, short.track,
        "the track's response, with no handle"
    );
}
