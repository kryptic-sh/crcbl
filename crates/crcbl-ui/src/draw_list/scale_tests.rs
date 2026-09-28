//! The list's scale: logical pixels pushed, window pixels recorded.

use super::*;

const WHITE: [f32; 4] = [1.0; 4];

/// One of every command that needs no font, some under a clip and some above
/// the overlay cut — what a scaled list and an unscaled one are compared on.
fn every_fontless_command(dl: &mut DrawList) {
    dl.rect(Vec2::new(10.0, 20.0), Vec2::new(30.0, 40.0), WHITE);
    dl.push_clip(Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0));
    dl.rect_outline(Vec2::new(1.0, 2.0), Vec2::new(33.0, 44.0), 2.0, WHITE);
    dl.line(Vec2::new(1.0, 1.0), Vec2::new(9.0, 5.0), 3.0, WHITE);
    dl.polyline(
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 7.0),
        ],
        1.5,
        true,
        WHITE,
    );
    dl.text(Vec2::new(4.0, 6.0), "clipped", WHITE, 14.0);
    dl.pop_clip().expect("pushed above");
    dl.begin_overlay();
    dl.push_command(DrawCommand::Image {
        min: Vec2::new(2.0, 3.0),
        max: Vec2::new(12.0, 13.0),
        uv_min: Vec2::new(0.25, 0.5),
        uv_max: Vec2::new(0.75, 1.0),
        tint: WHITE,
    });
    dl.rounded_rect(
        Vec2::new(5.0, 5.0),
        Vec2::new(45.0, 25.0),
        CornerRadii {
            top_left: 1.0,
            top_right: 2.0,
            bottom_right: 3.0,
            bottom_left: 4.0,
        },
        WHITE,
        Border {
            width: 1.5,
            color: [0.5; 4],
        },
    );
}

/// The list's expansion as bytes, for a comparison nothing about float
/// equality can soften.
fn triangle_bytes(dl: &DrawList) -> (Vec<u8>, Vec<u32>, usize) {
    let triangles = dl.to_triangles_split(Some(&FontAtlas::built_in()), None, 1.0);
    (
        bytemuck::cast_slice(&triangles.vertices).to_vec(),
        triangles.indices,
        triangles.overlay,
    )
}

/// **A scale of one records exactly what a list that was never scaled does**,
/// command for command and byte for byte in its expansion — the promise that
/// keeps every golden where it was.
#[test]
fn a_scale_of_one_is_byte_identical_to_no_scale() {
    let mut plain = DrawList::new();
    every_fontless_command(&mut plain);
    let mut unit = DrawList::new();
    unit.set_scale(1.0);
    every_fontless_command(&mut unit);

    assert_eq!(
        format!("{:?}", unit.commands()),
        format!("{:?}", plain.commands())
    );
    assert_eq!(unit.clips(), plain.clips());
    assert_eq!(unit.base_commands().len(), plain.base_commands().len());
    assert_eq!(triangle_bytes(&unit), triangle_bytes(&plain));
}

/// **Every length a command carries is scaled, and nothing that is not a
/// length is**: positions, sizes, strokes, a text size, radii and a border
/// width at twice the logical value; colours and UVs as they were.
#[test]
fn every_command_kind_scales_its_lengths() {
    let mut dl = DrawList::new();
    dl.set_scale(2.0);
    every_fontless_command(&mut dl);
    let commands = dl.commands();
    assert_eq!(commands.len(), 7);

    assert!(matches!(
        commands[0],
        DrawCommand::Rect { min, max, .. }
            if min == Vec2::new(20.0, 40.0) && max == Vec2::new(60.0, 80.0)
    ));
    assert!(matches!(
        commands[1],
        DrawCommand::RectOutline { min, max, thickness, .. }
            if min == Vec2::new(2.0, 4.0) && max == Vec2::new(66.0, 88.0) && thickness == 4.0
    ));
    assert!(matches!(
        commands[2],
        DrawCommand::Line { from, to, thickness, .. }
            if from == Vec2::new(2.0, 2.0) && to == Vec2::new(18.0, 10.0) && thickness == 6.0
    ));
    let DrawCommand::Polyline {
        points,
        thickness,
        closed,
        ..
    } = &commands[3]
    else {
        panic!("not a polyline: {:?}", commands[3]);
    };
    assert_eq!(
        points,
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(20.0, 14.0)
        ]
    );
    assert_eq!((*thickness, *closed), (3.0, true));
    assert!(matches!(
        &commands[4],
        DrawCommand::Text { pos, text, size, .. }
            if *pos == Vec2::new(8.0, 12.0) && text == "clipped" && *size == 28.0
    ));
    assert!(matches!(
        commands[5],
        DrawCommand::Image { min, max, uv_min, uv_max, .. }
            if min == Vec2::new(4.0, 6.0)
                && max == Vec2::new(24.0, 26.0)
                && uv_min == Vec2::new(0.25, 0.5)
                && uv_max == Vec2::new(0.75, 1.0)
    ));
    let DrawCommand::RoundedRect {
        min,
        max,
        radii,
        color,
        border,
    } = commands[6]
    else {
        panic!("not a rounded rectangle: {:?}", commands[6]);
    };
    assert_eq!((min, max), (Vec2::new(10.0, 10.0), Vec2::new(90.0, 50.0)));
    assert_eq!(
        radii,
        CornerRadii {
            top_left: 2.0,
            top_right: 4.0,
            bottom_right: 6.0,
            bottom_left: 8.0,
        }
    );
    assert_eq!(color, WHITE);
    assert_eq!(
        border,
        Border {
            width: 3.0,
            color: [0.5; 4],
        }
    );
}

