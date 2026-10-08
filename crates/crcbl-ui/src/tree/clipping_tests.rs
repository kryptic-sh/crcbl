//! Padding-box clips during tree emission.

use super::*;

#[test]
fn bitmap_text_uses_its_own_overflow_clip() {
    text_uses_its_own_overflow_clip(Ui::new(), "", false);
}

#[cfg(feature = "parsed-font")]
#[test]
fn sans_text_uses_its_own_overflow_clip() {
    let mut ui = Ui::new();
    ui.add_stylesheet("font.css", ".font { font-family: sans-serif; }");
    text_uses_its_own_overflow_clip(ui, ".font", true);
}

#[cfg(feature = "parsed-font")]
#[test]
fn registered_text_uses_its_own_overflow_clip() {
    use crate::font::{Font, FontMetrics};

    let mut ui = Ui::new();
    let font = Font::fixed_pitch(
        FontMetrics {
            units_per_em: 1000,
            ascent: 800.0,
            descent: -200.0,
            line_gap: 0.0,
        },
        500.0,
    );
    ui.register_font("fixture", font).expect("not reserved");
    ui.add_stylesheet("font.css", ".font { font-family: fixture; }");
    text_uses_its_own_overflow_clip(ui, ".font", true);
}

fn text_uses_its_own_overflow_clip(mut ui: Ui, selector: &str, parsed: bool) {
    ui.add_stylesheet(
        "clip.css",
        ".parent { flex-direction: column; }
         .text { width: 100px; height: 40px; white-space: nowrap;
                 border-width: 3px; padding: 5px; flex-shrink: 0; }",
    );
    let selector = format!(".text{selector}");
    for overflow in [Overflow::Visible, Overflow::Hidden, Overflow::Scroll] {
        for text in ["A", "A long line that overflows horizontally", "A\nB\nC"] {
            let mut key = None;
            frame(&mut ui, idle(), |ui| {
                ui.block(
                    ".parent",
                    &NodeStyle {
                        overflow: Overflow::Hidden,
                        border: Edges::all(2.0),
                        padding: Edges::all(Length::Px(3.0)),
                        background: [1.0; 4],
                        border_color: [1.0; 4],
                        ..sized(70.0, 50.0)
                    }
                    .declarations(),
                    |ui| {
                        key = Some(
                            ui.span(
                                &selector,
                                text,
                                &[crate::style::Declaration::Overflow(overflow)],
                            )
                            .key,
                        );
                        ui.span("", "child sibling", &[]);
                    },
                );
                ui.span("", "root sibling", &[]);
            });
            let key = key.expect("built");
            assert_eq!(ui.text(key), Some(text), "clipping must not cut the string");
            assert_eq!(
                ui.rect(key),
                Some((Vec2::splat(5.0), Vec2::new(105.0, 45.0)))
            );
            let ancestor = ClipRect {
                min: Vec2::splat(2.0),
                max: Vec2::new(68.0, 48.0),
            };
            let own = ClipRect {
                min: Vec2::splat(8.0),
                max: Vec2::new(102.0, 42.0),
            };
            for incoming in [
                ClipRect::NONE,
                ClipRect {
                    min: Vec2::splat(10.0),
                    max: Vec2::splat(60.0),
                },
                ClipRect {
                    min: Vec2::splat(200.0),
                    max: Vec2::splat(220.0),
                },
            ] {
                let mut list = DrawList::new();
                list.push_clip(incoming.min, incoming.max);
                ui.emit(&mut list);
                assert_eq!(list.len(), 5, "{:?}", list.commands());
                assert!(
                    matches!(list.commands()[0], DrawCommand::Rect { min, max, .. }
                    if min == Vec2::ZERO && max == Vec2::new(70.0, 50.0))
                );
                assert!(matches!(
                    list.commands()[1],
                    DrawCommand::RectOutline { .. }
                ));
                let parent_clip = incoming.intersect(ancestor);
                let text_clip = if overflow == Overflow::Visible {
                    parent_clip
                } else {
                    parent_clip.intersect(own)
                };
                assert_eq!(
                    list.clips(),
                    &[incoming, incoming, text_clip, parent_clip, incoming],
                    "{overflow:?}, {text:?}, incoming {incoming:?}"
                );
                match &list.commands()[2] {
                    DrawCommand::Text {
                        pos, text: drawn, ..
                    } => {
                        assert!(!parsed);
                        assert_eq!(*pos, Vec2::splat(13.0));
                        assert_eq!(drawn, text);
                    }
                    DrawCommand::Glyphs {
                        origin,
                        text: drawn,
                        glyphs,
                        ..
                    } => {
                        assert!(parsed);
                        assert_eq!(*origin, Vec2::splat(13.0));
                        assert_eq!(drawn.as_deref(), Some(text));
                        assert!(!glyphs.is_empty());
                        if text.contains('\n') {
                            assert!(
                                glyphs.last().expect("nonempty").offset.y > 24.0,
                                "the multiline fixture must exceed the content height"
                            );
                        }
                    }
                    command => panic!("expected text, got {command:?}"),
                }
                assert_eq!(list.clip(), incoming);
                list.text(Vec2::ZERO, "after emit", [1.0; 4], 16.0);
                assert_eq!(list.clips().last(), Some(&incoming));
                list.pop_clip().expect("incoming clip remains");
                assert_eq!(list.clip(), ClipRect::NONE);
                assert!(list.pop_clip().is_err(), "tree emission leaked a clip");
            }
        }
    }
}

/// **`overflow: hidden` clips exactly the block's children to its padding
/// box**, and pops the clip after them.
#[test]
fn overflow_hidden_clips_the_children_to_the_padding_box() {
    let mut ui = Ui::new();
    frame(&mut ui, idle(), |ui| {
        let clipper = NodeStyle {
            overflow: Overflow::Hidden,
            border: Edges::all(2.0),
            padding: Edges::all(Length::Px(3.0)),
            background: [1.0; 4],
            ..sized(40.0, 40.0)
        };
        ui.block("", &[], |ui| {
            ui.block("", &clipper.declarations(), |ui| {
                ui.block(
                    "",
                    &NodeStyle {
                        background: [0.5; 4],
                        ..sized(100.0, 100.0)
                    }
                    .declarations(),
                    |_| {},
                );
            });
            ui.block(
                "",
                &NodeStyle {
                    background: [0.25; 4],
                    ..sized(10.0, 10.0)
                }
                .declarations(),
                |_| {},
            );
        });
    });
    let list = emitted(&ui);
    let clips = list.clips();
    assert_eq!(list.len(), 3, "{:?}", list.commands());
    assert_eq!(
        clips[0],
        ClipRect::NONE,
        "the clipping block is not clipped itself"
    );
    assert_eq!(
        clips[1],
        ClipRect {
            min: Vec2::splat(2.0),
            max: Vec2::splat(38.0)
        },
        "the child is not clipped to the padding box"
    );
    assert_eq!(clips[2], ClipRect::NONE, "the clip outlived the block");
    assert_eq!(list.clip(), ClipRect::NONE);
}
