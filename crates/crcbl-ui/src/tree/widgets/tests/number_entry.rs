//! Typing a number into a drag-value: the ways in, the number read straight
//! to the field's own kind, refusal, the range, back, and one change for one
//! number put in.

use std::time::Duration;

use super::value::{FINE, HAIR, Lone};
use super::*;
use crate::edit::Edit;
use crate::style::PseudoClasses;
use crate::tree::{Content, DOUBLE_CLICK_TIME, DragNumber, Engagement, Response};

/// Every frame's length unless a test says otherwise.
const FRAME: Duration = Duration::from_millis(16);

/// Seventeen significant digits: more than an `f32` holds, as many as an
/// `f64` needs to name each of its values.
const SEVENTEEN: &str = "0.12345678901234567";

/// A frame's text input of `edits`, [`FRAME`] long.
fn typed(edits: impl IntoIterator<Item = Edit>) -> TextInput {
    TextInput {
        dt: FRAME,
        edits: edits.into_iter().collect(),
        clipboard: Vec::new(),
    }
}

/// The text the drag-value `key` shows this frame: its number, or the line
/// being typed.
fn shown(ui: &Ui, key: NodeKey) -> String {
    children_of(ui, key)
        .into_iter()
        .find_map(|child| {
            let node = ui.nodes.iter().find(|node| node.key == child)?;
            match node.content {
                Content::Text { start, end } => Some(ui.text[start..end].to_owned()),
                _ => None,
            }
        })
        .expect("the drag-value shows a text")
}

/// Whether `key` was built this frame with `:refused`.
fn refused(ui: &Ui, key: NodeKey) -> bool {
    ui.nodes
        .iter()
        .find(|node| node.key == key)
        .expect("built")
        .pseudo
        .contains(PseudoClasses::REFUSED)
}

impl<N: DragNumber> Lone<N> {
    /// A still frame with `nav`, [`FRAME`] long.
    fn nav(&mut self, nav: NavInput) -> Response {
        self.frame_with_text(idle(), nav, typed([]))
    }

    /// A frame that types `text` over the selection.
    fn type_text(&mut self, text: &str) -> Response {
        self.frame_with_text(
            idle(),
            NavInput::default(),
            typed([Edit::Insert(text.to_owned())]),
        )
    }

    /// A click that does not move, at `on`, each frame `dt` long; returns the
    /// press frame's response.
    fn click_at(&mut self, on: Vec2, dt: Duration) -> Response {
        let text = || TextInput {
            dt,
            ..TextInput::default()
        };
        let pressed = self.frame_with_text(press(on), NavInput::default(), text());
        self.frame_with_text(release(on), NavInput::default(), text());
        pressed
    }

    /// Focus landed by the keyboard and then accept, which opens it; returns
    /// the accept frame's response.
    fn open_by_accept(&mut self) -> Response {
        self.nav(NavInput::NAVIGATION);
        let opened = self.nav(NavInput::ACCEPT);
        assert_eq!(
            opened.engagement,
            Engagement::Began,
            "accept did not engage"
        );
        opened
    }

    /// Opens it, types `text` and accepts, counting the frames that reported
    /// a change; returns that count and the accept frame's response.
    fn enter(&mut self, text: &str) -> (usize, Response) {
        let mut changes = usize::from(self.open_by_accept().changed);
        changes += usize::from(self.type_text(text).changed);
        let accepted = self.nav(NavInput::ACCEPT);
        changes += usize::from(accepted.changed);
        (changes, accepted)
    }
}

/// **A double-click opens a drag-value for typing**, holding every digit of
/// its value — not the step's six decimals — all of it selected, so what is
/// typed replaces it; one click alone engages it for stepping, as before, and
/// a second click later than [`DOUBLE_CLICK_TIME`] is not a double-click.
#[test]
fn a_double_click_opens_it_for_typing_with_every_digit_selected() {
    let mut lone = Lone::new(FINE, f64::MIN..=f64::MAX, HAIR, HAIR);
    let on = lone.centre();
    let slow = DOUBLE_CLICK_TIME + FRAME;
    lone.click_at(on, slow);
    assert!(!lone.ui.text_editing(), "one click opened it for typing");
    assert!(lone.ui.engaged().is_some(), "one click did not engage it");
    lone.click_at(on, slow);
    assert!(
        !lone.ui.text_editing(),
        "a second click after the double-click time opened it"
    );
    lone.click_at(on, FRAME);
    assert!(
        !lone.ui.text_editing(),
        "a click soon after a slow one's release opened it"
    );

    lone.click_at(on, FRAME);
    let opened = lone.nav(NavInput::default());
    assert!(lone.ui.text_editing(), "the double-click did not open it");
    let digits = FINE.to_string();
    assert_eq!(shown(&lone.ui, opened.key), digits, "not every digit");
    let len = digits.chars().count();
    assert_eq!(
        lone.ui.text_caret(opened.key),
        Some((len, 0)),
        "the text is not all selected"
    );

    let replaced = lone.type_text("2");
    assert_eq!(
        shown(&lone.ui, replaced.key),
        "2",
        "typing did not replace it"
    );
    assert_eq!(lone.value.to_bits(), FINE.to_bits(), "typing put it in");
}

