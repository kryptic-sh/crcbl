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
//! controls by id, the pointer's position, motion and wheel each as one input
//! (the wheel taken by a [`Binding::ScrollChord`] as by a
//! [`Binding::MouseScroll`], and not its held key), and pad buttons, sticks
//! and triggers — every pad's South is one input, as every pad drives every
//! pad binding.
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
//!
//! # Withholding on request
//!
//! [`ActionMap::suppress_held`] withholds **every** held input, whoever owns
//! it, and [`ActionMap::suppress_held_action`] every held input one action's
//! bindings read — the walk held into a revival, a raid resolving or a menu
//! that is not a context. It is the same withholding a stack change does, and
//! it too cancels the patterns in flight, but it is wider in two ways:
//!
//! - **It withholds the input, not the action.** Every action reading a
//!   withheld input reads it as up, not only the one named; an action sharing
//!   none of them is untouched.
//! - **Levels are withheld too**, because the request is "nothing held moves
//!   the player": a pad stick, a pad trigger and an on-screen stick.
//!
//! What lifts it — what "pressed again" means — is per input:
//!
//! - **A key, pointer button, on-screen button or pad button**: released, as
//!   for a stack change. Its next press reads. A [`Binding::Wasd`] or
//!   [`Binding::KeyAxis`] withholds per key, so a key still held reads zero
//!   while a key pressed afresh moves the axis; a [`Binding::Chord`] is
//!   withheld through its key, and reads again when the key is pressed again
//!   with the modifier still down, since the modifier is read raw.
//! - **A pad stick**: back at rest on every pad — inside the dead zone of every
//!   [`Binding::PadStick`] on it, or exactly centred if none reads it. Once
//!   lifted, deflecting it reads at once: returning to rest is its release.
//! - **A pad trigger**: back at or below the threshold of every
//!   [`Binding::PadTrigger`] on it, on every pad, or fully out if none reads it.
//! - **An on-screen stick**: reporting exactly `(0.0, 0.0)`, which is what a
//!   stick nobody touches reports.
//!
//! The pointer's position is a place and is never withheld, and its motion and
//! wheel are deltas that nothing holds.

use std::collections::{HashMap, HashSet};

use crcbl_core::input::{KeyCode, PointerButton};

use super::{
    ActionMap, ActionMapError, Binding, HeldKeys, Modifier, PadButton, PadButtons, Stick, Trigger,
};

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
    /// The held keys of the scroll chords the wheel's **owner** binds: what
    /// shadows that owner's plain wheel bindings.
    scroll_chords: Vec<KeyCode>,
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
    /// The levels only [`ActionMap::suppress_held`] and
    /// [`ActionMap::suppress_held_action`] withhold, each until it rests.
    pub(crate) control_sticks: HashSet<String>,
    pub(crate) pad_sticks: HashSet<Stick>,
    pub(crate) pad_triggers: HashSet<Trigger>,
}

impl Suppressed {
    /// Withhold everything `other` does as well.
    fn extend(&mut self, other: Self) {
        self.keys.extend(other.keys);
        self.buttons.extend(other.buttons);
        self.controls.extend(other.controls);
        self.pad_buttons = self.pad_buttons.union(other.pad_buttons);
        self.control_sticks.extend(other.control_sticks);
        self.pad_sticks.extend(other.pad_sticks);
        self.pad_triggers.extend(other.pad_triggers);
    }

