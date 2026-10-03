//! The context menu: opened at the pointer by a secondary press and below the
//! focused widget by `ui_menu`, picked by a click or by moves and accept,
//! reported once, with submenus beside their items — and closed a level at a
//! time by back, or all at once by a pick or a press outside.

use super::*;
use crate::style::Sides;
use crate::tree::{Behavior, ContextItem, ContextMenuResponse, Position, Ui};

/// What the fixture's items report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Cut,
    Copy,
    Locked,
    Small,
    Large,
    Panel,
}

/// The size submenu.
const SIZES: [ContextItem<'static, Pick>; 2] = [
    ContextItem::action("Small", Pick::Small),
    ContextItem::action("Large", Pick::Large),
];

/// The panel's own menu, under every target.
const PANEL: [ContextItem<'static, Pick>; 1] = [ContextItem::action("Panel", Pick::Panel)];

/// A target's menu: two actions, a disabled one, a separator and a submenu.
fn items() -> [ContextItem<'static, Pick>; 5] {
    [
        ContextItem::action("Cut", Pick::Cut),
        ContextItem::action("Copy", Pick::Copy),
        ContextItem::action("Locked", Pick::Locked).enabled(false),
        ContextItem::Separator,
        ContextItem::submenu("Size", &SIZES),
    ]
}

/// The targets' size.
const TARGET: Vec2 = Vec2::new(100.0, 30.0);

/// What a page built.
struct Page {
    /// A focusable block near the top left with [`items`] as its menu.
    target: NodeKey,
    /// The same, at the page's bottom right corner.
    corner: NodeKey,
    /// A button with no menu of its own, inside the panel.
    other: NodeKey,
    /// The block around them all, with [`PANEL`] as its menu.
    panel: NodeKey,
    /// What each call reported, in that order: the target's, the corner's
    /// and the panel's.
    menus: [ContextMenuResponse<Pick>; 3],
}

/// One frame of the fixture.
fn page(ui: &mut Ui, pointer: PointerInput, nav: NavInput) -> Page {
    frame(ui, pointer, nav, |ui| {
        let size = [
            Declaration::Width(LengthAuto::Px(TARGET.x)),
            Declaration::Height(LengthAuto::Px(TARGET.y)),
            Declaration::FlexShrink(0.0),
        ];
        let fill = [
            Declaration::FlexGrow(1.0),
            Declaration::FlexDirection(FlexDirection::Column),
        ];
        let mut keys = None;
        let panel = ui.block("#panel", &fill, |ui| {
            let target = ui
                .block_with("#target", &size, Behavior::BUTTON, |_| {})
                .key;
            let target_menu = ui.context_menu(target, &items());
            let other = ui.block_with("#other", &size, Behavior::BUTTON, |_| {}).key;
            let corner_at = [
                Declaration::Position(Position::Absolute),
                Declaration::Inset(Sides::Left, LengthAuto::Px(PAGE - TARGET.x)),
                Declaration::Inset(Sides::Top, LengthAuto::Px(PAGE - TARGET.y)),
            ];
            let corner = ui
                .block_with(
                    "#corner",
                    &[&size[..], &corner_at[..]].concat(),
                    Behavior::BUTTON,
                    |_| {},
                )
                .key;
            let corner_menu = ui.context_menu(corner, &items());
            keys = Some((target, other, corner, target_menu, corner_menu));
        });
        let panel_menu = ui.context_menu(panel.key, &PANEL);
        let (target, other, corner, target_menu, corner_menu) = keys.expect("built");
        Page {
            target,
            corner,
            other,
            panel: panel.key,
            menus: [target_menu, corner_menu, panel_menu],
        }
    })
}

/// A secondary press at `pos`.
fn secondary(pos: Vec2) -> PointerInput {
    PointerInput {
        secondary_pressed: true,
        ..PointerInput::hovering(pos)
    }
}

/// The key of the item reading `label` in the menu hanging from `anchor`: the
/// parent of its label.
fn item(ui: &Ui, anchor: NodeKey, label: &str) -> NodeKey {
    children_of(ui, Ui::popup_key(anchor))
        .into_iter()
        .find(|&item| {
            children_of(ui, item)
                .first()
                .is_some_and(|&span| ui.text(span) == Some(label))
        })
        .unwrap_or_else(|| panic!("the menu holds no {label:?}"))
}

/// A point inside `key` a little way from its top-left corner.
fn inside(ui: &Ui, key: NodeKey) -> Vec2 {
    rect(ui, key).0 + Vec2::splat(5.0)
}

/// The menu opened on the target by a secondary press, at the point it was
/// opened at.
fn opened(ui: &mut Ui) -> (Page, Vec2) {
    let built = page(ui, idle(), NavInput::default());
    let at = inside(ui, built.target);
    (page(ui, secondary(at), NavInput::default()), at)
}

/// **A secondary press opens the target's menu with its corner at the
/// pointer**, reported as opened that frame and that frame only — the
/// target's and not the panel's around it, the innermost node that asked
/// winning — and a press on the panel itself closes it and opens the panel's
/// there instead.
#[test]
fn a_secondary_press_opens_the_menu_at_the_pointer() {
    let mut ui = Ui::new();
    let (built, at) = opened(&mut ui);
    assert!(built.menus[0].opened, "the press did not report opening");
    assert!(!built.menus[2].opened, "the panel's menu opened too");
    assert!(ui.is_popup_open(built.target));
    assert_eq!(rect(&ui, Ui::popup_key(built.target)).0, at);

    let next = page(&mut ui, idle(), NavInput::default());
    assert!(!next.menus[0].opened, "opening was reported twice");
    assert!(ui.is_popup_open(next.target), "the menu did not stay open");

    // On the target again, clear of the menu hanging from `at`.
    let again = rect(&ui, next.target).0 + Vec2::splat(2.0);
    let reopened = page(&mut ui, secondary(again), NavInput::default());
    assert!(
        reopened.menus[0].opened,
        "the second press did not reopen it"
    );
    assert_eq!(rect(&ui, Ui::popup_key(next.target)).0, again);

    // On the panel, clear of the targets and of the open menu.
    let beside = Vec2::splat(PAGE * 0.5);
    let (menu_min, menu_max) = rect(&ui, Ui::popup_key(next.target));
    assert!(!(beside.cmpge(menu_min).all() && beside.cmplt(menu_max).all()));
    let moved = page(&mut ui, secondary(beside), NavInput::default());
    assert!(moved.menus[2].opened, "the panel's menu did not open");
    assert!(!ui.is_popup_open(moved.target), "the target's stayed open");
    assert_eq!(rect(&ui, Ui::popup_key(moved.panel)).0, beside);
}

/// **A disabled widget's menu never opens**, and disabling a widget whose
/// menu is open closes it.
#[test]
fn a_disabled_widgets_menu_never_opens() {
    let build = |ui: &mut Ui, pointer: PointerInput, enabled: bool| {
        frame(ui, pointer, NavInput::default(), |ui| {
            let size = [
                Declaration::Width(LengthAuto::Px(TARGET.x)),
                Declaration::Height(LengthAuto::Px(TARGET.y)),
            ];
            let mut key = None;
            ui.enabled(enabled, |ui| {
                let target = ui
                    .block_with("#target", &size, Behavior::BUTTON, |_| {})
                    .key;
                ui.context_menu(target, &items());
                key = Some(target);
            });
            key.expect("built")
        })
    };
    let mut ui = Ui::new();
    let target = build(&mut ui, idle(), false);
    let at = inside(&ui, target);
    build(&mut ui, secondary(at), false);
    assert!(!ui.is_popup_open(target), "a disabled widget's menu opened");
    // A press is matched against last frame's marks, so one enabled frame
    // first.
    build(&mut ui, idle(), true);
    build(&mut ui, secondary(at), true);
    assert!(
        ui.is_popup_open(target),
        "the enabled widget's menu did not open"
    );
    build(&mut ui, idle(), false);
    assert!(!ui.is_popup_open(target), "disabling it left the menu open");
}

/// **A menu of nothing but disabled items keeps focus on its widget**, so
/// `ui_menu` can come again: the menu stays open and is not reported opened a
/// second time.
#[test]
fn ui_menu_again_over_a_menu_focus_cannot_enter_reports_nothing() {
    let disabled = [ContextItem::action("Locked", Pick::Locked).enabled(false)];
    let build = |ui: &mut Ui, nav: NavInput| {
        frame(ui, idle(), nav, |ui| {
            let size = [
                Declaration::Width(LengthAuto::Px(TARGET.x)),
                Declaration::Height(LengthAuto::Px(TARGET.y)),
            ];
            let target = ui
                .block_with("#target", &size, Behavior::BUTTON, |_| {})
                .key;
            (target, ui.context_menu(target, &disabled))
        })
    };
    let mut ui = Ui::new();
    build(&mut ui, NavInput::default());
    let (target, _) = build(&mut ui, NavInput::NEXT);
    let (_, first) = build(&mut ui, NavInput::MENU);
    assert!(first.opened);
    build(&mut ui, NavInput::NAVIGATION);
    assert_eq!(
        ui.focused(),
        Some(target),
        "focus entered a menu of nothing"
    );
    let (_, again) = build(&mut ui, NavInput::MENU);
    assert!(!again.opened, "an open menu was reported opened again");
    assert!(ui.is_popup_open(target));
}

/// **A primary press does not open it**, nor does its release.
#[test]
fn a_primary_press_does_not_open_the_menu() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default());
    let at = inside(&ui, built.target);
    let pressed = page(&mut ui, press(at), NavInput::default());
    let released = page(&mut ui, release(at), NavInput::default());
    assert!(!pressed.menus[0].opened && !released.menus[0].opened);
    assert!(!ui.is_popup_open(built.target), "a primary press opened it");
}

