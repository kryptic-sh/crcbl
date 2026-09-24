//! The editor's keyboard, as an [`ActionMap`] — and the two reserved contexts
//! that take it away from the editor when a panel is listening.
//!
//! # Why the keys are actions and not a `match` on the event
//!
//! Slice 1 read key events straight off the shell and matched them into an
//! [`Action`]. That works for a tool with no UI and cannot work for one with a
//! panel: the arrows nudge the selection **and** walk an outliner, Space
//! presses a button **and** would be a shortcut, and every letter is text while
//! a field is being typed into. `crcbl_input`'s context stack is the engine's
//! answer to exactly that — "the topmost active context binding an input
//! consumes it", and `crcbl_input::ui`'s docs say in as many words that it is
//! "the disambiguator, not a list of special cases". So the editor's own keys
//! are ordinary actions in the map's default context, with [`ui`] pushed over
//! them while a panel holds the keyboard and [`text`] over that while a field
//! is engaged.
//!
//! What that buys, stated as the thing a test can see: with `text` pushed, the
//! `s` of a typed word is `text_type` and not [`SAVE`], and the arrows are the
//! caret's and not a nudge.
//!
//! # The one thing the map cannot say, and how the chords are read
//!
//! [`Binding::Chord`] carries **one** [`Modifier`], so `Ctrl+Shift+Z` is not a
//! binding this engine can express. Rather than drop the second spelling of
//! redo, [`actions`] reads the chord's own modifier state — the [`Modifiers`]
//! the shell stamps on every key event, which the loop already keeps — and
//! treats an [`UNDO`] that arrived with Shift held as a redo. The *arbitration*
//! is still the context stack's: while `text` owns `z`, no chord on it fires at
//! all, and there is nothing here to decide.
//!
//! The same modifier state is what keeps `Ctrl` with an arrow meaning nothing,
//! which slice 1's table was explicit about: a chord shadows a plain binding on
//! **its own key** only, so without this the map would happily read `Ctrl+Left`
//! as a nudge.
//!
//! # And the keys the stack cannot arbitrate
//!
//! A context takes a key by **binding** it, so a key no reserved context binds
//! reaches the editor however deep the stack is — and Page Up and Page Down are
//! such keys: `text` binds what a single-line field types and edits with and
//! deliberately not these, and `ui` binds neither. Without a rule they would go
//! on moving the selection along Z while someone types a name.
//!
//! So [`actions`] takes `editing` and asks for **nothing** while a field is
//! engaged. One rule rather than a list of keys to keep in step with
//! `crcbl_input::text::KEYS`, and it leaves the stack doing the work for every
//! key a reserved context does bind — which
//! `a_pushed_context_takes_the_editors_keys_away_from_it` holds on its own,
//! with `editing` false throughout.

use crcbl::core::input::{KeyCode, Modifiers};
use crcbl::input::{
    ActionDecl, ActionKind, ActionMap, Binding, Cardinal, Modifier, Repeat, text, ui,
};

/// Move the selection along X or Y: the arrow keys, repeating while held.
pub const MOVE: &str = "editor_move";

/// Move it along Z: Page Up and Page Down, repeating while held.
pub const LIFT: &str = "editor_lift";

/// Walk the undo log back one entry — and forward, with Shift held; see the
/// module docs.
pub const UNDO: &str = "editor_undo";

/// Walk it forward one entry.
pub const REDO: &str = "editor_redo";

/// Write the scene back over the directory it came from.
pub const SAVE: &str = "editor_save";

/// Put the whole scene back in view.
pub const FRAME: &str = "editor_frame";

/// One thing the keyboard asked for this frame.
///
/// Collected out of the map and applied afterwards, because reading the map
/// borrows it and applying a command borrows the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Move the selection along an axis, in metres.
    Nudge {
        /// 0, 1 or 2 — the index the command's `position.N` path names.
        axis: usize,
        /// Which way, `+1` or `-1`; the distance is [`crate::app::NUDGE_M`].
        sign: f64,
    },
    /// Walk the undo log back one entry.
    Undo,
    /// Walk it forward one entry.
    Redo,
    /// Write the scene back over the directory it came from.
    Save,
    /// Put the whole scene back in view.
    Frame,
}

