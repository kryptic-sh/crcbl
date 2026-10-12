//! One pad's sticks, for a game reading analog movement and look outside the
//! [`crate::ActionMap`]: the pad that moved first keeps the sticks, a dead
//! zone filters them, and after a menu or a lost focus nothing moves until
//! each stick has been let go.

use std::collections::HashMap;

use crate::gamepad::{radial_deadzone, valid_deadzone};
use crate::{GamepadId, GamepadSnapshot, Stick};

/// What [`ActivePad::submit`] did with a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickUpdate {
    /// Another pad holds the sticks; this one was ignored.
    OtherPad,
    /// An axis was not finite; the snapshot was ignored.
    NotFinite,
    /// Both sticks read zero after the dead zone and any release.
    Neutral,
    /// A stick reads something.
    Moved,
}

/// The pad whose sticks drive the game, its filtered sticks, and the
/// let-go rule.
///
/// - **The first pad to move a stick past the dead zone holds them**, or the
///   first to be [`activate`](Self::activate)d by a button, and keeps them
///   until it [`disconnect`](Self::disconnect)s: a second pad cannot take
///   over mid-move.
/// - **Each stick reads through a scaled radial dead zone**
///   ([`crate::radial_deadzone`]): zero inside it, rescaled from its edge to
///   full throw.
/// - **[`release_until_neutral`](Self::release_until_neutral)** — on opening a
///   menu, on losing focus — zeroes the sticks and keeps each stick of each
///   pad at zero until that stick has come back inside the dead zone, so a
///   stick still held as the menu closes does not walk the player off. Every
///   pad first seen after a release is held to the same rule, and so is a
///   pad that disconnected while it held the sticks, when it returns.
#[derive(Clone, Debug)]
pub struct ActivePad {
    dead_zone: f32,
    active: Option<GamepadId>,
    sticks: [(f32, f32); 2],
    /// Per pad, per stick (left, right): whether it is still waiting to rest.
    waiting: HashMap<GamepadId, [bool; 2]>,
    /// Whether pads not yet seen must rest first: set by a release.
    released: bool,
}

impl ActivePad {
    /// No pad holding the sticks, with `dead_zone` in `0.0..1.0`; `None`
    /// outside it.
    #[must_use]
    pub fn new(dead_zone: f32) -> Option<Self> {
        valid_deadzone(dead_zone).then(|| Self {
            dead_zone,
            active: None,
            sticks: [(0.0, 0.0); 2],
            waiting: HashMap::new(),
            released: false,
        })
    }

    /// The pad holding the sticks.
    #[must_use]
    pub const fn active(&self) -> Option<GamepadId> {
        self.active
    }

    /// Gives `id` the sticks, as a button press on it does, unless another
    /// pad holds them. Returns whether `id` holds them now.
    pub fn activate(&mut self, id: GamepadId) -> bool {
        if self.active.is_some_and(|active| active != id) {
            return false;
        }
        self.active = Some(id);
        true
    }

    /// Zeroes the sticks and holds every stick of every pad at zero until it
    /// has rested.
    pub fn release_until_neutral(&mut self) {
        self.sticks = [(0.0, 0.0); 2];
        self.waiting.clear();
        self.released = true;
    }

    /// Reads `snapshot` from pad `id`: see [`StickUpdate`]. A pad that moves
    /// while none holds the sticks takes them.
    pub fn submit(&mut self, id: GamepadId, snapshot: &GamepadSnapshot) -> StickUpdate {
        if self.active.is_some_and(|active| active != id) {
            return StickUpdate::OtherPad;
        }
        if !snapshot.is_finite() {
            return StickUpdate::NotFinite;
        }
        let mut sticks = [Stick::Left, Stick::Right]
            .map(|stick| radial_deadzone(snapshot.stick(stick), self.dead_zone));
        if self.released {
            self.waiting.entry(id).or_insert([true; 2]);
        }
        if let Some(waiting) = self.waiting.get_mut(&id) {
            for (stick, waits) in sticks.iter_mut().zip(waiting) {
                *waits &= *stick != (0.0, 0.0);
                if *waits {
                    *stick = (0.0, 0.0);
                }
            }
        }
        let moved = sticks.iter().any(|&stick| stick != (0.0, 0.0));
        if self.active.is_none() && !moved {
            return StickUpdate::Neutral;
        }
        self.active = Some(id);
        self.sticks = sticks;
        if moved {
            StickUpdate::Moved
        } else {
            StickUpdate::Neutral
        }
    }

    /// Takes the sticks from `id` if it holds them, zeroing them; when it
    /// returns, its sticks must rest before they drive again. Returns whether
    /// it held them.
    pub fn disconnect(&mut self, id: GamepadId) -> bool {
        if self.active != Some(id) {
            return false;
        }
        self.waiting.insert(id, [true; 2]);
        self.active = None;
        self.sticks = [(0.0, 0.0); 2];
        true
    }

