//! `text-overflow: ellipsis` and [`Ui::text`]: a span's lines cut to fit its
//! box in every font a span can draw in, what the rule leaves alone, and the
//! text read back being the text drawn.

use super::ellipsis::{ELLIPSIS, ELLIPSIS_FALLBACK};
use super::tests::{frame, idle};
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::font::layout::TextLayout;
use crate::font::{Font, FontMetrics, GlyphId};
use crate::style::Declaration;

/// What EW's interaction card shows: longer than any of the widths below.
const ACTION: &str = "Take item 18 from the shelf";

/// The card's rules: `.line` is the ellipsis rule whole, and each other class
/// lacks one part of it.
const CSS: &str = "
    .card { flex-direction: column; }
    .line { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .clip { overflow: hidden; white-space: nowrap; }
    .wraps { overflow: hidden; text-overflow: ellipsis; }
    .visible { white-space: nowrap; text-overflow: ellipsis; }
    .inherits { white-space: nowrap; text-overflow: ellipsis; }
    .sans { font-family: sans-serif; font-size: 16px; }
    .roboto { font-family: roboto, sans-serif; font-size: 20px; }
";

/// The three kinds of font a span can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Bitmap,
    Sans,
    Registered,
}

impl Family {
    const ALL: [Self; 3] = [Self::Bitmap, Self::Sans, Self::Registered];

    /// The class that selects it.
    const fn class(self) -> &'static str {
        match self {
            Self::Bitmap => "",
            Self::Sans => ".sans",
            Self::Registered => ".roboto",
        }
    }

    /// `text`'s width in it, measured here rather than by the tree: the bitmap
    /// font's fixed advance, the committed font laid out unbroken, and the
    /// registered font's half-em advance at 20px.
    fn width(self, text: &str) -> f32 {
        match self {
            Self::Bitmap => crate::text::FontAtlas::built_in().text_width(text, 1.0),
            Self::Sans => TextLayout::new(Font::sans(), text, 16.0, 20.0, None).width(),
            Self::Registered => text.chars().count() as f32 * 10.0,
        }
    }

    /// The ellipsis it draws.
    const fn ellipsis(self) -> &'static str {
        match self {
            Self::Sans => ELLIPSIS,
            Self::Bitmap | Self::Registered => ELLIPSIS_FALLBACK,
        }
    }
}

/// A synthetic font every Latin-1 glyph of which is half an em wide, and
/// which has no `…`: past Latin-1 it maps nothing.
fn roboto() -> &'static Font {
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

/// A tree with [`CSS`] and the synthetic font registered as `roboto`.
fn tree() -> Ui {
    let mut ui = Ui::new();
    ui.add_stylesheet("card.css", CSS);
    ui.register_font("roboto", roboto()).expect("not reserved");
    ui
}

/// One frame of a `width`-pixel column holding one span `selector` of
/// `text`; returns the span's key and what the frame emitted.
fn card(ui: &mut Ui, width: f32, selector: &str, text: &str) -> (NodeKey, DrawList) {
    let mut key = None;
    frame(ui, idle(), |ui| {
        ui.block(
            ".card",
            &[Declaration::Width(LengthAuto::Px(width))],
            |ui| {
                key = Some(ui.span(selector, text, &[]).key);
            },
        );
    });
    let mut list = DrawList::new();
    ui.emit(&mut list);
    (key.expect("built"), list)
}

/// Every string and glyph run `list` draws, with where it draws it: what two
/// lists are compared by, a command's font aside.
fn drawn_text(list: &DrawList) -> Vec<String> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text {
                pos,
                text,
                color,
                size,
            } => Some(format!("{pos:?} {text:?} {color:?} {size}")),
            DrawCommand::Glyphs {
                origin,
                size,
                color,
                glyphs,
                ..
            } => Some(format!("{origin:?} {size} {color:?} {glyphs:?}")),
            _ => None,
        })
        .collect()
}

/// The span's content-box width, from its laid-out rectangle.
fn content_width(ui: &Ui, key: NodeKey) -> f32 {
    let (min, max) = ui.rect(key).expect("laid out");
    max.x - min.x
}

