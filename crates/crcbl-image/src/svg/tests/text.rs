//! `<text>` through the renderer with fonts of the tests' own: each layout
//! rule checked against the same glyphs drawn as `<path>` elements whose
//! data is read from the font here, through `skrifa`, and placed by the
//! rule worked by hand; refusals, fallbacks and the caps at their edges.

use std::path::PathBuf;

use crcbl_font::{FontMatch, FontQuery, FontStyle};
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use super::*;
use crate::svg::render::text::MAX_CHARACTERS;
use crate::svg::text::faces::{FontSource, MAX_FACES, MAX_LOOKUPS};
use crate::svg::{render, xml};

const ATKINSON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crcbl-ui/fonts/AtkinsonHyperlegible-Regular.ttf"
);
const ATKINSON: &[u8] =
    include_bytes!("../../../../crcbl-ui/fonts/AtkinsonHyperlegible-Regular.ttf");

/// An outline heart at U+2764 and nothing else.
const HEART_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crcbl-ui/tests/fixtures/emoji/text.ttf"
);
const HEART: &[u8] = include_bytes!("../../../../crcbl-ui/tests/fixtures/emoji/text.ttf");

fn face(path: impl Into<PathBuf>, family: &str) -> FontMatch {
    FontMatch {
        path: path.into(),
        index: 0,
        family: family.to_owned(),
        weight: 400,
        style: FontStyle::Normal,
    }
}

/// `Atkinson` (and `serif`) is Atkinson Hyperlegible, `Heart` the heart
/// font; with `fallback`, the heart font is every other face's fallback.
struct Fixtures {
    fallback: bool,
}

impl FontSource for Fixtures {
    fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        match query.family.as_str() {
            "Atkinson" | "serif" => Some(face(ATKINSON_PATH, "Atkinson")),
            "Heart" => Some(face(HEART_PATH, "Heart")),
            _ => None,
        }
    }

    fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        if self.fallback && query.family != "Heart" {
            vec![face(HEART_PATH, "Heart")]
        } else {
            Vec::new()
        }
    }
}

static FIXTURES: Fixtures = Fixtures { fallback: true };
static LISTED_ONLY: Fixtures = Fixtures { fallback: false };

/// A system with no fonts at all.
struct Nothing;

impl FontSource for Nothing {
    fn find(&self, _: &FontQuery) -> Option<FontMatch> {
        None
    }

    fn fallbacks(&self, _: &FontQuery) -> Vec<FontMatch> {
        Vec::new()
    }
}

static NOTHING: Nothing = Nothing;

fn draw_with(
    text: &str,
    side: u32,
    fonts: &'static dyn FontSource,
) -> Result<Rgba8Image, ImageError> {
    let document = xml::parse(text.as_bytes())?;
    Ok(render::render_with(&document, side, &Limits::DEFAULT, fonts)?.into_image())
}

/// `body` drawn in a 32-square with the fixture fonts.
fn draw(body: &str) -> Rgba8Image {
    let text = svg(32, body);
    draw_with(&text, 32, &FIXTURES).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn draw_refused(body: &str, fonts: &'static dyn FontSource) -> ImageError {
    let text = svg(32, body);
    draw_with(&text, 32, fonts).expect_err(&text)
}

/// Pixels with any coverage.
fn inked(image: &Rgba8Image) -> usize {
    image
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 0)
        .count()
}

/// Every channel of every pixel within `tolerance`, and the images not
/// blank, so two empty drawings never pass for a match.
fn assert_same(got: &Rgba8Image, want: &Rgba8Image, tolerance: u8) {
    assert!(inked(want) > 0, "the expected drawing is blank");
    for (index, (a, b)) in got.pixels().iter().zip(want.pixels()).enumerate() {
        assert!(
            a.abs_diff(*b) <= tolerance,
            "pixel {} channel {}: {a} against {b}",
            index / 4,
            index % 4
        );
    }
}

