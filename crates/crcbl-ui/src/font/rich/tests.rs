use super::*;
use crate::font::FontMetrics;

const SIZE: f32 = 20.0;
const LINE_HEIGHT: f32 = 24.0;

/// Every Latin-1 char of [`plain`] at [`SIZE`]: half an em.
const PLAIN: f32 = 10.0;
/// Every Latin-1 char of [`bold`] at [`SIZE`].
const BOLD: f32 = 12.0;

fn metrics() -> FontMetrics {
    FontMetrics {
        units_per_em: 1000,
        ascent: 800.0,
        descent: -200.0,
        line_gap: 0.0,
    }
}

fn plain() -> &'static Font {
    Font::fixed_pitch_face(metrics(), 500.0, 400.0, false)
}

fn bold() -> &'static Font {
    Font::fixed_pitch_face(metrics(), 600.0, 700.0, false)
}

fn run<'a>(text: &'a str, font: &'a Font) -> RichRun<'a> {
    RichRun {
        text,
        font,
        size: SIZE,
    }
}

/// Each segment of `layout`'s line `line`: its run, where its bytes start,
/// its left edge and its width.
fn segments(layout: &RichLayout, line: usize) -> Vec<(usize, usize, f32, f32)> {
    layout
        .line_segments(&layout.lines()[line])
        .iter()
        .map(|segment| (segment.run, segment.text.start, segment.x, segment.width))
        .collect()
}

/// **Runs break into lines together, and a run boundary is no break**: `un`
/// in the plain run and `known` in the bold one are one word, so the line
/// breaks before `un`, not between it and `known`, and that line holds a
/// segment of each run, the bold one starting where the plain one ends. A
/// line's width, and its last segment's, leave out the space it ends with.
#[test]
fn runs_break_together_and_a_run_boundary_is_no_break() {
    let (plain, bold) = (plain(), bold());
    let layout = RichLayout::new(
        &[run("a un", plain), run("known b", bold)],
        LINE_HEIGHT,
        Some(90.0),
    );
    assert_eq!(layout.lines().len(), 3, "{:#?}", layout.lines());
    assert_eq!(segments(&layout, 0), [(0, 0, 0.0, PLAIN)]);
    assert_eq!(
        segments(&layout, 1),
        [(0, 2, 0.0, 2.0 * PLAIN), (1, 0, 2.0 * PLAIN, 5.0 * BOLD)]
    );
    assert_eq!(layout.lines()[1].width, 2.0 * PLAIN + 5.0 * BOLD);
    assert_eq!(segments(&layout, 2), [(1, 6, 0.0, BOLD)]);
    assert_eq!(layout.lines()[0].width, PLAIN);
}

/// **A newline ends a line wherever it falls, and each line is a line
/// height tall**, one under the next; with no wrap width nothing else
/// breaks, and the box is the widest line by the lines' heights. The newline
/// takes no width.
#[test]
fn hard_breaks_end_lines_and_the_box_holds_them() {
    let (plain, bold) = (plain(), bold());
    let layout = RichLayout::new(
        &[run("one\ntwo ", plain), run("three", bold)],
        LINE_HEIGHT,
        None,
    );
    let tops: Vec<f32> = layout.lines().iter().map(|line| line.top).collect();
    assert_eq!(tops, [0.0, LINE_HEIGHT]);
    assert_eq!(segments(&layout, 0), [(0, 0, 0.0, 3.0 * PLAIN)]);
    assert_eq!(
        segments(&layout, 1),
        [(0, 4, 0.0, 4.0 * PLAIN), (1, 0, 4.0 * PLAIN, 5.0 * BOLD)]
    );
    // The ascent-to-descent box, 20 px, centred in the 24 px line.
    assert_eq!(layout.lines()[0].baseline, 2.0 + 16.0);
    assert_eq!(
        layout.measure(),
        Vec2::new(4.0 * PLAIN + 5.0 * BOLD, 2.0 * LINE_HEIGHT)
    );
}

/// **Alignment moves each line's segments across the box** by the room the
/// line leaves.
#[test]
fn alignment_moves_each_lines_segments() {
    let (plain, bold) = (plain(), bold());
    let mut layout = RichLayout::new(&[run("ab ", plain), run("c", bold)], LINE_HEIGHT, None);
    let width = 2.0 * PLAIN + PLAIN + BOLD;
    layout.align(width + 40.0, TextAlign::Center);
    assert_eq!(
        segments(&layout, 0),
        [(0, 0, 20.0, 3.0 * PLAIN), (1, 0, 20.0 + 3.0 * PLAIN, BOLD)]
    );
}
