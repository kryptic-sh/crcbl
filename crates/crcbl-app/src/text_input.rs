//! The join between the shell and `crcbl-ui`'s text input: key events and
//! committed text become the tree's [`TextInput`], and the tree's clipboard
//! requests become the shell's clipboard calls.
//!
//! Here, beside the shell and the tree rather than in either: the tree names no
//! shell and no action map, and the shell knows nothing of a tree. The `crcbl`
//! umbrella re-exports it as `crcbl::text_input::TextPump`.
//!
//! # How a caller drives it
//!
//! ```text
//! every ShellEvent ─→ TextPump::observe(event, ui.text_editing() || ui.popup_list_open())
//!                     (and the ActionMap, with input::text::sync(map, ui.text_editing())
//!                      and input::list::sync(map, ui.popup_list_open()))
//! ui.begin_frame_with(pointer, nav_input(map))
//! ui.set_text_input(pump.frame(dt))
//! ... build ...
//! pump.serve(ui.take_clipboard_requests(), shell, window)
//! ```
//!
//! **Typing is kept from actions by the context stack**, not by this pump:
//! `crcbl_input::text::sync` pushes the `text` context
//! over `ui` while the tree reports an engaged text input, and that context
//! owns every key that types or edits, so none of them is also `ui_move`,
//! `ui_accept` or a game's binding. The pump reads the same key events for
//! the edits they make, and only while the tree is editing.
//!
//! **An open pop-up list takes typing too**, for its typeahead:
//! `crcbl_input::list::sync` pushes the `list` context
//! over `ui` while [`Ui::popup_list_open`] says so, which owns the keys that
//! type a character, and the pump is told to collect while it is. The list
//! reads the committed text and nothing else, so the edits the same keys make
//! for a field are collected and ignored. Never both at once: a list takes no
//! keys while something is engaged, so pop whichever is coming off before
//! pushing the other.
//!
//! [`Ui::popup_list_open`]: crcbl_ui::tree::Ui::popup_list_open
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
//! [`ShellError::Unsupported`](crcbl_shell::ShellError::Unsupported) to both
//! calls — so in a browser copy, cut and paste each turn the field's border
//! red and nothing reaches or leaves the system clipboard. An unreadable or
//! non-UTF-8 clipboard is refused the same way; an empty one is not, and a
//! paste of nothing does nothing.
//!
//! # IME
//!
//! A [`ShellEvent::TextCommit`] is an [`Edit::Insert`], so an input method's
//! committed text lands like typing. **A composition in progress is shown**:
//! the shell reports it as [`ShellEvent::TextPreedit`] (the Win32, Wayland and
//! AppKit backends do), the pump keeps the latest until a commit, an empty one
//! or the field's end clears it, and hands it to the tree each frame as
//! [`TextInput::preedit`], which the engaged field underlines at its caret.
//! **The input method follows the field**: [`TextPump::engage`] turns it on
//! while the tree is editing and off otherwise, which is what lets a Wayland
//! input method compose at all (see `Shell::set_text_input`), and
//! [`TextPump::place`] tells it where the caret is, so its candidate window
//! opens there rather than at the window's corner.
//!
//! ```text
//! pump.engage(ui.text_editing_purpose(), shell, window)
//! pump.place(ui.text_input_area(), scale, shell, window)   // after engage
//! ```

use std::time::Duration;

use crcbl_shell::{
    ButtonState, ClipboardContent, ClipboardOffer, ClipboardRequestId, MimeType, PhysicalRect,
    Shell, ShellEvent, TextPurpose, WindowId,
};
use crcbl_ui::edit::{ClipboardOp, Edit};
use crcbl_ui::tree::{
    ClipboardAnswer, ClipboardReply, ClipboardRequest, NodeKey, Preedit, TextInput,
};
use glam::Vec2;

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
    /// What [`engage`](Self::engage) last told the shell, so it asks only on a
    /// change; `None` until it has told it anything.
    engaged: Option<Option<TextPurpose>>,
    /// The input method's composition in progress, until a commit, an empty
    /// one or the field's end clears it.
    preedit: Option<Preedit>,
    /// What [`place`](Self::place) last told the shell, so it asks only on a
    /// change; `None` until it has, and again whenever the field changes.
    area: Option<Option<PhysicalRect>>,
}

