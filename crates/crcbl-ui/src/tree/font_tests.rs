//! Fonts an application registers with [`Ui::register_font`]: what a
//! `font-family` list selects by name, what it measures and draws in, and what
//! a name nothing is registered under falls back to.

use glam::Vec2;

use super::tests::{frame, idle};
use super::*;
use crate::draw_list::{DrawCommand, DrawList};
use crate::font::{Font, FontMetrics, GlyphId, ReservedFamilyName};
use crate::style::Declaration;

/// A font whose every Latin-1 glyph is `advance` thousandths of an em wide, on
/// lines exactly one em tall.
fn fixed_pitch(advance: f32) -> &'static Font {
    Font::fixed_pitch(
        FontMetrics {
            units_per_em: 1000,
            ascent: 800.0,
            descent: -200.0,
            line_gap: 0.0,
        },
        advance,
    )
}

const TEXT: &str = "WIDE";
const SIZE: f32 = 20.0;

/// One frame of a span `class` holding [`TEXT`] at [`SIZE`] under `css`,
/// returning its size and what it emitted.
fn span_in(ui: &mut Ui, css: &str, class: &str) -> (Vec2, DrawList) {
    ui.add_stylesheet("fonts.css", css);
    let mut key = None;
    frame(ui, idle(), |ui| {
        key = Some(ui.span(class, TEXT, &[Declaration::FontSize(SIZE)]).key);
    });
    let (min, max) = ui.rect(key.expect("built")).expect("laid out");
    let mut list = DrawList::new();
    ui.emit(&mut list);
    (max - min, list)
}

/// The one glyph run `list` holds: its font and glyphs.
fn glyph_run(list: &DrawList) -> (&'static Font, Vec<crate::font::layout::PositionedGlyph>) {
    let runs: Vec<_> = list
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Glyphs { font, glyphs, .. } => Some((*font, glyphs.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1, "not one glyph run: {}", runs.len());
    runs.into_iter().next().expect("one run")
}

/// **A registered font is what a span measures in**, by quoted name or bare
/// words in any case: [`TEXT`]'s width is its advances and its height the
/// font's one-em line — not what `sans-serif`, the list's fallback, measures.
#[test]
fn a_registered_font_measures_the_spans_that_name_it() {
    let font = fixed_pitch(500.0);
    let css = r#"
        .quoted { font-family: "ROBOTO", sans-serif; }
        .bare { font-family: roboto, sans-serif; }
        .alone { font-family: Roboto; }
        .sans { font-family: sans-serif; }
    "#;
    let mut sans_ui = Ui::new();
    let (sans, _) = span_in(&mut sans_ui, css, ".sans");

    // Four glyphs half an em wide at 20px, on a 20px line.
    let want = Vec2::new(4.0 * 0.5 * SIZE, SIZE);
    assert_ne!(sans, want, "the fixture measures like sans-serif");
    for class in [".quoted", ".bare", ".alone"] {
        let mut ui = Ui::new();
        ui.register_font("Roboto", font).expect("not reserved");
        let (size, _) = span_in(&mut ui, css, class);
        assert_eq!(size, want, "{class} did not measure in the registered font");
    }
}

/// **A registered font is what a span draws in**: one glyph run of that font,
/// its glyphs the font's own ids at its advances.
#[test]
fn a_registered_font_is_the_font_its_spans_glyph_run_draws_in() {
    let font = fixed_pitch(500.0);
    let mut ui = Ui::new();
    ui.register_font("roboto", font).expect("not reserved");
    let (_, list) = span_in(&mut ui, ".a { font-family: roboto, bitmap; }", ".a");

    let (drawn, glyphs) = glyph_run(&list);
    assert_eq!(
        drawn.id(),
        font.id(),
        "the run is not in the registered font"
    );
    assert_eq!(glyphs.len(), TEXT.len());
    for (at, glyph) in glyphs.iter().enumerate() {
        assert_eq!(glyph.glyph, GlyphId(1));
        assert_eq!(glyph.offset.x, at as f32 * 0.5 * SIZE);
    }
    assert!(
        !list
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Text { .. })),
        "the span also drew in the bitmap font"
    );
}

/// **A name nothing is registered under falls back as an unknown name always
/// did**: to the list's built-in family, and — for a list naming none — to the
/// family the span had without the declaration, inherited or an earlier
/// rule's.
#[test]
fn an_unregistered_family_draws_in_what_the_list_falls_back_to() {
    let other = fixed_pitch(250.0);
    let reference = |css: &str| span_in(&mut Ui::new(), css, ".a");
    let (sans, _) = reference(".a { font-family: sans-serif; }");
    let (bitmap, _) = reference(".a { font-family: bitmap; }");
    assert_ne!(sans, bitmap);

    let cases = [
        (".a { font-family: roboto, sans-serif; }", sans),
        (".a { font-family: roboto, bitmap; }", bitmap),
        // An earlier rule's family stands.
        (
            ".a { font-family: sans-serif; } .a { font-family: roboto; }",
            sans,
        ),
        // The initial family stands.
        (".a { font-family: roboto; }", bitmap),
    ];
    for (css, want) in cases {
        let mut ui = Ui::new();
        // Registered under another name: this one is still unregistered.
        ui.register_font("inter", other).expect("not reserved");
        let (size, list) = span_in(&mut ui, css, ".a");
        assert_eq!(size, want, "{css}");
        let drew_sans = list
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Glyphs { .. }));
        assert_eq!(drew_sans, want == sans, "{css} drew in the wrong font");
    }

    // Inherited: the parent's family is the child's fallback.
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "fonts.css",
        ".column { font-family: sans-serif; } .a { font-family: roboto; }",
    );
    let mut key = None;
    frame(&mut ui, idle(), |ui| {
        ui.block(".column", &[], |ui| {
            key = Some(ui.span(".a", TEXT, &[Declaration::FontSize(SIZE)]).key);
        });
    });
    let (min, max) = ui.rect(key.expect("built")).expect("laid out");
    assert_eq!(
        max - min,
        sans,
        "an unregistered name dropped the inherited family"
    );
}

/// **Registering a name again replaces its font**, and the spans in it are
/// measured again rather than answered from the cache; a name no list could
/// select by is refused.
#[test]
fn registering_a_name_again_replaces_its_font_and_reserved_names_are_refused() {
    let narrow = fixed_pitch(250.0);
    let wide = fixed_pitch(750.0);
    let css = ".a { font-family: roboto, sans-serif; }";
    let mut ui = Ui::new();
    ui.register_font("roboto", narrow).expect("not reserved");
    let (first, _) = span_in(&mut ui, css, ".a");
    ui.register_font("ROBOTO", wide).expect("not reserved");
    let (second, list) = span_in(&mut ui, css, ".a");
    assert_eq!(first.x, 4.0 * 0.25 * SIZE);
    assert_eq!(
        second.x,
        4.0 * 0.75 * SIZE,
        "the replaced font's width stuck"
    );
    assert_eq!(glyph_run(&list).0.id(), wide.id());

    for name in [
        "",
        "bitmap",
        "Atkinson Hyperlegible",
        "Sans-Serif",
        "serif",
        "inherit",
    ] {
        assert_eq!(
            ui.register_font(name, wide),
            Err(ReservedFamilyName(name.to_owned())),
            "`{name}` was registered"
        );
    }
}
