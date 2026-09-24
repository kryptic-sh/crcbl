//! The tap, hold and double-tap patterns: what a press *was*, decided by how
//! long it lasted and how soon the next one came.
//!
//! # On the tick clock
//!
//! Like [`Repeat`](crate::Repeat), every pattern is timed on the clock
//! [`ActionMap::begin_tick`] advances, never on wall time: an edge that arrives
//! between two ticks is stamped with the clock as the earlier one left it, so a
//! scripted input sequence fires on the same ticks every run. A pattern firing
//! is an edge — [`ActionMap::tapped`], [`ActionMap::hold_fired`] and
//! [`ActionMap::double_tapped`] are true for the rest of the tick it fired on,
//! and `begin_tick` clears them.
//!
//! # When each one fires
//!
//! - A **tap** fires on the release of a press that lasted at most
//!   [`Tap::time`].
//! - A **hold** fires once, on the first tick a press has lasted at least
//!   [`Hold::time`]; [`ActionMap::hold_progress`] reports how far along it is.
//! - A **double tap** fires on the second press, when the first lasted at most
//!   [`DoubleTap::tap_time`] and the second went down at most
//!   [`DoubleTap::window`] after the first came up. One made with
//!   [`DoubleTap::on_release`] fires on the second press's release instead,
//!   and only if that press too lasted at most [`DoubleTap::tap_time`]: two
//!   taps, not a tap and a hold.
//!
//! **The window's end is inclusive.** A first tap stops waiting only once
//! `elapsed - released_at > window`, checked before the tick's press is read,
//! so a second press landing exactly [`DoubleTap::window`] after the release
//! still finds the first tap waiting and completes the double; a tap that
//! waits fires on the first tick past the window, never on a tick a second
//! press could still have claimed.
//!
//! # One press is one gesture
//!
//! The patterns on an action share its presses, and each press means one thing:
//!
//! - **A press that fired its hold is not a tap**, however soon it is released.
//!   A hold is checked at the start of each tick, before that tick's events, so
//!   a press released on the very tick it reaches [`Hold::time`] is a hold.
//! - **A press that completed a double tap is spent**: it is neither a tap nor
//!   a hold, and cannot start another double tap. That holds for an
//!   [`DoubleTap::on_release`] double too, from the moment the second press
//!   goes down: the press is the double's whether or not its release fires it.
//! - **With a double tap attached, a tap waits.** A tap that could still be the
//!   first half of a double tap fires only once [`DoubleTap::window`] passes
//!   with no second press, and not at all if one comes. That is the only way
//!   one key can carry both a single and a double tap; a game that wants the
//!   instant release reads [`ActionMap::just_released`] instead.
//!
//! # What cancels a pattern
//!
//! Letting go before [`Hold::time`] cancels the hold, and a press after the
//! window lets the double tap lapse. Beyond those, anything that interrupts the
//! action cancels whatever is in flight — the press in progress and any first
//! tap waiting for its second: a context pushed or popped
//! ([`ActionMap::push_context`], [`ActionMap::pop_context`]), held input
//! withheld ([`ActionMap::suppress_held`],
//! [`ActionMap::suppress_held_action`]), the action being disabled or rebound,
//! a pattern attached or detached, or the game asking with
//! [`ActionMap::cancel_patterns`]. A press that was down when it was cancelled
//! is spent until it is released, so it fires nothing, even if it outlives the
//! change.
//!
//! # What "down" means per kind
//!
//! What the repeat pattern calls held: a button while it is down, an axis while
//! it is off zero. Unlike the repeat pattern, a change of an axis's direction is
//! not a new press — the patterns time the press, not where it points.

use super::{ActionMap, ActionMapError, ActionValue};
use crate::repeat::Held;

/// The longest a press can last and still be a tap, in seconds.
///
/// Below [`HOLD_TIME`], so that with both patterns at their defaults a press
/// between the two is neither: the player who let go at the last moment gets
/// nothing rather than whichever of the two the frame rate picked.
pub const TAP_TIME: f32 = 0.25;

/// How long a press lasts before a hold fires, in seconds.
///
/// The `Hold(400, …)` the input plan's binding sketch used as its example:
/// long enough that a deliberate tap never becomes a hold.
pub const HOLD_TIME: f32 = 0.4;

/// The longest gap between the first tap's release and the second press that
/// still makes a double tap, in seconds.
///
/// Also how long a single tap waits when a double tap shares its action, so it
/// is kept short: the whole of it is latency on every single tap.
pub const DOUBLE_TAP_WINDOW: f32 = 0.25;