/// **A clip scales with the commands under it, and the overlay cut stays
/// where it was pushed**: the clipped commands carry the clip in window
/// pixels, the ones outside it carry none, and the cut still separates the
/// same commands.
#[test]
fn clips_and_the_overlay_cut_scale_with_the_list() {
    let mut dl = DrawList::new();
    dl.set_scale(2.0);
    every_fontless_command(&mut dl);

    assert_eq!(dl.base_commands().len(), 5);
    assert_eq!(dl.overlay_commands().len(), 2);
    let window_clip = ClipRect {
        min: Vec2::new(0.0, 0.0),
        max: Vec2::new(200.0, 100.0),
    };
    assert_eq!(
        dl.clips(),
        [
            ClipRect::NONE,
            window_clip,
            window_clip,
            window_clip,
            window_clip,
            ClipRect::NONE,
            ClipRect::NONE,
        ]
    );
}

/// **Nested clips intersect in window pixels**, and a side left at
/// [`ClipRect::NONE`]'s bound stays unbounded rather than overflowing or
/// shrinking with the scale.
#[test]
fn nested_clips_intersect_scaled_and_no_clip_stays_no_clip() {
    for scale in [0.5, 3.0] {
        let mut dl = DrawList::new();
        dl.set_scale(scale);
        dl.push_clip(ClipRect::NONE.min, ClipRect::NONE.max);
        assert_eq!(dl.clip(), ClipRect::NONE, "at {scale}");
        dl.push_clip(Vec2::new(10.0, 10.0), Vec2::new(40.0, 30.0));
        dl.push_clip(Vec2::new(20.0, 0.0), Vec2::new(f32::MAX, 20.0));
        assert_eq!(
            dl.clip(),
            ClipRect {
                min: Vec2::new(20.0, 10.0) * scale,
                max: Vec2::new(40.0, 20.0) * scale,
            },
            "at {scale}"
        );
    }
}

/// **A pointer maps back through the scale**, and a scale that means nothing
/// records at one rather than dividing a pointer by zero.
#[test]
fn to_logical_undoes_the_scale_and_a_meaningless_scale_is_one() {
    let mut dl = DrawList::new();
    assert_eq!(dl.scale(), 1.0);
    dl.set_scale(1.5);
    assert_eq!(dl.scale(), 1.5);
    assert_eq!(
        dl.to_logical(Vec2::new(960.0, 540.0)),
        Vec2::new(640.0, 360.0)
    );
    for meaningless in [0.0, -2.0, f32::NAN, f32::INFINITY] {
        dl.set_scale(meaningless);
        assert_eq!(dl.scale(), 1.0, "{meaningless} was kept");
    }
    assert_eq!(DrawList::default().scale(), 1.0);
}

/// **The scale applies to what is pushed after it is set**, so one list can
/// hold a game's UI at its scale and the engine's overlay at one — and
/// `clear` keeps it.
#[test]
fn a_scale_applies_from_where_it_is_set_and_survives_clear() {
    let mut dl = DrawList::new();
    dl.set_scale(2.0);
    dl.rect(Vec2::ZERO, Vec2::splat(10.0), WHITE);
    dl.begin_overlay();
    dl.set_scale(1.0);
    dl.rect(Vec2::ZERO, Vec2::splat(10.0), WHITE);
    let maxes: Vec<Vec2> = dl
        .commands()
        .iter()
        .map(|command| match command {
            DrawCommand::Rect { max, .. } => *max,
            other => panic!("not a rect: {other:?}"),
        })
        .collect();
    assert_eq!(maxes, [Vec2::splat(20.0), Vec2::splat(10.0)]);

    dl.set_scale(2.0);
    dl.clear();
    assert_eq!(dl.scale(), 2.0);
}

/// **A glyph run scales its origin, its size and every pen offset**, so it is
/// rasterised at the window's size rather than stretched from the logical one.
#[cfg(feature = "parsed-font")]
#[test]
fn a_glyph_run_scales_its_origin_size_and_pen_offsets() {
    use crate::font::layout::PositionedGlyph;

    let font = Font::sans();
    let run = [
        PositionedGlyph {
            glyph: font.glyph_id('A'),
            offset: Vec2::new(0.0, 12.0),
        },
        PositionedGlyph {
            glyph: font.glyph_id('b'),
            offset: Vec2::new(9.5, 12.0),
        },
    ];
    let mut dl = DrawList::new();
    dl.set_scale(1.5);
    dl.text_glyphs(Vec2::new(4.0, 6.0), font, 14.0, WHITE, run, "Ab");
    let DrawCommand::Glyphs {
        origin,
        size,
        glyphs,
        text,
        ..
    } = &dl.commands()[0]
    else {
        panic!("not a glyph run");
    };
    assert_eq!(*origin, Vec2::new(6.0, 9.0));
    assert_eq!(*size, 21.0);
    assert_eq!(text.as_deref(), Some("Ab"));
    assert_eq!(glyphs.len(), run.len());
    for (scaled, logical) in glyphs.iter().zip(&run) {
        assert_eq!(scaled.glyph, logical.glyph);
        assert_eq!(scaled.offset, logical.offset * 1.5);
    }
}
