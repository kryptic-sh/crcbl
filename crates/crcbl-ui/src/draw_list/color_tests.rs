//! [`DrawList::resolve_color_glyphs`]: colour glyphs swapped out of their
//! glyph runs for image quads.

use super::*;
use crate::font::fixtures;
use crate::font::layout::TextLayout;
use crate::image::TexelRect;

const SIZE: f32 = 16.0;
const INK: [f32; 4] = [0.2, 0.4, 0.6, 0.5];

/// `text` laid out in the sans face falling back to the CBDT fixture, one
/// glyph run per face from `origin`, as a tree emits it.
fn emoji_runs(list: &mut DrawList, origin: Vec2, text: &str) -> TextLayout {
    let font = Font::sans().with_fallback(&[fixtures::cbdt()]);
    let layout = TextLayout::new(&font, text, SIZE, 20.0, None);
    for run in layout.face_runs() {
        list.text_glyphs(origin, run.font, SIZE, INK, run.glyphs, &text[run.text]);
    }
    layout
}

/// Each command's kind, and a glyph run's glyph count.
fn kinds(list: &DrawList) -> Vec<String> {
    list.commands()
        .iter()
        .map(|command| match command {
            DrawCommand::Glyphs { glyphs, .. } => format!("glyphs {}", glyphs.len()),
            DrawCommand::Image { .. } => "image".to_owned(),
            _ => "other".to_owned(),
        })
        .collect()
}

/// **A colour glyph leaves its run for an image quad right after it**: on
/// whole pixels at its pen plus its bearing, the strike's own texels in the
/// atlas, and tinted by the run's opacity but not its colour. The emptied
/// run stays with its text; the runs either side keep their glyphs.
#[test]
fn a_colour_glyph_becomes_an_image_quad_after_its_run() {
    let mut list = DrawList::new();
    let origin = Vec2::new(10.25, 30.0);
    let layout = emoji_runs(&mut list, origin, "a\u{1F600}b");
    assert_eq!(kinds(&list), ["glyphs 1", "glyphs 1", "glyphs 1"]);
    let mut images = ImageAtlas::new();
    let mut cache = ColorGlyphCache::new();
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 1.0);

    assert_eq!(kinds(&list), ["glyphs 1", "glyphs 0", "image", "glyphs 1"]);
    let DrawCommand::Glyphs { text, font, .. } = &list.commands()[1] else {
        panic!("the emoji's run");
    };
    assert_eq!(text.as_deref(), Some("\u{1F600}"));
    assert_eq!(font, &fixtures::cbdt());
    let DrawCommand::Image {
        min,
        max,
        uv_min,
        uv_max,
        tint,
    } = list.commands()[2]
    else {
        panic!("the emoji's quad");
    };
    let pen = origin + layout.glyphs()[1].offset;
    assert_eq!(min, pen.round() + Vec2::new(0.0, -12.0));
    assert_eq!(max, min + Vec2::splat(16.0));
    assert_eq!(tint, [1.0, 1.0, 1.0, INK[3]]);
    let image = images.images()[0];
    assert_eq!((uv_min, uv_max), (image.uv_min(), image.uv_max()));
    let texels = images.region(TexelRect {
        x: image.x,
        y: image.y,
        width: image.width,
        height: image.height,
    });
    for (at, texel) in texels.chunks(4).enumerate() {
        let want = if at % 16 < 8 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert_eq!(texel, want, "texel {at}");
    }
}

/// **A quad keeps its run's clip and its side of the overlay cut**, and the
/// cut moves with the commands inserted below it.
#[test]
fn a_quad_keeps_its_runs_clip_and_side_of_the_cut() {
    let mut list = DrawList::new();
    let clip = (Vec2::new(1.0, 2.0), Vec2::new(300.0, 200.0));
    list.push_clip(clip.0, clip.1);
    emoji_runs(&mut list, Vec2::new(0.0, 20.0), "\u{1F600}");
    list.pop_clip().expect("pushed above");
    list.begin_overlay();
    emoji_runs(&mut list, Vec2::new(0.0, 60.0), "\u{1F600}");
    let mut images = ImageAtlas::new();
    let mut cache = ColorGlyphCache::new();
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 1.0);

    assert_eq!(list.base_commands().len(), 2);
    assert_eq!(list.overlay_commands().len(), 2);
    assert!(matches!(list.base_commands()[1], DrawCommand::Image { .. }));
    assert!(matches!(
        list.overlay_commands()[1],
        DrawCommand::Image { .. }
    ));
    let within = ClipRect {
        min: clip.0,
        max: clip.1,
    };
    assert_eq!(list.clips()[..2], [within, within]);
    assert_eq!(list.clips()[2..], [ClipRect::NONE, ClipRect::NONE]);
    // Both quads are the one picture: one glyph at one size is cached once.
    assert_eq!(images.images().len(), 1);
}