/// Whether `seconds` can time a pattern: a zero or non-finite time would fire
/// on every press, or never.
fn is_duration(seconds: f32) -> bool {
    seconds.is_finite() && seconds > 0.0
}

/// A tap: a press released within [`Tap::time`].
///
/// The field is private so the only taps that exist are ones the evaluator can
/// time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    time: f32,
}

impl Tap {
    /// A tap of at most `time` seconds, or `None` unless `time` is finite and
    /// positive.
    #[must_use]
    pub fn new(time: f32) -> Option<Self> {
        is_duration(time).then_some(Self { time })
    }

    /// The longest a press can last and still be a tap, in seconds.
    #[must_use]
    pub const fn time(self) -> f32 {
        self.time
    }
}

impl Default for Tap {
    /// A tap of at most [`TAP_TIME`].
    fn default() -> Self {
        Self { time: TAP_TIME }
    }
}

/// A hold: a press that lasts [`Hold::time`].
///
/// The field is private so the only holds that exist are ones the evaluator can
/// time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    time: f32,
}

impl Hold {
    /// A hold of `time` seconds, or `None` unless `time` is finite and
    /// positive.
    #[must_use]
    pub fn new(time: f32) -> Option<Self> {
        is_duration(time).then_some(Self { time })
    }

    /// How long a press lasts before the hold fires, in seconds.
    #[must_use]
    pub const fn time(self) -> f32 {
        self.time
    }
}

impl Default for Hold {
    /// A hold of [`HOLD_TIME`].
    fn default() -> Self {
        Self { time: HOLD_TIME }
    }
}

/// A double tap: two presses, the first no longer than
/// [`DoubleTap::tap_time`], the second going down within
/// [`DoubleTap::window`] of the first coming up.
///
/// Fires on the second press unless made with [`DoubleTap::on_release`].
///
/// The fields are private so the only double taps that exist are ones the
/// evaluator can time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoubleTap {
    tap_time: f32,
    window: f32,
    on_release: bool,
}

impl DoubleTap {
    /// A double tap that fires on the second press, or `None` unless both times
    /// are finite and positive.
    #[must_use]
    pub fn new(tap_time: f32, window: f32) -> Option<Self> {
        (is_duration(tap_time) && is_duration(window)).then_some(Self {
            tap_time,
            window,
            on_release: false,
        })
    }

    /// This double tap, firing on the second press's **release** instead of
    /// on the press — and only if that press lasted at most
    /// [`DoubleTap::tap_time`], as the first had to. See the module docs.
    #[must_use]
    pub const fn on_release(self) -> Self {
        Self {
            on_release: true,
            ..self
        }
    }

    /// Whether it fires on the second press's release rather than the press.
    #[must_use]
    pub const fn fires_on_release(self) -> bool {
        self.on_release
    }

    /// The longest the first press can last, in seconds — and, for a double
    /// tap made with [`DoubleTap::on_release`], the second.
    #[must_use]
    pub const fn tap_time(self) -> f32 {
        self.tap_time
    }

    /// The longest gap between the first release and the second press, in
    /// seconds.
    #[must_use]
    pub const fn window(self) -> f32 {
        self.window
    }
}

impl Default for DoubleTap {
    /// A first press of at most [`TAP_TIME`], and a gap of at most
    /// [`DOUBLE_TAP_WINDOW`], firing on the second press.
    fn default() -> Self {
        Self {
            tap_time: TAP_TIME,
            window: DOUBLE_TAP_WINDOW,
            on_release: false,
        }
    }
}

/// Where the action's current press is.
#[derive(Clone, Copy, Debug, Default)]
enum Press {
    #[default]
    Up,
    /// Down since the clock read `at`.
    Down { at: f64, hold_fired: bool },
    /// Down, but it can fire nothing more: it completed a double tap, or was
    /// cancelled while down.
    Spent,
    /// Down since the clock read `at`, as the second press of a
    /// [`DoubleTap::on_release`] double: spent for everything but that double,
    /// which its release fires if it was short enough.
    Second { at: f64 },
}

/// A first tap waiting for its second.
#[derive(Clone, Copy, Debug)]
struct Pending {
    released_at: f64,
    /// Whether it was also a [`Tap`], owed if no second press comes.
    tap: bool,
}