/// The editor's map: its own actions in the default context, with the reserved
/// `ui` and `text` contexts declared beside them and **off the stack**.
///
/// Neither reserved context is pushed here — [`push_ui`] and
/// [`crcbl::input::text::sync`] are what put them on, once a frame, from what
/// the tree says about itself.
///
/// # Panics
///
/// If a reserved name clashes with one of the editor's, which is a mistake in
/// this file and cannot depend on anything at run time.
#[must_use]
pub fn map() -> ActionMap {
    let mut map = ActionMap::new();
    let button = |name: &str, bindings: Vec<Binding>| ActionDecl {
        name: name.to_owned(),
        kind: ActionKind::Button,
        bindings,
    };
    map.declare(ActionDecl {
        name: MOVE.to_owned(),
        kind: ActionKind::Axis2,
        bindings: vec![Binding::Wasd {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
        }],
    });
    map.declare(ActionDecl {
        name: LIFT.to_owned(),
        kind: ActionKind::Axis1,
        bindings: vec![Binding::KeyAxis {
            negative: KeyCode::PageDown,
            positive: KeyCode::PageUp,
        }],
    });
    map.declare(button(
        UNDO,
        vec![Binding::Chord {
            modifier: Modifier::Control,
            key: KeyCode::KeyZ,
        }],
    ));
    map.declare(button(
        REDO,
        vec![Binding::Chord {
            modifier: Modifier::Control,
            key: KeyCode::KeyY,
        }],
    ));
    map.declare(button(
        SAVE,
        vec![Binding::Chord {
            modifier: Modifier::Control,
            key: KeyCode::KeyS,
        }],
    ));
    map.declare(button(FRAME, vec![Binding::Key(KeyCode::KeyF)]));

    // Holding a nudge key repeats it, which is how a coarse move is made — the
    // schedule the reserved navigation actions carry, so a held arrow moves an
    // entity at the rate a held arrow walks a list.
    for name in [MOVE, LIFT] {
        map.set_repeat(name, Some(Repeat::UI))
            .expect("the editor declares both nudge actions");
    }

    ui::declare(&mut map).expect("nothing reserved clashes with the editor's names");
    text::declare(&mut map).expect("nothing reserved clashes with the editor's names");

    // **The reserved `ui` context's WASD is rebound away**, to the arrows
    // alone. Its default binds `s`, so a pushed `ui` would own the key and
    // `Ctrl+S` would stop saving the moment a panel took the keyboard — and a
    // save that works only while nothing is selected is worse than no
    // shortcut. `docs/backlog.md`'s _What UI rung 7d1 shipped without_ records
    // that default as unsettled, for the same collision under four samples'
    // start panels. Navigation keeps the arrows, Tab, Enter, Space and Escape.
    // Only the keys are narrowed: the pad bindings stay, as the engine loop's
    // own `menu_actions` keeps them, though the editor feeds no pad yet.
    let pads = map
        .bindings(ui::MOVE)
        .unwrap_or_default()
        .iter()
        .filter(|binding| binding.reads_gamepad())
        .cloned();
    let arrows = Binding::Wasd {
        up: KeyCode::ArrowUp,
        down: KeyCode::ArrowDown,
        left: KeyCode::ArrowLeft,
        right: KeyCode::ArrowRight,
    };
    let bindings = std::iter::once(arrows).chain(pads).collect();
    map.rebind(ui::MOVE, bindings)
        .expect("the reserved context was just declared");
    map
}

/// Puts the reserved `ui` context on `map`'s stack, if it is not already there.
///
/// **Pushed and never popped here**, which is the engine loop's own asymmetry
/// (`crcbl::engine`'s menu pump): `text` goes on *over* `ui`, and the stack
/// refuses an out-of-order pop, so the frame that stops needing `ui` may still
/// have `text` above it. [`pop_ui`] is what the *next* frame calls, by which
/// time [`crcbl::input::text::sync`] has taken `text` off.
///
/// # Panics
///
/// If [`map`] never ran on this map.
pub fn push_ui(map: &mut ActionMap) {
    if !map.is_context_active(ui::CONTEXT) {
        map.push_context(ui::CONTEXT)
            .expect("the editor's map declares the ui context");
    }
}