/// A glyph of a font, read here.
struct Outline {
    /// Path data in font units, `y` up.
    commands: Vec<(char, Vec<(f32, f32)>)>,
    advance: f64,
    units_per_em: f64,
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(('M', vec![(x, y)]));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(('L', vec![(x, y)]));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.commands.push(('Q', vec![(cx0, cy0), (x, y)]));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.commands
            .push(('C', vec![(cx0, cy0), (cx1, cy1), (x, y)]));
    }
    fn close(&mut self) {
        self.commands.push(('Z', Vec::new()));
    }
}

/// `ch`'s glyph in `font`, its `.notdef` if the font has none.
fn outline(font: &[u8], ch: char) -> Outline {
    let font = FontRef::new(font).expect("a font");
    let id = font.charmap().map(ch).unwrap_or(GlyphId::NOTDEF);
    let location = LocationRef::default();
    let mut out = Outline {
        commands: Vec::new(),
        advance: f64::from(
            GlyphMetrics::new(&font, Size::unscaled(), location)
                .advance_width(id)
                .expect("an advance"),
        ),
        units_per_em: f64::from(Metrics::new(&font, Size::unscaled(), location).units_per_em),
    };
    font.outline_glyphs()
        .get(id)
        .expect("an outline")
        .draw(DrawSettings::unhinted(Size::unscaled(), location), &mut out)
        .expect("draws");
    out
}

impl Outline {
    /// Its advance at `size`, in user units.
    fn width(&self, size: f64) -> f64 {
        self.advance * size / self.units_per_em
    }

    /// Path data of it at `size` with its origin at `(x, y)`: font units
    /// scaled to the size and flipped, `y` down.
    fn data(&self, x: f64, y: f64, size: f64) -> String {
        let scale = size / self.units_per_em;
        let mut out = String::new();
        for (command, points) in &self.commands {
            out.push(*command);
            for &(px, py) in points {
                out.push_str(&format!(
                    " {} {} ",
                    x + f64::from(px) * scale,
                    y - f64::from(py) * scale
                ));
            }
        }
        out
    }
}

/// `<path>` elements drawing `glyphs` as text would, each `(font, ch, x,
/// y, size)`, with `attributes`.
fn paths(glyphs: &[(&[u8], char, f64, f64, f64)], attributes: &str) -> String {
    glyphs
        .iter()
        .map(|&(font, ch, x, y, size)| {
            format!(
                r#"<path d="{}" {attributes}/>"#,
                outline(font, ch).data(x, y, size)
            )
        })
        .collect()
}