/// A slot's tap, hold and double-tap patterns and where its presses are in
/// them.
///
/// The press is tracked whether or not a pattern is attached, so attaching
/// one only ever has to cancel, never to reconstruct.
#[derive(Clone, Debug, Default)]
pub(crate) struct PatternState {
    tap: Option<Tap>,
    hold: Option<Hold>,
    double_tap: Option<DoubleTap>,
    press: Press,
    pending: Option<Pending>,
    tapped: bool,
    hold_fired: bool,
    double_tapped: bool,
}

impl PatternState {
    /// Clear what fired on the last tick — [`ActionMap::begin_tick`]'s share.
    pub(crate) fn clear_fired(&mut self) {
        self.tapped = false;
        self.hold_fired = false;
        self.double_tapped = false;
    }

    /// Drop everything in flight: the press in progress is spent until it is
    /// released, and a first tap stops waiting. What already fired this tick
    /// stays fired.
    pub(crate) fn cancel(&mut self) {
        if !matches!(self.press, Press::Up) {
            self.press = Press::Spent;
        }
        self.pending = None;
    }

    /// Back to idle, as the action's value is: nothing down, nothing fired.
    pub(crate) fn reset(&mut self) {
        self.press = Press::Up;
        self.pending = None;
        self.clear_fired();
    }

    /// Advance the patterns against the slot's freshly resolved value.
    pub(crate) fn update(&mut self, value: &ActionValue, elapsed: f64) {
        if let (Some(pending), Some(double_tap)) = (self.pending, self.double_tap)
            && elapsed - pending.released_at > f64::from(double_tap.window)
        {
            // No second press in time: the first stands alone.
            self.pending = None;
            self.tapped |= pending.tap;
        }

        let down = Held::of(value).is_some();
        match (self.press, down) {
            (Press::Up, true) => {
                // `pending` is only ever still set inside its window, and only
                // while a double tap is attached.
                self.press = if self.pending.take().is_some() {
                    if self.double_tap.is_some_and(DoubleTap::fires_on_release) {
                        Press::Second { at: elapsed }
                    } else {
                        self.double_tapped = true;
                        Press::Spent
                    }
                } else {
                    Press::Down {
                        at: elapsed,
                        hold_fired: false,
                    }
                };
            }
            (
                Press::Down {
                    at,
                    hold_fired: false,
                },
                true,
            ) => {
                if self
                    .hold
                    .is_some_and(|hold| elapsed - at >= f64::from(hold.time))
                {
                    self.hold_fired = true;
                    self.press = Press::Down {
                        at,
                        hold_fired: true,
                    };
                }
            }
            (Press::Down { at, hold_fired }, false) => {
                self.press = Press::Up;
                if hold_fired {
                    return;
                }
                let lasted = elapsed - at;
                let tap = self.tap.is_some_and(|tap| lasted <= f64::from(tap.time));
                let first_of_two = self
                    .double_tap
                    .is_some_and(|double_tap| lasted <= f64::from(double_tap.tap_time));
                if first_of_two {
                    self.pending = Some(Pending {
                        released_at: elapsed,
                        tap,
                    });
                } else {
                    self.tapped |= tap;
                }
            }
            (Press::Second { at }, false) => {
                self.press = Press::Up;
                self.double_tapped |= self
                    .double_tap
                    .is_some_and(|double_tap| elapsed - at <= f64::from(double_tap.tap_time));
            }
            (Press::Spent, false) => self.press = Press::Up,
            (Press::Up, false)
            | (Press::Down { .. } | Press::Spent | Press::Second { .. }, true) => {}
        }
    }

    /// How far the press in progress is toward its hold, `0.0` to `1.0`.
    fn hold_progress(&self, elapsed: f64) -> f32 {
        match (self.press, self.hold) {
            (
                Press::Down {
                    hold_fired: true, ..
                },
                _,
            ) => 1.0,
            (Press::Down { at, .. }, Some(hold)) => {
                ((elapsed - at) / f64::from(hold.time)).min(1.0) as f32
            }
            _ => 0.0,
        }
    }
}

