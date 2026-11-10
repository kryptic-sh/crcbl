//! How long [`super::layout::MeasureCache`] holds a text's measurements: across
//! layouts while the text comes back, within its caps while it churns, and not
//! past its retention window once it stops.

use super::layout::{RETENTION_CYCLES, SWEEP_EVERY, TEXTS_KEPT};
use super::tests::{frame, idle};
use super::*;

/// One layout of one span of `text`, in the bitmap font.
fn label(ui: &mut Ui, text: &str) {
    frame(ui, idle(), |ui| {
        ui.block("", &[], |ui| {
            ui.span("", text, &[]);
        });
    });
}

/// **A label drawn as a layout of its own, among many on one [`Ui`], is
/// shaped once**: the second frame of many `nowrap` ellipsis labels, each laid
/// out and drawn in a begin-frame, layout and emit cycle of its own, shapes
/// nothing and measures nothing new.
#[cfg(feature = "parsed-font")]
#[test]
fn labels_drawn_a_layout_each_are_shaped_once() {
    use crate::draw_list::DrawList;
    use crate::font::Font;
    use crate::font::shape::shaped_runs;
    const LABELS: usize = 180;
    // A parse of its own, so no other test's shaping is counted.
    let font = Font::parse(crate::font::SANS_TTF).expect("parses");
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "labels.css",
        ".label { width: 140px; }
         .text { font-family: counted; white-space: nowrap; overflow: hidden;
                 text-overflow: ellipsis; }",
    );
    ui.register_font("counted", font.clone())
        .expect("not reserved");
    let texts: Vec<String> = (0..LABELS)
        .map(|i| format!("Healing potion of the ancient dragon {i}"))
        .collect();
    let draw_frame = |ui: &mut Ui| {
        let mut cut = 0;
        for text in &texts {
            let mut key = None;
            frame(ui, idle(), |ui| {
                ui.block(".label", &[], |ui| {
                    key = Some(ui.span(".text", text.as_str(), &[]).key);
                });
            });
            ui.emit(&mut DrawList::new());
            if ui.text(key.expect("built")) != Some(text.as_str()) {
                cut += 1;
            }
        }
        cut
    };

    let before = shaped_runs(&font);
    let cut = draw_frame(&mut ui);
    assert!(
        shaped_runs(&font) > before,
        "the first frame shaped nothing"
    );
    assert_eq!(cut, LABELS, "the labels were not cut, so no cut was kept");
    let (before, misses) = (shaped_runs(&font), ui.measure.misses);
    draw_frame(&mut ui);
    assert_eq!(
        shaped_runs(&font) - before,
        0,
        "a steady frame of one layout per label shaped its labels again"
    );
    assert_eq!(ui.measure.misses, misses, "a steady frame measured again");
}

/// **A label whose text is new every layout**, a timer, leaves no more texts
/// behind than the retention window and the layouts between its sweeps, over
/// many windows.
#[test]
fn a_churning_label_holds_at_most_its_window() {
    let mut ui = Ui::new();
    let mut most = 0;
    for tick in 0..4 * RETENTION_CYCLES {
        label(&mut ui, &format!("{tick}"));
        most = most.max(ui.measure.texts());
    }
    let window = usize::try_from(RETENTION_CYCLES + SWEEP_EVERY).expect("fits");
    assert!(
        most <= window,
        "{most} texts held past a window of {window}"
    );
    assert!(
        most > usize::try_from(RETENTION_CYCLES).expect("fits"),
        "only {most} texts were ever held: the window did not hold them"
    );
}

/// **Text churning faster than the window lets it go is held to the cap**:
/// many new texts every layout never leave more than [`TEXTS_KEPT`] held after
/// a layout, and a layout that evicts keeps the newest — its own texts.
#[test]
fn churn_past_the_window_is_held_to_the_cap() {
    const PER_LAYOUT: usize = 40;
    let build = |ui: &mut Ui, tick: usize| {
        frame(ui, idle(), |ui| {
            ui.block("", &[], |ui| {
                for i in 0..PER_LAYOUT {
                    ui.span("", format!("{tick}.{i}").as_str(), &[]);
                }
            });
        });
    };
    let mut ui = Ui::new();
    let (mut most, mut evictions) = (0, 0);
    for tick in 0..4 * TEXTS_KEPT / PER_LAYOUT {
        let held = ui.measure.texts();
        build(&mut ui, tick);
        most = most.max(ui.measure.texts());
        if ui.measure.texts() < held {
            evictions += 1;
            let built = ui
                .nodes
                .iter()
                .filter(|node| matches!(node.content, Content::Text { .. }));
            for node in built {
                assert!(
                    ui.measure.holds(node.content_hash),
                    "the evicting layout's own texts were evicted ahead of older ones"
                );
            }
        }
    }
    assert!(most <= TEXTS_KEPT, "{most} texts held past the cap");
    assert!(
        evictions > 1,
        "{evictions} evictions: the cap was never reached"
    );
}

