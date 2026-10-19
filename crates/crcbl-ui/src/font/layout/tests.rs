use super::*;
use crate::font::{SANS_TTF, fixtures};

const SIZE: f32 = 16.0;
const LINE: f32 = 20.0;
const TEXT: &str = "The quick brown fox jumps";

/// `piece` shaped straight through `harfrust` in the committed font: each
/// glyph's id and x advance in font units.
fn harfrust(piece: &str) -> Vec<(u32, i32)> {
    let font = harfrust::Font::new(SANS_TTF, 0).expect("reads");
    let mut buffer = harfrust::Buffer::new();
    buffer.push_str(piece);
    buffer.guess_segment_properties();
    harfrust::shape(
        &harfrust::ShaperFont::new(&font),
        &mut buffer,
        harfrust::ShapeOptions::new(),
    )
    .expect("shapes");
    buffer
        .glyph_infos()
        .iter()
        .zip(buffer.glyph_positions())
        .map(|(info, at)| (info.glyph_id, at.x_advance))
        .collect()
}

/// `text`'s width at [`SIZE`], summed here piece by piece: each word and each
/// run of spaces shaped apart by `harfrust`, every advance added in order.
fn width(text: &str) -> f32 {
    let scale = Font::sans().metrics().scale(SIZE);
    let mut pen = 0.0;
    let mut piece = String::new();
    let mut spaces = false;
    for c in text.chars().chain(['\n']) {
        if c == '\n' || (c == ' ') != spaces {
            for (_, advance) in harfrust(&piece) {
                pen += advance as f32 * scale;
            }
            piece.clear();
            spaces = c == ' ';
        }
        piece.push(c);
    }
    pen
}

/// Each line's glyphs, as the non-space glyphs of the string it should be.
fn assert_lines(layout: &TextLayout, want: &[&str], what: &str) {
    let font = Font::sans();
    let got: Vec<Vec<GlyphId>> = layout
        .lines()
        .iter()
        .map(|line| {
            layout.glyphs()[line.glyphs.clone()]
                .iter()
                .map(|g| g.glyph)
                .collect()
        })
        .collect();
    let expected: Vec<Vec<GlyphId>> = want
        .iter()
        .map(|line| {
            line.chars()
                .filter(|&c| c != ' ')
                .map(|c| font.glyph_id(c))
                .collect()
        })
        .collect();
    assert_eq!(got, expected, "{what}: lines are not {want:?}");
    for (line, text) in layout.lines().iter().zip(want) {
        assert!(
            (line.width - width(text.trim_end())).abs() < 1e-4,
            "{what}: {text:?} measured {} rather than {}",
            line.width,
            width(text.trim_end())
        );
    }
}

/// **Greedy breaks at exact places for fixed widths**, the widths chosen
/// either side of where each line's last word ends.
#[test]
fn lines_break_at_the_exact_places_for_fixed_widths() {
    let font = Font::sans();
    // The table's premise, from the independent sum.
    assert!(width("The quick brown fox jumps") > 188.0 && width(TEXT) < 189.0);
    assert!(width("The quick brown fox") > 141.0 && width("The quick brown fox") < 142.0);
    assert!(width("brown fox") < 68.0 && width("fox jumps") > 68.0);

    let cases: [(Option<f32>, &[&str]); 11] = [
        (None, &["The quick brown fox jumps"]),
        (Some(189.0), &["The quick brown fox jumps"]),
        (Some(188.0), &["The quick brown fox", "jumps"]),
        (Some(142.0), &["The quick brown fox", "jumps"]),
        (Some(141.9), &["The quick brown", "fox jumps"]),
        (Some(116.0), &["The quick brown", "fox jumps"]),
        (Some(69.0), &["The quick", "brown fox", "jumps"]),
        (Some(68.0), &["The", "quick", "brown fox", "jumps"]),
        (Some(67.0), &["The", "quick", "brown", "fox", "jumps"]),
        (Some(0.0), &["The", "quick", "brown", "fox", "jumps"]),
        // A word wider than the width overflows its own line unbroken.
        (Some(20.0), &["The", "quick", "brown", "fox", "jumps"]),
    ];
    for (wrap, want) in cases {
        let layout = TextLayout::new(font, TEXT, SIZE, LINE, wrap);
        assert_lines(&layout, want, &format!("wrap {wrap:?}"));
    }
}

