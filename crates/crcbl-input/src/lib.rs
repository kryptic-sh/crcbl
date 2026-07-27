//! Device-agnostic action system: gameplay and UI code consume **actions**,
//! never raw devices. Keyboard, mouse, and future gamepad/touch are
//! interchangeable binding sources behind one config layer.

use crcbl_core::input::{KeyCode, PointerButton};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Action primitives
// ---------------------------------------------------------------------------

/// A digital action: pressed / released / held with duration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonState {
    /// The button just went down this tick (or is being pressed for the first time).
    Pressed,
    /// The button came up this tick.
    Released,
    /// The button has been held for `duration` seconds.
    Held {
        /// Seconds since the button first went down.
        duration: f32,
    },
}

/// Button action with edge-detection flags.
///
/// `just_pressed` and `just_released` are true only on the tick the transition
/// occurred. They are cleared by [`ActionMap::begin_tick`] (and may also be
/// cleared earlier by the consumer via [`ActionMap::action_mut`]).
#[derive(Debug, Clone)]
pub struct ButtonAction {
    /// Current button state.
    pub state: ButtonState,
    /// True on the tick the button went down.
    pub just_pressed: bool,
    /// True on the tick the button came up.
    pub just_released: bool,
}

impl ButtonAction {
    fn reset_edges(&mut self) {
        self.just_pressed = false;
        self.just_released = false;
    }
}

impl Default for ButtonAction {
    fn default() -> Self {
        Self {
            state: ButtonState::Released,
            just_pressed: false,
            just_released: false,
        }
    }
}

/// 1-D analog axis: −1.0 … 1.0 (triggers, scroll wheels).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axis1Action {
    /// Current axis value.
    pub value: f32,
}

/// 2-D analog axis: normalized vector (WASD composite, stick, mouse motion).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axis2Action {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
}

/// A unified action value — one variant per action kind.
#[derive(Debug, Clone)]
pub enum ActionValue {
    /// A button action.
    Button(ButtonAction),
    /// A 1-D axis action.
    Axis1(Axis1Action),
    /// A 2-D axis action.
    Axis2(Axis2Action),
}

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

/// A binding source: what raw input triggers this action.
///
/// For P2 only keyboard and mouse are supported; gamepad lands at P10 and
/// touch post-MVP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// A single key.
    Key(KeyCode),
    /// A mouse button.
    MouseButton(PointerButton),
    /// Mouse motion (delta in pixels).
    MouseMotion,
    /// Mouse scroll wheel.
    MouseScroll,
    /// WASD composite: four keys mapped to a 2-D axis (unit vector).
    Wasd {
        /// Key for the +Y direction (forward / up).
        up: KeyCode,
        /// Key for the −Y direction (back / down).
        down: KeyCode,
        /// Key for the −X direction (left).
        left: KeyCode,
        /// Key for the +X direction (right).
        right: KeyCode,
    },
}

// ---------------------------------------------------------------------------
// Action declaration
// ---------------------------------------------------------------------------

/// What kind of value an action produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// A digital on/off action with edge detection.
    Button,
    /// A 1-D analog axis.
    Axis1,
    /// A 2-D analog axis.
    Axis2,
}

/// A declared action: name + kind + bindings.
#[derive(Debug, Clone)]
pub struct ActionDecl {
    /// Human-readable name, used as the lookup key in [`ActionMap`].
    pub name: String,
    /// What kind of value this action produces.
    pub kind: ActionKind,
    /// The raw inputs that drive this action.
    pub bindings: Vec<Binding>,
}

// ---------------------------------------------------------------------------
// Internal per-action state
// ---------------------------------------------------------------------------

/// Tracks the resolved value and edge-transition bookkeeping for one action.
#[derive(Debug, Clone)]
struct ActionSlot {
    decl: ActionDecl,
    /// The current resolved value.
    value: ActionValue,
    /// True when at least one binding for this action is "down" (key held,
    /// button held, etc.). Used to detect press / release edges.
    active: bool,
    /// Accumulated time at which the action became active (None when inactive).
    /// Used to compute `Held { duration }`.
    hold_start: Option<f32>,
}

