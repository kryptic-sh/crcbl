//! `text-input-v3`: an input method's pre-edit and commits, for the window a
//! text field has asked for one in.
//!
//! One `zwp_text_input_v3` per seat, made once the compositor offers
//! `zwp_text_input_manager_v3`. Its `enter` and `leave` follow the keyboard
//! focus; what turns the input method on is the window's own request,
//! [`Shell::set_text_input`](crate::Shell::set_text_input),
//! which this backend answers with `enable` while the window has the text
//! input focus and `disable` once it asks to stop. A `done` turns what the
//! input method sent into a [`ShellEvent::TextCommit`] and a
//! [`ShellEvent::TextPreedit`] whenever the pre-edit changed.
//!
//! # Decision: off until a field asks
//!
//! An enabled input method takes keys: a Chinese one in its native mode turns
//! WASD into a candidate list. So a window starts with it off and a text field
//! turns it on while it has focus, which is what the protocol asks of a client
//! ("as the keyboard focus moves across editable and non-editable elements").
//! Keys an input method does not take arrive on `wl_keyboard` as before, and
//! type through [`xkb`](crate::linux::xkb) as before.
//!
//! # What it does not send or read
//!
//! * **No surrounding text.** The seam has no way for a field to say what is
//!   around its caret, and the protocol reads an absent
//!   `set_surrounding_text` as "not supported". So no
//!   `delete_surrounding_text` should arrive, and one that does is ignored:
//!   there is nothing in the seam to carry it.
//! * **The content type is always `normal`, with no hints.**
//! * **`done`'s serial is not compared.** It matters to a client that re-sends
//!   state after the input method acts, which this backend never does; the
//!   protocol applies the events whatever the serial.

use crate::{PhysicalRect, ShellError, ShellEvent, TextPurpose, WindowId};

use super::ffi::{WlArgument, WlProxy};
use super::protocol::text_input::{zwp_text_input_manager_v3, zwp_text_input_v3};
use super::{ObjectKind, WaylandShell};

/// See [`COMPOSITOR_VERSION`](super::COMPOSITOR_VERSION). Version 2 adds input
/// panel requests and action events this backend does not use.
pub(super) const TEXT_INPUT_VERSION: u32 = 1;

/// What a window asked of the input method.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Field {
    /// [`Shell::set_text_input`](crate::Shell::set_text_input): the field
    /// taking typing, if one is.
    field: Option<TextPurpose>,
    /// [`Shell::set_text_input_area`](crate::Shell::set_text_input_area), in
    /// the window's device pixels.
    area: Option<PhysicalRect>,
}

/// One seat's `zwp_text_input_v3` and what has passed over it.
#[derive(Debug)]
pub(super) struct TextInput {
    proxy: *mut WlProxy,
    /// The window the compositor's `enter` named, until its `leave`.
    focus: Option<WindowId>,
    /// Whether the state last committed was `enable`.
    enabled: bool,
    pending: Pending,
    /// The pre-edit last reported, so a `done` that leaves it as it was
    /// reports nothing.
    shown: String,
}

/// What arrived since the last `done`, which applies it.
#[derive(Debug, Default)]
struct Pending {
    /// The pre-edit and where its cursor begins, in bytes.
    preedit: Option<(String, i32)>,
    commit: Option<String>,
}

/// One `zwp_text_input_v3` event, with its strings copied out.
#[derive(Clone, Debug)]
pub(super) enum Event {
    Enter { surface: usize },
    Leave,
    Preedit { text: String, cursor_begin: i32 },
    Commit { text: String },
    Done,
}