    /// Whether `binding` reads any input this withholds.
    fn withholds(&self, binding: &Binding) -> bool {
        let mut key = false;
        binding.visit_keys(|owned| key |= self.keys.contains(&owned));
        key || match binding {
            Binding::ScrollChord { held } => self.keys.contains(held),
            Binding::MouseButton(button) => self.buttons.contains(button),
            Binding::Virtual(id) => {
                self.controls.contains(id.as_str()) || self.control_sticks.contains(id.as_str())
            }
            Binding::PadButton(button) => self.pad_buttons.contains(*button),
            Binding::PadDpad => PadButton::DPAD
                .iter()
                .any(|&button| self.pad_buttons.contains(button)),
            Binding::PadStick { stick, .. } => self.pad_sticks.contains(stick),
            Binding::PadTrigger { trigger, .. } => self.pad_triggers.contains(trigger),
            Binding::Key(_)
            | Binding::KeyAxis { .. }
            | Binding::Chord { .. }
            | Binding::Wasd { .. }
            | Binding::MouseMotion
            | Binding::MouseScroll
            | Binding::PointerPosition { .. } => false,
        }
    }
}

/// What one context can see of the raw input.
pub(crate) struct View<'a> {
    pub(crate) context: usize,
    pub(crate) routes: &'a Routes,
    pub(crate) suppressed: &'a Suppressed,
    pub(crate) held_keys: &'a HeldKeys,
    pub(crate) held_buttons: &'a HashSet<PointerButton>,
    pub(crate) held_controls: &'a HashSet<String>,
    /// Every button some pad holds.
    pub(crate) held_pad_buttons: PadButtons,
}

impl View<'_> {
    /// The key is held, this context owns it, and it is not withheld.
    fn owns_held(&self, key: KeyCode) -> bool {
        self.held_keys.contains_key(&key)
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
        self.held_controls.contains(id)
            && !self.suppressed.controls.contains(id)
            && self.owns_control(id)
    }

    /// A stick's deflection is a level: a stack change never withholds it, and
    /// only a suppress does.
    pub(crate) fn stick(&self, id: &str) -> bool {
        self.owns_control(id) && !self.suppressed.control_sticks.contains(id)
    }

    fn owns_control(&self, id: &str) -> bool {
        self.routes.controls.get(id) == Some(&self.context)
    }

    pub(crate) fn motion(&self) -> bool {
        self.routes.motion == Some(self.context)
    }

    /// A plain [`Binding::MouseScroll`]'s read: this context owns the wheel,
    /// and none of its scroll chords' keys is down.
    pub(crate) fn scroll(&self) -> bool {
        self.routes.scroll == Some(self.context) && self.wheel_key().is_none()
    }

    /// A [`Binding::ScrollChord`]'s read: this context owns the wheel, and
    /// `held` is the most recently pressed of its scroll chords' keys down.
    pub(crate) fn scroll_chord(&self, held: KeyCode) -> bool {
        self.routes.scroll == Some(self.context) && self.wheel_key() == Some(held)
    }

    /// The most recently pressed scroll-chord key that is down and not
    /// withheld, in the context that owns the wheel.
    fn wheel_key(&self) -> Option<KeyCode> {
        self.routes
            .scroll_chords
            .iter()
            .filter(|key| !self.suppressed.keys.contains(key))
            .filter_map(|&key| self.held_keys.get(&key).map(|&serial| (serial, key)))
            .max()
            .map(|(_, key)| key)
    }

    pub(crate) fn pointer(&self) -> bool {
        self.routes.pointer == Some(self.context)
    }

    pub(crate) fn pad_button(&self, button: PadButton) -> bool {
        self.held_pad_buttons.contains(button)
            && !self.suppressed.pad_buttons.contains(button)
            && self.routes.pad_buttons.get(&button) == Some(&self.context)
    }

    /// A stick is a level: a stack change never withholds it, and only a
    /// suppress does.
    pub(crate) fn pad_stick(&self, stick: Stick) -> bool {
        self.routes.pad_sticks.get(&stick) == Some(&self.context)
            && !self.suppressed.pad_sticks.contains(&stick)
    }

    /// A trigger is a level: a stack change never withholds it, and only a
    /// suppress does.
    pub(crate) fn trigger(&self, trigger: Trigger) -> bool {
        self.routes.pad_triggers.get(&trigger) == Some(&self.context)
            && !self.suppressed.pad_triggers.contains(&trigger)
    }
}