impl ActionSlot {
    fn new(decl: ActionDecl) -> Self {
        let value = match decl.kind {
            ActionKind::Button => ActionValue::Button(ButtonAction::default()),
            ActionKind::Axis1 => ActionValue::Axis1(Axis1Action::default()),
            ActionKind::Axis2 => ActionValue::Axis2(Axis2Action::default()),
        };
        Self {
            decl,
            value,
            active: false,
            hold_start: None,
        }
    }

    /// Returns the declared kind.
    fn kind(&self) -> ActionKind {
        self.decl.kind
    }
}

// ---------------------------------------------------------------------------
// ActionMap
// ---------------------------------------------------------------------------

/// Resolves raw input events to action values.
///
/// Bindings are declared once via [`ActionMap::declare`] and then driven by
/// feeding raw events (`key_event`, `mouse_button`, `mouse_motion`,
/// `mouse_scroll`).  Per-tick bookkeeping is done with [`ActionMap::begin_tick`].
pub struct ActionMap {
    /// All declared actions, in registration order.
    slots: Vec<ActionSlot>,
    /// name → index into `slots`.
    name_to_idx: HashMap<String, usize>,
    /// Set of action names that are currently enabled (default: every declared
    /// action is enabled until explicitly disabled).
    enabled: HashSet<String>,

    // Raw input state -------------------------------------------------------
    held_keys: HashSet<KeyCode>,
    held_buttons: HashSet<PointerButton>,
    mouse_delta: (f32, f32),
    scroll_delta: (f32, f32),

    /// Seconds elapsed since creation (advanced by `begin_tick`).
    elapsed: f32,
}