/// **A line too long for its box is cut to the longest prefix that fits with
/// the ellipsis after it**, in the bitmap font, the committed font and a
/// registered one, each measured in its own font; the committed font has `…`
/// and the other two draw `...`.
#[test]
fn a_long_line_is_cut_to_fit_in_every_family() {
    assert_ne!(
        Font::sans().glyph_id('\u{2026}'),
        GlyphId::NOTDEF,
        "the committed font has no ellipsis, so nothing tests the glyph path"
    );
    for family in Family::ALL {
        for width in [60.0, 100.0, 137.0] {
            let mut ui = tree();
            let selector = format!(".line{}", family.class());
            let (key, _) = card(&mut ui, width, &selector, ACTION);
            let shown = ui.text(key).expect("a text span");
            let what = format!("{family:?} at {width}px: {shown:?}");

            let kept = shown
                .strip_suffix(family.ellipsis())
                .unwrap_or_else(|| panic!("{what} does not end in its ellipsis"));
            assert!(ACTION.starts_with(kept), "{what} is not a prefix");
            assert!(!kept.is_empty(), "{what} kept nothing");
            let room = content_width(&ui, key);
            assert_eq!(room, width, "{what}: the span is not the column's width");
            assert!(family.width(shown) <= room, "{what} does not fit {room}px");

            // The longest: one more char, trimmed as the cut trims, overflows.
            let next = ACTION[kept.len()..]
                .chars()
                .next()
                .expect("the text was cut");
            let longer = format!(
                "{}{}",
                ACTION[..kept.len() + next.len_utf8()].trim_end(),
                family.ellipsis()
            );
            if longer.trim_end_matches(family.ellipsis()) != kept {
                assert!(
                    family.width(&longer) > room,
                    "{what}: {longer:?} would have fit"
                );
            }
        }
    }

    // Exact where every glyph is ten pixels: seven chars and `...` in 100px.
    for family in [Family::Bitmap, Family::Registered] {
        let mut ui = tree();
        let (key, _) = card(&mut ui, 100.0, &format!(".line{}", family.class()), ACTION);
        assert_eq!(ui.text(key), Some("Take it..."), "{family:?}");
    }
}

/// **Each line is cut on its own**: a short line stays whole beside a cut
/// one, joined by the newline the text holds.
#[test]
fn each_line_is_cut_on_its_own() {
    let mut ui = tree();
    let (key, list) = card(&mut ui, 100.0, ".line", &format!("Short\n{ACTION}"));
    assert_eq!(ui.text(key), Some("Short\nTake it..."));
    assert!(
        list.commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Text { text, .. } if text == "Short\nTake it...")),
        "the bitmap span did not draw its cut text"
    );
}

/// **A line is cut between clusters, never inside one**: a keycap — a digit,
/// a presentation selector and U+20E3, which the committed font draws as
/// `.notdef` beside the digit — is kept whole or dropped whole at every
/// width, though its digit alone is narrower than it.
#[test]
fn a_line_is_cut_between_clusters() {
    let keycap = "1\u{FE0F}\u{20E3}";
    let text = keycap.repeat(12);
    assert!(
        Family::Sans.width("1") < Family::Sans.width(keycap),
        "the digit alone is as wide as the keycap, so no width tells them apart"
    );
    for width in 30..140 {
        let mut ui = tree();
        let (key, _) = card(&mut ui, width as f32, ".line.sans", &text);
        let shown = ui.text(key).expect("a text span");
        let kept = shown.strip_suffix(ELLIPSIS).unwrap_or(shown);
        assert_eq!(
            kept.len() % keycap.len(),
            0,
            "{width}px kept {kept:?}, part of a keycap"
        );
    }
}

