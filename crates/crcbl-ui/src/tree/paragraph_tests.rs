//! Paragraphs: runs broken into lines together, aligned on one baseline per
//! line, and hit on each fragment they lie on.

use glam::Vec2;

use super::tests::{frame, idle};
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::style::Declaration;
use crate::text::{GLYPH_ADVANCE, LINE_HEIGHT};

/// One frame of a root paragraph `width` pixels wide — unconstrained for
/// `None` — whose runs `build` adds, under `css`, with the pointer at
/// `pointer`. Returns what the runs' builders returned.
fn paragraph<R>(
    ui: &mut Ui,
    pointer: PointerInput,
    width: Option<f32>,
    build: impl FnOnce(&mut ParagraphBuilder<'_>) -> R,
) -> R {
    let mut out = None;
    let inline: Vec<Declaration> = width
        .map(|width| Declaration::Width(LengthAuto::Px(width)))
        .into_iter()
        .collect();
    frame(ui, pointer, |ui| {
        ui.paragraph("#p.p", &inline, |p| out = Some(build(p)));
    });
    out.expect("built")
}

/// What `list` draws as bitmap text: each command's text and top-left.
fn bitmap_text(list: &DrawList) -> Vec<(String, Vec2)> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text { text, pos, .. } => Some((text.clone(), *pos)),
            _ => None,
        })
        .collect()
}

/// **A paragraph in the bitmap font wraps its runs together**: a word that
/// spans two runs moves to the next line whole, each line's runs are drawn
/// edge to edge from its left, and the lines are the bitmap font's line
/// apart.
#[test]
fn a_bitmap_paragraph_wraps_its_runs_together() {
    let mut ui = Ui::new();
    paragraph(&mut ui, idle(), Some(6.5 * GLYPH_ADVANCE), |p| {
        p.run("", "aaa bb", &[]);
        p.run("", "b ccc", &[]);
    });
    let mut list = DrawList::new();
    ui.emit(&mut list);
    // `bb` + `b` is one word, too wide to follow `aaa`, so it moves whole;
    // `bbb ccc` is wider than the line too, so `ccc` takes a third.
    let advance = GLYPH_ADVANCE;
    assert_eq!(
        bitmap_text(&list),
        [
            ("aaa".to_owned(), Vec2::ZERO),
            ("bb".to_owned(), Vec2::new(0.0, LINE_HEIGHT)),
            ("b".to_owned(), Vec2::new(2.0 * advance, LINE_HEIGHT)),
            ("ccc".to_owned(), Vec2::new(0.0, 2.0 * LINE_HEIGHT)),
        ]
    );
    let paragraph = ui.nodes[0].key;
    assert_eq!(
        ui.child_keys(paragraph).len(),
        2,
        "each run is a node under the paragraph"
    );
    let (min, max) = ui.rect(paragraph).expect("laid out");
    assert_eq!((max - min).y, 3.0 * LINE_HEIGHT);
}

#[cfg(feature = "parsed-font")]
mod parsed {
    use super::*;
    use crate::font::{Font, FontMetrics};
    use crate::tree::emit::{
        FALLBACK_UNDERLINE_OFFSET_EM, FALLBACK_UNDERLINE_THICKNESS_EM, MIN_UNDERLINE_THICKNESS,
    };
    use crcbl_core::input::CursorIcon;

    /// Every glyph [`ADVANCE`] wide at [`SIZE`], on a line exactly one em
    /// tall with four fifths of it above the baseline.
    const SIZE: f32 = 20.0;
    const ADVANCE: f32 = SIZE / 2.0;
    /// [`SIZE`]'s line: its height, and how much of it is above the baseline.
    const LINE: f32 = SIZE;
    const ABOVE: f32 = SIZE * 0.8;
    /// What a run twice [`SIZE`] takes.
    const BIG: f32 = 2.0 * SIZE;

