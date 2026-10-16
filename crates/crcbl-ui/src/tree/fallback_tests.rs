//! Fallback faces in a tree, from [`Ui::set_fallback_fonts`]: a span and a
//! paragraph measured, drawn and hit in the face each char falls back to.

use std::sync::OnceLock;

use glam::Vec2;

use super::tests::{frame, idle};
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::font::fixtures::cbdt;
use crate::font::{Font, FontMetrics};
use crate::style::Declaration;

const SIZE: f32 = 20.0;

/// Half an em at [`SIZE`]: every Latin-1 char of [`primary`].
const NARROW: f32 = SIZE / 2.0;

/// A whole em at [`SIZE`]: every glyph of the emoji fixtures, which are
/// sixteen units wide on a sixteen-unit em.
const WIDE: f32 = SIZE;

/// A face mapping Latin-1 and nothing else, half an em a glyph, unkerned:
/// one face for every test, so two calls are one font.
fn primary() -> &'static Font {
    static PRIMARY: OnceLock<&'static Font> = OnceLock::new();
    PRIMARY.get_or_init(|| {
        Font::fixed_pitch(
            FontMetrics {
                units_per_em: 1000,
                ascent: 800.0,
                descent: -200.0,
                line_gap: 0.0,
            },
            500.0,
        )
    })
}

/// A tree drawing `.t` in [`primary`] at [`SIZE`], falling back to
/// [`cbdt`].
fn tree() -> Ui {
    let mut ui = Ui::new();
    ui.register_font("primary", primary())
        .expect("not reserved");
    ui.set_fallback_fonts([cbdt()]);
    ui.add_stylesheet(
        "fallback.css",
        &format!(".t {{ font-family: primary; font-size: {SIZE}px; }}"),
    );
    ui
}

/// Each glyph run `list` draws: its font, its text, and each glyph's pen
/// from the origin.
fn runs(list: &DrawList) -> Vec<(Font, String, Vec<Vec2>)> {
    list.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Glyphs {
                origin,
                font,
                glyphs,
                text,
                ..
            } => Some((
                font.clone(),
                text.as_deref().unwrap_or_default().to_owned(),
                glyphs.iter().map(|glyph| *origin + glyph.offset).collect(),
            )),
            _ => None,
        })
        .collect()
}

/// **A span's emoji is measured and drawn in the fallback face**: the box
/// is as wide as the advances the draw places, each face's glyphs are a run
/// of their own in that face, and the runs spell the span between them. The
/// presentation selector draws nothing and takes no width.
#[test]
fn a_span_measures_and_draws_each_char_in_its_fallback_face() {
    let mut ui = tree();
    let text = "a\u{1F600}\u{FE0F} b";
    let mut key = None;
    frame(&mut ui, idle(), |ui| {
        key = Some(ui.span(".t", text, &[]).key);
    });
    let (min, max) = ui.rect(key.expect("built")).expect("laid out");
    assert_eq!(max.x - min.x, NARROW + WIDE + NARROW + NARROW);

    let mut list = DrawList::new();
    ui.emit(&mut list);
    let drawn = runs(&list);
    let fonts: Vec<&Font> = drawn.iter().map(|(font, _, _)| font).collect();
    assert_eq!(fonts, [primary(), &cbdt(), primary()]);
    let texts: Vec<&str> = drawn.iter().map(|(_, text, _)| text.as_str()).collect();
    assert_eq!(texts, ["a", "\u{1F600}\u{FE0F} ", "b"]);
    assert_eq!(texts.concat(), text);
    let pens: Vec<f32> = drawn
        .iter()
        .flat_map(|(_, _, pens)| pens.iter().map(|pen| pen.x - min.x))
        .collect();
    assert_eq!(pens, [0.0, NARROW, NARROW + WIDE + NARROW]);
    // The emoji's glyph is the fallback face's own.
    let smile = cbdt().glyph_id('\u{1F600}');
    let DrawCommand::Glyphs { glyphs, .. } = &list.commands()[1] else {
        panic!("the emoji is the second command");
    };
    assert_eq!(glyphs[0].glyph, smile);
}

/// **A paragraph's runs fall back the same way**, and a run's fragment —
/// what it is hit on — is as wide as its glyphs, the emoji's included.
#[test]
fn a_paragraph_falls_back_and_hits_on_the_fallback_advance() {
    let mut ui = tree();
    let mut keys = Vec::new();
    frame(&mut ui, idle(), |ui| {
        ui.paragraph(".t", &[], |p| {
            keys.push(p.run("", "x\u{1F600}", &[]).key);
            keys.push(p.run("", "y", &[]).key);
        });
    });
    let (min, max) = ui.rect(keys[0]).expect("laid out");
    assert_eq!(max.x - min.x, NARROW + WIDE);
    let (next, _) = ui.rect(keys[1]).expect("laid out");
    assert_eq!(next.x - min.x, NARROW + WIDE);

    let mut list = DrawList::new();
    ui.emit(&mut list);
    let drawn = runs(&list);
    let summary: Vec<(bool, &str)> = drawn
        .iter()
        .map(|(font, text, _)| (font == primary(), text.as_str()))
        .collect();
    assert_eq!(summary, [(true, "x"), (false, "\u{1F600}"), (true, "y")]);
}

/// **A new list measures every span again**, and an empty one turns
/// fallback off: the emoji is then the primary face's `.notdef`, which this
/// face draws zero wide.
#[test]
fn changing_the_fallback_list_measures_again() {
    let mut ui = tree();
    let width = |ui: &mut Ui| {
        let mut key = None;
        frame(ui, idle(), |ui| {
            key = Some(
                ui.span(".t", "a\u{1F600}", &[Declaration::FontSize(SIZE)])
                    .key,
            );
        });
        let (min, max) = ui.rect(key.expect("built")).expect("laid out");
        max.x - min.x
    };
    assert_eq!(width(&mut ui), NARROW + WIDE);
    ui.set_fallback_fonts(Vec::<Font>::new());
    assert!(ui.fallback_fonts().is_empty());
    assert_eq!(width(&mut ui), NARROW);
    ui.set_fallback_fonts([cbdt()]);
    assert_eq!(width(&mut ui), NARROW + WIDE);
}

/// **A notification body's emoji draws in colour**: emitted in the fallback
/// face, then resolved into the image atlas as a quad on the span's
/// baseline, an em wide, after the run it left.
#[test]
fn an_emoji_in_a_span_resolves_to_a_colour_quad() {
    use crate::font::color::ColorGlyphCache;
    use crate::image::ImageAtlas;

    let mut ui = tree();
    let mut key = None;
    frame(&mut ui, idle(), |ui| {
        key = Some(ui.span(".t", "ok \u{1F600}", &[]).key);
    });
    let (min, _) = ui.rect(key.expect("built")).expect("laid out");
    let mut list = DrawList::new();
    ui.emit(&mut list);
    let (mut cache, mut images) = (ColorGlyphCache::new(), ImageAtlas::new());
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 1.0);
    let quads: Vec<(Vec2, Vec2)> = list
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Image { min, max, .. } => Some((*min, *max)),
            _ => None,
        })
        .collect();
    let baseline = runs(&list)[0].2[0].y;
    assert_eq!(quads.len(), 1);
    let (quad_min, quad_max) = quads[0];
    // The fixture's 16-pixel strike scaled to the span's size, its bearing
    // three quarters of an em above the baseline.
    assert_eq!(quad_min.x, (min.x + 3.0 * NARROW).round());
    assert_eq!(quad_min.y, baseline.round() - (WIDE * 0.75).round());
    assert_eq!(quad_max - quad_min, Vec2::splat(WIDE));
}
