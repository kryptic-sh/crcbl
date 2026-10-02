//! Patterns that emit a **named action**: a hold on `jump` that fires
//! `jump_charge`, as the input plan's binding sketch spelled it
//! (`Hold(400, "jump_charge")`, `docs/notes/simulation.md`).
//!
//! A pattern firing is an edge on the action it is attached to —
//! [`ActionMap::hold_fired`] and the rest read it there. [`ActionMap::set_emits`]
//! also names a **button** action the pattern presses when it fires, so the
//! game, a replay and the server read `jump_charge` as an action like any
//! other — and [`InputTickState::capture`](crate::InputTickState::capture)
//! carries it, which a pattern edge on its own never was.
//!
//! # What an emitted press looks like
//!
//! The emitted action goes down on the tick the pattern fires — with
//! [`ActionMap::just_pressed`] — and comes up at the next
//! [`ActionMap::begin_tick`], with [`ActionMap::just_released`]. A pattern that
//! fires as a tick begins (a hold, or a single tap that waited out its double
//! tap's window) presses it for that tick; one that fires on an event (a tap's
//! release, a double tap's second press) presses it from that event to the
//! next tick. Its own bindings still drive it as well, and an action already
//! down — held by a binding, or pressed by another pattern this tick — is not
//! pressed again.
//!
//! **The order actions were declared in does not change the answer.** Presses
//! raised while `begin_tick` re-resolves every action are applied after the
//! last one is resolved, so an emitted action declared before its pattern's
//! action and one declared after see the same edges on the same ticks.
//!
//! A disabled emitted action, or one in a context that is not active, is not
//! pressed: the emission obeys the same rules its bindings do.

use crate::{ActionKind, ActionMap, ActionMapError};

/// One of the patterns [`ActionMap::set_emits`] can make emit an action.
///
/// The repeat pattern is not one: it pulses on a schedule while held, which is
/// a rate rather than a gesture, and nothing has asked for it to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pattern {
    /// The [`Tap`](crate::Tap) [`ActionMap::set_tap`] attaches.
    Tap,
    /// The [`Hold`](crate::Hold) [`ActionMap::set_hold`] attaches.
    Hold,
    /// The [`DoubleTap`](crate::DoubleTap) [`ActionMap::set_double_tap`]
    /// attaches.
    DoubleTap,
}

impl Pattern {
    /// Every pattern, in the order an action's patterns are listed in.
    pub const ALL: [Self; 3] = [Self::Tap, Self::Hold, Self::DoubleTap];

    /// This pattern's position in [`Pattern::ALL`].
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Tap => 0,
            Self::Hold => 1,
            Self::DoubleTap => 2,
        }
    }
}

impl ActionMap {
    /// Make an action's pattern press `target` whenever it fires, or stop it
    /// with `None` — see the module docs for what the press looks like.
    ///
    /// Names the emitted action whether or not the pattern is attached yet:
    /// [`ActionMap::set_hold`] and the rest leave it alone, and a pattern that
    /// is not attached never fires. Nothing in flight is cancelled, since the
    /// pattern's timing does not change.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if `source` or `target` is not
    /// declared, [`ActionMapError::EmitsItself`] if they are the same action,
    /// [`ActionMapError::NotAButton`] if `target` is not an
    /// [`ActionKind::Button`] — a press is the only thing a pattern can give.
    pub fn set_emits(
        &mut self,
        source: &str,
        pattern: Pattern,
        target: Option<&str>,
    ) -> Result<(), ActionMapError> {
        let Some(&source_idx) = self.name_to_idx.get(source) else {
            return Err(ActionMapError::UnknownAction(source.to_owned()));
        };
        let target_idx = match target {
            None => None,
            Some(name) => {
                let Some(&idx) = self.name_to_idx.get(name) else {
                    return Err(ActionMapError::UnknownAction(name.to_owned()));
                };
                if idx == source_idx {
                    return Err(ActionMapError::EmitsItself(source.to_owned()));
                }
                if self.slots[idx].kind() != ActionKind::Button {
                    return Err(ActionMapError::NotAButton(name.to_owned()));
                }
                Some(idx)
            }
        };
        self.slots[source_idx].patterns.emits[pattern.index()] = target_idx;
        Ok(())
    }

