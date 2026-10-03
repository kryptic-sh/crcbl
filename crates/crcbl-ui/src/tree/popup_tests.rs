//! The pop-up layer: where a pop-up is placed and drawn, what it is hit and
//! clipped by, and how it closes and hands focus back — each held to the rule
//! in `popup.rs` it exists for.

use glam::Vec2;

use super::popup::hang;
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::style::Declaration;

/// The viewport every page is laid out in.
const VIEW: Vec2 = Vec2::new(200.0, 150.0);
/// How far the panel holding the anchor is scrolled.
const SCROLL: f32 = 10.0;
/// The pop-up's background, to find it in a draw list by.
const MENU: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
/// The background of the block built after the panel, which the pop-up
/// covers.
const LATER: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

const fn px(value: f32) -> LengthAuto {
    LengthAuto::Px(value)
}

/// What a page builds beside its fixed blocks.
#[derive(Clone, Copy, Default)]
struct Options {
    /// Open the pop-up this frame.
    opens: bool,
    /// Leave the pop-up unbuilt this frame, open or not.
    unbuilt: bool,
    /// Open a second pop-up hanging from the first's first item.
    sub: bool,
}

/// What a page built.
struct Page {
    anchor: Response,
    later: Response,
    /// The pop-up's root, if it was built.
    menu: Option<Response>,
    /// Its two items, if it was built.
    items: Vec<Response>,
}