impl ActionMap {
    /// Create an empty action map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            name_to_idx: HashMap::new(),
            enabled: HashSet::new(),
            held_keys: HashSet::new(),
            held_buttons: HashSet::new(),
            mouse_delta: (0.0, 0.0),
            scroll_delta: (0.0, 0.0),
            elapsed: 0.0,
        }
    }

    /// Register an action.  Actions are enabled by default.
    ///
    /// # Panics
    /// Panics if an action with the same name has already been declared.
    pub fn declare(&mut self, decl: ActionDecl) {
        assert!(
            !self.name_to_idx.contains_key(&decl.name),
            "duplicate action name: {}",
            decl.name
        );
        let idx = self.slots.len();
        self.name_to_idx.insert(decl.name.clone(), idx);
        self.enabled.insert(decl.name.clone());
        self.slots.push(ActionSlot::new(decl));
    }

    /// Enable or disable an action by name.  Disabled actions always return
    /// their default (idle) value and do not react to input.
    ///
    /// Has no effect if the named action does not exist.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        if enabled {
            self.enabled.insert(name.to_owned());
        } else {
            self.enabled.remove(name);
            // Reset the action to idle so it doesn't stick.
            if let Some(&idx) = self.name_to_idx.get(name) {
                let slot = &mut self.slots[idx];
                slot.active = false;
                slot.hold_start = None;
                match &mut slot.value {
                    ActionValue::Button(a) => {
                        a.state = ButtonState::Released;
                        a.just_pressed = false;
                        a.just_released = false;
                    }
                    ActionValue::Axis1(a) => a.value = 0.0,
                    ActionValue::Axis2(a) => {
                        a.x = 0.0;
                        a.y = 0.0;
                    }
                }
            }
        }
    }

    // -- feeding raw events --------------------------------------------------

    /// Feed a key event. `pressed` → key-down, `!pressed` → key-up.
    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            self.held_keys.insert(key);
        } else {
            self.held_keys.remove(&key);
        }
        self.resolve_affected_by_key(key);
    }

    /// Feed a mouse-button event.
    pub fn mouse_button(&mut self, button: PointerButton, pressed: bool) {
        if pressed {
            self.held_buttons.insert(button);
        } else {
            self.held_buttons.remove(&button);
        }
        self.resolve_affected_by_button(button);
    }

    /// Feed mouse motion (delta in pixels since the last event).
    pub fn mouse_motion(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
        self.resolve_mouse_motion_actions();
    }

    /// Feed scroll input (delta in detents or pixels — caller normalises).
    pub fn mouse_scroll(&mut self, dx: f32, dy: f32) {
        self.scroll_delta.0 += dx;
        self.scroll_delta.1 += dy;
        self.resolve_mouse_scroll_actions();
    }

    /// Called at the start of each server tick.
    ///
    /// - Resets per-frame edge flags (`just_pressed`, `just_released`) on every
    ///   button action.
    /// - Zeroes accumulated mouse-motion and scroll deltas.
    /// - Advances the internal clock by `dt` seconds so that [`ButtonState::Held`]
    ///   durations are up-to-date next time a button action is resolved.
    pub fn begin_tick(&mut self, dt: f32) {
        self.elapsed += dt;
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = (0.0, 0.0);

        // Reset edge flags and re-resolve every action so that Held durations
        // and axis values reflect the new elapsed time and cleared deltas.
        for i in 0..self.slots.len() {
            if let ActionValue::Button(a) = &mut self.slots[i].value {
                a.reset_edges();
            }
            // Re-resolve to update Held duration and zero out axis deltas.
            self.resolve_one(i);
        }
    }

    // -- reading resolved state ----------------------------------------------

    /// Get the current value of an action by name.
    ///
    /// Returns `None` if no action with that name was declared.
    #[must_use]
    pub fn action(&self, name: &str) -> Option<&ActionValue> {
        let &idx = self.name_to_idx.get(name)?;
        Some(&self.slots[idx].value)
    }

    /// Get a mutable reference to the current value of an action.
    ///
    /// The caller can use this to clear edge flags after consuming them
    /// (e.g. setting `just_pressed = false` after a jump was processed).
    #[must_use]
    pub fn action_mut(&mut self, name: &str) -> Option<&mut ActionValue> {
        let &idx = self.name_to_idx.get(name)?;
        Some(&mut self.slots[idx].value)
    }

    /// All declared action names, in registration order.
    pub fn action_names(&self) -> impl Iterator<Item = &str> {
        self.slots.iter().map(|s| s.decl.name.as_str())
    }

    // -- internal resolution -------------------------------------------------

    /// Re-resolve every action whose bindings include `key`.
    fn resolve_affected_by_key(&mut self, key: KeyCode) {
        let indices: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                self.enabled.contains(&slot.decl.name)
                    && slot.decl.bindings.iter().any(|b| match b {
                        Binding::Key(k) => *k == key,
                        Binding::Wasd {
                            up,
                            down,
                            left,
                            right,
                        } => *up == key || *down == key || *left == key || *right == key,
                        _ => false,
                    })
            })
            .map(|(i, _)| i)
            .collect();
        for idx in indices {
            self.resolve_one(idx);
        }
    }

    /// Re-resolve every action whose bindings include `button`.
    fn resolve_affected_by_button(&mut self, button: PointerButton) {
        let indices: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                self.enabled.contains(&slot.decl.name)
                    && slot
                        .decl
                        .bindings
                        .iter()
                        .any(|b| matches!(b, Binding::MouseButton(b2) if *b2 == button))
            })
            .map(|(i, _)| i)
            .collect();
        for idx in indices {
            self.resolve_one(idx);
        }
    }

    /// Re-resolve actions that depend on mouse motion.
    fn resolve_mouse_motion_actions(&mut self) {
        let indices: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                self.enabled.contains(&slot.decl.name)
                    && slot
                        .decl
                        .bindings
                        .iter()
                        .any(|b| matches!(b, Binding::MouseMotion))
            })
            .map(|(i, _)| i)
            .collect();
        for idx in indices {
            self.resolve_one(idx);
        }
    }

    /// Re-resolve actions that depend on mouse scroll.
    fn resolve_mouse_scroll_actions(&mut self) {
        let indices: Vec<usize> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                self.enabled.contains(&slot.decl.name)
                    && slot
                        .decl
                        .bindings
                        .iter()
                        .any(|b| matches!(b, Binding::MouseScroll))
            })
            .map(|(i, _)| i)
            .collect();
        for idx in indices {
            self.resolve_one(idx);
        }
    }

    /// Compute the current value of a single action slot from raw state.
    fn resolve_one(&mut self, idx: usize) {
        let held_keys = &self.held_keys;
        let held_buttons = &self.held_buttons;
        let mouse_delta = self.mouse_delta;
        let scroll_delta = self.scroll_delta;
        let elapsed = self.elapsed;

        let slot = &mut self.slots[idx];
        let kind = slot.kind();
        let bindings = &slot.decl.bindings;

        match kind {
            ActionKind::Button => {
                let down = bindings.iter().any(|b| match b {
                    Binding::Key(k) => held_keys.contains(k),
                    Binding::MouseButton(b) => held_buttons.contains(b),
                    Binding::MouseMotion | Binding::MouseScroll => false,
                    Binding::Wasd {
                        up,
                        down: w_down,
                        left,
                        right,
                    } => {
                        held_keys.contains(up)
                            || held_keys.contains(w_down)
                            || held_keys.contains(left)
                            || held_keys.contains(right)
                    }
                });

                let was_active = slot.active;
                slot.active = down;

                let action = match &mut slot.value {
                    ActionValue::Button(a) => a,
                    _ => unreachable!(),
                };

                match (was_active, down) {
                    (false, true) => {
                        action.state = ButtonState::Pressed;
                        action.just_pressed = true;
                        slot.hold_start = Some(elapsed);
                    }
                    (true, true) => {
                        let duration = elapsed - slot.hold_start.unwrap_or(elapsed);
                        action.state = ButtonState::Held { duration };
                    }
                    (true, false) => {
                        action.state = ButtonState::Released;
                        action.just_released = true;
                        slot.hold_start = None;
                    }
                    (false, false) => {
                        action.state = ButtonState::Released;
                    }
                }
            }
            ActionKind::Axis1 => {
                let mut value: f32 = 0.0;

                for binding in bindings {
                    match binding {
                        Binding::MouseScroll => {
                            value += scroll_delta.1;
                        }
                        Binding::Key(k) if held_keys.contains(k) => {
                            value += 1.0;
                        }
                        _ => {}
                    }
                }

                value = value.clamp(-1.0, 1.0);

                let action = match &mut slot.value {
                    ActionValue::Axis1(a) => a,
                    _ => unreachable!(),
                };
                action.value = value;
            }
            ActionKind::Axis2 => {
                let mut x: f32 = 0.0;
                let mut y: f32 = 0.0;

                for binding in bindings {
                    match binding {
                        Binding::Wasd {
                            up,
                            down: w_down,
                            left,
                            right,
                        } => {
                            if held_keys.contains(up) {
                                y += 1.0;
                            }
                            if held_keys.contains(w_down) {
                                y -= 1.0;
                            }
                            if held_keys.contains(left) {
                                x -= 1.0;
                            }
                            if held_keys.contains(right) {
                                x += 1.0;
                            }
                        }
                        Binding::MouseMotion => {
                            x += mouse_delta.0;
                            y += mouse_delta.1;
                        }
                        _ => {}
                    }
                }

                let action = match &mut slot.value {
                    ActionValue::Axis2(a) => a,
                    _ => unreachable!(),
                };
                action.x = x;
                action.y = y;
            }
        }
    }
}

