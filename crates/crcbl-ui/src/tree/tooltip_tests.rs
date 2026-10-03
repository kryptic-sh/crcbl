//! Tooltips: when one shows and hides, where it is drawn, and that the pointer
//! passes through it — each held to the rule in `tooltip.rs` it exists for.

use std::time::Duration;

use glam::Vec2;

use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::style::{Declaration, Sides};

/// The viewport every page is laid out in.
const VIEW: Vec2 = Vec2::new(400.0, 200.0);
/// The tooltip's background, set by the page's own sheet over
/// `default.css`'s, to find it in a draw list by.
const TIP: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
/// The background of the block under the anchor, which the tooltip covers.
const UNDER: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
/// The background of the pop-up the anchor opens, which the tooltip covers.
const MENU: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
/// What the anchor's tooltip says.
const HINT: &str = "Does the thing (F5)";
/// A frame much shorter than the delay.
const FRAME: Duration = Duration::from_millis(16);

const fn px(value: f32) -> LengthAuto {
    LengthAuto::Px(value)
}

/// What a frame is driven by beside the pointer and navigation.
#[derive(Clone, Copy, Default)]
struct Options {
    /// How long the frame was.
    dt: Duration,
    /// Leave the anchor, and its tooltip call, unbuilt.
    gone: bool,
    /// Open a pop-up hanging from the anchor.
    menu: bool,
    /// The wheel's movement, handed over before the build.
    wheel: Vec2,
}

/// What a page built.
struct Page {
    /// A button at the top-left, whose tooltip is [`HINT`].
    anchor: Option<Response>,
    /// A button filling the width under the anchor, with no tooltip.
    under: Response,
    /// A button at the bottom-right corner, with a tooltip too wide to fit
    /// beside it.
    corner: Response,
    /// The anchor's tooltip, if it was built.
    tip: Option<Response>,
    /// The corner's tooltip, if it was built.
    corner_tip: Option<Response>,
}

