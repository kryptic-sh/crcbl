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

/// 1-D analog axis: −1.0 … 1.0 (triggers, scroll wheels, an absolute pointer
/// coordinate).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axis1Action {
    /// Current axis value.
    pub value: f32,
    /// True on the tick a [`Binding::PointerPosition`] on this action reported a
    /// coordinate different from the one it was holding.
    ///
    /// The edge an *absolute* source has and a relative one does not. A scroll
    /// axis returns to zero every [`ActionMap::begin_tick`], so a non-zero value
    /// already means "this moved"; a pointer axis holds its position across
    /// ticks — that is the whole point of it — so its value says where the
    /// pointer is and nothing about whether the player just asked for anything.
    ///
    /// Cleared by [`ActionMap::begin_tick`], like [`ButtonAction::just_pressed`].
    /// A consumer that drives one thing from both a pointer and the keyboard
    /// reads this to decide which spoke most recently — see the
    /// [`Binding::PointerPosition`] docs.
    pub pointer_moved: bool,
}

/// 2-D analog axis (WASD composite, stick, mouse motion).
///
/// Key-driven composites are normalized to at most unit length; mouse motion is
/// a raw pixel delta and is not.
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

/// Which coordinate of the pointer a [`Binding::PointerPosition`] reads.
///
/// One binding with an axis rather than a `PointerX`/`PointerY` pair: the two
/// would be the same variant twice, and a rebind menu listing both as unrelated
/// sources is a menu that has to explain why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerAxis {
    /// Horizontal: −1.0 at the left edge of the surface, +1.0 at the right.
    X,
    /// Vertical: −1.0 at the **bottom** edge, +1.0 at the top.
    Y,
}

/// A binding source: what raw input triggers this action.
///
/// For P2 only keyboard and mouse are supported; gamepad lands at P10 and
/// multi-touch post-MVP. A single touch contact needs nothing extra — the
/// platforms deliver it as an ordinary pointer, so [`Binding::MouseButton`] and
/// [`Binding::PointerPosition`] are what a phone plays a game through.
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
    /// Where the pointer **is**, normalised to the surface it is over: −1.0 at
    /// one edge, +1.0 at the other, with +X right and +Y up.
    ///
    /// Drives an [`ActionKind::Axis1`], whose −1…1 range is already exactly a
    /// normalised coordinate. Feeding an `Axis2` instead would put a *place* in
    /// the same value as [`Binding::Wasd`]'s *direction*, and a consumer handed
    /// `(0.5, 0.0)` could not tell "half way to the right edge" from "moving
    /// right at half speed"; a fourth [`ActionKind`] for one binding would be a
    /// whole value shape for a case `Axis1` already covers. Any other kind
    /// ignores this binding, as [`Binding::MouseMotion`] does on a button.
    ///
    /// **Normalised, not pixels**, because pixels are the surface's business:
    /// a device-pixel coordinate makes every consumer redo the DPI arithmetic
    /// the windowing layer already did, and get it wrong on exactly the displays
    /// nobody develops on. What the consumer still owns is the step from the
    /// surface to its own world — a camera's half width is not the map's to
    /// know.
    ///
    /// **A pointer that leaves keeps its last position.** Leaving is not a
    /// command: the platform reports it with no coordinate at all, so there is
    /// nothing to resolve, and an axis that recentred itself would walk a
    /// paddle to the middle of the field every time a finger lifted.
    ///
    /// **On an axis that also has a relative source** — a [`Binding::KeyAxis`],
    /// a [`Binding::MouseScroll`] — this one wins whenever the pointer has a
    /// position, because summing them would add a place to a rate and produce
    /// neither. That is a resolution, not an arbitration: a consumer whose
    /// keyboard means something genuinely different from its pointer (a paddle
    /// driven left at a speed, versus a paddle put at a coordinate) declares two
    /// actions and picks between them with
    /// [`Axis1Action::pointer_moved`].
    PointerPosition {
        /// Which coordinate this action reads.
        axis: PointerAxis,
    },
    /// Two keys driving one 1-D axis: `negative` contributes −1.0, `positive`
    /// +1.0, and both at once cancel.
    ///
    /// A single [`Binding::Key`] on an `Axis1` can only ever push the axis
    /// positive, so a keyboard-driven throttle or zoom needs this.
    KeyAxis {
        /// Key for the −1.0 direction.
        negative: KeyCode,
        /// Key for the +1.0 direction.
        positive: KeyCode,
    },
    /// WASD composite: four keys mapped to a 2-D axis.
    ///
    /// The composite is normalized, so a diagonal is a unit vector rather than
    /// `(1, 1)` — otherwise diagonal movement is 41% faster than cardinal.
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
    hold_start: Option<f64>,
    /// Whether this action reacts to input at all.
    ///
    /// A flag on the slot rather than a `HashSet<String>` on the map: the set
    /// was hashed once per slot per raw event, and `set_enabled` on a name that
    /// was never declared grew it forever.
    enabled: bool,
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
            enabled: true,
        }
    }

    /// Returns the declared kind.
    fn kind(&self) -> ActionKind {
        self.decl.kind
    }

    /// Force the action back to its idle value and drop any hold state.
    fn reset(&mut self) {
        self.active = false;
        self.hold_start = None;
        match &mut self.value {
            ActionValue::Button(a) => {
                a.state = ButtonState::Released;
                a.just_pressed = false;
                a.just_released = false;
            }
            ActionValue::Axis1(a) => {
                a.value = 0.0;
                a.pointer_moved = false;
            }
            ActionValue::Axis2(a) => {
                a.x = 0.0;
                a.y = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// What can go wrong when mutating an [`ActionMap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionMapError {
    /// An action with this name is already declared.
    DuplicateName(String),
    /// No action with this name has been declared.
    UnknownAction(String),
}

impl std::fmt::Display for ActionMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName(name) => write!(f, "duplicate action name: {name}"),
            Self::UnknownAction(name) => write!(f, "no such action: {name}"),
        }
    }
}

