//! The join between the shell and `crcbl-ui`'s text input: key events and
//! committed text become the tree's [`TextInput`], and the tree's clipboard
//! requests become the shell's clipboard calls.
//!
//! Here, in the umbrella, for [`crate::nav`]'s reason: the tree names no shell
//! and no action map, and the shell knows nothing of a tree.
//!
//! # How a caller drives it
//!
//! ```text
//! every ShellEvent ─→ TextPump::observe(event, ui.text_editing())
//!                     (and the ActionMap, with input::text::sync(map, ui.text_editing()))
//! ui.begin_frame_with(pointer, nav_input(map))
//! ui.set_text_input(pump.frame(dt))
//! ... build ...
//! pump.serve(ui.take_clipboard_requests(), shell, window)
//! ```
//!
//! **Typing is kept from actions by the context stack**, not by this pump:
//! [`input::text::sync`](crate::input::text::sync) pushes the `text` context
//! over `ui` while the tree reports an engaged text input, and that context
//! owns every key that types or edits, so none of them is also `ui_move`,
//! `ui_accept` or a game's binding. The pump reads the same key events for
//! the edits they make, and only while the tree is editing.
//!
//! # The clipboard, per backend
//!
//! A copy or cut is [`Shell::clipboard_offer`] and a paste is
//! [`Shell::clipboard_request`], whose answer arrives later as a
//! [`ShellEvent::ClipboardData`] this pump matches by request id — so a game's
//! own read is not taken — and hands to the tree on the next frame. **A backend
//! that refuses says so on screen**: the offer or the read's error becomes a
//! [`ClipboardReply::Refused`] for the input that asked, which sets its
//! `:refused` state and `default.css`'s red border, and a warning names the
//! error. The web backend is that backend today — it answers
//! [`ShellError::Unsupported`](crate::shell::ShellError::Unsupported) to both
//! calls — so in a browser copy, cut and paste each turn the field's border
//! red and nothing reaches or leaves the system clipboard. An unreadable or
//! non-UTF-8 clipboard is refused the same way; an empty one is not, and a
//! paste of nothing does nothing.
//!
//! # IME
//!
//! A [`ShellEvent::TextCommit`] is an [`Edit::Insert`], so an input method's
//! committed text lands like typing. **Pre-edit is not drawn**: the shell reports
//! a composition in progress as [`ShellEvent::TextPreedit`] (the Win32 backend
//! does; the AppKit backend records how much marked text an input method holds
//! and surfaces none of it), and this field does not yet underline it at the
//! caret or call `Shell::set_text_input_area`.

use std::time::Duration;

use crate::shell::{
    ButtonState, ClipboardContent, ClipboardOffer, ClipboardRequestId, MimeType, Shell, ShellEvent,
    WindowId,
};
use crate::ui::edit::{ClipboardOp, Edit};
use crate::ui::tree::{ClipboardAnswer, ClipboardReply, ClipboardRequest, NodeKey, TextInput};

/// Collects a frame's text input from shell events and carries the tree's
/// clipboard requests to the shell. See the module docs.
#[derive(Debug, Default)]
pub struct TextPump {
    edits: Vec<Edit>,
    answers: Vec<ClipboardAnswer>,
    /// The read this pump issued and no answer has arrived for, and the input
    /// that asked. A newer paste replaces it: the newer press is the one the
    /// person is waiting on.
    awaiting: Option<(ClipboardRequestId, NodeKey)>,
}

impl TextPump {
    /// A pump with nothing collected and nothing outstanding.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes what `event` means to a text input: while `editing`, a key
    /// press's or repeat's [`Edit`] and a commit's text; and, whenever it
    /// arrives, the answer to this pump's outstanding read. Returns whether
    /// the event was the text input's — a key press that makes no edit is
    /// not, and neither is another caller's clipboard answer.
    pub fn observe(&mut self, event: &ShellEvent, editing: bool) -> bool {
        match event {
            ShellEvent::Key {
                key_code: Some(key),
                state: ButtonState::Pressed,
                modifiers,
                ..
            } if editing => match Edit::for_key(*key, *modifiers) {
                Some(edit) => {
                    self.edits.push(edit);
                    true
                }
                None => false,
            },
            ShellEvent::TextCommit { text, .. } if editing => {
                self.edits.push(Edit::Insert(text.clone()));
                true
            }
            ShellEvent::ClipboardData {
                request, content, ..
            } => {
                let Some((_, to)) = self.awaiting.take_if(|(awaited, _)| awaited == request) else {
                    return false;
                };
                let reply = match content {
                    ClipboardContent::Empty => ClipboardReply::Empty,
                    ClipboardContent::Bytes(_) => match content.text() {
                        Some(text) => ClipboardReply::Text(text.to_owned()),
                        None => {
                            crcbl_core::warn!("text input: the clipboard holds no UTF-8 text");
                            ClipboardReply::Refused
                        }
                    },
                    ClipboardContent::Unavailable => {
                        crcbl_core::warn!("text input: the clipboard could not be read");
                        ClipboardReply::Refused
                    }
                };
                self.answers.push(ClipboardAnswer { to, reply });
                true
            }
            _ => false,
        }
    }