/// One frame: a `VIEW`-sized column holding the anchor, the block under it,
/// and the corner button, laid out.
fn page(ui: &mut Ui, pointer: PointerInput, nav: NavInput, options: Options) -> Page {
    ui.begin_frame_with(pointer, nav);
    ui.set_text_input(TextInput {
        dt: options.dt,
        ..TextInput::default()
    });
    if options.wheel != Vec2::ZERO {
        ui.scroll_wheel(options.wheel);
    }
    let mut anchor = None;
    let mut tip = None;
    let mut under = None;
    let mut corner = None;
    let mut corner_tip = None;
    let page = [
        Declaration::Width(px(VIEW.x)),
        Declaration::Height(px(VIEW.y)),
        Declaration::FlexDirection(FlexDirection::Column),
    ];
    ui.block("#page", &page, |ui| {
        let size = [
            Declaration::Width(px(60.0)),
            Declaration::Height(px(20.0)),
            Declaration::FlexShrink(0.0),
        ];
        if !options.gone {
            let built = ui.block_with("#anchor", &size, Behavior::BUTTON, |_| {});
            tip = ui.tooltip(&built, HINT);
            if options.menu {
                ui.open_popup(built.key);
            }
            let menu = [
                Declaration::Width(px(120.0)),
                Declaration::Height(px(40.0)),
                Declaration::Background(MENU),
            ];
            ui.popup(built.key, "#menu", &menu, |_| {});
            anchor = Some(built);
        }
        let below = [
            Declaration::Width(px(VIEW.x)),
            Declaration::Height(px(40.0)),
            Declaration::FlexShrink(0.0),
            Declaration::Background(UNDER),
        ];
        under = Some(ui.block_with("#under", &below, Behavior::BUTTON, |_| {}));
        let low = [
            Declaration::Position(Position::Absolute),
            Declaration::Inset(Sides::Left, px(VIEW.x - 60.0)),
            Declaration::Inset(Sides::Top, px(VIEW.y - 20.0)),
            Declaration::Width(px(60.0)),
            Declaration::Height(px(20.0)),
        ];
        let built = ui.block_with("#corner", &low, Behavior::BUTTON, |_| {});
        corner_tip = ui.tooltip(&built, "A tooltip wider than the button");
        corner = Some(built);
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(VIEW),
        &FontAtlas::built_in(),
    );
    Page {
        anchor,
        under: under.expect("the page builds the block under the anchor"),
        corner: corner.expect("the page builds the corner button"),
        tip,
        corner_tip,
    }
}

/// A tree whose sheet colours the tooltip [`TIP`], laid out once with the
/// pointer nowhere so that the next frame has rectangles to hover.
fn laid_out() -> (Ui, Page) {
    let mut ui = Ui::new();
    ui.add_stylesheet("tip.css", "tooltip { background: #ff0000 }");
    let page = page(&mut ui, idle(), NavInput::default(), Options::default());
    (ui, page)
}

fn idle() -> PointerInput {
    PointerInput::hovering(Vec2::splat(-1.0))
}

fn centre(ui: &Ui, key: NodeKey) -> Vec2 {
    let (min, max) = ui.rect(key).expect("laid out");
    (min + max) * 0.5
}

/// A frame `dt` long with the pointer resting at `at`.
fn rest(ui: &mut Ui, at: Vec2, dt: Duration) -> Page {
    let options = Options {
        dt,
        ..Options::default()
    };
    page(ui, PointerInput::hovering(at), NavInput::default(), options)
}

/// Rests the pointer at `at` until the delay has passed: the first frame
/// starts the count and the second ends it. Asserts that the one tooltip
/// shown then is the widget's under `at`.
fn rest_until_shown(ui: &mut Ui, at: Vec2) -> Page {
    rest(ui, at, FRAME);
    let page = rest(ui, at, TOOLTIP_DELAY);
    let over = |key| {
        ui.rect(key)
            .is_some_and(|(min, max)| at.cmpge(min).all() && at.cmplt(max).all())
    };
    let over_anchor = page.anchor.is_some_and(|anchor| over(anchor.key));
    assert_eq!(
        (page.tip.is_some(), page.corner_tip.is_some()),
        (over_anchor, over(page.corner.key)),
        "the tooltip shown after the delay is not the hovered widget's"
    );
    page
}

/// The index of the first filled rect of `color` in `list`.
fn rect_of(list: &DrawList, color: [f32; 4]) -> usize {
    list.commands()
        .iter()
        .position(|command| matches!(command, DrawCommand::Rect { color: at, .. } if *at == color))
        .unwrap_or_else(|| panic!("no rect of {color:?} was drawn"))
}

/// **A tooltip shows once the pointer has rested for the delay, and not a
/// moment before**: the count starts at the first frame that found the anchor
/// hovered, and a millisecond short of the delay builds nothing.
#[test]
fn a_tooltip_shows_after_the_hover_delay_and_not_before() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    assert!(
        rest(&mut ui, at, TOOLTIP_DELAY).tip.is_none(),
        "shown on the first frame"
    );
    let short = TOOLTIP_DELAY - Duration::from_millis(1);
    assert!(
        rest(&mut ui, at, short).tip.is_none(),
        "shown before the delay"
    );
    let shown = rest(&mut ui, at, Duration::from_millis(1));
    let tip = shown.tip.expect("shown at the delay");
    let span = ui.child_keys(tip.key)[0];
    assert_eq!(ui.text(span), Some(HINT));
    assert_eq!(tip.key, Ui::tooltip_key(shown.anchor.expect("built").key));
}

/// **[`Ui::set_tooltip_delay`] replaces the delay.**
#[test]
fn the_delay_can_be_replaced() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    ui.set_tooltip_delay(Duration::from_millis(40));
    rest(&mut ui, at, FRAME);
    assert!(
        rest(&mut ui, at, FRAME).tip.is_none(),
        "shown before the new delay"
    );
    assert!(
        rest(&mut ui, at, FRAME * 2).tip.is_some(),
        "not shown at the new delay"
    );
}

