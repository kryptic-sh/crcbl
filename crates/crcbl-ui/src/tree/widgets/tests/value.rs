//! The slider and the drag-value: the engaged rule, the pointer, sensitivity
//! and clamping.

use super::*;
use std::ops::RangeInclusive;

use crate::tree::{DRAG_THRESHOLD, DragNumber, Engagement, Response};

/// The two sliders' and the drag-value's responses from one frame.
struct Values {
    volume: Response,
    pitch: Response,
    drag: Response,
}

/// One frame of a column of two sliders on `0..=10` in steps of one, and a
/// drag-value on `-5..=5` moving `0.5` a pixel and `0.25` a step.
///
/// The values are held as `f64`s, the drag-value's own kind; each slider's is
/// narrowed to the `f32` a slider edits and widened back here, which is exact
/// for every value these tests put in one.
fn values(ui: &mut Ui, pointer: PointerInput, nav: NavInput, held: &mut [f64; 3]) -> Values {
    frame(ui, pointer, nav, |ui| {
        let [volume, pitch, drag] = held;
        let slider = |ui: &mut Ui, selector: &str, value: &mut f64| {
            let mut narrow = *value as f32;
            let response = ui.slider(selector, &mut narrow, 0.0..=10.0, 1.0);
            *value = f64::from(narrow);
            response
        };
        Values {
            volume: slider(ui, "#volume", volume),
            pitch: slider(ui, "#pitch", pitch),
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
    assert_eq!(text_of(&ui, shown.drag.key), "0.75");
}

/// The text the drag-value `key` shows this frame.
fn text_of(ui: &Ui, key: NodeKey) -> String {
    let text_node = children_of(ui, key)[0];
    let node = ui
        .nodes
        .iter()
        .find(|node| node.key == text_node)
        .expect("built");
    let crate::tree::Content::Text { start, end } = node.content else {
        panic!("the drag-value's child is not its text");
    };
    ui.text[start..end].to_owned()
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
        (f64::NAN, 0),
    ] {
        assert_eq!(super::super::value::decimals(step), want, "step {step}");
    }
}

/// A lone drag-value over `value`, held inside `range`, moving `speed` a pixel
/// and `step` a notch, carried from frame to frame.
pub(super) struct Lone<N: DragNumber> {
    pub ui: Ui,
    pub value: N,
    range: RangeInclusive<N>,
    speed: f64,
    step: N,
}

impl<N: DragNumber> Lone<N> {
    /// The drag-value, laid out by one still frame.
    pub fn new(value: N, range: RangeInclusive<N>, speed: f64, step: N) -> Self {
        let mut lone = Self {
            ui: Ui::new(),
            value,
            range,
            speed,
            step,
        };
        lone.frame(idle(), NavInput::default());
        lone
    }

    /// One frame with `pointer` and `nav`.
    pub fn frame(&mut self, pointer: PointerInput, nav: NavInput) -> Response {
        self.frame_with_text(pointer, nav, TextInput::default())
    }

    /// One frame with `pointer`, `nav` and `text` as its text input.
    pub fn frame_with_text(
        &mut self,
        pointer: PointerInput,
        nav: NavInput,
        text: TextInput,
    ) -> Response {
        let Self {
            ui,
            value,
            range,
            speed,
            step,
        } = self;
        frame_with_text(ui, pointer, nav, text, |ui| {
            ui.drag_value("#lone", value, range.clone(), *speed, *step)
        })
    }

    /// The middle of the drag-value, where last frame laid it out.
    pub fn centre(&mut self) -> Vec2 {
        let key = self.frame(idle(), NavInput::default()).key;
        centre(&self.ui, key)
    }

    /// A drag `pixels` to the right from the middle — the press, the moved
    /// press, whose response is returned, and the release where it moved to.
    fn drag(&mut self, pixels: f32) -> Response {
        let origin = self.centre();
        let to = origin + Vec2::new(pixels, 0.0);
        self.frame(press(origin), NavInput::default());
        let moved = self.frame(press(to), NavInput::default());
        self.frame(release(to), NavInput::default());
        moved
    }

    /// A click that does not move, which engages it.
    fn engage(&mut self) {
        let on = self.centre();
        self.frame(press(on), NavInput::default());
        let clicked = self.frame(release(on), NavInput::default());
        assert_eq!(
            clicked.engagement,
            Engagement::Began,
            "the click did not engage"
        );
    }
}

/// Far enough past [`DRAG_THRESHOLD`] that the press is a drag, and a whole
/// number of pixels, so a speed of its reciprocal moves by one.
const DRAG_PIXELS: f32 = 8.0;

/// An `f64` an `f32` cannot hold: the nearest `f32` to it is another number,
/// so a value that went through one lands somewhere else.
pub(super) const FINE: f64 = 0.1 + 1e-12;

/// A distance far below an `f32`'s resolution at [`FINE`].
pub(super) const HAIR: f64 = 1e-12;

/// **A dragged and a stepped `f64` keep 64-bit precision**: moving [`FINE`]
/// by a hair lands on exactly the `f64` the same sum gives, which no value
/// rounded through an `f32` can be.
#[test]
fn an_f64_drags_and_steps_without_rounding_through_an_f32() {
    let mut lone = Lone::new(FINE, f64::MIN..=f64::MAX, HAIR, HAIR);
    assert!(lone.drag(DRAG_PIXELS).changed);
    let want = FINE + f64::from(DRAG_PIXELS) * HAIR;
    assert_eq!(
        lone.value.to_bits(),
        want.to_bits(),
        "{:e} is not {want:e}: the drag lost precision",
        lone.value
    );

    let mut lone = Lone::new(FINE, f64::MIN..=f64::MAX, HAIR, HAIR);
    lone.engage();
    assert!(lone.frame(idle(), RIGHT).changed);
    let want = FINE + HAIR;
    assert_eq!(
        lone.value.to_bits(),
        want.to_bits(),
        "{:e} is not {want:e}: the step lost precision",
        lone.value
    );
}

/// Far past every whole number an `f32` holds exactly.
const FAR: i64 = 1 << 40;

/// **A whole number at 2^40 moves by exactly one**, by a drag and by a notch,
/// and shows every digit: neighbouring `f32`s there are 2^16 apart, so a move
/// of one through an `f32` goes nowhere.
#[test]
fn an_i64_far_past_an_f32s_reach_moves_by_one() {
    let speed = 1.0 / f64::from(DRAG_PIXELS);
    let mut lone = Lone::new(FAR, i64::MIN..=i64::MAX, speed, 1);
    assert!(lone.drag(DRAG_PIXELS).changed);
    assert_eq!(
        lone.value,
        FAR + 1,
        "a drag of one did not land one past 2^40"
    );

    let mut lone = Lone::new(FAR, i64::MIN..=i64::MAX, 1.0, 1);
    lone.engage();
    lone.frame(idle(), RIGHT);
    assert_eq!(lone.value, FAR + 1, "a step right is not one");
    lone.frame(idle(), LEFT);
    let shown = lone.frame(idle(), LEFT);
    assert_eq!(lone.value, FAR - 1, "two steps left are not two");
    assert_eq!(
        text_of(&lone.ui, shown.key),
        (FAR - 1).to_string(),
        "the whole number is not shown digit for digit"
    );
}

/// A speed at which a drag of a few pixels is past any whole number type's
/// whole span.
const HUGE: f64 = 1e30;

/// **A whole number saturates at its type's ends and is held inside its
/// range**: a drag whose distance is past the type's whole span lands on its
/// end rather than wrapping or panicking, a step does the same, and a range
/// holds a drag at its end.
#[test]
fn a_whole_number_saturates_at_its_types_ends_and_clamps_to_its_range() {
    let full = i64::MIN..=i64::MAX;
    for (start, pixels, want) in [
        (i64::MAX - 1, DRAG_PIXELS, i64::MAX),
        (i64::MAX - 1, -DRAG_PIXELS, i64::MIN),
        (i64::MIN + 1, DRAG_PIXELS, i64::MAX),
        (i64::MIN + 1, -DRAG_PIXELS, i64::MIN),
    ] {
        let mut lone = Lone::new(start, full.clone(), HUGE, 1);
        lone.drag(pixels);
        assert_eq!(lone.value, want, "a drag of {pixels} px from {start}");
    }
    for (start, pixels, want) in [
        (1, DRAG_PIXELS, u64::MAX),
        (u64::MAX - 1, -DRAG_PIXELS, u64::MIN),
    ] {
        let mut lone = Lone::new(start, u64::MIN..=u64::MAX, HUGE, 1);
        lone.drag(pixels);
        assert_eq!(lone.value, want, "a drag of {pixels} px from {start}");
    }
    for (pixels, want) in [(DRAG_PIXELS, 10), (-DRAG_PIXELS, -10)] {
        let mut lone = Lone::new(0_i64, -10..=10, HUGE, 1);
        lone.drag(pixels);
        assert_eq!(lone.value, want, "a drag of {pixels} px left its range");
    }

    // A notch as wide as the type cannot wrap either way.
    let mut lone = Lone::new(i64::MAX - 1, full, 1.0, i64::MAX);
    lone.engage();
    lone.frame(idle(), RIGHT);
    assert_eq!(lone.value, i64::MAX, "a step right did not saturate");
    for _ in 0..3 {
        lone.frame(idle(), LEFT);
    }
    assert_eq!(lone.value, i64::MIN, "steps left did not saturate");

    let mut lone = Lone::new(3_u64, u64::MIN..=u64::MAX, 1.0, 5);
    lone.engage();
    lone.frame(idle(), LEFT);
    assert_eq!(lone.value, 0, "a step below zero did not saturate");
}