impl Routes {
    /// Whether `key` is the held key of a scroll chord the wheel's owner binds.
    pub(crate) fn is_scroll_chord_key(&self, key: KeyCode) -> bool {
        self.scroll_chords.contains(&key)
    }

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
                    Binding::MouseScroll | Binding::ScrollChord { .. } => {
                        routes.scroll.get_or_insert(context);
                    }
                    Binding::PointerPosition { .. } => {
                        routes.pointer.get_or_insert(context);
                    }
                    Binding::PadButton(button) => {
                        routes.pad_buttons.entry(*button).or_insert(context);
                    }
                    Binding::PadDpad => {
                        for button in PadButton::DPAD {
                            routes.pad_buttons.entry(button).or_insert(context);
                        }
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
            // After this context's keys and wheel are claimed, so a chord
            // registers only where its context is the one reading the key, and
            // a scroll chord only where its context is the one reading the
            // wheel.
            //
            // A scroll chord competes for the wheel only while its action is
            // enabled: a newer key whose action cannot use the wheel right now
            // must not take it from an older one that can, nor silence the
            // plain wheel. The wheel itself stays this context's either way.
            let slots = map.slots.iter().filter(|slot| slot.context == context);
            for slot in slots {
                for binding in &slot.decl.bindings {
                    match binding {
                        Binding::Chord { modifier, key }
                            if routes.keys.get(key) == Some(&context) =>
                        {
                            let modifiers = routes.chords.entry(*key).or_default();
                            if !modifiers.contains(modifier) {
                                modifiers.push(*modifier);
                            }
                        }
                        Binding::ScrollChord { held }
                            if slot.enabled
                                && routes.scroll == Some(context)
                                && !routes.scroll_chords.contains(held) =>
                        {
                            routes.scroll_chords.push(*held);
                        }
                        _ => {}
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
        self.resolve_live();
    }

    /// Re-resolve every live action.
    fn resolve_live(&mut self) {
        for idx in 0..self.slots.len() {
            if self.is_live(idx) {
                self.resolve_one(idx);
            }
        }
    }

    /// Withhold every input held right now, whoever owns it, until it is
    /// released — or for a level, until it rests — and cancel every pattern in
    /// flight. See the module docs for what lifts each input.
    ///
    /// For a state change that is not a context: movement keys held when a
    /// revival starts or a menu opens stop moving the player, and move it again
    /// only once pressed again. Every action re-resolves, so one that was down
    /// reads as released on this tick.
    pub fn suppress_held(&mut self) {
        let withheld = Suppressed {
            keys: self.held_keys.keys().copied().collect(),
            buttons: self.held_buttons.clone(),
            controls: self.held_controls.clone(),
            pad_buttons: self.held_pad_buttons,
            control_sticks: self
                .control_sticks
                .iter()
                .filter(|(_, deflection)| **deflection != (0.0, 0.0))
                .map(|(id, _)| id.clone())
                .collect(),
            pad_sticks: [Stick::Left, Stick::Right]
                .into_iter()
                .filter(|&stick| !self.stick_rests(stick))
                .collect(),
            pad_triggers: [Trigger::Left, Trigger::Right]
                .into_iter()
                .filter(|&trigger| !self.trigger_rests(trigger))
                .collect(),
        };
        for slot in &mut self.slots {
            slot.patterns.cancel();
        }
        self.withhold(withheld);
    }

    /// Withhold every input held right now that one action's bindings read,
    /// as [`ActionMap::suppress_held`] does for all of them, and cancel the
    /// patterns in flight on that action and on every action reading one of
    /// those inputs.
    ///
    /// The input is withheld, not the action: another action bound to the same
    /// key reads it as up too — see the module docs. A [`Binding::Chord`]
    /// withholds its key and not its modifier, and a [`Binding::ScrollChord`]
    /// its held key.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn suppress_held_action(&mut self, name: &str) -> Result<(), ActionMapError> {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return Err(ActionMapError::UnknownAction(name.to_owned()));
        };
        let mut withheld = Suppressed::default();
        for binding in &self.slots[idx].decl.bindings {
            self.held_by(binding, &mut withheld);
        }
        self.slots[idx].patterns.cancel();
        for slot in &mut self.slots {
            if slot.decl.bindings.iter().any(|b| withheld.withholds(b)) {
                slot.patterns.cancel();
            }
        }
        self.withhold(withheld);
        Ok(())
    }

    /// Add to `withheld` every input `binding` reads that is held right now:
    /// a key down, a stick or trigger off rest, an on-screen stick off centre.
    fn held_by(&self, binding: &Binding, withheld: &mut Suppressed) {
        binding.visit_keys(|key| {
            if self.held_keys.contains_key(&key) {
                withheld.keys.insert(key);
            }
        });
        match binding {
            Binding::ScrollChord { held } if self.held_keys.contains_key(held) => {
                withheld.keys.insert(*held);
            }
            Binding::MouseButton(button) if self.held_buttons.contains(button) => {
                withheld.buttons.insert(*button);
            }
            Binding::Virtual(id) => {
                if self.held_controls.contains(id.as_str()) {
                    withheld.controls.insert(id.clone());
                }
                if self
                    .control_sticks
                    .get(id.as_str())
                    .is_some_and(|&deflection| deflection != (0.0, 0.0))
                {
                    withheld.control_sticks.insert(id.clone());
                }
            }
            Binding::PadButton(button) if self.held_pad_buttons.contains(*button) => {
                withheld.pad_buttons.insert(*button);
            }
            Binding::PadDpad => {
                for button in PadButton::DPAD {
                    if self.held_pad_buttons.contains(button) {
                        withheld.pad_buttons.insert(button);
                    }
                }
            }
            Binding::PadStick { stick, .. } if !self.stick_rests(*stick) => {
                withheld.pad_sticks.insert(*stick);
            }
            Binding::PadTrigger { trigger, .. } if !self.trigger_rests(*trigger) => {
                withheld.pad_triggers.insert(*trigger);
            }
            _ => {}
        }
    }

    /// Add `withheld` to what is withheld, and re-resolve every live action
    /// against it. The caller cancels the patterns first, so a press taken
    /// away is not read as a release that taps.
    fn withhold(&mut self, withheld: Suppressed) {
        self.suppressed.extend(withheld);
        self.resolve_live();
    }

    /// Rebuild the routes after the stack changed: withhold every held input
    /// whose owner moved, idle the actions of contexts that left, and
    /// re-resolve the rest.
    fn restack(&mut self) {
        let routes = Routes::build(self);
        let old = std::mem::replace(&mut self.routes, routes);
        let new = &self.routes;
        for key in self.held_keys.keys() {
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

    fn axis(name: &str, kind: ActionKind, bindings: Vec<Binding>) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind,
            bindings,
        }
    }

    fn wasd() -> Binding {
        Binding::Wasd {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        }
    }

    /// **`suppress_held` withholds a held key until it is pressed again**: the
    /// action reads released at once, an OS auto-repeat does not bring it back,
    /// and a fresh press after the lift does.
    #[test]
    fn suppress_held_withholds_a_key_until_it_is_pressed_again() {
        let mut map = menu_over_gameplay();
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        map.begin_tick(TICK);
        map.suppress_held();
        assert!(map.just_released("jump"), "the walk stops on the spot");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true); // an OS auto-repeat
        assert!(!map.button_held("jump"));
        map.key_event(KeyCode::Space, false);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed("jump"), "pressed again");
    }

    /// **Suppressing mid-press fires neither the hold nor the tap**, and the
    /// press after the release fires as usual.
    #[test]
    fn suppress_held_mid_press_fires_no_pattern_and_the_next_press_does() {
        let mut map = menu_over_gameplay();
        map.set_tap("jump", Some(crate::Tap::default()))
            .expect("declared");
        map.set_hold("jump", Some(crate::Hold::default()))
            .expect("declared");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        map.begin_tick(TICK);
        map.suppress_held();
        let mut fired = map.tapped("jump");
        for _ in 0..60 {
            map.begin_tick(TICK);
            fired |= map.hold_fired("jump");
        }
        map.key_event(KeyCode::Space, false);
        fired |= map.tapped("jump");
        assert!(!fired, "neither a hold nor a tap");

        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, false);
        assert!(map.tapped("jump"), "suppress, release, press: it taps");
    }

    /// A first tap waiting for its second is dropped by `suppress_held`, with
    /// nothing held at the time.
    #[test]
    fn suppress_held_drops_a_waiting_first_tap() {
        let mut map = menu_over_gameplay();
        map.set_double_tap("jump", Some(crate::DoubleTap::default()))
            .expect("declared");
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        map.key_event(KeyCode::Space, false);
        map.suppress_held();
        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(!map.double_tapped("jump"));
    }

    /// **A `Wasd` axis is withheld per key**: a key held through the suppress
    /// reads zero until pressed again, and a key pressed afresh moves the axis
    /// meanwhile. A `KeyAxis` likewise.
    #[test]
    fn suppress_held_withholds_an_axis_per_key() {
        let mut map = ActionMap::new();
        map.declare(axis("move", ActionKind::Axis2, vec![wasd()]));
        map.declare(axis(
            "zoom",
            ActionKind::Axis1,
            vec![Binding::KeyAxis {
                negative: KeyCode::KeyQ,
                positive: KeyCode::KeyE,
            }],
        ));
        map.key_event(KeyCode::KeyW, true);
        map.key_event(KeyCode::KeyE, true);
        map.suppress_held();
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        assert_eq!(map.axis1("zoom"), 0.0);

        map.begin_tick(TICK);
        assert_eq!(map.axis2("move"), (0.0, 0.0), "still held, still withheld");
        map.key_event(KeyCode::KeyD, true);
        assert_eq!(map.axis2("move"), (1.0, 0.0), "a fresh key moves");
        map.key_event(KeyCode::KeyQ, true);
        assert_eq!(map.axis1("zoom"), -1.0);

        map.key_event(KeyCode::KeyW, false);
        map.key_event(KeyCode::KeyW, true);
        let (x, y) = map.axis2("move");
        assert!(
            x > 0.0 && y > 0.0,
            "W pressed again: diagonal, got {x}, {y}"
        );
    }

    /// A chord is withheld through its key: pressed again with the modifier
    /// still down, it reads again.
    #[test]
    fn suppress_held_withholds_a_chord_until_its_key_is_pressed_again() {
        let mut map = ActionMap::new();
        map.declare(button(
            "save",
            vec![Binding::Chord {
                modifier: Modifier::Control,
                key: KeyCode::KeyS,
            }],
        ));
        map.key_event(KeyCode::ControlLeft, true);
        map.key_event(KeyCode::KeyS, true);
        assert!(map.button_held("save"));
        map.suppress_held();
        assert!(!map.button_held("save"));
        map.key_event(KeyCode::KeyS, false);
        map.key_event(KeyCode::KeyS, true);
        assert!(map.just_pressed("save"), "Ctrl never let go");
    }

    /// Pointer buttons, on-screen buttons and on-screen sticks are withheld
    /// too, a stick until it reports centre.
    #[test]
    fn suppress_held_withholds_pointer_buttons_and_on_screen_controls() {
        let mut map = ActionMap::new();
        map.declare(button(
            "fire",
            vec![
                Binding::MouseButton(PointerButton::Left),
                Binding::Virtual("btn".to_owned()),
            ],
        ));
        map.declare(axis(
            "move",
            ActionKind::Axis2,
            vec![Binding::Virtual("stick".to_owned())],
        ));
        map.mouse_button(PointerButton::Left, true);
        map.virtual_button("btn", true);
        map.virtual_stick("stick", 0.0, 1.0);
        map.suppress_held();
        assert!(!map.button_held("fire"));
        assert_eq!(map.axis2("move"), (0.0, 0.0));

        map.virtual_stick("stick", 0.5, 0.5);
        assert_eq!(map.axis2("move"), (0.0, 0.0), "moved, never centred");
        map.virtual_stick("stick", 0.0, 0.0);
        map.virtual_stick("stick", 0.0, 1.0);
        assert_eq!(map.axis2("move"), (0.0, 1.0), "centred, then pushed");

        map.mouse_button(PointerButton::Left, false);
        assert!(
            !map.button_held("fire"),
            "the on-screen button is still held"
        );
        map.virtual_button("btn", false);
        map.mouse_button(PointerButton::Left, true);
        assert!(map.just_pressed("fire"));
    }

    fn pad(snapshot: impl FnOnce(&mut crate::GamepadSnapshot)) -> crate::GamepadEvent {
        let mut state = crate::GamepadSnapshot::neutral(crate::PadKind::Xbox);
        snapshot(&mut state);
        crate::GamepadEvent::State {
            id: crate::GamepadId(1),
            snapshot: state,
        }
    }

    fn stick_y(y: f32) -> crate::GamepadEvent {
        pad(|state| state.axes[crate::PadAxis::LeftY as usize] = y)
    }

    /// **A pad stick is withheld until it rests inside its dead zone**, and a
    /// trigger until it is back under its threshold; a pad button until it is
    /// released.
    #[test]
    fn suppress_held_withholds_pad_buttons_sticks_and_triggers() {
        let mut map = ActionMap::new();
        map.declare(axis(
            "move",
            ActionKind::Axis2,
            vec![Binding::PadStick {
                stick: Stick::Left,
                deadzone: 0.2,
            }],
        ));
        map.declare(button(
            "fire",
            vec![Binding::PadTrigger {
                trigger: Trigger::Right,
                threshold: 0.5,
            }],
        ));
        map.declare(button("jump", vec![Binding::PadButton(PadButton::South)]));
        map.gamepad_event(&pad(|state| {
            state.axes[crate::PadAxis::LeftY as usize] = 1.0;
            state.axes[crate::PadAxis::RightTrigger as usize] = 1.0;
            state.buttons.insert(PadButton::South);
        }));
        assert_eq!(map.axis2("move"), (0.0, 1.0));
        assert!(map.button_held("fire") && map.button_held("jump"));
        map.suppress_held();
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        assert!(!map.button_held("fire") && !map.button_held("jump"));

        map.gamepad_event(&stick_y(0.8));
        assert_eq!(map.axis2("move"), (0.0, 0.0), "eased off, never rested");
        assert!(!map.button_held("jump"), "and South was let go of");
        map.gamepad_event(&stick_y(0.1));
        map.gamepad_event(&stick_y(1.0));
        assert_eq!(map.axis2("move"), (0.0, 1.0), "rested, then pushed");
        map.gamepad_event(&pad(|state| {
            state.axes[crate::PadAxis::RightTrigger as usize] = 1.0;
            state.buttons.insert(PadButton::South);
        }));
        assert!(map.button_held("fire") && map.button_held("jump"));
    }

    /// **`suppress_held_action` withholds only what that action reads**: an
    /// action on another held key keeps it, and an action sharing the key
    /// loses it too.
    #[test]
    fn suppress_held_action_withholds_only_that_actions_inputs() {
        let mut map = ActionMap::new();
        map.declare(axis("move", ActionKind::Axis2, vec![wasd()]));
        map.declare(button("forward", vec![Binding::Key(KeyCode::KeyW)]));
        map.declare(button("jump", vec![Binding::Key(KeyCode::Space)]));
        map.key_event(KeyCode::KeyW, true);
        map.key_event(KeyCode::Space, true);
        map.suppress_held_action("move").expect("declared");
        assert_eq!(map.axis2("move"), (0.0, 0.0));
        assert!(!map.button_held("forward"), "W itself is withheld");
        assert!(map.button_held("jump"), "Space is not move's");
        map.key_event(KeyCode::KeyW, false);
        map.key_event(KeyCode::KeyW, true);
        assert_eq!(map.axis2("move"), (0.0, 1.0));
        assert_eq!(
            map.suppress_held_action("nope"),
            Err(ActionMapError::UnknownAction("nope".to_owned())),
        );
    }

    /// `zoom` on the plain wheel, `zoom_z` on Z+wheel, `zoom_ctrl` on
    /// Ctrl+wheel, and `z` on Z itself, all in gameplay.
    fn scroll_chords() -> ActionMap {
        let mut map = ActionMap::new();
        map.declare(axis("zoom", ActionKind::Axis1, vec![Binding::MouseScroll]));
        for (name, held) in [
            ("zoom_z", KeyCode::KeyZ),
            ("zoom_ctrl", KeyCode::ControlLeft),
        ] {
            map.declare(axis(
                name,
                ActionKind::Axis1,
                vec![Binding::ScrollChord { held }],
            ));
        }
        map.declare(button("z", vec![Binding::Key(KeyCode::KeyZ)]));
        map
    }

    /// Scroll one detent on a fresh tick and read who got it.
    fn wheel(map: &mut ActionMap) -> [f32; 3] {
        map.begin_tick(TICK);
        map.mouse_scroll(0.0, 1.0);
        ["zoom", "zoom_z", "zoom_ctrl"].map(|name| map.axis1(name))
    }

    /// **A disabled action's scroll chord takes the wheel from nothing.** Ctrl
    /// goes down for `zoom_ctrl`, then Z for `zoom_z` while `zoom_z` is
    /// disabled: the wheel stays Ctrl's. With Ctrl up too, the plain wheel is
    /// not silenced by Z. Enabled again, Z takes the wheel back from Ctrl by
    /// when it went down, which was after Ctrl, not by when it was enabled.
    #[test]
    fn a_disabled_scroll_chord_takes_the_wheel_from_nothing() {
        let mut map = scroll_chords();
        map.key_event(KeyCode::ControlLeft, true);
        map.set_enabled("zoom_z", false);
        map.key_event(KeyCode::KeyZ, true);
        assert_eq!(wheel(&mut map), [0.0, 0.0, 1.0], "Ctrl keeps the wheel");

        map.key_event(KeyCode::ControlLeft, false);
        assert_eq!(wheel(&mut map), [1.0, 0.0, 0.0], "Z silences nothing");

        map.key_event(KeyCode::ControlLeft, true);
        map.set_enabled("zoom_z", true);
        assert_eq!(
            wheel(&mut map),
            [0.0, 0.0, 1.0],
            "Ctrl went down after Z, so it is the newer press"
        );
        map.key_event(KeyCode::ControlLeft, false);
        map.key_event(KeyCode::ControlLeft, true);
        map.key_event(KeyCode::KeyZ, false);
        map.key_event(KeyCode::KeyZ, true);
        map.set_enabled("zoom_z", false);
        map.set_enabled("zoom_z", true);
        assert_eq!(
            wheel(&mut map),
            [0.0, 1.0, 0.0],
            "Z pressed after Ctrl takes it back once enabled"
        );
    }

    /// **A scroll chord takes the wheel from the plain binding while its key is
    /// down**, gives it back on release, and reads the key without consuming
    /// it.
    #[test]
    fn a_scroll_chord_takes_the_wheel_while_its_key_is_down() {
        let mut map = scroll_chords();
        assert_eq!(wheel(&mut map), [1.0, 0.0, 0.0]);
        map.key_event(KeyCode::KeyZ, true);
        assert_eq!(wheel(&mut map), [0.0, 1.0, 0.0]);
        assert!(map.button_held("z"), "Z is read, not consumed");
        map.key_event(KeyCode::KeyZ, false);
        assert_eq!(wheel(&mut map), [1.0, 0.0, 0.0], "the wheel is back");
    }

    /// **With two scroll-chord keys down, the most recently pressed one takes
    /// the wheel**, in either order, and releasing it hands the wheel back to
    /// the other; an auto-repeat of the earlier key takes nothing.
    #[test]
    fn the_most_recently_pressed_scroll_chord_key_takes_the_wheel() {
        let mut map = scroll_chords();
        map.key_event(KeyCode::ControlLeft, true);
        map.key_event(KeyCode::KeyZ, true);
        assert_eq!(wheel(&mut map), [0.0, 1.0, 0.0], "Ctrl then Z: Z's");
        map.key_event(KeyCode::ControlLeft, true); // an OS auto-repeat
        assert_eq!(wheel(&mut map), [0.0, 1.0, 0.0], "a repeat is no press");
        map.key_event(KeyCode::KeyZ, false);
        assert_eq!(wheel(&mut map), [0.0, 0.0, 1.0], "back to Ctrl");
        map.key_event(KeyCode::ControlLeft, false);

        map.key_event(KeyCode::KeyZ, true);
        map.key_event(KeyCode::ControlLeft, true);
        assert_eq!(wheel(&mut map), [0.0, 0.0, 1.0], "Z then Ctrl: Ctrl's");
        map.key_event(KeyCode::ControlLeft, false);
        assert_eq!(wheel(&mut map), [0.0, 1.0, 0.0], "back to Z");
        map.key_event(KeyCode::KeyZ, false);
        assert_eq!(wheel(&mut map), [1.0, 0.0, 0.0]);
    }

    /// A key pressed after the wheel turned, in the same tick, moves that
    /// tick's scroll to the chord: the key re-resolves the wheel's bindings.
    #[test]
    fn a_scroll_chord_key_pressed_mid_tick_takes_that_ticks_scroll() {
        let mut map = scroll_chords();
        map.begin_tick(TICK);
        map.mouse_scroll(0.0, 1.0);
        map.key_event(KeyCode::KeyZ, true);
        assert_eq!(["zoom", "zoom_z"].map(|name| map.axis1(name)), [0.0, 1.0]);
    }

    /// **A scroll chord in a pushed context takes the wheel for it**, and its
    /// key shadows only that context's plain wheel: the context beneath gets
    /// no wheel at all, as with any consumed input.
    #[test]
    fn a_scroll_chord_claims_the_wheel_for_its_context() {
        let mut map = ActionMap::new();
        map.declare(axis("zoom", ActionKind::Axis1, vec![Binding::MouseScroll]));
        map.declare_in(
            "map",
            axis(
                "pan",
                ActionKind::Axis1,
                vec![Binding::ScrollChord {
                    held: KeyCode::AltLeft,
                }],
            ),
        );
        map.push_context("map").expect("declared");
        map.begin_tick(TICK);
        map.mouse_scroll(0.0, 1.0);
        assert_eq!(map.axis1("zoom"), 0.0, "the pushed context owns the wheel");
        assert_eq!(map.axis1("pan"), 0.0, "and Alt is not down");
        map.key_event(KeyCode::AltLeft, true);
        assert_eq!(map.axis1("pan"), 1.0);
    }

    /// A scroll chord's key withheld by `suppress_held` reads as up: the plain
    /// wheel has it until the key is pressed again.
    #[test]
    fn a_suppressed_scroll_chord_key_leaves_the_wheel_plain() {
        let mut map = scroll_chords();
        map.key_event(KeyCode::KeyZ, true);
        map.suppress_held();
        assert_eq!(wheel(&mut map), [1.0, 0.0, 0.0]);
        map.key_event(KeyCode::KeyZ, false);
        map.key_event(KeyCode::KeyZ, true);
        assert_eq!(wheel(&mut map), [0.0, 1.0, 0.0]);
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
