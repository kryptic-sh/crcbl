//! Shaping: text to positioned glyphs, through `harfrust`.
//!
//! [`shape`] cuts a stretch of text into [`cluster`]s, asks
//! [`fallback::cluster_face`] which face draws each, and shapes every
//! maximal run of clusters one face draws with `harfrust` — HarfBuzz's Rust
//! port — in that face. GSUB and GPOS apply as the face writes them: kerning,
//! mark positioning, ligatures, contextual forms, complex scripts' reordering,
//! emoji ZWJ sequences and flags. What comes back is one [`ShapedGlyph`] per
//! glyph, in pixels at the size asked for, each naming the face that draws
//! it and the cluster of the text it came from.
//!
//! # What a run is shaped with
//!
//! * **In the direction the caller asks for**: left to right, or right to
//!   left for a run of an odd bidi level (`font::bidi`), which is
//!   shaped as such — mirrored brackets, Arabic joining — and comes back in
//!   logical order all the same, the glyphs `harfrust` puts in visual order
//!   reversed, so a caller places it by walking its glyphs backwards from the
//!   run's left edge. The script is `harfrust`'s guess from the run's chars
//!   (`Buffer::guess_segment_properties`), and the language is unset.
//! * **The text around the run is its context**, so a contextual lookup sees
//!   past the run's edges; nothing outside the run is shaped.
//! * **Default-ignorable chars are removed** — the zero-width joiner and the
//!   variation selectors among them — once GSUB has seen them, so they draw
//!   nothing and take no width.
//! * **Clusters are monotone graphemes** (`harfrust`'s default), so a glyph's
//!   cluster never comes before the one of the glyph before it, in either
//!   direction.
//! * **A synthesised bold** ([`face::Synthesis`](super::face::Synthesis))
//!   widens each glyph that advances at all by
//!   [`face::SYNTHETIC_BOLD_EM`](super::face::SYNTHETIC_BOLD_EM); a mark with
//!   no advance stays where the font put it.

use core::ops::Range;

use glam::Vec2;

#[cfg(feature = "parsed-font")]
use std::sync::{Arc, Mutex, PoisonError};

use super::{Font, GlyphId, cluster, fallback};

/// One glyph as [`shape`] placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Which face draws it, as [`Font::face`] numbers them.
    pub face: u8,
    /// The glyph, in that face.
    pub glyph: GlyphId,
    /// The byte offset in the text of the first char of its cluster.
    pub cluster: usize,
    /// How far the pen moves after it, in pixels.
    pub advance: f32,
    /// Where it is drawn from the pen, in pixels: x right and y **down**, as
    /// the screen runs.
    pub offset: Vec2,
    /// Whether cutting the text where its cluster starts, and shaping the two
    /// sides apart, could shape either side differently — `harfrust`'s
    /// unsafe-to-break flag: a kerning pair or a ligature reaching across.
    /// Never set where the face drawing it changes, since each face's run is
    /// shaped apart already.
    pub unsafe_to_break: bool,
}

/// Appends to `out` the glyphs `range` of `text` shapes into in `font` — and
/// the fallback faces it carries — at `size` pixels per em, right to left
/// when `rtl` says so, in logical order. The text outside `range` is context
/// only; every glyph's
/// [`cluster`](ShapedGlyph::cluster) is a byte offset in `text`. See the
/// module docs.
///
/// # Panics
///
/// If `range` is not a char-boundary range of `text`, or `text` is longer
/// than `u32::MAX` bytes, which `harfrust` numbers clusters in.
pub fn shape(
    font: &Font,
    size: f32,
    text: &str,
    range: Range<usize>,
    rtl: bool,
    out: &mut Vec<ShapedGlyph>,
) {
    let mut run: Option<(u8, Range<usize>)> = None;
    for cluster in cluster::clusters(&text[range.clone()]) {
        let bytes = range.start + cluster.start..range.start + cluster.end;
        let face = fallback::cluster_face(font, &text[bytes.clone()]);
        match &mut run {
            Some((current, run)) if *current == face => run.end = bytes.end,
            _ => {
                if let Some((face, run)) = run.take() {
                    shape_run(font, face, size, text, run, rtl, out);
                }
                run = Some((face, bytes));
            }
        }
    }
    if let Some((face, run)) = run {
        shape_run(font, face, size, text, run, rtl, out);
    }
}