/// **At the viewport's bottom right it flips up and to the left of the
/// pointer**, ending at it on both axes, and stays inside the viewport.
#[test]
fn at_the_bottom_right_the_menu_flips_to_end_at_the_pointer() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default());
    let (_, corner_max) = rect(&ui, built.corner);
    let at = corner_max - Vec2::splat(5.0);
    page(&mut ui, secondary(at), NavInput::default());
    let (min, max) = rect(&ui, Ui::popup_key(built.corner));
    assert_eq!(max, at, "the menu did not flip to end at the pointer");
    assert!(min.cmpge(Vec2::ZERO).all(), "it left the viewport");
}

/// **`ui_menu` opens the focused widget's menu below it**, and nothing
/// opens when focus is on a widget no one asked a menu for.
#[test]
fn ui_menu_opens_the_focused_widgets_menu_below_it() {
    let mut ui = Ui::new();
    page(&mut ui, idle(), NavInput::default());
    let built = page(&mut ui, idle(), NavInput::NEXT);
    assert_eq!(ui.focused(), Some(built.target));
    let menu = page(&mut ui, idle(), NavInput::MENU);
    assert!(
        menu.menus[0].opened,
        "ui_menu did not open the target's menu"
    );
    let (target_min, target_max) = rect(&ui, built.target);
    assert_eq!(
        rect(&ui, Ui::popup_key(built.target)).0,
        Vec2::new(target_min.x, target_max.y)
    );

    let mut ui = Ui::new();
    page(&mut ui, idle(), NavInput::default());
    page(&mut ui, idle(), NavInput::NEXT);
    let other = page(&mut ui, idle(), NavInput::NEXT);
    assert_eq!(ui.focused(), Some(other.other));
    let menu = page(&mut ui, idle(), NavInput::MENU);
    assert!(
        menu.menus[2].opened,
        "ui_menu on a widget with no menu did not reach the panel around it"
    );
    assert!(!ui.is_popup_open(other.target));
}