impl TextPump {
    /// A pump with nothing collected and nothing outstanding.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes what `event` means to a text input: while `editing` — a field
    /// engaged, or a list open for its typeahead (see the module docs) — a
    /// key press's or repeat's [`Edit`] and a commit's text; and, whenever it
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
                self.preedit = None;
                true
            }
            ShellEvent::TextPreedit { text, cursor, .. } if editing => {
                self.preedit = (!text.is_empty()).then(|| Preedit {
                    text: text.clone(),
                    cursor: *cursor,
                });
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
    /// edit and answer observed since the last call, and the composition in
    /// progress, which lasts until it is cleared.
    ///
    /// [`Ui::set_text_input`]: crcbl_ui::tree::Ui::set_text_input
    pub fn frame(&mut self, dt: Duration) -> TextInput {
        TextInput {
            dt,
            edits: std::mem::take(&mut self.edits),
            clipboard: std::mem::take(&mut self.answers),
            preedit: self.preedit.clone(),
        }
    }

    /// Carries `requests` — [`Ui::take_clipboard_requests`]'s — to `shell`
    /// for `window`: an offer as `text/plain`, a read as a request whose
    /// answer [`observe`](Self::observe) waits for. A refusal is a
    /// [`ClipboardReply::Refused`] on the next frame and a warning now.
    ///
    /// [`Ui::take_clipboard_requests`]: crcbl_ui::tree::Ui::take_clipboard_requests
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

    /// Tells the platform input method what field of `window` is taking
    /// typing, with [`Shell::set_text_input`]: `field` is
    /// [`Ui::text_editing_purpose`](crcbl_ui::tree::Ui::text_editing_purpose)
    /// or the like, `None` while nothing is being typed into. The shell is
    /// asked only when that changes, so an input method composes into the
    /// field that is taking the keys, keeps out of a game's way, and is told
    /// when a field is a password.
    pub fn engage<S: Shell + ?Sized>(
        &mut self,
        field: Option<TextPurpose>,
        shell: &mut S,
        window: WindowId,
    ) {
        if self.engaged == Some(field) {
            return;
        }
        // A field's end ends its composition, and a new field's input method
        // has not been told where it is.
        if field.is_none() {
            self.preedit = None;
        }
        self.area = None;
        match shell.set_text_input(window, field) {
            Ok(()) => self.engaged = Some(field),
            Err(error) => crcbl_core::warn!(
                "text input: the shell refused to set the input method to {field:?} — {error}"
            ),
        }
    }

    /// Tells the platform input method where the field of `window` taking
    /// typing has its caret, with [`Shell::set_text_input_area`]: `area` is
    /// [`Ui::text_input_area`](crcbl_ui::tree::Ui::text_input_area)'s corners
    /// in the tree's pixels, which `scale` turns into the window's device
    /// pixels — `1.0` for a tree laid out in them. Call it after
    /// [`engage`](Self::engage), which a Wayland input method needs first.
    /// The shell is asked only when the area changes, and is told `None`
    /// while [`engage`](Self::engage) last named no field: a tree can keep a
    /// field engaged while its panel is shut, and an area kept from then
    /// would be sent with the next `enable`, before this frame's.
    pub fn place<S: Shell + ?Sized>(
        &mut self,
        area: Option<(Vec2, Vec2)>,
        scale: f32,
        shell: &mut S,
        window: WindowId,
    ) {
        let area = area
            .filter(|_| matches!(self.engaged, Some(Some(_))))
            .map(|(min, max)| device_rect(min * scale, max * scale));
        if self.area == Some(area) {
            return;
        }
        match shell.set_text_input_area(window, area) {
            Ok(()) => self.area = Some(area),
            Err(error) => crcbl_core::warn!(
                "text input: the shell refused to place the input method at {area:?} — {error}"
            ),
        }
    }
}

