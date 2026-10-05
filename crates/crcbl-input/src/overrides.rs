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
//!
//! # As text
//!
//! [`ActionMap::override_text`] and [`ActionMap::apply_override_text`] are the
//! same pair over that text form, for a store that keeps strings and knows
//! nothing of bindings — `crcbl_store::profile` is one. Reading text is where
//! a file written by another build meets this one, so each entry that cannot
//! apply is refused on its own and named ([`OverrideRefusal`]): an action this
//! build does not declare is skipped, and a binding text this build cannot
//! read leaves its action on the defaults rather than on part of a list.

use core::fmt;

use crate::{ActionMap, ActionMapError, Binding, BindingParseError};

/// One action whose bindings differ from its declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOverride {
    /// The action's name.
    pub action: String,
    /// The bindings the player chose, in place of the declared ones.
    pub bindings: Vec<Binding>,
}

/// Why one entry of a saved list of rebinds was not applied — see
/// [`ActionMap::apply_override_text`].
#[derive(Debug, Clone, PartialEq)]
pub enum OverrideRefusal {
    /// The list names an action this map does not declare. The entry is
    /// skipped and the rest apply.
    UnknownAction(String),
    /// One of an action's binding texts does not parse. The whole entry is
    /// refused, so the action keeps its declared bindings rather than the
    /// part of the player's list that did read.
    BadBinding {
        /// The action the entry was for.
        action: String,
        /// The text that did not parse, and why.
        error: BindingParseError,
    },
    /// The map refused the bindings — a dead zone or threshold outside
    /// `0.0..1.0` — and the action keeps its declared ones.
    Refused(ActionMapError),
}

impl fmt::Display for OverrideRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAction(action) => {
                write!(f, "no action called `{action}`; its binds were skipped")
            }
            Self::BadBinding { action, error } => {
                write!(f, "`{action}` keeps its defaults: {error}")
            }
            Self::Refused(error) => write!(f, "{error}; that action keeps its defaults"),
        }
    }
}