/// **At a scale the quad follows the pen and size the expansion uses**: the
/// pen's offset and the size scaled, the origin not, and the strike resized
/// to the scaled size.
#[test]
fn a_quad_is_placed_and_sized_at_the_expansions_scale() {
    let mut list = DrawList::new();
    let origin = Vec2::new(5.0, 40.0);
    let layout = emoji_runs(&mut list, origin, "a\u{1F600}");
    let mut images = ImageAtlas::new();
    let mut cache = ColorGlyphCache::new();
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 2.0);
    let DrawCommand::Image { min, max, .. } = list.commands()[2] else {
        panic!("the emoji's quad");
    };
    let pen = origin + layout.glyphs()[1].offset * 2.0;
    assert_eq!(min, pen.round() + Vec2::new(0.0, -24.0));
    assert_eq!(max - min, Vec2::splat(32.0));
}

/// **A run in a font with no colour tables is left alone**, and a colour
/// glyph whose picture cannot be made stays in its run to draw its outline.
#[test]
fn plain_runs_and_glyphs_with_no_picture_stay_runs() {
    let mut list = DrawList::new();
    emoji_runs(&mut list, Vec2::ZERO, "plain");
    let cbdt = fixtures::cbdt();
    let broken = cbdt.glyph_id('\u{1F4A5}');
    list.glyphs(
        Vec2::ZERO,
        &cbdt,
        SIZE,
        INK,
        vec![PositionedGlyph {
            glyph: broken,
            offset: Vec2::new(0.0, 20.0),
        }],
    );
    let before = kinds(&list);
    let mut images = ImageAtlas::new();
    let mut cache = ColorGlyphCache::new();
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 1.0);
    assert_eq!(kinds(&list), before);
    assert!(images.images().is_empty());
}

/// **Tessellating with colours draws what resolving and then tessellating
/// draws**: one implementation, so a renderer of its own and the UI pass put
/// a colour glyph in the same place, and a list resolved first has nothing
/// left for the tessellation to swap.
#[test]
fn tessellating_with_colours_matches_resolving_first() {
    let mut list = DrawList::new();
    list.push_clip(Vec2::new(2.0, 3.0), Vec2::new(200.0, 100.0));
    emoji_runs(&mut list, Vec2::new(10.25, 30.0), "a\u{1F600}b");
    list.pop_clip().expect("pushed above");
    list.begin_overlay();
    emoji_runs(&mut list, Vec2::new(4.0, 70.0), "\u{1F600}c");
    let tessellate = |list: &DrawList, colors: bool| {
        let (mut cache, mut images) = (ColorGlyphCache::new(), ImageAtlas::new());
        cache.begin_frame(&mut images);
        let mut glyphs = GlyphAtlas::new(256, 1, 100);
        let colors = colors.then_some(ColorGlyphs {
            cache: &mut cache,
            images: &mut images,
        });
        let triangles = list.to_triangles_with_colors(None, Some(&mut glyphs), colors, 2.0);
        (triangles, images.images().len())
    };
    let mut resolved = list.clone();
    let (mut cache, mut images) = (ColorGlyphCache::new(), ImageAtlas::new());
    cache.begin_frame(&mut images);
    resolved.resolve_color_glyphs(&mut cache, &mut images, 2.0);

    let (direct, placed) = tessellate(&list, true);
    assert_eq!(placed, 1);
    assert_eq!(direct, tessellate(&resolved, false).0);
    assert_eq!(direct, tessellate(&resolved, true).0);
    let images_drawn = direct
        .vertices
        .iter()
        .filter(|vertex| vertex.primitive() == Some(Primitive::Image))
        .count();
    assert_eq!(images_drawn, 2 * 4, "a quad per emoji");
    assert_ne!(
        direct,
        tessellate(&list, false).0,
        "without colours, no quads"
    );
}

/// **A shaped ligature takes the colour path a cmap glyph does**: a flag
/// whose two regional indicators have no colour, joined by the face's GSUB
/// into one COLR glyph, leaves its run as an image quad at its pen, as
/// Segoe UI Emoji's flags need.
#[test]
fn a_shaped_flag_ligature_draws_in_colour() {
    let face = fixtures::shaped();
    let font = Font::sans().with_fallback(core::slice::from_ref(&face));
    let text = "a\u{1F1FA}\u{1F1F8}";
    let layout = TextLayout::new(&font, text, SIZE, 20.0, None);
    assert_eq!(layout.faces(), [0, 1], "the flag is not one glyph");
    let flag = layout.glyphs()[1].glyph;
    assert_ne!(
        flag,
        face.glyph_id('\u{1F1FA}'),
        "the indicators were not joined"
    );
    assert!(face.has_color_glyph(flag));
    let mut list = DrawList::new();
    let origin = Vec2::new(3.0, 20.0);
    for run in layout.face_runs() {
        list.text_glyphs(origin, run.font, SIZE, INK, run.glyphs, &text[run.text]);
    }
    let mut images = ImageAtlas::new();
    let mut cache = ColorGlyphCache::new();
    cache.begin_frame(&mut images);
    list.resolve_color_glyphs(&mut cache, &mut images, 1.0);
    assert_eq!(kinds(&list), ["glyphs 1", "glyphs 0", "image"]);
    let DrawCommand::Image { min, max, .. } = list.commands()[2] else {
        panic!("the flag's quad");
    };
    let pen = origin + layout.glyphs()[1].offset;
    assert_eq!(min.x, pen.round().x);
    assert_eq!(max.x - min.x, SIZE);
    assert_eq!(images.images().len(), 1);
}