/// **A right-to-left line is cut at its logical end, which is its left**: a
/// Hebrew line keeps its first words, as left-to-right text does, and the
/// ellipsis after them draws left of every glyph it kept, where the line's
/// reading ends.
#[test]
fn a_right_to_left_line_is_cut_at_its_left() {
    let words = [
        "\u{5E9}\u{5DC}\u{5D5}\u{5DD}",
        "\u{5E2}\u{5D5}\u{5DC}\u{5DD}",
        "\u{5D8}\u{5D5}\u{5D1}",
        "\u{5D1}\u{5D5}\u{5E7}\u{5E8}",
        "\u{5DC}\u{5D9}\u{5DC}\u{5D4}",
    ];
    let text = words.join(" ");
    let mut ui = tree();
    let (key, list) = card(&mut ui, 100.0, ".line.sans", &text);
    let shown = ui.text(key).expect("a text span");
    let kept = shown
        .strip_suffix(ELLIPSIS)
        .unwrap_or_else(|| panic!("{shown:?} does not end in the ellipsis"));
    assert!(
        !kept.is_empty() && text.starts_with(kept),
        "{shown:?} is not a prefix"
    );
    assert!(Family::Sans.width(shown) <= content_width(&ui, key));

    let ellipsis = Font::sans().glyph_id('\u{2026}');
    let glyphs = list
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Glyphs { glyphs, .. } => Some(glyphs),
            _ => None,
        })
        .expect("the span drew its glyphs");
    let at = glyphs
        .iter()
        .find(|glyph| glyph.glyph == ellipsis)
        .expect("the ellipsis was drawn")
        .offset
        .x;
    assert!(
        glyphs
            .iter()
            .filter(|glyph| glyph.glyph != ellipsis)
            .all(|glyph| glyph.offset.x > at),
        "the ellipsis is not left of the letters it follows"
    );
}

/// **Text that fits is left whole**, and emits the same text command as the
/// span without the rule, independently of its overflow clip.
#[test]
fn text_that_fits_is_untouched() {
    for family in Family::ALL {
        let mut ui = tree();
        let (key, cut) = card(&mut ui, 400.0, &format!(".line{}", family.class()), ACTION);
        assert_eq!(ui.text(key), Some(ACTION), "{family:?}");
        let mut plain = tree();
        let (_, whole) = card(&mut plain, 400.0, family.class(), ACTION);
        assert_eq!(drawn_text(&cut).len(), 1, "{family:?}");
        assert_eq!(
            drawn_text(&cut),
            drawn_text(&whole),
            "{family:?}: the rule changed what fitting text draws"
        );
    }
}

/// **Without every part of the rule the string is not cut**: `clip`, the
/// initial value, keeps the whole line, as does an ellipsis on a span that
/// wraps or does not clip. Overflow clipping still applies independently;
/// `text-overflow` is not inherited while `white-space` is.
#[test]
fn only_the_whole_rule_cuts() {
    for family in Family::ALL {
        for class in [".clip", ".visible"] {
            let mut ui = tree();
            let (key, _) = card(&mut ui, 60.0, &format!("{class}{}", family.class()), ACTION);
            assert_eq!(ui.text(key), Some(ACTION), "{family:?} {class}");
        }
    }
    // Wrapping spans break at spaces rather than overflowing; in the bitmap
    // font, which never wraps, the line overflows and is still not cut.
    for family in Family::ALL {
        let mut ui = tree();
        let (key, _) = card(&mut ui, 60.0, &format!(".wraps{}", family.class()), ACTION);
        assert_eq!(ui.text(key), Some(ACTION), "{family:?} .wraps");
    }

    // A block with the text properties and a span that clips: the span is
    // nowrap by inheritance, but its own `text-overflow` is `clip`.
    let mut ui = tree();
    let mut keys = None;
    let clips = Declaration::Overflow(Overflow::Hidden);
    frame(&mut ui, idle(), |ui| {
        ui.block(
            ".card.inherits",
            &[Declaration::Width(LengthAuto::Px(60.0))],
            |ui| {
                let inherited = ui.span(".sans", ACTION, &[clips]).key;
                let own = ui
                    .span(
                        ".sans",
                        ACTION,
                        &[clips, Declaration::TextOverflow(TextOverflow::Ellipsis)],
                    )
                    .key;
                keys = Some((inherited, own));
            },
        );
    });
    let (inherited, own) = keys.expect("built");
    assert_eq!(ui.text(inherited), Some(ACTION), "text-overflow inherited");
    let (min, max) = ui.rect(inherited).expect("laid out");
    assert_eq!(
        max.y - min.y,
        Font::sans().metrics().normal_line_height(16.0).round(),
        "white-space did not inherit: the span wrapped"
    );
    assert!(
        ui.text(own).expect("a span").ends_with(ELLIPSIS),
        "the span's own text-overflow did not cut it"
    );
}

