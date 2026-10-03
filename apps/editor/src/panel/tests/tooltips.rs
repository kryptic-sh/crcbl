//! The toolbar's tooltips: what a button does and the key that does the same,
//! once the pointer has rested on it, until the wheel turns.

use super::*;

use std::time::Duration;

use crcbl::ui::tree::TOOLTIP_DELAY;

impl Page {
    /// One frame `dt` long with the pointer resting at `at`, and `scroll`
    /// pixels of wheel.
    fn rest(&mut self, at: Vec2, dt: Duration, scroll: f32) {
        self.panels.frame(
            &mut self.document,
            PanelInput {
                pointer: PointerInput::hovering(at),
                nav: NavInput::default(),
                text: TextInput {
                    dt,
                    ..TextInput::default()
                },
                extent: EXTENT,
                select: SelectMode::Replace,
                scroll,
            },
        );
    }

    /// What the tooltip hanging from `anchor` says, if the last frame drew
    /// one.
    fn tooltip(&self, anchor: NodeKey) -> Option<&str> {
        let ui = self.panels.ui();
        let span = *ui.child_keys(Ui::tooltip_key(anchor)).first()?;
        ui.text(span)
    }
}

/// **A toolbar button's tooltip says what it does and names its key**, once
/// the pointer has rested on it for the delay — and not before.
#[test]
fn a_toolbar_buttons_tooltip_names_its_key_after_the_delay() {
    let mut page = Page::built_in();
    page.idle();
    let [play, _] = page.panels.toolbar_buttons();
    let at = page.centre(play);
    page.rest(at, TOOLTIP_DELAY, 0.0);
    assert_eq!(page.tooltip(play), None, "shown on the first frame");
    page.rest(at, TOOLTIP_DELAY, 0.0);
    assert_eq!(
        page.tooltip(play),
        Some("Run the scene's games from the scene as it stands (F5)")
    );

    let [_, open, _] = page.panels.file_buttons();
    let at = page.centre(open);
    page.rest(at, TOOLTIP_DELAY, 0.0);
    page.rest(at, TOOLTIP_DELAY, 0.0);
    assert_eq!(
        page.tooltip(play),
        None,
        "the play tooltip outlived the hover"
    );
    let tip = page.tooltip(open).expect("the open button's tooltip");
    assert!(tip.ends_with("(Ctrl+O)"), "{tip:?} does not name its key");
}

/// **The wheel hides a tooltip** though the panels scroll by offset rather
/// than through the tree's own wheel.
#[test]
fn the_wheel_hides_a_toolbar_tooltip() {
    let mut page = Page::built_in();
    page.idle();
    let [play, _] = page.panels.toolbar_buttons();
    let at = page.centre(play);
    page.rest(at, TOOLTIP_DELAY, 0.0);
    page.rest(at, TOOLTIP_DELAY, 0.0);
    assert!(page.tooltip(play).is_some(), "not shown after the delay");
    page.rest(at, TOOLTIP_DELAY, 10.0);
    assert_eq!(page.tooltip(play), None, "the wheel left the tooltip up");
}