impl std::error::Error for ActionMapError {}

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

    // Raw input state -------------------------------------------------------
    held_keys: HashSet<KeyCode>,
    held_buttons: HashSet<PointerButton>,
    mouse_delta: (f32, f32),
    scroll_delta: (f32, f32),
    /// Where the pointer is, normalised to the surface, or `None` until one has
    /// ever reported a position.
    ///
    /// Not cleared by [`ActionMap::begin_tick`] and not cleared when the pointer
    /// leaves — unlike the two deltas above, this is a *level* and it is the
    /// last thing the player actually asked for. See
    /// [`Binding::PointerPosition`].
    pointer: Option<(f32, f32)>,

    /// Seconds elapsed since creation (advanced by `begin_tick`).
    ///
    /// `f64` rather than `f32`: an `f32` accumulator quantizes `Held`
    /// durations after a few hours of uptime and stops advancing entirely
    /// after about nineteen days, which is well inside a dedicated server's
    /// lifetime.
    elapsed: f64,
}

impl ActionMap {
    /// Create an empty action map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            name_to_idx: HashMap::new(),
            held_keys: HashSet::new(),
            held_buttons: HashSet::new(),
            mouse_delta: (0.0, 0.0),
            scroll_delta: (0.0, 0.0),
            pointer: None,
            elapsed: 0.0,
        }
    }

    /// Register an action.  Actions are enabled by default.
    ///
    /// # Panics
    /// Panics if an action with the same name has already been declared. Use
    /// [`ActionMap::try_declare`] when the declaration comes from a user config
    /// file, where a duplicate is a message rather than a crash.
    pub fn declare(&mut self, decl: ActionDecl) {
        if let Err(error) = self.try_declare(decl) {
            panic!("{error}");
        }
    }

    /// Register an action, reporting a duplicate name rather than panicking.
    ///
    /// # Errors
    /// [`ActionMapError::DuplicateName`] if the name is already declared.
    pub fn try_declare(&mut self, decl: ActionDecl) -> Result<(), ActionMapError> {
        if self.name_to_idx.contains_key(&decl.name) {
            return Err(ActionMapError::DuplicateName(decl.name));
        }
        let idx = self.slots.len();
        self.name_to_idx.insert(decl.name.clone(), idx);
        self.slots.push(ActionSlot::new(decl));
        Ok(())
    }

    /// Replace an action's bindings in place — the rebind path.
    ///
    /// Keeps the action's registration order and enabled flag, and resets its
    /// value and hold state (the old bindings' "is it down" answer says nothing
    /// about the new ones). Rebuilding the whole map instead, which was the
    /// only option before, lost both.
    ///
    /// # Errors
    /// [`ActionMapError::UnknownAction`] if nothing with that name is declared.
    pub fn rebind(&mut self, name: &str, bindings: Vec<Binding>) -> Result<(), ActionMapError> {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return Err(ActionMapError::UnknownAction(name.to_owned()));
        };
        let slot = &mut self.slots[idx];
        slot.decl.bindings = bindings;
        slot.reset();
        if slot.enabled {
            self.resolve_one(idx);
        }
        Ok(())
    }

    /// The bindings currently driving an action, or `None` if it is not
    /// declared.
    #[must_use]
    pub fn bindings(&self, name: &str) -> Option<&[Binding]> {
        let &idx = self.name_to_idx.get(name)?;
        Some(&self.slots[idx].decl.bindings)
    }

    /// Whether an action is enabled. `None` if it is not declared.
    #[must_use]
    pub fn is_enabled(&self, name: &str) -> Option<bool> {
        let &idx = self.name_to_idx.get(name)?;
        Some(self.slots[idx].enabled)
    }

    /// Enable or disable an action by name.  Disabled actions always return
    /// their default (idle) value and do not react to input — including across
    /// [`ActionMap::begin_tick`], which used to re-read the held-key set and
    /// re-press a disabled action on the very next tick.
    ///
    /// Has no effect if the named action does not exist.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return;
        };
        let slot = &mut self.slots[idx];
        slot.enabled = enabled;
        if !enabled {
            // Reset the action to idle so it doesn't stick.
            slot.reset();
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
        self.resolve_matching(|b| match b {
            Binding::Key(k) => *k == key,
            Binding::KeyAxis { negative, positive } => *negative == key || *positive == key,
            Binding::Wasd {
                up,
                down,
                left,
                right,
            } => *up == key || *down == key || *left == key || *right == key,
            Binding::MouseButton(_)
            | Binding::MouseMotion
            | Binding::MouseScroll
            | Binding::PointerPosition { .. } => false,
        });
    }

    /// Feed a mouse-button event.
    pub fn mouse_button(&mut self, button: PointerButton, pressed: bool) {
        if pressed {
            self.held_buttons.insert(button);
        } else {
            self.held_buttons.remove(&button);
        }
        self.resolve_matching(|b| matches!(b, Binding::MouseButton(b2) if *b2 == button));
    }

    /// Feed mouse motion (delta in pixels since the last event).
    ///
    /// Non-finite deltas are dropped: a `NaN` from a driver or a divide-by-zero
    /// sensitivity would otherwise propagate straight into an action value and
    /// poison every comparison downstream.
    pub fn mouse_motion(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
        self.resolve_matching(|b| matches!(b, Binding::MouseMotion));
    }

    /// Feed scroll input (delta in detents or pixels — caller normalises).
    ///
    /// Non-finite deltas are dropped, as for [`ActionMap::mouse_motion`];
    /// `f32::clamp` returns `NaN` for a `NaN` input, so the clamp downstream is
    /// no defence.
    pub fn mouse_scroll(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.scroll_delta.0 += dx;
        self.scroll_delta.1 += dy;
        self.resolve_matching(|b| matches!(b, Binding::MouseScroll));
    }

    /// Feed the pointer's position, normalised to the surface: −1.0 at one edge
    /// and +1.0 at the other, +X right and +Y up.
    ///
    /// The caller does the conversion because the caller is the only thing that
    /// knows the surface — see [`Binding::PointerPosition`], which also says why
    /// a pointer that *leaves* is not reported here at all.
    ///
    /// A coordinate equal to the one already held is dropped, so
    /// [`Axis1Action::pointer_moved`] means the pointer moved rather than that
    /// an event arrived. Non-finite coordinates are dropped like
    /// [`ActionMap::mouse_motion`]'s deltas: a `NaN` would poison every
    /// comparison downstream of the axis.
    pub fn pointer_position(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let moved = Some((x, y));
        if self.pointer == moved {
            return;
        }
        self.pointer = moved;

        for i in 0..self.slots.len() {
            let slot = &self.slots[i];
            let drives = slot
                .decl
                .bindings
                .iter()
                .any(|b| matches!(b, Binding::PointerPosition { .. }));
            if !slot.enabled || !drives {
                continue;
            }
            self.resolve_one(i);
            // Set here rather than in `resolve_one`, which also runs from
            // `begin_tick`: an edge raised by re-resolving an unchanged value
            // would be raised on every tick forever.
            if let ActionValue::Axis1(axis) = &mut self.slots[i].value {
                axis.pointer_moved = true;
            }
        }
    }

    /// Called at the start of each server tick.
    ///
    /// - Resets per-frame edge flags (`just_pressed`, `just_released` on every
    ///   button action, `pointer_moved` on every 1-D axis).
    /// - Zeroes accumulated mouse-motion and scroll deltas.
    /// - Advances the internal clock by `dt` seconds so that [`ButtonState::Held`]
    ///   durations are up-to-date next time a button action is resolved.
    pub fn begin_tick(&mut self, dt: f32) {
        // A negative dt would move the clock backwards and report negative
        // `Held` durations; only forward time is meaningful here.
        if dt.is_finite() && dt >= 0.0 {
            self.elapsed += f64::from(dt);
        }
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = (0.0, 0.0);

        // Reset edge flags and re-resolve every action so that Held durations
        // and axis values reflect the new elapsed time and cleared deltas.
        //
        // Disabled actions are skipped entirely. Resolving them here re-read
        // `held_keys` and re-pressed them — so disabling "jump" while Space was
        // held cleared it, and the next tick emitted `just_pressed` again.
        for i in 0..self.slots.len() {
            if !self.slots[i].enabled {
                continue;
            }
            match &mut self.slots[i].value {
                ActionValue::Button(a) => a.reset_edges(),
                ActionValue::Axis1(a) => a.pointer_moved = false,
                ActionValue::Axis2(_) => {}
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

    /// Whether a button action is down — pressed this tick or still held.
    ///
    /// An action that was never declared, or that is not a button, is `false`
    /// rather than an error: a caller asking whether a button is down has no
    /// better answer to give, and every game was already collapsing both cases
    /// that way.
    ///
    /// The state is matched positively rather than as "not
    /// [`Released`](ButtonState::Released)". The two agree while
    /// [`ButtonState`] has three variants, and would stop agreeing the day it
    /// gains a fourth — silently, and only for whoever wrote the negation.
    #[must_use]
    pub fn button_held(&self, name: &str) -> bool {
        self.button(name).is_some_and(|button| {
            matches!(
                button.state,
                ButtonState::Pressed | ButtonState::Held { .. }
            )
        })
    }

    /// Whether a button action went down on this tick.
    ///
    /// The edge, not the level: true only for the tick of the transition, and
    /// cleared by [`ActionMap::begin_tick`]. This is what a jump or a flap
    /// reads, so that holding the key does not repeat it.
    #[must_use]
    pub fn just_pressed(&self, name: &str) -> bool {
        self.button(name).is_some_and(|button| button.just_pressed)
    }

    /// Whether a button action came up on this tick.
    #[must_use]
    pub fn just_released(&self, name: &str) -> bool {
        self.button(name).is_some_and(|button| button.just_released)
    }

    /// A 1-D axis action's value, or `0.0` if it is absent or another kind.
    #[must_use]
    pub fn axis1(&self, name: &str) -> f32 {
        match self.action(name) {
            Some(ActionValue::Axis1(axis)) => axis.value,
            _ => 0.0,
        }
    }

    /// A 2-D axis action's value, or `(0.0, 0.0)` if it is absent or another
    /// kind.
    ///
    /// Key-driven composites arrive normalised to at most unit length, so a
    /// diagonal is not faster than a cardinal; mouse motion is a raw pixel
    /// delta and is not normalised.
    #[must_use]
    pub fn axis2(&self, name: &str) -> (f32, f32) {
        match self.action(name) {
            Some(ActionValue::Axis2(axis)) => (axis.x, axis.y),
            _ => (0.0, 0.0),
        }
    }

    /// The button behind `name`, if it is declared and is one.
    fn button(&self, name: &str) -> Option<&ButtonAction> {
        match self.action(name)? {
            ActionValue::Button(button) => Some(button),
            _ => None,
        }
    }

    // -- internal resolution -------------------------------------------------

    /// Re-resolve every enabled action with a binding `matches` accepts.
    ///
    /// One helper rather than four near-identical bodies, and no `Vec<usize>`
    /// per event: the enabled flag lives on the slot, so nothing here allocates
    /// or hashes a `String`.
    fn resolve_matching(&mut self, matches: impl Fn(&Binding) -> bool) {
        for i in 0..self.slots.len() {
            let slot = &self.slots[i];
            if slot.enabled && slot.decl.bindings.iter().any(&matches) {
                self.resolve_one(i);
            }
        }
    }

    /// Compute the current value of a single action slot from raw state.
    fn resolve_one(&mut self, idx: usize) {
        let held_keys = &self.held_keys;
        let held_buttons = &self.held_buttons;
        let mouse_delta = self.mouse_delta;
        let scroll_delta = self.scroll_delta;
        let pointer = self.pointer;
        let elapsed = self.elapsed;

        let slot = &mut self.slots[idx];
        let kind = slot.kind();
        let bindings = &slot.decl.bindings;

        match kind {
            ActionKind::Button => {
                let down = bindings.iter().any(|b| match b {
                    Binding::Key(k) => held_keys.contains(k),
                    Binding::MouseButton(b) => held_buttons.contains(b),
                    Binding::MouseMotion
                    | Binding::MouseScroll
                    | Binding::PointerPosition { .. } => false,
                    Binding::KeyAxis { negative, positive } => {
                        held_keys.contains(negative) || held_keys.contains(positive)
                    }
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
                        let duration = (elapsed - slot.hold_start.unwrap_or(elapsed)) as f32;
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
                // The absolute source, if this action has one and a pointer has
                // ever reported a position. It **replaces** the relative
                // contributions below rather than adding to them: a place and a
                // rate do not sum. See [`Binding::PointerPosition`].
                let absolute = pointer.and_then(|(px, py)| {
                    bindings.iter().find_map(|binding| match binding {
                        Binding::PointerPosition { axis } => Some(match axis {
                            PointerAxis::X => px,
                            PointerAxis::Y => py,
                        }),
                        _ => None,
                    })
                });

                let mut value: f32 = absolute.unwrap_or(0.0);

                if absolute.is_none() {
                    for binding in bindings {
                        match binding {
                            Binding::MouseScroll => {
                                value += scroll_delta.1;
                            }
                            Binding::Key(k) if held_keys.contains(k) => {
                                value += 1.0;
                            }
                            Binding::KeyAxis { negative, positive } => {
                                if held_keys.contains(negative) {
                                    value -= 1.0;
                                }
                                if held_keys.contains(positive) {
                                    value += 1.0;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // The surface's edge is the axis's end, whatever the caller
                // passed: a coordinate outside it is a pointer outside the
                // window, not a stronger command.
                value = value.clamp(-1.0, 1.0);

                let action = match &mut slot.value {
                    ActionValue::Axis1(a) => a,
                    _ => unreachable!(),
                };
                action.value = value;
            }
            ActionKind::Axis2 => {
                // The key composite and the pointer delta are accumulated
                // separately: the composite is normalized so a diagonal is a
                // unit vector rather than 1.414 long, while a pointer delta is
                // a pixel count and normalizing it would throw away the speed.
                let mut key_x: f32 = 0.0;
                let mut key_y: f32 = 0.0;
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
                                key_y += 1.0;
                            }
                            if held_keys.contains(w_down) {
                                key_y -= 1.0;
                            }
                            if held_keys.contains(left) {
                                key_x -= 1.0;
                            }
                            if held_keys.contains(right) {
                                key_x += 1.0;
                            }
                        }
                        Binding::MouseMotion => {
                            x += mouse_delta.0;
                            y += mouse_delta.1;
                        }
                        _ => {}
                    }
                }

                let len = (key_x * key_x + key_y * key_y).sqrt();
                if len > 1.0 {
                    key_x /= len;
                    key_y /= len;
                }
                x += key_x;
                y += key_y;

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
            .field("pointer", &self.pointer)
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

    // -- typed accessors ----------------------------------------------------

    /// One tick at sixty hertz, which is what `begin_tick` wants — a delta,
    /// not a timestamp.
    const TICK: f32 = 1.0 / 60.0;

    /// Held is down, and so is the tick the key went down on.
    ///
    /// Both, not just the second: a game reading "is the thrust on" during the
    /// very tick the key arrived would otherwise see it off for one tick and
    /// stutter.
    #[test]
    fn a_button_is_held_while_it_is_down_and_not_after() {
        let mut map = ActionMap::new();
        map.declare(decl_button("thrust", Binding::Key(KeyCode::KeyW)));

        assert!(!map.button_held("thrust"), "nothing is down yet");

        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyW, true);
        assert!(map.button_held("thrust"), "the tick the key arrives counts");

        map.begin_tick(TICK);
        assert!(
            matches!(
                map.action("thrust"),
                Some(ActionValue::Button(ButtonAction {
                    state: ButtonState::Held { .. },
                    ..
                }))
            ),
            "a second tick with the key still down is Held, which is the other \
             state that must read as down"
        );
        assert!(map.button_held("thrust"));

        map.key_event(KeyCode::KeyW, false);
        assert!(!map.button_held("thrust"), "released is not down");
    }

    /// The edges are one tick wide, and they are not the level.
    #[test]
    fn the_edges_fire_once_each() {
        let mut map = ActionMap::new();
        map.declare(decl_button("flap", Binding::Key(KeyCode::Space)));

        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed("flap"));
        assert!(!map.just_released("flap"));

        map.begin_tick(TICK);
        assert!(
            !map.just_pressed("flap"),
            "holding the key does not press it again — this is the whole \
             reason a flap reads the edge and not the level"
        );
        assert!(map.button_held("flap"), "still down, though");

        map.key_event(KeyCode::Space, false);
        assert!(map.just_released("flap"));
        map.begin_tick(TICK);
        assert!(
            !map.just_released("flap"),
            "the release edge is one tick too"
        );
    }

    /// An axis answers with its value, and a composite is not longer than one.
    #[test]
    fn the_axis_accessors_read_the_value() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));
        map.declare(decl_axis1_scroll("zoom"));

        assert_eq!(map.axis2("move"), (0.0, 0.0));
        assert_eq!(map.axis1("zoom"), 0.0);

        map.begin_tick(TICK);
        map.key_event(KeyCode::KeyD, true);
        let (x, y) = map.axis2("move");
        assert!((x - 1.0).abs() < 1e-6, "right alone is +1 on x, got {x}");
        assert!(y.abs() < 1e-6, "and nothing on y, got {y}");

        map.key_event(KeyCode::KeyW, true);
        let (x, y) = map.axis2("move");
        let length = x.hypot(y);
        assert!(
            (length - 1.0).abs() < 1e-6,
            "a diagonal is normalised, or it is 41% faster than a cardinal: \
             got length {length}"
        );

        map.mouse_scroll(0.0, 3.0);
        assert_ne!(map.axis1("zoom"), 0.0, "the scroll reached the axis");
    }

    /// Asking the wrong kind, or a name nobody declared, is the idle value.
    ///
    /// Every game collapsed both cases this way already. Worth pinning because
    /// the alternative — a panic, or `true` — would turn a typo into a stuck
    /// input rather than a dead one.
    #[test]
    fn the_wrong_kind_and_the_wrong_name_are_idle() {
        let mut map = ActionMap::new();
        map.declare(decl_button("fire", Binding::Key(KeyCode::Space)));
        map.declare(decl_axis1_scroll("zoom"));

        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);

        assert!(!map.button_held("zoom"), "an axis is not a held button");
        assert!(!map.just_pressed("zoom"));
        assert!(!map.just_released("zoom"));
        assert_eq!(map.axis1("fire"), 0.0, "a button is not an axis");
        assert_eq!(map.axis2("fire"), (0.0, 0.0));

        assert!(!map.button_held("nonesuch"));
        assert!(!map.just_pressed("nonesuch"));
        assert_eq!(map.axis1("nonesuch"), 0.0);
        assert_eq!(map.axis2("nonesuch"), (0.0, 0.0));
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
    fn a_key_still_down_after_a_tick_reports_held_with_how_long_it_has_been() {
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
    fn a_wasd_composite_stays_a_unit_vector_from_idle_through_every_diagonal() {
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

        // Press D as well → a *unit* diagonal, not (1, 1).
        map.key_event(KeyCode::KeyD, true);
        let v = map.action("move").unwrap();
        let diag = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq_axis2(v, diag, diag);

        // Release W and D, then press S and A → the opposite unit diagonal.
        map.key_event(KeyCode::KeyW, false);
        map.key_event(KeyCode::KeyD, false);
        map.key_event(KeyCode::KeyA, true);
        map.key_event(KeyCode::KeyS, true);
        let v = map.action("move").unwrap();
        assert_eq_axis2(v, -diag, -diag);

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
    fn mouse_motion_accumulates_within_a_tick_and_begin_tick_clears_it() {
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
    fn scroll_sums_positive_and_negative_notches_and_resets_on_the_next_tick() {
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

    // -- ActionMap: absolute pointer position --------------------------------

    fn decl_axis1_pointer(name: &str, axis: PointerAxis) -> ActionDecl {
        ActionDecl {
            name: name.to_owned(),
            kind: ActionKind::Axis1,
            bindings: vec![Binding::PointerPosition { axis }],
        }
    }

    /// **Each axis reads its own coordinate, at the value it was given.**
    ///
    /// Two actions on one map and asymmetric coordinates, because that is what
    /// a swapped axis fails: a pointer at `(0.5, 0.5)` puts the right answer on
    /// both of them.
    #[test]
    fn a_pointer_position_lands_on_the_axis_it_is_bound_to() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_pointer("aim_x", PointerAxis::X));
        map.declare(decl_axis1_pointer("aim_y", PointerAxis::Y));

        assert_eq!(
            map.axis1("aim_x"),
            0.0,
            "an axis nothing has pointed at yet is idle, not at the left edge",
        );

        map.begin_tick(TICK);
        map.pointer_position(-0.75, 0.25);

        assert_eq_axis1(map.action("aim_x").unwrap(), -0.75);
        assert_eq_axis1(map.action("aim_y").unwrap(), 0.25);
    }

    /// **The position is a level: it survives ticks, unlike a scroll delta.**
    ///
    /// This is also what "a pointer that leaves keeps its last position" means
    /// mechanically — leaving reports no coordinate, so nothing here is fed and
    /// the axis is exactly where the last real position put it.
    #[test]
    fn an_absolute_axis_holds_its_position_across_ticks() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_pointer("aim", PointerAxis::X));
        map.declare(decl_axis1_scroll("zoom"));

        map.begin_tick(TICK);
        map.pointer_position(0.6, 0.0);
        map.mouse_scroll(0.0, 1.0);
        assert_eq_axis1(map.action("aim").unwrap(), 0.6);
        assert_eq_axis1(map.action("zoom").unwrap(), 1.0);

        map.begin_tick(TICK);
        assert_eq_axis1(map.action("zoom").unwrap(), 0.0);
        assert_eq_axis1(map.action("aim").unwrap(), 0.6);
    }

    /// **`pointer_moved` is an edge, and it is about the pointer moving.**
    ///
    /// A tick that re-sends the coordinate the axis already holds raises
    /// nothing: a consumer arbitrating between the pointer and the keyboard
    /// reads this as "the player just asked for this", and a browser that
    /// re-reports a resting cursor would otherwise hold the keyboard off
    /// forever.
    #[test]
    fn the_pointer_edge_is_one_tick_wide_and_only_for_a_new_position() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_pointer("aim", PointerAxis::X));

        let moved = |map: &ActionMap| match map.action("aim") {
            Some(ActionValue::Axis1(axis)) => axis.pointer_moved,
            other => panic!("expected Axis1, got {other:?}"),
        };

        map.begin_tick(TICK);
        assert!(!moved(&map), "nothing has pointed anywhere yet");
        map.pointer_position(0.2, 0.0);
        assert!(moved(&map), "the pointer moved on this tick");

        map.begin_tick(TICK);
        assert!(!moved(&map), "the edge is one tick wide");
        assert_eq_axis1(map.action("aim").unwrap(), 0.2);

        map.pointer_position(0.2, 0.0);
        assert!(
            !moved(&map),
            "the same coordinate again is not the player asking for anything",
        );
        map.pointer_position(0.2, 0.9);
        assert!(
            moved(&map),
            "the other coordinate changing is still the pointer moving",
        );
    }

    /// **An absolute source replaces the relative ones on the same axis.**
    ///
    /// Summing them would add a place to a rate. The keyboard drives the axis
    /// while no pointer has ever reported a position, and stops the moment one
    /// has — which is the documented reason a game whose keys mean something
    /// *different* from its pointer declares two actions instead of one.
    #[test]
    fn an_absolute_binding_replaces_the_relative_ones_on_the_same_axis() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "slide".to_owned(),
            kind: ActionKind::Axis1,
            bindings: vec![
                Binding::KeyAxis {
                    negative: KeyCode::ArrowLeft,
                    positive: KeyCode::ArrowRight,
                },
                Binding::PointerPosition {
                    axis: PointerAxis::X,
                },
            ],
        });

        map.begin_tick(TICK);
        map.key_event(KeyCode::ArrowRight, true);
        assert_eq_axis1(map.action("slide").unwrap(), 1.0);

        map.pointer_position(-0.5, 0.0);
        assert_eq_axis1(map.action("slide").unwrap(), -0.5);
    }

    /// **The surface's edge is the end of the axis.**
    #[test]
    fn a_pointer_outside_the_surface_is_clamped_to_its_edge() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_pointer("aim", PointerAxis::X));

        map.pointer_position(4.0, 0.0);
        assert_eq_axis1(map.action("aim").unwrap(), 1.0);

        map.pointer_position(-4.0, 0.0);
        assert_eq_axis1(map.action("aim").unwrap(), -1.0);
    }

    /// **A pointer binding joins the keys on a button, it does not replace
    /// them.**
    ///
    /// The flap that a phone taps and a keyboard presses is one action with
    /// both bindings on it, and each still fires on its own. A binding list
    /// that had been overwritten rather than appended to passes every test that
    /// only ever presses the pointer.
    #[test]
    fn a_pointer_button_joins_the_keys_bound_to_the_same_action() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "flap".to_owned(),
            kind: ActionKind::Button,
            bindings: vec![
                Binding::Key(KeyCode::Space),
                Binding::MouseButton(PointerButton::Left),
            ],
        });

        map.begin_tick(TICK);
        map.key_event(KeyCode::Space, true);
        assert!(map.just_pressed("flap"), "the key still flaps");
        map.key_event(KeyCode::Space, false);

        map.begin_tick(TICK);
        map.mouse_button(PointerButton::Left, true);
        assert!(map.just_pressed("flap"), "and so does the tap");
        map.mouse_button(PointerButton::Left, false);
        assert!(map.just_released("flap"));
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

        // …and it must stay zero across ticks. `begin_tick` used to re-resolve
        // every slot with no enabled check, so the still-held W came straight
        // back on the next tick.
        for tick in 0..3 {
            map.begin_tick(1.0 / 60.0);
            assert_eq_axis2(map.action("move").unwrap(), 0.0, 0.0);
            assert!(tick < 3);
        }
    }

    /// The same bug for a Button action: disabling "jump" while Space is held
    /// cleared it, and the very next `begin_tick` re-emitted
    /// `state: Pressed, just_pressed: true` — a menu opened with gameplay
    /// disabled still jumped.
    #[test]
    fn a_disabled_button_does_not_re_press_itself_on_the_next_tick() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));

        map.key_event(KeyCode::Space, true);
        map.set_enabled("jump", false);

        for _ in 0..3 {
            map.begin_tick(1.0 / 60.0);
            let v = map.action("jump").unwrap();
            assert!(
                matches!(
                    v,
                    ActionValue::Button(ButtonAction {
                        state: ButtonState::Released,
                        just_pressed: false,
                        just_released: false,
                    })
                ),
                "a disabled action fired anyway: {v:?}"
            );
        }

        // Re-enabling with the key still held presses it again, which is the
        // intended behaviour.
        map.set_enabled("jump", true);
        map.begin_tick(1.0 / 60.0);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                ..
            })
        ));
    }

    /// `set_enabled` on a name that was never declared used to insert into a
    /// `HashSet<String>` that nothing ever pruned.
    #[test]
    fn set_enabled_on_an_unknown_action_is_a_no_op() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        map.set_enabled("typo-in-the-profile", true);
        map.set_enabled("typo-in-the-profile", false);
        assert_eq!(map.is_enabled("typo-in-the-profile"), None);
        assert_eq!(map.is_enabled("jump"), Some(true));
        assert_eq!(map.action_names().count(), 1);
    }

    // -- declare / rebind ---------------------------------------------------

    #[test]
    fn try_declare_reports_a_duplicate_rather_than_aborting() {
        let mut map = ActionMap::new();
        assert!(
            map.try_declare(decl_button("jump", Binding::Key(KeyCode::Space)))
                .is_ok()
        );
        assert_eq!(
            map.try_declare(decl_button("jump", Binding::Key(KeyCode::KeyJ))),
            Err(ActionMapError::DuplicateName("jump".to_owned()))
        );
        // The first declaration is untouched.
        assert_eq!(
            map.bindings("jump"),
            Some(&[Binding::Key(KeyCode::Space)][..])
        );
    }

    #[test]
    fn rebind_replaces_bindings_and_keeps_the_enabled_flag() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        map.set_enabled("jump", false);

        map.rebind("jump", vec![Binding::Key(KeyCode::KeyJ)])
            .expect("declared");
        assert_eq!(
            map.bindings("jump"),
            Some(&[Binding::Key(KeyCode::KeyJ)][..])
        );
        assert_eq!(
            map.is_enabled("jump"),
            Some(false),
            "the rebind must not silently re-enable the action"
        );

        map.set_enabled("jump", true);
        map.key_event(KeyCode::KeyJ, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));
        // The old key no longer does anything.
        map.begin_tick(0.016);
        map.key_event(KeyCode::KeyJ, false);
        map.begin_tick(0.016);
        map.key_event(KeyCode::Space, true);
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Released,
                ..
            })
        ));
    }

    #[test]
    fn rebind_an_unknown_action_errors() {
        let mut map = ActionMap::new();
        assert_eq!(
            map.rebind("nope", vec![]),
            Err(ActionMapError::UnknownAction("nope".to_owned()))
        );
    }

    /// A rebind while the new key is already held resolves immediately.
    #[test]
    fn rebind_resolves_against_the_current_held_keys() {
        let mut map = ActionMap::new();
        map.declare(decl_button("jump", Binding::Key(KeyCode::Space)));
        map.key_event(KeyCode::KeyJ, true);
        map.rebind("jump", vec![Binding::Key(KeyCode::KeyJ)])
            .expect("declared");
        assert!(matches!(
            map.action("jump").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                just_pressed: true,
                ..
            })
        ));
    }

    // -- Axis1 key pairs ----------------------------------------------------

    /// A single `Key` binding can only ever add +1.0, so a keyboard axis could
    /// not go negative at all.
    #[test]
    fn key_axis_reaches_both_ends_of_the_range() {
        let mut map = ActionMap::new();
        map.declare(ActionDecl {
            name: "throttle".into(),
            kind: ActionKind::Axis1,
            bindings: vec![Binding::KeyAxis {
                negative: KeyCode::KeyS,
                positive: KeyCode::KeyW,
            }],
        });

        assert_eq_axis1(map.action("throttle").unwrap(), 0.0);

        map.key_event(KeyCode::KeyW, true);
        assert_eq_axis1(map.action("throttle").unwrap(), 1.0);

        // Both held → cancel.
        map.key_event(KeyCode::KeyS, true);
        assert_eq_axis1(map.action("throttle").unwrap(), 0.0);

        map.key_event(KeyCode::KeyW, false);
        assert_eq_axis1(map.action("throttle").unwrap(), -1.0);
    }

    /// A `KeyAxis` on a `Button` action is "either key is down".
    #[test]
    fn key_axis_drives_a_button_from_either_key() {
        let mut map = ActionMap::new();
        map.declare(decl_button(
            "any",
            Binding::KeyAxis {
                negative: KeyCode::KeyS,
                positive: KeyCode::KeyW,
            },
        ));
        map.key_event(KeyCode::KeyS, true);
        assert!(matches!(
            map.action("any").unwrap(),
            ActionValue::Button(ButtonAction {
                state: ButtonState::Pressed,
                ..
            })
        ));
    }

    // -- non-finite input ---------------------------------------------------

    /// `mouse_motion(NAN, 1.0)` used to write `Axis2 { x: NaN, y: 1.0 }`
    /// straight through, and `f32::clamp` returns `NaN` for a `NaN` scroll, so
    /// the clamp downstream was no defence either.
    #[test]
    fn non_finite_deltas_are_dropped_at_the_ingest_boundary() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_mouse("look"));
        map.declare(decl_axis1_scroll("zoom"));

        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            map.mouse_motion(bad, 1.0);
            map.mouse_motion(1.0, bad);
            map.mouse_scroll(0.0, bad);
        }

        match map.action("look").unwrap() {
            ActionValue::Axis2(a) => assert!(
                a.x.is_finite() && a.y.is_finite(),
                "look went non-finite: {a:?}"
            ),
            other => panic!("expected Axis2, got {other:?}"),
        }
        match map.action("zoom").unwrap() {
            ActionValue::Axis1(a) => {
                assert!(a.value.is_finite(), "zoom went non-finite: {a:?}");
            }
            other => panic!("expected Axis1, got {other:?}"),
        }

        // Good deltas still land.
        map.begin_tick(0.016);
        map.mouse_motion(3.0, -4.0);
        assert_eq_axis2(map.action("look").unwrap(), 3.0, -4.0);
    }

    /// A non-finite pointer coordinate must not become the axis a paddle is
    /// positioned from — and must not evict the last good position either.
    #[test]
    fn a_non_finite_pointer_position_is_dropped_and_the_last_one_survives() {
        let mut map = ActionMap::new();
        map.declare(decl_axis1_pointer("aim", PointerAxis::X));

        map.pointer_position(0.4, 0.0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            map.pointer_position(bad, 0.0);
            map.pointer_position(0.0, bad);
        }

        assert_eq_axis1(map.action("aim").unwrap(), 0.4);
    }

    /// A non-finite `dt` must not poison the clock every `Held` duration is
    /// measured against.
    #[test]
    fn a_non_finite_dt_does_not_poison_hold_durations() {
        let mut map = ActionMap::new();
        map.declare(decl_button("fire", Binding::Key(KeyCode::KeyF)));
        map.key_event(KeyCode::KeyF, true);
        map.begin_tick(f32::NAN);
        map.begin_tick(2.0);

        let ActionValue::Button(a) = map.action("fire").unwrap() else {
            panic!("expected Button");
        };
        let ButtonState::Held { duration } = a.state else {
            panic!("expected Held, got {:?}", a.state);
        };
        assert!((duration - 2.0).abs() < 0.001, "got {duration}");
    }

    /// A negative `dt` used to move the clock backwards, so a key held across
    /// it reported a negative `Held` duration — and the lost time then made the
    /// next tick measure the hold as though it had never been held.
    #[test]
    fn a_negative_dt_does_not_report_a_negative_hold_duration() {
        let mut map = ActionMap::new();
        map.declare(decl_button("fire", Binding::Key(KeyCode::KeyF)));
        map.key_event(KeyCode::KeyF, true);

        map.begin_tick(-1.0);
        let ActionValue::Button(a) = map.action("fire").unwrap() else {
            panic!("expected Button");
        };
        let ButtonState::Held { duration } = a.state else {
            panic!("expected Held, got {:?}", a.state);
        };
        assert!(
            duration >= 0.0,
            "a negative dt must not report a negative hold duration, got {duration}"
        );

        // The negative tick was dropped rather than absorbed: a real 1s tick
        // measures a full second, not the second minus the backwards one.
        map.begin_tick(1.0);
        let ActionValue::Button(a) = map.action("fire").unwrap() else {
            panic!("expected Button");
        };
        let ButtonState::Held { duration } = a.state else {
            panic!("expected Held, got {:?}", a.state);
        };
        assert!(
            (duration - 1.0).abs() < 0.001,
            "with the negative tick dropped the hold measures normally, got {duration}"
        );
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
    fn two_actions_bound_to_one_key_both_report_pressed() {
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
    fn every_input_type_debug_prints_without_panicking() {
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
        assert_eq!(
            Axis1Action::default(),
            Axis1Action {
                value: 0.0,
                pointer_moved: false,
            }
        );
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
        let diag = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq_axis2(map.action("move").unwrap(), diag, diag);
    }

    /// The docs promised a unit vector and the code never normalized, so a
    /// diagonal was 1.414 long — 41% faster than a cardinal direction.
    #[test]
    fn wasd_diagonal_is_not_faster_than_a_cardinal_direction() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_wasd(
            "move",
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ));

        map.key_event(KeyCode::KeyW, true);
        let cardinal = axis2_len(map.action("move").unwrap());
        assert!((cardinal - 1.0).abs() < 0.001, "got {cardinal}");

        map.key_event(KeyCode::KeyD, true);
        let diagonal = axis2_len(map.action("move").unwrap());
        assert!(
            (diagonal - 1.0).abs() < 0.001,
            "diagonal magnitude {diagonal} should be 1.0"
        );
    }

    /// Mouse motion is a pixel delta, not a direction: normalizing it would
    /// throw away how far the pointer actually moved.
    #[test]
    fn mouse_motion_is_not_normalized() {
        let mut map = ActionMap::new();
        map.declare(decl_axis2_mouse("look"));
        map.mouse_motion(30.0, 40.0);
        assert_eq_axis2(map.action("look").unwrap(), 30.0, 40.0);
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

    fn axis2_len(v: &ActionValue) -> f32 {
        match v {
            ActionValue::Axis2(a) => (a.x * a.x + a.y * a.y).sqrt(),
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