impl std::error::Error for OverrideRefusal {}

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

    /// [`ActionMap::overrides`] in the bindings' text form: each rebound
    /// action's name and its bindings' texts, in declaration order.
    #[must_use]
    pub fn override_text(&self) -> Vec<(String, Vec<String>)> {
        self.overrides()
            .into_iter()
            .map(|entry| {
                let texts = entry.bindings.iter().map(ToString::to_string).collect();
                (entry.action, texts)
            })
            .collect()
    }

    /// [`ActionMap::apply_overrides`] over a list in the bindings' text form,
    /// with every entry that could not apply refused by name: an action this
    /// map does not declare is skipped, and an entry with a binding text that
    /// does not parse is refused whole — see [`OverrideRefusal`].
    ///
    /// Like `apply_overrides`, every action not listed goes back to its
    /// declaration, and so does every action whose entry was refused.
    #[must_use = "an entry that did not apply is reported only here"]
    pub fn apply_override_text<'a>(
        &mut self,
        entries: impl IntoIterator<Item = (&'a str, &'a [String])>,
    ) -> Vec<OverrideRefusal> {
        let mut refusals = Vec::new();
        let mut parsed = Vec::new();
        for (action, texts) in entries {
            match texts
                .iter()
                .map(|text| text.parse::<Binding>())
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(bindings) => parsed.push(ActionOverride {
                    action: action.to_owned(),
                    bindings,
                }),
                Err(error) => refusals.push(OverrideRefusal::BadBinding {
                    action: action.to_owned(),
                    error,
                }),
            }
        }
        refusals.extend(
            self.apply_overrides(&parsed)
                .into_iter()
                .map(|error| match error {
                    ActionMapError::UnknownAction(action) => OverrideRefusal::UnknownAction(action),
                    other => OverrideRefusal::Refused(other),
                }),
        );
        refusals
    }

    /// The action other than `action`, in the same context, that `binding` is
    /// already one of the bindings of — what a rebind screen shows as a
    /// conflict before it takes the input away from that action.
    ///
    /// Exact bindings only: a chord that shares a key with `binding` is not a
    /// conflict here, because the chord rule already lets the two coexist.
    /// Actions in other contexts are not conflicts either; the context stack
    /// is what decides between them.
    #[must_use]
    pub fn bound_elsewhere(&self, action: &str, binding: &Binding) -> Option<&str> {
        let context = self.context_of(action)?;
        self.action_names().find(|other| {
            *other != action
                && self.context_of(other) == Some(context)
                && self
                    .bindings(other)
                    .is_some_and(|bindings| bindings.contains(binding))
        })
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

    /// A button chord saved as text comes back as the chord, and still takes
    /// its button from the plain binding once applied.
    #[test]
    fn a_button_chord_override_round_trips_and_still_shadows() {
        use crcbl_core::input::PointerButton;

        let mut map = map();
        map.declare(ActionDecl {
            name: "aim".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::MouseButton(PointerButton::Right)],
        });
        let chord = Binding::ButtonChord {
            modifier: crate::Modifier::Alt,
            button: PointerButton::Right,
        };
        map.rebind("use", vec![chord.clone()]).unwrap();
        let written: Vec<String> = map.overrides()[0]
            .bindings
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(written, ["Alt+Mouse:Right"]);

        let mut fresh = self::tests::map();
        fresh.declare(ActionDecl {
            name: "aim".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::MouseButton(PointerButton::Right)],
        });
        let read = one("use", written.iter().map(|t| t.parse().unwrap()).collect());
        assert_eq!(fresh.apply_overrides(&[read]), []);
        assert_eq!(fresh.bindings("use"), Some(&[chord][..]));

        fresh.key_event(KeyCode::AltLeft, true);
        fresh.mouse_button(PointerButton::Right, true);
        assert!(fresh.button_held("use"));
        assert!(!fresh.button_held("aim"), "the loaded chord did not shadow");
    }

    fn texts(entries: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        entries
            .iter()
            .map(|(action, texts)| {
                (
                    (*action).to_owned(),
                    texts.iter().map(|text| (*text).to_owned()).collect(),
                )
            })
            .collect()
    }

    fn apply_text(map: &mut ActionMap, entries: &[(String, Vec<String>)]) -> Vec<OverrideRefusal> {
        map.apply_override_text(
            entries
                .iter()
                .map(|(action, texts)| (action.as_str(), texts.as_slice())),
        )
    }

    /// **The text form round-trips**: a map's rebinds as text, applied to a
    /// fresh map, reproduce it, an action left with nothing included.
    #[test]
    fn override_text_round_trips_through_a_fresh_map() {
        let mut played = map();
        played
            .rebind(
                "use",
                vec![
                    Binding::Key(KeyCode::KeyF),
                    Binding::PadButton(crate::PadButton::West),
                ],
            )
            .unwrap();
        played.rebind("crouch", Vec::new()).unwrap();
        let saved = played.override_text();
        assert_eq!(
            saved,
            texts(&[("crouch", &[]), ("use", &["KeyF", "Pad:West"])])
        );

        let mut fresh = map();
        assert_eq!(apply_text(&mut fresh, &saved), []);
        assert_eq!(fresh.overrides(), played.overrides());
    }

    /// **An action this build does not declare is skipped and named**, and
    /// the entries beside it still apply.
    #[test]
    fn an_unknown_action_in_the_text_is_skipped_by_name() {
        let mut map = map();
        let refusals = apply_text(
            &mut map,
            &texts(&[("glide", &["KeyG"]), ("jump", &["KeyJ"])]),
        );
        assert_eq!(
            refusals,
            [OverrideRefusal::UnknownAction("glide".to_owned())]
        );
        assert!(refusals[0].to_string().contains("glide"));
        assert_eq!(
            map.bindings("jump"),
            Some(&[Binding::Key(KeyCode::KeyJ)][..])
        );
    }

    /// **A binding text naming no key is refused by name**, and its action
    /// keeps its defaults whole rather than the half of the list that read.
    #[test]
    fn a_bad_key_name_is_refused_by_name_and_its_action_keeps_its_defaults() {
        let mut map = map();
        let refusals = apply_text(
            &mut map,
            &texts(&[("jump", &["KeyJ", "KeyQwerty"]), ("use", &["KeyF"])]),
        );
        let [OverrideRefusal::BadBinding { action, error }] = refusals.as_slice() else {
            panic!("expected one bad binding, got {refusals:?}");
        };
        assert_eq!(action, "jump");
        assert_eq!(error.text, "KeyQwerty");
        assert!(
            refusals[0].to_string().contains("KeyQwerty"),
            "the refusal does not name the text: {}",
            refusals[0]
        );
        assert_eq!(
            map.bindings("jump"),
            Some(&[Binding::Key(KeyCode::Space)][..]),
            "half a list was applied"
        );
        assert_eq!(
            map.bindings("use"),
            Some(&[Binding::Key(KeyCode::KeyF)][..])
        );
    }

    /// **Rebinds saved by one build reach the next one, which added an
    /// action**: the new action comes up on its declared default, because the
    /// saved text is a diff and says nothing about it.
    #[test]
    fn saved_rebinds_survive_a_new_default_action() {
        let mut old = map();
        old.rebind("jump", vec![Binding::Key(KeyCode::KeyJ)])
            .unwrap();
        let saved = old.override_text();

        let mut updated = map();
        updated.declare(ActionDecl {
            name: "dash".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![Binding::Key(KeyCode::ShiftLeft)],
        });
        assert_eq!(apply_text(&mut updated, &saved), []);
        assert_eq!(
            updated.bindings("dash"),
            Some(&[Binding::Key(KeyCode::ShiftLeft)][..]),
            "the new action did not come up on its default"
        );
        assert_eq!(
            updated.bindings("jump"),
            Some(&[Binding::Key(KeyCode::KeyJ)][..])
        );
        assert_eq!(updated.override_text(), saved);
    }

    /// **A binding is a conflict only with another action in the same
    /// context**: not with the action itself, and not across contexts.
    #[test]
    fn a_binding_is_bound_elsewhere_only_in_the_same_context() {
        let mut map = map();
        map.declare_in(
            "vehicle",
            ActionDecl {
                name: "horn".to_owned(),
                kind: ActionKind::Button,
                bindings: vec![Binding::Key(KeyCode::KeyH)],
            },
        );
        let space = Binding::Key(KeyCode::Space);
        assert_eq!(map.bound_elsewhere("use", &space), Some("jump"));
        assert_eq!(map.bound_elsewhere("jump", &space), None, "itself");
        assert_eq!(map.bound_elsewhere("horn", &space), None, "another context");
        assert_eq!(
            map.bound_elsewhere("use", &Binding::Key(KeyCode::KeyH)),
            None
        );
        assert_eq!(
            map.bound_elsewhere("use", &Binding::Key(KeyCode::KeyQ)),
            None
        );
    }
}