/// **A `nowrap` span in a parsed font is one line under any width**, measured
/// and drawn, where the same span without it wraps.
#[test]
fn a_nowrap_span_is_one_line() {
    // One line, as layout rounds it.
    let line = Font::sans().metrics().normal_line_height(16.0).round();
    // The span's height and how many baselines its glyph run has.
    let lines = |selector: &str| {
        let mut ui = tree();
        let (key, list) = card(&mut ui, 60.0, selector, ACTION);
        let (min, max) = ui.rect(key).expect("laid out");
        let mut baselines: Vec<u32> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Glyphs { glyphs, .. } => Some(glyphs),
                _ => None,
            })
            .flatten()
            .map(|glyph| glyph.offset.y.to_bits())
            .collect();
        baselines.dedup();
        (max.y - min.y, baselines.len())
    };
    assert_eq!(lines(".clip.sans"), (line, 1));
    let (height, drawn) = lines(".sans");
    assert!(
        height > line && drawn > 1,
        "the fixture does not wrap without it"
    );
}

/// **Changing only a span's `white-space` re-measures it**: the measurement
/// is keyed by it, so the wrapped size is never reused for the unbroken line.
#[test]
fn changing_white_space_remeasures_the_span() {
    let line = Font::sans().metrics().normal_line_height(16.0).round();
    let mut ui = tree();
    let mut height = |selector: &str| {
        let (key, _) = card(&mut ui, 60.0, selector, ACTION);
        let (min, max) = ui.rect(key).expect("laid out");
        max.y - min.y
    };
    assert!(height("#label.sans") > line);
    assert_eq!(height("#label.clip.sans"), line, "the wrapped size stuck");
}

/// **A box too narrow for even the ellipsis shows nothing**, and draws no
/// glyph.
#[test]
fn a_box_too_narrow_for_the_ellipsis_shows_nothing() {
    for family in Family::ALL {
        let mut ui = tree();
        let (key, list) = card(&mut ui, 5.0, &format!(".line{}", family.class()), ACTION);
        assert_eq!(ui.text(key), Some(""), "{family:?}");
        let drawn = list.commands().iter().any(|command| match command {
            DrawCommand::Glyphs { glyphs, .. } => !glyphs.is_empty(),
            DrawCommand::Text { text, .. } => !text.is_empty(),
            _ => false,
        });
        assert!(!drawn, "{family:?} drew text in a 5px box");
    }
}