    /// A stick as the holding pad last reported it, filtered: `(x, y)`, +Y
    /// up, inside the unit disc.
    #[must_use]
    pub const fn stick(&self, stick: Stick) -> (f32, f32) {
        match stick {
            Stick::Left => self.sticks[0],
            Stick::Right => self.sticks[1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PadAxis, PadKind};

    const DEAD: f32 = 0.2;

    fn pushed(left: (f32, f32), right: (f32, f32)) -> GamepadSnapshot {
        let mut snapshot = GamepadSnapshot::neutral(PadKind::Xbox);
        snapshot.axes[PadAxis::LeftX as usize] = left.0;
        snapshot.axes[PadAxis::LeftY as usize] = left.1;
        snapshot.axes[PadAxis::RightX as usize] = right.0;
        snapshot.axes[PadAxis::RightY as usize] = right.1;
        snapshot
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    const REST: (f32, f32) = (0.0, 0.0);

    /// **The first pad to move keeps the sticks until it leaves**, through
    /// the dead zone, and a button press cannot steal them either.
    #[test]
    fn the_first_pad_to_move_holds_the_sticks() {
        let mut pad = ActivePad::new(DEAD).unwrap();
        let (one, two) = (GamepadId(1), GamepadId(2));
        assert_eq!(
            pad.submit(two, &pushed((0.1, 0.1), REST)),
            StickUpdate::Neutral
        );
        assert_eq!(
            pad.active(),
            None,
            "a pad inside its dead zone takes nothing"
        );
        assert_eq!(
            pad.submit(one, &pushed((0.6, 0.0), REST)),
            StickUpdate::Moved
        );
        assert_eq!(pad.active(), Some(one));
        assert!(
            close(pad.stick(Stick::Left), (0.5, 0.0)),
            "rescaled: {:?}",
            pad.stick(Stick::Left)
        );
        assert_eq!(
            pad.submit(two, &pushed((1.0, 0.0), REST)),
            StickUpdate::OtherPad
        );
        assert!(!pad.activate(two));
        assert!(pad.disconnect(one));
        assert_eq!(pad.stick(Stick::Left), REST);
        assert!(pad.activate(two));
    }

    /// **After a release, a held stick stays at zero until it rests**, each
    /// stick on its own, and a pad first seen after it waits too.
    #[test]
    fn a_released_stick_waits_until_it_has_rested() {
        let mut pad = ActivePad::new(DEAD).unwrap();
        let one = GamepadId(1);
        let both = pushed((0.0, 1.0), (1.0, 0.0));
        assert_eq!(pad.submit(one, &both), StickUpdate::Moved);
        pad.release_until_neutral();
        assert_eq!(pad.stick(Stick::Left), REST);
        assert_eq!(
            pad.submit(one, &both),
            StickUpdate::Neutral,
            "both still held"
        );
        assert_eq!(
            pad.submit(one, &pushed(REST, (1.0, 0.0))),
            StickUpdate::Neutral
        );
        assert_eq!(
            pad.submit(one, &both),
            StickUpdate::Moved,
            "the left one rested"
        );
        assert!(close(pad.stick(Stick::Left), (0.0, 1.0)));
        assert_eq!(pad.stick(Stick::Right), REST, "the right one never did");

        assert!(pad.disconnect(one));
        let newcomer = GamepadId(2);
        assert_eq!(pad.submit(newcomer, &both), StickUpdate::Neutral);
        assert_eq!(
            pad.submit(newcomer, &pushed(REST, REST)),
            StickUpdate::Neutral
        );
        assert_eq!(pad.submit(newcomer, &both), StickUpdate::Moved);
    }

    /// **A pad that leaves while holding the sticks must let go when it
    /// returns**, release or none.
    #[test]
    fn a_returning_pad_must_rest_first() {
        let mut pad = ActivePad::new(DEAD).unwrap();
        let one = GamepadId(1);
        let held = pushed((1.0, 0.0), REST);
        assert_eq!(pad.submit(one, &held), StickUpdate::Moved);
        assert!(pad.disconnect(one));
        assert!(!pad.disconnect(one), "it holds nothing now");
        assert_eq!(pad.submit(one, &held), StickUpdate::Neutral);
        assert_eq!(pad.submit(one, &pushed(REST, REST)), StickUpdate::Neutral);
        assert_eq!(pad.submit(one, &held), StickUpdate::Moved);
    }

    /// Bad input is refused rather than read.
    #[test]
    fn bad_dead_zones_and_snapshots_are_refused() {
        assert!(ActivePad::new(1.0).is_none());
        assert!(ActivePad::new(-0.1).is_none());
        assert!(ActivePad::new(f32::NAN).is_none());
        let mut pad = ActivePad::new(DEAD).unwrap();
        let broken = pushed((f32::NAN, 0.0), REST);
        assert_eq!(pad.submit(GamepadId(1), &broken), StickUpdate::NotFinite);
        assert_eq!(pad.active(), None);
    }
}
