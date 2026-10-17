//! The join between the shell and `crcbl-ui`'s text input.
//!
//! [`TextPump`] lives in [`crcbl_app::text_input`], whose module docs say how
//! a caller drives it, and is re-exported here at the path it has always had.
//! The tests below stay in the umbrella because they drive the pump the way a
//! game does, through [`crate::input`]'s `text` and `list` contexts and
//! [`crate::ui_nav::nav_input`], which `crcbl-app` does not link.

pub use crcbl_app::text_input::TextPump;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec2;

    use super::*;
    use crate::core::input::{KeyCode, Modifiers};
    use crate::input::{ActionDecl, ActionKind, ActionMap, Binding, list, text, ui};
    use crate::shell::{
        ButtonState, HeadlessShell, MimeType, Shell, ShellCaps, ShellEvent, WindowDesc, WindowId,
    };
    use crate::ui::draw_list::{DrawCommand, DrawList};
    use crate::ui::text::FontAtlas;
    use crate::ui::tree::{AvailableSpace, Direction, NavInput, Ui};
    use crate::ui::widget::PointerInput;
    use crate::ui_nav::nav_input;

    const FRAME: Duration = Duration::from_millis(16);
    const TICK: f32 = 1.0 / 60.0;

    /// What the menu's drop-down offers.
    const FRUIT: [&str; 3] = ["apple", "banana", "cherry"];

    /// A menu: a text input `#name`, a button `#ok` and a drop-down `#fruit`
    /// over [`FRUIT`], over a game that walks on WASD — with the shell, the
    /// action map and the pump that join them, driven the way the module docs
    /// say.
    struct Menu {
        shell: HeadlessShell,
        window: WindowId,
        map: ActionMap,
        pump: TextPump,
        ui: Ui,
        name: String,
        fruit: usize,
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
            list::declare(&mut map).expect("nothing reserved is taken");
            map.push_context(ui::CONTEXT).expect("declared");
            let mut menu = Self {
                shell,
                window,
                map,
                pump: TextPump::new(),
                ui: Ui::new(),
                name: String::new(),
                fruit: 0,
                list: DrawList::new(),
            };
            menu.frame();
            menu
        }

        /// One frame: pump the shell into the map and the text pump, run the
        /// tree, and serve its clipboard requests. Returns the frame's
        /// navigation input.
        fn frame(&mut self) -> NavInput {
            let editing = self.ui.text_editing() || self.ui.popup_list_open();
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
            let (name, fruit) = (&mut self.name, &mut self.fruit);
            self.ui.block("#page", &[], |ui| {
                ui.text_input("#name", name);
                ui.button("#ok", "OK");
                ui.select("#fruit", &FRUIT, fruit);
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
            // Whichever is coming off first: the two are never on together.
            let (editing, open) = (self.ui.text_editing(), self.ui.popup_list_open());
            if !editing {
                text::sync(&mut self.map, false).expect("declared and on top");
            }
            list::sync(&mut self.map, open).expect("declared and on top");
            text::sync(&mut self.map, editing).expect("declared and on top");
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

    /// **An open drop-down's list takes the letters and the jump keys**: a
    /// typed `c` reaches cherry through the shell's commit, and W — `ui_move`
    /// anywhere else — reaches neither navigation nor the game, Home goes to
    /// the first option and End to the last, nothing is typed into the field,
    /// and once the list closes W is `ui_move` again.
    #[test]
    fn an_open_list_takes_the_letters_and_the_jump_keys() {
        let mut menu = Menu::new(HeadlessShell::new());
        for _ in 0..3 {
            menu.tap(KeyCode::Tab, Modifiers::empty());
        }
        menu.tap(KeyCode::Enter, Modifiers::empty());
        assert!(menu.ui.popup_list_open(), "Enter did not open the list");
        // The focused option's label is its first child.
        let option = |menu: &Menu| {
            let focused = menu.ui.focused().expect("focus is in the list");
            let label = menu.ui.child_keys(focused).first().copied();
            FRUIT
                .into_iter()
                .find(|&fruit| label.is_some_and(|label| menu.ui.text(label) == Some(fruit)))
                .expect("focus is on an option")
        };
        menu.type_text(&[KeyCode::KeyC], "c");
        assert_eq!(option(&menu), "cherry", "the typed c was not followed");
        let navs = menu.type_text(&[KeyCode::KeyW], "w");
        assert_eq!(navs, [NavInput::NAVIGATION], "W reached the ui");
        assert_eq!(menu.map.axis2("walk"), (0.0, 0.0), "W walked");
        menu.tap(KeyCode::Home, Modifiers::empty());
        assert_eq!(option(&menu), "apple", "Home did not reach the first");
        menu.tap(KeyCode::End, Modifiers::empty());
        assert_eq!(option(&menu), "cherry", "End did not reach the last");
        assert_eq!(menu.name, "", "the typing reached the field");

        menu.tap(KeyCode::Escape, Modifiers::empty());
        assert!(!menu.ui.popup_list_open(), "Escape left the list open");
        let walk = menu.tap(KeyCode::KeyW, Modifiers::empty());
        assert_eq!(walk, NavInput::toward(Direction::Up), "W is not ui_move");
        assert_eq!(menu.fruit, 0, "nothing was picked");
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
}
