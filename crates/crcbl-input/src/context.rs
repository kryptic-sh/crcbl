//! The context stack: which active context an input reaches.
//!
//! # The stack
//!
//! Every action is in one context. [`GAMEPLAY_CONTEXT`] is the base: it is
//! always on the stack, is never pushed or popped, and is where
//! [`ActionMap::declare`] puts an action. Any other context is on the stack
//! only between [`ActionMap::push_context`] and [`ActionMap::pop_context`], and
//! the stack is a stack — a pop names the context it expects on top and is
//! refused otherwise, so two owners pushing and popping out of order fail
//! loudly instead of popping each other's context. An action in a context that
//! is not on the stack is idle, as a disabled one is.
//!
//! # Consumption
//!
//! **An input belongs to the topmost active context that binds it**, and only
//! that context's actions read it; an input no active context binds reaches
//! nothing, and one the top context does not bind falls through to the first
//! context beneath that does. "Binds" is by binding, not by enabled flag: a
//! disabled action is silenced, and its keys stay its context's until
//! [`ActionMap::rebind`] moves them. Inputs are keys (the keys a
//! [`Binding::Chord`] owns, not its modifier), pointer buttons, on-screen
//! controls by id, the pointer's position, motion and wheel each as one input,
//! and pad buttons, sticks and triggers — every pad's South is one input, as
//! every pad drives every pad binding.
//!
//! # A key held while the stack changes
//!
//! **A held input whose owner a push or a pop changes is withheld from its new
//! owner until it is released.** Its old owner sees it come up — a walk stops
//! when a menu opens over it — and its new owner does not see it go down: the
//! Enter that accepted a menu item and popped the `ui` context does not then
//! fire the game's Enter binding, and the Escape that opened a menu does not
//! reach the `ui_back` that would close it. Unreal's Enhanced Input makes the
//! same call with `FModifyContextOptions::bIgnoreAllPressedKeysUntilRelease`,
//! on by default; this is narrower, withholding only inputs whose owner
//! actually changed, so a key the new context does not bind keeps working
//! without a lift. Levels are not withheld — a pointer position or a stick
//! deflection is a place, not a press — only routed; a pad button is a press
//! and is withheld like a key, but a pad trigger is a level even when a button
//! action reads it. A rebind moves ownership
//! without withholding, because a rebind already resolves against the keys
//! held at the time.

use std::collections::{HashMap, HashSet};

use crcbl_core::input::{KeyCode, PointerButton};

use super::{ActionMap, ActionMapError, Binding, Modifier, PadButton, PadButtons, Stick, Trigger};

/// The base context: always active, and where [`ActionMap::declare`] puts an
/// action.
pub const GAMEPLAY_CONTEXT: &str = "gameplay";

/// The owner of each input some active context binds, as an index into
/// [`ActionMap::contexts`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Routes {
    keys: HashMap<KeyCode, usize>,
    /// For each key, the modifiers of the chords its **owner** binds on it:
    /// what shadows that owner's plain bindings on the key.
    chords: HashMap<KeyCode, Vec<Modifier>>,
    buttons: HashMap<PointerButton, usize>,
    controls: HashMap<String, usize>,
    motion: Option<usize>,
    scroll: Option<usize>,
    pub(crate) pointer: Option<usize>,
    pad_buttons: HashMap<PadButton, usize>,
    pad_sticks: HashMap<Stick, usize>,
    pad_triggers: HashMap<Trigger, usize>,
}

/// Held inputs withheld from their owner until released — see the module docs.
#[derive(Debug, Default, Clone)]
pub(crate) struct Suppressed {
    pub(crate) keys: HashSet<KeyCode>,
    pub(crate) buttons: HashSet<PointerButton>,
    pub(crate) controls: HashSet<String>,
    /// Pad buttons withheld — by a stack change, or by
    /// [`ActionMap::release_gamepads`].
    pub(crate) pad_buttons: PadButtons,
}

/// What one context can see of the raw input.
pub(crate) struct View<'a> {
    pub(crate) context: usize,
    pub(crate) routes: &'a Routes,
    pub(crate) suppressed: &'a Suppressed,
    pub(crate) held_keys: &'a HashSet<KeyCode>,
    pub(crate) held_buttons: &'a HashSet<PointerButton>,
    pub(crate) held_controls: &'a HashSet<String>,
    /// Every button some pad holds.
    pub(crate) held_pad_buttons: PadButtons,
}