/// **A text no layout builds is dropped once the retention window passes**,
/// at the next sweep, and measured anew when it comes back; within the window
/// it is held. A text every layout builds is held however long it runs,
/// though an unchanged layout never measures it.
#[test]
fn a_text_unused_past_the_window_is_evicted() {
    let mut ui = Ui::new();
    label(&mut ui, "OLD");
    for _ in 0..RETENTION_CYCLES - 1 {
        label(&mut ui, "NEW");
    }
    assert_eq!(ui.measure.texts(), 2, "dropped inside the window");
    let measured = (ui.measure.hits, ui.measure.misses);
    for _ in 0..RETENTION_CYCLES + SWEEP_EVERY {
        label(&mut ui, "NEW");
    }
    assert_eq!(
        (ui.measure.hits, ui.measure.misses),
        measured,
        "an unchanged layout measured its text"
    );
    assert_eq!(ui.measure.texts(), 1, "held past the window and a sweep");
    label(&mut ui, "NEW");
    assert_eq!(
        ui.measure.misses, measured.1,
        "the text every layout built was dropped"
    );
    let misses = ui.measure.misses;
    label(&mut ui, "OLD");
    assert!(
        ui.measure.misses > misses,
        "came back without being measured"
    );
}

/// **A wrapping text is held at no more than [`super::layout::WIDTHS_KEPT`]
/// definite widths**, so a box dragged through every width does not grow its
/// text's measurements without bound — and it is still answered at the width
/// it settles at.
#[cfg(feature = "parsed-font")]
#[test]
fn a_text_holds_a_bounded_number_of_widths() {
    use super::layout::WIDTHS_KEPT;
    use crate::style::Declaration;
    let mut ui = Ui::new();
    ui.add_stylesheet("wrap.css", ".wrap { font-family: sans-serif; }");
    let build = |ui: &mut Ui, width: f32| {
        frame(ui, idle(), |ui| {
            ui.block("", &[Declaration::Width(LengthAuto::Px(width))], |ui| {
                ui.span(".wrap", "Wrapped labels lay out one line at a time", &[]);
            });
        });
    };
    for width in 0..4 * WIDTHS_KEPT {
        build(&mut ui, 100.0 + width as f32);
    }
    assert_eq!(ui.measure.texts(), 1);
    let held = ui.measure.measurements();
    // Min-content and max-content beside the definite widths.
    assert!(
        held <= WIDTHS_KEPT + 2,
        "{held} measurements held for one text"
    );
    assert!(held > WIDTHS_KEPT, "only {held}: the widths never filled");
    let misses = ui.measure.misses;
    build(&mut ui, 100.0 + (4 * WIDTHS_KEPT - 1) as f32);
    assert_eq!(ui.measure.misses, misses, "the last width was not held");
}

/// A [`Ui`] whose `.counted` spans are drawn in `font`.
#[cfg(feature = "parsed-font")]
fn counted(font: &crate::font::Font) -> Ui {
    let mut ui = Ui::new();
    ui.add_stylesheet("counted.css", ".counted { font-family: counted; }");
    ui.register_font("counted", font.clone())
        .expect("not reserved");
    ui
}

/// One frame of a `width`-pixel column holding one wrapped `.counted` span of
/// `text`, `align`ed, laid out and drawn; returns what it drew.
#[cfg(feature = "parsed-font")]
fn wrapped_card(ui: &mut Ui, width: f32, align: TextAlign, text: &str) -> String {
    use crate::draw_list::DrawList;
    use crate::style::Declaration;
    frame(ui, idle(), |ui| {
        ui.block("", &[Declaration::Width(LengthAuto::Px(width))], |ui| {
            ui.span(".counted", text, &[Declaration::TextAlign(align)]);
        });
    });
    let mut list = DrawList::new();
    ui.emit(&mut list);
    format!("{:?}", list.commands())
}

/// What a fresh [`Ui`] draws for [`wrapped_card`]: nothing kept from before.
#[cfg(feature = "parsed-font")]
fn drawn_afresh(font: &crate::font::Font, width: f32, align: TextAlign, text: &str) -> String {
    wrapped_card(&mut counted(font), width, align, text)
}