/// **Moves and accept pick an item, reported once, closing the menu**: focus
/// lands on the first item, a move reaches the next, accept picks it — the
/// one frame that reports it — and focus is back on the target.
#[test]
fn moves_and_accept_pick_an_item_once_and_close_the_menu() {
    let mut ui = Ui::new();
    let (built, _) = opened(&mut ui);
    let mut picks = Vec::new();
    let mut step = |ui: &mut Ui, nav: NavInput| picks.extend(page(ui, idle(), nav).menus[0].picked);
    step(&mut ui, NavInput::NAVIGATION);
    assert_eq!(ui.focused(), Some(item(&ui, built.target, "Cut")));
    step(&mut ui, DOWN);
    assert_eq!(ui.focused(), Some(item(&ui, built.target, "Copy")));

    step(&mut ui, NavInput::ACCEPT);
    assert!(!ui.is_popup_open(built.target), "the pick left it open");
    step(&mut ui, NavInput::NAVIGATION);
    step(&mut ui, NavInput::ACCEPT);
    assert_eq!(picks, [Pick::Copy], "one pick was not reported once");
    assert_eq!(ui.focused(), Some(built.target), "focus did not return");
}

/// **A disabled item is never picked or focused**: a click on it leaves the
/// menu open with nothing reported, and a move from the item above it goes
/// past it and the separator to the submenu's item.
#[test]
fn a_disabled_item_is_neither_picked_nor_focused() {
    let mut ui = Ui::new();
    let (built, _) = opened(&mut ui);
    let on = centre(&ui, item(&ui, built.target, "Locked"));
    page(&mut ui, press(on), NavInput::default());
    let released = page(&mut ui, release(on), NavInput::default());
    assert_eq!(
        released.menus[0].picked, None,
        "the disabled item was picked"
    );
    assert!(ui.is_popup_open(built.target), "the click closed the menu");

    page(&mut ui, idle(), NavInput::NAVIGATION);
    page(&mut ui, idle(), DOWN);
    assert_eq!(ui.focused(), Some(item(&ui, built.target, "Copy")));
    page(&mut ui, idle(), DOWN);
    assert_eq!(
        ui.focused(),
        Some(item(&ui, built.target, "Size")),
        "a move stopped on the disabled item or the separator"
    );
}