impl View<'_> {
    /// The key is held, this context owns it, and it is not withheld.
    fn owns_held(&self, key: KeyCode) -> bool {
        self.held_keys.contains(&key)
            && !self.suppressed.keys.contains(&key)
            && self.routes.keys.get(&key) == Some(&self.context)
    }

    /// A plain binding's read of `key`: down unless a chord on it is satisfied.
    pub(crate) fn key(&self, key: KeyCode) -> bool {
        self.owns_held(key)
            && !self
                .routes
                .chords
                .get(&key)
                .is_some_and(|modifiers| modifiers.iter().any(|m| m.held(self.held_keys)))
    }

    /// A [`Binding::Chord`]'s read: its key, and its modifier read raw.
    pub(crate) fn chord(&self, modifier: Modifier, key: KeyCode) -> bool {
        self.owns_held(key) && modifier.held(self.held_keys)
    }

    pub(crate) fn button(&self, button: PointerButton) -> bool {
        self.held_buttons.contains(&button)
            && !self.suppressed.buttons.contains(&button)
            && self.routes.buttons.get(&button) == Some(&self.context)
    }

    pub(crate) fn control(&self, id: &str) -> bool {
        self.held_controls.contains(id) && !self.suppressed.controls.contains(id) && self.stick(id)
    }

    /// Owner only: a stick's deflection is a level and is never withheld.
    pub(crate) fn stick(&self, id: &str) -> bool {
        self.routes.controls.get(id) == Some(&self.context)
    }

    pub(crate) fn motion(&self) -> bool {
        self.routes.motion == Some(self.context)
    }

    pub(crate) fn scroll(&self) -> bool {
        self.routes.scroll == Some(self.context)
    }

    pub(crate) fn pointer(&self) -> bool {
        self.routes.pointer == Some(self.context)
    }

    pub(crate) fn pad_button(&self, button: PadButton) -> bool {
        self.held_pad_buttons.contains(button)
            && !self.suppressed.pad_buttons.contains(button)
            && self.routes.pad_buttons.get(&button) == Some(&self.context)
    }

    /// Owner only: a stick is a level and is never withheld.
    pub(crate) fn pad_stick(&self, stick: Stick) -> bool {
        self.routes.pad_sticks.get(&stick) == Some(&self.context)
    }

    /// Owner only: a trigger is a level and is never withheld.
    pub(crate) fn trigger(&self, trigger: Trigger) -> bool {
        self.routes.pad_triggers.get(&trigger) == Some(&self.context)
    }
}

impl Routes {
    /// Owners for the stack as it stands: each context from the top down
    /// claims what nothing above it already has.
    fn build(map: &ActionMap) -> Self {
        let mut routes = Self::default();
        for &context in map.stack.iter().rev() {
            let bindings = || {
                map.slots
                    .iter()
                    .filter(move |slot| slot.context == context)
                    .flat_map(|slot| slot.decl.bindings.iter())
            };
            for binding in bindings() {
                binding.visit_keys(|key| {
                    routes.keys.entry(key).or_insert(context);
                });
                match binding {
                    Binding::MouseButton(button) => {
                        routes.buttons.entry(*button).or_insert(context);
                    }
                    Binding::Virtual(id) => {
                        if !routes.controls.contains_key(id.as_str()) {
                            routes.controls.insert(id.clone(), context);
                        }
                    }
                    Binding::MouseMotion => {
                        routes.motion.get_or_insert(context);
                    }
                    Binding::MouseScroll => {
                        routes.scroll.get_or_insert(context);
                    }
                    Binding::PointerPosition { .. } => {
                        routes.pointer.get_or_insert(context);
                    }
                    Binding::PadButton(button) => {
                        routes.pad_buttons.entry(*button).or_insert(context);
                    }
                    Binding::PadStick { stick, .. } => {
                        routes.pad_sticks.entry(*stick).or_insert(context);
                    }
                    Binding::PadTrigger { trigger, .. } => {
                        routes.pad_triggers.entry(*trigger).or_insert(context);
                    }
                    Binding::Key(_)
                    | Binding::KeyAxis { .. }
                    | Binding::Chord { .. }
                    | Binding::Wasd { .. } => {}
                }
            }
            // After this context's keys are claimed, so a chord registers only
            // where its context is the one reading the key.
            for binding in bindings() {
                if let Binding::Chord { modifier, key } = binding
                    && routes.keys.get(key) == Some(&context)
                {
                    let modifiers = routes.chords.entry(*key).or_default();
                    if !modifiers.contains(modifier) {
                        modifiers.push(*modifier);
                    }
                }
            }
        }
        routes
    }
}