/// **Accept on a focused drag-value opens it for typing** with its text all
/// selected, and accept again puts the typed number in.
#[test]
fn accept_on_a_focused_drag_value_opens_it_for_typing() {
    let mut lone = Lone::new(1.0, -10.0..=10.0, 0.5, 0.5);
    let opened = lone.open_by_accept();
    assert!(lone.ui.text_editing(), "accept did not open it for typing");
    assert_eq!(lone.ui.text_caret(opened.key), Some((1, 0)));
    lone.type_text("7.5");
    let accepted = lone.nav(NavInput::ACCEPT);
    assert_eq!(accepted.engagement, Engagement::Committed);
    assert_eq!(lone.value, 7.5);
    assert!(!lone.ui.text_editing(), "accept did not close it");
    assert_eq!(shown(&lone.ui, accepted.key), "7.5");
}

/// **A typed `f64` of seventeen significant digits goes in bit for bit** — the
/// `f64` the text names, which the nearest `f32` is not — and a value opened
/// and accepted untouched comes back as itself, because its text holds every
/// digit.
#[test]
fn a_typed_f64_of_seventeen_significant_digits_goes_in_bit_for_bit() {
    let want: f64 = SEVENTEEN.parse().expect("a number");
    let narrowed: f32 = SEVENTEEN.parse().expect("a number");
    assert_ne!(want.to_bits(), f64::from(narrowed).to_bits());

    let mut lone = Lone::new(0.0, f64::MIN..=f64::MAX, HAIR, HAIR);
    let (changes, _) = lone.enter(SEVENTEEN);
    assert_eq!(changes, 1);
    assert_eq!(
        lone.value.to_bits(),
        want.to_bits(),
        "{:e} is not {want:e}",
        lone.value
    );

    let mut lone = Lone::new(FINE, f64::MIN..=f64::MAX, HAIR, HAIR);
    lone.open_by_accept();
    let accepted = lone.nav(NavInput::ACCEPT);
    assert!(!accepted.changed, "an untouched value changed");
    assert_eq!(lone.value.to_bits(), FINE.to_bits());
}

/// **A whole number near its type's ends goes in exactly, and one past them
/// is refused**: the neighbours of `i64::MAX` and `i64::MIN` and `u64::MAX`,
/// none of which an `f64` holds, and the first number past each end.
#[test]
fn a_whole_number_near_its_ends_goes_in_exactly_and_past_them_is_refused() {
    let mut lone = Lone::new(0_i64, i64::MIN..=i64::MAX, 1.0, 1);
    for want in [i64::MAX - 1, i64::MIN + 1, i64::MAX, i64::MIN] {
        lone.enter(&want.to_string());
        assert_eq!(lone.value, want, "{want} did not go in exactly");
    }
    for past in ["9223372036854775808", "-9223372036854775809"] {
        let (changes, accepted) = lone.enter(past);
        assert_eq!(changes, 0, "{past} changed it");
        assert_eq!(lone.value, i64::MIN, "{past} was not refused");
        assert!(refused(&lone.ui, accepted.key));
        lone.nav(NavInput::BACK);
    }

    let mut lone = Lone::new(0_u64, u64::MIN..=u64::MAX, 1.0, 1);
    lone.enter(&(u64::MAX - 1).to_string());
    assert_eq!(lone.value, u64::MAX - 1);
    for past in ["18446744073709551616", "-1"] {
        let (changes, _) = lone.enter(past);
        assert_eq!(changes, 0, "{past} changed it");
        assert_eq!(lone.value, u64::MAX - 1, "{past} was not refused");
        lone.nav(NavInput::BACK);
    }
}

/// **A whole number refuses a fraction and an exponent**, even one that
/// lands on a whole number.
#[test]
fn a_whole_number_refuses_a_fraction_and_an_exponent() {
    let mut lone = Lone::new(4_i64, i64::MIN..=i64::MAX, 1.0, 1);
    for text in ["1.5", "2.0", "1e3", "0x10"] {
        let (changes, _) = lone.enter(text);
        assert_eq!(changes, 0, "{text} changed it");
        assert_eq!(lone.value, 4, "{text} was not refused");
        lone.nav(NavInput::BACK);
    }
    lone.enter(" -12 ");
    assert_eq!(lone.value, -12, "white space around a number refused it");
}