/// **A click on a submenu's item opens it beside the item, and a click in
/// it picks and closes the whole chain**, reported once.
#[test]
fn a_submenu_opens_beside_its_item_and_its_pick_closes_the_chain() {
    let mut ui = Ui::new();
    let (built, _) = opened(&mut ui);
    let size = item(&ui, built.target, "Size");
    let on = centre(&ui, size);
    page(&mut ui, press(on), NavInput::default());
    page(&mut ui, release(on), NavInput::default());
    assert!(ui.is_popup_open(size), "the click did not open the submenu");
    let (item_min, item_max) = rect(&ui, size);
    assert_eq!(
        rect(&ui, Ui::popup_key(size)).0,
        Vec2::new(item_max.x, item_min.y),
        "the submenu is not beside its item"
    );

    let on = centre(&ui, item(&ui, size, "Large"));
    page(&mut ui, press(on), NavInput::default());
    let picked = page(&mut ui, release(on), NavInput::default());
    assert_eq!(picked.menus[0].picked, Some(Pick::Large));
    assert!(!ui.is_popup_open(size), "the submenu stayed open");
    assert!(!ui.is_popup_open(built.target), "the menu stayed open");
}

/// **At the viewport's right edge a submenu opens to the left of its
/// item**, ending at the item's left edge — reached by the right arrow, which
/// opens it as a click does.
#[test]
fn a_submenu_flips_left_at_the_right_edge() {
    let mut ui = Ui::new();
    let built = page(&mut ui, idle(), NavInput::default());
    let at = inside(&ui, built.corner);
    page(&mut ui, secondary(at), NavInput::default());
    page(&mut ui, idle(), NavInput::NAVIGATION);
    for _ in 0..2 {
        page(&mut ui, idle(), DOWN);
    }
    let size = item(&ui, built.corner, "Size");
    assert_eq!(ui.focused(), Some(size));
    page(&mut ui, idle(), RIGHT);
    assert!(ui.is_popup_open(size), "the right arrow did not open it");
    let (item_min, _) = rect(&ui, size);
    let (_, sub_max) = rect(&ui, Ui::popup_key(size));
    assert_eq!(
        sub_max.x, item_min.x,
        "the submenu did not flip to the left"
    );
}

/// **Back closes only the top submenu, then the menu**, focus going back to
/// the submenu's item and then to the target; left inside a submenu closes
/// it the same way.
#[test]
fn back_closes_the_top_submenu_first_then_the_menu() {
    let mut ui = Ui::new();
    let (built, _) = opened(&mut ui);
    page(&mut ui, idle(), NavInput::NAVIGATION);
    page(&mut ui, idle(), DOWN);
    page(&mut ui, idle(), DOWN);
    let size = item(&ui, built.target, "Size");
    page(&mut ui, idle(), RIGHT);
    page(&mut ui, idle(), NavInput::NAVIGATION);
    assert_eq!(
        ui.focused(),
        Some(item(&ui, size, "Small")),
        "focus did not move into the submenu"
    );

    page(&mut ui, idle(), NavInput::BACK);
    assert!(!ui.is_popup_open(size), "back left the submenu open");
    assert!(ui.is_popup_open(built.target), "back closed the menu too");
    assert_eq!(ui.focused(), Some(size));

    page(&mut ui, idle(), RIGHT);
    page(&mut ui, idle(), NavInput::NAVIGATION);
    page(&mut ui, idle(), LEFT);
    assert!(!ui.is_popup_open(size), "left left the submenu open");
    assert!(ui.is_popup_open(built.target), "left closed the menu");
    page(&mut ui, idle(), NavInput::NAVIGATION);
    assert_eq!(
        ui.focused(),
        Some(size),
        "left did not put focus on the item"
    );

    page(&mut ui, idle(), NavInput::BACK);
    assert!(!ui.is_popup_open(built.target), "back left the menu open");
    assert_eq!(ui.focused(), Some(built.target));
    assert!(
        !ui.back_requested(),
        "a back spent on the menu was reported"
    );
}

/// **A press outside closes the menu and is swallowed**: the block under it
/// is neither pressed nor clicked, and nothing is reported picked.
#[test]
fn a_press_outside_closes_the_menu_and_is_swallowed() {
    let mut ui = Ui::new();
    let (built, _) = opened(&mut ui);
    let on = centre(&ui, built.corner);
    let (menu_min, menu_max) = rect(&ui, Ui::popup_key(built.target));
    assert!(
        !(on.cmpge(menu_min).all() && on.cmplt(menu_max).all()),
        "the fixture's press lands on the menu"
    );
    let interaction = |ui: &Ui| ui.interaction_of(built.corner);
    page(&mut ui, press(on), NavInput::default());
    assert!(!ui.is_popup_open(built.target), "the press left it open");
    assert!(!interaction(&ui).pressed, "the dismissing press pressed it");
    let released = page(&mut ui, release(on), NavInput::default());
    assert_eq!(released.menus[0].picked, None);
    assert!(!interaction(&ui).clicked, "the dismissing press clicked it");
}
