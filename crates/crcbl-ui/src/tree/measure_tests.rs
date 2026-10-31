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
