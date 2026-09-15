//! The slider and the drag-value: the engaged rule, the pointer, sensitivity
//! and clamping.

use super::*;
use crate::tree::{DRAG_THRESHOLD, Engagement, Response};

/// The two sliders' and the drag-value's responses from one frame.
struct Values {
    volume: Response,
    pitch: Response,
    drag: Response,
}

/// One frame of a column of two sliders on `0..=10` in steps of one, and a
/// drag-value on `-5..=5` moving `0.5` a pixel and `0.25` a step.
fn values(ui: &mut Ui, pointer: PointerInput, nav: NavInput, held: &mut [f32; 3]) -> Values {
    frame(ui, pointer, nav, |ui| {
        let [volume, pitch, drag] = held;
        Values {
            volume: ui.slider("#volume", volume, 0.0..=10.0, 1.0),
            pitch: ui.slider("#pitch", pitch, 0.0..=10.0, 1.0),
            drag: ui.drag_value("#drag", drag, -5.0..=5.0, 0.5, 0.25),
        }
    })
}

/// **The LOCKED rule on a slider**: focus moves past it untouched, accept
/// engages it, right and left step it while up and down do nothing, back
/// cancels to the value it engaged with, accept commits — and the slider
/// that was never engaged keeps its value throughout.
#[test]
fn a_slider_engages_steps_cancels_to_its_snapshot_and_commits() {
    let mut ui = Ui::new();
    let mut held = [4.0, 7.0, 0.0];
    values(&mut ui, idle(), NavInput::default(), &mut held);
    values(&mut ui, idle(), NavInput::NAVIGATION, &mut held);
    let passed = values(&mut ui, idle(), DOWN, &mut held);
    assert!(
        passed.pitch.focused,
        "down did not move focus past the slider"
    );
    let back_up = values(&mut ui, idle(), UP, &mut held);
    assert!(back_up.volume.focused);
    assert_eq!(held, [4.0, 7.0, 0.0], "a focused slider took a step");

    let engaged = values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    assert_eq!(engaged.volume.engagement, Engagement::Began);
    for nav in [RIGHT, RIGHT, RIGHT, LEFT, UP, DOWN] {
        let step = values(&mut ui, idle(), nav, &mut held);
        assert!(
            step.volume.focused,
            "{nav:?} moved focus off the engaged slider"
        );
    }
    assert_eq!(held[0], 6.0, "three rights and a left from four");
    assert_eq!(held[1], 7.0, "the slider that was never engaged moved");

    let cancelled = values(&mut ui, idle(), NavInput::BACK, &mut held);
    assert_eq!(cancelled.volume.engagement, Engagement::Cancelled);
    assert_eq!(held[0], 4.0, "back did not restore the snapshot");
    assert!(cancelled.volume.changed, "the restore was not reported");

    values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    for _ in 0..20 {
        values(&mut ui, idle(), RIGHT, &mut held);
    }
    assert_eq!(held[0], 10.0, "steps ran past the range's end");
    let committed = values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    assert_eq!(committed.volume.engagement, Engagement::Committed);
    values(&mut ui, idle(), NavInput::BACK, &mut held);
    assert_eq!(
        held,
        [10.0, 7.0, 0.0],
        "commit lost the edit, or back after it undid it"
    );
}

/// **The pointer drives a slider without engaging it**: a press puts the
/// value where it lands on the track, snapped to the step; a drag carries it
/// and ends focused but not engaged; and a click that does not move engages.
#[test]
fn a_slider_follows_the_pointer_and_a_drag_ends_without_engaging() {
    let mut ui = Ui::new();
    let mut held = [0.0, 0.0, 0.0];
    let first = values(&mut ui, idle(), NavInput::default(), &mut held);
    let (min, max) = rect(&ui, first.volume.key);
    // The track is the content box: one pixel of border and two of padding.
    let track = (min.x + 3.0, max.x - 3.0);
    let at = |share: f32| Vec2::new(track.0 + share * (track.1 - track.0), (min.y + max.y) * 0.5);

    let pressed = values(&mut ui, press(at(0.28)), NavInput::default(), &mut held);
    assert_eq!(held[0], 3.0, "a press at 28% of the track is not 3");
    assert!(pressed.volume.changed);
    values(&mut ui, press(at(0.81)), NavInput::default(), &mut held);
    assert_eq!(held[0], 8.0, "a drag to 81% is not 8");
    values(
        &mut ui,
        press(Vec2::new(track.1 + 50.0, min.y)),
        NavInput::default(),
        &mut held,
    );
    assert_eq!(
        held[0], 10.0,
        "a drag past the track's end is not the maximum"
    );
    let released = values(&mut ui, release(at(0.9)), NavInput::default(), &mut held);
    assert_eq!(held[0], 9.0, "the release did not carry the value to 90%");
    assert!(
        released.volume.focused,
        "a drag released over the slider did not focus it"
    );
    assert_eq!(ui.engaged(), None, "a drag left the slider engaged");

    let still = at(0.5);
    values(&mut ui, press(still), NavInput::default(), &mut held);
    let clicked = values(&mut ui, release(still), NavInput::default(), &mut held);
    assert_eq!(
        clicked.volume.engagement,
        Engagement::Began,
        "a click that did not move did not engage"
    );
    assert_eq!(held[0], 5.0);
    assert_eq!(held[1], 0.0, "the other slider moved");
}