impl ActionMap {
    /// Attach a tap to an action, or detach it with `None`.
    ///
    /// Attaching or detaching cancels whatever the action's patterns had in
    /// flight, so a press already down when this is called fires nothing — see
    /// the module docs.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn set_tap(&mut self, name: &str, tap: Option<Tap>) -> Result<(), ActionMapError> {
        let patterns = self.patterns_mut(name)?;
        patterns.tap = tap;
        patterns.cancel();
        Ok(())
    }

    /// Attach a hold to an action, or detach it with `None`. Cancels what was
    /// in flight, as [`ActionMap::set_tap`] does.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn set_hold(&mut self, name: &str, hold: Option<Hold>) -> Result<(), ActionMapError> {
        let patterns = self.patterns_mut(name)?;
        patterns.hold = hold;
        patterns.cancel();
        Ok(())
    }

    /// Attach a double tap to an action, or detach it with `None`. Cancels what
    /// was in flight, as [`ActionMap::set_tap`] does.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn set_double_tap(
        &mut self,
        name: &str,
        double_tap: Option<DoubleTap>,
    ) -> Result<(), ActionMapError> {
        let patterns = self.patterns_mut(name)?;
        patterns.double_tap = double_tap;
        patterns.cancel();
        Ok(())
    }

    /// Cancel whatever an action's patterns have in flight, exactly as a
    /// context push does: the press in progress is spent until it is released,
    /// and a first tap waiting for its second is dropped. The action's value is
    /// untouched — a held button stays held — and with nothing in flight this
    /// changes nothing.
    ///
    /// For a press the game decides was something else: a Z held while the
    /// wheel turns was a Z+wheel, not the first half of a Z double tap.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn cancel_patterns(&mut self, name: &str) -> Result<(), ActionMapError> {
        self.patterns_mut(name)?.cancel();
        Ok(())
    }

    /// The tap attached to an action, if any.
    #[must_use]
    pub fn tap(&self, name: &str) -> Option<Tap> {
        self.patterns(name)?.tap
    }

    /// The hold attached to an action, if any.
    #[must_use]
    pub fn hold(&self, name: &str) -> Option<Hold> {
        self.patterns(name)?.hold
    }

    /// The double tap attached to an action, if any.
    #[must_use]
    pub fn double_tap(&self, name: &str) -> Option<DoubleTap> {
        self.patterns(name)?.double_tap
    }

    /// Whether an action's tap fired on this tick. `false` for an action with
    /// no tap, or none declared.
    #[must_use]
    pub fn tapped(&self, name: &str) -> bool {
        self.patterns(name).is_some_and(|patterns| patterns.tapped)
    }

    /// Whether an action's hold fired on this tick. `false` for an action with
    /// no hold, or none declared.
    #[must_use]
    pub fn hold_fired(&self, name: &str) -> bool {
        self.patterns(name)
            .is_some_and(|patterns| patterns.hold_fired)
    }

    /// Whether an action's double tap fired on this tick. `false` for an
    /// action with no double tap, or none declared.
    #[must_use]
    pub fn double_tapped(&self, name: &str) -> bool {
        self.patterns(name)
            .is_some_and(|patterns| patterns.double_tapped)
    }

    /// How far the press in progress is toward the action's hold, from `0.0`
    /// at the press to `1.0` once it fired — the fill of a hold-to-interact
    /// ring. `0.0` while nothing is down, for a spent or cancelled press, for
    /// an action with no hold, or none declared.
    #[must_use]
    pub fn hold_progress(&self, name: &str) -> f32 {
        self.patterns(name)
            .map_or(0.0, |patterns| patterns.hold_progress(self.elapsed))
    }

    fn patterns(&self, name: &str) -> Option<&PatternState> {
        let &idx = self.name_to_idx.get(name)?;
        Some(&self.slots[idx].patterns)
    }

    fn patterns_mut(&mut self, name: &str) -> Result<&mut PatternState, ActionMapError> {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return Err(ActionMapError::UnknownAction(name.to_owned()));
        };
        Ok(&mut self.slots[idx].patterns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, Binding, Repeat};
    use crcbl_core::input::KeyCode;

    /// Sixteen ticks a second: `0.0625` is exact in binary, so a clock summed
    /// from it lands on every threshold below exactly, and a boundary test
    /// checks the comparison rather than rounding.
    const TICK: f32 = 0.0625;

    /// A quarter second: four ticks.
    const QUARTER: f32 = 0.25;

    const KEY: KeyCode = KeyCode::Space;

    fn map_with(attach: impl FnOnce(&mut ActionMap)) -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "act".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KEY)],
        });
        attach(&mut map);
        map
    }

    fn tap(time: f32) -> Option<Tap> {
        Some(Tap::new(time).expect("a runnable tap"))
    }

    fn hold(time: f32) -> Option<Hold> {
        Some(Hold::new(time).expect("a runnable hold"))
    }

    fn double_tap(tap_time: f32, window: f32) -> Option<DoubleTap> {
        Some(DoubleTap::new(tap_time, window).expect("a runnable double tap"))
    }

    /// The ticks each pattern fired on.
    #[derive(Debug, Default, PartialEq)]
    struct Fires {
        tap: Vec<u32>,
        hold: Vec<u32>,
        double_tap: Vec<u32>,
    }

    /// Run `ticks` ticks, pressing (`true`) or releasing the key on the ticks
    /// `edges` names, just after that tick begins.
    fn fires(map: &mut ActionMap, edges: &[(u32, bool)], ticks: u32) -> Fires {
        let mut fires = Fires::default();
        for tick in 0..ticks {
            map.begin_tick(TICK);
            for &(_, down) in edges.iter().filter(|(at, _)| *at == tick) {
                map.key_event(KEY, down);
            }
            if map.tapped("act") {
                fires.tap.push(tick);
            }
            if map.hold_fired("act") {
                fires.hold.push(tick);
            }
            if map.double_tapped("act") {
                fires.double_tap.push(tick);
            }
        }
        fires
    }

    fn taps(tap: Vec<u32>) -> Fires {
        Fires {
            tap,
            ..Fires::default()
        }
    }

    /// **A tap fires on the release, up to and including its time.**
    #[test]
    fn a_tap_fires_on_a_release_within_its_time() {
        for (released, expected) in [(3, vec![3]), (4, vec![4]), (5, vec![])] {
            let mut map = map_with(|map| map.set_tap("act", tap(QUARTER)).expect("declared"));
            assert_eq!(
                fires(&mut map, &[(0, true), (released, false)], 8),
                taps(expected),
                "released after {released} ticks",
            );
        }
    }

    /// **A hold fires once, on the tick the press reaches its time**, and a
    /// release before then cancels it.
    #[test]
    fn a_hold_fires_once_at_its_time_and_a_release_cancels_it() {
        let mut map = map_with(|map| map.set_hold("act", hold(QUARTER)).expect("declared"));
        let held = fires(&mut map, &[(0, true)], 10);
        assert_eq!(
            held,
            Fires {
                hold: vec![4],
                ..Fires::default()
            },
        );

        let mut map = map_with(|map| map.set_hold("act", hold(QUARTER)).expect("declared"));
        let let_go = fires(&mut map, &[(0, true), (3, false)], 10);
        assert_eq!(let_go, Fires::default(), "released one tick short");
    }

    /// The progress a hold-to-interact ring fills with: the fraction of the
    /// hold time, `1.0` from the fire, and back to `0.0` on release.
    #[test]
    fn hold_progress_fills_to_one_and_empties_on_release() {
        let mut map = map_with(|map| map.set_hold("act", hold(QUARTER)).expect("declared"));
        map.begin_tick(TICK);
        assert_eq!(map.hold_progress("act"), 0.0, "nothing down");
        map.key_event(KEY, true);
        let mut progress = vec![map.hold_progress("act")];
        for _ in 0..5 {
            map.begin_tick(TICK);
            progress.push(map.hold_progress("act"));
        }
        assert_eq!(progress, [0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
        map.key_event(KEY, false);
        assert_eq!(map.hold_progress("act"), 0.0);
        assert_eq!(map.hold_progress("nope"), 0.0);
    }

    /// **A press that fired its hold is not a tap**, even when the tap's time
    /// is the longer of the two and the release comes inside it.
    #[test]
    fn a_press_that_held_does_not_tap() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(0.5)).expect("declared");
            map.set_hold("act", hold(QUARTER)).expect("declared");
        });
        assert_eq!(
            fires(&mut map, &[(0, true), (6, false)], 10),
            Fires {
                hold: vec![4],
                ..Fires::default()
            },
            "released at 0.375 s, inside the tap's 0.5 s",
        );
    }

    /// Released on the very tick the hold fires: the hold wins, since it is
    /// checked as the tick begins.
    #[test]
    fn a_release_on_the_hold_tick_is_a_hold() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_hold("act", hold(QUARTER)).expect("declared");
        });
        assert_eq!(
            fires(&mut map, &[(0, true), (4, false)], 8),
            Fires {
                hold: vec![4],
                ..Fires::default()
            },
        );
    }

    /// Both patterns at their defaults: a short press taps, a long one holds,
    /// and one between the two is neither.
    #[test]
    fn tap_and_hold_split_presses_by_length() {
        for (released, expected) in [
            (3, taps(vec![3])),
            (5, Fires::default()),
            (
                8,
                Fires {
                    hold: vec![7],
                    ..Fires::default()
                },
            ),
        ] {
            let mut map = map_with(|map| {
                map.set_tap("act", Some(Tap::default())).expect("declared");
                map.set_hold("act", Some(Hold::default()))
                    .expect("declared");
            });
            assert_eq!(
                fires(&mut map, &[(0, true), (released, false)], 12),
                expected,
                "released after {released} ticks",
            );
        }
    }

    /// **A double tap fires on the second press, up to and including the end
    /// of its window**, and not after.
    #[test]
    fn a_double_tap_fires_on_a_second_press_inside_the_window() {
        for (gap, expected) in [(3, vec![5]), (4, vec![6]), (5, vec![])] {
            let mut map = map_with(|map| {
                map.set_double_tap("act", double_tap(QUARTER, QUARTER))
                    .expect("declared");
            });
            let second = 2 + gap;
            let fired = fires(
                &mut map,
                &[(0, true), (2, false), (second, true), (second + 1, false)],
                14,
            );
            assert_eq!(
                fired,
                Fires {
                    double_tap: expected,
                    ..Fires::default()
                },
                "second press {gap} ticks after the first release",
            );
        }
    }

    /// A first press longer than the double tap's tap time starts nothing.
    #[test]
    fn a_long_first_press_does_not_start_a_double_tap() {
        let mut map = map_with(|map| {
            map.set_double_tap("act", double_tap(QUARTER, QUARTER))
                .expect("declared");
        });
        let fired = fires(&mut map, &[(0, true), (5, false), (6, true)], 10);
        assert_eq!(fired, Fires::default());
    }

    /// **With a double tap attached, a single tap waits out the window**, and
    /// a double tap swallows the tap it began with; the double's second press
    /// is spent, so its release does not tap either.
    #[test]
    fn a_tap_waits_for_the_double_tap_window() {
        let attach = |map: &mut ActionMap| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_double_tap("act", double_tap(QUARTER, QUARTER))
                .expect("declared");
        };

        let mut map = map_with(attach);
        let single = fires(&mut map, &[(0, true), (2, false)], 10);
        assert_eq!(
            single,
            taps(vec![7]),
            "released at tick 2, the window closes after tick 6",
        );

        let mut map = map_with(attach);
        let double = fires(
            &mut map,
            &[(0, true), (2, false), (4, true), (5, false)],
            12,
        );
        assert_eq!(
            double,
            Fires {
                double_tap: vec![4],
                ..Fires::default()
            },
        );
    }

    /// **Pushing a context over a held press cancels it**: the press lost to
    /// the context is not a tap, and the hold never fires.
    #[test]
    fn a_context_push_cancels_the_press_in_progress() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_hold("act", hold(QUARTER)).expect("declared");
        });
        map.declare_in(
            "menu",
            ActionDecl {
                name: "confirm".to_owned(),
                kind: ActionKind::Button,
                bindings: vec![Binding::Key(KEY)],
            },
        );
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.begin_tick(TICK);
        map.push_context("menu").expect("declared");
        assert!(!map.button_held("act"), "the menu took the key");
        assert!(!map.tapped("act"), "a press taken away is not a tap");
        for _ in 0..6 {
            map.begin_tick(TICK);
            assert!(!map.hold_fired("act"));
        }
    }

    /// A context that takes nothing from the action still cancels its press:
    /// the press outlives the change, spent, and fires neither tap nor hold.
    #[test]
    fn a_context_switch_spends_a_press_it_does_not_take() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_hold("act", hold(QUARTER)).expect("declared");
        });
        map.declare_in(
            "menu",
            ActionDecl {
                name: "other".to_owned(),
                kind: ActionKind::Button,
                bindings: vec![Binding::Key(KeyCode::Enter)],
            },
        );
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.push_context("menu").expect("declared");
        assert!(map.button_held("act"), "the key is still the action's");
        let mut fired = false;
        for _ in 0..6 {
            map.begin_tick(TICK);
            fired |= map.hold_fired("act");
        }
        map.key_event(KEY, false);
        fired |= map.tapped("act");
        assert!(!fired, "neither a hold nor a tap");
    }

    /// A first tap waiting for its second is dropped by a context switch.
    #[test]
    fn a_context_switch_drops_a_waiting_first_tap() {
        let mut map = map_with(|map| {
            map.set_double_tap("act", double_tap(QUARTER, QUARTER))
                .expect("declared");
        });
        map.declare_in(
            "menu",
            ActionDecl {
                name: "other".to_owned(),
                kind: ActionKind::Button,
                bindings: vec![Binding::Key(KeyCode::Enter)],
            },
        );
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        map.push_context("menu").expect("declared");
        map.pop_context("menu").expect("on top");
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        assert!(!map.double_tapped("act"));
    }

    /// Disabling the action mid-press cancels it: a key let go while the
    /// action was disabled is no tap once it is enabled again.
    #[test]
    fn disabling_cancels_the_press() {
        let mut map = map_with(|map| map.set_tap("act", tap(QUARTER)).expect("declared"));
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.set_enabled("act", false);
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        map.set_enabled("act", true);
        map.begin_tick(TICK);
        assert!(!map.tapped("act"), "released while disabled");
    }

    /// Attaching to a held action does not fire on its release.
    #[test]
    fn attaching_mid_press_fires_nothing() {
        let mut map = map_with(|_| {});
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.set_tap("act", tap(QUARTER)).expect("declared");
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        assert!(!map.tapped("act"));
        assert_eq!(map.tap("act"), tap(QUARTER));
        assert_eq!(map.hold("act"), None);
        assert_eq!(map.double_tap("act"), None);
        assert_eq!(
            map.set_hold("nope", None),
            Err(ActionMapError::UnknownAction("nope".to_owned())),
        );
    }

    /// Every fire is an edge: gone on the next tick.
    #[test]
    fn a_fire_lasts_one_tick() {
        let mut map = map_with(|map| map.set_tap("act", tap(QUARTER)).expect("declared"));
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.key_event(KEY, false);
        assert!(map.tapped("act"));
        map.begin_tick(TICK);
        assert!(!map.tapped("act"));
    }

    /// The ticks a repeat on the action pulsed on, over a tap, a double tap
    /// and a long hold.
    fn repeat_pulses(map: &mut ActionMap) -> Vec<u32> {
        let edges = [(0, true), (1, false), (3, true), (4, false), (6, true)];
        let mut pulses = Vec::new();
        for tick in 0..24 {
            map.begin_tick(TICK);
            for &(_, down) in edges.iter().filter(|(at, _)| *at == tick) {
                map.key_event(KEY, down);
            }
            if map.repeated("act") {
                pulses.push(tick);
            }
        }
        pulses
    }

    /// **The repeat schedule is untouched by patterns on the same action.**
    #[test]
    fn patterns_leave_repeat_alone() {
        let mut bare = map_with(|map| map.set_repeat("act", Some(Repeat::UI)).expect("declared"));
        let mut patterned = map_with(|map| {
            map.set_repeat("act", Some(Repeat::UI)).expect("declared");
            map.set_tap("act", Some(Tap::default())).expect("declared");
            map.set_hold("act", Some(Hold::default()))
                .expect("declared");
            map.set_double_tap("act", Some(DoubleTap::default()))
                .expect("declared");
        });
        let expected = repeat_pulses(&mut bare);
        // Each press pulses; the last is held from tick 6, so its first repeat
        // is 0.5 s / 0.0625 s = 8 ticks later.
        assert_eq!(expected[..4], [0, 3, 6, 14]);
        assert_eq!(repeat_pulses(&mut patterned), expected);
    }

    /// **`cancel_patterns` with nothing in flight changes nothing**: a press
    /// on the same tick taps as if it had never been called.
    #[test]
    fn cancel_patterns_with_nothing_in_flight_spends_nothing() {
        let mut map = map_with(|map| map.set_tap("act", tap(QUARTER)).expect("declared"));
        map.begin_tick(TICK);
        map.cancel_patterns("act").expect("declared");
        map.key_event(KEY, true);
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        assert!(map.tapped("act"));
        assert_eq!(
            map.cancel_patterns("nope"),
            Err(ActionMapError::UnknownAction("nope".to_owned())),
        );
    }

    /// **`cancel_patterns` drops a first tap waiting for its second**, as a
    /// context switch does: the next press starts a new gesture instead.
    #[test]
    fn cancel_patterns_drops_a_waiting_first_tap() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_double_tap("act", double_tap(QUARTER, QUARTER))
                .expect("declared");
        });
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        map.cancel_patterns("act").expect("declared");
        let fired = fires(&mut map, &[(0, true)], 8);
        assert_eq!(
            fired,
            Fires::default(),
            "neither the double nor the dropped tap fires"
        );
    }

    /// **`cancel_patterns` spends the press in progress** until it is
    /// released: no hold, no tap, and the button itself stays held.
    #[test]
    fn cancel_patterns_spends_the_press_in_progress() {
        let mut map = map_with(|map| {
            map.set_tap("act", tap(QUARTER)).expect("declared");
            map.set_hold("act", hold(QUARTER)).expect("declared");
        });
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.cancel_patterns("act").expect("declared");
        assert!(map.button_held("act"), "only the patterns are cancelled");
        let mut fired = false;
        for _ in 0..6 {
            map.begin_tick(TICK);
            fired |= map.hold_fired("act");
        }
        map.key_event(KEY, false);
        fired |= map.tapped("act");
        assert!(!fired, "neither a hold nor a tap");
        assert_eq!(fires(&mut map, &[(0, true), (1, false)], 2), taps(vec![1]));
    }

    fn released_double(tap_time: f32, window: f32) -> Option<DoubleTap> {
        double_tap(tap_time, window).map(DoubleTap::on_release)
    }

    fn doubles(double_tap: Vec<u32>) -> Fires {
        Fires {
            double_tap,
            ..Fires::default()
        }
    }

    /// **An on-release double tap fires when the second press comes up**, not
    /// when it goes down, and keeps the window's inclusive end.
    #[test]
    fn an_on_release_double_tap_fires_on_the_second_release() {
        for (gap, expected) in [(3, vec![7]), (4, vec![8]), (5, vec![])] {
            let mut map = map_with(|map| {
                map.set_double_tap("act", released_double(QUARTER, QUARTER))
                    .expect("declared");
            });
            let second = 2 + gap;
            let fired = fires(
                &mut map,
                &[(0, true), (2, false), (second, true), (second + 2, false)],
                16,
            );
            assert_eq!(
                fired,
                doubles(expected),
                "second press {gap} ticks after the first release"
            );
        }
    }

    /// **The second press must be a tap too**: up to and including the tap
    /// time it fires, one tick longer it is spent and fires nothing.
    #[test]
    fn an_on_release_double_tap_needs_a_short_second_press() {
        for (held, expected) in [(4, vec![7]), (5, vec![])] {
            let mut map = map_with(|map| {
                map.set_tap("act", tap(QUARTER)).expect("declared");
                map.set_hold("act", hold(QUARTER)).expect("declared");
                map.set_double_tap("act", released_double(QUARTER, QUARTER))
                    .expect("declared");
            });
            let fired = fires(
                &mut map,
                &[(0, true), (2, false), (3, true), (3 + held, false)],
                14,
            );
            assert_eq!(
                fired,
                doubles(expected),
                "second press held {held} ticks: never a hold or a tap either",
            );
        }
    }

    /// Cancelled between its press and its release, an on-release double tap
    /// does not fire.
    #[test]
    fn a_cancelled_second_press_does_not_fire_on_release() {
        let mut map = map_with(|map| {
            map.set_double_tap("act", released_double(QUARTER, QUARTER))
                .expect("declared");
        });
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        map.begin_tick(TICK);
        map.key_event(KEY, true);
        map.cancel_patterns("act").expect("declared");
        map.begin_tick(TICK);
        map.key_event(KEY, false);
        assert!(!map.double_tapped("act"));
    }

    /// The default fires on the press, as it always did, and `on_release`
    /// changes when it fires and nothing else.
    #[test]
    fn a_double_tap_fires_on_the_press_unless_asked_otherwise() {
        let press = DoubleTap::new(QUARTER, 0.5).expect("a runnable double tap");
        assert!(!press.fires_on_release());
        assert!(!DoubleTap::default().fires_on_release());
        let release = press.on_release();
        assert!(release.fires_on_release());
        assert_eq!(
            (release.tap_time(), release.window()),
            (QUARTER, 0.5),
            "same times"
        );
        assert_ne!(press, release);
    }

    /// A pattern that could not be timed is not constructible.
    #[test]
    fn only_timeable_patterns_exist() {
        for time in [0.0, -0.1, f32::NAN, f32::INFINITY] {
            assert_eq!(Tap::new(time), None, "{time}");
            assert_eq!(Hold::new(time), None, "{time}");
            assert_eq!(DoubleTap::new(time, QUARTER), None, "{time}");
            assert_eq!(DoubleTap::new(QUARTER, time), None, "{time}");
        }
        assert_eq!(Tap::new(TAP_TIME), Some(Tap::default()));
        assert_eq!(Hold::new(HOLD_TIME), Some(Hold::default()));
        assert_eq!(
            DoubleTap::new(TAP_TIME, DOUBLE_TAP_WINDOW),
            Some(DoubleTap::default()),
        );
    }
}
