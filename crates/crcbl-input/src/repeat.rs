//! The repeat pattern: an action held down pulses once when it goes down and
//! then again on a schedule — the key-repeat every menu list expects.
//!
//! # On the tick clock
//!
//! The schedule runs on the clock [`ActionMap::begin_tick`] advances, never on
//! wall time, so a scripted input sequence repeats on the same ticks every run.
//! A pulse is an edge like [`ActionMap::just_pressed`]: [`ActionMap::repeated`]
//! is true for the rest of the tick it fired on, and `begin_tick` clears it. A
//! tick long enough to cover several repeats pulses once, and the schedule
//! moves past the time that tick covered rather than firing a backlog on the
//! ticks after it.
//!
//! # What "held" means per kind
//!
//! A button is held while it is down. A 1-D axis is held while its value is
//! non-zero, and changing sign is letting go and pressing again. A 2-D axis is
//! held toward its [`Cardinal`]: the larger component decides, a tie goes to
//! the vertical, and turning to another cardinal restarts the schedule with a
//! pulse. That is Unity's `InputSystemUIInputModule` rule — its move direction
//! is horizontal only when `|x| > |y|`, and a new direction resets its repeat
//! count — so a keyboard diagonal navigates vertically there and here.

use super::{ActionMap, ActionMapError, ActionValue, ButtonState};

/// How long an action is held before its first repeat, in seconds.
///
/// Unity's `InputSystemUIInputModule` default (`m_MoveRepeatDelay = 0.5f`):
/// long enough that a deliberate single press never repeats, short enough that
/// holding to scroll a list does not feel stuck.
pub const REPEAT_DELAY: f32 = 0.5;

/// The time between repeats once they start, in seconds.
///
/// Unity's `InputSystemUIInputModule` default (`m_MoveRepeatRate = 0.1f`),
/// which that module documents as the delay between repeated moves — ten steps
/// a second.
pub const REPEAT_INTERVAL: f32 = 0.1;

/// A repeat schedule: the first repeat after `delay` seconds held, then one
/// every `interval`.
///
/// The fields are private so the only schedules that exist are ones the
/// evaluator can run: a zero or non-finite interval would repeat every tick
/// forever, or never advance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repeat {
    delay: f32,
    interval: f32,
}

impl Repeat {
    /// [`REPEAT_DELAY`] then [`REPEAT_INTERVAL`]: the schedule the reserved
    /// `ui` context's navigation actions carry.
    pub const UI: Self = Self {
        delay: REPEAT_DELAY,
        interval: REPEAT_INTERVAL,
    };

    /// A schedule, or `None` unless `delay` is finite and not negative and
    /// `interval` is finite and positive.
    #[must_use]
    pub fn new(delay: f32, interval: f32) -> Option<Self> {
        let valid = delay.is_finite() && delay >= 0.0 && interval.is_finite() && interval > 0.0;
        valid.then_some(Self { delay, interval })
    }

    /// Seconds held before the first repeat.
    #[must_use]
    pub const fn delay(self) -> f32 {
        self.delay
    }

    /// Seconds between repeats.
    #[must_use]
    pub const fn interval(self) -> f32 {
        self.interval
    }
}

/// One of four directions, with +Y up as [`Binding::Wasd`](crate::Binding::Wasd)
/// has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cardinal {
    /// +Y.
    Up,
    /// −Y.
    Down,
    /// −X.
    Left,
    /// +X.
    Right,
}

impl Cardinal {
    /// The direction `(x, y)` points in, or `None` for the zero vector: the
    /// larger component decides and a tie is vertical — see the module docs.
    #[must_use]
    pub fn of(x: f32, y: f32) -> Option<Self> {
        if x == 0.0 && y == 0.0 {
            None
        } else if x.abs() > y.abs() {
            Some(if x > 0.0 { Self::Right } else { Self::Left })
        } else {
            Some(if y > 0.0 { Self::Up } else { Self::Down })
        }
    }
}

/// What an action is held as, for the schedule: a change is a new press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Down,
    Positive,
    Negative,
    Toward(Cardinal),
}

impl Held {
    fn of(value: &ActionValue) -> Option<Self> {
        match value {
            ActionValue::Button(button) => matches!(
                button.state,
                ButtonState::Pressed | ButtonState::Held { .. }
            )
            .then_some(Self::Down),
            ActionValue::Axis1(axis) if axis.value > 0.0 => Some(Self::Positive),
            ActionValue::Axis1(axis) if axis.value < 0.0 => Some(Self::Negative),
            ActionValue::Axis1(_) => None,
            ActionValue::Axis2(axis) => Cardinal::of(axis.x, axis.y).map(Self::Toward),
        }
    }
}

/// A slot's repeat schedule and where it is in it.
#[derive(Clone, Debug)]
pub(crate) struct RepeatState {
    pattern: Repeat,
    held: Option<Held>,
    /// The clock reading the next repeat is due at.
    next_at: f64,
    /// Whether the action pulsed on this tick.
    pub(crate) fired: bool,
}