/// Takes the reserved `ui` context back off, if it is on and nothing is over
/// it. See [`push_ui`].
pub fn pop_ui(map: &mut ActionMap) {
    if map.is_context_active(ui::CONTEXT) && !map.is_context_active(text::CONTEXT) {
        map.pop_context(ui::CONTEXT)
            .expect("nothing is above the ui context");
    }
}

/// Releases every key any of the editor's actions is bound to.
///
/// What a window that lost focus obliges: no platform sends the releases for
/// what was held, so a map that was not told would nudge for ever on the next
/// frame the arrow key it never saw come up is read.
pub fn release_keys(map: &mut ActionMap) {
    let mut keys = Vec::new();
    for name in [MOVE, LIFT, UNDO, REDO, SAVE, FRAME] {
        for binding in map.bindings(name).unwrap_or_default() {
            binding.visit_keys(|key| keys.push(key));
        }
    }
    for key in keys {
        map.key_event(key, false);
    }
}

/// What the keyboard asked for this frame, from a map whose tick has already
/// begun and been fed this frame's key events.
///
/// `modifiers` is what the shell last stamped on a key event and `editing` is
/// [`crate::panel::Panels::text_editing`]; the module docs say what each
/// decides and why neither is the map's job.
#[must_use]
pub fn actions(map: &ActionMap, modifiers: Modifiers, editing: bool) -> Vec<Action> {
    let mut actions = Vec::new();
    if editing {
        return actions;
    }
    let ctrl = modifiers.contains(Modifiers::CTRL);

    // Ctrl is matched first, so a chord is never also a nudge — and the
    // unmodified half is read only when no Ctrl is held, because a chord
    // shadows a plain binding on its own key alone.
    if map.just_pressed(UNDO) {
        actions.push(if modifiers.contains(Modifiers::SHIFT) {
            Action::Redo
        } else {
            Action::Undo
        });
    }
    if map.just_pressed(REDO) {
        actions.push(Action::Redo);
    }
    if map.just_pressed(SAVE) {
        actions.push(Action::Save);
    }
    if ctrl {
        return actions;
    }

    if map.repeated(MOVE)
        && let Some(cardinal) = map.cardinal(MOVE)
    {
        let (axis, sign) = match cardinal {
            Cardinal::Left => (0, -1.0),
            Cardinal::Right => (0, 1.0),
            Cardinal::Down => (1, -1.0),
            Cardinal::Up => (1, 1.0),
        };
        actions.push(Action::Nudge { axis, sign });
    }
    if map.repeated(LIFT) {
        let lift = map.axis1(LIFT);
        if lift != 0.0 {
            actions.push(Action::Nudge {
                axis: 2,
                sign: f64::from(lift.signum()),
            });
        }
    }
    if map.just_pressed(FRAME) {
        actions.push(Action::Frame);
    }
    actions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::shell::{ButtonState, HeadlessShell, Shell, ShellEvent, WindowDesc, WindowId};

    /// A frame's length, at the editor's own rate.
    const FRAME_SECONDS: f32 = 1.0 / crate::args::DEFAULT_TICK_HZ as f32;

    /// The keys a keyboard actually holds down for `modifiers` — the left-hand
    /// one of each pair, as [`Modifier::keys`] spells them.
    fn modifier_keys(modifiers: Modifiers) -> Vec<KeyCode> {
        [
            (Modifiers::CTRL, Modifier::Control),
            (Modifiers::SHIFT, Modifier::Shift),
            (Modifiers::ALT, Modifier::Alt),
        ]
        .into_iter()
        .filter(|(bit, _)| modifiers.contains(*bit))
        .map(|(_, modifier)| modifier.keys()[0])
        .collect()
    }

    /// A map driven by a **real** backend: the headless shell builds the
    /// events, so a key carries the scancode, the keysym and the modifier state
    /// the seam actually stamps onto one rather than what this file believes.
    struct Keyboard {
        shell: HeadlessShell,
        window: WindowId,
        map: ActionMap,
        modifiers: Modifiers,
    }

    impl Keyboard {
        fn new() -> Self {
            let mut shell = HeadlessShell::new();
            let window = shell
                .create_window(&WindowDesc::default())
                .expect("the headless shell opens a window");
            Self {
                shell,
                window,
                map: map(),
                modifiers: Modifiers::empty(),
            }
        }

        /// One frame: begin the tick, pump the shell into the map, read the
        /// actions.
        fn frame(&mut self) -> Vec<Action> {
            self.map.begin_tick(FRAME_SECONDS);
            let (map, modifiers) = (&mut self.map, &mut self.modifiers);
            self.shell.pump(&mut |event| {
                if let ShellEvent::Key {
                    key_code: Some(key),
                    state,
                    repeat: false,
                    modifiers: held,
                    ..
                } = event
                {
                    *modifiers = held;
                    map.key_event(key, state == ButtonState::Pressed);
                }
            });
            actions(&self.map, self.modifiers, false)
        }

        /// Presses and releases `key` with `modifiers` held, and answers what
        /// the press frame asked for.
        ///
        /// **The modifier keys are pressed too**, not only stamped: a
        /// [`Binding::Chord`] reads its modifier as a held key, which is what a
        /// real keyboard sends — a test that only set the stamped state would
        /// pass against an editor whose chords never fire.
        fn tap(&mut self, key: KeyCode, modifiers: Modifiers) -> Vec<Action> {
            let held = modifier_keys(modifiers);
            self.shell.set_modifiers(modifiers);
            for modifier in &held {
                self.shell
                    .key_press(self.window, *modifier)
                    .expect("live window");
            }
            self.frame();

            self.shell.key_press(self.window, key).expect("live window");
            let asked = self.frame();
            self.shell
                .key_release(self.window, key)
                .expect("live window");
            self.frame();

            self.shell.set_modifiers(Modifiers::empty());
            for modifier in &held {
                self.shell
                    .key_release(self.window, *modifier)
                    .expect("live window");
            }
            self.frame();
            asked
        }
    }

    /// Each arrow names the axis and the sign the help text promises, and Page
    /// Up and Page Down are the third axis.
    #[test]
    fn the_arrows_nudge_the_axes_the_help_text_names() {
        let mut keys = Keyboard::new();
        let asked: Vec<Vec<Action>> = [
            KeyCode::ArrowRight,
            KeyCode::ArrowLeft,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ]
        .into_iter()
        .map(|key| keys.tap(key, Modifiers::empty()))
        .collect();
        assert_eq!(
            asked,
            [
                vec![Action::Nudge { axis: 0, sign: 1.0 }],
                vec![Action::Nudge {
                    axis: 0,
                    sign: -1.0
                }],
                vec![Action::Nudge { axis: 1, sign: 1.0 }],
                vec![Action::Nudge {
                    axis: 1,
                    sign: -1.0
                }],
                vec![Action::Nudge { axis: 2, sign: 1.0 }],
                vec![Action::Nudge {
                    axis: 2,
                    sign: -1.0
                }],
            ],
        );
    }

    /// **A modifier changes what a key means.** Z alone is not undo; Ctrl+Z is;
    /// Ctrl+Shift+Z is redo, which no binding in this engine can spell — see
    /// the module docs — and Ctrl+arrow is nothing at all.
    #[test]
    fn a_modifier_decides_what_a_key_means() {
        let mut keys = Keyboard::new();
        assert_eq!(keys.tap(KeyCode::KeyZ, Modifiers::empty()), []);
        assert_eq!(keys.tap(KeyCode::KeyZ, Modifiers::CTRL), [Action::Undo]);
        assert_eq!(
            keys.tap(KeyCode::KeyZ, Modifiers::CTRL | Modifiers::SHIFT),
            [Action::Redo],
        );
        assert_eq!(keys.tap(KeyCode::KeyY, Modifiers::CTRL), [Action::Redo]);
        assert_eq!(keys.tap(KeyCode::KeyS, Modifiers::CTRL), [Action::Save]);
        assert_eq!(keys.tap(KeyCode::KeyF, Modifiers::empty()), [Action::Frame]);
        assert_eq!(keys.tap(KeyCode::ArrowLeft, Modifiers::CTRL), []);
        assert_eq!(keys.tap(KeyCode::KeyF, Modifiers::CTRL), []);
    }

    /// A key this editor has no meaning for asks for nothing.
    #[test]
    fn an_unbound_key_asks_for_nothing() {
        let mut keys = Keyboard::new();
        for key in [KeyCode::KeyQ, KeyCode::Digit1, KeyCode::Home] {
            assert_eq!(keys.tap(key, Modifiers::empty()), [], "{key:?}");
        }
    }

    /// **While the `ui` context is pushed the arrows are the panel's**, and
    /// while `text` is over it the editor's chords are the field's too — which
    /// is the whole of what the contexts are for. The same keystrokes are asked
    /// for at each depth, so the difference is the stack and nothing else.
    #[test]
    fn a_pushed_context_takes_the_editors_keys_away_from_it() {
        let taps = |keys: &mut Keyboard| {
            let mut asked = Vec::new();
            asked.extend(keys.tap(KeyCode::ArrowLeft, Modifiers::empty()));
            asked.extend(keys.tap(KeyCode::KeyZ, Modifiers::CTRL));
            asked.extend(keys.tap(KeyCode::KeyS, Modifiers::CTRL));
            asked.extend(keys.tap(KeyCode::KeyF, Modifiers::empty()));
            asked
        };

        let mut keys = Keyboard::new();
        assert_eq!(
            taps(&mut keys),
            [
                Action::Nudge {
                    axis: 0,
                    sign: -1.0
                },
                Action::Undo,
                Action::Save,
                Action::Frame,
            ],
            "the editor's own keys do not reach it with no context pushed",
        );

        push_ui(&mut keys.map);
        assert_eq!(
            taps(&mut keys),
            [Action::Undo, Action::Save, Action::Frame],
            "the `ui` context did not take the arrows, or took a chord as well",
        );

        text::sync(&mut keys.map, true).expect("declared, and `ui` is below");
        assert_eq!(
            taps(&mut keys),
            [],
            "a key typed into a field still reached the editor",
        );

        text::sync(&mut keys.map, false).expect("declared and on top");
        pop_ui(&mut keys.map);
        assert_eq!(
            taps(&mut keys),
            [
                Action::Nudge {
                    axis: 0,
                    sign: -1.0
                },
                Action::Undo,
                Action::Save,
                Action::Frame,
            ],
            "the keys did not come back when both contexts came off",
        );
    }

    /// **Page Up is the key the context stack cannot take away**, and the rule
    /// that does: no reserved context binds it, so with `text` pushed it still
    /// asks for a nudge — and `editing` is what stops it.
    #[test]
    fn a_key_no_reserved_context_binds_is_stopped_by_the_editing_rule() {
        let mut keys = Keyboard::new();
        push_ui(&mut keys.map);
        text::sync(&mut keys.map, true).expect("declared, and `ui` is below");

        let lift = Action::Nudge { axis: 2, sign: 1.0 };
        assert_eq!(
            keys.tap(KeyCode::PageUp, Modifiers::empty()),
            [lift],
            "Page Up is bound by a reserved context after all, so the rule below \
             is not the thing keeping it out",
        );

        keys.shell
            .key_press(keys.window, KeyCode::PageUp)
            .expect("live window");
        keys.map.begin_tick(FRAME_SECONDS);
        let (map, modifiers) = (&mut keys.map, &mut keys.modifiers);
        keys.shell.pump(&mut |event| {
            if let ShellEvent::Key {
                key_code: Some(key),
                state,
                repeat: false,
                modifiers: held,
                ..
            } = event
            {
                *modifiers = held;
                map.key_event(key, state == ButtonState::Pressed);
            }
        });
        assert_eq!(
            actions(&keys.map, keys.modifiers, false),
            [lift],
            "the same press asks for nothing even when nothing is being typed into",
        );
        assert_eq!(
            actions(&keys.map, keys.modifiers, true),
            [],
            "a key typed into a field still reached the editor",
        );
    }

    /// **A key held when focus goes elsewhere is released**, so a nudge does
    /// not repeat for ever against a release the platform never sends.
    #[test]
    fn losing_focus_releases_what_was_held() {
        let mut keys = Keyboard::new();
        keys.shell
            .key_press(keys.window, KeyCode::ArrowRight)
            .expect("live window");
        assert_eq!(keys.frame(), [Action::Nudge { axis: 0, sign: 1.0 }]);

        release_keys(&mut keys.map);
        // Long enough that a still-held key would have repeated several times.
        for _ in 0..60 {
            assert_eq!(keys.frame(), [], "a released key kept nudging");
        }
    }
}