/// One frame: a `VIEW`-sized column holding a clipped panel — scrolled by
/// `SCROLL`, holding a spacer and then the anchor, a button that opens the
/// pop-up when clicked — and then a button `later` that the pop-up, hanging
/// out of the panel, covers.
fn page(ui: &mut Ui, pointer: PointerInput, nav: NavInput, options: Options) -> Page {
    ui.begin_frame_with(pointer, nav);
    let mut anchor = None;
    let mut menu = None;
    let mut items = Vec::new();
    let mut later = None;
    let page = [
        Declaration::Width(px(VIEW.x)),
        Declaration::Height(px(VIEW.y)),
        Declaration::FlexDirection(FlexDirection::Column),
    ];
    ui.block("#page", &page, |ui| {
        let panel = [
            Declaration::Width(px(100.0)),
            Declaration::Height(px(40.0)),
            Declaration::FlexShrink(0.0),
            Declaration::FlexDirection(FlexDirection::Column),
            Declaration::Overflow(Overflow::Hidden),
        ];
        ui.block("#panel", &panel, |ui| {
            ui.set_scroll_offset(Vec2::new(0.0, SCROLL));
            let spacer = [Declaration::Height(px(30.0)), Declaration::FlexShrink(0.0)];
            ui.block("#spacer", &spacer, |_| {});
            let size = [
                Declaration::Width(px(60.0)),
                Declaration::Height(px(20.0)),
                Declaration::FlexShrink(0.0),
            ];
            let built = ui.block_with("#anchor", &size, Behavior::BUTTON, |_| {});
            if options.opens || built.clicked {
                ui.open_popup(built.key);
            }
            anchor = Some(built);
            if options.unbuilt {
                return;
            }
            let style = [Declaration::Width(px(120.0)), Declaration::Background(MENU)];
            menu = ui.popup(built.key, "#menu", &style, |ui| {
                for name in ["first", "second"] {
                    let item = [Declaration::Height(px(20.0)), Declaration::FlexShrink(0.0)];
                    items.push(ui.block_keyed_with(name, "", &item, Behavior::BUTTON, |_| {}));
                }
                if options.sub {
                    ui.open_popup(items[0].key);
                }
                let sub = [Declaration::Width(px(50.0)), Declaration::Height(px(20.0))];
                ui.popup(items[0].key, "#sub", &sub, |_| {});
            });
        });
        let size = [
            Declaration::Width(px(VIEW.x)),
            Declaration::Height(px(80.0)),
            Declaration::FlexShrink(0.0),
            Declaration::Background(LATER),
        ];
        later = Some(ui.block_with("#later", &size, Behavior::BUTTON, |_| {}));
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(VIEW),
        &FontAtlas::built_in(),
    );
    Page {
        anchor: anchor.expect("the panel builds the anchor"),
        later: later.expect("the page builds the later block"),
        menu,
        items,
    }
}

fn idle() -> PointerInput {
    PointerInput::hovering(Vec2::splat(-1.0))
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

const OPEN: Options = Options {
    opens: true,
    unbuilt: false,
    sub: false,
};

const KEEP: Options = Options {
    opens: false,
    unbuilt: false,
    sub: false,
};

/// The index of the first filled rect of `color` in `list`.
fn rect_of(list: &DrawList, color: [f32; 4]) -> usize {
    list.commands()
        .iter()
        .position(|command| matches!(command, DrawCommand::Rect { color: at, .. } if *at == color))
        .unwrap_or_else(|| panic!("no rect of {color:?} was drawn"))
}

/// A point inside the pop-up's first item that lies over `later` too.
fn over_both(ui: &Ui, page: &Page) -> Vec2 {
    let (min, max) = ui.rect(page.items[0].key).expect("laid out");
    let point = (min + max) * 0.5;
    let (later_min, later_max) = ui.rect(page.later.key).expect("laid out");
    assert!(
        point.cmpge(later_min).all() && point.cmplt(later_max).all(),
        "the fixture's pop-up does not cover the later block"
    );
    point
}

/// **A pop-up hangs below its anchor and is drawn after everything built
/// after it**, its outline included: the block built after the panel draws
/// first, though the pop-up was built inside the panel.
#[test]
fn a_popup_hangs_below_its_anchor_and_is_drawn_over_what_was_built_after_it() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    let menu = built.menu.expect("an open pop-up is built");
    let (anchor_min, anchor_max) = ui.rect(built.anchor.key).expect("laid out");
    assert_eq!(anchor_max.y, 40.0, "the panel's scroll moved the anchor");
    let (menu_min, _) = ui.rect(menu.key).expect("laid out");
    assert_eq!(menu_min, Vec2::new(anchor_min.x, anchor_max.y));

    let mut list = DrawList::new();
    ui.emit(&mut list);
    assert!(
        rect_of(&list, MENU) > rect_of(&list, LATER),
        "the pop-up was drawn under the block built after it"
    );
}

/// **A pop-up is clipped by the viewport and by nothing its anchor is in**:
/// the panel it hangs out of clips everything below its 40 pixels, which is
/// all of the pop-up, and the pop-up's draws are under the viewport's clip.
#[test]
fn a_popup_is_clipped_by_the_viewport_not_by_the_scrolled_panel_it_hangs_from() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    let menu = built.menu.expect("an open pop-up is built");
    let (_, anchor_max) = ui.rect(built.anchor.key).expect("laid out");
    let (menu_min, _) = ui.rect(menu.key).expect("laid out");
    assert!(
        menu_min.y >= anchor_max.y,
        "the fixture's pop-up is not wholly outside the panel"
    );

    let mut list = DrawList::new();
    ui.emit(&mut list);
    let clip = list.clips()[rect_of(&list, MENU)];
    assert_eq!((clip.min, clip.max), (Vec2::ZERO, VIEW));
    assert_eq!(list.clip(), ClipRect::NONE, "a clip was left pushed");
}

/// **A pop-up is hit before what it covers**, whatever the build order says:
/// over the pop-up's item and the later block both, the item is hovered and
/// the block is not.
#[test]
fn a_popup_is_hit_before_the_block_it_covers() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    let point = over_both(&ui, &built);
    let next = page(
        &mut ui,
        PointerInput::hovering(point),
        NavInput::default(),
        KEEP,
    );
    assert!(next.items[0].hovered, "the pop-up's item is not hovered");
    assert!(!next.later.hovered, "the covered block is hovered");
}