/// **A drag-value moves by `speed` per pixel, only once the press is a drag,
/// measured from where the press began, and never outside its range** — and
/// comes back to its start when the pointer does.
#[test]
fn a_drag_value_moves_by_its_speed_past_the_threshold_and_clamps() {
    let mut ui = Ui::new();
    let mut held = [0.0, 0.0, 1.0];
    let first = values(&mut ui, idle(), NavInput::default(), &mut held);
    let origin = centre(&ui, first.drag.key);
    let right = |pixels: f32| origin + Vec2::new(pixels, 0.0);

    values(&mut ui, press(origin), NavInput::default(), &mut held);
    let under = values(
        &mut ui,
        press(right(DRAG_THRESHOLD)),
        NavInput::default(),
        &mut held,
    );
    assert_eq!(
        held[2], 1.0,
        "a press that has not passed the threshold moved it"
    );
    assert!(!under.drag.changed);

    let moved = values(&mut ui, press(right(6.0)), NavInput::default(), &mut held);
    assert_eq!(held[2], 4.0, "six pixels at 0.5 a pixel from 1");
    assert!(moved.drag.changed);
    values(&mut ui, press(right(-2.0)), NavInput::default(), &mut held);
    assert_eq!(
        held[2], 0.0,
        "the drag is not measured from where the press began"
    );
    values(&mut ui, press(right(100.0)), NavInput::default(), &mut held);
    assert_eq!(
        held[2], 5.0,
        "a drag past the range was not clamped to its end"
    );
    values(
        &mut ui,
        press(right(-100.0)),
        NavInput::default(),
        &mut held,
    );
    assert_eq!(
        held[2], -5.0,
        "a drag below the range was not clamped to its start"
    );
    values(&mut ui, press(origin), NavInput::default(), &mut held);
    assert_eq!(
        held[2], 1.0,
        "returning to the press did not return the value"
    );

    let released = values(
        &mut ui,
        release(right(10.0)),
        NavInput::default(),
        &mut held,
    );
    assert_eq!(held[2], 5.0, "the release frame's drag was not clamped");
    assert!(released.drag.focused && ui.engaged().is_none());
}

/// **The LOCKED rule on a drag-value, and what it shows**: accept engages,
/// right and left step by `step`, back restores, accept commits; its text
/// has as many decimals as its step.
#[test]
fn a_drag_value_engages_steps_cancels_and_shows_its_steps_decimals() {
    let mut ui = Ui::new();
    let mut held = [0.0, 0.0, 1.0];
    values(&mut ui, idle(), NavInput::default(), &mut held);
    values(&mut ui, idle(), NavInput::NAVIGATION, &mut held);
    values(&mut ui, idle(), DOWN, &mut held);
    let focused = values(&mut ui, idle(), DOWN, &mut held);
    assert!(focused.drag.focused);

    values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    values(&mut ui, idle(), RIGHT, &mut held);
    values(&mut ui, idle(), RIGHT, &mut held);
    assert_eq!(held[2], 1.5);
    let cancelled = values(&mut ui, idle(), NavInput::BACK, &mut held);
    assert_eq!(cancelled.drag.engagement, Engagement::Cancelled);
    assert_eq!(held[2], 1.0, "back did not restore the drag-value");

    values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    values(&mut ui, idle(), LEFT, &mut held);
    values(&mut ui, idle(), NavInput::ACCEPT, &mut held);
    let shown = values(&mut ui, idle(), NavInput::default(), &mut held);
    assert_eq!(held[2], 0.75, "commit lost the step");
    let text_node = children_of(&ui, shown.drag.key)[0];
    let node = ui
        .nodes
        .iter()
        .find(|node| node.key == text_node)
        .expect("built");
    let crate::tree::Content::Text { start, end } = node.content else {
        panic!("the drag-value's child is not its text");
    };
    assert_eq!(&ui.text[start..end], "0.75");
}

/// **Decimals follow the step**, by multiplying, for the steps an editor
/// uses.
#[test]
fn a_steps_decimals_are_counted_without_logarithms() {
    for (step, want) in [
        (1.0, 0),
        (5.0, 0),
        (0.5, 1),
        (0.1, 1),
        (0.25, 2),
        (0.05, 2),
        (0.001, 3),
        (1e-9, 6),
        (0.0, 0),
        (f32::NAN, 0),
    ] {
        assert_eq!(super::super::value::decimals(step), want, "step {step}");
    }
}