/// Without the `parsed-font` feature there is no face to shape in.
#[cfg(not(feature = "parsed-font"))]
fn shape_run(
    font: &Font,
    face: u8,
    _size: f32,
    _text: &str,
    _range: Range<usize>,
    _rtl: bool,
    _out: &mut Vec<ShapedGlyph>,
) {
    match font.face(face).source.never {}
}

/// Appends `range` of `text` shaped in `font`'s face `face` at `size`, right
/// to left when `rtl` says so, in logical order.
#[cfg(feature = "parsed-font")]
fn shape_run(
    font: &Font,
    face: u8,
    size: f32,
    text: &str,
    range: Range<usize>,
    rtl: bool,
    out: &mut Vec<ShapedGlyph>,
) {
    let drawn = font.face(face);
    let metrics = drawn.metrics();
    let scale = metrics.scale(size);
    let bold = if drawn.synthesis().bold {
        f32::from(metrics.units_per_em) * super::face::SYNTHETIC_BOLD_EM
    } else {
        0.0
    };
    let Some(shaper) = &drawn.parsed().shaper else {
        nominal(drawn, face, scale, text, range, out);
        return;
    };
    #[cfg(test)]
    shaper
        .runs
        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let buffer = run_shaper(shaper, text, range, rtl);
    let first = out.len();
    for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
        let mut advance = position.x_advance as f32;
        if position.x_advance != 0 {
            advance += bold;
        }
        out.push(ShapedGlyph {
            face,
            glyph: GlyphId(info.glyph_id),
            cluster: info.cluster as usize,
            advance: advance * scale,
            offset: Vec2::new(
                position.x_offset as f32 * scale,
                -(position.y_offset as f32) * scale,
            ),
            unsafe_to_break: info.unsafe_to_break(),
        });
    }
    if rtl {
        out[first..].reverse();
    }
    if let Some(glyph) = out.get_mut(first) {
        glyph.unsafe_to_break = false;
    }
}

/// `range` of `text` shaped by `shaper`, right to left when `rtl` says so,
/// as the module docs say a run is: its glyphs in visual order.
#[cfg(feature = "parsed-font")]
fn run_shaper(shaper: &Shaper, text: &str, range: Range<usize>, rtl: bool) -> harfrust::Buffer {
    let mut buffer = harfrust::Buffer::new();
    for (at, c) in text[range.clone()].char_indices() {
        let cluster = u32::try_from(range.start + at)
            .expect("a text no longer than `u32::MAX` bytes, as `shape` documents");
        buffer.push(u32::from(c), cluster);
    }
    buffer.set_pre_context(&text[..range.start]);
    buffer.set_post_context(&text[range.end..]);
    buffer.set_direction(if rtl {
        harfrust::Direction::RightToLeft
    } else {
        harfrust::Direction::LeftToRight
    });
    buffer.guess_segment_properties();
    buffer.set_flags(harfrust::BufferFlags::REMOVE_DEFAULT_IGNORABLES);
    let plan = shaper.plan(&buffer);
    harfrust::shape(
        &harfrust::ShaperFont::new(&shaper.font),
        &mut buffer,
        harfrust::ShapeOptions::new().plan(Some(&plan)),
    )
    .expect("a buffer with its direction set, and a plan made for its direction and script");
    buffer
}

/// How many shape plans one face keeps: more than the scripts a face is
/// handed in practice — fallback gives a face only the clusters it maps —
/// times the two directions, so a steady UI compiles each plan once.
#[cfg(feature = "parsed-font")]
const PLANS_KEPT: usize = 16;