/// **Newlines always break, spaces are measured only inside a line, and an
/// empty line is still a line.**
#[test]
fn newlines_and_spaces_follow_the_rules() {
    let font = Font::sans();
    let layout = TextLayout::new(font, "ab\n\ncd", SIZE, LINE, None);
    assert_lines(&layout, &["ab", "", "cd"], "blank line");
    assert_eq!(layout.height(), 3.0 * LINE);

    let empty = TextLayout::new(font, "", SIZE, LINE, None);
    assert_eq!(empty.lines().len(), 1);
    assert_eq!(empty.measure(), Vec2::new(0.0, LINE));

    let spaced = TextLayout::new(font, "  The  quick   \nfox ", SIZE, LINE, None);
    assert_lines(&spaced, &["  The  quick", "fox"], "spaces");
    assert!(spaced.lines()[0].width > width("The quick"));

    // The spaces at a soft break vanish: the next line starts at zero.
    let broken = TextLayout::new(font, "The     quick", SIZE, LINE, Some(40.0));
    assert_lines(&broken, &["The", "quick"], "break in a run of spaces");
    assert_eq!(
        broken.glyphs()[broken.lines()[1].glyphs.start].offset.x,
        0.0
    );
}

/// **Advances are `harfrust`'s for a known string**: each glyph's pen is the
/// sum of the advances `harfrust` gives the word before it, scaled.
#[test]
fn advances_equal_harfrusts_for_a_known_string() {
    let font = Font::sans();
    let scale = font.metrics().scale(SIZE);
    let word = "WAVY Toyota";
    let layout = TextLayout::new(font, word, SIZE, LINE, None);
    let mut want = Vec::new();
    let mut pen = 0.0;
    for (index, piece) in ["WAVY", " ", "Toyota"].into_iter().enumerate() {
        for (glyph, advance) in harfrust(piece) {
            if index != 1 {
                want.push((GlyphId(glyph), pen));
            }
            pen += advance as f32 * scale;
        }
    }
    let got: Vec<(GlyphId, f32)> = layout
        .glyphs()
        .iter()
        .map(|glyph| (glyph.glyph, glyph.offset.x))
        .collect();
    assert_eq!(got, want);
    assert_eq!(layout.width(), pen);
}

/// **Kerning applies within a word and not across a space or a break**:
/// `AV`'s second glyph sits at `A`'s shaped advance, which is less than its
/// own. The shaped fixture kerns heart before heart, and heart and space
/// either way round: two hearts in a word are kerned, two hearts a space
/// apart are not, and a heart a break ends a line with measures its own
/// advance, not the one its space would kern it to.
#[test]
fn kerning_applies_within_a_line_and_not_across_a_break() {
    let font = Font::sans();
    let scale = font.metrics().scale(SIZE);
    let a = font.glyph_id('A');
    let layout = TextLayout::new(font, "AV", SIZE, LINE, None);
    let step = layout.glyphs()[1].offset.x - layout.glyphs()[0].offset.x;
    assert_eq!(step, harfrust("AV")[0].1 as f32 * scale);
    assert!(step < font.advance(a) * scale, "AV is not kerned");

    // The fixture's em is sixteen units, so at sixteen pixels a unit is a
    // pixel: a heart is 16 wide, 13 before a heart.
    let hearts = fixtures::shaped();
    let x = |layout: &TextLayout| -> Vec<f32> {
        layout.glyphs().iter().map(|glyph| glyph.offset.x).collect()
    };
    let word = TextLayout::new(&hearts, "\u{2764}\u{2764}", SIZE, LINE, None);
    assert_eq!(x(&word), [0.0, 13.0]);
    let spaced = TextLayout::new(&hearts, "\u{2764} \u{2764}", SIZE, LINE, None);
    assert_eq!(x(&spaced), [0.0, 32.0], "a heart kerned against a space");
    let broken = TextLayout::new(&hearts, "\u{2764} \u{2764}", SIZE, LINE, Some(20.0));
    let widths: Vec<f32> = broken.lines().iter().map(|line| line.width).collect();
    assert_eq!(widths, [16.0, 16.0], "kerning reached across the break");
    assert_eq!(x(&broken), [0.0, 0.0]);
}