impl RepeatState {
    pub(crate) fn reset(&mut self) {
        self.held = None;
        self.fired = false;
    }

    /// Advance the schedule against the slot's freshly resolved value.
    pub(crate) fn update(&mut self, value: &ActionValue, elapsed: f64) {
        let now = Held::of(value);
        if now.is_none() {
            self.held = None;
            return;
        }
        if now != self.held {
            self.held = now;
            self.fired = true;
            self.next_at = elapsed + f64::from(self.pattern.delay);
            return;
        }
        if elapsed >= self.next_at {
            self.fired = true;
            // Past `elapsed` in one step: a long tick pulses once.
            let interval = f64::from(self.pattern.interval);
            let missed = ((elapsed - self.next_at) / interval).floor() + 1.0;
            self.next_at += missed * interval;
            if self.next_at <= elapsed {
                self.next_at += interval;
            }
        }
    }
}

impl ActionMap {
    /// Attach a repeat schedule to an action, or detach it with `None`.
    ///
    /// Attaching does not pulse: an action already held when its schedule is
    /// attached is treated as having gone down now, and first repeats a
    /// [`Repeat::delay`] later.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn set_repeat(&mut self, name: &str, repeat: Option<Repeat>) -> Result<(), ActionMapError> {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return Err(ActionMapError::UnknownAction(name.to_owned()));
        };
        let elapsed = self.elapsed;
        let slot = &mut self.slots[idx];
        slot.repeat = repeat.map(|pattern| RepeatState {
            pattern,
            held: Held::of(&slot.value),
            next_at: elapsed + f64::from(pattern.delay),
            fired: false,
        });
        Ok(())
    }

    /// The repeat schedule attached to an action, if any.
    #[must_use]
    pub fn repeat(&self, name: &str) -> Option<Repeat> {
        let &idx = self.name_to_idx.get(name)?;
        self.slots[idx].repeat.as_ref().map(|state| state.pattern)
    }

    /// Whether an action pulsed on this tick: it went down (or turned to a new
    /// direction), or a repeat fell due. `false` for an action with no
    /// schedule, or none declared.
    #[must_use]
    pub fn repeated(&self, name: &str) -> bool {
        self.name_to_idx
            .get(name)
            .and_then(|&idx| self.slots[idx].repeat.as_ref())
            .is_some_and(|state| state.fired)
    }

    /// The [`Cardinal`] a 2-D axis action points in, or `None` when it is at
    /// rest, absent or another kind.
    #[must_use]
    pub fn cardinal(&self, name: &str) -> Option<Cardinal> {
        let (x, y) = self.axis2(name);
        Cardinal::of(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, Binding};
    use crcbl_core::input::KeyCode;

    /// Twenty ticks a second, so the delay is ten ticks and the interval two.
    ///
    /// `0.05` is not exact in binary: as an `f32` it is a little over, so a
    /// clock summed from it reaches each due time on the tick the arithmetic
    /// says rather than one later — the same side sixty hertz's `f32` falls
    /// on, which the next test pins separately.
    const TICK: f32 = 0.05;

    fn map_with_repeat(kind: ActionKind, bindings: Vec<Binding>) -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "nav".to_owned(),
            kind,
            bindings,
        });
        map.set_repeat("nav", Some(Repeat::UI)).expect("declared");
        map
    }

    /// The ticks (counting the press as tick 0) on which the action pulsed,
    /// over `ticks` ticks of holding it.
    fn pulses(map: &mut ActionMap, press: impl FnOnce(&mut ActionMap), ticks: u32) -> Vec<u32> {
        let mut fired = Vec::new();
        map.begin_tick(TICK);
        press(map);
        for tick in 0..ticks {
            if tick > 0 {
                map.begin_tick(TICK);
            }
            if map.repeated("nav") {
                fired.push(tick);
            }
        }
        fired
    }

    /// **The press pulses, the first repeat lands `delay` later, and each one
    /// after that `interval` later** — on ticks, whatever the wall clock did.
    #[test]
    fn a_held_button_repeats_after_the_delay_then_at_the_interval() {
        let mut map = map_with_repeat(ActionKind::Button, vec![Binding::Key(KeyCode::Enter)]);
        let fired = pulses(&mut map, |map| map.key_event(KeyCode::Enter, true), 17);
        // 0.5 s / 0.05 s = tick 10, then every 0.1 s / 0.05 s = 2 ticks.
        assert_eq!(fired, [0, 10, 12, 14, 16]);
    }

    /// The same schedule at sixty hertz, whose tick is not exact in binary:
    /// the delay's thirty ticks and the interval's six.
    #[test]
    fn the_schedule_holds_at_sixty_hertz() {
        let mut map = map_with_repeat(ActionKind::Button, vec![Binding::Key(KeyCode::Enter)]);
        let mut fired = Vec::new();
        map.begin_tick(1.0 / 60.0);
        map.key_event(KeyCode::Enter, true);
        for tick in 0..50 {
            if tick > 0 {
                map.begin_tick(1.0 / 60.0);
            }
            if map.repeated("nav") {
                fired.push(tick);
            }
        }
        assert_eq!(fired, [0, 30, 36, 42, 48]);
    }

    /// Releasing ends the schedule, and the next press starts a new one.
    #[test]
    fn a_release_ends_the_schedule() {
        let mut map = map_with_repeat(ActionKind::Button, vec![Binding::Key(KeyCode::Enter)]);
        let _ = pulses(&mut map, |map| map.key_event(KeyCode::Enter, true), 8);
        map.key_event(KeyCode::Enter, false);
        let fired = pulses(&mut map, |map| map.key_event(KeyCode::Enter, true), 11);
        assert_eq!(
            fired,
            [0, 10],
            "a fresh delay, not the old schedule's remainder"
        );
    }

    /// A tick that covers several repeats pulses once, and does not leave a
    /// backlog for the ticks after it.
    #[test]
    fn a_long_tick_pulses_once_and_leaves_no_backlog() {
        let mut map = map_with_repeat(ActionKind::Button, vec![Binding::Key(KeyCode::Enter)]);
        map.begin_tick(TICK);
        map.key_event(KeyCode::Enter, true);
        // Sixteen repeats fall due inside this tick, the last at 2.0 s held;
        // the next is due at 2.1 s.
        map.begin_tick(2.03);
        assert!(map.repeated("nav"), "the long tick pulses");
        map.begin_tick(TICK);
        assert!(
            !map.repeated("nav"),
            "2.08 s: the missed repeats are gone rather than queued",
        );
        map.begin_tick(TICK);
        assert!(map.repeated("nav"), "2.13 s: and the schedule carries on");
    }

    /// **A 2-D axis repeats per direction**: turning pulses at once and
    /// restarts the delay, and a diagonal is vertical.
    #[test]
    fn a_direction_change_restarts_the_schedule() {
        let wasd = Binding::Wasd {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        };
        let mut map = map_with_repeat(ActionKind::Axis2, vec![wasd]);
        let _ = pulses(&mut map, |map| map.key_event(KeyCode::KeyD, true), 6);
        assert_eq!(map.cardinal("nav"), Some(Cardinal::Right));

        // S joins the held D: `(0.707, -0.707)`, a tie, so the vertical wins
        // and the action turns from Right to Down without ever being released.
        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyS, true);
        assert_eq!(
            map.cardinal("nav"),
            Some(Cardinal::Down),
            "a diagonal's tie goes to the vertical",
        );
        assert!(map.repeated("nav"), "turning is a new press");
        let mut fired = Vec::new();
        for tick in 1..11 {
            map.begin_tick(TICK);
            if map.repeated("nav") {
                fired.push(tick);
            }
        }
        assert_eq!(fired, [10], "the delay restarted at the turn");
    }

    /// A 1-D axis is held by sign.
    #[test]
    fn a_one_dimensional_axis_repeats_by_sign() {
        let mut map = map_with_repeat(
            ActionKind::Axis1,
            vec![Binding::KeyAxis {
                negative: KeyCode::KeyQ,
                positive: KeyCode::KeyE,
            }],
        );
        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyE, true);
        assert!(map.repeated("nav"));
        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyE, false);
        map.key_event(KeyCode::KeyQ, true);
        assert!(map.repeated("nav"), "the other sign is another press");
    }

    /// A schedule that could not advance is not constructible.
    #[test]
    fn only_runnable_schedules_exist() {
        assert_eq!(Repeat::new(0.5, 0.1), Some(Repeat::UI));
        assert!(Repeat::new(0.0, 0.1).is_some(), "no delay is a schedule");
        for (delay, interval) in [
            (0.5, 0.0),
            (0.5, -1.0),
            (-0.1, 0.1),
            (f32::NAN, 0.1),
            (0.5, f32::INFINITY),
        ] {
            assert_eq!(Repeat::new(delay, interval), None, "({delay}, {interval})");
        }
    }

    /// Attaching to a held action does not pulse; no schedule never pulses.
    #[test]
    fn attaching_does_not_pulse_and_no_schedule_never_does() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "nav".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::Enter)],
        });
        map.key_event(KeyCode::Enter, true);
        assert!(!map.repeated("nav"), "no schedule");
        map.set_repeat("nav", Some(Repeat::UI)).expect("declared");
        assert!(!map.repeated("nav"));
        assert_eq!(map.repeat("nav"), Some(Repeat::UI));
        assert_eq!(
            map.set_repeat("nope", None),
            Err(ActionMapError::UnknownAction("nope".to_owned())),
        );
    }
}