    fn fixture() -> &'static Font {
        Font::fixed_pitch(
            FontMetrics {
                units_per_em: 1000,
                ascent: 800.0,
                descent: -200.0,
                line_gap: 0.0,
            },
            500.0,
        )
    }

    /// A tree with the fixture registered and `css` over a paragraph in it.
    fn ui_with(css: &str) -> Ui {
        let mut ui = Ui::new();
        ui.register_font("fixture", fixture())
            .expect("not reserved");
        ui.add_stylesheet(
            "paragraph.css",
            &format!(".p {{ font-family: fixture; font-size: {SIZE}px; }} {css}"),
        );
        ui
    }

    /// Each glyph run `list` draws: its text, its first glyph's pen from the
    /// paragraph's top-left (x, baseline), its size and its colour.
    fn glyph_runs(list: &DrawList) -> Vec<(String, Vec2, f32, [f32; 4])> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Glyphs {
                    origin,
                    glyphs,
                    text,
                    size,
                    color,
                    ..
                } => Some((
                    text.as_deref().unwrap_or_default().to_owned(),
                    *origin + glyphs.first().map_or(Vec2::ZERO, |glyph| glyph.offset),
                    *size,
                    *color,
                )),
                _ => None,
            })
            .collect()
    }

    fn texts_and_pens(ui: &Ui) -> Vec<(String, Vec2)> {
        let mut list = DrawList::new();
        ui.emit(&mut list);
        glyph_runs(&list)
            .into_iter()
            .map(|(text, pen, _, _)| (text, pen))
            .collect()
    }

    fn at(x: f32, line: usize) -> Vec2 {
        Vec2::new(x * ADVANCE, line as f32 * LINE + ABOVE)
    }

    /// **Runs wrap together as one paragraph**: a run boundary inside a word
    /// is no break — `bb` and `b` move as `bbb` — every line starts at the
    /// paragraph's left, and each fragment of a run starts where the one
    /// before it on its line ends.
    #[test]
    fn runs_wrap_together_across_run_boundaries_at_the_paragraphs_width() {
        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), Some(6.5 * ADVANCE), |p| {
            p.run("", "aaa bb", &[]);
            p.run("", "b ccc", &[]);
            p.run("", " dd", &[]);
        });
        assert_eq!(
            texts_and_pens(&ui),
            [
                ("aaa".to_owned(), at(0.0, 0)),
                ("bb".to_owned(), at(0.0, 1)),
                ("b".to_owned(), at(2.0, 1)),
                ("ccc".to_owned(), at(0.0, 2)),
                (" dd".to_owned(), at(4.0, 2)),
            ]
        );
        let (min, max) = ui.rect(ui.nodes[0].key).expect("laid out");
        assert_eq!(max - min, Vec2::new(6.5 * ADVANCE, 3.0 * LINE));

        // Wide enough for all of it, it is one line, run after run.
        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), None, |p| {
            p.run("", "aaa bb", &[]);
            p.run("", "b ccc", &[]);
        });
        assert_eq!(
            texts_and_pens(&ui),
            [
                ("aaa bb".to_owned(), at(0.0, 0)),
                ("b ccc".to_owned(), at(6.0, 0))
            ]
        );
    }

    /// **Mixed sizes share a baseline, and a line is as tall as its runs
    /// reach**: the big run's box puts the baseline lower for every run on
    /// its line, and the next line, of small runs only, is a small line.
    #[test]
    fn runs_of_mixed_sizes_share_one_baseline_per_line() {
        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), Some(7.5 * ADVANCE), |p| {
            p.run("", "ab ", &[]);
            p.run("", "CD", &[Declaration::FontSize(BIG)]);
            p.run("", " ef gh", &[]);
        });
        // `ab CD` is three and four advances; `ef` would end at 10.
        let baseline = BIG * 0.8;
        let mut list = DrawList::new();
        ui.emit(&mut list);
        let runs = glyph_runs(&list);
        let pens: Vec<(&str, Vec2, f32)> = runs
            .iter()
            .map(|(text, pen, size, _)| (text.as_str(), *pen, *size))
            .collect();
        assert_eq!(
            pens,
            [
                ("ab ", Vec2::new(0.0, baseline), SIZE),
                ("CD", Vec2::new(3.0 * ADVANCE, baseline), BIG),
                ("ef gh", Vec2::new(0.0, BIG + ABOVE), SIZE),
            ]
        );
        let (min, max) = ui.rect(ui.nodes[0].key).expect("laid out");
        assert_eq!((max - min).y, BIG + LINE, "a line is not its runs' reach");
    }

    /// **A newline breaks, always, and an empty line takes the run it is in**:
    /// the newlines in a big run make a big empty line, and the paragraph's
    /// height is every line's.
    #[test]
    fn a_hard_break_ends_a_line_and_an_empty_line_takes_its_runs_height() {
        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), None, |p| {
            p.run("", "ab\ncd", &[]);
            p.run("", "\n\nef", &[Declaration::FontSize(BIG)]);
        });
        let big_above = BIG * 0.8;
        assert_eq!(
            texts_and_pens(&ui),
            [
                ("ab".to_owned(), Vec2::new(0.0, ABOVE)),
                ("cd".to_owned(), Vec2::new(0.0, LINE + ABOVE)),
                (
                    "ef".to_owned(),
                    Vec2::new(0.0, 2.0 * LINE + BIG + big_above)
                ),
            ]
        );
        let (min, max) = ui.rect(ui.nodes[0].key).expect("laid out");
        assert_eq!(max - min, Vec2::new(BIG, 2.0 * LINE + 2.0 * BIG));
    }

    /// **A word wider than the line breaks between characters**, at the last
    /// that fits — and after a hyphen first, where there is one — while the
    /// paragraph's min-content width stays its widest word unbroken.
    #[test]
    fn an_overlong_word_breaks_between_characters_and_a_hyphen_breaks_first() {
        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), Some(3.5 * ADVANCE), |p| {
            p.run("", "abcde", &[]);
            p.run("", "fghij", &[]);
        });
        assert_eq!(
            texts_and_pens(&ui),
            [
                ("abc".to_owned(), at(0.0, 0)),
                ("de".to_owned(), at(0.0, 1)),
                ("f".to_owned(), at(2.0, 1)),
                ("ghi".to_owned(), at(0.0, 2)),
                ("j".to_owned(), at(0.0, 3)),
            ]
        );

        let mut ui = ui_with("");
        paragraph(&mut ui, idle(), Some(6.0 * ADVANCE), |p| {
            p.run("", "well-known", &[]);
        });
        assert_eq!(
            texts_and_pens(&ui),
            [
                ("well-".to_owned(), at(0.0, 0)),
                ("known".to_owned(), at(0.0, 1))
            ]
        );

        // Min-content: the widest word, `abcdefghij`, unbroken.
        let mut ui = ui_with("");
        ui.begin_frame(idle());
        let key = ui
            .paragraph("#p.p", &[], |p| {
                p.run("", "ab ", &[]);
                p.run("", "abcde", &[]);
                p.run("", "fghij cd", &[]);
            })
            .key;
        ui.layout(
            Vec2::ZERO,
            AvailableSpace {
                width: Available::MinContent,
                height: Available::MaxContent,
            },
            &FontAtlas::built_in(),
        );
        let (min, max) = ui.rect(key).expect("laid out");
        assert_eq!(max - min, Vec2::new(10.0 * ADVANCE, 3.0 * LINE));
    }

    /// **A run whose text changes is measured again**: the paragraph's size
    /// follows it rather than its cached measurement.
    #[test]
    fn a_paragraph_is_measured_again_when_a_run_changes() {
        let mut ui = ui_with("");
        for (text, width) in [("aa", 3.0), ("aaaa", 5.0), ("aa", 3.0)] {
            paragraph(&mut ui, idle(), None, |p| {
                p.run("", "a", &[]);
                p.run("", text, &[]);
            });
            let (min, max) = ui.rect(ui.nodes[0].key).expect("laid out");
            assert_eq!((max - min).x, width * ADVANCE, "{text}");
        }
    }

    /// **A link that wraps onto two lines is one run hit on both fragments**:
    /// hovering either hovers it — and styles it, on both — a press and a
    /// release on its second line click it, and the space beside its first
    /// fragment, inside the paragraph, is not the link.
    #[test]
    fn a_link_wrapping_onto_two_lines_is_hit_on_both_fragments() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let mut ui = ui_with("a { cursor: pointer; } a:hover { color: #ff0000; }");
        let build = |p: &mut ParagraphBuilder<'_>| {
            p.run("", "see ", &[]);
            let link = p.run("a#more", "the full log", &[]);
            p.run("", " now", &[]);
            link
        };
        let width = Some(9.5 * ADVANCE);
        let link = paragraph(&mut ui, idle(), width, build);
        assert!(!link.hovered);
        // `see the` | `full log` | `now`: the link is `the` on the first line
        // and `full log` on the second.
        let first = (
            at(4.0, 0) - Vec2::new(0.0, ABOVE),
            at(7.0, 0) + Vec2::new(0.0, LINE - ABOVE),
        );
        let second = (Vec2::new(0.0, LINE), Vec2::new(8.0 * ADVANCE, 2.0 * LINE));
        assert_eq!(
            ui.rect(link.key),
            Some((Vec2::new(0.0, 0.0), second.1)),
            "the link's box is not round both fragments"
        );
        let mut list = DrawList::new();
        ui.emit(&mut list);
        let link_runs = glyph_runs(&list)
            .into_iter()
            .filter(|(text, ..)| text == "the" || text == "full log")
            .count();
        assert_eq!(link_runs, 2, "the link is not drawn as two fragments");

        let centre = |(min, max): (Vec2, Vec2)| (min + max) * 0.5;
        for (pointer, over) in [
            (centre(first), true),
            (centre(second), true),
            // Right of `the`, on the first line: the paragraph, not the link.
            (Vec2::new(8.5 * ADVANCE, LINE * 0.5), false),
            // Left of `the`, on `see `.
            (Vec2::new(ADVANCE, LINE * 0.5), false),
        ] {
            paragraph(&mut ui, PointerInput::hovering(pointer), width, build);
            let link = paragraph(&mut ui, PointerInput::hovering(pointer), width, build);
            assert_eq!(link.hovered, over, "at {pointer}");
            assert_eq!(
                ui.cursor(),
                Some(if over {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }),
                "at {pointer}"
            );
            let mut list = DrawList::new();
            ui.emit(&mut list);
            let reds = glyph_runs(&list)
                .into_iter()
                .filter(|(.., color)| *color == red)
                .map(|(text, ..)| text)
                .collect::<Vec<_>>();
            let want: &[&str] = if over { &["the", "full log"] } else { &[] };
            assert_eq!(reds, want, "at {pointer}");
        }

        let on_second = centre(second);
        let press = PointerInput {
            down: true,
            ..PointerInput::hovering(on_second)
        };
        let release = PointerInput {
            released: true,
            ..PointerInput::hovering(on_second)
        };
        assert!(!paragraph(&mut ui, press, width, build).clicked);
        assert!(
            paragraph(&mut ui, release, width, build).clicked,
            "a click on the link's second line did not click it"
        );
    }

    /// **Kerning reaches across a run boundary between two runs of one font
    /// and size, and not between runs that differ**: `AV` split into two runs
    /// draws as `AV` does, and with the `V` larger it draws unkerned.
    #[test]
    fn kerning_crosses_a_run_boundary_only_between_runs_of_one_font_and_size() {
        let sans = Font::sans();
        let (a, v) = (sans.glyph_id('A'), sans.glyph_id('V'));
        assert!(
            sans.kerning(a, v) < 0.0,
            "the committed font does not kern AV"
        );
        let scale = sans.metrics().scale(SIZE);
        let second_pen = |v_size: f32| {
            let mut ui = Ui::new();
            ui.add_stylesheet("kern.css", ".p { font-family: sans-serif; }");
            paragraph(&mut ui, idle(), None, |p| {
                p.run("", "A", &[Declaration::FontSize(SIZE)]);
                p.run("", "V", &[Declaration::FontSize(v_size)]);
            });
            let mut list = DrawList::new();
            ui.emit(&mut list);
            glyph_runs(&list)[1].1.x
        };
        let advance = sans.advance(a) * scale;
        assert_eq!(second_pen(SIZE), advance + sans.kerning(a, v) * scale);
        assert_eq!(second_pen(BIG), advance);
    }

    /// Every filled rectangle `list` draws: an underline, here.
    fn rects(list: &DrawList) -> Vec<(Vec2, Vec2, [f32; 4])> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color } => Some((*min, *max, *color)),
                _ => None,
            })
            .collect()
    }

    /// **An underlined run is underlined under each of its fragments**, from
    /// its left to its right on each line, the fallback distance below that
    /// line's baseline for a font that does not say — and the runs beside it
    /// are not.
    #[test]
    fn an_underlined_run_is_underlined_under_each_fragment() {
        let blue = [0.0, 0.0, 1.0, 1.0];
        let mut ui = ui_with("a { text-decoration: underline; color: #0000ff; }");
        paragraph(&mut ui, idle(), Some(9.5 * ADVANCE), |p| {
            p.run("", "see ", &[]);
            p.run("a", "the full log", &[]);
            p.run("", " now", &[]);
        });
        let mut list = DrawList::new();
        ui.emit(&mut list);
        let below = SIZE * FALLBACK_UNDERLINE_OFFSET_EM;
        let thick = SIZE * FALLBACK_UNDERLINE_THICKNESS_EM;
        assert!(
            thick > MIN_UNDERLINE_THICKNESS,
            "the fixture's line is clamped"
        );
        let line = |left: f32, right: f32, baseline: f32| {
            (
                Vec2::new(left * ADVANCE, baseline + below),
                Vec2::new(right * ADVANCE, baseline + below + thick),
                blue,
            )
        };
        assert_eq!(
            rects(&list),
            [line(4.0, 7.0, ABOVE), line(0.0, 8.0, LINE + ABOVE)]
        );
    }

    /// **A font's own underline is where its `post` table says**, scaled to
    /// the run's size and measured from the baseline its glyphs sit on; a run
    /// inherits the paragraph's underline, and one that says `none` has
    /// none.
    #[test]
    fn a_fonts_own_underline_is_drawn_where_its_post_table_says() {
        let sans = Font::sans();
        let line = sans.underline().expect("the committed font says");
        assert!(line.offset < 0.0, "the committed underline is not below");
        let scale = sans.metrics().scale(BIG);
        let mut ui = Ui::new();
        ui.add_stylesheet(
            "underline.css",
            ".p { font-family: sans-serif; text-decoration: underline; } .plain {              text-decoration: none; }",
        );
        paragraph(&mut ui, idle(), None, |p| {
            p.run("", "Hello", &[Declaration::FontSize(BIG)]);
            p.run(".plain", " world", &[Declaration::FontSize(BIG)]);
        });
        let mut list = DrawList::new();
        ui.emit(&mut list);
        let baseline = glyph_runs(&list)[0].1.y;
        let drawn = rects(&list);
        assert_eq!(drawn.len(), 1, "the plain run was underlined: {drawn:?}");
        let (min, max, _) = drawn[0];
        assert_eq!(min.y, baseline - line.offset * scale);
        let thickness = (line.thickness * scale).max(MIN_UNDERLINE_THICKNESS);
        assert!(
            (max.y - min.y - thickness).abs() < 1e-4,
            "the underline is {} thick rather than {thickness}",
            max.y - min.y
        );
        let hello: f32 = "Hello"
            .chars()
            .map(|c| sans.advance(sans.glyph_id(c)) * scale)
            .sum();
        assert_eq!(min.x, 0.0);
        assert!(
            (max.x - hello).abs() < 1.0,
            "the underline ends at {} rather than under `Hello`'s {hello}",
            max.x
        );
    }

    /// **A text span is underlined line by line too**: under each line its
    /// text breaks into, in a parsed font and in the bitmap font.
    #[test]
    fn a_text_span_is_underlined_under_each_line() {
        let mut ui = ui_with(".u { text-decoration: underline; } .bitmap { font-family: bitmap; }");
        frame(&mut ui, idle(), |ui| {
            ui.block(
                "",
                &[Declaration::FlexDirection(FlexDirection::Column)],
                |ui| {
                    ui.span(
                        ".p.u",
                        "ab
cde",
                        &[],
                    );
                    ui.span(".u.bitmap", "xy", &[]);
                },
            );
        });
        let mut list = DrawList::new();
        ui.emit(&mut list);
        let below = SIZE * FALLBACK_UNDERLINE_OFFSET_EM;
        let thick = SIZE * FALLBACK_UNDERLINE_THICKNESS_EM;
        let white = [1.0; 4];
        let bitmap_top = 2.0 * LINE;
        let bitmap_size = NodeStyle::DEFAULT.font_size;
        let bitmap_baseline = bitmap_top + crate::text::ASCENDER as f32;
        let bitmap_below = bitmap_size * FALLBACK_UNDERLINE_OFFSET_EM;
        let bitmap_thick =
            (bitmap_size * FALLBACK_UNDERLINE_THICKNESS_EM).max(MIN_UNDERLINE_THICKNESS);
        assert_eq!(
            rects(&list),
            [
                (
                    Vec2::new(0.0, ABOVE + below),
                    Vec2::new(2.0 * ADVANCE, ABOVE + below + thick),
                    white
                ),
                (
                    Vec2::new(0.0, LINE + ABOVE + below),
                    Vec2::new(3.0 * ADVANCE, LINE + ABOVE + below + thick),
                    white
                ),
                (
                    Vec2::new(0.0, bitmap_baseline + bitmap_below),
                    Vec2::new(
                        2.0 * GLYPH_ADVANCE,
                        bitmap_baseline + bitmap_below + bitmap_thick
                    ),
                    white
                ),
            ]
        );
    }
}