/// **What [`Ui::text`] returns is what was drawn**: the bitmap font's string,
/// or a parsed font's glyph run, one glyph per char but spaces, each the
/// font's glyph for it.
#[test]
fn the_text_read_back_is_the_text_drawn() {
    for family in Family::ALL {
        for width in [60.0, 100.0, 400.0] {
            let mut ui = tree();
            let (key, list) = card(&mut ui, width, &format!(".line{}", family.class()), ACTION);
            let shown = ui.text(key).expect("a text span");
            let what = format!("{family:?} at {width}px");
            match family {
                Family::Bitmap => {
                    let drawn: Vec<&str> = list
                        .commands()
                        .iter()
                        .filter_map(|command| match command {
                            DrawCommand::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(drawn, [shown], "{what}");
                }
                Family::Sans | Family::Registered => {
                    let (font, glyphs) = list
                        .commands()
                        .iter()
                        .find_map(|command| match command {
                            DrawCommand::Glyphs { font, glyphs, .. } => {
                                Some((font.clone(), glyphs.clone()))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("{what} drew no glyph run"));
                    let want: Vec<GlyphId> = shown
                        .chars()
                        .filter(|&c| c != ' ')
                        .map(|c| font.glyph_id(c))
                        .collect();
                    let got: Vec<GlyphId> = glyphs.iter().map(|glyph| glyph.glyph).collect();
                    assert_eq!(got, want, "{what}: {shown:?}");
                }
            }
        }
    }
}

/// **[`Ui::text`] is `None` for a block, an unknown key and a span not laid
/// out yet**, and a span's text for a span in any family.
#[test]
fn text_is_none_for_anything_but_a_laid_out_span() {
    let selectors = ["#bitmap", "#sans.sans", "#roboto.roboto"];
    let build = |ui: &mut Ui| {
        let mut spans = Vec::new();
        let block = ui.block("#card.card", &[], |ui| {
            for selector in selectors {
                spans.push(ui.span(selector, ACTION, &[]).key);
            }
        });
        (block.key, spans)
    };
    let mut ui = tree();
    let mut keys = None;
    frame(&mut ui, idle(), |ui| keys = Some(build(ui)));
    let (block, spans) = keys.expect("built");
    for span in &spans {
        assert_eq!(ui.text(*span), Some(ACTION));
    }
    assert_eq!(ui.text(block), None, "a block");
    assert_eq!(ui.text(NodeKey(0xdead_beef)), None, "an unknown key");

    // Built again but not laid out: nothing to read yet.
    ui.begin_frame(idle());
    let (_, again) = build(&mut ui);
    assert_eq!(again, spans, "the spans kept their keys");
    assert_eq!(ui.text(spans[0]), None, "a span read before its layout");
}

/// **A span's glyph run carries the text it displays**, cut or whole, so a
/// test holding only the frame's [`DrawList`] reads a parsed-font span as it
/// reads a bitmap one; a run built from glyph ids by hand carries none.
#[test]
fn a_spans_glyph_run_carries_the_text_it_shows() {
    let mut ui = tree();
    for width in [400.0, 60.0] {
        let selector = format!(".line{}", Family::Registered.class());
        let (key, list) = card(&mut ui, width, &selector, ACTION);
        let carried: Vec<Option<&str>> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Glyphs { text, .. } => Some(text.as_deref()),
                _ => None,
            })
            .collect();
        assert_eq!(carried, [ui.text(key)], "at {width} px");
    }
    let (_, cut) = card(&mut ui, 60.0, ".line.roboto", ACTION);
    assert!(
        cut.commands().iter().any(|command| matches!(
            command,
            DrawCommand::Glyphs { text: Some(text), .. } if text.ends_with(ELLIPSIS_FALLBACK)
        )),
        "the cut run carries its ellipsis"
    );

    let mut by_hand = DrawList::new();
    by_hand.glyphs(Vec2::ZERO, roboto(), 20.0, [1.0; 4], Vec::new());
    assert!(matches!(
        by_hand.commands(),
        [DrawCommand::Glyphs { text: None, .. }]
    ));
}

/// **A one-line span in a parsed font is shaped once for cutting and
/// drawing**: the layout its cut ended on is kept and drawn, and draws what
/// laying its shown text out afresh draws — cut or whole, in the committed
/// font and the registered one. Two lines, and the bitmap font, keep none.
#[test]
fn a_cut_spans_layout_is_the_one_drawn() {
    let kept = |ui: &Ui, key: NodeKey| {
        ui.nodes
            .iter()
            .find(|node| node.key == key)
            .expect("built")
            .shown_layout
            .is_some()
    };
    for family in [Family::Sans, Family::Registered] {
        for width in [60.0, 137.0, 1000.0] {
            let mut ui = tree();
            let selector = format!(".line{}", family.class());
            let (key, list) = card(&mut ui, width, &selector, ACTION);
            let what = format!("{family:?} at {width}px: {:?}", ui.text(key));
            assert!(kept(&ui, key), "{what} kept no layout");
            for node in &mut ui.nodes {
                node.shown_layout = None;
            }
            let mut afresh = DrawList::new();
            ui.emit(&mut afresh);
            assert_eq!(
                format!("{:?}", list.commands()),
                format!("{:?}", afresh.commands()),
                "{what} drew differently from its shown text laid out afresh"
            );
        }
    }
    let mut ui = tree();
    let (key, _) = card(&mut ui, 60.0, ".line.sans", &format!("{ACTION}\n{ACTION}"));
    assert!(!kept(&ui, key), "a layout of one line was kept for two");
    let (key, _) = card(&mut ui, 60.0, ".line", ACTION);
    assert!(!kept(&ui, key), "the bitmap font kept a layout");
}

/// **A `nowrap` span is shaped once, at its first layout, and never again
/// while it is unchanged**: layout measures it once under every width it is
/// asked about, cutting and drawing it reuse that layout, and a second frame
/// of the same tree shapes nothing — with and without the ellipsis rule. A
/// span that is cut shapes its whole text once and then only the prefix it is
/// cut to, with its ellipsis — its whole text and then the prefix — and keeps
/// that cut, so an unchanged frame does not cut it again.
#[test]
fn a_nowrap_span_is_shaped_once() {
    use crate::font::shape::shaped_runs;
    const LABEL: &str = "Iron Sword";
    for (selector, width, text) in [
        (".clip.counted", 140.0, LABEL),
        (".line.counted", 1000.0, LABEL),
        (".line.counted", 60.0, ACTION),
        (".line.counted", 60.0, "Incomprehensibilities"),
    ] {
        // A parse of its own, so no other test's shaping is counted.
        let font = Font::parse(crate::font::SANS_TTF).expect("parses");
        let mut ui = tree();
        ui.add_stylesheet("counted.css", ".counted { font-family: counted; }");
        ui.register_font("counted", font.clone())
            .expect("not reserved");
        let what = format!("{selector} at {width}px");
        let before = shaped_runs(&font);
        let _ = TextLayout::nowrap(&font, text, 16.0, 20.0);
        let once = shaped_runs(&font) - before;
        assert!(once > 0, "laying {text:?} out shaped nothing");

        let before = shaped_runs(&font);
        card(&mut ui, width, selector, text);
        let first = shaped_runs(&font) - before;
        let before = shaped_runs(&font);
        let (key, _) = card(&mut ui, width, selector, text);
        let steady = shaped_runs(&font) - before;
        assert_eq!(steady, 0, "{what}: an unchanged frame shaped it again");
        let shown = ui.text(key).expect("a text span");
        if shown == text {
            assert_eq!(first, once, "{what}: a fresh tree shaped it more than once");
        } else {
            let before = shaped_runs(&font);
            let _ = TextLayout::nowrap(&font, shown, 16.0, 20.0);
            let prefix = shaped_runs(&font) - before;
            assert_eq!(
                first,
                once + prefix,
                "{what}: cutting it to {shown:?} shaped more than the prefix it was cut to"
            );
            if !text.contains(' ') {
                assert!(first <= 2, "{what}: a one-word cut shaped {first} runs");
            }
        }
    }
}

/// **A kept layout is only ever handed to the text it lays out**: asked under
/// the right hash for other text, another font, size or line height, the
/// measure cache answers none.
#[test]
fn a_kept_layout_is_checked_against_what_it_lays_out() {
    let font = Font::sans();
    let mut ui = tree();
    let (key, _) = card(&mut ui, 1000.0, ".clip.sans", ACTION);
    let node = ui.nodes.iter().find(|node| node.key == key).expect("built");
    let (hash, size) = (node.content_hash, node.style.font_size);
    let line_height = node.style.text_line_height(font);
    let held = |font: &Font, text: &str, size: f32, line_height: f32| {
        ui.measure
            .nowrap_layout(hash, font, text, size, line_height)
            .is_some()
    };
    assert!(held(font, ACTION, size, line_height), "no layout was kept");
    assert!(!held(font, "Take item 19", size, line_height), "other text");
    assert!(!held(roboto(), ACTION, size, line_height), "another font");
    assert!(!held(font, ACTION, size + 1.0, line_height), "another size");
    assert!(
        !held(font, ACTION, size, line_height + 1.0),
        "another line height"
    );
    let fallback = font.with_fallback(&[roboto().clone()]);
    assert!(
        !held(&fallback, ACTION, size, line_height),
        "other fallback faces"
    );
}

/// The cut as it was first written: a binary search over every cluster
/// boundary, each probe its prefix and ellipsis shaped afresh — what
/// [`a_cut_lands_where_a_binary_search_does`] holds the cut to. Returns the
/// cut text, whether any line was cut, and the layout the cut kept.
fn binary_searched(
    measure: &super::ellipsis::Measure<'_>,
    text: &str,
    width: f32,
) -> (String, bool, Option<TextLayout>) {
    use crate::font::cluster;
    use crate::font::layout::WRAP_EPSILON;
    let mut out = String::new();
    let fit = width.max(0.0) + WRAP_EPSILON;
    let mut scratch = String::new();
    let mut cut_any = false;
    let mut shown = None;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let (whole, layout) = measure.width(line);
        if whole <= fit {
            out.push_str(line);
            shown = layout;
            continue;
        }
        cut_any = true;
        let ellipsis = measure.ellipsis();
        let mut fitted = None;
        let mut fits = |end: usize| {
            scratch.clear();
            scratch.push_str(line[..end].trim_end());
            scratch.push_str(ellipsis);
            let (width, layout) = measure.width(&scratch);
            let fits = width <= fit;
            if fits {
                fitted = layout;
            }
            fits
        };
        let ends: Vec<usize> = cluster::clusters(line).map(|range| range.start).collect();
        if !fits(0) {
            continue;
        }
        let (mut low, mut high) = (0, ends.len());
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if fits(ends[mid]) {
                low = mid;
            } else {
                high = mid;
            }
        }
        out.push_str(line[..ends[low]].trim_end());
        out.push_str(ellipsis);
        shown = fitted;
    }
    let layout = shown
        .filter(|_| !text.contains('\n'))
        .map(|layout| (*layout).clone());
    (out, cut_any, layout)
}