/// A face as `harfrust` shapes with it: the font, its layout tables read
/// once, and the shape plans compiled for it.
///
/// **A plan is compiled once per face and segment** rather than per run:
/// compiling one — choosing the features and lookups a script, direction and
/// language apply — costs more than shaping a short label with it. A plan
/// depends on the face, the buffer's direction, script and language, the user
/// features (none: every run is shaped with the face's defaults) and the
/// feature variations the face's variation coordinates select (none: a face is
/// read at its default instance). `harfrust::ShapePlanKey` is matched on all
/// of those but the face, and the face is covered by keeping one cache per
/// face, so a kept plan is the plan `harfrust::shape` would compile.
/// The [`PLANS_KEPT`] most recently used plans are kept, the least recently
/// used one dropped past that.
#[cfg(feature = "parsed-font")]
pub(super) struct Shaper {
    font: harfrust::Font,
    /// The plans kept, the most recently used first. Behind a lock, as a
    /// [`Font`] is shared between threads; a plan is compiled under it, so two
    /// threads never compile the same one.
    plans: Mutex<Vec<Arc<harfrust::ShapePlan>>>,
    /// How many plans have been compiled, for the tests to count.
    #[cfg(test)]
    compiled: core::sync::atomic::AtomicUsize,
    /// How many runs have been shaped for a layout, for the tests to count.
    #[cfg(test)]
    runs: core::sync::atomic::AtomicUsize,
}