impl Default for ActionMap {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ActionMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionMap")
            .field(
                "actions",
                &self.slots.iter().map(|s| &s.decl.name).collect::<Vec<_>>(),
            )
            .field("held_keys", &self.held_keys.len())
            .field("held_buttons", &self.held_buttons.len())
            .field("elapsed", &self.elapsed)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// InputTickState
// ---------------------------------------------------------------------------

/// The input state for one server tick.
///
/// The client builds one per tick with [`InputTickState::capture`], sends it to
/// the server, which applies it before running the tick.
#[derive(Debug, Clone, Default)]
pub struct InputTickState {
    /// Active actions and their values at this tick.
    pub actions: Vec<(String, ActionValue)>,
}

impl InputTickState {
    /// Create an empty tick state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Capture the current state of every declared action from `map`.
    ///
    /// This snapshots enabled and disabled actions alike; disabled actions will
    /// be in their idle state.
    pub fn capture(map: &ActionMap) -> Self {
        let actions = map
            .slots
            .iter()
            .map(|slot| (slot.decl.name.clone(), slot.value.clone()))
            .collect();
        Self { actions }
    }

    /// Look up an action value by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ActionValue> {
        self.actions.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn decl_button(name: &str, binding: Binding) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Button,
            bindings: vec![binding],
        }
    }

    fn decl_axis2_wasd(
        name: &str,
        up: KeyCode,
        down: KeyCode,
        left: KeyCode,
        right: KeyCode,
    ) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Axis2,
            bindings: vec![Binding::Wasd {
                up,
                down,
                left,
                right,
            }],
        }
    }

    fn decl_axis2_mouse(name: &str) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Axis2,
            bindings: vec![Binding::MouseMotion],
        }
    }

    fn decl_axis1_scroll(name: &str) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Axis1,
            bindings: vec![Binding::MouseScroll],
        }
    }

    // -- ButtonState / ButtonAction -----------------------------------------

    #[test]
    fn button_action_default_is_released() {
        let a = ButtonAction::default();
        assert!(matches!(a.state, ButtonState::Released));
        assert!(!a.just_pressed);
        assert!(!a.just_released);
    }

    #[test]
    fn reset_edges_clears_flags() {
        let mut a = ButtonAction {
            state: ButtonState::Pressed,
            just_pressed: true,
            just_released: true,
        };
        a.reset_edges();
        assert!(!a.just_pressed);
        assert!(!a.just_released);
        // state is untouched by reset_edges.
        assert!(matches!(a.state, ButtonState::Pressed));
    }

    // -- ActionMap: key binding ---------------------------------------------

    #[test]
    fn key_press_and_release() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));

        // Initially released.
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                just_pressed: false,
                just_released: false,
            })
        ));

        // Press space.
        map.key_event(KeyCode::Space, true);
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                just_released: false,
            })
        ));

        // Next tick: edges cleared, still held → Held { duration }.
        map.begin_tick(1.0 / 60.0);
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Held { .. },
                just_pressed: false,
                just_released: false,
            })
        ));

        // Release space.
        map.key_event(KeyCode::Space, false);
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                just_pressed: false,
                just_released: true,
            })
        ));
    }

    #[test]
    fn key_held_duration() {
        let mut map = ActionMap::new();
        map.declare(decl_button("fire", Binding::Key(KeyCode::KeyF)));

        map.key_event(KeyCode::KeyF, true);
        // Advance 2 seconds.
        map.begin_tick(2.0);

        let v = map.action("fire").unwrap();
        if let ActionValue::Button(a) = v {
            if let ButtonState::Held { duration } = a.state {
                assert!(
                    (duration - 2.0).abs() < 0.001,
                    "expected ~2s, got {duration}"
                );
            } else {
                panic!("expected Held, got {:?}", a.state);
            }
        } else {
            panic!("expected Button, got {:?}", v);
        }
    }

    // -- ActionMap: mouse-button binding ------------------------------------

    #[test]
    fn mouse_button_press_and_release() {
        let mut map = ActionMap::new();
        map.declare(decl_button(
            "shoot",
            Binding::MouseButton(PointerButton::Left),
        ));

        map.mouse_button(PointerButton::Left, true);
        let v = map.action("shoot").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                just_released: false,
            })
        ));

        map.begin_tick(0.016);
        map.mouse_button(PointerButton::Left, false);
        let v = map.action("shoot").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                just_pressed: false,
                just_released: true,
            })
        ));
    }

    // -- ActionMap: WASD composite -------------------------------------------

    #[test]
    fn wasd_composite_basics() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));

        // Idle.
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 0.0, 0.0);

        // Press W → (0, 1).
        map.key_event(KeyCode::KeyW, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 0.0, 1.0);

        // Press D as well → (1, 1).
        map.key_event(KeyCode::KeyD, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 1.0, 1.0);

        // Release W and D, then press S and A → (−1, −1).
        map.key_event(KeyCode::KeyW, false);
        map.key_event(KeyCode::KeyD, false);
        map.key_event(KeyCode::KeyA, true);
        map.key_event(KeyCode::KeyS, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, -1.0, -1.0);

        // Release all.
        map.key_event(KeyCode::KeyD, false);
        map.key_event(KeyCode::KeyA, false);
        map.key_event(KeyCode::KeyS, false);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 0.0, 0.0);
    }

    #[test]
    fn wasd_opposing_keys_cancel() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));

        // W + S simultaneously → (0, 0).
        map.key_event(KeyCode::KeyW, true);
        map.key_event(KeyCode::KeyS, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 0.0, 0.0);

        // A + D simultaneously → (0, 0).
        map.key_event(KeyCode::KeyA, true);
        map.key_event(KeyCode::KeyD, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, 0.0, 0.0);
    }

    // -- ActionMap: mouse motion --------------------------------------------

    #[test]
    fn mouse_motion_axis2() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_mouse("look"));

        // Accumulate delta over several events in one tick.
        map.mouse_motion(5.0, -3.0);
        map.mouse_motion(2.0, 1.0);
        let v = map.action("look").unwrap();
        assert_eq_axis2(v, 7.0, -2.0);

        // begin_tick resets deltas.
        map.begin_tick(0.016);
        let v = map.action("look").unwrap();
        assert_eq_axis2(v, 0.0, 0.0);
    }

    // -- ActionMap: mouse scroll --------------------------------------------

    #[test]
    fn mouse_scroll_axis1() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_scroll("zoom"));

        map.mouse_scroll(0.0, 1.0);
        let v = map.action("zoom").unwrap();
        assert_eq_axis1(v, 1.0);

        map.mouse_scroll(0.0, -0.5);
        let v = map.action("zoom").unwrap();
        assert_eq_axis1(v, 0.5);

        // begin_tick resets.
        map.begin_tick(0.016);
        let v = map.action("zoom").unwrap();
        assert_eq_axis1(v, 0.0);
    }

    #[test]
    fn scroll_clamped_to_range() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_scroll("zoom"));

        map.mouse_scroll(0.0, 5.0);
        let v = map.action("zoom").unwrap();
        assert_eq_axis1(v, 1.0);

        map.begin_tick(0.016);
        map.mouse_scroll(0.0, -5.0);
        let v = map.action("zoom").unwrap();
        assert_eq_axis1(v, -1.0);
    }

    // -- begin_tick resets edges and deltas ----------------------------------

    #[test]
    fn begin_tick_resets_just_pressed_and_just_released() {
        let mut map = ActionMap::new();
        map.declare(decl_button("a", Binding::Key(KeyCode::KeyA)));

        map.key_event(KeyCode::KeyA, true);
        assert!(matches!(
            map.action("a").unwrap(),
            ActionValue::Button(ButtonAction {
                just_pressed: true,
                ..
            })
        ));

        map.begin_tick(0.016);
        assert!(matches!(
            map.action("a").unwrap(),
            ActionValue::Button(ButtonAction {
                just_pressed: false,
                just_released: false,
                ..
            })
        ));

        map.key_event(KeyCode::KeyA, false);
        assert!(matches!(
            map.action("a").unwrap(),
            ActionValue::Button(ButtonAction {
                just_released: true,
                ..
            })
        ));

        map.begin_tick(0.016);
        assert!(matches!(
            map.action("a").unwrap(),
            ActionValue::Button(ButtonAction {
                just_released: false,
                ..
            })
        ));
    }

    // -- enable / disable ---------------------------------------------------

    #[test]
    fn disabled_action_returns_idle() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));

        map.key_event(KeyCode::Space, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));

        map.set_enabled("jump", false);
        // Disabled → Released and edges cleared.
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                just_pressed: false,
                just_released: false,
            })
        ));

        // Re-enable: key is still held, so it becomes pressed again.
        map.set_enabled("jump", true);
        map.key_event(KeyCode::Space, true); // need to re-trigger resolution
        let v = map.action("jump").unwrap();
        assert!(matches!(
            v,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                ..
            })
        ));
    }

    #[test]
    fn disabled_wasd_returns_zero() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));
        map.key_event(KeyCode::KeyW, true);
        assert_eq_axis2(map.action("move").unwrap(), 0.0, 1.0);

        map.set_enabled("move", false);
        assert_eq_axis2(map.action("move").unwrap(), 0.0, 0.0);
    }

    // -- action_mut ---------------------------------------------------------

    #[test]
    fn action_mut_lets_caller_clear_edges() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));

        map.key_event(KeyCode::Space, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                just_pressed: true,
                ..
            })
        ));

        // Consume the edge mid-tick.
        if let Some(ActionValue::Button(a)) = map.action_mut("jump") {
            a.just_pressed = false;
        }

        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                just_pressed: false,
                ..
            })
        ));
    }

    // -- multiple bindings per action ---------------------------------------

    #[test]
    fn multiple_bindings_any_triggers() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "jump".into(),
            kind: ActionKind::Button,
            bindings: vec![
                Binding::Key(KeyCode::Space),
                Binding::MouseButton(PointerButton::Left),
            ],
        });

        // Either binding triggers.
        map.key_event(KeyCode::Space, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));

        map.begin_tick(0.016);
        map.key_event(KeyCode::Space, false);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                ..
            })
        ));

        map.begin_tick(0.016);
        map.mouse_button(PointerButton::Left, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));
    }

    #[test]
    fn multiple_bindings_stays_held_until_all_released() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "crouch".into(),
            kind: ActionKind::Button,
            bindings: vec![
                Binding::Key(KeyCode::ControlLeft),
                Binding::Key(KeyCode::KeyC),
            ],
        });

        map.key_event(KeyCode::ControlLeft, true);
        assert!(matches!(
            map.action("crouch").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));

        map.begin_tick(0.016);
        map.key_event(KeyCode::KeyC, true);
        // Still held (via ControlLeft).
        assert!(matches!(
            map.action("crouch").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Held { .. },
                ..
            })
        ));

        // Release one, still held via the other.
        map.key_event(KeyCode::ControlLeft, false);
        assert!(matches!(
            map.action("crouch").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Held { .. },
                ..
            })
        ));

        // Release both → released.
        map.key_event(KeyCode::KeyC, false);
        assert!(matches!(
            map.action("crouch").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                just_released: true,
                ..
            })
        ));
    }

    // -- action_names -------------------------------------------------------

    #[test]
    fn action_names_returns_in_order() {
        let mut map = ActionMap::new();
        map.declare(decl_button("a", Binding::Key(KeyCode::KeyA)));
        map.declare(decl_button("b", Binding::Key(KeyCode::KeyB)));
        map.declare(decl_button("c", Binding::Key(KeyCode::KeyC)));

        let names: Vec<&str> = map.action_names().collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    // -- InputTickState -----------------------------------------------------

    #[test]
    fn tick_state_capture_and_get() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));

        map.key_event(KeyCode::Space, true);
        map.key_event(KeyCode::KeyW, true);

        let tick = InputTickState::capture(&map);
        assert_eq!(tick.actions.len(), 2);

        // jump should be pressed.
        let jump = tick.get("jump").unwrap();
        assert!(matches!(
            jump,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                just_released: false,
            })
        ));

        // move should be (0, 1).
        let mv = tick.get("move").unwrap();
        assert_eq_axis2(mv, 0.0, 1.0);

        // Unknown name → None.
        assert!(tick.get("nope").is_none());
    }

    #[test]
    fn tick_state_new_is_empty() {
        let tick = InputTickState::new();
        assert!(tick.actions.is_empty());
        assert!(tick.get("anything").is_none());
    }

    // -- edge case: multiple actions with overlapping bindings ---------------

    #[test]
    fn overlapping_bindings_independent() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        map.declare(decl_button("fire", Binding::Key(KeyCode::Space)));

        map.key_event(KeyCode::Space, true);

        // Both should be pressed independently.
        let jump = map.action("jump").unwrap();
        let fire = map.action("fire").unwrap();
        assert!(matches!(
            jump,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                ..
            })
        ));
        assert!(matches!(
            fire,
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                ..
            })
        ));
    }

    // -- edge case: non-existent action -------------------------------------

    #[test]
    fn unknown_action_returns_none() {
        let mut map = ActionMap::new();
        assert!(map.action("nope").is_none());
        assert!(map.action_mut("nope").is_none());
    }

    // -- Debug implementations ----------------------------------------------

    #[test]
    fn debug_impls() {
        // Just make sure they don't panic.
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        let _ = format!("{map:?}");

        let tick = InputTickState::capture(&map);
        let _ = format!("{tick:?}");

        let _ = format!("{:?}", ButtonAction::default());
        let _ = format!("{:?}", ButtonState::Pressed);
        let _ = format!("{:?}", ActionKind::Button);
        let _ = format!(
            "{:?}",
            Binding::Wasd {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            }
        );
    }

    // -- Default impls ------------------------------------------------------

    #[test]
    fn axis_defaults_are_zero() {
        assert_eq!(Axis1Action::default(), Axis1Action { value: 0.0 });
        assert_eq!(Axis2Action::default(), Axis2Action { x: 0.0, y: 0.0 });
    }

    #[test]
    fn input_tick_state_default_is_empty() {
        let tick = InputTickState::default();
        assert!(tick.actions.is_empty());
    }

    // -- WASD with non-standard keys ----------------------------------------

    #[test]
    fn wasd_with_arrow_keys() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
        ));

        map.key_event(KeyCode::ArrowRight, true);
        map.key_event(KeyCode::ArrowUp, true);
        assert_eq_axis2(map.action("move").unwrap(), 1.0, 1.0);
    }

    // -- assert helpers -----------------------------------------------------

    fn assert_eq_axis2(v: &ActionValue, x: f32, y: f32) {
        match v {
            ActionValue::Axis2(a) => {
                assert!(
                    (a.x - x).abs() < 0.001 && (a.y - y).abs() < 0.001,
                    "expected Axis2({x}, {y}), got Axis2({}, {})",
                    a.x,
                    a.y
                );
            }
            other => panic!("expected Axis2, got {other:?}"),
        }
    }

    fn assert_eq_axis1(v: &ActionValue, val: f32) {
        match v {
            ActionValue::Axis1(a) => {
                assert!(
                    (a.value - val).abs() < 0.001,
                    "expected Axis1({val}), got Axis1({})",
                    a.value
                );
            }
            other => panic!("expected Axis1, got {other:?}"),
        }
    }
}