/// What [`a_cut_lands_where_a_binary_search_does`] cuts: Latin kerning pairs,
/// a ligature of three clusters, combining marks, Arabic, Hebrew and both
/// mixed with Latin, ideographs, emoji — a ZWJ sequence, a flag, a keycap and a
/// kerned pair, which the emoji fixture draws — trailing spaces, two lines, and
/// a line a hard break other than a newline breaks.
const CUT_TEXTS: [&str; 11] = [
    "AVATAR Wavy Type To Yo Tea, LT P. F. r. y. W. V.",
    "Office affinity fluffier \u{FB03} efficient waffle",
    "nai\u{308}ve cafe\u{301} re\u{301}sume\u{301} a\u{301}\u{323}\u{304}b",
    "\u{645}\u{631}\u{62D}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645} \u{627}\u{644}\u{639}\u{631}\u{628}\u{64A}",
    "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} \u{5D8}\u{5D5}\u{5D1} \u{5D1}\u{5D5}\u{5E7}\u{5E8}",
    "Hello \u{5E9}\u{5DC}\u{5D5}\u{5DD} world \u{645}\u{631}\u{62D}\u{628}\u{627} again",
    "\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{30C6}\u{30AD}\u{30B9}\u{30C8}\u{3092}\u{5207}\u{308B}",
    "\u{2764}\u{2764} \u{2764}\u{200D}\u{1F525} \u{1F1FA}\u{1F1F8} 1\u{FE0F}\u{20E3} \u{2764}\u{1F525}\u{2764}",
    "Rusty Iron Sword of the Ancient Dragon   ",
    "Take item 18\nfrom the shelf at the back",
    "one\u{2028}line two",
];