    /// This frame's text input for [`Ui::set_text_input`], `dt` long: every
    /// edit and answer observed since the last call.
    ///
    /// [`Ui::set_text_input`]: crate::ui::tree::Ui::set_text_input
    pub fn frame(&mut self, dt: Duration) -> TextInput {
        TextInput {
            dt,
            edits: std::mem::take(&mut self.edits),
            clipboard: std::mem::take(&mut self.answers),
        }
    }

    /// Carries `requests` — [`Ui::take_clipboard_requests`]'s — to `shell`
    /// for `window`: an offer as `text/plain`, a read as a request whose
    /// answer [`observe`](Self::observe) waits for. A refusal is a
    /// [`ClipboardReply::Refused`] on the next frame and a warning now.
    ///
    /// [`Ui::take_clipboard_requests`]: crate::ui::tree::Ui::take_clipboard_requests
    pub fn serve<S: Shell + ?Sized>(
        &mut self,
        requests: Vec<ClipboardRequest>,
        shell: &mut S,
        window: WindowId,
    ) {
        for ClipboardRequest { from, op } in requests {
            let refused = match op {
                ClipboardOp::Offer(text) => shell
                    .clipboard_offer(window, &[ClipboardOffer::text(&text)])
                    .err()
                    .map(|error| ("copy or cut", error)),
                ClipboardOp::Read => match shell.clipboard_request(window, MimeType::TextUtf8) {
                    Ok(request) => {
                        self.awaiting = Some((request, from));
                        None
                    }
                    Err(error) => Some(("paste", error)),
                },
            };
            if let Some((what, error)) = refused {
                crcbl_core::warn!("text input: the clipboard refused the {what} — {error}");
                self.answers.push(ClipboardAnswer {
                    to: from,
                    reply: ClipboardReply::Refused,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::core::input::{KeyCode, Modifiers};
    use crate::input::{ActionDecl, ActionKind, ActionMap, Binding, text, ui};
    use crate::nav::nav_input;
    use crate::shell::{HeadlessShell, ShellCaps, WindowDesc};
    use crate::ui::draw_list::{DrawCommand, DrawList};
    use crate::ui::text::FontAtlas;
    use crate::ui::tree::{AvailableSpace, Direction, NavInput, Ui};
    use crate::ui::widget::PointerInput;

    const FRAME: Duration = Duration::from_millis(16);
    const TICK: f32 = 1.0 / 60.0;

    /// A menu: a text input `#name` above a button `#ok`, over a game that
    /// walks on WASD — with the shell, the action map and the pump that join
    /// them, driven the way the module docs say.
    struct Menu {
        shell: HeadlessShell,
        window: WindowId,
        map: ActionMap,
        pump: TextPump,
        ui: Ui,
        name: String,
        list: DrawList,
    }

    impl Menu {
        fn new(mut shell: HeadlessShell) -> Self {
            let window = shell
                .create_window(&WindowDesc::default())
                .expect("a headless window");
            let mut map = ActionMap::new();
            map.declare(ActionDecl {
                name: "walk".to_owned(),
                kind: ActionKind::Axis2,
                bindings: vec![Binding::Wasd {
                    up: KeyCode::KeyW,
                    down: KeyCode::KeyS,
                    left: KeyCode::KeyA,
                    right: KeyCode::KeyD,
                }],
            });
            ui::declare(&mut map).expect("nothing reserved is taken");
            text::declare(&mut map).expect("nothing reserved is taken");
            map.push_context(ui::CONTEXT).expect("declared");
            let mut menu = Self {
                shell,
                window,
                map,
                pump: TextPump::new(),
                ui: Ui::new(),
                name: String::new(),
                list: DrawList::new(),
            };
            menu.frame();
            menu
        }

        /// One frame: pump the shell into the map and the text pump, run the
        /// tree, and serve its clipboard requests. Returns the frame's
        /// navigation input.
        fn frame(&mut self) -> NavInput {
            let editing = self.ui.text_editing();
            self.map.begin_tick(TICK);
            let mut events = Vec::new();
            self.shell.pump(&mut |event| events.push(event));
            for event in &events {
                if let ShellEvent::Key {
                    key_code: Some(key),
                    state,
                    repeat: false,
                    ..
                } = event
                {
                    self.map.key_event(*key, *state == ButtonState::Pressed);
                }
                self.pump.observe(event, editing);
            }
            let nav = nav_input(&self.map);
            self.ui
                .begin_frame_with(PointerInput::hovering(Vec2::splat(-1.0)), nav);
            self.ui.set_text_input(self.pump.frame(FRAME));
            let name = &mut self.name;
            self.ui.block("#page", &[], |ui| {
                ui.text_input("#name", name);
                ui.button("#ok", "OK");
            });
            self.ui.layout(
                Vec2::ZERO,
                AvailableSpace::definite(Vec2::splat(200.0)),
                &FontAtlas::built_in(),
            );
            self.list = DrawList::new();
            self.ui.emit(&mut self.list);
            let requests = self.ui.take_clipboard_requests();
            self.pump.serve(requests, &mut self.shell, self.window);
            text::sync(&mut self.map, self.ui.text_editing()).expect("declared and on top");
            nav
        }

        /// Presses and releases `key` with `modifiers` held, a frame each.
        fn tap(&mut self, key: KeyCode, modifiers: Modifiers) -> NavInput {
            self.shell.set_modifiers(modifiers);
            self.shell.key_press(self.window, key).expect("live window");
            let nav = self.frame();
            self.shell
                .key_release(self.window, key)
                .expect("live window");
            self.frame();
            self.shell.set_modifiers(Modifiers::empty());
            nav
        }

        /// Types `text` the way a keyboard does: each character's key press,
        /// then the layout's commit of it.
        fn type_text(&mut self, keys: &[KeyCode], text: &str) -> Vec<NavInput> {
            let mut navs = Vec::new();
            for (key, character) in keys.iter().zip(text.chars()) {
                self.shell
                    .key_press(self.window, *key)
                    .expect("live window");
                self.shell
                    .commit_text(self.window, &character.to_string())
                    .expect("live window");
                navs.push(self.frame());
                self.shell
                    .key_release(self.window, *key)
                    .expect("live window");
                self.frame();
            }
            navs
        }

        /// Focuses the input with the keyboard and engages it with Enter.
        fn engage(&mut self) {
            self.tap(KeyCode::ArrowDown, Modifiers::empty());
            self.tap(KeyCode::Enter, Modifiers::empty());
            assert!(self.ui.text_editing(), "Enter did not engage the input");
        }

        /// Whether the frame drew the input's border in `default.css`'s
        /// refused red.
        fn drew_refused_border(&self) -> bool {
            let red = [0xe5, 0x48, 0x4d].map(|byte: u8| {
                let encoded = f32::from(byte) / 255.0;
                if encoded <= 0.040_45 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                }
            });
            self.list.commands().iter().any(|command| {
                matches!(command, DrawCommand::RectOutline { color, .. }
                    if color[..3].iter().zip(red).all(|(got, want)| (got - want).abs() < 1e-3))
            })
        }
    }

    /// **While the input is engaged, the keys that type reach neither `ui_*`
    /// nor the game, and type instead**: W, A, S and D, Space and the arrows
    /// move no focus and walk nothing, while the text arrives and the arrows
    /// move the caret. Enter still commits, and once it has, W is `ui_move`
    /// again.
    #[test]
    fn typing_keys_type_and_reach_neither_navigation_nor_the_game() {
        let mut menu = Menu::new(HeadlessShell::new());
        menu.engage();
        let keys = [
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::Space,
            KeyCode::KeyX,
        ];
        let navs = menu.type_text(&keys, "wasd x");
        for (key, nav) in keys.iter().zip(&navs) {
            assert_eq!(*nav, NavInput::NAVIGATION, "{key} reached the ui");
            assert_eq!(menu.map.axis2("walk"), (0.0, 0.0), "{key} walked");
        }
        assert_eq!(menu.name, "wasd x");
        let left = menu.tap(KeyCode::ArrowLeft, Modifiers::empty());
        assert_eq!(left, NavInput::NAVIGATION, "the arrow moved focus");
        menu.type_text(&[KeyCode::KeyQ], "q");
        assert_eq!(menu.name, "wasd qx", "the arrow did not move the caret");
        assert!(menu.ui.text_editing());

        let enter = menu.tap(KeyCode::Enter, Modifiers::empty());
        assert!(enter.accept, "Enter did not reach ui_accept");
        assert!(!menu.ui.text_editing(), "Enter did not commit");
        let walk = menu.tap(KeyCode::KeyS, Modifiers::empty());
        assert_eq!(
            walk,
            NavInput::toward(Direction::Down),
            "S is not ui_move after the commit"
        );
        assert_eq!(menu.name, "wasd qx", "a key typed after the commit");
    }

    /// **Copy offers the selection to the shell's clipboard, and paste reads
    /// it back into the selection a frame later** — through the shell's own
    /// asynchronous answer, matched to this pump's request.
    #[test]
    fn copy_and_paste_round_trip_through_the_shell() {
        let mut menu = Menu::new(HeadlessShell::new());
        menu.engage();
        menu.type_text(&[KeyCode::KeyH, KeyCode::KeyI], "hi");
        menu.tap(KeyCode::KeyA, Modifiers::CTRL);
        menu.tap(KeyCode::KeyC, Modifiers::CTRL);
        assert_eq!(
            menu.shell.clipboard_bytes(MimeType::TextUtf8),
            Some(b"hi".as_slice()),
            "the copy did not reach the clipboard"
        );
        menu.tap(KeyCode::End, Modifiers::empty());
        menu.tap(KeyCode::KeyV, Modifiers::CTRL);
        menu.frame();
        assert_eq!(menu.name, "hihi", "the paste did not land");
        menu.tap(KeyCode::KeyA, Modifiers::SUPER);
        menu.tap(KeyCode::KeyX, Modifiers::SUPER);
        assert_eq!(menu.name, "", "Command+X did not cut");
        assert_eq!(
            menu.shell.clipboard_bytes(MimeType::TextUtf8),
            Some(b"hihi".as_slice())
        );
        assert!(!menu.drew_refused_border());
    }

    /// **A backend with no clipboard shows the refusal on the field**: copy
    /// and paste both turn its border `default.css`'s red — what a browser
    /// shows today — and the next edit clears it.
    #[test]
    fn a_backend_without_a_clipboard_turns_the_border_red() {
        let caps = HeadlessShell::new().caps() - ShellCaps::CLIPBOARD;
        for shortcut in [KeyCode::KeyC, KeyCode::KeyV] {
            let mut menu = Menu::new(HeadlessShell::new().with_caps(caps));
            menu.engage();
            menu.type_text(&[KeyCode::KeyH], "h");
            menu.tap(KeyCode::KeyA, Modifiers::CTRL);
            assert!(
                !menu.drew_refused_border(),
                "red before anything was refused"
            );
            menu.tap(shortcut, Modifiers::CTRL);
            assert!(
                menu.drew_refused_border(),
                "{shortcut} was refused silently"
            );
            menu.type_text(&[KeyCode::KeyJ], "j");
            assert!(
                !menu.drew_refused_border(),
                "an edit did not clear the refusal"
            );
            assert_eq!(menu.name, "j");
        }
    }

    /// **The pump takes nothing while nothing is editing, and never takes
    /// another caller's clipboard answer.**
    #[test]
    fn nothing_is_taken_while_not_editing_nor_another_callers_answer() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        shell.key_press(window, KeyCode::ArrowLeft).expect("live");
        shell.commit_text(window, "a").expect("live");
        shell
            .clipboard_offer(window, &[ClipboardOffer::text("theirs")])
            .expect("offered");
        let theirs = shell
            .clipboard_request(window, MimeType::TextUtf8)
            .expect("read");
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        let mut pump = TextPump::new();
        for event in &events {
            assert!(!pump.observe(event, false), "{} was taken", event.name());
        }
        assert_eq!(
            pump.frame(FRAME),
            TextInput {
                dt: FRAME,
                ..TextInput::default()
            }
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ShellEvent::ClipboardData { request, .. } if *request == theirs)),
            "the other caller's answer never arrived, so the check proves nothing"
        );
    }
}