/// **A pop-up flips above its anchor when there is no room below and more
/// above, and is shifted the least that keeps it in the viewport** — its
/// top-left corner winning when it is larger — and an unbounded viewport
/// moves nothing.
#[test]
fn a_popup_flips_or_shifts_to_stay_inside_the_viewport() {
    let viewport = ClipRect {
        min: Vec2::ZERO,
        max: Vec2::new(200.0, 150.0),
    };
    let size = Vec2::new(80.0, 60.0);
    let anchor = |x: f32, y: f32| (Vec2::new(x, y), Vec2::new(x + 40.0, y + 20.0));

    assert_eq!(
        hang(anchor(10.0, 10.0), size, viewport, Placement::Below),
        Vec2::new(10.0, 30.0),
        "room below: below, lined up with the anchor"
    );
    assert_eq!(
        hang(anchor(10.0, 120.0), size, viewport, Placement::Below),
        Vec2::new(10.0, 60.0),
        "no room below and more above: flipped above"
    );
    assert_eq!(
        hang(
            anchor(10.0, 40.0),
            Vec2::new(80.0, 100.0),
            viewport,
            Placement::Below
        ),
        Vec2::new(10.0, 50.0),
        "no room below and less above: shifted up from below"
    );
    assert_eq!(
        hang(anchor(170.0, 10.0), size, viewport, Placement::Below),
        Vec2::new(120.0, 30.0),
        "past the right edge: shifted left"
    );
    assert_eq!(
        hang(
            anchor(10.0, 10.0),
            Vec2::new(300.0, 60.0),
            viewport,
            Placement::Below
        ),
        Vec2::new(0.0, 30.0),
        "wider than the viewport: its left edge kept"
    );
    assert_eq!(
        hang(anchor(170.0, 140.0), size, ClipRect::NONE, Placement::Below),
        Vec2::new(170.0, 160.0),
        "an unbounded viewport moves nothing"
    );
}

/// **A pop-up at a point starts there, and flips to end there on each axis
/// it would run past with more room before the point**; one that fits
/// neither way is shifted inside instead.
#[test]
fn a_popup_at_a_point_flips_on_each_axis_at_the_far_edges() {
    let viewport = ClipRect {
        min: Vec2::ZERO,
        max: Vec2::new(200.0, 150.0),
    };
    let size = Vec2::new(80.0, 60.0);
    let at = |x: f32, y: f32| Placement::At(Vec2::new(x, y));
    // The anchor plays no part.
    let anchor = (Vec2::splat(-50.0), Vec2::splat(-40.0));

    assert_eq!(
        hang(anchor, size, viewport, at(10.0, 10.0)),
        Vec2::new(10.0, 10.0),
        "room both ways: its corner at the point"
    );
    assert_eq!(
        hang(anchor, size, viewport, at(190.0, 140.0)),
        Vec2::new(110.0, 80.0),
        "the bottom right: flipped on both axes to end at the point"
    );
    assert_eq!(
        hang(anchor, size, viewport, at(60.0, 140.0)),
        Vec2::new(60.0, 80.0),
        "the bottom: flipped up only"
    );
    assert_eq!(
        hang(anchor, Vec2::new(180.0, 60.0), viewport, at(70.0, 10.0)),
        Vec2::new(20.0, 10.0),
        "less room before than after: not flipped, shifted left"
    );
}

/// **A pop-up beside its anchor goes right of it, lined up with its top, and
/// flips to its left at the right edge** when there is more room there,
/// shifted up from the bottom like any other.
#[test]
fn a_popup_beside_its_anchor_flips_left_at_the_right_edge() {
    let viewport = ClipRect {
        min: Vec2::ZERO,
        max: Vec2::new(200.0, 150.0),
    };
    let size = Vec2::new(80.0, 60.0);
    let anchor = |x: f32, y: f32| (Vec2::new(x, y), Vec2::new(x + 40.0, y + 20.0));

    assert_eq!(
        hang(anchor(10.0, 10.0), size, viewport, Placement::Beside),
        Vec2::new(50.0, 10.0),
        "room on the right: right of it, its top on the anchor's"
    );
    assert_eq!(
        hang(anchor(150.0, 10.0), size, viewport, Placement::Beside),
        Vec2::new(70.0, 10.0),
        "no room on the right: flipped to end at the anchor's left edge"
    );
    assert_eq!(
        hang(anchor(10.0, 120.0), size, viewport, Placement::Beside),
        Vec2::new(50.0, 90.0),
        "past the bottom: shifted up, not flipped"
    );
}

