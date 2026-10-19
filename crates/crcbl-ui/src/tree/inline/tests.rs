use super::*;
use crate::font::fixtures;

const SIZE: f32 = 16.0;

/// Each line's text, its fragments' texts joined, and its width.
fn lines(layout: &InlineLayout, runs: &[InlineRun<'_>]) -> Vec<(String, f32)> {
    layout
        .lines()
        .iter()
        .map(|line| {
            let text = layout.fragments()[line.fragments.clone()]
                .iter()
                .map(|fragment| &runs[fragment.run].text[fragment.text.clone()])
                .collect();
            (text, line.width)
        })
        .collect()
}

/// **A word too wide is broken between clusters and never inside one**:
/// at a width nothing fits, each `h` with its accent — two glyphs, one
/// cluster — keeps a line of its own.
#[test]
fn an_overlong_word_never_breaks_inside_a_cluster() {
    let run = InlineRun::new(
        "hh\u{301}h\u{301}",
        RunFont::Parsed(Font::sans()),
        SIZE,
        20.0,
    );
    let runs = [run];
    let layout = InlineLayout::new(&runs, Wrap::Width(1.0), &FontAtlas::built_in());
    let texts: Vec<String> = lines(&layout, &runs)
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(texts, ["h", "h\u{301}", "h\u{301}"]);
}

/// **A word too wide is never broken inside a UAX #29 cluster the shaper
/// split**: `harfrust` gives an Arabic number sign (U+0600, a prepended
/// mark) a cluster of its own, apart from the digit it joins, and the
/// word still breaks only between the two clusters UAX #29 finds.
#[test]
fn an_overlong_word_never_breaks_inside_a_cluster_the_shaper_split() {
    let text = "\u{600}1\u{600}2";
    let runs = [InlineRun::new(
        text,
        RunFont::Parsed(Font::sans()),
        SIZE,
        20.0,
    )];
    let layout = InlineLayout::new(&runs, Wrap::Width(1.0), &FontAtlas::built_in());
    let starts: Vec<usize> = layout.glyphs.iter().map(|glyph| glyph.start).collect();
    assert_eq!(
        starts,
        [0, 2, 3, 5],
        "the shaper joined the sign to its digit"
    );
    let texts: Vec<String> = lines(&layout, &runs)
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(texts, ["\u{600}1", "\u{600}2"]);
}

/// **A paragraph's lines are in visual order across runs**: Hebrew in a
/// second run of a Latin paragraph is its own fragment, right of the
/// Latin, its glyphs' x falling as its chars go on; a paragraph whose
/// first strong char is Hebrew runs right to left whichever run holds
/// it, puts its Latin run on the left, and `start` alignment puts it
/// against the right edge.
#[test]
fn lines_are_in_visual_order_across_runs() {
    let font = Font::sans();
    let atlas = FontAtlas::built_in();
    let hebrew = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
    let runs = [
        InlineRun::new("ab ", RunFont::Parsed(font), SIZE, 20.0),
        InlineRun::new(hebrew, RunFont::Parsed(font), SIZE, 20.0),
    ];
    let layout = InlineLayout::new(&runs, Wrap::None, &atlas);
    let fragments = layout.fragments();
    assert_eq!(fragments.len(), 2);
    assert_eq!((fragments[0].run, fragments[1].run), (0, 1));
    assert!(fragments[0].x + fragments[0].width <= fragments[1].x);
    let x: Vec<f32> = layout.glyphs[fragments[1].glyphs.clone()]
        .iter()
        .map(|glyph| glyph.x)
        .collect();
    assert!(
        x.windows(2).all(|pair| pair[0] > pair[1]),
        "the Hebrew runs left to right: {x:?}"
    );
    assert!(!layout.lines()[0].rtl);

    let runs = [
        InlineRun::new(hebrew, RunFont::Parsed(font), SIZE, 20.0),
        InlineRun::new(" ab", RunFont::Parsed(font), SIZE, 20.0),
    ];
    let mut layout = InlineLayout::new(&runs, Wrap::None, &atlas);
    assert!(layout.lines()[0].rtl);
    let order: Vec<usize> = layout
        .fragments()
        .iter()
        .map(|fragment| fragment.run)
        .collect();
    // The space between the two takes the paragraph's level, one, and the
    // Latin level two, so the second run is two fragments.
    assert_eq!(order, [1, 1, 0], "the Latin is not on the left");
    layout.align(300.0, TextAlign::Start);
    let line = &layout.lines()[0];
    assert_eq!(line.offset, 300.0 - line.width);

    // An override shapes its Latin right to left, which kerns `AV` the
    // other way round.
    let forced = "\u{202E}AVAT\u{202C}";
    let runs = [InlineRun::new(forced, RunFont::Parsed(font), SIZE, 20.0)];
    let layout = InlineLayout::new(&runs, Wrap::None, &atlas);
    let advances = |rtl: bool| -> Vec<f32> {
        let mut shaped = Vec::new();
        shape(font, SIZE, forced, 3..7, rtl, &mut shaped);
        shaped.iter().map(|glyph| glyph.advance).collect()
    };
    let got: Vec<f32> = layout.glyphs.iter().map(|glyph| glyph.advance).collect();
    assert_eq!(got, advances(true));
    assert_ne!(got, advances(false), "the direction shows in no advance");
}

/// **Soft breaks are UAX #14's across runs**: a word in two runs breaks
/// after its hyphen, and closing punctuation after a space never starts
/// a line, so the word before it goes down with it; at min-content every
/// soft break is taken, between ideographs too.
#[test]
fn soft_breaks_follow_uax_14_across_runs() {
    let font = Font::sans();
    let atlas = FontAtlas::built_in();
    let runs = [
        InlineRun::new("The well-", RunFont::Parsed(font), SIZE, 20.0),
        InlineRun::new("known fox )", RunFont::Parsed(font), SIZE, 20.0),
    ];
    let texts = |wrap: Wrap| -> Vec<String> {
        let layout = InlineLayout::new(&runs, wrap, &atlas);
        lines(&layout, &runs)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    };
    assert_eq!(texts(Wrap::MinContent), ["The", "well-", "known", "fox )"]);
    let width = |text: &str| -> f32 {
        let run = [InlineRun::new(text, RunFont::Parsed(font), SIZE, 20.0)];
        lines(&InlineLayout::new(&run, Wrap::None, &atlas), &run)[0].1
    };
    let fox = width("The well-known fox") + 1.0;
    assert!(width("The well-known fox )") > fox, "the punctuation fits");
    assert_eq!(texts(Wrap::Width(fox)), ["The well-known", "fox )"]);
    let ideographs = [InlineRun::new(
        "\u{65E5}\u{672C}",
        RunFont::Parsed(font),
        SIZE,
        20.0,
    )];
    let layout = InlineLayout::new(&ideographs, Wrap::MinContent, &atlas);
    assert_eq!(layout.lines().len(), 2, "no break between ideographs");
}

/// **A paragraph draws a mark at the shaper's offset from its pen**, on
/// its line's baseline, and the mark takes no width.
#[test]
fn a_mark_is_drawn_at_the_shapers_offset() {
    let font = Font::sans();
    let runs = [InlineRun::new(
        "xh\u{301}",
        RunFont::Parsed(font),
        SIZE,
        20.0,
    )];
    let layout = InlineLayout::new(&runs, Wrap::None, &FontAtlas::built_in());
    let mut shaped = Vec::new();
    shape(font, SIZE, "xh\u{301}", 0..4, false, &mut shaped);
    let fragment = &layout.fragments()[0];
    let (_, glyphs, _) = &layout.face_runs(fragment)[0];
    let baseline = layout.lines()[0].baseline;
    let pen = shaped[0].advance + shaped[1].advance;
    assert_ne!(
        shaped[2].offset,
        Vec2::ZERO,
        "the font does not move the mark"
    );
    assert_eq!(
        glyphs[2].offset,
        Vec2::new(pen, baseline) + shaped[2].offset
    );
    assert_eq!(fragment.width, pen);
}

/// **A word broken between clusters has its sides shaped again apart**:
/// three hearts kerned together are each a heart's own width once each
/// takes a line, with nothing of the kerning towards the next.
#[test]
fn a_broken_word_is_shaped_again_on_each_side() {
    let hearts = fixtures::shaped();
    let run = InlineRun::new(
        "\u{2764}\u{2764}\u{2764}",
        RunFont::Parsed(&hearts),
        SIZE,
        20.0,
    );
    let runs = [run];
    let whole = InlineLayout::new(&runs, Wrap::None, &FontAtlas::built_in());
    assert_eq!(
        lines(&whole, &runs)[0].1,
        13.0 + 13.0 + 16.0,
        "the hearts kern"
    );
    let broken = InlineLayout::new(&runs, Wrap::Width(20.0), &FontAtlas::built_in());
    let widths: Vec<f32> = lines(&broken, &runs)
        .into_iter()
        .map(|(_, width)| width)
        .collect();
    assert_eq!(widths, [16.0, 16.0, 16.0]);
}