/// **Baselines are one line height apart**, with the font's box centred in
/// each line.
#[test]
fn baselines_are_a_line_height_apart_with_the_box_centred() {
    let font = Font::sans();
    let metrics = font.metrics();
    let scale = metrics.scale(SIZE);
    let layout = TextLayout::new(font, TEXT, SIZE, LINE, Some(0.0));
    let first = (LINE - (metrics.ascent - metrics.descent) * scale) * 0.5 + metrics.ascent * scale;
    for (index, line) in layout.lines().iter().enumerate() {
        assert_eq!(line.baseline, index as f32 * LINE + first);
        for glyph in &layout.glyphs()[line.glyphs.clone()] {
            assert_eq!(glyph.offset.y, line.baseline);
        }
    }
}

/// **A mark is drawn at the shaper's offset from its pen**, on its line's
/// baseline moved up by the offset, and takes no width.
#[test]
fn a_mark_is_drawn_at_the_shapers_offset() {
    let font = Font::sans();
    let layout = TextLayout::new(font, "xh\u{301}", SIZE, LINE, None);
    let mut shaped = Vec::new();
    shape(font, SIZE, "xh\u{301}", 0..4, &mut shaped);
    assert_eq!(layout.glyphs().len(), 3);
    let baseline = layout.lines()[0].baseline;
    let pen = shaped[0].advance + shaped[1].advance;
    let mark = layout.glyphs()[2];
    assert_eq!(mark.offset, Vec2::new(pen, baseline) + shaped[2].offset);
    assert!(shaped[2].offset.y < 0.0, "the mark is not raised");
    assert_eq!(layout.width(), pen, "the mark took width");
}

/// **A cluster falls back whole and ligates in its face**: the heart joined
/// to fire goes to the face that maps both, past one that maps only the
/// heart, as one glyph that spells the whole sequence.
#[test]
fn a_cluster_falls_back_whole_and_ligates() {
    let font = Font::sans().with_fallback(&[fixtures::cbdt(), fixtures::shaped()]);
    let text = "a\u{2764}\u{200D}\u{1F525}b";
    let layout = TextLayout::new(&font, text, SIZE, LINE, None);
    assert_eq!(layout.faces(), [0, 2, 0]);
    let runs = layout.face_runs();
    let spelled: Vec<&str> = runs.iter().map(|run| &text[run.text.clone()]).collect();
    assert_eq!(spelled, ["a", "\u{2764}\u{200D}\u{1F525}", "b"]);
    assert_eq!(runs[1].font, &fixtures::shaped());
}

/// **Alignment moves each line across its box**: left at zero, centred at
/// half the room, right at all of it, and an overflowing line at zero.
#[test]
fn alignment_places_each_line_in_its_box() {
    let font = Font::sans();
    let base = TextLayout::new(font, "The quick\nfox", SIZE, LINE, None);
    for (align, share) in [
        (TextAlign::Left, 0.0),
        (TextAlign::Center, 0.5),
        (TextAlign::Right, 1.0),
    ] {
        let mut layout = base.clone();
        layout.align(100.0, align);
        // Aligning twice is aligning once.
        layout.align(100.0, align);
        for (line, before) in layout.lines().iter().zip(base.lines()) {
            let want = (100.0 - line.width) * share;
            assert_eq!(line.offset, want, "{align:?}");
            let first = line.glyphs.start;
            assert_eq!(
                layout.glyphs()[first].offset.x,
                base.glyphs()[first].offset.x + want,
                "{align:?}"
            );
            assert_eq!(line.width, before.width);
        }
    }
    let mut wide = TextLayout::new(font, TEXT, SIZE, LINE, None);
    wide.align(50.0, TextAlign::Center);
    assert_eq!(wide.lines()[0].offset, 0.0);
}

/// **The wrap width snaps to whole pixels with a sliver of slack**, so two
/// computations of one width an ulp apart agree.
#[test]
fn the_wrap_width_is_whole_pixels() {
    assert_eq!(wrap_width(100.0), 100.0);
    assert_eq!(wrap_width(99.999_99), 100.0);
    assert_eq!(wrap_width(100.000_01), 100.0);
    assert_eq!(wrap_width(100.9), 100.0);
    let measured = TextLayout::new(Font::sans(), TEXT, SIZE, LINE, None).measure();
    let again = TextLayout::new(Font::sans(), TEXT, SIZE, LINE, Some(measured.x));
    assert_eq!(
        again.lines().len(),
        1,
        "a measured width does not fit its own line"
    );
}