/// **Layout places a pop-up by that rule**: one hanging from a block at the
/// viewport's bottom right is flipped above it and shifted left, and stays
/// inside the viewport.
#[test]
fn layout_flips_and_shifts_a_popup_at_the_viewports_corner() {
    let mut ui = Ui::new();
    let mut keys = None;
    ui.begin_frame(idle());
    ui.block("#page", &[Declaration::Width(px(VIEW.x))], |ui| {
        let corner = [
            Declaration::Position(Position::Absolute),
            Declaration::Inset(crate::style::Sides::Left, px(VIEW.x - 30.0)),
            Declaration::Inset(crate::style::Sides::Top, px(VIEW.y - 20.0)),
            Declaration::Width(px(30.0)),
            Declaration::Height(px(20.0)),
        ];
        let anchor = ui.block("#corner", &corner, |_| {}).key;
        ui.open_popup(anchor);
        let size = [Declaration::Width(px(80.0)), Declaration::Height(px(60.0))];
        let menu = ui.popup(anchor, "#menu", &size, |_| {}).expect("open");
        keys = Some((anchor, menu.key));
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(VIEW),
        &FontAtlas::built_in(),
    );
    let (anchor, menu) = keys.expect("built");
    let (anchor_min, _) = ui.rect(anchor).expect("laid out");
    assert_eq!(anchor_min, VIEW - Vec2::new(30.0, 20.0));
    let (menu_min, menu_max) = ui.rect(menu).expect("laid out");
    assert_eq!(menu_max.y, anchor_min.y, "not flipped above the anchor");
    assert_eq!(menu_max.x, VIEW.x, "not shifted inside the right edge");
    assert!(menu_min.cmpge(Vec2::ZERO).all());
}

/// **A press outside a pop-up closes it and reaches nothing beneath**: the
/// block under the press is neither pressed nor clicked by it, the pop-up is
/// gone from the next frame, and the next press on the block is its own.
#[test]
fn a_press_outside_closes_the_popup_and_is_not_delivered_beneath() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    let (menu_min, menu_max) = ui.rect(built.menu.expect("open").key).expect("laid out");
    let (later_min, later_max) = ui.rect(built.later.key).expect("laid out");
    let point = Vec2::new(menu_max.x + 20.0, (later_min.y + later_max.y) * 0.5);
    assert!(point.x < later_max.x && !(point.cmpge(menu_min).all() && point.cmplt(menu_max).all()));

    let pressed = page(&mut ui, press(point), NavInput::default(), KEEP);
    assert!(
        !pressed.later.pressed,
        "the dismissing press pressed the block"
    );
    assert!(
        !ui.is_popup_open(built.anchor.key),
        "the press left it open"
    );
    assert!(pressed.menu.is_none(), "a closed pop-up was built");
    let released = page(&mut ui, release(point), NavInput::default(), KEEP);
    assert!(
        !released.later.clicked,
        "the dismissing press clicked the block"
    );

    page(&mut ui, press(point), NavInput::default(), KEEP);
    let clicked = page(&mut ui, release(point), NavInput::default(), KEEP);
    assert!(clicked.later.clicked, "the next press was swallowed too");
}

/// **A press on the anchor of an open pop-up only closes it**, rather than
/// closing it and clicking the anchor open again.
#[test]
fn a_press_on_the_anchor_closes_its_popup_rather_than_reopening_it() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    let (min, max) = ui.rect(built.anchor.key).expect("laid out");
    let on = (min + max) * 0.5;
    page(&mut ui, press(on), NavInput::default(), KEEP);
    let released = page(&mut ui, release(on), NavInput::default(), KEEP);
    assert!(!released.anchor.clicked);
    assert!(
        !ui.is_popup_open(built.anchor.key),
        "the anchor reopened it"
    );
}