#[cfg(feature = "parsed-font")]
impl Shaper {
    /// Shapes with `font`, no plan compiled yet.
    pub(super) fn new(font: harfrust::Font) -> Self {
        Self {
            font,
            plans: Mutex::new(Vec::new()),
            #[cfg(test)]
            compiled: core::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            runs: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The plan `buffer` — its direction, script and language set — is
    /// shaped with in this face: a kept one, else one compiled and kept.
    fn plan(&self, buffer: &harfrust::Buffer) -> Arc<harfrust::ShapePlan> {
        let (direction, script, language) =
            (buffer.direction(), buffer.script(), buffer.language());
        let key = harfrust::ShapePlanKey::new(&self.font, script, direction).language(language);
        let mut plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = plans.iter().position(|plan| key.matches(plan)) {
            plans[..=at].rotate_right(1);
            return Arc::clone(&plans[0]);
        }
        let plan = Arc::new(harfrust::ShapePlan::new(
            &self.font,
            direction,
            script,
            language,
            &[],
        ));
        #[cfg(test)]
        self.compiled
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        plans.truncate(PLANS_KEPT - 1);
        plans.insert(0, Arc::clone(&plan));
        plan
    }
}

/// Whether `face` draws any of `cluster` as a colour glyph once the cluster
/// is shaped in it alone — so a keycap or a flag whose parts are plain
/// outlines, and which only a GSUB ligature colours, counts as colour.
#[cfg(feature = "parsed-font")]
pub(super) fn shapes_to_colour(face: &Font, cluster: &str) -> bool {
    match &face.parsed().shaper {
        Some(shaper) => run_shaper(shaper, cluster, 0..cluster.len(), false)
            .glyph_infos()
            .iter()
            .any(|info| face.has_color_glyph(GlyphId(info.glyph_id))),
        None => cluster
            .chars()
            .any(|c| face.has_color_glyph(face.glyph_id(c))),
    }
}

/// Without the `parsed-font` feature there is no face to shape in.
#[cfg(not(feature = "parsed-font"))]
pub(super) fn shapes_to_colour(face: &Font, _cluster: &str) -> bool {
    match face.source.never {}
}

/// `range` of `text` in `face` with no tables to shape with: each char that
/// draws through the cmap, advanced by its advance. Only a test's
/// fixed-pitch face, which has no bytes, takes this path.
#[cfg(feature = "parsed-font")]
fn nominal(
    face: &Font,
    index: u8,
    scale: f32,
    text: &str,
    range: Range<usize>,
    out: &mut Vec<ShapedGlyph>,
) {
    for (at, c) in text[range.clone()].char_indices() {
        if fallback::is_invisible(c) {
            continue;
        }
        let glyph = face.glyph_id(c);
        out.push(ShapedGlyph {
            face: index,
            glyph,
            cluster: range.start + at,
            advance: face.advance(glyph) * scale,
            offset: Vec2::ZERO,
            unsafe_to_break: false,
        });
    }
}

/// How many runs `font`'s own face has shaped, fallback probes aside: what a
/// test counts shaping by. A face of its own parse counts only its own runs.
#[cfg(all(test, feature = "parsed-font"))]
pub(crate) fn shaped_runs(font: &Font) -> usize {
    font.parsed()
        .shaper
        .as_ref()
        .expect("a parsed face")
        .runs
        .load(core::sync::atomic::Ordering::Relaxed)
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;

    const SIZE: f32 = 16.0;

    /// `text` shaped whole in `font`.
    fn shaped(font: &Font, text: &str) -> Vec<ShapedGlyph> {
        let mut out = Vec::new();
        shape(font, SIZE, text, 0..text.len(), false, &mut out);
        out
    }

    /// `text` shaped straight through `harfrust` in the committed font, as
    /// `(glyph, cluster, x advance, x offset, y offset)` in font units.
    fn reference(text: &str) -> Vec<(u32, u32, i32, i32, i32)> {
        let font = harfrust::Font::new(crate::font::SANS_TTF, 0).expect("reads");
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(text);
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
            .map(|(info, at)| {
                (
                    info.glyph_id,
                    info.cluster,
                    at.x_advance,
                    at.x_offset,
                    at.y_offset,
                )
            })
            .collect()
    }

    /// **Advances, offsets and clusters are `harfrust`'s, scaled**: a
    /// kerned pair, a mark the font positions over a base with no
    /// precomposed form, and a plain word, glyph for glyph.
    #[test]
    fn advances_and_offsets_are_harfrusts_scaled_to_the_size() {
        let font = Font::sans();
        let scale = font.metrics().scale(SIZE);
        for text in ["AVATAR", "h\u{301}x", "The quick"] {
            let got = shaped(font, text);
            let want = reference(text);
            assert_eq!(got.len(), want.len(), "{text:?}");
            for (got, (glyph, cluster, advance, x, y)) in got.iter().zip(want) {
                assert_eq!(got.face, 0);
                assert_eq!(got.glyph, GlyphId(glyph), "{text:?}");
                assert_eq!(got.cluster, cluster as usize, "{text:?}");
                assert_eq!(got.advance, advance as f32 * scale, "{text:?}");
                assert_eq!(got.offset, Vec2::new(x as f32, -y as f32) * scale);
            }
        }
        // The pair kerns, and the mark sits on its base with no advance.
        let pair = shaped(font, "AV");
        let a = font.glyph_id('A');
        assert!(
            pair[0].advance < font.advance(a) * scale,
            "AV is not kerned"
        );
        let mark = shaped(font, "h\u{301}");
        assert_eq!(mark.len(), 2);
        assert_eq!(mark[1].cluster, 0, "the mark is in its base's cluster");
        assert_eq!(mark[1].advance, 0.0);
        assert_ne!(mark[1].offset, Vec2::ZERO, "the mark is not positioned");
    }

    /// **A synthesised bold widens each glyph that advances**, by its share
    /// of an em at the size, and leaves a mark's zero advance alone.
    #[test]
    fn a_synthesised_bold_widens_what_advances() {
        let font = Font::sans();
        let bold = font.synthesized(crate::font::face::Synthesis {
            bold: true,
            italic: false,
        });
        let extra = SIZE * crate::font::face::SYNTHETIC_BOLD_EM;
        let (plain, wide) = (shaped(font, "xh\u{301}"), shaped(&bold, "xh\u{301}"));
        for (plain, wide) in plain.iter().zip(&wide).take(2) {
            assert!((wide.advance - plain.advance - extra).abs() < 1e-4);
        }
        assert_eq!(wide[2].advance, 0.0, "the mark was widened");
    }

    /// **The committed font's GSUB applies**: its `ccmp` swaps `i` for a
    /// dotless `ı` under a mark above, which a cmap lookup never would.
    #[test]
    fn the_committed_fonts_gsub_applies() {
        let font = Font::sans();
        let glyphs = shaped(font, "i\u{307}");
        assert_eq!(glyphs[0].glyph, font.glyph_id('\u{131}'));
        assert_ne!(glyphs[0].glyph, font.glyph_id('i'));
    }

    /// **A ZWJ sequence and a flag become one ligature each**, in the face
    /// that maps all of them, and each ligature is a colour glyph, while its
    /// components are not.
    #[test]
    fn sequences_and_flags_ligate_into_colour_glyphs() {
        let face = fixtures::shaped();
        let font = Font::sans().with_fallback(&[fixtures::cbdt(), face.clone()]);
        let text = "a\u{2764}\u{200D}\u{1F525}\u{1F1FA}\u{1F1F8}b";
        let glyphs = shaped(&font, text);
        let faces: Vec<u8> = glyphs.iter().map(|glyph| glyph.face).collect();
        assert_eq!(faces, [0, 2, 2, 0]);
        let clusters: Vec<usize> = glyphs.iter().map(|glyph| glyph.cluster).collect();
        assert_eq!(clusters, [0, 1, 11, 19]);
        for ligature in &glyphs[1..3] {
            assert!(face.has_color_glyph(ligature.glyph));
            assert_eq!(ligature.advance, SIZE, "an em wide, as every glyph is");
        }
        // A selector draws nothing, not even an empty glyph, in a face
        // that has a space glyph to hide it as.
        let hearts = shaped(&face, "\u{2764}\u{FE0F}\u{2764}");
        assert_eq!(hearts.len(), 2, "the selector left a glyph");
        for component in ['\u{2764}', '\u{1F525}', '\u{1F1FA}', '\u{1F1F8}'] {
            assert!(!face.has_color_glyph(face.glyph_id(component)));
        }
    }

    /// **A right-to-left run is `harfrust`'s right-to-left shaping, back in
    /// logical order**: its glyphs reversed are the glyphs, clusters and
    /// advances `harfrust` gives the same text shaped right to left, which
    /// differ from the left-to-right ones as kerning is looked up the other
    /// way round.
    #[test]
    fn a_right_to_left_run_is_harfrusts_reversed() {
        let font = Font::sans();
        let scale = font.metrics().scale(SIZE);
        let text = "AVAT";
        let mut got = Vec::new();
        shape(font, SIZE, text, 0..text.len(), true, &mut got);
        let shaper = harfrust::Font::new(crate::font::SANS_TTF, 0).expect("reads");
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(text);
        buffer.set_direction(harfrust::Direction::RightToLeft);
        buffer.guess_segment_properties();
        harfrust::shape(
            &harfrust::ShaperFont::new(&shaper),
            &mut buffer,
            harfrust::ShapeOptions::new(),
        )
        .expect("shapes");
        let want: Vec<(GlyphId, usize, f32)> = buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .rev()
            .map(|(info, at)| {
                (
                    GlyphId(info.glyph_id),
                    info.cluster as usize,
                    at.x_advance as f32 * scale,
                )
            })
            .collect();
        let got: Vec<(GlyphId, usize, f32)> = got
            .iter()
            .map(|glyph| (glyph.glyph, glyph.cluster, glyph.advance))
            .collect();
        assert_eq!(got, want);
        let clusters: Vec<usize> = got.iter().map(|glyph| glyph.1).collect();
        assert_eq!(clusters, [0, 1, 2, 3], "not in logical order");
        assert_ne!(
            got,
            {
                let mut ltr = Vec::new();
                shape(font, SIZE, text, 0..text.len(), false, &mut ltr);
                ltr.iter()
                    .map(|glyph| (glyph.glyph, glyph.cluster, glyph.advance))
                    .collect::<Vec<_>>()
            },
            "the direction changed nothing, so the test cannot tell it was set"
        );
    }

    /// How many shape plans `font`'s own face has compiled.
    fn compiled(font: &Font) -> usize {
        font.parsed()
            .shaper
            .as_ref()
            .expect("a parsed face")
            .compiled
            .load(core::sync::atomic::Ordering::Relaxed)
    }

    /// **A plan is compiled once per face, direction and script**: shaping
    /// the same label again, or another label of the same script, compiles
    /// none, while a new direction or script compiles its own — and the
    /// glyphs are the same as the first shaping's.
    #[test]
    fn a_face_compiles_one_plan_per_direction_and_script() {
        // A parse of its own, so no other test's shaping is counted.
        let font = Font::parse(crate::font::SANS_TTF).expect("parses");
        let first = shaped(&font, "Iron Sword");
        assert_eq!(compiled(&font), 1);
        assert_eq!(
            shaped(&font, "Iron Sword"),
            first,
            "a kept plan shaped it differently"
        );
        shaped(&font, "Health Potion");
        assert_eq!(compiled(&font), 1, "a second Latin label compiled a plan");
        let mut rtl = Vec::new();
        shape(&font, SIZE, "AVAT", 0..4, true, &mut rtl);
        assert_eq!(
            compiled(&font),
            2,
            "right to left shared left to right's plan"
        );
        shaped(&font, "\u{3b1}\u{3b2}");
        assert_eq!(compiled(&font), 3, "Greek shared Latin's plan");
        shaped(&font, "Iron Sword");
        shape(&font, SIZE, "VA", 0..2, true, &mut rtl);
        assert_eq!(compiled(&font), 3, "a kept plan was compiled again");
    }

    /// **Past [`PLANS_KEPT`] plans the least recently used goes**, not the
    /// oldest compiled: with the first script's plan used again, one script
    /// more than are kept drops the second's, and the first's stays.
    #[test]
    fn the_least_recently_used_plan_goes_past_the_bound() {
        let font = Font::parse(crate::font::SANS_TTF).expect("parses");
        // A letter of each of seventeen scripts.
        let scripts = [
            "a", "\u{3b1}", "\u{431}", "\u{561}", "\u{5d0}", "\u{628}", "\u{915}", "\u{995}",
            "\u{a15}", "\u{a95}", "\u{b15}", "\u{b95}", "\u{c15}", "\u{c95}", "\u{d15}", "\u{e01}",
            "\u{10d0}",
        ];
        assert_eq!(scripts.len(), PLANS_KEPT + 1);
        let (last, kept) = scripts.split_last().expect("not empty");
        for letter in kept {
            shaped(&font, letter);
        }
        assert_eq!(compiled(&font), PLANS_KEPT, "two scripts shared a plan");
        shaped(&font, scripts[0]);
        shaped(&font, last);
        assert_eq!(compiled(&font), PLANS_KEPT + 1);
        shaped(&font, scripts[0]);
        assert_eq!(
            compiled(&font),
            PLANS_KEPT + 1,
            "the recently used plan was dropped"
        );
        shaped(&font, scripts[1]);
        assert_eq!(
            compiled(&font),
            PLANS_KEPT + 2,
            "the least recently used plan was kept"
        );
    }

    /// **A run is shaped only where asked**: shaping part of a text appends
    /// that part's glyphs, clusters counted in the whole text.
    #[test]
    fn only_the_range_is_shaped_with_clusters_in_the_whole_text() {
        let font = Font::sans();
        let mut out = Vec::new();
        shape(font, SIZE, "ab cd", 3..5, false, &mut out);
        let clusters: Vec<usize> = out.iter().map(|glyph| glyph.cluster).collect();
        assert_eq!(clusters, [3, 4]);
        assert_eq!(out[0].glyph, font.glyph_id('c'));
    }
}