/// **Caret stops sit on the pens of clusters**: before each plain char where
/// layout draws it, a char inside a cluster at its cluster's stop, and a
/// ligature of two clusters split evenly between them.
#[test]
fn caret_stops_sit_on_cluster_pens() {
    let font = Font::sans();
    let mut stops = Vec::new();
    caret_stops(font, "Ta h\u{301}", SIZE, &mut stops);
    let layout = TextLayout::new(font, "Ta h\u{301}", SIZE, LINE, None);
    let x: Vec<f32> = layout.glyphs().iter().map(|glyph| glyph.offset.x).collect();
    // T, a, space, h, the mark (inside h's cluster), the end.
    assert_eq!(stops.len(), 6);
    assert_eq!(stops[..2], [x[0], x[1]]);
    assert_eq!(stops[3], x[2], "the stop before h is where h is drawn");
    assert_eq!(stops[4], stops[3], "the mark takes its base's stop");
    assert_eq!(stops[5], layout.width());

    // The shaped face's flag is one glyph for one cluster; two flags' worth
    // of regional indicators is two clusters and two glyphs.
    let flags = Font::sans().with_fallback(&[fixtures::shaped()]);
    let text = "\u{1F1FA}\u{1F1F8}\u{1F1FA}\u{1F1F8}";
    caret_stops(&flags, text, SIZE, &mut stops);
    assert_eq!(stops, [0.0, 0.0, SIZE, SIZE, 2.0 * SIZE]);
    // Heart and fire with no joiner are two clusters the face ligates into
    // one glyph an em wide: the stop between them is half way across it.
    caret_stops(&flags, "\u{2764}\u{1F525}", SIZE, &mut stops);
    assert_eq!(stops, [0.0, SIZE / 2.0, SIZE]);
}

/// **Soft breaks are UAX #14's**: a line breaks after a hyphen inside a
/// word, between two ideographs, and never before closing punctuation, even
/// after a space — the words before it go down with it instead.
#[test]
fn lines_break_where_uax_14_allows() {
    let font = Font::sans();
    let hyphen = wrap_width(width("well-") + 1.0);
    assert!(width("well-known") > hyphen, "the word fits whole");
    let layout = TextLayout::new(font, "well-known", SIZE, LINE, Some(hyphen));
    assert_lines(&layout, &["well-", "known"], "a hyphen");

    let ideographs = "\u{65E5}\u{672C}\u{8A9E}";
    let layout = TextLayout::new(font, ideographs, SIZE, LINE, Some(1.0));
    assert_lines(&layout, &["\u{65E5}", "\u{672C}", "\u{8A9E}"], "ideographs");

    let glued = wrap_width(width("The fox") + 1.0);
    assert!(width("The fox )") > glued, "the punctuation fits");
    let layout = TextLayout::new(font, "The fox )", SIZE, LINE, Some(glued));
    assert_lines(&layout, &["The", "fox )"], "closing punctuation");
}

/// **Every UAX #14 hard break ends a line**, a carriage return and line
/// feed together being one, and none of them is drawn.
#[test]
fn every_hard_break_ends_a_line() {
    let font = Font::sans();
    let layout = TextLayout::new(font, "ab\r\ncd\u{2028}ef\u{B}", SIZE, LINE, None);
    assert_lines(&layout, &["ab", "cd", "ef", ""], "hard breaks");
}

/// **A word cut at a soft break inside it is shaped again on each side when
/// the cut changes its shaping**: two hearts kerned together in one word,
/// with a soft break between them as two ideograph-class pictographs, are
/// each a heart's own width once each takes a line.
#[test]
fn a_word_cut_inside_is_shaped_again_where_the_cut_matters() {
    let hearts = fixtures::shaped();
    let x = |layout: &TextLayout| -> Vec<f32> {
        layout.glyphs().iter().map(|glyph| glyph.offset.x).collect()
    };
    let whole = TextLayout::new(&hearts, "\u{2764}\u{2764}", SIZE, LINE, None);
    assert_eq!(x(&whole), [0.0, 13.0], "the hearts kern");
    let cut = TextLayout::new(&hearts, "\u{2764}\u{2764}", SIZE, LINE, Some(20.0));
    let widths: Vec<f32> = cut.lines().iter().map(|line| line.width).collect();
    assert_eq!(widths, [16.0, 16.0], "the kerning survived the cut");
    assert_eq!(x(&cut), [0.0, 0.0]);
}