/// **A press in a lower pop-up closes only the ones above it, and is that
/// pop-up's**: the second item, beside the open sub-pop-up, is pressed, and
/// the first pop-up stays open.
#[test]
fn a_press_in_a_lower_popup_closes_only_the_popups_above_it() {
    let mut ui = Ui::new();
    let first = page(
        &mut ui,
        idle(),
        NavInput::default(),
        Options { sub: true, ..OPEN },
    );
    let sub = Ui::popup_key(first.items[0].key);
    page(&mut ui, idle(), NavInput::default(), KEEP);
    let (sub_min, sub_max) = ui.rect(sub).expect("the sub-pop-up was laid out");
    let (min, max) = ui.rect(first.items[1].key).expect("laid out");
    let point = Vec2::new(sub_max.x + 20.0, (min.y + max.y) * 0.5);
    assert!(point.x < max.x && !(point.cmpge(sub_min).all() && point.cmplt(sub_max).all()));

    let pressed = page(&mut ui, press(point), NavInput::default(), KEEP);
    assert!(pressed.items[1].pressed, "the lower pop-up lost the press");
    assert!(
        ui.is_popup_open(first.anchor.key),
        "the lower pop-up closed"
    );
    assert!(
        !ui.is_popup_open(first.items[0].key),
        "the upper one stayed open"
    );
}

/// **Focus moves into an open pop-up, back closes it, and focus returns to
/// its anchor**: accept on the anchor opens the pop-up, the next frame's
/// focus is inside it, a move stays inside it, and back closes it with focus
/// on the anchor in the same frame — spent, so the caller hears no back.
#[test]
fn back_closes_the_popup_and_focus_returns_to_the_anchor() {
    let mut ui = Ui::new();
    page(&mut ui, idle(), NavInput::default(), KEEP);
    let built = page(&mut ui, idle(), NavInput::NEXT, KEEP);
    assert_eq!(ui.focused(), Some(built.anchor.key));
    let opened = page(&mut ui, idle(), NavInput::ACCEPT, KEEP);
    assert!(
        opened.menu.is_some(),
        "accept on the anchor did not open it"
    );

    let inside = page(&mut ui, idle(), NavInput::NAVIGATION, KEEP);
    assert_eq!(
        ui.focused(),
        Some(inside.items[0].key),
        "focus did not move into the pop-up"
    );
    page(&mut ui, idle(), NavInput::toward(Direction::Down), KEEP);
    page(&mut ui, idle(), NavInput::toward(Direction::Down), KEEP);
    assert_eq!(
        ui.focused(),
        Some(inside.items[1].key),
        "a move left the pop-up"
    );

    let closed = page(&mut ui, idle(), NavInput::BACK, KEEP);
    assert!(closed.menu.is_none(), "back left the pop-up open");
    assert_eq!(ui.focused(), Some(built.anchor.key));
    assert!(!ui.back_requested(), "the back that closed it was reported");
}

/// **A press outside hands focus back to the anchor too.**
#[test]
fn an_outside_press_returns_focus_to_the_anchor() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    page(&mut ui, idle(), NavInput::default(), KEEP);
    assert!(
        ui.focused()
            .is_some_and(|key| built.items.iter().any(|item| item.key == key)),
        "focus did not move into the pop-up"
    );
    let point = Vec2::new(VIEW.x - 1.0, VIEW.y - 1.0);
    page(&mut ui, press(point), NavInput::default(), KEEP);
    page(&mut ui, release(point), NavInput::default(), KEEP);
    assert_eq!(ui.focused(), Some(built.anchor.key));
}

/// **An open pop-up a frame does not build is closed** at that frame's
/// layout, and nothing of it is drawn or traps focus after.
#[test]
fn an_open_popup_the_frame_does_not_build_is_closed() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default(), OPEN);
    page(
        &mut ui,
        idle(),
        NavInput::default(),
        Options {
            unbuilt: true,
            ..KEEP
        },
    );
    assert!(!ui.is_popup_open(built.anchor.key));
    let next = page(&mut ui, idle(), NavInput::default(), KEEP);
    assert!(next.menu.is_none());
}