/// **Text that spells no number is refused and the field stays open**: the
/// value stays, nothing changes, `:refused` draws the red border until the
/// next edit, and a number typed then goes in. A float that is not finite is
/// no number either.
#[test]
fn text_that_spells_no_number_is_refused_and_the_field_stays_open() {
    let mut lone = Lone::new(1.0, f64::MIN..=f64::MAX, 0.5, 0.5);
    for text in ["1..2", "inf", "NaN"] {
        let (changes, accepted) = lone.enter(text);
        assert_eq!(changes, 0, "{text:?} changed it");
        assert_eq!(lone.value, 1.0, "{text:?} was not refused");
        assert!(lone.ui.text_editing(), "{text:?} closed the field");
        assert!(refused(&lone.ui, accepted.key), "{text:?} set no :refused");
        lone.nav(NavInput::BACK);
    }

    lone.enter("x");
    let still = lone.nav(NavInput::default());
    assert!(refused(&lone.ui, still.key), ":refused lasted one frame");
    assert_eq!(
        style_of(&lone.ui, still.key).border_color,
        linear("#e5484d")
    );
    let mended = lone.frame_with_text(
        idle(),
        NavInput::default(),
        typed([Edit::Backspace, Edit::Insert("3".to_owned())]),
    );
    assert!(
        !refused(&lone.ui, mended.key),
        "an edit did not clear :refused"
    );
    let accepted = lone.nav(NavInput::ACCEPT);
    assert!(accepted.changed);
    assert_eq!(lone.value, 3.0, "the mended number did not go in");
}

/// **A click elsewhere puts the typed number in, and drops text it would
/// refuse**, closing the field either way.
#[test]
fn a_click_elsewhere_puts_the_number_in_or_drops_refused_text() {
    let away = Vec2::splat(PAGE - 1.0);
    let mut lone = Lone::new(1.0, f64::MIN..=f64::MAX, 0.5, 0.5);
    lone.open_by_accept();
    lone.type_text("2.25");
    lone.click_at(away, FRAME);
    assert_eq!(lone.value, 2.25, "the click elsewhere lost the number");
    assert!(!lone.ui.text_editing());

    lone.open_by_accept();
    lone.type_text("two");
    lone.click_at(away, FRAME);
    assert_eq!(lone.value, 2.25, "refused text went in");
    assert!(!lone.ui.text_editing(), "the field stayed open");
}

/// **A typed number is held inside the range.**
#[test]
fn the_range_holds_a_typed_number() {
    let mut lone = Lone::new(0.0, -5.0..=5.0, 0.5, 0.5);
    lone.enter("12");
    assert_eq!(lone.value, 5.0, "past the end");
    lone.enter("-1e9");
    assert_eq!(lone.value, -5.0, "before the start");

    let mut lone = Lone::new(0_i64, -10..=10, 1.0, 1);
    lone.enter("99");
    assert_eq!(lone.value, 10, "a whole number past the end");
}

/// **Back puts back the value it engaged with, bit for bit**: after typing,
/// after steps — which move the value as they always have, from the typed
/// number — and after a refused accept, whose engagement ended once.
#[test]
fn back_puts_back_the_value_it_engaged_with_bit_for_bit() {
    let mut lone = Lone::new(FINE, f64::MIN..=f64::MAX, HAIR, HAIR);
    lone.open_by_accept();
    lone.type_text("5");
    let stepped = lone.nav(UP);
    assert!(stepped.changed, "a step while typing did not move it");
    assert_eq!(lone.value.to_bits(), (5.0 + HAIR).to_bits());
    lone.nav(DOWN);
    lone.nav(DOWN);
    lone.nav(RIGHT);
    assert_eq!(
        lone.value.to_bits(),
        (5.0 + HAIR - HAIR - HAIR + HAIR).to_bits()
    );
    let cancelled = lone.nav(NavInput::BACK);
    assert_eq!(cancelled.engagement, Engagement::Cancelled);
    assert_eq!(lone.value.to_bits(), FINE.to_bits(), "back after typing");
    assert!(!lone.ui.text_editing());

    lone.open_by_accept();
    lone.type_text("nonsense");
    lone.nav(NavInput::ACCEPT);
    assert!(lone.ui.text_editing(), "the refused accept closed it");
    lone.type_text("7");
    lone.nav(UP);
    lone.nav(NavInput::BACK);
    assert_eq!(
        lone.value.to_bits(),
        FINE.to_bits(),
        "back after a refused accept"
    );
}

/// **One number put in is one change**, reported the frame it goes in — not
/// a change per frame typed — and none for a number that is the value
/// already.
#[test]
fn one_number_put_in_is_one_change() {
    let mut lone = Lone::new(0.0, f64::MIN..=f64::MAX, 0.5, 0.5);
    let mut changes = usize::from(lone.open_by_accept().changed);
    for text in ["1", "2", "3"] {
        changes += usize::from(lone.type_text(text).changed);
    }
    assert_eq!(lone.value, 0.0, "typing put the number in");
    let accepted = lone.nav(NavInput::ACCEPT);
    assert!(accepted.changed);
    changes += usize::from(accepted.changed);
    changes += usize::from(lone.nav(NavInput::default()).changed);
    assert_eq!(changes, 1);
    assert_eq!(lone.value, 123.0);

    let (changes, _) = lone.enter("123.0");
    assert_eq!(changes, 0, "the same number again changed it");
}
