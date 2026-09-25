//! A player's rebinds as a diff over the game's declared defaults.
//!
//! The rule this keeps (the input rules in `docs/notes/simulation.md`): a
//! player's rebinds are stored as **diffs over the defaults, never as a copy
//! of the whole set**. [`ActionMap::overrides`] lists only the actions whose
//! bindings differ from their declaration, so an action a game adds later, or
//! a default it changes, reaches every player who never rebound that action.
//! [`ActionMap::apply_overrides`] is the inverse. Where the list is stored,
//! and in what file, is the game's business; each binding's text form is the
//! [`Binding`] `Display`/`FromStr` pair.

use crate::{ActionMap, ActionMapError, Binding};

/// One action whose bindings differ from its declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOverride {
    /// The action's name.
    pub action: String,
    /// The bindings the player chose, in place of the declared ones.
    pub bindings: Vec<Binding>,
}

impl ActionMap {
    /// Every action whose bindings differ from the ones it was declared with,
    /// in declaration order, with its current bindings.
    ///
    /// An action rebound and then rebound back to its declaration is not
    /// listed: the diff is by value, not by history.
    #[must_use]
    pub fn overrides(&self) -> Vec<ActionOverride> {
        self.slots
            .iter()
            .filter(|slot| slot.decl.bindings != slot.defaults)
            .map(|slot| ActionOverride {
                action: slot.decl.name.clone(),
                bindings: slot.decl.bindings.clone(),
            })
            .collect()
    }