/// **A tooltip hangs below its anchor and is drawn over everything**: over
/// the block built after the anchor, and over a pop-up open from it.
#[test]
fn a_tooltip_hangs_below_its_anchor_drawn_over_the_tree_and_every_popup() {
    let (mut ui, built) = laid_out();
    let anchor = built.anchor.expect("built").key;
    let at = centre(&ui, anchor);
    let menu = Options {
        menu: true,
        ..Options::default()
    };
    page(
        &mut ui,
        PointerInput::hovering(at),
        NavInput::default(),
        menu,
    );
    let hover = |dt| Options { dt, ..menu };
    page(
        &mut ui,
        PointerInput::hovering(at),
        NavInput::default(),
        hover(FRAME),
    );
    let shown = page(
        &mut ui,
        PointerInput::hovering(at),
        NavInput::default(),
        hover(TOOLTIP_DELAY),
    );
    let tip = shown.tip.expect("shown after the delay");
    let (anchor_min, anchor_max) = ui.rect(anchor).expect("laid out");
    let (tip_min, tip_max) = ui.rect(tip.key).expect("laid out");
    assert_eq!(tip_min, Vec2::new(anchor_min.x, anchor_max.y));
    assert!(tip_max.x < VIEW.x, "the tooltip is not as wide as its text");

    let mut list = DrawList::new();
    ui.emit(&mut list);
    let tip = rect_of(&list, TIP);
    assert!(
        tip > rect_of(&list, UNDER),
        "drawn under the block built after it"
    );
    assert!(tip > rect_of(&list, MENU), "drawn under the open pop-up");
}

/// **A tooltip that would run past the bottom of the viewport flips above its
/// anchor**, and is shifted left to stay inside it.
#[test]
fn a_tooltip_at_the_viewports_corner_flips_above_and_shifts_inside() {
    let (mut ui, built) = laid_out();
    let corner = built.corner.key;
    let at = centre(&ui, corner);
    let shown = rest_until_shown(&mut ui, at);
    let tip = shown.corner_tip.expect("shown after the delay");
    let (corner_min, _) = ui.rect(corner).expect("laid out");
    let (tip_min, tip_max) = ui.rect(tip.key).expect("laid out");
    assert_eq!(tip_max.y, corner_min.y, "not flipped above the anchor");
    assert_eq!(tip_max.x, VIEW.x, "not shifted inside the viewport");
    assert!(
        tip_min.x < corner_min.x,
        "the fixture's tooltip is no wider than its anchor"
    );
}

/// **The pointer passes through a tooltip**: a click on the block under it
/// reaches the block, and the press hides the tooltip.
#[test]
fn a_click_on_what_lies_under_a_tooltip_reaches_it() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    let shown = rest_until_shown(&mut ui, at);
    let (min, max) = ui.rect(shown.tip.expect("shown").key).expect("laid out");
    let (under_min, under_max) = ui.rect(shown.under.key).expect("laid out");
    let at = Vec2::new(min.x + 2.0, (min.y + max.y) * 0.5);
    assert!(
        at.cmpge(under_min).all() && at.cmplt(under_max).all() && at.cmplt(max).all(),
        "the fixture's tooltip does not cover the block under it"
    );
    let press = PointerInput {
        pos: at,
        down: true,
        released: false,
        secondary_pressed: false,
    };
    let pressed = page(&mut ui, press, NavInput::default(), Options::default());
    assert!(
        pressed.under.pressed,
        "the press did not reach the block under"
    );
    let release = PointerInput {
        pos: at,
        down: false,
        released: true,
        secondary_pressed: false,
    };
    let released = page(&mut ui, release, NavInput::default(), Options::default());
    assert!(
        released.under.clicked,
        "the click did not reach the block under"
    );
}

/// **A press hides the tooltip until the pointer leaves its anchor**: resting
/// on after the click does not bring it back; leaving and resting again does.
#[test]
fn a_press_hides_the_tooltip_until_the_pointer_leaves_and_comes_back() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    rest_until_shown(&mut ui, at);
    let press = PointerInput {
        pos: at,
        down: true,
        released: false,
        secondary_pressed: false,
    };
    let pressed = page(&mut ui, press, NavInput::default(), Options::default());
    assert!(pressed.tip.is_none(), "the press left the tooltip up");
    let release = PointerInput {
        pos: at,
        down: false,
        released: true,
        secondary_pressed: false,
    };
    page(&mut ui, release, NavInput::default(), Options::default());
    assert!(
        rest(&mut ui, at, TOOLTIP_DELAY * 2).tip.is_none(),
        "back after the click"
    );
    rest(&mut ui, Vec2::splat(-1.0), FRAME);
    rest_until_shown(&mut ui, at);
}