impl ActionMap {
    /// Push a declared context on top of the stack.
    ///
    /// Every held input the context takes from a context beneath is withheld
    /// until released — see the module docs — and every action re-resolves, so
    /// what the context beneath lost reads as released on this tick.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownContext`] if no action was ever declared in it,
    /// [`ActionMapError::ContextAlreadyActive`] if it is already on the stack.
    pub fn push_context(&mut self, context: &str) -> Result<(), ActionMapError> {
        let index = self.context_index(context)?;
        if self.stack.contains(&index) {
            return Err(ActionMapError::ContextAlreadyActive(context.to_owned()));
        }
        self.stack.push(index);
        self.restack();
        Ok(())
    }

    /// Pop the topmost pushed context, which must be `context`.
    ///
    /// Its actions go idle without a release edge, as a disabled action does,
    /// and inputs it held fall through to the contexts beneath only once they
    /// have been released.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownContext`] if it was never declared,
    /// [`ActionMapError::ContextNotOnTop`] if it is not the topmost pushed
    /// context — including [`GAMEPLAY_CONTEXT`], which is never pushed.
    pub fn pop_context(&mut self, context: &str) -> Result<(), ActionMapError> {
        let index = self.context_index(context)?;
        if self.stack.len() == 1 || self.stack.last() != Some(&index) {
            return Err(ActionMapError::ContextNotOnTop(context.to_owned()));
        }
        self.stack.pop();
        self.restack();
        Ok(())
    }

    /// The active contexts' names, bottom first: [`GAMEPLAY_CONTEXT`], then
    /// each pushed context in the order it was pushed.
    pub fn active_contexts(&self) -> impl Iterator<Item = &str> {
        self.stack
            .iter()
            .map(|&index| self.contexts[index].as_str())
    }

    /// Whether `context` is on the stack. `false` for one never declared.
    #[must_use]
    pub fn is_context_active(&self, context: &str) -> bool {
        self.contexts
            .iter()
            .position(|name| name == context)
            .is_some_and(|index| self.stack.contains(&index))
    }

    /// The context an action was declared in, or `None` if it is not declared.
    #[must_use]
    pub fn context_of(&self, name: &str) -> Option<&str> {
        let &idx = self.name_to_idx.get(name)?;
        Some(self.contexts[self.slots[idx].context].as_str())
    }

    fn context_index(&self, context: &str) -> Result<usize, ActionMapError> {
        self.contexts
            .iter()
            .position(|name| name == context)
            .ok_or_else(|| ActionMapError::UnknownContext(context.to_owned()))
    }

    /// Whether the slot at `idx` reacts to input: enabled, in an active context.
    pub(crate) fn is_live(&self, idx: usize) -> bool {
        let slot = &self.slots[idx];
        slot.enabled && self.stack.contains(&slot.context)
    }

    /// Rebuild the routes after a binding changed, and re-resolve every live
    /// action against them. Nothing is withheld.
    pub(crate) fn reroute(&mut self) {
        self.routes = Routes::build(self);
        for idx in 0..self.slots.len() {
            if self.is_live(idx) {
                self.resolve_one(idx);
            }
        }
    }

    /// Rebuild the routes after the stack changed: withhold every held input
    /// whose owner moved, idle the actions of contexts that left, and
    /// re-resolve the rest.
    fn restack(&mut self) {
        let routes = Routes::build(self);
        let old = std::mem::replace(&mut self.routes, routes);
        let new = &self.routes;
        for key in &self.held_keys {
            if old.keys.get(key) != new.keys.get(key) {
                self.suppressed.keys.insert(*key);
            }
        }
        for button in &self.held_buttons {
            if old.buttons.get(button) != new.buttons.get(button) {
                self.suppressed.buttons.insert(*button);
            }
        }
        for control in &self.held_controls {
            if old.controls.get(control) != new.controls.get(control) {
                self.suppressed.controls.insert(control.clone());
            }
        }
        for button in PadButton::ALL {
            if self.held_pad_buttons.contains(button)
                && old.pad_buttons.get(&button) != new.pad_buttons.get(&button)
            {
                self.suppressed.pad_buttons.insert(button);
            }
        }
        for idx in 0..self.slots.len() {
            // Before resolving, so a press the new stack takes away is not
            // read as a release that taps: a stack change cancels every
            // pattern in flight — see `patterns.rs`.
            self.slots[idx].patterns.cancel();
            if self.stack.contains(&self.slots[idx].context) {
                if self.slots[idx].enabled {
                    self.resolve_one(idx);
                }
            } else {
                self.slots[idx].reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionDecl, ActionKind, ActionValue, InputTickState, Repeat};

    const TICK: f32 = 1.0 / 60.0;

    fn button(name: &str, bindings: Vec<Binding>) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Button,
            bindings,
        }
    }