    /// Sets the map to its declared defaults with `overrides` on top, through
    /// [`ActionMap::rebind`]: each listed action gets its listed bindings and
    /// **every action not listed goes back to its declaration**, so applying
    /// what [`ActionMap::overrides`] returned reproduces the map it came from.
    ///
    /// An action whose bindings already match is left alone, so its value and
    /// hold state survive. An override that cannot be applied is skipped and
    /// its error returned, and the rest still apply: a settings file naming an
    /// action the game no longer declares costs that one entry, not the load.
    /// The returned list is empty when every override applied.
    #[must_use = "an override that did not apply is reported only here"]
    pub fn apply_overrides(&mut self, overrides: &[ActionOverride]) -> Vec<ActionMapError> {
        let mut errors = Vec::new();
        let mut wanted: Vec<Vec<Binding>> = self
            .slots
            .iter()
            .map(|slot| slot.defaults.clone())
            .collect();
        for entry in overrides {
            match self.name_to_idx.get(&entry.action) {
                Some(&idx) if entry.bindings.iter().all(Binding::deadzone_is_valid) => {
                    wanted[idx].clone_from(&entry.bindings);
                }
                Some(_) => errors.push(ActionMapError::InvalidDeadzone(entry.action.clone())),
                None => errors.push(ActionMapError::UnknownAction(entry.action.clone())),
            }
        }
        for (idx, bindings) in wanted.into_iter().enumerate() {
            if self.slots[idx].decl.bindings != bindings {
                let name = self.slots[idx].decl.name.clone();
                if let Err(error) = self.rebind(&name, bindings) {
                    errors.push(error);
                }
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use crcbl_core::input::KeyCode;

    use super::*;
    use crate::{ActionDecl, ActionKind, ActionValue, ButtonAction, ButtonState, Trigger};

    fn map() -> ActionMap {
        let mut map = ActionMap::new();
        for (name, key) in [
            ("jump", KeyCode::Space),
            ("crouch", KeyCode::KeyC),
            ("use", KeyCode::KeyE),
        ] {
            map.declare(ActionDecl {
                name: name.to_owned(),
                kind: ActionKind::Button,
                bindings: vec![Binding::Key(key)],
            });
        }
        map
    }

    fn one(action: &str, bindings: Vec<Binding>) -> ActionOverride {
        ActionOverride {
            action: action.to_owned(),
            bindings,
        }
    }

    #[test]
    fn a_map_nobody_rebound_has_no_overrides() {
        assert_eq!(map().overrides(), []);
    }

    #[test]
    fn only_the_rebound_actions_are_listed_in_declaration_order() {
        let mut map = map();
        map.rebind("use", vec![Binding::Key(KeyCode::KeyF)])
            .unwrap();
        map.rebind("jump", vec![Binding::Key(KeyCode::KeyJ)])
            .unwrap();
        assert_eq!(
            map.overrides(),
            [
                one("jump", vec![Binding::Key(KeyCode::KeyJ)]),
                one("use", vec![Binding::Key(KeyCode::KeyF)]),
            ]
        );
        // Rebound back by hand: the diff is by value.
        map.rebind("use", vec![Binding::Key(KeyCode::KeyE)])
            .unwrap();
        assert_eq!(
            map.overrides(),
            [one("jump", vec![Binding::Key(KeyCode::KeyJ)])]
        );
    }

    #[test]
    fn applying_a_maps_overrides_to_a_fresh_map_reproduces_it() {
        let mut played = map();
        played
            .rebind(
                "crouch",
                vec![
                    Binding::Key(KeyCode::ControlLeft),
                    Binding::PadTrigger {
                        trigger: Trigger::Left,
                        threshold: 0.25,
                    },
                ],
            )
            .unwrap();
        let saved = played.overrides();

        let mut fresh = map();
        assert_eq!(fresh.apply_overrides(&saved), []);
        assert_eq!(fresh.overrides(), saved);
        assert_eq!(fresh.bindings("crouch"), played.bindings("crouch"));
        assert_eq!(
            fresh.bindings("jump"),
            Some(&[Binding::Key(KeyCode::Space)][..])
        );
    }

    #[test]
    fn an_action_not_listed_goes_back_to_its_declaration() {
        let mut map = map();
        map.rebind("jump", vec![Binding::Key(KeyCode::KeyJ)])
            .unwrap();
        assert_eq!(map.apply_overrides(&[]), []);
        assert_eq!(
            map.bindings("jump"),
            Some(&[Binding::Key(KeyCode::Space)][..])
        );
        assert_eq!(map.overrides(), []);
    }

    #[test]
    fn a_bad_override_is_reported_and_the_rest_still_apply() {
        let mut map = map();
        let errors = map.apply_overrides(&[
            one("retired_action", vec![Binding::Key(KeyCode::KeyR)]),
            one(
                "crouch",
                vec![Binding::PadTrigger {
                    trigger: Trigger::Left,
                    threshold: 1.5,
                }],
            ),
            one("use", vec![Binding::Key(KeyCode::KeyF)]),
        ]);
        assert_eq!(
            errors,
            [
                ActionMapError::UnknownAction("retired_action".to_owned()),
                ActionMapError::InvalidDeadzone("crouch".to_owned()),
            ]
        );
        assert_eq!(
            map.bindings("crouch"),
            Some(&[Binding::Key(KeyCode::KeyC)][..])
        );
        assert_eq!(
            map.overrides(),
            [one("use", vec![Binding::Key(KeyCode::KeyF)])]
        );
    }

    #[test]
    fn an_action_already_as_asked_keeps_its_hold() {
        let mut map = map();
        map.key_event(KeyCode::Space, true);
        map.begin_tick(0.016);
        assert_eq!(map.apply_overrides(&[]), []);
        // A rebind resets hold state, and would read as a fresh press with no
        // time held; jump was not rebound, so its hold carries on.
        let ActionValue::Button(ButtonAction {
            state: ButtonState::Held { duration },
            just_pressed,
            ..
        }) = *map.action("jump").unwrap()
        else {
            panic!("jump is a held button");
        };
        assert!(!just_pressed, "pressed again by a rebind nobody asked for");
        assert!(duration > 0.0, "the hold restarted: {duration}");
    }

    #[test]
    fn overrides_round_trip_through_their_text() {
        let mut map = map();
        map.rebind(
            "use",
            vec![
                Binding::Key(KeyCode::KeyF),
                Binding::PadButton(crate::PadButton::West),
            ],
        )
        .unwrap();
        let written: Vec<(String, Vec<String>)> = map
            .overrides()
            .into_iter()
            .map(|entry| {
                let texts = entry.bindings.iter().map(ToString::to_string).collect();
                (entry.action, texts)
            })
            .collect();
        assert_eq!(
            written,
            [(
                "use".to_owned(),
                vec!["KeyF".to_owned(), "Pad:West".to_owned()]
            )]
        );
        let read: Vec<ActionOverride> = written
            .iter()
            .map(|(action, texts)| ActionOverride {
                action: action.clone(),
                bindings: texts.iter().map(|text| text.parse().unwrap()).collect(),
            })
            .collect();
        let mut fresh = self::tests::map();
        assert_eq!(fresh.apply_overrides(&read), []);
        assert_eq!(fresh.overrides(), map.overrides());
    }
}