    /// The action an action's pattern emits, if it names one.
    #[must_use]
    pub fn emits(&self, source: &str, pattern: Pattern) -> Option<&str> {
        let &idx = self.name_to_idx.get(source)?;
        let target = self.slots[idx].patterns.emits[pattern.index()]?;
        Some(self.slots[target].decl.name.as_str())
    }

    /// Press every action a pattern queued since this was last called — see
    /// the module docs. Each press can fire the pressed action's own patterns
    /// and queue more; an action is pressed at most once between two ticks,
    /// which is what ends the chain.
    pub(crate) fn drain_emits(&mut self) {
        let mut next = 0;
        while let Some(&target) = self.pending_emits.get(next) {
            next += 1;
            if !self.slots[target].emitted && self.is_live(target) {
                self.slots[target].emitted = true;
                self.resolve_slot(target);
            }
        }
        self.pending_emits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionValue, Binding, ButtonState, Hold, InputTickState, Tap};
    use crcbl_core::input::KeyCode;

    /// Sixteen ticks a second, exact in binary, as `patterns.rs`'s tests use.
    const TICK: f32 = 0.0625;

    /// Four ticks.
    const QUARTER: f32 = 0.25;

    fn button(map: &mut ActionMap, name: &str, bindings: Vec<Binding>) {
        map.declare(ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Button,
            bindings,
        });
    }

    fn hold(time: f32) -> Option<Hold> {
        Some(Hold::new(time).expect("a runnable hold"))
    }

    /// A map with `jump` on Space carrying a hold that emits `jump_charge`.
    fn charge_map() -> ActionMap {
        let mut map = ActionMap::new();
        button(&mut map, "jump", vec![Binding::Key(KeyCode::Space)]);
        button(&mut map, "jump_charge", Vec::new());
        map.set_hold("jump", hold(QUARTER)).expect("declared");
        map.set_emits("jump", Pattern::Hold, Some("jump_charge"))
            .expect("a button");
        map
    }

    /// The edges `name` showed on each of `ticks` ticks, pressing and
    /// releasing keys just after the tick begins as `edges` says.
    fn edges(
        map: &mut ActionMap,
        name: &str,
        script: &[(u32, KeyCode, bool)],
        ticks: u32,
    ) -> Vec<(u32, bool, bool)> {
        let mut seen = Vec::new();
        for tick in 0..ticks {
            map.begin_tick(TICK);
            for &(_, key, down) in script.iter().filter(|(at, ..)| *at == tick) {
                map.key_event(key, down);
            }
            let (pressed, released) = (map.just_pressed(name), map.just_released(name));
            if pressed || released {
                seen.push((tick, pressed, released));
            }
        }
        seen
    }

    /// **A hold presses its named action on the tick it fires**, and the
    /// action comes up on the next — while the hold is still an edge on the
    /// action it is attached to.
    #[test]
    fn a_hold_presses_the_action_it_emits_for_one_tick() {
        let mut map = charge_map();
        let mut seen = Vec::new();
        for tick in 0..8 {
            map.begin_tick(TICK);
            if tick == 0 {
                map.key_event(KeyCode::Space, true);
            }
            seen.push((
                map.just_pressed("jump_charge"),
                map.button_held("jump_charge"),
                map.just_released("jump_charge"),
            ));
            assert_eq!(map.hold_fired("jump"), tick == 4, "tick {tick}");
        }
        let idle = (false, false, false);
        assert_eq!(
            seen,
            [
                idle,
                idle,
                idle,
                idle,
                (true, true, false),
                (false, false, true),
                idle,
                idle
            ]
        );
    }

    /// A tap fires on the release event, and presses its action from there
    /// to the next tick.
    #[test]
    fn a_tap_presses_its_action_from_the_release() {
        let mut map = ActionMap::new();
        button(&mut map, "dash", vec![Binding::Key(KeyCode::KeyQ)]);
        button(&mut map, "dash_tap", Vec::new());
        map.set_tap("dash", Some(Tap::new(QUARTER).expect("runnable")))
            .expect("declared");
        map.set_emits("dash", Pattern::Tap, Some("dash_tap"))
            .expect("a button");
        let script = [(1, KeyCode::KeyQ, true), (2, KeyCode::KeyQ, false)];
        assert_eq!(
            edges(&mut map, "dash_tap", &script, 6),
            [(2, true, false), (3, false, true)]
        );
    }

    /// **Declaration order does not change the edges.** Two holds a tick apart
    /// press one action on consecutive ticks; the second press is a fresh
    /// press whether the emitted action was declared before its sources or
    /// after them.
    #[test]
    fn the_edges_do_not_depend_on_declaration_order() {
        let script = [(0, KeyCode::KeyA, true), (1, KeyCode::KeyB, true)];
        let mut per_order = Vec::new();
        for charge_first in [true, false] {
            let mut map = ActionMap::new();
            if charge_first {
                button(&mut map, "charge", Vec::new());
            }
            button(&mut map, "a", vec![Binding::Key(KeyCode::KeyA)]);
            button(&mut map, "b", vec![Binding::Key(KeyCode::KeyB)]);
            if !charge_first {
                button(&mut map, "charge", Vec::new());
            }
            for source in ["a", "b"] {
                map.set_hold(source, hold(QUARTER)).expect("declared");
                map.set_emits(source, Pattern::Hold, Some("charge"))
                    .expect("a button");
            }
            per_order.push(edges(&mut map, "charge", &script, 8));
        }
        assert_eq!(
            per_order[0],
            [(4, true, false), (5, true, true), (6, false, true)],
            "declared first"
        );
        assert_eq!(per_order[1], per_order[0], "declared last");
    }

    /// The press reaches a captured tick, which a pattern edge alone never
    /// did: the server sees `jump_charge` go down.
    #[test]
    fn a_captured_tick_carries_the_emitted_press() {
        let mut map = charge_map();
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        for _ in 0..4 {
            map.begin_tick(TICK);
        }
        let captured = InputTickState::capture(&map);
        let Some(ActionValue::Button(charge)) = captured.get("jump_charge") else {
            panic!("jump_charge is a captured button");
        };
        assert!(charge.just_pressed);
        assert_eq!(charge.state, ButtonState::Pressed);
    }

    /// A disabled emitted action is not pressed, as a disabled action's
    /// bindings press nothing.
    #[test]
    fn a_disabled_emitted_action_is_not_pressed() {
        let mut map = charge_map();
        map.set_enabled("jump_charge", false);
        let script = [(0, KeyCode::Space, true)];
        assert_eq!(edges(&mut map, "jump_charge", &script, 8), []);
    }

    #[test]
    fn set_emits_refuses_by_name_and_reads_back() {
        let mut map = ActionMap::new();
        button(&mut map, "jump", vec![Binding::Key(KeyCode::Space)]);
        map.declare(ActionDecl {
            name: "aim".to_owned(),
            kind: ActionKind::Axis1,
            bindings: Vec::new(),
        });
        button(&mut map, "charge", Vec::new());
        let unknown = |name: &str| Err(ActionMapError::UnknownAction(name.to_owned()));
        assert_eq!(
            map.set_emits("nope", Pattern::Hold, Some("charge")),
            unknown("nope")
        );
        assert_eq!(
            map.set_emits("jump", Pattern::Hold, Some("nope")),
            unknown("nope")
        );
        assert_eq!(
            map.set_emits("jump", Pattern::Hold, Some("jump")),
            Err(ActionMapError::EmitsItself("jump".to_owned()))
        );
        assert_eq!(
            map.set_emits("jump", Pattern::Hold, Some("aim")),
            Err(ActionMapError::NotAButton("aim".to_owned()))
        );
        assert_eq!(map.emits("jump", Pattern::Hold), None, "nothing stuck");

        map.set_emits("jump", Pattern::DoubleTap, Some("charge"))
            .expect("a button");
        assert_eq!(map.emits("jump", Pattern::DoubleTap), Some("charge"));
        assert_eq!(map.emits("jump", Pattern::Tap), None);
        map.set_emits("jump", Pattern::DoubleTap, None)
            .expect("declared");
        assert_eq!(map.emits("jump", Pattern::DoubleTap), None);
    }
}