/// The device pixels a box from `min` to `max` covers: out to whole pixels on
/// every side, and never narrower or shorter than one.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window's pixels fit an i32, and a floored-and-ceiled extent of at least one is positive"
)]
fn device_rect(min: Vec2, max: Vec2) -> PhysicalRect {
    let (min, max) = (min.floor(), max.ceil());
    PhysicalRect {
        x: min.x as i32,
        y: min.y as i32,
        width: (max.x - min.x).max(1.0) as u32,
        height: (max.y - min.y).max(1.0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_core::input::KeyCode;
    use crcbl_shell::{HeadlessShell, WindowDesc};

    const FRAME: Duration = Duration::from_millis(16);

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

    /// **The input method follows the field**: told while the tree is
    /// editing, and what for, cleared once it stops, and the shell is asked
    /// only when that changes — a state set behind the pump's back is left
    /// alone until it does.
    #[test]
    fn the_input_method_is_told_of_the_field_only_while_one_is_editing() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        let mut pump = TextPump::new();
        let field = |shell: &HeadlessShell| shell.text_input(window).expect("live");
        assert_eq!(field(&shell), None, "a window starts with no field");

        pump.engage(Some(TextPurpose::Normal), &mut shell, window);
        assert_eq!(field(&shell), Some(TextPurpose::Normal));
        pump.engage(Some(TextPurpose::Password), &mut shell, window);
        assert_eq!(field(&shell), Some(TextPurpose::Password));
        pump.engage(None, &mut shell, window);
        assert_eq!(field(&shell), None);

        shell
            .set_text_input(window, Some(TextPurpose::Pin))
            .expect("set behind the pump's back");
        pump.engage(None, &mut shell, window);
        assert_eq!(
            field(&shell),
            Some(TextPurpose::Pin),
            "no change, so nothing was asked"
        );
        pump.engage(Some(TextPurpose::Normal), &mut shell, window);
        pump.engage(None, &mut shell, window);
        assert_eq!(field(&shell), None);
    }

    /// Pumps `shell` into `pump`, editing, and hands back the frame after.
    fn pumped(shell: &mut HeadlessShell, pump: &mut TextPump) -> TextInput {
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        for event in &events {
            pump.observe(event, true);
        }
        pump.frame(FRAME)
    }

    /// **A composition is held until a commit, an empty one or the field's
    /// end clears it**, frame after frame, and one arriving while nothing is
    /// editing is not the pump's.
    #[test]
    fn a_composition_is_held_until_a_commit_an_empty_one_or_the_fields_end() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        let mut pump = TextPump::new();
        let kana = Some(Preedit {
            text: "かな".to_owned(),
            cursor: Some(3),
        });

        shell.preedit_text(window, "かな", Some(3)).expect("live");
        assert_eq!(pumped(&mut shell, &mut pump).preedit, kana);
        assert_eq!(
            pumped(&mut shell, &mut pump).preedit,
            kana,
            "the composition was not held to the next frame"
        );

        shell.commit_text(window, "仮名").expect("live");
        let committed = pumped(&mut shell, &mut pump);
        assert_eq!(committed.edits, [Edit::Insert("仮名".to_owned())]);
        assert_eq!(committed.preedit, None, "a commit left its composition");

        shell.preedit_text(window, "かな", Some(3)).expect("live");
        pumped(&mut shell, &mut pump);
        shell.preedit_text(window, "", None).expect("live");
        assert_eq!(
            pumped(&mut shell, &mut pump).preedit,
            None,
            "an empty one did not clear it"
        );

        shell.preedit_text(window, "かな", Some(3)).expect("live");
        pumped(&mut shell, &mut pump);
        pump.engage(Some(TextPurpose::Normal), &mut shell, window);
        pump.engage(None, &mut shell, window);
        assert_eq!(
            pump.frame(FRAME).preedit,
            None,
            "the field's end did not clear it"
        );

        shell.preedit_text(window, "かな", None).expect("live");
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        assert!(
            !pump.observe(&events[0], false),
            "taken while nothing edits"
        );
        assert_eq!(pump.frame(FRAME).preedit, None);
    }

    /// **The caret's area reaches the shell in device pixels**, out to whole
    /// pixels, only when it changes — and again once the field changes, whose
    /// input method has not been told it.
    #[test]
    fn the_caret_area_is_told_in_device_pixels_when_it_changes() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        let mut pump = TextPump::new();
        let area = |shell: &HeadlessShell| shell.text_input_area(window).expect("live");
        let caret = Some((Vec2::new(10.2, 20.0), Vec2::new(11.2, 36.0)));
        let told = Some(PhysicalRect {
            x: 20,
            y: 40,
            width: 3,
            height: 32,
        });

        pump.engage(Some(TextPurpose::Normal), &mut shell, window);
        pump.place(caret, 2.0, &mut shell, window);
        assert_eq!(area(&shell), told);

        shell
            .set_text_input_area(window, None)
            .expect("cleared behind the pump's back");
        pump.place(caret, 2.0, &mut shell, window);
        assert_eq!(area(&shell), None, "no change, so nothing was asked");

        pump.engage(Some(TextPurpose::Password), &mut shell, window);
        pump.place(caret, 2.0, &mut shell, window);
        assert_eq!(area(&shell), told, "a new field was not told");

        pump.place(None, 2.0, &mut shell, window);
        assert_eq!(area(&shell), None);
    }

    /// **No field, no area**: a caret handed over while nothing is engaged
    /// is not told, so the shell has no stale area to send with the next
    /// `enable`.
    #[test]
    fn no_area_is_told_while_no_field_is_engaged() {
        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("a headless window");
        let mut pump = TextPump::new();
        let caret = Some((Vec2::new(10.0, 20.0), Vec2::new(11.0, 36.0)));
        let area = |shell: &HeadlessShell| shell.text_input_area(window).expect("live");

        pump.engage(None, &mut shell, window);
        pump.place(caret, 1.0, &mut shell, window);
        assert_eq!(area(&shell), None, "an area was told with no field engaged");

        pump.engage(Some(TextPurpose::Normal), &mut shell, window);
        pump.place(caret, 1.0, &mut shell, window);
        assert!(
            area(&shell).is_some(),
            "the engaged field's area was not told"
        );
    }
}