/// **A wrapped span is not shaped again to draw it while it is unchanged**:
/// the second frame of the same wrapped span, left- or centre-aligned, shapes
/// nothing, and draws what the first frame drew — which is what laying its
/// text out afresh draws.
#[cfg(feature = "parsed-font")]
#[test]
fn a_wrapped_span_is_shaped_once() {
    use crate::font::Font;
    use crate::font::shape::shaped_runs;
    const TEXT: &str = "Wrapped labels lay out under Taffy, one greedy line at a time.";
    for align in [TextAlign::Left, TextAlign::Center] {
        // A parse of its own, so no other test's shaping is counted.
        let font = Font::parse(crate::font::SANS_TTF).expect("parses");
        let mut ui = counted(&font);
        let before = shaped_runs(&font);
        let first = wrapped_card(&mut ui, 140.0, align, TEXT);
        assert!(
            shaped_runs(&font) > before,
            "{align:?}: the first frame shaped nothing"
        );
        let before = shaped_runs(&font);
        let steady = wrapped_card(&mut ui, 140.0, align, TEXT);
        assert_eq!(
            shaped_runs(&font) - before,
            0,
            "{align:?}: an unchanged frame shaped the wrapped span again"
        );
        let wrapped = ui
            .nodes
            .iter()
            .find_map(|node| node.shown_layout.as_ref())
            .expect("the span kept no layout");
        assert!(
            wrapped.lines().len() > 1,
            "{align:?}: the span did not wrap"
        );
        assert_eq!(steady, first, "{align:?}: a steady frame drew differently");
        assert_eq!(
            steady,
            drawn_afresh(&font, 140.0, align, TEXT),
            "{align:?}: the kept layout drew differently from a fresh one"
        );
    }
}

/// **Two widths broken at the same [`crate::font::layout::wrap_width`] share
/// one kept layout, and each is still aligned to its own box**: the second
/// width shapes nothing, yet draws what a fresh [`Ui`] draws at it, not what
/// the first width drew — so the kept layout is the unaligned one.
#[cfg(feature = "parsed-font")]
#[test]
fn a_wrap_width_s_widths_share_a_layout_and_align_apart() {
    use crate::font::Font;
    use crate::font::layout::wrap_width;
    use crate::font::shape::shaped_runs;
    const TEXT: &str = "Wrapped labels lay out under Taffy, one greedy line at a time.";
    const FIRST: f32 = 140.3;
    const SECOND: f32 = 140.8;
    assert_eq!(
        wrap_width(FIRST).to_bits(),
        wrap_width(SECOND).to_bits(),
        "the two widths must break alike for this test to ask its question"
    );
    for align in [TextAlign::Center, TextAlign::Right] {
        let font = Font::parse(crate::font::SANS_TTF).expect("parses");
        let mut ui = counted(&font);
        let first = wrapped_card(&mut ui, FIRST, align, TEXT);
        let before = shaped_runs(&font);
        let second = wrapped_card(&mut ui, SECOND, align, TEXT);
        assert_eq!(
            shaped_runs(&font) - before,
            0,
            "{align:?}: a width in the same wrap width shaped the span again"
        );
        assert_ne!(
            second, first,
            "{align:?}: the two boxes aligned alike, so this cannot see a kept alignment"
        );
        assert_eq!(
            second,
            drawn_afresh(&font, SECOND, align, TEXT),
            "{align:?}: the shared layout drew differently from a fresh one at {SECOND}px"
        );
    }
}

/// **A held wrapped layout is never another text's**, even under the same
/// content hash: [`MeasureCache::wrapped_layout`] checks the text itself.
#[cfg(feature = "parsed-font")]
#[test]
fn a_wrapped_layout_under_a_colliding_hash_is_not_served() {
    use crate::font::Font;
    let font = Font::parse(crate::font::SANS_TTF).expect("parses");
    let mut cache = MeasureCache::default();
    let (size, line_height, width) = (16.0, 20.0, 140.0);
    let held = cache.wrapped_layout(7, &font, "one text", size, line_height, width);
    assert!(held.lays_out(&font, "one text", size, line_height));
    let other = cache.wrapped_layout(7, &font, "another text", size, line_height, width);
    assert!(
        other.lays_out(&font, "another text", size, line_height),
        "the layout held for one text was served for another under its hash"
    );
}

/// **A wrapped span whose width or text changes is laid out again**, never
/// drawn from the layout kept for the old one: each change draws what a fresh
/// [`Ui`] draws for it.
#[cfg(feature = "parsed-font")]
#[test]
fn a_changed_wrapped_span_is_laid_out_again() {
    use crate::font::Font;
    const TEXT: &str = "Wrapped labels lay out under Taffy, one greedy line at a time.";
    const OTHER: &str = "Other labels break somewhere else entirely, a word or two on.";
    let font = Font::parse(crate::font::SANS_TTF).expect("parses");
    let mut ui = counted(&font);
    let align = TextAlign::Left;
    for (width, text) in [
        (140.0, TEXT),
        (140.0, TEXT),
        (100.0, TEXT),
        (140.0, TEXT),
        (140.0, OTHER),
        (100.0, OTHER),
    ] {
        assert_eq!(
            wrapped_card(&mut ui, width, align, text),
            drawn_afresh(&font, width, align, text),
            "{text:?} at {width}px drew differently from a fresh tree"
        );
    }
}