/// Decodes one event for the dispatcher.
///
/// # Safety
///
/// `args` must be the arguments of `zwp_text_input_v3` event `opcode`, as
/// libwayland hands them to a dispatcher.
pub(super) unsafe fn decode(opcode: u32, args: &WlArgument) -> Option<Event> {
    // SAFETY: the caller guarantees `args` matches `opcode` on this interface.
    Some(
        match unsafe { zwp_text_input_v3::decode_event(opcode, args) }? {
            zwp_text_input_v3::Event::Enter { surface } => Event::Enter {
                surface: surface as usize,
            },
            zwp_text_input_v3::Event::Leave { .. } => Event::Leave,
            zwp_text_input_v3::Event::PreeditString {
                text, cursor_begin, ..
            } => Event::Preedit {
                text: text
                    .map(|text| text.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cursor_begin,
            },
            zwp_text_input_v3::Event::CommitString { text } => Event::Commit {
                text: text
                    .map(|text| text.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            zwp_text_input_v3::Event::Done { .. } => Event::Done,
            // `delete_surrounding_text`: see the module docs. The rest arrive only
            // at version 2.
            _ => return None,
        },
    )
}

/// Where the input method's cursor sits in `text`: `cursor_begin` when it is
/// a byte offset on a `char` boundary, which is what
/// [`ShellEvent::TextPreedit`]'s `cursor` promises, and `None` for a hidden
/// cursor (`-1`), an offset past the end or one inside a character, and for
/// empty text.
fn caret(text: &str, cursor_begin: i32) -> Option<usize> {
    let offset = usize::try_from(cursor_begin).ok()?;
    (!text.is_empty() && text.is_char_boundary(offset)).then_some(offset)
}

/// The `content_hint` and `content_purpose` a field of `purpose` sends. A
/// sensitive field is `hidden_text` and `sensitive_data` as well, which is
/// what tells an input method not to learn from or suggest on what is typed.
fn content_type(purpose: TextPurpose) -> (u32, u32) {
    use zwp_text_input_v3::{content_hint, content_purpose};
    let hint = if purpose.is_sensitive() {
        content_hint::HIDDEN_TEXT | content_hint::SENSITIVE_DATA
    } else {
        content_hint::NONE
    };
    let purpose = match purpose {
        TextPurpose::Normal => content_purpose::NORMAL,
        TextPurpose::Password => content_purpose::PASSWORD,
        TextPurpose::Pin => content_purpose::PIN,
    };
    (hint, purpose)
}

/// `area`, in device pixels at `scale`, as the surface-local logical
/// rectangle `set_cursor_rectangle` takes, grown outward to whole units so it
/// still covers the caret.
fn logical_rect(area: PhysicalRect, scale: f64) -> [i32; 4] {
    let scale = scale.max(f64::MIN_POSITIVE);
    let left = (f64::from(area.x) / scale).floor();
    let top = (f64::from(area.y) / scale).floor();
    let right = ((f64::from(area.x) + f64::from(area.width)) / scale).ceil();
    let bottom = ((f64::from(area.y) + f64::from(area.height)) / scale).ceil();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a window's own pixels over a positive scale, already whole"
    )]
    [
        left as i32,
        top as i32,
        (right - left) as i32,
        (bottom - top) as i32,
    ]
}

impl WaylandShell {
    /// Gives every seat a `zwp_text_input_v3`, once there is a manager to make
    /// one with. Called after each registry batch, as
    /// [`ensure_data_devices`](Self::ensure_data_devices) is and for its
    /// reason.
    pub(super) fn ensure_text_inputs(&mut self) {
        let manager = self.conn.text_input_manager;
        if manager.is_null() {
            return;
        }
        for index in 0..self.seats.len() {
            if self.seats[index].text_input.is_some() {
                continue;
            }
            let seat = self.seats[index].proxy;
            // SAFETY: the manager and the seat are both live on this
            // connection.
            let proxy = unsafe { zwp_text_input_manager_v3::get_text_input(manager, seat) };
            if proxy.is_null() {
                continue;
            }
            self.conn.watch(proxy, ObjectKind::TextInput);
            self.seats[index].text_input = Some(TextInput {
                proxy,
                focus: None,
                enabled: false,
                pending: Pending::default(),
                shown: String::new(),
            });
        }
        self.conn.flush();
    }

    /// Destroys a seat's text input, for a seat that is going away.
    pub(super) fn remove_text_input(&mut self, index: usize) {
        if let Some(text_input) = self.seats[index].text_input.take() {
            // SAFETY: the proxy is live, and `destroy` is its own destructor.
            unsafe {
                self.conn
                    .release(text_input.proxy, zwp_text_input_v3::destroy);
            }
        }
    }

    /// One event on a seat's text input.
    pub(super) fn text_input_event(&mut self, proxy: usize, event: Event) {
        let Some(index) = self.seats.iter().position(|seat| {
            seat.text_input
                .as_ref()
                .is_some_and(|text_input| text_input.proxy as usize == proxy)
        }) else {
            return;
        };
        match event {
            Event::Enter { surface } => {
                let window = self.window_by_proxy(surface, |window| window.surface as usize);
                if let Some(text_input) = self.seats[index].text_input.as_mut() {
                    text_input.focus = window;
                    // `enter` invalidates every state the client set.
                    text_input.enabled = false;
                }
                self.sync_text_input(index);
            }
            Event::Leave => {
                let Some(text_input) = self.seats[index].text_input.as_mut() else {
                    return;
                };
                let window = text_input.focus.take();
                text_input.enabled = false;
                text_input.pending = Pending::default();
                // The client resets its pre-edit on `leave`, so a field that
                // was drawing one stops.
                if let Some(window) = window
                    && !core::mem::take(&mut text_input.shown).is_empty()
                {
                    self.queue.push_back(ShellEvent::TextPreedit {
                        window,
                        time: self.time.event_time_now(),
                        text: String::new(),
                        cursor: None,
                    });
                }
            }
            Event::Preedit { text, cursor_begin } => {
                if let Some(text_input) = self.seats[index].text_input.as_mut() {
                    text_input.pending.preedit = Some((text, cursor_begin));
                }
            }
            Event::Commit { text } => {
                if let Some(text_input) = self.seats[index].text_input.as_mut() {
                    text_input.pending.commit = Some(text);
                }
            }
            Event::Done => self.apply_text_input(index),
        }
    }

    /// Applies what arrived before a `done`: the commit, then the pre-edit if
    /// it changed. A pre-edit not sent again since the last `done` is gone,
    /// which is how the protocol ends one.
    fn apply_text_input(&mut self, index: usize) {
        let Some(text_input) = self.seats[index].text_input.as_mut() else {
            return;
        };
        let pending = core::mem::take(&mut text_input.pending);
        let Some(window) = text_input.focus else {
            return;
        };
        let time = self.time.event_time_now();
        if let Some(text) = pending.commit.filter(|text| !text.is_empty()) {
            self.queue
                .push_back(ShellEvent::TextCommit { window, time, text });
        }
        let (text, cursor_begin) = pending.preedit.unwrap_or_default();
        if text != text_input.shown {
            text_input.shown.clone_from(&text);
            let cursor = caret(&text, cursor_begin);
            self.queue.push_back(ShellEvent::TextPreedit {
                window,
                time,
                text,
                cursor,
            });
        }
    }

    /// Sends a seat's text input what its focused window asked for: `enable`
    /// with the window's caret, or `disable` if it was enabled and the window
    /// no longer asks.
    fn sync_text_input(&mut self, index: usize) {
        let Some(text_input) = self.seats[index].text_input.as_ref() else {
            return;
        };
        let (proxy, focus, enabled) = (text_input.proxy, text_input.focus, text_input.enabled);
        let wanted = focus.and_then(|window| {
            let state = self.window(window).ok()?;
            state
                .text_input
                .field
                .map(|purpose| (purpose, state.text_input.area, state.scale_factor()))
        });
        // SAFETY: the proxy is live; every argument is a plain integer.
        unsafe {
            match wanted {
                Some((purpose, area, scale)) => {
                    let (hint, purpose) = content_type(purpose);
                    zwp_text_input_v3::enable(proxy);
                    zwp_text_input_v3::set_content_type(proxy, hint, purpose);
                    if let Some(area) = area {
                        let [x, y, width, height] = logical_rect(area, scale);
                        zwp_text_input_v3::set_cursor_rectangle(proxy, x, y, width, height);
                    }
                    zwp_text_input_v3::commit(proxy);
                }
                None if enabled => {
                    zwp_text_input_v3::disable(proxy);
                    zwp_text_input_v3::commit(proxy);
                }
                None => return,
            }
        }
        if let Some(text_input) = self.seats[index].text_input.as_mut() {
            text_input.enabled = wanted.is_some();
        }
        self.conn.flush();
    }

    /// [`Shell::set_text_input`](crate::Shell::set_text_input). A field
    /// whose purpose changes is enabled again, since its content type is part
    /// of the state `enable` resets.
    pub(super) fn enable_text_input(
        &mut self,
        window: WindowId,
        field: Option<TextPurpose>,
    ) -> Result<(), ShellError> {
        let state = &mut self.window_mut(window)?.text_input;
        if state.field == field {
            return Ok(());
        }
        state.field = field;
        self.sync_focused(window);
        Ok(())
    }

    /// [`Shell::set_text_input_area`](crate::Shell::set_text_input_area).
    pub(super) fn place_text_input(
        &mut self,
        window: WindowId,
        area: Option<PhysicalRect>,
    ) -> Result<(), ShellError> {
        let state = self.window_mut(window)?;
        if state.text_input.area == area {
            return Ok(());
        }
        state.text_input.area = area;
        let scale = state.scale_factor();
        let Some(area) = area else {
            // The protocol has no request that withdraws a cursor rectangle
            // short of the state reset `enable` makes, so a field that stops
            // naming its caret is enabled again.
            self.sync_focused(window);
            return Ok(());
        };
        // A caret that moved is a new rectangle and nothing else: enabling
        // again would reset the input method mid-composition.
        let [x, y, width, height] = logical_rect(area, scale);
        for seat in &self.seats {
            if let Some(text_input) = &seat.text_input
                && text_input.enabled
                && text_input.focus == Some(window)
            {
                // SAFETY: the proxy is live; every argument is a plain
                // integer.
                unsafe {
                    zwp_text_input_v3::set_cursor_rectangle(text_input.proxy, x, y, width, height);
                    zwp_text_input_v3::commit(text_input.proxy);
                }
            }
        }
        self.conn.flush();
        Ok(())
    }

    /// [`sync_text_input`](Self::sync_text_input) on every seat whose text
    /// input focus is `window`.
    fn sync_focused(&mut self, window: WindowId) {
        for index in 0..self.seats.len() {
            let focused = self.seats[index]
                .text_input
                .as_ref()
                .is_some_and(|text_input| text_input.focus == Some(window));
            if focused {
                self.sync_text_input(index);
            }
        }
    }

    /// Forgets `window` as any seat's text input focus, for a window being
    /// destroyed: its surface is gone, so there is nothing to disable.
    pub(super) fn forget_text_input_focus(&mut self, window: WindowId) {
        for seat in &mut self.seats {
            if let Some(text_input) = seat.text_input.as_mut()
                && text_input.focus == Some(window)
            {
                text_input.focus = None;
                text_input.pending = Pending::default();
                text_input.shown.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each purpose's hint and purpose, by the numbers the protocol XML gives
    /// `hidden_text` (0x40), `sensitive_data` (0x80) and the `normal`,
    /// `password` and `pin` purposes, not by the generated constants this
    /// code names.
    #[test]
    fn a_sensitive_field_is_hidden_and_sensitive_with_its_purpose() {
        assert_eq!(content_type(TextPurpose::Normal), (0, 0));
        assert_eq!(content_type(TextPurpose::Password), (0x40 | 0x80, 8));
        assert_eq!(content_type(TextPurpose::Pin), (0x40 | 0x80, 9));
    }

    /// The pre-edit cursor is `cursor_begin` where it is a real position in
    /// the text, and nothing where the input method hid it or named a byte
    /// that is not one.
    #[test]
    fn the_preedit_cursor_is_a_char_boundary_in_the_text_or_none() {
        assert_eq!(caret("ka", 2), Some(2), "the end is a position");
        assert_eq!(caret("ka", 0), Some(0));
        assert_eq!(caret("ka", -1), None, "hidden");
        assert_eq!(caret("ka", 3), None, "past the end");
        assert_eq!(caret("か", 1), None, "inside a character");
        assert_eq!(caret("か", 3), Some(3));
        assert_eq!(caret("", 0), None, "no text, no cursor");
    }

    /// The caret's rectangle becomes logical units at the window's scale,
    /// grown outward so a fractional edge still covers it.
    #[test]
    fn the_caret_rectangle_is_logical_and_covers_the_caret() {
        let area = PhysicalRect::new(30, 45, 3, 30);
        assert_eq!(logical_rect(area, 1.0), [30, 45, 3, 30]);
        assert_eq!(logical_rect(area, 2.0), [15, 22, 2, 16]);
        assert_eq!(logical_rect(area, 1.5), [20, 30, 2, 20]);
    }
}