/// **A glyph's outline sits at its pen position**, scaled from font units
/// by the font size and flipped, and the pen moves on by the glyph's
/// `hmtx` advance.
#[test]
fn glyphs_are_drawn_at_the_pen() {
    let h = outline(ATKINSON, 'H');
    let got =
        draw(r#"<text x="2" y="24" font-family="Atkinson" font-size="20" fill="red">Hi</text>"#);
    let want = draw(&paths(
        &[
            (ATKINSON, 'H', 2.0, 24.0, 20.0),
            (ATKINSON, 'i', 2.0 + h.width(20.0), 24.0, 20.0),
        ],
        r#"fill="red""#,
    ));
    assert_same(&got, &want, 1);
}

/// **`text-anchor` shifts a chunk back by its advance**, all of it for
/// `end` and half for `middle`.
#[test]
fn anchors_shift_by_the_advance() {
    let width = outline(ATKINSON, 'H').width(16.0) + outline(ATKINSON, 'i').width(16.0);
    for (anchor, shift) in [("end", width), ("middle", width / 2.0)] {
        let got = draw(&format!(
            r#"<text x="28" y="24" font-family="Atkinson" font-size="16" text-anchor="{anchor}">Hi</text>"#
        ));
        let want = draw(&format!(
            r#"<text x="{}" y="24" font-family="Atkinson" font-size="16">Hi</text>"#,
            28.0 - shift
        ));
        assert_same(&got, &want, 1);
    }
}

/// **Position lists assign to characters in order**: a list on the text
/// places each character, a `tspan`'s list wins for its own, and `dx` and
/// `dy` shift from where the pen is.
#[test]
fn position_lists_place_characters() {
    let separate = draw(
        r#"<g font-family="Atkinson" font-size="16"><text x="2" y="24">H</text><text x="16" y="24">i</text></g>"#,
    );
    for body in [
        r#"<text x="2 16" y="24">Hi</text>"#,
        r#"<text x="2 30" y="24">H<tspan x="16">i</tspan></text>"#,
        r#"<text x="2" y="24">H<tspan x="16" y="24 0">i</tspan></text>"#,
    ] {
        let got = draw(&format!(
            r#"<g font-family="Atkinson" font-size="16">{body}</g>"#
        ));
        assert_same(&got, &separate, 1);
    }
    let h = outline(ATKINSON, 'H').width(16.0);
    let shifted = draw(
        r#"<text x="2" y="24" font-family="Atkinson" font-size="16">H<tspan dx="3" dy="-4">i</tspan></text>"#,
    );
    let want = draw(&format!(
        r#"<g font-family="Atkinson" font-size="16"><text x="2" y="24">H</text><text x="{}" y="20">i</text></g>"#,
        2.0 + h + 3.0
    ));
    assert_same(&shifted, &want, 1);
}

/// **White space collapses**, across elements and at the ends, unless
/// `xml:space` preserves it, in which case each space advances the pen.
#[test]
fn white_space_collapses_unless_preserved() {
    let plain = draw(r#"<text x="2" y="24" font-family="Atkinson" font-size="12">H i</text>"#);
    let spaced = draw(
        "<text x=\"2\" y=\"24\" font-family=\"Atkinson\" font-size=\"12\">  H \n\t<tspan> i</tspan>  </text>",
    );
    assert_same(&spaced, &plain, 0);
    let space = outline(ATKINSON, ' ').width(12.0);
    let preserved = draw(
        r#"<text xml:space="preserve" x="2" y="24" font-family="Atkinson" font-size="12">  H</text>"#,
    );
    let want = draw(&format!(
        r#"<text x="{}" y="24" font-family="Atkinson" font-size="12">H</text>"#,
        2.0 + 2.0 * space
    ));
    assert_same(&preserved, &want, 1);
    // `xml:space` is inherited, from outside the text and into its spans,
    // and a span may set it back.
    for body in [
        r#"<g xml:space="preserve"><text x="2" y="24">  H</text></g>"#,
        r#"<text xml:space="preserve" x="2" y="24"><tspan>  H</tspan></text>"#,
        r#"<text x="2" y="24"><tspan xml:space="preserve"> </tspan> H</text>"#,
        r#"<g xml:space="preserve"><text xml:space="default" x="2" y="24">  <tspan xml:space="preserve">  </tspan>H</text></g>"#,
    ] {
        let got = draw(&format!(
            r#"<g font-family="Atkinson" font-size="12">{body}</g>"#
        ));
        assert_same(&got, &want, 1);
    }
}

/// **Each `tspan` paints with its own style**, and a percentage font size
/// is of its parent's.
#[test]
fn spans_paint_and_size_themselves() {
    let h = outline(ATKINSON, 'H').width(20.0);
    let got = draw(
        r#"<text x="2" y="24" font-family="Atkinson" font-size="20" fill="red">H<tspan fill="blue" font-size="50%">i</tspan></text>"#,
    );
    let want = draw(&format!(
        "{}{}",
        paths(&[(ATKINSON, 'H', 2.0, 24.0, 20.0)], r#"fill="red""#),
        paths(&[(ATKINSON, 'i', 2.0 + h, 24.0, 10.0)], r#"fill="blue""#)
    ));
    assert_same(&got, &want, 1);
    // Text between spans comes back in document order, each piece once.
    let interleaved = draw(
        r#"<text x="1" y="24" font-family="Atkinson" font-size="8">H<tspan>i</tspan>i<tspan>H</tspan>i</text>"#,
    );
    let plain = draw(r#"<text x="1" y="24" font-family="Atkinson" font-size="8">HiiHi</text>"#);
    assert_same(&interleaved, &plain, 0);
}

/// **Letter spacing follows every character and word spacing every
/// space**, in user units or ems.
#[test]
fn spacing_widens_the_advance() {
    let (h, space) = (
        outline(ATKINSON, 'H').width(10.0),
        outline(ATKINSON, ' ').width(10.0),
    );
    let got = draw(
        r#"<text x="1" y="24" font-family="Atkinson" font-size="10" letter-spacing="0.3em" word-spacing="5">H i</text>"#,
    );
    let want = draw(&format!(
        r#"<text x="1 {} {}" y="24" font-family="Atkinson" font-size="10">H i</text>"#,
        1.0 + h + 3.0,
        1.0 + h + 3.0 + space + 3.0 + 5.0
    ));
    assert_same(&got, &want, 1);
}

/// **A character no family of the list has comes from a fallback face**,
/// one the first face's fallbacks name, at the pen.
#[test]
fn missing_characters_fall_back() {
    let h = outline(ATKINSON, 'H').width(16.0);
    let got = draw(r#"<text x="2" y="24" font-family="Atkinson" font-size="16">H&#x2764;</text>"#);
    let want = draw(&paths(
        &[
            (ATKINSON, 'H', 2.0, 24.0, 16.0),
            (HEART, '\u{2764}', 2.0 + h, 24.0, 16.0),
        ],
        "",
    ));
    assert_same(&got, &want, 1);
    // Families of the list are tried in order, character by character,
    // before any fallback: `Heart` has no `H` and `Atkinson` no heart.
    let listed = draw_with(
        &svg(
            32,
            r#"<text x="2" y="24" font-family="Missing, Heart, Atkinson" font-size="16">H&#x2764;</text>"#,
        ),
        32,
        &LISTED_ONLY,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_same(&listed, &want, 1);
}

/// **A character no face has draws the first face's `.notdef`**, as
/// browsers draw it.
#[test]
fn unmapped_characters_draw_notdef() {
    let got = draw(r#"<text x="2" y="24" font-family="Atkinson" font-size="16">&#x4E00;</text>"#);
    let want = draw(&paths(&[(ATKINSON, '\u{4E00}', 2.0, 24.0, 16.0)], ""));
    assert_same(&got, &want, 1);
}

/// **`display: none` takes a span's characters out of the text; hidden
/// ones keep their place.**
#[test]
fn hidden_spans_keep_or_lose_their_place() {
    let separate = draw(
        r#"<g font-family="Atkinson" font-size="16"><text x="2" y="24">H</text><text x="16" y="24">i</text></g>"#,
    );
    let gone = draw(
        r#"<text x="2 16" y="24" font-family="Atkinson" font-size="16">H<tspan display="none">X</tspan>i</text>"#,
    );
    assert_same(&gone, &separate, 1);
    let (h, i) = (
        outline(ATKINSON, 'H').width(12.0),
        outline(ATKINSON, 'i').width(12.0),
    );
    let hidden = draw(
        r#"<text x="2" y="24" font-family="Atkinson" font-size="12">H<tspan visibility="hidden">i</tspan>H</text>"#,
    );
    let want = draw(&paths(
        &[
            (ATKINSON, 'H', 2.0, 24.0, 12.0),
            (ATKINSON, 'H', 2.0 + h + i, 24.0, 12.0),
        ],
        "",
    ));
    assert_same(&hidden, &want, 1);
}

/// **Text strokes as a path does.**
#[test]
fn text_strokes_like_a_path() {
    let got = draw(
        r#"<text x="2" y="24" font-family="Atkinson" font-size="24" fill="none" stroke="blue" stroke-width="0.75">H</text>"#,
    );
    let want = draw(&paths(
        &[(ATKINSON, 'H', 2.0, 24.0, 24.0)],
        r#"fill="none" stroke="blue" stroke-width="0.75""#,
    ));
    assert_same(&got, &want, 1);
}

/// **A text's bounding box is its glyphs' cells**: each advance wide, and
/// from the face's ascent to its descent high, as a clip path in
/// bounding-box units shows by cutting it in half each way.
#[test]
fn the_bounding_box_is_the_cells() {
    let font = FontRef::new(ATKINSON).expect("a font");
    let metrics = Metrics::new(&font, Size::new(16.0), LocationRef::default());
    let (ascent, descent) = (f64::from(metrics.ascent), f64::from(metrics.descent));
    let width = 2.0 * outline(ATKINSON, 'H').width(16.0);
    let text = r#"x="2" y="20" font-family="Atkinson" font-size="16">HH</text>"#;
    let whole = draw(&format!("<text {text}"));
    let left = draw(&format!(
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.5" height="1"/></clipPath><text clip-path="url(#c)" {text}"#
    ));
    let top = draw(&format!(
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="1" height="0.5"/></clipPath><text clip-path="url(#c)" {text}"#
    ));
    let middle_x = 2.0 + width / 2.0;
    let middle_y = 20.0 - ascent + (ascent - descent) / 2.0;
    let mut cut = (0, 0);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let (fx, fy) = (f64::from(x), f64::from(y));
            for (image, before, after, count) in [
                (&left, fx + 1.0 <= middle_x, fx >= middle_x, &mut cut.0),
                (&top, fy + 1.0 <= middle_y, fy >= middle_y, &mut cut.1),
            ] {
                if before {
                    assert_eq!(pixel(image, x, y), pixel(&whole, x, y), "({x}, {y}) kept");
                } else if after {
                    assert_eq!(alpha(image, x, y), 0, "({x}, {y}) cut");
                    *count += usize::from(alpha(&whole, x, y) > 0);
                }
            }
        }
    }
    assert!(cut.0 > 0 && cut.1 > 0, "each clip cut some ink: {cut:?}");
}

/// **Text that draws no character needs no font**: empty, white space,
/// empty spans. **Text that would draw one with none to draw it in is
/// refused**, naming the families tried.
#[test]
fn text_without_fonts_is_refused() {
    for body in [
        r#"<text x="1" y="1"/>"#,
        "<text>  \n  </text>",
        r#"<text x="1" y="2"><tspan x="1" y="3"/></text>"#,
    ] {
        let image = draw_with(&svg(8, body), 8, &NOTHING).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(inked(&image), 0, "{body}");
    }
    let error = draw_refused(
        r#"<text font-family="'Noto Sans', sans-serif">Hi</text>"#,
        &NOTHING,
    );
    assert!(
        matches!(&error, ImageError::Unsupported(why)
            if why.contains("none of which is installed") && why.contains(r#""Noto Sans", sans-serif, serif"#)),
        "{error:?}"
    );
}

/// **What text does that this renderer does not lay out refuses the
/// file**, and an undrawn text property on a shape refuses nothing.
#[test]
fn undrawn_text_features_refuse_the_file() {
    for body in [
        r##"<path id="p" d="M0 0 H8"/><text><textPath href="#p">a</textPath></text>"##,
        r#"<text>a<tref href="x"/></text>"#,
        r#"<text>a<g/></text>"#,
        r#"<text rotate="30">a</text>"#,
        r#"<text>a<tspan rotate="30">b</tspan></text>"#,
        r#"<text textLength="20">a</text>"#,
        r#"<text>a<tspan opacity="0.5">b</tspan></text>"#,
        r#"<text>a<tspan filter="url(#f)">b</tspan></text>"#,
        r#"<text writing-mode="tb-rl">a</text>"#,
        r#"<g style="writing-mode: vertical-rl"><text>a</text></g>"#,
        r#"<text>a<tspan text-decoration="underline">b</tspan></text>"#,
        r#"<text font-variant="small-caps">a</text>"#,
        r#"<text baseline-shift="super">a</text>"#,
        r#"<text>a<tspan x="1em">b</tspan></text>"#,
        r#"<text dy="1ch">a</text>"#,
        r#"<text style="font: 12px serif">a</text>"#,
        r#"<text font-size="2rem">a</text>"#,
        r#"<clipPath id="c"><text>a</text></clipPath><rect width="2" height="2" clip-path="url(#c)"/>"#,
    ] {
        assert!(
            matches!(draw_refused(body, &FIXTURES), ImageError::Unsupported(_)),
            "{body}"
        );
    }
    for body in [
        r#"<rect width="2" height="2" writing-mode="tb-rl" font-variant="small-caps"/>"#,
        r#"<g writing-mode="tb-rl"><text writing-mode="lr-tb" baseline-shift="0">a</text></g>"#,
        r#"<text>a<tspan transform="scale(2)" lengthAdjust="spacing">b</tspan></text>"#,
    ] {
        draw(body);
    }
}

/// **The characters drawn are capped**, `use` instances counted, at
/// exactly [`MAX_CHARACTERS`].
#[test]
fn characters_are_capped() {
    let line = "H".repeat(100);
    let uses: String = (0..MAX_CHARACTERS / 100)
        .map(|_| r##"<use href="#t"/>"##)
        .collect();
    let body = format!(
        r#"<defs><text id="t" font-family="Atkinson" font-size="1">{line}</text></defs>{uses}"#
    );
    assert_eq!(MAX_CHARACTERS % 100, 0);
    draw(&body);
    let error = draw_refused(
        &format!(r#"{body}<text font-family="Atkinson">H</text>"#),
        &FIXTURES,
    );
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("characters")),
        "{error:?}"
    );
}

/// **Font lookups are capped** at exactly [`MAX_LOOKUPS`], every family
/// of a list and the `serif` after it counted once.
#[test]
fn font_lookups_are_capped() {
    let list = |count: usize| -> String {
        (0..count)
            .map(|family| format!("Missing{family}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // Every missing family, then `serif`, which draws.
    draw(&format!(
        r#"<text font-family="{}">H</text>"#,
        list(MAX_LOOKUPS - 1)
    ));
    let error = draw_refused(
        &format!(r#"<text font-family="{}">H</text>"#, list(MAX_LOOKUPS)),
        &FIXTURES,
    );
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("looking up")),
        "{error:?}"
    );
}

/// Copies of Atkinson Hyperlegible, each a face of its own file: `F{n}`
/// is the `n`th copy, `serif` the first, and every one falls back on all
/// of them.
struct Copies {
    files: Vec<PathBuf>,
}

impl FontSource for Copies {
    fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        let index = match query.family.as_str() {
            "serif" => 0,
            family => family.strip_prefix('F')?.parse().ok()?,
        };
        Some(face(self.files.get(index)?.clone(), &query.family))
    }

    fn fallbacks(&self, _: &FontQuery) -> Vec<FontMatch> {
        self.files
            .iter()
            .map(|file| face(file.clone(), "copy"))
            .collect()
    }
}

/// **The faces read are capped** at exactly [`MAX_FACES`], and a fallback
/// search that reaches the cap draws `.notdef` rather than refusing.
#[test]
fn faces_read_are_capped() {
    let dir = std::env::temp_dir().join(format!("crcbl-image-svg-faces-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    let files: Vec<PathBuf> = (0..=MAX_FACES + 4)
        .map(|index| {
            let file = dir.join(format!("copy{index}.ttf"));
            std::fs::write(&file, ATKINSON).expect("writes");
            file
        })
        .collect();
    let copies: &'static Copies = Box::leak(Box::new(Copies {
        files: files.clone(),
    }));
    let texts = |count: usize| -> String {
        (0..count)
            .map(|index| format!(r#"<text font-family="F{index}">H</text>"#))
            .collect()
    };
    let result = (|| {
        draw_with(&svg(8, &texts(MAX_FACES)), 8, copies)?;
        // A character none of them has searches every fallback, and stops
        // at the cap.
        draw_with(
            &svg(8, "<text font-family=\"F0\">&#x2764;</text>"),
            8,
            copies,
        )?;
        Ok::<_, ImageError>(draw_with(&svg(8, &texts(MAX_FACES + 1)), 8, copies))
    })();
    for file in &files {
        std::fs::remove_file(file).expect("removes");
    }
    std::fs::remove_dir(&dir).expect("removes");
    let error = result
        .unwrap_or_else(|error| panic!("{error}"))
        .expect_err("past the cap");
    assert!(
        matches!(&error, ImageError::Unsupported(why) if why.contains("faces")),
        "{error:?}"
    );
}