/// **A secondary press hides the tooltip too, and so does `ui_menu`**: each
/// opens a context menu the tooltip would sit over.
#[test]
fn a_secondary_press_or_ui_menu_hides_the_tooltip() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    rest_until_shown(&mut ui, at);
    let secondary = PointerInput {
        secondary_pressed: true,
        ..PointerInput::hovering(at)
    };
    let pressed = page(&mut ui, secondary, NavInput::default(), Options::default());
    assert!(pressed.tip.is_none(), "the secondary press left it up");

    let (mut ui, built) = laid_out();
    let anchor = built.anchor.expect("built").key;
    page(&mut ui, idle(), NavInput::NEXT, Options::default());
    assert_eq!(ui.focused(), Some(anchor));
    let held = Options {
        dt: TOOLTIP_DELAY,
        ..Options::default()
    };
    page(&mut ui, idle(), NavInput::NAVIGATION, held);
    let shown = page(&mut ui, idle(), NavInput::NAVIGATION, held);
    assert!(
        shown.tip.is_some(),
        "the focused anchor's tooltip never showed"
    );
    let menu = page(&mut ui, idle(), NavInput::MENU, Options::default());
    assert!(menu.tip.is_none(), "ui_menu left it up");
}

/// **The pointer leaving the anchor hides its tooltip.**
#[test]
fn leaving_the_anchor_hides_the_tooltip() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    rest_until_shown(&mut ui, at);
    let away = centre(&ui, built.under.key);
    assert!(
        rest(&mut ui, away, FRAME).tip.is_none(),
        "still up once the pointer left"
    );
}

/// **A wheel hides the tooltip**, though nothing under the pointer scrolls.
#[test]
fn a_wheel_hides_the_tooltip() {
    let (mut ui, built) = laid_out();
    let at = centre(&ui, built.anchor.expect("built").key);
    rest_until_shown(&mut ui, at);
    let wheel = Options {
        dt: FRAME,
        wheel: Vec2::new(0.0, 10.0),
        ..Options::default()
    };
    let wheeled = page(
        &mut ui,
        PointerInput::hovering(at),
        NavInput::default(),
        wheel,
    );
    assert!(wheeled.tip.is_none(), "the wheel left the tooltip up");
}

/// **A tooltip goes with its widget**, and a widget built again starts the
/// delay again.
#[test]
fn a_tooltip_goes_with_its_widget_and_starts_over_when_it_returns() {
    let (mut ui, built) = laid_out();
    let anchor = built.anchor.expect("built").key;
    let at = centre(&ui, anchor);
    rest_until_shown(&mut ui, at);
    let gone = Options {
        dt: FRAME,
        gone: true,
        ..Options::default()
    };
    let without = page(
        &mut ui,
        PointerInput::hovering(at),
        NavInput::default(),
        gone,
    );
    assert!(without.tip.is_none());
    assert_eq!(
        ui.rect(Ui::tooltip_key(anchor)),
        None,
        "the tooltip outlived its widget"
    );
    rest(&mut ui, at, FRAME);
    assert!(
        rest(&mut ui, at, FRAME).tip.is_none(),
        "the delay did not start over"
    );
}

/// **Focus by navigation shows the focused widget's tooltip after the same
/// delay, and moving focus off it hides it.**
#[test]
fn navigation_focus_shows_the_tooltip_after_the_delay_and_blur_hides_it() {
    let (mut ui, _) = laid_out();
    let nav = |dt| Options {
        dt,
        ..Options::default()
    };
    let landed = page(&mut ui, idle(), NavInput::NEXT, nav(FRAME));
    assert!(
        landed.anchor.expect("built").focused,
        "focus did not land on the anchor"
    );
    assert!(landed.tip.is_none(), "shown on the frame focus landed");
    let short = TOOLTIP_DELAY - FRAME;
    assert!(
        page(&mut ui, idle(), NavInput::NAVIGATION, nav(short))
            .tip
            .is_none()
    );
    let shown = page(&mut ui, idle(), NavInput::NAVIGATION, nav(FRAME));
    assert!(
        shown.tip.is_some(),
        "not shown once focus had been held for the delay"
    );
    let moved = page(&mut ui, idle(), NavInput::NEXT, nav(FRAME));
    assert!(moved.under.focused, "focus did not move off the anchor");
    assert!(moved.tip.is_none(), "still up once focus moved off");
}
