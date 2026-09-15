//! The widget set through the tree: each widget held to what it promises by
//! pointer and by navigation, with the engine's own stylesheet in force.

mod button;
mod disclosure;
mod list;
mod split;
mod style;
mod text_input;
mod value;

use glam::Vec2;

use crate::style::Declaration;
use crate::text::FontAtlas;
use crate::tree::{
    AvailableSpace, Direction, FlexDirection, LengthAuto, NavInput, NodeKey, NodeStyle, TextInput,
    Ui,
};
use crate::widget::PointerInput;

/// The page every fixture is built on: a column this many pixels square.
pub(super) const PAGE: f32 = 400.0;

/// One frame: begin with `pointer` and `nav`, build `build` inside a
/// [`PAGE`]-square column, lay it out, and return what `build` returned.
pub(super) fn frame<R>(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    build: impl FnOnce(&mut Ui) -> R,
) -> R {
    frame_with_text(ui, pointer, nav, TextInput::default(), build)
}

/// [`frame`], with `text` as the frame's text input.
pub(super) fn frame_with_text<R>(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    text: TextInput,
    build: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.begin_frame_with(pointer, nav);
    ui.set_text_input(text);
    let mut out = None;
    ui.block(
        "#page",
        &[
            Declaration::FlexDirection(FlexDirection::Column),
            Declaration::Width(LengthAuto::Px(PAGE)),
            Declaration::Height(LengthAuto::Px(PAGE)),
        ],
        |ui| out = Some(build(ui)),
    );
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::splat(PAGE)),
        &FontAtlas::built_in(),
    );
    out.expect("the page builds its content")
}

/// The pointer away from everything.
pub(super) fn idle() -> PointerInput {
    PointerInput::hovering(Vec2::splat(-1.0))
}

/// The pointer held down at `pos`.
pub(super) fn press(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: true,
        released: false,
    }
}

/// The pointer coming up at `pos`.
pub(super) fn release(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: false,
        released: true,
    }
}

pub(super) const UP: NavInput = NavInput::toward(Direction::Up);
pub(super) const DOWN: NavInput = NavInput::toward(Direction::Down);
pub(super) const LEFT: NavInput = NavInput::toward(Direction::Left);
pub(super) const RIGHT: NavInput = NavInput::toward(Direction::Right);

/// Last layout's rectangle for `key`.
pub(super) fn rect(ui: &Ui, key: NodeKey) -> (Vec2, Vec2) {
    ui.rect(key).expect("laid out last frame")
}

/// The middle of `key`'s rectangle.
pub(super) fn centre(ui: &Ui, key: NodeKey) -> Vec2 {
    let (min, max) = rect(ui, key);
    (min + max) * 0.5
}

/// The style `key` was built with this frame.
pub(super) fn style_of(ui: &Ui, key: NodeKey) -> NodeStyle {
    ui.nodes
        .iter()
        .find(|node| node.key == key)
        .expect("built this frame")
        .style
}

/// The keys of every child `key` was built with this frame, in order.
pub(super) fn children_of(ui: &Ui, key: NodeKey) -> Vec<NodeKey> {
    let Some(parent) = ui.nodes.iter().position(|node| node.key == key) else {
        return Vec::new();
    };
    ui.nodes
        .iter()
        .filter(|node| node.parent == Some(parent))
        .map(|node| node.key)
        .collect()
}

/// `#rrggbb` as the linear-light colour the stylesheet decodes it to.
pub(super) fn linear(hex: &str) -> [f32; 4] {
    let mut style = NodeStyle::DEFAULT;
    let mut ui = Ui::new();
    ui.add_stylesheet("colour.css", &format!("#probe {{ background: {hex}; }}"));
    frame(&mut ui, idle(), NavInput::default(), |ui| {
        let key = ui.block("#probe", &[], |_| {}).key;
        style = style_of(ui, key);
    });
    style.background
}