    /// `jump` on Space in gameplay, `accept` on Space and Enter in `menu`.
    fn menu_over_gameplay() -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(button("jump", vec![Binding::Key(KeyCode::Space)]));
        map.declare(button("crouch", vec![Binding::Key(KeyCode::KeyC)]));
        map.declare_in(
            "menu",
            button(
                "accept",
                vec![Binding::Key(KeyCode::Space), Binding::Key(KeyCode::Enter)],
            ),
        );
        map
    }

    /// **A key bound in two contexts reaches only the topmost one while it is
    /// pushed, and the one beneath once it is popped.**
    #[test]
    fn a_key_bound_in_both_reaches_only_the_pushed_context_then_gameplay_after_pop() {
        let mut map = menu_over_gameplay();
        assert_eq!(map.context_of("accept"), Some("menu"));
        assert_eq!(map.context_of("jump"), Some(GAMEPLAY_CONTEXT));

        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(
            map.just_pressed("jump"),
            "nothing is pushed: gameplay has it"
        );
        assert!(
            !map.button_held("accept"),
            "and a context off the stack is idle"
        );
        map.key_event(KeyCode::Space, false);

        map.push_context("menu").expect("declared, not active");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed("accept"), "the pushed context owns Space");
        assert!(
            !map.button_held("jump"),
            "and gameplay beneath it does not see it"
        );
        map.key_event(KeyCode::Space, false);

        map.pop_context("menu").expect("menu is on top");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(
            map.just_pressed("jump"),
            "popped: Space is gameplay's again"
        );
        assert!(!map.button_held("accept"));
    }

    /// **An input the top context does not bind falls through**, and one no
    /// active context binds reaches nothing.
    #[test]
    fn an_input_the_top_context_does_not_bind_falls_through() {
        let mut map = menu_over_gameplay();
        map.push_context("menu").expect("declared");

        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyC, true);
        assert!(
            map.just_pressed("crouch"),
            "menu binds no C, so it falls through"
        );
        map.key_event(KeyCode::KeyQ, true);
        assert!(!map.button_held("accept") && !map.button_held("jump"));
    }

    /// **Nested pushes**: each input goes to the topmost context binding it,
    /// the stack is enforced, and popping the top hands its inputs back to the
    /// next one down.
    #[test]
    fn nested_pushes_route_each_input_to_the_topmost_binder() {
        let mut map = menu_over_gameplay();
        map.declare_in(
            "editor",
            button("select", vec![Binding::Key(KeyCode::Enter)]),
        );

        map.push_context("menu").expect("declared");
        map.push_context("editor").expect("declared");
        assert_eq!(
            map.active_contexts().collect::<Vec<_>>(),
            [GAMEPLAY_CONTEXT, "menu", "editor"],
        );
        assert_eq!(
            map.push_context("menu"),
            Err(ActionMapError::ContextAlreadyActive("menu".to_owned())),
        );
        assert_eq!(
            map.pop_context("menu"),
            Err(ActionMapError::ContextNotOnTop("menu".to_owned())),
            "menu is under editor, and popping it would pop editor's",
        );

        map.begin_tick(TICK);
        map.key_event(KeyCode::Enter, true);
        map.key_event(KeyCode::Space, true);
        map.key_event(KeyCode::KeyC, true);
        assert!(
            map.button_held("select"),
            "Enter: editor is topmost and binds it"
        );
        assert!(
            map.button_held("accept"),
            "Space falls through editor to menu"
        );
        assert!(
            !map.button_held("jump"),
            "and menu consumes it before gameplay"
        );
        assert!(map.button_held("crouch"), "C falls through both");
        for key in [KeyCode::Enter, KeyCode::Space, KeyCode::KeyC] {
            map.key_event(key, false);
        }

        map.pop_context("editor").expect("editor is on top");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Enter, true);
        assert!(map.just_pressed("accept"), "editor popped: menu owns Enter");
        assert!(!map.button_held("select"), "and a popped context is idle");
    }

    /// **A key held while the stack changes is withheld from its new owner
    /// until it is released** — in both directions.
    #[test]
    fn a_key_held_across_a_push_or_pop_waits_for_its_release() {
        let mut map = menu_over_gameplay();

        // Held into a push: gameplay sees it come up, menu does not see it go
        // down.
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        map.begin_tick(TICK);
        map.push_context("menu").expect("declared");
        assert!(
            map.just_released("jump"),
            "the walk stops when a menu opens over it"
        );
        assert!(
            !map.button_held("accept"),
            "and the menu is not pressed by it"
        );
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true); // an OS auto-repeat is not a new press
        assert!(!map.button_held("accept"));
        map.key_event(KeyCode::Space, false);
        map.key_event(KeyCode::Space, true);
        assert!(
            map.just_pressed("accept"),
            "a fresh press after the lift is menu's"
        );

        // Held out through a pop: the accept that closed the menu does not jump.
        map.pop_context("menu").expect("menu is on top");
        assert!(
            !map.button_held("jump"),
            "the key that closed the menu does not jump"
        );
        map.begin_tick(TICK);
        assert!(!map.button_held("jump"));
        map.key_event(KeyCode::Space, false);
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed("jump"));
    }

    /// A held key the pushed context does not bind keeps its owner, and so
    /// keeps working with no lift.
    #[test]
    fn a_held_key_whose_owner_does_not_change_is_not_withheld() {
        let mut map = menu_over_gameplay();
        map.key_event(KeyCode::KeyC, true);
        map.push_context("menu").expect("declared");
        map.begin_tick(TICK);
        assert!(map.button_held("crouch"));
        assert!(!map.just_released("crouch"));
    }

    /// Unknown contexts and the base are refused rather than ignored.
    #[test]
    fn the_stack_refuses_unknown_contexts_and_the_base() {
        let mut map = menu_over_gameplay();
        assert_eq!(
            map.push_context("nope"),
            Err(ActionMapError::UnknownContext("nope".to_owned())),
        );
        assert_eq!(
            map.push_context(GAMEPLAY_CONTEXT),
            Err(ActionMapError::ContextAlreadyActive(
                GAMEPLAY_CONTEXT.to_owned()
            )),
        );
        assert_eq!(
            map.pop_context(GAMEPLAY_CONTEXT),
            Err(ActionMapError::ContextNotOnTop(GAMEPLAY_CONTEXT.to_owned())),
        );
        assert!(map.is_context_active(GAMEPLAY_CONTEXT));
        assert!(!map.is_context_active("menu"));
        assert!(!map.is_context_active("nope"));
    }

    /// The pointer, a button and an on-screen control are consumed like keys.
    #[test]
    fn pointer_inputs_and_controls_are_consumed_too() {
        let mut map = ActionMap::new();
        map.declare(button(
            "fire",
            vec![
                Binding::MouseButton(PointerButton::Left),
                Binding::Virtual("btn".to_owned()),
            ],
        ));
        map.declare(ActionDecl {
            name: "aim".to_owned(),
            kind: ActionKind::Axis1,
            bindings: vec![Binding::PointerPosition {
                axis: crate::PointerAxis::X,
            }],
        });
        map.declare_in(
            "menu",
            ActionDecl {
                name: "cursor".to_owned(),
                kind: ActionKind::Axis1,
                bindings: vec![Binding::PointerPosition {
                    axis: crate::PointerAxis::X,
                }],
            },
        );
        map.declare_in(
            "menu",
            button(
                "click",
                vec![
                    Binding::MouseButton(PointerButton::Left),
                    Binding::Virtual("btn".to_owned()),
                ],
            ),
        );
        map.push_context("menu").expect("declared");

        map.begin_tick(TICK);
        map.pointer_position(0.5, 0.0);
        map.mouse_button(PointerButton::Left, true);
        map.virtual_button("btn", true);
        assert!(map.just_pressed("click"));
        assert!(!map.button_held("fire"));
        assert_eq!(map.axis1("cursor"), 0.5);
        assert_eq!(
            map.axis1("aim"),
            0.0,
            "the menu's pointer is not the game's"
        );
        let Some(ActionValue::Axis1(aim)) = map.action("aim") else {
            panic!("aim is an Axis1");
        };
        assert!(!aim.pointer_moved, "nor did it move for the game");

        // And not on the next tick either, which re-resolves every action.
        map.begin_tick(TICK);
        assert_eq!(map.axis1("aim"), 0.0, "a consumed pointer stays consumed");
        assert!(!map.button_held("fire"));
    }

    /// A rebind can take a key from a context beneath, and the context beneath
    /// releases it on the spot.
    #[test]
    fn a_rebind_moves_ownership_and_re_resolves_the_loser() {
        let mut map = menu_over_gameplay();
        map.push_context("menu").expect("declared");
        map.key_event(KeyCode::KeyC, true);
        assert!(map.button_held("crouch"));

        map.rebind("accept", vec![Binding::Key(KeyCode::KeyC)])
            .expect("declared");
        assert!(
            !map.button_held("crouch"),
            "menu took C, so gameplay lost it"
        );
        assert!(
            map.button_held("accept"),
            "and a rebind resolves against held keys"
        );
    }

    /// **Chords take the key from plain bindings on it**: Tab and Shift+Tab in
    /// one context never both fire, and the modifier is not consumed.
    #[test]
    fn a_chord_shadows_the_plain_key_and_leaves_its_modifier_alone() {
        let mut map = ActionMap::new();
        map.declare(button("sprint", vec![Binding::Key(KeyCode::ShiftLeft)]));
        map.declare_in("menu", button("next", vec![Binding::Key(KeyCode::Tab)]));
        map.declare_in(
            "menu",
            button(
                "prev",
                vec![Binding::Chord {
                    modifier: Modifier::Shift,
                    key: KeyCode::Tab,
                }],
            ),
        );
        map.push_context("menu").expect("declared");

        map.begin_tick(TICK);
        map.key_event(KeyCode::Tab, true);
        assert!(map.button_held("next") && !map.button_held("prev"));

        map.key_event(KeyCode::ShiftLeft, true);
        assert!(
            map.just_released("next"),
            "Shift turns the held Tab into Shift+Tab"
        );
        assert!(map.just_pressed("prev"));
        assert!(
            map.button_held("sprint"),
            "the modifier still reaches gameplay"
        );

        map.key_event(KeyCode::ShiftLeft, false);
        assert!(!map.button_held("prev") && map.button_held("next"));
    }

    /// **The same scripted input yields the same action stream twice**, from
    /// two maps built independently — each with its own hash seeds, so a rule
    /// that leaned on a `HashMap`'s iteration order would split them.
    #[test]
    fn a_scripted_sequence_yields_the_same_action_stream_twice() {
        fn run() -> Vec<String> {
            let mut map = ActionMap::new();
            map.declare(ActionDecl {
                name: "move".to_owned(),
                kind: ActionKind::Axis2,
                bindings: vec![Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                }],
            });
            map.declare(button("jump", vec![Binding::Key(KeyCode::Space)]));
            crate::ui::declare(&mut map).expect("no clash with move or jump");
            map.set_repeat("jump", Some(Repeat::UI)).expect("declared");

            let script: &[(u32, KeyCode, bool)] = &[
                (0, KeyCode::KeyD, true),
                (1, KeyCode::Space, true),
                (3, KeyCode::KeyW, true),
                (5, KeyCode::Tab, true),
                (6, KeyCode::ShiftRight, true),
                (9, KeyCode::Space, false),
                (12, KeyCode::KeyD, false),
                (14, KeyCode::Space, true),
                (20, KeyCode::KeyW, false),
                (40, KeyCode::Tab, false),
                (41, KeyCode::ShiftRight, false),
                (44, KeyCode::Space, false),
            ];
            let mut stream = Vec::new();
            for tick in 0..60 {
                map.begin_tick(TICK);
                match tick {
                    4 | 30 => map.push_context(crate::ui::CONTEXT).expect("off the stack"),
                    18 | 50 => map.pop_context(crate::ui::CONTEXT).expect("on top"),
                    _ => {}
                }
                for &(at, key, pressed) in script {
                    if at == tick {
                        map.key_event(key, pressed);
                    }
                }
                let repeats: Vec<bool> =
                    map.action_names().map(|name| map.repeated(name)).collect();
                stream.push(format!(
                    "{tick} {:?} {repeats:?} {:?}",
                    InputTickState::capture(&map),
                    map.last_device(),
                ));
            }
            stream
        }

        let first = run();
        assert_eq!(first, run());
        assert!(
            first.iter().any(|line| line.contains("just_pressed: true")),
            "the script pressed something, or equality says nothing",
        );
    }
}