/// Cuts every one of [`CUT_TEXTS`] in `font` at `size` at every whole pixel
/// from nothing to wider than it, a quarter past each, and asserts the cut
/// keeps the text and layout [`binary_searched`] does. Returns how many of
/// them were cut.
fn assert_cuts_match(font: &Font, size: f32) -> usize {
    use super::ellipsis::{Measure, cut};
    let measure = Measure::Parsed {
        font,
        size,
        line_height: size * 1.25,
        measured: None,
    };
    let mut cuts = 0;
    for text in CUT_TEXTS {
        let widest = measure.width(text).0;
        for step in 0..=(widest + 10.0) as u32 {
            let width = step as f32 + 0.25;
            let mut out = String::new();
            let got = cut(&measure, text, width, &mut out);
            let layout = got.layout.map(|layout| (*layout).clone());
            let want = binary_searched(&measure, text, width);
            assert_eq!(
                (&out, got.any, &layout),
                (&want.0, want.1, &want.2),
                "{text:?} at {width}px, {size}px"
            );
            cuts += usize::from(got.any);
        }
    }
    cuts
}

/// **The cut lands where a binary search over every cluster boundary does**,
/// keeping the same layout, for every one of [`CUT_TEXTS`] at every width: in
/// the committed font at two sizes, with the emoji fixture behind it, and in
/// the fixed-pitch one.
#[test]
fn a_cut_lands_where_a_binary_search_does() {
    let sans = Font::sans();
    let emoji = sans.with_fallback(&[crate::font::fixtures::shaped()]);
    let cuts: usize = [(sans, 16.0), (sans, 31.0), (&emoji, 16.0), (roboto(), 20.0)]
        .into_iter()
        .map(|(font, size)| assert_cuts_match(font, size))
        .sum();
    assert!(cuts > 1000, "only {cuts} cuts were compared");
}
